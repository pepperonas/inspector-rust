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
/// when nothing is selected. v0.35.2+: a 2-second watchdog kills the
/// osascript process if Finder is hung, so the Ctrl+Shift+F hotkey
/// can't wedge the popup indefinitely on a frozen Finder.
#[cfg(target_os = "macos")]
pub fn read() -> Result<Vec<PathBuf>, String> {
    use crate::osascript_util::{run_osascript, OsaResult};
    use std::time::Duration;

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

    // 2 s — more headroom than the frontmost-app probe because
    // Finder genuinely can take ~100-300 ms when a large selection
    // is open + a slow network volume is mounted, but still well
    // below the user's patience threshold for "hotkey did nothing".
    let output = match run_osascript(SCRIPT, Duration::from_secs(2)) {
        OsaResult::Done(out) => out,
        OsaResult::TimedOut => {
            return Err(
                "finder selection: osascript timed out after 2 s (Finder hung?)".into(),
            );
        }
        OsaResult::SpawnFailed(e) => return Err(format!("osascript spawn failed: {e}")),
    };

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

// ── touch / mkdir in the front Finder window's folder ────────────────────────

/// Validate a user-supplied file/folder name: non-empty, no path
/// separators or traversal, no NUL byte — so creation can't escape the
/// target folder.
#[cfg(target_os = "macos")]
fn sanitize_name(name: &str) -> Result<&str, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("name is empty".into());
    }
    if n.contains('/') || n.contains('\0') || n == "." || n == ".." {
        return Err("name must be a plain file/folder name (no '/', '.', '..')".into());
    }
    Ok(n)
}

/// POSIX path of the folder where Finder would create a new item — the
/// frontmost window's target, or the Desktop if no window is open
/// (`insertion location`). Needs the Automation→Finder TCC grant.
#[cfg(target_os = "macos")]
fn front_dir() -> Result<PathBuf, String> {
    use crate::osascript_util::{run_osascript, OsaResult};
    use std::time::Duration;

    const SCRIPT: &str =
        r#"tell application "Finder" to return POSIX path of (insertion location as alias)"#;
    let output = match run_osascript(SCRIPT, Duration::from_secs(2)) {
        OsaResult::Done(o) => o,
        OsaResult::TimedOut => return Err("finder dir: osascript timed out (Finder hung?)".into()),
        OsaResult::SpawnFailed(e) => return Err(format!("osascript spawn failed: {e}")),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("-1743") || stderr.contains("not allowed") || stderr.contains("not authorized") {
            return Err(ERR_AUTOMATION_DENIED.into());
        }
        return Err(format!("osascript: {}", stderr.trim()));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return Err("finder dir: no insertion location (no open window + no Desktop?)".into());
    }
    Ok(PathBuf::from(path))
}

/// Best-effort: select the freshly-created item in Finder so the user
/// sees it appear. Failures are ignored (the file/folder already exists).
#[cfg(target_os = "macos")]
fn reveal_in_finder(path: &std::path::Path) {
    use crate::osascript_util::run_osascript;
    use std::time::Duration;
    let escaped = path
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let script = format!(
        r#"tell application "Finder" to reveal (POSIX file "{escaped}" as alias)"#,
    );
    let _ = run_osascript(&script, Duration::from_secs(2));
}

/// Create an empty file `name` in the front Finder folder. Errors if it
/// already exists. Returns the absolute path created.
#[cfg(target_os = "macos")]
pub fn create_file(name: &str) -> Result<PathBuf, String> {
    let n = sanitize_name(name)?;
    let path = front_dir()?.join(n);
    if path.exists() {
        return Err(format!("already exists: {}", path.display()));
    }
    std::fs::File::create(&path).map_err(|e| format!("create file failed: {e}"))?;
    reveal_in_finder(&path);
    Ok(path)
}

/// Create a folder `name` in the front Finder folder. Errors if it
/// already exists. Returns the absolute path created.
#[cfg(target_os = "macos")]
pub fn create_dir(name: &str) -> Result<PathBuf, String> {
    let n = sanitize_name(name)?;
    let path = front_dir()?.join(n);
    if path.exists() {
        return Err(format!("already exists: {}", path.display()));
    }
    std::fs::create_dir(&path).map_err(|e| format!("create folder failed: {e}"))?;
    reveal_in_finder(&path);
    Ok(path)
}

/// Open a terminal at the front Finder folder. Prefers **iTerm2** (the
/// user's terminal) if installed, falling back to Terminal.app. Returns
/// the directory opened.
#[cfg(target_os = "macos")]
pub fn open_terminal() -> Result<PathBuf, String> {
    let dir = front_dir()?;
    // Try iTerm2 by bundle id first; `open -b` exits non-zero if it isn't
    // installed, in which case fall back to the built-in Terminal.app.
    let iterm_ok = std::process::Command::new("/usr/bin/open")
        .args(["-b", "com.googlecode.iterm2"])
        .arg(&dir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !iterm_ok {
        let status = std::process::Command::new("/usr/bin/open")
            .args(["-a", "Terminal"])
            .arg(&dir)
            .status()
            .map_err(|e| format!("open Terminal failed: {e}"))?;
        if !status.success() {
            return Err("failed to open a terminal".into());
        }
    }
    Ok(dir)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::sanitize_name;

    #[test]
    fn accepts_plain_names_and_trims() {
        assert_eq!(sanitize_name("notes.txt").unwrap(), "notes.txt");
        assert_eq!(sanitize_name("  spaced.md  ").unwrap(), "spaced.md");
        assert_eq!(sanitize_name("My Folder").unwrap(), "My Folder");
        // A dot inside the name (not the whole name) is fine.
        assert_eq!(sanitize_name("archive.tar.gz").unwrap(), "archive.tar.gz");
        // Leading dot (hidden file) is allowed — it's not "." or "..".
        assert_eq!(sanitize_name(".gitignore").unwrap(), ".gitignore");
        // Unicode is fine.
        assert_eq!(sanitize_name("Über.txt").unwrap(), "Über.txt");
    }

    #[test]
    fn rejects_empty_or_whitespace() {
        assert!(sanitize_name("").is_err());
        assert!(sanitize_name("   ").is_err());
        assert!(sanitize_name("\t").is_err());
    }

    #[test]
    fn rejects_path_separators_so_creation_cant_escape_the_folder() {
        assert!(sanitize_name("a/b").is_err());
        assert!(sanitize_name("/etc/passwd").is_err());
        assert!(sanitize_name("../secret").is_err());
        assert!(sanitize_name("sub/dir/file").is_err());
    }

    #[test]
    fn rejects_dot_and_dotdot() {
        assert!(sanitize_name(".").is_err());
        assert!(sanitize_name("..").is_err());
        // After trimming too.
        assert!(sanitize_name("  ..  ").is_err());
    }

    #[test]
    fn rejects_nul_byte() {
        assert!(sanitize_name("evil\0name").is_err());
    }
}
