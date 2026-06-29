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
        let n = tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut buf))
            .await
            .expect("server did not respond in 10s")
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
        let n = tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut buf))
            .await
            .expect("server did not respond in 10s")
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
    // Each test gets its own storage dir so secrets, models, and
    // providers from one test don't leak into another. The dir is
    // wiped on entry and removed on drop via a scopeguard.
    let unique = format!(
        "{}-{}",
        tag,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let data_root = std::env::temp_dir().join(format!("flowntier-e2e-{unique}"));
    let _ = std::fs::remove_dir_all(&data_root);
    let _ = std::fs::create_dir_all(&data_root);
    let state = ServerState::new(data_root.clone(), data_root.clone()).await;
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

// v0.4.12 (event 000048): the api_key_env fallback in
// /api/run_task was removed. The Tauri shell resolves the key
// from the OS credential store (DPAPI via keyring) and sends
// plaintext in body.api_key. This test pins the contract:
// sending api_key_env alone (even with the env var set in the
// process) MUST NOT authenticate.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn run_task_rejects_api_key_env_fallback() {
    // Make sure a stray env var from the host environment does
    // NOT silently satisfy the new contract.
    let unique_var = "FLOWNTIER_E2E_DUMMY_KEY_DO_NOT_USE";
    std::env::set_var(unique_var, "sk-leaked-value-from-env");

    let (addr, handle) = spawn_server("nokey").await;
    // Wait a beat longer than the default spawn_server delay so
    // the JSON-RPC dispatcher is fully wired before we hit it.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "POST",
            "params": {
                "path": "/api/run_task",
                "body": {
                    "task": "ping",
                    "role": "agent:worker",
                    "provider_kind": "openai_compat",
                    "base_url": "http://127.0.0.1:1",
                    "model": "gpt-4o-mini",
                    "api_key_env": unique_var
                }
            }
        }),
    )
    .await;

    // The handler returns Err(...) because api_key is missing.
    // dispatcher wraps that into a JSON-RPC error response:
    //   { "jsonrpc": "2.0", "id": 1, "error": { "code": -32603, "message": "<e>" } }
    // Either a JSON-RPC error OR a result with status >= 400 counts
    // as "rejected". Both prove the env var was NOT read.
    let resp_text = serde_json::to_string(&resp).unwrap_or_default();
    let rpc_error_msg = resp["error"]["message"].as_str().unwrap_or("");
    let status = resp["result"]["status"].as_u64().unwrap_or(0);
    let body_str = resp["result"]["body"].to_string();
    let rejected = !rpc_error_msg.is_empty()
        || status >= 400
        || body_str.contains("missing")
        || body_str.contains("api_key")
        || body_str.contains("no env-var fallback");
    assert!(
        rejected,
        "expected api_key_env fallback to be rejected; got resp={resp_text}"
    );
    // And the error message must mention the api_key, NOT the env var.
    let combined = format!("{rpc_error_msg} {body_str}");
    assert!(
        combined.contains("api_key"),
        "rejection reason should mention api_key; got resp={resp_text}"
    );

    // Sanity: providing api_key explicitly also passes the auth
    // gate (the request gets past line 1003 of handlers.rs and
    // proceeds to provider-build + agent.run). We assert the
    // server RESPONDS — even if the response is a downstream
    // failure (e.g. network unreachable to api.openai.com).
    // This proves the env-var fallback was the ONLY thing being
    // tested here; explicit keys work as before.
    let resp2 = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "POST",
            "params": {
                "path": "/api/run_task",
                "body": {
                    "task": "ping",
                    "role": "agent:worker",
                    "provider_kind": "openai_compat",
                    "base_url": "http://127.0.0.1:1",
                    "model": "gpt-4o-mini",
                    "api_key": "sk-explicit-from-keyring"
                }
            }
        }),
    )
    .await;
    let resp2_text = serde_json::to_string(&resp2).unwrap_or_default();
    // We expect ANY response (200 with ok:false, 4xx, 5xx, or even
    // a JSON-RPC error from a downstream panic) — just NOT a hang.
    // And it must NOT contain the missing-api_key error, because
    // we explicitly passed api_key.
    assert!(
        !resp2_text.contains("missing or empty 'api_key'"),
        "api_key path should not be rejected at the missing-api_key gate; got resp={resp2_text}"
    );

    std::env::remove_var(unique_var);
    handle.abort();
}

// v0.4.14 (event 000050): chairman reported "保存失败：no handler
// registered for path /api/settings/secrets/MINIMAX_API_KEY".
// This test pins the exact request shape the Tauri shell sends
// and asserts the PUT handler is found. Without this test the
// regression would only surface in production (real keyring).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn put_secret_handler_is_registered() {
    let (addr, handle) = spawn_server("putsec").await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "PUT",
            "params": {
                "path": "/api/settings/secrets/MINIMAX_API_KEY",
                "body": { "value": "sk-minimax-test-1234" }
            }
        }),
    )
    .await;
    let resp_text = serde_json::to_string(&resp).unwrap_or_default();
    let rpc_err = resp["error"]["message"].as_str().unwrap_or("");
    let status = resp["result"]["status"].as_u64().unwrap_or(0);
    let body_str = resp["result"]["body"].to_string();
    // The handler must exist (no "no handler registered" error).
    // It may legitimately fail with a keyring / DPAPI error —
    // that's a 4xx/5xx with a real cause, NOT the dispatcher
    // "no handler registered" message.
    assert!(
        !rpc_err.contains("no handler registered"),
        "PUT /api/settings/secrets/{{name}} handler not registered! resp={resp_text}"
    );
    // And the body, if present, must not echo the dispatcher's
    // not-found code.
    assert!(
        !body_str.contains("no handler registered"),
        "PUT handler missing — body reports not-found: {resp_text}"
    );
    // We expect EITHER a 200 (keyring worked) OR a 4xx/5xx with
    // a meaningful inner error from SecretStore (not the
    // dispatcher).
    assert!(
        status == 200 || status >= 400,
        "expected HTTP-style response; got status={status} resp={resp_text}"
    );

    handle.abort();
}