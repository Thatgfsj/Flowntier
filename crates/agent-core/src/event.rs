//! Events emitted by the agent loop.
//!
//! These are published to the rest of Flowntier (Tauri webview, event
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
        /// Display role name (e.g. `主理`, `实施`).
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

    /// v0.4.22 (event 000112): reviewer (or bug-hunter) actor
    /// finished a verdict pass. Orchestrator emits this once per
    /// critic per review phase (plan-review + final-review), so
    /// a single workflow produces up to four verdict events —
    /// two per review phase, one per critic (agent:critic:a, b).
    /// `verdict` is the prose-scanned PASS/REPAIR/REWRITE token
    /// (see `verdict_of` in pipe-server/orchestrator.rs); the
    /// front-end treats an event with `phase == "final-review"`
    /// as the binding verdict and replaces the placeholder
    /// "审核员 B — 架构审查" card with this verdict payload.
    /// `confidence` is currently always 0.0; the actor loop does
    /// not yet emit a structured score. `issues` is non-empty
    /// only after event 000115 (JSON-structured reviewer output).
    ReviewerVerdict {
        /// Workflow id.
        wf_id: String,
        /// Review phase: `"plan-review"` or `"final-review"`.
        phase: String,
        /// Critic role id: `"agent:critic:a"` (BugHunter) or
        /// `"agent:critic:b"` (Reviewer).
        role: String,
        /// Verdict token: `"PASS"`, `"REPAIR"`, or `"REWRITE"`.
        verdict: String,
        /// Confidence 0.0..=1.0 (currently 0.0 — placeholder until
        /// event 000115 gives reviewers a structured JSON prompt).
        confidence: f64,
        /// Per-issue notes (currently empty — placeholder).
        issues: Vec<String>,
        /// One-sentence reviewer rationale (first sentence of the
        /// critic's text, truncated to 200 chars).
        summary: String,
    },
}