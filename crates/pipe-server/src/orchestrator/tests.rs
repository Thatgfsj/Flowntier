use super::verdict::verdict_of;
use super::*;

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use tokio::sync::broadcast;

#[test]
fn rate_limit_detection_covers_provider_variants() {
    // SiliconFlow's shape (observed live in the v0.4.36 GUI test).
    assert!(status_is_rate_limited(
        "FAILED: provider error: provider returned 429: {\"code\":50602,\"message\":\"Request was rejected due to rate limiting. Details: TPM limit reached.\"}"
    ));
    // Plain OpenAI-style 429 and prose variants.
    assert!(status_is_rate_limited(
        "FAILED: provider error: 429 too many requests"
    ));
    assert!(status_is_rate_limited("FAILED: Rate Limit exceeded"));
    // Non-rate failures must NOT match.
    assert!(!status_is_rate_limited(
        "FAILED: provider error: 500 internal"
    ));
    assert!(!status_is_rate_limited("DONE"));
    assert!(!status_is_rate_limited("FAILED: resolve: no key"));
}

#[test]
fn rate_limit_backoff_is_bounded() {
    assert_eq!(RATE_LIMIT_MAX_RETRIES, RATE_LIMIT_BACKOFF_SECS.len());
    // Worst-case wait per agent stays finite and sane (< 1 min).
    let total: u64 = RATE_LIMIT_BACKOFF_SECS.iter().sum();
    assert!(total < 60);
    assert!(RATE_LIMIT_BACKOFF_SECS[0] >= 1);
}

#[test]
fn test_from_chief_text_with_think_tags() {
    let text = "<think>Let me reason about the architecture... {not json}</think>\n```json\n{\n  \"summary\": \"Web app\",\n  \"architecture\": \"React + Rust\",\n  \"tasks\": [\n    {\n      \"id\": \"w_api\",\n      \"title\": \"Build API\",\n      \"objective\": \"Implement HTTP routes\",\n      \"interfaces\": \"\",\n      \"dependencies\": [],\n      \"requirements\": \"\"\n    }\n  ]\n}\n```";
    let plan = PlanDoc::from_chief_text(text, "Build app");
    assert_eq!(plan.summary, "Web app");
    assert_eq!(plan.tasks.len(), 1);
    assert_eq!(plan.tasks[0].id, "w_api");
}

#[test]
fn test_from_chief_text_with_tasks_only_json() {
    // In Round B & C, prompt tells LLM to only output tasks without summary/architecture
    let text = "Here are the tasks:\n```json\n{\n  \"tasks\": [\n    {\n      \"id\": \"w_db\",\n      \"title\": \"Database schema\",\n      \"objective\": \"Setup SQLite\",\n      \"interfaces\": \"\",\n      \"dependencies\": [],\n      \"requirements\": \"\"\n    }\n  ]\n}\n```";
    let plan = PlanDoc::from_chief_text(text, "Build app");
    assert_eq!(plan.tasks.len(), 1);
    assert_eq!(plan.tasks[0].id, "w_db");
}

#[test]
fn test_from_chief_text_with_bare_array() {
    let text = "[\n  {\n    \"id\": \"w_ui\",\n    \"title\": \"UI Component\",\n    \"objective\": \"Design buttons\",\n    \"interfaces\": \"\",\n    \"dependencies\": [],\n    \"requirements\": \"\"\n  }\n]";
    let plan = PlanDoc::from_chief_text(text, "Build app");
    assert_eq!(plan.tasks.len(), 1);
    assert_eq!(plan.tasks[0].id, "w_ui");
}

#[test]
fn test_partition_tasks_into_stages_dag() {
    let tasks = vec![
        WorkerTask {
            id: "w_db".into(),
            title: "Database".into(),
            objective: "DB".into(),
            interfaces: "".into(),
            dependencies: vec![],
            requirements: "".into(),
            label: "Database".into(),
        },
        WorkerTask {
            id: "w_api".into(),
            title: "API".into(),
            objective: "API".into(),
            interfaces: "".into(),
            dependencies: vec!["w_db".into()],
            requirements: "".into(),
            label: "Backend".into(),
        },
        WorkerTask {
            id: "w_ui".into(),
            title: "UI".into(),
            objective: "UI".into(),
            interfaces: "".into(),
            dependencies: vec!["w_api".into()],
            requirements: "".into(),
            label: "Frontend".into(),
        },
        WorkerTask {
            id: "w_docs".into(),
            title: "Docs".into(),
            objective: "Docs".into(),
            interfaces: "".into(),
            dependencies: vec![],
            requirements: "".into(),
            label: "Documentation".into(),
        },
    ];

    let stages = Orchestrator::partition_tasks_into_stages(&tasks);
    // Stage 0 should contain w_db and w_docs (indices 0 and 3)
    assert_eq!(stages[0], vec![0, 3]);
    // Stage 1 should contain w_api (index 1)
    assert_eq!(stages[1], vec![1]);
    // Stage 2 should contain w_ui (index 2)
    assert_eq!(stages[2], vec![2]);
}

#[test]
fn test_worker_task_serde_defaults() {
    let json = r#"{"id": "w1", "title": "Task 1"}"#;
    let task: WorkerTask = serde_json::from_str(json).expect("should deserialize with defaults");
    assert_eq!(task.id, "w1");
    assert_eq!(task.title, "Task 1");
    assert_eq!(task.objective, "");
    assert_eq!(task.interfaces, "");
    assert!(task.dependencies.is_empty());
    assert_eq!(task.requirements, "");
    assert_eq!(task.label, "");
}

#[test]
fn test_verdict_of_severity_and_boundaries() {
    let make_outcome = |text: &str| TaskOutcome {
        role_id: "agent:critic:a".into(),
        role_display: "Critic".into(),
        text: text.into(),
        summary: None,
        status: "DONE".into(),
        elapsed_ms: 100,
        structured_verdict: None,
    };

    // Word boundary check: password must not be PASS
    assert_eq!(
        verdict_of(&make_outcome("Please enter your password")),
        "UNKNOWN"
    );
    // Negation check: cannot pass must be REPAIR
    assert_eq!(
        verdict_of(&make_outcome("The tests cannot pass yet")),
        "REPAIR"
    );
    assert_eq!(verdict_of(&make_outcome("验收不通过，请修正")), "REPAIR");
    // Chinese checks
    assert_eq!(verdict_of(&make_outcome("代码编写完成，通过")), "PASS");
    assert_eq!(verdict_of(&make_outcome("发现死锁风险，需修复")), "REPAIR");
    assert_eq!(
        verdict_of(&make_outcome("严重设计缺陷，必须重写")),
        "REWRITE"
    );
    // Severity precedence: REWRITE > REPAIR > PASS
    assert_eq!(
        verdict_of(&make_outcome("虽然部分测试 PASS，但架构缺陷需要 REWRITE")),
        "REWRITE"
    );
}

// ── v0.4.38 (issue #26 defects 1 + 2): terminal-state regressions ──────

/// Defect 1: once a provider-infrastructure failure has set the abort
/// reason, run() must stop at the phase boundary — the final verdict
/// (both critics PASS → DONE) must never rewrite the terminal state.
#[test]
fn provider_abort_stops_before_next_phase_and_keeps_failure_status() {
    let why = "角色 agent:worker — FAILED: provider error: provider returned 429";
    let stop = terminal_stop(Some(why), false);
    assert_eq!(stop, Some(TerminalStop::ProviderFailure));
    assert_eq!(stop.unwrap().status(), "FAILED: provider_failure");
    // The verdict-derived status would be DONE for a dual PASS; the abort
    // must win regardless.
    assert_eq!(
        terminal_done_status("PASS", "PASS", "agent:critic:a", "agent:critic:b"),
        "DONE"
    );
    // Abort also wins when a cancel arrives afterwards.
    assert_eq!(
        terminal_stop(Some(why), true),
        Some(TerminalStop::ProviderFailure)
    );
    // No abort + no cancel = normal path untouched.
    assert_eq!(terminal_stop(None, false), None);
}

/// Defect 2: a cancel must terminate the run as ABORTED, not as the
/// verdict-derived DONE, and must not wait for a later phase.
#[test]
fn cancelled_stop_is_aborted_and_not_overridden_by_pass_verdict() {
    assert_eq!(
        terminal_done_status("PASS", "PASS", "agent:critic:a", "agent:critic:b"),
        "DONE"
    );
    let stop = terminal_stop(None, true);
    assert_eq!(stop, Some(TerminalStop::Cancelled));
    assert_eq!(stop.unwrap().status(), "ABORTED");
}

/// Defect 1 + 2: only one terminal Done event may ever be emitted for a
/// workflow, even when abort/cancel paths race.
#[test]
fn terminal_done_event_is_emitted_at_most_once() {
    let (tx, mut rx) = broadcast::channel(8);
    let emitted = AtomicBool::new(false);
    assert!(emit_done_once(
        &emitted,
        &tx,
        "wf_regression",
        "FAILED: provider_failure",
        Some("abort".into())
    ));
    // The verdict tail / second cancel must not emit a second Done.
    assert!(!emit_done_once(
        &emitted,
        &tx,
        "wf_regression",
        "DONE",
        Some("verdict".into())
    ));
    let mut statuses = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let AgentEvent::Done { status, .. } = ev {
            statuses.push(status);
        }
    }
    assert_eq!(statuses, vec!["FAILED: provider_failure".to_string()]);
}

/// Defect 1: the persisted summary must keep the original provider
/// failure reason (same text emitted and stored).
#[test]
fn provider_failure_summary_keeps_original_reason() {
    let why = "角色 agent:chief — FAILED: all candidates exhausted";
    let summary = provider_failure_summary(why);
    assert!(summary.contains(why));
    assert!(summary.contains("模型服务不可用"));
}

/// Defect 2: cancelling during a 429 backoff yields ABORTED, not the
/// pending 429 (which would be reclassified as provider_failure).
#[test]
fn cancelled_backoff_outcome_is_aborted_not_429() {
    let outcome = cancelled_outcome("agent:chief".into(), "Chief".into());
    assert_eq!(outcome.status, "ABORTED");
    assert!(!status_is_rate_limited(&outcome.status));
    assert!(!outcome.status.contains("429"));
    assert_eq!(outcome.summary.as_deref(), Some(CANCELLED_SUMMARY));
}

/// Defect 2: the 429 backoff sleep must return promptly when the token
/// fires instead of waiting out the full (up to 30s) delay.
#[tokio::test]
async fn rate_limit_backoff_sleep_is_cancelled_promptly() {
    let token = tokio_util::sync::CancellationToken::new();
    let canceller = {
        let token = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            token.cancel();
        })
    };
    let started = Instant::now();
    let cancelled = super::phases::sleep_cancellable(&token, Duration::from_secs(10)).await;
    assert!(cancelled, "backoff sleep must observe the cancel token");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "cancel must not wait out the full backoff (elapsed {:?})",
        started.elapsed()
    );
    canceller.await.unwrap();
}

/// Defect 2: without a cancel the same helper still waits and reports
/// "not cancelled" (normal path unchanged).
#[tokio::test]
async fn rate_limit_backoff_sleep_completes_without_cancel() {
    let token = tokio_util::sync::CancellationToken::new();
    let cancelled = super::phases::sleep_cancellable(&token, Duration::from_millis(10)).await;
    assert!(!cancelled);
}
