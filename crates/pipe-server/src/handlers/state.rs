//! Pipe-server shared state: ErrorRecord, ServerState, ActiveWorkflows.
//! Split out of handlers.rs - pure move, no logic change.

use super::*;

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
    /// Current tool registry. v0.4.36: wrapped in
    /// `RwLock<Arc<…>>` so background MCP rebuilds can swap it
    /// atomically — readers snapshot the Arc and are never
    /// blocked mid-task (same pattern as `workspace` below).
    pub tools: Arc<std::sync::RwLock<Arc<ToolRegistry>>>,
    /// v0.4.36: live MCP server connections of the current
    /// generation. Replaced wholesale on every rebuild; replaced
    /// servers are shut down (process-tree kill).
    pub mcp_servers: Arc<tokio::sync::Mutex<Vec<Arc<McpServer>>>>,
    /// v0.4.36: last MCP scan outcome, for `GET /api/mcp/status`.
    pub mcp_status: Arc<tokio::sync::Mutex<Vec<tools_registry::McpServerStatus>>>,
    /// v0.4.36: active `[permissions] mode` for the current
    /// workspace (read by run_task / orchestrator when building
    /// each agent's config).
    pub permission_mode: Arc<std::sync::RwLock<PermissionMode>>,
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
    pub(super) dispatcher: Arc<std::sync::Mutex<Option<Arc<Dispatcher>>>>,
    /// v0.4.22 (event 000091 fix #32): active workflows map
    /// keyed by `wf_id`. The orchestrator's cancel token is
    /// stored here when the workflow starts; the cancel route
    /// reads it to fire cancellation, then removes the
    /// entry on natural completion. Without this, the
    /// `cancel_workflow` Tauri command was a no-op stub and
    /// the chairman had no way to interrupt a runaway
    /// 30-minute workflow.
    pub(super) active_workflows: Arc<ActiveWorkflows>,
}

/// v0.4.22 (event 000118, fix 7 hardening): thin newtype around
/// the wf_id → cancel_token map. Exists primarily so the test
/// suite at `tests/hardening_fix7.rs` can exercise insert/get/
/// remove without spinning up a full ServerState.
#[derive(Default)]
pub struct ActiveWorkflows {
    inner: std::sync::Mutex<std::collections::HashMap<String, tokio_util::sync::CancellationToken>>,
}

impl ActiveWorkflows {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, wf_id: String, token: tokio_util::sync::CancellationToken) {
        let mut m = self.inner.lock().expect("ActiveWorkflows mutex poisoned");
        m.insert(wf_id, token);
    }

    pub fn get(&self, wf_id: &str) -> Option<tokio_util::sync::CancellationToken> {
        let m = self.inner.lock().expect("ActiveWorkflows mutex poisoned");
        m.get(wf_id).cloned()
    }

    pub fn remove(&self, wf_id: &str) -> Option<tokio_util::sync::CancellationToken> {
        let mut m = self.inner.lock().expect("ActiveWorkflows mutex poisoned");
        m.remove(wf_id)
    }

    pub fn len(&self) -> usize {
        let m = self.inner.lock().expect("ActiveWorkflows mutex poisoned");
        m.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl ServerState {
    /// New default state. Opens the SQLite repo at
    /// `<data_dir>/storage.sqlite` and constructs a SecretStore
    /// bound to the same data dir.
    pub async fn new(workspace_root: std::path::PathBuf, data_dir: std::path::PathBuf) -> Self {
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

        // v0.4.36: seed the registry from the workspace config
        // synchronously (cheap — builtins + permission filtering),
        // then hand MCP handshakes to a background rebuild so a
        // hung server can never delay the listen loop.
        let ws_toml = crate::workspace_toml::WorkspaceToml::load(&workspace_root);
        let mut initial = ToolRegistry::with_builtins();
        if ws_toml.permissions.mode == PermissionMode::ReadOnly {
            initial.retain_read_only();
        }
        let mode = ws_toml.permissions.mode;

        let state = Self {
            events,
            tools: Arc::new(std::sync::RwLock::new(Arc::new(initial))),
            mcp_servers: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            mcp_status: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            permission_mode: Arc::new(std::sync::RwLock::new(mode)),
            workspace: Arc::new(std::sync::RwLock::new(Workspace::new(
                workspace_root.clone(),
                "flowntier",
            ))),
            errors: Arc::new(std::sync::Mutex::new(
                std::collections::VecDeque::with_capacity(200),
            )),
            secrets,
            repo,
            dispatcher: Arc::new(std::sync::Mutex::new(None)),
            active_workflows: Arc::new(ActiveWorkflows::new()),
        };

        // v0.4.36: MCP handshakes run in the background — the pipe
        // server must start listening even if every configured MCP
        // server is offline (debate point C).
        let for_rebuild = state.clone();
        tokio::spawn(async move {
            tools_registry::rebuild(&for_rebuild, "startup").await;
        });

        state
    }

    /// v0.4.36: snapshot the current tool registry. Cheap (Arc
    /// clone), used everywhere an agent is built so a background
    /// MCP rebuild can swap the registry atomically.
    pub fn tools_snapshot(&self) -> Arc<ToolRegistry> {
        self.tools.read().expect("tools rwlock poisoned").clone()
    }

    /// v0.4.36: current `[permissions] mode`.
    pub fn permission(&self) -> PermissionMode {
        *self
            .permission_mode
            .read()
            .expect("permission rwlock poisoned")
    }

    /// v0.4.36: build the [`AgentConfig`] for a run in workspace
    /// `ws`: capability set from the permission mode + project
    /// rule files re-read per run (an `AGENTS.md` edit applies to
    /// the next task without a restart).
    pub fn agent_config(&self, ws: &Workspace) -> AgentConfig {
        let caps = tools_registry::capabilities_for(self.permission());
        AgentConfig {
            capabilities: caps,
            rules: agent_core::rules::load_project_rules(&ws.root),
            ..AgentConfig::default()
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
    ///
    /// v0.4.36: a workspace swap now also triggers a background
    /// tool-registry rebuild (that project's `flowntier.toml`,
    /// rule files, and permission mode take over; the previous
    /// generation of MCP servers is shut down once the new
    /// registry is live).
    pub fn set_workspace(&self, root: std::path::PathBuf) {
        let mut g = self.workspace.write().expect("workspace rwlock poisoned");
        *g = Workspace::new(root, "flowntier");
        tracing::info!(
            target: "pipe_server",
            "v0.4.21 (event 000066): workspace swapped to {}",
            g.root.display()
        );
        drop(g);
        let s = self.clone();
        tokio::spawn(async move {
            tools_registry::rebuild(&s, "workspace_set").await;
        });
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
            if g.len() >= 200 {
                g.pop_front();
            }
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
        self.dispatcher
            .lock()
            .expect("dispatcher mutex poisoned")
            .clone()
    }
}
