//! Spotlight-like app launcher — discover, search, launch installed
//! macOS apps from the popup search bar.
//!
//! ## Discovery
//!
//! Walks the four standard macOS app directories on the user's machine
//! at startup (one-shot; a Settings → Apps refresh button re-scans):
//!
//!   - `/Applications`
//!   - `~/Applications`
//!   - `/System/Applications`
//!   - `/System/Applications/Utilities`
//!
//! Top-level only (no recursion past `*.app`). Display name = the
//! `.app` directory's filename without the suffix, which matches the
//! Info.plist `CFBundleDisplayName` for ~99 % of installed apps.
//! Parsing Info.plist for the remaining ~1 % (weird Adobe names, …)
//! is left for a follow-up.
//!
//! ## Launching
//!
//! `/usr/bin/open <path-to-.app>`. Activates the existing instance if
//! the app is already running; otherwise starts a fresh one. Standard
//! macOS app-launch UX; no extra TCC permission needed.
//!
//! ## Icons
//!
//! Lazy: only the currently-selected app row in the popup requests
//! its icon (via `get_app_icon` IPC). Cached after the first render
//! so re-selecting the same app is instant. Each icon is generated
//! via `sips -s format png <Resources/*.icns> --out <tmp>.png`,
//! base64-encoded for the React `<img src="data:image/png;base64,…">`.
//! ~50 ms cold per icon; cache turns subsequent loads into a HashMap
//! lookup.

use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use parking_lot::Mutex;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// One installed app surface-able in the popup.
#[derive(Clone, Debug, Serialize)]
pub struct AppEntry {
    /// Display name (filesystem stem of the `.app` bundle, e.g. `Safari`).
    pub name: String,
    /// Absolute path to the `.app` bundle. Used for launching + icon
    /// extraction; also doubles as a stable id for the React row key.
    pub path: String,
    /// Lower-case cached copy of `name` — used by the frontend's
    /// fuzzy matcher so we don't pay a `.toLowerCase()` per keystroke
    /// × per app.
    pub name_lower: String,
}

/// Bounded LRU cache for icon PNGs. v0.37.1+ — was an unbounded
/// HashMap pre-0.37.1, which would (theoretically) grow indefinitely
/// as the user navigated through hundreds of apps. Cap at 100 entries
/// (≈500 KB of base64 PNG data); evict oldest insertion order on
/// overflow. Insertion order is FIFO not strict-LRU because tracking
/// access requires reshuffling per lookup — overkill for the size +
/// access frequency we see.
pub struct IconCache {
    map: std::collections::HashMap<String, String>,
    order: std::collections::VecDeque<String>,
    cap: usize,
}

impl IconCache {
    pub fn new(cap: usize) -> Self {
        Self {
            map: std::collections::HashMap::with_capacity(cap),
            order: std::collections::VecDeque::with_capacity(cap),
            cap,
        }
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.map.get(key)
    }

    #[allow(clippy::map_entry)]
    pub fn insert(&mut self, key: String, value: String) {
        if self.map.contains_key(&key) {
            // Refresh existing entry — overwrite value (in case a
            // refresh_apps cleared+refilled at the same path) but
            // keep its position in the insertion order.
            self.map.insert(key, value);
            return;
        }
        if self.order.len() >= self.cap {
            if let Some(evicted) = self.order.pop_front() {
                self.map.remove(&evicted);
            }
        }
        self.order.push_back(key.clone());
        self.map.insert(key, value);
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }
}

/// Tauri-managed cache of every installed app + their (lazily filled)
/// icons. All access serialised behind `parking_lot::Mutex` — the
/// scan happens once at startup, lookups are read-only after that.
pub struct AppIndex {
    pub apps: Mutex<Vec<AppEntry>>,
    /// path → base64 PNG. Filled lazily by `get_app_icon` IPC.
    /// Bounded at 100 entries (~500 KB) via the [`IconCache`] LRU.
    pub icons: Mutex<IconCache>,
}

impl Default for AppIndex {
    fn default() -> Self {
        Self {
            apps: Mutex::new(Vec::new()),
            icons: Mutex::new(IconCache::new(100)),
        }
    }
}

#[cfg(target_os = "macos")]
const APP_DIRS: &[&str] = &[
    "/Applications",
    "/System/Applications",
    "/System/Applications/Utilities",
    // ~/Applications added at runtime via dirs::home_dir() below.
];

/// Walk the standard macOS app dirs and collect every `.app` bundle.
/// Sorted alphabetically (case-insensitive) so the cached list reads
/// predictably in tests and in the Settings UI.
#[cfg(target_os = "macos")]
pub fn scan() -> Vec<AppEntry> {
    let mut dirs: Vec<PathBuf> = APP_DIRS.iter().map(PathBuf::from).collect();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Applications"));
    }

    let mut found: Vec<AppEntry> = Vec::new();
    for dir in dirs {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue, // ~/Applications often doesn't exist
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // `.app` is a directory (bundle) on macOS, not a file —
            // is_dir() + extension == "app" is the canonical test.
            if path.extension().and_then(|s| s.to_str()) != Some("app") {
                continue;
            }
            if !path.is_dir() {
                continue;
            }
            let name = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let path_str = path.to_string_lossy().into_owned();
            let name_lower = name.to_lowercase();
            found.push(AppEntry {
                name,
                path: path_str,
                name_lower,
            });
        }
    }

    // Dedupe by path (in case a dir appears twice — unlikely but
    // defensive against symlink loops or future config additions).
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.dedup_by(|a, b| a.path == b.path);

    // Final ordering: alphabetical by lowercase display name. Stable
    // sort so the Settings list is predictable.
    found.sort_by(|a, b| a.name_lower.cmp(&b.name_lower));
    found
}

#[cfg(target_os = "linux")]
pub fn scan() -> Vec<AppEntry> {
    linux::scan()
}

#[cfg(target_os = "windows")]
pub fn scan() -> Vec<AppEntry> {
    win::scan()
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn scan() -> Vec<AppEntry> {
    // No launcher on other platforms — an empty list means the frontend's
    // `appEntry` useMemo never surfaces a row (no UI breakage).
    Vec::new()
}

/// Launch the app at `path` via `/usr/bin/open`. If the app is
/// already running, `open` activates the existing instance instead
/// of starting a duplicate (standard macOS Launch Services behaviour).
#[cfg(target_os = "macos")]
pub fn launch(path: &Path) -> Result<()> {
    let status = std::process::Command::new("/usr/bin/open")
        .arg(path)
        .status()
        .with_context(|| format!("/usr/bin/open {}", path.display()))?;
    if !status.success() {
        return Err(anyhow!(
            "/usr/bin/open exited {:?} for {}",
            status.code(),
            path.display()
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn launch(path: &Path) -> Result<()> {
    linux::launch(path)
}

#[cfg(target_os = "windows")]
pub fn launch(path: &Path) -> Result<()> {
    win::launch(path)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn launch(_path: &Path) -> Result<()> {
    Err(anyhow!("app launching not implemented on this platform"))
}

// ── Pure `.desktop` parsers (Linux; unit-tested) ─────────────────────────────

/// Parse a `.desktop` file's `[Desktop Entry]` into `(Name, should_show)`.
/// `should_show` is true only for `Type=Application` entries that are neither
/// `Hidden=true` nor `NoDisplay=true`. Localised `Name[xx]=` keys are ignored
/// (we keep the unlocalised `Name=`). Pure → unit-tested without a filesystem.
#[cfg(any(target_os = "linux", test))]
pub fn parse_desktop_entry(contents: &str) -> Option<(String, bool)> {
    let mut in_entry = false;
    let mut name: Option<String> = None;
    let mut is_application = false;
    let mut hidden = false;
    let mut nodisplay = false;
    for line in contents.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_entry = l == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some(v) = l.strip_prefix("Name=") {
            if name.is_none() {
                name = Some(v.trim().to_string());
            }
        } else if let Some(v) = l.strip_prefix("Type=") {
            is_application = v.trim() == "Application";
        } else if let Some(v) = l.strip_prefix("NoDisplay=") {
            nodisplay = v.trim().eq_ignore_ascii_case("true");
        } else if let Some(v) = l.strip_prefix("Hidden=") {
            hidden = v.trim().eq_ignore_ascii_case("true");
        }
    }
    let name = name?;
    Some((name, is_application && !hidden && !nodisplay))
}

/// Extract the `Exec=` command from a `.desktop` file, stripping the `%U`/`%f`/…
/// field codes. Used as the fallback when `gtk-launch` is unavailable.
#[cfg(any(target_os = "linux", test))]
pub fn parse_desktop_exec(contents: &str) -> Option<String> {
    let mut in_entry = false;
    for line in contents.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_entry = l == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some(v) = l.strip_prefix("Exec=") {
            let cleaned = v
                .split_whitespace()
                .filter(|tok| !(tok.len() == 2 && tok.starts_with('%')))
                .collect::<Vec<_>>()
                .join(" ");
            return Some(cleaned.trim().to_string());
        }
    }
    None
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{parse_desktop_entry, parse_desktop_exec, AppEntry};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    /// XDG application directories, user-first so a user override shadows the
    /// system entry of the same desktop-file id.
    fn app_dirs() -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        let user = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")));
        if let Some(u) = user {
            dirs.push(u.join("applications"));
        }
        if let Some(h) = dirs::home_dir() {
            dirs.push(h.join(".local/share/flatpak/exports/share/applications"));
        }
        let data_dirs =
            std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        for d in data_dirs.split(':').filter(|s| !s.is_empty()) {
            dirs.push(PathBuf::from(d).join("applications"));
        }
        dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
        dirs
    }

    pub fn scan() -> Vec<AppEntry> {
        let mut found: Vec<AppEntry> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for dir in app_dirs() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
                    continue;
                }
                let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
                    continue;
                };
                // First occurrence wins (user dir precedes system dirs).
                if !seen.insert(id) {
                    continue;
                }
                let Ok(contents) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Some((name, show)) = parse_desktop_entry(&contents) else {
                    continue;
                };
                if !show {
                    continue;
                }
                found.push(AppEntry {
                    name_lower: name.to_lowercase(),
                    name,
                    path: path.to_string_lossy().into_owned(),
                });
            }
        }
        found.sort_by(|a, b| a.name_lower.cmp(&b.name_lower));
        found
    }

    pub fn launch(path: &Path) -> anyhow::Result<()> {
        // Prefer `gtk-launch <id>` — it honours Exec field codes, Terminal=true,
        // and DBus activation. Fall back to parsing Exec ourselves.
        if let Some(id) = path.file_stem().and_then(|s| s.to_str()) {
            if let Ok(status) = std::process::Command::new("gtk-launch").arg(id).status() {
                if status.success() {
                    return Ok(());
                }
            }
        }
        let contents = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
        let exec = parse_desktop_exec(&contents)
            .ok_or_else(|| anyhow::anyhow!("no Exec in {}", path.display()))?;
        let mut parts = exec.split_whitespace();
        let program = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("empty Exec line"))?;
        std::process::Command::new(program)
            .args(parts)
            .spawn()
            .map_err(|e| anyhow::anyhow!("launch {program}: {e}"))?;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod win {
    use super::AppEntry;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    fn start_menu_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Ok(pd) = std::env::var("ProgramData") {
            dirs.push(PathBuf::from(pd).join(r"Microsoft\Windows\Start Menu\Programs"));
        }
        if let Ok(ad) = std::env::var("APPDATA") {
            dirs.push(PathBuf::from(ad).join(r"Microsoft\Windows\Start Menu\Programs"));
        }
        dirs
    }

    fn walk(dir: &Path, out: &mut Vec<AppEntry>, seen: &mut HashSet<String>, depth: usize) {
        if depth > 6 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out, seen, depth + 1);
                continue;
            }
            let is_lnk = path
                .extension()
                .and_then(|s| s.to_str())
                .map(|e| e.eq_ignore_ascii_case("lnk"))
                == Some(true);
            if !is_lnk {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
                continue;
            };
            let key = name.to_lowercase();
            if !seen.insert(key.clone()) {
                continue;
            }
            out.push(AppEntry {
                name_lower: key,
                name,
                path: path.to_string_lossy().into_owned(),
            });
        }
    }

    pub fn scan() -> Vec<AppEntry> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for dir in start_menu_dirs() {
            walk(&dir, &mut out, &mut seen, 0);
        }
        out.sort_by(|a, b| a.name_lower.cmp(&b.name_lower));
        out
    }

    pub fn launch(path: &Path) -> anyhow::Result<()> {
        use std::os::windows::process::CommandExt;
        // `cmd /C start "" <lnk>` — the shell resolves the shortcut and launches
        // its target. The empty "" is the (required) window-title argument.
        let status = std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .status()
            .map_err(|e| anyhow::anyhow!("cmd start failed: {e}"))?;
        if !status.success() {
            return Err(anyhow::anyhow!("launch exited {:?}", status.code()));
        }
        Ok(())
    }
}

/// Generate a base64-encoded PNG of the app's icon, sized 128×128.
/// Strategy:
///   1. Find a `.icns` in `<app>/Contents/Resources/` — prefer files
///      matching `CFBundleIconFile` (Info.plist), fall back to the
///      first `.icns` (~99 % have exactly one).
///   2. Shell out to `sips -s format png` to decode `.icns` → PNG.
///      `sips` is bundled with every macOS, fast (~50 ms), and
///      handles every `.icns` format variant cleanly.
///   3. Read the result, base64-encode, return.
///
/// Cached upstream by [`get_app_icon`] in `commands.rs`.
#[cfg(target_os = "macos")]
pub fn icon_png_base64(app_path: &Path) -> Result<String> {
    let resources = app_path.join("Contents").join("Resources");
    if !resources.is_dir() {
        return Err(anyhow!("no Contents/Resources in {}", app_path.display()));
    }

    // Try Info.plist's CFBundleIconFile first — handles apps like
    // Adobe that put multiple .icns files in Resources and name the
    // canonical one explicitly. Falls through to glob if missing or
    // parse fails (don't bring down icon rendering for one parse hiccup).
    let icns_path = info_plist_icon(app_path)
        .or_else(|| first_icns_in(&resources))
        .ok_or_else(|| anyhow!("no .icns found in {}", resources.display()))?;

    // sips writes to a temp file; we read + delete + base64. Per-call
    // unique suffix via an AtomicUsize counter — pre-v0.37.1 used only
    // PID, so two concurrent `get_app_icon` IPCs (user scrolls list
    // quickly, multiple rows lazy-load in parallel) both wrote to the
    // *same* path → race → last writer wins → wrong icon cached.
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!(
        "inspector-rust-appicon-{}-{}.png",
        std::process::id(),
        seq
    ));
    let status = std::process::Command::new("/usr/bin/sips")
        .args([
            "-s",
            "format",
            "png",
            "-z",
            "128",
            "128", // target size to keep base64 small
        ])
        .arg(&icns_path)
        .arg("--out")
        .arg(&tmp)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("sips spawn")?;
    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(anyhow!("sips failed ({}): {}", status, icns_path.display()));
    }

    let bytes = std::fs::read(&tmp)
        .with_context(|| format!("read sips output {}", tmp.display()))?;
    let _ = std::fs::remove_file(&tmp);
    Ok(B64.encode(&bytes))
}

#[cfg(not(target_os = "macos"))]
pub fn icon_png_base64(_app_path: &Path) -> Result<String> {
    Err(anyhow!("icon extraction: only macOS implemented in v0.37"))
}

/// Best-effort Info.plist parse to find `CFBundleIconFile`. Uses
/// `plutil -convert json -o -` to avoid a plist crate dependency.
/// Returns `None` (not Err) on any parse failure — the caller falls
/// back to a glob-the-Resources-dir strategy that works for almost
/// every app.
#[cfg(target_os = "macos")]
fn info_plist_icon(app_path: &Path) -> Option<PathBuf> {
    let info = app_path.join("Contents").join("Info.plist");
    if !info.is_file() {
        return None;
    }
    let output = std::process::Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(&info)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let icon = json.get("CFBundleIconFile")?.as_str()?.to_string();
    // CFBundleIconFile may or may not include the .icns extension —
    // canonicalise.
    let with_ext = if icon.ends_with(".icns") {
        icon
    } else {
        format!("{icon}.icns")
    };
    let candidate = app_path.join("Contents").join("Resources").join(with_ext);
    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}

/// Find the first `.icns` in a Resources dir. Slow path: only runs
/// when `info_plist_icon` returned None. macOS app conventions almost
/// always put exactly one `.icns` here, so first-match is reliable.
#[cfg(target_os = "macos")]
fn first_icns_in(resources: &Path) -> Option<PathBuf> {
    std::fs::read_dir(resources)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|s| s.to_str()) == Some("icns"))
}

#[cfg(test)]
mod desktop_parser_tests {
    use super::{parse_desktop_entry, parse_desktop_exec};

    #[test]
    fn parses_a_normal_application_entry() {
        let s = "[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox %u\n";
        assert_eq!(parse_desktop_entry(s), Some(("Firefox".to_string(), true)));
        assert_eq!(parse_desktop_exec(s).as_deref(), Some("firefox"));
    }

    #[test]
    fn hidden_and_nodisplay_are_not_shown() {
        let hidden = "[Desktop Entry]\nType=Application\nName=Bg\nHidden=true\n";
        assert_eq!(parse_desktop_entry(hidden), Some(("Bg".to_string(), false)));
        let nd = "[Desktop Entry]\nType=Application\nName=Bg\nNoDisplay=True\n";
        assert_eq!(parse_desktop_entry(nd), Some(("Bg".to_string(), false)));
    }

    #[test]
    fn non_application_types_are_not_shown() {
        let dir = "[Desktop Entry]\nType=Directory\nName=Stuff\n";
        assert_eq!(parse_desktop_entry(dir), Some(("Stuff".to_string(), false)));
    }

    #[test]
    fn ignores_localised_name_keys_and_other_sections() {
        let s = "[Desktop Entry]\nType=Application\nName=Editor\nName[de]=Editor DE\nExec=editor %F\n\
                 [Desktop Action new]\nName=New Window\nExec=other-binary --new %F\n";
        // The unlocalised Name + the entry's own Exec win; the action section
        // (its Name and Exec) is ignored.
        assert_eq!(parse_desktop_entry(s), Some(("Editor".to_string(), true)));
        assert_eq!(parse_desktop_exec(s).as_deref(), Some("editor"));
    }

    #[test]
    fn strips_multiple_field_codes_from_exec() {
        let s = "[Desktop Entry]\nType=Application\nName=X\nExec=/usr/bin/x -p %U %i %c\n";
        assert_eq!(parse_desktop_exec(s).as_deref(), Some("/usr/bin/x -p"));
    }

    #[test]
    fn missing_name_yields_none() {
        let s = "[Desktop Entry]\nType=Application\nExec=foo\n";
        assert_eq!(parse_desktop_entry(s), None);
    }
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use super::*;

    #[test]
    fn scan_returns_at_least_a_few_system_apps() {
        // /System/Applications/Utilities/Terminal.app is on every Mac.
        // If scan() doesn't find it our discovery is broken.
        let apps = scan();
        assert!(!apps.is_empty(), "scan should find at least some apps");
        let terminal = apps.iter().any(|a| a.name == "Terminal");
        assert!(terminal, "expected to find Terminal among {} apps", apps.len());
    }

    #[test]
    fn scan_produces_lowercased_name_for_matching() {
        let apps = scan();
        for app in apps.iter().take(5) {
            assert_eq!(app.name_lower, app.name.to_lowercase());
        }
    }

    #[test]
    fn scan_results_are_alphabetically_sorted() {
        let apps = scan();
        let names: Vec<&str> = apps.iter().map(|a| a.name_lower.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn scan_results_are_deduped_by_path() {
        let apps = scan();
        let mut paths: Vec<&String> = apps.iter().map(|a| &a.path).collect();
        let before = paths.len();
        paths.sort();
        paths.dedup();
        let after = paths.len();
        assert_eq!(before, after, "path dedup invariant broken");
    }

    #[test]
    fn icon_cache_evicts_oldest_at_cap() {
        let mut c = IconCache::new(3);
        c.insert("a".into(), "A".into());
        c.insert("b".into(), "B".into());
        c.insert("c".into(), "C".into());
        assert_eq!(c.get("a").map(String::as_str), Some("A"));
        // 4th insertion evicts "a" (oldest).
        c.insert("d".into(), "D".into());
        assert_eq!(c.get("a"), None);
        assert_eq!(c.get("d").map(String::as_str), Some("D"));
        assert_eq!(c.get("b").map(String::as_str), Some("B"));
    }

    #[test]
    fn icon_cache_overwrite_keeps_order_position() {
        let mut c = IconCache::new(3);
        c.insert("a".into(), "A1".into());
        c.insert("b".into(), "B".into());
        c.insert("c".into(), "C".into());
        // Overwrite "a" — should update value but NOT reposition,
        // so the next insertion still evicts "a".
        c.insert("a".into(), "A2".into());
        assert_eq!(c.get("a").map(String::as_str), Some("A2"));
        c.insert("d".into(), "D".into());
        // "a" was oldest by insertion-order; eviction took it.
        assert_eq!(c.get("a"), None);
    }

    #[test]
    fn icon_cache_clear_empties_both_structures() {
        let mut c = IconCache::new(3);
        c.insert("a".into(), "A".into());
        c.insert("b".into(), "B".into());
        c.clear();
        assert_eq!(c.get("a"), None);
        assert_eq!(c.get("b"), None);
        // Inserting after clear works as fresh.
        c.insert("z".into(), "Z".into());
        assert_eq!(c.get("z").map(String::as_str), Some("Z"));
    }
}
