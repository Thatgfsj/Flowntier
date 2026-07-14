//! Agent loop: stream LLM → tool calls → execute → repeat.
//!
//! This is the v0.3 core. It owns a [`Provider`], a
//! [`ToolRegistry`], a [`ContextManager`], and emits an
//! [`AgentEvent`] stream that the rest of Flowntier subscribes to.

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
    /// If the same `(tool_name, args)` combination fails this
    /// many times in a row, the loop emits `Done { status:
    /// "ABORTED_REPEAT" }` and exits. 0 = disabled.
    pub repeat_abort_after: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_iterations: 50,
            context: ContextConfig::default(),
            repeat_abort_after: 3,
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
                // v0.4.22 (event 000081): include the error
                // message in the summary so the chairman sees
                // the failure cause rather than an empty
                // response. Previously we just emitted
                // `summary: None` here, which downstream caused
                // `summary_len: 0` in the runtime log and an
                // empty Status field in the UI. Surfacing the
                // error here makes provider failures (401 bad
                // key, 429 rate limit, malformed stream, etc.)
                // visible to the chairman.
                let err_msg = format!("{e}");
                let _ = tx.send(AgentEvent::Done {
                    wf_id: String::new(),
                    status: format!("FAILED: {e}"),
                    summary: Some(err_msg),
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

    // Repeat-failure tracking. We hash (tool_name, normalised args)
    // and count consecutive failures of the same key. Reset on
    // success or on a *different* failure key.
    let mut last_failure_key: Option<String> = None;
    let mut repeat_count: usize = 0;

    // Snapshot tool schemas once; they don't change between
    // iterations. (Each `tools.schemas()` call sorts + serialises
    // every tool's JSON schema, which adds up over many rounds.)
    let tool_schemas_cached = tools.schemas();

    // Initial history: system + user task envelope.
    let mut history: Vec<Message> = vec![
        Message::system(derive_system(&agent_id, tools.schemas())),
        Message::user(task.clone()),
    ];

    let tool_ctx = ToolContext {
        workspace: workspace.clone(),
        approved: true,
        capabilities: crate::tool::Capabilities::default(),
        cancel: Some(cancel.clone()),
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
            .stream_chat(&history, &tool_schemas_cached, cancel.clone())
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
            history.push(Message::tool(call.id.clone(), content.clone()));

            // Repeat-failure detection.
            if is_error && cfg.repeat_abort_after > 0 {
                let key = format!(
                    "{}|{}",
                    call.name,
                    stable_hash(&call.args)
                );
                if last_failure_key.as_deref() == Some(&key) {
                    repeat_count += 1;
                } else {
                    last_failure_key = Some(key);
                    repeat_count = 1;
                }
                if repeat_count >= cfg.repeat_abort_after {
                    let msg = format!(
                        "aborted: {tool} failed {n} times in a row with the same arguments",
                        tool = call.name,
                        n = cfg.repeat_abort_after
                    );
                    let _ = tx.send(AgentEvent::Done {
                        wf_id: String::new(),
                        status: "ABORTED_REPEAT".into(),
                        summary: Some(msg),
                    });
                    return Ok(());
                }
            } else {
                last_failure_key = None;
                repeat_count = 0;
            }
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
/// Stable, order-independent hash of a JSON args object.
///
/// Used to compare two tool-call argument sets: `{"a":1,"b":2}`
/// and `{"b":2,"a":1}` should produce the same key. Falls back to
/// the raw `to_string()` form if the value is not an object, which
/// is fine for arrays and primitives.
fn stable_hash(v: &serde_json::Value) -> String {
    use std::collections::BTreeMap;
    fn normalise(v: &serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Object(m) => {
                let sorted: BTreeMap<String, serde_json::Value> = m
                    .iter()
                    .map(|(k, val)| (k.clone(), normalise(val)))
                    .collect();
                serde_json::Value::Object(sorted.into_iter().collect())
            }
            other => other.clone(),
        }
    }
    normalise(v).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_hash_is_key_order_independent() {
        let a = serde_json::json!({"b": 2, "a": 1});
        let b = serde_json::json!({"a": 1, "b": 2});
        assert_eq!(stable_hash(&a), stable_hash(&b));
    }

    #[test]
    fn stable_hash_differs_on_value() {
        let a = serde_json::json!({"x": 1});
        let b = serde_json::json!({"x": 2});
        assert_ne!(stable_hash(&a), stable_hash(&b));
    }
}
