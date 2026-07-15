//! Standalone binary entry point.
//!
//! Reads `--workspace <dir>` (default: cwd) and `--data-dir
//! <dir>` (default: same as the storage `Repository::default_data_dir()`)
//! and starts the server. The Tauri shell can spawn this as a
//! sidecar, OR link it in-process via the `Server::run()` API.

use std::path::PathBuf;

use pipe_server::{
    logs, register_all, run_http_bridge, run_quota_scheduler, Dispatcher, Server,
    ServerConfig, ServerState,
};

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> std::io::Result<()> {
    // v0.4.22 (event 000080): set up the global tracing
    // subscriber FIRST so all the `tracing::info!` /
    // `tracing::warn!` calls below actually emit to stderr
    // (and to ~/Desktop/Flwntier.log when FLWNTIER_LOG_FILE
    // is set, which is the default). Per chairman: "日志
    // 暂时放桌面" — so the default is the desktop on
    // Windows. FLWNTIER_LOG_FILE=0 disables file logging.
    let _log_file = logs::init();
    tracing::info!(target: "pipe_server", "[TRACE] v0.4.22 (event 000084): flowntier-runtime binary started — trace logging active");

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

    // v0.4.22 (event 000085): read the persisted workdir from
    // `<data_dir>/workdir.json` and prefer it over the launch-time
    // cwd. The Tauri shell spawns this sidecar WITHOUT
    // `--workspace`, so without this fallback the runtime would
    // use the directory the sidecar was launched from (typically
    // the install dir like `O:\Flowntier`) and chief's file
    // writes would land there instead of the user's selected
    // `O:\try\…` workdir. The Tauri shell's
    // `set_workdir_with_nwt` command writes this JSON file
    // atomically (tmp + rename) and the same file is what
    // `get_workdir` reads back in the UI — keeping the runtime
    // in sync on cold start means chief never has a wrong
    // workspace even before the first `set_workspace` round-trip
    // happens. Best-effort: missing or malformed file → fall
    // back to cwd (preserves legacy behaviour).
    let workdir_file = data_dir.join("workdir.json");
    if workdir_file.exists() {
        match std::fs::read_to_string(&workdir_file) {
            Ok(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(v) => {
                    if let Some(p) = v.get("workdir").and_then(|x| x.as_str()) {
                        let candidate = PathBuf::from(p);
                        if candidate.is_dir() {
                            tracing::info!(
                                target: "pipe_server",
                                workdir = %candidate.display(),
                                "v0.4.22 (event 000085): restored workspace from workdir.json"
                            );
                            workspace = candidate;
                        } else {
                            tracing::warn!(
                                target: "pipe_server",
                                workdir = %candidate.display(),
                                "v0.4.22 (event 000085): workdir.json points at a non-directory; using cwd instead"
                            );
                        }
                    }
                }
                Err(e) => tracing::warn!(
                    target: "pipe_server",
                    error = %e,
                    path = %workdir_file.display(),
                    "v0.4.22 (event 000085): failed to parse workdir.json; using cwd"
                ),
            },
            Err(e) => tracing::warn!(
                target: "pipe_server",
                error = %e,
                path = %workdir_file.display(),
                "v0.4.22 (event 000085): failed to read workdir.json; using cwd"
            ),
        }
    }

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

    // v0.4.21 (event 000057): HTTP + SSE bridge for the portable
    // HTML frontend. Loopback only (127.0.0.1:8765 by default;
    // FLOWNTIER_HTTP_BRIDGE env var to override). Provides
    //   POST /rpc     — JSON-RPC 2.0
    //   GET  /events  — Server-Sent Events
    //   GET  /health  — health probe
    // Dies with the runtime process.
    let bind = pipe_server::ws_bridge::bind_from_env();
    let dispatcher_for_bridge = state.dispatcher().expect("dispatcher wired by register_all");
    let events_for_bridge = state.events.clone();
    let bridge = tokio::spawn(run_http_bridge(
        bind,
        dispatcher_for_bridge,
        events_for_bridge,
    ));

    let events_for_server = state.events.clone();
    let server = Server::new(cfg, d, events_for_server);
    tokio::select! {
        r = server.run() => r,
        _ = bridge => Ok(()),
    }
}