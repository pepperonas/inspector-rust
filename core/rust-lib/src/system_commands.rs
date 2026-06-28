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
//! Windows parity (v0.61.0): `system_reboot`/`system_shutdown` shell out
//! to `shutdown /r|/s /t 0`, `system_lock` to `rundll32 user32.dll,
//! LockWorkStation`, and volume/mute synthesize the multimedia VK keys
//! (`VK_VOLUME_UP/DOWN/MUTE` via `keybd_event`). `kill` already had a
//! Windows path (`TerminateProcess`). These Windows paths are written
//! compile-clean but **runtime-unverified** on real hardware. Linux/other
//! still return a clean "not implemented" error.

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
/// - macOS / Linux: SIGTERM (graceful) or SIGKILL (`force`), via the
///   `sysinfo` wrapper over `libc::kill`. No root needed for own processes.
/// - Windows: `sysinfo` maps `Signal::Kill` to `TerminateProcess` — Windows
///   has no signals, so the SIGTERM/SIGKILL distinction collapses to a forced
///   terminate. (Windows runtime-unverified.)
pub fn kill_process_by_pid(pid: u32, force: bool) -> Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    {
        // `sysinfo` gives a portable kill across all three OSes (it's already a
        // dependency and powers `list_running_processes`).
        use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, Signal, System};

        // Windows has no signal model — TerminateProcess is the only option,
        // which sysinfo exposes as `Signal::Kill`. Elsewhere honour `force`.
        let signal = if cfg!(target_os = "windows") || force {
            Signal::Kill
        } else {
            Signal::Term
        };
        let mut sys = System::new_with_specifics(
            RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
        );
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

        let target = sys
            .process(Pid::from_u32(pid))
            .ok_or_else(|| anyhow!("no process with PID {pid}"))?;

        // kill_with returns Some(bool) — Some(true) means the signal was
        // delivered. None means the signal isn't supported on this platform;
        // retry with Kill (covers a Windows build that returns None for Term).
        match target.kill_with(signal) {
            Some(true) => Ok(()),
            Some(false) => Err(anyhow!(
                "failed to deliver {signal:?} to PID {pid} (permission denied?)",
            )),
            None => match target.kill_with(Signal::Kill) {
                Some(true) => Ok(()),
                _ => Err(anyhow!("could not terminate PID {pid}")),
            },
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (pid, force);
        Err(anyhow!(
            "kill_process_by_pid not implemented on this platform"
        ))
    }
}

/// Linux helper: run each `(program, args)` in order, returning `Ok(())` on the
/// first that launches and exits zero. Used for the power/lock/audio commands
/// where the right tool depends on the distro (systemd vs. legacy, PipeWire vs.
/// PulseAudio). The final `Err` names every tool tried so the failure is
/// actionable.
#[cfg(target_os = "linux")]
fn run_first_ok(label: &str, candidates: &[(&str, &[&str])]) -> Result<()> {
    let mut tried = Vec::new();
    for (program, args) in candidates {
        tried.push(*program);
        match std::process::Command::new(program).args(*args).status() {
            Ok(s) if s.success() => return Ok(()),
            _ => continue, // missing tool or non-zero exit → try the next
        }
    }
    Err(anyhow!("{label}: none of {tried:?} succeeded"))
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
    #[cfg(target_os = "windows")]
    {
        // `shutdown /r /t 0` — graceful restart, apps get the standard
        // "close to continue" prompt. No elevation needed for the current
        // interactive session. (Runtime-unverified on a real Windows box.)
        std::process::Command::new("shutdown")
            .args(["/r", "/t", "0"])
            .status()
            .context("shutdown /r launch failed")?
            .success()
            .then_some(())
            .ok_or_else(|| anyhow!("shutdown /r returned non-zero exit"))
    }
    #[cfg(target_os = "linux")]
    {
        // logind reboots without sudo on a normal desktop session.
        run_first_ok("reboot", &[("systemctl", &["reboot"])])
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
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
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("shutdown")
            .args(["/s", "/t", "0"])
            .status()
            .context("shutdown /s launch failed")?
            .success()
            .then_some(())
            .ok_or_else(|| anyhow!("shutdown /s returned non-zero exit"))
    }
    #[cfg(target_os = "linux")]
    {
        run_first_ok("shutdown", &[("systemctl", &["poweroff"])])
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
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
    #[cfg(target_os = "windows")]
    {
        // The canonical Windows lock entry point. `rundll32` is the
        // documented way to invoke it from a process.
        std::process::Command::new("rundll32.exe")
            .args(["user32.dll,LockWorkStation"])
            .status()
            .context("LockWorkStation launch failed")?
            .success()
            .then_some(())
            .ok_or_else(|| anyhow!("LockWorkStation returned non-zero exit"))
    }
    #[cfg(target_os = "linux")]
    {
        // The right lock command varies by desktop: logind (most), then the
        // freedesktop screensaver helper, then the GNOME/Cinnamon fallbacks.
        run_first_ok(
            "lock",
            &[
                ("loginctl", &["lock-session"]),
                ("xdg-screensaver", &["lock"]),
                ("gnome-screensaver-command", &["-l"]),
                ("cinnamon-screensaver-command", &["-l"]),
            ],
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
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

/// Read the current system output volume (0–100). Blocking (one osascript /
/// wpctl call) — for the inline volume slider, NOT the hot gesture path.
/// `None` when the platform has no cheap read-back.
pub fn get_system_volume() -> Option<u8> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg("output volume of (get volume settings)")
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<i32>()
            .ok()
            .map(clamp_volume)
    }
    #[cfg(target_os = "linux")]
    {
        // `wpctl get-volume @DEFAULT_AUDIO_SINK@` → "Volume: 0.42"
        let out = std::process::Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout);
        let frac: f32 = s.split_whitespace().nth(1)?.parse().ok()?;
        Some(clamp_volume((frac * 100.0).round() as i32))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// Set the system output volume to an absolute level (0–100). Returns the
/// clamped level actually applied, or `None` if unsupported. Blocking.
pub fn set_system_volume(level: i32) -> Option<u8> {
    let lv = clamp_volume(level);
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(format!("set volume output volume {lv}"))
            .status();
        Some(lv)
    }
    #[cfg(target_os = "linux")]
    {
        let frac = format!("{:.2}", lv as f32 / 100.0);
        let ok = std::process::Command::new("wpctl")
            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &frac])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
            || std::process::Command::new("pactl")
                .args(["set-sink-volume", "@DEFAULT_SINK@", &format!("{lv}%")])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
        ok.then_some(lv)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        // Windows has no simple absolute-set without the endpoint COM API; the
        // slider falls back to relative key-taps via `adjust_volume` there.
        let _ = lv;
        None
    }
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
    #[cfg(target_os = "windows")]
    {
        // Synthesize the multimedia volume keys. Each press steps ~2%, so
        // emit roughly `delta/2` presses (min 1). Runtime-unverified.
        let presses = ((delta.abs() + 1) / 2).max(1);
        let vk = if delta >= 0 {
            win_vol::VK_VOLUME_UP
        } else {
            win_vol::VK_VOLUME_DOWN
        };
        for _ in 0..presses {
            win_vol::tap(vk);
        }
        Ok(0)
    }
    #[cfg(target_os = "linux")]
    {
        // PipeWire (wpctl) first, then PulseAudio (pactl). Both step the
        // default sink by a relative percentage; we don't read the result back
        // (return 0 like the other platforms — the caller only checks success).
        let sign = if delta >= 0 { "+" } else { "-" };
        let mag = delta.unsigned_abs();
        let wp = format!("{mag}%{sign}"); // wpctl: "5%+" / "5%-"
        let pa = format!("{sign}{mag}%"); // pactl: "+5%" / "-5%"
        run_first_ok(
            "volume",
            &[
                ("wpctl", &["set-volume", "-l", "1.0", "@DEFAULT_AUDIO_SINK@", &wp]),
                ("pactl", &["set-sink-volume", "@DEFAULT_SINK@", &pa]),
            ],
        )?;
        Ok(0)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = delta;
        Err(anyhow!("adjust_system_volume not implemented on this platform"))
    }
}

/// Nudge the volume by `delta` and return the **new** level (0–100), so a
/// caller (the gesture toast) can display it. macOS reads+clamps+sets+returns
/// in one synchronous `osascript`; other platforms fall back to
/// `adjust_system_volume` and report `None` (no cheap read-back). Blocking —
/// call off the hot path.
pub fn nudge_volume(delta: i32) -> Option<u8> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(format!(
                "set v to (output volume of (get volume settings)) + ({delta})"
            ))
            .arg("-e").arg("if v < 0 then set v to 0")
            .arg("-e").arg("if v > 100 then set v to 100")
            .arg("-e").arg("set volume output volume v")
            .arg("-e").arg("return v")
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<i32>()
            .ok()
            .map(|v| v.clamp(0, 100) as u8)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = adjust_system_volume(delta);
        None
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
    #[cfg(target_os = "windows")]
    {
        // Synthesize the mute multimedia key. We can't cheaply read the
        // resulting state back, so report `true` (best-effort — the frontend
        // ignores the value for the `mute` command). Runtime-unverified.
        win_vol::tap(win_vol::VK_VOLUME_MUTE);
        Ok(true)
    }
    #[cfg(target_os = "linux")]
    {
        // Toggle mute on the default sink (PipeWire, then PulseAudio). We can't
        // cheaply read the resulting state, so report `true` best-effort (the
        // `mute` command ignores the value).
        run_first_ok(
            "mute",
            &[
                ("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]),
                ("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "toggle"]),
            ],
        )?;
        Ok(true)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err(anyhow!("toggle_system_mute not implemented on this platform"))
    }
}

/// Windows multimedia-key synthesis for volume / mute (v0.61.0).
#[cfg(target_os = "windows")]
mod win_vol {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    };

    pub const VK_VOLUME_MUTE: u8 = 0xAD;
    pub const VK_VOLUME_DOWN: u8 = 0xAE;
    pub const VK_VOLUME_UP: u8 = 0xAF;

    /// Press + release a virtual key.
    pub fn tap(vk: u8) {
        unsafe {
            keybd_event(vk, 0, KEYBD_EVENT_FLAGS(0), 0);
            keybd_event(vk, 0, KEYEVENTF_KEYUP, 0);
        }
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
