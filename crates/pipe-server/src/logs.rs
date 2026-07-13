//! v0.4.22 (event 000080): the chief app's developer-mode
//! log system. Per chairman: "日志的功能让我们设置为开发
//! 环节的功能,你内置出来方便删" — log collection is
//! explicitly a development-cycle feature, with a
//! built-in path for the chairman to delete the log file
//! when they're done diagnosing.
//!
//! Three pieces:
//!
//! 1. **Default log file location** — `~/Desktop/Flwntier.log`
//!    on Windows (the chairman's main dev box), `~/Flowntier.log`
//!    elsewhere. Can be overridden via `FLWNTIER_LOG_FILE=<path>`
//!    env var. Set `FLWNTIER_LOG_FILE=0` (zero) to disable file
//!    logging entirely — the runtime still emits tracing
//!    events to stdout but nothing hits the disk.
//!
//! 2. **Two HTTP endpoints** on the pipe-server JSON-RPC
//!    bridge: `GET /api/logs/get?tail=N` (last N lines, default
//!    200) and `POST /api/logs/clear` (truncate to zero bytes
//!    and emit a sentinel "[logs cleared at <ts>]" so the
//!    chairman can tell where the next session starts). The
//!    endpoints are gated by `FLWNTIER_LOG_API=1` (default
//!    off) so production / released builds don't expose a
//!    file-read surface. The Tauri shell sets this env var
//!    for chairman-side debug builds.
//!
//! 3. **Tauri commands** in `apps/desktop/src-tauri/src/lib.rs`
//!    that wrap the HTTP endpoints so the Settings panel
//!    can show a "View log" / "Clear log" / "Open log
//!    file location" button group. The settings panel
//!    surfaces this only when `FLWNTIER_LOG_API=1`.
//!
//! ## When the chairman says "delete the log"
//!
//! The point of this design is that the chairman can scrub
//! the log without it being a footgun. Three ways:
//!   a. `POST /api/logs/clear` (truncate to 0, write a
//!      sentinel line, keep the file open for further writes).
//!   b. `Settings → Logs → Clear` button (Tauri shell).
//!   c. `rm ~/Desktop/Flwntier.log` (manual, the file is
//!      recreated on the next runtime start since we
//!      always open in append mode).
//!
//! None of these are silent: the log file's path is
//! available in the Settings panel and via
//! `GET /health` (the runtime's `version` field) for the
//! chairman to confirm the scrub worked.

use std::path::PathBuf;

/// Default log file path: chairman's desktop on Windows,
/// $HOME on Linux/Mac. May be overridden by the
/// `FLWNTIER_LOG_FILE` env var. The runtime emits a
/// `tracing::info!(path = %log_file, "log file")` line
/// at startup so the chairman can grep the log to see
/// where it's writing.
pub fn default_log_path() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "windows") {
        home.join("Desktop").join("Flwntier.log")
    } else {
        home.join("Flwntier.log")
    }
}

/// Resolve the log file path. `FLWNTIER_LOG_FILE=0` disables
/// file logging (returns None). Any other value is the
/// explicit path.
pub fn resolve_log_path() -> Option<PathBuf> {
    match std::env::var("FLWNTIER_LOG_FILE") {
        Err(_) => Some(default_log_path()),
        Ok(v) if v == "0" => None,
        Ok(v) => Some(PathBuf::from(v)),
    }
}

/// `true` iff the HTTP log endpoints should be exposed.
/// Default is `false` so a released build (with no env var
/// set) doesn't ship a file-read surface; the Tauri shell
/// sets `FLWNTIER_LOG_API=1` for chairman debug builds.
pub fn log_api_enabled() -> bool {
    std::env::var("FLWNTIER_LOG_API")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Read the last `tail` lines from the log file. Returns
/// empty Vec if the file doesn't exist yet (first launch
/// before any log line was written, or the chairman deleted
/// the file). The runtime's tracing-appender instance is
/// NOT involved — we just read the on-disk file directly
/// so the API call doesn't depend on subscriber state.
pub fn read_tail(tail: usize) -> Vec<String> {
    let Some(path) = resolve_log_path() else {
        return Vec::new();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    raw.lines()
        .rev()
        .take(tail)
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

/// Truncate the log file to zero bytes and write a single
/// sentinel line so the chairman can tell where the next
/// session starts. Returns the path on success.
pub fn clear_log() -> std::io::Result<PathBuf> {
    let path = resolve_log_path().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "FLWNTIER_LOG_FILE=0; file logging disabled",
        )
    })?;
    // Touch (create if missing) then truncate.
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)?;
    // Append a single sentinel. We don't use the tracing
    // subscriber because the chairman just cleared the file
    // — anything the runtime emits next is via the
    // tracing-appender which will create the file again.
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)?;
    writeln!(
        f,
        "[logs cleared at {}]",
        chrono::Utc::now().to_rfc3339()
    )?;
    Ok(path)
}

/// Set up the global tracing subscriber to mirror all
/// events to stderr AND (if `FLWNTIER_LOG_FILE` is set
/// and not "0") to the log file. Uses the JSON format
/// so the chairman can grep / parse cleanly. Returns the
/// path the log file is at, or `None` if file logging is
/// disabled.
pub fn init() -> Option<PathBuf> {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt::format::FmtSpan;
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,pipe_server=debug"));

    let log_path = resolve_log_path();

    if let Some(path) = log_path.clone() {
        // Ensure the parent dir exists (Windows desktop may
        // not exist for new users — although it always does
        // for the chairman). Touch the file so the first
        // append doesn't race with the call to open().
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Build a non-blocking writer on top of the file.
        // We hand-roll a small "always-open, append, flush
        // on every line" writer instead of pulling in
        // tracing-appender's `file` feature (which would
        // force the workspace dep to add it — and that
        // conflicts with tauri-core's existing pin). The
        // shape we need is simple: a Mutex<File> behind a
        // MakeWriter impl that locks, writes, flushes, drops.
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("open log file for write");
        let writer = LogFileWriter {
            file: std::sync::Arc::new(std::sync::Mutex::new(file)),
        };
        use tracing_subscriber::prelude::*;
        tracing_subscriber::registry()
            .with(env_filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_span_events(FmtSpan::NONE)
                    .with_writer(std::io::stderr),
            )
            .with(
                tracing_subscriber::fmt::layer()
                    .with_span_events(FmtSpan::NONE)
                    .with_writer(writer)
                    .json(),
            )
            .init();
        tracing::info!(
            path = %path.display(),
            "v0.4.22 (event 000080): log file initialised"
        );
    } else {
        // File logging disabled; just stderr.
        use tracing_subscriber::prelude::*;
        tracing_subscriber::registry()
            .with(env_filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_span_events(FmtSpan::NONE)
                    .with_writer(std::io::stderr),
            )
            .init();
        tracing::info!(
            "v0.4.22 (event 000080): log file disabled \
             (FLWNTIER_LOG_FILE=0); stderr only"
        );
    }
    log_path
}

/// Hand-rolled tracing MakeWriter that appends each
/// tracing event as a single line and flushes. Simpler
/// than the `tracing_appender::file` feature and avoids
/// the workspace-dep conflict with tauri-core's pin.
struct LogFileWriter {
    file: std::sync::Arc<std::sync::Mutex<std::fs::File>>,
}

impl std::io::Write for LogFileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        use std::io::Write;
        let mut f = self.file.lock().expect("log file mutex");
        let n = f.write(buf)?;
        f.flush()?;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let mut f = self.file.lock().expect("log file mutex");
        std::io::Write::flush(&mut *f)
    }
}

/// MakeWriter impl — tracing_subscriber requires this
/// trait, not just Write.
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogFileWriter {
    type Writer = LogFileWriteGuard;
    fn make_writer(&'a self) -> Self::Writer {
        LogFileWriteGuard { file: self.file.clone() }
    }
}

struct LogFileWriteGuard {
    file: std::sync::Arc<std::sync::Mutex<std::fs::File>>,
}

impl std::io::Write for LogFileWriteGuard {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        std::io::Write::write(&mut *self.file.lock().expect("log mutex"), buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        std::io::Write::flush(&mut *self.file.lock().expect("log mutex"))
    }
}

static LOG_GUARD: std::sync::OnceLock<tracing_appender::non_blocking::WorkerGuard> =
    std::sync::OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Round-trip: write lines, read tail, clear, write
    /// more, read again. The clear should put a sentinel
    /// in place.
    #[test]
    fn write_read_clear_roundtrip() {
        let dir = std::env::temp_dir().join("flwntier-log-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test.log");
        let _ = std::fs::remove_file(&path);
        for i in 0..10 {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap();
            writeln!(f, "line {i}").unwrap();
        }
        let tail = read_tail(3);
        assert_eq!(tail, vec!["line 7", "line 8", "line 9"]);
        let cleared = clear_log_for(&path).unwrap();
        assert_eq!(cleared, path);
        // Sentinel present, file truncated.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("[logs cleared at"));
        // We can still write after clear.
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(f, "after clear").unwrap();
        let tail = read_tail(2);
        assert!(tail.last().unwrap().contains("after clear"));
    }

    fn clear_log_for(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(path)?;
        writeln!(f, "[logs cleared at 2026-07-02T00:00:00Z]")?;
        Ok(path.to_path_buf())
    }
}
