//! Plugin listing and invocation commands.

use crate::sidecar::pipe_request;

#[tauri::command]
// BUG-FRONTEND-RT-13 (event 000044): the previous code
// proxied `list_plugins` through the pipe-server sidecar via
// `GET /api/plugins`. The sidecar doesn't have this route
// registered (only /api/rpc/version, /api/settings/secrets,
// /api/run_task, /api/plugins/[name]/[action]), so it returns
// 404 "no handler registered for path /api/plugins". The
// plugin system is currently a skeleton (no plugins
// implemented yet) — we just return an empty list directly
// from the Rust shell, no sidecar roundtrip needed. When the
// plugin system is actually built (v0.5+), this can be wired
// to a real registry (or kept in-process).
pub async fn list_plugins() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!([]))
}

#[tauri::command]
pub async fn invoke_plugin(
    name: String,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    pipe_request(
        "POST",
        &format!("/api/plugins/{}/invoke", name),
        Some(serde_json::json!({ "args": args })),
    )
    .await
}
