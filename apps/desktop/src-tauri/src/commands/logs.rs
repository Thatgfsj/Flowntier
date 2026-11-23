//! Log file search, redaction, and frontend log forwarding commands.

use crate::sidecar::pipe_request;

/// Redact common API-key / token shapes from a log line before
/// it leaves the Tauri boundary. BUG-006 fix.
///
/// Handles 4 substring patterns:
///
///   1. `Bearer …` — Authorization header value in HTTP logs.
///   2. `sk-…` — OpenAI / Anthropic / DeepSeek key prefixes.
///   3. `sk_live_…` / `sk_test_…` — Stripe-style keys.
///   4. `KEY=value` for `*_KEY`, `*_TOKEN`, `*_SECRET`, `API_KEY`,
///      `PASSWORD` (uppercase keys; common env-var serialisation).
///
/// Implementation note: we use stdlib only (no regex dep). For
/// v0.5+, switch to the `regex` crate — these hand-rolled
/// scanners are correct for the tested inputs but conservative
/// about edge cases (e.g. values with embedded `=`).
fn redact_secrets(line: &str) -> String {
    let mut out = line.to_string();

    // Helper: replace all occurrences of `prefix` followed by
    // alphanumeric/_/- chars with `replacement` (no recursion —
    // each match is replaced independently).
    fn replace_token(s: &str, prefix: &str, replacement: &str, include_prefix: bool) -> String {
        let mut out = String::with_capacity(s.len());
        let bytes = s.as_bytes();
        let prefix_bytes = prefix.as_bytes();
        let plen = prefix_bytes.len();
        let mut i = 0;
        while i + plen <= bytes.len() {
            if &bytes[i..i + plen] == prefix_bytes {
                let after_start = i + plen;
                if include_prefix {
                    // Copy the prefix verbatim, then replace the
                    // value chars that follow.
                    let mut j = after_start;
                    while j < bytes.len()
                        && (bytes[j].is_ascii_alphanumeric()
                            || bytes[j] == b'_'
                            || bytes[j] == b'-')
                    {
                        j += 1;
                    }
                    out.push_str(prefix);
                    out.push_str(replacement);
                    i = j;
                } else {
                    // Don't copy the prefix; replace it AND the
                    // value chars that follow.
                    let mut j = after_start;
                    while j < bytes.len()
                        && (bytes[j].is_ascii_alphanumeric()
                            || bytes[j] == b'_'
                            || bytes[j] == b'-')
                    {
                        j += 1;
                    }
                    out.push_str(replacement);
                    i = j;
                }
            } else {
                // Copy one char (UTF-8 safe via char_indices).
                let c = s[i..].chars().next().unwrap();
                out.push(c);
                i += c.len_utf8();
            }
        }
        // Copy any trailing bytes we missed (shouldn't happen with
        // ASCII-only inputs but be safe).
        if i < bytes.len() {
            out.push_str(&s[i..]);
        }
        out
    }

    // Pattern 1: Bearer
    out = replace_token(&out, "Bearer ", "<redacted>", true);

    // Pattern 3 first (more specific): sk_live_/sk_test_ must run
    // BEFORE Pattern 2 so the longer prefix wins. We replace the
    // whole "sk_live_xxx" with "sk_live_<redacted>".
    for prefix in &["sk_live_", "sk_test_"] {
        // Manually: copy prefix, then run Pattern 2 logic on the
        // value chars that follow.
        let mut new_out = String::with_capacity(out.len());
        let bytes = out.as_bytes();
        let prefix_bytes = prefix.as_bytes();
        let plen = prefix_bytes.len();
        let mut i = 0;
        while i + plen <= bytes.len() {
            if &bytes[i..i + plen] == prefix_bytes {
                let mut j = i + plen;
                while j < bytes.len()
                    && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] == b'-')
                {
                    j += 1;
                }
                new_out.push_str(prefix);
                new_out.push_str("<redacted>");
                i = j;
            } else {
                let c = out[i..].chars().next().unwrap();
                new_out.push(c);
                i += c.len_utf8();
            }
        }
        if i < bytes.len() {
            new_out.push_str(&out[i..]);
        }
        out = new_out;
    }

    // Pattern 2: sk- (any alphanumeric/-/_ after)
    out = replace_token(&out, "sk-", "sk-<redacted>", false);

    // Pattern 4: KEY=value for env-var-shaped secrets. We do a
    // single forward pass, looking for any of the keyword suffixes
    // preceded by uppercase letters/digits/_, followed by `=` or
    // `:` and a value. When found, replace the value with
    // `<redacted>`. Skip if the value already contains `<redacted>`
    // (Pattern 2/3 handled it).
    for keyword in &["_KEY", "_TOKEN", "_SECRET", "API_KEY", "PASSWORD"] {
        let klen = keyword.len();
        let bytes = out.as_bytes();
        let mut new_out = String::with_capacity(out.len());
        let mut i = 0;
        while i < bytes.len() {
            // Find the next occurrence of `keyword` starting at i.
            let mut j = i;
            let mut found = false;
            while j + klen <= bytes.len() {
                if &bytes[j..j + klen] == keyword.as_bytes() {
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                new_out.push_str(&out[i..]);
                break;
            }
            // Walk backwards from j to find key start.
            let mut key_start = j;
            while key_start > 0 {
                let prev = bytes[key_start - 1];
                if prev.is_ascii_uppercase() || prev.is_ascii_digit() || prev == b'_' {
                    key_start -= 1;
                } else {
                    break;
                }
            }
            // Need at least one char before the keyword.
            if key_start == j {
                // Skip past this keyword occurrence.
                new_out.push_str(&out[i..j + klen]);
                i = j + klen;
                continue;
            }
            // Copy everything from i up to key_end verbatim.
            let key_end = j + klen;
            new_out.push_str(&out[i..key_end]);
            // Skip whitespace, expect `=` or `:`.
            let mut sep = key_end;
            while sep < bytes.len() && (bytes[sep] == b' ' || bytes[sep] == b'\t') {
                sep += 1;
            }
            if sep >= bytes.len() || (bytes[sep] != b'=' && bytes[sep] != b':') {
                // No separator; skip past this keyword.
                i = key_end;
                continue;
            }
            new_out.push(bytes[sep] as char);
            let mut v = sep + 1;
            // Optional opening quote.
            if v < bytes.len() && (bytes[v] == b'"' || bytes[v] == b'\'') {
                new_out.push(bytes[v] as char);
                v += 1;
            }
            let v_start = v;
            while v < bytes.len() {
                let b = bytes[v];
                if b.is_ascii_whitespace()
                    || b == b','
                    || b == b'}'
                    || b == b']'
                    || b == b'"'
                    || b == b'\''
                {
                    break;
                }
                v += 1;
            }
            // Check if already redacted.
            let value_seg = &out[v_start..v];
            if value_seg.contains("<redacted>") {
                // Already handled — copy value verbatim.
                new_out.push_str(value_seg);
            } else {
                new_out.push_str("<redacted>");
            }
            i = v;
        }
        out = new_out;
    }

    out
}

/// Write a frontend error to the Rust log file.
///
/// Called from `apps/desktop/src/components/ErrorBoundary.tsx` when
/// the React tree throws. Best-effort: failures are logged but
/// never thrown back to the JS side (that would deadlock the
/// error UI).
#[tauri::command]
pub async fn log_frontend_error(
    message: String,
    stack: String,
    component_stack: String,
) -> Result<(), String> {
    tracing::error!(
        target: "frontend",
        component_stack = component_stack,
        "React uncaught error: {message}"
    );
    // Write the stack at debug level so the file includes it but
    // it doesn't show up in stderr (dev) by default. Users can
    // grep the log file directly.
    tracing::debug!(target: "frontend.stack", "{stack}");
    Ok(())
}

// v0.4.21 (event 000063): webview console forward. The webview
// side has an inline script in apps/desktop/index.html that
// intercepts console.log / console.error / console.warn /
// window.onerror / unhandledrejection and invokes this command
// with one line at a time. We log each line under the
// `flowntier_webview_console` target so it lands in
// %APPDATA%\flowntier\logs\flowntier.log.<date>. Critical for
// diagnosing "首页打不开" reports in production builds where
// devtools are off and Tauri 2.x does NOT forward webview
// console to the Rust log by default.
#[tauri::command]
pub async fn log_webview_console(line: String) -> Result<(), String> {
    tracing::info!(target: "flowntier_webview_console", "{}", line);
    Ok(())
}

/// Search the daily-rolling log file for lines containing the
/// given error code (e.g. "FE-3a7b9c2d"). Settings → About →
/// "Search my bug" panel calls this; the user pastes the code
/// shown on the ErrorBoundary screen and we surface every log
/// line that references it.
///
/// Returns a JSON envelope:
///   { "matches": string[], "scanned": number, "truncated": bool }
///
/// `matches` is capped at 200 lines (oldest first within each
/// file, files newest-first across the log dir) so a runaway
/// code doesn't melt the modal. `truncated` is set if we hit
/// that cap.
///
/// `since` is an ISO-8601 date string (e.g. "2026-06-26") used
/// to skip older log files. The frontend currently passes null
/// because the daily rolling files are small enough that
/// scanning all of them is fine for v0.4. We accept the
/// parameter so we don't need to bump the schema later.
///
/// Panic files (`panic-*.log`) are deliberately excluded —
/// they have a different format (free-form backtrace dump) and
/// false-positive heavily on user-provided error codes.
#[tauri::command]
pub fn search_log(
    code: String,
    since: Option<String>,
    include_panic_logs: Option<bool>,
) -> Result<serde_json::Value, String> {
    let needle = code.trim();
    if needle.is_empty() {
        return Err("code is empty".into());
    }
    let include_panic_logs = include_panic_logs.unwrap_or(false);
    let Some(data_dir) = storage::Repository::default_data_dir() else {
        return Err("cannot determine data dir".into());
    };
    let log_dir = tauri_core::logging::log_dir(&data_dir);
    let entries = match std::fs::read_dir(&log_dir) {
        Ok(e) => e,
        Err(e) => {
            // BUG-003 fix: a fresh install (or post-wipe_all_data)
            // has no logs/ dir yet. Treat as "empty result" rather
            // than an error — the user's UI shows "scanned 0 files"
            // instead of a scary "search failed" message.
            if e.kind() == std::io::ErrorKind::NotFound {
                return Ok(serde_json::json!({
                    "matches": Vec::<String>::new(),
                    "scanned": 0,
                    "truncated": false,
                }));
            }
            return Err(format!("read_dir {}: {e}", log_dir.display()));
        }
    };
    // Collect (path, modified) so we can scan newest-first. A
    // user who just hit the error wants to see today's lines
    // before yesterday's.
    //
    // BUG-010 fix (event 000023): panic dumps are now opt-in
    // via the `include_panic_logs` param. Default false because
    // panic dumps are hundreds of KB of backtrace and false-
    // positive heavily on substring searches, but the user
    // knows best when they're hunting a crash.
    let mut files: Vec<(std::path::PathBuf, std::time::SystemTime, u64)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?;
            let is_panic = name.starts_with("panic-");
            let is_log = name.starts_with("flowntier.log");
            if !(is_log || (include_panic_logs && is_panic)) {
                return None;
            }
            let meta = e.metadata().ok()?;
            let mtime = meta.modified().ok()?;
            let size = meta.len();
            Some((p, mtime, size))
        })
        .collect();
    files.sort_by(|a, b| b.1.cmp(&a.1));

    // `since` is reserved for the v0.5 frontend (when the panel
    // grows a date picker). For v0.4 the panel always passes
    // null because daily rolling files are small enough that
    // scanning all of them is cheap. We accept the param so we
    // don't have to bump the IPC schema later. Malformed input
    // is treated as "no filter" so a bad value can't brick the
    // modal.
    let _since_ignored = since;

    const MAX_MATCHES: usize = 200;
    /// BUG-004 fix: cap the bytes we'll read from a single log
    /// file at 64 MiB. Without this cap, a runaway log file
    /// (e.g. an agent loop spamming errors) could OOM the Tauri
    /// process. We stop reading at the cap but mark `truncated`
    /// so the user knows there's more content we didn't scan.
    const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
    /// BUG-004 fix: cap individual lines at 8 KiB. Without this
    /// cap, a single log line could be GB (e.g. an http body blob
    /// without newlines) and one bad file would still pin RAM.
    /// Lines longer than this are truncated and appended with
    /// `…[truncated]` so the user can still see what matched.
    const MAX_LINE_BYTES: usize = 8 * 1024;

    let mut matches: Vec<String> = Vec::new();
    let mut scanned: usize = 0;
    let mut truncated = false;

    'outer: for (path, _mtime, size) in files {
        // Skip files larger than MAX_FILE_BYTES upfront (cheap).
        if size > MAX_FILE_BYTES {
            truncated = true;
            continue;
        }
        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => continue, // permission denied / file vanished — skip
        };
        let mut reader = std::io::BufReader::with_capacity(64 * 1024, file);
        let mut bytes_read: u64 = 0;
        let mut line = String::new();
        loop {
            line.clear();
            let n = match std::io::BufRead::read_line(&mut reader, &mut line) {
                Ok(0) => break, // EOF
                Ok(n) => n,
                Err(_) => break, // read error — skip rest of file
            };
            bytes_read += n as u64;
            if bytes_read > MAX_FILE_BYTES {
                truncated = true;
                break;
            }
            // Strip the trailing newline (or \r\n) so the trimmed
            // line we display doesn't have whitespace artifacts.
            let trimmed = line.trim_end_matches(['\n', '\r']);
            // Per-line cap (BUG-004 fix).
            let truncated_line = if trimmed.len() > MAX_LINE_BYTES {
                let mut s = String::with_capacity(MAX_LINE_BYTES + 16);
                s.push_str(&trimmed[..MAX_LINE_BYTES]);
                s.push_str("…[truncated]");
                s
            } else {
                trimmed.to_string()
            };
            scanned += 1;
            if truncated_line.contains(needle) {
                // BUG-006 fix (event 000020): redact API keys,
                // bearer tokens, and similar secrets before
                // returning the line. The user might paste an
                // error code from the ErrorBoundary, but the
                // matched log line could contain
                // `OPENAI_API_KEY=sk-…` or an Authorization header
                // — sending that back to the webview exposes the
                // secret (and the user can copy it from the
                // modal). The redactor runs per-line, so
                // non-secret context is preserved.
                matches.push(redact_secrets(&truncated_line));
                if matches.len() >= MAX_MATCHES {
                    truncated = true;
                    break 'outer;
                }
            }
        }
    }

    Ok(serde_json::json!({
        "matches": matches,
        "scanned": scanned,
        "truncated": truncated,
    }))
}

/// v0.4.21 (event 000066): read recent errors captured by the
/// pipe-server for the TopBar red-dot badge. Returns at most
/// `limit` entries (default 10). Used by the ErrorAggregator
/// polling job in App.tsx.
#[tauri::command]
pub async fn get_recent_errors(body: serde_json::Value) -> Result<serde_json::Value, String> {
    let limit = body.get("limit").and_then(|v| v.as_u64()).unwrap_or(10);
    let path = format!("/api/errors/recent?limit={limit}");
    pipe_request("GET", &path, None).await
}

/// v0.4.22 (event 000080): dev-mode log endpoints.
/// Wraps the runtime's `GET /api/logs/get` so the Settings
/// panel can show a "View log" / "Clear log" pair of buttons.
/// The runtime gates the routes behind `FLWNTIER_LOG_API=1`;
/// this command just surfaces the gate to the user.
#[tauri::command]
pub async fn get_log_tail(body: serde_json::Value) -> Result<serde_json::Value, String> {
    let tail = body.get("tail").and_then(|v| v.as_u64()).unwrap_or(200);
    let path = format!("/api/logs/get?tail={tail}");
    pipe_request("GET", &path, None).await
}

#[tauri::command]
pub async fn clear_log_file() -> Result<serde_json::Value, String> {
    pipe_request("POST", "/api/logs/clear", None).await
}

/// v0.4.22 (event 000080): companion command that tells the
/// UI where the log file is, so the Settings panel can show
/// "open log file location" / show the path. Reads the env var
/// path on the runtime side via the route we just added
/// (the route returns `log_file` in its response, so this
/// command is a thin pass-through that just calls /api/logs/get
/// with tail=0 and surfaces the `log_file` field).
#[tauri::command]
pub async fn get_log_file_path() -> Result<serde_json::Value, String> {
    pipe_request("GET", "/api/logs/get?tail=0", None).await
}
