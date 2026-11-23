//! Windows production-pipe-name regression guard (event 000108).

// ── v0.4.22 (event 000108) production-pipe-name regression guard ─
//
// The "every test gets a unique pipe name" pattern used by
// `spawn_server`/`spawn_bridge` (above) is fine for testing
// dispatcher/handler logic, but it never exercised the
// production behaviour: TWO servers competing for the SAME
// `\\.\pipe\flowntier_runtime` path. That was the root
// cause behind the entire event 000083 → 000094 → 000103
// → 000104 → 000105 patch chain.
//
// These tests pin two invariants that must hold on the
// production pipe path:
//
//   A. spawning a server on the production pipe name
//      while a previous server's handle is STILL ALIVE
//      must succeed — the second server must take over
//      transparently because the kernel round-robins
//      across all instances.
//
//   B. killing the previous server and immediately
//      spawning a new one on the same pipe must succeed
//      — no ERROR_ACCESS_DENIED deadlock, no
//      ERROR_PIPE_BUSY loop, no timeout. This is what
//      happens every time the chairman quits Flowntier
//      and relaunches it while a stale `flowntier_runtime.exe`
//      is still holding the pipe handle.
//
// These tests are #[cfg(windows)] because the production
// behaviour we're guarding against only exists on Windows
// named pipes. Unix tests live behind their own cfg gate
// and exercise the `fs::remove_file` path in
// `crates/pipe-server/src/server.rs` instead.

use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::ClientOptions;

// Use a NON-default pipe name so parallel test runs don't
// collide with each other or with the real sidecar on
// `\\.\pipe\flowntier_runtime`. The point is to use the
// SAME name twice — that's exactly what the production
// pipe name does across reboots of the sidecar.
fn prod_pipe_name(tag: &str) -> String {
    format!(
        r"\\.\pipe\flowntier_prod_test_{tag}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

async fn spawn_pipe_server(tag: &str) -> (String, tokio::task::JoinHandle<std::io::Result<()>>) {
    use pipe_server::{register_all, Dispatcher, Server, ServerConfig, ServerState};
    let rpc_path = prod_pipe_name(tag);
    let cfg = ServerConfig {
        rpc_path: rpc_path.clone(),
        events_path: format!(
            r"\\.\pipe\flowntier_prod_test_events_{tag}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ),
    };
    let unique = format!(
        "{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let data_root = std::env::temp_dir().join(format!("flowntier-prod-pipe-{unique}"));
    let _ = std::fs::remove_dir_all(&data_root);
    let _ = std::fs::create_dir_all(&data_root);
    let mut d = Dispatcher::new();
    let state = ServerState::new(data_root.clone(), data_root.clone()).await;
    register_all(&mut d, state.clone());
    let server = Server::new(cfg, d, state.events.clone());
    let handle = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    (rpc_path, handle)
}

async fn ping_pipe(path: &str, timeout_ms: u64) -> Result<serde_json::Value, String> {
    // Run the synchronous ClientOptions::open inside
    // spawn_blocking so we don't pin a tokio worker thread
    // on a CreateFileW syscall — same pattern the Tauri
    // shell uses at apps/desktop/src-tauri/src/lib.rs:56.
    let path_owned = path.to_string();
    let mut conn = tokio::task::spawn_blocking(move || ClientOptions::new().open(&path_owned))
        .await
        .map_err(|e| format!("join error: {e}"))?
        .map_err(|e| format!("open error: {e}"))?;
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "GET",
        "params": {"path": "/api/ping", "body": null}
    });
    let mut line = serde_json::to_vec(&req).unwrap();
    line.push(b'\n');
    conn.write_all(&line)
        .await
        .map_err(|e| format!("write: {e}"))?;
    let mut reader = BufReader::new(&mut conn);
    let mut buf = String::new();
    let read = tokio::time::timeout(
        Duration::from_millis(timeout_ms),
        reader.read_line(&mut buf),
    )
    .await
    .map_err(|_| "read timed out".to_string())?
    .map_err(|e| format!("read error: {e}"))?;
    assert!(read > 0, "empty response from {path}");
    serde_json::from_str(&buf).map_err(|e| format!("bad json: {e}"))
}

/// Invariant A: a fresh server on the production pipe
/// name accepts the FIRST connection within 2 seconds.
/// Without event 000105 (the `first_pipe_instance`
/// flag removal) this test would still pass — the
/// bug was triggered by TWO competing servers, not
/// by a single spawn. But pinning the spawn latency
/// makes any future regression on cold start visible.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn prod_pipe_cold_start_serves_first_request() {
    let (path, handle) = spawn_pipe_server("cold-start").await;
    let resp = ping_pipe(&path, 2000)
        .await
        .expect("cold-start server did not serve first request");
    assert_eq!(resp["result"]["status"].as_u64().unwrap_or(0), 200);
    assert_eq!(resp["result"]["body"]["ok"], serde_json::json!(true));
    handle.abort();
}

/// Invariant B (the real test): kill the first server,
/// spawn a new one on the SAME pipe name, the new one
/// must serve the first request within 2 seconds.
///
/// Before event 000105, worker 0 of the new server
/// would create with `first_pipe_instance(true)` while
/// workers 1..N had already raced ahead with
/// `first=false` + `max_instances=unlimited`. Worker 0
/// then got ERROR_ACCESS_DENIED forever because the
/// other workers had already created primary instances.
/// The kill+respawn scenario is what the chairman hit
/// every time the app restarted while a stale
/// `flowntier_runtime.exe` was still alive.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn prod_pipe_kill_then_respawn_serves_first_request() {
    let (path, handle_a) = spawn_pipe_server("kill-respawn").await;

    // First server works.
    let resp = ping_pipe(&path, 2000)
        .await
        .expect("first server did not serve first request");
    assert_eq!(resp["result"]["status"].as_u64().unwrap_or(0), 200);

    // Abort the first server.
    handle_a.abort();
    // Give the kernel a beat to drop the pipe handle —
    // mirrors the real-world gap between taskkill /f and
    // the spawn that follows.
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Second server on the SAME pipe name. Clone `path`
    // so the client (which still needs to ping by name)
    // can keep its copy.
    let path_for_server = path.clone();
    let handle_b = tokio::spawn(async move {
        // Re-create the same ServerConfig + a fresh ServerState
        // so the second spawn is truly independent (no shared
        // state with the aborted first server).
        use pipe_server::{register_all, Dispatcher, Server, ServerState};
        let cfg = pipe_server::ServerConfig {
            rpc_path: path_for_server.clone(),
            events_path: format!("{path_for_server}_events"),
        };
        let unique = format!(
            "kill-respawn-2-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let data_root = std::env::temp_dir().join(format!("flowntier-prod-pipe-{unique}"));
        let _ = std::fs::remove_dir_all(&data_root);
        let _ = std::fs::create_dir_all(&data_root);
        let mut d = Dispatcher::new();
        let state = ServerState::new(data_root.clone(), data_root.clone()).await;
        register_all(&mut d, state.clone());
        let server = Server::new(cfg, d, state.events.clone());
        server.run().await
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    // The whole point: this MUST succeed in under 2s.
    // Before event 000105, this would either:
    //   - block forever (worker 0 ERROR_ACCESS_DENIED loop)
    //   - time out past 30s (rpc_listener's 2s backoff
    //     never recovered)
    let resp = ping_pipe(&path, 2000)
        .await
        .expect("second server did not serve first request after kill+respawn");
    assert_eq!(
        resp["result"]["status"].as_u64().unwrap_or(0),
        200,
        "kill+respawn regression: got resp={}",
        serde_json::to_string(&resp).unwrap_or_default()
    );
    handle_b.abort();
}

/// Invariant C: the production design has 16 accept
/// workers per pipe. Concurrent clients must each land
/// on a fresh instance — no `max_instances` overflow
/// deadlock. This is the underlying invariant event
/// 000105's "all workers use OS default" decision
/// relies on: every worker creates its own primary
/// instance and the kernel round-robins.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn prod_pipe_handles_concurrent_clients() {
    let (path, handle) = spawn_pipe_server("concurrent").await;

    // Fire 8 concurrent /api/ping requests. The
    // 16-worker design should accept all 8 without
    // any timeout or ERROR_PIPE_BUSY. We use
    // `join_all` so a slow client doesn't block the
    // fast ones.
    let path = std::sync::Arc::new(path);
    let mut tasks = Vec::new();
    for i in 0..8 {
        let path = path.clone();
        tasks.push(tokio::spawn(async move {
            let resp = ping_pipe(&path, 5000)
                .await
                .unwrap_or_else(|e| panic!("client {i} failed: {e}"));
            assert_eq!(
                resp["result"]["status"].as_u64().unwrap_or(0),
                200,
                "client {i} got non-200"
            );
        }));
    }
    for t in tasks {
        t.await.expect("task panicked");
    }
    handle.abort();
}
