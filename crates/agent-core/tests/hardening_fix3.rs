//! Standalone hardening test for event 000118 fix 3 — Phase 5
//! per-worker `task_id` propagation. Uses `fn main()` instead of
//! `#[test]` so the binary exits with a non-zero code on failure,
//! bypassing the libtest harness (which crashes on Windows under
//! the current Rust toolchain — see flake8 in the codebase).
//!
//! Each sub-test is wrapped in `check(name, closure)` that
//! records PASS/FAIL. The whole file writes the result to
//! `target/hardening_fix3_result.txt` and `target/hardening_fix3.log`
//! then exits. The exit code itself is unreliable on Windows
//! (the teardown of the test runner msvcrt atexit handlers
//! produces STATUS_ACCESS_VIOLATION even after a clean main()).
//! We rely on the log file as the source of truth, and the build
//! script can `cat` it to assert success.
//!
//! Coverage:
//!   1. TextDelta with task_id = Some roundtrips cleanly
//!   2. TextDelta with task_id = None serialises to JSON `null`
//!      (not omitted) — preserves "explicitly None" vs "omitted"
//!      distinction for downstream filters
//!   3. ToolStarted/Finished/TokenUsage all carry task_id when set
//!   4. Empty-string task_id is preserved (not normalised to None)
//!   5. PhaseTransition/Done/ReviewerVerdict/RepairLoop do NOT
//!      carry a task_id field
//!   6. AGENT_EVENT_KINDS stays in sync with serde-emitted tags
//!      (re-asserts the existing event 000116 contract)

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;

use agent_core::event::{AgentEvent, AGENT_EVENT_KINDS};
use agent_core::message::ToolCall;
use serde_json::{json, Value};

fn log_path() -> String {
    // tests/ subdir is in OUT_DIR-relative; target/ is the cargo target.
    // We use CARGO_MANIFEST_DIR + /../target so it works whether
    // invoked from `cargo test` or from the test runner.
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|_| "O:/clawwork/Flowntier/crates/agent-core".into());
    let target = std::path::Path::new(&manifest).join("..").join("target");
    target
        .join("hardening_fix3.log")
        .to_string_lossy()
        .into_owned()
}

fn result_path() -> String {
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|_| "O:/clawwork/Flowntier/crates/agent-core".into());
    let target = std::path::Path::new(&manifest).join("..").join("target");
    target
        .join("hardening_fix3_result.txt")
        .to_string_lossy()
        .into_owned()
}

fn record(log: &mut String, name: &str, f: impl FnOnce() -> Result<(), String>) -> bool {
    match f() {
        Ok(()) => {
            log.push_str(&format!("[PASS] {name}\n"));
            true
        }
        Err(e) => {
            log.push_str(&format!("[FAIL] {name}: {e}\n"));
            false
        }
    }
}

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
                call: ToolCall {
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

fn main() {
    let mut log = String::new();
    let mut pass = 0u32;
    let mut total = 0u32;

    let mut run = |name: &str, f: &dyn Fn() -> Result<(), String>| -> bool {
        total += 1;
        if record(&mut log, name, f) {
            pass += 1;
            true
        } else {
            false
        }
    };

    run("TextDelta task_id=Some roundtrips", &|| {
        let ev = AgentEvent::TextDelta {
            agent_id: "agent:worker".into(),
            agent_display: "实施".into(),
            delta: "x".into(),
            task_id: Some("t2".into()),
        };
        let v: Value = serde_json::to_value(&ev).map_err(|e| e.to_string())?;
        if v["task_id"] != json!("t2") {
            return Err(format!("task_id in JSON: {:?}", v["task_id"]));
        }
        if v["kind"] != json!("text_delta") {
            return Err(format!("kind drifted: {:?}", v["kind"]));
        }
        let back: AgentEvent = serde_json::from_value(v).map_err(|e| e.to_string())?;
        match back {
            AgentEvent::TextDelta { task_id, .. } if task_id.as_deref() == Some("t2") => Ok(()),
            other => Err(format!("unexpected variant after roundtrip: {other:?}")),
        }
    });

    run(
        "TextDelta task_id=None serialises to JSON null (not omitted)",
        &|| {
            let ev = AgentEvent::TextDelta {
                agent_id: "agent:chief".into(),
                agent_display: "主理".into(),
                delta: "x".into(),
                task_id: None,
            };
            let v: Value = serde_json::to_value(&ev).map_err(|e| e.to_string())?;
            if v.get("task_id").is_none() {
                return Err("task_id field was OMITTED for None — must be present as null".into());
            }
            if v["task_id"] != json!(null) {
                return Err(format!("task_id != null: {:?}", v["task_id"]));
            }
            let back: AgentEvent = serde_json::from_value(v).map_err(|e| e.to_string())?;
            match back {
                AgentEvent::TextDelta { task_id, .. } if task_id.is_none() => Ok(()),
                other => Err(format!("unexpected variant after roundtrip: {other:?}")),
            }
        },
    );

    run(
        "task_id=\"\" is preserved as a distinct key (not normalised to None)",
        &|| {
            let ev = AgentEvent::ToolStarted {
                agent_id: "agent:worker".into(),
                agent_display: "实施".into(),
                call: ToolCall {
                    id: "c".into(),
                    name: "bash".into(),
                    args: json!({}),
                },
                task_id: Some(String::new()),
            };
            let v: Value = serde_json::to_value(&ev).map_err(|e| e.to_string())?;
            if v["task_id"] != json!("") {
                return Err(format!(
                    "task_id \"\" not preserved in JSON: {:?}",
                    v["task_id"]
                ));
            }
            let back: AgentEvent = serde_json::from_value(v).map_err(|e| e.to_string())?;
            match back {
                AgentEvent::ToolStarted { task_id, .. } => match task_id {
                    Some(s) if s.is_empty() => Ok(()),
                    Some(_) => Err("task_id non-empty after roundtrip".into()),
                    None => Err("task_id was silently normalised to None".into()),
                },
                other => Err(format!("unexpected variant: {other:?}")),
            }
        },
    );

    run("task_id present on all 4 task-scoped variants", &|| {
        let variants: Vec<(&str, AgentEvent)> = vec![
            (
                "text_delta",
                AgentEvent::TextDelta {
                    agent_id: "a".into(),
                    agent_display: "d".into(),
                    delta: "x".into(),
                    task_id: Some("t0".into()),
                },
            ),
            (
                "tool_started",
                AgentEvent::ToolStarted {
                    agent_id: "a".into(),
                    agent_display: "d".into(),
                    call: ToolCall {
                        id: "c".into(),
                        name: "bash".into(),
                        args: json!({}),
                    },
                    task_id: Some("t0".into()),
                },
            ),
            (
                "tool_finished",
                AgentEvent::ToolFinished {
                    agent_id: "a".into(),
                    agent_display: "d".into(),
                    tool_call_id: "c".into(),
                    preview: "p".into(),
                    is_error: false,
                    elapsed_ms: 0,
                    task_id: Some("t0".into()),
                },
            ),
            (
                "token_usage",
                AgentEvent::TokenUsage {
                    agent_id: "a".into(),
                    provider: "p".into(),
                    model: "m".into(),
                    input_tokens: 0,
                    output_tokens: 0,
                    cost_usd: None,
                    task_id: Some("t0".into()),
                },
            ),
        ];
        for (expected_kind, ev) in variants {
            let v: Value = serde_json::to_value(&ev).map_err(|e| e.to_string())?;
            if v["kind"] != json!(expected_kind) {
                return Err(format!(
                    "{}: kind drifted to {:?}",
                    expected_kind, v["kind"]
                ));
            }
            if v["task_id"] != json!("t0") {
                return Err(format!(
                    "{}: task_id missing in JSON: {:?}",
                    expected_kind, v["task_id"]
                ));
            }
        }
        Ok(())
    });

    run(
        "task_id ABSENT from phase_transition/done/reviewer/repair",
        &|| {
            let variants: Vec<(&str, AgentEvent)> = vec![
                (
                    "phase_transition",
                    AgentEvent::PhaseTransition {
                        wf_id: "wf".into(),
                        from: None,
                        to: "1-requirement".into(),
                    },
                ),
                (
                    "done",
                    AgentEvent::Done {
                        wf_id: "wf".into(),
                        status: "DONE".into(),
                        summary: None,
                    },
                ),
                (
                    "reviewer_verdict",
                    AgentEvent::ReviewerVerdict {
                        wf_id: "wf".into(),
                        phase: "plan-review".into(),
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
                        wf_id: "wf".into(),
                        loop_index: 1,
                        max_loops: 3,
                        verdict_a: "PASS".into(),
                        verdict_b: "PASS".into(),
                        issues_a: vec![],
                        issues_b: vec![],
                    },
                ),
            ];
            for (expected_kind, ev) in variants {
                let v: Value = serde_json::to_value(&ev).map_err(|e| e.to_string())?;
                if v["kind"] != json!(expected_kind) {
                    return Err(format!("{}: kind drifted", expected_kind));
                }
                if v.get("task_id").is_some() {
                    return Err(format!(
                        "{expected_kind} must NOT carry task_id, got: {v:?}"
                    ));
                }
            }
            Ok(())
        },
    );

    run("AGENT_EVENT_KINDS matches actual serde tags", &|| {
        let declared: BTreeSet<&str> = AGENT_EVENT_KINDS.iter().copied().collect();
        let actual: BTreeSet<&str> = sample_payloads()
            .iter()
            .map(|(kind, ev)| {
                let v: Value = serde_json::to_value(ev).expect("serialize");
                let tag = v.get("kind").and_then(|k| k.as_str()).unwrap().to_owned();
                assert_eq!(tag, *kind);
                Box::leak(tag.into_boxed_str()) as &'static str
            })
            .collect();
        if declared != actual {
            return Err(format!(
                "declared != actual: declared={declared:?}, actual={actual:?}"
            ));
        }
        Ok(())
    });

    let summary = format!("hardening-fix3: {pass}/{total} PASS\n");
    log.push_str(&summary);
    let final_status = if pass == total { "OK" } else { "FAIL" };

    // Persist to file so cargo's failure of the wrapper doesn't
    // obscure the result. Wrap in a parent dir mkdir to be safe.
    let _ = fs::write(log_path(), &log);
    let _ = fs::write(result_path(), format!("{final_status}\n{summary}"));

    // Best-effort flush; do NOT depend on the OS exit code path
    // because the msvcrt atexit handler crashes (0xc0000005) on
    // this specific toolchain under Windows when stdout is
    // captured by the cargo test runner. Source of truth is the
    // log/result file above.
    let mut out = std::io::stdout();
    let _ = out.write_all(log.as_bytes());
    let _ = out.flush();

    if pass != total {
        // Use _exit to skip Rust's atexit handlers entirely.
        // This is a Unix API though; on Windows it still routes
        // through ExitProcess. We just don't unwind destructors.
        std::process::exit(1);
    }
}
