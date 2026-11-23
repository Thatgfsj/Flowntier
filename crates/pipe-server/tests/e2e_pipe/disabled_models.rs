//! Per-provider model disable tests (event 000110).

use super::common::*;

// v0.4.22 (event 000110): per-provider model disable. The chairman
// reported "设置里面也没办法删除模型啊，比如mimo的模型我已经到期了，我也删不了"
// — there was no UI or backend RPC to drop a specific (provider,model)
// pair out of the catalog. This file pins the three new contracts:
//   1. PUT /api/providers/{id}/models/{model}/disable persists the
//      pair into `disabled_models`.
//   2. DELETE on the same path removes the pair and is idempotent
//      (returns was_disabled=false on a fresh call).
//   3. GET /api/providers/{id}/models filters disabled models out
//      of the response list.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disable_model_persists_pair() {
    let (addr, handle) = spawn_server("disable-persist").await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Disable anthropic/claude-haiku-4-5 via PUT.
    let put_resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "PUT",
            "params": {
                "path": "/api/providers/anthropic/models/claude-haiku-4-5-20251022/disable",
                "body": {}
            }
        }),
    )
    .await;
    let put_text = serde_json::to_string(&put_resp).unwrap_or_default();
    assert_eq!(
        put_resp["result"]["status"].as_u64().unwrap_or(0),
        200,
        "PUT status should be 200; resp={put_text}"
    );
    let body = &put_resp["result"]["body"];
    assert_eq!(body["disabled"], serde_json::json!(true), "resp={put_text}");
    assert_eq!(
        body["provider_id"],
        serde_json::json!("anthropic"),
        "resp={put_text}"
    );
    assert_eq!(
        body["model_id"],
        serde_json::json!("claude-haiku-4-5-20251022"),
        "resp={put_text}"
    );

    // PUT again — must be idempotent (ON CONFLICT DO NOTHING).
    let put2 = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "PUT",
            "params": {
                "path": "/api/providers/anthropic/models/claude-haiku-4-5-20251022/disable",
                "body": {}
            }
        }),
    )
    .await;
    assert_eq!(
        put2["result"]["status"].as_u64().unwrap_or(0),
        200,
        "second PUT must also return 200"
    );

    // PUT on an unknown provider must return 404.
    let notfound = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "PUT",
            "params": {
                "path": "/api/providers/nosuchprovider/models/foo/disable",
                "body": {}
            }
        }),
    )
    .await;
    assert_eq!(
        notfound["result"]["status"].as_u64().unwrap_or(0),
        404,
        "unknown provider must 404"
    );

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enable_model_restores_pair() {
    let (addr, handle) = spawn_server("enable-restore").await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // DELETE on a pair that was never disabled → was_disabled=false.
    let first = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "DELETE",
            "params": {
                "path": "/api/providers/anthropic/models/claude-opus-4-6/disable",
                "body": {}
            }
        }),
    )
    .await;
    let first_text = serde_json::to_string(&first).unwrap_or_default();
    assert_eq!(
        first["result"]["status"].as_u64().unwrap_or(0),
        200,
        "DELETE status should be 200 even when nothing to delete; resp={first_text}"
    );
    assert_eq!(
        first["result"]["body"]["was_disabled"],
        serde_json::json!(false),
        "fresh DELETE → was_disabled:false; resp={first_text}"
    );

    // Disable, then DELETE → was_disabled=true.
    client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "PUT",
            "params": {
                "path": "/api/providers/anthropic/models/claude-opus-4-6/disable",
                "body": {}
            }
        }),
    )
    .await;

    let second = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "DELETE",
            "params": {
                "path": "/api/providers/anthropic/models/claude-opus-4-6/disable",
                "body": {}
            }
        }),
    )
    .await;
    let second_text = serde_json::to_string(&second).unwrap_or_default();
    assert_eq!(
        second["result"]["status"].as_u64().unwrap_or(0),
        200,
        "second DELETE must still be 200; resp={second_text}"
    );
    assert_eq!(
        second["result"]["body"]["was_disabled"],
        serde_json::json!(true),
        "DELETE on disabled pair → was_disabled:true; resp={second_text}"
    );

    // DELETE on unknown provider must 404.
    let nf = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "DELETE",
            "params": {
                "path": "/api/providers/ghost/models/x/disable",
                "body": {}
            }
        }),
    )
    .await;
    assert_eq!(
        nf["result"]["status"].as_u64().unwrap_or(0),
        404,
        "DELETE on unknown provider must 404"
    );

    handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn list_models_filters_disabled_pairs() {
    let (addr, handle) = spawn_server("disable-filter").await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // Disable one anthropic fallback model.
    let put = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "PUT",
            "params": {
                "path": "/api/providers/anthropic/models/claude-haiku-4-5-20251022/disable",
                "body": {}
            }
        }),
    )
    .await;
    assert_eq!(put["result"]["status"].as_u64().unwrap_or(0), 200);

    // GET must now exclude the disabled pair.
    let get_resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "GET",
            "params": {
                "path": "/api/providers/anthropic/models",
                "body": {}
            }
        }),
    )
    .await;
    let resp_text = serde_json::to_string(&get_resp).unwrap_or_default();
    assert_eq!(
        get_resp["result"]["status"].as_u64().unwrap_or(0),
        200,
        "GET status should be 200; resp={resp_text}"
    );
    let models = get_resp["result"]["body"]["models"]
        .as_array()
        .expect("models array");
    // Note: the anthropic fallback catalog typically has multiple
    // entries; we just want to confirm haiku was excluded and that
    // any surviving entry is NOT haiku.
    let blocked = "claude-haiku-4-5-20251022";
    for m in models {
        let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("");
        assert_ne!(
            id, blocked,
            "disabled model leaked into response: {resp_text}"
        );
    }
    // Sanity: the catalog still lists other entries (otherwise
    // 'disabled disabled everything' would pass trivially).
    assert!(
        !models.is_empty(),
        "filter should not empty the catalog entirely; resp={resp_text}"
    );

    handle.abort();
}
