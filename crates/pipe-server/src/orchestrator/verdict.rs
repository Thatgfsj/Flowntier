//! Verdict parsing / terminal status helpers.
//! Split out of orchestrator.rs - pure move, no logic change.

use super::*;

/// Pull a PASS / REPAIR / REWRITE token out of an agent's text.
///
/// v0.4.22 (event 000115): deprecated as the *primary* extractor
/// — superseded by `parse_verdict_from_text` which reads the
/// ```flowntier-verdict``` JSON block critics are now instructed
/// to emit. Kept as the prose-scan fallback when the JSON block
/// is missing or malformed, so old models (or a critic that
/// timed out before reaching the verdict block) still produce
/// a usable verdict instead of "UNKNOWN".
///
/// Looks at the text and summary for the most recent verdict
/// token; if none found, returns "UNKNOWN".
/// Priority order: REWRITE → REPAIR → PASS.
/// Word boundary and negation checks prevent false matches (e.g. "password", "cannot pass", "不通过").
pub(super) fn verdict_of(o: &TaskOutcome) -> String {
    scan_text_verdict(&o.text)
        .or_else(|| o.summary.as_ref().and_then(|s| scan_text_verdict(s)))
        .unwrap_or_else(|| "UNKNOWN".into())
}

pub(super) fn scan_text_verdict(raw: &str) -> Option<String> {
    let upper = raw.to_uppercase();

    // 1. Explicit negations for PASS: "不通过", "未通过", "无法通过", "不能通过", "暂不通过", "未能通过", "未予通过", "NOT PASS", "CANNOT PASS", etc.
    if upper.contains("NOT PASS")
        || upper.contains("CANNOT PASS")
        || upper.contains("CAN NOT PASS")
        || upper.contains("DOES NOT PASS")
        || raw.contains("不通过")
        || raw.contains("未通过")
        || raw.contains("无法通过")
        || raw.contains("不能通过")
        || raw.contains("暂不通过")
        || raw.contains("未能通过")
        || raw.contains("未予通过")
    {
        return Some("REPAIR".into());
    }

    // 2. Highest severity: REWRITE / 重写 (excluding "无需重写", "不用重写", "NO REWRITE")
    let has_rewrite = (upper.contains("REWRITE")
        && !upper.contains("NO REWRITE")
        && !upper.contains("WITHOUT REWRITE"))
        || (raw.contains("需重写")
            || (raw.contains("重写") && !raw.contains("无需重写") && !raw.contains("不用重写")));
    if has_rewrite {
        return Some("REWRITE".into());
    }

    // 3. Medium severity: REPAIR / 需修复 / 修复 (excluding "无需修复", "不用修复", "无需修改", "已经修复", "修复完成")
    let is_negative_repair = raw.contains("无需修复")
        || raw.contains("不用修复")
        || raw.contains("无需修改")
        || raw.contains("不用修改")
        || raw.contains("无需再修复")
        || raw.contains("已经修复")
        || raw.contains("修复完成")
        || upper.contains("NO REPAIR")
        || upper.contains("NO REPAIRS");

    let has_repair = (upper.contains("REPAIR") && !is_negative_repair)
        || raw.contains("需修复")
        || (raw.contains("修复") && !is_negative_repair);
    if has_repair {
        return Some("REPAIR".into());
    }

    // 4. PASS / 通过 / 无需修复
    if raw.contains("通过") || is_negative_repair {
        return Some("PASS".into());
    }
    for word in upper.split(|c: char| !c.is_alphabetic()) {
        if word == "PASS" {
            return Some("PASS".into());
        }
    }
    None
}

/// v0.4.22 (event 000115): convenience wrapper around
/// `parse_verdict_from_text` that drops the structured payload
/// and returns just the verdict token. Used in the chief's
/// repair-decision prompt + the final-review summary line +
/// the terminal-status mapping — all of which only need the
/// PASS/REPAIR/REWRITE/UNKNOWN label.
///
/// Prefer this over calling `verdict_of` directly so all six
/// former `verdict_of()` call sites agree on the source-of-
/// truth: structured JSON block first, prose scan fallback.
pub fn critic_verdict(outcome: &TaskOutcome) -> String {
    parse_verdict_from_text(outcome).0
}

/// v0.4.22 (event 000115): extract a structured reviewer
/// verdict from the agent's final text. Two-layer strategy:
///
/// 1. Try to find a ```flowntier-verdict ... ``` fenced JSON
///    block anywhere in the text. If found, deserialize into
///    `ReviewerVerdictJson` and validate the verdict token is
///    one of PASS / REPAIR / REWRITE. The fenced block wins
///    because it is the format critics are explicitly told to
///    emit at the end of their output.
/// 2. Fall back to the legacy prose scan (`verdict_of`).
///    Confidence / issues / summary are filled with sensible
///    defaults so callers that expect a fully-populated struct
///    always get one.
///
/// Returns a tuple of `(verdict_token, structured_verdict)` so
/// callers that only need the verdict can ignore the second
/// element, and callers that want the rich fields (event emit,
/// review card) can use it directly. When the fallback path
/// fires, the returned `ReviewerVerdictJson` carries
/// `verdict = verdict_of(...)`, `confidence = 0.0` (we have
/// no signal), `issues = []`, `summary = first sentence of
/// text`.
pub fn parse_verdict_from_text(o: &TaskOutcome) -> (String, ReviewerVerdictJson) {
    if let Some(parsed) = extract_fenced_verdict_block(&o.text) {
        let verdict = normalize_verdict_token(&parsed.verdict);
        let confidence = parsed.confidence.clamp(0.0, 1.0);
        return (
            verdict.clone(),
            ReviewerVerdictJson {
                verdict,
                confidence,
                issues: parsed.issues,
                summary: parsed.summary,
            },
        );
    }
    // Fallback — prose scan, confidence unknown.
    let verdict = verdict_of(o);
    let summary = first_sentence(&o.text, &o.summary);
    (
        verdict.clone(),
        ReviewerVerdictJson {
            verdict,
            confidence: 0.0,
            issues: Vec::new(),
            summary,
        },
    )
}

/// Look for a ```flowntier-verdict ... ``` fenced block anywhere
/// in the text and try to parse its body as JSON. Returns None
/// when the fence is absent or the JSON is malformed. The fence
/// marker is intentionally distinct from common languages
/// (rust, json, ts, ...) so it cannot be confused with code the
/// critic is reviewing.
pub(super) fn extract_fenced_verdict_block(text: &str) -> Option<ReviewerVerdictJson> {
    const MARKER: &str = "```flowntier-verdict";
    let start = text.find(MARKER)?;
    // Skip past the opening fence line (and any trailing
    // language tag — our marker carries no tag, but tolerate
    // one for future-proofing).
    let after_fence = start + MARKER.len();
    let after_fence = text[after_fence..]
        .find('\n')
        .map(|i| after_fence + i + 1)?;
    // Find the closing ``` (start of line, optionally indented).
    let body_start = after_fence;
    let body_end_rel = text[body_start..].find("```").map(|i| body_start + i)?;
    let body = text[body_start..body_end_rel].trim();
    serde_json::from_str::<ReviewerVerdictJson>(body).ok()
}

/// Map any case-variant of the verdict token to its canonical
/// uppercase form. Anything outside the known set is treated
/// as UNKNOWN so downstream severity logic (REWRITE > REPAIR)
/// never crashes on a typo.
pub(super) fn normalize_verdict_token(raw: &str) -> String {
    let trimmed = raw.trim();
    let upper = trimmed.to_uppercase();
    if upper == "PASS" || trimmed == "通过" {
        return "PASS".into();
    }
    if upper == "REPAIR" || trimmed == "需修复" || trimmed == "修复" {
        return "REPAIR".into();
    }
    if upper == "REWRITE" || trimmed == "重写" || trimmed == "需重写" {
        return "REWRITE".into();
    }
    "UNKNOWN".into()
}

/// First non-empty sentence of the text, falling back to the
/// summary when text is empty. Used as the rationale shown
/// next to the verdict token on the webview's ReviewerCard
/// when no JSON `summary` was provided.
pub(super) fn first_sentence(text: &str, summary: &Option<String>) -> String {
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        trimmed
            .split(['\n', '。'])
            .next()
            .unwrap_or("")
            .chars()
            .take(200)
            .collect::<String>()
    } else {
        summary.clone().unwrap_or_default()
    }
}

/// v0.4.22 (event 000114): translate a critic's verdict token
/// into a 0..=2 severity rank used to pick the "worst" of two
/// final-review verdicts. REWRITE > REPAIR > everything else.
/// Pure function, exposed for unit tests in the e2e_pipe file.
/// v0.4.22 (event 000113): pure decision fn driving the
/// Phase 5 ↔ Phase 7 repair loop. Given a (verdict_a,
/// verdict_b) pair from the latest Phase 6 run and the current
/// `loop_count` plus the configured `max_loops`, returns
/// `true` iff the orchestrator should re-run Phase 5. Pure
/// so it is fully covered by e2e tests without spinning up
/// a real LLM loop.
///
/// Decision matrix:
/// - both PASS  → never repair
/// - any non-PASS and loop_count < max_loops → repair
/// - any non-PASS and loop_count >= max_loops → bail
///   (terminal status reflects the unrepaired verdict pair,
///    handled by event 000114's `terminal_done_status`)
pub fn should_repair_decision(
    verdict_a: &str,
    verdict_b: &str,
    loop_count: u32,
    max_loops: u32,
) -> bool {
    if (verdict_a == "PASS" && verdict_b == "PASS")
        || (verdict_a == "UNKNOWN" && verdict_b == "UNKNOWN")
    {
        return false;
    }
    loop_count < max_loops
}

pub(crate) fn verdict_severity_rank(v: &str) -> u8 {
    match v {
        "REWRITE" => 2,
        "REPAIR" => 1,
        _ => 0,
    }
}

/// v0.4.22 (event 000114): turn the (verdict_a, verdict_b)
/// pair into a final terminal `AgentEvent::Done.status` string.
/// Both PASS → "DONE". Dual UNKNOWN → failure indicating critics
/// unresponsive. Any REPAIR or REWRITE → "FAILED: reviewer_<reason>_<role_id>".
pub fn terminal_done_status(
    verdict_a: &str,
    verdict_b: &str,
    role_a: &str,
    role_b: &str,
) -> String {
    if verdict_a == "UNKNOWN" && verdict_b == "UNKNOWN" {
        return "FAILED: reviewer_UNKNOWN_both_critics_unresponsive".to_string();
    }
    let pick = if verdict_severity_rank(verdict_a) >= verdict_severity_rank(verdict_b) {
        (verdict_a, role_a)
    } else {
        (verdict_b, role_b)
    };
    match pick.0 {
        "PASS" | "UNKNOWN" => "DONE".to_string(),
        other => format!("FAILED: reviewer_{}_{}", other, pick.1),
    }
}

/// Small random suffix to make task ids unique within a single
/// process. Not cryptographic — just collision-resistant
/// enough for the dashboard's per-row rendering.
pub(super) fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    // 8 hex chars from the nanosecond tail.
    format!("{:08x}", (nanos as u64) & 0xFFFFFFFF)
}

/// Map the orchestrator's unprefixed phase names to the
/// storage layer's WorkflowPhase enum. Kept in sync with
/// `crates/storage/src/lib.rs` WorkflowPhase definition.
pub(super) fn map_phase_name(name: &str) -> String {
    match name {
        "requirement" => "1-requirement".into(),
        "plan" => "2-plan".into(),
        "plan-review" => "3-plan-review".into(),
        "dispatch" => "4-dispatch".into(),
        "develop" => "5-develop".into(),
        "final-review" => "6-final-review".into(),
        "repair" => "7-repair".into(),
        "delivery" => "8-delivery".into(),
        _ => name.to_string(),
    }
}

// Silence unused import warnings on platforms that drop them.
#[allow(dead_code)]
pub(super) fn _json_silence(v: Value) -> Value {
    json!(v)
}
