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
    /// Whether it's on by default (the user can still uncheck it).
    pub default_enabled: bool,
}

fn home() -> Option<PathBuf> {
    dirs::home_dir()
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
            default_enabled: true,
        });
    }
    // OS temp (this is std::env::temp_dir — /tmp-equivalent / %TEMP%).
    out.push(Category {
        key: "os_temp".into(),
        label: "OS temporary files".into(),
        level: Level::Safe,
        roots: vec![tmp],
        default_enabled: true,
    });

    #[cfg(target_os = "macos")]
    if let Some(h) = home() {
        out.push(Category {
            key: "browser_cache".into(),
            label: "Browser caches".into(),
            level: Level::Standard,
            roots: vec![
                h.join("Library/Caches/com.apple.Safari"),
                h.join("Library/Caches/Google/Chrome"),
                h.join("Library/Caches/com.google.Chrome"),
                h.join("Library/Caches/Firefox"),
                h.join("Library/Caches/com.microsoft.edgemac"),
            ],
            default_enabled: true,
        });
        out.push(Category {
            key: "logs".into(),
            label: "Application logs".into(),
            level: Level::Standard,
            roots: vec![h.join("Library/Logs")],
            default_enabled: true,
        });
        out.push(Category {
            key: "dev_caches".into(),
            label: "Developer tool caches (npm / pnpm / Gradle / Cargo registry)".into(),
            level: Level::Aggressive,
            roots: vec![
                h.join(".npm/_cacache"),
                h.join("Library/pnpm/store"),
                h.join(".gradle/caches"),
                h.join(".cargo/registry/cache"),
            ],
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
            default_enabled: true,
        });
        out.push(Category {
            key: "dev_caches".into(),
            label: "Developer tool caches (npm / pnpm / Gradle / Cargo registry)".into(),
            level: Level::Aggressive,
            roots: vec![
                h.join("AppData/Roaming/npm-cache/_cacache"),
                local.join("pnpm/store"),
                h.join(".gradle/caches"),
                h.join(".cargo/registry/cache"),
            ],
            default_enabled: false,
        });
    }

    #[cfg(target_os = "linux")]
    if let Some(h) = home() {
        let xdg_cache = dirs::cache_dir().unwrap_or_else(|| h.join(".cache"));
        out.push(Category {
            key: "browser_cache".into(),
            label: "Browser caches".into(),
            level: Level::Standard,
            roots: vec![
                xdg_cache.join("google-chrome"),
                xdg_cache.join("chromium"),
                xdg_cache.join("mozilla"),
            ],
            default_enabled: true,
        });
        out.push(Category {
            key: "dev_caches".into(),
            label: "Developer tool caches (npm / pnpm / Gradle / Cargo registry)".into(),
            level: Level::Aggressive,
            roots: vec![
                h.join(".npm/_cacache"),
                xdg_cache.join("pnpm"),
                h.join(".gradle/caches"),
                h.join(".cargo/registry/cache"),
            ],
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

/// Recursively collect deletable files under `root`. Does **not** follow
/// symlinks (uses `symlink_metadata`); only regular files older than
/// `min_age` (by mtime) are included. Each file's path is containment-checked
/// against `root` so a symlinked subdir can't smuggle outside paths in.
fn collect_files(
    root: &Path,
    min_age: Duration,
    now: SystemTime,
    out: &mut Vec<(PathBuf, u64)>,
) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // lstat — never follow symlinks.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            // Skip symlinks entirely (neither recurse nor delete).
            continue;
        }
        if meta.is_dir() {
            // Only recurse into real subdirs that are genuinely under root.
            if is_contained(&path, root) {
                collect_files(&path, min_age, now, out);
            }
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

/// Build a plan from explicit `(key, label, roots)` groups. Read-only. The
/// pure heart of `scan` — tests drive it with temp dirs.
pub fn scan_roots(
    groups: &[(String, String, Vec<PathBuf>)],
    min_age_days: u32,
    now: SystemTime,
) -> CleanPlan {
    let min_age = Duration::from_secs(u64::from(min_age_days) * 86_400);
    let mut plan = CleanPlan::default();
    for (key, label, roots) in groups {
        let mut cat_bytes = 0u64;
        for root in roots {
            let mut files = Vec::new();
            collect_files(root, min_age, now, &mut files);
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
/// is (still) a real file contained in `allowed_roots` and not a symlink. Any
/// item that fails re-validation is skipped and recorded in `errors`, never
/// aborting the batch.
pub fn execute_plan(plan: &CleanPlan, allowed_roots: &[PathBuf]) -> CleanResult {
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
        if !contained_in_any(&path, allowed_roots) {
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
fn enabled_groups(cfg: &CleanerConfig) -> Vec<(String, String, Vec<PathBuf>)> {
    categories()
        .into_iter()
        .filter(|c| cfg.level.includes(c.level))
        .filter(|c| *cfg.categories.get(&c.key).unwrap_or(&c.default_enabled))
        .map(|c| (c.key, c.label, c.roots))
        .collect()
}

/// All roots that the current config could touch — the allowlist passed to
/// `execute_plan` for re-validation.
fn enabled_roots(cfg: &CleanerConfig) -> Vec<PathBuf> {
    enabled_groups(cfg).into_iter().flat_map(|(_, _, r)| r).collect()
}

/// Read-only scan for the current config. Safe to call any time.
pub fn scan(cfg: &CleanerConfig) -> CleanPlan {
    scan_roots(&enabled_groups(cfg), cfg.min_age_days, SystemTime::now())
}

/// Execute a previously-scanned plan, re-validating against the config's
/// allowlist. The plan should come from `scan(cfg)` with the same `cfg`.
pub fn execute(cfg: &CleanerConfig, plan: &CleanPlan) -> CleanResult {
    execute_plan(plan, &enabled_roots(cfg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

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
        let groups = vec![("logs".into(), "Logs".into(), vec![root.clone()])];
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
        let groups = vec![("logs".into(), "Logs".into(), vec![root.clone()])];
        assert!(scan_roots(&groups, 365, SystemTime::now()).items.is_empty());
        assert_eq!(scan_roots(&groups, 0, SystemTime::now()).items.len(), 1);
    }

    #[test]
    fn scan_recurses_subdirs() {
        let root = tmp();
        let sub = root.join("nested/deep");
        fs::create_dir_all(&sub).unwrap();
        write_old(&sub.join("x.tmp"), b"abcd", 30);
        let groups = vec![("t".into(), "Temp".into(), vec![root.clone()])];
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
        let groups = vec![("c".into(), "Cache".into(), vec![root.clone()])];
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
        let groups = vec![("t".into(), "Temp".into(), vec![root.clone()])];
        // Scan picks up all three (age 0 threshold).
        let mut plan = scan_roots(&groups, 0, SystemTime::now());
        // Remove "keep.txt" from the plan so execute must not touch it.
        plan.items.retain(|i| !i.path.ends_with("keep.txt"));
        plan.total_bytes = plan.items.iter().map(|i| i.size).sum();
        let res = execute_plan(&plan, &[root.clone()]);
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
        let res = execute_plan(&plan, &[root.clone()]);
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
        let res = execute_plan(&plan, &[root.clone()]);
        assert_eq!(res.deleted, 0);
        assert_eq!(res.errors.len(), 1);
        // The symlink wasn't removed and the target is intact.
        assert!(target.exists());
    }

    #[test]
    fn empty_plan_is_a_noop() {
        let res = execute_plan(&CleanPlan::default(), &[]);
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
}
