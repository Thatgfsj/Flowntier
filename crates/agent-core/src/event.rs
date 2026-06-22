//! Events emitted by the agent loop.
//!
//! These are published to the rest of ACO (Tauri webview, event
//! log, etc.) so the UI can render the agent's actions as they
//! happen.

use serde::{Deserialize, Serialize};

use crate::message::ToolCall;

/// A single event in the agent's life cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEvent {
    /// Assistant streamed a fragment of text. Concatenated in
    /// arrival order, this reconstructs the full message.
    TextDelta {
        /// Logical role id (e.g. `agent:chief`).
        agent_id: String,
        /// Display role name (e.g. `首席`, `工匠`).
        agent_display: String,
        /// Fragment of text.
        delta: String,
    },

    /// Assistant emitted one or more tool calls.
    ToolStarted {
        /// Logical role id.
        agent_id: String,
        /// Display role name.
        agent_display: String,
        /// The call. `args` may be partial during streaming;
        /// the UI should re-render on each update.
        call: ToolCall,
    },

    /// Tool execution finished.
    ToolFinished {
        /// Logical role id.
        agent_id: String,
        /// Display role name.
        agent_display: String,
        /// The id of the call this result corresponds to.
        tool_call_id: String,
        /// Short preview for the timeline (first ~200 chars).
        preview: String,
        /// Whether the tool returned an error.
        is_error: bool,
        /// Wall-clock duration in milliseconds.
        elapsed_ms: u64,
    },

    /// The loop transitioned between high-level phases.
    /// Useful for the "milestone" UI bar.
    PhaseTransition {
        /// Workflow id.
        wf_id: String,
        /// Previous phase name (None on the very first).
        from: Option<String>,
        /// New phase name.
        to: String,
    },

    /// Token usage report after a provider call completes.
    TokenUsage {
        /// Logical role id.
        agent_id: String,
        /// Provider id (e.g. `anthropic`, `openai_compat`).
        provider: String,
        /// Model id as reported by the provider.
        model: String,
        /// Tokens consumed by the prompt.
        input_tokens: u64,
        /// Tokens generated in the completion.
        output_tokens: u64,
        /// USD cost if computable; `None` for local models.
        cost_usd: Option<f64>,
    },

    /// Final event of an agent run.
    Done {
        /// Workflow id.
        wf_id: String,
        /// Terminal status string (e.g. `DONE`, `FAILED`).
        status: String,
        /// Final summary, if any.
        summary: Option<String>,
    },
}