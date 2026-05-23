//! Read the current Finder selection via AppleScript (`osascript`).
//!
//! macOS Finder exposes its selection through AppleEvents. We shell
//! out to `/usr/bin/osascript` rather than linking ScriptingBridge —
//! the script is 6 lines, the round-trip is ~30 ms cold, and we sidestep
//! every objc / runtime quirk.
//!
//! ## TCC requirements
//!
//! For osascript to actually drive Finder from a Hardened-Runtime app,
//! three things must align:
//!
//! 1. The bundle has the `com.apple.security.automation.apple-events`
//!    entitlement (declared in `entitlements.plist`).
//! 2. `Info.plist` carries `NSAppleEventsUsageDescription` — the
//!    permission-prompt copy macOS shows the user. We inject this in
//!    `scripts/install-macos.sh` post-build, since the Tauri bundler
//!    has no first-class field for arbitrary Info.plist keys.
//! 3. The user grants Automation → Finder in System Settings → Privacy
//!    & Security → Automation. The TCC prompt fires on the first call.
//!
//! When the user denies (or hasn't been prompted yet on a stale grant),
//! osascript returns errno -1743 ("not authorized to send AppleEvents
//! to Finder"). We translate that to [`ERR_AUTOMATION_DENIED`] so the
//! frontend can show a tailored "open System Settings" banner instead
//! of a generic error.

use std::path::PathBuf;

/// Sentinel returned to the frontend when Automation→Finder is not
/// authorised. Mirrors the existing `ax.permission_denied` /
/// `screen.permission_denied` sentinels (expander, OCR).
pub const ERR_AUTOMATION_DENIED: &str = "finder.automation_denied";

/// Read the current Finder selection. Returns the list of POSIX
/// paths of every selected item (files + folders), or an empty list
/// when nothing is selected.
#[cfg(target_os = "macos")]
pub fn read() -> Result<Vec<PathBuf>, String> {
    use std::process::Command;

    // The script iterates Finder's selection, coerces each item to an
    // `alias` (works for both files and folders, fails silently for
    // weird items like network mount placeholders), and emits the
    // POSIX path one per line. `linefeed` over a manual `\n` so the
    // newline survives any AppleScript string-escaping quirks.
    const SCRIPT: &str = r#"tell application "Finder"
    set sel to selection
    set out to ""
    repeat with x in sel
        try
            set out to out & POSIX path of (x as alias) & linefeed
        end try
    end repeat
    return out
end tell"#;

    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(SCRIPT)
        .output()
        .map_err(|e| format!("osascript spawn failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // -1743 = errAEEventNotPermitted (TCC Automation denied)
        // -600 = procNotFound (Finder isn't running — basically never
        //        happens, but treat it as "no selection" rather than an
        //        error so the popup just shows nothing).
        if stderr.contains("-1743")
            || stderr.contains("not allowed")
            || stderr.contains("not authorized")
        {
            return Err(ERR_AUTOMATION_DENIED.into());
        }
        if stderr.contains("-600") {
            return Ok(Vec::new());
        }
        return Err(format!("osascript: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<PathBuf> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect();
    Ok(paths)
}

#[cfg(not(target_os = "macos"))]
pub fn read() -> Result<Vec<PathBuf>, String> {
    Err("finder selection: only supported on macOS".into())
}
