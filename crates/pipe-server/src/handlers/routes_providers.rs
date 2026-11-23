//! Provider/model management routes.

use super::*;

pub(super) fn register(d: &mut Dispatcher, state: Arc<ServerState>) {
    // PATCH /api/providers/{id} — toggle enabled, override
    // default_model, override base_url. Reads from body:
    //   { enabled?: bool, default_model?: string|null,
    //     base_url?: string|null }
    // Body fields are individually optional; only those present
    // are updated.
    let patch_state = state.clone();
    d.register("PATCH", "/api/providers/{id}", move |body| {
        let s = patch_state.clone();
        Box::pin(async move { patch_provider(body, s).await })
    });

    // event 000110 (fix D1): PUT /api/providers/{id}/models/{model}/disable
    //   hide (provider_id, model_id) from the catalog so the user
    //   can remove expired/unused models from Settings AND from the
    //   role-router's selectable models. Idempotent.
    // DELETE /api/providers/{id}/models/{model}/disable
    //   re-enable a previously hidden model.
    let disable_state = state.clone();
    d.register(
        "PUT",
        "/api/providers/{id}/models/{model}/disable",
        move |body| {
            let s = disable_state.clone();
            Box::pin(async move { disable_model(body, s).await })
        },
    );
    let disable_state2 = state.clone();
    d.register(
        "PUT",
        "/api/providers/{provider_id}/models/{model_id}/disable",
        move |body| {
            let s = disable_state2.clone();
            Box::pin(async move { disable_model(body, s).await })
        },
    );
    let enable_state = state.clone();
    d.register(
        "DELETE",
        "/api/providers/{id}/models/{model}/disable",
        move |body| {
            let s = enable_state.clone();
            Box::pin(async move { enable_model(body, s).await })
        },
    );
    let enable_state2 = state.clone();
    d.register(
        "DELETE",
        "/api/providers/{provider_id}/models/{model_id}/disable",
        move |body| {
            let s = enable_state2.clone();
            Box::pin(async move { enable_model(body, s).await })
        },
    );

    // event 000135 (v0.4.35): GET /api/disabled-models — return
    //   every (provider_id, model_id) pair the user has hidden.
    //   Used by the desktop DisabledModelsProvider on mount so
    //   Settings → Providers starts with the persisted truth,
    //   not an empty Set that only sees local mutations.
    let list_disabled_state = state.clone();
    d.register("GET", "/api/disabled-models", move |_body| {
        let s = list_disabled_state.clone();
        Box::pin(async move {
            match s.repo.list_disabled_models().await {
                Ok(rows) => Ok((
                    200,
                    json!({
                        "models": rows.into_iter().map(|(provider_id, model_id)| {
                            json!({"provider_id": provider_id, "model_id": model_id})
                        }).collect::<Vec<_>>()
                    }),
                )),
                Err(e) => Ok((500, json!({"error": format!("list_disabled_models: {e}")}))),
            }
        })
    });

    // GET /api/providers/{id}/models — list models. Tries the
    // provider's /v1/models endpoint (if has_live_models_endpoint
    // is true); falls back to ANTHROPIC_FALLBACK_MODELS for
    // Anthropic. Caches results in `model_cache` for 1 hour.
    let models_state = state.clone();
    d.register("GET", "/api/providers/{id}/models", move |body| {
        let s = models_state.clone();
        Box::pin(async move { list_models(body, s).await })
    });

    // POST /api/providers/custom — add a user-defined relay
    // station / private gateway. Body:
    //   { name: string, base_url: string,
    //     kind: "openai-compatible" | "anthropic-compatible",
    //     default_model?: string|null }
    let custom_add_state = state.clone();
    d.register("POST", "/api/providers/custom", move |body| {
        let s = custom_add_state.clone();
        Box::pin(async move { add_custom_provider(body, s).await })
    });

    // DELETE /api/providers/custom/{id} — remove a custom provider.
    let custom_del_state = state.clone();
    d.register("DELETE", "/api/providers/custom/{id}", move |body| {
        let s = custom_del_state.clone();
        Box::pin(async move { delete_custom_provider(body, s).await })
    });

    // v0.4.30 (audit 000130): per-model metadata override.
    // PUT /api/providers/{id}/models/{model} upserts
    // context_length + thinking_strength into model_overrides.
    let model_ovr_put = state.clone();
    d.register("PUT", "/api/providers/{id}/models/{model}", move |body| {
        let s = model_ovr_put.clone();
        Box::pin(async move { put_model_override(body, s).await })
    });
    // DELETE removes the override (back to built-in fallback).
    let model_ovr_del = state.clone();
    d.register(
        "DELETE",
        "/api/providers/{id}/models/{model}",
        move |body| {
            let s = model_ovr_del.clone();
            Box::pin(async move { delete_model_override(body, s).await })
        },
    );
}
