//! Provider read handlers (list/patch/models).

use super::*;

// ── v0.4 provider endpoints ───────────────────────────────────

/// GET /api/providers — list built-in presets + custom providers,
/// joined with the per-preset `provider` row for overrides and
/// `secret` table for `has_secret`.
pub(super) async fn list_providers(
    _body: Value,
    state: Arc<ServerState>,
) -> Result<(u16, Value), String> {
    // Pull all rows once.
    let preset_rows = state
        .repo
        .list_providers()
        .await
        .map_err(|e| format!("list_providers: {e}"))?;
    let secret_rows = state
        .secrets
        .list()
        .await
        .map_err(|e| format!("list_secrets: {e}"))?;
    let custom_rows = state
        .repo
        .list_custom_providers()
        .await
        .map_err(|e| format!("list_custom: {e}"))?;

    // Index overrides + secrets by id.
    let override_by_id: std::collections::HashMap<&str, &storage::ProviderRow> =
        preset_rows.iter().map(|p| (p.id.as_str(), p)).collect();
    let secret_names: std::collections::HashSet<&str> =
        secret_rows.iter().map(|s| s.name.as_str()).collect();

    let presets: Vec<Value> = crate::providers::PRESETS
        .iter()
        .map(|p| {
            let ovr = override_by_id.get(p.id);
            let enabled = ovr.map(|r| r.enabled).unwrap_or(true);
            let default_model = ovr
                .and_then(|r| r.default_model.clone())
                .unwrap_or_else(|| p.default_model.to_string());
            let base_url = ovr
                .and_then(|r| r.base_url.clone())
                .unwrap_or_else(|| p.base_url.to_string());
            json!({
                "id": p.id,
                "kind": "preset",
                "display_name": p.display_name,
                "api_kind": p.kind,
                "base_url": base_url,
                "default_model": default_model,
                "secret_name": p.secret_name,
                "has_secret": secret_names.contains(p.secret_name),
                "enabled": enabled,
                "note": p.note,
                "has_live_models_endpoint": p.has_live_models_endpoint,
                // v0.4.15 (event 000051): emit empty models array +
                // is_local:false so the TS ProviderInfo type can read
                // both fields without runtime undefined. UI must hit
                // discover_models to populate models[].
                "models": [],
                "is_local": false,
            })
        })
        .collect();

    let custom: Vec<Value> = custom_rows
        .iter()
        .map(|c| {
            // v0.4.30 (audit 000130): the previous secret name
            // `CUSTOM_PROVIDER_KEY_<id>` was visually similar to
            // shell env var naming. We now use the
            // `flowntier/custom/<id>` internal namespace (matching
            // resolve_role_provider's convention). Migration 0008
            // renames any pre-existing custom rows.
            let custom_secret = format!("flowntier/custom/{}", c.id);
            let legacy_secret = format!("CUSTOM_PROVIDER_KEY_{}", c.id);
            let has_sec = secret_names.contains(custom_secret.as_str())
                || secret_names.contains(legacy_secret.as_str());
            json!({
                "id": c.id,
                "kind": "custom",
                "display_name": c.name,
                "api_kind": c.kind,
                "base_url": c.base_url,
                "default_model": c.default_model,
                "secret_name": custom_secret,
                "has_secret": has_sec,
                "enabled": c.enabled,
                "note": null,
                "has_live_models_endpoint": c.kind == "openai-compatible",
            })
        })
        .collect();

    Ok((
        200,
        json!({
            "providers": presets,
            "custom_providers": custom,
            "count": presets.len() + custom.len(),
        }),
    ))
}

/// PATCH /api/providers/{id} — toggle enabled, override
/// default_model, override base_url. Reads from body:
///   { enabled?: bool, default_model?: string|null,
///     base_url?: string|null }
pub(super) async fn patch_provider(
    body: Value,
    state: Arc<ServerState>,
) -> Result<(u16, Value), String> {
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'id' in path".to_string())?
        .to_string();

    // Built-in presets: validate id + upsert into the `provider`
    // table (which was pre-populated by migration 0003).
    if crate::providers::get(&id).is_none() {
        return Ok((404, json!({ "error": format!("unknown provider: {id}") })));
    }

    // Load current row, merge with patch fields.
    let mut row = state
        .repo
        .get_provider(&id)
        .await
        .map_err(|e| format!("get_provider: {e}"))?
        .unwrap_or_else(|| storage::ProviderRow {
            id: id.clone(),
            enabled: true,
            default_model: None,
            base_url: None,
            preset_json: "{}".into(),
            updated_at: chrono::Utc::now().timestamp(),
        });
    let now = chrono::Utc::now().timestamp();
    let mut changed: Vec<&str> = Vec::new();
    if let Some(v) = body.get("enabled").and_then(|v| v.as_bool()) {
        row.enabled = v;
        changed.push("enabled");
    }
    if let Some(v) = body.get("default_model") {
        row.default_model = v.as_str().map(|s| s.to_string());
        changed.push("default_model");
    }
    if let Some(v) = body.get("base_url") {
        row.base_url = v.as_str().map(|s| s.to_string());
        changed.push("base_url");
    }
    row.updated_at = now;
    state
        .repo
        .upsert_provider(&row)
        .await
        .map_err(|e| format!("upsert_provider: {e}"))?;

    Ok((
        200,
        json!({
            "id": id,
            "updated": changed,
            "enabled": row.enabled,
            "default_model": row.default_model,
            "base_url": row.base_url,
        }),
    ))
}

/// GET /api/providers/{id}/models — fetch available models.
/// Cached for 1 hour per provider id.
pub(super) async fn list_models(
    body: Value,
    state: Arc<ServerState>,
) -> Result<(u16, Value), String> {
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'id' in path".to_string())?
        .to_string();

    // event 000110 (fix D1): the user may have hidden some models
    // via `PUT /api/providers/{id}/models/{model}/disable`. Load
    // the set once per request and apply it to every return path
    // below so the catalog the webview sees always excludes the
    // user's hidden models.
    let disabled: std::collections::HashSet<String> = state
        .repo
        .list_disabled_models()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|(pid, _)| pid == &id)
        .map(|(_, mid)| mid)
        .collect();
    let filter_disabled = |models: Vec<Value>| -> Vec<Value> {
        if disabled.is_empty() {
            models
        } else {
            models
                .into_iter()
                .filter(|m| {
                    m.get("id")
                        .and_then(|v| v.as_str())
                        .map(|mid| !disabled.contains(mid))
                        .unwrap_or(true)
                })
                .collect()
        }
    };

    // Check cache first.
    if let Ok(Some(cached)) = state.repo.get_model_cache(&id).await {
        let now = chrono::Utc::now().timestamp();
        if now - cached.fetched_at < 3600 {
            let models: Vec<Value> =
                serde_json::from_str(&cached.models_json).unwrap_or_else(|_| Vec::new());
            return Ok((
                200,
                json!({
                    "ok": true,
                    "provider_id": id,
                    "models": filter_disabled(models),
                    "cached": true,
                    "fetched_at": cached.fetched_at,
                }),
            ));
        }
    }

    // Resolve provider (preset or custom). Only base_url and
    // has_live are used downstream; kind + default_model are
    // extracted for clarity but ignored (they're available via
    // the provider row if a future caller needs them).
    let (base_url, has_live) = if let Some(preset) = crate::providers::get(&id) {
        (preset.base_url.to_string(), preset.has_live_models_endpoint)
    } else {
        // Look up custom_provider.
        let custom = match state
            .repo
            .get_custom_provider(&id)
            .await
            .map_err(|e| format!("get_custom: {e}"))?
        {
            Some(c) => c,
            None => {
                return Ok((
                    404,
                    json!({
                        "ok": false,
                        "error": format!("unknown provider: {id}"),
                    }),
                ))
            }
        };
        (custom.base_url.clone(), custom.kind == "openai-compatible")
    };

    // Anthropic has no /v1/models endpoint — return the hard-coded
    // fallback list directly.
    if !has_live {
        // v0.4.16: prefer the per-provider OPENAI_FALLBACK_MODELS
        // entry if one exists, then fall back to Anthropic's.
        let entries: &[crate::providers::ModelEntry] = crate::providers::OPENAI_FALLBACK_MODELS
            .iter()
            .find(|(pid, _)| *pid == id)
            .map(|(_, m)| *m)
            .unwrap_or(crate::providers::ANTHROPIC_FALLBACK_MODELS);
        // v0.4.30 (audit 000130): overlay the user's
        // per-(provider, model) metadata overrides on top of
        // the built-in fallback list. Lets the chairman fix
        // e.g. MiniMax-M3 context_length to 1M without
        // waiting for an app release.
        let overrides = state
            .repo
            .list_model_overrides_for_provider(&id)
            .await
            .unwrap_or_default();
        let ovr_map: std::collections::HashMap<&str, (Option<i64>, Option<String>)> = overrides
            .iter()
            .map(|(mid, c, t)| (mid.as_str(), (*c, t.clone())))
            .collect();
        let models: Vec<Value> = entries
            .iter()
            .map(|m| {
                let ovr = ovr_map.get(m.id);
                json!({
                    "id": m.id,
                    "display_name": m.display_name,
                    "thinking_strength": ovr.and_then(|(_, t)| t.clone())
                        .unwrap_or_else(|| m.thinking_strength.to_string()),
                    "context_length": ovr.and_then(|(c, _)| c.as_ref().copied())
                        .unwrap_or(m.context_length as i64),
                    "source": "fallback",
                })
            })
            .collect();
        let body_str = serde_json::to_string(&models).unwrap();
        let _ = state
            .repo
            .put_model_cache(&storage::ModelCacheRow {
                provider_id: id.clone(),
                models_json: body_str,
                fetched_at: chrono::Utc::now().timestamp(),
            })
            .await;
        return Ok((
            200,
            json!({
                "ok": true,
                "provider_id": id,
                "models": filter_disabled(models),
                "cached": false,
                "fallback": true,
            }),
        ));
    }

    // OpenAI-compatible fetch.
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let secret_name = if crate::providers::get(&id).is_some() {
        crate::providers::get(&id).unwrap().secret_name.to_string()
    } else {
        // v0.4.30: use the flowntier/custom/<id> namespace
        // (matches the value baked into list_providers + the
        // client-side CustomProviderForm).
        format!("flowntier/custom/{id}")
    };
    let api_key = match state.secrets.reveal(&secret_name).await {
        Ok(k) => k,
        Err(crate::secrets::SecretStoreError::NotFound(_)) => {
            let legacy_name = format!("CUSTOM_PROVIDER_KEY_{id}");
            match state.secrets.reveal(&legacy_name).await {
                Ok(k) => k,
                Err(crate::secrets::SecretStoreError::NotFound(_)) => {
                    return Ok((
                        200,
                        json!({
                            "ok": false,
                            "error": "no API key configured",
                            "secret_name": secret_name,
                            "url": url,
                            "provider_id": id,
                        }),
                    ));
                }
                Err(e) => {
                    return Ok((
                        200,
                        json!({
                            "ok": false,
                            "error": format!("reveal: {e}"),
                            "provider_id": id,
                        }),
                    ))
                }
            }
        }
        Err(e) => {
            return Ok((
                200,
                json!({
                    "ok": false,
                    "error": format!("reveal: {e}"),
                    "provider_id": id,
                }),
            ))
        }
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("reqwest build: {e}"))?;
    let resp = match client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key.as_str()))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return Ok((
                200,
                json!({
                    "ok": false,
                    "error": format!("network error: {e}"),
                    "url": url,
                    "provider_id": id,
                }),
            ));
        }
    };
    let status = resp.status();
    if !status.is_success() {
        return Ok((
            200,
            json!({
                "ok": false,
                "error": format!("provider returned {status}"),
                "url": url,
                "provider_id": id,
            }),
        ));
    }
    let body: Value = match resp.json().await {
        Ok(b) => b,
        Err(e) => {
            return Ok((
                200,
                json!({
                    "ok": false,
                    "error": format!("parse {url}: {e}"),
                    "url": url,
                    "provider_id": id,
                }),
            ));
        }
    };
    // OpenAI-compatible /models response shape:
    // { "object": "list", "data": [{ id, object, ... }, ...] }
    // v0.4.30 (audit 000130): overlay per-(provider, model)
    // metadata overrides onto the live list too, so the user
    // can fix the reported context_length / thinking_strength
    // even for providers with a real /models endpoint.
    let overrides = state
        .repo
        .list_model_overrides_for_provider(&id)
        .await
        .unwrap_or_default();
    let ovr_map: std::collections::HashMap<&str, (Option<i64>, Option<String>)> = overrides
        .iter()
        .map(|(mid, c, t)| (mid.as_str(), (*c, t.clone())))
        .collect();
    let models: Vec<Value> = body
        .get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    m.get("id").and_then(|v| v.as_str()).map(|id| {
                        let ovr = ovr_map.get(id);
                        json!({
                            "id": id,
                            "display_name": id,
                            "thinking_strength": ovr.and_then(|(_, t)| t.clone()),
                            "context_length": ovr.and_then(|(c, _)| c.as_ref().copied()),
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
            provider_id: id.clone(),
            models_json: body_str,
            fetched_at: chrono::Utc::now().timestamp(),
        })
        .await;
    Ok((
        200,
        json!({
            "ok": true,
            "provider_id": id,
            "models": filter_disabled(models),
            "cached": false,
            "fallback": false,
            "url": url,
        }),
    ))
}
