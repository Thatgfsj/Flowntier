//! Phase helpers: per-phase agent orchestration and candidate building.
//! Split out of orchestrator.rs - pure move, no logic change.

use super::*;

/// v0.4.22 (event 000091 fix #23): one resolved candidate for
/// a (provider_short, model_id) pair, with everything needed
/// to build an `OpenAiProvider`. The orchestrator iterates
/// over a Vec of these when the primary fails — the chairman
/// can configure `fallback_chain: ["anthropic:claude-opus-4-8",
/// "minimax:MiniMax-Text-01"]` so a mimo 401 falls through to
/// anthropic / MiniMax instead of killing the whole phase.
pub(super) struct ResolvedCandidate {
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

impl Orchestrator {
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
    pub(super) async fn emit_phase(&mut self, from: Option<&str>, to: &str) {
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
    pub(super) fn emit_reviewer_verdict(&self, phase: &str, outcome: &TaskOutcome) {
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
    pub(super) async fn run_phase_5_workers(
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
    pub(super) async fn run_phase_6_final_review(
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
    pub(super) async fn run_agent(&self, spec: AgentRunSpec) -> TaskOutcome {
        self.run_agent_with_task_id(spec, None).await
    }

    /// v0.4.22 (event 000118, fix 3): like `run_agent` but
    /// attaches a per-task id (`t{idx}`) to every event
    /// emitted from the agent's loop. The frontend uses the
    /// tag to render N Phase-5 worker cards instead of one.
    pub(super) async fn run_agent_with_task_id(
        &self,
        spec: AgentRunSpec,
        task_id: Option<String>,
    ) -> TaskOutcome {
        let role_id = spec.role.id().to_string();
        let role_display = spec.role.display().to_string();

        // v0.4.37: once a work-producing phase died on provider
        // infrastructure, later phases short-circuit here. Driving
        // them anyway is how v0.4.36 produced a fake "8/8 delivery"
        // with zero LLM work behind it.
        if let Some(why) = self.abort_reason.lock().clone() {
            return TaskOutcome {
                role_id,
                role_display,
                status: format!("FAILED: abort_provider_failure ({why})"),
                summary: Some(format!("工作流已中止: {why}")),
                text: format!("工作流已中止: {why}"),
                elapsed_ms: 0,
                structured_verdict: None,
            };
        }

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

        // v0.4.36 (checkpoints): snapshot the workspace before any
        // worker run so the chairman can always roll back agent
        // edits. Best-effort: a failed checkpoint is logged and
        // surfaced as an error-record but never blocks the task.
        if spec.role == agent_core::prompt::Role::Worker {
            let ws = self.state.workspace_snapshot();
            let label = format!("auto: before worker {task_id:?}");
            match crate::checkpoint::create(&ws.root, &label).await {
                Ok(m) => {
                    info!(target: "orchestrator", wf_id = %self.wf_id,
                          checkpoint = %m.id, kind = %m.kind,
                          "auto-checkpoint created before worker run");
                }
                Err(e) => {
                    warn!(target: "orchestrator", wf_id = %self.wf_id, error = %e,
                          "auto-checkpoint failed (continuing)");
                    self.state.push_error(crate::handlers::ErrorRecord {
                        at: chrono::Utc::now().timestamp(),
                        severity: "warn".into(),
                        source: "checkpoint".into(),
                        summary: "auto-checkpoint before worker failed".into(),
                        detail: Some(e.to_string()),
                    });
                }
            }
        }

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

            // v0.4.37: rate-limit-aware retry. A 429/TPM rejection is
            // provider health, not candidate quality — retry the SAME
            // candidate with backoff instead of burning the (often
            // single-element) chain in milliseconds.
            let mut rate_retries = 0usize;
            let outcome = loop {
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
                        if !cand.base_url.is_empty() && cand.base_url != "https://api.anthropic.com"
                        {
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
                let ws_for_cfg = self.state.workspace_snapshot();
                let agent = agent_core::Agent::new(
                    spec.role,
                    provider,
                    // v0.4.36: registry snapshot + permission capabilities
                    // + project rules (was hard-coded AgentConfig::default()).
                    self.state.tools_snapshot(),
                    ws_for_cfg.clone(),
                    self.state.agent_config(&ws_for_cfg),
                );
                let task = if let Some(ref ctx) = spec.context {
                    format!("{}\n\n{}", spec.task, ctx)
                } else {
                    spec.task.clone()
                };

                let attempt = self
                    .drive_single_agent(
                        agent,
                        task,
                        role_id.clone(),
                        role_display.clone(),
                        task_id.clone(),
                    )
                    .await;
                if status_is_rate_limited(&attempt.status) && rate_retries < RATE_LIMIT_MAX_RETRIES
                {
                    rate_retries += 1;
                    let wait = RATE_LIMIT_BACKOFF_SECS[rate_retries - 1];
                    warn!(
                        target: "orchestrator",
                        wf_id = %self.wf_id,
                        role = %role_id,
                        candidate_idx = idx,
                        retry = rate_retries,
                        backoff_secs = wait,
                        status = %attempt.status,
                        "v0.4.37: provider rate-limited; backing off before retrying the same candidate"
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                    if self.cancel_token.is_cancelled() {
                        break attempt;
                    }
                    continue;
                }
                break attempt;
            };
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
        let final_outcome = last_outcome.unwrap_or_else(|| TaskOutcome {
            role_id: role_id.clone(),
            role_display,
            status: "FAILED: all candidates exhausted".into(),
            summary: None,
            text: String::new(),
            elapsed_ms: 0,
            structured_verdict: None,
        });

        // v0.4.37: total provider-infrastructure failure on a
        // work-producing role aborts the workflow. Critics are
        // exempt — their unresponsive case already has a dedicated
        // verdict path (event 000114). Previously the failure simply
        // fell through and every later phase "completed" without any
        // LLM work, ending in a fake delivery.
        let is_critic = role_id == "agent:critic:a" || role_id == "agent:critic:b";
        let infra_failure = final_outcome
            .status
            .starts_with("FAILED: all candidates exhausted")
            || (!is_critic
                && (final_outcome.status.contains("provider error")
                    || status_is_rate_limited(&final_outcome.status)));
        if infra_failure && !is_critic {
            // Claim the abort slot first (sync — the MutexGuard must
            // NOT be held across the awaits below; a held guard makes
            // the run() future !Send and tokio::spawn rejects it).
            let claimed = {
                let mut g = self.abort_reason.lock();
                if g.is_none() {
                    let why = format!("角色 {role_id} — {}", final_outcome.status);
                    *g = Some(why.clone());
                    Some(why)
                } else {
                    None
                }
            };
            if let Some(why) = claimed {
                error!(
                    target: "orchestrator",
                    wf_id = %self.wf_id,
                    why = %why,
                    "v0.4.37: provider-infrastructural failure on a work-producing role — aborting workflow instead of fake-completing"
                );
                self.state.push_error(crate::handlers::ErrorRecord {
                    at: chrono::Utc::now().timestamp(),
                    severity: "error".into(),
                    source: "orchestrator".into(),
                    summary: "工作流中止: 模型服务不可用或被限流".into(),
                    detail: Some(final_outcome.status.clone()),
                });
                let msg = format!(
                    "工作流中止：模型服务不可用或被限流（{why}）。剩余阶段已停止，避免零产出假完成。可稍后重试或更换模型/供应商。"
                );
                let _ = self.events.send(AgentEvent::Done {
                    wf_id: self.wf_id.clone(),
                    status: "FAILED: provider_failure".into(),
                    summary: Some(msg.clone()),
                });
                let _ = self
                    .state
                    .repo
                    .set_workflow_summary(&self.wf_id, &msg)
                    .await;
            }
        }
        final_outcome
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
    pub(super) async fn build_candidates(
        &self,
        role_id: &str,
    ) -> Result<Vec<ResolvedCandidate>, String> {
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
    pub(super) async fn resolve_fallback_string(
        &self,
        fb: &str,
    ) -> Result<ResolvedCandidate, String> {
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
