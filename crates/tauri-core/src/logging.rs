//! Persistent logging for the Flowntier desktop shell.
//!
//! Writes to a daily rolling file under `<data_dir>/logs/flowntier.log.YYYY-MM-DD`
//! AND to stderr (when stderr is a TTY, i.e. `cargo tauri dev`). The file
//! is what we ship to users — stderr is invisible in release builds
//! because `windows_subsystem = "windows"` strips the console.
//!
//! All log levels are filtered via the `FLOWNTIER_LOG` env var
//! (or `RUST_LOG` if you want the standard Rust filter syntax).
//! Default: `info`.
//!
//! Also installs a `std::panic::set_hook` so any panic in the Rust
//! shell (or in any Rust crate) is written to the same file before
//! the process dies. Without this, panics are silent in release
//! builds — the user sees the Tauri splash disappear and nothing
//! else, which is exactly the failure mode that hurt Phase 0 testing.
//!
//! The hook is process-global. Call [`init_logging`] exactly once at
//! startup (from the Tauri shell's `setup`); calling it twice will
//! double every log line.

// No `unsafe` here — `lib.rs` has `#![forbid(unsafe_code)]` and
// std::io::IsTerminal (stable since 1.70) is the safe stdlib
// isatty. We don't need libc or winapi FFI for this.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// RAII guard that keeps the logging worker thread alive for the
/// lifetime of the process. Drop it at shutdown (or just let it
/// drop naturally when `main` exits) to flush pending log lines.
pub struct LoggingGuard {
    /// Held for its `Drop` impl which flushes + joins the
    /// background writer thread.
    _file_guard: WorkerGuard,
    /// Held for symmetry / future use (we don't currently pipe
    /// stderr to a separate file but the wiring is in place).
    _stderr_guard: WorkerGuard,
}

/// Resolve the log directory for the given data dir. Creates the
/// directory if it doesn't exist.
pub fn log_dir(data_dir: &Path) -> PathBuf {
    let dir = data_dir.join("logs");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Initialise logging.
///
/// Writes to:
///   * `<data_dir>/logs/flowntier.log.YYYY-MM-DD`  (rolling daily)
///   * stderr (only if it's a TTY — i.e. dev mode)
///
/// Returns a guard that MUST be held for the lifetime of the
/// process. The guard's Drop impl flushes pending log lines and
/// joins the background writer thread.
pub fn init_logging(data_dir: &Path) -> LoggingGuard {
    let dir = log_dir(data_dir);

    // Daily rolling file appender. tracing-appender rotates at
    // midnight UTC by default; that's good enough for v0.4.
    // Future improvement: rotate by size or by local TZ.
    let file_appender = tracing_appender::rolling::daily(&dir, "flowntier.log");
    let (file_writer, file_guard) = tracing_appender::non_blocking(file_appender);

    // stderr writer — only created if stderr is a TTY (release
    // builds strip the console window, so this is effectively
    // dev-only).
    let stderr_writer: Box<dyn std::io::Write + Send + 'static> = if atty_stderr() {
        Box::new(std::io::stderr())
    } else {
        Box::new(std::io::sink())
    };
    let (stderr_writer, stderr_guard) = tracing_appender::non_blocking(stderr_writer);

    // EnvFilter: respect FLOWNTIER_LOG first, then RUST_LOG, then
    // default to 'info'.
    let filter = EnvFilter::try_from_env("FLOWNTIER_LOG")
        .or_else(|_| EnvFilter::try_from_env("RUST_LOG"))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let file_layer = fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false) // files don't want ANSI escape codes
        .with_target(true)
        .with_thread_ids(false)
        .json(); // structured logs are easier to grep / ship to Sentry later

    let stderr_layer = fmt::layer()
        .with_writer(stderr_writer)
        .with_ansi(true)
        .with_target(true)
        .compact();

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer)
        .try_init();

    install_panic_hook(&dir);

    tracing::info!(
        data_dir = %data_dir.display(),
        log_dir = %dir.display(),
        "Flowntier logging initialised"
    );

    LoggingGuard {
        _file_guard: file_guard,
        _stderr_guard: stderr_guard,
    }
}

/// Install a panic hook that writes the panic info to a
/// `panic-<timestamp>.log` file in the log directory, in addition
/// to the default stderr output.
///
/// We deliberately do NOT abort on panic (the default behaviour on
/// release is to unwind and exit; on dev it's the same). The hook
/// just records the panic for post-mortem.
fn install_panic_hook(log_dir: &Path) {
    let dir = log_dir.to_path_buf();
    std::panic::set_hook(Box::new(move |info| {
        // Default hook prints to stderr (visible in dev).
        eprintln!("\n[panic] {info}\n");

        // Best-effort file write. Don't panic inside a panic hook.
        let now = chrono::Utc::now();
        let path = dir.join(format!("panic-{}.log", now.format("%Y%m%d-%H%M%S")));
        let body = format!(
            "Flowntier panic\nTimestamp: {}\nLocation: {}\nPayload: {}\nBacktrace:\n{:?}\n",
            now.to_rfc3339(),
            info.location()
                .map(|l| l.to_string())
                .unwrap_or_else(|| "(unknown)".into()),
            info.payload()
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| {
                    info.payload()
                        .downcast_ref::<String>()
                        .cloned()
                })
                .unwrap_or_else(|| "(non-string panic payload)".into()),
            std::backtrace::Backtrace::force_capture(),
        );
        let _ = std::fs::write(&path, body);

        // Also write to the tracing file so users looking at
        // flowntier.log.<date> see the panic alongside normal logs.
        tracing::error!(
            panic_file = %path.display(),
            "Rust panic: {info}"
        );
    }));
}

/// Returns true if stderr is attached to a TTY. On Windows release
/// builds `stderr` is disconnected (no console window), so this
/// returns false and we skip the stderr layer entirely.
fn atty_stderr() -> bool {
    std::io::stderr().is_terminal()
}