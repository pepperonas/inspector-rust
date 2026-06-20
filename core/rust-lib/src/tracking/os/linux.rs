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
    let id = run("xdotool", &["getactivewindow"])?;
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    let title = run("xdotool", &["getwindowname", id])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let app_name = run("xdotool", &["getwindowpid", id])
        .map(|s| s.trim().to_string())
        .filter(|p| !p.is_empty())
        .and_then(|pid| run("ps", &["-p", &pid, "-o", "comm="]))
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
