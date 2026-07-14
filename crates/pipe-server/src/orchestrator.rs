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
use agent_core::workspace::Workspace;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::handlers::ServerState;

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerTask {
    pub id: String,
    pub title: String,
    pub objective: String,
    pub interfaces: String,
    pub dependencies: Vec<String>,
    pub requirements: String,
    /// Optional worker label (Backend / Frontend / Database /
    /// API / Testing / Documentation). When the chief picks one
    /// of these, the orchestrator still spawns a generic
    /// `agent:worker` agent but tags the task row + PhaseTimeline
    /// entry so the UI can group them.
    #[serde(default)]
    pub label: String,
}

/// The structured Planning Doc chief produces in Phase 2 and
/// critic reviews in Phase 3 + Phase 6.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanDoc {
    pub summary: String,
    pub architecture: String,
    pub tasks: Vec<WorkerTask>,
}

impl PlanDoc {
    /// Merge another PlanDoc into this one. The summary /
    /// architecture fields are kept from `self` (the first
    /// round's authoritative text); tasks are appended, with
    /// id-based dedup so a chief that re-uses an id in two
    /// rounds doesn't create duplicate dispatch rows.
    /// Empty summaries / architectures from the secondary
    /// doc don't overwrite the primary doc's values.
    pub fn merge(&mut self, other: &PlanDoc) {
        if self.summary.is_empty() && !other.summary.is_empty() {
            self.summary = other.summary.clone();
        }
        if self.architecture.is_empty() && !other.architecture.is_empty() {
            self.architecture = other.architecture.clone();
        }
        let existing_ids: std::collections::HashSet<String> =
            self.tasks.iter().map(|t| t.id.clone()).collect();
        for t in &other.tasks {
            if !existing_ids.contains(&t.id) {
                self.tasks.push(t.clone());
            }
        }
    }

    /// Best-effort JSON extractor. The chief is asked to wrap
    /// the PlanDoc in a fenced ```json block; we strip the
    /// fences and parse the first `{...}` block. Falls back to
    /// a single-task doc derived from the user's raw request
    /// if no JSON is found, so the orchestrator never silently
    /// hangs on a "no plan" response.
    pub fn from_chief_text(text: &str, fallback_request: &str) -> Self {
        if let Some(start) = text.find("```json") {
            if let Some(end_rel) = text[start + 7..].find("```") {
                let body = &text[start + 7..start + 7 + end_rel];
                if let Ok(parsed) = serde_json::from_str::<PlanDoc>(body) {
                    return parsed;
                }
            }
        }
        if let Some(start) = text.find('{') {
            if let Some(end_rel) = text[start..].rfind('}') {
                let body = &text[start..start + end_rel + 1];
                if let Ok(parsed) = serde_json::from_str::<PlanDoc>(body) {
                    return parsed;
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
pub struct TaskOutcome {
    pub role_id: String,
    pub role_display: String,
    pub status: String,
    pub summary: Option<String>,
    pub text: String,
    pub elapsed_ms: u64,
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
}

impl Orchestrator {
    /// Build a new orchestrator. Generates a stable wf_id so
    /// PhaseTimeline + tasks rows are linkable from the moment
    /// the workflow starts.
    pub fn new(
        state: Arc<ServerState>,
        events: broadcast::Sender<AgentEvent>,
        user_request: String,
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
        if let Some(prev) = from {
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
        if let Err(e) = self.state.repo.update_workflow_state(
            &self.wf_id,
            "ACTIVE",
            &wf_phase,
        ).await {
            warn!(target: "orchestrator", error = %e, wf_id = %self.wf_id, "update_workflow_state failed");
        }
    }

    /// Spawn one agent run and return its outcome. The agent
    /// gets its own Workspace snapshot (so chief writing into
    /// workspace A doesn't leak into worker B's view — though
    /// they all share the same on-disk path; that's by design,
    /// so workers can read each other's artefacts via file ops
    /// but the spec says they shouldn't communicate via in-mem
    /// state).
    async fn run_agent(&self, spec: AgentRunSpec) -> TaskOutcome {
        let role_id = spec.role.id().to_string();
        let role_display = spec.role.display().to_string();

        // v0.4.22 (event 000082): per-agent start log so the
        // chairman can see which agent role is currently
        // running. Combined with the phase-completed log
        // (in emit_phase), the runtime log is now sufficient
        // to debug any stall without grepping the events
        // pipe. Model + api_kind surface so the chairman
        // sees which provider is being hit (mimo:...
        // vs minimax:...).
        info!(
            target: "orchestrator",
            wf_id = %self.wf_id,
            role = %role_id,
            role_display = %role_display,
            "[TRACE] agent run starting (event 000082)"
        );

        // Resolve provider + model from role_overrides. If the
        // role isn't configured, return FAILED immediately —
        // don't sit on a 30-minute timeout for nothing.
        let resolved = match crate::handlers::resolve_role_for_orchestrator(
            &self.state, &role_id,
        ).await {
            Ok(r) => r,
            Err(e) => {
                warn!(target: "orchestrator", role = %role_id, error = %e, "resolve_role failed");
                return TaskOutcome {
                    role_id, role_display,
                    status: format!("FAILED: resolve: {e}"),
                    summary: None,
                    text: String::new(),
                    elapsed_ms: 0,
                };
            }
        };

        let provider: Arc<dyn agent_core::Provider> = match resolved.api_kind.as_str() {
            "openai" => Arc::new(agent_core::provider::openai::OpenAiProvider::openai(
                resolved.model_id.clone(),
                resolved.api_key.to_string(),
            )),
            _ => Arc::new(agent_core::provider::openai::OpenAiProvider::compat(
                resolved.base_url.clone(),
                resolved.model_id.clone(),
                resolved.api_key.to_string(),
            )),
        };
        let agent = agent_core::Agent::new(
            spec.role.clone(),
            provider,
            self.state.tools.clone(),
            self.state.workspace_snapshot(),
            agent_core::AgentConfig::default(),
        );
        let task = if let Some(ctx) = spec.context {
            format!("{}\n\n{}", spec.task, ctx)
        } else {
            spec.task
        };

        let start = Instant::now();
        let mut rx = agent.run(task);
        let mut text = String::new();
        let mut last_status = "UNKNOWN".to_string();
        let mut summary: Option<String> = None;
        // 5-minute per-agent ceiling so a runaway critic doesn't
        // block the orchestrator forever. Per-task budget is
        // enforced at the run_task handler; per-agent is new in
        // event 000068 because critics + workers run unattended.
        let timed_out = tokio::time::timeout(
            Duration::from_secs(300),
            async {
                while let Some(ev) = rx.recv().await {
                    let _ = self.events.send(ev.clone());
                    match ev {
                        AgentEvent::TextDelta { delta, .. } => text.push_str(&delta),
                        AgentEvent::Done { status, summary: s, .. } => {
                            last_status = status;
                            summary = s;
                            // v0.4.22 (event 000082): agent
                            // finished (success OR error).
                            // Chairman sees which role finished
                            // + what status + how long it
                            // took + whether text was emitted.
                            info!(
                                target: "orchestrator",
                                wf_id = %self.wf_id,
                                role = %role_id,
                                status = %last_status,
                                text_len = text.len(),
                                "v0.4.22 (event 000082): agent run finished"
                            );
                            if matches!(last_status.as_str(), "DONE" | "FAILED" | "ABORTED" | "ABORTED_REPEAT" | "TIMEOUT (300s)") {
                                return false;
                            }
                        }
                        _ => {}
                    }
                }
                true
            },
        ).await.unwrap_or(true);

        let status = if timed_out && !matches!(last_status.as_str(), "DONE" | "FAILED" | "ABORTED" | "ABORTED_REPEAT") {
            let _ = self.events.send(AgentEvent::Done {
                wf_id: self.wf_id.clone(),
                status: format!("TIMEOUT (300s)"),
                summary: Some(format!("agent {role_display} exceeded 300s")),
            });
            "TIMEOUT (300s)".into()
        } else {
            last_status
        };

        TaskOutcome {
            role_id, role_display,
            status, summary,
            text, elapsed_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Run the full 8-phase workflow. Returns the final summary
    /// string the chairman sees.
    pub async fn run(mut self) -> String {
        info!(target: "orchestrator", wf_id = %self.wf_id, user_request_len = self.user_request.len(), "[TRACE] Orchestrator::run() ENTERED — starting 8-phase workflow");
        let mut phase_idx = 0;
        self.emit_phase(None, PHASES[phase_idx]).await;

        // ── Phase 1: requirement analysis ─────────────────
        let chief_clarify = self.run_agent(AgentRunSpec {
            role: Role::Chief,
            task: format!(
                "用户需求:{}\n\n只做一件事: 1-3 句判断需求是否清楚。如果清楚, 直接说 OK 准备进入下一阶段; 如果不清楚, 追问 1-3 个关键问题。不要做规划, 不要做拆任务。",
                self.user_request,
            ),
            context: None,
        }).await;
        self.persist_task_row(&chief_clarify, "1-requirement").await;
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
        self.persist_task_row(&plan_round_a, "2-plan-A-summary").await;
        let mut plan = PlanDoc::from_chief_text(&plan_round_a.text, &self.user_request);

        let plan_round_b = self.run_agent(AgentRunSpec {
            role: Role::Chief,
            task: format!(
                "用户需求:{}\n\n已知架构:\n```\n{}\n```\n\n任务: 输出后端 / API / 数据库 / 数据 / 算法 这一类任务, 严格按 JSON 格式包在 ```json 围栏里:\n```json\n{{\n  \"tasks\": [\n    {{\n      \"id\": \"w_<unique>\",\n      \"title\": \"<中文短标题>\",\n      \"label\": \"Backend|API|Database|Worker\",\n      \"objective\": \"<本 worker 的目标, 一句话>\",\n      \"interfaces\": \"<输入输出接口说明, 可空>\",\n      \"dependencies\": [],\n      \"requirements\": \"<编码要求, 可空>\"\n    }}\n  ]\n}}\n```\n**只输出 tasks 数组, 不要重复 summary / architecture**。",
                self.user_request, plan.architecture,
            ),
            context: None,
        }).await;
        self.persist_task_row(&plan_round_b, "2-plan-B-backend").await;
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
        self.persist_task_row(&plan_round_c, "2-plan-C-frontend").await;
        let plan_c = PlanDoc::from_chief_text(&plan_round_c.text, &self.user_request);
        plan.merge(&plan_c);

        // ── Phase 3: plan review (parallel) ──────────────
        phase_idx = 2;
        self.emit_phase(Some(PHASES[1]), PHASES[phase_idx]).await;
        let plan_ctx = format!("需要评审的 PlanDoc:\n```json\n{}\n```", serde_json::to_string_pretty(&plan).unwrap_or_default());
        let (critic_a, critic_b) = tokio::join!(
            self.run_agent(AgentRunSpec {
                role: Role::BugHunter,
                task: "评审上述 PlanDoc 的 bug / runtime 风险 / 边界条件。给出: PASS / REPAIR / REWRITE 之一, 加一句话理由。".into(),
                context: Some(plan_ctx.clone()),
            }),
            self.run_agent(AgentRunSpec {
                role: Role::Reviewer,
                task: "评审上述 PlanDoc 的架构 / 可维护性 / 可读性。给出: PASS / REPAIR / REWRITE 之一, 加一句话理由。".into(),
                context: Some(plan_ctx),
            }),
        );
        self.persist_task_row(&critic_a, "3-plan-review-criticA").await;
        self.persist_task_row(&critic_b, "3-plan-review-criticB").await;

        // ── Phase 4: dispatch (chief declares worker list) ──
        phase_idx = 3;
        self.emit_phase(Some(PHASES[2]), PHASES[phase_idx]).await;
        let plan_review = format!(
            "Critic A: {} → {}\nCritic B: {} → {}",
            critic_a.role_display,
            verdict_of(&critic_a),
            critic_b.role_display,
            verdict_of(&critic_b),
        );
        let dispatch = self.run_agent(AgentRunSpec {
            role: Role::Chief,
            task: format!(
                "PlanDoc 已经定了。Critic 评审: {}\n\n确认 worker 列表(原 PlanDoc 的 tasks 字段), 或修改后说'OK 派发'。如果 critic 给出 REPAIR/REWRITE, 改 plan 并说明原因。",
                plan_review,
            ),
            context: Some(format!("PlanDoc: {}", serde_json::to_string_pretty(&plan).unwrap_or_default())),
        }).await;
        self.persist_task_row(&dispatch, "4-dispatch").await;

        // ── Phase 5: develop (workers in parallel) ─────────
        phase_idx = 4;
        self.emit_phase(Some(PHASES[3]), PHASES[phase_idx]).await;
        let mut worker_futures = Vec::new();
        for t in &plan.tasks {
            let task_text = format!(
                "任务: {}\n目标: {}\n接口: {}\n依赖: {:?}\n要求: {}\n\n只做这一件事, 不要碰其他 worker 的文件。完成后用一句话汇报。",
                t.title, t.objective, t.interfaces, t.dependencies, t.requirements,
            );
            worker_futures.push(self.run_agent(AgentRunSpec {
                role: Role::Worker,
                task: task_text,
                context: None,
            }));
        }
        let worker_results = futures::future::join_all(worker_futures).await;
        for (i, w) in worker_results.iter().enumerate() {
            let title = plan.tasks.get(i).map(|t| t.title.clone()).unwrap_or_default();
            self.persist_task_row(w, &format!("5-develop-{}", title)).await;
        }

        // ── Phase 6: final review (parallel) ───────────────
        phase_idx = 5;
        self.emit_phase(Some(PHASES[4]), PHASES[phase_idx]).await;
        let workers_summary: Vec<String> = worker_results.iter().enumerate().map(|(i, w)| {
            let title = plan.tasks.get(i).map(|t| t.title.clone()).unwrap_or_default();
            format!("[{}] {} ({}): {}\n  -> {}",
                title, w.role_display, w.status,
                w.text.chars().take(120).collect::<String>(),
                w.summary.clone().unwrap_or_default(),
            )
        }).collect();
        let review_ctx = workers_summary.join("\n\n");
        let (final_a, final_b) = tokio::join!(
            self.run_agent(AgentRunSpec {
                role: Role::BugHunter,
                task: "评审上面所有 worker 的产出 (找 bug / runtime 风险)。给出: PASS / REPAIR / REWRITE。".into(),
                context: Some(review_ctx.clone()),
            }),
            self.run_agent(AgentRunSpec {
                role: Role::Reviewer,
                task: "评审上面所有 worker 的产出 (架构 / 维护性)。给出: PASS / REPAIR / REWRITE。".into(),
                context: Some(review_ctx),
            }),
        );
        self.persist_task_row(&final_a, "6-final-review-criticA").await;
        self.persist_task_row(&final_b, "6-final-review-criticB").await;

        // ── Phase 7: repair (one pass — chief decides) ────
        phase_idx = 6;
        self.emit_phase(Some(PHASES[5]), PHASES[phase_idx]).await;
        let final_review = format!(
            "Final Critic A: {} → {}\nFinal Critic B: {} → {}",
            final_a.role_display, verdict_of(&final_a),
            final_b.role_display, verdict_of(&final_b),
        );
        let _repair = self.run_agent(AgentRunSpec {
            role: Role::Chief,
            task: format!(
                "Final review:\n{}\n\n决定: 如果两位 critic 都是 PASS, 说 PASS; 否则说明 REPAIR 的 worker 是哪些, 一句话即可。本 phase 只做决策, 不做实际修复。",
                final_review,
            ),
            context: None,
        }).await;
        self.persist_task_row(&_repair, "7-repair").await;

        // ── Phase 8: delivery ─────────────────────────────
        phase_idx = 7;
        self.emit_phase(Some(PHASES[6]), PHASES[phase_idx]).await;
        let delivery = self.run_agent(AgentRunSpec {
            role: Role::Chief,
            task: format!(
                "用一段人话总结这次 workflow 的产出:\n- 用户需求:{}\n- 计划: {}\n- Worker 数量: {}\n- Critic 评审: A={}, B={}\n- 最终状态: PASS or REPAIR\n\n不要列代码, 不要列任务细节, 一段中文给用户看。",
                self.user_request,
                plan.summary,
                plan.tasks.len(),
                verdict_of(&final_a),
                verdict_of(&final_b),
            ),
            context: None,
        }).await;
        self.persist_task_row(&delivery, "8-delivery").await;

        // v0.4.22 (event 000081): if the chief's LLM returned
        // an empty summary AND an empty text, fall back to a
        // synthetic one-line summary so the chairman sees
        // something useful in the UI / status endpoint / log.
        // Per the chairman's manual test (event 000080 log):
        // three workflows all returned summary_len: 0 —
        // the LLM was timing out or returning empty bodies
        // (see NWT 000081 for the full bug writeup).
        let effective_summary = match delivery.summary.as_deref() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => match delivery.text.as_str() {
                t if !t.is_empty() => t.to_string(),
                _ => format!(
                    "chief phase 8 returned empty summary; \
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
        // orchestrator-emitted events.
        let _ = self.events.send(AgentEvent::Done {
            wf_id: self.wf_id.clone(),
            status: "DONE".into(),
            summary: Some(effective_summary.clone()),
        });
        // v0.4.22 (event 000069): mark the workflow as DONE in
        // the workflows row so /api/workflow/{wf_id}/status
        // can be polled by clients that didn't watch the
        // events pipe.
        let _ = self.state.repo.update_workflow_state(
            &self.wf_id, "DONE", "8-delivery",
        );
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
    async fn persist_task_row(&self, outcome: &TaskOutcome, phase_title: &str) {
        let task_id = format!("t_{}_{}", self.wf_id, rand_suffix());
        let now = chrono::Utc::now().timestamp();
        let title = if phase_title.len() > 60 {
            format!("{}…", phase_title.chars().take(60).collect::<String>())
        } else {
            phase_title.to_string()
        };
        let result = self.events.send(AgentEvent::Done {
            wf_id: self.wf_id.clone(),
            status: outcome.status.clone(),
            summary: outcome.summary.clone(),
        });
        let _ = result;
        // Make sure the workflows row exists before the tasks
        // INSERT — tasks.wf_id has a FK to workflows.id and
        // FK enforcement is on per-connection in storage.
        let _ = self.state.repo.ensure_workflow_row(
            &self.wf_id, &format!("[{}] {}", phase_title, outcome.role_id), &outcome.status,
        ).await;
        let _ = self.state.repo.create_task(&storage::Task {
            id: task_id,
            wf_id: self.wf_id.clone(),
            parent_id: None,
            title,
            status: outcome.status.to_lowercase(),
            assigned_to: Some(outcome.role_id.clone()),
            model: None, // resolved model id is opaque from this layer
            repair_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: None,
            files_modified: None,
            started_at: Some(now),
            finished_at: Some(now + outcome.elapsed_ms as i64 / 1000),
            result: Some(if outcome.text.is_empty() {
                outcome.summary.clone().unwrap_or_default()
            } else {
                outcome.text.chars().take(2000).collect::<String>()
            }),
        }).await;
    }
}

/// Pull a PASS / REPAIR / REWRITE token out of an agent's text.
/// Looks at the last 200 chars for the most recent verdict; if
/// none found, returns "UNKNOWN".
fn verdict_of(o: &TaskOutcome) -> String {
    let s = o.text.to_uppercase();
    for v in &["PASS", "REWRITE", "REPAIR"] {
        if s.contains(v) {
            return v.to_string();
        }
    }
    if let Some(s) = &o.summary {
        let su = s.to_uppercase();
        for v in &["PASS", "REWRITE", "REPAIR"] {
            if su.contains(v) {
                return v.to_string();
            }
        }
    }
    "UNKNOWN".into()
}

/// Small random suffix to make task ids unique within a single
/// process. Not cryptographic — just collision-resistant
/// enough for the dashboard's per-row rendering.
fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
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
fn _json_silence(v: Value) -> Value { json!(v) }

// Bridge for the orchestrator's resolve_role call. Defined in
// handlers.rs to keep storage access local.
mod storage {
    pub use storage::*;
}