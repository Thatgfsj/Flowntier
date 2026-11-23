//! Provider CRUD, secrets, presets and model-catalog tests.

use super::common::*;

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
                "path": "/api/settings/secrets/flowntier/openai",
                "body": { "name": "flowntier/openai", "value": "sk-test-1234567890" }
            }
        }),
    )
    .await;
    assert_eq!(
        resp["result"]["status"], 200,
        "save should return 200: {resp}"
    );
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
    assert_eq!(secrets[0]["name"], "flowntier/openai");
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
                "path": "/api/settings/secrets/flowntier/openai/reveal",
                "body": { "name": "flowntier/openai" }
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
                "path": "/api/settings/secrets/flowntier/openai",
                "body": { "name": "flowntier/openai" }
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
                "path": "/api/settings/secrets/flowntier/openai/reveal",
                "body": { "name": "flowntier/openai" }
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
                "path": "/api/settings/secrets/flowntier/openai",
                "body": { "name": "flowntier/openai", "value": "sk-test" }
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
    assert_eq!(openai["secret_name"], "flowntier/openai");

    let anthropic = presets.iter().find(|p| p["id"] == "anthropic").unwrap();
    assert_eq!(anthropic["has_secret"], serde_json::json!(false));
    assert_eq!(anthropic["default_model"], "claude-opus-4-8");

    let custom = resp["result"]["body"]["custom_providers"]
        .as_array()
        .unwrap();
    assert_eq!(custom.len(), 0);

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_provider_full_crud() {
    let (addr, handle) = spawn_server("custom").await;

    // v0.4.22 (event 000096 + event 000108): the Tauri shell
    // does a TWO-STEP add:
    //   1. PUT /api/settings/secrets/CUSTOM_PROVIDER_KEY_<id>
    //      to persist the api_key into the secret store, then
    //   2. POST /api/providers/custom with id/display_name/
    //      kind/base_url/models[] to register the relay.
    //
    // The previous test had a single POST with `name`/
    // `default_model`/`api_key` — that was the pre-v0.4.22
    // schema and silently failed at the missing-`id` gate (the
    // dispatcher routed the error into `error.message` not
    // `result.status`, so the assertion looked like a Null).
    //
    // The two-step shape also matches the real Tauri shell
    // (`apps/desktop/src-tauri/src/lib.rs:add_custom_provider`
    // assumes the secret was already PUT). Crucially,
    // `list_providers` joins `has_secret` on
    // `CUSTOM_PROVIDER_KEY_<id>` (handlers.rs:1520) — so the
    // test must use that exact secret name or `has_secret`
    // will read false even after a successful POST.
    let id = "my-relay";
    let secret_name = format!("CUSTOM_PROVIDER_KEY_{id}");

    // Step 1 — PUT the api_key under the expected secret name.
    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 30,
            "method": "PUT",
            "params": {
                "path": format!("/api/settings/secrets/{secret_name}"),
                "body": { "name": secret_name, "value": "sk-relay-test-1234567890" }
            }
        }),
    )
    .await;
    assert_eq!(
        resp["result"]["status"].as_u64().unwrap_or(0),
        200,
        "PUT secret should return 200; got resp={}",
        serde_json::to_string(&resp).unwrap_or_default()
    );

    // Step 2 — POST the custom_provider record.
    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 31,
            "method": "POST",
            "params": {
                "path": "/api/providers/custom",
                "body": {
                    "id": id,
                    "display_name": "My Relay",
                    "kind": "openai-compatible",
                    "base_url": "https://relay.example.com/v1",
                    "models": [
                        { "id": "gpt-4o-mini", "display_name": "GPT-4o mini" }
                    ]
                }
            }
        }),
    )
    .await;
    assert_eq!(
        resp["result"]["status"].as_u64().unwrap_or(0),
        201,
        "POST custom_provider should return 201; got resp={}",
        serde_json::to_string(&resp).unwrap_or_default()
    );
    let returned_id = resp["result"]["body"]["id"].as_str().unwrap().to_string();
    assert_eq!(returned_id, id);

    // Step 3 — GET /api/providers must show the new relay with
    // has_secret:true (because step 1 wrote the secret under
    // the name list_providers joins on).
    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 32,
            "method": "GET",
            "params": {"path": "/api/providers", "body": null}
        }),
    )
    .await;
    let custom = resp["result"]["body"]["custom_providers"]
        .as_array()
        .unwrap();
    assert_eq!(
        custom.len(),
        1,
        "expected 1 custom_provider; got resp={}",
        serde_json::to_string(&resp).unwrap_or_default()
    );
    assert_eq!(
        custom[0]["has_secret"],
        serde_json::json!(true),
        "custom_provider should have has_secret:true after PUT + POST"
    );

    // Step 4 — DELETE the custom_provider. The path uses the
    // concrete id (the dispatcher placeholder {id} only matches
    // real segments, not the literal string "{id}").
    let delete_path = format!("/api/providers/custom/{id}");
    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 33,
            "method": "DELETE",
            "params": {
                "path": delete_path,
                "body": { "id": id }
            }
        }),
    )
    .await;
    assert_eq!(
        resp["result"]["status"].as_u64().unwrap_or(0),
        200,
        "DELETE custom_provider should return 200; got resp={}",
        serde_json::to_string(&resp).unwrap_or_default()
    );

    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 34,
            "method": "GET",
            "params": {"path": "/api/providers", "body": null}
        }),
    )
    .await;
    let custom = resp["result"]["body"]["custom_providers"]
        .as_array()
        .unwrap();
    assert_eq!(custom.len(), 0);

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
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "openai")
        .unwrap();
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

// v0.4.15 (event 000051): chairman reported "供应商（0）" — the
// provider list panel showed zero providers even after a key
// was saved. Root cause: TS ProviderInfo type had wrong field
// names (api_key_env, key_present, is_local, notes, models)
// that the Rust list_providers handler never emits. Every read
// returned undefined, so the UI-side filter dropped all 9
// presets. This test pins the new contract:
//
//   1. PUT a secret, then GET /api/providers — the matching
//      preset must come back with has_secret=true, and the
//      other 8 presets must still have has_secret=false.
//   2. Every preset row must include the new schema fields
//      (models: [], is_local: false) so TS doesn't have to
//      defend against undefined.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_providers_returns_presets_with_has_secret_set_after_put() {
    let (addr, handle) = spawn_server("provlist").await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // 1. PUT flowntier/minimax secret (v0.4.30 renamed from MINIMAX_API_KEY)
    let put_resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "PUT",
            "params": {
                "path": "/api/settings/secrets/flowntier/minimax",
                "body": { "name": "flowntier/minimax", "value": "sk-minimax-fake-1234" }
            }
        }),
    )
    .await;
    let put_status = put_resp["result"]["status"].as_u64().unwrap_or(0);
    assert!(
        put_status == 200 || put_status == 201,
        "PUT secret should succeed; got status={put_status} resp={}",
        serde_json::to_string(&put_resp).unwrap_or_default()
    );

    // 2. GET /api/providers
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
    let resp_text = serde_json::to_string(&resp).unwrap_or_default();
    assert_eq!(resp["result"]["status"].as_u64().unwrap_or(0), 200);

    let providers = resp["result"]["body"]["providers"]
        .as_array()
        .expect("providers should be array");

    // 3. All 9 presets must be present.
    assert_eq!(
        providers.len(),
        9,
        "expected 9 presets; got {} resp={resp_text}",
        providers.len()
    );

    // 4. Each preset must have the new schema fields.
    for p in providers {
        let id = p["id"].as_str().unwrap_or("<missing>");
        assert!(
            p.get("has_secret").is_some(),
            "preset {id} missing has_secret; resp={resp_text}"
        );
        assert!(
            p.get("secret_name").is_some(),
            "preset {id} missing secret_name; resp={resp_text}"
        );
        assert!(
            p.get("models").is_some() && p["models"].is_array(),
            "preset {id} missing models[]; resp={resp_text}"
        );
        assert!(
            p.get("is_local").is_some(),
            "preset {id} missing is_local; resp={resp_text}"
        );
    }

    // 5. The MiniMax row must now report has_secret=true; the
    //    other 8 must remain false. This is the actual bug
    //    chairman hit.
    let minimax = providers
        .iter()
        .find(|p| p["id"] == "minimax")
        .expect("minimax preset must exist");
    assert_eq!(
        minimax["has_secret"],
        serde_json::json!(true),
        "minimax should have has_secret:true after PUT; resp={resp_text}"
    );
    let openai = providers
        .iter()
        .find(|p| p["id"] == "openai")
        .expect("openai preset must exist");
    assert_eq!(
        openai["has_secret"],
        serde_json::json!(false),
        "openai should have has_secret:false; resp={resp_text}"
    );

    handle.abort();
}

// v0.4.14 (event 000050): chairman reported "保存失败：no handler
// registered for path /api/settings/secrets/MINIMAX_API_KEY".
// v0.4.30 (audit 000130): secret namespace renamed to flowntier/<id>;
// the same dispatcher wildcard that fixed the v0.4.14 regression
// now matches "flowntier/minimax". This test pins the request shape
// the Tauri shell sends and asserts the PUT handler is found.
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
                "path": "/api/settings/secrets/flowntier/minimax",
                "body": { "name": "flowntier/minimax", "value": "sk-minimax-test-1234" }
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

// v0.4.16 (event 000052): chairman rejected the v0.4.15 hard-coded
// defaults of "anthropic:claude-opus-4-8" + ["anthropic:claude-sonnet-4-6"].
// Every role must start with default_model:"" and fallback_chain:[].
// No migration is needed because the handler returns the defaults
// in-memory each call (no DB row written until chairman explicitly
// saves via PATCH).
