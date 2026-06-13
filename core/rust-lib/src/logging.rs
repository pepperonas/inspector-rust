//! Persistent file logging + crash capture.
//!
//! Bundled macOS/Windows builds run without a terminal, so the previous
//! stderr-only `tracing` output vanished — making hangs and crashes
//! undiagnosable in the field. This module sets up a **daily-rolling log file**
//! in the data dir (`<data>/InspectorRust/logs/inspector-rust.log`) *in addition
//! to* stderr, and installs a **panic hook** that appends crashes to a dedicated
//! `crash.log` synchronously (so a crash trace survives even if the async file
//! writer hasn't flushed before the process dies).
//!
//! Log level defaults to `info`; override with `RUST_LOG` (e.g. `RUST_LOG=debug`).

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

/// `<platform data dir>/InspectorRust/logs` — the directory holding the log
/// files. `None` if the platform has no data dir.
pub fn log_dir() -> Option<PathBuf> {
    let mut d = dirs::data_dir()?;
    d.push("InspectorRust");
    d.push("logs");
    Some(d)
}

static CRASH_LOG: OnceLock<Option<PathBuf>> = OnceLock::new();

fn crash_log_path() -> Option<&'static PathBuf> {
    CRASH_LOG
        .get_or_init(|| log_dir().map(|d| d.join("crash.log")))
        .as_ref()
}

/// Initialise tracing: an `info`-default, `RUST_LOG`-overridable subscriber
/// writing to BOTH stderr and a daily-rolling file. Returns the non-blocking
/// writer's guard, which the caller MUST keep alive for the app's lifetime
/// (dropping it performs the final flush). Returns `None` (stderr only) if the
/// log directory couldn't be created.
#[must_use]
pub fn init() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::prelude::*;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let stderr_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr);

    // Resolve the crash-log path up front so the panic hook can use it even if
    // the panic happens before anything else touches it.
    let _ = crash_log_path();

    let dir = log_dir();
    if let Some(d) = &dir {
        let _ = std::fs::create_dir_all(d);
    }

    match dir.filter(|d| d.is_dir()) {
        Some(dir) => {
            let appender = tracing_appender::rolling::daily(&dir, "inspector-rust.log");
            let (non_blocking, guard) = tracing_appender::non_blocking(appender);
            let file_layer = tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(non_blocking);
            tracing_subscriber::registry()
                .with(filter)
                .with(stderr_layer)
                .with(file_layer)
                .init();
            Some(guard)
        }
        None => {
            tracing_subscriber::registry()
                .with(filter)
                .with(stderr_layer)
                .init();
            None
        }
    }
}

/// Install a panic hook that records the panic to `crash.log` (synchronously —
/// so it survives an immediate abort that the async log writer would miss) and
/// to `tracing`, then chains to the previous hook so the default behaviour
/// (backtrace print / abort) is preserved.
pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".into());
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".into());
        let thread = std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string();

        tracing::error!(target: "crash", %location, %thread, "PANIC: {msg}");

        // Synchronous append — the async (non-blocking) file writer may not
        // flush before the process aborts, so persist the crash directly too.
        if let Some(path) = crash_log_path() {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
                let _ = writeln!(f, "[{ts}] PANIC on '{thread}' at {location}: {msg}");
            }
        }

        prev(info);
    }));
}
