//! Workflow entry point: agent drive loop, main run() and task persistence.
//! Split out of orchestrator.rs - pure move, no logic change.

use super::*;

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
    pub(super) async fn drive_single_agent(
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
    pub(super) async fn persist_task_row(
        &self,
        outcome: &TaskOutcome,
        phase_title: &str,
        repair_count: u32,
    ) {
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
