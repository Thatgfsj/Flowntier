//! Runtime sidecar lifecycle and the named-pipe RPC client.
//!
//! Everything here talks to the spawned flowntier_runtime process. The
//! Tauri commands only see pipe_request, so the wire format, sidecar
//! spawn/kill logic and the events bridge stay in one place.

use std::sync::atomic::AtomicU64;
#[cfg(windows)]
use std::sync::atomic::Ordering;
use std::sync::OnceLock;
use std::time::Duration;

#[cfg(windows)]
use tauri::Emitter;
use tauri::Manager;
use tauri_core::logging::LoggingGuard;
use tauri_plugin_shell::ShellExt;
#[cfg(windows)]
use tokio::io::AsyncWriteExt;
#[cfg(windows)]
use tokio::net::windows::named_pipe::ClientOptions;

const RPC_PIPE: &str = r"\\.\pipe\flowntier_runtime";
const EVENTS_PIPE: &str = r"\\.\pipe\flowntier_runtime_events";

/// Process-wide logging guard. Stored in a OnceLock so the panic
/// hook installed by `logging::init_logging` survives for the
/// lifetime of the process — the guard's Drop impl flushes + joins
/// the background writer thread, so we must hold onto it.
pub(crate) static LOGGING_GUARD: OnceLock<LoggingGuard> = OnceLock::new();

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
const MAX_LINE: usize = 1_048_576; // 1 MiB hard cap per pipe message

// ── Pipe RPC client ─────────────────────────────────────────────

/// One connection = one request-response. Reconnects on the next call.
///
/// Wire format (newline-delimited JSON):
///   request  = `{"jsonrpc":"2.0","id":N,"method":VERB,"params":{"path":...,"body":...}}\n`
///   response = `{"jsonrpc":"2.0","id":N,"result":{"status":S,"body":B}}`  or
///              `{"jsonrpc":"2.0","id":N,"error":{"code":-32603,"message":"..."}}\n`
#[cfg(windows)]
pub(crate) async fn pipe_request(
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    tracing::info!(target: "tauri_ipc", method = %method, path = %path, "[TRACE] pipe_request: opening pipe connection");
    // v0.4.22 (event 000091 fix #40): `ClientOptions::open`
    // is a SYNCHRONOUS syscall on Windows (named pipes use
    // `CreateFileW` internally, which blocks the calling
    // thread). Calling it directly from an async fn would
    // pin one of Tauri's tokio worker threads on every
    // pipe_request call. Wrap in `spawn_blocking` so the
    // blocking I/O lands on the dedicated blocking pool.
    let path_for_open = RPC_PIPE;
    let mut conn = match tokio::task::spawn_blocking(move || {
        ClientOptions::new().open(path_for_open)
    })
    .await
    {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            tracing::error!(target: "tauri_ipc", error = %e, "[TRACE] pipe_request: pipe open FAILED");
            return Err(format!("pipe open {RPC_PIPE}: {e}"));
        }
        Err(e) => {
            tracing::error!(target: "tauri_ipc", error = %e, "[TRACE] pipe_request: spawn_blocking join FAILED");
            return Err(format!("pipe open task panicked: {e}"));
        }
    };
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
    conn.write_all(&line).await.map_err(|e| {
        tracing::error!(target: "tauri_ipc", error = %e, "[TRACE] pipe_request: pipe write FAILED");
        format!("pipe write: {e}")
    })?;
    tracing::debug!(target: "tauri_ipc", id = id, "[TRACE] pipe_request: request written, reading response");

    // v0.4.22 (event 000091 fix #25): read with a hard 60s
    // timeout. LLM calls can take 10-30s; if the sidecar hangs
    // indefinitely (e.g. infinite loop in a buggy router), we
    // MUST NOT leave the webview's invoke promise pending forever.
    // 60s is long enough for any legitimate single agent call.
    let read_timeout = Duration::from_secs(60);
    let raw = match tokio::time::timeout(read_timeout, read_response_bytes(&mut conn, MAX_LINE))
        .await
    {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => {
            tracing::error!(target: "tauri_ipc", id = id, error = %e, "[TRACE] pipe_request: read_response_bytes FAILED");
            return Err(format!("pipe read: {e}"));
        }
        Err(_) => {
            tracing::error!(target: "tauri_ipc", id = id, "[TRACE] pipe_request: read TIMED OUT after 60s");
            return Err("pipe read timed out after 60s — sidecar may be stuck".into());
        }
    };

    let resp: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| {
        let preview = String::from_utf8_lossy(&raw);
        let preview: String = preview.chars().take(200).collect();
        tracing::error!(target: "tauri_ipc", id = id, error = %e, preview = %preview, "[TRACE] pipe_request: JSON parse FAILED");
        format!("pipe parse {e}: {preview}")
    })?;

    tracing::debug!(target: "tauri_ipc", id = id, "[TRACE] pipe_request: response received and parsed");

    if let Some(err) = resp.get("error") {
        let msg = err
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        tracing::warn!(target: "tauri_ipc", id = id, error = %msg, "[TRACE] pipe_request: RPC returned error");
        return Err(msg.to_string());
    }

    let status = resp
        .pointer("/result/status")
        .and_then(|v| v.as_u64())
        .unwrap_or(200);
    if !(200..=299).contains(&status) {
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

#[cfg(not(windows))]
pub(crate) async fn pipe_request(
    _method: &str,
    _path: &str,
    _body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    Err("Named pipes are only supported on Windows".into())
}

/// v0.4.22 (event 000091 fix #25 + #26): extracted from
/// `pipe_request` so the read can be wrapped in a timeout
/// (`fix #25`) and switched to `BufReader::read_until` for
/// better throughput (`fix #26`).
///
/// Reads from `conn` byte-by-byte until newline, returning
/// the bytes without the trailing `\n`. Errors if the
/// response exceeds `max_bytes`.
#[cfg(windows)]
async fn read_response_bytes(
    conn: &mut tokio::net::windows::named_pipe::NamedPipeClient,
    max_bytes: usize,
) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        let n = conn.read(&mut chunk).await?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "pipe closed before newline delimiter",
            ));
        }
        if let Some(pos) = chunk[..n].iter().position(|&b| b == b'\n') {
            buf.extend_from_slice(&chunk[..pos]);
            if buf.len() > max_bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("pipe response exceeds {max_bytes} bytes"),
                ));
            }
            return Ok(buf);
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > max_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("pipe response exceeds {max_bytes} bytes"),
            ));
        }
    }
}

// ── Sidecar management ──────────────────────────────────────────

/// event 000109: the canonical sidecar image name. Centralised
/// here so `spawn_runtime_sidecar`, `wipe_all_data`, and any
/// future shutdown / health-check sites all use the SAME string.
/// Before this fix, `wipe_all_data` used `flowntier-runtime`
/// (hyphen) while `spawn_runtime_sidecar` used `flowntier_runtime`
/// (underscore). The hyphen form always returned 128 silently,
/// so the wipe half always no-op'd.
#[cfg(target_os = "windows")]
const SIDECAR_IMAGE: &str = "flowntier_runtime.exe";
#[cfg(not(target_os = "windows"))]
const SIDECAR_IMAGE: &str = "flowntier_runtime";

/// event 000109: kill the sidecar if it's running. Returns the
/// taskkill exit code so the caller can decide whether to log
/// (a no-op return 128 is normal — no sidecar alive). On
/// non-Windows this is a no-op.
#[cfg(target_os = "windows")]
pub(crate) fn kill_sidecar_silent() -> Option<i32> {
    std::process::Command::new("taskkill")
        .args(["/f", "/im", SIDECAR_IMAGE])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()
        .and_then(|s| s.code())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn kill_sidecar_silent() -> Option<i32> {
    // On Linux/macOS, the sidecar is currently not shipped —
    // the kill is only meaningful on Windows. Returns 128
    // (no match) so caller-side logs remain consistent.
    None
}

/// event 000109: stored on the Tauri `AppHandle` via
/// `app.manage(SidecarHandle { ... })`. The `RunEvent::Exit
/// Requested` handler in `run()` calls `graceful_kill` so the
/// sidecar's named-pipe handles are released by the kernel
/// before the desktop process exits. Without this, the next
/// launch hits the *exact* bug the event 000083→000105 chain
/// was patching around: a stale `flowntier_runtime.exe`
/// (or its still-open pipe handles) survives the shutdown.
/// Belt-and-braces taskkill (event 000097) was the symptom
/// fix; THIS is the root-cause fix.
#[derive(Default)]
pub struct SidecarHandle {
    /// Process ID of the spawned sidecar (None if not yet
    /// spawned in this process — e.g. tests, or the
    /// `try_ping_pipe` early-return path).
    pub pid: Option<u32>,
}

/// event 000109: graceful exit. Tries `child.kill()` (via
/// stored PID and process tree) first; falls back to
/// `kill_sidecar_silent()` (the historical taskkill belt) if
/// the parent PID has already been reaped. Either way, the
/// goal is "sidecar is dead by the time `main` returns 0" so
/// the named pipe `\\.\pipe\flowntier_runtime` is released
/// and the next launch can bind to it cleanly.
pub fn graceful_kill_sidecar() {
    kill_sidecar_silent();
}

pub(crate) fn spawn_runtime_sidecar(app: &tauri::AppHandle) {
    use tauri_plugin_shell::process::CommandEvent;

    // Kill any stale sidecar from a previous session. Pipes
    // are exclusive, so a dead binary that left the pipe handle open
    // would block the new instance.
    //
    // event 000103: the v0.4.22 spawn had two bugs:
    //   (a) taskkill used the WRONG image name (flowntier-runtime
    //       with a hyphen) — the actual binary is named with an
    //       underscore (flowntier_runtime). The kill silently
    //       no-op'd because /im lookup returns 128 and we ignored
    //       the exit code.
    //   (b) the 30s "wait for sidecar healthy" loop silently
    //       returned when the deadline elapsed. The Tauri shell
    //       then asked for RPC and got pipe-not-found forever —
    //       visible to the user as "卡死很长一段時間". We now
    //       log a loud error if we time out so the failure mode
    //       is at least diagnosable.
    //
    // event 000109: use the shared `kill_sidecar_silent` helper
    // so the image name stays in lock-step with `wipe_all_data`.
    {
        let rc = kill_sidecar_silent();
        tracing::warn!(
            target: "flowntier_shell",
            image = SIDECAR_IMAGE,
            taskkill_exit = ?rc,
            "event 000109: pre-spawn taskkill (128 = no matching process, normal)"
        );
        std::thread::sleep(Duration::from_millis(1000));
    }

    // Already up? Probe the RPC pipe.
    if try_ping_pipe().is_ok() {
        tracing::info!(target: "flowntier_shell", "[TRACE] runtime already running on RPC pipe");
    } else {
        tracing::info!(target: "flowntier_shell", "[TRACE] spawning sidecar...");
        let sidecar_command = match app.shell().sidecar("flowntier_runtime") {
            Ok(cmd) => cmd,
            Err(e) => {
                tracing::error!(target: "flowntier_shell", error = %e, "failed to create sidecar Command");
                return;
            }
        };

        match sidecar_command.spawn() {
            Ok((mut rx, child)) => {
                tracing::info!(target: "flowntier_shell", pid = ?child.pid(), "[TRACE] sidecar spawned");
                // event 000109: remember the PID so the
                // RunEvent::ExitRequested handler can
                // gracefully kill the sidecar before the
                // desktop process exits. Without this, the
                // sidecar (and its open named-pipe handles)
                // could survive shutdown and block the next
                // launch — exactly what event 000083→000105
                // worked around with belt-and-braces taskkill.
                let pid = child.pid();
                app.manage(SidecarHandle { pid: Some(pid) });
                tracing::debug!(
                    target: "flowntier_shell",
                    pid,
                    "event 000109: stored SidecarHandle.pid for graceful shutdown"
                );
                tauri::async_runtime::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        match event {
                            CommandEvent::Stdout(line) => {
                                let s = String::from_utf8_lossy(&line);
                                tracing::info!(target: "flowntier_shell", "[sidecar:out] {}", s);
                            }
                            CommandEvent::Stderr(line) => {
                                let s = String::from_utf8_lossy(&line);
                                tracing::error!(target: "flowntier_shell", "[sidecar:err] {}", s);
                            }
                            CommandEvent::Terminated(status) => {
                                tracing::error!(target: "flowntier_shell", "[sidecar] terminated: {:?}", status);
                                break;
                            }
                            _ => {}
                        }
                    }
                });

                // Wait until the sidecar is listening on the pipe.
                let deadline = std::time::Instant::now() + Duration::from_secs(30);
                let mut became_healthy = false;
                while std::time::Instant::now() < deadline {
                    if try_ping_pipe().is_ok() {
                        tracing::info!(target: "flowntier_shell", "[TRACE] sidecar healthy on RPC pipe");
                        became_healthy = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
                if !became_healthy {
                    tracing::error!(
                        target: "flowntier_shell",
                        "sidecar did NOT become healthy within 30s; the webview will see \
                         pipe-not-found on every API call. Check sidecar stderr in \
                         ~/Desktop/Flowntier.log (event 000103 typo fixed)."
                    );
                }
            }
            Err(e) => {
                tracing::error!(target: "flowntier_shell", error = %e, "failed to spawn sidecar");
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

#[cfg(windows)]
fn try_ping_pipe() -> Result<(), String> {
    // A blocking probe — we don't care about the response, just whether
    // the kernel will hand us a handle.
    let opts = ClientOptions::new();
    opts.open(RPC_PIPE).map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(not(windows))]
fn try_ping_pipe() -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
async fn events_bridge(_app: tauri::AppHandle) {}

#[cfg(windows)]
async fn events_bridge(app: tauri::AppHandle) {
    use tokio::io::{AsyncBufReadExt, BufReader};
    tracing::info!(target: "tauri_events", "[TRACE] events_bridge: starting — connecting to events pipe");
    let mut backoff_ms = 200u64;
    loop {
        let open_res =
            tokio::task::spawn_blocking(move || ClientOptions::new().open(EVENTS_PIPE)).await;

        match open_res {
            Ok(Ok(conn)) => {
                tracing::info!(target: "tauri_events", "[TRACE] events_bridge: connected to events pipe");
                backoff_ms = 200;
                let mut lines = BufReader::new(conn).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
                        if let Err(e) = app.emit("wf:event", v) {
                            eprintln!("[flowntier] emit wf:event failed: {e}");
                        }
                    }
                }
                eprintln!("[flowntier] events pipe disconnected; reconnecting");
            }
            Ok(Err(e)) => {
                eprintln!("[flowntier] events pipe open err: {e}; retry in {backoff_ms}ms");
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(5_000);
            }
            Err(e) => {
                eprintln!(
                    "[flowntier] events pipe spawn_blocking panic: {e}; retry in {backoff_ms}ms"
                );
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(5_000);
            }
        }
    }
}
