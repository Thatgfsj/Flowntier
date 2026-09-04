//! e2e test for audit 000135 — global disabled-models state via
//! `GET /api/disabled-models`.
//!
//! Reproduces the production bug:
//!   1. user opens Settings → Providers
//!   2. user clicks × to hide a model (e.g. `flowntier/openai` gpt-4)
//!   3. user closes Settings, reopens Settings
//!   4. the model is back — UI shows × delete again, as if the
//!      disable never persisted
//!
//! Root cause: `useDisabledModels()` held its Set in
//! `useState` per-instance. Each Settings provider detail row
//! mounted its own copy, so each copy started empty. The
//! persisted truth lived only in SQLite and the backend's
//! `listRouterModels` filter.
//!
//! Fix (event 000135): new endpoint `GET /api/disabled-models`
//! returns the persisted truth. Desktop lifts the Set into
//! `DisabledModelsProvider` mounted once at the App root, which
//! hydrates from this endpoint on mount.
//!
//! These tests drive the JSON-RPC surface to verify the
//! endpoint exists, returns the right shape, and reflects
//! disable/enable cycles.

use std::time::Duration;

use pipe_server::{register_all, Dispatcher, Server, ServerConfig, ServerState};

// ── Test transport helpers (mirror e2e_pipe.rs) ────────────────

#[cfg(not(windows))]
mod client {
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    pub async fn connect_and_request(addr: &str, body: serde_json::Value) -> serde_json::Value {
        let mut conn = UnixStream::connect(addr).await.expect("connect failed");
        let mut line = serde_json::to_vec(&body).unwrap();
        line.push(b'\n');
        conn.write_all(&line).await.unwrap();
        let mut reader = BufReader::new(&mut conn);
        let mut buf = String::new();
        let n = tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut buf))
            .await
            .expect("server did not respond in 10s")
            .expect("read failed");
        assert!(n > 0, "server closed connection without sending a response");
        serde_json::from_str(&buf).expect("server sent non-JSON")
    }
}

#[cfg(windows)]
mod client {
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ClientOptions;

    pub async fn connect_and_request(addr: &str, body: serde_json::Value) -> serde_json::Value {
        let path = addr.to_string();
        let mut conn = ClientOptions::new().open(&path).expect("connect failed");
        let mut line = serde_json::to_vec(&body).unwrap();
        line.push(b'\n');
        conn.write_all(&line).await.unwrap();
        let mut reader = BufReader::new(&mut conn);
        let mut buf = String::new();
        let n = tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut buf))
            .await
            .expect("server did not respond in 10s")
            .expect("read failed");
        assert!(n > 0, "server closed connection without sending a response");
        serde_json::from_str(&buf).expect("server sent non-JSON")
    }
}

fn free_pipe_name(tag: &str, kind: &str) -> String {
    let unique = format!(
        "{}-{}-{}",
        tag,
        kind,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );

    #[cfg(windows)]
    {
        format!(r"\\.\pipe\flowntier_a0135_{unique}")
    }
    #[cfg(not(windows))]
    {
        let dir = std::env::temp_dir().join(format!("flowntier-a0135-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{kind}.sock"));
        let _ = std::fs::remove_file(&p);
        p.to_string_lossy().into_owned()
    }
}

async fn spawn_server(tag: &str) -> (String, tokio::task::JoinHandle<std::io::Result<()>>) {
    let rpc_path = free_pipe_name(tag, "rpc");
    let events_path = free_pipe_name(tag, "events");
    let cfg = ServerConfig {
        rpc_path: rpc_path.clone(),
        events_path,
    };
    let mut d = Dispatcher::new();
    let unique = format!(
        "{}-{}",
        tag,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let data_root = std::env::temp_dir().join(format!("flowntier-a0135-{unique}"));
    let _ = std::fs::remove_dir_all(&data_root);
    let _ = std::fs::create_dir_all(&data_root);
    let state = ServerState::new(data_root.clone(), data_root.clone()).await;
    register_all(&mut d, state.clone());
    let server = Server::new(cfg, d, state.events.clone());
    let handle = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(Duration::from_millis(300)).await;
    (rpc_path, handle)
}

async fn rpc(
    method: &str,
    path: &str,
    body: serde_json::Value,
    addr: &str,
    id: u64,
) -> serde_json::Value {
    client::connect_and_request(
        addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": { "path": path, "body": body }
        }),
    )
    .await
}

// ── Tests ──────────────────────────────────────────────────────

/// Sanity: the endpoint exists and returns the documented shape
/// `{ models: [{ provider_id, model_id }] }`, even when nothing
/// is disabled. The empty case is the hydration baseline that
/// the desktop `DisabledModelsProvider` reads on a fresh app
/// start.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audit_000135_list_disabled_models_endpoint_returns_empty_when_fresh() {
    let (addr, handle) = spawn_server("a0135-empty").await;

    let resp = rpc(
        "GET",
        "/api/disabled-models",
        serde_json::Value::Null,
        &addr,
        1,
    )
    .await;
    let resp_text = serde_json::to_string(&resp).unwrap_or_default();
    assert_eq!(
        resp["result"]["status"].as_u64().unwrap_or(0),
        200,
        "GET /api/disabled-models must return 200; resp={resp_text}"
    );
    let body = &resp["result"]["body"];
    assert!(
        body["models"].is_array(),
        "body must have a `models` array; got body={body}"
    );
    assert_eq!(
        body["models"].as_array().unwrap().len(),
        0,
        "fresh server has no disabled models; got {body}"
    );

    handle.abort();
}

/// The hydration path must reflect the persisted truth:
/// disable a pair → list shows it; enable it → list no longer
/// shows it. This is exactly what the desktop Provider does on
/// mount, so the round-trip proves the frontend will see the
/// right Set.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audit_000135_list_disabled_models_reflects_disable_then_enable() {
    let (addr, handle) = spawn_server("a0135-cycle").await;

    // Baseline: no pairs.
    let before = rpc(
        "GET",
        "/api/disabled-models",
        serde_json::Value::Null,
        &addr,
        1,
    )
    .await;
    assert_eq!(before["result"]["status"].as_u64().unwrap_or(0), 200);
    let before_models = before["result"]["body"]["models"].as_array().unwrap();
    assert_eq!(
        before_models.len(),
        0,
        "precondition: no disabled models on fresh server"
    );

    // Disable anthropic :: claude-haiku-4-5. We use a flat
    // provider_id here because the dispatcher's pattern matcher
    // splits on '/' (one segment per placeholder), so the URL
    // path can't carry a '/' inside a single {id}.
    let disable = rpc(
        "PUT",
        "/api/providers/anthropic/models/claude-haiku-4-5/disable",
        serde_json::json!({
            "provider_id": "anthropic",
            "model_id": "claude-haiku-4-5",
        }),
        &addr,
        2,
    )
    .await;
    let disable_text = serde_json::to_string(&disable).unwrap_or_default();
    assert_eq!(
        disable["result"]["status"].as_u64().unwrap_or(0),
        200,
        "PUT disable must succeed; resp={disable_text}"
    );

    // After disable: list contains the pair.
    let after_disable = rpc(
        "GET",
        "/api/disabled-models",
        serde_json::Value::Null,
        &addr,
        3,
    )
    .await;
    let after_disable_text = serde_json::to_string(&after_disable).unwrap_or_default();
    assert_eq!(after_disable["result"]["status"].as_u64().unwrap_or(0), 200);
    let models = after_disable["result"]["body"]["models"]
        .as_array()
        .unwrap();
    assert_eq!(
        models.len(),
        1,
        "exactly one disabled pair after one disable; resp={after_disable_text}"
    );
    assert_eq!(models[0]["provider_id"], "anthropic");
    assert_eq!(models[0]["model_id"], "claude-haiku-4-5");

    // Enable the same pair.
    let enable = rpc(
        "DELETE",
        "/api/providers/anthropic/models/claude-haiku-4-5/disable",
        serde_json::json!({
            "provider_id": "anthropic",
            "model_id": "claude-haiku-4-5",
        }),
        &addr,
        4,
    )
    .await;
    let enable_text = serde_json::to_string(&enable).unwrap_or_default();
    assert_eq!(
        enable["result"]["status"].as_u64().unwrap_or(0),
        200,
        "DELETE enable must succeed; resp={enable_text}"
    );

    // After enable: list no longer contains the pair.
    let after_enable = rpc(
        "GET",
        "/api/disabled-models",
        serde_json::Value::Null,
        &addr,
        5,
    )
    .await;
    assert_eq!(after_enable["result"]["status"].as_u64().unwrap_or(0), 200);
    let models = after_enable["result"]["body"]["models"].as_array().unwrap();
    assert_eq!(
        models.len(),
        0,
        "after enable, list is empty again (round-trip works)"
    );

    handle.abort();
}
