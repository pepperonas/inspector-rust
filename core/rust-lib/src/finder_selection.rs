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

// ── touch / mkdir in the front Explorer window's folder (Windows) ─────────────
//
// Windows analog of the macOS Finder path above. macOS asks Finder for its
// `insertion location` via osascript; Windows has no equivalent single query,
// so we (1) find the frontmost File Explorer window natively by walking the
// top-level z-order for the first `CabinetWClass` window, then (2) resolve
// that window's current folder to a filesystem path via the `Shell.Application`
// COM object — driven from PowerShell, mirroring the osascript shell-out. If
// no Explorer window is open we fall back to the Desktop, exactly like Finder's
// insertion-location → Desktop behaviour.

/// Validate a user-supplied file/folder name on Windows: non-empty, not
/// `.`/`..`, and free of the reserved path characters — so creation can't
/// escape the target folder.
#[cfg(target_os = "windows")]
fn sanitize_name(name: &str) -> Result<&str, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("name is empty".into());
    }
    if n == "." || n == ".." {
        return Err("name must be a plain file/folder name (not '.' or '..')".into());
    }
    if n.chars().any(|c| {
        matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0')
    }) {
        return Err(r#"name contains an invalid character (\ / : * ? " < > |)"#.into());
    }
    Ok(n)
}

/// HWND (as `isize`) of the frontmost File Explorer window, or `None` when
/// no Explorer window is open. Walks the top-level z-order from the top and
/// returns the first visible window whose class is `CabinetWClass` (the
/// standard File Explorer window class). Our own popup sits above it in the
/// z-order but has a different class, so it's skipped.
#[cfg(target_os = "windows")]
fn topmost_explorer_hwnd() -> Option<isize> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetTopWindow, GetWindow, IsWindowVisible, GW_HWNDNEXT,
    };
    unsafe {
        let mut h = GetTopWindow(None).ok()?;
        loop {
            if h.0.is_null() {
                break;
            }
            if IsWindowVisible(h).as_bool() {
                let mut buf = [0u16; 256];
                let n = GetClassNameW(h, &mut buf);
                if n > 0 {
                    let class = String::from_utf16_lossy(&buf[..n as usize]);
                    if class == "CabinetWClass" {
                        return Some(h.0 as isize);
                    }
                }
            }
            h = match GetWindow(h, GW_HWNDNEXT) {
                Ok(next) => next,
                Err(_) => break,
            };
        }
    }
    None
}

/// Run a PowerShell snippet with a watchdog timeout, returning trimmed
/// stdout on success. Windows analog of `osascript_util::run_osascript`:
/// a hung Shell COM call can't wedge the popup indefinitely.
#[cfg(target_os = "windows")]
fn run_powershell(script: &str, timeout: std::time::Duration) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => {
                let mut stdout = String::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = s.read_to_string(&mut stdout);
                }
                return Some(stdout);
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    tracing::warn!("finder(win): powershell timed out after {timeout:?}");
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(_) => {
                let _ = child.kill();
                return None;
            }
        }
    }
}

/// Read the window title for an HWND. Used to detect the active tab in
/// Windows 11 tabbed Explorer (the title shows the active tab's name).
#[cfg(target_os = "windows")]
fn explorer_window_title(hwnd: isize) -> Option<String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::GetWindowTextW;
    unsafe {
        let h = HWND(hwnd as *mut _);
        let mut buf = [0u16; 512];
        let n = GetWindowTextW(h, &mut buf);
        if n > 0 {
            Some(String::from_utf16_lossy(&buf[..n as usize]))
        } else {
            None
        }
    }
}

/// Extract the active tab's display name from a Windows 11 Explorer title.
///
/// Formats observed (note: the separator is EN DASH U+2013, not ASCII hyphen):
/// - Single tab:  `Downloads \u{2013} Explorer`
/// - Multi tab:   `Downloads und 2 weitere Registerkarten \u{2013} Explorer`
/// - English:     `Downloads and 2 more tabs \u{2013} File Explorer`
/// - Older Win10: `Downloads - File Explorer` (ASCII hyphen)
///
/// Returns `None` if the title cannot be parsed.
#[cfg(any(target_os = "windows", test))]
fn active_tab_name_from_title(title: &str) -> Option<String> {
    // The separator between the tab name and "Explorer" can be:
    //   " \u{2013} " (EN DASH, Win11)  or  " - " (ASCII hyphen, Win10).
    // Find the LAST occurrence of either separator followed by an Explorer
    // suffix. We search right-to-left so folder names that happen to
    // contain " - " (rare but possible) are handled correctly.
    let separators: &[&str] = &[" \u{2013} ", " - "];
    let mut best_split: Option<usize> = None;

    for sep in separators {
        if let Some(idx) = title.rfind(sep) {
            let after = &title[idx + sep.len()..];
            // Suffix must be one of: "Explorer", "File Explorer", "Datei-Explorer"
            let after_trimmed = after.trim_end();
            if after_trimmed == "Explorer"
                || after_trimmed == "File Explorer"
                || after_trimmed == "Datei-Explorer"
            {
                // Pick the rightmost separator (largest idx).
                if best_split.map_or(true, |prev| idx > prev) {
                    best_split = Some(idx);
                }
            }
        }
    }

    let name = best_split.map(|idx| title[..idx].trim_end())?;
    if name.is_empty() {
        return None;
    }

    // Strip the tab-count suffix: " und N weitere Registerkarte[n]" (German)
    // or " and N more tab[s]" (English).
    let tab_name = strip_tab_count_suffix(name);
    if tab_name.is_empty() {
        None
    } else {
        Some(tab_name.to_string())
    }
}

/// Strip " und N weitere Registerkarte[n]" / " and N more tab[s]" from the end.
#[cfg(any(target_os = "windows", test))]
fn strip_tab_count_suffix(s: &str) -> &str {
    // Look for " und " or " and " followed by a digit — simple scan.
    // German: " und 2 weitere Registerkarten"
    if let Some(idx) = s.rfind(" und ") {
        let after = &s[idx + 5..];
        if after.starts_with(|c: char| c.is_ascii_digit())
            && (after.contains("weitere Registerkarte"))
        {
            return s[..idx].trim_end();
        }
    }
    // English: " and 2 more tabs"
    if let Some(idx) = s.rfind(" and ") {
        let after = &s[idx + 5..];
        if after.starts_with(|c: char| c.is_ascii_digit()) && after.contains("more tab") {
            return s[..idx].trim_end();
        }
    }
    s
}

/// Resolve an Explorer window HWND to its current folder's filesystem path
/// via `Shell.Application`. For Windows 11 tabbed Explorer we disambiguate
/// by matching the active tab's `LocationName` (derived from the window title)
/// against the COM windows collection. Returns `None` on any failure or for
/// non-filesystem locations (This PC, Control Panel, …) whose `Path` is empty.
#[cfg(target_os = "windows")]
fn explorer_path_for_hwnd(hwnd: isize) -> Option<String> {
    // Determine the active tab name so we can pick the right Shell window
    // when multiple tabs share the same HWND (Windows 11).
    let tab_filter = explorer_window_title(hwnd)
        .as_deref()
        .and_then(active_tab_name_from_title)
        .unwrap_or_default();

    // Build the PowerShell script. If we have a tab name, match by
    // LocationName and take the last hit (handles duplicate tab names —
    // the last instance is typically the most recently focused). Without a
    // tab name we fall back to taking the first path (pre-Win11 behaviour).
    let script = if tab_filter.is_empty() {
        // Legacy / single-tab: take the first window with matching HWND.
        format!(
            "$ErrorActionPreference='SilentlyContinue';\
             $t={hwnd};\
             $sh=New-Object -ComObject Shell.Application;\
             foreach($w in $sh.Windows()){{try{{if([int64]$w.HWND -eq $t)\
             {{$p=$w.Document.Folder.Self.Path;if($p){{Write-Output $p}};break}}}}catch{{}}}}"
        )
    } else {
        // Win11 tabbed: match LocationName to the active tab title.
        // Take the LAST match so duplicate-named tabs resolve to the newest.
        // PowerShell escaping: the tab name comes from GetWindowText and is
        // safe (no user input injection), but we single-quote it anyway.
        let escaped = tab_filter.replace('\'', "''"); // PS single-quote escape
        format!(
            "$ErrorActionPreference='SilentlyContinue';\
             $t={hwnd};$n='{escaped}';\
             $sh=New-Object -ComObject Shell.Application;$r='';\
             foreach($w in $sh.Windows()){{try{{if([int64]$w.HWND -eq $t -and $w.LocationName -eq $n)\
             {{$p=$w.Document.Folder.Self.Path;if($p){{$r=$p}}}}}}catch{{}}}};\
             if($r){{Write-Output $r}}"
        )
    };
    let out = run_powershell(&script, std::time::Duration::from_secs(3))?;
    let p = out.trim().to_string();
    if p.is_empty() {
        None
    } else {
        Some(p)
    }
}

/// First open Explorer window whose folder is a real filesystem path. Used as
/// a fallback when the precise frontmost-HWND/active-tab match misses (some
/// Win11 tab layouts) — better to land in *an* Explorer folder than to dump
/// the new item onto the Desktop.
#[cfg(target_os = "windows")]
fn first_explorer_path() -> Option<String> {
    let script = "$ErrorActionPreference='SilentlyContinue';\
         $sh=New-Object -ComObject Shell.Application;\
         foreach($w in $sh.Windows()){try{$p=$w.Document.Folder.Self.Path;\
         if($p -and (Test-Path -LiteralPath $p)){Write-Output $p;break}}catch{}}";
    let out = run_powershell(script, std::time::Duration::from_secs(3))?;
    let p = out.trim().to_string();
    if p.is_empty() {
        None
    } else {
        Some(p)
    }
}

/// Folder where a new item should be created — the frontmost Explorer
/// window's folder, or the Desktop when no Explorer window is open.
#[cfg(target_os = "windows")]
fn front_dir() -> Result<PathBuf, String> {
    if let Some(hwnd) = topmost_explorer_hwnd() {
        if let Some(p) = explorer_path_for_hwnd(hwnd) {
            return Ok(PathBuf::from(p));
        }
    }
    // The precise match missed — take any open Explorer folder before Desktop.
    if let Some(p) = first_explorer_path() {
        return Ok(PathBuf::from(p));
    }
    dirs::desktop_dir().ok_or_else(|| "no active Explorer window and no Desktop folder".into())
}

/// Best-effort: open Explorer with the freshly-created item selected so the
/// user sees it appear (the Windows analog of Finder reveal). `raw_arg` is
/// used because explorer.exe parses its own command line and is picky about
/// the `/select,"<path>"` form.
#[cfg(target_os = "windows")]
fn reveal_in_explorer(path: &std::path::Path) {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    let arg = format!("/select,\"{}\"", path.display());
    let _ = Command::new("explorer.exe").raw_arg(arg).spawn();
}

/// Create an empty file `name` in the front Explorer folder. Errors if it
/// already exists. Returns the absolute path created.
#[cfg(target_os = "windows")]
pub fn create_file(name: &str) -> Result<PathBuf, String> {
    let n = sanitize_name(name)?;
    let path = front_dir()?.join(n);
    if path.exists() {
        return Err(format!("already exists: {}", path.display()));
    }
    std::fs::File::create(&path).map_err(|e| format!("create file failed: {e}"))?;
    reveal_in_explorer(&path);
    Ok(path)
}

/// Create a folder `name` in the front Explorer folder. Errors if it
/// already exists. Returns the absolute path created.
#[cfg(target_os = "windows")]
pub fn create_dir(name: &str) -> Result<PathBuf, String> {
    let n = sanitize_name(name)?;
    let path = front_dir()?.join(n);
    if path.exists() {
        return Err(format!("already exists: {}", path.display()));
    }
    std::fs::create_dir(&path).map_err(|e| format!("create folder failed: {e}"))?;
    reveal_in_explorer(&path);
    Ok(path)
}

/// Open a terminal at the front Finder folder. Prefers **iTerm2** (the
/// user's terminal) if installed, falling back to Terminal.app. Returns
/// the directory opened.
/// Wrap `s` in POSIX single quotes for safe use in a shell `cd` (handles
/// spaces, `$`, etc.; embedded `'` becomes `'\''`). Pure + unit-tested.
#[cfg(any(target_os = "macos", test))]
pub fn sh_squote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Escape a string for embedding inside an AppleScript double-quoted literal
/// (`"\""` and `"\\"`). Pure + unit-tested.
#[cfg(any(target_os = "macos", test))]
pub fn osa_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "macos")]
fn iterm_installed() -> bool {
    if std::path::Path::new("/Applications/iTerm.app").exists() {
        return true;
    }
    dirs::home_dir()
        .map(|h| h.join("Applications/iTerm.app").exists())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
pub fn open_terminal() -> Result<PathBuf, String> {
    use std::process::Command;
    let dir = front_dir()?;
    let dir_str = dir.to_string_lossy().to_string();

    // iTerm2 first, if installed. `open -b … <dir>` opens iTerm but does NOT
    // cd into the folder, so drive it with AppleScript: a new window + an
    // explicit `cd '<dir>'`. (Terminal.app, by contrast, honours the folder
    // arg to `open -a` directly.)
    if iterm_installed() {
        let cd_cmd = format!("cd {}", sh_squote(&dir_str));
        let script = format!(
            "tell application \"iTerm\"\n\
                 activate\n\
                 create window with default profile\n\
                 tell current session of current window to write text \"{}\"\n\
             end tell",
            osa_escape(&cd_cmd)
        );
        let ok = Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(&script)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Ok(dir);
        }
        // fall through to Terminal.app on any iTerm failure
    }

    // Terminal.app — `open -a Terminal <dir>` opens a new window already at
    // the directory.
    let status = Command::new("/usr/bin/open")
        .args(["-a", "Terminal"])
        .arg(&dir)
        .status()
        .map_err(|e| format!("open Terminal failed: {e}"))?;
    if !status.success() {
        return Err("failed to open a terminal".into());
    }
    Ok(dir)
}

/// Open a terminal at the front Explorer folder (Windows). Prefers **Windows
/// Terminal** (`wt.exe -d <dir>`, the default on Windows 11), falling back to
/// PowerShell, then `cmd.exe` — each launched in its own console window with
/// the working directory set to the folder. Returns the directory opened.
///
/// **Windows runtime-unverified** — written compile-clean against the std
/// `CommandExt` API; validated for compilation but not yet exercised on a real
/// Windows box.
#[cfg(target_os = "windows")]
pub fn open_terminal() -> Result<PathBuf, String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    // CREATE_NEW_CONSOLE — give the spawned shell its own visible window
    // (a GUI app's children don't inherit a console otherwise).
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

    let dir = front_dir()?;

    // 1) Windows Terminal — its own GUI window, opens directly at `-d <dir>`.
    //    (wt.exe is reachable via the App Execution Alias on Win11.)
    if Command::new("wt.exe").arg("-d").arg(&dir).spawn().is_ok() {
        return Ok(dir);
    }
    // 2) PowerShell in a fresh console, started in the folder.
    if Command::new("powershell.exe")
        .arg("-NoExit")
        .current_dir(&dir)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .is_ok()
    {
        return Ok(dir);
    }
    // 3) cmd.exe fallback.
    Command::new("cmd.exe")
        .arg("/K")
        .current_dir(&dir)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map_err(|e| format!("open terminal failed: {e}"))?;
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

#[cfg(all(test, target_os = "windows"))]
mod win_tests {
    use super::sanitize_name;

    #[test]
    fn accepts_plain_names_and_trims() {
        assert_eq!(sanitize_name("notes.txt").unwrap(), "notes.txt");
        assert_eq!(sanitize_name("  spaced.md  ").unwrap(), "spaced.md");
        assert_eq!(sanitize_name("My Folder").unwrap(), "My Folder");
        assert_eq!(sanitize_name("archive.tar.gz").unwrap(), "archive.tar.gz");
        assert_eq!(sanitize_name(".gitignore").unwrap(), ".gitignore");
        assert_eq!(sanitize_name("Über.txt").unwrap(), "Über.txt");
    }

    #[test]
    fn rejects_empty_or_whitespace() {
        assert!(sanitize_name("").is_err());
        assert!(sanitize_name("   ").is_err());
        assert!(sanitize_name("\t").is_err());
    }

    #[test]
    fn rejects_reserved_windows_chars_so_creation_cant_escape() {
        assert!(sanitize_name("a\\b").is_err()); // backslash separator
        assert!(sanitize_name("a/b").is_err()); // forward slash
        assert!(sanitize_name("C:evil").is_err()); // drive/stream colon
        assert!(sanitize_name("na*me").is_err());
        assert!(sanitize_name("na?me").is_err());
        assert!(sanitize_name(r#"na"me"#).is_err());
        assert!(sanitize_name("na<me").is_err());
        assert!(sanitize_name("na>me").is_err());
        assert!(sanitize_name("na|me").is_err());
    }

    #[test]
    fn rejects_dot_and_dotdot_and_nul() {
        assert!(sanitize_name(".").is_err());
        assert!(sanitize_name("..").is_err());
        assert!(sanitize_name("  ..  ").is_err());
        assert!(sanitize_name("evil\0name").is_err());
    }
}

#[cfg(test)]
mod quote_tests {
    use super::{osa_escape, sh_squote};

    #[test]
    fn sh_squote_wraps_and_escapes() {
        assert_eq!(sh_squote("/Users/martin"), "'/Users/martin'");
        // Spaces are handled by the surrounding quotes (Google Drive case).
        assert_eq!(sh_squote("/Users/martin/My Drive"), "'/Users/martin/My Drive'");
        // An embedded single quote → '\'' so the shell sees a literal '.
        assert_eq!(sh_squote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn osa_escape_escapes_backslash_and_quote() {
        assert_eq!(osa_escape("cd '/x'"), "cd '/x'");
        assert_eq!(osa_escape(r#"a"b"#), "a\\\"b");
        assert_eq!(osa_escape(r"a\b"), "a\\\\b");
        // Order matters: backslash first, then quote.
        assert_eq!(osa_escape(r#"\""#), "\\\\\\\"");
    }
}

#[cfg(test)]
mod title_parse_tests {
    use super::active_tab_name_from_title;

    #[test]
    fn win11_german_multi_tab_en_dash() {
        // Real-world title observed on Win11 DE with 3 tabs, Desktop active.
        let title = "Desktop und 2 weitere Registerkarten \u{2013} Explorer";
        assert_eq!(
            active_tab_name_from_title(title),
            Some("Desktop".to_string())
        );
    }

    #[test]
    fn win11_german_single_tab_en_dash() {
        let title = "Downloads \u{2013} Explorer";
        assert_eq!(
            active_tab_name_from_title(title),
            Some("Downloads".to_string())
        );
    }

    #[test]
    fn win11_english_multi_tab() {
        let title = "Documents and 3 more tabs \u{2013} File Explorer";
        assert_eq!(
            active_tab_name_from_title(title),
            Some("Documents".to_string())
        );
    }

    #[test]
    fn win10_ascii_hyphen() {
        let title = "Downloads - File Explorer";
        assert_eq!(
            active_tab_name_from_title(title),
            Some("Downloads".to_string())
        );
    }

    #[test]
    fn win11_singular_registerkarte() {
        // 1 additional tab: "Registerkarte" (singular)
        let title = "Projekte und 1 weitere Registerkarte \u{2013} Explorer";
        assert_eq!(
            active_tab_name_from_title(title),
            Some("Projekte".to_string())
        );
    }

    #[test]
    fn folder_name_with_spaces() {
        let title = "My Documents \u{2013} Explorer";
        assert_eq!(
            active_tab_name_from_title(title),
            Some("My Documents".to_string())
        );
    }

    #[test]
    fn datei_explorer_suffix() {
        let title = "Downloads \u{2013} Datei-Explorer";
        assert_eq!(
            active_tab_name_from_title(title),
            Some("Downloads".to_string())
        );
    }

    #[test]
    fn returns_none_for_unparseable() {
        assert_eq!(active_tab_name_from_title(""), None);
        assert_eq!(active_tab_name_from_title("Some Random Window"), None);
    }
}
