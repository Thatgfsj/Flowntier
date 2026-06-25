//! End-to-end test: bring up the pipe server in-process, open a
//! client connection, send a JSON-RPC request, and verify the
//! response.

use std::time::Duration;

use pipe_server::{register_all, Dispatcher, Server, ServerConfig, ServerState};

fn free_pipe_name(tag: &str, kind: &str) -> String {
    // Both Windows named pipes and Unix domain sockets get a
    // unique path per test invocation.
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
        // Windows: \\.\pipe\aco_test_<unique>
        format!(r"\\.\pipe\aco_test_{unique}")
    }
    #[cfg(not(windows))]
    {
        // Unix: per-test temp dir + .sock
        let dir = std::env::temp_dir().join(format!("aco-pipe-test-{unique}"));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{kind}.sock"));
        let _ = std::fs::remove_file(&p);
        p.to_string_lossy().into_owned()
    }
}

// ── Transport abstraction for the test client ────────────────────

#[cfg(not(windows))]
mod client {
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    pub async fn connect_and_request(
        addr: &str,
        body: serde_json::Value,
    ) -> serde_json::Value {
        let mut conn = UnixStream::connect(addr).await.expect("connect failed");
        let mut line = serde_json::to_vec(&body).unwrap();
        line.push(b'\n');
        conn.write_all(&line).await.unwrap();

        let mut reader = BufReader::new(&mut conn);
        let mut buf = String::new();
        let n = tokio::time::timeout(Duration::from_secs(3), reader.read_line(&mut buf))
            .await
            .expect("server did not respond in 3s")
            .expect("read failed");
        assert!(n > 0, "empty response");
        serde_json::from_str(&buf).expect("server sent non-JSON")
    }
}

#[cfg(windows)]
mod client {
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ClientOptions;

    pub async fn connect_and_request(
        addr: &str,
        body: serde_json::Value,
    ) -> serde_json::Value {
        let mut conn = ClientOptions::new()
            .open(addr)
            .expect("connect failed");
        let mut line = serde_json::to_vec(&body).unwrap();
        line.push(b'\n');
        conn.write_all(&line).await.unwrap();

        let mut reader = BufReader::new(&mut conn);
        let mut buf = String::new();
        let n = tokio::time::timeout(Duration::from_secs(3), reader.read_line(&mut buf))
            .await
            .expect("server did not respond in 3s")
            .expect("read failed");
        assert!(n > 0, "empty response");
        serde_json::from_str(&buf).expect("server sent non-JSON")
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
    let state = ServerState::new(
        std::env::temp_dir(),
        std::env::temp_dir().join("flowntier-e2e-test"),
    )
    .await;
    register_all(&mut d, state.clone());
    let server = Server::new(cfg, d, state.events.clone());
    let handle = tokio::spawn(async move { server.run().await });
    tokio::time::sleep(Duration::from_millis(200)).await;
    (rpc_path, handle)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ping_over_pipe_returns_ok() {
    let (addr, handle) = spawn_server("ping").await;
    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "GET",
            "params": {"path": "/api/ping", "body": null}
        }),
    )
    .await;
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert_eq!(resp["result"]["status"], 200);
    assert_eq!(resp["result"]["body"]["ok"], serde_json::json!(true));
    assert_eq!(resp["result"]["body"]["runtime"], serde_json::json!("flowntier-rs"));
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_method_returns_jsonrpc_error() {
    let (addr, handle) = spawn_server("404").await;
    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "GET",
            "params": {"path": "/nope", "body": null}
        }),
    )
    .await;
    assert_eq!(resp["error"]["code"], -32601);
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn providers_endpoint_returns_ok() {
    let (addr, handle) = spawn_server("providers").await;
    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "GET",
            "params": {"path": "/api/providers", "body": null}
        }),
    )
    .await;
    assert_eq!(resp["result"]["status"], 200);
    assert!(resp["result"]["body"]["providers"].is_array());
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn secret_roundtrip_persists_across_clients() {
    let (addr, handle) = spawn_server("secret").await;

    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "PUT",
            "params": {
                "path": "/api/settings/secrets/OPENAI_API_KEY",
                "body": { "name": "OPENAI_API_KEY", "value": "sk-test-1234567890" }
            }
        }),
    )
    .await;
    assert_eq!(resp["result"]["status"], 200, "save should return 200: {resp}");
    assert_eq!(resp["result"]["body"]["saved"], serde_json::json!(true));

    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "GET",
            "params": {"path": "/api/settings/secrets", "body": null}
        }),
    )
    .await;
    assert_eq!(resp["result"]["status"], 200);
    let secrets = resp["result"]["body"]["secrets"].as_array().unwrap();
    assert_eq!(secrets.len(), 1, "expected 1 secret, got {secrets:?}");
    assert_eq!(secrets[0]["name"], "OPENAI_API_KEY");
    assert_eq!(secrets[0]["has_value"], serde_json::json!(true));
    assert!(secrets[0].get("value").is_none());
    assert!(secrets[0].get("ciphertext").is_none());

    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "GET",
            "params": {
                "path": "/api/settings/secrets/OPENAI_API_KEY/reveal",
                "body": { "name": "OPENAI_API_KEY" }
            }
        }),
    )
    .await;
    assert_eq!(resp["result"]["status"], 200);
    assert_eq!(
        resp["result"]["body"]["value"],
        serde_json::json!("sk-test-1234567890")
    );

    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 13,
            "method": "DELETE",
            "params": {
                "path": "/api/settings/secrets/OPENAI_API_KEY",
                "body": { "name": "OPENAI_API_KEY" }
            }
        }),
    )
    .await;
    assert_eq!(resp["result"]["status"], 200);
    assert_eq!(resp["result"]["body"]["deleted"], serde_json::json!(true));

    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 14,
            "method": "GET",
            "params": {
                "path": "/api/settings/secrets/OPENAI_API_KEY/reveal",
                "body": { "name": "OPENAI_API_KEY" }
            }
        }),
    )
    .await;
    assert_eq!(resp["result"]["status"], 404);

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn providers_list_returns_presets_with_has_secret_join() {
    let (addr, handle) = spawn_server("providers-list").await;

    let _ = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 20,
            "method": "PUT",
            "params": {
                "path": "/api/settings/secrets/OPENAI_API_KEY",
                "body": { "name": "OPENAI_API_KEY", "value": "sk-test" }
            }
        }),
    )
    .await;

    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 21,
            "method": "GET",
            "params": {"path": "/api/providers", "body": null}
        }),
    )
    .await;
    assert_eq!(resp["result"]["status"], 200);
    let presets = resp["result"]["body"]["providers"].as_array().unwrap();
    assert_eq!(presets.len(), 9);

    let openai = presets.iter().find(|p| p["id"] == "openai").unwrap();
    assert_eq!(openai["has_secret"], serde_json::json!(true));
    assert_eq!(openai["secret_name"], "OPENAI_API_KEY");

    let anthropic = presets.iter().find(|p| p["id"] == "anthropic").unwrap();
    assert_eq!(anthropic["has_secret"], serde_json::json!(false));
    assert_eq!(anthropic["default_model"], "claude-opus-4-8");

    let custom = resp["result"]["body"]["custom_providers"].as_array().unwrap();
    assert_eq!(custom.len(), 0);

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_provider_full_crud() {
    let (addr, handle) = spawn_server("custom").await;

    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 30,
            "method": "POST",
            "params": {
                "path": "/api/providers/custom",
                "body": {
                    "name": "My Relay",
                    "base_url": "https://relay.example.com/v1",
                    "kind": "openai-compatible",
                    "default_model": "gpt-4o-mini",
                    "api_key": "sk-relay-test-1234567890"
                }
            }
        }),
    )
    .await;
    assert_eq!(resp["result"]["status"], 201);
    let id = resp["result"]["body"]["id"].as_str().unwrap().to_string();

    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 31,
            "method": "GET",
            "params": {"path": "/api/providers", "body": null}
        }),
    )
    .await;
    let custom = resp["result"]["body"]["custom_providers"].as_array().unwrap();
    assert_eq!(custom.len(), 1);
    assert_eq!(custom[0]["has_secret"], serde_json::json!(true));

    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 32,
            "method": "DELETE",
            "params": {
                "path": "/api/providers/custom/{id}",
                "body": { "id": id }
            }
        }),
    )
    .await;
    assert_eq!(resp["result"]["status"], 200);

    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 33,
            "method": "GET",
            "params": {"path": "/api/providers", "body": null}
        }),
    )
    .await;
    let custom = resp["result"]["body"]["custom_providers"].as_array().unwrap();
    assert_eq!(custom.len(), 0);

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn router_roles_have_real_model_ids() {
    let (addr, handle) = spawn_server("router-roles").await;
    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 40,
            "method": "GET",
            "params": {"path": "/api/router/roles", "body": null}
        }),
    )
    .await;
    let roles = resp["result"]["body"]["roles"].as_array().unwrap();
    assert_eq!(roles.len(), 6);
    for r in roles {
        let m = r["default_model"].as_str().unwrap();
        assert!(
            m == "anthropic:claude-opus-4-8" || m == "anthropic:claude-sonnet-4-6",
            "stale model id: {m}"
        );
        let chain = r["fallback_chain"].as_array().unwrap();
        assert!(!chain.is_empty(), "role {} has empty fallback", r["role"]);
        assert_eq!(chain[0].as_str(), Some("anthropic:claude-sonnet-4-6"));
    }
    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn patch_provider_toggles_enabled() {
    let (addr, handle) = spawn_server("patch").await;
    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 50,
            "method": "PATCH",
            "params": {
                "path": "/api/providers/openai",
                "body": { "id": "openai", "enabled": false }
            }
        }),
    )
    .await;
    assert_eq!(resp["result"]["status"], 200);
    assert_eq!(resp["result"]["body"]["enabled"], serde_json::json!(false));

    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 51,
            "method": "GET",
            "params": {"path": "/api/providers", "body": null}
        }),
    )
    .await;
    let openai = resp["result"]["body"]["providers"]
        .as_array().unwrap()
        .iter().find(|p| p["id"] == "openai").unwrap();
    assert_eq!(openai["enabled"], serde_json::json!(false));

    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 52,
            "method": "PATCH",
            "params": {
                "path": "/api/providers/{id}",
                "body": { "id": "nonexistent", "enabled": false }
            }
        }),
    )
    .await;
    assert_eq!(resp["result"]["status"], 404);

    handle.abort();
}