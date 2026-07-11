//! Cleaning workflow — delete unimportant cache / log / temp files (v0.60.0).
//!
//! **This deletes the user's files, so safety is the whole design.** The
//! guarantees, in order of importance:
//!
//! 1. **Strict allowlist, never a blocklist.** Deletion only ever happens
//!    *inside* a small set of hard-coded, well-known cache/log/temp roots per
//!    OS (`categories()`). Never user documents / Desktop / Pictures.
//! 2. **Canonicalisation + containment check before every delete.** A path is
//!    only removed if its canonical form is genuinely under one of the allowed
//!    roots. Symlinks are **never followed** (we `lstat` and skip them) so a
//!    symlink inside a cache dir can't be used to escape the allowlist.
//! 3. **Dry-run first.** `scan` is purely read-only and returns a `CleanPlan`.
//!    Nothing is deleted until `execute(plan)` is called with an explicit
//!    plan — and `execute` **re-validates** every path against the allowlist
//!    a second time (TOCTOU-resistant: a path that no longer canonicalises
//!    under an allowed root is skipped).
//! 4. **Conservative, opt-in levels.** `Safe` (default) → `Standard` →
//!    `Aggressive`; each level only touches an explicitly listed set of
//!    categories. Age threshold (only files older than N days) is applied too.
//!
//! The pure core (`scan_roots`, `execute_plan`, `is_contained`) takes explicit
//! roots so it can be exhaustively unit-tested against temp fixtures — **no
//! test ever touches a real user path.**

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use crate::db::DbHandle;

// ── Levels + categories ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// App-own cache + OS temp older than the threshold. The default.
    Safe,
    /// + browser caches (not cookies/logins) + known log dirs.
    Standard,
    /// + global dev tool caches (npm/pnpm/gradle/…). Re-download cost.
    Aggressive,
}

impl Level {
    pub fn from_str_loose(s: &str) -> Level {
        match s.trim().to_ascii_lowercase().as_str() {
            "standard" => Level::Standard,
            "aggressive" => Level::Aggressive,
            _ => Level::Safe,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Safe => "safe",
            Level::Standard => "standard",
            Level::Aggressive => "aggressive",
        }
    }
    /// Is `cat_level` included when the user picked `self`?
    fn includes(self, cat_level: Level) -> bool {
        let rank = |l: Level| match l {
            Level::Safe => 0,
            Level::Standard => 1,
            Level::Aggressive => 2,
        };
        rank(cat_level) <= rank(self)
    }
}

/// One cleaning category: a stable key, a human label, the level at which it
/// becomes eligible, and the concrete on-disk roots it owns on this OS.
#[derive(Debug, Clone, Serialize)]
pub struct Category {
    pub key: String,
    pub label: String,
    pub level: Level,
    /// Absolute roots. Empty (e.g. a dir that doesn't exist on this machine)
    /// → the category simply contributes nothing.
    pub roots: Vec<PathBuf>,
    /// Subtrees under `roots` this category does NOT own — used by broad
    /// categories (e.g. all of `~/Library/Caches`) to carve out the roots that
    /// belong to more specific categories, so no file is ever claimed (or
    /// counted) twice. Excluded subtrees are neither recursed nor deleted.
    #[serde(skip)]
    pub exclude: Vec<PathBuf>,
    /// Lower-case extension filter (empty = every file) — e.g. the
    /// old-installers category only touches `dmg`/`pkg`/`iso`/….
    #[serde(skip)]
    pub exts: Vec<String>,
    /// Whether it's on by default (the user can still uncheck it).
    pub default_enabled: bool,
}

fn home() -> Option<PathBuf> {
    dirs::home_dir()
}

/// The regenerable cache subdirs of the Electron-based editors (VS Code /
/// Cursor / VSCodium) under `base` — the platform's app-data dir
/// (`~/Library/Application Support` / `%APPDATA%` / `~/.config`), where
/// Electron caches live (NOT the OS cache dir, so the broad cache category
/// never sees them). Index/GPU/renderer caches only — settings, extensions
/// and state are never listed.
fn editor_cache_roots(base: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for app in ["Code", "Cursor", "VSCodium"] {
        for sub in [
            "Cache",
            "CachedData",
            "Code Cache",
            "GPUCache",
            "DawnGraphiteCache",
            "DawnWebGPUCache",
            "CachedExtensionVSIXs",
            "Service Worker/CacheStorage",
            "Service Worker/ScriptCache",
        ] {
            out.push(base.join(app).join(sub));
        }
    }
    out
}

/// The hard-coded category → roots map for this OS. **This is the allowlist.**
/// Only paths under these roots can ever be deleted. Roots that don't exist on
/// the current machine are still listed (harmless — scanning a missing root
/// yields nothing).
pub fn categories() -> Vec<Category> {
    let mut out: Vec<Category> = Vec::new();
    let cache = dirs::cache_dir();
    let tmp = std::env::temp_dir();

    // Always-safe: our own app cache.
    if let Some(c) = cache.as_ref() {
        out.push(Category {
            key: "app_cache".into(),
            label: "Inspector Rust cache".into(),
            level: Level::Safe,
            roots: vec![c.join("InspectorRust")],
            exclude: vec![],
            exts: vec![],
            default_enabled: true,
        });
    }
    // OS temp (this is std::env::temp_dir — /tmp-equivalent / %TEMP%).
    out.push(Category {
        key: "os_temp".into(),
        label: "OS temporary files".into(),
        level: Level::Safe,
        roots: vec![tmp],
        exclude: vec![],
        exts: vec![],
        default_enabled: true,
    });

    // Downloads-based categories (every platform; v0.84.243). These touch USER
    // files, so the clean picker pre-deselects them (frontend `PRESELECT_OFF`)
    // — they're offered, never silently included.
    if let Some(dl) = dirs::download_dir() {
        // Old installers: re-downloadable by definition; the age filter applies
        // (only installers untouched for ≥ N days).
        out.push(Category {
            key: "installers".into(),
            label: "Old installers in Downloads (dmg / pkg / iso / …)".into(),
            level: Level::Standard,
            roots: vec![dl.clone()],
            exclude: vec![],
            exts: ["dmg", "pkg", "mpkg", "iso", "xip", "msi", "appimage"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            default_enabled: true,
        });
        // Content-identical duplicates — scanned by `append_duplicates`, NOT the
        // generic walker (the key is special-cased in `scan`); the OLDEST copy
        // of every duplicate group is always kept.
        out.push(Category {
            key: KEY_DUPES.into(),
            label: "Duplicate files in Downloads (oldest copy kept)".into(),
            level: Level::Standard,
            roots: vec![dl],
            exclude: vec![],
            exts: vec![],
            default_enabled: true,
        });
    }

    // Docker build cache — command-based, every platform (see KEY_DOCKER).
    // No file roots: sized via `docker system df`, freed via
    // `docker builder prune -f`. Contributes nothing when docker is missing
    // or the daemon isn't running. Pre-deselected in the picker (rebuilds
    // get slower until the cache re-populates).
    out.push(Category {
        key: KEY_DOCKER.into(),
        label: "Docker build cache (docker builder prune)".into(),
        level: Level::Standard,
        roots: vec![],
        exclude: vec![],
        exts: vec![],
        default_enabled: true,
    });

    #[cfg(target_os = "macos")]
    if let Some(h) = home() {
        let browser_roots = vec![
            h.join("Library/Caches/com.apple.Safari"),
            h.join("Library/Caches/Google/Chrome"),
            h.join("Library/Caches/com.google.Chrome"),
            h.join("Library/Caches/Firefox"),
            h.join("Library/Caches/com.microsoft.edgemac"),
        ];
        out.push(Category {
            key: "browser_cache".into(),
            label: "Browser caches".into(),
            level: Level::Standard,
            roots: browser_roots.clone(),
            exclude: vec![],
            exts: vec![],
            default_enabled: true,
        });
        // The big one (v0.84.241): everything else under ~/Library/Caches —
        // per Apple's guidelines strictly regenerable data, and routinely the
        // largest reclaimable tree on a Mac (Homebrew, pip, go-build, Yarn,
        // Playwright, Electron apps, …). The browser + our own roots are
        // carved out so no file is claimed twice.
        out.push(Category {
            key: "other_caches".into(),
            label: "Other app caches (~/Library/Caches)".into(),
            level: Level::Standard,
            roots: vec![h.join("Library/Caches")],
            exclude: {
                let mut ex = browser_roots;
                ex.push(h.join("Library/Caches/InspectorRust"));
                ex
            },
            exts: vec![],
            default_enabled: true,
        });
        out.push(Category {
            key: "logs".into(),
            label: "Application logs".into(),
            level: Level::Standard,
            // /Library/Logs (system-wide app logs) too — root-owned files in
            // there simply fail the per-item delete and are recorded, never
            // aborting the batch. Plus npm's + the Electron editors' log dirs
            // (they log outside ~/Library/Logs).
            roots: vec![
                h.join("Library/Logs"),
                h.join(".pm2/logs"),
                PathBuf::from("/Library/Logs"),
                h.join(".npm/_logs"),
                h.join("Library/Application Support/Code/logs"),
                h.join("Library/Application Support/Cursor/logs"),
            ],
            exclude: vec![],
            exts: vec![],
            default_enabled: true,
        });
        // Electron editors (VS Code / Cursor / VSCodium) cache under
        // Application Support, NOT ~/Library/Caches — so the broad category
        // never sees them. Index/GPU/renderer caches only; settings,
        // extensions and state stay untouched.
        out.push(Category {
            key: "editor_caches".into(),
            label: "Editor caches (VS Code / Cursor)".into(),
            level: Level::Standard,
            roots: editor_cache_roots(&h.join("Library/Application Support")),
            exclude: vec![],
            exts: vec![],
            default_enabled: true,
        });
        out.push(Category {
            key: "dev_caches".into(),
            label: "Developer tool caches (npm / pnpm / Gradle / Maven / Cargo)".into(),
            level: Level::Aggressive,
            roots: vec![
                h.join(".npm/_cacache"),
                h.join("Library/pnpm/store"),
                h.join(".gradle/caches"),
                // Gradle daemon logs + re-downloadable wrapper distributions.
                h.join(".gradle/daemon"),
                h.join(".gradle/wrapper/dists"),
                // Maven's local repository — re-downloaded on demand.
                h.join(".m2/repository"),
                h.join(".cargo/registry/cache"),
                // The unpacked crate sources + git checkouts dwarf the .crate
                // cache itself; cargo re-extracts / re-clones on demand.
                h.join(".cargo/registry/src"),
                h.join(".cargo/git"),
                h.join(".rustup/downloads"),
                h.join(".rustup/tmp"),
                h.join(".android/cache"),
                h.join(".android/build-cache"),
            ],
            exclude: vec![],
            exts: vec![],
            default_enabled: false,
        });
        out.push(Category {
            key: "xcode_caches".into(),
            label: "Xcode caches (DerivedData / device support / simulators)".into(),
            level: Level::Aggressive,
            roots: vec![
                h.join("Library/Developer/Xcode/DerivedData"),
                h.join("Library/Developer/CoreSimulator/Caches"),
                // Per-iOS-version debug symbols, re-extracted on the next
                // device connect — routinely 5–20 GB of stale versions.
                h.join("Library/Developer/Xcode/iOS DeviceSupport"),
                h.join("Library/Developer/XCTestDevices"),
                h.join("Library/Developer/XCPGDevices"),
            ],
            exclude: vec![],
            exts: vec![],
            default_enabled: false,
        });
        // Trash: genuinely user-discarded files, but still user files — strictly
        // opt-in, and the age filter applies (only items trashed ≥ N days ago).
        out.push(Category {
            key: "trash".into(),
            label: "Trash (items older than the age filter)".into(),
            level: Level::Aggressive,
            roots: vec![h.join(".Trash")],
            exclude: vec![],
            exts: vec![],
            default_enabled: false,
        });
    }

    #[cfg(target_os = "windows")]
    if let Some(h) = home() {
        let local = dirs::data_local_dir().unwrap_or_else(|| h.join("AppData/Local"));
        out.push(Category {
            key: "browser_cache".into(),
            label: "Browser caches".into(),
            level: Level::Standard,
            roots: vec![
                local.join("Google/Chrome/User Data/Default/Cache"),
                local.join("Microsoft/Edge/User Data/Default/Cache"),
                local.join("Mozilla/Firefox/Profiles"),
            ],
            exclude: vec![],
            exts: vec![],
            default_enabled: true,
        });
        // Electron editors cache under %APPDATA% (Roaming), not the OS cache dir.
        if let Some(roaming) = dirs::config_dir() {
            out.push(Category {
                key: "editor_caches".into(),
                label: "Editor caches (VS Code / Cursor)".into(),
                level: Level::Standard,
                roots: editor_cache_roots(&roaming),
                exclude: vec![],
                exts: vec![],
                default_enabled: true,
            });
        }
        out.push(Category {
            key: "dev_caches".into(),
            label: "Developer tool caches (npm / pnpm / Gradle / Maven / Cargo / VS / NuGet)".into(),
            level: Level::Aggressive,
            roots: vec![
                h.join("AppData/Roaming/npm-cache/_cacache"),
                local.join("pnpm/store"),
                h.join(".gradle/caches"),
                h.join(".gradle/daemon"),
                h.join(".gradle/wrapper/dists"),
                h.join(".m2/repository"),
                h.join(".cargo/registry/cache"),
                h.join(".cargo/registry/src"),
                h.join(".cargo/git"),
                h.join(".rustup/downloads"),
                h.join(".rustup/tmp"),
                h.join(".android/cache"),
                h.join(".android/build-cache"),
                // NuGet HTTP + package caches (re-downloaded on demand).
                // NOTE: %LOCALAPPDATA%\Microsoft\VisualStudio is deliberately
                // NOT listed — its per-instance dirs mix caches with window
                // layouts/instance state, and the cache subdirs have
                // versioned instance names we can't target precisely.
                local.join("NuGet/v3-cache"),
                h.join(".nuget/packages"),
            ],
            exclude: vec![],
            exts: vec![],
            default_enabled: false,
        });
    }

    #[cfg(target_os = "linux")]
    if let Some(h) = home() {
        let xdg_cache = dirs::cache_dir().unwrap_or_else(|| h.join(".cache"));
        let browser_roots = vec![
            xdg_cache.join("google-chrome"),
            xdg_cache.join("chromium"),
            xdg_cache.join("mozilla"),
        ];
        out.push(Category {
            key: "browser_cache".into(),
            label: "Browser caches".into(),
            level: Level::Standard,
            roots: browser_roots.clone(),
            exclude: vec![],
            exts: vec![],
            default_enabled: true,
        });
        // Everything else under ~/.cache (XDG: strictly regenerable), minus the
        // more specific categories' roots.
        out.push(Category {
            key: "other_caches".into(),
            label: "Other app caches (~/.cache)".into(),
            level: Level::Standard,
            roots: vec![xdg_cache.clone()],
            exclude: {
                let mut ex = browser_roots;
                ex.push(xdg_cache.join("InspectorRust"));
                ex.push(xdg_cache.join("pnpm")); // owned by dev_caches
                ex
            },
            exts: vec![],
            default_enabled: true,
        });
        // Electron editors cache under ~/.config, not ~/.cache.
        if let Some(cfg_dir) = dirs::config_dir() {
            out.push(Category {
                key: "editor_caches".into(),
                label: "Editor caches (VS Code / Cursor)".into(),
                level: Level::Standard,
                roots: editor_cache_roots(&cfg_dir),
                exclude: vec![],
                exts: vec![],
                default_enabled: true,
            });
        }
        out.push(Category {
            key: "dev_caches".into(),
            label: "Developer tool caches (npm / pnpm / Gradle / Maven / Cargo)".into(),
            level: Level::Aggressive,
            roots: vec![
                h.join(".npm/_cacache"),
                xdg_cache.join("pnpm"),
                h.join(".gradle/caches"),
                h.join(".gradle/daemon"),
                h.join(".gradle/wrapper/dists"),
                h.join(".m2/repository"),
                h.join(".cargo/registry/cache"),
                h.join(".cargo/registry/src"),
                h.join(".cargo/git"),
                h.join(".rustup/downloads"),
                h.join(".rustup/tmp"),
                h.join(".android/cache"),
                h.join(".android/build-cache"),
            ],
            exclude: vec![],
            exts: vec![],
            default_enabled: false,
        });
        out.push(Category {
            key: "trash".into(),
            label: "Trash (items older than the age filter)".into(),
            level: Level::Aggressive,
            roots: vec![h.join(".local/share/Trash/files")],
            exclude: vec![],
            exts: vec![],
            default_enabled: false,
        });
    }

    out
}

// ── Config ───────────────────────────────────────────────────────────────

pub const KEY_LEVEL: &str = "cleaner.level";
pub const KEY_MIN_AGE: &str = "cleaner.min_age_days";
/// JSON map of `{ category_key: enabled }` overrides.
pub const KEY_CATEGORIES: &str = "cleaner.categories";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanerConfig {
    pub level: Level,
    pub min_age_days: u32,
    /// Per-category enable overrides (key → enabled). Missing key = the
    /// category's own `default_enabled`.
    pub categories: std::collections::BTreeMap<String, bool>,
}

impl Default for CleanerConfig {
    fn default() -> Self {
        CleanerConfig {
            level: Level::Safe,
            min_age_days: 7,
            categories: std::collections::BTreeMap::new(),
        }
    }
}

pub fn load_config(db: &DbHandle) -> CleanerConfig {
    let d = CleanerConfig::default();
    let level = crate::settings::get(db, KEY_LEVEL)
        .ok()
        .flatten()
        .map(|s| Level::from_str_loose(&s))
        .unwrap_or(d.level);
    let min_age_days = crate::settings::get(db, KEY_MIN_AGE)
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(d.min_age_days);
    let categories = crate::settings::get(db, KEY_CATEGORIES)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    CleanerConfig {
        level,
        min_age_days,
        categories,
    }
}

pub fn save_config(db: &DbHandle, cfg: &CleanerConfig) -> anyhow::Result<()> {
    crate::settings::set(db, KEY_LEVEL, cfg.level.as_str())?;
    crate::settings::set(db, KEY_MIN_AGE, &cfg.min_age_days.to_string())?;
    crate::settings::set(db, KEY_CATEGORIES, &serde_json::to_string(&cfg.categories)?)?;
    Ok(())
}

// ── Plan / result types ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanItem {
    pub path: String,
    pub size: u64,
    pub category: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CleanPlan {
    pub items: Vec<CleanItem>,
    pub total_bytes: u64,
    /// Categories that were scanned (key → human label), for the UI summary.
    pub categories: Vec<(String, String, u64)>, // (key, label, bytes)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CleanResult {
    pub deleted: usize,
    pub freed_bytes: u64,
    pub errors: Vec<String>,
}

// ── Safety primitive: containment ────────────────────────────────────────

/// True iff `path` is genuinely inside `root` after canonicalisation. Both
/// must exist (canonicalize fails otherwise → `false`, the safe default).
/// This is the gate every deletion passes through.
pub fn is_contained(path: &Path, root: &Path) -> bool {
    let (Ok(cp), Ok(cr)) = (path.canonicalize(), root.canonicalize()) else {
        return false;
    };
    cp.starts_with(&cr) && cp != cr
}

/// True iff `path` is contained in **any** of `roots`.
fn contained_in_any(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| is_contained(path, r))
}

// ── Pure scan / execute core (explicit roots → unit-testable) ────────────

/// Whether `path` sits at or under any of the `exclude` subtrees. Plain
/// prefix matching is correct here: both sides come from the same walk /
/// category construction (no symlinks were followed to reach `path`).
fn is_excluded(path: &Path, exclude: &[PathBuf]) -> bool {
    exclude.iter().any(|e| path.starts_with(e))
}

/// Recursively collect deletable files under `root`. Does **not** follow
/// symlinks (uses `symlink_metadata`); only regular files older than
/// `min_age` (by mtime) are included. Each file's path is containment-checked
/// against `root` so a symlinked subdir can't smuggle outside paths in.
/// Subtrees under `exclude` are neither recursed nor collected (they belong
/// to a more specific category).
fn collect_files(
    root: &Path,
    min_age: Duration,
    now: SystemTime,
    exclude: &[PathBuf],
    out: &mut Vec<(PathBuf, u64)>,
) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if is_excluded(&path, exclude) {
            continue; // another category's territory
        }
        // lstat — never follow symlinks.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            // Skip symlinks entirely (neither recurse nor delete).
            continue;
        }
        if meta.is_dir() {
            // Recurse. No canonicalise here (v0.84.244 perf): `path` is
            // `parent.join(name)` from read_dir and symlinked dirs were
            // already skipped via lstat above, so every recursed dir is
            // physically under `root`. The old per-dir `is_contained` cost
            // TWO full canonicalise syscall chains per directory — the
            // dominant scan cost on a 10k-dir cache tree. The security gate
            // stays at EXECUTE time, which canonicalises every deletion.
            collect_files(&path, min_age, now, exclude, out);
            continue;
        }
        if meta.is_file() {
            // Age filter by mtime; if mtime is unavailable, be conservative
            // and skip (don't delete files we can't age-check).
            let old_enough = match meta.modified() {
                Ok(mtime) => now
                    .duration_since(mtime)
                    .map(|age| age >= min_age)
                    .unwrap_or(false),
                Err(_) => false,
            };
            if old_enough {
                out.push((path, meta.len()));
            }
        }
    }
}

/// One scan group — the category fields the scanner needs.
#[derive(Debug, Clone)]
pub struct ScanGroup {
    pub key: String,
    pub label: String,
    pub roots: Vec<PathBuf>,
    pub exclude: Vec<PathBuf>,
    /// Lower-case extension filter (`["dmg", "pkg"]`); empty = every file.
    pub exts: Vec<String>,
}

/// Whether `path`'s extension passes the (possibly empty) filter.
fn ext_matches(path: &Path, exts: &[String]) -> bool {
    if exts.is_empty() {
        return true;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let e = e.to_ascii_lowercase();
            exts.iter().any(|want| *want == e)
        })
        .unwrap_or(false)
}

/// Build a plan from explicit scan groups. Read-only. The pure heart of
/// `scan` — tests drive it with temp dirs.
pub fn scan_roots(groups: &[ScanGroup], min_age_days: u32, now: SystemTime) -> CleanPlan {
    let min_age = Duration::from_secs(u64::from(min_age_days) * 86_400);
    let mut plan = CleanPlan::default();
    for ScanGroup { key, label, roots, exclude, exts } in groups {
        let mut cat_bytes = 0u64;
        for root in roots {
            let mut files = Vec::new();
            collect_files(root, min_age, now, exclude, &mut files);
            files.retain(|(p, _)| ext_matches(p, exts));
            for (path, size) in files {
                cat_bytes += size;
                plan.total_bytes += size;
                plan.items.push(CleanItem {
                    path: path.to_string_lossy().to_string(),
                    size,
                    category: key.clone(),
                });
            }
        }
        plan.categories.push((key.clone(), label.clone(), cat_bytes));
    }
    plan
}

/// Execute a plan: delete each item, but only after **re-validating** that it
/// is (still) a real file contained in `allowed_roots`, not a symlink, and not
/// inside a subtree its OWN category excluded (exclusions are per-category —
/// a specific category legitimately owns paths a broad one carved out). Any
/// item that fails re-validation is skipped and recorded in `errors`, never
/// aborting the batch.
pub fn execute_plan(
    plan: &CleanPlan,
    allowed_roots: &[PathBuf],
    excludes_by_cat: &std::collections::BTreeMap<String, Vec<PathBuf>>,
) -> CleanResult {
    let mut res = CleanResult::default();
    for item in &plan.items {
        let path = PathBuf::from(&item.path);
        // Re-validate: must exist, be a non-symlink regular file, and still be
        // contained in an allowed root.
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                res.errors.push(format!("{}: {e}", item.path));
                continue;
            }
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            res.errors.push(format!("{}: not a regular file (skipped)", item.path));
            continue;
        }
        let own_excludes = excludes_by_cat.get(&item.category).map(Vec::as_slice).unwrap_or(&[]);
        if !contained_in_any(&path, allowed_roots) || is_excluded(&path, own_excludes) {
            res.errors
                .push(format!("{}: outside allowlist (skipped)", item.path));
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => {
                res.deleted += 1;
                res.freed_bytes += item.size;
            }
            Err(e) => res.errors.push(format!("{}: {e}", item.path)),
        }
    }
    res
}

// ── Public production API (resolves categories from config) ──────────────

/// Categories enabled under `cfg` (level + per-category overrides), as
/// `(key, label, roots)` groups ready for [`scan_roots`].
fn enabled_groups(cfg: &CleanerConfig) -> Vec<ScanGroup> {
    categories()
        .into_iter()
        .filter(|c| cfg.level.includes(c.level))
        .filter(|c| *cfg.categories.get(&c.key).unwrap_or(&c.default_enabled))
        .map(|c| ScanGroup {
            key: c.key,
            label: c.label,
            roots: c.roots,
            exclude: c.exclude,
            exts: c.exts,
        })
        .collect()
}

/// All roots that the current config could touch — the allowlist passed to
/// `execute_plan` for re-validation.
fn enabled_roots(cfg: &CleanerConfig) -> Vec<PathBuf> {
    enabled_groups(cfg).into_iter().flat_map(|g| g.roots).collect()
}

/// Per-category exclusions of the enabled categories (execute-time re-check —
/// per-category, because a specific category legitimately owns paths a broad
/// one carved out).
fn enabled_excludes(cfg: &CleanerConfig) -> std::collections::BTreeMap<String, Vec<PathBuf>> {
    enabled_groups(cfg).into_iter().map(|g| (g.key, g.exclude)).collect()
}

/// The duplicate-finder category's key — special-cased in `scan` (its items
/// come from content hashing, not the generic walker).
pub const KEY_DUPES: &str = "dupes";

/// The Docker category's key — command-based, not file-based (v0.84.244).
/// Docker's images/volumes live inside ONE VM disk file; deleting files there
/// would destroy everything. The only safe reclaim is Docker's own
/// `docker builder prune` (build cache — exactly what `docker system df`
/// reports as reclaimable). Scan estimates via `system df`; execute runs the
/// prune. No file roots → the file allowlist is untouched.
pub const KEY_DOCKER: &str = "docker";

/// Parse docker's human size strings: "2.5GB", "512.3MB", "1.2kB", "0B".
/// Decimal units (docker uses SI). Unknown/garbage → 0.
pub fn parse_docker_size(s: &str) -> u64 {
    let s = s.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let Ok(v) = num.trim().parse::<f64>() else { return 0 };
    let mult = match unit.trim().to_ascii_uppercase().as_str() {
        "B" => 1.0,
        "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        _ => return 0,
    };
    (v * mult).round() as u64
}

/// Extract the Build-Cache reclaimable bytes from
/// `docker system df --format '{{.Type}}|{{.Reclaimable}}'` output
/// (e.g. a `Build Cache|2.5GB` line; a trailing " (59%)" is stripped).
pub fn parse_docker_df_build_cache(output: &str) -> u64 {
    for line in output.lines() {
        let Some((typ, reclaim)) = line.split_once('|') else { continue };
        if typ.trim().eq_ignore_ascii_case("build cache") {
            let val = reclaim.split('(').next().unwrap_or(reclaim);
            return parse_docker_size(val);
        }
    }
    0
}

/// Extract freed bytes from `docker builder prune -f` output
/// ("Total reclaimed space: 1.23GB").
pub fn parse_docker_prune_output(output: &str) -> u64 {
    for line in output.lines() {
        if let Some(rest) = line.trim().strip_prefix("Total reclaimed space:") {
            return parse_docker_size(rest);
        }
    }
    0
}

/// Reclaimable Docker build-cache bytes, or `None` when docker is missing /
/// the daemon isn't running (→ the category simply contributes nothing).
fn docker_build_cache_reclaimable() -> Option<u64> {
    let out = std::process::Command::new("docker")
        .args(["system", "df", "--format", "{{.Type}}|{{.Reclaimable}}"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_docker_df_build_cache(&String::from_utf8_lossy(&out.stdout)))
}

/// Run `docker builder prune -f`; returns freed bytes.
fn docker_builder_prune() -> Result<u64, String> {
    let out = std::process::Command::new("docker")
        .args(["builder", "prune", "-f"])
        .output()
        .map_err(|e| format!("docker: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "docker builder prune failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(parse_docker_prune_output(&String::from_utf8_lossy(&out.stdout)))
}

/// SHA-256 of a file's content; `None` on any I/O error (an unhashable file
/// can never be proven duplicate → conservatively skipped).
fn sha256_file(path: &Path) -> Option<[u8; 32]> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut f, &mut hasher).ok()?;
    Some(hasher.finalize().into())
}

/// SHA-256 of just the first 64 KiB — the cheap pre-filter (v0.84.244 perf):
/// same-size files whose prefixes already differ can never be duplicates, so
/// the expensive full-content hash only runs on prefix collisions. On a
/// Downloads folder full of large same-size media files this avoids reading
/// gigabytes.
const PREFIX_HASH_LEN: usize = 64 * 1024;

fn sha256_prefix(path: &Path) -> Option<[u8; 32]> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; PREFIX_HASH_LEN];
    let mut read = 0;
    while read < buf.len() {
        match f.read(&mut buf[read..]) {
            Ok(0) => break,
            Ok(n) => read += n,
            Err(_) => return None,
        }
    }
    let mut hasher = Sha256::new();
    hasher.update(&buf[..read]);
    Some(hasher.finalize().into())
}

/// Content-identical duplicates under `roots` (pure over explicit roots →
/// unit-testable): group by size, then by SHA-256 — only same-size files are
/// ever hashed. Within a group the **oldest** copy (mtime, then path, for
/// determinism) is KEPT; the rest are returned as deletable `(path, size)`.
/// Zero-byte files are ignored (all "identical", none worth deleting), and a
/// file whose mtime or hash can't be read drops out of its group entirely.
pub fn duplicate_items(roots: &[PathBuf], exclude: &[PathBuf]) -> Vec<(PathBuf, u64)> {
    use std::collections::BTreeMap;
    let mut files = Vec::new();
    for root in roots {
        collect_files(root, Duration::ZERO, SystemTime::now(), exclude, &mut files);
    }
    let mut by_size: BTreeMap<u64, Vec<PathBuf>> = BTreeMap::new();
    for (path, size) in files {
        if size > 0 {
            by_size.entry(size).or_default().push(path);
        }
    }
    let mut out = Vec::new();
    for (size, candidates) in by_size {
        if candidates.len() < 2 {
            continue;
        }
        // Stage 1 (cheap): group same-size candidates by a 64-KiB prefix hash.
        let mut by_prefix: BTreeMap<[u8; 32], Vec<PathBuf>> = BTreeMap::new();
        for path in candidates {
            let Some(pre) = sha256_prefix(&path) else { continue };
            by_prefix.entry(pre).or_default().push(path);
        }
        // Stage 2 (full content hash) only where prefixes collide. Files that
        // fit entirely inside the prefix are already fully hashed — skip the
        // second read.
        for (_, prefix_group) in by_prefix {
            if prefix_group.len() < 2 {
                continue;
            }
            let mut by_hash: BTreeMap<[u8; 32], Vec<(SystemTime, PathBuf)>> = BTreeMap::new();
            for path in prefix_group {
                let hash = if (size as usize) <= PREFIX_HASH_LEN {
                    sha256_prefix(&path) // == full hash for small files
                } else {
                    sha256_file(&path)
                };
                let Some(hash) = hash else { continue };
                let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
                let Ok(mtime) = meta.modified() else { continue };
                by_hash.entry(hash).or_default().push((mtime, path));
            }
            for (_, mut group) in by_hash {
                if group.len() < 2 {
                    continue;
                }
                group.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
                // group[0] = the oldest copy → the keeper. The rest go.
                for (_, path) in group.into_iter().skip(1) {
                    out.push((path, size));
                }
            }
        }
    }
    out
}

/// Run the duplicate finder for its enabled category and append the results to
/// the plan — skipping any path another category already claimed (e.g. an old
/// installer that is also a duplicate must not be planned twice).
fn append_duplicates(plan: &mut CleanPlan, group: &ScanGroup) {
    use std::collections::HashSet;
    let already: HashSet<&str> = plan.items.iter().map(|i| i.path.as_str()).collect();
    let dupes = duplicate_items(&group.roots, &group.exclude);
    let mut cat_bytes = 0u64;
    let mut fresh = Vec::new();
    for (path, size) in dupes {
        let p = path.to_string_lossy().to_string();
        if already.contains(p.as_str()) {
            continue;
        }
        cat_bytes += size;
        fresh.push(CleanItem { path: p, size, category: group.key.clone() });
    }
    plan.total_bytes += cat_bytes;
    plan.items.extend(fresh);
    plan.categories.push((group.key.clone(), group.label.clone(), cat_bytes));
}

/// Read-only scan for the current config. Safe to call any time. Two special
/// categories bypass the generic walker: the duplicate finder (content
/// hashing) and Docker (a `docker system df` estimate).
pub fn scan(cfg: &CleanerConfig) -> CleanPlan {
    let groups = enabled_groups(cfg);
    let dupes_group = groups.iter().find(|g| g.key == KEY_DUPES).cloned();
    let docker_group = groups.iter().find(|g| g.key == KEY_DOCKER).cloned();
    let generic: Vec<ScanGroup> = groups
        .into_iter()
        .filter(|g| g.key != KEY_DUPES && g.key != KEY_DOCKER)
        .collect();
    let mut plan = scan_roots(&generic, cfg.min_age_days, SystemTime::now());
    if let Some(g) = dupes_group {
        append_duplicates(&mut plan, &g);
    }
    if let Some(g) = docker_group {
        if let Some(bytes) = docker_build_cache_reclaimable() {
            if bytes > 0 {
                plan.items.push(CleanItem {
                    path: "Docker build cache — freed via `docker builder prune`".into(),
                    size: bytes,
                    category: g.key.clone(),
                });
                plan.total_bytes += bytes;
                plan.categories.push((g.key, g.label, bytes));
            }
        }
    }
    plan
}

/// Execute a previously-scanned plan, re-validating against the config's
/// allowlist. The plan should come from `scan(cfg)` with the same `cfg`.
/// Docker items are pseudo-items (no file path) — they run the builder prune
/// instead of the file deleter, and only if the category is enabled in `cfg`.
pub fn execute(cfg: &CleanerConfig, plan: &CleanPlan) -> CleanResult {
    let docker_requested = plan.items.iter().any(|i| i.category == KEY_DOCKER);
    let file_plan = CleanPlan {
        items: plan.items.iter().filter(|i| i.category != KEY_DOCKER).cloned().collect(),
        total_bytes: 0,
        categories: vec![],
    };
    let mut res = execute_plan(&file_plan, &enabled_roots(cfg), &enabled_excludes(cfg));
    if docker_requested && enabled_groups(cfg).iter().any(|g| g.key == KEY_DOCKER) {
        match docker_builder_prune() {
            Ok(freed) => {
                res.deleted += 1;
                res.freed_bytes += freed;
            }
            Err(e) => res.errors.push(e),
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

    fn group(key: &str, roots: Vec<PathBuf>, exclude: Vec<PathBuf>) -> ScanGroup {
        ScanGroup {
            key: key.into(),
            label: key.into(),
            roots,
            exclude,
            exts: vec![],
        }
    }

    fn tmp() -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "ir-cleaner-test-{}-{}",
            std::process::id(),
            // monotonic-ish unique suffix without Date/Instant
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// Write a file. (The age filter is tested via a large/zero `min_age_days`
    /// threshold against freshly-created files — no fragile mtime backdating.)
    fn write_old(path: &Path, contents: &[u8], _age_days: u64) {
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn level_inclusion_is_monotonic() {
        assert!(Level::Safe.includes(Level::Safe));
        assert!(!Level::Safe.includes(Level::Standard));
        assert!(Level::Standard.includes(Level::Safe));
        assert!(Level::Standard.includes(Level::Standard));
        assert!(!Level::Standard.includes(Level::Aggressive));
        assert!(Level::Aggressive.includes(Level::Aggressive));
        assert!(Level::Aggressive.includes(Level::Safe));
    }

    #[test]
    fn level_parses_loosely() {
        assert_eq!(Level::from_str_loose("AGGRESSIVE"), Level::Aggressive);
        assert_eq!(Level::from_str_loose("standard "), Level::Standard);
        assert_eq!(Level::from_str_loose("nonsense"), Level::Safe);
    }

    #[test]
    fn is_contained_basic() {
        let root = tmp();
        let sub = root.join("a/b");
        fs::create_dir_all(&sub).unwrap();
        let f = sub.join("f.txt");
        fs::write(&f, b"x").unwrap();
        assert!(is_contained(&f, &root));
        assert!(is_contained(&sub, &root));
        // root is not contained in itself.
        assert!(!is_contained(&root, &root));
        // A sibling dir is not contained.
        let other = tmp();
        let g = other.join("g.txt");
        fs::write(&g, b"x").unwrap();
        assert!(!is_contained(&g, &root));
    }

    #[test]
    fn scan_collects_old_files_and_sums_bytes() {
        let root = tmp();
        write_old(&root.join("a.log"), b"12345", 30); // 5 bytes
        write_old(&root.join("b.log"), b"678", 30); // 3 bytes
        let groups = vec![group("logs", vec![root.clone()], vec![])];
        let plan = scan_roots(&groups, 0, SystemTime::now());
        assert_eq!(plan.items.len(), 2);
        assert_eq!(plan.total_bytes, 8);
        assert_eq!(plan.categories.len(), 1);
        assert_eq!(plan.categories[0].2, 8);
    }

    #[test]
    fn scan_age_filter_excludes_recent_files() {
        // Fresh files are younger than a 365-day threshold → excluded; with a
        // 0-day threshold the same files qualify. No mtime backdating needed.
        let root = tmp();
        fs::write(root.join("fresh.log"), b"fresh-now").unwrap();
        let groups = vec![group("logs", vec![root.clone()], vec![])];
        assert!(scan_roots(&groups, 365, SystemTime::now()).items.is_empty());
        assert_eq!(scan_roots(&groups, 0, SystemTime::now()).items.len(), 1);
    }

    #[test]
    fn scan_recurses_subdirs() {
        let root = tmp();
        let sub = root.join("nested/deep");
        fs::create_dir_all(&sub).unwrap();
        write_old(&sub.join("x.tmp"), b"abcd", 30);
        let groups = vec![group("t", vec![root.clone()], vec![])];
        let plan = scan_roots(&groups, 0, SystemTime::now());
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.total_bytes, 4);
    }

    #[test]
    #[cfg(unix)]
    fn scan_does_not_follow_symlinked_dir_outside_root() {
        // An attacker symlinks a dir inside the cache root to point at a
        // sensitive dir outside it. We must NOT collect files through it.
        let root = tmp();
        let outside = tmp();
        fs::write(outside.join("secret.txt"), b"do not touch").unwrap();
        let link = root.join("evil");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let groups = vec![group("c", vec![root.clone()], vec![])];
        let plan = scan_roots(&groups, 0, SystemTime::now());
        // Nothing collected — the symlink was skipped.
        assert!(plan.items.is_empty(), "must not traverse symlinked dir");
        assert!(outside.join("secret.txt").exists());
    }

    #[test]
    fn execute_deletes_only_planned_files_and_sums_freed() {
        let root = tmp();
        write_old(&root.join("a.tmp"), b"123", 30);
        write_old(&root.join("b.tmp"), b"4567", 30);
        let keep = root.join("keep.txt");
        write_old(&keep, b"keepme", 30);
        let groups = vec![group("t", vec![root.clone()], vec![])];
        // Scan picks up all three (age 0 threshold).
        let mut plan = scan_roots(&groups, 0, SystemTime::now());
        // Remove "keep.txt" from the plan so execute must not touch it.
        plan.items.retain(|i| !i.path.ends_with("keep.txt"));
        plan.total_bytes = plan.items.iter().map(|i| i.size).sum();
        let res = execute_plan(&plan, &[root.clone()], &Default::default());
        assert_eq!(res.deleted, 2);
        assert_eq!(res.freed_bytes, 7);
        assert!(res.errors.is_empty());
        assert!(keep.exists(), "file not in the plan must survive");
        assert!(!root.join("a.tmp").exists());
    }

    #[test]
    fn execute_refuses_path_outside_allowlist() {
        let root = tmp();
        let outside = tmp();
        let victim = outside.join("victim.txt");
        fs::write(&victim, b"important").unwrap();
        // Hand-craft a malicious plan pointing outside the allowed root.
        let plan = CleanPlan {
            items: vec![CleanItem {
                path: victim.to_string_lossy().to_string(),
                size: 9,
                category: "x".into(),
            }],
            total_bytes: 9,
            categories: vec![],
        };
        let res = execute_plan(&plan, &[root.clone()], &Default::default());
        assert_eq!(res.deleted, 0);
        assert_eq!(res.errors.len(), 1);
        assert!(victim.exists(), "path outside allowlist must NOT be deleted");
    }

    #[test]
    #[cfg(unix)]
    fn execute_refuses_symlink_even_if_listed() {
        let root = tmp();
        let outside = tmp();
        let target = outside.join("target.txt");
        fs::write(&target, b"keep").unwrap();
        let link = root.join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let plan = CleanPlan {
            items: vec![CleanItem {
                path: link.to_string_lossy().to_string(),
                size: 0,
                category: "x".into(),
            }],
            total_bytes: 0,
            categories: vec![],
        };
        let res = execute_plan(&plan, &[root.clone()], &Default::default());
        assert_eq!(res.deleted, 0);
        assert_eq!(res.errors.len(), 1);
        // The symlink wasn't removed and the target is intact.
        assert!(target.exists());
    }

    #[test]
    fn empty_plan_is_a_noop() {
        let res = execute_plan(&CleanPlan::default(), &[], &Default::default());
        assert_eq!(res.deleted, 0);
        assert_eq!(res.freed_bytes, 0);
        assert!(res.errors.is_empty());
    }

    #[test]
    fn config_round_trips_through_settings() {
        use parking_lot::Mutex;
        use rusqlite::Connection;
        use std::sync::Arc;
        let db = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        crate::settings::init_table(&db).unwrap();
        let mut cfg = CleanerConfig {
            level: Level::Aggressive,
            min_age_days: 14,
            categories: Default::default(),
        };
        cfg.categories.insert("dev_caches".into(), true);
        cfg.categories.insert("os_temp".into(), false);
        save_config(&db, &cfg).unwrap();
        assert_eq!(load_config(&db), cfg);
    }

    #[test]
    fn default_config_is_conservative() {
        let d = CleanerConfig::default();
        assert_eq!(d.level, Level::Safe);
        assert_eq!(d.min_age_days, 7);
        assert!(d.categories.is_empty());
    }

    #[test]
    fn categories_are_nonempty_and_aggressive_dev_is_opt_out_by_default() {
        let cats = categories();
        assert!(!cats.is_empty());
        if let Some(dev) = cats.iter().find(|c| c.key == "dev_caches") {
            assert_eq!(dev.level, Level::Aggressive);
            assert!(!dev.default_enabled, "dev caches must be opt-in");
        }
    }

    #[test]
    fn scan_skips_excluded_subtrees() {
        // A broad category (whole cache dir) must not claim files that live in
        // the subtree carved out for a more specific category.
        let root = tmp();
        let owned = root.join("mine");
        let carved = root.join("browser");
        fs::create_dir_all(&owned).unwrap();
        fs::create_dir_all(&carved).unwrap();
        write_old(&owned.join("a.tmp"), b"123", 30);
        write_old(&carved.join("b.tmp"), b"4567", 30);
        let groups = vec![group("other", vec![root.clone()], vec![carved.clone()])];
        let plan = scan_roots(&groups, 0, SystemTime::now());
        assert_eq!(plan.items.len(), 1, "excluded subtree must not be scanned");
        assert!(plan.items[0].path.contains("mine"));
        assert_eq!(plan.total_bytes, 3);
    }

    #[test]
    fn execute_applies_exclusions_per_category_not_globally() {
        // The broad category's exclusion must not block the SPECIFIC category
        // that legitimately owns the carved-out subtree.
        let root = tmp();
        let carved = root.join("browser");
        fs::create_dir_all(&carved).unwrap();
        write_old(&carved.join("cache.bin"), b"12345", 30);
        let groups = vec![
            group("other", vec![root.clone()], vec![carved.clone()]),
            group("browser", vec![carved.clone()], vec![]),
        ];
        let plan = scan_roots(&groups, 0, SystemTime::now());
        assert_eq!(plan.items.len(), 1); // claimed once, by "browser"
        assert_eq!(plan.items[0].category, "browser");
        let mut excludes = std::collections::BTreeMap::new();
        excludes.insert("other".to_string(), vec![carved.clone()]);
        excludes.insert("browser".to_string(), vec![]);
        let res = execute_plan(&plan, &[root.clone(), carved.clone()], &excludes);
        assert_eq!(res.deleted, 1, "the owning category must be allowed to delete");
        assert!(res.errors.is_empty());

        // But a hand-crafted item claiming the carved subtree under the BROAD
        // key is rejected by the per-category re-check.
        write_old(&carved.join("cache2.bin"), b"12345", 30);
        let bad = CleanPlan {
            items: vec![CleanItem {
                path: carved.join("cache2.bin").to_string_lossy().to_string(),
                size: 5,
                category: "other".into(),
            }],
            total_bytes: 5,
            categories: vec![],
        };
        let res = execute_plan(&bad, &[root.clone(), carved.clone()], &excludes);
        assert_eq!(res.deleted, 0);
        assert_eq!(res.errors.len(), 1);
        assert!(carved.join("cache2.bin").exists());
    }

    fn set_mtime(path: &Path, t: SystemTime) {
        let f = fs::File::options().write(true).open(path).unwrap();
        f.set_times(fs::FileTimes::new().set_modified(t)).unwrap();
    }

    #[test]
    fn duplicate_finder_keeps_the_oldest_copy() {
        let root = tmp();
        let old = root.join("original.bin");
        let newer = root.join("original (1).bin");
        let unique = root.join("unique.bin");
        fs::write(&old, b"same-content").unwrap();
        fs::write(&newer, b"same-content").unwrap();
        fs::write(&unique, b"different!!!").unwrap(); // same size, other content
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        set_mtime(&old, t0);
        set_mtime(&newer, t0 + Duration::from_secs(3600));
        let dupes = duplicate_items(&[root.clone()], &[]);
        assert_eq!(dupes.len(), 1, "exactly the newer duplicate is deletable");
        assert_eq!(dupes[0].0, newer);
        assert_eq!(dupes[0].1, 12);
        assert!(old.exists() && unique.exists());
    }

    #[test]
    fn duplicate_finder_ignores_unique_and_zero_byte_files() {
        let root = tmp();
        fs::write(root.join("a.txt"), b"").unwrap(); // zero-byte "twins"
        fs::write(root.join("b.txt"), b"").unwrap();
        fs::write(root.join("c.txt"), b"abc").unwrap(); // same size…
        fs::write(root.join("d.txt"), b"xyz").unwrap(); // …different content
        assert!(duplicate_items(&[root], &[]).is_empty());
    }

    #[test]
    fn extension_filter_limits_a_group_to_matching_files() {
        let root = tmp();
        write_old(&root.join("setup.DMG"), b"12345", 30); // case-insensitive
        write_old(&root.join("notes.txt"), b"123", 30);
        let mut g = group("inst", vec![root.clone()], vec![]);
        g.exts = vec!["dmg".into()];
        let plan = scan_roots(&[g], 0, SystemTime::now());
        assert_eq!(plan.items.len(), 1);
        assert!(plan.items[0].path.to_lowercase().ends_with("setup.dmg"));
    }

    #[test]
    fn docker_size_parsing() {
        assert_eq!(parse_docker_size("0B"), 0);
        assert_eq!(parse_docker_size("1.5kB"), 1_500);
        assert_eq!(parse_docker_size("512.3MB"), 512_300_000);
        assert_eq!(parse_docker_size("2.5GB"), 2_500_000_000);
        assert_eq!(parse_docker_size(" 1TB "), 1_000_000_000_000);
        assert_eq!(parse_docker_size("garbage"), 0);
    }

    #[test]
    fn docker_df_and_prune_parsing() {
        let df = "Images|3.2GB (59%)\nContainers|50MB (50%)\nBuild Cache|2.5GB\n";
        assert_eq!(parse_docker_df_build_cache(df), 2_500_000_000);
        assert_eq!(parse_docker_df_build_cache("Images|1GB (10%)\n"), 0);
        // A percent suffix on the build-cache row is stripped too.
        assert_eq!(parse_docker_df_build_cache("Build Cache|1.5GB (100%)\n"), 1_500_000_000);
        let prune = "Deleted build cache objects:\nabc123\n\nTotal reclaimed space: 1.23GB\n";
        assert_eq!(parse_docker_prune_output(prune), 1_230_000_000);
        assert_eq!(parse_docker_prune_output("nothing relevant"), 0);
    }

    #[test]
    fn prefix_collision_with_different_tail_is_not_a_duplicate() {
        // Identical first 64 KiB, different tails → the full-content hash
        // must disambiguate after the cheap prefix filter.
        let root = tmp();
        let a = vec![0xABu8; PREFIX_HASH_LEN + 10];
        let mut b = a.clone();
        b[PREFIX_HASH_LEN + 5] = 0xCD;
        fs::write(root.join("a.bin"), &a).unwrap();
        fs::write(root.join("b.bin"), &b).unwrap();
        assert!(duplicate_items(&[root], &[]).is_empty());
    }

    #[test]
    fn risky_new_categories_are_aggressive_and_opt_in() {
        let cats = categories();
        for key in ["trash", "xcode_caches"] {
            if let Some(c) = cats.iter().find(|c| c.key == key) {
                assert_eq!(c.level, Level::Aggressive, "{key} must be Aggressive");
                assert!(!c.default_enabled, "{key} must be opt-in");
            }
        }
        // The broad caches category carves out the specific categories' roots.
        if let Some(other) = cats.iter().find(|c| c.key == "other_caches") {
            assert_eq!(other.level, Level::Standard);
            assert!(!other.exclude.is_empty(), "other_caches must carve out specifics");
        }
    }
}
