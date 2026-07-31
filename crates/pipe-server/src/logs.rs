//! v0.4.26 (event 000119): per-role log routing.
//!
//! Chairman's directive (event 000119): "日志详细写一下, 在桌面
//! 新建一个文件夹, 给所有角色的日志跟系统的日志分开写" — keep
//! per-role traces separate from system events, and put them in a
//! dedicated folder on the user's desktop so they're easy to find
//! (and easy to delete when the dev cycle is done).
//!
//! ## Layout
//!
//! On Windows, the default log directory is
//! `~/Desktop/Flowntier/logs/`. Inside that directory we emit
//! one file per role + one for the system + one merged
//! "everything" file:
//!
//! ```text
//! ~/Desktop/Flowntier/logs/
//!   chief.log       — events emitted while driving  `agent:chief`
//!                     (主理: requirement / plan / dispatch /
//!                     repair / delivery phases).
//!   critic-a.log    — events while driving `agent:critic:a`
//!                     (找茬 — bug / security review).
//!   critic-b.log    — events while driving `agent:critic:b`
//!                     (架构审查 — code quality review).
//!   worker.log      — events while driving `agent:worker` (all
//!                     N Phase-5 workers merged into one file so
//!                     the chairman can grep the multi-worker
//!                     stream in chronological order).
//!   system.log      — orchestrator phase transitions, dispatch
//!                     routing, HTTP server start/stop, quota
//!                     scheduler, panics.
//!   runtime.log     — every event regardless of target. Useful
//!                     as a single-file fallback / chronological
//!                     view of the whole session.
//! ```
//!
//! ## Routing
//!
//! tracing-subscriber lets us stack layers, each with its own
//! `Filter` + `MakeWriter`. We attach 7 layers:
//!   1. stderr          — every event (live console for the
//!                        `pnpm tauri:dev` process).
//!   2. chief.log       — `target == "chief"` (event 000119 — the
//!                        orchestrator's `drive_single_agent` now
//!                        emits role-specific targets so each
//!                        file gets only its author's events).
//!   3. critic-a.log    — `target == "critic-a"`.
//!   4. critic-b.log    — `target == "critic-b"`.
//!   5. worker.log      — `target == "worker"`.
//!   6. system.log      — `target in {pipe_server, dispatcher,
//!                        orchestrator, quota, pipe_server::scheduler,
//!                        flowntier_shell, tauri_ipc}`.
//!   7. runtime.log     — every event.
//!
//! Each file layer is paired with a small `NullWriter` that
//! drops bytes — we use that when an individual file open
//! fails (e.g. disk full mid-session) so the rest of the
//! routing stays alive.
//!
//! ## Backwards-compatible env vars
//!
//! - `FLWNTIER_LOG_DIR=<dir>` — override the per-role directory.
//! - `FLWNTIER_LOG_FILE=<path>` — legacy single-file mode. When
//!   set, we ignore `FLWNTIER_LOG_DIR` and write the whole
//!   stream to that one file (chairman's old grep scripts keep
//!   working).
//! - `FLWNTIER_LOG_FILE=0` — disable file logging entirely.
//! - `FLWNTIER_LOG_API=1` — expose the legacy HTTP-log endpoints
//!   (`GET /api/logs/get`, `POST /api/logs/clear`). Default off.

use std::path::{Path, PathBuf};

// Canonical targets used by the rest of the codebase. Keep
// in sync with the `target: "…"` strings emitted by the
// orchestrator / runtime binary.
pub const TARGET_CHIEF: &str = "chief";
pub const TARGET_CRITIC_A: &str = "critic-a";
pub const TARGET_CRITIC_B: &str = "critic-b";
pub const TARGET_WORKER: &str = "worker";

/// v0.4.26 (event 000119): per-role log helper.
///
/// `tracing::info!(target: $t, ...)` requires `$t` to be a
/// compile-time string literal because the macro expands the
/// target into a `static __CALLSITE`. Runtime `&'static str`
/// doesn't satisfy that. So we use a `match` to pick the
/// right literal at the call site.
///
/// Usage:
/// ```ignore
/// role_info!(orch, role_id, "message {x}", x = 1);
/// ```
///
/// Expands to the right `info!(target: "chief", ...)` etc.
#[macro_export]
macro_rules! role_info {
    ($self:expr, $role_id:expr, $($arg:tt)+) => {{
        let __role = $role_id.as_str();
        match __role {
            "agent:chief" => {
                tracing::info!(target: $crate::logs::TARGET_CHIEF, $($arg)+)
            }
            "agent:critic:a" => {
                tracing::info!(target: $crate::logs::TARGET_CRITIC_A, $($arg)+)
            }
            "agent:critic:b" => {
                tracing::info!(target: $crate::logs::TARGET_CRITIC_B, $($arg)+)
            }
            "agent:worker" => {
                tracing::info!(target: $crate::logs::TARGET_WORKER, $($arg)+)
            }
            // planner / reporter / unknown -> chief.log (they
            // run inside the chief's pipeline).
            _ => {
                tracing::info!(target: $crate::logs::TARGET_CHIEF, $($arg)+)
            }
        }
    }};
}

/// Like `role_info!` but for `warn!`.
#[macro_export]
macro_rules! role_warn {
    ($self:expr, $role_id:expr, $($arg:tt)+) => {{
        let __role = $role_id.as_str();
        match __role {
            "agent:chief" => {
                tracing::warn!(target: $crate::logs::TARGET_CHIEF, $($arg)+)
            }
            "agent:critic:a" => {
                tracing::warn!(target: $crate::logs::TARGET_CRITIC_A, $($arg)+)
            }
            "agent:critic:b" => {
                tracing::warn!(target: $crate::logs::TARGET_CRITIC_B, $($arg)+)
            }
            "agent:worker" => {
                tracing::warn!(target: $crate::logs::TARGET_WORKER, $($arg)+)
            }
            _ => {
                tracing::warn!(target: $crate::logs::TARGET_CHIEF, $($arg)+)
            }
        }
    }};
}

const SYSTEM_TARGETS: &[&str] = &[
    "pipe_server",
    "dispatcher",
    "orchestrator",
    "quota",
    "pipe_server::scheduler",
    "flowntier_shell",
    "tauri_ipc",
];

/// Default log directory (event 000119). Replaces the legacy
/// `default_log_path() == ~/Desktop/Flowntier.log`.
pub fn default_log_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "windows") {
        home.join("Desktop").join("Flowntier").join("logs")
    } else {
        home.join("Flowntier").join("logs")
    }
}

/// Resolve the log directory per the env vars. Returns None if
/// file logging is disabled (`FLWNTIER_LOG_FILE=0`).
pub fn resolve_log_dir() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("FLWNTIER_LOG_FILE") {
        if v == "0" {
            return None;
        }
    }
    if let Ok(v) = std::env::var("FLWNTIER_LOG_DIR") {
        if !v.is_empty() {
            return Some(PathBuf::from(v));
        }
    }
    Some(default_log_dir())
}

/// Legacy single-file path. Used when `FLWNTIER_LOG_FILE` is
/// set to a real path (not `0`).
pub fn legacy_log_file_path() -> Option<PathBuf> {
    match std::env::var("FLWNTIER_LOG_FILE") {
        Err(_) => None,
        Ok(v) if v == "0" => None,
        Ok(v) => Some(PathBuf::from(v)),
    }
}

/// v0.4.26 (event 000119): backwards-compat shim. The old
/// HTTP handler at `handlers.rs::get_log_tail` calls
/// `logs::resolve_log_path()` to get the single file path.
/// With per-role routing there is no single file, so we point
/// at `runtime.log` under the per-role directory (or the legacy
/// single-file path when `FLWNTIER_LOG_FILE` is set).
pub fn resolve_log_path() -> Option<PathBuf> {
    if let Some(p) = legacy_log_file_path() {
        return Some(p);
    }
    resolve_log_dir().map(|d| d.join("runtime.log"))
}

/// v0.4.26 (event 000119): backwards-compat shim. Legacy
/// callers that used to get `~/Desktop/Flowntier.log` now get
/// `~/Desktop/Flowntier/logs/runtime.log` so the same path
/// still holds the merged stream.
pub fn default_log_path() -> PathBuf {
    default_log_dir().join("runtime.log")
}

/// `true` iff the HTTP log endpoints should be exposed.
pub fn log_api_enabled() -> bool {
    std::env::var("FLWNTIER_LOG_API")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Initialize the per-role log file routing. Returns the
/// resolved log directory (or legacy file path) on success,
/// `None` if file logging is disabled.
pub fn init() -> Option<PathBuf> {
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::fmt::format::FmtSpan;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,pipe_server=debug,orchestrator=debug"));

    // Legacy single-file mode (FLWNTIER_LOG_FILE=<path>). Skip
    // the per-role routing.
    if let Some(legacy_path) = legacy_log_file_path() {
        if let Some(parent) = legacy_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let writer = FileWriter::open_or_null(&legacy_path);
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
            path = %legacy_path.display(),
            "v0.4.26 (event 000119): legacy single-file log mode"
        );
        return Some(legacy_path);
    }

    // Per-role directory mode (default).
    let dir = match resolve_log_dir() {
        Some(d) => d,
        None => return init_stderr_only(&env_filter),
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!(
            "[flowntier-runtime] could not create log dir {}: {}",
            dir.display(),
            e
        );
        return init_stderr_only(&env_filter);
    }

    let chief = FileWriter::open_or_null(&dir.join("chief.log"));
    let critic_a = FileWriter::open_or_null(&dir.join("critic-a.log"));
    let critic_b = FileWriter::open_or_null(&dir.join("critic-b.log"));
    let worker = FileWriter::open_or_null(&dir.join("worker.log"));
    let system = FileWriter::open_or_null(&dir.join("system.log"));
    let runtime = FileWriter::open_or_null(&dir.join("runtime.log"));

    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_span_events(FmtSpan::NONE)
        .with_writer(std::io::stderr);

    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        // 1. stderr — every event (live console).
        .with(stderr_layer)
        // 2/3/4/5. per-role targets.
        .with(role_layer(TARGET_CHIEF, chief))
        .with(role_layer(TARGET_CRITIC_A, critic_a))
        .with(role_layer(TARGET_CRITIC_B, critic_b))
        .with(role_layer(TARGET_WORKER, worker))
        // 6. system targets.
        .with(targets_layer(SYSTEM_TARGETS, system))
        // 7. catch-all chronological mirror.
        .with(catch_all_layer(runtime));

    if let Err(e) = subscriber.try_init() {
        eprintln!("[flowntier-runtime] tracing subscriber init failed: {e}");
        // Don't propagate — we already initialised stderr in
        // the layer above, so the runtime still has live
        // logging even if the per-role files fail.
    }

    tracing::info!(
        dir = %dir.display(),
        "v0.4.26 (event 000119): per-role log directory initialised"
    );
    Some(dir)
}

/// Build a JSON layer for one target. Generic over the
/// subscriber `S` so the returned value can be `with()`'d
/// onto any in-progress subscriber (Registry, Layered, ...).
fn role_layer<S>(
    target: &'static str,
    writer: FileWriter,
) -> impl tracing_subscriber::layer::Layer<S>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    use tracing_subscriber::filter::FilterFn;
    use tracing_subscriber::layer::Layer as _;
    let predicate = FilterFn::new(move |m| m.target() == target);
    tracing_subscriber::fmt::layer()
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::NONE)
        .with_writer(writer)
        .json()
        .with_filter(predicate)
}

/// Multi-target filter (e.g. system.log).
fn targets_layer<S>(
    targets: &'static [&'static str],
    writer: FileWriter,
) -> impl tracing_subscriber::layer::Layer<S>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    use tracing_subscriber::filter::FilterFn;
    use tracing_subscriber::layer::Layer as _;
    let predicate = FilterFn::new(move |m| targets.contains(&m.target()));
    tracing_subscriber::fmt::layer()
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::NONE)
        .with_writer(writer)
        .json()
        .with_filter(predicate)
}

/// Catch-all (no filter).
fn catch_all_layer<S>(
    writer: FileWriter,
) -> impl tracing_subscriber::layer::Layer<S>
where
    S: tracing::Subscriber + for<'span> tracing_subscriber::registry::LookupSpan<'span>,
{
    use tracing_subscriber::layer::Layer;
    tracing_subscriber::fmt::layer()
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::NONE)
        .with_writer(writer)
        .json()
}

/// Stderr-only fallback (used when the file open fails or
/// `FLWNTIER_LOG_FILE=0` is set).
pub fn init_stderr_only(env_filter: &tracing_subscriber::EnvFilter) -> Option<PathBuf> {
    use tracing_subscriber::fmt::format::FmtSpan;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    tracing_subscriber::registry()
        .with(env_filter.clone())
        .with(
            tracing_subscriber::fmt::layer()
                .with_span_events(FmtSpan::NONE)
                .with_writer(std::io::stderr),
        )
        .init();
    tracing::info!("v0.4.26 (event 000119): stderr-only logging initialised");
    None
}

/// Install a panic hook that writes the panic to chief.log
/// (the runtime panics usually happen while the chief is
/// running). event 000109: same shape as the legacy hook but
/// points at the role directory, not the single file.
pub fn install_panic_hook(log_dir: &Path) {
    let dir = log_dir.to_path_buf();
    let target = dir.join("chief.log");
    std::panic::set_hook(Box::new(move |info| {
        let bt = std::backtrace::Backtrace::force_capture();
        eprintln!("[flowntier-runtime] PANIC: {info}\n{bt}");
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&target)
        {
            let _ = writeln!(
                f,
                "[flowntier-runtime] PANIC at {}: {info}\n{bt}",
                chrono::Utc::now().to_rfc3339()
            );
        }
    }));
}

// ── writers ────────────────────────────────────────────────

/// Mutex-protected file writer. Always returns a writer; if
/// the file open fails, falls back to a `NullWriter` so the
/// layer still works (it just doesn't write anywhere).
enum FileWriter {
    Real(RealFileWriter),
    Null,
}

struct RealFileWriter {
    file: std::sync::Arc<std::sync::Mutex<std::fs::File>>,
}

impl FileWriter {
    fn open_or_null(path: &Path) -> Self {
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            Ok(f) => FileWriter::Real(RealFileWriter {
                file: std::sync::Arc::new(std::sync::Mutex::new(f)),
            }),
            Err(e) => {
                if cfg!(debug_assertions) {
                    eprintln!(
                        "[flowntier-runtime] could not open log file {}: {}",
                        path.display(),
                        e
                    );
                }
                FileWriter::Null
            }
        }
    }
}

impl std::io::Write for FileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            FileWriter::Null => Ok(buf.len()),
            FileWriter::Real(r) => r.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            FileWriter::Null => Ok(()),
            FileWriter::Real(r) => r.flush(),
        }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for FileWriter {
    type Writer = FileWriterHandle;
    fn make_writer(&'a self) -> Self::Writer {
        match self {
            FileWriter::Null => FileWriterHandle::Null,
            FileWriter::Real(r) => FileWriterHandle::Real(RealFileWriterHandle {
                file: r.file.clone(),
            }),
        }
    }
}

impl std::io::Write for RealFileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        std::io::Write::write(&mut *self.file.lock().expect("log file mutex"), buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        std::io::Write::flush(&mut *self.file.lock().expect("log file mutex"))
    }
}

enum FileWriterHandle {
    Real(RealFileWriterHandle),
    Null,
}

struct RealFileWriterHandle {
    file: std::sync::Arc<std::sync::Mutex<std::fs::File>>,
}

impl std::io::Write for FileWriterHandle {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            FileWriterHandle::Null => Ok(buf.len()),
            FileWriterHandle::Real(r) => r.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            FileWriterHandle::Null => Ok(()),
            FileWriterHandle::Real(r) => r.flush(),
        }
    }
}

impl std::io::Write for RealFileWriterHandle {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        std::io::Write::write(&mut *self.file.lock().expect("log mutex"), buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        std::io::Write::flush(&mut *self.file.lock().expect("log mutex"))
    }
}

// ── legacy read_tail / clear_log (operate on runtime.log) ────

/// Read the last `tail` lines from the merged `runtime.log`.
/// In the per-role regime, runtime.log is the union stream.
pub fn read_tail(tail: usize, path_override: Option<&Path>) -> Vec<String> {
    let path = match path_override {
        Some(p) => p.to_path_buf(),
        None => match resolve_log_dir() {
            Some(d) => d.join("runtime.log"),
            None => return Vec::new(),
        },
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

/// Truncate runtime.log to zero bytes and write a sentinel
/// line. Per-role files are left untouched (the chairman can
/// clear them individually by `rm` or with a future "clear
/// per-role" endpoint).
pub fn clear_log(path_override: Option<&Path>) -> std::io::Result<PathBuf> {
    let path = match path_override {
        Some(p) => p.to_path_buf(),
        None => match resolve_log_dir() {
            Some(d) => d.join("runtime.log"),
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "FLWNTIER_LOG_FILE=0; file logging disabled",
                ))
            }
        },
    };
    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_log_dir_is_under_desktop_flowntier_logs() {
        let d = default_log_dir();
        assert!(d.ends_with("logs"), "got {d:?}");
        if cfg!(target_os = "windows") {
            assert!(d.to_string_lossy().contains("Desktop"));
        }
    }

    #[test]
    fn resolve_log_dir_respects_env_var() {
        let prev = std::env::var("FLWNTIER_LOG_DIR").ok();
        std::env::set_var("FLWNTIER_LOG_DIR", "/tmp/flowntier-test-logs");
        let d = resolve_log_dir().unwrap();
        assert_eq!(d, PathBuf::from("/tmp/flowntier-test-logs"));
        match prev {
            Some(v) => std::env::set_var("FLWNTIER_LOG_DIR", v),
            None => std::env::remove_var("FLWNTIER_LOG_DIR"),
        }
    }

    #[test]
    fn write_read_clear_roundtrip() {
        let dir = std::env::temp_dir().join("flwntier-log-test-v0426");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("runtime.log");
        let _ = std::fs::remove_file(&path);
        for i in 0..10 {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap();
            use std::io::Write;
            writeln!(f, "line {i}").unwrap();
        }
        let tail = read_tail(3, Some(&path));
        assert_eq!(tail, vec!["line 7", "line 8", "line 9"]);
        let cleared = clear_log(Some(&path)).unwrap();
        assert_eq!(cleared, path);
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("[logs cleared at"));
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        use std::io::Write;
        writeln!(f, "after clear").unwrap();
        let tail = read_tail(2, Some(&path));
        assert!(tail.last().unwrap().contains("after clear"));
    }
}