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
    tracing::info!(target: "tauri_ipc", method = %method, path = %path, "[TRACE] pipe_request: opening pipe connection");
    let mut conn = ClientOptions::new()
        .open(RPC_PIPE)
        .map_err(|e| {
            tracing::error!(target: "tauri_ipc", error = %e, "[TRACE] pipe_request: pipe open FAILED");
            format!("pipe open {RPC_PIPE}: {e}")
        })?;
    tracing::debug!(target: "tauri_ipc", "[TRACE] pipe_request: pipe opened successfully");

    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": {"path": path, "body": body}
    });
    let mut line = serde_json::to_vec(&req).map_err(|e| e.to_string())?;
    line.push(b'\n');
    tracing::info!(target: "tauri_ipc", id = id, method = %method, path = %path, "[TRACE] pipe_request: writing request to pipe");
    conn.write_all(&line)
        .await
        .map_err(|e| {
            tracing::error!(target: "tauri_ipc", error = %e, "[TRACE] pipe_request: pipe write FAILED");
            format!("pipe write: {e}")
        })?;
    tracing::debug!(target: "tauri_ipc", id = id, "[TRACE] pipe_request: request written, reading response");

    let mut buf = Vec::with_capacity(4096);
    let mut byte = [0u8; 1];
    loop {
        conn.read_exact(&mut byte)
            .await
            .map_err(|e| {
                tracing::error!(target: "tauri_ipc", error = %e, id = id, "[TRACE] pipe_request: pipe read FAILED");
                format!("pipe read: {e}")
            })?;
        if byte[0] == b'\n' {
            break;
        }
        if buf.len() >= MAX_LINE {
            return Err(format!("pipe response exceeds {MAX_LINE} bytes"));
        }
        buf.push(byte[0]);
    }
    tracing::debug!(target: "tauri_ipc", id = id, resp_len = buf.len(), "[TRACE] pipe_request: response received, parsing JSON");

    let resp: serde_json::Value =
        serde_json::from_slice(&buf).map_err(|e| format!("pipe bad json: {e}"))?;

    if let Some(err) = resp.get("error") {
        let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("pipe error").to_string();
        tracing::warn!(target: "tauri_ipc", id = id, error = %msg, "[TRACE] pipe_request: server returned error");
        return Err(msg);
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
    tracing::info!(target: "tauri_events", "[TRACE] events_bridge: starting — connecting to events pipe");
    let mut backoff_ms = 200u64;
    loop {
        match ClientOptions::new().open(EVENTS_PIPE) {
            Ok(mut conn) => {
                tracing::info!(target: "tauri_events", "[TRACE] events_bridge: connected to events pipe");
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
                                    // The events pipe is newline-delimited JSON;
                                    // each line is a serialised value of one of
                                    // two shapes:
                                    //   - WfEvent   : {"event": "<kind>", ...}
                                    //   - AgentEvent: {"kind": "<kind>", ...}
                                    //
                                    // Both are forwarded to the webview under
                                    // the same `wf:event` channel — the
                                    // frontend `useAgentStream` hook filters
                                    // by `kind` field (AgentEvent) so WfEvent
                                    // payloads are ignored at the JS layer.
                                    //
                                    // v0.4.20 bug: only the WfEvent branch
                                    // was forwarded (v.get("event")) — chief
                                    // AgentEvent was silently dropped, leaving
                                    // the ChatZone transcript empty.
                                    if let Err(e) = app.emit("wf:event", v) {
                                        eprintln!("[flowntier] emit wf:event failed: {e}");
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

/// v0.4.22 (event 000068): kick off the 8-phase workflow
/// described in history/PROJECT_SPEC.md. The orchestrator runs
/// chief → plan → critic review → dispatch workers → final
/// review → repair → delivery, emitting a `PhaseTransition`
/// event on each step so the UI's PhaseTimeline animates.
///
/// Body: { task: "<user_request>" }. Optional `mode` field
/// selects "single" (legacy run_task) vs "workflow" (this
/// orchestrator) — for now we only support workflow.
#[tauri::command]
async fn run_workflow(body: serde_json::Value) -> Result<serde_json::Value, String> {
    tracing::info!(target: "tauri_ipc", "[TRACE] run_workflow Tauri command invoked from frontend");
    let result = pipe_request("POST", "/api/run_workflow", Some(body)).await;
    match &result {
        Ok(v) => tracing::info!(target: "tauri_ipc", response = %v, "[TRACE] run_workflow: success"),
        Err(e) => tracing::error!(target: "tauri_ipc", error = %e, "[TRACE] run_workflow: FAILED"),
    }
    result
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

// v0.4.21 (event 000063): webview console forward. The webview
// side has an inline script in apps/desktop/index.html that
// intercepts console.log / console.error / console.warn /
// window.onerror / unhandledrejection and invokes this command
// with one line at a time. We log each line under the
// `flowntier_webview_console` target so it lands in
// %APPDATA%\flowntier\logs\flowntier.log.<date>. Critical for
// diagnosing "首页打不开" reports in production builds where
// devtools are off and Tauri 2.x does NOT forward webview
// console to the Rust log by default.
#[tauri::command]
async fn log_webview_console(line: String) -> Result<(), String> {
    tracing::info!(target: "flowntier_webview_console", "{}", line);
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
    // v0.4.13: still returns String at the Tauri IPC boundary
    // (Tauri's serde_json serialization will clone it anyway),
    // but we explicitly null out the local pipe-response buffer
    // after extraction to minimise time-on-heap in this process.
    let data = pipe_request(
        "POST",
        &format!("/api/settings/secrets/{}/reveal", name),
        None,
    )
    .await?;
    let value = data["value"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "no value".to_string())?;
    // We can't zeroize the IPC payload (already serialized), but
    // we can drop the `data` reference now.
    drop(data);
    Ok(value)
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
async fn get_role_resolve_status(role: String) -> Result<serde_json::Value, String> {
    pipe_request(
        "GET",
        &format!("/api/router/roles/{role}/resolve"),
        Some(serde_json::json!({ "role": role })),
    )
    .await
}

// ── v0.4.20 quota tracker ─────────────────────────────────────────
// Surface the per-(role, model) quota state to the frontend.
// Settings → 角色额度状态 block consumes `get_quota_status`;
// the chairman's "重置" button calls `reset_quota`;
// `get_role_quota_status` is a convenience wrapper used by
// the inline StatusLine.

#[tauri::command]
async fn get_quota_status() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/quota/status", None).await
}

#[tauri::command]
async fn reset_quota(role: String, model_id: Option<String>) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({
        "role": role,
        "model_id": model_id,
    });
    pipe_request("POST", "/api/quota/reset", Some(body)).await
}

#[tauri::command]
async fn get_role_quota_status(role: String) -> Result<serde_json::Value, String> {
    // Forward to the resolve endpoint, which embeds quota_status
    // inline; the frontend unwraps `.quota_status` if present.
    pipe_request(
        "GET",
        &format!("/api/router/roles/{role}/resolve"),
        Some(serde_json::json!({ "role": role })),
    )
    .await
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
// BUG-FRONTEND-RT-13 (event 000044): the previous code
// proxied `list_plugins` through the pipe-server sidecar via
// `GET /api/plugins`. The sidecar doesn't have this route
// registered (only /api/rpc/version, /api/settings/secrets,
// /api/run_task, /api/plugins/[name]/[action]), so it returns
// 404 "no handler registered for path /api/plugins". The
// plugin system is currently a skeleton (no plugins
// implemented yet) — we just return an empty list directly
// from the Rust shell, no sidecar roundtrip needed. When the
// plugin system is actually built (v0.5+), this can be wired
// to a real registry (or kept in-process).
async fn list_plugins() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!([]))
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

/// Redact common API-key / token shapes from a log line before
/// it leaves the Tauri boundary. BUG-006 fix.
///
/// Handles 4 substring patterns:
///
///   1. `Bearer …` — Authorization header value in HTTP logs.
///   2. `sk-…` — OpenAI / Anthropic / DeepSeek key prefixes.
///   3. `sk_live_…` / `sk_test_…` — Stripe-style keys.
///   4. `KEY=value` for `*_KEY`, `*_TOKEN`, `*_SECRET`, `API_KEY`,
///      `PASSWORD` (uppercase keys; common env-var serialisation).
///
/// Implementation note: we use stdlib only (no regex dep). For
/// v0.5+, switch to the `regex` crate — these hand-rolled
/// scanners are correct for the tested inputs but conservative
/// about edge cases (e.g. values with embedded `=`).
fn redact_secrets(line: &str) -> String {
    let mut out = line.to_string();

    // Helper: replace all occurrences of `prefix` followed by
    // alphanumeric/_/- chars with `replacement` (no recursion —
    // each match is replaced independently).
    fn replace_token(
        s: &str,
        prefix: &str,
        replacement: &str,
        include_prefix: bool,
    ) -> String {
        let mut out = String::with_capacity(s.len());
        let bytes = s.as_bytes();
        let prefix_bytes = prefix.as_bytes();
        let plen = prefix_bytes.len();
        let mut i = 0;
        while i + plen <= bytes.len() {
            if &bytes[i..i + plen] == prefix_bytes {
                let after_start = i + plen;
                if include_prefix {
                    // Copy the prefix verbatim, then replace the
                    // value chars that follow.
                    let mut j = after_start;
                    while j < bytes.len()
                        && (bytes[j].is_ascii_alphanumeric()
                            || bytes[j] == b'_'
                            || bytes[j] == b'-')
                    {
                        j += 1;
                    }
                    out.push_str(prefix);
                    out.push_str(replacement);
                    i = j;
                } else {
                    // Don't copy the prefix; replace it AND the
                    // value chars that follow.
                    let mut j = after_start;
                    while j < bytes.len()
                        && (bytes[j].is_ascii_alphanumeric()
                            || bytes[j] == b'_'
                            || bytes[j] == b'-')
                    {
                        j += 1;
                    }
                    out.push_str(replacement);
                    i = j;
                }
            } else {
                // Copy one char (UTF-8 safe via char_indices).
                let c = s[i..].chars().next().unwrap();
                out.push(c);
                i += c.len_utf8();
            }
        }
        // Copy any trailing bytes we missed (shouldn't happen with
        // ASCII-only inputs but be safe).
        if i < bytes.len() {
            out.push_str(&s[i..]);
        }
        out
    }

    // Pattern 1: Bearer
    out = replace_token(&out, "Bearer ", "<redacted>", true);

    // Pattern 3 first (more specific): sk_live_/sk_test_ must run
    // BEFORE Pattern 2 so the longer prefix wins. We replace the
    // whole "sk_live_xxx" with "sk_live_<redacted>".
    for prefix in &["sk_live_", "sk_test_"] {
        // Manually: copy prefix, then run Pattern 2 logic on the
        // value chars that follow.
        let mut new_out = String::with_capacity(out.len());
        let bytes = out.as_bytes();
        let prefix_bytes = prefix.as_bytes();
        let plen = prefix_bytes.len();
        let mut i = 0;
        while i + plen <= bytes.len() {
            if &bytes[i..i + plen] == prefix_bytes {
                let mut j = i + plen;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric()
                        || bytes[j] == b'_'
                        || bytes[j] == b'-')
                {
                    j += 1;
                }
                new_out.push_str(prefix);
                new_out.push_str("<redacted>");
                i = j;
            } else {
                let c = out[i..].chars().next().unwrap();
                new_out.push(c);
                i += c.len_utf8();
            }
        }
        if i < bytes.len() {
            new_out.push_str(&out[i..]);
        }
        out = new_out;
    }

    // Pattern 2: sk- (any alphanumeric/-/_ after)
    out = replace_token(&out, "sk-", "sk-<redacted>", false);

    // Pattern 4: KEY=value for env-var-shaped secrets. We do a
    // single forward pass, looking for any of the keyword suffixes
    // preceded by uppercase letters/digits/_, followed by `=` or
    // `:` and a value. When found, replace the value with
    // `<redacted>`. Skip if the value already contains `<redacted>`
    // (Pattern 2/3 handled it).
    for keyword in &["_KEY", "_TOKEN", "_SECRET", "API_KEY", "PASSWORD"] {
        let klen = keyword.len();
        let bytes = out.as_bytes();
        let mut new_out = String::with_capacity(out.len());
        let mut i = 0;
        while i < bytes.len() {
            // Find the next occurrence of `keyword` starting at i.
            let mut j = i;
            let mut found = false;
            while j + klen <= bytes.len() {
                if &bytes[j..j + klen] == keyword.as_bytes() {
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                new_out.push_str(&out[i..]);
                break;
            }
            // Walk backwards from j to find key start.
            let mut key_start = j;
            while key_start > 0 {
                let prev = bytes[key_start - 1];
                if prev.is_ascii_uppercase() || prev.is_ascii_digit() || prev == b'_' {
                    key_start -= 1;
                } else {
                    break;
                }
            }
            // Need at least one char before the keyword.
            if key_start == j {
                // Skip past this keyword occurrence.
                new_out.push_str(&out[i..j + klen]);
                i = j + klen;
                continue;
            }
            // Copy everything from i up to key_end verbatim.
            let key_end = j + klen;
            new_out.push_str(&out[i..key_end]);
            // Skip whitespace, expect `=` or `:`.
            let mut sep = key_end;
            while sep < bytes.len()
                && (bytes[sep] == b' ' || bytes[sep] == b'\t')
            {
                sep += 1;
            }
            if sep >= bytes.len()
                || (bytes[sep] != b'=' && bytes[sep] != b':')
            {
                // No separator; skip past this keyword.
                i = key_end;
                continue;
            }
            new_out.push(bytes[sep] as char);
            let mut v = sep + 1;
            // Optional opening quote.
            if v < bytes.len() && (bytes[v] == b'"' || bytes[v] == b'\'') {
                new_out.push(bytes[v] as char);
                v += 1;
            }
            let v_start = v;
            while v < bytes.len() {
                let b = bytes[v];
                if b.is_ascii_whitespace()
                    || b == b',' || b == b'}' || b == b']'
                    || b == b'"' || b == b'\''
                {
                    break;
                }
                v += 1;
            }
            // Check if already redacted.
            let value_seg = &out[v_start..v];
            if value_seg.contains("<redacted>") {
                // Already handled — copy value verbatim.
                new_out.push_str(value_seg);
            } else {
                new_out.push_str("<redacted>");
            }
            i = v;
        }
        out = new_out;
    }

    out
}

/// Search the daily-rolling log file for lines containing the
/// given error code (e.g. "FE-3a7b9c2d"). Settings → About →
/// "Search my bug" panel calls this; the user pastes the code
/// shown on the ErrorBoundary screen and we surface every log
/// line that references it.
///
/// Returns a JSON envelope:
///   { "matches": string[], "scanned": number, "truncated": bool }
///
/// `matches` is capped at 200 lines (oldest first within each
/// file, files newest-first across the log dir) so a runaway
/// code doesn't melt the modal. `truncated` is set if we hit
/// that cap.
///
/// `since` is an ISO-8601 date string (e.g. "2026-06-26") used
/// to skip older log files. The frontend currently passes null
/// because the daily rolling files are small enough that
/// scanning all of them is fine for v0.4. We accept the
/// parameter so we don't need to bump the schema later.
///
/// Panic files (`panic-*.log`) are deliberately excluded —
/// they have a different format (free-form backtrace dump) and
/// false-positive heavily on user-provided error codes.
#[tauri::command]
fn search_log(
    code: String,
    since: Option<String>,
    include_panic_logs: Option<bool>,
) -> Result<serde_json::Value, String> {
    let needle = code.trim();
    if needle.is_empty() {
        return Err("code is empty".into());
    }
    let include_panic_logs = include_panic_logs.unwrap_or(false);
    let Some(data_dir) = storage::Repository::default_data_dir() else {
        return Err("cannot determine data dir".into());
    };
    let log_dir = tauri_core::logging::log_dir(&data_dir);
    let entries = match std::fs::read_dir(&log_dir) {
        Ok(e) => e,
        Err(e) => {
            // BUG-003 fix: a fresh install (or post-wipe_all_data)
            // has no logs/ dir yet. Treat as "empty result" rather
            // than an error — the user's UI shows "scanned 0 files"
            // instead of a scary "search failed" message.
            if e.kind() == std::io::ErrorKind::NotFound {
                return Ok(serde_json::json!({
                    "matches": Vec::<String>::new(),
                    "scanned": 0,
                    "truncated": false,
                }));
            }
            return Err(format!("read_dir {}: {e}", log_dir.display()));
        }
    };
    // Collect (path, modified) so we can scan newest-first. A
    // user who just hit the error wants to see today's lines
    // before yesterday's.
    //
    // BUG-010 fix (event 000023): panic dumps are now opt-in
    // via the `include_panic_logs` param. Default false because
    // panic dumps are hundreds of KB of backtrace and false-
    // positive heavily on substring searches, but the user
    // knows best when they're hunting a crash.
    let mut files: Vec<(std::path::PathBuf, std::time::SystemTime, u64)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?;
            let is_panic = name.starts_with("panic-");
            let is_log = name.starts_with("flowntier.log");
            if !(is_log || (include_panic_logs && is_panic)) {
                return None;
            }
            let meta = e.metadata().ok()?;
            let mtime = meta.modified().ok()?;
            let size = meta.len();
            Some((p, mtime, size))
        })
        .collect();
    files.sort_by(|a, b| b.1.cmp(&a.1));

    // `since` is reserved for the v0.5 frontend (when the panel
    // grows a date picker). For v0.4 the panel always passes
    // null because daily rolling files are small enough that
    // scanning all of them is cheap. We accept the param so we
    // don't have to bump the IPC schema later. Malformed input
    // is treated as "no filter" so a bad value can't brick the
    // modal.
    let _since_ignored = since;

    const MAX_MATCHES: usize = 200;
    /// BUG-004 fix: cap the bytes we'll read from a single log
    /// file at 64 MiB. Without this cap, a runaway log file
    /// (e.g. an agent loop spamming errors) could OOM the Tauri
    /// process. We stop reading at the cap but mark `truncated`
    /// so the user knows there's more content we didn't scan.
    const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
    /// BUG-004 fix: cap individual lines at 8 KiB. Without this
    /// cap, a single log line could be GB (e.g. an http body blob
    /// without newlines) and one bad file would still pin RAM.
    /// Lines longer than this are truncated and appended with
    /// `…[truncated]` so the user can still see what matched.
    const MAX_LINE_BYTES: usize = 8 * 1024;

    let mut matches: Vec<String> = Vec::new();
    let mut scanned: usize = 0;
    let mut truncated = false;

    'outer: for (path, _mtime, size) in files {
        // Skip files larger than MAX_FILE_BYTES upfront (cheap).
        if size > MAX_FILE_BYTES {
            truncated = true;
            continue;
        }
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => continue, // permission denied / file vanished — skip
        };
        let mut reader = std::io::BufReader::with_capacity(64 * 1024, file);
        let mut bytes_read: u64 = 0;
        let mut line = String::new();
        loop {
            line.clear();
            let n = match std::io::BufRead::read_line(&mut reader, &mut line) {
                Ok(0) => break, // EOF
                Ok(n) => n,
                Err(_) => break, // read error — skip rest of file
            };
            bytes_read += n as u64;
            if bytes_read > MAX_FILE_BYTES {
                truncated = true;
                break;
            }
            // Strip the trailing newline (or \r\n) so the trimmed
            // line we display doesn't have whitespace artifacts.
            let trimmed = line.trim_end_matches(|c| c == '\n' || c == '\r');
            // Per-line cap (BUG-004 fix).
            let truncated_line = if trimmed.len() > MAX_LINE_BYTES {
                let mut s = String::with_capacity(MAX_LINE_BYTES + 16);
                s.push_str(&trimmed[..MAX_LINE_BYTES]);
                s.push_str("…[truncated]");
                s
            } else {
                trimmed.to_string()
            };
            scanned += 1;
            if truncated_line.contains(needle) {
                // BUG-006 fix (event 000020): redact API keys,
                // bearer tokens, and similar secrets before
                // returning the line. The user might paste an
                // error code from the ErrorBoundary, but the
                // matched log line could contain
                // `OPENAI_API_KEY=sk-…` or an Authorization header
                // — sending that back to the webview exposes the
                // secret (and the user can copy it from the
                // modal). The redactor runs per-line, so
                // non-secret context is preserved.
                matches.push(redact_secrets(&truncated_line));
                if matches.len() >= MAX_MATCHES {
                    truncated = true;
                    break 'outer;
                }
            }
        }
    }

    Ok(serde_json::json!({
        "matches": matches,
        "scanned": scanned,
        "truncated": truncated,
    }))
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
async fn wipe_all_data() -> Result<(), String> {
    let Some(data_dir) = storage::Repository::default_data_dir() else {
        return Err("cannot determine data dir".into());
    };
    // Best-effort: kill any running sidecar first so its file
    // handles on storage.sqlite are released. We don't need the
    // sidecar to acknowledge — the rm -rf below will fail loudly
    // on Windows if the file is still locked.
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/f", "/im", "flowntier-runtime.exe"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        // Give the kernel a moment to actually release the handle.
        std::thread::sleep(Duration::from_millis(500));
    }
    // Recursively remove the data dir. Errors are surfaced to the
    // user (Settings modal shows a "Clear failed: <err>" message).
    std::fs::remove_dir_all(&data_dir)
        .map_err(|e| format!("rm {}: {e}", data_dir.display()))?;
    // Re-create the dir so the next launch's logging::init_logging
    // has somewhere to write. Empty dir + empty SQLite is the
    // "fresh install" state.
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| format!("mkdir {}: {e}", data_dir.display()))?;
    Ok(())
}


/// Persist the workspace workdir path. Called from the
/// WorkdirSetup dialog on first launch (and from Settings >
/// About > Change workdir). Stores the path in
/// <data_dir>/workdir.json so it survives quit+relaunch.
#[tauri::command]
async fn set_workdir(path: String) -> Result<(), String> {
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
async fn clear_workdir() -> Result<(), String> {
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
async fn get_diagnostics() -> Result<serde_json::Value, String> {
    use serde_json::json;
    let mut out = serde_json::Map::new();

    // Paths
    let data_dir = storage::Repository::default_data_dir();
    out.insert("data_dir".into(), json!(data_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "<unknown>".to_string())));
    if let Some(ref dd) = data_dir {
        out.insert("log_dir".into(), json!(tauri_core::logging::log_dir(dd).display().to_string()));
    } else {
        out.insert("log_dir".into(), json!(null));
    }

    // Env vars
    let env_keys = [
        "MINIMAX_API_KEY", "OPENAI_API_KEY", "ANTHROPIC_API_KEY",
        "GOOGLE_API_KEY", "DEEPSEEK_API_KEY", "MOONSHOT_API_KEY",
        "OPEN_BIGMODEL_API_KEY", "ZHIPUAI_API_KEY",
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
            dd.parent().map(|p| p.join("flowntier_runtime.exe")).unwrap_or_default(),
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

    out.insert("timestamp".into(), json!(unix_secs_to_iso8601(std::time::SystemTime::now())));

    Ok(serde_json::Value::Object(out))
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
async fn set_workdir_with_nwt(path: String) -> Result<String, String> {
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
        return Err(format!(
            "workdir is not a directory: {}",
            root.display()
        ));
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
    std::fs::create_dir_all(nwt_dir.join("indices"))
        .map_err(|e| format!("mkdir indices: {e}"))?;
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
        let bytes = serde_json::to_vec_pretty(&payload)
            .map_err(|e| format!("serialize metadata: {e}"))?;
        std::fs::write(&meta, bytes)
            .map_err(|e| format!("write metadata: {e}"))?;
    }
    let tags_idx = nwt_dir.join("indices").join("tags.json");
    if !tags_idx.exists() {
        std::fs::write(&tags_idx, b"{}\n")
            .map_err(|e| format!("write tags: {e}"))?;
    }
    let files_idx = nwt_dir.join("indices").join("files.json");
    if !files_idx.exists() {
        std::fs::write(&files_idx, b"{}\n")
            .map_err(|e| format!("write files: {e}"))?;
    }

    // Step 3: only NOW persist the workdir, atomically (tmp + rename
    // to avoid a torn write that would brick get_workdir).
    let wd_file = data_dir.join("workdir.json");
    let payload = serde_json::json!({ "workdir": path });
    let bytes = serde_json::to_vec_pretty(&payload)
        .map_err(|e| format!("serialize workdir: {e}"))?;
    let tmp = wd_file.with_extension("json.tmp");
    std::fs::write(&tmp, &bytes)
        .map_err(|e| format!("write tmp {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &wd_file)
        .map_err(|e| format!("rename to {}: {e}", wd_file.display()))?;

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
        match std::fs::write(&nrt_tmp, &bytes)
            .and_then(|_| std::fs::rename(&nrt_tmp, &nrt_file))
        {
            Ok(()) => tracing::info!(path = %nrt_file.display(), "nwt_root.json written"),
            Err(e) => tracing::warn!(error = %e, "failed to write nwt_root.json; agent-core nwt tool will see no root"),
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
        Ok(v) => tracing::info!(target: "tauri_runtime", response = %v, "v0.4.21 (event 000066): pipe-server workspace updated"),
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
async fn get_workdir() -> Result<Option<String>, String> {
    let Some(data_dir) = storage::Repository::default_data_dir() else {
        return Ok(None);
    };
    let p = data_dir.join("workdir.json");
    if !p.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&p)
        .map_err(|e| format!("read {}: {e}", p.display()))?;
    let v: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("parse {}: {e}", p.display()))?;
    Ok(v.get("workdir").and_then(|v| v.as_str()).map(|s| s.to_string()))
}

/// v0.4.21 (event 000066): read the pipe-server's *actual*
/// workspace root (the path the agent loop will use as the
/// chief's filesystem context). Distinct from `get_workdir`,
/// which reads the `workdir.json` on disk — they should agree
/// after a successful `set_workdir_with_nwt`, but in transient
/// states (sidecar just restarted, sidecar unreachable) they
/// can disagree. Both surfaces matter for diagnostics.
#[tauri::command]
async fn get_runtime_workspace() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/workspace", None).await
}

/// v0.4.21 (event 000066): list a directory tree under the
/// runtime workspace root. Body shape: `{ path?: string,
/// depth?: number, max_entries?: number }`. Used by the
/// `FileTree` component to render the chairman's project view.
#[tauri::command]
async fn get_workspace_tree(body: serde_json::Value) -> Result<serde_json::Value, String> {
    // GET-with-body: encode the body into query params so the
    // pipe-server side's dispatcher pattern match still hits
    // `/api/workspace/tree` (the dispatcher strips query before
    // matching; see event 000064 follow-up).
    let mut q = Vec::<String>::new();
    if let Some(p) = body.get("path").and_then(|v| v.as_str()) {
        if !p.is_empty() { q.push(format!("path={}", url_encode(p))); }
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

/// v0.4.21 (event 000066): minimal percent-encoder for the
/// query-string fields above. Keeps this command self-contained
/// — `urlencoding` crate is not pulled in.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => out.push(*b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// v0.4.21 (event 000066): read recent errors captured by the
/// pipe-server for the TopBar red-dot badge. Returns at most
/// `limit` entries (default 10). Used by the ErrorAggregator
/// polling job in App.tsx.
#[tauri::command]
async fn get_recent_errors(body: serde_json::Value) -> Result<serde_json::Value, String> {
    let limit = body.get("limit").and_then(|v| v.as_u64()).unwrap_or(10);
    let path = format!("/api/errors/recent?limit={limit}");
    pipe_request("GET", &path, None).await
}

/// v0.4.22 (event 000069): poll a workflow's current status.
/// The orchestrator runs on a background tokio task (POST
/// returns wf_id immediately); clients poll this to know when
/// the workflow finished + what the final summary is.
#[tauri::command]
async fn get_workflow_status(body: serde_json::Value) -> Result<serde_json::Value, String> {
    let wf_id = body.get("wf_id").and_then(|v| v.as_str()).unwrap_or("");
    if wf_id.is_empty() {
        return Err("missing 'wf_id'".into());
    }
    let path = format!("/api/workflow/{}/status", url_encode(wf_id));
    pipe_request("GET", &path, None).await
}

/// v0.4.22 (event 000080): dev-mode log endpoints.
/// Wraps the runtime's `GET /api/logs/get` so the Settings
/// panel can show a "View log" / "Clear log" pair of buttons.
/// The runtime gates the routes behind `FLWNTIER_LOG_API=1`;
/// this command just surfaces the gate to the user.
#[tauri::command]
async fn get_log_tail(body: serde_json::Value) -> Result<serde_json::Value, String> {
    let tail = body.get("tail").and_then(|v| v.as_u64()).unwrap_or(200);
    let path = format!("/api/logs/get?tail={tail}");
    pipe_request("GET", &path, None).await
}

#[tauri::command]
async fn clear_log_file() -> Result<serde_json::Value, String> {
    pipe_request("POST", "/api/logs/clear", None).await
}

/// v0.4.22 (event 000080): companion command that tells the
/// UI where the log file is, so the Settings panel can show
/// "open log file location" / show the path. Reads the env var
/// path on the runtime side via the route we just added
/// (the route returns `log_file` in its response, so this
/// command is a thin pass-through that just calls /api/logs/get
/// with tail=0 and surfaces the `log_file` field).
#[tauri::command]
async fn get_log_file_path() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/logs/get?tail=0", None).await
}

/// Convert a `SystemTime` to an ISO 8601 UTC string with second
/// precision (e.g. "2026-06-27T12:34:56Z"). Howard Hinnant's
/// `days_from_civil` algorithm — no chrono dep required.
///
/// Returns `"1970-01-01T00:00:00Z"` if `t` predates the unix
/// epoch (vanishingly unlikely but Rust's `SystemTime` allows it).
pub fn unix_secs_to_iso8601(t: std::time::SystemTime) -> String {
    use std::time::UNIX_EPOCH;
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let time_of_day = (secs % 86_400) as u32;
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, minute, second
    )
}

/// Howard Hinnant's `civil_from_days` (inverse of
/// `days_from_civil`). Given days since 1970-01-01, returns
/// (year, month, day). See http://howardhinnant.github.io/date_algorithms.html
pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = (yoe as i32) + (era as i32) * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// (Removed: standalone `nwt_init_workspace` command — folded
/// into `set_workdir_with_nwt` for BUG-016 atomicity. See event
/// 000022. App.tsx no longer calls it.)

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
            list_router_roles, list_router_models, update_router_roles, get_role_resolve_status,
            get_quota_status, reset_quota, get_role_quota_status,
            list_plugins, invoke_plugin, fetch_provider_models,
            add_custom_provider, remove_custom_provider,
            start_workflow_cmd, get_workflow, cancel_workflow,
            run_agent_task, run_workflow,
            draw_i_ching,
            log_frontend_error,
            log_webview_console,
            kv_get, kv_set,
            load_sample_workflow, first_run_complete,
            rpc_version,
            wipe_all_data,
            search_log,
            get_workdir, set_workdir, set_workdir_with_nwt, clear_workdir,
            get_diagnostics,
            get_runtime_workspace, get_workspace_tree, get_recent_errors,
            get_workflow_status,
            get_log_tail, clear_log_file, get_log_file_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
