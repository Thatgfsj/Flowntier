//! Standalone binary entry point.
//!
//! Reads `--workspace <dir>` (default: cwd) and `--data-dir
//! <dir>` (default: same as the storage `Repository::default_data_dir()`)
//! and starts the server. The Tauri shell can spawn this as a
//! sidecar, OR link it in-process via the `Server::run()` API.

use std::path::PathBuf;

use pipe_server::{register_all, run_quota_scheduler, Dispatcher, Server, ServerConfig, ServerState};

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut data_dir: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--workspace" => {
                if let Some(v) = args.next() {
                    workspace = PathBuf::from(v);
                }
            }
            "--data-dir" => {
                if let Some(v) = args.next() {
                    data_dir = Some(PathBuf::from(v));
                }
            }
            "--rpc" => {
                eprintln!("--rpc override is honoured by FLOWNTIER_RPC_PIPE env var instead");
            }
            _ => {
                eprintln!("ignoring unknown arg: {arg}");
            }
        }
    }
    // Default: OS-specific app data dir.
    let data_dir = data_dir.unwrap_or_else(|| {
        storage::Repository::default_data_dir()
            .unwrap_or_else(|| workspace.clone())
    });

    let cfg = ServerConfig::default();
    tracing::info!(
        rpc = %cfg.rpc_path,
        events = %cfg.events_path,
        workspace = %workspace.display(),
        data_dir = %data_dir.display(),
        "starting flowntier-runtime (Rust)"
    );

    let mut d = Dispatcher::new();
    let state = std::sync::Arc::new(ServerState::new(workspace, data_dir).await);
    register_all(&mut d, (*state).clone());

    // v0.4.20 (event 000056): background quota scheduler.
    // Spawned AFTER register_all so state.dispatcher() returns Some.
    // Dies with the runtime process. Pending_5h_wait rows persist in
    // SQLite and the next process restart will pick them up.
    let _scheduler = tokio::spawn(run_quota_scheduler(state.clone()));

    let server = Server::new(cfg, d, state.events.clone());
    server.run().await
}