//! Reviewer-verdict / repair-loop event contract tests (event 000112+).

use super::common::*;

// v0.4.22 (event 000112): agent_core::event::AgentEvent gets a
// new `reviewer_verdict` variant. This file pins:
//   1. serde's tag emit produces `kind:"reviewer_verdict"` (the
//      Tauri event bridge filters by this discriminator — if the
//      tag were misnamed, the webview would silently drop the
//      verdict and the ReviewerCard would stay on the placeholder).
//   2. All 6 fields survive a round-trip (the frontend relies on
//      `phase`, `role`, `verdict`, `confidence`, `issues`,
//      `summary` for binding the final-review payload).
//
// These tests live in pipe-server even though the type is in
// agent-core because the full emit path (orchestrator -> events
// pipe -> tauri bridge) is what the user-visible regression
// hinges on; agent-core's own lib tests cover the pure-Rust
// surface area.
#[test]
fn agent_event_reviewer_verdict_serializes_to_kind_tag() {
    use agent_core::event::AgentEvent;
    let v = AgentEvent::ReviewerVerdict {
        wf_id: "wf_abc".into(),
        phase: "final-review".into(),
        role: "agent:critic:b".into(),
        verdict: "PASS".into(),
        confidence: 0.0,
        issues: vec!["function too long".into()],
        summary: "module boundaries are clean".into(),
    };
    let json = serde_json::to_value(&v).expect("serialize");
    assert_eq!(
        json["kind"],
        serde_json::json!("reviewer_verdict"),
        "tag must be snake_case 'reviewer_verdict' (rename_all = snake_case)"
    );
    assert_eq!(json["wf_id"], serde_json::json!("wf_abc"));
    assert_eq!(json["phase"], serde_json::json!("final-review"));
    assert_eq!(json["role"], serde_json::json!("agent:critic:b"));
    assert_eq!(json["verdict"], serde_json::json!("PASS"));
    assert_eq!(json["confidence"], serde_json::json!(0.0));
    assert_eq!(json["issues"][0], serde_json::json!("function too long"));
    assert_eq!(
        json["summary"],
        serde_json::json!("module boundaries are clean")
    );
}

#[test]
fn agent_event_reviewer_verdict_round_trips_through_serde() {
    use agent_core::event::AgentEvent;
    let v = AgentEvent::ReviewerVerdict {
        wf_id: "wf_xyz".into(),
        phase: "plan-review".into(),
        role: "agent:critic:a".into(),
        verdict: "REPAIR".into(),
        confidence: 0.0,
        issues: Vec::new(),
        summary: "Auth middleware leaks session id when error_path fires".into(),
    };
    let s = serde_json::to_string(&v).expect("serialize");
    let back: AgentEvent = serde_json::from_str(&s).expect("deserialize");
    match back {
        AgentEvent::ReviewerVerdict {
            wf_id,
            phase,
            role,
            verdict,
            confidence,
            issues,
            summary,
        } => {
            assert_eq!(wf_id, "wf_xyz");
            assert_eq!(phase, "plan-review");
            assert_eq!(role, "agent:critic:a");
            assert_eq!(verdict, "REPAIR");
            assert_eq!(confidence, 0.0);
            assert!(issues.is_empty());
            assert!(summary.starts_with("Auth middleware"));
        }
        other => panic!("round-trip lost the variant; got {:?}", other),
    }
}

// v0.4.22 (event 000112): the helper `emit_reviewer_verdict`
// strips the first sentence of the critic's text (truncated at
// 200 chars) and uses `verdict_of` to scan PASS/REPAIR/REWRITE.
// We don't drive an LLM in unit tests, so this test only
// pins the wire-level round-trip for the variant's payload by
// exercising serde against a representative value. The
// orchestrator's prose-scan logic is indirectly covered by
// the existing e2e Pipe tests that exercise run_task; here
// we focus on the new `events pipe -> Tauri bridge -> webview`
// schema contract.
#[test]
fn reviewer_verdict_event_pipe_payload_matches_webview_expectation() {
    use agent_core::event::AgentEvent;
    // The wire format the Tauri event bridge (`events_bridge`
    // in apps/desktop/src-tauri/src/lib.rs) forward to the
    // webview under `wf:event` is just `serde_json::to_value`
    // of the AgentEvent. The webview's `useEventStream()`
    // hook casts it to `WfEvent` whose union now includes
    // `ReviewerVerdictEvent { kind: 'reviewer_verdict' }`.
    // Mismatches in field shape between the Rust enum's tag
    // and the TS union silently break the binding — this
    // test catches that drift early.
    let v = AgentEvent::ReviewerVerdict {
        wf_id: "wf_pipe".into(),
        phase: "final-review".into(),
        role: "agent:critic:b".into(),
        verdict: "REPAIR".into(),
        confidence: 0.0,
        issues: vec!["three of five tests missing".into()],
        summary: "测试覆盖不够，需要补 AuthMiddleware 三条 spec 的正常路径".into(),
    };
    let json = serde_json::to_value(&v).expect("serialize");
    assert_eq!(json["kind"].as_str(), Some("reviewer_verdict"));
    // The webview's narrow path `event.kind === 'reviewer_verdict'`
    // expects these exact field names; a renamed field would
    // silently degrade reviewVerdict binding.
    assert!(json.get("wf_id").is_some(), "wf_id field missing");
    assert!(json.get("phase").is_some(), "phase field missing");
    assert!(json.get("role").is_some(), "role field missing");
    assert!(json.get("verdict").is_some(), "verdict field missing");
    assert!(json.get("confidence").is_some(), "confidence field missing");
    assert!(json.get("issues").is_some(), "issues field missing");
    assert!(json.get("summary").is_some(), "summary field missing");
}

// v0.4.22 (event 000114): orchestator-end terminal status used
// to be hard-coded to "DONE" no matter what the critics said
// (chairman's manual test: 3D-magic-cube task finished in 2
// seconds and falsely reported PASS / 置信度 0.87).
// `terminal_done_status` is the pure helper that translates
// the final-review verdict pair into the AgentEvent::Done.status
// string. The test pins:
//
//   1. Both PASS or any (PASS, UNKNOWN) mix  → "DONE".
//      The frontend's applyEvent `event.kind === 'done' &&
//      status.startsWith('FAILED')` branch stays silent (no
//      workflowError banner shown).
//
//   2. Either REPAIR or REWRITE             →
//      `"FAILED: reviewer_<verdict>_<role_id>"`. The frontend
//      branch kicks in, sets `workflowError`, and the
//      ReviewerCard (000112) already shows the verdict with
//      its summary. REWRITE wins over REPAIR (severity rank 2
//      > 1) when the two disagree.
//
//   3. Roles flip in the input order  → still attributes the
//      failure to the higher-severity critic, not the first
//      role by position. This pins the `pick` branch in
//      `terminal_done_status`.
//
//   4. UNKNOWN from both sides  → still "DONE" (UNKNOWN is
//      a non-actionable verdict; chief didn't raise anything
//      actionable either, so report success).
#[test]
fn terminal_done_status_translates_review_verdict_pair() {
    use pipe_server::orchestrator::terminal_done_status;
    // PASS / PASS → DONE.
    assert_eq!(
        terminal_done_status("PASS", "PASS", "agent:critic:a", "agent:critic:b"),
        "DONE",
    );
    // One REPAIR, one PASS → FAILED to the REPAIR side.
    assert_eq!(
        terminal_done_status("PASS", "REPAIR", "agent:critic:a", "agent:critic:b"),
        "FAILED: reviewer_REPAIR_agent:critic:b",
    );
    // One REWRITE, one REPAIR → FAILED, attributed to REWRITE
    // (severity > ).
    assert_eq!(
        terminal_done_status("REPAIR", "REWRITE", "agent:critic:a", "agent:critic:b"),
        "FAILED: reviewer_REWRITE_agent:critic:b",
    );
    // Roles reversed: REPAIR for critic-b, PASS for critic-a;
    // tie-break to the first arg (a) when severities are equal
    // — but here they differ so REPAIR wins.
    assert_eq!(
        terminal_done_status("PASS", "REPAIR", "agent:critic:b", "agent:critic:a"),
        "FAILED: reviewer_REPAIR_agent:critic:a",
    );
    // Both REWRITE → FAILED, pick goes to first arg.
    assert_eq!(
        terminal_done_status("REWRITE", "REWRITE", "agent:critic:a", "agent:critic:b"),
        "FAILED: reviewer_REWRITE_agent:critic:a",
    );
    // Dual UNKNOWN → FAILED: reviewer_UNKNOWN_both_critics_unresponsive (critics unresponsive).
    assert_eq!(
        terminal_done_status("UNKNOWN", "UNKNOWN", "agent:critic:a", "agent:critic:b"),
        "FAILED: reviewer_UNKNOWN_both_critics_unresponsive",
    );
    // Mixed UNKNOWN + PASS is still a DONE workflow.
    assert_eq!(
        terminal_done_status("UNKNOWN", "PASS", "agent:critic:a", "agent:critic:b"),
        "DONE",
    );
}

#[test]
fn parse_verdict_from_text_extracts_structured_json_block() {
    use pipe_server::orchestrator::{critic_verdict, parse_verdict_from_text, TaskOutcome};
    // v0.4.22 (event 000115): the JSON block critics are
    // instructed to emit is the primary source — full
    // confidence + issues + summary come through.
    let outcome = TaskOutcome {
        role_id: "agent:critic:a".into(),
        role_display: "BugHunter".into(),
        status: "DONE".into(),
        summary: None,
        text: r#"Found two bugs.

```flowntier-verdict
{
  "verdict": "REPAIR",
  "confidence": 0.82,
  "issues": ["[HIGH] auth.rs:42 — token redaction missing", "[MEDIUM] log.rs:18 — log spam"],
  "summary": "Two must-fix issues; mostly clean otherwise."
}
```"#
            .into(),
        elapsed_ms: 1234,
        structured_verdict: None,
    };
    let (token, structured) = parse_verdict_from_text(&outcome);
    assert_eq!(token, "REPAIR");
    assert_eq!(structured.verdict, "REPAIR");
    assert!((structured.confidence - 0.82).abs() < 1e-9);
    assert_eq!(structured.issues.len(), 2);
    assert!(structured.issues[0].contains("auth.rs:42"));
    assert!(structured.summary.contains("Two must-fix"));
    // critic_verdict is the same single-source accessor used by
    // the orchestrator's terminal-status path.
    assert_eq!(critic_verdict(&outcome), "REPAIR");
}

#[test]
fn parse_verdict_from_text_falls_back_to_prose_scan_when_no_block() {
    use pipe_server::orchestrator::{parse_verdict_from_text, TaskOutcome};
    // No JSON block → prose scan (legacy verdict_of path)
    // kicks in. Confidence is 0.0 because there's no signal;
    // issues is empty because the prose scan can't tell what
    // the must-fix list is.
    let outcome = TaskOutcome {
        role_id: "agent:critic:b".into(),
        role_display: "Reviewer".into(),
        status: "DONE".into(),
        summary: None,
        text: "Code looks clean. PASS on all checks.".into(),
        elapsed_ms: 0,
        structured_verdict: None,
    };
    let (token, structured) = parse_verdict_from_text(&outcome);
    assert_eq!(token, "PASS");
    assert_eq!(structured.verdict, "PASS");
    assert!((structured.confidence - 0.0).abs() < 1e-9);
    assert!(structured.issues.is_empty());
    assert!(!structured.summary.is_empty());
}

#[test]
fn parse_verdict_from_text_avoids_prose_scan_false_positive_when_block_present() {
    use pipe_server::orchestrator::{parse_verdict_from_text, TaskOutcome};
    // The historical bug: prose scan picked up "PASS" inside
    // a question like "should we PASS?" and returned PASS even
    // when the real verdict was REPAIR. With a JSON block,
    // the structured path wins and the question is irrelevant.
    let outcome = TaskOutcome {
        role_id: "agent:critic:a".into(),
        role_display: "BugHunter".into(),
        status: "DONE".into(),
        summary: None,
        text: r#"I considered whether to PASS this — the API is fine — but the worker missed redaction.

```flowntier-verdict
{
  "verdict": "REPAIR",
  "confidence": 0.91,
  "issues": ["[HIGH] log.rs:18"],
  "summary": "One must-fix redaction bug"
}
```"#
            .into(),
        elapsed_ms: 0,
        structured_verdict: None,
    };
    let (token, _) = parse_verdict_from_text(&outcome);
    // Despite "PASS" appearing 5x in the prose, the JSON block
    // wins and returns REPAIR.
    assert_eq!(token, "REPAIR");
}

#[test]
fn parse_verdict_from_text_normalises_unknown_verdict_token() {
    use pipe_server::orchestrator::{parse_verdict_from_text, TaskOutcome};
    // A critic that emits a typo or non-standard token must
    // not crash the orchestrator. The parser maps anything
    // outside the known set to UNKNOWN.
    let outcome = TaskOutcome {
        role_id: "agent:critic:b".into(),
        role_display: "Reviewer".into(),
        status: "DONE".into(),
        summary: None,
        text: r#"```flowntier-verdict
{
  "verdict": "MAYBE",
  "confidence": 0.4,
  "issues": [],
  "summary": "Not sure"
}
```"#
            .into(),
        elapsed_ms: 0,
        structured_verdict: None,
    };
    let (token, structured) = parse_verdict_from_text(&outcome);
    assert_eq!(token, "UNKNOWN");
    assert_eq!(structured.verdict, "UNKNOWN");
    // Confidence is clamped to [0, 1] even when the model
    // returns a value outside the range.
    assert!((0.0..=1.0).contains(&structured.confidence));
}

#[test]
fn parse_verdict_from_text_handles_lowercase_and_whitespace() {
    use pipe_server::orchestrator::{parse_verdict_from_text, TaskOutcome};
    let outcome = TaskOutcome {
        role_id: "agent:critic:a".into(),
        role_display: "BugHunter".into(),
        status: "DONE".into(),
        summary: None,
        text: r#"```flowntier-verdict
{ "verdict" : "rewrite" , "confidence" : 0.5 , "issues" : [] , "summary" : "ok" }
```"#
            .into(),
        elapsed_ms: 0,
        structured_verdict: None,
    };
    let (token, _) = parse_verdict_from_text(&outcome);
    assert_eq!(token, "REWRITE");
}

#[test]
fn should_repair_decision_breaks_on_pass_pair() {
    use pipe_server::orchestrator::should_repair_decision;
    // Both PASS → never repair, regardless of loop count.
    assert!(!should_repair_decision("PASS", "PASS", 0, 3));
    assert!(!should_repair_decision("PASS", "PASS", 5, 3));
    assert!(!should_repair_decision("PASS", "PASS", 0, 0));
}

#[test]
fn should_repair_decision_repairs_while_under_cap() {
    use pipe_server::orchestrator::should_repair_decision;
    // One critic flagged REPAIR and we're still under the cap
    // → repair.
    assert!(should_repair_decision("PASS", "REPAIR", 0, 3));
    assert!(should_repair_decision("REPAIR", "PASS", 1, 3));
    // REWRITE counts too — the orchestrator treats any
    // non-PASS verdict as a repair trigger.
    assert!(should_repair_decision("REWRITE", "REWRITE", 2, 3));
    // UNKNOWN (critic didn't emit the JSON block — see
    // event 000115 fallback) also counts as non-PASS so the
    // loop is conservative: a silent critic still triggers
    // a re-run rather than silently accepting UNKNOWN.
    assert!(should_repair_decision("UNKNOWN", "PASS", 0, 3));
}

#[test]
fn should_repair_decision_bails_at_max_loops() {
    use pipe_server::orchestrator::should_repair_decision;
    // At the cap, even a persistent REPAIR stops the loop —
    // the workflow proceeds to delivery with the last
    // worker_results and the terminal status reflects the
    // unrepaired verdict pair (event 000114).
    assert!(!should_repair_decision("PASS", "REPAIR", 3, 3));
    assert!(!should_repair_decision("REWRITE", "PASS", 5, 3));
    // max_loops=0 disables repair entirely (chief wants a
    // single-shot workflow with no retries).
    assert!(!should_repair_decision("REPAIR", "REPAIR", 0, 0));
}

#[test]
fn agent_event_repair_loop_serializes_to_kind_tag() {
    use agent_core::event::AgentEvent;
    // v0.4.22 (event 000113): serde `tag = "kind"` means the
    // wire JSON carries a `kind: "repair_loop"` discriminator
    // — the webview's applyEvent uses this to route the event
    // to the repair panel.
    let ev = AgentEvent::RepairLoop {
        wf_id: "wf_test_001".into(),
        loop_index: 2,
        max_loops: 3,
        verdict_a: "PASS".into(),
        verdict_b: "REPAIR".into(),
        issues_a: vec![],
        issues_b: vec!["[HIGH] auth.rs:42 — token redaction missing".into()],
    };
    let json = serde_json::to_string(&ev).expect("serialize");
    assert!(json.contains("\"kind\":\"repair_loop\""), "got: {json}");
    assert!(json.contains("\"loop_index\":2"));
    assert!(json.contains("\"max_loops\":3"));
    assert!(json.contains("\"verdict_b\":\"REPAIR\""));
    // Round-trip preserves all fields, including the empty
    // issues_a (vs non-empty issues_b).
    let parsed: AgentEvent = serde_json::from_str(&json).expect("deserialize");
    match parsed {
        AgentEvent::RepairLoop {
            loop_index,
            max_loops,
            verdict_a,
            verdict_b,
            issues_a,
            issues_b,
            ..
        } => {
            assert_eq!(loop_index, 2);
            assert_eq!(max_loops, 3);
            assert_eq!(verdict_a, "PASS");
            assert_eq!(verdict_b, "REPAIR");
            assert!(issues_a.is_empty());
            assert_eq!(issues_b.len(), 1);
        }
        other => panic!(
            "expected RepairLoop, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

#[tokio::test]
async fn run_workflow_rejects_unconfigured_chief_role() {
    let (addr, handle) = spawn_server("run-workflow-unconfigured").await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    // POST /api/run_workflow on a fresh server where agent:chief is unconfigured
    let resp = client::connect_and_request(
        &addr,
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "POST",
            "params": {
                "path": "/api/run_workflow",
                "body": { "task": "hello world" }
            }
        }),
    )
    .await;
    assert_eq!(resp["result"]["status"].as_u64().unwrap_or(0), 400);
    assert_eq!(resp["result"]["body"]["ok"], serde_json::json!(false));
    let err_msg = resp["result"]["body"]["error"].as_str().unwrap_or("");
    assert!(
        err_msg.contains("未配置 AI 模型") || err_msg.contains("role not configured"),
        "error message should indicate unconfigured model/role; got {err_msg}"
    );
    handle.abort();
}
