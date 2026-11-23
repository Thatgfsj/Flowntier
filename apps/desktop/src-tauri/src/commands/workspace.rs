//! Workdir and workspace file commands.

use crate::sidecar::pipe_request;
use crate::util::{unix_secs_to_iso8601, url_encode};

/// Persist the workspace workdir path. Called from the
/// WorkdirSetup dialog on first launch (and from Settings >
/// About > Change workdir). Stores the path in
/// <data_dir>/workdir.json so it survives quit+relaunch.
#[tauri::command]
pub async fn set_workdir(path: String) -> Result<(), String> {
    // BUG-016 fix: defer to set_workdir_with_nwt so the workdir
    // write is gated on a successful `.nwt/` init. The atomicity
    // guarantee (workdir.json is never written unless `.nwt/` is
    // fully set up) holds for both `set_workdir` and the new
    // `set_workdir_with_nwt` callers.
    set_workdir_with_nwt(path).await.map(|_| ())
}

/// BUG-017 partial fix (event 000032): explicitly clear the
/// workdir + nwt root sentinel. Used by App.tsx when the user
/// clicks "Skip" on the WorkdirSetup dialog (treats skipping
/// the workdir as "no project" — agent-core nwt tool will see
/// no root and refuse to log). On next launch, get_workdir
/// returns null → WorkdirSetup dialog re-shows.
#[tauri::command]
pub async fn clear_workdir() -> Result<(), String> {
    let Some(data_dir) = storage::Repository::default_data_dir() else {
        return Ok(()); // best-effort; nothing to clear
    };
    let wd_file = data_dir.join("workdir.json");
    let nrt_file = data_dir.join("nwt_root.json");
    let _ = std::fs::remove_file(&wd_file);
    let _ = std::fs::remove_file(&nrt_file);
    tracing::info!("workdir + nwt_root sentinel cleared");
    Ok(())
}

/// Atomically set the workdir AND initialise the project's
/// `.nwt/` directory. BUG-016 fix.
///
/// Order of operations is deliberately:
///
///   1. Validate the path (exists + is_dir — BUG-011 reuse).
///   2. Initialise `.nwt/` in the workdir (creates metadata.json,
///      timeline/, indices/{tags,files}.json — idempotent).
///   3. Only after step 2 succeeds, atomically write
///      `workdir.json` to the data dir (tmp + rename).
///
/// If step 1 or 2 fails, `workdir.json` is never written, so on
/// the next launch `get_workdir` returns `None` and the user is
/// re-shown the WorkdirSetup dialog. This is the recovery
/// strategy — we never persist half-initialised state.
///
/// Used by the Webview/TS frontend in App.tsx's WorkdirSetup
/// confirm handler. Replaces the previous two-command dance
/// (`set_workdir` + `nwt_init_workspace`) which had a partial-
/// failure window where workdir was set but `.nwt/` was not.
#[tauri::command]
pub async fn set_workdir_with_nwt(path: String) -> Result<String, String> {
    // Step 1: data dir + path validation.
    let Some(data_dir) = storage::Repository::default_data_dir() else {
        return Err("cannot determine data dir".into());
    };
    let _ = std::fs::create_dir_all(&data_dir);
    let root = std::path::PathBuf::from(&path);
    if !root.exists() {
        return Err(format!("workdir does not exist: {}", path));
    }
    if !root.is_dir() {
        return Err(format!("workdir is not a directory: {}", root.display()));
    }
    // BUG-012 fix (event 000023): reject filesystem root paths
    // (`/` on Unix, `C:\` / drive roots on Windows). Without this
    // guard, a power user typing `C:\` into the manual text
    // input would silently succeed and create `C:\.nwt\`,
    // polluting the system drive. We compare against the OS-
    // reported root components.
    let root_components = root.components().count();
    if root_components <= 1 {
        return Err(format!(
            "workdir cannot be a filesystem root: {}",
            root.display()
        ));
    }

    // Step 2: initialise `.nwt/` in the workdir. Idempotent.
    let nwt_dir = root.join(".nwt");
    std::fs::create_dir_all(nwt_dir.join("timeline"))
        .map_err(|e| format!("mkdir timeline: {e}"))?;
    std::fs::create_dir_all(nwt_dir.join("indices")).map_err(|e| format!("mkdir indices: {e}"))?;
    let meta = nwt_dir.join("metadata.json");
    if !meta.exists() {
        let project_name = root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("flowntier-project");
        let payload = serde_json::json!({
            "project_name": project_name,
            "created_at": unix_secs_to_iso8601(std::time::SystemTime::now()),
            "schema_version": 1,
            "format": "nwt/0.1",
        });
        let bytes =
            serde_json::to_vec_pretty(&payload).map_err(|e| format!("serialize metadata: {e}"))?;
        std::fs::write(&meta, bytes).map_err(|e| format!("write metadata: {e}"))?;
    }
    let tags_idx = nwt_dir.join("indices").join("tags.json");
    if !tags_idx.exists() {
        std::fs::write(&tags_idx, b"{}\n").map_err(|e| format!("write tags: {e}"))?;
    }
    let files_idx = nwt_dir.join("indices").join("files.json");
    if !files_idx.exists() {
        std::fs::write(&files_idx, b"{}\n").map_err(|e| format!("write files: {e}"))?;
    }

    // Step 3: only NOW persist the workdir, atomically (tmp + rename
    // to avoid a torn write that would brick get_workdir).
    let wd_file = data_dir.join("workdir.json");
    let payload = serde_json::json!({ "workdir": path });
    let bytes =
        serde_json::to_vec_pretty(&payload).map_err(|e| format!("serialize workdir: {e}"))?;
    let tmp = wd_file.with_extension("json.tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("write tmp {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &wd_file).map_err(|e| format!("rename to {}: {e}", wd_file.display()))?;

    tracing::info!(path = %path, nwt = %nwt_dir.display(), "Workdir + .nwt initialised atomically");

    // BUG-017 partial fix (event 000032): the agent-core nwt
    // tool lives in pipe-server (a separate process). It can't
    // see the desktop's in-memory NWT_ROOT static. The bridge
    // is a tiny sentinel file `<data_dir>/nwt_root.json` that
    // pipe-server can read on every workflow start (see
    // `crates/pipe-server/src/state.rs` init). We write it here
    // — atomic (tmp + rename) and best-effort (failure is
    // logged but doesn't fail the workdir change).
    let nwt_root_payload = serde_json::json!({
        "nwt_root": nwt_dir.to_string_lossy(),
        "set_at": unix_secs_to_iso8601(std::time::SystemTime::now()),
    });
    if let Ok(bytes) = serde_json::to_vec_pretty(&nwt_root_payload) {
        let nrt_file = data_dir.join("nwt_root.json");
        let nrt_tmp = nrt_file.with_extension("json.tmp");
        match std::fs::write(&nrt_tmp, &bytes).and_then(|_| std::fs::rename(&nrt_tmp, &nrt_file)) {
            Ok(()) => tracing::info!(path = %nrt_file.display(), "nwt_root.json written"),
            Err(e) => {
                tracing::warn!(error = %e, "failed to write nwt_root.json; agent-core nwt tool will see no root")
            }
        }
    }

    // v0.4.21 (event 000066): notify the pipe-server sidecar so
    // its in-process `Workspace` (used as the agent's fs root)
    // actually points at the new workdir. Until this call the
    // runtime was stuck on whatever cwd the sidecar was launched
    // from — chief would write to e.g. O:\Flowntier\workspace\
    // while the chairman's UI said O:\try, so the
    // "切工作目录不显示新文件" bug shipped. Best-effort: if the
    // pipe-server is unreachable we still return success because
    // the workdir.json write itself is what the on-disk logic
    // needs; the agent will just write to the stale root until
    // the runtime is restarted. The Tauri shell logs a warning
    // so the chairman can spot it.
    let abs = root.canonicalize().unwrap_or_else(|_| root.clone());
    match pipe_request(
        "POST",
        "/api/workspace/set",
        Some(serde_json::json!({ "path": abs.to_string_lossy() })),
    )
    .await
    {
        Ok(v) => {
            tracing::info!(target: "tauri_runtime", response = %v, "v0.4.21 (event 000066): pipe-server workspace updated")
        }
        Err(e) => tracing::warn!(
            target: "tauri_runtime",
            error = %e,
            "v0.4.21 (event 000066): pipe-server workspace swap failed; runtime will keep its launch-time cwd until restarted"
        ),
    }

    Ok(nwt_dir.to_string_lossy().into_owned())
}

/// Read the workspace workdir. Returns null if not yet set
/// (i.e. first launch before the user has picked a workdir).
#[tauri::command]
pub async fn get_workdir() -> Result<Option<String>, String> {
    let Some(data_dir) = storage::Repository::default_data_dir() else {
        return Ok(None);
    };
    let p = data_dir.join("workdir.json");
    if !p.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&p).map_err(|e| format!("read {}: {e}", p.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", p.display()))?;
    Ok(v.get("workdir")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string()))
}

/// v0.4.21 (event 000066): read the pipe-server's *actual*
/// workspace root (the path the agent loop will use as the
/// chief's filesystem context). Distinct from `get_workdir`,
/// which reads the `workdir.json` on disk — they should agree
/// after a successful `set_workdir_with_nwt`, but in transient
/// states (sidecar just restarted, sidecar unreachable) they
/// can disagree. Both surfaces matter for diagnostics.
#[tauri::command]
pub async fn get_runtime_workspace() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/workspace", None).await
}

/// v0.4.21 (event 000066): list a directory tree under the
/// runtime workspace root. Body shape: `{ path?: string,
/// depth?: number, max_entries?: number }`. Used by the
/// `FileTree` component to render the chairman's project view.
#[tauri::command]
pub async fn get_workspace_tree(body: serde_json::Value) -> Result<serde_json::Value, String> {
    // GET-with-body: encode the body into query params so the
    // pipe-server side's dispatcher pattern match still hits
    // `/api/workspace/tree` (the dispatcher strips query before
    // matching; see event 000064 follow-up).
    let mut q = Vec::<String>::new();
    if let Some(p) = body.get("path").and_then(|v| v.as_str()) {
        if !p.is_empty() {
            q.push(format!("path={}", url_encode(p)));
        }
    }
    if let Some(d) = body.get("depth").and_then(|v| v.as_u64()) {
        q.push(format!("depth={d}"));
    }
    if let Some(m) = body.get("max_entries").and_then(|v| v.as_u64()) {
        q.push(format!("max_entries={m}"));
    }
    let path = if q.is_empty() {
        "/api/workspace/tree".to_string()
    } else {
        format!("/api/workspace/tree?{}", q.join("&"))
    };
    pipe_request("GET", &path, None).await
}

// ── v0.4.36: editor file access + checkpoints + MCP status ──────

/// Read one workspace file for the in-app editor (EditorPane).
/// Body: `{ path: string }`. The pipe-server enforces workspace
/// containment + the 2 MiB cap and returns `{ ok, content,
/// mtime_ms }` — the mtime is what the editor echoes back on save
/// to detect concurrent modification.
#[tauri::command]
pub async fn get_workspace_file(body: serde_json::Value) -> Result<serde_json::Value, String> {
    let p = body.get("path").and_then(|v| v.as_str()).unwrap_or("");
    if p.is_empty() {
        return Err("missing 'path'".into());
    }
    let path = format!("/api/workspace/file?path={}", url_encode(p));
    pipe_request("GET", &path, None).await
}

/// Write one workspace file. Body: `{ path, content,
/// expected_mtime_ms? }`. A stale `expected_mtime_ms` yields
/// `{ ok:false, conflict:true }` instead of clobbering the file.
#[tauri::command]
pub async fn write_workspace_file(body: serde_json::Value) -> Result<serde_json::Value, String> {
    pipe_request("POST", "/api/workspace/file", Some(body)).await
}
