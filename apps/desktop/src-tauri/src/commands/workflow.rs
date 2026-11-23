//! Workflow run, query and cancel commands.

use crate::sidecar::pipe_request;
use crate::util::{url_encode, workflow_to_json};
use tauri_core::{AppState, NewWorkflowResponse};

/// Run a single agent task envelope end-to-end.
///
/// Bridges the React `ChatZone` to the embedded pipe-server's
/// `/api/run_task` handler. The handler drives the in-process
/// agent-core loop, which streams events back over
/// `\\.\pipe\flowntier_runtime_events` and surfaces them through
/// the normal `wf:event` Tauri channel.
///
/// Body shape matches `run_task` in `crates/pipe-server/src/handlers.rs`:
///   { task: string, role?: string, provider_kind?: string,
///     base_url?: string, model?: string, api_key?: string,
///     api_key_env?: string }
#[tauri::command]
pub async fn run_agent_task(body: serde_json::Value) -> Result<serde_json::Value, String> {
    pipe_request("POST", "/api/run_task", Some(body)).await
}

/// v0.4.22 (event 000068): kick off the 8-phase workflow
/// described in history/PROJECT_SPEC.md. The orchestrator runs
/// chief → plan → critic review → dispatch workers → final
/// review → repair → delivery, emitting a `PhaseTransition`
/// event on each step so the UI's PhaseTimeline animates.
///
/// Body: { task: "<user_request>" }. Optional `mode` field
/// selects "single" (legacy run_task) vs "workflow" (this
/// orchestrator) — for now we only support workflow.
#[tauri::command]
pub async fn run_workflow(body: serde_json::Value) -> Result<serde_json::Value, String> {
    tracing::info!(target: "tauri_ipc", "[TRACE] run_workflow Tauri command invoked from frontend");
    let result = pipe_request("POST", "/api/run_workflow", Some(body)).await;
    match &result {
        Ok(v) => {
            tracing::info!(target: "tauri_ipc", response = %v, "[TRACE] run_workflow: success")
        }
        Err(e) => tracing::error!(target: "tauri_ipc", error = %e, "[TRACE] run_workflow: FAILED"),
    }
    result
}

#[tauri::command]
pub async fn start_workflow_cmd(
    _state: tauri::State<'_, AppState>,
    text: String,
    project_id: Option<String>,
) -> Result<NewWorkflowResponse, String> {
    // v0.4.22 (event 000094): the tauri_core::start_workflow is
    // a STUB — it writes a workflow row to the DB and returns,
    // but never invokes the orchestrator. The UI was calling
    // this stub, so chairman's "tarot" requests completed
    // instantly with state REQ_RECEIVED and no agent runs.
    //
    // Route through the pipe-server's `/api/run_workflow`
    // instead — that's where the real 8-phase orchestrator
    // lives (event 000068). The pipe-server handles workflow
    // row creation, agent spawning, and broadcast events on
    // its own.
    let _ = project_id; // currently unused — orchestrator derives id
    let body = serde_json::json!({ "task": text });
    let resp = pipe_request("POST", "/api/run_workflow", Some(body)).await?;
    let wf_id = resp
        .get("wf_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "runtime returned no wf_id".to_string())?
        .to_string();
    Ok(NewWorkflowResponse { id: wf_id })
}

#[tauri::command]
pub async fn get_workflow(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Option<serde_json::Value>, String> {
    let wf_opt = state
        .repo
        .get_workflow(&id)
        .await
        .map_err(|e| e.to_string())?;
    let Some(wf) = wf_opt else {
        return Ok(None);
    };
    let mut json = workflow_to_json(wf);
    if let Ok(tasks) = state.repo.list_tasks_for(&id).await {
        let total = tasks.len();
        let done = tasks.iter().filter(|t| t.status == "done").count();
        if let Some(obj) = json.as_object_mut() {
            obj.insert("tasks_done".into(), serde_json::json!(done));
            obj.insert("tasksDone".into(), serde_json::json!(done));
            obj.insert("tasks_total".into(), serde_json::json!(total));
            obj.insert("tasksTotal".into(), serde_json::json!(total));
            let tasks_json: Vec<serde_json::Value> = tasks
                .into_iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.id,
                        "workflow_id": t.wf_id,
                        "workflowId": t.wf_id,
                        "parent_id": t.parent_id,
                        "title": t.title,
                        "status": t.status,
                        "state": t.status,
                        "assigned_to": t.assigned_to,
                        "repair_count": t.repair_count,
                        "repairCount": t.repair_count,
                        "result": t.result,
                        "started_at": t.started_at,
                        "finished_at": t.finished_at,
                    })
                })
                .collect();
            obj.insert("tasks".into(), serde_json::json!(tasks_json));
        }
    }
    Ok(Some(json))
}

#[tauri::command]
pub async fn cancel_workflow(id: String) -> Result<(), String> {
    // v0.4.22 (event 000091 fix #33): URL-encode the wf_id so
    // a workflow id with `/` or other URL-unsafe chars
    // doesn't break the route. Also now uses a JSON body
    // instead of a path placeholder, since the runtime's
    // dispatcher matches `body.get("wf_id")` for this route.
    let body = serde_json::json!({ "wf_id": id });
    pipe_request("POST", "/api/workflow/cancel", Some(body)).await?;
    Ok(())
}

/// v0.4.22 (event 000069): poll a workflow's current status.
/// The orchestrator runs on a background tokio task (POST
/// returns wf_id immediately); clients poll this to know when
/// the workflow finished + what the final summary is.
#[tauri::command]
pub async fn get_workflow_status(body: serde_json::Value) -> Result<serde_json::Value, String> {
    let wf_id = body.get("wf_id").and_then(|v| v.as_str()).unwrap_or("");
    if wf_id.is_empty() {
        return Err("missing 'wf_id'".into());
    }
    let path = format!("/api/workflow/{}/status", url_encode(wf_id));
    pipe_request("GET", &path, None).await
}
