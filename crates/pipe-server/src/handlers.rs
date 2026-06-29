//! Built-in RPC handlers.
//!
//! This module wires a handful of common endpoints so the Tauri
//! client has something to talk to. New handlers are added as
//! the Python routes get ported.

use agent_core::provider::openai::OpenAiProvider;
use agent_core::tool::ToolRegistry;
use agent_core::workspace::Workspace;
use agent_core::{Agent, AgentConfig, AgentEvent};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::dispatcher::Dispatcher;
use crate::secrets::SecretStore;

/// Shared state held by the pipe server.
#[derive(Clone)]
pub struct ServerState {
    /// Event bus that all event-pipe clients subscribe to.
    pub events: broadcast::Sender<AgentEvent>,
    /// Default tool registry.
    pub tools: Arc<ToolRegistry>,
    /// CWD-style workspace root for the current pipe server run.
    pub workspace: Workspace,
    /// v0.4: persistent secret store (OS keystore + AES-GCM).
    pub secrets: Arc<SecretStore>,
    /// v0.4: SQLite repository for provider / custom_provider /
    /// kv tables.
    pub repo: Arc<storage::Repository>,
}

impl ServerState {
    /// New default state. Opens the SQLite repo at
    /// `<data_dir>/storage.sqlite` and constructs a SecretStore
    /// bound to the same data dir.
    pub async fn new(
        workspace_root: std::path::PathBuf,
        data_dir: std::path::PathBuf,
    ) -> Self {
        let (events, _rx) = broadcast::channel(1024);

        let db_path = data_dir.join("storage.sqlite");
        let repo = match storage::Repository::open(&db_path).await {
            Ok(r) => Arc::new(r),
            Err(e) => {
                eprintln!("[flowntier-runtime] failed to open storage: {e}");
                // Fallback to in-memory; the v0.4 secret endpoints
                // will work but data won't persist.
                Arc::new(
                    storage::Repository::open_in_memory()
                        .await
                        .expect("in-memory storage always opens"),
                )
            }
        };
        let secrets = Arc::new(SecretStore::new(repo.clone(), data_dir));

        Self {
            events,
            tools: Arc::new(ToolRegistry::with_builtins()),
            workspace: Workspace::new(workspace_root, "flowntier"),
            secrets,
            repo,
        }
    }
}

/// Register every built-in handler on `d`.
pub fn register_all(d: &mut Dispatcher, state: ServerState) {
    let state = Arc::new(state);

    // Health check.
    let s1 = state.clone();
    d.register("GET", "/api/ping", move |_body| {
        let _ = &s1;
        Box::pin(async {
            Ok((200, json!({
                "ok": true,
                "runtime": "flowntier-rs",
                "version": env!("CARGO_PKG_VERSION"),
            })))
        })
    });

    // Version handshake (v0.4). Returns the sidecar's version +
    // a min_compatible field the Tauri shell compares against the
    // shell's expected version. If sidecar < min_compatible, the
    // shell shows a drift banner.
    //
    // The shell learns about the sidecar through:
    //   - this endpoint (read by the desktop shell on startup)
    //   - the CARGO_PKG_VERSION env var (written by the desktop
    //     shell's build script and read by the spawned sidecar
    //     process). For v0.4 we only have the endpoint; the
    //     cross-process var is a v0.5 nicety.
    d.register("GET", "/api/rpc/version", |_body| {
        Box::pin(async {
            Ok((
                200,
                json!({
                    "sidecar": env!("CARGO_PKG_VERSION"),
                    "min_compatible": "0.4.0",
                    "build": "rust",
                }),
            ))
        })
    });

    // List providers — v0.4 reads the 9 built-in presets from
    // `providers::PRESETS` and joins them with the `provider`
    // table for per-preset overrides (enabled, default_model,
    // base_url) plus a `has_secret` flag from the secret store.
    // The result also includes any custom_provider rows.
    let s2 = state.clone();
    d.register("GET", "/api/providers", move |body| {
        let s = s2.clone();
        Box::pin(async move { list_providers(body, s).await })
    });

    // Run a workflow / task envelope. This is the central
    // entry point that the Tauri client will use to talk to
    // the agent loop. For now we require the caller to pass
    // a fully-formed provider spec — the router (W3 follow-up)
    // will resolve names to providers.
    let s3 = state.clone();
    d.register("POST", "/api/run_task", move |body| {
        let state = s3.clone();
        Box::pin(async move { run_task(body, state).await })
    });

    // ── Stub handlers for endpoints the Tauri shell calls but
    // the v0.2.5-era Python server implemented. These are
    // best-effort placeholders that return a shape the UI
    // already expects; full implementations land as the
    // corresponding Rust modules come online (v0.4+).
    //
    // We keep them in this file rather than in a separate
    // handlers_i_ching module because they belong to a
    // different domain (provider / router / secret store /
    // plugin registry) and have no shared logic to factor out.
    // ── KV store (Phase 4 onboarding) ─────────────────────────
    // GET /api/kv/{key}   — returns the JSON value or null
    let kv_get_state = state.clone();
    d.register("GET", "/api/kv/{key}", move |body| {
        let s = kv_get_state.clone();
        Box::pin(async move {
            let key = body.get("key").and_then(|v| v.as_str())
                .ok_or_else(|| "missing 'key'".to_string())?.to_string();
            match s.repo.kv_get(&key).await {
                Ok(Some(v)) => match serde_json::from_str::<Value>(&v) {
                    Ok(parsed) => Ok((200, json!({ "k": key, "v": parsed }))),
                    Err(_) => Ok((200, json!({ "k": key, "v": Value::String(v) }))),
                },
                Ok(None) => Ok((200, json!({ "k": key, "v": Value::Null }))),
                Err(e) => Ok((500, json!({ "error": format!("kv_get: {e}") }))),
            }
        })
    });

    // POST /api/kv/{key}   — body { value: <json> }
    let kv_set_state = state.clone();
    d.register("POST", "/api/kv/{key}", move |body| {
        let s = kv_set_state.clone();
        Box::pin(async move {
            let key = body.get("key").and_then(|v| v.as_str())
                .ok_or_else(|| "missing 'key'".to_string())?.to_string();
            let value = body.get("value").cloned().unwrap_or(Value::Null);
            let value_str = serde_json::to_string(&value)
                .map_err(|e| format!("serialize: {e}"))?;
            if let Err(e) = s.repo.kv_set(&key, &value_str).await {
                return Ok((500, json!({ "error": format!("kv_set: {e}") })));
            }
            Ok((200, json!({ "k": key, "v": value })))
        })
    });

    // POST /api/kv/first_run/complete  — convenience that writes
    // first_run=false in one call.
    let kv_first_run_complete_state = state.clone();
    d.register("POST", "/api/kv/first_run/complete", move |_body| {
        let s = kv_first_run_complete_state.clone();
        Box::pin(async move {
            if let Err(e) = s.repo.kv_set("first_run", "false").await {
                return Ok((500, json!({ "error": format!("kv_set: {e}") })));
            }
            Ok((200, json!({ "first_run": false })))
        })
    });

    // ── Sample workflows (Phase 4 onboarding) ──────────────────
    // GET /api/sample/{name}  — returns a serialized WorkflowRun
    // envelope. The frontend can submit it via run_agent_task.
    d.register("GET", "/api/sample/{name}", |body| {
        let name = body.get("name").and_then(|v| v.as_str())
            .unwrap_or("auth_login").to_string();
        Box::pin(async move { Ok((200, sample_workflow(&name))) })
    });

    register_placeholder_handlers(d, state.clone());
}

fn register_placeholder_handlers(d: &mut Dispatcher, state: Arc<ServerState>) {
    // ── Secrets: v0.4 real persistence (OS keystore + AES-GCM).
    // The Tauri shell writes a plaintext via PUT
    // /api/settings/secrets/{name}; the pipe server encrypts
    // with the OS keystore DEK and stores ciphertext + nonce
    // in SQLite. The plaintext is never returned to the UI
    // (only metadata). The agent loop calls GET
    // /api/settings/secrets/{name}/reveal to fetch the plaintext
    // at runtime.
    let list_state = state.clone();
    d.register("GET", "/api/settings/secrets", move |_body| {
        let s = list_state.clone();
        Box::pin(async move {
            match s.secrets.list().await {
                Ok(list) => Ok((200, json!({ "secrets": list, "count": list.len() }))),
                Err(e) => Ok((500, json!({ "error": format!("list_secrets: {e}") }))),
            }
        })
    });

    let put_state = state.clone();
    d.register("PUT", "/api/settings/secrets/{name}", move |body| {
        let s = put_state.clone();
        Box::pin(async move {
            let name = body.get("name").and_then(|v| v.as_str())
                .ok_or_else(|| "missing 'name' in path".to_string())?.to_string();
            let value = body.get("value").and_then(|v| v.as_str())
                .ok_or_else(|| "missing 'value' in body".to_string())?.to_string();
            if value.len() > 4096 {
                return Ok((400, json!({ "error": "secret value exceeds 4 KiB cap" })));
            }
            if name.is_empty() || name.len() > 128 {
                return Ok((400, json!({ "error": "secret name must be 1..=128 chars" })));
            }
            match s.secrets.put(&name, &value).await {
                Ok(()) => Ok((200, json!({ "saved": true, "name": name }))),
                Err(e) => Ok((500, json!({ "error": format!("put_secret: {e}") }))),
            }
        })
    });

    let del_state = state.clone();
    d.register("DELETE", "/api/settings/secrets/{name}", move |body| {
        let s = del_state.clone();
        Box::pin(async move {
            let name = body.get("name").and_then(|v| v.as_str())
                .ok_or_else(|| "missing 'name' in path".to_string())?.to_string();
            match s.secrets.delete(&name).await {
                Ok(removed) => Ok((200, json!({ "deleted": removed, "name": name }))),
                Err(e) => Ok((500, json!({ "error": format!("delete_secret: {e}") }))),
            }
        })
    });

    // Internal-only: returns plaintext. The Tauri shell's IPC
    // bridge should NOT expose this to the webview. Used by
    // the agent loop when making provider requests.
    let reveal_state = state.clone();
    d.register("GET", "/api/settings/secrets/{name}/reveal", move |body| {
        let s = reveal_state.clone();
        Box::pin(async move {
            let name = body.get("name").and_then(|v| v.as_str())
                .ok_or_else(|| "missing 'name' in path".to_string())?.to_string();
            match s.secrets.reveal(&name).await {
                Ok(value) => Ok((200, json!({ "name": name, "value": value.as_str() }))),
                Err(crate::secrets::SecretStoreError::NotFound(_)) => {
                    Ok((404, json!({ "error": "not found", "name": name })))
                }
                Err(e) => Ok((500, json!({ "error": format!("reveal: {e}") }))),
            }
        })
    });

    // v0.4: trigger the legacy plaintext migration if a
    // v0.3-era secrets.json exists at <data_dir>/secrets.json.
    let seed_state = state.clone();
    d.register("POST", "/api/settings/secrets/seed", move |_body| {
        let s = seed_state.clone();
        Box::pin(async move {
            let legacy = s.secrets.data_dir().join("secrets.json");
            match s.secrets.migrate_legacy_plaintext(&legacy).await {
                Ok(n) => Ok((200, json!({
                    "seeded": n,
                    "source": legacy.display().to_string(),
                }))),
                Err(e) => Ok((500, json!({ "error": format!("seed: {e}") }))),
            }
        })
    });

    // Router: per-role default-model + fallback chain. v0.4 fixes
    // v0.4.16 (event 000052): chairman rejected the v0.4.15 hard-coded
    // defaults of "anthropic:claude-opus-4-8" + ["anthropic:claude-sonnet-4-6"].
    // Every role now starts empty — the user picks what they actually have.
    d.register("GET", "/api/router/roles", |_body| {
        Box::pin(async {
            Ok((
                200,
                json!({
                    "roles": [
                        { "role": "agent:chief",    "default_model": "", "fallback_chain": [] },
                        { "role": "agent:worker",   "default_model": "", "fallback_chain": [] },
                        { "role": "agent:planner",  "default_model": "", "fallback_chain": [] },
                        { "role": "agent:critic:a", "default_model": "", "fallback_chain": [] },
                        { "role": "agent:critic:b", "default_model": "", "fallback_chain": [] },
                        { "role": "agent:reporter", "default_model": "", "fallback_chain": [] },
                    ],
                }),
            ))
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
            let providers = s.repo.list_providers().await
                .unwrap_or_default();
            let custom = s.repo.list_custom_providers().await
                .unwrap_or_default();
            let mut by_id = std::collections::HashMap::new();
            for p in providers {
                if let Ok(Some(c)) = s.repo.get_model_cache(&p.id).await {
                    by_id.insert(p.id.clone(), c);
                }
            }
            for c in custom {
                if let Ok(Some(cache)) = s.repo.get_model_cache(&c.id).await {
                    by_id.insert(c.id.clone(), cache);
                }
            }
            let models: Vec<Value> = by_id.iter()
                .filter_map(|(id, cache)| {
                    serde_json::from_str::<Vec<Value>>(&cache.models_json).ok()
                        .map(|m| json!({ "provider_id": id, "models": m }))
                })
                .collect();
            Ok((200, json!({
                "models": models,
                "count": models.len(),
            })))
        })
    });

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
    d.register("PUT", "/api/router/roles", |body| {
        // PUT (update) variant — distinguished from GET above by
        // the dispatcher key. We register on the same path so the
        // last write wins; the GET handler above never sees the PUT
        // body because Dispatcher only routes by method+path.
        let _ = body;
        Box::pin(async {
            Ok((
                200,
                json!({
                    "ok": true,
                    "note": "router role update accepted (no-op stub); v0.5 will persist the role -> model mapping via a new role_model_override table.",
                }),
            ))
        })
    });

    // Plugin registry. v0.3 ships zero user-loadable plugins in
    // the sidecar; the Tauri shell's plugin panel will render
    // an empty list, which matches the spec.
    d.register("GET", "/api/plugins", |_body| {
        Box::pin(async {
            Ok((
                200,
                json!({
                    "plugins": [],
                    "note": "no plugins registered in v0.3; the Tauri shell's plugin panel is empty by design.",
                }),
            ))
        })
    });
    d.register("POST", "/api/plugins/{name}/invoke", |body| {
        let _ = body;
        Box::pin(async {
            Ok((
                501,
                json!({
                    "error": "plugin invocation not yet implemented; no plugins are registered in v0.3.",
                }),
            ))
        })
    });

    // I Ching oracle (64-gua divination). Implements the full
    // King Wen sequence. The data set is a 12 KB JSON file
    // baked into the binary; the draw path uses
    // `rand::random::<u64>()` for uniform selection across the
    // 64 hexagrams.
    d.register("GET", "/api/i_ching/draw", |_body| {
        Box::pin(async {
            match crate::i_ching::draw_hexagram() {
                Ok(hex) => Ok((
                    200,
                    json!({
                        "draw": {
                            "id": hex.id,
                            "name_zh": hex.name_zh,
                            "name_pinyin": hex.name_pinyin,
                            "name_en": hex.name_en,
                            "binary": hex.binary,
                            "lines": hex.lines().iter().map(|l| json!({
                                "position": l.position,
                                "kind": match l.kind {
                                    crate::i_ching::LineKind::Yang => "yang",
                                    crate::i_ching::LineKind::Yin  => "yin",
                                },
                                "glyph": l.glyph,
                            })).collect::<Vec<_>>(),
                            "judgment": hex.judgment,
                            "image": hex.image,
                        },
                        "drawn_at_ms": std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0),
                    }),
                )),
                Err(e) => Ok((500, json!({ "error": e }))),
            }
        })
    });
    d.register("GET", "/api/i_ching/list", |_body| {
        Box::pin(async {
            match crate::i_ching::all_hexagrams() {
                Ok(hexes) => Ok((
                    200,
                    json!({
                        "list": hexes.iter().map(|h| json!({
                            "id": h.id,
                            "name_zh": h.name_zh,
                            "name_pinyin": h.name_pinyin,
                            "name_en": h.name_en,
                            "binary": h.binary,
                        })).collect::<Vec<_>>(),
                        "count": hexes.len(),
                    }),
                )),
                Err(e) => Ok((500, json!({ "error": e }))),
            }
        })
    });

    // Workflow control. The end-to-end workflow loop is not
    // yet wired up; start_workflow_cmd is exposed but the
    // actual run is gated on v0.4 work.
    d.register("POST", "/api/workflow/{id}/cancel", |_body| {
        Box::pin(async {
            Ok((
                200,
                json!({
                    "ok": true,
                    "note": "no active workflow to cancel; v0.4 will route this through the in-process agent loop.",
                }),
            ))
        })
    });
}

// ── v0.4 provider endpoints ───────────────────────────────────

/// GET /api/providers — list built-in presets + custom providers,
/// joined with the per-preset `provider` row for overrides and
/// `secret` table for `has_secret`.
async fn list_providers(
    _body: Value,
    state: Arc<ServerState>,
) -> Result<(u16, Value), String> {
    // Pull all rows once.
    let preset_rows = state.repo.list_providers().await
        .map_err(|e| format!("list_providers: {e}"))?;
    let secret_rows = state.secrets.list().await
        .map_err(|e| format!("list_secrets: {e}"))?;
    let custom_rows = state.repo.list_custom_providers().await
        .map_err(|e| format!("list_custom: {e}"))?;

    // Index overrides + secrets by id.
    let override_by_id: std::collections::HashMap<&str, &storage::ProviderRow> =
        preset_rows.iter().map(|p| (p.id.as_str(), p)).collect();
    let secret_names: std::collections::HashSet<&str> =
        secret_rows.iter().map(|s| s.name.as_str()).collect();

    let presets: Vec<Value> = crate::providers::PRESETS.iter().map(|p| {
        let ovr = override_by_id.get(p.id);
        let enabled = ovr.map(|r| r.enabled).unwrap_or(true);
        let default_model = ovr.and_then(|r| r.default_model.clone())
            .unwrap_or_else(|| p.default_model.to_string());
        let base_url = ovr.and_then(|r| r.base_url.clone())
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
    }).collect();

    let custom: Vec<Value> = custom_rows.iter().map(|c| {
        let custom_secret = format!("CUSTOM_PROVIDER_KEY_{}", c.id);
        json!({
            "id": c.id,
            "kind": "custom",
            "display_name": c.name,
            "api_kind": c.kind,
            "base_url": c.base_url,
            "default_model": c.default_model,
            "secret_name": custom_secret,
            "has_secret": secret_names.contains(custom_secret.as_str()),
            "enabled": c.enabled,
            "note": null,
            "has_live_models_endpoint": c.kind == "openai-compatible",
        })
    }).collect();

    Ok((200, json!({
        "providers": presets,
        "custom_providers": custom,
        "count": presets.len() + custom.len(),
    })))
}

/// PATCH /api/providers/{id} — toggle enabled, override
/// default_model, override base_url. Reads from body:
///   { enabled?: bool, default_model?: string|null,
///     base_url?: string|null }
async fn patch_provider(
    body: Value,
    state: Arc<ServerState>,
) -> Result<(u16, Value), String> {
    let id = body.get("id").and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'id' in path".to_string())?.to_string();

    // Built-in presets: validate id + upsert into the `provider`
    // table (which was pre-populated by migration 0003).
    if crate::providers::get(&id).is_none() {
        return Ok((404, json!({ "error": format!("unknown provider: {id}") })));
    }

    // Load current row, merge with patch fields.
    let mut row = state.repo.get_provider(&id).await
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
    state.repo.upsert_provider(&row).await
        .map_err(|e| format!("upsert_provider: {e}"))?;

    Ok((200, json!({
        "id": id,
        "updated": changed,
        "enabled": row.enabled,
        "default_model": row.default_model,
        "base_url": row.base_url,
    })))
}

/// GET /api/providers/{id}/models — fetch available models.
/// Cached for 1 hour per provider id.
async fn list_models(
    body: Value,
    state: Arc<ServerState>,
) -> Result<(u16, Value), String> {
    let id = body.get("id").and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'id' in path".to_string())?.to_string();

    // Check cache first.
    if let Ok(Some(cached)) = state.repo.get_model_cache(&id).await {
        let now = chrono::Utc::now().timestamp();
        if now - cached.fetched_at < 3600 {
            let models: Vec<Value> = serde_json::from_str(&cached.models_json)
                .unwrap_or_else(|_| Vec::new());
            return Ok((200, json!({
                "provider_id": id,
                "models": models,
                "cached": true,
                "fetched_at": cached.fetched_at,
            })));
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
        let custom = match state.repo.get_custom_provider(&id).await
            .map_err(|e| format!("get_custom: {e}"))? {
            Some(c) => c,
            None => return Ok((404, json!({ "error": format!("unknown provider: {id}") }))),
        };
        (custom.base_url.clone(), custom.kind == "openai-compatible")
    };

    // Anthropic has no /v1/models endpoint — return the hard-coded
    // fallback list directly.
    if !has_live {
        // v0.4.16: prefer the per-provider OPENAI_FALLBACK_MODELS
        // entry if one exists, then fall back to Anthropic's.
        let entries: &[crate::providers::ModelEntry] =
            crate::providers::OPENAI_FALLBACK_MODELS.iter()
                .find(|(pid, _)| *pid == id)
                .map(|(_, m)| *m)
                .unwrap_or(crate::providers::ANTHROPIC_FALLBACK_MODELS);
        let models: Vec<Value> = entries.iter()
            .map(|m| json!({
                "id": m.id,
                "display_name": m.display_name,
                "thinking_strength": m.thinking_strength,
                "context_length": m.context_length,
                "source": "fallback",
            }))
            .collect();
        let body_str = serde_json::to_string(&models).unwrap();
        let _ = state.repo.put_model_cache(&storage::ModelCacheRow {
            provider_id: id.clone(),
            models_json: body_str,
            fetched_at: chrono::Utc::now().timestamp(),
        }).await;
        return Ok((200, json!({
            "provider_id": id,
            "models": models,
            "cached": false,
            "fallback": true,
        })));
    }

    // OpenAI-compatible fetch.
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let secret_name = if crate::providers::get(&id).is_some() {
        crate::providers::get(&id).unwrap().secret_name.to_string()
    } else {
        format!("CUSTOM_PROVIDER_KEY_{id}")
    };
    let api_key = match state.secrets.reveal(&secret_name).await {
        Ok(k) => k,
        Err(crate::secrets::SecretStoreError::NotFound(_)) => {
            return Ok((401, json!({
                "error": "no API key configured",
                "secret_name": secret_name,
            })));
        }
        Err(e) => return Ok((500, json!({ "error": format!("reveal: {e}") }))),
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("reqwest build: {e}"))?;
    let resp = client.get(&url)
        .header("Authorization", format!("Bearer {}", api_key.as_str()))
        .send()
        .await
        .map_err(|e| format!("GET {url}: {e}"))?;
    let status = resp.status();
if !status.is_success() {
            return Ok((status.as_u16(), json!({
                "error": format!("provider returned {status}"),
                "url": url,
                "provider_id": id,
            })));
        }
        let body: Value = resp.json().await
            .map_err(|e| format!("parse {url}: {e}"))?;
    // OpenAI-compatible /models response shape:
    // { "object": "list", "data": [{ id, object, ... }, ...] }
    let models: Vec<Value> = body.get("data")
        .and_then(|d| d.as_array())
        .map(|arr| arr.iter().filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(|id| json!({
            "id": id,
            "display_name": id,
            "source": "live",
        }))).collect())
        .unwrap_or_default();
    let body_str = serde_json::to_string(&models).unwrap_or_default();
    let _ = state.repo.put_model_cache(&storage::ModelCacheRow {
        provider_id: id.clone(),
        models_json: body_str,
        fetched_at: chrono::Utc::now().timestamp(),
    }).await;
Ok((200, json!({
            "provider_id": id,
            "models": models,
            "cached": false,
            "fallback": false,
            "url": url,
        })))
    }

/// POST /api/providers/custom — add a relay-station / private-gateway
/// provider. The api_key lives in the encrypted secret store under
/// `CUSTOM_PROVIDER_KEY_<id>`.
async fn add_custom_provider(
    body: Value,
    state: Arc<ServerState>,
) -> Result<(u16, Value), String> {
    let name = body.get("name").and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'name'".to_string())?.to_string();
    let base_url = body.get("base_url").and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'base_url'".to_string())?.to_string();
    let kind = body.get("kind").and_then(|v| v.as_str())
        .unwrap_or("openai-compatible").to_string();
    let default_model = body.get("default_model")
        .and_then(|v| v.as_str()).map(|s| s.to_string());

    if name.is_empty() || name.len() > 64 {
        return Ok((400, json!({ "error": "name must be 1..=64 chars" })));
    }
    if !base_url.starts_with("https://") && !base_url.starts_with("http://") {
        return Ok((400, json!({ "error": "base_url must start with http(s)://" })));
    }
    if kind != "openai-compatible" && kind != "anthropic-compatible" {
        return Ok((400, json!({ "error": "kind must be openai-compatible or anthropic-compatible" })));
    }

    let id = ulid::Ulid::new().to_string();
    let now = chrono::Utc::now().timestamp();
    let row = storage::CustomProvider {
        id: id.clone(),
        name: name.clone(),
        base_url: base_url.clone(),
        kind: kind.clone(),
        default_model: default_model.clone(),
        enabled: true,
        created_at: now,
        updated_at: now,
    };
    state.repo.insert_custom_provider(&row).await
        .map_err(|e| format!("insert_custom: {e}"))?;

    // If an api_key was supplied in the same POST, encrypt and
    // store it under CUSTOM_PROVIDER_KEY_<id>.
    if let Some(key) = body.get("api_key").and_then(|v| v.as_str()) {
        let secret_name = format!("CUSTOM_PROVIDER_KEY_{id}");
        state.secrets.put(&secret_name, key).await
            .map_err(|e| format!("put secret: {e}"))?;
    }

    Ok((201, json!({
        "id": id,
        "name": name,
        "base_url": base_url,
        "kind": kind,
        "default_model": default_model,
        "enabled": true,
    })))
}

/// DELETE /api/providers/custom/{id} — remove a custom provider
/// and any associated secret.
async fn delete_custom_provider(
    body: Value,
    state: Arc<ServerState>,
) -> Result<(u16, Value), String> {
    let id = body.get("id").and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'id' in path".to_string())?.to_string();

    let removed = state.repo.delete_custom_provider(&id).await
        .map_err(|e| format!("delete_custom: {e}"))?;
    // Best-effort: clean up the associated api_key secret.
    let secret_name = format!("CUSTOM_PROVIDER_KEY_{id}");
    let _ = state.secrets.delete(&secret_name).await;
    Ok((200, json!({
        "id": id,
        "deleted": removed,
    })))
}

/// Walk every provider's model_cache; if any entry is older than
/// 1 hour, refresh it in the background. Non-blocking — returns
/// the number of caches that were stale (and therefore refreshed).
async fn refresh_stale_caches(state: &Arc<ServerState>) -> usize {
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
            let cached = state.repo.get_model_cache(preset.id).await
                .ok().flatten();
            if cached.is_none() {
                let entries: &[crate::providers::ModelEntry] =
                    crate::providers::OPENAI_FALLBACK_MODELS.iter()
                        .find(|(pid, _)| *pid == preset.id)
                        .map(|(_, m)| *m)
                        .unwrap_or(crate::providers::ANTHROPIC_FALLBACK_MODELS);
                let models: Vec<Value> = entries.iter()
                    .map(|m| json!({
                        "id": m.id,
                        "display_name": m.display_name,
                        "thinking_strength": m.thinking_strength,
                        "context_length": m.context_length,
                        "source": "fallback",
                    }))
                    .collect();
                let body = serde_json::to_string(&models).unwrap_or_default();
                let _ = state.repo.put_model_cache(&storage::ModelCacheRow {
                    provider_id: preset.id.to_string(),
                    models_json: body,
                    fetched_at: now,
                }).await;
                stale += 1;
            }
            continue;
        }
        let cached = state.repo.get_model_cache(preset.id).await
            .ok().flatten();
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
                if let Ok(resp) = client.get(&url)
                    .header("Authorization", format!("Bearer {}", api_key.as_str()))
                    .send().await
                {
                    if resp.status().is_success() {
                        if let Ok(body) = resp.json::<Value>().await {
                            let models: Vec<Value> = body.get("data")
                                .and_then(|d| d.as_array())
                                .map(|arr| arr.iter().filter_map(|m|
                                    m.get("id").and_then(|v| v.as_str()).map(|id| json!({
                                        "id": id, "display_name": id,
                                        "source": "live",
                                    }))
                                ).collect())
                                .unwrap_or_default();
                            let body_str = serde_json::to_string(&models).unwrap_or_default();
                            let _ = state.repo.put_model_cache(&storage::ModelCacheRow {
                                provider_id: preset.id.to_string(),
                                models_json: body_str,
                                fetched_at: now,
                            }).await;
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
        let cached = state.repo.get_model_cache(&c.id).await
            .ok().flatten();
        if cached.is_none() {
            stale += 1;
        }
    }

    stale
}

/// Return a hardcoded sample workflow envelope. The frontend
/// posts this to run_agent_task to start a workflow.
///
/// Currently only one sample: 'auth_login' — a small Flask-style
/// "implement POST /auth/login endpoint" task that exercises
/// the full plan-then-execute flow.
fn sample_workflow(name: &str) -> Value {
    match name {
        "auth_login" => json!({
            "name": "auth_login",
            "display_name": "示例任务 - 实现 POST /auth/login",
            "description":
                "通过一个完整的工作流示例展示 Flowntier 的工作方式: \
                 首席 Agent 拆解任务, 规划 Agent 出方案, 工匠 Agent 写代码, \
                 缺陷猎手 和 质检师审核, 最后汇报.",
"user_request": concat!(
                "实现 POST /auth/login 接口. 要求: ",
                "1. 接收 JSON ", "{ username, password }", " ",
                "2. 校验非空、长度 >= 3 ",
                "3. 与内存中的用户表比对 ",
                "4. 成功返回 ", "{ token: <random_hex_32>, expires_in: 3600 }", " ",
                "5. 失败返回 401 ",
                "6. 写至少 4 个测试用例 (valid, missing fields, wrong password, unknown user)"
            ),
            "expected_tasks": [
                "分析需求 (chief)",
                "设计方案 (planner)",
                "实现代码 (worker)",
                "测试 + 自检 (worker)",
                "代码审查 (critic)",
                "汇报 (reporter)",
            ],
        }),
        _ => json!({
            "error": format!("unknown sample: {name}"),
            "known_samples": ["auth_login"],
        }),
    }
}

async fn run_task(body: Value, state: Arc<ServerState>) -> Result<(u16, Value), String> {
    // ── Parse request ─────────────────────────────────────────
    let task_text = body
        .get("task")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'task'".to_string())?;
    let provider_kind = body
        .get("provider_kind")
        .and_then(|v| v.as_str())
        .unwrap_or("openai_compat");
    let base_url = body
        .get("base_url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'base_url'".to_string())?;
    let base_url = agent_core::provider::openai::validate_base_url(base_url)
        .map_err(|e| format!("invalid base_url: {e}"))?;
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'model'".to_string())?
        .to_string();
    // v0.4.12 (event 000048): the api_key_env fallback is
    // removed. The Tauri shell now resolves the key from the OS
    // credential store via reveal_secret() and passes the
    // plaintext in body.api_key. We never read process env vars
    // for credentials — this prevents the key from leaking via
    // /proc/<pid>/environ, task manager, or shell history.
    let api_key = body
        .get("api_key")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing or empty 'api_key' (no env-var fallback in v0.4.12)".to_string())?
        .to_string();
    let role = body
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("agent:worker");
    let wf_id = body
        .get("wf_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // ── Build provider ────────────────────────────────────────
    let provider: Arc<dyn agent_core::Provider> = match provider_kind {
        "openai" => Arc::new(OpenAiProvider::openai(model, api_key)),
        "openai_compat" => Arc::new(OpenAiProvider::compat(base_url, model, api_key)),
        other => return Err(format!("unsupported provider_kind: {other}")),
    };

    // ── Build agent ───────────────────────────────────────────
    let role_enum = match role {
        "agent:chief" => agent_core::prompt::Role::Chief,
        "agent:critic:a" => agent_core::prompt::Role::BugHunter,
        "agent:critic:b" => agent_core::prompt::Role::Reviewer,
        "agent:planner" => agent_core::prompt::Role::Planner,
        "agent:reporter" => agent_core::prompt::Role::Reporter,
        _ => agent_core::prompt::Role::Worker,
    };
    let agent = Agent::new(
        role_enum,
        provider,
        state.tools.clone(),
        state.workspace.clone(),
        AgentConfig::default(),
    );

    // ── Stream events to subscribers while running ────────────
    let mut rx = agent.run(task_text);
    let mut last_status = "UNKNOWN".to_string();
    let mut summary: Option<String> = None;
    while let Some(ev) = rx.recv().await {
        // Best-effort fan-out; if no subscribers, that's fine.
        let _ = state.events.send(ev.clone());
        if let AgentEvent::Done { status, summary: s, .. } = ev {
            last_status = status;
            summary = s;
        }
        if matches!(last_status.as_str(), "DONE" | "FAILED" | "ABORTED" | "ABORTED_REPEAT") {
            // If the wf_id was provided, replace the empty one.
            if !wf_id.is_empty() {
                last_status = format!("{last_status} (wf={wf_id})");
            }
            break;
        }
    }

    Ok((
        200,
        json!({
            "ok": true,
            "status": last_status,
            "summary": summary,
        }),
    ))
}