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
use tracing::warn;
use zeroize::Zeroizing;

use crate::dispatcher::Dispatcher;
use crate::secrets::SecretStore;

/// v0.4.21 (event 000066): one error surfaced by the pipe-server
/// to the desktop TopBar red-dot badge. Kept in-memory only —
/// persistence belongs in `quota_failures` (provider-level) or
/// in the user's project log (`workflow_log`). This struct is
/// the transient, "something interesting just happened, the
/// chairman should know" channel.
///
/// `severity` is one of: `error`, `warn`, `info`. `source`
/// identifies which subsystem emitted it (`run_task`,
/// `workspace_swap`, `quota`, `agent_loop`, `events_pipe`,
/// `init`). `detail` is a free-text payload — usually the
/// error message itself.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ErrorRecord {
    pub at: i64,
    pub severity: String,
    pub source: String,
    pub summary: String,
    pub detail: Option<String>,
}

/// Shared state held by the pipe server.
#[derive(Clone)]
pub struct ServerState {
    /// Event bus that all event-pipe clients subscribe to.
    pub events: broadcast::Sender<AgentEvent>,
    /// Default tool registry.
    pub tools: Arc<ToolRegistry>,
    /// v0.4.21 (event 000066): in-memory ring buffer of the
    /// 200 most-recent error records. Surfaces to the desktop
    /// TopBar via `GET /api/errors/recent`. Avoids hammering
    /// SQLite for transient errors that don't deserve a
    /// persisted row but DO deserve user attention.
    /// `Arc<Mutex<…>>` so the ServerState `#[derive(Clone)]`
    /// keeps working (Mutex isn't Clone on its own).
    pub errors: Arc<std::sync::Mutex<std::collections::VecDeque<ErrorRecord>>>,
    /// v0.4.21 (event 000066): workspace root for the current
    /// pipe-server run. Wrapped in `Arc<RwLock<…>>` so the
    /// `POST /api/workspace/set` handler can swap it mid-process
    /// when the chairman changes the workdir via the desktop UI
    /// (`About > Change workdir`). Prior to this the workspace
    /// was the runtime's launch-time cwd — whatever it was when
    /// flowntier-runtime.exe started — so a chief agent writing
    /// files always landed them under `O:\Flowntier\workspace\…`
    /// regardless of what workdir.json said. Event 000066
    /// fixes that by routing `set_workdir_with_nwt` through a
    /// new pipe-server route that updates this field in place.
    pub workspace: Arc<std::sync::RwLock<Workspace>>,
    /// v0.4: persistent secret store (OS keystore + AES-GCM).
    pub secrets: Arc<SecretStore>,
    /// v0.4: SQLite repository for provider / custom_provider /
    /// kv tables.
    pub repo: Arc<storage::Repository>,
    /// v0.4.20 (event 000056, Phase-2): dispatcher handle so the
    /// quota scheduler can dispatch internal retry requests
    /// (`POST /api/run_task`). Populated by `register_all` after
    /// all handlers are wired up. `Arc<Mutex<...>>` (not bare
    /// `Mutex`) so `#[derive(Clone)]` on ServerState keeps
    /// working.
    dispatcher: Arc<std::sync::Mutex<Option<Arc<Dispatcher>>>>,
    /// v0.4.22 (event 000091 fix #32): active workflows map
    /// keyed by `wf_id`. The orchestrator's cancel token is
    /// stored here when the workflow starts; the cancel route
    /// reads it to fire cancellation, then removes the
    /// entry on natural completion. Without this, the
    /// `cancel_workflow` Tauri command was a no-op stub and
    /// the chairman had no way to interrupt a runaway
    /// 30-minute workflow.
    active_workflows: Arc<std::sync::Mutex<std::collections::HashMap<String, tokio_util::sync::CancellationToken>>>,
}

impl ServerState {
    /// New default state. Opens the SQLite repo at
    /// `<data_dir>/storage.sqlite` and constructs a SecretStore
    /// bound to the same data dir.
    pub async fn new(
        workspace_root: std::path::PathBuf,
        data_dir: std::path::PathBuf,
    ) -> Self {
        let (events, _rx) = broadcast::channel(8192);

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
            workspace: Arc::new(std::sync::RwLock::new(
                Workspace::new(workspace_root, "flowntier"),
            )),
            errors: Arc::new(std::sync::Mutex::new(
                std::collections::VecDeque::with_capacity(200),
            )),
            secrets,
            repo,
            dispatcher: Arc::new(std::sync::Mutex::new(None)),
            active_workflows: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// v0.4.21 (event 000066): swap the workspace root at runtime.
    /// Called by `POST /api/workspace/set`. In-flight agent runs
    /// that already cloned the workspace via `state.workspace.read()`
    /// will keep their old root (that's the agent-loop contract)
    /// — only subsequent reads pick up the new path. The Tauri
    /// shell ensures this is called *before* the next
    /// `POST /api/run_task` lands, so in practice the swap is
    /// observed by every new task.
    pub fn set_workspace(&self, root: std::path::PathBuf) {
        let mut g = self
            .workspace
            .write()
            .expect("workspace rwlock poisoned");
        *g = Workspace::new(root, "flowntier");
        tracing::info!(
            target: "pipe_server",
            "v0.4.21 (event 000066): workspace swapped to {}",
            g.root.display()
        );
    }

    /// v0.4.21 (event 000066): snapshot the current workspace
    /// root. Cheap (just a clone of the Arc), used by handlers
    /// that need a stable Workspace reference for the duration
    /// of a single request (e.g. run_task builds an Agent with
    /// this snapshot).
    pub fn workspace_snapshot(&self) -> Workspace {
        self.workspace
            .read()
            .expect("workspace rwlock poisoned")
            .clone()
    }

    /// v0.4.21 (event 000066): push an error record onto the
    /// in-memory ring buffer. Returns immediately; never blocks.
    /// Called from key failure sites: `run_task` timeout,
    /// workspace swap rejection, quota-recording failures, and
    /// the pipe pipe-error reporter. The Tauri shell polls
    /// `GET /api/errors/recent` and lights up the TopBar red
    /// badge when count > 0.
    pub fn push_error(&self, rec: ErrorRecord) {
        if let Ok(mut g) = self.errors.lock() {
            if g.len() >= 200 { g.pop_front(); }
            g.push_back(rec);
        }
    }

    /// Snapshot the most-recent N error records (newest first).
    pub fn recent_errors(&self, n: usize) -> Vec<ErrorRecord> {
        let g = self.errors.lock().expect("errors mutex poisoned");
        g.iter().rev().take(n).cloned().collect()
    }

    /// v0.4.20: install the dispatcher once `register_all` has
    /// finished populating it.
    pub fn set_dispatcher(&self, d: Arc<Dispatcher>) {
        let mut g = self.dispatcher.lock().expect("dispatcher mutex poisoned");
        *g = Some(d);
    }

    /// v0.4.20: clone the dispatcher handle. Used by the quota
    /// scheduler (Phase-2) to dispatch internal retry requests.
    pub fn dispatcher(&self) -> Option<Arc<Dispatcher>> {
        self.dispatcher.lock().expect("dispatcher mutex poisoned").clone()
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

    // ── v0.4.20: GET /api/quota/status ────────────────────────
    // Returns all quota_failures rows. Frontend (Settings →
    // 角色额度状态) renders the union.
    let s_qstatus = state.clone();
    d.register("GET", "/api/quota/status", move |_body| {
        let s = s_qstatus.clone();
        Box::pin(async move {
            let rows = match s.repo.list_all_quota_failures().await {
                Ok(r) => r,
                Err(e) => return Ok((500, json!({ "ok": false, "error": format!("list: {e}") }))),
            };
            let items: Vec<Value> = rows.into_iter().map(|r| json!({
                "role_id": r.role_id,
                "model_id": r.model_id,
                "last_error_at": r.last_error_at,
                "last_error_message": r.last_error_message,
                "status": r.status,
                "attempt_count": r.attempt_count,
                "next_attempt_at": r.next_attempt_at,
            })).collect();
            Ok((200, json!({ "ok": true, "rows": items })))
        })
    });

    // ── v0.4.20: POST /api/quota/reset ─────────────────────
    // Body: { role: "agent:chief", model_id?: "minimax:MiniMax-Text-01" }
    // model_id absent = clear every row for that role (chief's
    // "forget history" button).
    let s_qreset = state.clone();
    d.register("POST", "/api/quota/reset", move |body| {
        let s = s_qreset.clone();
        Box::pin(async move {
            let Some(role) = body.get("role").and_then(|v| v.as_str()) else {
                return Ok((400, json!({ "ok": false, "error": "missing 'role'" })));
            };
            let model_id = body.get("model_id").and_then(|v| v.as_str());
            match s.repo.clear_quota_failure(role, model_id).await {
                Ok(n) => Ok((200, json!({ "ok": true, "cleared_rows": n }))),
                Err(e) => Ok((500, json!({ "ok": false, "error": format!("reset: {e}") }))),
            }
        })
    });

    // v0.4.21 (event 000064): GET /api/tasks?wf_id=... returns
    // the persisted task list for a workflow so the dashboard's
    // "任务列表 0/0 完成" panel can show real progress. Before
    // this fix the tasks table was always empty — the run_task
    // handler wrote AgentEvents but never INSERT INTO tasks.
    let s_tasks = state.clone();
    d.register("GET", "/api/tasks", move |body| {
        let s = s_tasks.clone();
        let wf_id = body.get("wf_id").and_then(|v| v.as_str())
            .unwrap_or("").to_string();
        Box::pin(async move {
            if wf_id.is_empty() {
                return Ok((400, json!({
                    "ok": false,
                    "error": "missing 'wf_id'",
                })));
            }
            match s.repo.list_tasks_for(&wf_id).await {
                Ok(rows) => {
                    let items: Vec<Value> = rows.into_iter().map(|t| json!({
                        "id": t.id,
                        "wf_id": t.wf_id,
                        "parent_id": t.parent_id,
                        "title": t.title,
                        "status": t.status,
                        "assigned_to": t.assigned_to,
                        "model": t.model,
                        "repair_count": t.repair_count,
                        "input_tokens": t.input_tokens,
                        "output_tokens": t.output_tokens,
                        "cost_usd": t.cost_usd,
                        "files_modified": t.files_modified,
                        "started_at": t.started_at,
                        "finished_at": t.finished_at,
                        "result": t.result,
                    })).collect();
                    let (done, total) = s.repo.count_tasks(&wf_id).await
                        .unwrap_or((0, 0));
                    Ok((200, json!({
                        "ok": true,
                        "wf_id": wf_id,
                        "done": done,
                        "total": total,
                        "rows": items,
                    })))
                }
                Err(e) => Ok((500, json!({
                    "ok": false,
                    "error": format!("list_tasks: {e}"),
                }))),
            }
        })
    });

    // ── v0.4.21 (event 000066): GET /api/workspace ────────
    // Returns the current workspace root + display name. Cheap
    // snapshot — used by the Tauri shell's "is the workdir the
    // same as what runtime thinks it is?" diagnostic and by the
    // new FileTree component to anchor its root before
    // `GET /api/workspace/tree`.
    let s_wsget = state.clone();
    d.register("GET", "/api/workspace", move |_body| {
        let s = s_wsget.clone();
        Box::pin(async move {
            let ws = s.workspace_snapshot();
            Ok((200, json!({
                "ok": true,
                "root": ws.root.to_string_lossy(),
                "name": ws.name,
            })))
        })
    });

    // ── v0.4.21 (event 000066): POST /api/workspace/set ────
    // Body: { path: "C:\\path\\to\\workdir" } or { path: "/abs/path" }
    // Swaps the runtime's workspace root in place. Idempotent —
    // calling with the same path is a no-op (just rewrites the
    // RwLock with the same value). This is the bridge between
    // Tauri `set_workdir_with_nwt` (which only writes
    // workdir.json) and the actual chief agent's filesystem
    // context. Without this, the chairman's "切工作目录" UX
    // changed workdir.json but chief kept writing to the
    // launch-time cwd — silently diverging.
    let s_wsset = state.clone();
    d.register("POST", "/api/workspace/set", move |body| {
        let s = s_wsset.clone();
        Box::pin(async move {
            let Some(path) = body.get("path").and_then(|v| v.as_str()) else {
                let rec = ErrorRecord {
                    at: chrono::Utc::now().timestamp(),
                    severity: "warn".into(),
                    source: "workspace_swap".into(),
                    summary: "POST /api/workspace/set missing 'path'".into(),
                    detail: None,
                };
                s.push_error(rec);
                return Ok((400, json!({
                    "ok": false,
                    "error": "missing 'path'",
                })));
            };
            let p = std::path::PathBuf::from(path);
            if !p.exists() {
                let rec = ErrorRecord {
                    at: chrono::Utc::now().timestamp(),
                    severity: "warn".into(),
                    source: "workspace_swap".into(),
                    summary: format!("workdir path does not exist: {path}"),
                    detail: None,
                };
                s.push_error(rec);
                return Ok((400, json!({
                    "ok": false,
                    "error": format!("path does not exist: {}", p.display()),
                })));
            }
            if !p.is_dir() {
                let rec = ErrorRecord {
                    at: chrono::Utc::now().timestamp(),
                    severity: "warn".into(),
                    source: "workspace_swap".into(),
                    summary: format!("workdir not a directory: {path}"),
                    detail: None,
                };
                s.push_error(rec);
                return Ok((400, json!({
                    "ok": false,
                    "error": format!("not a directory: {}", p.display()),
                })));
            }
            let abs = match p.canonicalize() {
                Ok(a) => a,
                Err(e) => return Ok((500, json!({
                    "ok": false,
                    "error": format!("canonicalize: {e}"),
                }))),
            };
            s.set_workspace(abs.clone());
            Ok((200, json!({
                "ok": true,
                "root": abs.to_string_lossy(),
                "previous_root": s.workspace_snapshot().root.to_string_lossy(),
            })))
        })
    });

    // ── v0.4.21 (event 000066): GET /api/workspace/tree ──
    // Body: { path?: "<relative>", depth?: 2, max_entries?: 200 }
    // Lists directory entries under the current workspace root.
    // Used by the FileTree component. Hidden files are filtered
    // out by default. Result is depth-limited and entry-limited
    // to keep the payload small.
    let s_wstree = state.clone();
    d.register("GET", "/api/workspace/tree", move |body| {
        let s = s_wstree.clone();
        let rel = body.get("path").and_then(|v| v.as_str())
            .unwrap_or("").to_string();
        let depth = body.get("depth").and_then(|v| v.as_u64())
            .unwrap_or(2).min(8) as usize;
        let max_entries = body.get("max_entries").and_then(|v| v.as_u64())
            .unwrap_or(200).min(2000) as usize;
        Box::pin(async move {
            let ws = s.workspace_snapshot();
            let target = if rel.is_empty() {
                ws.root.clone()
            } else {
                let candidate = ws.resolve(&rel);
                // Safety: refuse paths that escape the workspace.
                if !ws.contains(&candidate) {
                    return Ok((403, json!({
                        "ok": false,
                        "error": format!("path escapes workspace: {}", candidate.display()),
                    })));
                }
                candidate
            };
            if !target.exists() {
                return Ok((404, json!({
                    "ok": false,
                    "error": format!("path does not exist: {}", target.display()),
                })));
            }
            let mut entries: Vec<Value> = Vec::new();
            let mut truncated = false;
            walk_tree(&target, depth, max_entries, &mut entries, &mut truncated);
            Ok((200, json!({
                "ok": true,
                "root": ws.root.to_string_lossy(),
                "path": ws.relativize(&target).to_string_lossy(),
                "entries": entries,
                "truncated": truncated,
                "count": entries.len(),
            })))
        })
    });

    // ── v0.4.21 (event 000066): GET /api/errors/recent ──
    // Returns the last N error rows captured by the pipe-server
    // (quota failures, run_task timeouts, workspace swap
    // rejections, agent errors). Backed by an in-memory
    // `Arc<Mutex<VecDeque<ErrorRecord>>>` so the TopBar red-dot
    // badge can poll without hammering SQLite. Capacity 200;
    // oldest entries evicted FIFO.
    //
    // Query: ?limit=N (default 10, capped at 200).
    let s_errors = state.clone();
    d.register("GET", "/api/errors/recent", move |body| {
        let s = s_errors.clone();
        let limit = body.get("limit").and_then(|v| v.as_u64())
            .unwrap_or(10).min(200) as usize;
        Box::pin(async move {
            let rows = s.recent_errors(limit);
            Ok((200, json!({
                "ok": true,
                "count": rows.len(),
                "rows": rows,
            })))
        })
    });

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
        let role = body.get("role").and_then(|v| v.as_str())
            .unwrap_or("").to_string();
        Box::pin(async move {
            let resolved = match resolve_role(&s, &role).await {
                Ok(r) => r,
                Err(e) => {
                    return Ok((200, json!({
                        "ok": false,
                        "role": role,
                        "error": e,
                        "hint": "open Settings → 角色 → 模型 分配",
                    })));
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
            Ok((200, json!({
                "ok": true,
                "role": resolved.role,
                "provider_short": resolved.provider_short,
                "model_id": resolved.model_id,
                "base_url": resolved.base_url,
                "api_kind": resolved.api_kind,
                "has_key": !resolved.api_key.is_empty(),
                "fallback_chain": resolved.fallback_chain,
                "quota_status": quota_status,
            })))
        })
    });

    register_placeholder_handlers(d, state.clone());

    // v0.4.20: stash dispatcher handle so the quota scheduler
    // (Phase-2) can dispatch internal retry requests.
    state.set_dispatcher(Arc::new(d.clone()));
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
            let roles_array: Vec<Value> = roles.into_iter()
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

    // v0.4.19 (event 000055): GET /api/router/roles/{role}/resolve
    // resolves a role's default_model + fallback_chain + keyring
    // availability into a single structured preview, used by the
    // ChatZone status line. Always returns 200 with `{ok:true,...}`
    // or `{ok:false,error}` so the frontend can render inline.
    let resolve_state = state.clone();
    d.register("GET", "/api/router/roles/{role}/resolve", move |body| {
        let s = resolve_state.clone();
        Box::pin(async move {
            let role = body.get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if role.is_empty() {
                return Ok((200, json!({ "ok": false, "error": "missing 'role' in path" })));
            }
            match resolve_role(&s, &role).await {
                Ok(r) => Ok((200, json!({
                    "ok": true,
                    "role": r.role,
                    "provider_short": r.provider_short,
                    "model_id": r.model_id,
                    "base_url": r.base_url,
                    "api_kind": r.api_kind,
                    "has_key": true,
                    "fallback_chain": r.fallback_chain,
                }))),
                Err(e) => Ok((200, json!({
                    "ok": false,
                    "role": role,
                    "error": e,
                }))),
            }
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

    // ── v0.4.22 (event 000068): POST /api/run_workflow ────
    // Body: { task: "<user_request>" }
    //
    // v0.4.22 (event 000069 follow-up): run the workflow on a
    // background tokio task and return wf_id immediately. The
    // monolithic call blocked the JSON-RPC handler for up to
    // 30+ minutes on large requests (78-card tarot app =
    // 1-requirement + 2-plan (3 rounds) + 3-plan-review
    // (2 parallel) + 4-dispatch + 5-develop (N workers) +
    // 6-final-review + 7-repair + 8-delivery — at 5 min/agent
    // worst case that's way past any reasonable HTTP timeout).
    //
    // Now: POST returns wf_id + status "running" in ~50ms.
    // Clients poll GET /api/workflow/{wf_id}/status for the
    // current phase + summary, or listen on the events pipe
    // for PhaseTransition updates (which the UI does anyway).
    let s_wf = state.clone();
    d.register("POST", "/api/run_workflow", move |body| {
        let s = s_wf.clone();
        Box::pin(async move {
            tracing::info!(target: "pipe_server", "[TRACE] /api/run_workflow handler ENTERED");
            let Some(task) = body.get("task").and_then(|v| v.as_str()) else {
                tracing::warn!(target: "pipe_server", "[TRACE] /api/run_workflow: missing 'task' field");
                return Ok((400, json!({
                    "ok": false,
                    "error": "missing 'task'",
                })));
            };
            if task.trim().is_empty() {
                tracing::warn!(target: "pipe_server", "[TRACE] /api/run_workflow: task is empty");
                return Ok((400, json!({
                    "ok": false,
                    "error": "'task' is empty",
                })));
            }
            tracing::info!(target: "pipe_server", task_len = task.len(), task_preview = %task.chars().take(80).collect::<String>(), "[TRACE] /api/run_workflow: creating Orchestrator");
            let orch = crate::orchestrator::Orchestrator::new(
                s.clone(),
                s.events.clone(),
                task.to_string(),
            );
            let wf_id = orch.wf_id.clone();
            tracing::info!(target: "pipe_server", wf_id = %wf_id, "[TRACE] /api/run_workflow: spawning orch.run() on background tokio task");
            // Spawn the orchestrator on a background task so
            // the JSON-RPC response returns immediately. Each
            // phase still emits AgentEvent::PhaseTransition on
            // the events pipe so the UI animates as the
            // workflow progresses.
            // v0.4.22 (event 000091 fix #32): the cancel token
            // is created here and stashed in the active_workflows
            // map so the `POST /api/workflow/{id}/cancel` route
            // can fire it. The token is removed when the
            // workflow finishes (cleanup below). For now the
            // orchestrator itself doesn't observe the token —
            // firing it just marks the workflow as cancelled in
            // the active map and prevents new phases from
            // starting; agents in flight will run to their
            // natural 5-min timeout. That's a v0.4.23 polish
            // (plumb the token into run_agent's tokio::select!).
            let cancel_token = tokio_util::sync::CancellationToken::new();
            {
                let mut map = s.active_workflows.lock().expect("active_workflows mutex");
                map.insert(wf_id.clone(), cancel_token);
            }
            let wf_id_for_log = wf_id.clone();
            let s_for_cleanup = s.clone();
            let wf_id_for_cleanup = wf_id.clone();
            tokio::spawn(async move {
                tracing::info!(target: "pipe_server", wf_id = %wf_id_for_log, "[TRACE] orch.run() STARTING on background task");
                let summary = orch.run().await;
                // Remove the cancel token now that the
                // workflow is done — the route will return
                // 404 from this point on, which matches the
                // "no longer active" semantic.
                {
                    let mut map = s_for_cleanup.active_workflows.lock().expect("active_workflows mutex");
                    map.remove(&wf_id_for_cleanup);
                }
                tracing::info!(
                    target: "pipe_server",
                    wf_id = %wf_id_for_log,
                    summary_len = summary.len(),
                    "[TRACE] orch.run() FINISHED — workflow complete (cancel token removed)"
                );
            });
            tracing::info!(target: "pipe_server", wf_id = %wf_id, "[TRACE] /api/run_workflow: returning wf_id to caller (cancel token registered)");
            Ok((200, json!({
                "ok": true,
                "wf_id": wf_id,
                "status": "running",
                "note": "poll GET /api/workflow/{wf_id}/status for current phase + summary, or listen on wf:event channel. POST /api/workflow/{wf_id}/cancel to interrupt.",
            })))
        })
    });

    // ── v0.4.22 (event 000069): GET /api/workflow/{wf_id}/status ─
    // Returns the current status of a workflow. Reads the
    // workflows row (state / phase) plus aggregates the tasks
    // table to count done vs in-flight tasks under the wf_id.
    // The orchestrator updates workflows.state to DONE when
    // delivery finishes.
    let s_wfstatus = state.clone();
    d.register("GET", "/api/workflow/{wf_id}/status", move |body| {
        let s = s_wfstatus.clone();
        let wf_id = body.get("wf_id").and_then(|v| v.as_str())
            .unwrap_or("").to_string();
        Box::pin(async move {
            if wf_id.is_empty() {
                return Ok((400, json!({
                    "ok": false,
                    "error": "missing 'wf_id' in path placeholder",
                })));
            }
            let wf_row = s.repo.get_workflow(&wf_id).await.unwrap_or(None);
            // If workflows row missing, orchestrator hasn't
            // started yet (or never persisted — fall through).
            let (state, phase, summary) = match &wf_row {
                Some(w) => {
                    // WorkflowState + WorkflowPhase enums aren't
                    // `pub` re-exported across the workspace,
                    // so we just Debug-print and trim. The
                    // orchestrator writes prefixed strings
                    // ("1-requirement" / "ACTIVE" / "DONE") so
                    // a simple string suffix match works.
                    let state_str = format!("{:?}", w.state).to_lowercase();
                    let phase_str = format!("{:?}", w.phase).to_lowercase();
                    (state_str, phase_str, w.summary.clone())
                }
                None => ("unknown".to_string(), "unknown".to_string(), None),
            };
            // Count tasks for this wf_id to show progress.
            let (done, total) = s.repo.count_tasks(&wf_id).await.unwrap_or((0, 0));
            Ok((200, json!({
                "ok": true,
                "wf_id": wf_id,
                "status": state,
                "phase": phase,
                "summary": summary,
                "tasks_done": done,
                "tasks_total": total,
            })))
        })
    });

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
                return Ok((400, json!({
                    "ok": false,
                    "error": "missing 'roles' array in body",
                })));
            };
            let mut updated = 0usize;
            let mut bad: Vec<String> = Vec::new();
            for role in roles_arr {
                let Some(role_id) = role.get("role").and_then(|v| v.as_str()) else {
                    bad.push("<missing role id>".to_string());
                    continue;
                };
                let default_model = role
                    .get("default_model").and_then(|v| v.as_str()).unwrap_or("");
                let fallback_chain: Vec<String> = role
                    .get("fallback_chain").and_then(|v| v.as_array())
                    .map(|arr| arr.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect())
                    .unwrap_or_default();
                if let Err(e) = s.repo.upsert_role_override(
                    role_id, default_model, &fallback_chain,
                ).await {
                    bad.push(format!("{}: {}", role_id, e));
                    continue;
                }
                updated += 1;
            }
            Ok((200, json!({
                "ok": true,
                "updated": updated,
                "errors": bad,
            })))
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

    // ── v0.4.22 (event 000074): Tarot random oracle ──────
    // Used by the Flwntier Android chief client (apps/ChiefApp).
    // The chairman's spec (NWT 000073): the Android client
    // looks like iching-oracle but the data path runs
    // through this Flwntier runtime endpoint, not a local
    // 64-gua JSON. Two modes:
    //   GET  /api/tarot/draw           → single card
    //   GET  /api/tarot/draw?spread=3  → past/present/future
    //   GET  /api/tarot/list           → 78-card deck metadata
    // The card payload is JSON with id, arcana, suit, rank,
    // name_zh, name_pinyin, name_en, symbol_svg
    // (100x140 inline SVG the Android app renders as
    // 翻牌 / 正逆位 image), upright_meaning, reversed_meaning.
    d.register("GET", "/api/tarot/draw", |body| {
        // Pull `spread` out of the body BEFORE the async move
        // so the closure doesn't borrow body across the await
        // boundary (E0515).
        let spread = body
            .get("spread")
            .and_then(|v| v.as_str())
            .unwrap_or("1")
            .to_string();
        Box::pin(async move {
            let spread = spread.as_str();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let drawn: Vec<crate::tarot::DrawnCard> = match spread {
                "3" | "three" | "past-present-future" => {
                    crate::tarot::draw_three_card_spread()
                }
                _ => vec![crate::tarot::draw_single("抽卡")],
            };
            let items: Vec<Value> = drawn.iter().map(|d| json!({
                "position": d.position,
                "reversed": d.reversed,
                "meaning": d.meaning,
                "card": {
                    "id": d.card.id,
                    "arcana": d.card.arcana,
                    "suit": d.card.suit,
                    "rank": d.card.rank,
                    "name_zh": d.card.name_zh,
                    "name_pinyin": d.card.name_pinyin,
                    "name_en": d.card.name_en,
                    "symbol_svg": d.card.symbol_svg,
                    "upright_meaning": d.card.upright_meaning,
                    "reversed_meaning": d.card.reversed_meaning,
                },
            })).collect();
            Ok((200, json!({
                "ok": true,
                "spread": spread,
                "count": items.len(),
                "drawn_at_ms": now,
                "items": items,
            })))
        })
    });

    d.register("GET", "/api/tarot/list", |_body| {
        Box::pin(async {
            let cards: Vec<Value> = crate::tarot::deck().iter().map(|c| json!({
                "id": c.id,
                "arcana": c.arcana,
                "suit": c.suit,
                "rank": c.rank,
                "name_zh": c.name_zh,
                "name_pinyin": c.name_pinyin,
                "name_en": c.name_en,
            })).collect();
            Ok((200, json!({
                "ok": true,
                "count": cards.len(),
                "cards": cards,
            })))
        })
    });

    // ── v0.4.22 (event 000091 fix #32): real cancel. Looks
    // up the active workflow in `state.active_workflows`, fires
    // its `CancellationToken` (so the in-flight agent loop
    // returns `ABORTED` and the orchestrator unwinds), and
    // returns 200 with the wf_id. If no such workflow is
    // active, returns 404 — the previous stub always returned
    // 200 with "no active workflow to cancel", which was
    // indistinguishable from a real cancel and let the UI
    // believe it had stopped a runaway workflow.
    let s_cancel = Arc::clone(&state);
    d.register("POST", "/api/workflow/{id}/cancel", move |body| {
        let s = s_cancel.clone();
        Box::pin(async move {
            let wf_id = body.get("wf_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if wf_id.is_empty() {
                return Ok((400, json!({
                    "ok": false,
                    "error": "missing 'wf_id' in path placeholder",
                })));
            }
            let token = {
                let map = s.active_workflows.lock().expect("active_workflows mutex");
                map.get(&wf_id).cloned()
            };
            match token {
                Some(tok) => {
                    tok.cancel();
                    Ok((200, json!({
                        "ok": true,
                        "wf_id": wf_id,
                        "note": "cancellation fired; orchestrator unwinds at next agent boundary",
                    })))
                }
                None => Ok((404, json!({
                    "ok": false,
                    "wf_id": wf_id,
                    "error": "no active workflow with this id",
                }))),
            }
        })
    });

    // ── v0.4.22 (event 000080): log endpoints ────────
    // Per chairman: "日志的功能让我们设置为开发环节的
    // 功能,你内置出来方便删". These two routes expose
    // the runtime's tracing log file (~/Desktop/Flwntier.log
    // by default) for the chairman to inspect / scrub.
    // Gated by `FLWNTIER_LOG_API=1` so a released build
    // (no env var set) does not expose a file-read surface.
    // When disabled, both endpoints return 403 with a
    // "FLWNTIER_LOG_API is not set" message.
    //
    // GET  /api/logs/get?tail=N — last N lines, default 200.
    // POST /api/logs/clear       — truncate the log file
    //                              to 0 bytes and write a
    //                              sentinel so the next session
    //                              starts cleanly. Returns
    //                              the new (empty) log file
    //                              path on success.
    d.register("GET", "/api/logs/get", |body| {
        let tail = body
            .get("tail")
            .and_then(|v| v.as_u64())
            .unwrap_or(200)
            .min(10_000) as usize;
        Box::pin(async move {
            if !crate::logs::log_api_enabled() {
                return Ok((403, json!({
                    "ok": false,
                    "error": "FLWNTIER_LOG_API is not set; \
                              log API is a development feature \
                              (event 000080).",
                })));
            }
            let path = crate::logs::resolve_log_path();
            let lines = crate::logs::read_tail(tail);
            Ok((200, json!({
                "ok": true,
                "log_file": path.as_ref().map(|p| p.display().to_string()),
                "log_file_enabled": path.is_some(),
                "tail": lines.len(),
                "lines": lines,
            })))
        })
    });
    d.register("POST", "/api/logs/clear", |_body| {
        Box::pin(async move {
            if !crate::logs::log_api_enabled() {
                return Ok((403, json!({
                    "ok": false,
                    "error": "FLWNTIER_LOG_API is not set; \
                              log API is a development feature \
                              (event 000080).",
                })));
            }
            match crate::logs::clear_log() {
                Ok(path) => Ok((200, json!({
                    "ok": true,
                    "path": path.display().to_string(),
                    "cleared_at": chrono::Utc::now().to_rfc3339(),
                }))),
                Err(e) => Ok((500, json!({
                    "ok": false,
                    "error": e.to_string(),
                }))),
            }
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
                "ok": true,
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
            None => return Ok((404, json!({
                "ok": false,
                "error": format!("unknown provider: {id}"),
            }))),
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
            "ok": true,
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
            return Ok((200, json!({
                "ok": false,
                "error": "no API key configured",
                "secret_name": secret_name,
                "url": url,
                "provider_id": id,
            })));
        }
        Err(e) => return Ok((200, json!({
            "ok": false,
            "error": format!("reveal: {e}"),
            "provider_id": id,
        }))),
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("reqwest build: {e}"))?;
    let resp = match client.get(&url)
        .header("Authorization", format!("Bearer {}", api_key.as_str()))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return Ok((200, json!({
                "ok": false,
                "error": format!("network error: {e}"),
                "url": url,
                "provider_id": id,
            })));
        }
    };
    let status = resp.status();
    if !status.is_success() {
        return Ok((200, json!({
            "ok": false,
            "error": format!("provider returned {status}"),
            "url": url,
            "provider_id": id,
        })));
    }
    let body: Value = match resp.json().await {
        Ok(b) => b,
        Err(e) => {
            return Ok((200, json!({
                "ok": false,
                "error": format!("parse {url}: {e}"),
                "url": url,
                "provider_id": id,
            })));
        }
    };
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
        "ok": true,
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
    let id = body.get("id").and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'id'".to_string())?.to_string();
    let display_name = body.get("display_name").and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'display_name'".to_string())?.to_string();
    let base_url = body.get("base_url").and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'base_url'".to_string())?.to_string();
    let kind = body.get("kind").and_then(|v| v.as_str())
        .unwrap_or("openai-compatible").to_string();
    let api_key_env = body.get("api_key_env").and_then(|v| v.as_str())
        .map(|s| s.to_string());
    // v0.4.22 (event 000096): `default_model` is now taken from
    // the user's first model row (or explicit field). The shell
    // sends `models[].id` (the model id string) and
    // `models[].display_name`; we persist the id as a
    // comma-separated fallback chain so resolve_role sees the
    // user's actual model list when they pick `<custom_id>:*`.
    let models_json: Vec<String> = body.get("models")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter()
            .filter_map(|m| m.get("id").and_then(|x| x.as_str()).map(String::from))
            .collect())
        .unwrap_or_default();
    let default_model = models_json.first().cloned();

    if id.is_empty() || id.len() > 64 {
        return Ok((400, json!({ "error": "id must be 1..=64 chars" })));
    }
    if display_name.is_empty() || display_name.len() > 64 {
        return Ok((400, json!({ "error": "display_name must be 1..=64 chars" })));
    }
    if !base_url.starts_with("https://") && !base_url.starts_with("http://") {
        return Ok((400, json!({ "error": "base_url must start with http(s)://" })));
    }
    if kind != "openai-compatible" && kind != "anthropic-compatible" {
        return Ok((400, json!({ "error": "kind must be openai-compatible or anthropic-compatible" })));
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
    state.repo.insert_custom_provider(&row).await
        .map_err(|e| format!("insert_custom: {e}"))?;

    // v0.4.22 (event 000096): persist the models list. The
    // custom_provider table only stores `default_model`; we
    // mirror the full list into the model_cache row so the
    // Settings UI can render them.
    if !models_json.is_empty() {
        let body_str = serde_json::to_string(&models_json).unwrap_or_else(|_| "[]".into());
        let _ = state.repo.put_model_cache(&storage::ModelCacheRow {
            provider_id: format!("custom:{id}"),
            models_json: body_str,
            fetched_at: now,
        }).await;
    }

    // If an api_key was supplied in the same POST (the Tauri
    // shell calls save_secret() first to encrypt-and-store,
    // then addCustomProvider), nothing else needed here. But
    // also fall back to reading api_key directly (legacy
    // compat) so any caller still works.
    if let Some(key) = body.get("api_key").and_then(|v| v.as_str()) {
        let secret_name = api_key_env.clone()
            .unwrap_or_else(|| format!("CUSTOM_PROVIDER_KEY_{id}"));
        state.secrets.put(&secret_name, key).await
            .map_err(|e| format!("put secret: {e}"))?;
    }

    Ok((201, json!({
        "id": id,
        "name": display_name,
        "base_url": base_url,
        "kind": kind,
        "default_model": models_json.first(),
        "models": models_json,
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

async fn resolve_role(
    state: &Arc<ServerState>,
    role: &str,
) -> Result<ResolvedRole, String> {
    // 1. Read the override row (DB). May be absent — caller uses
    //    in-memory defaults (which are all empty as of v0.4.16).
    let ov = state.repo.get_role_override(role).await
        .map_err(|e| format!("get_role_override: {e}"))?;
    let (default_model, fallback_chain) = match ov {
        Some(r) => (r.default_model, r.fallback_chain),
        None => (String::new(), Vec::new()),
    };
    if default_model.is_empty() {
        return Err("role not configured: open Settings → 角色 → 模型 分配 and pick a default_model".into());
    }
    // 2. Split "<provider_short>:<model_id>".
    let (provider_short, model_id) = match default_model.split_once(':') {
        Some((p, m)) => (p.to_string(), m.to_string()),
        None => return Err(format!(
            "default_model '{}' must be in '<provider>:<model>' form", default_model
        )),
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
            // convention `CUSTOM_<ID>_API_KEY`.
            let cp = state.repo.get_custom_provider(&provider_short).await
                .map_err(|e| format!("get_custom: {e}"))?
                .ok_or_else(|| format!(
                    "unknown provider preset or custom '{}' from default_model '{}' \
                     (Settings → 中转站 → 添加 custom relay, or change default_model)",
                    provider_short, default_model
                ))?;
            // The shell saved the secret under an env-var name
            // like CUSTOM_MIMIMU_API_KEY (uppercased id).
            // Convention: id + '_API_KEY' uppercase.
            let secret = format!("CUSTOM_{}_API_KEY", provider_short.to_uppercase());
            (cp.base_url, cp.kind, secret)
        };
    // 4. Reveal the API key from the keychain. Empty defaults give
    //    503 so the chairman knows the cause.
    let api_key: Zeroizing<String> = match state.secrets.reveal(&secret_name).await {
        Ok(z) if !z.is_empty() => z,
        _ => return Err(format!(
            "no API key configured for {} (set it in Settings → 供应商)", secret_name
        )),
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

async fn run_task(body: Value, state: Arc<ServerState>) -> Result<(u16, Value), String> {
    // ── v0.4.19: minimal body shape ───────────────────────────
    // Required: { task, role }
    // Optional (legacy): { provider_kind, base_url, model, api_key, wf_id }
    //
    // When the optional fields are absent, the server resolves
    // them from role_overrides (v0.4.18) + the matching preset's
    // base_url / secret_name, revealing the API key from the OS
    // keystore. The frontend (ChatZone v0.4.19) now sends only
    // { task, role }.
    let task_text = body
        .get("task")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'task'".to_string())?
        .to_string();
    let role = body
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("agent:worker")
        .to_string();

    // Legacy path: caller supplied concrete provider/model/key.
    let legacy_explicit = body.get("base_url").is_some()
        || body.get("model").is_some()
        || body.get("api_key").is_some();
    let explicit_provider_kind = body
        .get("provider_kind")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "openai_compat".to_string());
    let explicit_base_url = body
        .get("base_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let explicit_model = body
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let explicit_api_key = body
        .get("api_key")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| zeroize::Zeroizing::new(s.to_string()));

    let wf_id = body
        .get("wf_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // ── Build the primary provider ─────────────────────────────
    let (provider_kind, base_url, model, api_key) = if legacy_explicit {
        // Caller supplied everything — honor it (debug / scripted use).
        let base_url = explicit_base_url
            .clone()
            .ok_or_else(|| "missing 'base_url'".to_string())?;
        let base_url = agent_core::provider::openai::validate_base_url(&base_url)
            .map_err(|e| format!("invalid base_url: {e}"))?;
        let api_key = explicit_api_key
            .clone()
            .ok_or_else(|| "missing or empty 'api_key'".to_string())?;
        let model = explicit_model
            .clone()
            .ok_or_else(|| "missing 'model'".to_string())?;
        (explicit_provider_kind.clone(), base_url, model, api_key)
    } else {
        // v0.4.19: resolve from role_overrides + preset keychain.
        // If the primary fails, we'll iterate the fallback_chain
        // below; for now we attempt the primary once.
        let resolved = match resolve_role(&state, &role).await {
            Ok(r) => r,
            Err(e) => {
                // Friendlier 200/503 envelope than the legacy raw
                // 500/Err so the frontend can render it nicely.
                return Ok((503, json!({
                    "ok": false,
                    "role": role,
                    "error": e,
                    "hint": "open Settings → 角色 → 模型 分配",
                })));
            }
        };
        let provider_kind = match resolved.api_kind.as_str() {
            "openai-compatible" => "openai_compat".to_string(),
            "anthropic-compatible" => "openai_compat".to_string(), // best-effort
            other => other.to_string(),
        };
        (provider_kind, resolved.base_url, resolved.model_id, resolved.api_key)
    };

    // ── Build provider ────────────────────────────────────────
    // v0.4.20: keep a clone of `model` so the quota-tracking
    // block below (line ~1490) can reference it after OpenAiProvider
    // has moved it.
    let model_for_quota = model.clone();
    let provider: Arc<dyn agent_core::Provider> = match provider_kind.as_str() {
        "openai" => Arc::new(OpenAiProvider::openai(model, api_key.to_string())),
        "openai_compat" => Arc::new(OpenAiProvider::compat(base_url, model, api_key.to_string())),
        other => return Err(format!("unsupported provider_kind: {other}")),
    };

    // ── Build agent ───────────────────────────────────────────
    let role_enum = match role.as_str() {
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
        state.workspace_snapshot(),
        AgentConfig::default(),
    );

    // ── Stream events to subscribers while running ────────────
    // Keep a copy of the task_text so we can write it into the
    // tasks row at the end (v0.4.21 event 000064).
    let task_text_for_record = task_text.clone();

    // v0.4.21 (event 000066): outer timeout wraps the whole
    // agent.run() loop. Default 5 min — chief tasks with the
    // current provider set rarely exceed 2 min for tool-heavy
    // runs, and we'd rather the chairman see a clean TIMEOUT
    // status than stare at a spinner for an hour because
    // api.minimaxi.com is throttling. Caller can override via
    // body.timeout_secs; cap at 30 min so a buggy client can't
    // DoS the runtime with a 24-hour task.
    let timeout_secs: u64 = body
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(300)
        .clamp(10, 1800);

    let mut rx = agent.run(task_text);
    let mut last_status = "UNKNOWN".to_string();
    let mut summary: Option<String> = None;
    let timed_out = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        async {
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
                    return false; // not a timeout
                }
            }
            true // channel closed without Done — treat as timeout-shaped
        },
    )
    .await
    .unwrap_or(true);
    if timed_out && !matches!(last_status.as_str(), "DONE" | "FAILED" | "ABORTED" | "ABORTED_REPEAT") {
        // Synthesize a Done event so subscribers see the
        // terminal state, and stamp the status the frontend
        // can grep on (event 000066 introduces "TIMEOUT" as a
        // new terminal status; before this the UI would have
        // hung in 'sending=true' indefinitely).
        last_status = format!("TIMEOUT ({timeout_secs}s)");
        summary = Some(format!(
            "agent.run() exceeded the {timeout_secs}s outer timeout. \
             This usually means the upstream provider (e.g. minimax, \
             openai) is hanging, throttling, or returning malformed \
             streaming responses. Inspect /api/errors/recent and \
             consider lowering the per-task timeout_secs."
        ));
        let _ = state.events.send(AgentEvent::Done {
            wf_id: wf_id.clone(),
            status: last_status.clone(),
            summary: summary.clone(),
        });
        // v0.4.21 (event 000066): surface to TopBar badge.
        state.push_error(ErrorRecord {
            at: chrono::Utc::now().timestamp(),
            severity: "error".into(),
            source: "run_task".into(),
            summary: format!("run_task timed out after {timeout_secs}s"),
            detail: Some(format!("role={role} model={model_for_quota}")),
        });
    }

    // ── v0.4.20: quota-failure recording ──────────────────
    // Any non-DONE status counts as a quota-class failure
    // (deliberately conservative — false positives just promote
    // the row, never auto-block; per-provider error-code parsing
    // is Phase-2/v0.4.21). On success: clear any prior row.
    let status_clean = last_status
        .split_whitespace().next().unwrap_or("UNKNOWN").to_string();
    if status_clean != "DONE" {
        let err_msg = summary.clone().unwrap_or_else(|| last_status.clone());
        let err_msg: String = err_msg.chars().take(240).collect();
        // v0.4.21 (event 000066): surface quota-class failures
        // to the TopBar badge so the chairman sees them without
        // having to dig through Settings.
        state.push_error(ErrorRecord {
            at: chrono::Utc::now().timestamp(),
            severity: "error".into(),
            source: "quota".into(),
            summary: format!("{role} run failed: {status_clean}"),
            detail: Some(err_msg.clone()),
        });
        if let Err(e) = state.repo.record_quota_failure(
            &role, &model_for_quota, &err_msg,
        ).await {
            warn!(error = %e, "v0.4.20: failed to record_quota_failure");
        }
        // Chief escalation: chief's own failure flips to
        // pending_5h_wait so the scheduler retries at the next
        // 5h boundary. Other (role, model) failures just record —
        // chief's loop picks them up via the events bus on its
        // next iteration.
        if role == "agent:chief" {
            if let Err(e) = state.repo.set_quota_pending_5h_wait(
                &role, &model_for_quota,
            ).await {
                warn!(error = %e, "v0.4.20: failed to set_quota_pending_5h_wait for chief");
            }
        }
    } else {
        // Success path — clear any prior (chief, model) row so
        // the StatusLine flips back to "正常" automatically. We
        // also drop all chief rows when chief itself succeeds
        // (cheap; lets the chairman see a clean slate).
        if let Err(e) = state.repo.clear_quota_failure(
            &role, Some(&model_for_quota),
        ).await {
            warn!(error = %e, "v0.4.20: failed to clear_quota_failure on success");
        }
        if role == "agent:chief" {
            let _ = state.repo.clear_quota_failure(&role, None).await;
        }
    }

    // v0.4.21 (event 000064): persist a task row so the
    // dashboard's "任务列表" panel shows real progress. The
    // tasks table was always empty before this fix (run_task
    // only wrote AgentEvents, never INSERT INTO tasks), so the
    // chairman always saw "0/0 完成" no matter how much chief
    // actually did. We write a single row per run_task call,
    // with the role as `assigned_to` and the final status as
    // `status` (done | failed | aborted). The result column
    // stores the summary so the chairman can re-read it.
    let now = chrono::Utc::now().timestamp();
    let wf_id_for_task = if wf_id.is_empty() {
        // v0.4.19+ chat path doesn't always carry a wf_id
        // (run_task handler's wf_id comes from the legacy
        // "run_agent_task" body). Synthesise a stable per-chat
        // wf_id from user_request + role + now so the task is
        // grouped under something the chairman can recognise.
        let key = format!("{}|{}|{}", role, model_for_quota, task_text_for_record);
        let mut h: u64 = 1469598103934665603;
        for b in key.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        format!("wf_chat_{:016x}", h)
    } else {
        wf_id.clone()
    };
    // v0.4.21 (event 000064 follow-up): the tasks table has a
    // FK constraint (`wf_id REFERENCES workflows(id)`) and the
    // storage crate enables `foreign_keys=true` per-connection.
    // For chat-derived wf_ids (synthesised above) there is no
    // matching row in `workflows`, so the INSERT into tasks
    // would fail and the dashboard would still see "0/0 完成".
    // Fix: ensure a workflow row exists for the synthetic id.
    // Use `INSERT OR IGNORE` so concurrent chat tasks with the
    // same wf_chat_* id don't fight — the FIRST one creates the
    // row, the rest no-op. Real (non-synthetic) wf_ids already
    // have a workflows row from the dispatcher, but we still
    // call this — the OR IGNORE makes it safe and idempotent.
    let wf_insert = state.repo.ensure_workflow_row(
        &wf_id_for_task,
        &task_text_for_record,
        &status_clean,
    ).await;
    if let Err(e) = wf_insert {
        warn!("v0.4.21 (event 000064 follow-up): ensure_workflow_row failed for {wf_id_for_task}: {e}");
    }
    let task_id = format!("t_{}", ulid::Ulid::new());
    let status_for_task = status_clean.clone();
    let title = if task_text_for_record.chars().count() > 60 {
        let truncated: String = task_text_for_record.chars().take(60).collect();
        format!("{truncated}…")
    } else {
        task_text_for_record.clone()
    };
    let task_result = state.repo.create_task(&storage::Task {
        id: task_id.clone(),
        wf_id: wf_id_for_task.clone(),
        parent_id: None,
        title,
        status: status_for_task.to_lowercase(),
        assigned_to: Some(role.clone()),
        model: Some(model_for_quota.clone()),
        repair_count: 0,
        input_tokens: 0,
        output_tokens: 0,
        cost_usd: None,
        files_modified: None,
        started_at: Some(now),
        finished_at: Some(now),
        result: summary.clone(),
    }).await;
    if let Err(e) = task_result {
        // Log but do not fail the run_task response — the agent
        // work already succeeded, the dashboard just won't see
        // a new task row this time. This is the path that was
        // silently dropped before event 000064.
        warn!("v0.4.21 (event 000064): create_task failed for {task_id}: {e}");
    }

    Ok((
        200,
        json!({
            "ok": true,
            "status": last_status,
            "summary": summary,
            "task_id": task_id,
            "wf_id": wf_id_for_task,
        }),
    ))
}

/// v0.4.21 (event 000066): depth-limited recursive directory
/// walker for `GET /api/workspace/tree`. Filters hidden files
/// (`.` prefix on Unix, also `node_modules` / `.git` / `target`
/// on all platforms — these are noise for the chairman's
/// project browser). Caps the result at `max_entries` and
/// flips `truncated = true` when the cap kicks in so the UI
/// can show a "…more" footer.
fn walk_tree(
    dir: &std::path::Path,
    depth: usize,
    max_entries: usize,
    out: &mut Vec<serde_json::Value>,
    truncated: &mut bool,
) {
    if depth == 0 || out.len() >= max_entries {
        if out.len() >= max_entries { *truncated = true; }
        return;
    }
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return, // unreadable — skip silently
    };
    let mut entries: Vec<_> = read
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            // Skip hidden + known-noisy dirs.
            !(name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == "dist")
        })
        .collect();
    entries.sort_by_key(|e| {
        let name = e.file_name().to_string_lossy().to_string();
        // Directories first, then alphabetical.
        (e.file_type().map(|t| !t.is_dir()).unwrap_or(false), name)
    });
    for entry in entries {
        if out.len() >= max_entries {
            *truncated = true;
            return;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let ft = entry.file_type().ok();
        let is_dir = ft.as_ref().map(|t| t.is_dir()).unwrap_or(false);
        let is_file = ft.as_ref().map(|t| t.is_file()).unwrap_or(false);
        let size = if is_file {
            entry.metadata().ok().map(|m| m.len())
        } else {
            None
        };
        let mut node = json!({
            "name": name,
            "path": entry.path().to_string_lossy(),
            "is_dir": is_dir,
            "is_file": is_file,
        });
        if let Some(s) = size { node["size"] = json!(s); }
        if is_dir && depth > 1 {
            let mut children = Vec::new();
            let mut sub_truncated = false;
            walk_tree(&entry.path(), depth - 1, max_entries - out.len() - 1, &mut children, &mut sub_truncated);
            node["children"] = json!(children);
            if sub_truncated { *truncated = true; }
        }
        out.push(node);
    }
}