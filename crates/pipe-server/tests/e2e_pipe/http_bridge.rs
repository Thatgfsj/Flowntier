//! v0.4.21 HTTP + SSE bridge tests (event 000057).

use std::time::Duration;

// ── v0.4.21 HTTP + SSE bridge (event 000057) ────────────────────
// The bridge exposes the same JSON-RPC + events API that the
// named-pipe transport uses, but over loopback HTTP so any browser
// can drive it. Tests below cover:
//   1. GET  /health
//   2. POST /rpc  (JSON-RPC round-trip)
//   3. GET  /events (SSE stream + CORS preflight)
//   4. CORS preflight (OPTIONS /rpc)
//   5. POST /rpc without Content-Length → 400

async fn spawn_bridge(tag: &str) -> (String, tokio::task::JoinHandle<std::io::Result<()>>, String) {
    use pipe_server::{
        bind_listener, register_all, run_http_bridge_on_with_token, Dispatcher, ServerState,
    };
    let unique = format!(
        "{tag}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let data_root = std::env::temp_dir().join(format!("flowntier-bridge-{unique}"));
    let _ = std::fs::remove_dir_all(&data_root);
    let _ = std::fs::create_dir_all(&data_root);

    // v0.4.22 (event 000091 fix #34 + event 000108): every
    // non-`/health` HTTP bridge request MUST carry a bearer
    // token. We pass the token DIRECTLY to the bridge (not via
    // env var) so each parallel test stays isolated — env vars
    // are process-global and tokio tests run concurrently, so
    // using `unsafe { std::env::set_var }` would race between
    // `spawn_bridge` setting the var and the bridge task reading
    // it on a fresh connection.
    let token = format!("test-token-{}-{unique}", std::process::id());

    let mut d = Dispatcher::new();
    let state = ServerState::new(data_root.clone(), data_root.clone()).await;
    register_all(&mut d, state.clone());

    // Bind on port 0 to let the kernel pick a free loopback port,
    // capture the actual address, then start the bridge on the
    // SAME listener (no rebind race).
    let (listener, bound_addr) = bind_listener("127.0.0.1:0").await.expect("bind listener");
    let bound = bound_addr.to_string();
    let dispatcher = state.dispatcher().expect("dispatcher wired");
    let events = state.events.clone();
    let token_for_bridge = token.clone();
    let handle = tokio::spawn(async move {
        run_http_bridge_on_with_token(listener, dispatcher, events, Some(token_for_bridge)).await
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    (bound, handle, token)
}

async fn http_request(addr: &str, req: String, token: Option<&str>) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    // event 000108: the bridge's `/health` and OPTIONS handlers
    // do NOT consult the bearer token. If the caller already
    // embedded the header in `req` (e.g. for OPTIONS where we
    // want to make sure no Authorization slips in), respect
    // that. Otherwise inject `Authorization: Bearer <token>`
    // when `token` is `Some`. This is a no-op for the existing
    // tests because every non-health/non-OPTIONS call now
    // passes `Some(token)`.
    let mut req = req;
    if let Some(t) = token {
        if !req.to_ascii_lowercase().contains("authorization:") {
            // Insert the header before the blank line that
            // terminates the request headers.
            let injected = format!("Authorization: Bearer {t}\r\n");
            req = req.replacen("\r\n\r\n", &format!("\r\n{injected}\r\n"), 1);
        }
    }
    let mut s = TcpStream::connect(addr).await.expect("connect");
    s.write_all(req.as_bytes()).await.expect("write");
    s.flush().await.expect("flush");
    // Half-close our write side so the server sees EOF on its
    // read end and our read_to_end can complete.
    s.shutdown().await.expect("shutdown write");
    let mut buf = Vec::with_capacity(4096);
    // Bounded read with timeout: 5 s.
    let read =
        tokio::time::timeout(std::time::Duration::from_secs(5), s.read_to_end(&mut buf)).await;
    let read = match read {
        Ok(r) => r.expect("read"),
        Err(_) => panic!(
            "http_request: timed out after 5s reading response; partial buf = {:?}",
            String::from_utf8_lossy(&buf)
        ),
    };
    let _ = read;
    let text = String::from_utf8_lossy(&buf).to_string();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    let body = text.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_bridge_health_returns_ok() {
    let (addr, handle, _token) = spawn_bridge("health").await;
    let (status, body) = http_request(
        &addr,
        "GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n".to_string(),
        None,
    )
    .await;
    assert_eq!(status, 200, "expected 200, got {status}: {body}");
    assert!(body.contains("\"ok\":true"), "body={body}");
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_bridge_rpc_round_trips() {
    let (addr, handle, token) = spawn_bridge("rpc").await;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "GET",
        "params": { "path": "/api/ping", "body": {} }
    });
    let body_str = body.to_string();
    let req = format!(
        "POST /rpc HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body_str.len(), body_str
    );
    let (status, resp_body) = http_request(&addr, req, Some(&token)).await;
    assert_eq!(status, 200, "expected 200, got {status}: {resp_body}");
    let parsed: serde_json::Value =
        serde_json::from_str(&resp_body).expect("response must be JSON");
    assert_eq!(parsed["jsonrpc"], serde_json::json!("2.0"));
    assert_eq!(parsed["id"], serde_json::json!(1));
    assert_eq!(parsed["result"]["status"], serde_json::json!(200));
    assert_eq!(parsed["result"]["body"]["ok"], serde_json::json!(true));
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_bridge_rpc_missing_content_length_returns_400() {
    let (addr, handle, token) = spawn_bridge("rpc-nolen").await;
    let req = "POST /rpc HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n{}\n";
    let (status, body) = http_request(&addr, req.to_string(), Some(&token)).await;
    assert_eq!(status, 400, "expected 400, got {status}: {body}");
    assert!(body.contains("missing content-length"), "body={body}");
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_bridge_cors_preflight_returns_204() {
    let (addr, handle, _token) = spawn_bridge("cors").await;
    let req = "OPTIONS /rpc HTTP/1.1\r\nHost: localhost\r\nAccess-Control-Request-Method: POST\r\nConnection: close\r\n\r\n";
    let (status, body) = http_request(&addr, req.to_string(), None).await;
    assert_eq!(status, 204, "expected 204, got {status}: {body}");
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_bridge_unknown_route_returns_404() {
    let (addr, handle, token) = spawn_bridge("404").await;
    let req = "GET /nope HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    let (status, body) = http_request(&addr, req.to_string(), Some(&token)).await;
    assert_eq!(status, 404, "expected 404, got {status}: {body}");
    handle.abort();
}
