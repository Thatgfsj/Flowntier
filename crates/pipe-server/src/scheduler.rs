//! Quota-failure background scheduler (v0.4.20, event 000056).
//!
//! Runs every minute (`tokio::time::interval`). At each 5-hour
//! boundary (`00:01`, `05:01`, `10:01`, `15:01`, `20:01` local time)
//! it scans `quota_failures` for rows in `status='pending_5h_wait'`,
//! then for each row:
//!   1. **Phase-2 (v0.4.20, fully implemented here)**: dispatch a
//!      real `POST /api/run_task` to the in-process dispatcher
//!      using the same `(role, model_id)` the user originally
//!      selected. The body is `{ task: "__quota_retry__", role,
//!      model_id }` — a tiny probe task the chief agent loop
//!      recognises and turns into a no-op + status update.
//!      On success → `clear_quota_failure`. On failure → flip to
//!      `rate_limited`, emit the chairman-mandated nudge to the
//!      events bus.
//!   2. Quiet recovery is automatic: any successful `run_task`
//!      against `(role, model)` (whether the chairman typed it
//!      manually, or chief's loop called it, or this scheduler
//!      retried it) calls `clear_quota_failure` from the run_task
//!      handler, removing the row from the index.
//!
//! Lives for the lifetime of the runtime process. On restart, any
//! `pending_5h_wait` rows persist in SQLite and the next tick
//! after restart retries them at the upcoming boundary.

use std::sync::Arc;
use std::time::Duration;

use chrono::Timelike;
use serde_json::json;
use tracing::{error, info, warn};

use crate::handlers::ServerState;

/// Number of seconds between scheduler ticks. Cheap (one indexed
/// SELECT + a few comparisons per tick).
const TICK_SECS: u64 = 60;

/// 5-hour boundaries expressed as minutes-since-midnight
/// (local time). The nudge fires when current_minute matches
/// one of these. `compute_next_boundary_minute()` picks the
/// next upcoming minute in this set.
const NUDGE_BOUNDARY_MINUTES: &[i32] = &[1, 60 * 5 + 1, 60 * 10 + 1, 60 * 15 + 1, 60 * 20 + 1];

/// Entry point. Spawned via `tokio::spawn` from
/// `bin/flowntier-runtime.rs` and `Server::run`.
pub async fn run_quota_scheduler(state: Arc<ServerState>) {
    let mut interval = tokio::time::interval(Duration::from_secs(TICK_SECS));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut next_boundary = compute_next_boundary_minute(None);
    info!(
        "v0.4.20 quota scheduler started; next 5h boundary in {} min",
        next_boundary.saturating_sub(current_local_minute())
    );

    loop {
        interval.tick().await;
        let now_minute = current_local_minute();
        if now_minute >= next_boundary {
            tick_5h_boundary(&state).await;
            next_boundary = compute_next_boundary_minute(Some(now_minute));
        }
    }
}

/// Compute the next 5-hour-boundary minute-of-day that is
/// strictly greater than `after` (or greater than now if `None`).
fn compute_next_boundary_minute(after: Option<i32>) -> i32 {
    let now = after.unwrap_or_else(current_local_minute);
    NUDGE_BOUNDARY_MINUTES
        .iter()
        .copied()
        .find(|m| *m > now)
        .unwrap_or(NUDGE_BOUNDARY_MINUTES[0])
}

fn current_local_minute() -> i32 {
    use chrono::Local;
    let now = Local::now();
    now.hour() as i32 * 60 + now.minute() as i32
}

/// Fire at every 5-hour boundary. Scan quota_failures for
/// `pending_5h_wait` rows; for each, dispatch a real
/// `POST /api/run_task` against the in-process dispatcher
/// (Phase-2). On success → `clear_quota_failure`. On failure →
/// `mark_quota_rate_limited` + emit the chairman-mandated nudge
/// event + structured tracing::error.
async fn tick_5h_boundary(state: &Arc<ServerState>) {
    let rows = match state.repo.list_pending_5h_wait().await {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "v0.4.20: list_pending_5h_wait failed");
            return;
        }
    };

    if rows.is_empty() {
        // Nothing pending. Silent success (no log churn).
        return;
    }

    info!(
        "v0.4.20: 5h tick — {} pending role/model pair(s) to retry",
        rows.len()
    );

    // v0.4.22 (event 000091 fix #29 + #30): we no longer
    // dispatch through the in-process Dispatcher — instead
    // `probe_provider_alive` does a direct HTTP HEAD/GET to
    // the provider's `/v1/models`. This means we don't need
    // the dispatcher handle and the `RpcRequest`/`RpcParams`
    // imports, which is why they're removed below.
    // (The legacy `let dispatcher = state.dispatcher()` is
    // kept behind a feature for any future retry that needs
    // it.)

    for row in rows {
        let role = row.role_id.clone();
        let model_id = row.model_id.clone();
        let last_error = row.last_error_message.clone();

        // v0.4.22 (event 000091 fix #29 + #30): a real
        // model call wastes tokens AND would itself trigger
        // `record_quota_failure` (loop). Skip the agent loop
        // entirely — instead, do a cheap HEAD/GET to the
        // provider's `/v1/models` endpoint with the same API
        // key. If it returns 2xx, the key is alive and the
        // rate-limit window has rolled over → clear the row.
        // 4xx/5xx → mark rate_limited. No LLM tokens spent,
        // no quota row re-recorded.
        let succeeded = probe_provider_alive(&state, &role, &model_id).await;

        if succeeded {
            // Quiet recovery path: clear the row, no nudge.
            if let Err(e) = state.repo.clear_quota_failure(&role, Some(&model_id)).await {
                warn!(
                    error = %e, role = %role, model = %model_id,
                    "v0.4.20: clear_quota_failure after successful retry failed"
                );
            } else {
                info!(
                    role = %role, model = %model_id,
                    "v0.4.20: 5h tick recovered — quota_failures row cleared"
                );
            }
            continue;
        }

        // Failure path: flip to rate_limited, emit nudge, log.
        if let Err(e) = state
            .repo
            .mark_quota_rate_limited(&role, &model_id)
            .await
        {
            warn!(
                error = %e,
                role = %role, model = %model_id,
                "v0.4.20: failed to mark rate_limited"
            );
            continue;
        }

        // Structured system log (no LLM call — chairman said
        // "日志自动而不是llm"). grep `target=quota` in the
        // runtime log to audit.
        error!(
            target: "quota",
            "rate_limited: role={} model={} last_error='{}' last_attempt=now",
            role, model_id, last_error,
        );

        // Emit the chairman-mandated nudge to the events bus.
        // ChatZone's existing event-stream subscription picks it
        // up and renders the banner. Use a stable AgentEvent shape
        // that ChatZone already recognises (Done with a special
        // status flag would be intrusive; instead we send a custom
        // System variant via the broadcast channel — but to avoid
        // extending the AgentEvent enum in v0.4.20, we just push
        // a textual Done event with a marker the ChatZone can
        // detect).
        use agent_core::AgentEvent;
        let nudge_text = "AI 之前疑似到达上限，目前已经刷新，检查工作进度并且继续工作";
        let _ = state.events.send(AgentEvent::Done {
            wf_id: format!("quota_nudge:{role}:{model_id}"),
            status: format!("QUOTA_NUDGE:{role}:{model_id}"),
            summary: Some(nudge_text.to_string()),
        });
    }
}

/// v0.4.22 (event 000091 fix #29 + #30): cheap liveness probe
/// for a (role, model) pair. Resolves the role's provider +
/// model from `role_overrides`, retrieves the API key from
/// the keystore, and does a `GET /v1/models` against the
/// provider's base_url. 2xx = alive → caller clears the
/// quota_failures row. 4xx/5xx/timeout = still restricted →
/// caller flips to `rate_limited` and emits the nudge.
///
/// Crucially this does **not** invoke the agent loop, so:
///  - no LLM tokens are spent on the probe itself
///  - the probe failure does not increment `attempt_count`
///    in the quota_failures table (preventing a recursive
///    failure → record → fail → record loop)
async fn probe_provider_alive(
    state: &Arc<crate::handlers::ServerState>,
    role: &str,
    model_id: &str,
) -> bool {
    use std::time::Duration;
    // Reuse the same resolution path as the orchestrator.
    let resolved = match crate::handlers::resolve_role_for_orchestrator(state, role).await {
        Ok(r) => r,
        Err(e) => {
            warn!(
                target: "pipe_server::scheduler",
                role, model_id, error = %e,
                "probe_provider_alive: resolve_role failed; skipping probe"
            );
            return false;
        }
    };
    if resolved.model_id != model_id {
        // The role's primary model has been changed since the
        // quota row was created — clear the row, the issue
        // is moot under the new default.
        return true;
    }
    let url = format!("{}/models", resolved.base_url.trim_end_matches('/'));
    let api_key = resolved.api_key.to_string();
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(
                target: "pipe_server::scheduler",
                error = %e, role, model_id,
                "probe_provider_alive: reqwest client build failed"
            );
            return false;
        }
    };
    let resp = match client.get(&url)
        .bearer_auth(&api_key)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            info!(
                target: "pipe_server::scheduler",
                role, model_id, error = %e,
                "probe_provider_alive: HTTP request failed; still restricted"
            );
            return false;
        }
    };
    let alive = resp.status().is_success();
    info!(
        target: "pipe_server::scheduler",
        role, model_id, status = resp.status().as_u16(),
        "probe_provider_alive: result"
    );
    alive
}

