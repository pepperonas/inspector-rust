//! macOS active-window + idle queries.

use super::FocusInfo;
use crate::osascript_util::{run_osascript, OsaResult};
use std::time::Duration;

// One osascript call returns three linefeed-separated fields: app name, bundle
// id (best-effort), front window title (best-effort). App name needs the
// Automation→System Events grant (same as the screenshot-name feature); the
// window title additionally needs Accessibility — both are `try`-guarded so a
// missing grant just yields an empty field, never an error.
const FRONTMOST_SCRIPT: &str = r#"tell application "System Events"
  set p to first application process whose frontmost is true
  set a to name of p
  set b to ""
  try
    set b to bundle identifier of p
  end try
  set w to ""
  try
    set w to name of front window of p
  end try
end tell
a & linefeed & b & linefeed & w"#;

pub fn frontmost() -> Option<FocusInfo> {
    let out = match run_osascript(FRONTMOST_SCRIPT, Duration::from_millis(1500)) {
        OsaResult::Done(o) => o,
        OsaResult::TimedOut | OsaResult::SpawnFailed(_) => return None,
    };
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    let mut lines = raw.splitn(3, '\n');
    let app = lines.next().unwrap_or("").trim().to_string();
    if app.is_empty() {
        return None;
    }
    let bundle = lines.next().unwrap_or("").trim().to_string();
    let title = lines.next().unwrap_or("").trim().to_string();
    Some(FocusInfo {
        app_name: app,
        app_id: (!bundle.is_empty()).then_some(bundle),
        window_title: (!title.is_empty()).then_some(title),
    })
}

/// Seconds since the last HID input (keyboard/mouse), via CoreGraphics. `None`
/// if the call returns a non-finite/negative value.
pub fn idle_seconds() -> Option<f64> {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: u32, event_type: u32) -> f64;
    }
    // kCGEventSourceStateHIDSystemState = 1; kCGAnyInputEventType = ~0.
    const HID_SYSTEM_STATE: u32 = 1;
    const ANY_INPUT_EVENT: u32 = 0xFFFF_FFFF;
    let s = unsafe { CGEventSourceSecondsSinceLastEventType(HID_SYSTEM_STATE, ANY_INPUT_EVENT) };
    (s.is_finite() && s >= 0.0).then_some(s)
}
