//! Onboarding routes: KV store, sample workflows, quota, tasks, workspace get/set.

use super::*;

pub(super) fn register(d: &mut Dispatcher, state: Arc<ServerState>) {
    // ── Stub handlers for endpoints the Tauri shell calls but
    // the v0.2.5-era Python server implemented. These are
    // best-effort placeholders that return a shape the UI
    // already expects; full implementations land as the
    // corresponding Rust modules come online (v0.4+).
    //
    // We keep them in this file rather than in a separate
    // handlers_i_ching module because they belong to a
    // different domain (provider / router / secret store /
    // plugin registry) and have no shared logic to factor out.
    // ── KV store (Phase 4 onboarding) ─────────────────────────
    // GET /api/kv/{key}   — returns the JSON value or null
    let kv_get_state = state.clone();
    d.register("GET", "/api/kv/{key}", move |body| {
        let s = kv_get_state.clone();
        Box::pin(async move {
            let key = body
                .get("key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing 'key'".to_string())?
                .to_string();
            match s.repo.kv_get(&key).await {
                Ok(Some(v)) => match serde_json::from_str::<Value>(&v) {
                    Ok(parsed) => Ok((200, json!({ "k": key, "v": parsed }))),
                    Err(_) => Ok((200, json!({ "k": key, "v": Value::String(v) }))),
                },
                Ok(None) => Ok((200, json!({ "k": key, "v": Value::Null }))),
                Err(e) => Ok((500, json!({ "error": format!("kv_get: {e}") }))),
            }
        })
    });

    // POST /api/kv/{key}   — body { value: <json> }
    let kv_set_state = state.clone();
    d.register("POST", "/api/kv/{key}", move |body| {
        let s = kv_set_state.clone();
        Box::pin(async move {
            let key = body
                .get("key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "missing 'key'".to_string())?
                .to_string();
            let value = body.get("value").cloned().unwrap_or(Value::Null);
            let value_str = serde_json::to_string(&value).map_err(|e| format!("serialize: {e}"))?;
            if let Err(e) = s.repo.kv_set(&key, &value_str).await {
                return Ok((500, json!({ "error": format!("kv_set: {e}") })));
            }
            Ok((200, json!({ "k": key, "v": value })))
        })
    });

    // POST /api/kv/first_run/complete  — convenience that writes
    // first_run=false in one call.
    let kv_first_run_complete_state = state.clone();
    d.register("POST", "/api/kv/first_run/complete", move |_body| {
        let s = kv_first_run_complete_state.clone();
        Box::pin(async move {
            if let Err(e) = s.repo.kv_set("first_run", "false").await {
                return Ok((500, json!({ "error": format!("kv_set: {e}") })));
            }
            Ok((200, json!({ "first_run": false })))
        })
    });

    // ── Sample workflows (Phase 4 onboarding) ──────────────────
    // GET /api/sample/{name}  — returns a serialized WorkflowRun
    // envelope. The frontend can submit it via run_agent_task.
    d.register("GET", "/api/sample/{name}", |body| {
        let name = body
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("auth_login")
            .to_string();
        Box::pin(async move { Ok((200, sample_workflow(&name))) })
    });

    // ── v0.4.20: GET /api/quota/status ────────────────────────
    // Returns all quota_failures rows. Frontend (Settings →
    // 角色额度状态) renders the union.
    let s_qstatus = state.clone();
    d.register("GET", "/api/quota/status", move |_body| {
        let s = s_qstatus.clone();
        Box::pin(async move {
            let rows = match s.repo.list_all_quota_failures().await {
                Ok(r) => r,
                Err(e) => return Ok((500, json!({ "ok": false, "error": format!("list: {e}") }))),
            };
            let items: Vec<Value> = rows
                .into_iter()
                .map(|r| {
                    json!({
                        "role_id": r.role_id,
                        "model_id": r.model_id,
                        "last_error_at": r.last_error_at,
                        "last_error_message": r.last_error_message,
                        "status": r.status,
                        "attempt_count": r.attempt_count,
                        "next_attempt_at": r.next_attempt_at,
                    })
                })
                .collect();
            Ok((200, json!({ "ok": true, "rows": items })))
        })
    });

    // ── v0.4.20: POST /api/quota/reset ─────────────────────
    // Body: { role: "agent:chief", model_id?: "minimax:MiniMax-Text-01" }
    // model_id absent = clear every row for that role (chief's
    // "forget history" button).
    let s_qreset = state.clone();
    d.register("POST", "/api/quota/reset", move |body| {
        let s = s_qreset.clone();
        Box::pin(async move {
            let Some(role) = body.get("role").and_then(|v| v.as_str()) else {
                return Ok((400, json!({ "ok": false, "error": "missing 'role'" })));
            };
            let model_id = body.get("model_id").and_then(|v| v.as_str());
            match s.repo.clear_quota_failure(role, model_id).await {
                Ok(n) => Ok((200, json!({ "ok": true, "cleared_rows": n }))),
                Err(e) => Ok((500, json!({ "ok": false, "error": format!("reset: {e}") }))),
            }
        })
    });

    // v0.4.21 (event 000064): GET /api/tasks?wf_id=... returns
    // the persisted task list for a workflow so the dashboard's
    // "任务列表 0/0 完成" panel can show real progress. Before
    // this fix the tasks table was always empty — the run_task
    // handler wrote AgentEvents but never INSERT INTO tasks.
    let s_tasks = state.clone();
    d.register("GET", "/api/tasks", move |body| {
        let s = s_tasks.clone();
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
                        "error": "missing 'wf_id'",
                    }),
                ));
            }
            match s.repo.list_tasks_for(&wf_id).await {
                Ok(rows) => {
                    let items: Vec<Value> = rows
                        .into_iter()
                        .map(|t| {
                            json!({
                                "id": t.id,
                                "wf_id": t.wf_id,
                                "parent_id": t.parent_id,
                                "title": t.title,
                                "status": t.status,
                                "assigned_to": t.assigned_to,
                                "model": t.model,
                                "repair_count": t.repair_count,
                                "input_tokens": t.input_tokens,
                                "output_tokens": t.output_tokens,
                                "cost_usd": t.cost_usd,
                                "files_modified": t.files_modified,
                                "started_at": t.started_at,
                                "finished_at": t.finished_at,
                                "result": t.result,
                            })
                        })
                        .collect();
                    let (done, total) = s.repo.count_tasks(&wf_id).await.unwrap_or((0, 0));
                    Ok((
                        200,
                        json!({
                            "ok": true,
                            "wf_id": wf_id,
                            "done": done,
                            "total": total,
                            "rows": items,
                        }),
                    ))
                }
                Err(e) => Ok((
                    500,
                    json!({
                        "ok": false,
                        "error": format!("list_tasks: {e}"),
                    }),
                )),
            }
        })
    });

    // ── v0.4.21 (event 000066): GET /api/workspace ────────
    // Returns the current workspace root + display name. Cheap
    // snapshot — used by the Tauri shell's "is the workdir the
    // same as what runtime thinks it is?" diagnostic and by the
    // new FileTree component to anchor its root before
    // `GET /api/workspace/tree`.
    let s_wsget = state.clone();
    d.register("GET", "/api/workspace", move |_body| {
        let s = s_wsget.clone();
        Box::pin(async move {
            let ws = s.workspace_snapshot();
            Ok((
                200,
                json!({
                    "ok": true,
                    "root": ws.root.to_string_lossy(),
                    "name": ws.name,
                }),
            ))
        })
    });

    // ── v0.4.21 (event 000066): POST /api/workspace/set ────
    // Body: { path: "C:\\path\\to\\workdir" } or { path: "/abs/path" }
    // Swaps the runtime's workspace root in place. Idempotent —
    // calling with the same path is a no-op (just rewrites the
    // RwLock with the same value). This is the bridge between
    // Tauri `set_workdir_with_nwt` (which only writes
    // workdir.json) and the actual chief agent's filesystem
    // context. Without this, the chairman's "切工作目录" UX
    // changed workdir.json but chief kept writing to the
    // launch-time cwd — silently diverging.
    let s_wsset = state.clone();
    d.register("POST", "/api/workspace/set", move |body| {
        let s = s_wsset.clone();
        Box::pin(async move {
            let Some(path) = body.get("path").and_then(|v| v.as_str()) else {
                let rec = ErrorRecord {
                    at: chrono::Utc::now().timestamp(),
                    severity: "warn".into(),
                    source: "workspace_swap".into(),
                    summary: "POST /api/workspace/set missing 'path'".into(),
                    detail: None,
                };
                s.push_error(rec);
                return Ok((
                    400,
                    json!({
                        "ok": false,
                        "error": "missing 'path'",
                    }),
                ));
            };
            let p = std::path::PathBuf::from(path);
            if !p.exists() {
                let rec = ErrorRecord {
                    at: chrono::Utc::now().timestamp(),
                    severity: "warn".into(),
                    source: "workspace_swap".into(),
                    summary: format!("workdir path does not exist: {path}"),
                    detail: None,
                };
                s.push_error(rec);
                return Ok((
                    400,
                    json!({
                        "ok": false,
                        "error": format!("path does not exist: {}", p.display()),
                    }),
                ));
            }
            if !p.is_dir() {
                let rec = ErrorRecord {
                    at: chrono::Utc::now().timestamp(),
                    severity: "warn".into(),
                    source: "workspace_swap".into(),
                    summary: format!("workdir not a directory: {path}"),
                    detail: None,
                };
                s.push_error(rec);
                return Ok((
                    400,
                    json!({
                        "ok": false,
                        "error": format!("not a directory: {}", p.display()),
                    }),
                ));
            }
            let abs = match p.canonicalize() {
                Ok(a) => agent_core::workspace::normalize_path(&a),
                Err(e) => {
                    return Ok((
                        500,
                        json!({
                            "ok": false,
                            "error": format!("canonicalize: {e}"),
                        }),
                    ))
                }
            };
            s.set_workspace(abs.clone());
            Ok((
                200,
                json!({
                    "ok": true,
                    "root": abs.to_string_lossy(),
                    "previous_root": s.workspace_snapshot().root.to_string_lossy(),
                }),
            ))
        })
    });
}

/// Return a hardcoded sample workflow envelope. The frontend
/// posts this to run_agent_task to start a workflow.
///
/// Currently only one sample: 'auth_login' — a small Flask-style
/// "implement POST /auth/login endpoint" task that exercises
/// the full plan-then-execute flow.
fn sample_workflow(name: &str) -> Value {
    match name {
        "auth_login" => json!({
            "name": "auth_login",
            "display_name": "示例任务 - 实现 POST /auth/login",
            "description":
                "通过一个完整的工作流示例展示 Flowntier 的工作方式: \
                 首席 Agent 拆解任务, 规划 Agent 出方案, 工匠 Agent 写代码, \
                 缺陷猎手 和 质检师审核, 最后汇报.",
        "user_request": concat!(
                "实现 POST /auth/login 接口. 要求: ",
                "1. 接收 JSON ", "{ username, password }", " ",
                "2. 校验非空、长度 >= 3 ",
                "3. 与内存中的用户表比对 ",
                "4. 成功返回 ", "{ token: <random_hex_32>, expires_in: 3600 }", " ",
                "5. 失败返回 401 ",
                "6. 写至少 4 个测试用例 (valid, missing fields, wrong password, unknown user)"
            ),
            "expected_tasks": [
                "分析需求 (chief)",
                "设计方案 (planner)",
                "实现代码 (worker)",
                "测试 + 自检 (worker)",
                "代码审查 (critic)",
                "汇报 (reporter)",
            ],
        }),
        _ => json!({
            "error": format!("unknown sample: {name}"),
            "known_samples": ["auth_login"],
        }),
    }
}
