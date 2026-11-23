//! Health, version, and one-off utility commands.

use crate::sidecar::pipe_request;

// ── Tauri commands (React → Rust) ───────────────────────────────

#[tauri::command]
pub async fn health_check() -> Result<bool, String> {
    Ok(pipe_request("GET", "/health", None).await.is_ok())
}

/// Returns the sidecar runtime's version + a min_compatible
/// field. The frontend compares sidecar against its own
/// expected version; if sidecar < min_compatible, the app
/// shows a non-blocking drift banner.
///
/// Used by the desktop shell on startup, after the runtime
/// pipe becomes reachable.
#[tauri::command]
pub async fn rpc_version() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/rpc/version", None).await
}
