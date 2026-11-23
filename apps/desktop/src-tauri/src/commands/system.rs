//! Diagnostics, I-Ching easter egg, and destructive reset commands.

use crate::sidecar::{kill_sidecar_silent, pipe_request};
use crate::util::unix_secs_to_iso8601;
use std::time::Duration;

/// Draw one hexagram at random from the 64-entry I Ching dataset.
/// Stateless; safe to spam-click from the oracle UI.
#[tauri::command]
pub async fn draw_i_ching() -> Result<serde_json::Value, String> {
    pipe_request("POST", "/api/i_ching/draw", None).await
}

/// Wipe ALL local data: API keys, custom providers, run logs,
/// error logs, kv table, the entire SQLite database. Idempotent
/// but irreversible. The Settings → About → "Clear local data"
/// button calls this. After wipe, the next launch sees an empty
/// data dir and falls back to the Welcome screen (first_run
/// defaults to 'true' until the Welcome complete handler writes
/// 'false' — but since we deleted kv, kv.first_run is null,
/// which Welcome reads as 'show me the wizard').
///
/// Why a Tauri command (not a pipe-server endpoint): the pipe
/// server's own SQLite is open against the data dir. Removing
/// the data dir while the server is running would leave the
/// server with a stale file handle. We close the server first
/// (via shutdown_runtime), then rm -rf, then the user restarts
/// the app. The Tauri shell process keeps running and serves
/// the Settings modal even after the runtime is gone (since the
/// modal just calls back into Rust, no pipe needed).
#[tauri::command]
pub async fn wipe_all_data() -> Result<(), String> {
    let Some(data_dir) = storage::Repository::default_data_dir() else {
        return Err("cannot determine data dir".into());
    };
    // Best-effort: kill any running sidecar first so its file
    // handles on storage.sqlite are released. We don't need the
    // sidecar to acknowledge — the rm -rf below will fail loudly
    // on Windows if the file is still locked.
    //
    // event 000109: use the SAME image name as the actual
    // sidecar. Before this fix, line 195 used `flowntier_runtime`
    // (underscore — matches `app.shell().sidecar("flowntier_
    // runtime")` at line 212) but THIS site used
    // `flowntier-runtime` (hyphen). The hyphen form always
    // no-op'd (returned 128 silently) so `wipe_all_data` left
    // the running sidecar alive, and the subsequent rm -rf on
    // the data dir was racing the sidecar's still-open
    // SQLite handle — Windows locks would sometimes let the
    // wipe succeed anyway, sometimes not.
    #[cfg(target_os = "windows")]
    {
        kill_sidecar_silent();
        // Give the kernel a moment to actually release the
        // handle. The previous 500ms is plenty for a normal
        // `taskkill /f` round-trip.
        std::thread::sleep(Duration::from_millis(500));
    }
    // Recursively remove the data dir. Errors are surfaced to the
    // user (Settings modal shows a "Clear failed: <err>" message).
    std::fs::remove_dir_all(&data_dir).map_err(|e| format!("rm {}: {e}", data_dir.display()))?;
    // Re-create the dir so the next launch's logging::init_logging
    // has somewhere to write. Empty dir + empty SQLite is the
    // "fresh install" state.
    std::fs::create_dir_all(&data_dir).map_err(|e| format!("mkdir {}: {e}", data_dir.display()))?;
    Ok(())
}

// BUG-FRONTEND-RT-??: comprehensive diagnostics command for
// hands-on debugging. Returns a JSON snapshot of:
//   - data dir + log dir paths
//   - Tauri version
//   - which env vars (MINIMAX_API_KEY, OPENAI_API_KEY, etc.) are set
//   - workdir + nwt_root sentinel file state
//   - keychain size (count of stored keys)
//   - whether the sidecar binary was found
//   - last 5 lines of the current log file
//
// The chairman can invoke this from the dev console:
//   await invoke('get_diagnostics')
// and paste the result so we can compare against our local run.
#[tauri::command]
pub async fn get_diagnostics() -> Result<serde_json::Value, String> {
    use serde_json::json;
    let mut out = serde_json::Map::new();

    // Paths
    let data_dir = storage::Repository::default_data_dir();
    out.insert(
        "data_dir".into(),
        json!(data_dir
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unknown>".to_string())),
    );
    if let Some(ref dd) = data_dir {
        out.insert(
            "log_dir".into(),
            json!(tauri_core::logging::log_dir(dd).display().to_string()),
        );
    } else {
        out.insert("log_dir".into(), json!(null));
    }

    // Env vars
    let env_keys = [
        "MINIMAX_API_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "GOOGLE_API_KEY",
        "DEEPSEEK_API_KEY",
        "MOONSHOT_API_KEY",
        "OPEN_BIGMODEL_API_KEY",
        "ZHIPUAI_API_KEY",
    ];
    let env_state: serde_json::Map<String, serde_json::Value> = env_keys
        .iter()
        .map(|k| {
            let state = match std::env::var(k) {
                Ok(v) if v.is_empty() => "<empty>",
                Ok(_) => "<set>",
                Err(_) => "<unset>",
            };
            (k.to_string(), json!(state))
        })
        .collect();
    out.insert("env_vars".into(), json!(env_state));

    // Tauri version + runtime version
    out.insert("tauri_version".into(), json!(env!("CARGO_PKG_VERSION")));

    // Workdir sentinel state
    if let Some(ref dd) = data_dir {
        let wd = dd.join("workdir.json");
        out.insert("workdir_json_exists".into(), json!(wd.exists()));
        if wd.exists() {
            if let Ok(content) = std::fs::read_to_string(&wd) {
                out.insert("workdir_json_content".into(), json!(content));
            }
        }
        let nrt = dd.join("nwt_root.json");
        out.insert("nwt_root_json_exists".into(), json!(nrt.exists()));
        if nrt.exists() {
            if let Ok(content) = std::fs::read_to_string(&nrt) {
                out.insert("nwt_root_json_content".into(), json!(content));
            }
        }
    }

    // Keychain (settings/secrets.json) — count entries
    if let Some(ref dd) = data_dir {
        let secrets = dd.join("settings").join("secrets.json");
        if secrets.exists() {
            if let Ok(content) = std::fs::read_to_string(&secrets) {
                let count = content.matches("\"api_key\"").count();
                out.insert("secrets_count".into(), json!(count));
                out.insert("secrets_file".into(), json!(content));
            }
        } else {
            out.insert("secrets_count".into(), json!(0));
        }
    }

    // Sidecar binary
    if let Some(ref dd) = data_dir {
        let candidates = [
            dd.join("flowntier_runtime.exe"),
            dd.parent()
                .map(|p| p.join("flowntier_runtime.exe"))
                .unwrap_or_default(),
        ];
        let mut found_path: Option<String> = None;
        for c in &candidates {
            if c.exists() {
                found_path = Some(c.display().to_string());
                break;
            }
        }
        out.insert("sidecar_path".into(), json!(found_path));
    }

    // Log file tail (last 5 lines)
    if let Some(ref dd) = data_dir {
        let log_path = tauri_core::logging::log_dir(dd).join("flowntier.log");
        // The actual filename includes the date suffix
        if let Ok(entries) = std::fs::read_dir(tauri_core::logging::log_dir(dd)) {
            let mut logs: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    name.starts_with("flowntier.log")
                })
                .collect();
            logs.sort_by_key(|e| e.file_name());
            if let Some(last) = logs.last() {
                if let Ok(content) = std::fs::read_to_string(last.path()) {
                    let lines: Vec<&str> = content.lines().rev().take(5).collect();
                    let mut tail: Vec<&str> = lines.into_iter().rev().collect();
                    tail.reverse();
                    out.insert("log_tail".into(), json!(tail.join("\n")));
                }
            }
        }
        let _ = log_path; // suppress unused
    }

    out.insert(
        "timestamp".into(),
        json!(unix_secs_to_iso8601(std::time::SystemTime::now())),
    );

    Ok(serde_json::Value::Object(out))
}
