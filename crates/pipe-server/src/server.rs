//! Local IPC server.
//!
//! * **Windows**: `\\.\pipe\flowntier_runtime` (RPC) and
//!   `\\.\pipe\flowntier_runtime_events` (events). Multiple accept
//!   workers per pipe, see [CONCURRENCY] below.
//! * **Unix**: a Unix domain socket pair under the user's
//!   runtime dir (XDG_RUNTIME_DIR or ~/.cache/flowntier/sockets/).
//!
//! ## CONCURRENCY
//!
//! Windows named pipes are **per-instance**: a new client cannot
//! connect to a busy instance. We pre-spawn N accept workers
//! per pipe (16 for RPC, 4 for events) so concurrent clients
//! each land on a fresh instance.
//!
//! On Unix, `bind` + `listen` are already concurrency-safe; we
//! accept in a loop and dispatch each connection to a tokio task.

use crate::protocol::{RpcRequest, RpcResponse, MAX_LINE};
use crate::dispatcher::Dispatcher;
use agent_core::event::AgentEvent;
use std::sync::Arc;
use tokio::sync::broadcast;

#[cfg(windows)]
const RPC_WORKERS: usize = 16;
#[cfg(windows)]
const EVENTS_WORKERS: usize = 4;

#[cfg(not(windows))]
fn socket_paths() -> (std::path::PathBuf, std::path::PathBuf) {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            dirs_cache_dir().unwrap_or_else(|| std::env::temp_dir()).join("flowntier")
        });
    let _ = std::fs::create_dir_all(&base);
    (base.join("flowntier_runtime.sock"), base.join("flowntier_runtime_events.sock"))
}

#[cfg(not(windows))]
fn dirs_cache_dir() -> Option<std::path::PathBuf> {
    if let Some(home) = std::env::var_os("HOME") {
        return Some(std::path::PathBuf::from(home).join(".cache"));
    }
    None
}

/// Configuration for starting the server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub rpc_path: String,
    pub events_path: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        #[cfg(windows)]
        {
            Self {
                rpc_path: r"\\.\pipe\flowntier_runtime".into(),
                events_path: r"\\.\pipe\flowntier_runtime_events".into(),
            }
        }
        #[cfg(not(windows))]
        {
            let (rpc, events) = socket_paths();
            Self {
                rpc_path: rpc.to_string_lossy().into_owned(),
                events_path: events.to_string_lossy().into_owned(),
            }
        }
    }
}

/// Bundle of resources the server holds.
pub struct Server {
    pub cfg: ServerConfig,
    pub dispatcher: Arc<Dispatcher>,
    pub events_tx: broadcast::Sender<AgentEvent>,
}

impl Server {
    pub fn new(cfg: ServerConfig, dispatcher: Dispatcher, events_tx: broadcast::Sender<AgentEvent>) -> Self {
        Self {
            cfg,
            dispatcher: Arc::new(dispatcher),
            events_tx,
        }
    }

    /// Run until the process is killed. Returns when one of the
    /// accept loops exits (which usually means a fatal IO error).
    pub async fn run(self) -> std::io::Result<()> {
        let rpc_path = self.cfg.rpc_path.clone();
        let events_path = self.cfg.events_path.clone();
        let dispatcher = self.dispatcher.clone();
        let events_tx = self.events_tx.clone();

        let rpc_task = tokio::spawn(rpc_listener(rpc_path, dispatcher));
        let events_task = tokio::spawn(events_listener(events_path, events_tx));

        tokio::select! {
            r = rpc_task => match r {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(std::io::Error::other(e)),
            },
            r = events_task => match r {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(std::io::Error::other(e)),
            },
        }
    }
}

// ── Windows named-pipe accept loops ──────────────────────────────

#[cfg(windows)]
async fn rpc_listener(path: String, dispatcher: Arc<Dispatcher>) -> std::io::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    // v0.4.22 (event 000083): The first worker creates the
    // pipe (first_pipe_instance=true). The pipe path stays
    // alive as long as at least one worker instance is open.
    // The key bug was that all workers used
    // first_pipe_instance(false), and after the initial
    // instance was dropped (client disconnect), there was a
    // brief window where no instance existed on the path —
    // the next client got ERROR_FILE_NOT_FOUND. Setting
    // first_pipe_instance(true) on the first worker ensures
    // the pipe always has at least one "first" instance
    // alive, even when all serve_rpc_connection calls have
    // returned.
    let mut handles = Vec::with_capacity(RPC_WORKERS);
    for i in 0..RPC_WORKERS {
        let d = dispatcher.clone();
        let p = path.clone();
        let h = tokio::spawn(async move {
            // v0.4.22 (event 000094): always use
            // first_pipe_instance(false). The previous
            // "first worker creates the first instance" design
            // failed in practice — the kernel refuses the
            // second first-instance call with ERROR_ACCESS_DENIED
            // (os error 5) when a stale pipe survives from a
            // previous session, and also when the first worker
            // tries again after its first client disconnects.
            // With `first=false` every listener joins the
            // existing pipe and the OS handles creation
            // correctly. Tokio's `ServerOptions::create`
            // already creates the pipe on first call; the
            // FIRST_PIPE_INSTANCE flag only matters for
            // SECONDARY-instance creation semantics.
            loop {
                let server = match ServerOptions::new()
                    .first_pipe_instance(false)
                    .create(&p)
                {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(error = %e, worker = i, "[TRACE] rpc pipe create failed; backing off");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                };
                tracing::debug!(worker = i, "[TRACE] rpc pipe instance created, waiting for client connect");
                if let Err(e) = server.connect().await {
                    tracing::error!(error = %e, worker = i, "[TRACE] rpc pipe connect failed; retrying");
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    continue;
                }
                tracing::info!(worker = i, "[TRACE] rpc pipe client connected — entering serve_rpc_connection");
                if let Err(e) = serve_rpc_connection(server, d.clone()).await {
                    tracing::warn!(error = %e, worker = i, "[TRACE] rpc serve error");
                }
                tracing::debug!(worker = i, "[TRACE] rpc pipe client disconnected, looping to accept next");
            }
        });
        handles.push(h);
    }
    tracing::info!(rpc_workers = RPC_WORKERS, "rpc_listener started (first_pipe_instance=false; tokio handles primary creation)");
    futures::future::join_all(handles).await;
    Ok(())
}

#[cfg(windows)]
async fn events_listener(path: String, tx: broadcast::Sender<AgentEvent>) -> std::io::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let mut handles = Vec::with_capacity(EVENTS_WORKERS);
    for i in 0..EVENTS_WORKERS {
        let txc = tx.clone();
        let p = path.clone();
        let h = tokio::spawn(async move {
            let mut rx = txc.subscribe();
            // v0.4.22 (event 000094): same as rpc — use
            // first_pipe_instance(false) always; avoid the
            // os-error-5 refused access from a stale first
            // instance after a previous session.
            loop {
                let mut server = match ServerOptions::new()
                    .first_pipe_instance(false)
                    .create(&p)
                {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(error = %e, "events pipe create failed; backing off");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                };
                if let Err(e) = server.connect().await {
                    tracing::error!(error = %e, "events pipe connect failed; retrying");
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    continue;
                }
                loop {
                    match rx.recv().await {
                        Ok(ev) => {
                            let line = match serde_json::to_string(&ev) {
                                Ok(s) => s,
                                Err(e) => {
                                    tracing::warn!(error = %e, "event serialize failed");
                                    continue;
                                }
                            };
                            use tokio::io::AsyncWriteExt;
                            let _ = server.write_all(line.as_bytes()).await;
                            let _ = server.write_all(b"\n").await;
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                let _ = server.disconnect();
            }
        });
        handles.push(h);
    }
    futures::future::join_all(handles).await;
    Ok(())
}

#[cfg(windows)]
async fn serve_rpc_connection(
    mut conn: tokio::net::windows::named_pipe::NamedPipeServer,
    dispatcher: Arc<Dispatcher>,
) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut reader = BufReader::new(&mut conn);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(conn);
        }
        if line.len() > MAX_LINE {
            drop(reader);
            let _ = conn
                .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":0,\"error\":{\"code\":-32700,\"message\":\"line too long\"}}\n")
                .await;
            reader = BufReader::new(&mut conn);
            continue;
        }
        let resp = handle_one(&line, dispatcher.clone()).await;
        let mut out = serde_json::to_vec(&resp).unwrap_or_default();
        out.push(b'\n');
        drop(reader);
        conn.write_all(&out).await?;
        conn.flush().await?;
        reader = BufReader::new(&mut conn);
    }
}

// ── Unix socket accept loops ─────────────────────────────────────

#[cfg(not(windows))]
async fn rpc_listener(path: String, dispatcher: Arc<Dispatcher>) -> std::io::Result<()> {
    // Stale socket from a previous run.
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path)?;
    // Drop guard: remove the socket file on graceful exit so the
    // next start doesn't see a stale inode.
    let path_for_cleanup = path.clone();
    let _cleanup = scopeguard::guard((), |_| {
        let _ = std::fs::remove_file(&path_for_cleanup);
    });
    let mut incoming = listener.incoming();
    while let Some(stream) = incoming.next().await {
        match stream {
            Ok(s) => {
                let d = dispatcher.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve_rpc_connection(s, d).await {
                        tracing::warn!(error = %e, "rpc serve error");
                    }
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "rpc accept error");
            }
        }
    }
    Ok(())
}

#[cfg(not(windows))]
async fn events_listener(path: String, tx: broadcast::Sender<AgentEvent>) -> std::io::Result<()> {
    // Stale socket from a previous run — remove before bind.
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path)?;
    // Drop guard: remove the socket file on graceful exit so the
    // next start doesn't see a stale inode.
    let path_for_cleanup = path.clone();
    let _cleanup = scopeguard::guard((), |_| {
        let _ = std::fs::remove_file(&path_for_cleanup);
    });
    let mut incoming = listener.incoming();
    while let Some(stream) = incoming.next().await {
        match stream {
            Ok(mut s) => {
                let mut rx = tx.subscribe();
                tokio::spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(ev) => {
                                let line = serde_json::to_string(&ev).unwrap_or_default();
                                let _ = s.write_all(line.as_bytes()).await;
                                let _ = s.write_all(b"\n").await;
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });
            }
            Err(e) => {
                tracing::warn!(error = %e, "events accept error");
            }
        }
    }
    Ok(())
}

#[cfg(not(windows))]
async fn serve_rpc_connection(
    conn: tokio::net::UnixStream,
    dispatcher: Arc<Dispatcher>,
) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    let (read_half, mut write_half) = conn.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            return Ok(());
        }
        if line.len() > MAX_LINE {
            let _ = write_half
                .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":0,\"error\":{\"code\":-32700,\"message\":\"line too long\"}}\n")
                .await;
            continue;
        }
        let resp = handle_one(&line, dispatcher.clone()).await;
        let mut out = serde_json::to_vec(&resp).unwrap_or_default();
        out.push(b'\n');
        write_half.write_all(&out).await?;
    }
}

async fn handle_one(line: &str, dispatcher: Arc<Dispatcher>) -> RpcResponse {
    tracing::info!(target: "pipe_server", "[TRACE] handle_one: raw request = {}", line.trim());
    let req: RpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(target: "pipe_server", error = %e, "[TRACE] handle_one: JSON parse FAILED");
            return RpcResponse::err(0, crate::protocol::codes::PARSE, format!("bad json: {e}"));
        }
    };
    if req.jsonrpc != "2.0" {
        tracing::warn!(target: "pipe_server", "[TRACE] handle_one: invalid jsonrpc version = {}", req.jsonrpc);
        return RpcResponse::err(req.id, crate::protocol::codes::INVALID, "jsonrpc must be 2.0");
    }
    tracing::info!(
        target: "pipe_server",
        id = req.id,
        method = %req.method,
        path = %req.params.path,
        "[TRACE] handle_one: dispatching to handler"
    );
    let resp = dispatcher.dispatch(req.id, req).await;
    if resp.error.is_some() {
        tracing::warn!(target: "pipe_server", error = ?resp.error, "[TRACE] handle_one: handler returned error");
    } else {
        tracing::info!(target: "pipe_server", id = resp.id, "[TRACE] handle_one: handler returned success");
    }
    resp
}