//! POST /api/run_task handler body.

use super::*;

pub(super) async fn run_task(body: Value, state: Arc<ServerState>) -> Result<(u16, Value), String> {
    // ── v0.4.19: minimal body shape ───────────────────────────
    // Required: { task, role }
    // Optional (legacy): { provider_kind, base_url, model, api_key, wf_id }
    //
    // When the optional fields are absent, the server resolves
    // them from role_overrides (v0.4.18) + the matching preset's
    // base_url / secret_name, revealing the API key from the OS
    // keystore. The frontend (ChatZone v0.4.19) now sends only
    // { task, role }.
    let task_text = body
        .get("task")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'task'".to_string())?
        .to_string();
    let role = body
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("agent:worker")
        .to_string();

    // Legacy path: caller supplied concrete provider/model/key.
    let legacy_explicit = body.get("base_url").is_some()
        || body.get("model").is_some()
        || body.get("api_key").is_some();
    let explicit_provider_kind = body
        .get("provider_kind")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "openai_compat".to_string());
    let explicit_base_url = body
        .get("base_url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let explicit_model = body
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let explicit_api_key = body
        .get("api_key")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| zeroize::Zeroizing::new(s.to_string()));

    let wf_id = body
        .get("wf_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // v0.4.22 (event 000118, fix 6): the ChatZone "对话" mode
    // sends prior user/assistant turns here so the agent has
    // memory of the conversation and doesn't fall into a
    // clarification loop. Shape:
    //   { role: "user"|"assistant"|"system", content: string,
    //     tool_calls?: [...], tool_call_id?: string }
    // We tolerate malformed entries by silently dropping them
    // (the user can always re-send a corrected turn) so a single
    // bad record doesn't kill the whole turn. The actual
    // filter_map is in `parse_chat_history` in agent-core so
    // we can unit-test it without spinning up the pipe server.
    let chat_history: Vec<Message> = parse_chat_history(&body);

    // ── Build the primary provider ─────────────────────────────
    let (provider_kind, base_url, model, api_key) = if legacy_explicit {
        // Caller supplied everything — honor it (debug / scripted use).
        let base_url = explicit_base_url
            .clone()
            .ok_or_else(|| "missing 'base_url'".to_string())?;
        let base_url = agent_core::provider::openai::validate_base_url(&base_url)
            .map_err(|e| format!("invalid base_url: {e}"))?;
        let api_key = explicit_api_key
            .clone()
            .ok_or_else(|| "missing or empty 'api_key'".to_string())?;
        let model = explicit_model
            .clone()
            .ok_or_else(|| "missing 'model'".to_string())?;
        (explicit_provider_kind.clone(), base_url, model, api_key)
    } else {
        // v0.4.19: resolve from role_overrides + preset keychain.
        // If the primary fails, we'll iterate the fallback_chain
        // below; for now we attempt the primary once.
        let resolved = match resolve_role(&state, &role).await {
            Ok(r) => r,
            Err(e) => {
                // Friendlier 200/503 envelope than the legacy raw
                // 500/Err so the frontend can render it nicely.
                return Ok((
                    503,
                    json!({
                        "ok": false,
                        "role": role,
                        "error": e,
                        "hint": "open Settings → 角色 → 模型 分配",
                    }),
                ));
            }
        };
        let provider_kind = match resolved.api_kind.as_str() {
            "openai-compatible" => "openai_compat".to_string(),
            "anthropic-compatible" | "anthropic" => "anthropic".to_string(),
            other => other.to_string(),
        };
        (
            provider_kind,
            resolved.base_url,
            resolved.model_id,
            resolved.api_key,
        )
    };

    // ── Build provider ────────────────────────────────────────
    // v0.4.20: keep a clone of `model` so the quota-tracking
    // block below (line ~1490) can reference it after OpenAiProvider
    // has moved it.
    let model_for_quota = model.clone();
    let provider: Arc<dyn agent_core::Provider> = match provider_kind.as_str() {
        "openai" => Arc::new(OpenAiProvider::openai(model, api_key.to_string())),
        "anthropic" => {
            let mut p =
                agent_core::provider::anthropic::AnthropicProvider::new(model, api_key.to_string());
            if !base_url.is_empty() && base_url != "https://api.anthropic.com" {
                p.base_url = Some(base_url);
            }
            Arc::new(p)
        }
        "openai_compat" => Arc::new(OpenAiProvider::compat(base_url, model, api_key.to_string())),
        other => return Err(format!("unsupported provider_kind: {other}")),
    };

    // ── Build agent ───────────────────────────────────────────
    let role_enum = match role.as_str() {
        "agent:chief" => agent_core::prompt::Role::Chief,
        "agent:critic:a" => agent_core::prompt::Role::BugHunter,
        "agent:critic:b" => agent_core::prompt::Role::Reviewer,
        "agent:planner" => agent_core::prompt::Role::Planner,
        "agent:reporter" => agent_core::prompt::Role::Reporter,
        _ => agent_core::prompt::Role::Worker,
    };
    let agent = Agent::new(
        role_enum,
        provider,
        // v0.4.36: snapshot (background MCP rebuilds swap the Arc)
        // + permission capabilities + project rules.
        state.tools_snapshot(),
        state.workspace_snapshot(),
        state.agent_config(&state.workspace_snapshot()),
    );

    // ── Stream events to subscribers while running ────────────
    // Keep a copy of the task_text so we can write it into the
    // tasks row at the end (v0.4.21 event 000064).
    let task_text_for_record = task_text.clone();

    // v0.4.21 (event 000066): outer timeout wraps the whole
    // agent.run() loop. Default 5 min — chief tasks with the
    // current provider set rarely exceed 2 min for tool-heavy
    // runs, and we'd rather the chairman see a clean TIMEOUT
    // status than stare at a spinner for an hour because
    // api.minimaxi.com is throttling. Caller can override via
    // body.timeout_secs; cap at 30 min so a buggy client can't
    // DoS the runtime with a 24-hour task.
    let timeout_secs: u64 = body
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(300)
        .clamp(10, 1800);

    let mut rx = agent.run_with_history_and_task_id(task_text, chat_history, None);
    let mut last_status = "UNKNOWN".to_string();
    let mut summary: Option<String> = None;
    let timed_out = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), async {
        while let Some(ev) = rx.recv().await {
            // Best-effort fan-out; if no subscribers, that's fine.
            let _ = state.events.send(ev.clone());
            if let AgentEvent::Done {
                status, summary: s, ..
            } = ev
            {
                last_status = status;
                summary = s;
            }
            if matches!(
                last_status.as_str(),
                "DONE" | "FAILED" | "ABORTED" | "ABORTED_REPEAT"
            ) {
                // If the wf_id was provided, replace the empty one.
                if !wf_id.is_empty() {
                    last_status = format!("{last_status} (wf={wf_id})");
                }
                return false; // not a timeout
            }
        }
        true // channel closed without Done — treat as timeout-shaped
    })
    .await
    .unwrap_or(true);
    if timed_out
        && !matches!(
            last_status.as_str(),
            "DONE" | "FAILED" | "ABORTED" | "ABORTED_REPEAT"
        )
    {
        // Synthesize a Done event so subscribers see the
        // terminal state, and stamp the status the frontend
        // can grep on (event 000066 introduces "TIMEOUT" as a
        // new terminal status; before this the UI would have
        // hung in 'sending=true' indefinitely).
        last_status = format!("TIMEOUT ({timeout_secs}s)");
        summary = Some(format!(
            "agent.run() exceeded the {timeout_secs}s outer timeout. \
             This usually means the upstream provider (e.g. minimax, \
             openai) is hanging, throttling, or returning malformed \
             streaming responses. Inspect /api/errors/recent and \
             consider lowering the per-task timeout_secs."
        ));
        let _ = state.events.send(AgentEvent::Done {
            wf_id: wf_id.clone(),
            status: last_status.clone(),
            summary: summary.clone(),
        });
        // v0.4.21 (event 000066): surface to TopBar badge.
        state.push_error(ErrorRecord {
            at: chrono::Utc::now().timestamp(),
            severity: "error".into(),
            source: "run_task".into(),
            summary: format!("run_task timed out after {timeout_secs}s"),
            detail: Some(format!("role={role} model={model_for_quota}")),
        });
    }

    // ── v0.4.20: quota-failure recording ──────────────────
    // Any non-DONE status counts as a runtime failure.
    // However, ONLY genuine quota / rate-limit failures (HTTP 429, "quota", "rate limit")
    // should record a quota failure and trigger the 5h pending wait.
    // Tool failures or ABORTED_REPEAT should NEVER lock models out for 5 hours!
    let status_clean = last_status
        .split_whitespace()
        .next()
        .unwrap_or("UNKNOWN")
        .to_string();
    if status_clean != "DONE" {
        let err_msg = summary.clone().unwrap_or_else(|| last_status.clone());
        let err_msg: String = err_msg.chars().take(240).collect();
        let is_aborted_or_tool_failure = status_clean == "ABORTED"
            || status_clean == "ABORTED_REPEAT"
            || status_clean == "CANCELLED";
        let is_true_quota_failure = err_msg.to_ascii_lowercase().contains("quota")
            || err_msg.contains("429")
            || err_msg.to_ascii_lowercase().contains("rate limit")
            || err_msg.to_ascii_lowercase().contains("insufficient");

        // Surface failure to the TopBar badge
        state.push_error(ErrorRecord {
            at: chrono::Utc::now().timestamp(),
            severity: "error".into(),
            source: if is_true_quota_failure {
                "quota".into()
            } else {
                "runtime".into()
            },
            summary: format!("{role} run failed: {status_clean}"),
            detail: Some(err_msg.clone()),
        });

        if is_true_quota_failure && !is_aborted_or_tool_failure {
            if let Err(e) = state
                .repo
                .record_quota_failure(&role, &model_for_quota, &err_msg)
                .await
            {
                warn!(error = %e, "v0.4.20: failed to record_quota_failure");
            }
            if role == "agent:chief" {
                if let Err(e) = state
                    .repo
                    .set_quota_pending_5h_wait(&role, &model_for_quota)
                    .await
                {
                    warn!(error = %e, "v0.4.20: failed to set_quota_pending_5h_wait for chief");
                }
            }
        }
    } else {
        // Success path — clear any prior (chief, model) row so
        // the StatusLine flips back to "正常" automatically. We
        // also drop all chief rows when chief itself succeeds
        // (cheap; lets the chairman see a clean slate).
        if let Err(e) = state
            .repo
            .clear_quota_failure(&role, Some(&model_for_quota))
            .await
        {
            warn!(error = %e, "v0.4.20: failed to clear_quota_failure on success");
        }
        if role == "agent:chief" {
            let _ = state.repo.clear_quota_failure(&role, None).await;
        }
    }

    // v0.4.21 (event 000064): persist a task row so the
    // dashboard's "任务列表" panel shows real progress. The
    // tasks table was always empty before this fix (run_task
    // only wrote AgentEvents, never INSERT INTO tasks), so the
    // chairman always saw "0/0 完成" no matter how much chief
    // actually did. We write a single row per run_task call,
    // with the role as `assigned_to` and the final status as
    // `status` (done | failed | aborted). The result column
    // stores the summary so the chairman can re-read it.
    let now = chrono::Utc::now().timestamp();
    let wf_id_for_task = if wf_id.is_empty() {
        // v0.4.19+ chat path doesn't always carry a wf_id
        // (run_task handler's wf_id comes from the legacy
        // "run_agent_task" body). Synthesise a stable per-chat
        // wf_id from user_request + role + now so the task is
        // grouped under something the chairman can recognise.
        let key = format!("{}|{}|{}", role, model_for_quota, task_text_for_record);
        let mut h: u64 = 1469598103934665603;
        for b in key.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(1099511628211);
        }
        format!("wf_chat_{:016x}", h)
    } else {
        wf_id.clone()
    };
    // v0.4.21 (event 000064 follow-up): the tasks table has a
    // FK constraint (`wf_id REFERENCES workflows(id)`) and the
    // storage crate enables `foreign_keys=true` per-connection.
    // For chat-derived wf_ids (synthesised above) there is no
    // matching row in `workflows`, so the INSERT into tasks
    // would fail and the dashboard would still see "0/0 完成".
    // Fix: ensure a workflow row exists for the synthetic id.
    // Use `INSERT OR IGNORE` so concurrent chat tasks with the
    // same wf_chat_* id don't fight — the FIRST one creates the
    // row, the rest no-op. Real (non-synthetic) wf_ids already
    // have a workflows row from the dispatcher, but we still
    // call this — the OR IGNORE makes it safe and idempotent.
    let wf_insert = state
        .repo
        .ensure_workflow_row(&wf_id_for_task, &task_text_for_record, &status_clean)
        .await;
    if let Err(e) = wf_insert {
        warn!("v0.4.21 (event 000064 follow-up): ensure_workflow_row failed for {wf_id_for_task}: {e}");
    }
    let task_id = format!("t_{}", ulid::Ulid::new());
    let status_for_task = status_clean.clone();
    let title = if task_text_for_record.chars().count() > 60 {
        let truncated: String = task_text_for_record.chars().take(60).collect();
        format!("{truncated}…")
    } else {
        task_text_for_record.clone()
    };
    let task_result = state
        .repo
        .create_task(&storage::Task {
            id: task_id.clone(),
            wf_id: wf_id_for_task.clone(),
            parent_id: None,
            title,
            status: status_for_task.to_lowercase(),
            assigned_to: Some(role.clone()),
            model: Some(model_for_quota.clone()),
            repair_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: None,
            files_modified: None,
            started_at: Some(now),
            finished_at: Some(now),
            result: summary.clone(),
        })
        .await;
    if let Err(e) = task_result {
        // Log but do not fail the run_task response — the agent
        // work already succeeded, the dashboard just won't see
        // a new task row this time. This is the path that was
        // silently dropped before event 000064.
        warn!("v0.4.21 (event 000064): create_task failed for {task_id}: {e}");
    }

    Ok((
        200,
        json!({
            "ok": true,
            "status": last_status,
            "summary": summary,
            "task_id": task_id,
            "wf_id": wf_id_for_task,
        }),
    ))
}
