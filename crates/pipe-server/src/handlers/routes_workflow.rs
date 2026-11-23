//! Workflow run/status/cancel routes.

use super::*;

pub(super) fn register_run(d: &mut Dispatcher, state: Arc<ServerState>) {
    // ── v0.4.22 (event 000068): POST /api/run_workflow ────
    // Body: { task: "<user_request>" }
    //
    // v0.4.22 (event 000069 follow-up): run the workflow on a
    // background tokio task and return wf_id immediately. The
    // monolithic call blocked the JSON-RPC handler for up to
    // 30+ minutes on large requests (78-card tarot app =
    // 1-requirement + 2-plan (3 rounds) + 3-plan-review
    // (2 parallel) + 4-dispatch + 5-develop (N workers) +
    // 6-final-review + 7-repair + 8-delivery — at 5 min/agent
    // worst case that's way past any reasonable HTTP timeout).
    //
    // Now: POST returns wf_id + status "running" in ~50ms.
    // Clients poll GET /api/workflow/{wf_id}/status for the
    // current phase + summary, or listen on the events pipe
    // for PhaseTransition updates (which the UI does anyway).
    let s_wf = state.clone();
    d.register("POST", "/api/run_workflow", move |body| {
        let s = s_wf.clone();
        Box::pin(async move {
            tracing::info!(target: "pipe_server", "[TRACE] /api/run_workflow handler ENTERED");
            let Some(task) = body.get("task").and_then(|v| v.as_str()) else {
                tracing::warn!(target: "pipe_server", "[TRACE] /api/run_workflow: missing 'task' field");
                return Ok((400, json!({
                    "ok": false,
                    "error": "missing 'task'",
                })));
            };
            if task.trim().is_empty() {
                tracing::warn!(target: "pipe_server", "[TRACE] /api/run_workflow: task is empty");
                return Ok((400, json!({
                    "ok": false,
                    "error": "'task' is empty",
                })));
            }
            // Pre-flight check: ensure chief role has a configured model and API key
            if let Err(e) = resolve_role(&s, "agent:chief").await {
                tracing::warn!(target: "pipe_server", error = %e, "[TRACE] /api/run_workflow: chief role not configured");
                return Ok((400, json!({
                    "ok": false,
                    "error": format!("未配置 AI 模型或 API Key：请先在「设置 → 供应商与密钥」配置 API Key，并在「角色模型」中分配模型。\n({e})"),
                    "hint": "open Settings → 角色 → 模型 分配 and pick a default_model",
                })));
            }
            tracing::info!(target: "pipe_server", task_len = task.len(), task_preview = %task.chars().take(80).collect::<String>(), "[TRACE] /api/run_workflow: creating Orchestrator");
            // v0.4.22 (event 000118, fix 7): build the cancel
            // token FIRST, then pass it to the Orchestrator so
            // drive_single_agent can race it against the
            // in-flight agent.run() inside tokio::select!. The
            // same token handle is also stashed in the
            // active_workflows map so /api/workflow/cancel
            // can fire it from the Stop button.
            let cancel_token = tokio_util::sync::CancellationToken::new();
            let orch = crate::orchestrator::Orchestrator::new(
                s.clone(),
                s.events.clone(),
                task.to_string(),
                // v0.4.22 (event 000113): bounded repair loop.
                // Mirrors tauri-core's default (see
                // crates/tauri-core/src/lib.rs:50); a future
                // event will thread this through /api/run_workflow
                // request body so the chairman can tune it from
                // the desktop without rebuilding the runtime.
                3,
                cancel_token.clone(),
            );
            let wf_id = orch.wf_id.clone();
            tracing::info!(target: "pipe_server", wf_id = %wf_id, "[TRACE] /api/run_workflow: spawning orch.run() on background tokio task");
            // Spawn the orchestrator on a background task so
            // the JSON-RPC response returns immediately. Each
            // phase still emits AgentEvent::PhaseTransition on
            // the events pipe so the UI animates as the
            // workflow progresses.
            // v0.4.22 (event 000091 fix #32): the cancel token
            // is created here and stashed in the active_workflows
            // map so the `POST /api/workflow/cancel` route
            // can fire it. The token is removed when the
            // workflow finishes (cleanup below). event 000118
            // fix 7 plumbed the token into run_agent's
            // tokio::select! so cancellation actually unwinds
            // within ~1s instead of waiting for the 5-min
            // per-task timeout.
            {
                s.active_workflows.insert(wf_id.clone(), cancel_token);
            }
            let wf_id_for_log = wf_id.clone();
            let s_for_cleanup = s.clone();
            let wf_id_for_cleanup = wf_id.clone();
            tokio::spawn(async move {
                tracing::info!(target: "pipe_server", wf_id = %wf_id_for_log, "[TRACE] orch.run() STARTING on background task");
                let summary = orch.run().await;
                // Remove the cancel token now that the
                // workflow is done — the route will return
                // 404 from this point on, which matches the
                // "no longer active" semantic.
                s_for_cleanup.active_workflows.remove(&wf_id_for_cleanup);
                tracing::info!(
                    target: "pipe_server",
                    wf_id = %wf_id_for_log,
                    summary_len = summary.len(),
                    "[TRACE] orch.run() FINISHED — workflow complete (cancel token removed)"
                );
            });
            tracing::info!(target: "pipe_server", wf_id = %wf_id, "[TRACE] /api/run_workflow: returning wf_id to caller (cancel token registered)");
            Ok((200, json!({
                "ok": true,
                "wf_id": wf_id,
                "status": "running",
                "note": "poll GET /api/workflow/{wf_id}/status for current phase + summary, or listen on wf:event channel. POST /api/workflow/{wf_id}/cancel to interrupt.",
            })))
        })
    });
}

pub(super) fn register_status(d: &mut Dispatcher, state: Arc<ServerState>) {
    // ── v0.4.22 (event 000069): GET /api/workflow/{wf_id}/status ─
    // Returns the current status of a workflow. Reads the
    // workflows row (state / phase) plus aggregates the tasks
    // table to count done vs in-flight tasks under the wf_id.
    // The orchestrator updates workflows.state to DONE when
    // delivery finishes.
    let s_wfstatus = state.clone();
    d.register("GET", "/api/workflow/{wf_id}/status", move |body| {
        let s = s_wfstatus.clone();
        let wf_id = body
            .get("wf_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Box::pin(async move {
            if wf_id.is_empty() {
                return Ok((
                    400,
                    json!({
                        "ok": false,
                        "error": "missing 'wf_id' in path placeholder",
                    }),
                ));
            }
            let wf_row = s.repo.get_workflow(&wf_id).await.unwrap_or(None);
            // If workflows row missing, orchestrator hasn't
            // started yet (or never persisted — fall through).
            let (state, phase, summary, user_request) = match &wf_row {
                Some(w) => {
                    // WorkflowState + WorkflowPhase enums aren't
                    // `pub` re-exported across the workspace,
                    // so we just Debug-print and trim. The
                    // orchestrator writes prefixed strings
                    // ("1-requirement" / "ACTIVE" / "DONE") so
                    // a simple string suffix match works.
                    let state_str = format!("{:?}", w.state).to_lowercase();
                    let phase_str = format!("{:?}", w.phase).to_lowercase();
                    (
                        state_str,
                        phase_str,
                        w.summary.clone(),
                        w.user_request.clone(),
                    )
                }
                None => (
                    "unknown".to_string(),
                    "unknown".to_string(),
                    None,
                    String::new(),
                ),
            };
            // Count tasks for this wf_id to show progress.
            let (done, total) = s.repo.count_tasks(&wf_id).await.unwrap_or((0, 0));
            Ok((
                200,
                json!({
                    "ok": true,
                    "wf_id": wf_id,
                    "status": state,
                    "phase": phase,
                    "summary": summary,
                    "user_request": user_request,
                    "tasks_done": done,
                    "tasks_total": total,
                }),
            ))
        })
    });
}

pub(super) fn register_cancel_v1(d: &mut Dispatcher, state: Arc<ServerState>) {
    // ── v0.4.22 (event 000118, fix 7): POST /api/workflow/cancel ─
    // Stops a running workflow by firing the orchestrator's
    // CancellationToken. Body shape: { wf_id: string }.
    // The Tauri shell calls this from the Stop button.
    // Previously the matching route was /api/workflow/{id}/cancel
    // which the dispatcher couldn't match (segment count off),
    // so cancel_workflow was a 404 no-op.
    let s_wfcancel = state.clone();
    d.register("POST", "/api/workflow/cancel", move |body| {
        let s = s_wfcancel.clone();
        Box::pin(async move {
            let wf_id = body
                .get("wf_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if wf_id.is_empty() {
                return Ok((
                    400,
                    json!({
                        "ok": false,
                        "error": "missing 'wf_id' in body",
                    }),
                ));
            }
            let token_opt = s.active_workflows.get(&wf_id);
            match token_opt {
                Some(token) => {
                    tracing::info!(
                        target: "pipe_server",
                        wf_id = %wf_id,
                        "[TRACE] /api/workflow/cancel: firing cancel_token"
                    );
                    token.cancel();
                    Ok((
                        200,
                        json!({
                            "ok": true,
                            "wf_id": wf_id,
                            "status": "cancelling",
                            "note": "cancel_token fired; orchestrator should exit within 1s",
                        }),
                    ))
                }
                None => {
                    // Workflow finished already (or wf_id is wrong).
                    // Don't 404 — the chair just sees "ok, no-op".
                    tracing::warn!(
                        target: "pipe_server",
                        wf_id = %wf_id,
                        "[TRACE] /api/workflow/cancel: no active workflow (already done?)"
                    );
                    Ok((
                        200,
                        json!({
                            "ok": true,
                            "wf_id": wf_id,
                            "status": "not_active",
                            "note": "no active workflow with this id (already finished?)",
                        }),
                    ))
                }
            }
        })
    });
}

pub(super) fn register_cancel_v2(d: &mut Dispatcher, state: Arc<ServerState>) {
    // ── v0.4.22 (event 000091 fix #32): real cancel. Looks
    // up the active workflow in `state.active_workflows`, fires
    // its `CancellationToken` (so the in-flight agent loop
    // returns `ABORTED` and the orchestrator unwinds), and
    // returns 200 with the wf_id. If no such workflow is
    // active, returns 404 — the previous stub always returned
    // 200 with "no active workflow to cancel", which was
    // indistinguishable from a real cancel and let the UI
    // believe it had stopped a runaway workflow.
    let s_cancel = Arc::clone(&state);
    d.register("POST", "/api/workflow/{id}/cancel", move |body| {
        let s = s_cancel.clone();
        Box::pin(async move {
            let wf_id = body.get("wf_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default();
            if wf_id.is_empty() {
                return Ok((400, json!({
                    "ok": false,
                    "error": "missing 'wf_id' in path placeholder",
                })));
            }
            let token = s.active_workflows.get(&wf_id);
            match token {
                Some(tok) => {
                    tok.cancel();
                    Ok((200, json!({
                        "ok": true,
                        "wf_id": wf_id,
                        "note": "cancellation fired; orchestrator unwinds at next agent boundary",
                    })))
                }
                None => Ok((404, json!({
                    "ok": false,
                    "wf_id": wf_id,
                    "error": "no active workflow with this id",
                }))),
            }
        })
    });
}
