//! Read the name of the currently-frontmost application via AppleScript.
//!
//! Used by the screenshot pipeline so saved PNGs are named
//! `<App>-YYYYMMDD-HHMMSS.png` instead of the generic
//! `inspector-rust-screenshot-<ts>.png`. Knowing which app the
//! screenshot was taken on makes a long Downloads folder grep-able.
//!
//! macOS only — we shell out to `/usr/bin/osascript` and ask System
//! Events for the frontmost process name. Same TCC Automation grant
//! the Finder-selection feature uses (`com.apple.security.automation.apple-events`
//! entitlement + `NSAppleEventsUsageDescription` in Info.plist). If
//! Automation is denied or the call fails for any other reason, we
//! return `None` and the caller falls back to a generic filename —
//! never blocks the screenshot itself.
//!
//! Sanitised output: control chars + filesystem-hostile separators
//! (`/`, `:`, `\`) stripped, max 40 chars to keep filenames readable.

/// Sentinel set of characters we never let through into a filename.
/// `/` and `:` are path separators on macOS HFS+/APFS; `\` is rejected
/// by some downstream tools; the control chars cover nulls and CRLF
/// that an injected app name could otherwise sneak through.
fn is_unsafe(c: char) -> bool {
    matches!(c, '/' | ':' | '\\' | '\0' | '\n' | '\r' | '\t')
}

fn sanitize(s: &str) -> String {
    let cleaned: String = s.chars().filter(|c| !is_unsafe(*c)).collect();
    let trimmed = cleaned.trim();
    // Cap length so the final filename stays reasonable
    // (40 chars + 16-char timestamp + ".png" stays comfortably under
    // any POSIX/HFS 255-char filename limit).
    trimmed.chars().take(40).collect()
}

/// Native fast path (v0.105.0): `NSWorkspace.frontmostApplication.
/// localizedName` — no subprocess, no Automation TCC prompt, microseconds.
/// The clipboard watcher consults [`name`] on EVERY copy once the
/// exclude-apps list is non-empty; the osascript route forked a process +
/// a System Events round trip per copy (~30 ms median, up to the 1.5 s
/// watchdog when System Events is busy). Off-main-thread read is a snapshot
/// — the same doctrine as `tracking/os/macos.rs`'s `frontmost_native`.
#[cfg(target_os = "macos")]
fn name_native() -> Option<String> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    // Autorelease pool: this runs per COPY (clipboard watcher, exclude-list)
    // on the watcher's pool-less thread — same leak class as tracking's
    // frontmost_native, fixed the same day. The owned Rust String is built
    // inside; only the autoreleased Cocoa temporaries die with the pool.
    objc2::rc::autoreleasepool(|_| unsafe {
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
        if name_obj.is_null() {
            return None;
        }
        let utf8: *const std::os::raw::c_char = msg_send![name_obj, UTF8String];
        if utf8.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned())
    })
}

/// Best-effort lookup of the currently-frontmost app's name. Native
/// NSWorkspace first; the AppleScript route stays as the fallback (it can
/// answer in edge states where `frontmostApplication` is nil). Returns
/// `None` if both fail, the name comes back empty, the osascript call
/// **times out** (v0.35.2+ — see `osascript_util`), or we're not on macOS.
/// Caller treats `None` as "fall back to the generic filename / skip the
/// related feature".
#[cfg(target_os = "macos")]
pub fn name() -> Option<String> {
    use crate::osascript_util::{run_osascript, OsaResult};
    use std::time::Duration;

    if let Some(native) = name_native() {
        let clean = sanitize(&native);
        if !clean.is_empty() {
            return Some(clean);
        }
    }

    // `System Events` is the standard target for frontmost-app probes;
    // it's pre-installed on every macOS and exposes the process list
    // without needing to scriptable-bridge into the target app itself.
    const SCRIPT: &str = r#"tell application "System Events" to get name of first application process whose frontmost is true"#;

    // 1.5 s is ~50× the median (~30 ms) and 10× the 95th percentile.
    // Beyond that the call is hung — usually because System Events
    // itself is being talked to by something else, or the user
    // hasn't granted Automation yet. The hotkey handler that calls
    // us bails to a no-op cleanly on `None`.
    let out = match run_osascript(SCRIPT, Duration::from_millis(1500)) {
        OsaResult::Done(out) => out,
        OsaResult::TimedOut | OsaResult::SpawnFailed(_) => return None,
    };
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let clean = sanitize(&raw);
    if clean.is_empty() {
        None
    } else {
        Some(clean)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn name() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_path_separators() {
        assert_eq!(sanitize("foo/bar"), "foobar");
        assert_eq!(sanitize("a:b\\c"), "abc");
    }

    #[test]
    fn sanitize_strips_control_chars() {
        assert_eq!(sanitize("Safari\n\r\t"), "Safari");
        assert_eq!(sanitize("a\0b"), "ab");
    }

    #[test]
    fn sanitize_caps_at_40_chars() {
        let huge = "x".repeat(100);
        assert_eq!(sanitize(&huge).len(), 40);
    }

    #[test]
    fn sanitize_trims_surrounding_whitespace() {
        assert_eq!(sanitize("  Safari  "), "Safari");
    }

    #[test]
    fn sanitize_preserves_unicode() {
        assert_eq!(sanitize("Notizen"), "Notizen");
        assert_eq!(sanitize("Übersicht"), "Übersicht");
    }
}
