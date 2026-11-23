//! Router role/model assignment and quota commands.

use crate::sidecar::pipe_request;

/// v0.4.30 (audit 000130): upsert a per-(provider, model)
/// metadata override (context_length + thinking_strength).
/// Either field may be null to mean "don't touch the
/// existing override for that column" — but the frontend
/// usually sends both together or neither.
#[tauri::command]
pub async fn set_model_override(
    provider_id: String,
    model_id: String,
    context_length: Option<Option<i64>>,
    thinking_strength: Option<Option<String>>,
) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({
        "context_length": context_length.map(|v| match v {
            Some(n) => serde_json::json!(n),
            None => serde_json::json!(null),
        }),
        "thinking_strength": thinking_strength.map(|v| match v {
            Some(s) => serde_json::json!(s),
            None => serde_json::json!(null),
        }),
    });
    let path = format!("/api/providers/{}/models/{}", provider_id, model_id);
    pipe_request("PUT", &path, Some(body)).await
}

/// v0.4.30 (audit 000130): drop the override row entirely.
#[tauri::command]
pub async fn clear_model_override(
    provider_id: String,
    model_id: String,
) -> Result<serde_json::Value, String> {
    let path = format!("/api/providers/{}/models/{}", provider_id, model_id);
    pipe_request("DELETE", &path, None).await
}

#[tauri::command]
pub async fn list_router_roles() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/router/roles", None).await
}

#[tauri::command]
pub async fn list_router_models() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/router/models", None).await
}

#[tauri::command]
pub async fn get_role_resolve_status(role: String) -> Result<serde_json::Value, String> {
    pipe_request(
        "GET",
        &format!("/api/router/roles/{role}/resolve"),
        Some(serde_json::json!({ "role": role })),
    )
    .await
}

// ── v0.4.20 quota tracker ─────────────────────────────────────────
// Surface the per-(role, model) quota state to the frontend.
// Settings → 角色额度状态 block consumes `get_quota_status`;
// the chairman's "重置" button calls `reset_quota`;
// `get_role_quota_status` is a convenience wrapper used by
// the inline StatusLine.

#[tauri::command]
pub async fn get_quota_status() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/quota/status", None).await
}

#[tauri::command]
pub async fn reset_quota(
    role: String,
    model_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({
        "role": role,
        "model_id": model_id,
    });
    pipe_request("POST", "/api/quota/reset", Some(body)).await
}

#[tauri::command]
pub async fn get_role_quota_status(role: String) -> Result<serde_json::Value, String> {
    // Forward to the resolve endpoint, which embeds quota_status
    // inline; the frontend unwraps `.quota_status` if present.
    pipe_request(
        "GET",
        &format!("/api/router/roles/{role}/resolve"),
        Some(serde_json::json!({ "role": role })),
    )
    .await
}

#[tauri::command]
pub async fn update_router_roles(roles: serde_json::Value) -> Result<(), String> {
    pipe_request(
        "PUT",
        "/api/router/roles",
        Some(serde_json::json!({ "roles": roles })),
    )
    .await?;
    Ok(())
}
