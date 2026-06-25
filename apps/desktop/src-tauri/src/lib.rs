//! Tauri app glue for the desktop shell.
//!
//! Architecture: React → invoke() → Rust → Windows named pipe → pipe-server
//! Frontend NEVER touches HTTP. The webview loads the dev server URL only.
//! There is no `127.0.0.1:7317` HTTP server anymore — all RPC and event
//! streaming travel over `\\.\pipe\flowntier_runtime` (RPC) and
//! `\\.\pipe\flowntier_runtime_events` (server-push events).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use tauri::Manager;
use tauri::Emitter;
use tauri_plugin_shell::ShellExt;
use tauri_core::logging::{self, LoggingGuard};
use tauri_core::{start_workflow, AppState, NewWorkflowRequest, NewWorkflowResponse};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ClientOptions;

const RPC_PIPE: &str = r"\\.\pipe\flowntier_runtime";
const EVENTS_PIPE: &str = r"\\.\pipe\flowntier_runtime_events";

/// Process-wide logging guard. Stored in a OnceLock so the panic
/// hook installed by `logging::init_logging` survives for the
/// lifetime of the process — the guard's Drop impl flushes + joins
/// the background writer thread, so we must hold onto it.
static LOGGING_GUARD: OnceLock<LoggingGuard> = OnceLock::new();

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
const MAX_LINE: usize = 1_048_576; // 1 MiB hard cap per pipe message

// ── Pipe RPC client ─────────────────────────────────────────────

/// One connection = one request-response. Reconnects on the next call.
///
/// Wire format (newline-delimited JSON):
///   request  = `{"jsonrpc":"2.0","id":N,"method":VERB,"params":{"path":...,"body":...}}\n`
///   response = `{"jsonrpc":"2.0","id":N,"result":{"status":S,"body":B}}`  or
///              `{"jsonrpc":"2.0","id":N,"error":{"code":-32603,"message":"..."}}\n`
async fn pipe_request(
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let mut conn = ClientOptions::new()
        .open(RPC_PIPE)
        .map_err(|e| format!("pipe open {RPC_PIPE}: {e}"))?;

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": {"path": path, "body": body}
    });
    let mut line = serde_json::to_vec(&req).map_err(|e| e.to_string())?;
    line.push(b'\n');
    conn.write_all(&line)
        .await
        .map_err(|e| format!("pipe write: {e}"))?;

    let mut buf = Vec::with_capacity(4096);
    let mut byte = [0u8; 1];
    loop {
        conn.read_exact(&mut byte)
            .await
            .map_err(|e| format!("pipe read: {e}"))?;
        if byte[0] == b'\n' {
            break;
        }
        if buf.len() >= MAX_LINE {
            return Err(format!("pipe response exceeds {MAX_LINE} bytes"));
        }
        buf.push(byte[0]);
    }

    let resp: serde_json::Value =
        serde_json::from_slice(&buf).map_err(|e| format!("pipe bad json: {e}"))?;

    if let Some(err) = resp.get("error") {
        return Err(err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("pipe error")
            .to_string());
    }
    let status = resp
        .pointer("/result/status")
        .and_then(|s| s.as_u64())
        .unwrap_or(0);
    if !(200..300).contains(&status) {
        // Non-2xx body is wrapped into a string so the caller can show it
        // (e.g. "HTTP 422: {detail: '...'}").
        let body = resp
            .pointer("/result/body")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        return Err(format!("HTTP {status}: {body}"));
    }
    Ok(resp
        .pointer("/result/body")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

// ── Sidecar management ──────────────────────────────────────────

fn spawn_runtime_sidecar(app: &tauri::AppHandle) {
    use tauri_plugin_shell::process::CommandEvent;

// Kill any stale flowntier-runtime.exe from a previous session. Pipes
    // are exclusive, so a dead binary that left the pipe handle open
    // would block the new instance.
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/f", "/im", "flowntier-runtime.exe"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        std::thread::sleep(Duration::from_millis(1000));
    }

    // Already up? Probe the RPC pipe.
    if try_ping_pipe().is_ok() {
        println!("[flowntier] runtime already running");
    } else {
        println!("[flowntier] spawning sidecar...");
        let sidecar_command = match app.shell().sidecar("flowntier_runtime") {
            Ok(cmd) => cmd,
            Err(e) => {
                eprintln!("[flowntier] failed to create sidecar: {}", e);
                return;
            }
        };

        match sidecar_command.spawn() {
            Ok((mut rx, child)) => {
                println!("[flowntier] sidecar pid={:?}", child.pid());
                tauri::async_runtime::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        match event {
                            CommandEvent::Stdout(line) => {
                                println!("[sidecar] {}", String::from_utf8_lossy(&line));
                            }
                            CommandEvent::Stderr(line) => {
                                eprintln!("[sidecar:err] {}", String::from_utf8_lossy(&line));
                            }
                            CommandEvent::Terminated(status) => {
                                eprintln!("[sidecar] terminated: {:?}", status);
                                break;
                            }
                            _ => {}
                        }
                    }
                });

                // Wait until the sidecar is listening on the pipe.
                let deadline = std::time::Instant::now() + Duration::from_secs(30);
                while std::time::Instant::now() < deadline {
                    if try_ping_pipe().is_ok() {
                        println!("[flowntier] sidecar healthy!");
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            Err(e) => {
                eprintln!("[flowntier] failed to spawn: {}", e);
                return;
            }
        }
    }

    // Start the event stream bridge: connect to the events pipe and
    // forward every WfEvent to the webview via `app.emit("wf:event", …)`.
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        events_bridge(app_handle).await;
    });
}

fn try_ping_pipe() -> Result<(), String> {
    // A blocking probe — we don't care about the response, just whether
    // the kernel will hand us a handle.
    let opts = ClientOptions::new();
    opts.open(RPC_PIPE).map(|_| ()).map_err(|e| e.to_string())
}

async fn events_bridge(app: tauri::AppHandle) {
    let mut backoff_ms = 200u64;
    loop {
        match ClientOptions::new().open(EVENTS_PIPE) {
            Ok(mut conn) => {
                backoff_ms = 200;
                let mut buf = Vec::new();
                let mut byte = [0u8; 1];
                loop {
                    match conn.read_exact(&mut byte).await {
                        Ok(_) => {
                            if byte[0] == b'\n' {
                                if buf.is_empty() {
                                    continue;
                                }
                                if let Ok(v) =
                                    serde_json::from_slice::<serde_json::Value>(&buf)
                                {
                                    if let Some(ev) = v.get("event") {
                                        if let Err(e) = app.emit("wf:event", ev.clone()) {
                                            eprintln!("[flowntier] emit wf:event failed: {e}");
                                        }
                                    }
                                }
                                buf.clear();
                            } else if buf.len() < MAX_LINE {
                                buf.push(byte[0]);
                            }
                        }
                        Err(e) => {
                            eprintln!("[flowntier] events pipe read err: {e}; reconnecting");
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[flowntier] events pipe open err: {e}; retry in {backoff_ms}ms");
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(5_000);
            }
        }
    }
}

// ── Tauri commands (React → Rust) ───────────────────────────────

#[tauri::command]
async fn health_check() -> Result<bool, String> {
    Ok(pipe_request("GET", "/health", None).await.is_ok())
}

/// Returns the sidecar runtime's version + a min_compatible
/// field. The frontend compares sidecar against its own
/// expected version; if sidecar < min_compatible, the app
/// shows a non-blocking drift banner.
///
/// Used by the desktop shell on startup, after the runtime
/// pipe becomes reachable.
#[tauri::command]
async fn rpc_version() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/rpc/version", None).await
}

#[tauri::command]
async fn list_secrets() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/settings/secrets", None).await
}

/// Run a single agent task envelope end-to-end.
///
/// Bridges the React `ChatZone` to the embedded pipe-server's
/// `/api/run_task` handler. The handler drives the in-process
/// agent-core loop, which streams events back over
/// `\\.\pipe\flowntier_runtime_events` and surfaces them through
/// the normal `wf:event` Tauri channel.
///
/// Body shape matches `run_task` in `crates/pipe-server/src/handlers.rs`:
///   { task: string, role?: string, provider_kind?: string,
///     base_url?: string, model?: string, api_key?: string,
///     api_key_env?: string }
#[tauri::command]
async fn run_agent_task(body: serde_json::Value) -> Result<serde_json::Value, String> {
    pipe_request("POST", "/api/run_task", Some(body)).await
}

/// Draw one hexagram at random from the 64-entry I Ching dataset.
/// Stateless; safe to spam-click from the oracle UI.
#[tauri::command]
async fn draw_i_ching() -> Result<serde_json::Value, String> {
    pipe_request("POST", "/api/i_ching/draw", None).await
}

/// Write a frontend error to the Rust log file.
///
/// Called from `apps/desktop/src/components/ErrorBoundary.tsx` when
/// the React tree throws. Best-effort: failures are logged but
/// never thrown back to the JS side (that would deadlock the
/// error UI).
#[tauri::command]
async fn log_frontend_error(
    message: String,
    stack: String,
    component_stack: String,
) -> Result<(), String> {
    tracing::error!(
        target: "frontend",
        component_stack = component_stack,
        "React uncaught error: {message}"
    );
    // Write the stack at debug level so the file includes it but
    // it doesn't show up in stderr (dev) by default. Users can
    // grep the log file directly.
    tracing::debug!(target: "frontend.stack", "{stack}");
    Ok(())
}

/// Read a key from the persistent kv store. Used by the frontend
/// to check first-run flags, the last-opened tab, etc.
///
/// Body: empty. Returns the JSON-encoded value or null.
#[tauri::command]
async fn kv_get(key: String) -> Result<serde_json::Value, String> {
    pipe_request("GET", &format!("/api/kv/{key}"), None).await
}

/// Set a key in the persistent kv store. Used by the Welcome
/// screen to clear the `first_run` flag, and by Settings to
/// persist UI preferences.
///
/// Body: { value: <json> }. Returns { k, v }.
#[tauri::command]
async fn kv_set(key: String, value: serde_json::Value) -> Result<serde_json::Value, String> {
    pipe_request("POST", &format!("/api/kv/{key}"), Some(serde_json::json!({
        "value": value,
    })))
    .await
}

/// Return the v0.4 sample workflow envelope. The frontend calls
/// this from the Welcome screen's "Try sample" button; the user
/// can then submit it as a real workflow via run_agent_task.
///
/// Body: empty. Returns the full WorkflowRun JSON.
#[tauri::command]
async fn load_sample_workflow() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/sample/auth_login", None).await
}

/// Mark the first-run flow as complete. Called from the Welcome
/// screen's "Enter workspace" button. Idempotent — calling twice
/// is a no-op.
#[tauri::command]
async fn first_run_complete() -> Result<serde_json::Value, String> {
    pipe_request("POST", "/api/kv/first_run/complete", None).await
}

#[tauri::command]
async fn save_secret(name: String, value: String) -> Result<serde_json::Value, String> {
    // 1) Persist to keychain via runtime.
    pipe_request(
        "PUT",
        &format!("/api/settings/secrets/{}", name),
        Some(serde_json::json!({ "value": value })),
    )
    .await?;

    // 2) Best-effort seed to os.environ. The keychain write already
    //    succeeded; we MUST NOT fail the user-visible save on a seed
    //    hiccup. Surface the warning so the UI can show "saved, but
    //    click Re-inject to retry the seed".
    let warning: Option<String> = match pipe_request(
        "POST",
        "/api/settings/secrets/seed",
        Some(serde_json::json!({})),
    )
    .await
    {
        Ok(_) => None,
        Err(e) => {
            eprintln!("[save_secret] seed post failed (non-fatal): {}", e);
            Some(format!("seed failed: {}", e))
        }
    };

    Ok(serde_json::json!({
        "saved": true,
        "warning": warning,
    }))
}

#[tauri::command]
async fn delete_secret(name: String) -> Result<(), String> {
    // The runtime returns 204 on success and 404 when the key was never
    // set; both are treated as success from the UI's perspective.
    match pipe_request(
        "DELETE",
        &format!("/api/settings/secrets/{}", name),
        None,
    )
    .await
    {
        Ok(_) => Ok(()),
        Err(e) if e.contains("HTTP 404") => Ok(()),
        Err(e) => Err(e),
    }
}

#[tauri::command]
async fn reveal_secret(name: String) -> Result<String, String> {
    let data = pipe_request(
        "POST",
        &format!("/api/settings/secrets/{}/reveal", name),
        None,
    )
    .await?;
    data["value"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "no value".to_string())
}

#[tauri::command]
async fn seed_secrets() -> Result<Vec<String>, String> {
    let data = pipe_request(
        "POST",
        "/api/settings/secrets/seed",
        Some(serde_json::json!({})),
    )
    .await?;
    let seeded = data["seeded"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Ok(seeded)
}

#[tauri::command]
async fn list_providers() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/providers", None).await
}

#[tauri::command]
async fn list_router_roles() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/router/roles", None).await
}

#[tauri::command]
async fn list_router_models() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/router/models", None).await
}

#[tauri::command]
async fn toggle_provider(id: String, enabled: bool) -> Result<(), String> {
    // PATCH /api/providers/{id} body {"enabled": bool}
    pipe_request(
        "PATCH",
        &format!("/api/providers/{}", id),
        Some(serde_json::json!({ "enabled": enabled })),
    )
    .await?;
    Ok(())
}

#[tauri::command]
async fn update_router_roles(roles: serde_json::Value) -> Result<(), String> {
    pipe_request(
        "PUT",
        "/api/router/roles",
        Some(serde_json::json!({ "roles": roles })),
    )
    .await?;
    Ok(())
}

#[tauri::command]
async fn list_plugins() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/plugins", None).await
}

#[tauri::command]
async fn fetch_provider_models(id: String) -> Result<serde_json::Value, String> {
    // Pulls the live model catalog from the provider's own /models
    // (or equivalent) endpoint. The Python side dispatches per
    // provider id (Ollama uses /api/tags, Gemini uses ?key=, etc.)
    // so the Rust shell just forwards.
    pipe_request("GET", &format!("/api/providers/{}/models", id), None).await
}

#[tauri::command]
async fn add_custom_provider(
    id: String,
    display_name: String,
    kind: String,
    base_url: String,
    api_key_env: String,
    models: Vec<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    // Register a user-defined provider (relay station / private
    // gateway). Body shape matches the Python `register_custom`
    // signature; the persistence happens in the pipe-server's
    // SQLite (custom_provider table).
    pipe_request(
        "POST",
        "/api/providers/custom",
        Some(serde_json::json!({
            "id": id,
            "display_name": display_name,
            "kind": kind,
            "base_url": base_url,
            "api_key_env": api_key_env,
            "models": models,
        })),
    )
    .await
}

#[tauri::command]
async fn remove_custom_provider(id: String) -> Result<serde_json::Value, String> {
    pipe_request("DELETE", &format!("/api/providers/custom/{}", id), None).await
}

#[tauri::command]
async fn invoke_plugin(
    name: String,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    pipe_request(
        "POST",
        &format!("/api/plugins/{}/invoke", name),
        Some(serde_json::json!({ "args": args })),
    )
    .await
}

#[tauri::command]
async fn start_workflow_cmd(
    state: tauri::State<'_, AppState>,
    text: String,
    project_id: Option<String>,
) -> Result<NewWorkflowResponse, String> {
    // Flatten at the Tauri boundary so the webview can pass each field
    // as a top-level arg (`invoke('start_workflow_cmd', { text, project_id })`).
    // The alternative `{ req: { text, ... } }` confused Tauri's IPC deserializer.
    start_workflow(state, NewWorkflowRequest { text, project_id }).await
}

#[tauri::command]
async fn get_workflow(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Option<serde_json::Value>, String> {
    state
        .repo
        .get_workflow(&id)
        .await
        .map(|opt| opt.map(workflow_to_json))
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cancel_workflow(id: String) -> Result<(), String> {
    // Was a no-op before; actually call the runtime so the orchestrator
    // can stop the running workflow.
    pipe_request(
        "POST",
        &format!("/api/workflow/{}/cancel", id),
        None,
    )
    .await?;
    Ok(())
}

fn workflow_to_json(wf: storage::Workflow) -> serde_json::Value {
    serde_json::json!({
        "id": wf.id,
        "createdAt": wf.created_at,
        "updatedAt": wf.updated_at,
        "state": wf.state,
        "phase": wf.phase,
        "userRequest": wf.user_request,
        "planDoc": wf.plan_doc,
        "summary": wf.summary,
        "finalStatus": wf.final_status.map(|s| match s {
            storage::WorkflowStatus::Active  => "ACTIVE",
            storage::WorkflowStatus::Done    => "DONE",
            storage::WorkflowStatus::Failed  => "FAILED",
            storage::WorkflowStatus::Aborted => "ABORTED",
        }),
        "totalInputTokens": wf.total_input_tokens,
        "totalOutputTokens": wf.total_output_tokens,
        "totalCostUsd": wf.total_cost_usd,
    })
}

// ── Entry point ──────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();

            // Bring up logging FIRST so every subsequent log line
            // (including the panic hook) lands in the file. We need
            // a data_dir to know where to write — derive it the
            // same way AppState::build does.
            if let Some(data_dir) = storage::Repository::default_data_dir() {
                let _ = std::fs::create_dir_all(&data_dir);
                let _ = LOGGING_GUARD.set(logging::init_logging(&data_dir));
            } else {
                eprintln!(
                    "[flowntier] cannot determine data_dir; logs will only go to stderr"
                );
            }

            tauri::async_runtime::block_on(async move {
                spawn_runtime_sidecar(&handle);
                match AppState::build().await {
                    Ok(state) => {
                        handle.manage(state);
                    }
                    Err(e) => {
                        // Show a native error dialog so the user
                        // gets a clear message instead of a silent
                        // process exit. The dialog plugin handles
                        // Windows MessageBox / macOS NSAlert /
                        // Linux GTK dialog automatically.
                        tracing::error!("failed to build AppState: {e}");
                        eprintln!("[flowntier] failed to build AppState: {}", e);

                        let log_path = storage::Repository::default_data_dir()
                            .map(|d| d.join("logs").display().to_string())
                            .unwrap_or_else(|| "(unavailable)".into());
                        let body = format!(
                            "Flowntier failed to start:\n\n{e}\n\nDiagnostic logs:\n{log_path}\n\n\
                             Please open a bug report at:\n  https://github.com/Thatgfsj/Flowntier/issues\n\n\
                             (Click OK to close.)"
                        );

                        // tauri-plugin-dialog's message() is async;
                        // we block on it inside the async setup so
                        // the user sees the dialog before exit.
                        use tauri_plugin_dialog::DialogExt;
                        let _ = handle
                            .dialog()
                            .message(body)
                            .title("Flowntier — startup failed")
                            .kind(tauri_plugin_dialog::MessageDialogKind::Error)
                            .blocking_show();

                        std::process::exit(1);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health_check,
            list_secrets, save_secret, delete_secret, reveal_secret, seed_secrets,
            list_providers, toggle_provider,
            list_router_roles, list_router_models, update_router_roles,
            list_plugins, invoke_plugin, fetch_provider_models,
            add_custom_provider, remove_custom_provider,
            start_workflow_cmd, get_workflow, cancel_workflow,
            run_agent_task,
            draw_i_ching,
            log_frontend_error,
            kv_get, kv_set,
            load_sample_workflow, first_run_complete,
            rpc_version,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
