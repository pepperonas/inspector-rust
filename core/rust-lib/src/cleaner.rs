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
    /// Skip the global age filter for this category. Set by the stale-project
    /// categories: staleness is already decided per *project* (untouched for N
    /// days), so re-filtering the artifact's files by their own mtime would be
    /// wrong — an `npm install` in an otherwise dead project leaves fresh files
    /// inside a `node_modules` we still want to reclaim.
    #[serde(skip)]
    pub ignore_age: bool,
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
pub fn categories(cfg: &CleanerConfig) -> Vec<Category> {
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
            ignore_age: false,
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
        ignore_age: false,
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
            ignore_age: false,
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
            ignore_age: false,
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
        ignore_age: false,
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
            ignore_age: false,
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
            ignore_age: false,
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
            ignore_age: false,
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
            ignore_age: false,
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
                // uv keeps its cache in XDG-land, not ~/Library/Caches.
                h.join(".cache/uv"),
                h.join(".android/cache"),
                h.join(".android/build-cache"),
            ],
            exclude: vec![],
            exts: vec![],
            default_enabled: false,
            ignore_age: false,
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
            ignore_age: false,
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
            ignore_age: false,
        });

        // ── Developer targets (v0.84.264) ────────────────────────────────
        //
        // Xcode archives hold the dSYMs you need to symbolicate a crash report
        // from a shipped build — losing them is unrecoverable. Aggressive +
        // opt-in, and the age filter still applies.
        out.push(Category {
            key: "xcode_archives".into(),
            label: "Xcode archives — CONTAINS dSYMs (can't symbolicate old crashes after this)".into(),
            level: Level::Aggressive,
            roots: vec![h.join("Library/Developer/Xcode/Archives")],
            exclude: vec![],
            exts: vec![],
            default_enabled: false,
            ignore_age: false,
        });
        // Support/log dirs of JetBrains IDE versions that are gone (uninstalled
        // product, or superseded by a newer version dir).
        let jb = jetbrains_orphan_roots();
        if !jb.is_empty() {
            out.push(Category {
                key: KEY_JETBRAINS.into(),
                label: "JetBrains leftovers of uninstalled / superseded IDE versions".into(),
                level: Level::Standard,
                roots: jb,
                exclude: vec![],
                exts: vec![],
                default_enabled: true,
                ignore_age: true, // the whole version is dead; per-file mtime is noise
            });
        }
        // Command-based (no file roots → the file allowlist is untouched; the
        // tool's own reclaim command runs instead).
        out.push(Category {
            key: KEY_SIMCTL.into(),
            label: "Unavailable simulators (xcrun simctl delete unavailable)".into(),
            level: Level::Aggressive,
            roots: vec![],
            exclude: vec![],
            exts: vec![],
            default_enabled: false,
            ignore_age: false,
        });
        out.push(Category {
            key: KEY_BREW.into(),
            label: "Homebrew: outdated downloads (brew cleanup)".into(),
            level: Level::Standard,
            roots: vec![],
            exclude: vec![],
            exts: vec![],
            default_enabled: true,
            ignore_age: false,
        });
    }

    // ── Stale project artifacts (all platforms; roots come from the config) ──
    //
    // These are the only categories whose allowlist is *derived*: the roots are
    // the concrete `node_modules` / `target` dirs of projects that haven't been
    // touched in `stale_days`, each of which was verified to sit next to its
    // manifest. Nothing else under the dev roots is ever reachable.
    if !cfg.dev_roots.is_empty() {
        let roots: Vec<PathBuf> = cfg.dev_roots.iter().map(PathBuf::from).collect();
        let now = SystemTime::now();
        let node = find_stale_artifacts(&roots, ArtifactKind::Node, now, cfg.stale_days);
        if !node.is_empty() {
            out.push(Category {
                key: KEY_STALE_NODE.into(),
                label: format!("Stale node_modules (projects untouched {}+ days)", cfg.stale_days),
                level: Level::Standard,
                roots: node,
                exclude: vec![],
                exts: vec![],
                default_enabled: true,
                ignore_age: true, // staleness is decided per project, not per file
            });
        }
        let rust = find_stale_artifacts(&roots, ArtifactKind::Rust, now, cfg.stale_days);
        if !rust.is_empty() {
            out.push(Category {
                key: KEY_STALE_TARGET.into(),
                label: format!("Stale Rust target/ dirs (projects untouched {}+ days)", cfg.stale_days),
                level: Level::Standard,
                roots: rust,
                exclude: vec![],
                exts: vec![],
                default_enabled: true,
                ignore_age: true,
            });
        }
    }
    // pnpm's store prune removes only *orphaned* packages — a path delete
    // would nuke the whole store, so this one is command-based too.
    out.push(Category {
        key: KEY_PNPM.into(),
        label: "pnpm store: orphaned packages (pnpm store prune)".into(),
        level: Level::Standard,
        roots: vec![],
        exclude: vec![],
        exts: vec![],
        default_enabled: true,
        ignore_age: false,
    });

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
            ignore_age: false,
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
                ignore_age: false,
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
            ignore_age: false,
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
            ignore_age: false,
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
            ignore_age: false,
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
                ignore_age: false,
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
            ignore_age: false,
        });
        out.push(Category {
            key: "trash".into(),
            label: "Trash (items older than the age filter)".into(),
            level: Level::Aggressive,
            roots: vec![h.join(".local/share/Trash/files")],
            exclude: vec![],
            exts: vec![],
            default_enabled: false,
            ignore_age: false,
        });
    }

    out
}

// ── Config ───────────────────────────────────────────────────────────────

pub const KEY_LEVEL: &str = "cleaner.level";
pub const KEY_MIN_AGE: &str = "cleaner.min_age_days";
/// JSON map of `{ category_key: enabled }` overrides.
pub const KEY_CATEGORIES: &str = "cleaner.categories";
/// Newline/comma-separated project folders searched for stale build artifacts.
pub const KEY_DEV_ROOTS: &str = "cleaner.dev_roots";
/// How long a project must have been untouched to count as stale (days).
pub const KEY_STALE_DAYS: &str = "cleaner.stale_days";

/// Where we look for dead projects by default. Non-existent entries are simply
/// skipped, so shipping a few likely names costs nothing.
pub fn default_dev_roots() -> Vec<String> {
    let Some(h) = home() else { return Vec::new() };
    ["claude", "cursor", "dev"]
        .iter()
        .map(|d| h.join(d).to_string_lossy().to_string())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanerConfig {
    pub level: Level,
    pub min_age_days: u32,
    /// Per-category enable overrides (key → enabled). Missing key = the
    /// category's own `default_enabled`.
    pub categories: std::collections::BTreeMap<String, bool>,
    /// Project folders the stale-artifact scanner searches (absolute paths).
    #[serde(default)]
    pub dev_roots: Vec<String>,
    /// Staleness threshold in days for the stale-artifact categories.
    #[serde(default = "default_stale_days")]
    pub stale_days: u32,
}

fn default_stale_days() -> u32 {
    90
}

impl Default for CleanerConfig {
    fn default() -> Self {
        CleanerConfig {
            level: Level::Safe,
            min_age_days: 7,
            categories: std::collections::BTreeMap::new(),
            dev_roots: default_dev_roots(),
            stale_days: default_stale_days(),
        }
    }
}

/// Split a stored dev-root list (newline or comma separated) into paths.
pub fn parse_dev_roots(raw: &str) -> Vec<String> {
    raw.split(['\n', ','])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
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
    // An *unset* dev-root list falls back to the defaults; an explicitly
    // emptied one stays empty (the user opted out of project scanning).
    let dev_roots = crate::settings::get(db, KEY_DEV_ROOTS)
        .ok()
        .flatten()
        .map(|s| parse_dev_roots(&s))
        .unwrap_or(d.dev_roots);
    let stale_days = crate::settings::get(db, KEY_STALE_DAYS)
        .ok()
        .flatten()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(d.stale_days);
    CleanerConfig {
        level,
        min_age_days,
        categories,
        dev_roots,
        stale_days,
    }
}

pub fn save_config(db: &DbHandle, cfg: &CleanerConfig) -> anyhow::Result<()> {
    crate::settings::set(db, KEY_LEVEL, cfg.level.as_str())?;
    crate::settings::set(db, KEY_MIN_AGE, &cfg.min_age_days.to_string())?;
    crate::settings::set(db, KEY_CATEGORIES, &serde_json::to_string(&cfg.categories)?)?;
    crate::settings::set(db, KEY_DEV_ROOTS, &cfg.dev_roots.join("\n"))?;
    crate::settings::set(db, KEY_STALE_DAYS, &cfg.stale_days.to_string())?;
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// Ignore the global age filter (see `Category::ignore_age`).
    pub ignore_age: bool,
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
            exts.contains(&e)
        })
        .unwrap_or(false)
}

/// Build a plan from explicit scan groups. Read-only. The pure heart of
/// `scan` — tests drive it with temp dirs.
pub fn scan_roots(groups: &[ScanGroup], min_age_days: u32, now: SystemTime) -> CleanPlan {
    let global_min_age = Duration::from_secs(u64::from(min_age_days) * 86_400);
    let mut plan = CleanPlan::default();
    for ScanGroup { key, label, roots, exclude, exts, ignore_age } in groups {
        let min_age = if *ignore_age { Duration::ZERO } else { global_min_age };
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
    categories(cfg)
        .into_iter()
        .filter(|c| cfg.level.includes(c.level))
        .filter(|c| *cfg.categories.get(&c.key).unwrap_or(&c.default_enabled))
        .map(|c| ScanGroup {
            key: c.key,
            label: c.label,
            roots: c.roots,
            exclude: c.exclude,
            exts: c.exts,
            ignore_age: c.ignore_age,
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

// ── Developer targets (v0.84.264) ────────────────────────────────────────
//
// Everything below is macOS-first but path-portable; the categories are only
// *registered* on the platforms where their roots exist.

/// Stale `node_modules` of projects untouched for `stale_days`.
pub const KEY_STALE_NODE: &str = "stale_node_modules";
/// Stale Cargo `target/` dirs of projects untouched for `stale_days`.
pub const KEY_STALE_TARGET: &str = "stale_rust_target";
/// JetBrains support/log dirs of IDE versions that are gone.
pub const KEY_JETBRAINS: &str = "jetbrains_orphans";
/// `xcrun simctl delete unavailable` (command-based, like Docker).
pub const KEY_SIMCTL: &str = "simctl_unavailable";
/// `pnpm store prune` (command-based).
pub const KEY_PNPM: &str = "pnpm_store";
/// `brew cleanup` (command-based).
pub const KEY_BREW: &str = "brew_cleanup";

/// The command-based categories: no file roots, so they never touch the file
/// allowlist — they run the tool's own reclaim command instead. Each is
/// previewed with the tool's dry-run and executed only when checked.
pub fn is_command_category(key: &str) -> bool {
    matches!(key, KEY_DOCKER | KEY_SIMCTL | KEY_PNPM | KEY_BREW)
}

/// Which build artifact a project kind leaves behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    /// `package.json` → `node_modules`
    Node,
    /// `Cargo.toml` → `target`
    Rust,
}

impl ArtifactKind {
    fn manifest(self) -> &'static str {
        match self {
            ArtifactKind::Node => "package.json",
            ArtifactKind::Rust => "Cargo.toml",
        }
    }
    fn artifact(self) -> &'static str {
        match self {
            ArtifactKind::Node => "node_modules",
            ArtifactKind::Rust => "target",
        }
    }
}

/// Directory names we never descend into while looking for projects — build
/// artifacts (which can be enormous) and VCS internals.
const WALK_SKIP: [&str; 4] = ["node_modules", "target", ".git", "Pods"];
/// How deep below a dev root a project may sit.
const WALK_MAX_DEPTH: u32 = 5;

/// When a project was last worked on: the newest mtime among the signals that
/// track *human* activity — the manifest, the source dir, the git HEAD, and
/// the project dir itself. Deliberately NOT the artifact's own mtime (a
/// background `cargo build` or a dependency install would otherwise keep a
/// long-dead project looking alive). `None` = can't tell → treated as active
/// (never stale), the safe default.
pub fn project_last_active(project: &Path) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    for rel in [
        "",
        "package.json",
        "Cargo.toml",
        "src",
        "lib",
        ".git/HEAD",
        "README.md",
    ] {
        let p = if rel.is_empty() { project.to_path_buf() } else { project.join(rel) };
        let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        newest = Some(match newest {
            Some(cur) if cur >= mtime => cur,
            _ => mtime,
        });
    }
    newest
}

/// Pure staleness decision. `None` (unknown last-active) is never stale.
pub fn is_stale(last_active: Option<SystemTime>, now: SystemTime, stale_days: u32) -> bool {
    let Some(last) = last_active else { return false };
    let threshold = Duration::from_secs(u64::from(stale_days) * 86_400);
    now.duration_since(last).map(|age| age >= threshold).unwrap_or(false)
}

/// Walk `roots` for projects of `kind` whose artifact dir exists and whose
/// project has been untouched for `stale_days`; returns the **artifact** dirs
/// (`…/node_modules`, `…/target`).
///
/// Plausibility gate (non-negotiable): the artifact is only ever reported when
/// its parent actually holds the matching manifest — a bare `node_modules`
/// without a `package.json` next to it is left alone. Symlinks are never
/// followed, and the walk never descends into artifacts.
pub fn find_stale_artifacts(
    roots: &[PathBuf],
    kind: ArtifactKind,
    now: SystemTime,
    stale_days: u32,
) -> Vec<PathBuf> {
    fn walk(
        dir: &Path,
        depth: u32,
        kind: ArtifactKind,
        now: SystemTime,
        stale_days: u32,
        out: &mut Vec<PathBuf>,
    ) {
        if depth > WALK_MAX_DEPTH {
            return;
        }
        // A project here? (manifest + artifact side by side)
        let artifact = dir.join(kind.artifact());
        if dir.join(kind.manifest()).is_file() {
            let is_real_dir = std::fs::symlink_metadata(&artifact)
                .map(|m| m.is_dir() && !m.file_type().is_symlink())
                .unwrap_or(false);
            if is_real_dir && is_stale(project_last_active(dir), now, stale_days) {
                out.push(artifact);
            }
        }
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
            if !meta.is_dir() || meta.file_type().is_symlink() {
                continue;
            }
            let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
            // Never descend into artifacts / VCS internals / hidden dirs.
            if WALK_SKIP.contains(&name) || name.starts_with('.') {
                continue;
            }
            walk(&p, depth + 1, kind, now, stale_days, out);
        }
    }

    let mut out = Vec::new();
    for root in roots {
        if root.is_dir() {
            walk(root, 0, kind, now, stale_days, &mut out);
        }
    }
    out.sort();
    out
}

// ── JetBrains orphans ────────────────────────────────────────────────────

/// Split a JetBrains support-dir name into (product, version):
/// `"IntelliJIdea2023.1"` → `("IntelliJIdea", "2023.1")`. `None` if it carries
/// no version (then we never touch it — we can't reason about it).
pub fn parse_jetbrains_dir(name: &str) -> Option<(String, String)> {
    let idx = name.find(|c: char| c.is_ascii_digit())?;
    let (product, version) = name.split_at(idx);
    if product.is_empty() || version.is_empty() {
        return None;
    }
    Some((product.to_string(), version.to_string()))
}

/// Compare two JetBrains versions ("2023.1" < "2023.10" < "2024.2") by numeric
/// segments, so a plain string sort can't mis-rank 2023.10 below 2023.2.
fn version_key(v: &str) -> Vec<u64> {
    v.split(['.', '-'])
        .map(|seg| seg.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
        .map(|digits| digits.parse::<u64>().unwrap_or(0))
        .collect()
}

/// Normalise a product name for comparison: `"IntelliJ IDEA.app"` and
/// `"IntelliJIdea"` both collapse to `intellijidea`.
fn normalise_product(s: &str) -> String {
    s.trim_end_matches(".app")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Which of the JetBrains dirs in `present` belong to no live IDE.
///
/// Two independent reasons to call a dir an orphan, and both are strictly
/// conservative — a version that might still be in use is never listed:
///   * its product isn't installed at all (no matching app bundle), or
///   * a **newer** version of the same product exists in `present` (the newest
///     is the one the installed IDE uses, so it's always kept).
///
/// Dirs without a parseable version are never listed.
pub fn jetbrains_orphans(present: &[String], installed_apps: &[String]) -> Vec<String> {
    use std::collections::BTreeMap;
    let installed: Vec<String> = installed_apps.iter().map(|a| normalise_product(a)).collect();
    let mut by_product: BTreeMap<String, Vec<(Vec<u64>, String)>> = BTreeMap::new();
    for name in present {
        let Some((product, version)) = parse_jetbrains_dir(name) else { continue };
        by_product
            .entry(normalise_product(&product))
            .or_default()
            .push((version_key(&version), name.clone()));
    }
    let mut out = Vec::new();
    for (product, mut versions) in by_product {
        // An app bundle counts as this product if either name contains the
        // other once normalised ("IntelliJ IDEA.app" ⊃ "intellijidea";
        // "PyCharm Community" ⊃ "pycharm").
        let product_installed = installed
            .iter()
            .any(|a| a.contains(&product) || product.contains(a.as_str()));
        if !product_installed {
            out.extend(versions.into_iter().map(|(_, name)| name));
            continue;
        }
        versions.sort();
        versions.pop(); // keep the newest — that's the installed IDE's
        out.extend(versions.into_iter().map(|(_, name)| name));
    }
    out.sort();
    out
}

/// The orphaned JetBrains dirs on this machine (support + logs), as roots.
#[cfg(target_os = "macos")]
fn jetbrains_orphan_roots() -> Vec<PathBuf> {
    let Some(h) = home() else { return Vec::new() };
    let installed = list_dir_names(&PathBuf::from("/Applications"))
        .into_iter()
        .chain(list_dir_names(&h.join("Applications")))
        .collect::<Vec<_>>();
    let mut out = Vec::new();
    for base in [
        h.join("Library/Application Support/JetBrains"),
        h.join("Library/Logs/JetBrains"),
    ] {
        let present = list_dir_names(&base);
        for name in jetbrains_orphans(&present, &installed) {
            out.push(base.join(name));
        }
    }
    out
}

/// Immediate subdirectory names of `dir` (empty if unreadable).
fn list_dir_names(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect()
}

// ── Command-based dev tools (preview via dry-run, like Docker) ───────────

/// Is `bin` on PATH? (Command categories contribute nothing when their tool
/// isn't installed — never an error.)
fn has_binary(bin: &str) -> bool {
    std::process::Command::new("which")
        .arg(bin)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Parse `brew cleanup --dry-run`'s summary line: "This operation would free
/// approximately 1.2GB of disk space". Also matches the past-tense line that
/// the real run prints ("has freed approximately …"). 0 if absent.
pub fn parse_brew_freeable(output: &str) -> u64 {
    for line in output.lines() {
        let l = line.trim();
        let Some(idx) = l.find("approximately") else { continue };
        let rest = l[idx + "approximately".len()..].trim();
        let size: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.')
            .collect();
        let bytes = parse_docker_size(&size); // same SI-suffix grammar
        if bytes > 0 {
            return bytes;
        }
    }
    0
}

/// UDIDs of simulators `xcrun simctl list devices` marks unavailable — the
/// ones `simctl delete unavailable` would remove. Lines look like
/// `    iPhone 12 (UDID) (Shutdown) (unavailable, runtime profile not found)`.
pub fn parse_unavailable_udids(output: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in output.lines() {
        if !line.contains("(unavailable") {
            continue;
        }
        // The UDID is the first parenthesised group that looks like one.
        for part in line.split('(') {
            let cand = part.split(')').next().unwrap_or("").trim();
            let is_udid = cand.len() == 36
                && cand.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
                && cand.matches('-').count() == 4;
            if is_udid {
                out.push(cand.to_string());
                break;
            }
        }
    }
    out
}

/// Recursive on-disk size of `dir` (lstat, never follows symlinks). 0 if gone.
fn dir_size(dir: &Path) -> u64 {
    let mut files = Vec::new();
    collect_files(dir, Duration::ZERO, SystemTime::now(), &[], &mut files);
    files.iter().map(|(_, s)| *s).sum()
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

/// Preview a command-based category: `(pseudo-item label, reclaimable bytes)`,
/// or `None` when the tool is missing / has nothing to reclaim (→ the category
/// contributes nothing, never an error).
fn command_preview(key: &str) -> Option<(String, u64)> {
    match key {
        KEY_DOCKER => {
            let bytes = docker_build_cache_reclaimable()?;
            (bytes > 0).then(|| ("Docker build cache — freed via `docker builder prune`".into(), bytes))
        }
        KEY_BREW => {
            if !has_binary("brew") {
                return None;
            }
            let out = std::process::Command::new("brew")
                .args(["cleanup", "--dry-run"])
                .output()
                .ok()?;
            let bytes = parse_brew_freeable(&String::from_utf8_lossy(&out.stdout));
            (bytes > 0).then(|| ("Homebrew outdated downloads — freed via `brew cleanup`".into(), bytes))
        }
        KEY_PNPM => {
            if !has_binary("pnpm") {
                return None;
            }
            // pnpm has no dry-run that reports a size, and the orphaned share of
            // the store can't be known without pruning — so we show the item
            // with an unknown size rather than inventing one.
            Some(("pnpm store: orphaned packages — freed via `pnpm store prune` (size unknown until run)".into(), 0))
        }
        KEY_SIMCTL => {
            if !has_binary("xcrun") {
                return None;
            }
            let out = std::process::Command::new("xcrun")
                .args(["simctl", "list", "devices"])
                .output()
                .ok()?;
            let udids = parse_unavailable_udids(&String::from_utf8_lossy(&out.stdout));
            if udids.is_empty() {
                return None;
            }
            // The devices' own directories give an exact size.
            let base = home()?.join("Library/Developer/CoreSimulator/Devices");
            let bytes: u64 = udids.iter().map(|u| dir_size(&base.join(u))).sum();
            Some((
                format!("{} unavailable simulator(s) — freed via `simctl delete unavailable`", udids.len()),
                bytes,
            ))
        }
        _ => None,
    }
}

/// Run a command category's reclaim command. Returns freed bytes (0 when the
/// tool doesn't report a size).
fn command_execute(key: &str) -> Result<u64, String> {
    let run = |bin: &str, args: &[&str]| -> Result<String, String> {
        let out = std::process::Command::new(bin)
            .args(args)
            .output()
            .map_err(|e| format!("{bin}: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "{bin} {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    };
    match key {
        KEY_DOCKER => docker_builder_prune(),
        KEY_BREW => Ok(parse_brew_freeable(&run("brew", &["cleanup"])?)),
        KEY_PNPM => run("pnpm", &["store", "prune"]).map(|_| 0),
        KEY_SIMCTL => run("xcrun", &["simctl", "delete", "unavailable"]).map(|_| 0),
        _ => Err(format!("{key}: not a command category")),
    }
}

/// Read-only scan for the current config. Safe to call any time. Two kinds of
/// category bypass the generic walker: the duplicate finder (content hashing)
/// and the command-based tools (Docker / brew / pnpm / simctl — previewed via
/// their dry-run).
pub fn scan(cfg: &CleanerConfig) -> CleanPlan {
    let groups = enabled_groups(cfg);
    let dupes_group = groups.iter().find(|g| g.key == KEY_DUPES).cloned();
    let command_groups: Vec<ScanGroup> =
        groups.iter().filter(|g| is_command_category(&g.key)).cloned().collect();
    let generic: Vec<ScanGroup> = groups
        .into_iter()
        .filter(|g| g.key != KEY_DUPES && !is_command_category(&g.key))
        .collect();
    let mut plan = scan_roots(&generic, cfg.min_age_days, SystemTime::now());
    if let Some(g) = dupes_group {
        append_duplicates(&mut plan, &g);
    }
    for g in command_groups {
        if let Some((label, bytes)) = command_preview(&g.key) {
            plan.items.push(CleanItem {
                path: label,
                size: bytes,
                category: g.key.clone(),
            });
            plan.total_bytes += bytes;
            plan.categories.push((g.key, g.label, bytes));
        }
    }
    plan
}

/// Execute a previously-scanned plan, re-validating against the config's
/// allowlist. The plan should come from `scan(cfg)` with the same `cfg`.
/// Command items are pseudo-items (their `path` is a label, not a file) — they
/// run the tool's reclaim command instead of the file deleter, and only if the
/// category is still enabled in `cfg`.
pub fn execute(cfg: &CleanerConfig, plan: &CleanPlan) -> CleanResult {
    let enabled: Vec<String> = enabled_groups(cfg).into_iter().map(|g| g.key).collect();
    let mut commands: Vec<String> = plan
        .items
        .iter()
        .filter(|i| is_command_category(&i.category))
        .map(|i| i.category.clone())
        .collect();
    commands.sort();
    commands.dedup();

    let file_plan = CleanPlan {
        items: plan
            .items
            .iter()
            .filter(|i| !is_command_category(&i.category))
            .cloned()
            .collect(),
        total_bytes: 0,
        categories: vec![],
    };
    let mut res = execute_plan(&file_plan, &enabled_roots(cfg), &enabled_excludes(cfg));
    for key in commands {
        if !enabled.contains(&key) {
            continue;
        }
        match command_execute(&key) {
            Ok(freed) => {
                res.deleted += 1;
                res.freed_bytes += freed;
            }
            Err(e) => res.errors.push(e),
        }
    }
    res
}

// ── Directory aggregation (v0.84.264) ────────────────────────────────────
//
// The plan is (and stays) file-granular — that's what makes "execute deletes
// exactly what was planned, re-validated" true. But a DerivedData scan is
// 100k+ files, which is useless to look at and expensive to ship over IPC. So
// the plan stays in the backend (`PlanStore`) and the UI gets this: one row per
// directory, with the files aggregated into it.

/// One selectable row in the UI: a directory (or a command pseudo-item).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanDir {
    /// The directory's path — also the selection key sent back to `execute`.
    pub path: String,
    pub size: u64,
    /// How many files of the plan live under it (1 for a command pseudo-item).
    pub count: u64,
    pub category: String,
}

/// What the frontend gets from a scan: aggregated rows, never the raw items.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CleanPlanView {
    pub dirs: Vec<CleanDir>,
    pub total_bytes: u64,
    /// `(key, label, bytes)` per scanned category.
    pub categories: Vec<(String, String, u64)>,
}

/// The directory row an item belongs to: its owning root plus one more path
/// component (so `~/Library/Caches/foo/bar/baz.bin` rolls up into
/// `~/Library/Caches/foo`), or the root itself if the file sits directly in it.
/// A path that matches no root (a command pseudo-item) is its own row.
pub fn entry_for(item: &CleanItem, roots: &[PathBuf]) -> String {
    let path = Path::new(&item.path);
    // Longest matching root wins (roots can nest).
    let root = roots
        .iter()
        .filter(|r| path.starts_with(r))
        .max_by_key(|r| r.as_os_str().len());
    let Some(root) = root else {
        return item.path.clone();
    };
    match path.strip_prefix(root).ok().and_then(|rel| rel.components().next()) {
        Some(first) if path != root.join(first.as_os_str()) => {
            root.join(first.as_os_str()).to_string_lossy().to_string()
        }
        // The file sits directly in the root → the root is the row.
        _ => root.to_string_lossy().to_string(),
    }
}

/// Roll a file-granular plan up into directory rows, largest first. Pure.
pub fn aggregate_dirs(
    plan: &CleanPlan,
    roots_by_cat: &std::collections::BTreeMap<String, Vec<PathBuf>>,
) -> CleanPlanView {
    use std::collections::BTreeMap;
    let mut acc: BTreeMap<(String, String), (u64, u64)> = BTreeMap::new(); // (cat, dir) → (bytes, count)
    for item in &plan.items {
        let roots = roots_by_cat.get(&item.category).map(Vec::as_slice).unwrap_or(&[]);
        let dir = entry_for(item, roots);
        let e = acc.entry((item.category.clone(), dir)).or_insert((0, 0));
        e.0 += item.size;
        e.1 += 1;
    }
    let mut dirs: Vec<CleanDir> = acc
        .into_iter()
        .map(|((category, path), (size, count))| CleanDir { path, size, count, category })
        .collect();
    dirs.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    CleanPlanView {
        dirs,
        total_bytes: plan.total_bytes,
        categories: plan.categories.clone(),
    }
}

/// Keep only the items whose directory row the user actually ticked. The
/// backend still re-validates every path at delete time — this is purely
/// "what did the user consent to".
pub fn filter_by_selection(
    plan: &CleanPlan,
    roots_by_cat: &std::collections::BTreeMap<String, Vec<PathBuf>>,
    selected: &[String],
) -> CleanPlan {
    use std::collections::HashSet;
    let want: HashSet<&str> = selected.iter().map(String::as_str).collect();
    let mut out = CleanPlan::default();
    for item in &plan.items {
        let roots = roots_by_cat.get(&item.category).map(Vec::as_slice).unwrap_or(&[]);
        if !want.contains(entry_for(item, roots).as_str()) {
            continue;
        }
        out.total_bytes += item.size;
        out.items.push(item.clone());
    }
    out
}

/// Per-category roots of the enabled categories — what `aggregate_dirs` /
/// `filter_by_selection` need to roll paths up.
pub fn enabled_roots_by_cat(
    cfg: &CleanerConfig,
) -> std::collections::BTreeMap<String, Vec<PathBuf>> {
    enabled_groups(cfg).into_iter().map(|g| (g.key, g.roots)).collect()
}

/// Sentinel when a scan/execute is already in flight (`cleaner_scan` /
/// `cleaner_execute` reject a concurrent start so Esc→reopen can't double-run).
pub const ERR_BUSY: &str = "clean.busy";

/// Backend phase for the `clean` job — survives the overlay closing (v0.101.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CleanPhase {
    #[default]
    Idle,
    Scanning,
    Executing,
}

impl CleanPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Scanning => "scanning",
            Self::Executing => "executing",
        }
    }
}

/// What the panel (or a reconnect) sees: phase + a pending scan view when idle.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CleanStatus {
    /// `"idle"` | `"scanning"` | `"executing"`.
    pub phase: String,
    /// Aggregated view from the last successful scan, while idle and not yet
    /// consumed by a successful execute. Lets Esc mid-scan → reopen land on
    /// the finished picker without re-walking the disk.
    pub view: Option<CleanPlanView>,
}

/// Payload of the `clean-done` event — emitted when a scan or execute finishes
/// so a closed overlay still surfaces a toast / a reopened panel can reconnect.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanDone {
    /// `"scan"` | `"execute"`.
    pub kind: String,
    pub view: Option<CleanPlanView>,
    pub result: Option<CleanResult>,
    pub error: Option<String>,
}

/// The scanned plan + job phase, held in the backend between `cleaner_scan`
/// and `cleaner_execute` (Tauri-managed state). Keeping the file-granular plan
/// here — instead of shipping 100k items to the webview and back — is what
/// makes the directory-row UI cheap. The phase + `last_view` let the job
/// **survive Esc / overlay-hide** the same way `shazam_listen` does.
#[derive(Default)]
pub struct PlanStore {
    plan: parking_lot::Mutex<Option<CleanPlan>>,
    phase: parking_lot::Mutex<CleanPhase>,
    last_view: parking_lot::Mutex<Option<CleanPlanView>>,
}

impl PlanStore {
    /// Snapshot for a freshly-opened panel (reconnect or reuse pending view).
    pub fn status(&self) -> CleanStatus {
        let phase = *self.phase.lock();
        let view = if phase == CleanPhase::Idle {
            self.last_view.lock().clone()
        } else {
            None
        };
        CleanStatus {
            phase: phase.as_str().into(),
            view,
        }
    }

    /// Start a scan. Clears any pending view so a fresh walk replaces it.
    /// Returns `false` when another job is already running.
    pub fn begin_scan(&self) -> bool {
        let mut phase = self.phase.lock();
        if *phase != CleanPhase::Idle {
            return false;
        }
        *phase = CleanPhase::Scanning;
        *self.last_view.lock() = None;
        true
    }

    pub fn finish_scan(&self, plan: CleanPlan, view: CleanPlanView) {
        *self.plan.lock() = Some(plan);
        *self.last_view.lock() = Some(view);
        *self.phase.lock() = CleanPhase::Idle;
    }

    /// Abort a scan/execute back to idle (keeps any existing plan/view on
    /// execute failure so the user can retry; scan failure has no plan yet).
    pub fn fail_job(&self) {
        *self.phase.lock() = CleanPhase::Idle;
    }

    /// Take the stored plan and mark executing. Err = busy or no prior scan.
    pub fn begin_execute(&self) -> Result<CleanPlan, String> {
        let mut phase = self.phase.lock();
        if *phase != CleanPhase::Idle {
            return Err(ERR_BUSY.into());
        }
        let plan = self
            .plan
            .lock()
            .clone()
            .ok_or_else(|| "no scan to execute — run the scan first".to_string())?;
        *phase = CleanPhase::Executing;
        Ok(plan)
    }

    /// Successful delete — drop the plan so the next `clean` re-scans.
    pub fn finish_execute_ok(&self) {
        *self.plan.lock() = None;
        *self.last_view.lock() = None;
        *self.phase.lock() = CleanPhase::Idle;
    }

    /// Failed delete — keep plan/view for retry.
    pub fn finish_execute_err(&self) {
        *self.phase.lock() = CleanPhase::Idle;
    }
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
            ignore_age: false,
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
        let res = execute_plan(&plan, std::slice::from_ref(&root), &Default::default());
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
        let res = execute_plan(&plan, std::slice::from_ref(&root), &Default::default());
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
        let res = execute_plan(&plan, std::slice::from_ref(&root), &Default::default());
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
            dev_roots: vec!["/nonexistent/dev-root".into()],
            stale_days: 45,
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
        let cats = categories(&CleanerConfig::default());
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
        let dupes = duplicate_items(std::slice::from_ref(&root), &[]);
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
        let cats = categories(&CleanerConfig::default());
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

    // ── Developer targets (v0.84.264) ────────────────────────────────────

    /// Backdate a path's mtime by `days` — via std's `FileTimes` (no extra dep;
    /// works for directories too, since Unix `futimens` accepts a read-only fd).
    fn age_by_days(path: &Path, days: u64) {
        let when = SystemTime::now() - Duration::from_secs(days * 86_400);
        let f = fs::File::open(path).unwrap();
        f.set_times(fs::FileTimes::new().set_modified(when)).unwrap();
    }

    /// A project dir with its manifest + artifact, aged `days` days.
    fn make_project(base: &Path, name: &str, kind: ArtifactKind, days: u64) -> PathBuf {
        let proj = base.join(name);
        fs::create_dir_all(proj.join(kind.artifact())).unwrap();
        fs::write(proj.join(kind.manifest()), b"{}").unwrap();
        fs::write(proj.join(kind.artifact()).join("junk.bin"), vec![0u8; 100]).unwrap();
        // Age every activity signal, incl. the project dir itself.
        age_by_days(&proj.join(kind.manifest()), days);
        age_by_days(&proj, days);
        proj
    }

    #[test]
    fn stale_projects_are_found_fresh_ones_are_not() {
        let base = tmp();
        make_project(&base, "dead", ArtifactKind::Node, 200);
        make_project(&base, "alive", ArtifactKind::Node, 3);
        let found =
            find_stale_artifacts(std::slice::from_ref(&base), ArtifactKind::Node, SystemTime::now(), 90);
        assert_eq!(found.len(), 1, "only the dead project's artifact: {found:?}");
        assert!(found[0].ends_with("dead/node_modules"));
    }

    #[test]
    fn node_modules_without_a_manifest_is_never_touched() {
        // The plausibility gate: a bare node_modules (no package.json beside it)
        // might be anything — never delete it.
        let base = tmp();
        let orphan = base.join("mystery/node_modules");
        fs::create_dir_all(&orphan).unwrap();
        fs::write(orphan.join("x.bin"), b"x").unwrap();
        age_by_days(&base.join("mystery"), 300);
        let found = find_stale_artifacts(&[base], ArtifactKind::Node, SystemTime::now(), 90);
        assert!(found.is_empty(), "no manifest → not a project: {found:?}");
    }

    #[test]
    fn a_symlinked_artifact_is_never_reported() {
        let base = tmp();
        let proj = base.join("linky");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join("package.json"), b"{}").unwrap();
        let real = base.join("elsewhere");
        fs::create_dir_all(&real).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, proj.join("node_modules")).unwrap();
        age_by_days(&proj, 300);
        let found = find_stale_artifacts(&[base], ArtifactKind::Node, SystemTime::now(), 90);
        assert!(found.is_empty(), "symlinked artifact must be skipped: {found:?}");
    }

    #[test]
    fn recent_source_edits_keep_a_project_alive() {
        // The manifest is ancient but src/ was touched today → not stale.
        let base = tmp();
        let proj = make_project(&base, "worked_on", ArtifactKind::Rust, 300);
        fs::create_dir_all(proj.join("src")).unwrap(); // fresh mtime
        let found = find_stale_artifacts(&[base], ArtifactKind::Rust, SystemTime::now(), 90);
        assert!(found.is_empty(), "a recently-edited project is not stale: {found:?}");
    }

    #[test]
    fn is_stale_treats_unknown_activity_as_alive() {
        assert!(!is_stale(None, SystemTime::now(), 1));
    }

    #[test]
    fn stale_scan_ignores_the_age_filter() {
        // A stale project may still hold brand-new files (a dependency install
        // that never led anywhere). The per-file age filter must not save them.
        let base = tmp();
        make_project(&base, "dead", ArtifactKind::Node, 200);
        let artifacts = find_stale_artifacts(&[base], ArtifactKind::Node, SystemTime::now(), 90);
        let g = ScanGroup {
            key: KEY_STALE_NODE.into(),
            label: "stale".into(),
            roots: artifacts,
            exclude: vec![],
            exts: vec![],
            ignore_age: true,
        };
        // min_age of 365 days would otherwise exclude the freshly-written junk.
        let plan = scan_roots(&[g], 365, SystemTime::now());
        assert_eq!(plan.items.len(), 1, "ignore_age must bypass the age filter");
    }

    #[test]
    fn jetbrains_orphans_keep_the_newest_and_drop_uninstalled_products() {
        let present = vec![
            "IntelliJIdea2023.1".to_string(),
            "IntelliJIdea2023.10".to_string(), // newest (numeric compare, not string!)
            "IntelliJIdea2023.2".to_string(),
            "PyCharm2022.3".to_string(), // product not installed at all
            "consentOptions".to_string(), // no version → never touched
        ];
        let installed = vec!["IntelliJ IDEA.app".to_string()];
        let orphans = jetbrains_orphans(&present, &installed);
        assert_eq!(
            orphans,
            vec!["IntelliJIdea2023.1", "IntelliJIdea2023.2", "PyCharm2022.3"],
            "keep the newest installed version; drop older + uninstalled products"
        );
    }

    #[test]
    fn parse_brew_and_simctl_output() {
        assert_eq!(
            parse_brew_freeable("==> This operation would free approximately 1.2GB of disk space."),
            1_200_000_000
        );
        assert_eq!(parse_brew_freeable("nothing to do"), 0);

        let sim = "    iPhone 12 (A1B2C3D4-1111-2222-3333-444455556666) (Shutdown) (unavailable, runtime profile not found)\n\
                       iPhone 15 (DEADBEEF-0000-1111-2222-333344445555) (Booted)";
        assert_eq!(
            parse_unavailable_udids(sim),
            vec!["A1B2C3D4-1111-2222-3333-444455556666"]
        );
    }

    #[test]
    fn aggregation_rolls_files_up_into_directory_rows() {
        let root = PathBuf::from("/cache/root");
        let plan = CleanPlan {
            items: vec![
                CleanItem { path: "/cache/root/a/1.bin".into(), size: 10, category: "c".into() },
                CleanItem { path: "/cache/root/a/deep/2.bin".into(), size: 5, category: "c".into() },
                CleanItem { path: "/cache/root/b/3.bin".into(), size: 30, category: "c".into() },
                CleanItem { path: "/cache/root/loose.bin".into(), size: 1, category: "c".into() },
            ],
            total_bytes: 46,
            categories: vec![("c".into(), "C".into(), 46)],
        };
        let mut roots = std::collections::BTreeMap::new();
        roots.insert("c".to_string(), vec![root.clone()]);
        let view = aggregate_dirs(&plan, &roots);
        let rows: Vec<(&str, u64, u64)> =
            view.dirs.iter().map(|d| (d.path.as_str(), d.size, d.count)).collect();
        assert_eq!(
            rows,
            vec![
                ("/cache/root/b", 30, 1),  // largest first
                ("/cache/root/a", 15, 2),  // both files rolled up, incl. the deep one
                ("/cache/root", 1, 1),     // a file directly in the root is its own row
            ]
        );
        assert_eq!(view.total_bytes, 46);
    }

    #[test]
    fn selection_filters_the_plan_to_exactly_what_was_ticked() {
        let root = PathBuf::from("/cache/root");
        let plan = CleanPlan {
            items: vec![
                CleanItem { path: "/cache/root/a/1.bin".into(), size: 10, category: "c".into() },
                CleanItem { path: "/cache/root/b/2.bin".into(), size: 30, category: "c".into() },
            ],
            total_bytes: 40,
            categories: vec![],
        };
        let mut roots = std::collections::BTreeMap::new();
        roots.insert("c".to_string(), vec![root]);
        let chosen = filter_by_selection(&plan, &roots, &["/cache/root/a".to_string()]);
        assert_eq!(chosen.items.len(), 1);
        assert_eq!(chosen.items[0].path, "/cache/root/a/1.bin");
        assert_eq!(chosen.total_bytes, 10);

        // Nothing ticked → nothing planned (never "all" by accident).
        assert!(filter_by_selection(&plan, &roots, &[]).items.is_empty());
    }

    #[test]
    fn a_command_pseudo_item_is_its_own_row_and_survives_selection() {
        let plan = CleanPlan {
            items: vec![CleanItem {
                path: "Docker build cache — freed via `docker builder prune`".into(),
                size: 2_000_000_000,
                category: KEY_DOCKER.into(),
            }],
            total_bytes: 2_000_000_000,
            categories: vec![],
        };
        let roots = std::collections::BTreeMap::new(); // command categories have none
        let view = aggregate_dirs(&plan, &roots);
        assert_eq!(view.dirs.len(), 1);
        let chosen = filter_by_selection(&plan, &roots, &[view.dirs[0].path.clone()]);
        assert_eq!(chosen.items.len(), 1);
    }

    // ── developer-target pure helpers (v0.84.264) ────────────────────────────

    #[test]
    fn parse_dev_roots_splits_on_commas_and_newlines_trimming_blanks() {
        assert_eq!(
            parse_dev_roots("~/claude\n ~/cursor , ~/dev"),
            vec!["~/claude", "~/cursor", "~/dev"],
        );
        // A trailing/double separator never yields an empty entry.
        assert_eq!(parse_dev_roots("a,,b,\n"), vec!["a", "b"]);
        // Whitespace-only / empty input → no roots (feature disabled, not a
        // single "" root that would scan the cwd).
        assert!(parse_dev_roots("   ").is_empty());
        assert!(parse_dev_roots("").is_empty());
    }

    #[test]
    fn is_command_category_is_exactly_the_tool_driven_ones() {
        for k in [KEY_DOCKER, KEY_SIMCTL, KEY_PNPM, KEY_BREW] {
            assert!(is_command_category(k), "{k} should be command-based");
        }
        // File-based categories are NOT command categories (they have roots the
        // executor deletes; a command category has none).
        for k in [KEY_DUPES, "logs", "dev_caches", "node_modules"] {
            assert!(!is_command_category(k), "{k} must not be command-based");
        }
    }

    #[test]
    fn parse_jetbrains_dir_splits_product_from_version_at_the_first_digit() {
        assert_eq!(
            parse_jetbrains_dir("IntelliJIdea2023.2"),
            Some(("IntelliJIdea".into(), "2023.2".into())),
        );
        assert_eq!(
            parse_jetbrains_dir("PyCharm2024.1"),
            Some(("PyCharm".into(), "2024.1".into())),
        );
        // No digit → no version → not a versioned JetBrains dir.
        assert_eq!(parse_jetbrains_dir("consoles"), None);
        // A leading digit means an empty product → rejected (can't attribute it).
        assert_eq!(parse_jetbrains_dir("2023.2"), None);
        assert_eq!(parse_jetbrains_dir(""), None);
    }

    #[test]
    fn version_key_orders_numerically_not_lexically() {
        // The whole point: "2023.10" is NEWER than "2023.2". A string sort would
        // put ".10" before ".2" and delete the wrong (newest) version.
        assert!(version_key("2023.10") > version_key("2023.2"));
        assert!(version_key("2024.1") > version_key("2023.10"));
        // Non-digit suffixes on a segment are truncated to their leading digits
        // ("2023.2-eap" / "2023.2b" → [2023, 2]); "-" also separates.
        assert_eq!(version_key("2023.2-eap"), vec![2023, 2, 0]);
        assert_eq!(version_key("2023.2b"), vec![2023, 2]);
    }

    #[test]
    fn normalise_product_collapses_app_bundle_and_spacing() {
        // The bundle name and the support-dir name must compare equal.
        assert_eq!(normalise_product("IntelliJ IDEA.app"), "intellijidea");
        assert_eq!(normalise_product("IntelliJIdea"), "intellijidea");
        assert_eq!(normalise_product("IntelliJ IDEA.app"), normalise_product("IntelliJIdea"));
        assert_eq!(normalise_product("PyCharm Community"), "pycharmcommunity");
    }

    #[test]
    fn jetbrains_orphans_keeps_newest_and_flags_uninstalled() {
        let present = vec![
            "IntelliJIdea2023.2".to_string(),
            "IntelliJIdea2023.10".to_string(), // NEWER than .2 (numeric)
            "PyCharm2024.1".to_string(),
            "notaversion".to_string(), // unparseable → never touched
        ];
        // IntelliJ is installed, PyCharm is not.
        let installed = vec!["IntelliJ IDEA.app".to_string()];
        let orphans = jetbrains_orphans(&present, &installed);
        // The older IntelliJ version is an orphan; the newest is KEPT.
        assert!(orphans.contains(&"IntelliJIdea2023.2".to_string()));
        assert!(!orphans.contains(&"IntelliJIdea2023.10".to_string()));
        // The uninstalled product's dir is fully orphaned.
        assert!(orphans.contains(&"PyCharm2024.1".to_string()));
        // Unparseable dirs are never listed.
        assert!(!orphans.contains(&"notaversion".to_string()));
    }

    #[test]
    fn plan_store_rejects_concurrent_jobs_and_reuses_view() {
        let store = PlanStore::default();
        assert_eq!(store.status().phase, "idle");
        assert!(store.begin_scan());
        assert!(!store.begin_scan()); // concurrent scan rejected
        assert_eq!(store.status().phase, "scanning");
        assert!(store.begin_execute().is_err()); // can't execute while scanning

        let plan = CleanPlan {
            items: vec![],
            total_bytes: 42,
            categories: vec![],
        };
        let view = CleanPlanView {
            dirs: vec![],
            total_bytes: 42,
            categories: vec![],
        };
        store.finish_scan(plan, view.clone());
        let st = store.status();
        assert_eq!(st.phase, "idle");
        assert_eq!(st.view.as_ref().map(|v| v.total_bytes), Some(42));

        assert!(store.begin_execute().is_ok());
        assert_eq!(store.status().phase, "executing");
        assert!(!store.begin_scan()); // concurrent scan rejected
        store.finish_execute_ok();
        assert_eq!(store.status().phase, "idle");
        assert!(store.status().view.is_none()); // cleared after successful delete
    }

    #[test]
    fn clean_phase_as_str_is_stable_ipc_contract() {
        assert_eq!(CleanPhase::Idle.as_str(), "idle");
        assert_eq!(CleanPhase::Scanning.as_str(), "scanning");
        assert_eq!(CleanPhase::Executing.as_str(), "executing");
        assert_eq!(ERR_BUSY, "clean.busy");
    }

    #[test]
    fn plan_store_hides_view_while_busy_and_fail_job_returns_idle() {
        let store = PlanStore::default();
        assert!(store.begin_scan());
        // Mid-scan: status must not leak a stale view (cleared on begin_scan).
        assert!(store.status().view.is_none());
        assert_eq!(store.status().phase, "scanning");
        store.fail_job();
        assert_eq!(store.status().phase, "idle");
        // Can start again after fail.
        assert!(store.begin_scan());
        store.fail_job();
    }

    #[test]
    fn plan_store_begin_execute_without_scan_errors() {
        let store = PlanStore::default();
        let err = store.begin_execute().unwrap_err();
        assert!(err.to_lowercase().contains("scan"));
        assert_eq!(store.status().phase, "idle");
    }

    #[test]
    fn plan_store_keeps_view_after_execute_error() {
        let store = PlanStore::default();
        assert!(store.begin_scan());
        store.finish_scan(
            CleanPlan::default(),
            CleanPlanView {
                dirs: vec![],
                total_bytes: 7,
                categories: vec![],
            },
        );
        assert!(store.begin_execute().is_ok());
        store.finish_execute_err();
        assert_eq!(store.status().view.as_ref().map(|v| v.total_bytes), Some(7));
    }
}
