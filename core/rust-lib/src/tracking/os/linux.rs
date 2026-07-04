//! Linux active-window + idle queries (X11, **best-effort**). Uses `xdotool`
//! (active window + title + pid) and `xprintidle` (idle ms) when present; under
//! Wayland these fail → `None`, and the tracker degrades cleanly (the
//! clipboard-paste / focus-less paths still work). Installing
//! `xdotool` + `xprintidle` enables it on X11/XWayland.

use super::FocusInfo;
use std::process::Command;

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn frontmost() -> Option<FocusInfo> {
    // ONE chained xdotool invocation instead of three (`getwindowname` /
    // `getwindowpid` consume the window id `getactivewindow` pushes on
    // xdotool's window stack), and `/proc/<pid>/comm` instead of a `ps`
    // spawn — this runs every 1.5 s tick while tracking, so the old shape
    // was 4 fork/execs per tick (~77k per 8-h day); now it's 1.
    let out = run("xdotool", &["getactivewindow", "getwindowname", "getwindowpid"])?;
    let mut lines = out.lines();
    let title = lines
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let app_name = lines
        .next()
        .map(|s| s.trim().to_string())
        .filter(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        .and_then(|pid| std::fs::read_to_string(format!("/proc/{pid}/comm")).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown".to_string());
    Some(FocusInfo {
        app_name,
        app_id: None,
        window_title: title,
    })
}

pub fn idle_seconds() -> Option<f64> {
    run("xprintidle", &[])
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map(|ms| ms / 1000.0)
}
