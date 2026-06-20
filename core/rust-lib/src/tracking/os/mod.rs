//! Per-OS active-window + idle queries for the tracker. Each platform behind a
//! `#[cfg]` so a missing OS module never breaks the build; non-implemented
//! platforms return `None` and the tracker degrades to "no data" cleanly.
//!
//! - **macOS** (`macos.rs`): frontmost app/title via `osascript` (the proven
//!   path, same TCC surface as the screenshot-name feature) + idle via
//!   `CGEventSourceSecondsSinceLastEventType`.
//! - **Windows / Linux**: land in a later delivery step (see `docs/timesheet.md`).

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

#[cfg(not(target_os = "macos"))]
pub fn frontmost() -> Option<FocusInfo> {
    None
}
#[cfg(not(target_os = "macos"))]
pub fn idle_seconds() -> Option<f64> {
    None
}
