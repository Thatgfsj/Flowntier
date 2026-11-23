//! Orchestrator — multi-agent workflow runner.
//!
//! Implements the 8-phase workflow from `history/PROJECT_SPEC.md`:
//!
//! 1. Requirement  (chief reads user request, asks 1-3 clarifying Qs)
//! 2. Plan         (chief writes structured Planning Doc)
//! 3. PlanReview   (critic:a + critic:b review the plan in parallel)
//! 4. Dispatch     (chief decomposes plan into N worker tasks)
//! 5. Develop      (workers run in parallel, no inter-worker comms)
//! 6. FinalReview  (critic:a + critic:b review worker outputs)
//! 7. Repair       (chief decides PASS / REPAIR / REWRITE; loop until PASS)
//! 8. Delivery     (chief writes a human-readable summary)
//!
//! Every phase emits a `PhaseTransition` event so the UI's
//! `PhaseTimeline` can show progress; every agent run emits its
//! own `TextDelta` / `ToolStarted` / `ToolFinished` events so
//! the chat zone shows what each agent did.
//!
//! Every worker + every critic review gets its own row in the
//! `tasks` table so the dashboard's "任务列表" panel shows real
//! per-unit progress (not just one row per chief run, which was
//! the v0.4.21-era behaviour).
//!
//! Event 000068. Spawns concurrent agents via `tokio::spawn` so
//! the critics in Phase 3 + Phase 6 actually run in parallel,
//! and so workers in Phase 5 don't serialise on each other.
//!
//! Event 000082: per-phase progress log. Each phase emits
//! "phase N started at <ts>" and "phase N completed in <ms>"
//! so the chairman can see how far a workflow got if it
//! stalls or crashes mid-run (e.g. when the v0.4.22
//! mimo:mimo-2.5-pro config returned a 401 and the
//! workflow hung). See NWT 000082 for the boundary.

use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_core::event::AgentEvent;
use agent_core::prompt::Role;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;
use tracing::{error, info, warn};

/// v0.4.26 (event 000119): map the orchestrator's `role_id`
/// ("agent:chief" / "agent:critic:a" / "agent:critic:b" /
/// "agent:worker" / "agent:planner" / "agent:reporter") to the
/// per-role log target ("chief" / "critic-a" / "critic-b" /
/// "worker" / "chief" / "chief"). The planner + reporter
/// delegate to the chief because they run in the chief's
/// pipeline phases; their detail belongs in chief.log.
#[allow(dead_code)]
fn role_to_log_target(role_id: &str) -> &'static str {
    match role_id {
        "agent:chief" => logs::TARGET_CHIEF,
        "agent:critic:a" => logs::TARGET_CRITIC_A,
        "agent:critic:b" => logs::TARGET_CRITIC_B,
        "agent:worker" => logs::TARGET_WORKER,
        // planner / reporter share the chief's file (they run
        // in the chief's pipeline).
        "agent:planner" | "agent:reporter" => logs::TARGET_CHIEF,
        _ => logs::TARGET_CHIEF,
    }
}

use crate::handlers::ServerState;
use crate::logs;
use crate::role_info;

/// Spec-defined phase names. Order is meaningful — the
/// orchestrator advances through these in lockstep.
///
/// NOTE (event 000068): names are unprefixed so the desktop
/// shell's existing PhaseTimeline component (which matches
/// on the suffix) lights up the right dot. The full
/// "1-requirement" / "2-plan" labels are used internally for
/// the tasks table (`title` column) where the chairman reads
/// the value directly.
pub const PHASES: [&str; 8] = [
    "requirement",
    "plan",
    "plan-review",
    "dispatch",
    "develop",
    "final-review",
    "repair",
    "delivery",
];

/// One unit of work the chief has carved out of the plan.
///
/// `objective`, `interfaces`, `dependencies`, `requirements`
/// are the exact fields the spec calls out for Worker handoff
/// (PROJECT_SPEC.md §Phase 4). Kept as plain strings so the
/// chief's LLM output is easy to JSON-parse.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkerTask {
    #[serde(default, alias = "task_id", alias = "taskId", alias = "id")]
    pub id: String,
    #[serde(
        default,
        alias = "name",
        alias = "task_name",
        alias = "title",
        alias = "标题",
        alias = "任务名称"
    )]
    pub title: String,
    #[serde(
        default,
        alias = "goal",
        alias = "desc",
        alias = "description",
        alias = "objective",
        alias = "目标",
        alias = "任务目标"
    )]
    pub objective: String,
    #[serde(default, alias = "interface", alias = "interfaces", alias = "接口")]
    pub interfaces: String,
    #[serde(
        default,
        alias = "depends_on",
        alias = "deps",
        alias = "dependencies",
        alias = "依赖"
    )]
    pub dependencies: Vec<String>,
    #[serde(
        default,
        alias = "requirement",
        alias = "requirements",
        alias = "要求",
        alias = "需求"
    )]
    pub requirements: String,
    /// Optional worker label (Backend / Frontend / Database /
    /// API / Testing / Documentation). When the chief picks one
    /// of these, the orchestrator still spawns a generic
    /// `agent:worker` agent but tags the task row + PhaseTimeline
    /// entry so the UI can group them.
    #[serde(
        default,
        alias = "role",
        alias = "tag",
        alias = "type",
        alias = "label",
        alias = "标签"
    )]
    pub label: String,
}

/// The structured Planning Doc chief produces in Phase 2 and
/// critic reviews in Phase 3 + Phase 6.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanDoc {
    #[serde(
        default,
        alias = "overview",
        alias = "desc",
        alias = "summary",
        alias = "概述",
        alias = "总结"
    )]
    pub summary: String,
    #[serde(
        default,
        alias = "arch",
        alias = "design",
        alias = "architecture",
        alias = "架构",
        alias = "架构设计"
    )]
    pub architecture: String,
    #[serde(
        default,
        alias = "taskList",
        alias = "task_list",
        alias = "subtasks",
        alias = "tasks",
        alias = "任务",
        alias = "任务列表"
    )]
    pub tasks: Vec<WorkerTask>,
}

impl PlanDoc {
    /// Merge another PlanDoc into this one. The summary /
    /// architecture fields are kept from `self` (the first
    /// round's authoritative text); tasks are appended.
    /// If an id collides with an existing task, it is auto-suffixed
    /// so subsequent round tasks (e.g. Frontend from Round C) are
    /// never dropped.
    pub fn merge(&mut self, other: &PlanDoc) {
        if self.summary.is_empty() && !other.summary.is_empty() {
            self.summary = other.summary.clone();
        }
        if self.architecture.is_empty() && !other.architecture.is_empty() {
            self.architecture = other.architecture.clone();
        }
        let mut existing_ids: std::collections::HashSet<String> =
            self.tasks.iter().map(|t| t.id.clone()).collect();
        for mut t in other.tasks.clone() {
            if existing_ids.contains(&t.id) {
                let mut suffix = 2;
                let mut new_id = format!("{}_{}", t.id, suffix);
                while existing_ids.contains(&new_id) {
                    suffix += 1;
                    new_id = format!("{}_{}", t.id, suffix);
                }
                t.id = new_id;
            }
            existing_ids.insert(t.id.clone());
            self.tasks.push(t);
        }
    }

    /// Best-effort JSON extractor. The chief is asked to wrap
    /// the PlanDoc in a fenced ```json block; we strip the
    /// fences, strip thinking blocks if present, and parse the
    /// `{...}` or `[...]` block. Falls back to a single-task
    /// doc only if all parsing attempts fail.
    pub fn from_chief_text(raw_text: &str, fallback_request: &str) -> Self {
        // Strip thinking blocks emitted by reasoning models (DeepSeek-R1, Qwen, etc.)
        let mut text_owned = raw_text.to_string();
        while let Some(start) = text_owned.find("<think>") {
            if let Some(end_rel) = text_owned[start..].find("</think>") {
                text_owned.replace_range(start..start + end_rel + 8, "");
            } else {
                // Unclosed <think>. Check if there is JSON after the opening tag
                if let Some(json_start) = text_owned[start + 7..]
                    .find("```json")
                    .or_else(|| text_owned[start + 7..].find('{'))
                    .or_else(|| text_owned[start + 7..].find('['))
                {
                    text_owned.replace_range(start..start + 7 + json_start, "");
                } else {
                    text_owned.truncate(start);
                }
                break;
            }
        }
        let text = text_owned.as_str();

        // 1. Try ```json ... ``` blocks
        if let Some(start) = text.find("```json") {
            if let Some(end_rel) = text[start + 7..].find("```") {
                let body = text[start + 7..start + 7 + end_rel].trim();
                if let Ok(parsed) = serde_json::from_str::<PlanDoc>(body) {
                    if !parsed.tasks.is_empty() || !parsed.summary.is_empty() {
                        return parsed;
                    }
                }
                if let Ok(tasks) = serde_json::from_str::<Vec<WorkerTask>>(body) {
                    if !tasks.is_empty() {
                        return PlanDoc {
                            summary: String::new(),
                            architecture: String::new(),
                            tasks,
                        };
                    }
                }
            }
        }

        // 2. Try any fenced ``` ... ``` blocks
        if let Some(start) = text.find("```") {
            let after_fence = &text[start + 3..];
            let after_lang = if let Some(newline) = after_fence.find('\n') {
                &after_fence[newline + 1..]
            } else {
                after_fence
            };
            if let Some(end_rel) = after_lang.find("```") {
                let body = after_lang[..end_rel].trim();
                if let Ok(parsed) = serde_json::from_str::<PlanDoc>(body) {
                    if !parsed.tasks.is_empty() || !parsed.summary.is_empty() {
                        return parsed;
                    }
                }
                if let Ok(tasks) = serde_json::from_str::<Vec<WorkerTask>>(body) {
                    if !tasks.is_empty() {
                        return PlanDoc {
                            summary: String::new(),
                            architecture: String::new(),
                            tasks,
                        };
                    }
                }
            }
        }

        // 3. Try finding outermost `{...}`
        if let Some(start) = text.find('{') {
            if let Some(end_rel) = text[start..].rfind('}') {
                let body = &text[start..start + end_rel + 1];
                if let Ok(parsed) = serde_json::from_str::<PlanDoc>(body) {
                    if !parsed.tasks.is_empty() || !parsed.summary.is_empty() {
                        return parsed;
                    }
                }
            }
        }

        // 4. Try finding outermost `[...]`
        if let Some(start) = text.find('[') {
            if let Some(end_rel) = text[start..].rfind(']') {
                let body = &text[start..start + end_rel + 1];
                if let Ok(tasks) = serde_json::from_str::<Vec<WorkerTask>>(body) {
                    if !tasks.is_empty() {
                        return PlanDoc {
                            summary: String::new(),
                            architecture: String::new(),
                            tasks,
                        };
                    }
                }
            }
        }

        // Fallback: a single worker that just does the literal
        // request. Better than crashing the workflow because
        // chief failed to format JSON.
        PlanDoc {
            summary: format!("single-task fallback for: {fallback_request}"),
            architecture: "no structured plan — chief did not emit JSON".into(),
            tasks: vec![WorkerTask {
                id: "w_fallback_0".into(),
                title: fallback_request.chars().take(40).collect(),
                objective: fallback_request.to_string(),
                interfaces: String::new(),
                dependencies: Vec::new(),
                requirements: String::new(),
                label: "Worker".into(),
            }],
        }
    }
}

/// One agent run. Multiple instances exist concurrently inside
/// a single phase (e.g. critic:a + critic:b in Phase 3).
pub struct AgentRunSpec {
    pub role: Role,
    pub task: String,
    /// Optional extra context passed to the LLM (e.g. "the plan
    /// under review is: <PlanDoc>").
    pub context: Option<String>,
}

/// Outcome of a single agent run. The orchestrator collects
/// these into `Vec<TaskOutcome>` per phase.
///
/// v0.4.22 (event 000115): `structured_verdict` carries a
/// JSON-block-parsed reviewer verdict when the role was a
/// critic (BugHunter / Reviewer) and the model emitted a
/// ```flowntier-verdict ... ``` block. Non-critic roles leave
/// this `None`. `verdict_of()` (and its replacement
/// `parse_verdict_from_text()`) always have `text` / `summary`
/// as fallback inputs, so the field is opt-in only.
pub struct TaskOutcome {
    pub role_id: String,
    pub role_display: String,
    pub status: String,
    pub summary: Option<String>,
    pub text: String,
    pub elapsed_ms: u64,
    pub structured_verdict: Option<ReviewerVerdictJson>,
}

/// v0.4.22 (event 000115): shape of the JSON block the critic
/// roles are instructed to emit at the end of their output.
/// Parsed out of ```flowntier-verdict ... ``` fences; if the
/// fence is missing or malformed we fall back to prose scan.
///
/// `confidence` is the critic's own self-rated confidence in
/// the verdict (0.0..1.0). `issues` is a flat list of one-line
/// problems ordered by severity — frontend's ReviewerCard
/// renders each as a bullet. `summary` is a one-sentence
/// rationale shown next to the verdict token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewerVerdictJson {
    pub verdict: String,
    pub confidence: f64,
    #[serde(default)]
    pub issues: Vec<String>,
    #[serde(default)]
    pub summary: String,
}

/// Top-level orchestrator handle. One Orchestrator owns a
/// single workflow (wf_id) and runs the 8 phases serially,
/// spawning parallel agents inside each phase as the spec
/// requires.
/// v0.4.37: retry policy for rate-limited agent runs. The backoff
/// (5s / 15s / 30s) spreads the burst that a parallel worker fan-out
/// creates against a per-minute token cap — previously a 429 storm
/// burned the whole fallback chain in milliseconds.
const RATE_LIMIT_MAX_RETRIES: usize = 3;

const RATE_LIMIT_BACKOFF_SECS: [u64; RATE_LIMIT_MAX_RETRIES] = [5, 15, 30];

/// v0.4.37: does a TaskOutcome status look like provider rate
/// limiting? SiliconFlow answers `50602 ... TPM limit reached`,
/// OpenAI-style providers answer plain 429 / "Too Many Requests".
pub(crate) fn status_is_rate_limited(status: &str) -> bool {
    let s = status.to_ascii_lowercase();
    s.contains("429")
        || s.contains("rate limit")
        || s.contains("too many requests")
        || s.contains("tpm limit")
        || s.contains("50602")
}

pub struct Orchestrator {
    pub(super) state: Arc<ServerState>,
    pub(super) events: broadcast::Sender<AgentEvent>,
    pub wf_id: String,
    pub user_request: String,
    /// Per-phase wall-clock start time (Instant::now). Used
    /// by `phase_finished` to log the elapsed time the
    /// chairman sees in the runtime log. Helps debug stalls
    /// (e.g. mimo:mimo-2.5-pro returning 401 in NWT 000081).
    pub(super) phase_started_at: std::time::Instant,
    /// Index of the currently-running phase (matches
    /// `PHASES[phase_idx]`). Used to label the
    /// `phase_finished` log line so the chairman can match
    /// it to the `phase_started` they saw.
    pub(super) current_phase: &'static str,
    /// v0.4.22 (event 000113): max times Phase 5 (develop)
    /// will re-run when Phase 6 reviewers return REPAIR /
    /// REWRITE. The loop is bounded so a critic that
    /// permanently flags REPAIR doesn't burn the chairman's
    /// budget. Default 3 (matches tauri-core default). When
    /// the cap is hit, the workflow proceeds to Phase 8 with
    /// the last `worker_results` and the terminal `Done`
    /// status reflects the unrepaired REPAIR / REWRITE
    /// verdict pair (handled by event 000114's
    /// `terminal_done_status`).
    pub(super) max_repair_loops: u32,
    /// v0.4.22 (event 000118, fix 7): external stop switch.
    /// Fired by `POST /api/workflow/cancel` so a stuck
    /// workflow can be interrupted within ~1s instead of
    /// waiting for the 5-min per-task timeout. `drive_single_agent`
    /// races this against the in-flight agent.run().
    pub(super) cancel_token: tokio_util::sync::CancellationToken,
    /// v0.4.37: first provider-infrastructure total failure (rate
    /// limit exhausted, provider error after all candidates) on a
    /// work-producing role. Once set, every subsequent `run_agent`
    /// short-circuits — the workflow is already doomed, and letting
    /// the remaining phases "complete" on zero LLM work produced a
    /// fake 8/8 delivery (observed live: a SiliconFlow TPM 429 storm
    /// sailed through all 8 phases in ~2s with nothing written).
    pub(super) abort_reason: parking_lot::Mutex<Option<String>>,
}

impl Orchestrator {
    /// Build a new orchestrator. Generates a stable wf_id so
    /// PhaseTimeline + tasks rows are linkable from the moment
    /// the workflow starts.
    pub fn new(
        state: Arc<ServerState>,
        events: broadcast::Sender<AgentEvent>,
        user_request: String,
        max_repair_loops: u32,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Self {
        // 12-char ulid-ish — collision-resistant enough for a
        // single process. Real wf_ids (legacy path) use full ULIDs.
        let now = chrono::Utc::now().timestamp_millis();
        let wf_id = format!("wf_{:x}_{}", now, rand_suffix());
        let phase_started_at = std::time::Instant::now();
        Self {
            state,
            events,
            wf_id,
            user_request,
            phase_started_at,
            current_phase: PHASES[0],
            max_repair_loops,
            cancel_token,
            abort_reason: parking_lot::Mutex::new(None),
        }
    }
}

mod phases;
mod run;
#[cfg(test)]
mod tests;
mod verdict;

// Bridge for the orchestrator's resolve_role call. Defined in
// handlers.rs to keep storage access local.
mod storage {
    pub use storage::*;
}

pub use verdict::{
    critic_verdict, parse_verdict_from_text, should_repair_decision, terminal_done_status,
};

use verdict::{map_phase_name, rand_suffix};
