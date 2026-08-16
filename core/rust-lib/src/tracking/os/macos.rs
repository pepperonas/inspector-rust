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
    // Native fast path first: NSWorkspace (app name/bundle — no permission
    // needed) + AX (focused-window title — needs Accessibility). Two in-process
    // calls instead of an osascript spawn per 1.5 s tick (~19 000 spawns per
    // 8-hour session). With Accessibility granted the AX title is
    // authoritative → zero process spawns. Without it, osascript (Automation
    // grant) still supplies the title as before; if that fails too, the native
    // app-level info is returned (better than the old None).
    if let Some(fi) = frontmost_native() {
        if crate::expander::accessibility_granted() {
            return Some(fi);
        }
        return frontmost_osascript().or(Some(fi));
    }
    frontmost_osascript()
}

// ── Native (no process spawn) ────────────────────────────────────────────────

use std::ffi::{c_void, CString};

type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFAllocatorRef = *const c_void;
type AXUIElementRef = *const c_void;
const KCF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFAllocatorDefault: CFAllocatorRef;
    fn CFStringCreateWithCString(a: CFAllocatorRef, s: *const i8, enc: u32) -> CFStringRef;
    fn CFRelease(t: CFTypeRef);
}
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        el: AXUIElementRef,
        attr: CFStringRef,
        out: *mut CFTypeRef,
    ) -> i32;
}

fn cfstr(s: &str) -> CFStringRef {
    let c = CString::new(s).unwrap();
    unsafe { CFStringCreateWithCString(kCFAllocatorDefault, c.as_ptr(), KCF_STRING_ENCODING_UTF8) }
}

unsafe fn nsstring_to_string(s: *mut objc2::runtime::AnyObject) -> Option<String> {
    use objc2::msg_send;
    if s.is_null() {
        return None;
    }
    let utf8: *const std::os::raw::c_char = msg_send![s, UTF8String];
    if utf8.is_null() {
        return None;
    }
    Some(std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned())
}

/// Frontmost app via `NSWorkspace` (+ focused-window title via AX when
/// Accessibility is granted). Reading `frontmostApplication` off the main
/// thread is a snapshot read — a tick's worth of staleness is fine here.
fn frontmost_native() -> Option<FocusInfo> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    // Autorelease pool (fixed 2026-08-16): frontmostApplication /
    // localizedName / bundleIdentifier all return AUTORELEASED objects, and
    // this runs every ~1.5 s on the tracker's own thread — which has no pool,
    // so an 8-hour tracked day (~19 000 ticks) accumulated them in a page that
    // was never drained. The AX/CF half below is manually balanced already;
    // this covers the Cocoa half.
    let (app_name, bundle, pid) = objc2::rc::autoreleasepool(|_| unsafe {
        let ws_cls = AnyClass::get(c"NSWorkspace")?;
        let ws: *mut AnyObject = msg_send![ws_cls, sharedWorkspace];
        if ws.is_null() {
            return None;
        }
        let app: *mut AnyObject = msg_send![ws, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let name_obj: *mut AnyObject = msg_send![app, localizedName];
        let name = nsstring_to_string(name_obj)?;
        if name.is_empty() {
            return None;
        }
        let bid_obj: *mut AnyObject = msg_send![app, bundleIdentifier];
        let bid = nsstring_to_string(bid_obj).filter(|s| !s.is_empty());
        let pid: i32 = msg_send![app, processIdentifier];
        Some((name, bid, pid))
    })?;
    let window_title = focused_window_title(pid).filter(|t| !t.is_empty());
    Some(FocusInfo { app_name, app_id: bundle, window_title })
}

/// The focused window's `AXTitle` for `pid`. `None` without Accessibility (AX
/// returns an error), for title-less windows, or on any AX hiccup.
fn focused_window_title(pid: i32) -> Option<String> {
    unsafe {
        let app_el = AXUIElementCreateApplication(pid);
        if app_el.is_null() {
            return None;
        }
        let attr_win = cfstr("AXFocusedWindow");
        let mut win: CFTypeRef = std::ptr::null();
        let e = AXUIElementCopyAttributeValue(app_el, attr_win, &mut win);
        CFRelease(attr_win);
        CFRelease(app_el);
        if e != 0 || win.is_null() {
            return None;
        }
        let attr_title = cfstr("AXTitle");
        let mut title: CFTypeRef = std::ptr::null();
        let e2 = AXUIElementCopyAttributeValue(win, attr_title, &mut title);
        CFRelease(attr_title);
        CFRelease(win);
        if e2 != 0 || title.is_null() {
            return None;
        }
        // CFString is toll-free bridged to NSString → read via UTF8String.
        let s = nsstring_to_string(title as *mut objc2::runtime::AnyObject);
        CFRelease(title);
        s
    }
}

// ── osascript fallback (Automation grant; one process spawn per call) ────────

fn frontmost_osascript() -> Option<FocusInfo> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_frontmost_does_not_crash_and_yields_sane_info() {
        // Exercises the NSWorkspace + AX FFI. Headless CI has no frontmost
        // app (returns None) — assert only the invariants of a Some result.
        if let Some(fi) = frontmost_native() {
            assert!(!fi.app_name.is_empty());
            if let Some(t) = &fi.window_title {
                assert!(!t.is_empty(), "empty titles must be filtered to None");
            }
        }
    }
}
