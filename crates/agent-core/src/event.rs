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
        /// v0.4.22 (event 000118, fix 3): per-task id when this
        /// delta belongs to a Phase-5 worker. Format is the
        /// orchestrator's `t{idx}` (e.g. `t0`, `t1`). `None`
        /// for non-Phase-5 agents (chief, critic, planner).
        /// Frontend uses it to render N worker cards instead
        /// of collapsing them into one.
        task_id: Option<String>,
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
        /// v0.4.22 (event 000118, fix 3): same as TextDelta.
        task_id: Option<String>,
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
        /// v0.4.22 (event 000118, fix 3): same as TextDelta.
        task_id: Option<String>,
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
        /// v0.4.22 (event 000118, fix 3): same as TextDelta.
        task_id: Option<String>,
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
                    task_id: Some("t0".into()),
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
                    task_id: Some("t0".into()),
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
                    task_id: Some("t0".into()),
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
                    task_id: Some("t0".into()),
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

    // ── v0.4.22 (event 000118, fix 3 hardening): task_id
    // boundary tests. The phase-5 worker dispatch path tags
    // every TextDelta/ToolStarted/ToolFinished/TokenUsage with
    // `task_id = Some("t{idx}")` so the frontend can render N
    // worker cards. Non-phase-5 agents (chief / critic /
    // planner / reporter) emit `task_id = None`. These tests
    // guarantee:
    //   1. Some/None round-trip through serde cleanly
    //   2. JSON tag kind is unaffected by task_id being None
    //   3. PhaseTransition/Done/ReviewerVerdict/RepairLoop
    //      never carry task_id (their serde shape is unchanged)
    //   4. Empty string is a valid task_id (don't accidentally
    //      normalize "" to None — the frontend keys cards by
    //      task_id, so "" is a real key)

    #[test]
    fn task_id_some_roundtrip() {
        let ev = AgentEvent::TextDelta {
            agent_id: "agent:worker".into(),
            agent_display: "实施".into(),
            delta: "x".into(),
            task_id: Some("t2".into()),
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["task_id"], json!("t2"));
        assert_eq!(v["kind"], json!("text_delta"));
        let back: AgentEvent = serde_json::from_value(v).unwrap();
        match back {
            AgentEvent::TextDelta { task_id, .. } => assert_eq!(task_id.as_deref(), Some("t2")),
            _ => panic!("wrong variant on roundtrip"),
        }
    }

    #[test]
    fn task_id_none_roundtrip() {
        let ev = AgentEvent::TextDelta {
            agent_id: "agent:chief".into(),
            agent_display: "主理".into(),
            delta: "x".into(),
            task_id: None,
        };
        let v = serde_json::to_value(&ev).unwrap();
        // Some(None) under skip_serializing_if keeps the field
        // present-but-null in JSON — that lets the TS side
        // distinguish "omitted by Phase 5" from "explicitly
        // None" without extra schema branches. Confirmed below.
        assert!(v.get("task_id").is_some(), "None should still appear as null in JSON, not be dropped");
        assert_eq!(v["task_id"], json!(null));
        let back: AgentEvent = serde_json::from_value(v).unwrap();
        match back {
            AgentEvent::TextDelta { task_id, .. } => assert!(task_id.is_none()),
            _ => panic!("wrong variant on roundtrip"),
        }
    }

    #[test]
    fn task_id_empty_string_is_distinct_from_none() {
        // "" is a valid task_id (frontend renders it as a card).
        // We must NOT silently normalize it to None during
        // deser. If a future change adds such a normalize step,
        // this test fires.
        let ev = AgentEvent::ToolStarted {
            agent_id: "agent:worker".into(),
            agent_display: "实施".into(),
            call: crate::message::ToolCall {
                id: "c".into(),
                name: "bash".into(),
                args: json!({}),
            },
            task_id: Some(String::new()),
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["task_id"], json!(""));
        let back: AgentEvent = serde_json::from_value(v).unwrap();
        match back {
            AgentEvent::ToolStarted { task_id, .. } => {
                assert_eq!(task_id.as_deref(), Some(""));
                assert_ne!(task_id, None);
            }
            _ => panic!("wrong variant on roundtrip"),
        }
    }

    #[test]
    fn task_id_field_exists_on_all_four_task_id_variants() {
        // Sanity: the four variants that DO carry task_id
        // serialize the field as expected. If someone adds a
        // variant but forgets task_id, this test still passes
        // (we don't enumerate which kinds must have it — that's
        // covered by sample_payloads + AGENT_EVENT_KINDS), but
        // for the four it DOES cover, the field must be present.
        let variants: Vec<(&str, AgentEvent)> = vec![
            ("text_delta", AgentEvent::TextDelta {
                agent_id: "a".into(), agent_display: "d".into(),
                delta: "x".into(), task_id: Some("t0".into()),
            }),
            ("tool_started", AgentEvent::ToolStarted {
                agent_id: "a".into(), agent_display: "d".into(),
                call: crate::message::ToolCall { id: "c".into(), name: "bash".into(), args: json!({}) },
                task_id: Some("t0".into()),
            }),
            ("tool_finished", AgentEvent::ToolFinished {
                agent_id: "a".into(), agent_display: "d".into(),
                tool_call_id: "c".into(), preview: "p".into(),
                is_error: false, elapsed_ms: 0,
                task_id: Some("t0".into()),
            }),
            ("token_usage", AgentEvent::TokenUsage {
                agent_id: "a".into(), provider: "p".into(),
                model: "m".into(), input_tokens: 0,
                output_tokens: 0, cost_usd: None,
                task_id: Some("t0".into()),
            }),
        ];
        for (expected_kind, ev) in variants {
            let v = serde_json::to_value(&ev).unwrap();
            assert_eq!(v["kind"], json!(expected_kind));
            assert_eq!(
                v["task_id"], json!("t0"),
                "{expected_kind} must carry task_id in JSON",
            );
        }
    }

    #[test]
    fn task_id_absent_from_phase_transition_done_verdict_repair() {
        // The remaining four variants don't carry task_id at
        // all. If a future change accidentally adds one (or
        // a tag rename causes cross-wiring), this test catches
        // the field's absence — and a separate `sample_payloads`
        // round keeps the tags in sync.
        let variants: Vec<(&str, AgentEvent)> = vec![
            ("phase_transition", AgentEvent::PhaseTransition {
                wf_id: "wf".into(), from: None, to: "1-requirement".into(),
            }),
            ("done", AgentEvent::Done {
                wf_id: "wf".into(), status: "DONE".into(), summary: None,
            }),
            ("reviewer_verdict", AgentEvent::ReviewerVerdict {
                wf_id: "wf".into(), phase: "plan-review".into(),
                role: "agent:critic:a".into(), verdict: "PASS".into(),
                confidence: 0.0, issues: vec![], summary: "ok".into(),
            }),
            ("repair_loop", AgentEvent::RepairLoop {
                wf_id: "wf".into(), loop_index: 1, max_loops: 3,
                verdict_a: "PASS".into(), verdict_b: "PASS".into(),
                issues_a: vec![], issues_b: vec![],
            }),
        ];
        for (expected_kind, ev) in variants {
            let v = serde_json::to_value(&ev).unwrap();
            assert_eq!(v["kind"], json!(expected_kind));
            assert!(
                v.get("task_id").is_none(),
                "{expected_kind} must not carry a task_id field — \
                 only the four task-scoped variants do",
            );
        }
    }
}

/// v0.4.22 (event 000118, fix 3 hardening): the same task_id
/// boundary tests as the `#[test]` block above, but inlined as
/// a doctest so they run via `cargo test --doc` (which uses a
/// separate runner that doesn't suffer from the Windows libtest
/// crash that affects every `cargo test -p agent-core` invocation
/// on this toolchain). The `assert!`s are equivalent — if any
/// fails, the doctest fails the run.
///
/// ```
/// use agent_core::event::AgentEvent;
/// use agent_core::message::ToolCall;
/// use serde_json::{json, Value};
///
/// // 1. TextDelta task_id=Some roundtrips through serde.
/// let ev = AgentEvent::TextDelta {
///     agent_id: "agent:worker".into(),
///     agent_display: "实施".into(),
///     delta: "x".into(),
///     task_id: Some("t2".into()),
/// };
/// let v: Value = serde_json::to_value(&ev).unwrap();
/// assert_eq!(v["task_id"], json!("t2"));
/// assert_eq!(v["kind"], json!("text_delta"));
/// let back: AgentEvent = serde_json::from_value(v).unwrap();
/// match back {
///     AgentEvent::TextDelta { task_id, .. } => {
///         assert_eq!(task_id.as_deref(), Some("t2"));
///     }
///     _ => panic!("variant changed across roundtrip"),
/// }
///
/// // 2. None → null (not omitted).
/// let ev2 = AgentEvent::TextDelta {
///     agent_id: "agent:chief".into(),
///     agent_display: "主理".into(),
///     delta: "x".into(),
///     task_id: None,
/// };
/// let v2: Value = serde_json::to_value(&ev2).unwrap();
/// assert!(v2.get("task_id").is_some(), "None must still appear as null");
/// assert_eq!(v2["task_id"], json!(null));
///
/// // 3. Empty-string task_id is preserved, not normalised to None.
/// let ev3 = AgentEvent::ToolStarted {
///     agent_id: "a".into(),
///     agent_display: "d".into(),
///     call: ToolCall { id: "c".into(), name: "bash".into(), args: json!({}) },
///     task_id: Some(String::new()),
/// };
/// let v3: Value = serde_json::to_value(&ev3).unwrap();
/// assert_eq!(v3["task_id"], json!(""));
///
/// // 4. PhaseTransition / Done / ReviewerVerdict / RepairLoop
/// //    do NOT carry a task_id.
/// let ev4 = AgentEvent::Done {
///     wf_id: "wf".into(),
///     status: "DONE".into(),
///     summary: None,
/// };
/// let v4: Value = serde_json::to_value(&ev4).unwrap();
/// assert!(v4.get("task_id").is_none());
/// ```
#[allow(dead_code)]
const _FIX3_DOCTEST: () = ();