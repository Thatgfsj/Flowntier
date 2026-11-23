//! Provider and custom-provider management commands.

use crate::sidecar::pipe_request;

#[tauri::command]
pub async fn list_providers() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/providers", None).await
}

#[tauri::command]
pub async fn toggle_provider(id: String, enabled: bool) -> Result<(), String> {
    // PATCH /api/providers/{id} body {"enabled": bool}
    pipe_request(
        "PATCH",
        &format!("/api/providers/{}", id),
        Some(serde_json::json!({ "enabled": enabled })),
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn fetch_provider_models(id: String) -> Result<serde_json::Value, String> {
    // Pulls the live model catalog from the provider's own /models
    // (or equivalent) endpoint. The Python side dispatches per
    // provider id (Ollama uses /api/tags, Gemini uses ?key=, etc.)
    // so the Rust shell just forwards.
    pipe_request("GET", &format!("/api/providers/{}/models", id), None).await
}

#[tauri::command]
pub async fn add_custom_provider(
    id: String,
    display_name: String,
    kind: String,
    base_url: String,
    api_key_env: String,
    models: Vec<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    // Register a user-defined provider (relay station / private
    // gateway). Body shape matches the Python `register_custom`
    // signature; the persistence happens in the pipe-server's
    // SQLite (custom_provider table).
    pipe_request(
        "POST",
        "/api/providers/custom",
        Some(serde_json::json!({
            "id": id,
            "display_name": display_name,
            "kind": kind,
            "base_url": base_url,
            "api_key_env": api_key_env,
            "models": models,
        })),
    )
    .await
}

#[tauri::command]
pub async fn remove_custom_provider(id: String) -> Result<serde_json::Value, String> {
    pipe_request("DELETE", &format!("/api/providers/custom/{}", id), None).await
}

// event 000110 (fix D1): hide / unhide a (provider, model) pair.
// Both commands are idempotent on the sidecar — disabling an
// already-disabled model returns 200; enabling a never-disabled
// model returns was_disabled:false.
#[tauri::command]
pub async fn disable_provider_model(
    provider_id: String,
    model_id: String,
) -> Result<serde_json::Value, String> {
    pipe_request(
        "PUT",
        &format!("/api/providers/{}/models/{}/disable", provider_id, model_id),
        None,
    )
    .await
}

#[tauri::command]
pub async fn enable_provider_model(
    provider_id: String,
    model_id: String,
) -> Result<serde_json::Value, String> {
    pipe_request(
        "DELETE",
        &format!("/api/providers/{}/models/{}/disable", provider_id, model_id),
        None,
    )
    .await
}

// v0.4.35 (event 000135): DisabledModelsProvider hydrates from
//   this on mount so Settings → Providers sees the persisted
//   truth, not an empty Set.
#[tauri::command]
pub async fn list_disabled_models() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/disabled-models", None).await
}
