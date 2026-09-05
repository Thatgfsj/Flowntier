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

/// v0.4.22 (event 000091 fix #23): one resolved candidate for
/// a (provider_short, model_id) pair, with everything needed
/// to build an `OpenAiProvider`. The orchestrator iterates
/// over a Vec of these when the primary fails — the chairman
/// can configure `fallback_chain: ["anthropic:claude-opus-4-8",
/// "minimax:MiniMax-Text-01"]` so a mimo 401 falls through to
/// anthropic / MiniMax instead of killing the whole phase.
struct ResolvedCandidate {
    provider_short: String,
    model_id: String,
    base_url: String,
    api_kind: String,
    secret_name: String,
    api_key: zeroize::Zeroizing<String>,
}

impl ResolvedCandidate {
    fn from_resolved(r: &crate::handlers::ResolvedRole) -> Self {
        Self {
            provider_short: r.provider_short.clone(),
            model_id: r.model_id.clone(),
            base_url: r.base_url.clone(),
            api_kind: r.api_kind.clone(),
            secret_name: r.secret_name.clone(),
            api_key: r.api_key.clone(),
        }
    }
}

/// Top-level orchestrator handle. One Orchestrator owns a
/// single workflow (wf_id) and runs the 8 phases serially,
/// spawning parallel agents inside each phase as the spec
/// requires.
pub struct Orchestrator {
    state: Arc<ServerState>,
    events: broadcast::Sender<AgentEvent>,
    pub wf_id: String,
    pub user_request: String,
    /// Per-phase wall-clock start time (Instant::now). Used
    /// by `phase_finished` to log the elapsed time the
    /// chairman sees in the runtime log. Helps debug stalls
    /// (e.g. mimo:mimo-2.5-pro returning 401 in NWT 000081).
    phase_started_at: std::time::Instant,
    /// Index of the currently-running phase (matches
    /// `PHASES[phase_idx]`). Used to label the
    /// `phase_finished` log line so the chairman can match
    /// it to the `phase_started` they saw.
    current_phase: &'static str,
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
    max_repair_loops: u32,
    /// v0.4.22 (event 000118, fix 7): external stop switch.
    /// Fired by `POST /api/workflow/cancel` so a stuck
    /// workflow can be interrupted within ~1s instead of
    /// waiting for the 5-min per-task timeout. `drive_single_agent`
    /// races this against the in-flight agent.run().
    cancel_token: tokio_util::sync::CancellationToken,
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
        }
    }

    /// Emit a phase transition. Always best-effort — if no
    /// subscribers are listening (e.g. headless build) the
    /// broadcast::send fails silently.
    ///
    /// Sends TWO event flavours so both UIs animate:
    /// 1. AgentEvent::PhaseTransition — `useAgentStream` hook
    ///    in ChatZone picks this up (kind: "phase_transition").
    /// 2. Plain JSON with `kind: "transition"` and WfEvent-shape
    ///    fields — App.tsx's transition handler picks this up.
    ///    We send via a generic JSON value; the events pipe
    ///    forwards anything that's newline-delimited JSON.
    async fn emit_phase(&mut self, from: Option<&str>, to: &str) {
        // v0.4.22 (event 000082): log the previous phase's
        // elapsed time before the new one starts, so the
        // chairman can see how long each phase took when
        // the workflow stalls (NWT 000081 root cause:
        // provider 401 → LLM never called → phase never
        // returned → no log line for it). Without this,
        // a stuck workflow looked like 'no new log lines
        // since the previous phase started'.
        if let Some(_prev) = from {
            let elapsed_ms = self.phase_started_at.elapsed().as_millis() as u64;
            info!(
                target: "orchestrator",
                wf_id = %self.wf_id,
                from_phase = %self.current_phase,
                to_phase = %to,
                phase_runtime_ms = elapsed_ms,
                "v0.4.22 (event 000082): phase completed"
            );
        } else {
            info!(
                target: "orchestrator",
                wf_id = %self.wf_id,
                to_phase = %to,
                "v0.4.22 (event 000082): workflow started"
            );
        }

        let _ = self.events.send(AgentEvent::PhaseTransition {
            wf_id: self.wf_id.clone(),
            from: from.map(|s| s.to_string()),
            to: to.to_string(),
        });
        info!(
            target: "orchestrator",
            wf_id = %self.wf_id,
            ?from,
            to = %to,
            "phase transition"
        );
        // v0.4.22 (event 000082): reset the phase timer for
        // the next phase's elapsed-time log.
        self.phase_started_at = std::time::Instant::now();
        self.current_phase = match to {
            "1-requirement" => "1-requirement",
            "2-plan" => "2-plan",
            "3-plan-review" => "3-plan-review",
            "4-dispatch" => "4-dispatch",
            "5-develop" => "5-develop",
            "6-final-review" => "6-final-review",
            "7-repair" => "7-repair",
            "8-delivery" => "8-delivery",
            _ => "unknown",
        };
        // v0.4.22 (event 000069): also update the workflows
        // row so GET /api/workflow/{wf_id}/status returns the
        // current phase without needing to scrape the events
        // pipe. Best-effort — log on failure but don't block
        // the phase transition.
        // v0.4.22 (event 000069): also update the workflows
        // row so GET /api/workflow/{wf_id}/status returns the
        // current phase without needing to scrape the events
        // pipe. Best-effort — log on failure but don't block
        // the phase transition.
        let wf_phase = map_phase_name(to);
        if let Err(e) = self
            .state
            .repo
            .update_workflow_state(&self.wf_id, "ACTIVE", &wf_phase)
            .await
        {
            warn!(target: "orchestrator", error = %e, wf_id = %self.wf_id, "update_workflow_state failed");
        }
    }

    /// v0.4.22 (event 000112): publish one critic's verdict as a
    /// first-class `AgentEvent::ReviewerVerdict` so the webview
    /// can replace its placeholder ReviewerCard with the real
    /// PASS/REPAIR/REWRITE token + summary instead of a hardcoded
    /// "通过 置信度 0.87".
    ///
    /// `phase` is `"plan-review"` or `"final-review"`. v0.4.22
    /// (event 000115): the verdict, confidence, issues and
    /// summary now come from the JSON block critics are
    /// instructed to emit (parsed by `parse_verdict_from_text`),
    /// not the legacy prose scan.
    fn emit_reviewer_verdict(&self, phase: &str, outcome: &TaskOutcome) {
        // v0.4.22 (event 000115): prefer the structured verdict
        // parsed out of the critic's JSON block. If the critic
        // didn't emit one (older model, timeout before reaching
        // the verdict, prose-only output), fall back to the
        // verdict_of() prose scan so the event still carries a
        // usable verdict token — confidence is 0.0 in that case.
        let (verdict, structured) = match outcome.structured_verdict.clone() {
            Some(v) => (v.verdict.clone(), v),
            None => parse_verdict_from_text(outcome),
        };
        let _ = self.events.send(AgentEvent::ReviewerVerdict {
            wf_id: self.wf_id.clone(),
            phase: phase.to_string(),
            role: outcome.role_id.clone(),
            verdict,
            confidence: structured.confidence,
            issues: structured.issues,
            summary: structured.summary,
        });
    }

    /// Partition worker tasks into sequential execution stages (batches) based on `dependencies`.
    /// Tasks within each stage have their dependencies satisfied by earlier stages and can run concurrently.
    pub fn partition_tasks_into_stages(tasks: &[WorkerTask]) -> Vec<Vec<usize>> {
        if tasks.is_empty() {
            return Vec::new();
        }
        let mut stages: Vec<Vec<usize>> = Vec::new();
        let mut completed: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut remaining: Vec<usize> = (0..tasks.len()).collect();

        while !remaining.is_empty() {
            let mut current_stage = Vec::new();
            let mut next_remaining = Vec::new();

            for &idx in &remaining {
                let t = &tasks[idx];
                // Check if all declared dependencies have been completed in earlier stages
                let ready = t.dependencies.iter().all(|dep| {
                    let d = dep.trim();
                    d.is_empty()
                        || completed.contains(d)
                        || !tasks.iter().any(|other| other.id.trim() == d)
                });
                if ready {
                    current_stage.push(idx);
                } else {
                    next_remaining.push(idx);
                }
            }

            if current_stage.is_empty() {
                // Cycle or deadlock fallback: execute remaining in one stage
                current_stage = next_remaining;
                remaining = Vec::new();
            } else {
                for &idx in &current_stage {
                    completed.insert(tasks[idx].id.trim().to_string());
                }
                remaining = next_remaining;
            }

            stages.push(current_stage);
        }

        stages
    }

    /// Phase 5 (develop) DAG runner: partitions tasks into stages by dependency,
    /// runs each stage concurrently, and passes upstream summaries and context
    /// downstream so workers can see what previous workers have built.
    async fn run_phase_5_workers(
        &self,
        plan: &PlanDoc,
        repair_context: Option<String>,
    ) -> Vec<TaskOutcome> {
        let stages = Self::partition_tasks_into_stages(&plan.tasks);
        let mut all_results: Vec<Option<TaskOutcome>> =
            (0..plan.tasks.len()).map(|_| None).collect();
        let mut upstream_context: Vec<String> = Vec::new();

        if !plan.architecture.is_empty() {
            upstream_context.push(format!("系统整体架构:\n{}", plan.architecture));
        }

        for (stage_idx, stage) in stages.iter().enumerate() {
            if self.cancel_token.is_cancelled() {
                break;
            }

            let mut stage_futures = Vec::new();
            for &idx in stage {
                let t = &plan.tasks[idx];
                let mut task_text = format!(
                    "任务: {}\n目标: {}\n接口: {}\n依赖: {:?}\n要求: {}\n\n只做这一件事, 不要碰其他 worker 的文件。完成后用一句话汇报。",
                    t.title, t.objective, t.interfaces, t.dependencies, t.requirements,
                );
                if let Some(ctx) = &repair_context {
                    task_text.push_str("\n\n# 修复循环反馈\n");
                    task_text.push_str(ctx);
                }

                let task_context = if !upstream_context.is_empty() {
                    Some(format!(
                        "上游任务产出与上下文 (阶段 {}/{}):\n{}",
                        stage_idx + 1,
                        stages.len(),
                        upstream_context.join("\n\n")
                    ))
                } else {
                    None
                };

                let task_id = Some(format!("t{idx}"));
                let fut = self.run_agent_with_task_id(
                    AgentRunSpec {
                        role: Role::Worker,
                        task: task_text,
                        context: task_context,
                    },
                    task_id,
                );
                stage_futures.push((idx, fut));
            }

            // Run tasks in this stage concurrently
            let stage_tasks: Vec<_> = stage_futures
                .into_iter()
                .map(|(idx, fut)| async move {
                    let res = fut.await;
                    (idx, res)
                })
                .collect();

            let stage_results = futures::future::join_all(stage_tasks).await;
            for (idx, outcome) in stage_results {
                let detail = outcome.summary.as_deref().unwrap_or_else(|| {
                    if outcome.text.len() > 300 {
                        &outcome.text[..300]
                    } else {
                        &outcome.text
                    }
                });
                let summary_line = format!(
                    "- [{}] {}: {}",
                    plan.tasks[idx].title, outcome.status, detail
                );
                upstream_context.push(summary_line);
                all_results[idx] = Some(outcome);
            }
        }

        all_results.into_iter().flatten().collect()
    }

    /// Phase 6 (final review) runner: provides reviewers with comprehensive
    /// summaries and details from all workers rather than tiny 120-char snippets.
    async fn run_phase_6_final_review(
        &self,
        plan: &PlanDoc,
        worker_results: &[TaskOutcome],
    ) -> (TaskOutcome, TaskOutcome) {
        let workers_summary: Vec<String> = worker_results
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let title = plan
                    .tasks
                    .get(i)
                    .map(|t| t.title.clone())
                    .unwrap_or_default();
                format!(
                    "[{}] {} ({}):\n产出总结: {}\n详细输出: {}",
                    title,
                    w.role_display,
                    w.status,
                    w.summary.clone().unwrap_or_default(),
                    w.text.chars().take(800).collect::<String>(),
                )
            })
            .collect();
        let review_ctx = workers_summary.join("\n\n---\n\n");
        tokio::join!(
            self.run_agent(AgentRunSpec {
                role: Role::BugHunter,
                task: "评审上面所有 worker 的产出 (找 bug / runtime 风险 / 编译或接口不一致)。给出: PASS / REPAIR / REWRITE 之一, 并列举具体问题。".into(),
                context: Some(review_ctx.clone()),
            }),
            self.run_agent(AgentRunSpec {
                role: Role::Reviewer,
                task: "评审上面所有 worker 的产出 (架构 / 维护性 / 代码完整度)。给出: PASS / REPAIR / REWRITE 之一, 并列举具体问题。".into(),
                context: Some(review_ctx),
            }),
        )
    }

    /// Spawn one agent run and return its outcome. The agent
    /// gets its own Workspace snapshot (so chief writing into
    /// workspace A doesn't leak into worker B's view — though
    /// they all share the same on-disk path; that's by design,
    /// so workers can read each other's artefacts via file ops
    /// but the spec says they shouldn't communicate via in-mem
    /// state).
    async fn run_agent(&self, spec: AgentRunSpec) -> TaskOutcome {
        self.run_agent_with_task_id(spec, None).await
    }

    /// v0.4.22 (event 000118, fix 3): like `run_agent` but
    /// attaches a per-task id (`t{idx}`) to every event
    /// emitted from the agent's loop. The frontend uses the
    /// tag to render N Phase-5 worker cards instead of one.
    async fn run_agent_with_task_id(
        &self,
        spec: AgentRunSpec,
        task_id: Option<String>,
    ) -> TaskOutcome {
        let role_id = spec.role.id().to_string();
        let role_display = spec.role.display().to_string();

        // v0.4.22 (event 000091 fix #23): build a list of
        // (provider, model_id, base_url, api_kind) candidates
        // from the primary default_model + the configured
        // fallback_chain. If the primary fails with a
        // retriable error (DNS, 401, 429, 5xx), try the next
        // candidate instead of letting the whole phase die.
        let candidates = match self.build_candidates(&role_id).await {
            Ok(c) if !c.is_empty() => c,
            Ok(_) => {
                warn!(target: "orchestrator", role = %role_id, "no candidates resolved; default_model missing or empty");
                return TaskOutcome {
                    role_id,
                    role_display,
                    status: "FAILED: no candidates resolved".into(),
                    summary: None,
                    text: String::new(),
                    elapsed_ms: 0,
                    structured_verdict: None,
                };
            }
            Err(e) => {
                warn!(target: "orchestrator", role = %role_id, error = %e, "resolve_role failed");
                return TaskOutcome {
                    role_id,
                    role_display,
                    status: format!("FAILED: resolve: {e}"),
                    summary: Some(format!("未配置模型或密钥: {e}")),
                    text: format!("错误: 角色未配置模型或密钥 ({e})"),
                    elapsed_ms: 0,
                    structured_verdict: None,
                };
            }
        };

        // Try each candidate in order. Stop on first success.
        let mut last_outcome: Option<TaskOutcome> = None;
        for (idx, cand) in candidates.iter().enumerate() {
            info!(
                target: "orchestrator",
                wf_id = %self.wf_id,
                role = %role_id,
                role_display = %role_display,
                candidate_idx = idx,
                provider_short = %cand.provider_short,
                model_id = %cand.model_id,
                base_url = %cand.base_url,
                api_kind = %cand.api_kind,
                "[TRACE] event 000091 fix #23: agent run starting (with fallback)"
            );

            let provider: Arc<dyn agent_core::Provider> = match cand.api_kind.as_str() {
                "openai" => Arc::new(agent_core::provider::openai::OpenAiProvider::openai(
                    cand.model_id.clone(),
                    cand.api_key.to_string(),
                )),
                "anthropic" | "anthropic-compatible" => {
                    let mut p = agent_core::provider::anthropic::AnthropicProvider::new(
                        cand.model_id.clone(),
                        cand.api_key.to_string(),
                    );
                    if !cand.base_url.is_empty() && cand.base_url != "https://api.anthropic.com" {
                        p.base_url = Some(cand.base_url.clone());
                    }
                    Arc::new(p)
                }
                _ => Arc::new(agent_core::provider::openai::OpenAiProvider::compat(
                    cand.base_url.clone(),
                    cand.model_id.clone(),
                    cand.api_key.to_string(),
                )),
            };
            let agent = agent_core::Agent::new(
                spec.role,
                provider,
                self.state.tools.clone(),
                self.state.workspace_snapshot(),
                agent_core::AgentConfig::default(),
            );
            let task = if let Some(ref ctx) = spec.context {
                format!("{}\n\n{}", spec.task, ctx)
            } else {
                spec.task.clone()
            };

            let outcome = self
                .drive_single_agent(
                    agent,
                    task,
                    role_id.clone(),
                    role_display.clone(),
                    task_id.clone(),
                )
                .await;
            // If the outcome looks like a retriable failure, log
            // it and try the next candidate. "DONE" / "ABORTED"
            // (user-cancelled) / "TIMEOUT" (5-min wall clock) are
            // not retriable — they reflect intent, not provider
            // health.
            //
            // v0.4.28 (event 000121): auth errors (401 / 403 /
            // "Invalid API Key") are NOT retriable — the same
            // key on a different provider preset won't
            // magically work. Burning the fallback chain on
            // auth just hides the real problem and turns one
            // error into N×70 ms of confusion (chief
            // appeared to "time out" while retry-loop spun).
            // Fast-fail with a clear `auth_error` status so
            // the UI can surface "API key 无效,请到
            // Settings → 供应商 配置 <secret_name>" instead of
            // letting the workflow spin until the per-phase
            // timeout.
            let is_auth_failure = outcome.status.contains("401")
                || outcome.status.contains("403")
                || outcome.status.contains("Invalid API Key")
                || outcome.status.contains("invalid_key")
                || outcome.status.contains("Authentication")
                || outcome.status.contains("PermissionDenied");
            let is_timeout = outcome.status.starts_with("TIMEOUT");

            // Check if there are any subsequent candidates with a different secret
            let next_candidates_have_different_key = candidates[idx + 1..]
                .iter()
                .any(|next_c| next_c.secret_name != cand.secret_name);

            let retriable = (outcome.status.starts_with("FAILED") || is_timeout)
                && !outcome.status.starts_with("FAILED: abort")
                && !outcome.status.starts_with("FAILED: cancel")
                && (!is_auth_failure || next_candidates_have_different_key);

            if is_auth_failure && !next_candidates_have_different_key {
                error!(
                    target: "orchestrator",
                    wf_id = %self.wf_id,
                    role = %role_id,
                    provider = %cand.provider_short,
                    secret_name = %cand.secret_name,
                    base_url = %cand.base_url,
                    "v0.4.28 (event 000121): auth failure — no alternative key in candidates. \
                     fix in Settings → 供应商 (provider={}, secret={})",
                    cand.provider_short, cand.secret_name,
                );
                return TaskOutcome {
                    role_id: outcome.role_id,
                    role_display: outcome.role_display,
                    status: format!(
                        "FAILED: auth_error (provider={}, secret={}): {}",
                        cand.provider_short, cand.secret_name, outcome.status
                    ),
                    summary: outcome.summary,
                    text: outcome.text,
                    elapsed_ms: outcome.elapsed_ms,
                    structured_verdict: outcome.structured_verdict,
                };
            }
            if !retriable {
                return outcome;
            }
            warn!(
                target: "orchestrator",
                wf_id = %self.wf_id,
                role = %role_id,
                candidate_idx = idx,
                status = %outcome.status,
                "event 000091 fix #23: candidate failed; trying next"
            );
            last_outcome = Some(outcome);
        }
        // All candidates exhausted — return the last failure.
        last_outcome.unwrap_or_else(|| TaskOutcome {
            role_id,
            role_display,
            status: "FAILED: all candidates exhausted".into(),
            summary: None,
            text: String::new(),
            elapsed_ms: 0,
            structured_verdict: None,
        })
    }

    /// Build the list of (provider, model, base_url, api_kind)
    /// candidates. Primary = `default_model`. Fallback = each
    /// entry in `fallback_chain` (which is itself in
    /// "<provider>:<model>" form). Both are resolved against
    /// the preset table. API key is taken from the matching
    /// preset's `secret_name` in the OS keystore. Returns an
    /// error only if the primary can't be resolved; the
    /// fallback is best-effort (skip entries with a missing
    /// preset or no API key, with a warn log).
    async fn build_candidates(&self, role_id: &str) -> Result<Vec<ResolvedCandidate>, String> {
        let primary = crate::handlers::resolve_role_for_orchestrator(&self.state, role_id).await?;
        let mut out = vec![ResolvedCandidate::from_resolved(&primary)];
        for fb in &primary.fallback_chain {
            match self.resolve_fallback_string(fb).await {
                Ok(c) => {
                    info!(
                        target: "orchestrator", role = %role_id,
                        fallback = %fb, "added fallback candidate"
                    );
                    out.push(c);
                }
                Err(e) => warn!(
                    target: "orchestrator", role = %role_id,
                    fallback = %fb, error = %e,
                    "fallback candidate unresolvable; skipping"
                ),
            }
        }
        Ok(out)
    }

    /// Resolve a single "<provider>:<model>" string into a
    /// candidate (uses the preset's base_url + secret_name).
    async fn resolve_fallback_string(&self, fb: &str) -> Result<ResolvedCandidate, String> {
        let (provider_short, model_id) = match fb.split_once(':') {
            Some((p, m)) => (p.to_string(), m.to_string()),
            None => {
                return Err(format!(
                    "fallback '{fb}' must be in '<provider>:<model>' form"
                ))
            }
        };
        let preset = crate::providers::get(&provider_short)
            .ok_or_else(|| format!("unknown provider preset '{provider_short}'"))?;
        let api_key: zeroize::Zeroizing<String> =
            match self.state.secrets.reveal(preset.secret_name).await {
                Ok(z) if !z.is_empty() => z,
                _ => return Err(format!("no API key for {}", preset.secret_name)),
            };
        Ok(ResolvedCandidate {
            provider_short,
            model_id,
            base_url: preset.base_url.to_string(),
            api_kind: preset.kind.to_string(),
            secret_name: preset.secret_name.to_string(),
            api_key,
        })
    }
}

/// v0.4.22 (event 000118, fix 7): inner-loop outcome tag
/// for `drive_single_agent`. Lives outside the impl so
/// nested async closures (returned via `tokio::select!`)
/// can name it without `Self::` qualification.
///   - Terminal: agent emitted Done{...}, loop returned
///   - Cancelled: cancel_token fired, loop returned early
///   - Exhausted: rx channel closed without Done (rare)
///
/// The outer `tokio::time::timeout` reports Err on the
/// 5-min ceiling — distinct from the three above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutcomeShape {
    Terminal,
    Cancelled,
    Exhausted,
}

impl Orchestrator {
    /// Run one agent and collect its outcome (extracted from
    /// the original `run_agent` body so the fallback loop can
    /// call it multiple times).
    async fn drive_single_agent(
        &self,
        agent: agent_core::Agent,
        task: String,
        role_id: String,
        role_display: String,
        task_id: Option<String>,
    ) -> TaskOutcome {
        let start = Instant::now();
        // v0.4.22 (event 000118, fix 7): race the in-flight
        // agent against the orchestrator's external cancel
        // token (fired by POST /api/workflow/cancel from the
        // Stop button). When the user clicks Stop, this future
        // resolves immediately with `cancelled=true` and we
        // synthesize a Done{ABORTED} event so the run returns
        // in ~1s instead of waiting for the 5-min timeout.
        let cancel_token = self.cancel_token.clone();
        // v0.4.26 (event 000119): extract metadata before `task` and
        // `task_id` are moved into `run_with_task_id` — we use them
        // in the "agent run started" log line below.
        let task_len = task.len();
        let task_id_str = task_id.as_deref().unwrap_or("").to_string();
        let mut rx = agent.run_with_task_id(task, task_id);
        let mut text = String::new();
        let mut last_status = "UNKNOWN".to_string();
        let mut summary: Option<String> = None;
        // v0.4.22 (event 000118, fix 5): per-phase TextDelta
        // counter so debugging "ChatZone transcript 空" is a
        // one-line log read instead of re-running the whole
        // workflow. The chairman's fix5 report cited phase 8
        // (chief 5-second delivery) as the smoke case — this
        // shows how many text_deltas actually streamed.
        let mut delta_count: u32 = 0;
        let mut last_delta_len: u32 = 0;
        // v0.4.26 (event 000119): per-role detail counters — the
        // chairman wants detailed per-role logs, so we record
        // how many tool calls, deltas, and final text lands in
        // each role's file. Cheap (u32 increments) and helps
        // diagnose "did this agent actually do anything?" without
        // re-running the workflow.
        let mut tool_started_count: u32 = 0;
        let mut tool_finished_count: u32 = 0;
        // v0.4.26 (event 000119): session-level start line — goes
        // to the agent's own log file so the chairman can grep
        // `chief.log` for "agent run started" and jump to the
        // trace. Captures role + task_id + task length so we can
        // spot runaway prompts.
        role_info!(
            self,
            &role_id,
            wf_id = %self.wf_id,
            role = %role_id,
            role_display = %role_display,
            task_id = %task_id_str,
            task_len,
            "v0.4.26 (event 000119): agent run started"
        );
        // 5-minute per-agent ceiling so a runaway critic doesn't
        // block the orchestrator forever. Per-task budget is
        // enforced at the run_task handler; per-agent is new in
        // event 000068 because critics + workers run unattended.
        let outcome = tokio::time::timeout(Duration::from_secs(300), async {
            loop {
                // tokio::select! races the rx channel, the
                // 5-min outer timeout, and the cancel
                // token. Whichever fires first wins. The
                // cancel branch synthesizes a Done{ABORTED}
                // and exits the loop.
                tokio::select! {
                    biased;
                    _ = cancel_token.cancelled() => {
                        // v0.4.26 (event 000119): per-role target
                        // so this lands in the agent's own log
                        // file (chief.log / critic-a.log / etc),
                        // not in the system.log noise.
                        role_info!(
                            self,
                            &role_id,
                            wf_id = %self.wf_id,
                            role = %role_id,
                            "v0.4.22 (event 000118 fix 7): cancel_token fired during phase"
                        );
                        let _ = self.events.send(AgentEvent::Done {
                            wf_id: self.wf_id.clone(),
                            status: "ABORTED".into(),
                            summary: Some("workflow cancelled by user".into()),
                        });
                        last_status = "ABORTED".into();
                        summary = Some("workflow cancelled by user".into());
                        return OutcomeShape::Cancelled;
                    }
                    ev_opt = rx.recv() => {
                        let Some(ev) = ev_opt else {
                            // Channel closed (run finished).
                            return OutcomeShape::Exhausted;
                        };
                        let _ = self.events.send(ev.clone());
                        match ev {
                            AgentEvent::TextDelta { delta, .. } => {
                                delta_count += 1;
                                last_delta_len = delta.len() as u32;
                                text.push_str(&delta);
                                // v0.4.26 (event 000119): per-role
                                // detail log on every text delta —
                                // the chairman wants detailed logs
                                // ("日志详细写一下"). We emit
                                // every delta at debug level so the
                                // runtime.log stream still shows
                                // the full stream, but chief.log /
                                // critic-*.log / worker.log stay
                                // scannable.
                                //
                                // Preview is truncated to 80 chars
                                // so a 5KB delta doesn't bloat the
                                // JSON file; the chairman can
                                // re-run with RUST_LOG=trace for
                                // full payloads.
                                role_info!(
                                    self,
                                    &role_id,
                                    wf_id = %self.wf_id,
                                    role = %role_id,
                                    delta_count,
                                    delta_len = delta.len(),
                                    preview = %delta.chars().take(80).collect::<String>(),
                                    "text delta"
                                );
                            }
                            AgentEvent::ToolStarted { call, .. } => {
                                tool_started_count += 1;
                                role_info!(
                                    self,
                                    &role_id,
                                    wf_id = %self.wf_id,
                                    role = %role_id,
                                    tool_started_count,
                                    tool_name = %call.name,
                                    "tool started"
                                );
                            }
                            AgentEvent::ToolFinished { preview, elapsed_ms, .. } => {
                                tool_finished_count += 1;
                                role_info!(
                                    self,
                                    &role_id,
                                    wf_id = %self.wf_id,
                                    role = %role_id,
                                    tool_finished_count,
                                    elapsed_ms,
                                    preview = %preview.chars().take(80).collect::<String>(),
                                    "tool finished"
                                );
                            }
                            AgentEvent::Done { status, summary: s, .. } => {
                                last_status = status;
                                summary = s;
                                // v0.4.26 (event 000119): per-role
                                // targets so the streaming trace
                                // and the run-finished line land
                                // in the author's log file. The
                                // chairman can `tail -f chief.log`
                                // to watch the chief's progress
                                // without grepping system noise.
                                role_info!(
                                    self,
                                    &role_id,
                                    wf_id = %self.wf_id,
                                    role = %role_id,
                                    status = %last_status,
                                    text_len = text.len(),
                                    delta_count,
                                    last_delta_len,
                                    "v0.4.22 (event 000118 fix 5): phase streaming trace"
                                );
                                role_info!(
                                    self,
                                    &role_id,
                                    wf_id = %self.wf_id,
                                    role = %role_id,
                                    status = %last_status,
                                    text_len = text.len(),
                                    "v0.4.22 (event 000082): agent run finished"
                                );
                                if last_status == "DONE"
                                    || last_status.starts_with("FAILED")
                                    || last_status.starts_with("ABORTED")
                                    || last_status.starts_with("TIMEOUT")
                                {
                                    return OutcomeShape::Terminal;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        })
        .await;

        let timed_out = outcome.is_err();
        let cancelled = matches!(outcome, Ok(OutcomeShape::Cancelled));

        let status = if cancelled {
            "ABORTED".to_string()
        } else if timed_out
            && !(last_status == "DONE"
                || last_status.starts_with("FAILED")
                || last_status.starts_with("ABORTED"))
        {
            let _ = self.events.send(AgentEvent::Done {
                wf_id: self.wf_id.clone(),
                status: "TIMEOUT (300s)".to_string(),
                summary: Some(format!("agent {role_display} exceeded 300s")),
            });
            "TIMEOUT (300s)".into()
        } else {
            last_status
        };

        // v0.4.26 (event 000119): per-role terminal summary line,
        // written to the agent's own log file with the final
        // status + every counter we tracked. The chairman can grep
        // `chief.log` for "agent run finished" and see the full
        // picture (deltas, tool calls, final text length, total
        // runtime) without reading system.log.
        let started = start.elapsed();
        role_info!(
            self,
            &role_id,
            wf_id = %self.wf_id,
            role = %role_id,
            role_display = %role_display,
            status = %status,
            text_len = text.len(),
            delta_count,
            tool_started_count,
            tool_finished_count,
            elapsed_ms = started.as_millis() as u64,
            summary_len = summary.as_deref().map(str::len).unwrap_or(0),
            summary_preview = %summary
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect::<String>(),
            "v0.4.26 (event 000119): agent run finished (per-role summary)"
        );

        // v0.4.22 (event 000115): only BugHunter + Reviewer are
        // expected to emit a verdict block. Chief / Planner /
        // Worker / Reporter leave this None — the orchestrator
        // only consults structured_verdict on critic outcomes
        // (event 000112 emit + final terminal status mapping),
        // and a non-critic with a verdict block is treated as
        // advisory prose.
        //
        // We match on `role_id` (a `String` propagated up from
        // `run_agent`) rather than `Role` because this fn
        // `drive_single_agent` only receives the id — re-binding
        // the Role would mean re-deciding a routing choice
        // already made by `run_agent`'s caller. The id set is
        // stable (see `Role::id` in agent-core/src/prompt).
        let is_critic = role_id == "agent:critic:a" || role_id == "agent:critic:b";
        let structured_verdict = if is_critic {
            Some(
                parse_verdict_from_text(&TaskOutcome {
                    role_id: role_id.clone(),
                    role_display: role_display.clone(),
                    status: status.clone(),
                    summary: summary.clone(),
                    text: text.clone(),
                    elapsed_ms: 0,
                    structured_verdict: None,
                })
                .1,
            )
        } else {
            None
        };

        TaskOutcome {
            role_id,
            role_display,
            status,
            summary,
            text,
            elapsed_ms: start.elapsed().as_millis() as u64,
            structured_verdict,
        }
    }

    /// Run the full 8-phase workflow. Returns the final summary
    /// string the chairman sees.
    pub async fn run(mut self) -> String {
        info!(target: "orchestrator", wf_id = %self.wf_id, user_request_len = self.user_request.len(), "[TRACE] Orchestrator::run() ENTERED — starting 8-phase workflow");

        // Pre-flight check: ensure chief role has a configured model and API key
        if let Err(e) = self.build_candidates("agent:chief").await {
            error!(target: "orchestrator", wf_id = %self.wf_id, error = %e, "Chief role not configured — aborting workflow immediately");
            let error_msg = format!("未配置 AI 模型或 API Key：请前往「设置 → 供应商与密钥」配置 API Key 并分配模型。\n({e})");
            let _ = self.events.send(AgentEvent::Done {
                wf_id: self.wf_id.clone(),
                status: "FAILED: role_not_configured".into(),
                summary: Some(error_msg.clone()),
            });
            let _ = self
                .state
                .repo
                .update_workflow_state(&self.wf_id, "FAILED: role_not_configured", "1-requirement")
                .await;
            let _ = self
                .state
                .repo
                .set_workflow_summary(&self.wf_id, &error_msg)
                .await;
            return error_msg;
        }

        let mut phase_idx = 0;
        self.emit_phase(None, PHASES[phase_idx]).await;

        // ── Phase 1: requirement analysis ─────────────────
        let chief_clarify = self.run_agent(AgentRunSpec {
            role: Role::Chief,
            task: format!(
                "用户需求:{}\n\n只做一件事: 1-3 句判断需求是否清楚。\n如果清楚, 直接说 OK 准备进入下一阶段; 如果不清楚, 追问 1-3 个关键问题。\n不要做规划, 不要做拆任务。\n\nv0.4.22 (event 000118, fix 6): 当用户只说了一句很短的话 (例如 \"打开给我看看\" / \"运行上次的工作\" / \"查看项目根目录\"), 先用 1-2 次 grep/read 看一眼项目根目录 (README / package.json / 当前目录列表) 判断上下文, 再决定是追问还是直接 OK 进入 Phase 2。不要在没看上下文的情况下无脑追问。",
                self.user_request,
            ),
            context: None,
        }).await;
        self.persist_task_row(&chief_clarify, "1-requirement", 0)
            .await;
        phase_idx = 1;
        self.emit_phase(Some(PHASES[0]), PHASES[phase_idx]).await;

        // ── Phase 2: planning (segmented) ─────────────────
        // v0.4.22 (event 000069): the previous monolithic Plan
        // phase asked the chief to dump the entire PlanDoc in
        // one LLM turn. For large requests (e.g. "build a 78-card
        // tarot app") that's >5min of chief token streaming and
        // the chief runs out of budget mid-way, leaving the
        // PlanDoc half-formed. We now split the Plan phase into
        // 3 rounds:
        //
        //   Round A: summary + architecture (one short call)
        //   Round B: Backend / API / Database / Worker tasks
        //   Round C: Frontend / Testing / Documentation tasks
        //
        // Each round's response is wrapped in ```json``` and
        // gets merged into the cumulative PlanDoc via
        // PlanDoc::merge(). If a round fails (timeout, no JSON),
        // we accept what we got and proceed with the partial
        // doc — the next phase will surface that as PASS with
        // a note.
        let plan_round_a = self.run_agent(AgentRunSpec {
            role: Role::Chief,
            task: format!(
                "用户需求:{}\n\n任务: 输出 PlanDoc 的上半部分。严格按 JSON 格式包在 ```json 围栏里:\n```json\n{{\n  \"summary\": \"<一段中文总结>\",\n  \"architecture\": \"<一段中文架构说明>\",\n  \"tasks\": []\n}}\n```\n只输出 summary + architecture, **不要输出 tasks 数组**(下一轮再加 tasks)。控制在 200 字以内。",
                self.user_request,
            ),
            context: None,
        }).await;
        self.persist_task_row(&plan_round_a, "2-plan-A-summary", 0)
            .await;
        let mut plan = PlanDoc::from_chief_text(&plan_round_a.text, &self.user_request);

        let plan_round_b = self.run_agent(AgentRunSpec {
            role: Role::Chief,
            task: format!(
                "用户需求:{}\n\n已知架构:\n```\n{}\n```\n\n任务: 输出后端 / API / 数据库 / 数据 / 算法 这一类任务, 严格按 JSON 格式包在 ```json 围栏里:\n```json\n{{\n  \"tasks\": [\n    {{\n      \"id\": \"w_<unique>\",\n      \"title\": \"<中文短标题>\",\n      \"label\": \"Backend|API|Database|Worker\",\n      \"objective\": \"<本 worker 的目标, 一句话>\",\n      \"interfaces\": \"<输入输出接口说明, 可空>\",\n      \"dependencies\": [],\n      \"requirements\": \"<编码要求, 可空>\"\n    }}\n  ]\n}}\n```\n**只输出 tasks 数组, 不要重复 summary / architecture**。",
                self.user_request, plan.architecture,
            ),
            context: None,
        }).await;
        self.persist_task_row(&plan_round_b, "2-plan-B-backend", 0)
            .await;
        let plan_b = PlanDoc::from_chief_text(&plan_round_b.text, &self.user_request);
        plan.merge(&plan_b);

        let plan_round_c = self.run_agent(AgentRunSpec {
            role: Role::Chief,
            task: format!(
                "用户需求:{}\n\n已知架构:\n```\n{}\n```\n\n已有任务(后端/API):\n```\n{}\n```\n\n任务: 输出前端 / 测试 / 文档 这一类任务, 严格按 JSON 格式包在 ```json 围栏里:\n```json\n{{\n  \"tasks\": [\n    {{\n      \"id\": \"w_<unique>\",\n      \"title\": \"<中文短标题>\",\n      \"label\": \"Frontend|Testing|Documentation\",\n      \"objective\": \"<本 worker 的目标, 一句话>\",\n      \"interfaces\": \"<输入输出接口说明, 可空>\",\n      \"dependencies\": [<可填上面的 task id>],\n      \"requirements\": \"<编码要求, 可空>\"\n    }}\n  ]\n}}\n```\n**只输出 tasks 数组**。",
                self.user_request, plan.architecture, serde_json::to_string_pretty(&plan.tasks).unwrap_or_default(),
            ),
            context: None,
        }).await;
        self.persist_task_row(&plan_round_c, "2-plan-C-frontend", 0)
            .await;
        let plan_c = PlanDoc::from_chief_text(&plan_round_c.text, &self.user_request);
        plan.merge(&plan_c);

        // If specific tasks were successfully extracted, drop any fallback placeholder from Round A
        if plan.tasks.iter().any(|t| t.id != "w_fallback_0") {
            plan.tasks.retain(|t| t.id != "w_fallback_0");
        }
        if let Ok(json_str) = serde_json::to_string(&plan) {
            let _ = self
                .state
                .repo
                .set_workflow_plan_doc(&self.wf_id, &json_str)
                .await;
        }

        // ── Phase 3: plan review (parallel) ──────────────
        phase_idx = 2;
        self.emit_phase(Some(PHASES[1]), PHASES[phase_idx]).await;
        let plan_ctx = format!(
            "需要评审的 PlanDoc:\n```json\n{}\n```",
            serde_json::to_string_pretty(&plan).unwrap_or_default()
        );
        let (critic_a, critic_b) = tokio::join!(
            self.run_agent(AgentRunSpec {
                role: Role::BugHunter,
                task: "评审上述 PlanDoc 的 bug / runtime 风险 / 边界条件。给出: PASS / REPAIR / REWRITE 之一, 加一句话理由。".into(),
                context: Some(plan_ctx.clone()),
            }),
            self.run_agent(AgentRunSpec {
                role: Role::Reviewer,
                task: "评审上述 PlanDoc 的架构合理性与完整性。给出: PASS / REPAIR / REWRITE 之一, 加一句话理由。".into(),
                context: Some(plan_ctx),
            }),
        );
        self.persist_task_row(&critic_a, "3-plan-review-A", 0).await;
        self.persist_task_row(&critic_b, "3-plan-review-B", 0).await;
        // v0.4.22 (event 000112): publish each critic's verdict
        // as an AgentEvent::ReviewerVerdict so the webview can
        // render the real verdict (front-end also uses the
        // final-review verdict to populate the ReviewerCard).
        self.emit_reviewer_verdict("plan-review", &critic_a);
        self.emit_reviewer_verdict("plan-review", &critic_b);

        // ── Phase 4: dispatch (chief declares worker list) ──
        phase_idx = 3;
        self.emit_phase(Some(PHASES[2]), PHASES[phase_idx]).await;
        let plan_review = format!(
            "Critic A: {} → {}\nCritic B: {} → {}",
            critic_a.role_display,
            critic_verdict(&critic_a),
            critic_b.role_display,
            critic_verdict(&critic_b),
        );
        let dispatch = self.run_agent(AgentRunSpec {
            role: Role::Chief,
            task: format!(
                "根据两名审核员的意见:\n{plan_review}\n调整并最终确认任务列表。以 ```json 代码块输出确认后的 PlanDoc。"
            ),
            context: Some(serde_json::to_string_pretty(&plan).unwrap_or_default()),
        }).await;
        self.persist_task_row(&dispatch, "4-dispatch", 0).await;

        // Parse any modified plan or tasks from Chief's dispatch response
        let updated_plan = PlanDoc::from_chief_text(&dispatch.text, &self.user_request);
        if !updated_plan.tasks.is_empty() && updated_plan.tasks[0].id != "w_fallback_0" {
            plan.tasks = updated_plan.tasks;
            if !updated_plan.summary.is_empty() {
                plan.summary = updated_plan.summary;
            }
            if !updated_plan.architecture.is_empty() {
                plan.architecture = updated_plan.architecture;
            }
        }
        if let Ok(json_str) = serde_json::to_string(&plan) {
            let _ = self
                .state
                .repo
                .set_workflow_plan_doc(&self.wf_id, &json_str)
                .await;
        }

        // ── Phase 5: develop (workers in parallel) ─────────
        phase_idx = 4;
        self.emit_phase(Some(PHASES[3]), PHASES[phase_idx]).await;
        let mut worker_results = self.run_phase_5_workers(&plan, None).await;
        for (i, w) in worker_results.iter().enumerate() {
            let title = plan
                .tasks
                .get(i)
                .map(|t| t.title.clone())
                .unwrap_or_default();
            self.persist_task_row(w, &format!("5-develop-{}", title), 0)
                .await;
        }

        // ── Phase 6 + 7: final review + repair loop ────────
        // v0.4.22 (event 000113): the historical stub let chief
        // emit a one-line "REPAIR needed on worker X" note and
        // then proceeded straight to Phase 8 — workers never
        // re-ran, the chairman saw a "DONE" status with the
        // same broken artefacts. We now loop: if any critic
        // returns REPAIR / REWRITE, rebuild worker prompts with
        // the critic feedback appended and re-run Phase 5.
        // Bounded by `max_repair_loops` (default 3) so a
        // permanently-flagging critic can't burn the budget.
        phase_idx = 5;
        self.emit_phase(Some(PHASES[4]), PHASES[phase_idx]).await;
        let mut loop_count: u32 = 0;
        let (final_a, final_b) = loop {
            let (a, b) = self.run_phase_6_final_review(&plan, &worker_results).await;
            self.persist_task_row(
                &a,
                &format!("6-final-review-criticA-loop{loop_count}"),
                loop_count + 1,
            )
            .await;
            self.persist_task_row(
                &b,
                &format!("6-final-review-criticB-loop{loop_count}"),
                loop_count + 1,
            )
            .await;
            // v0.4.22 (event 000112): publish final-review
            // verdicts so the webview's ReviewerCard updates
            // for every loop, not just the first.
            self.emit_reviewer_verdict("final-review", &a);
            self.emit_reviewer_verdict("final-review", &b);
            let verdict_a = critic_verdict(&a);
            let verdict_b = critic_verdict(&b);
            let is_fatal_critic_failure = a.status.starts_with("FAILED: resolve")
                || b.status.starts_with("FAILED: resolve")
                || a.status.contains("auth_error")
                || b.status.contains("auth_error");
            if self.cancel_token.is_cancelled()
                || a.status == "ABORTED"
                || b.status == "ABORTED"
                || is_fatal_critic_failure
            {
                break (a, b);
            }
            if !should_repair_decision(&verdict_a, &verdict_b, loop_count, self.max_repair_loops) {
                if loop_count >= self.max_repair_loops
                    && !(verdict_a == "PASS" && verdict_b == "PASS")
                {
                    warn!(
                        target: "orchestrator",
                        wf_id = %self.wf_id,
                        loop_count,
                        verdict_a = %verdict_a,
                        verdict_b = %verdict_b,
                        "v0.4.22 (event 000113): max_repair_loops reached; \
                         proceeding to delivery with last worker_results"
                    );
                }
                break (a, b);
            }
            // ── Phase 7: repair — re-run workers with feedback ──
            phase_idx = 6;
            self.emit_phase(Some(PHASES[5]), PHASES[phase_idx]).await;
            // v0.4.22 (event 000113): persist a chief "repair
            // decision" row so the dashboard 任务列表 shows the
            // round-trip, not just the silent retry.
            let repair_decision = self.run_agent(AgentRunSpec {
                role: Role::Chief,
                task: format!(
                    "Repair decision loop {}:\nFinal Critic A: {} → {}\nFinal Critic B: {} → {}\n\n决定: 哪些 worker 需要重做, 一句话即可。",
                    loop_count + 1,
                    a.role_display, verdict_a,
                    b.role_display, verdict_b,
                ),
                context: None,
            }).await;
            self.persist_task_row(
                &repair_decision,
                &format!("7-repair-loop{}", loop_count + 1),
                loop_count + 1,
            )
            .await;
            // v0.4.22 (event 000113): emit a dedicated event so
            // the webview can show "修复循环 2/3" instead of
            // guessing from repeated 5-develop rows.
            let issues_a_list = a
                .structured_verdict
                .clone()
                .map(|v| v.issues)
                .unwrap_or_default();
            let issues_b_list = b
                .structured_verdict
                .clone()
                .map(|v| v.issues)
                .unwrap_or_default();
            let _ = self.events.send(AgentEvent::RepairLoop {
                wf_id: self.wf_id.clone(),
                loop_index: loop_count + 1,
                max_loops: self.max_repair_loops,
                verdict_a: verdict_a.clone(),
                verdict_b: verdict_b.clone(),
                issues_a: issues_a_list.clone(),
                issues_b: issues_b_list.clone(),
            });
            // Rebuild worker prompts with the detailed critic feedback
            // appended, then re-run Phase 5. The next loop
            // iteration's Phase 6 will see the new outputs.
            let issues_a_str = if issues_a_list.is_empty() {
                a.text.chars().take(500).collect::<String>()
            } else {
                format!("- {}", issues_a_list.join("\n- "))
            };
            let issues_b_str = if issues_b_list.is_empty() {
                b.text.chars().take(500).collect::<String>()
            } else {
                format!("- {}", issues_b_list.join("\n- "))
            };
            let repair_ctx = format!(
                "Critic A ({}) 裁决: {}\n审查发现缺陷:\n{}\n\nCritic B ({}) 裁决: {}\n架构问题与建议:\n{}\n\n主理修复决策: {}\n\n请针对上述问题修复文件与代码。",
                a.role_display, verdict_a, issues_a_str,
                b.role_display, verdict_b, issues_b_str,
                repair_decision.text.chars().take(200).collect::<String>(),
            );
            worker_results = self.run_phase_5_workers(&plan, Some(repair_ctx)).await;
            for (i, w) in worker_results.iter().enumerate() {
                let title = plan
                    .tasks
                    .get(i)
                    .map(|t| t.title.clone())
                    .unwrap_or_default();
                self.persist_task_row(
                    w,
                    &format!("5-develop-loop{}-{}", loop_count + 1, title),
                    loop_count + 1,
                )
                .await;
            }
            loop_count += 1;
            // Re-emit Phase 6 transition so the webview's
            // PhaseTimeline shows the loop back to "final-review".
            phase_idx = 5;
            self.emit_phase(Some(PHASES[6]), PHASES[phase_idx]).await;
        };

        // ── Phase 8: delivery ─────────────────────────────
        phase_idx = 7;
        self.emit_phase(Some(PHASES[6]), PHASES[phase_idx]).await;
        let workers_summary = worker_results
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let task_title = plan
                    .tasks
                    .get(i)
                    .map(|t| t.title.as_str())
                    .unwrap_or("任务");
                let snippet: String = w.text.chars().take(200).collect();
                format!(
                    "- 任务「{}」(状态: {}): 摘要: {}",
                    task_title, w.status, snippet
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let delivery = self.run_agent(AgentRunSpec {
            role: Role::Chief,
            task: format!(
                "用 4-6 段中文给用户看这次 workflow 的产出。\
                 每段先讲结果再讲关键证据, 不要列代码, 不要列任务细节, 段落之间留一行空行:\n\
                 - 用户需求:{}\n- 计划:{}\n- Worker 数量:{}\n- Critic 评审: A={}, B={}\n- 最终状态: PASS or REPAIR\n\n\
                 第一段先告诉用户「做了什么」, 第二段「关键证据」, 第三段「评审结论」, 后续段落补建议 / 风险 / 下一步。",
                self.user_request,
                plan.summary,
                plan.tasks.len(),
                critic_verdict(&final_a),
                critic_verdict(&final_b),
            ),
            context: Some(format!("Worker 实际执行产出列表:\n{}", workers_summary)),
        }).await;
        self.persist_task_row(&delivery, "8-delivery", 0).await;

        // v0.4.22 (event 000114): the workflow's terminal status
        // must reflect the final-review verdict, not blindly
        // report "DONE". If either critic replied REPAIR or
        // REWRITE, the workflow has not actually shipped clean
        // code — the chairman's manual test (event 000080 log:
        // "input word 3D-magic-cube → 2-second run → 通过
        // 置信度 0.87") surfaced exactly this lie. We translate
        // the verdict-pair into a stable "FAILED: reviewer_<why>"
        // status string so the frontend's existing
        // `event.status.startsWith('FAILED')` branch (App.tsx
        // applyEvent) reports the real error and the
        // useAgentStream hook gets a meaningful terminal state.
        let verdict_a = critic_verdict(&final_a);
        let verdict_b = critic_verdict(&final_b);
        // Whichever critic flagged the workflow, attribute the
        // terminal failure to it. REWRITE > REPAIR so prefer
        // the more-severe verdict when the two disagree.
        // v0.4.22 (event 000114): extracted to pure
        // `terminal_done_status` for unit testing.
        let terminal_status =
            terminal_done_status(&verdict_a, &verdict_b, &final_a.role_id, &final_b.role_id);
        // Diagnostic line the chairman sees at the top of the
        // delivery summary — explicit so a "FAILED" outcome
        // doesn't leave the user guessing which reviewer
        // raised the concern.
        let final_review_line = format!(
            "[Final Review] Critic A ({}): {}. Critic B ({}): {}.\n",
            final_a.role_id, verdict_a, final_b.role_id, verdict_b,
        );

        // v0.4.22 (event 000114): prepend the [Final Review]
        // line so the chairman sees the verdict pair at the top
        // of the summary regardless of whether the LLM filled
        // the rest in. Prepend — does not replace — so the
        // existing fallback chain (chief text -> synthetic
        // placeholder) still kicks in for empty deliveries.
        let effective_summary = match delivery.summary.as_deref() {
            Some(s) if !s.is_empty() => format!("{final_review_line}{s}"),
            _ => match delivery.text.as_str() {
                t if !t.is_empty() => format!("{final_review_line}{t}"),
                _ => format!(
                    "{final_review_line}chief phase 8 returned empty summary; \
                     workflow ran 8 phases ({} tasks planned, \
                     2 critic reviews, {} worker runs) but \
                     the LLM didn't produce text. Likely a \
                     provider timeout — check the runtime log \
                     on the desktop for details.",
                    plan.tasks.len(),
                    plan.tasks.len(),
                ),
            },
        };

        // Final Done event so subscribers' useAgentStream sees
        // the terminal state even if they were only listening to
        // orchestrator-emitted events. v0.4.22 (event 000114):
        // status is no longer the hard-coded "DONE" — it
        // reflects the final-review verdict pair.
        let terminal_status_for_done = terminal_status.clone();
        let _ = self.events.send(AgentEvent::Done {
            wf_id: self.wf_id.clone(),
            status: terminal_status_for_done.clone(),
            summary: Some(effective_summary.clone()),
        });
        // v0.4.22 (event 000069): mark the workflow in the
        // workflows row so /api/workflow/{wf_id}/status can be
        // polled by clients that didn't watch the events pipe.
        // v0.4.22 (event 000114): the status column now follows
        // the verdict-derived terminal status (was hard-coded
        // "DONE" pre-event).
        let _ = self
            .state
            .repo
            .update_workflow_state(&self.wf_id, &terminal_status_for_done, "8-delivery")
            .await;
        // Per the chairman's manual test (event 000080 log):
        // three workflows all returned summary_len: 0 —
        // the LLM was timing out or returning empty bodies
        // (see NWT 000081 for the full bug writeup).
        // Also persist the final summary so status endpoint
        // returns it.
        if let Err(e) = self
            .state
            .repo
            .set_workflow_summary(&self.wf_id, &effective_summary)
            .await
        {
            warn!(
                target: "orchestrator",
                error = %e,
                wf_id = %self.wf_id,
                "v0.4.22 (event 000081): set_workflow_summary failed; \
                 status endpoint will show the placeholder"
            );
        }

        effective_summary
    }

    /// Persist a single agent run as a row in the `tasks` table.
    /// This is what makes the dashboard "任务列表" panel show
    /// each worker + each critic review as a separate row
    /// (event 000064 follow-up; the legacy run_task only wrote
    /// one row per chief call which made the panel look empty).
    ///
    /// v0.4.22 (event 000113): `repair_count` records which
    /// repair-loop round produced this row, so the dashboard
    /// can group re-runs under a common parent. Pass 0 for
    /// rows from the initial Phase 5 / 6 / 7 / 8 pass, and
    /// the 1-based loop index for any re-run triggered by a
    /// REPAIR / REWRITE verdict.
    async fn persist_task_row(&self, outcome: &TaskOutcome, phase_title: &str, repair_count: u32) {
        let task_id = format!("t_{}_{}", self.wf_id, rand_suffix());
        let now = chrono::Utc::now().timestamp();
        let title = if phase_title.len() > 60 {
            format!("{}…", phase_title.chars().take(60).collect::<String>())
        } else {
            phase_title.to_string()
        };
        // Make sure the workflows row exists before the tasks
        // INSERT — tasks.wf_id has a FK to workflows.id and
        // FK enforcement is on per-connection in storage.
        let _ = self
            .state
            .repo
            .ensure_workflow_row(&self.wf_id, &self.user_request, &outcome.status)
            .await;
        let _ = self
            .state
            .repo
            .create_task(&storage::Task {
                id: task_id,
                wf_id: self.wf_id.clone(),
                parent_id: None,
                title,
                status: outcome.status.to_lowercase(),
                assigned_to: Some(outcome.role_id.clone()),
                model: None, // resolved model id is opaque from this layer
                repair_count: repair_count as i64,
                input_tokens: 0,
                output_tokens: 0,
                cost_usd: None,
                files_modified: None,
                started_at: Some(now.saturating_sub(outcome.elapsed_ms as i64 / 1000)),
                finished_at: Some(now),
                result: Some(if outcome.text.is_empty() {
                    outcome.summary.clone().unwrap_or_default()
                } else {
                    outcome.text.chars().take(2000).collect::<String>()
                }),
            })
            .await;
    }
}

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
fn verdict_of(o: &TaskOutcome) -> String {
    scan_text_verdict(&o.text)
        .or_else(|| o.summary.as_ref().and_then(|s| scan_text_verdict(s)))
        .unwrap_or_else(|| "UNKNOWN".into())
}

fn scan_text_verdict(raw: &str) -> Option<String> {
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
fn extract_fenced_verdict_block(text: &str) -> Option<ReviewerVerdictJson> {
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
fn normalize_verdict_token(raw: &str) -> String {
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
fn first_sentence(text: &str, summary: &Option<String>) -> String {
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
fn rand_suffix() -> String {
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
fn map_phase_name(name: &str) -> String {
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
fn _json_silence(v: Value) -> Value {
    json!(v)
}

// Bridge for the orchestrator's resolve_role call. Defined in
// handlers.rs to keep storage access local.
mod storage {
    pub use storage::*;
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let task: WorkerTask =
            serde_json::from_str(json).expect("should deserialize with defaults");
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
}
