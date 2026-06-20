//! Tauri app glue for the desktop shell. Bridges React commands to
//! the ACO `tauri-core` library.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;
use tauri::path::BaseDirectory;
use tauri_core::{start_workflow, AppState, NewWorkflowRequest, NewWorkflowResponse};

/// v0.2.3: Spawn the bundled Python runtime sidecar and wait for
/// it to be reachable on 127.0.0.1:7317. The exe ships with the
/// installer (bundle.externalBin in tauri.conf.json). If the
/// runtime is already running (e.g. dev mode with `aco-runtime`
/// in a terminal), we leave it alone.
fn spawn_runtime_sidecar(handle: &tauri::AppHandle) {
    use std::time::{Duration, Instant};
    const HEALTH_URL: &str = "http://127.0.0.1:7317/health";
    const SIDECAR_NAME: &str = "aco_runtime";

    // Already up?
    if ureq_get_status(HEALTH_URL, Duration::from_millis(200)).is_some() {
        tracing::info!("runtime already running on 7317, skipping sidecar spawn");
        return;
    }

    let resource_path = match handle.path().resolve(SIDECAR_NAME, BaseDirectory::Resource) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                "sidecar resource '{}' not found ({}); runtime will not start. \
                 Run `aco-runtime` in a terminal manually.",
                SIDECAR_NAME, e
            );
            return;
        }
    };

    tracing::info!("spawning runtime sidecar at {:?}", resource_path);
    let mut cmd = std::process::Command::new(&resource_path);
    if let Some(parent) = resource_path.parent() {
        cmd.current_dir(parent);
    }
    // Detach: don't hold a stdio handle on the GUI process.
    match cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => {
            tracing::info!("runtime sidecar pid={}", child.id());
            // Poll /health for up to 30s before giving up.
            let deadline = Instant::now() + Duration::from_secs(30);
            while Instant::now() < deadline {
                if ureq_get_status(HEALTH_URL, Duration::from_millis(500)).is_some() {
                    tracing::info!("runtime sidecar is healthy");
                    return;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            tracing::warn!("runtime sidecar did not become healthy in 30s");
        }
        Err(e) => {
            tracing::warn!("failed to spawn sidecar: {}", e);
        }
    }
}

fn ureq_get_status(url: &str, timeout: std::time::Duration) -> Option<u16> {
    // Minimal synchronous HTTP GET — avoid pulling in a crate.
    // Falls back to None on any error (treated as "not ready").
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpStream};
    let url = url.strip_prefix("http://")?;
    let (host_port, path) = match url.split_once('/') {
        Some((hp, rest)) => (hp, format!("/{}", rest)),
        None => (url, "/".to_string()),
    };
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => (h, p),
        None => return None,
    };
    let port: u16 = port.parse().ok()?;
    let addr: SocketAddr = format!("{}:{}", host, port).parse().ok()?;
    let mut stream = TcpStream::connect_timeout(&addr, timeout).ok()?;
    stream.set_read_timeout(Some(timeout)).ok()?;
    stream.set_write_timeout(Some(timeout)).ok()?;
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, host_port
    );
    stream.write_all(req.as_bytes()).ok()?;
    let mut buf = [0u8; 64];
    let n = stream.read(&mut buf).ok()?;
    let header = std::str::from_utf8(&buf[..n.min(32)]).ok()?;
    let mut parts = header.split_whitespace();
    let _ = parts.next()?; // HTTP/1.1
    parts.next()?.parse().ok()
}

#[tauri::command]
async fn start_workflow_cmd(
    state: tauri::State<'_, AppState>,
    req: NewWorkflowRequest,
) -> Result<NewWorkflowResponse, String> {
    start_workflow(state, req).await.map_err(|e| e)
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
async fn cancel_workflow(
    _state: tauri::State<'_, AppState>,
    _id: String,
) -> Result<(), String> {
    // Stub for Phase 0. Real impl in Phase 1.
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::block_on(async move {
                // v0.2.3: spawn the Python runtime sidecar so users
                // don't need to install Python themselves. The exe
                // ships with the installer (bundle.externalBin) and
                // listens on 127.0.0.1:7317. We poll /health for up
                // to 30s before giving up. AppState::build() talks
                // to this same endpoint.
                spawn_runtime_sidecar(&handle);
                match AppState::build().await {
                    Ok(state) => {
                        handle.manage(state);
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "failed to build AppState");
                        std::process::exit(1);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![start_workflow_cmd, get_workflow, cancel_workflow])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
