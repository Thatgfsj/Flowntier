//! Ping / unknown-method / providers-endpoint smoke tests.

use super::common::*;

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
    assert_eq!(
        resp["result"]["body"]["runtime"],
        serde_json::json!("flowntier-rs")
    );
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
