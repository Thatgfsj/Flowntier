//! Persistent kv and first-run preference commands.

use crate::sidecar::pipe_request;

/// Read a key from the persistent kv store. Used by the frontend
/// to check first-run flags, the last-opened tab, etc.
///
/// Body: empty. Returns the JSON-encoded value or null.
#[tauri::command]
pub async fn kv_get(key: String) -> Result<serde_json::Value, String> {
    pipe_request("GET", &format!("/api/kv/{key}"), None).await
}

/// Set a key in the persistent kv store. Used by the Welcome
/// screen to clear the `first_run` flag, and by Settings to
/// persist UI preferences.
///
/// Body: { value: <json> }. Returns { k, v }.
#[tauri::command]
pub async fn kv_set(key: String, value: serde_json::Value) -> Result<serde_json::Value, String> {
    pipe_request(
        "POST",
        &format!("/api/kv/{key}"),
        Some(serde_json::json!({
            "value": value,
        })),
    )
    .await
}

/// Return the v0.4 sample workflow envelope. The frontend calls
/// this from the Welcome screen's "Try sample" button; the user
/// can then submit it as a real workflow via run_agent_task.
///
/// Body: empty. Returns the full WorkflowRun JSON.
#[tauri::command]
pub async fn load_sample_workflow() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/sample/auth_login", None).await
}

/// Mark the first-run flow as complete. Called from the Welcome
/// screen's "Enter workspace" button. Idempotent — calling twice
/// is a no-op.
#[tauri::command]
pub async fn first_run_complete() -> Result<serde_json::Value, String> {
    pipe_request("POST", "/api/kv/first_run/complete", None).await
}
