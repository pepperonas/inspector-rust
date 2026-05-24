//! Run an AppleScript with a watchdog timeout.
//!
//! Two of our pre-hotkey paths (`frontmost_app::name` +
//! `finder_selection::read`) shell out to `osascript` synchronously
//! on the main thread. Without a timeout, a hung target app (a frozen
//! Finder, a stuck System Events daemon) wedges the hotkey handler
//! indefinitely — the popup never opens, the user thinks the app
//! crashed. A 2-second cap is plenty for any real osascript call
//! (median is <30 ms) and lets us bail cleanly with a useful log
//! line instead.
//!
//! Implementation: `Command::spawn()` + poll `try_wait()` every
//! 20 ms. On timeout, `Child::kill()` (SIGKILL) the osascript
//! process. No external dependencies; macOS-only by intent (the
//! callers are macOS-only too).

#![cfg(target_os = "macos")]

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Result of [`run_osascript`].
pub enum OsaResult {
    /// osascript finished within the timeout (regardless of exit code).
    Done(Output),
    /// The process didn't finish in time; we sent it `SIGKILL`.
    /// The caller treats this the same as a generic failure (return
    /// `None` for frontmost-app, return Err for finder-selection).
    TimedOut,
    /// Spawn itself failed (osascript binary missing, fork OOM, …).
    SpawnFailed(std::io::Error),
}

/// Run `/usr/bin/osascript -e <script>` with a hard upper bound on
/// wall-clock time. Captures stdout + stderr like `Command::output()`.
pub fn run_osascript(script: &str, timeout: Duration) -> OsaResult {
    let spawn_result = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match spawn_result {
        Ok(c) => c,
        Err(e) => return OsaResult::SpawnFailed(e),
    };

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = s.read_to_end(&mut stdout);
                }
                if let Some(mut s) = child.stderr.take() {
                    let _ = s.read_to_end(&mut stderr);
                }
                return OsaResult::Done(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    // Watchdog: kill the hung process. `kill()` sends
                    // SIGKILL; on macOS osascript handles this cleanly.
                    // `wait()` reaps the zombie so it doesn't pile up.
                    let _ = child.kill();
                    let _ = child.wait();
                    tracing::warn!(
                        "osascript timed out after {timeout:?}: script {:?} (first 80 chars)",
                        &script.chars().take(80).collect::<String>()
                    );
                    return OsaResult::TimedOut;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                // try_wait shouldn't ordinarily fail; if it does, treat
                // it like a spawn error.
                let _ = child.kill();
                return OsaResult::SpawnFailed(e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_script_returns_done() {
        // `true` is a no-op AppleScript that returns immediately.
        let r = run_osascript("return 42", Duration::from_secs(2));
        match r {
            OsaResult::Done(out) => {
                assert!(out.status.success());
                let s = String::from_utf8_lossy(&out.stdout);
                assert!(s.contains("42"));
            }
            OsaResult::TimedOut => panic!("a 'return 42' script should not time out"),
            OsaResult::SpawnFailed(e) => panic!("spawn failed: {e}"),
        }
    }

    #[test]
    fn slow_script_times_out_and_is_killed() {
        // `delay 5` blocks osascript for 5 s; we give it 250 ms.
        // The watchdog should fire and reap it.
        let start = Instant::now();
        let r = run_osascript("delay 5", Duration::from_millis(250));
        let elapsed = start.elapsed();
        assert!(matches!(r, OsaResult::TimedOut));
        // Returned in roughly the timeout window — generous upper
        // bound so a busy CI doesn't false-positive.
        assert!(
            elapsed < Duration::from_secs(2),
            "watchdog should have killed osascript within ~250 ms but took {elapsed:?}"
        );
    }
}
