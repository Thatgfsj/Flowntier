//! Standalone binary entry point.
//!
//! Reads `--workspace <dir>` (default: cwd) and `--data-dir
//! <dir>` (default: same as the storage `Repository::default_data_dir()`)
//! and starts the server. The Tauri shell can spawn this as a
//! sidecar, OR link it in-process via the `Server::run()` API.

use std::path::PathBuf;

use pipe_server::{
    logs, register_all, run_quota_scheduler, Dispatcher, Server,
    ServerConfig, ServerState,
};

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> std::io::Result<()> {
    // v0.4.22 (event 000080): set up the global tracing
    // subscriber FIRST so all the `tracing::info!` /
    // `tracing::warn!` calls below actually emit to stderr
    // (and to ~/Desktop/Flowntier.log when FLWNTIER_LOG_FILE
    // is set, which is the default). Per chairman: "日志
    // 暂时放桌面" — so the default is the desktop on
    // Windows. FLWNTIER_LOG_FILE=0 disables file logging.
    //
    // event 000109: install the panic hook AFTER logs::init
    // (so we can read the resolved log file path), but the
    // hook writes to the SAME file as the subscriber — no
    // more "panic goes to eprintln, log goes to file,
    // chairman greps the wrong place" inconsistency.
    let _log_file = logs::init();
    if let Some(path) = &_log_file {
        logs::install_panic_hook(path);
    }
    tracing::info!(target: "pipe_server", "[TRACE] v0.4.23 (event 000109): flowntier-runtime binary started — panic hook + token override");

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
    let state = std::sync::Arc::new(ServerState::new(workspace, data_dir.clone()).await);
    register_all(&mut d, (*state).clone());

    // event 000109: Resolve the bridge auth token WITHOUT touching
    // env vars. Priority:
    //   1. `FLOWNTIER_HTTP_BRIDGE_TOKEN` env var (already set
    //      by the caller — power users / shared deployments).
    //   2. Otherwise generate a 32-byte random hex token.
    //
    // We then pass the token DIRECTLY to
    // `run_http_bridge_on_with_token` (no env var round-trip
    // needed). The token is also written to
    // `<data_dir>/.bridge_token` so the portable HTML frontend
    // and the Tauri shell can read it for their Authorization
    // header.
    //
    // Before event 000109 we used `std::env::set_var(...)` to
    // stuff the generated token into the process env, then
    // `token_from_env()` read it back. That worked but
    // `std::env::set_var` is `unsafe` since Rust 1.84 (MSRV
    // 1.85 — we're already on the unsafe side) AND it polluted
    // the process env which the parallel test suite
    // (`cargo test`) saw as a global, making per-test token
    // isolation impossible without explicit `unsafe` blocks in
    // every test. Direct param-pass is faster, safer, and
    // makes the test suite cleaner.
    let bridge_token: Option<String> = if let Some(existing) =
        pipe_server::ws_bridge::token_from_env()
    {
        Some(existing)
    } else {
        use rand::Rng;
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill(&mut bytes[..]);
        let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
        let token_path = data_dir.join(".bridge_token");
        if let Err(e) = std::fs::write(&token_path, hex.as_bytes()) {
            tracing::warn!(
                target: "pipe_server",
                error = %e,
                path = %token_path.display(),
                "event 000109: failed to write .bridge_token; the portable HTML \
                 frontend will not be able to auth against the HTTP bridge"
            );
        } else {
            tracing::info!(
                target: "pipe_server",
                path = %token_path.display(),
                "event 000109: generated bridge token; portable HTML frontend \
                 should read this file and use it as `Authorization: Bearer <hex>`"
            );
        }
        Some(hex)
    };
    let _ = bridge_token.as_ref().map(|t| tracing::debug!(
        target: "pipe_server",
        token_len = t.len(),
        "event 000109: HTTP bridge auth token active"
    ));

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
    // event 000109: pass the resolved token DIRECTLY instead of
    // going through FLOWNTIER_HTTP_BRIDGE_TOKEN env var. Same
    // auth semantics (override > env var > generate-and-write),
    // no process-global env mutation, no `unsafe { set_var }`.
    // Bind synchronously up front (TcpListener::bind is sync
    // in std) so we know the listener is ready before
    // tokio::spawn starts polling.
    let bridge_listener = std::net::TcpListener::bind(&bind)
        .or_else(|_| std::net::TcpListener::bind(pipe_server::ws_bridge::DEFAULT_BIND))
        .expect("bind HTTP bridge listener");
    bridge_listener.set_nonblocking(true).expect("set_nonblocking");
    let bridge_listener = tokio::net::TcpListener::from_std(bridge_listener)
        .expect("convert std TcpListener to tokio");
    let bridge = tokio::spawn(pipe_server::run_http_bridge_on_with_token(
        bridge_listener,
        dispatcher_for_bridge,
        events_for_bridge,
        bridge_token,
    ));

    let events_for_server = state.events.clone();
    let server = Server::new(cfg, d, events_for_server);
    tokio::select! {
        r = server.run() => r,
        _ = bridge => Ok(()),
    }
}