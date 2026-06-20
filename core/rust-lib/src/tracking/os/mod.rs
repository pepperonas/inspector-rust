//! Per-OS active-window + idle queries for the tracker. Each platform behind a
//! `#[cfg]` so a missing OS module never breaks the build; non-implemented
//! platforms return `None` and the tracker degrades to "no data" cleanly.
//!
//! - **macOS** (`macos.rs`): frontmost app/title via `osascript` (the proven
//!   path, same TCC surface as the screenshot-name feature) + idle via
//!   `CGEventSourceSecondsSinceLastEventType`.
//! - **Windows** (`windows.rs`, runtime-unverified): `GetForegroundWindow` +
//!   `QueryFullProcessImageNameW` + `GetWindowTextW`; idle via `GetLastInputInfo`.
//! - **Linux** (`linux.rs`, X11 best-effort): `xdotool` for the active window +
//!   `xprintidle` for idle (Wayland blocks these → `None`, clean degrade).

/// The frontmost application + (best-effort) its front window's title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusInfo {
    pub app_name: String,
    pub app_id: Option<String>,
    pub window_title: Option<String>,
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{frontmost, idle_seconds};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{frontmost, idle_seconds};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{frontmost, idle_seconds};

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn frontmost() -> Option<FocusInfo> {
    None
}
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn idle_seconds() -> Option<f64> {
    None
}
