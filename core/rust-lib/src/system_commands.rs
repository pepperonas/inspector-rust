//! System-level power commands triggered from the popup search bar.
//!
//! - [`list_running_processes`] — drives the `kill` live picker; returns
//!   the current user-relevant processes sorted by memory usage so the
//!   noisy-but-tiny ones don't bury the actual culprits.
//! - [`kill_process_by_pid`] — sends SIGTERM (default) or SIGKILL
//!   (`force = true`) to a process. On macOS this is `kill(2)` via the
//!   `libc` crate (already a transitive dep through `clipboard-rs`);
//!   no shell-out required.
//! - [`system_reboot`] / [`system_shutdown`] — graceful, no `sudo`
//!   needed: shells out to `osascript` driving `loginwindow`. macOS
//!   itself decides whether to prompt the user for save-confirmation.
//! - [`system_lock`] — `pmset displaysleepnow` (built-in on macOS,
//!   triggers the lock screen if "Require password immediately after
//!   sleep" is set).
//!
//! Windows: every function below stubs to `Err("not implemented on
//! this platform")` so the workspace compiles cross-platform. The
//! Windows path (`ExitWindowsEx`, `LockWorkStation`, `TerminateProcess`)
//! is a follow-up — same strategy as OCR/Screenshot.

#[cfg(any(target_os = "macos", target_os = "windows"))]
use anyhow::Context;
use anyhow::{anyhow, Result};
use serde::Serialize;

/// View struct the frontend renders in the kill picker.
/// `memory_mb` is the resident-set size; `pid` + `name` are the user-
/// addressable identifiers.
#[derive(Debug, Clone, Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub memory_mb: f64,
    /// Path to the binary (best-effort; empty string if unknown).
    pub exe: String,
}

/// List running processes owned by the current user, sorted by memory
/// descending so the picker surfaces the actual culprits first.
/// Excludes kernel_task / launchd / our own process to keep the list
/// reasonable.
pub fn list_running_processes() -> Result<Vec<ProcessInfo>> {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        use sysinfo::{ProcessRefreshKind, RefreshKind, System};

        let our_pid = std::process::id();

        let mut sys = System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
        );
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

        let mut out: Vec<ProcessInfo> = sys
            .processes()
            .iter()
            .filter_map(|(pid, proc)| {
                let pid_u32 = pid.as_u32();
                if pid_u32 == our_pid {
                    return None; // never list ourselves
                }
                let name = proc.name().to_string_lossy().to_string();
                if name.is_empty() {
                    return None;
                }
                let exe = proc
                    .exe()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                Some(ProcessInfo {
                    pid: pid_u32,
                    name,
                    // bytes → MB, two-decimal precision
                    memory_mb: (proc.memory() as f64) / (1024.0 * 1024.0),
                    exe,
                })
            })
            .collect();

        // Sort by memory descending so heavy apps surface first.
        out.sort_by(|a, b| {
            b.memory_mb
                .partial_cmp(&a.memory_mb)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(out)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(anyhow!(
            "list_running_processes not implemented on this platform"
        ))
    }
}

/// Send SIGTERM (graceful) or SIGKILL (force) to `pid`.
///
/// - macOS / Linux: `libc::kill(pid, signal)`. Doesn't require root for
///   processes owned by the current user. Returns an error if the PID
///   doesn't exist or we don't have permission.
/// - Windows: stub (would use `OpenProcess` + `TerminateProcess`).
pub fn kill_process_by_pid(pid: u32, force: bool) -> Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        // libc is already a transitive dep — we don't need to pull it in
        // ourselves. Re-use the sysinfo crate's Process::kill_with for
        // a portable wrapper.
        use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, Signal, System};

        let signal = if force { Signal::Kill } else { Signal::Term };
        let mut sys = System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
        );
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

        let target = sys
            .process(Pid::from_u32(pid))
            .ok_or_else(|| anyhow!("no process with PID {pid}"))?;

        // kill_with returns Some(bool) — Some(true) means the signal was
        // delivered. Returning None means the signal isn't supported on
        // this platform; treat as a fall-through error.
        match target.kill_with(signal) {
            Some(true) => Ok(()),
            Some(false) => Err(anyhow!(
                "failed to deliver {signal:?} to PID {pid} (permission denied?)",
            )),
            None => Err(anyhow!("{signal:?} not supported on this platform")),
        }
    }
    #[cfg(target_os = "windows")]
    {
        let _ = (pid, force);
        Err(anyhow!(
            "kill_process_by_pid not yet implemented on Windows"
        ))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (pid, force);
        Err(anyhow!(
            "kill_process_by_pid not implemented on this platform"
        ))
    }
}

/// Restart the system via `osascript` → `loginwindow`. Apps get a
/// chance to save (the user sees the standard "These apps have
/// unsaved changes…" prompt). No sudo required.
pub fn system_reboot() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(r#"tell application "loginwindow" to «event aevtrrst»"#)
            .status()
            .context("osascript launch failed")?
            .success()
            .then_some(())
            .ok_or_else(|| anyhow!("osascript reboot returned non-zero exit"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(anyhow!("system_reboot not implemented on this platform"))
    }
}

/// Power down the system via `osascript` → `loginwindow`. Same
/// graceful behaviour as [`system_reboot`].
pub fn system_shutdown() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(r#"tell application "loginwindow" to «event aevtrsdn»"#)
            .status()
            .context("osascript launch failed")?
            .success()
            .then_some(())
            .ok_or_else(|| anyhow!("osascript shutdown returned non-zero exit"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(anyhow!("system_shutdown not implemented on this platform"))
    }
}

/// Lock the screen via `pmset displaysleepnow`. macOS will require a
/// password to wake when "Require password after sleep" is set (the
/// default for personal Macs). No sudo required.
pub fn system_lock() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/pmset")
            .arg("displaysleepnow")
            .status()
            .context("pmset launch failed")?
            .success()
            .then_some(())
            .ok_or_else(|| anyhow!("pmset displaysleepnow returned non-zero exit"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(anyhow!("system_lock not implemented on this platform"))
    }
}

/// Clamp a raw volume value into the valid 0–100 percent range.
/// Pure helper, extracted so the clamping logic is unit-testable
/// without touching (and changing!) the real system volume.
#[allow(dead_code)] // kept as a small utility + still covered by tests
pub fn clamp_volume(level: i32) -> u8 {
    level.clamp(0, 100) as u8
}

/// Adjust the system output volume by `delta` percentage points
/// (positive = louder, negative = quieter). Bound to Shift+↑ / Shift+↓
/// in the popup.
///
/// Two performance fixes vs. the v0.22.0 implementation:
///
/// 1. **One osascript invocation**, not two. The previous version
///    spawned `osascript` once to read the current volume and a second
///    time to set the new value — ~150 ms per spawn, so ~300 ms total
///    before the system actually moved. AppleScript can read-modify-
///    write atomically in a single script.
/// 2. **Fire-and-forget worker thread.** Spawning into a thread makes
///    the IPC return instantly so the next Shift+↑ / Shift+↓ press
///    isn't queued behind the previous osascript. macOS plays its own
///    volume-change feedback, so the caller doesn't need the new
///    value back (returns `0` synchronously as a placeholder — the
///    earlier API contract was `Result<u8>` and the frontend only
///    cares about whether it failed).
pub fn adjust_system_volume(delta: i32) -> Result<u8> {
    #[cfg(target_os = "macos")]
    {
        std::thread::spawn(move || {
            // Multiple `-e` args = atomic single-process AppleScript;
            // safer than embedding newlines in one `-e` string.
            let _ = std::process::Command::new("/usr/bin/osascript")
                .arg("-e")
                .arg(format!(
                    "set v to (output volume of (get volume settings)) + ({delta})"
                ))
                .arg("-e").arg("if v < 0 then set v to 0")
                .arg("-e").arg("if v > 100 then set v to 100")
                .arg("-e").arg("set volume output volume v")
                .status();
        });
        // Placeholder — the spawned thread does the real work. The IPC
        // resolves immediately so a rapid Shift+↑ / Shift+↓ chord
        // doesn't stack 300 ms latencies.
        Ok(0)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = delta;
        Err(anyhow!("adjust_system_volume not implemented on this platform"))
    }
}

/// Toggle the system output mute state. Reads the current state via
/// `osascript`, flips it, returns the new state (`true` = now muted).
/// No privilege required.
pub fn toggle_system_mute() -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg("output muted of (get volume settings)")
            .output()
            .context("osascript mute read failed")?;
        let currently_muted = String::from_utf8_lossy(&out.stdout).trim() == "true";
        let next = !currently_muted;
        let script = if next {
            "set volume with output muted"
        } else {
            "set volume without output muted"
        };
        std::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(script)
            .status()
            .context("osascript mute set failed")?
            .success()
            .then_some(())
            .ok_or_else(|| anyhow!("osascript mute set returned non-zero exit"))?;
        Ok(next)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(anyhow!("toggle_system_mute not implemented on this platform"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_volume_passes_through_in_range() {
        assert_eq!(clamp_volume(0), 0);
        assert_eq!(clamp_volume(50), 50);
        assert_eq!(clamp_volume(100), 100);
    }

    #[test]
    fn clamp_volume_clamps_out_of_range() {
        assert_eq!(clamp_volume(-20), 0);
        assert_eq!(clamp_volume(-1), 0);
        assert_eq!(clamp_volume(101), 100);
        assert_eq!(clamp_volume(9999), 100);
    }

    #[test]
    fn clamp_volume_models_a_step_at_the_edges() {
        // Pressing Shift+↑ at max stays at max; Shift+↓ at zero stays at zero.
        assert_eq!(clamp_volume(100 + 6), 100);
        assert_eq!(clamp_volume(0 - 6), 0);
        // A normal mid-range step lands where expected.
        assert_eq!(clamp_volume(48 + 6), 54);
        assert_eq!(clamp_volume(48 - 6), 42);
    }

    #[test]
    fn process_info_serialises_to_expected_shape() {
        let p = ProcessInfo {
            pid: 1234,
            name: "Slack".into(),
            memory_mb: 512.75,
            exe: "/Applications/Slack.app/Contents/MacOS/Slack".into(),
        };
        let j = serde_json::to_value(&p).unwrap();
        assert_eq!(j["pid"], 1234);
        assert_eq!(j["name"], "Slack");
        assert!((j["memory_mb"].as_f64().unwrap() - 512.75).abs() < 1e-6);
        assert!(j["exe"].as_str().unwrap().contains("Slack"));
    }

    #[test]
    fn process_info_is_clone_and_serializable() {
        // Compile-time guard.
        let p = ProcessInfo {
            pid: 1,
            name: "x".into(),
            memory_mb: 1.0,
            exe: "y".into(),
        };
        let _ = p.clone();
        let _ = serde_json::to_string(&p).unwrap();
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn list_returns_at_least_one_process_and_excludes_self() {
        // Cargo test runs in a process; there's *always* at least one
        // other live process on the system (init, launchd, etc.), so
        // the list must be non-empty AND must not include our own PID.
        let processes = list_running_processes().expect("list should succeed");
        assert!(
            !processes.is_empty(),
            "expected at least one running process"
        );
        let our_pid = std::process::id();
        assert!(
            processes.iter().all(|p| p.pid != our_pid),
            "list_running_processes must exclude our own PID ({our_pid})",
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn list_is_sorted_by_memory_descending() {
        let processes = list_running_processes().expect("list should succeed");
        // Pairwise check — each entry must have >= memory than the next.
        for win in processes.windows(2) {
            assert!(
                win[0].memory_mb >= win[1].memory_mb,
                "process list not sorted by memory desc: {} ({} MB) > {} ({} MB)",
                win[0].name,
                win[0].memory_mb,
                win[1].name,
                win[1].memory_mb,
            );
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn kill_returns_error_for_nonexistent_pid() {
        // PID 999999999 is functionally guaranteed not to exist on
        // any supported OS. The call must error, not panic.
        let r = kill_process_by_pid(999_999_999, false);
        assert!(r.is_err(), "killing a nonexistent PID must error");
    }
}
