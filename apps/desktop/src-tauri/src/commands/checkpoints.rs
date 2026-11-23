//! Checkpoint and MCP status commands.

use crate::sidecar::pipe_request;

/// Create a checkpoint. Body: `{ label?: string }`.
#[tauri::command]
pub async fn create_checkpoint(body: serde_json::Value) -> Result<serde_json::Value, String> {
    pipe_request("POST", "/api/checkpoint/create", Some(body)).await
}

/// List checkpoints, newest first.
#[tauri::command]
pub async fn list_checkpoints() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/checkpoint/list", None).await
}

/// Restore preview / apply. Body: `{ id, confirm?: bool }` —
/// confirm=false returns the affected-file list, confirm=true
/// applies (the UI must show the preview and ask first).
#[tauri::command]
pub async fn restore_checkpoint(body: serde_json::Value) -> Result<serde_json::Value, String> {
    pipe_request("POST", "/api/checkpoint/restore", Some(body)).await
}

/// Last MCP scan: per-server status + active permission mode.
#[tauri::command]
pub async fn get_mcp_status() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/mcp/status", None).await
}

/// Kick a background MCP/permission registry rebuild.
#[tauri::command]
pub async fn rescan_mcp() -> Result<serde_json::Value, String> {
    pipe_request("POST", "/api/mcp/rescan", None).await
}
