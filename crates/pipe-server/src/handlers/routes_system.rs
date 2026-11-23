//! System routes: MCP status/rescan, recent errors.

use super::*;

pub(super) fn register(d: &mut Dispatcher, state: Arc<ServerState>) {
    // ── v0.4.36: GET /api/mcp/status ─────────────────────────
    // Last tool-registry scan: per-server status + the active
    // permission mode. Purely observational.
    let s_mcp_status = state.clone();
    d.register("GET", "/api/mcp/status", move |_body| {
        let s = s_mcp_status.clone();
        Box::pin(async move {
            let rows = s.mcp_status.lock().await.clone();
            Ok((
                200,
                json!({
                    "ok": true,
                    "mode": s.permission(),
                    "servers": rows,
                }),
            ))
        })
    });

    // ── v0.4.36: POST /api/mcp/rescan ────────────────────────
    // Kick a background registry rebuild (same path as startup and
    // workspace-set). Returns immediately; poll /api/mcp/status.
    let s_mcp_rescan = state.clone();
    d.register("POST", "/api/mcp/rescan", move |_body| {
        let s = s_mcp_rescan.clone();
        Box::pin(async move {
            let s2 = s.clone();
            tokio::spawn(async move {
                tools_registry::rebuild(&s2, "manual_rescan").await;
            });
            Ok((200, json!({"ok": true, "started": true})))
        })
    });

    // ── v0.4.21 (event 000066): GET /api/errors/recent ──
    // Returns the last N error rows captured by the pipe-server
    // (quota failures, run_task timeouts, workspace swap
    // rejections, agent errors). Backed by an in-memory
    // `Arc<Mutex<VecDeque<ErrorRecord>>>` so the TopBar red-dot
    // badge can poll without hammering SQLite. Capacity 200;
    // oldest entries evicted FIFO.
    //
    // Query: ?limit=N (default 10, capped at 200).
    let s_errors = state.clone();
    d.register("GET", "/api/errors/recent", move |body| {
        let s = s_errors.clone();
        let limit = body
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(10)
            .min(200) as usize;
        Box::pin(async move {
            let rows = s.recent_errors(limit);
            Ok((
                200,
                json!({
                    "ok": true,
                    "count": rows.len(),
                    "rows": rows,
                }),
            ))
        })
    });
}
