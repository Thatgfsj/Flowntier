//! Runtime log routes.

use super::*;

pub(super) fn register(d: &mut Dispatcher, _state: Arc<ServerState>) {
    // ── v0.4.22 (event 000080): log endpoints ────────
    // Per chairman: "日志的功能让我们设置为开发环节的
    // 功能,你内置出来方便删". These two routes expose
    // the runtime's tracing log file (~/Desktop/Flwntier.log
    // by default) for the chairman to inspect / scrub.
    // Gated by `FLWNTIER_LOG_API=1` so a released build
    // (no env var set) does not expose a file-read surface.
    // When disabled, both endpoints return 403 with a
    // "FLWNTIER_LOG_API is not set" message.
    //
    // GET  /api/logs/get?tail=N — last N lines, default 200.
    // POST /api/logs/clear       — truncate the log file
    //                              to 0 bytes and write a
    //                              sentinel so the next session
    //                              starts cleanly. Returns
    //                              the new (empty) log file
    //                              path on success.
    d.register("GET", "/api/logs/get", |body| {
        let tail = body
            .get("tail")
            .and_then(|v| v.as_u64())
            .unwrap_or(200)
            .min(10_000) as usize;
        Box::pin(async move {
            if !crate::logs::log_api_enabled() {
                return Ok((
                    403,
                    json!({
                        "ok": false,
                        "error": "FLWNTIER_LOG_API is not set; \
                                  log API is a development feature \
                                  (event 000080).",
                    }),
                ));
            }
            let path = crate::logs::resolve_log_path();
            let lines = crate::logs::read_tail(tail, None);
            Ok((
                200,
                json!({
                    "ok": true,
                    "log_file": path.as_ref().map(|p| p.display().to_string()),
                    "log_file_enabled": path.is_some(),
                    "tail": lines.len(),
                    "lines": lines,
                }),
            ))
        })
    });
    d.register("POST", "/api/logs/clear", |_body| {
        Box::pin(async move {
            if !crate::logs::log_api_enabled() {
                return Ok((
                    403,
                    json!({
                        "ok": false,
                        "error": "FLWNTIER_LOG_API is not set; \
                                  log API is a development feature \
                                  (event 000080).",
                    }),
                ));
            }
            match crate::logs::clear_log(None) {
                Ok(path) => Ok((
                    200,
                    json!({
                        "ok": true,
                        "path": path.display().to_string(),
                        "cleared_at": chrono::Utc::now().to_rfc3339(),
                    }),
                )),
                Err(e) => Ok((
                    500,
                    json!({
                        "ok": false,
                        "error": e.to_string(),
                    }),
                )),
            }
        })
    });
}
