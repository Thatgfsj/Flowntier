//! Agent loop: stream LLM → tool calls → execute → repeat.
//!
//! This is the v0.3 core. It owns a [`Provider`], a
//! [`ToolRegistry`], a [`ContextManager`], and emits an
//! [`AgentEvent`] stream that the rest of ACO subscribes to.

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::context::{ContextConfig, ContextManager};
use crate::event::AgentEvent;
use crate::message::{Message, ToolCall};
use crate::prompt::{system_prompt, Role};
use crate::provider::{Provider, StreamChunk};
use crate::tool::{ToolContext, ToolRegistry};
use crate::workspace::Workspace;
use crate::{AgentError, Result};

/// Static configuration for an agent run.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// Cap on loop iterations (LLM round-trips). Prevents
    /// pathological "tool → tool → tool → ..." runs.
    pub max_iterations: usize,
    /// Context window budget (see [`ContextConfig`]).
    pub context: ContextConfig,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: 50,
            context: ContextConfig::default(),
        }
    }
}

/// A single agent — the unit that runs one task envelope end-to-end.
pub struct Agent {
    role: Role,
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    workspace: Workspace,
    cfg: AgentConfig,
    ctx: ContextManager,
    cancel: CancellationToken,
}

impl Agent {
    /// Build a new agent.
    pub fn new(
        role: Role,
        provider: Arc<dyn Provider>,
        tools: Arc<ToolRegistry>,
        workspace: Workspace,
        cfg: AgentConfig,
    ) -> Self {
        let ctx = ContextManager::new(cfg.context.clone());
        Self {
            role,
            provider,
            tools,
            workspace,
            cfg,
            ctx,
            cancel: CancellationToken::new(),
        }
    }

    /// Get a handle to cancel the run.
    pub fn cancel_handle(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Run the agent against a task envelope. Returns a stream
    /// of [`AgentEvent`]s.
    pub fn run(
        self,
        task: impl Into<String>,
    ) -> mpsc::UnboundedReceiver<AgentEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        let task = task.into();
        let role = self.role;
        let display = role.display().to_string();
        let agent_id = role.id().to_string();
        let provider = self.provider;
        let tools = self.tools;
        let workspace = self.workspace;
        let cfg = self.cfg;
        let ctx_mgr = self.ctx;
        let cancel = self.cancel;

        tokio::spawn(async move {
            if let Err(e) =
                drive_loop(tx.clone(), agent_id, display, task, provider, tools, workspace, cfg, ctx_mgr, cancel)
                    .await
            {
                let _ = tx.send(AgentEvent::Done {
                    wf_id: String::new(),
                    status: format!("FAILED: {e}"),
                    summary: None,
                });
            }
        });
        rx
    }
}

#[allow(clippy::too_many_arguments)]
async fn drive_loop(
    tx: mpsc::UnboundedSender<AgentEvent>,
    agent_id: String,
    display: String,
    task: String,
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    workspace: Workspace,
    cfg: AgentConfig,
    ctx: ContextManager,
    cancel: CancellationToken,
) -> Result<()> {
    let tool_schemas = tools.schemas();
    let system = system_prompt(role_from_id(&agent_id).unwrap_or(Role::Worker), "[]");
    let _ = tool_schemas; // currently unused; system prompt doesn't inline JSON schemas yet
    let _ = system;

    // Initial history: system + user task envelope.
    let mut history: Vec<Message> = vec![
        Message::system(derive_system(&agent_id, tools.schemas())),
        Message::user(task.clone()),
    ];

    let tool_ctx = ToolContext {
        workspace: workspace.clone(),
        approved: true,
    };

    for iteration in 0..cfg.max_iterations {
        if cancel.is_cancelled() {
            let _ = tx.send(AgentEvent::Done {
                wf_id: String::new(),
                status: "ABORTED".into(),
                summary: Some("cancelled by user".into()),
            });
            return Ok(());
        }
        ctx.enforce_hard_limit(&history)?;
        let compacted = ctx.compact(history.clone());
        history = compacted;

        // ── Stream one LLM turn ───────────────────────────────
        let mut stream = provider
            .stream_chat(&history, &tools.schemas(), cancel.clone())
            .await?;
        let mut text_buf = String::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            match chunk? {
                StreamChunk::Text { delta } => {
                    if !delta.is_empty() {
                        text_buf.push_str(&delta);
                        let _ = tx.send(AgentEvent::TextDelta {
                            agent_id: agent_id.clone(),
                            agent_display: display.clone(),
                            delta,
                        });
                    }
                }
                StreamChunk::ToolUse { call } => {
                    let _ = tx.send(AgentEvent::ToolStarted {
                        agent_id: agent_id.clone(),
                        agent_display: display.clone(),
                        call: call.clone(),
                    });
                    tool_calls.push(call);
                }
                StreamChunk::Done { .. } => break,
            }
        }

        // Push the assistant turn into history.
        history.push(Message::assistant(text_buf.clone(), tool_calls.clone()));

        if tool_calls.is_empty() {
            let _ = tx.send(AgentEvent::Done {
                wf_id: String::new(),
                status: "DONE".into(),
                summary: Some(text_buf),
            });
            return Ok(());
        }

        // ── Execute each tool call ────────────────────────────
        for call in tool_calls {
            let started = Instant::now();
            let result = tools.execute(&call.name, call.args.clone(), &tool_ctx).await;
            let (content, is_error) = match result {
                Ok(o) => (o.content, o.is_error),
                Err(e) => (format!("tool error: {e}"), true),
            };
            let preview: String = content.chars().take(200).collect();
            let _ = tx.send(AgentEvent::ToolFinished {
                agent_id: agent_id.clone(),
                agent_display: display.clone(),
                tool_call_id: call.id.clone(),
                preview,
                is_error,
                elapsed_ms: started.elapsed().as_millis() as u64,
            });
            history.push(Message::tool(call.id, content));
        }

        let _ = iteration; // silence unused
    }

    Err(AgentError::MaxIterationsReached(cfg.max_iterations))
}

fn derive_system(agent_id: &str, _schemas: Vec<serde_json::Value>) -> String {
    let role = role_from_id(agent_id).unwrap_or(Role::Worker);
    let schemas_json = serde_json::to_string(&_schemas).unwrap_or_else(|_| "[]".into());
    system_prompt(role, &schemas_json)
}

fn role_from_id(id: &str) -> Option<Role> {
    Some(match id {
        "agent:chief" => Role::Chief,
        "agent:critic:a" => Role::BugHunter,
        "agent:critic:b" => Role::Reviewer,
        "agent:planner" => Role::Planner,
        "agent:worker" => Role::Worker,
        "agent:reporter" => Role::Reporter,
        _ => return None,
    })
}