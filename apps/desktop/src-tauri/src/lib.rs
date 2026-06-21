//! Tauri app glue for the desktop shell.
//!
//! Architecture: React → invoke() → Rust → HTTP → Python Runtime
//! Frontend NEVER touches HTTP directly. All API calls go through Tauri commands.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use tauri::Manager;
use tauri_plugin_shell::ShellExt;
use tauri_core::{start_workflow, AppState, NewWorkflowRequest, NewWorkflowResponse};

const RUNTIME_URL: &str = "http://127.0.0.1:7317";

// ── Sidecar management ──────────────────────────────────────────

fn spawn_runtime_sidecar(app: &tauri::AppHandle) {
    use std::time::{Duration, Instant};
    const HEALTH_URL: &str = "http://127.0.0.1:7317/health";

    // Kill stale processes
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/f", "/im", "aco_runtime.exe"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        std::thread::sleep(Duration::from_millis(1000));
    }

    // Already up?
    if ureq_get_status(HEALTH_URL, Duration::from_millis(500)).is_some() {
        println!("[aco] runtime already running");
        return;
    }

    println!("[aco] spawning sidecar...");
    let sidecar_command = match app.shell().sidecar("aco_runtime") {
        Ok(cmd) => cmd,
        Err(e) => {
            eprintln!("[aco] failed to create sidecar: {}", e);
            return;
        }
    };

    match sidecar_command.spawn() {
        Ok((mut rx, child)) => {
            println!("[aco] sidecar pid={:?}", child.pid());
            tauri::async_runtime::spawn(async move {
                use tauri_plugin_shell::process::CommandEvent;
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

            // Wait for health
            let deadline = Instant::now() + Duration::from_secs(30);
            while Instant::now() < deadline {
                if ureq_get_status(HEALTH_URL, Duration::from_millis(500)).is_some() {
                    println!("[aco] sidecar healthy!");
                    return;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            eprintln!("[aco] sidecar timeout");
        }
        Err(e) => {
            eprintln!("[aco] failed to spawn: {}", e);
        }
    }
}

fn ureq_get_status(url: &str, timeout: std::time::Duration) -> Option<u16> {
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
    let req = format!("GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", path, host_port);
    stream.write_all(req.as_bytes()).ok()?;
    let mut buf = [0u8; 64];
    let n = stream.read(&mut buf).ok()?;
    let header = std::str::from_utf8(&buf[..n.min(32)]).ok()?;
    let mut parts = header.split_whitespace();
    let _ = parts.next()?;
    parts.next()?.parse().ok()
}

// ── HTTP helper (Rust → Python) ─────────────────────────────────

fn runtime_get(path: &str) -> Result<serde_json::Value, String> {
    let url = format!("{}{}", RUNTIME_URL, path);
    let resp = reqwest::blocking::get(&url).map_err(|e| e.to_string())?;
    resp.json().map_err(|e| e.to_string())
}

fn runtime_post(path: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
    let url = format!("{}{}", RUNTIME_URL, path);
    let client = reqwest::blocking::Client::new();
    let resp = client.post(&url).json(&body).send().map_err(|e| e.to_string())?;
    resp.json().map_err(|e| e.to_string())
}

fn runtime_put(path: &str, body: serde_json::Value) -> Result<u16, String> {
    let url = format!("{}{}", RUNTIME_URL, path);
    let client = reqwest::blocking::Client::new();
    let resp = client.put(&url).json(&body).send().map_err(|e| e.to_string())?;
    Ok(resp.status().as_u16())
}

fn runtime_delete(path: &str) -> Result<u16, String> {
    let url = format!("{}{}", RUNTIME_URL, path);
    let client = reqwest::blocking::Client::new();
    let resp = client.delete(&url).send().map_err(|e| e.to_string())?;
    Ok(resp.status().as_u16())
}

// ── Tauri commands (React → Rust) ───────────────────────────────

#[tauri::command]
fn health_check() -> Result<bool, String> {
    match ureq_get_status(&format!("{}/health", RUNTIME_URL), std::time::Duration::from_millis(2000)) {
        Some(200) => Ok(true),
        _ => Ok(false),
    }
}

#[tauri::command]
fn list_secrets() -> Result<serde_json::Value, String> {
    runtime_get("/api/settings/secrets")
}

#[tauri::command]
fn save_secret(name: String, value: String) -> Result<(), String> {
    let status = runtime_put(
        &format!("/api/settings/secrets/{}", name),
        serde_json::json!({ "value": value }),
    )?;
    if status >= 200 && status < 300 {
        // Seed to env
        let _ = runtime_post("/api/settings/secrets/seed", serde_json::json!(null));
        Ok(())
    } else {
        Err(format!("HTTP {}", status))
    }
}

#[tauri::command]
fn delete_secret(name: String) -> Result<(), String> {
    let status = runtime_delete(&format!("/api/settings/secrets/{}", name))?;
    if status >= 200 && status < 300 || status == 404 {
        Ok(())
    } else {
        Err(format!("HTTP {}", status))
    }
}

#[tauri::command]
fn reveal_secret(name: String) -> Result<String, String> {
    let data = runtime_post(
        &format!("/api/settings/secrets/{}/reveal", name),
        serde_json::json!(null),
    )?;
    data["value"].as_str().map(|s| s.to_string()).ok_or_else(|| "no value".to_string())
}

#[tauri::command]
fn seed_secrets() -> Result<Vec<String>, String> {
    let data = runtime_post("/api/settings/secrets/seed", serde_json::json!(null))?;
    let seeded = data["seeded"].as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    Ok(seeded)
}

#[tauri::command]
fn list_providers() -> Result<serde_json::Value, String> {
    runtime_get("/api/providers")
}

#[tauri::command]
fn list_router_roles() -> Result<serde_json::Value, String> {
    runtime_get("/api/router/roles")
}

#[tauri::command]
fn list_router_models() -> Result<serde_json::Value, String> {
    runtime_get("/api/router/models")
}

#[tauri::command]
fn toggle_provider(id: String, enabled: bool) -> Result<(), String> {
    let status = runtime_post(
        &format!("/api/providers/{}", id),
        serde_json::json!({ "enabled": enabled }),
    )?;
    if status.is_object() { Ok(()) } else { Ok(()) }
}

#[tauri::command]
fn update_router_roles(roles: serde_json::Value) -> Result<(), String> {
    let status = runtime_put("/api/router/roles", serde_json::json!({ "roles": roles }))?;
    if status >= 200 && status < 300 {
        Ok(())
    } else {
        Err(format!("HTTP {}", status))
    }
}

#[tauri::command]
fn list_plugins() -> Result<serde_json::Value, String> {
    runtime_get("/api/plugins")
}

#[tauri::command]
fn invoke_plugin(name: String, args: serde_json::Value) -> Result<serde_json::Value, String> {
    runtime_post(&format!("/api/plugins/{}/invoke", name), serde_json::json!({ "args": args }))
}

#[tauri::command]
async fn start_workflow_cmd(
    state: tauri::State<'_, AppState>,
    req: NewWorkflowRequest,
) -> Result<NewWorkflowResponse, String> {
    start_workflow(state, req).await
}

#[tauri::command]
async fn get_workflow(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Option<serde_json::Value>, String> {
    state.repo.get_workflow(&id).await
        .map(|opt| opt.map(workflow_to_json))
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cancel_workflow(
    _state: tauri::State<'_, AppState>,
    _id: String,
) -> Result<(), String> {
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
        .plugin(tauri_plugin_http::init())
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::block_on(async move {
                spawn_runtime_sidecar(&handle);
                match AppState::build().await {
                    Ok(state) => { handle.manage(state); }
                    Err(e) => {
                        eprintln!("[aco] failed to build AppState: {}", e);
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
            list_plugins, invoke_plugin,
            start_workflow_cmd, get_workflow, cancel_workflow,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
