//! Events emitted by the agent loop.
//!
//! These are published to the rest of Flowntier (Tauri webview, event
//! log, etc.) so the UI can render the agent's actions as they
//! happen.
//!
//! ## Cross-language schema contract (event 000116)
//!
//! The wire protocol is `serde(tag = "kind", rename_all = "snake_case")`
//! over JSON, carried on the named pipes
//! `\\.\pipe\flowntier_runtime` (RPC) and
//! `\\.\pipe\flowntier_runtime_events` (events). The TypeScript
//! webview consumes this stream as `WfEvent` (see
//! `packages/shared/src/events.ts`) — the two unions MUST stay
//! in sync. To make drift detectable, [`AGENT_EVENT_KINDS`]
//! below is a single source of truth for every `kind` tag the
//! runtime can emit, and
//! `tests/event_kind_set_matches_actual_serde_tags` cross-checks
//! it against the real serde output. When you add a variant:
//!
//! 1. Add it to `AgentEvent`.
//! 2. Add the snake_case tag to [`AGENT_EVENT_KINDS`].
//! 3. Mirror the variant + `kind` literal in `events.ts`'s
//!    `WfEvent` union + add the tag to `WF_EVENT_KINDS`.
//! 4. Run `cargo test -p agent-core` and the vitest suite —
//!    both must pass.
//!
//! Drift between Rust and TS without updating both sides is the
//! bug class this contract guards against; it bit us at least
//! three times pre-event-000116 (e.g. `WorkflowCompleteEvent`
//! was declared in TS but never emitted by any Rust variant).

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
    /// v0.4.22 (event 000113): emitted once each time the
    /// orchestrator enters Phase 7 (repair) and decides to
    /// re-run Phase 5 (develop). The webview's repair panel
    /// listens for this to render "修复循环 N / max" instead of
    /// guessing from repeated `5-develop-*` rows. Also carries
    /// both critics' verdicts + their structured issues so the
    /// panel can show "what to fix" without re-parsing
    /// `ReviewerVerdict` history.
    RepairLoop {
        wf_id: String,
        /// 1-based index of this repair round.
        loop_index: u32,
        /// Cap configured at workflow start (`max_repair_loops`).
        max_loops: u32,
        /// Verdict token from critic A (`"PASS"` / `"REPAIR"` /
        /// `"REWRITE"` / `"UNKNOWN"`).
        verdict_a: String,
        /// Verdict token from critic B.
        verdict_b: String,
        /// Structured issues from critic A's JSON block (empty
        /// when the critic didn't emit one — see event 000115).
        issues_a: Vec<String>,
        /// Structured issues from critic B's JSON block.
        issues_b: Vec<String>,
    },
}

/// v0.4.22 (event 000116): single source of truth for every
/// `kind` tag the runtime can emit. Each entry is the
/// `snake_case` form of the corresponding `AgentEvent` variant
/// (per `#[serde(tag = "kind", rename_all = "snake_case")]`).
///
/// The TypeScript webview's `WfEvent` union mirrors this set;
/// see `packages/shared/src/events.ts` (`WF_EVENT_KINDS`) and
/// the matching vitest for the cross-language check.
///
/// Order does not matter — `tests::event_kind_set_matches_…`
/// sorts both sides before comparing.
pub const AGENT_EVENT_KINDS: &[&str] = &[
    "text_delta",
    "tool_started",
    "tool_finished",
    "phase_transition",
    "token_usage",
    "done",
    "reviewer_verdict",
    "repair_loop",
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    /// One representative payload per AgentEvent variant. When
    /// adding a variant, add a row here too — the loop below
    /// will then assert both that the variant exists AND that
    /// its serde tag matches the entry in `AGENT_EVENT_KINDS`.
    fn sample_payloads() -> Vec<(&'static str, AgentEvent)> {
        vec![
            (
                "text_delta",
                AgentEvent::TextDelta {
                    agent_id: "agent:worker".into(),
                    agent_display: "实施".into(),
                    delta: "hello".into(),
                },
            ),
            (
                "tool_started",
                AgentEvent::ToolStarted {
                    agent_id: "agent:worker".into(),
                    agent_display: "实施".into(),
                    call: crate::message::ToolCall {
                        id: "call_1".into(),
                        name: "read".into(),
                        args: json!({"path": "/x"}),
                    },
                },
            ),
            (
                "tool_finished",
                AgentEvent::ToolFinished {
                    agent_id: "agent:worker".into(),
                    agent_display: "实施".into(),
                    tool_call_id: "call_1".into(),
                    preview: "ok".into(),
                    is_error: false,
                    elapsed_ms: 42,
                },
            ),
            (
                "phase_transition",
                AgentEvent::PhaseTransition {
                    wf_id: "wf_x".into(),
                    from: None,
                    to: "1-requirement".into(),
                },
            ),
            (
                "token_usage",
                AgentEvent::TokenUsage {
                    agent_id: "agent:worker".into(),
                    provider: "anthropic".into(),
                    model: "claude-opus-4-8".into(),
                    input_tokens: 100,
                    output_tokens: 50,
                    cost_usd: None,
                },
            ),
            (
                "done",
                AgentEvent::Done {
                    wf_id: "wf_x".into(),
                    status: "DONE".into(),
                    summary: None,
                },
            ),
            (
                "reviewer_verdict",
                AgentEvent::ReviewerVerdict {
                    wf_id: "wf_x".into(),
                    phase: "final-review".into(),
                    role: "agent:critic:a".into(),
                    verdict: "PASS".into(),
                    confidence: 0.0,
                    issues: vec![],
                    summary: "ok".into(),
                },
            ),
            (
                "repair_loop",
                AgentEvent::RepairLoop {
                    wf_id: "wf_x".into(),
                    loop_index: 1,
                    max_loops: 3,
                    verdict_a: "PASS".into(),
                    verdict_b: "REPAIR".into(),
                    issues_a: vec![],
                    issues_b: vec![],
                },
            ),
        ]
    }

    /// v0.4.22 (event 000116): every variant in `AgentEvent`
    /// serialises to a JSON object whose `kind` tag appears in
    /// [`AGENT_EVENT_KINDS`]. If you add a variant and forget
    /// to add its tag here, this test fails — exactly the
    /// failure mode we want when TS is about to silently lose
    /// the new event.
    #[test]
    fn event_kind_set_matches_actual_serde_tags() {
        let declared: std::collections::BTreeSet<&str> =
            AGENT_EVENT_KINDS.iter().copied().collect();
        let actual: std::collections::BTreeSet<&'static str> = sample_payloads()
            .iter()
            .map(|(kind, ev)| {
                let v: Value = serde_json::to_value(ev)
                    .expect("AgentEvent should serialize");
                let tag = v
                    .get("kind")
                    .and_then(|k| k.as_str())
                    .unwrap_or_else(|| panic!("no kind tag for variant tagged {kind}"))
                    .to_owned();
                assert_eq!(tag, *kind, "declared kind != serde tag");
                // Leak into 'static — `BTreeSet<&'static str>`
                // requires 'static; the strings come from a
                // temporary `Value`. The set is local to this
                // test, so the leak is bounded to the test's
                // lifetime and harmless in practice.
                Box::leak(tag.into_boxed_str()) as &'static str
            })
            .collect();
        assert_eq!(
            declared, actual,
            "AGENT_EVENT_KINDS drifted from actual serde tags — \
             update one to match the other (event 000116 contract)"
        );
    }

    /// v0.4.22 (event 000116): every entry in `AGENT_EVENT_KINDS`
    /// has a matching sample payload. Catches the case where a
    /// kind is declared but no variant is exercised (e.g. someone
    /// types the wrong snake_case string).
    #[test]
    fn every_declared_kind_has_a_sample_variant() {
        let declared: std::collections::BTreeSet<&str> =
            AGENT_EVENT_KINDS.iter().copied().collect();
        let covered: std::collections::BTreeSet<&str> = sample_payloads()
            .iter()
            .map(|(kind, _)| *kind)
            .collect();
        assert_eq!(
            declared, covered,
            "every AGENT_EVENT_KINDS entry needs a sample_payloads() row"
        );
    }
}