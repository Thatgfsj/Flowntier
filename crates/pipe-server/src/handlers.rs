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

/// Shared state held by the pipe server.
#[derive(Clone)]
pub struct ServerState {
    /// Event bus that all event-pipe clients subscribe to.
    pub events: broadcast::Sender<AgentEvent>,
    /// Default tool registry.
    pub tools: Arc<ToolRegistry>,
    /// CWD-style workspace root for the current pipe server run.
    pub workspace: Workspace,
}

impl ServerState {
    /// New default state. The event channel is bounded so a
    /// slow subscriber cannot grow memory unboundedly.
    pub fn new(workspace_root: std::path::PathBuf) -> Self {
        let (events, _rx) = broadcast::channel(1024);
        Self {
            events,
            tools: Arc::new(ToolRegistry::with_builtins()),
            workspace: Workspace::new(workspace_root, "aco"),
        }
    }
}

/// Register every built-in handler on `d`.
pub fn register_all(d: &mut Dispatcher, state: ServerState) {
    let state = Arc::new(state);

    // Health check.
    let s1 = state.clone();
    d.register("/api/ping", move |_body| {
        let _ = &s1;
        Box::pin(async {
            Ok((200, json!({
                "ok": true,
                "runtime": "aco-rs",
                "version": env!("CARGO_PKG_VERSION"),
            })))
        })
    });

    // List providers — minimal; the full implementation lives in
    // `crates/provider-presets` (W3 follow-up).
    let s2 = state.clone();
    d.register("/api/providers", move |_body| {
        let _ = &s2;
        Box::pin(async {
            Ok((200, json!({
                "providers": [],
                "note": "provider presets not yet wired in v0.3 W3",
            })))
        })
    });

    // Run a workflow / task envelope. This is the central
    // entry point that the Tauri client will use to talk to
    // the agent loop. For now we require the caller to pass
    // a fully-formed provider spec — the router (W3 follow-up)
    // will resolve names to providers.
    let s3 = state.clone();
    d.register("/api/run_task", move |body| {
        let state = s3.clone();
        Box::pin(async move { run_task(body, state).await })
    });
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
        .ok_or_else(|| "missing 'base_url'".to_string())?
        .to_string();
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'model'".to_string())?
        .to_string();
    // Two ways to pass the key: explicit `api_key` (preferred for
    // an embedded sidecar) or `api_key_env` (read from process env
    // — useful when the Tauri shell wants to keep secrets out of
    // the JSON payload).
    let api_key = match body.get("api_key").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => match body.get("api_key_env").and_then(|v| v.as_str()) {
            Some(var) => std::env::var(var).map_err(|_| {
                format!("api_key_env '{var}' not set in process environment")
            })?,
            None => String::new(),
        },
    };
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