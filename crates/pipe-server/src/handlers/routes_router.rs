//! Router role/model routes and resolve_role helpers.

use super::*;

pub(super) fn register_resolve(d: &mut Dispatcher, state: Arc<ServerState>) {
    // ── v0.4.20: GET /api/router/roles/{role}/resolve ──────
    // Returns {ok:true, role, provider_short, model_id, base_url,
    // api_kind, has_key, fallback_chain, quota_status} on
    // success or {ok:false, role, error, hint} on failure.
    // Always HTTP 200 so the frontend can render inline without
    // surfacing 5xx. quota_status embeds the per-(role, model)
    // quota_failures row (or null) so Settings can show
    // "上次失败 · 等 5h 刷新点" inline.
    let s_resolve = state.clone();
    d.register("GET", "/api/router/roles/{role}/resolve", move |body| {
        let s = s_resolve.clone();
        let role = body
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Box::pin(async move {
            let resolved = match resolve_role(&s, &role).await {
                Ok(r) => r,
                Err(e) => {
                    return Ok((
                        200,
                        json!({
                            "ok": false,
                            "role": role,
                            "error": e,
                            "hint": "open Settings → 角色 → 模型 分配",
                        }),
                    ));
                }
            };
            let quota_status = match s.repo.quota_status_for(&role, &resolved.model_id).await {
                Ok(Some(row)) => Some(json!({
                    "status": row.status,
                    "attempt_count": row.attempt_count,
                    "last_error_at": row.last_error_at,
                    "last_error_message": row.last_error_message,
                    "next_attempt_at": row.next_attempt_at,
                })),
                _ => None,
            };
            Ok((
                200,
                json!({
                    "ok": true,
                    "role": resolved.role,
                    "provider_short": resolved.provider_short,
                    "model_id": resolved.model_id,
                    "base_url": resolved.base_url,
                    "api_kind": resolved.api_kind,
                    "has_key": !resolved.api_key.is_empty(),
                    "fallback_chain": resolved.fallback_chain,
                    "quota_status": quota_status,
                }),
            ))
        })
    });
}

pub(super) fn register_roles_models(d: &mut Dispatcher, state: Arc<ServerState>) {
    // Router: per-role default-model + fallback chain. v0.4 fixes
    // v0.4.16 (event 000052): chairman rejected the v0.4.15 hard-coded
    // defaults of "anthropic:claude-opus-4-8" + ["anthropic:claude-sonnet-4-6"].
    // Every role now starts empty — the user picks what they actually have.
    // v0.4.18 (event 000054): GET now overlays the in-memory defaults
    // with any DB-stored role_overrides (see migrations/0004 and
    // storage::Repository::list_role_overrides). DB rows win on
    // match. The 6 in-memory defaults still appear for roles the
    // chairman hasn't touched yet.
    let get_roles_state = state.clone();
    d.register("GET", "/api/router/roles", move |_body| {
        let s = get_roles_state.clone();
        Box::pin(async move {
            // In-memory defaults.
            let mut roles = serde_json::Map::new();
            for role_id in [
                "agent:chief",
                "agent:worker",
                "agent:planner",
                "agent:critic:a",
                "agent:critic:b",
                "agent:reporter",
            ] {
                roles.insert(
                    role_id.to_string(),
                    json!({ "default_model": "", "fallback_chain": [] }),
                );
            }
            // Overlay DB overrides.
            let overrides = s.repo.list_role_overrides().await.unwrap_or_default();
            for ov in overrides {
                roles.insert(
                    ov.role_id,
                    json!({
                        "default_model": ov.default_model,
                        "fallback_chain": ov.fallback_chain,
                    }),
                );
            }
            let roles_array: Vec<Value> = roles
                .into_iter()
                .map(|(role, body)| {
                    let mut obj = json!({ "role": role });
                    if let Some(obj_mut) = obj.as_object_mut() {
                        if let Some(b) = body.as_object() {
                            for (k, v) in b {
                                obj_mut.insert(k.clone(), v.clone());
                            }
                        }
                    }
                    obj
                })
                .collect();
            Ok((200, json!({ "roles": roles_array })))
        })
    });

    // /api/router/models — aggregate model_cache across all
    // configured providers. The agent loop uses this when
    // resolving "<role>:default" references.
    let router_models_state = state.clone();
    d.register("GET", "/api/router/models", move |_body| {
        let s = router_models_state.clone();
        Box::pin(async move {
            // Lazy fetch: if any provider's cache is older than
            // 1h, refresh it in the background. We don't block.
            let _ = refresh_stale_caches(&s).await;
            let providers = s.repo.list_providers().await.unwrap_or_default();
            let custom = s.repo.list_custom_providers().await.unwrap_or_default();
            // event 000110 (fix D1): filter out any model the user
            // has hidden via disable_model. Keyed by raw provider_id
            // for presets and by `custom:<id>` for custom providers
            // (matches the keys we emit below).
            let disabled_pairs: std::collections::HashSet<(String, String)> = s
                .repo
                .list_disabled_models()
                .await
                .unwrap_or_default()
                .into_iter()
                .collect();
            let mut by_id = std::collections::HashMap::new();
            for p in providers {
                if let Ok(Some(c)) = s.repo.get_model_cache(&p.id).await {
                    by_id.insert(p.id.clone(), c);
                }
            }
            // v0.4.22 (event 000097): the previous version
            // returned ONLY providers that had a model_cache
            // row. For a fresh install the cache is empty (no
            // model has been pulled live yet), so the role
            // picker in Settings showed zero models for the
            // default mimo role — locking the chairman out
            // of editing role_overrides (you can't pick an
            // empty option).
            //
            // Fix: for any provider (preset OR custom) that
            // has no cache row, fall back to the static
            // OPENAI_FALLBACK_MODELS catalog. For custom
            // providers, fall back to their stored
            // `default_model` (which is the only model the
            // chairman explicitly added). This guarantees the
            // role picker always has at least one selectable
            // option per provider.
            // v0.4.22 (event 000118, fix 2): the chairman
            // reported that the Settings → 角色 → 模型 dropdown
            // rendered every row as `(unknown provider) ·
            // (unknown model)`. Root cause: this endpoint
            // emitted a nested shape
            //   `{ provider_id, models: { id, display_name, ... } }`
            // but the TS `AvailableModel` type expects a FLAT
            // shape
            //   `{ provider, provider_display, model, display_name }`.
            // Tauri dropped the missing top-level fields to
            // `undefined`, and the frontend coalesced them to
            // `(unknown provider) · (unknown model)`.
            //
            // Fix: emit one flat row per model. Provider
            // id (the storage key) and provider display name
            // (the user-facing brand) come from PRESETS for
            // built-ins and from `c.name` for custom providers.
            let mut models: Vec<Value> = Vec::new();
            // Presets — overlay catalog when cache is missing.
            for preset in crate::providers::PRESETS.iter() {
                match by_id.get(preset.id) {
                    Some(cache) => {
                        if let Ok(m) = serde_json::from_str::<Vec<Value>>(&cache.models_json) {
                            for mm in m {
                                // event 000110 (fix D1): skip
                                // user-disabled models so the role
                                // router cannot pick them.
                                if let Some(mid) = mm.get("id").and_then(|v| v.as_str()) {
                                    if disabled_pairs
                                        .contains(&(preset.id.to_string(), mid.to_string()))
                                    {
                                        continue;
                                    }
                                }
                                // Flatten: keep every field the
                                // live /v1/models endpoint emits
                                // (so the user still sees
                                // thinking_strength / context_length
                                // / source when available) AND
                                // hoist the provider metadata to
                                // the top level.
                                let mut row = mm;
                                if let Some(obj) = row.as_object_mut() {
                                    obj.insert("provider".to_string(), json!(preset.id));
                                    obj.insert(
                                        "provider_display".to_string(),
                                        json!(preset.display_name),
                                    );
                                }
                                models.push(row);
                            }
                        }
                    }
                    None => {
                        // Pull the fallback catalog entry for this
                        // preset (only has ids for the openai /
                        // anthropic tiers).
                        if let Some((_, entries)) = crate::providers::OPENAI_FALLBACK_MODELS
                            .iter()
                            .find(|(pid, _)| *pid == preset.id)
                        {
                            for entry in *entries {
                                if disabled_pairs
                                    .contains(&(preset.id.to_string(), entry.id.to_string()))
                                {
                                    continue;
                                }
                                models.push(json!({
                                    "provider": preset.id,
                                    "provider_display": preset.display_name,
                                    "id": entry.id,
                                    "display_name": entry.display_name,
                                    "thinking_strength": entry.thinking_strength,
                                    "context_length": entry.context_length,
                                    "source": "fallback",
                                }));
                            }
                        }
                    }
                }
            }
            // Custom providers — overlay catalog or use stored
            // default_model.
            //
            // v0.4.22 (event 000097): cache key is the bare
            // custom id (e.g. "mimimu"), matching what
            // `add_custom_provider` writes via
            // `repo.put_model_cache("custom:<id>", ...)`. The
            // prefix in the storage key is for SQL row
            // disambiguation only; the runtime by_id map keys
            // are normalized to the bare id so the role
            // resolver can look it up via
            // `state.repo.get_custom_provider(&provider_short)`.
            //
            // v0.4.22 (fix 2): same flattening as presets —
            // emit `{ provider: "custom:<id>", provider_display: <c.name>,
            // id, display_name, ... }` directly.
            for c in custom {
                let custom_pid = format!("custom:{}", c.id);
                if let Ok(Some(cache)) = s.repo.get_model_cache(&c.id).await {
                    by_id.insert(c.id.clone(), cache);
                }
                if let Some(cache) = by_id.get(c.id.as_str()) {
                    if let Ok(m) = serde_json::from_str::<Vec<Value>>(&cache.models_json) {
                        for mm in m {
                            if let Some(mid) = mm.get("id").and_then(|v| v.as_str()) {
                                if disabled_pairs.contains(&(custom_pid.clone(), mid.to_string())) {
                                    continue;
                                }
                            }
                            let mut row = mm;
                            if let Some(obj) = row.as_object_mut() {
                                obj.insert("provider".to_string(), json!(custom_pid));
                                obj.insert("provider_display".to_string(), json!(c.name));
                            }
                            models.push(row);
                        }
                    }
                } else if let Some(dm) = c.default_model.as_ref() {
                    // event 000110 (fix D1): also honor the disabled
                    // set for the custom provider's stored
                    // default_model — a hidden default_model is a
                    // contradiction, so don't surface it.
                    if !disabled_pairs.contains(&(custom_pid.clone(), dm.clone())) {
                        models.push(json!({
                            "provider": custom_pid,
                            "provider_display": c.name,
                            "id": dm,
                            "display_name": dm,
                            "thinking_strength": "medium",
                            "context_length": null,
                            "source": "fallback",
                        }));
                    }
                }
            }
            Ok((
                200,
                json!({
                    "models": models,
                    "count": models.len(),
                }),
            ))
        })
    });
}

pub(super) fn register_resolve_placeholder(d: &mut Dispatcher, state: Arc<ServerState>) {
    // v0.4.19 (event 000055): GET /api/router/roles/{role}/resolve
    // resolves a role's default_model + fallback_chain + keyring
    // availability into a single structured preview, used by the
    // ChatZone status line. Always returns 200 with `{ok:true,...}`
    // or `{ok:false,error}` so the frontend can render inline.
    let resolve_state = state.clone();
    d.register("GET", "/api/router/roles/{role}/resolve", move |body| {
        let s = resolve_state.clone();
        Box::pin(async move {
            let role = body
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if role.is_empty() {
                return Ok((
                    200,
                    json!({ "ok": false, "error": "missing 'role' in path" }),
                ));
            }
            match resolve_role(&s, &role).await {
                Ok(r) => Ok((
                    200,
                    json!({
                        "ok": true,
                        "role": r.role,
                        "provider_short": r.provider_short,
                        "model_id": r.model_id,
                        "base_url": r.base_url,
                        "api_kind": r.api_kind,
                        "has_key": true,
                        "fallback_chain": r.fallback_chain,
                    }),
                )),
                Err(e) => Ok((
                    200,
                    json!({
                        "ok": false,
                        "role": role,
                        "error": e,
                    }),
                )),
            }
        })
    });
}

pub(super) fn register_put_roles(d: &mut Dispatcher, state: Arc<ServerState>) {
    let state_for_router_roles = Arc::clone(&state);
    d.register("PUT", "/api/router/roles", move |body| {
        // v0.4.18 (event 000054): real persistence. Body shape:
        // { "roles": [
        //     { "role": "agent:chief", "default_model": "...",
        //       "fallback_chain": ["provider:model", ...] },
        //     ...
        // ] }
        // We iterate and upsert each into role_overrides. Empty
        // default_model / empty fallback_chain are valid
        // overrides (user explicitly cleared the defaults).
        let s = Arc::clone(&state_for_router_roles);
        Box::pin(async move {
            let Some(roles_arr) = body.get("roles").and_then(|v| v.as_array()) else {
                return Ok((
                    400,
                    json!({
                        "ok": false,
                        "error": "missing 'roles' array in body",
                    }),
                ));
            };
            let mut updated = 0usize;
            let mut bad: Vec<String> = Vec::new();
            for role in roles_arr {
                let Some(role_id) = role.get("role").and_then(|v| v.as_str()) else {
                    bad.push("<missing role id>".to_string());
                    continue;
                };
                let default_model = role
                    .get("default_model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let fallback_chain: Vec<String> = role
                    .get("fallback_chain")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                if let Err(e) = s
                    .repo
                    .upsert_role_override(role_id, default_model, &fallback_chain)
                    .await
                {
                    bad.push(format!("{}: {}", role_id, e));
                    continue;
                }
                updated += 1;
            }
            Ok((
                200,
                json!({
                    "ok": true,
                    "updated": updated,
                    "errors": bad,
                }),
            ))
        })
    });
}

/// v0.4.19 (event 000055): resolve a role's default model from
/// the role_overrides SQL table and the matching preset. Returns
/// the (provider, model, base_url, secret_name, has_key, api_key)
/// tuple so the caller can build an OpenAiProvider. Used by both
/// `run_task` and `GET /api/router/roles/{role}/resolve`.
pub async fn resolve_role_for_orchestrator(
    state: &Arc<ServerState>,
    role: &str,
) -> Result<ResolvedRole, String> {
    resolve_role(state, role).await
}

pub(super) async fn resolve_role(
    state: &Arc<ServerState>,
    role: &str,
) -> Result<ResolvedRole, String> {
    // 1. Read the override row (DB). May be absent — caller uses
    //    in-memory defaults (which are all empty as of v0.4.16).
    let ov = state
        .repo
        .get_role_override(role)
        .await
        .map_err(|e| format!("get_role_override: {e}"))?;
    let (default_model, fallback_chain) = match ov {
        Some(r) => (r.default_model, r.fallback_chain),
        None => (String::new(), Vec::new()),
    };
    if default_model.is_empty() {
        return Err(
            "role not configured: open Settings → 角色 → 模型 分配 and pick a default_model".into(),
        );
    }
    // 2. Split "<provider_short>:<model_id>".
    let (provider_short, model_id) = match default_model.split_once(':') {
        Some((p, m)) => (p.to_string(), m.to_string()),
        None => {
            return Err(format!(
                "default_model '{}' must be in '<provider>:<model>' form",
                default_model
            ))
        }
    };
    // 3. Look up the preset FIRST, then fall back to
    //    custom_provider. The custom_provider table lets the
    //    chairman register relays (e.g. token-plan.cn.xiaomimimo.com)
    //    that the 9 built-in PRESETS don't cover. Without this
    //    fallback the chairman has to bypass his relay entirely.
    let (base_url, api_kind, secret_name) =
        if let Some(preset) = crate::providers::get(&provider_short) {
            (
                preset.base_url.to_string(),
                preset.kind.to_string(),
                preset.secret_name.to_string(),
            )
        } else {
            // v0.4.22 (event 000096): look up the custom
            // provider row by id. Determine the secret name
            // from the row's `kind` (env var name) — by
            // convention `flowntier/custom/<ID>` (lowercased id).
            // v0.4.30 (audit 000130): the old `CUSTOM_<ID>_API_KEY`
            // name collided visually with shell env var naming; we
            // now use the `flowntier/custom/<id>` internal namespace.
            // Migration 0008 also renames legacy custom rows.
            let cp = state
                .repo
                .get_custom_provider(&provider_short)
                .await
                .map_err(|e| format!("get_custom: {e}"))?
                .ok_or_else(|| {
                    format!(
                        "unknown provider preset or custom '{}' from default_model '{}' \
                     (Settings → 中转站 → 添加 custom relay, or change default_model)",
                        provider_short, default_model
                    )
                })?;
            let secret = format!("flowntier/custom/{}", provider_short.to_lowercase());
            (cp.base_url, cp.kind, secret)
        };
    // 4. Reveal the API key from the keychain. Empty defaults give
    //    503 so the chairman knows the cause.
    let legacy_secret = format!("CUSTOM_PROVIDER_KEY_{provider_short}");
    let api_key: Zeroizing<String> = match state.secrets.reveal(&secret_name).await {
        Ok(z) if !z.is_empty() => z,
        _ => match state.secrets.reveal(&legacy_secret).await {
            Ok(z) if !z.is_empty() => z,
            _ => {
                // ── v0.4.34 (audit 000132, root fix) ─────────────
                // The user removed this provider's API key from
                // Settings (or it was never set after they switched
                // models in the UI). The orchestrator still drives
                // this role through `<this provider>:<model>` because
                // `role_overrides.default_model` / `fallback_chain`
                // are independent of the secret table.
                //
                // Old behaviour: surface a 503 telling the user to
                // re-save the key. The next workflow run hits the
                // exact same 503. Patching the UI button to be more
                // prominent (v0.4.30) didn't help — the user already
                // *intentionally* removed the key.
                //
                // Root fix: cascade. For built-in presets, clear any
                // role_overrides rows that reference this provider
                // AND flip the provider row to `enabled=false` so the
                // Settings UI reflects "not configured" without the
                // user having to dig. The next workflow run lands on
                // the existing `default_model.is_empty()` branch
                // above (line ~2463) which surfaces the *actionable*
                // error: "open Settings → 角色 → 模型 分配 and pick a
                // default_model".
                //
                // Only built-in presets: custom-provider resolutions
                // below stay untouched — the user may be mid-typing
                // into a relay they just added and we shouldn't blow
                // away their role assignments while they configure.
                if crate::providers::get(&provider_short).is_some() {
                    tracing::warn!(
                        target: "flowntier_pipe",
                        provider = %provider_short,
                        role = %role,
                        secret_name = %secret_name,
                        "audit 000132: cascading cleanup — clearing role_overrides referencing provider with no API key"
                    );
                    match state
                        .repo
                        .clear_role_overrides_for_provider(&provider_short)
                        .await
                    {
                        Ok(n) => tracing::info!(
                            target: "flowntier_pipe",
                            provider = %provider_short, rows_cleared = n,
                            "audit 000132: role_overrides cleanup complete"
                        ),
                        Err(e) => tracing::warn!(
                            target: "flowntier_pipe",
                            provider = %provider_short, error = %e,
                            "audit 000132: clear_role_overrides_for_provider failed (continuing)"
                        ),
                    }
                    // Best-effort disable; the provider row may not
                    // exist yet (lazy-created on first GET).
                    if let Err(e) = state
                        .repo
                        .set_provider_enabled(&provider_short, false)
                        .await
                    {
                        tracing::warn!(
                            target: "flowntier_pipe",
                            provider = %provider_short, error = %e,
                            "audit 000132: set_provider_enabled(false) failed (continuing)"
                        );
                    }
                    return Err(format!(
                        "no API key configured for {} — role_overrides referencing this provider \
                     have been cleared; open Settings → 角色 → 模型 分配 to pick a new default",
                        secret_name
                    ));
                }
                return Err(format!(
                    "no API key configured for {} (set it in Settings → 供应商)",
                    secret_name
                ));
            }
        },
    };
    Ok(ResolvedRole {
        role: role.to_string(),
        provider_short,
        model_id,
        base_url,
        api_kind,
        secret_name,
        api_key,
        fallback_chain,
    })
}

/// Helper struct returned by `resolve_role`. Cheap to clone by
/// the OpenAiProvider ctor below.
pub struct ResolvedRole {
    pub role: String,
    pub provider_short: String,
    pub model_id: String,
    pub base_url: String,
    pub api_kind: String,
    pub secret_name: String,
    pub api_key: zeroize::Zeroizing<String>,
    pub fallback_chain: Vec<String>,
}
