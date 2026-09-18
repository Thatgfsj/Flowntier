//! Tool-registry rebuild pipeline — v0.4.36.
//!
//! One rebuild = parse workspace `flowntier.toml` → build the base
//! registry (builtins, permission-filtered) → MCP handshake fan-out
//! (concurrent, per-server timeout) → atomically swap the registry
//! inside [`ServerState`].
//!
//! Rebuilds are always spawned in the background:
//! - at startup, so a hung MCP server can never delay the pipe
//!   server's listen loop (debate point C), and
//! - on `POST /api/workspace/set`, so switching projects picks up
//!   that project's servers instead of silently keeping the old
//!   set (debate point B).

use std::sync::Arc;

use agent_core::mcp::{McpServer, McpTool, DEFAULT_HANDSHAKE_TIMEOUT};
use agent_core::tool::{Capabilities, ToolRegistry};

use crate::handlers::{ErrorRecord, ServerState};
use crate::workspace_toml::{PermissionMode, WorkspaceToml};

/// One server's scan outcome, for status reporting.
#[derive(Debug, Clone, serde::Serialize)]
pub struct McpServerStatus {
    /// Server name from the config.
    pub name: String,
    /// `ok` (registered), `failed` (skipped), `disabled` (config).
    pub status: String,
    /// Number of tools registered from this server.
    pub tools: usize,
    /// Error detail when `status == "failed"`.
    pub error: Option<String>,
}

/// Rebuild the tool registry for the current workspace and swap it
/// in. Never panics, never blocks indefinitely: MCP failures are
/// demoted to error records + skipped servers.
pub async fn rebuild(state: &ServerState, reason: &str) {
    let ws = state.workspace_snapshot();
    let cfg = WorkspaceToml::load(&ws.root);
    let mode = cfg.permissions.mode;

    let mut base = ToolRegistry::with_builtins();
    if mode == PermissionMode::ReadOnly {
        // Belt-and-braces with Capabilities::no_modify(): in
        // readonly mode the model never even sees write-class tools.
        base.retain_read_only();
        // MCP tools are excluded entirely (debate point A): their
        // side effects are unknowable from here, matching the
        // industry's plan-mode behavior.
    }

    let mut statuses: Vec<McpServerStatus> = Vec::new();
    let mut live_servers: Vec<Arc<McpServer>> = Vec::new();

    if mode != PermissionMode::ReadOnly {
        let servers = cfg.enabled_mcp_servers();
        if !servers.is_empty() {
            tracing::info!(
                target: "mcp",
                "rebuild({reason}): connecting {} MCP server(s)",
                servers.len()
            );
            // Fan out: each server gets its own handshake timeout so
            // one slow server doesn't eat the others' budget.
            let results = futures::future::join_all(servers.iter().map(|s| {
                let s = s.clone();
                async move {
                    let res = McpServer::connect(&s, DEFAULT_HANDSHAKE_TIMEOUT).await;
                    (s, res)
                }
            }))
            .await;

            for (cfg, res) in results {
                match res {
                    Ok((server, tools)) => {
                        let count = tools.len();
                        for def in &tools {
                            base.register(Box::new(McpTool::new(server.clone(), def)));
                        }
                        live_servers.push(server);
                        statuses.push(McpServerStatus {
                            name: cfg.name.clone(),
                            status: "ok".into(),
                            tools: count,
                            error: None,
                        });
                        tracing::info!(
                            target: "mcp",
                            "server '{}' registered {} tool(s)",
                            cfg.name,
                            count
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "mcp",
                            "server '{}' skipped: {e}",
                            cfg.name
                        );
                        state.push_error(ErrorRecord {
                            at: chrono::Utc::now().timestamp(),
                            severity: "warn".into(),
                            source: "mcp".into(),
                            summary: format!("MCP server '{}' skipped", cfg.name),
                            detail: Some(e.to_string()),
                        });
                        statuses.push(McpServerStatus {
                            name: cfg.name.clone(),
                            status: "failed".into(),
                            tools: 0,
                            error: Some(e.to_string()),
                        });
                    }
                }
            }
        }
    } else if !cfg.mcp.servers.is_empty() {
        statuses.push(McpServerStatus {
            name: "(all)".into(),
            status: "disabled".into(),
            tools: 0,
            error: Some("permissions mode = \"readonly\": MCP tools are not registered".into()),
        });
    }

    // Shutdown any servers from the previous generation that are
    // not part of this one (workspace switch / rescan).
    let live_count = live_servers.len();
    let previous = {
        let mut g = state.mcp_servers.lock().await;
        std::mem::replace(&mut *g, std::mem::take(&mut live_servers))
    };
    for old in previous {
        old.shutdown().await;
    }

    *state
        .permission_mode
        .write()
        .expect("permission rwlock poisoned") = mode;
    *state.tools.write().expect("tools rwlock poisoned") = Arc::new(base);

    let registered: usize = statuses.iter().map(|s| s.tools).sum();
    tracing::info!(
        target: "mcp",
        "rebuild({reason}): mode={mode:?}, {} mcp tool(s) from {} server(s), registry swapped",
        registered,
        live_count
    );
    // Remember the last scan for GET /api/mcp/status.
    *state.mcp_status.lock().await = statuses;
}

/// Capabilities for the current permission mode. Callers combine
/// this with [`AgentConfig`] so the mode reaches every tool
/// execution (debate point 3: a mode that doesn't reach the loop is
/// a UI decoration).
pub fn capabilities_for(mode: PermissionMode) -> Capabilities {
    match mode {
        PermissionMode::Auto => Capabilities::default(),
        PermissionMode::ReadOnly => Capabilities::no_modify(),
    }
}
