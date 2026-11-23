//! Provider write handlers (disable/enable/custom/model overrides/cache refresh).

use super::*;

// ── event 000110 (fix D1): disable / enable a model ──────────
//
// PUT /api/providers/{id}/models/{model}/disable
//   Path fields: id, model (URL-encoded).
//   Body: empty.
//   Returns: 200 {disabled: true, provider_id, model_id}
//            404 if the provider id is unknown (preset or custom).
//
// DELETE /api/providers/{id}/models/{model}/disable
//   Re-enable a previously hidden model.
//   Returns: 200 {enabled: true, was_disabled: bool, provider_id, model_id}
//            200 {enabled: true, was_disabled: false, ...} when the
//            model was not in the disabled set (idempotent no-op).
//
// Both handlers validate that the provider id exists (preset OR
// custom) — they refuse to write a `disabled_models` row for a
// provider that does not exist, so the table cannot drift from the
// provider catalog.
pub(super) async fn disable_model(
    body: Value,
    state: Arc<ServerState>,
) -> Result<(u16, Value), String> {
    let provider_id = body
        .get("provider_id")
        .or_else(|| body.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'provider_id' in path".to_string())?
        .to_string();
    let model_id = body
        .get("model_id")
        .or_else(|| body.get("model"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'model_id' in path".to_string())?
        .to_string();
    if model_id.is_empty() || model_id.len() > 256 {
        return Ok((400, json!({ "error": "model_id must be 1..=256 chars" })));
    }
    if !provider_exists(&state, &provider_id).await {
        return Ok((
            404,
            json!({
                "error": format!("unknown provider: {provider_id}"),
            }),
        ));
    }
    state
        .repo
        .disable_model(&provider_id, &model_id)
        .await
        .map_err(|e| format!("disable_model: {e}"))?;
    Ok((
        200,
        json!({
            "disabled": true,
            "provider_id": provider_id,
            "model_id": model_id,
        }),
    ))
}

pub(super) async fn enable_model(
    body: Value,
    state: Arc<ServerState>,
) -> Result<(u16, Value), String> {
    let provider_id = body
        .get("provider_id")
        .or_else(|| body.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'provider_id' in path".to_string())?
        .to_string();
    let model_id = body
        .get("model_id")
        .or_else(|| body.get("model"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'model_id' in path".to_string())?
        .to_string();
    if !provider_exists(&state, &provider_id).await {
        return Ok((
            404,
            json!({
                "error": format!("unknown provider: {provider_id}"),
            }),
        ));
    }
    let was_disabled = state
        .repo
        .enable_model(&provider_id, &model_id)
        .await
        .map_err(|e| format!("enable_model: {e}"))?;
    Ok((
        200,
        json!({
            "enabled": true,
            "was_disabled": was_disabled,
            "provider_id": provider_id,
            "model_id": model_id,
        }),
    ))
}

/// Returns true if `provider_id` resolves to either a preset (built-in)
/// or a `custom_provider` row. Used by disable_model/enable_model
/// to refuse writes that would orphan `disabled_models` rows.
pub(super) async fn provider_exists(state: &Arc<ServerState>, provider_id: &str) -> bool {
    if crate::providers::get(provider_id).is_some() {
        return true;
    }
    matches!(
        state.repo.get_custom_provider(provider_id).await,
        Ok(Some(_))
    )
}

/// POST /api/providers/custom — add a relay-station / private-gateway
/// provider. The api_key lives in the encrypted secret store under
/// `flowntier/custom/<id>`.
pub(super) async fn add_custom_provider(
    body: Value,
    state: Arc<ServerState>,
) -> Result<(u16, Value), String> {
    // v0.4.22 (event 000096): the Tauri shell
    // (`apps/desktop/src-tauri/src/lib.rs:add_custom_provider`)
    // sends a body with keys: id, display_name, kind, base_url,
    // api_key_env, models. The previous handler read `name`
    // (missing field → 400), `api_key` (raw value), and ignored
    // `models`. As a result every "添加自定义中转站" form
    // failed silently and custom_provider table stayed empty,
    // forcing the chairman into the preset mimo (whose base_url
    // is api.xiaomimimo.com, not the relay at
    // token-plan.cn.xiaomimimo.com) — 208 × 401.
    //
    // Fix: read all the fields the shell actually sends; persist
    // models (so the UI's display matches reality); save the API
    // key under the env var name the shell specified, NOT
    // auto-generated.
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'id'".to_string())?
        .to_string();
    let display_name = body
        .get("display_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'display_name'".to_string())?
        .to_string();
    let base_url = body
        .get("base_url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'base_url'".to_string())?
        .to_string();
    let kind = body
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("openai-compatible")
        .to_string();
    let api_key_env = body
        .get("api_key_env")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    // v0.4.22 (event 000096 + 000118-fix4): the user's
    // models now carry their own display_name /
    // thinking_strength / context_length. We persist the FULL
    // row into the model_cache (not just the id) so the role
    // picker can show real metadata instead of "(unknown model)".
    // Empty thinking_strength / context_length collapse to
    // null in the cache so the runtime falls back to "no
    // hint" instead of forcing a default.
    // v0.4.30 (audit 000130): the user-curated POST body is
    // already the authoritative metadata, but if the user
    // previously PUT an override for one of these model ids
    // (e.g. to force a thinking_strength), overlay it now so
    // the runtime sees a single merged view.
    let override_rows = state
        .repo
        .list_model_overrides_for_provider(&id)
        .await
        .unwrap_or_default();
    let ovr_map: std::collections::HashMap<&str, (Option<i64>, Option<String>)> = override_rows
        .iter()
        .map(|(mid, c, t)| (mid.as_str(), (*c, t.clone())))
        .collect();
    let models_rows: Vec<Value> = body
        .get("models")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let mid = m.get("id").and_then(|x| x.as_str())?;
                    let display = m
                        .get("display_name")
                        .and_then(|x| x.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or(mid);
                    let thinking = m
                        .get("thinking_strength")
                        .and_then(|x| x.as_str())
                        .filter(|s| matches!(*s, "low" | "medium" | "high"));
                    let context = m
                        .get("context_length")
                        .and_then(|x| x.as_i64())
                        .filter(|n| *n > 0);
                    let ovr = ovr_map.get(mid);
                    let thinking_final = ovr
                        .and_then(|(_, t)| t.clone())
                        .or_else(|| thinking.map(String::from));
                    let context_final = ovr.and_then(|(c, _)| c.as_ref().copied()).or(context);
                    let mut obj = json!({
                        "id": mid,
                        "display_name": display,
                    });
                    if let Some(o) = obj.as_object_mut() {
                        if let Some(t) = thinking_final {
                            o.insert("thinking_strength".to_string(), json!(t));
                        }
                        if let Some(c) = context_final {
                            o.insert("context_length".to_string(), json!(c));
                        }
                        o.insert("source".to_string(), json!("user-curated"));
                    }
                    Some(obj)
                })
                .collect()
        })
        .unwrap_or_default();
    let default_model = models_rows
        .first()
        .and_then(|m| m.get("id").and_then(|x| x.as_str()))
        .map(String::from);

    if id.is_empty() || id.len() > 64 {
        return Ok((400, json!({ "error": "id must be 1..=64 chars" })));
    }
    if display_name.is_empty() || display_name.len() > 64 {
        return Ok((400, json!({ "error": "display_name must be 1..=64 chars" })));
    }
    if !base_url.starts_with("https://") && !base_url.starts_with("http://") {
        return Ok((
            400,
            json!({ "error": "base_url must start with http(s)://" }),
        ));
    }
    if kind != "openai-compatible" && kind != "anthropic-compatible" {
        return Ok((
            400,
            json!({ "error": "kind must be openai-compatible or anthropic-compatible" }),
        ));
    }

    let now = chrono::Utc::now().timestamp();
    let row = storage::CustomProvider {
        id: id.clone(),
        name: display_name.clone(),
        base_url: base_url.clone(),
        kind: kind.clone(),
        default_model,
        enabled: true,
        created_at: now,
        updated_at: now,
    };
    state
        .repo
        .insert_custom_provider(&row)
        .await
        .map_err(|e| format!("insert_custom: {e}"))?;

    // v0.4.22 (event 000096): persist the models list. The
    // custom_provider table only stores `default_model`; we
    // mirror the full list into the model_cache row so the
    // Settings UI can render them.
    if !models_rows.is_empty() {
        let body_str = serde_json::to_string(&models_rows).unwrap_or_else(|_| "[]".into());
        let _ = state
            .repo
            .put_model_cache(&storage::ModelCacheRow {
                provider_id: format!("custom:{id}"),
                models_json: body_str,
                fetched_at: now,
            })
            .await;
    }

    // If an api_key was supplied in the same POST (the Tauri
    // shell calls save_secret() first to encrypt-and-store,
    // then addCustomProvider), nothing else needed here. But
    // also fall back to reading api_key directly (legacy
    // compat) so any caller still works.
    if let Some(key) = body.get("api_key").and_then(|v| v.as_str()) {
        let secret_name = api_key_env
            .clone()
            .unwrap_or_else(|| format!("flowntier/custom/{id}"));
        state
            .secrets
            .put(&secret_name, key)
            .await
            .map_err(|e| format!("put secret: {e}"))?;
    }

    Ok((
        201,
        json!({
            "id": id,
            "name": display_name,
            "base_url": base_url,
            "kind": kind,
            "default_model": models_rows.first(),
            "models": models_rows,
            "enabled": true,
        }),
    ))
}

/// DELETE /api/providers/custom/{id} — remove a custom provider
/// and any associated secret.
pub(super) async fn delete_custom_provider(
    body: Value,
    state: Arc<ServerState>,
) -> Result<(u16, Value), String> {
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'id' in path".to_string())?
        .to_string();

    let removed = state
        .repo
        .delete_custom_provider(&id)
        .await
        .map_err(|e| format!("delete_custom: {e}"))?;
    // Best-effort: clean up the associated api_key secret.
    // v0.4.30: secret lives in the `flowntier/custom/<id>`
    // namespace (matches Settings → CustomProviderForm and the
    // list_providers handler).
    let secret_name = format!("flowntier/custom/{id}");
    let _ = state.secrets.delete(&secret_name).await;
    Ok((
        200,
        json!({
            "id": id,
            "deleted": removed,
        }),
    ))
}

/// PUT /api/providers/{id}/models/{model} — upsert a
/// per-(provider, model) metadata override. Used by the
/// Settings → provider detail "✎ edit" inline form so the
/// chairman can fix e.g. MiniMax-M3 context_length to 1M
/// without waiting for an app release.
///
/// Body (all fields optional — only what is supplied is
/// touched, the rest inherits the existing override row):
///   {
///     context_length?:    number | null,
///     thinking_strength?: "low" | "medium" | "high" | null,
///   }
///
/// `context_length: null` and `thinking_strength: null`
/// explicitly clear that field on the override row (kept as
/// SQL NULL — the read path then falls back to the built-in
/// fallback for that field while still honoring the override
/// row's other column).
pub(super) async fn put_model_override(
    body: Value,
    state: Arc<ServerState>,
) -> Result<(u16, Value), String> {
    let provider_id = body
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'id' in path".to_string())?
        .to_string();
    let model_id = body
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'model' in path".to_string())?
        .to_string();

    // Outer Option = "field absent from body" → don't touch
    // existing row. Inner Option = "field present and null" →
    // clear it.
    let ctx: Option<Option<i64>> = body.get("context_length").map(|v| match v {
        Value::Null => None,
        Value::Number(n) => n.as_i64().filter(|x| *x > 0),
        _ => None,
    });
    let ts: Option<Option<String>> = body.get("thinking_strength").map(|v| match v {
        Value::Null => None,
        Value::String(s) if matches!(s.as_str(), "low" | "medium" | "high") => Some(s.to_string()),
        _ => None,
    });

    state
        .repo
        .upsert_model_override(&provider_id, &model_id, ctx, ts.clone())
        .await
        .map_err(|e| format!("upsert_model_override: {e}"))?;

    // Bust the model_cache so the next GET reflects the new
    // override. Empty fetch triggers the fallback path again,
    // which re-applies the overlay.
    let _ = state.repo.delete_model_cache(&provider_id).await;

    Ok((
        200,
        json!({
            "ok": true,
            "provider_id": provider_id,
            "model": model_id,
            "context_length": ctx.and_then(|c| c),
            "thinking_strength": ts.and_then(|t| t),
        }),
    ))
}

/// DELETE /api/providers/{id}/models/{model} — remove the
/// override row entirely. Subsequent reads fall back to the
/// built-in fallback list (preset) or live /v1/models (custom).
pub(super) async fn delete_model_override(
    body: Value,
    state: Arc<ServerState>,
) -> Result<(u16, Value), String> {
    let provider_id = body
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'id' in path".to_string())?
        .to_string();
    let model_id = body
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'model' in path".to_string())?
        .to_string();

    let removed = state
        .repo
        .delete_model_override(&provider_id, &model_id)
        .await
        .map_err(|e| format!("delete_model_override: {e}"))?;
    let _ = state.repo.delete_model_cache(&provider_id).await;

    Ok((
        200,
        json!({
            "ok": true,
            "provider_id": provider_id,
            "model": model_id,
            "removed": removed,
        }),
    ))
}

/// Walk every provider's model_cache; if any entry is older than
/// 1 hour, refresh it in the background. Non-blocking — returns
/// the number of caches that were stale (and therefore refreshed).
pub(super) async fn refresh_stale_caches(state: &Arc<ServerState>) -> usize {
    // We iterate the in-memory PRESETS list directly rather than
    // the `provider` DB table; the override flag (enabled=false)
    // only affects whether the UI shows the row, not whether the
    // cache should be populated.
    let custom = state.repo.list_custom_providers().await.unwrap_or_default();
    let now = chrono::Utc::now().timestamp();
    let mut stale = 0;

    // For each provider with a non-live endpoint, populate the
    // fallback list into the cache if it's empty. This handles
    // the "first time the user opens Settings" case where no
    // /v1/models has ever been fetched.
    for preset in crate::providers::PRESETS {
        if !preset.has_live_models_endpoint {
            let cached = state.repo.get_model_cache(preset.id).await.ok().flatten();
            if cached.is_none() {
                let entries: &[crate::providers::ModelEntry] =
                    crate::providers::OPENAI_FALLBACK_MODELS
                        .iter()
                        .find(|(pid, _)| *pid == preset.id)
                        .map(|(_, m)| *m)
                        .unwrap_or(crate::providers::ANTHROPIC_FALLBACK_MODELS);
                let models: Vec<Value> = entries
                    .iter()
                    .map(|m| {
                        json!({
                            "id": m.id,
                            "display_name": m.display_name,
                            "thinking_strength": m.thinking_strength,
                            "context_length": m.context_length,
                            "source": "fallback",
                        })
                    })
                    .collect();
                let body = serde_json::to_string(&models).unwrap_or_default();
                let _ = state
                    .repo
                    .put_model_cache(&storage::ModelCacheRow {
                        provider_id: preset.id.to_string(),
                        models_json: body,
                        fetched_at: now,
                    })
                    .await;
                stale += 1;
            }
            continue;
        }
        let cached = state.repo.get_model_cache(preset.id).await.ok().flatten();
        let needs_refresh = match cached {
            Some(c) => now - c.fetched_at > 3600,
            None => true,
        };
        if needs_refresh {
            // Lazy refresh: try to fetch via SecretStore. If no
            // secret is configured, skip — the UI will surface
            // 'no API key' on user-driven refresh.
            let secret_name = preset.secret_name.to_string();
            let api_key = match state.secrets.reveal(&secret_name).await {
                Ok(k) => k,
                Err(_) => continue,
            };
            let url = format!("{}/models", preset.base_url.trim_end_matches('/'));
            if let Ok(client) = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
            {
                if let Ok(resp) = client
                    .get(&url)
                    .header("Authorization", format!("Bearer {}", api_key.as_str()))
                    .send()
                    .await
                {
                    if resp.status().is_success() {
                        if let Ok(body) = resp.json::<Value>().await {
                            let models: Vec<Value> = body
                                .get("data")
                                .and_then(|d| d.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|m| {
                                            m.get("id").and_then(|v| v.as_str()).map(|id| {
                                                json!({
                                                    "id": id, "display_name": id,
                                                    "source": "live",
                                                })
                                            })
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            let body_str = serde_json::to_string(&models).unwrap_or_default();
                            let _ = state
                                .repo
                                .put_model_cache(&storage::ModelCacheRow {
                                    provider_id: preset.id.to_string(),
                                    models_json: body_str,
                                    fetched_at: now,
                                })
                                .await;
                            stale += 1;
                        }
                    }
                }
            }
        }
    }

    // Custom providers: just check staleness, don't refetch
    // (the user adds them manually and may have a slow API).
    for c in custom {
        let cached = state.repo.get_model_cache(&c.id).await.ok().flatten();
        if cached.is_none() {
            stale += 1;
        }
    }

    stale
}
