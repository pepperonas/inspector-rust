//! `disk` / `daisy` — DaisyDisk-style disk-usage visualisation (v0.120.0).
//!
//! DaisyDisk scans a volume/folder and draws the usage as a **concentric
//! sunburst**: each ring is a directory level, each segment a folder/file,
//! its angular span proportional to the space it occupies; the centre shows
//! the volume's free/used space, clicking a segment drills in. This module is
//! the scanner behind that view.
//!
//! **On-disk size, not apparent size.** Like DaisyDisk we report the space a
//! file actually occupies — `blocks × 512` on Unix (`MetadataExt::blocks`) —
//! so sparse files and block rounding match what the volume readout says. The
//! walk does NOT follow symlinks (`symlink_metadata`) and STAYS ON ONE
//! FILESYSTEM (same `st_dev` as the root) so scanning `~` never wanders into a
//! network mount or `/Volumes/*`. Permission errors are skipped (counted 0),
//! never fatal — scanning `/` hits many protected trees and must still finish.
//!
//! **Bounded payload, client-side drill.** The full tree can be millions of
//! nodes, so `prune` produces a bounded view once (depth ≤ `MAX_DEPTH`, ≤
//! `MAX_CHILDREN` per node with the remainder folded into a synthetic
//! `Other`, a global node cap) and the frontend drills within it — instant,
//! no managed state, no re-scan per click. The `top_files` list is collected
//! across the WHOLE walk (independent of pruning) so "largest files" is
//! honest even for files pruned out of the chart.
//!
//! House style: the walk is the impure shell; `prune`, `fold_children` and the volume-matching are pure with a file-final test
//! module.

use serde::Serialize;
use std::cmp::Reverse;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Max ring depth kept in the pruned view (DaisyDisk shows ~5 rings).
pub const MAX_DEPTH: usize = 6;
/// Max real children per node before the rest folds into `Other`.
pub const MAX_CHILDREN: usize = 24;
/// Safety cap on total pruned nodes (keeps the payload a few hundred KB).
pub const MAX_NODES: usize = 20_000;
/// How many largest files to surface.
pub const TOP_FILES: usize = 30;

/// A node of the FULL in-memory tree (built by the walk, then pruned away).
struct Raw {
    name: String,
    size: u64,
    is_dir: bool,
    children: Vec<Raw>,
}

/// A pruned node sent to the frontend. `other` marks the synthetic
/// aggregate segment (not drillable); `child_count` is the real number of
/// children before pruning (shown in the detail readout).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DiskNode {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub other: bool,
    pub child_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<DiskNode>,
}

#[derive(Serialize, Clone, Debug)]
pub struct DiskScan {
    /// Absolute path scanned.
    pub root_path: String,
    /// Display label (the folder's own name).
    pub root_name: String,
    /// Total on-disk bytes under the root.
    pub total: u64,
    /// Containing volume, when matched (DaisyDisk's centre free/used).
    pub volume_mount: String,
    pub volume_total: u64,
    pub volume_free: u64,
    /// True when the scanned path IS the volume's mount point — only then does
    /// free space belong "inside" the chart as a segment.
    pub is_volume_root: bool,
    pub tree: DiskNode,
    pub top_files: Vec<TopFile>,
    /// Files/dirs visited (for the "N items scanned" footer).
    pub items: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TopFile {
    pub path: String,
    pub size: u64,
}

/// On-disk size of a single entry's metadata (Unix: allocated blocks).
#[cfg(unix)]
fn on_disk_size(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.blocks() * 512
}
#[cfg(not(unix))]
fn on_disk_size(meta: &std::fs::Metadata) -> u64 {
    meta.len()
}

#[cfg(unix)]
fn device_of(meta: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    meta.dev()
}
#[cfg(not(unix))]
fn device_of(_meta: &std::fs::Metadata) -> u64 {
    0
}

/// Live progress the walk publishes (throttled by the caller into events).
#[derive(Default)]
pub struct ScanProgress {
    pub items: AtomicU64,
    pub bytes: AtomicU64,
}

/// Recursively size a directory. Impure (touches the FS); everything it feeds
/// is pure. `root_dev` pins the filesystem; `top` accumulates the largest
/// files; `progress` is bumped per entry so the UI can show a live count.
fn walk(
    dir: &Path,
    depth: usize,
    root_dev: u64,
    top: &mut Vec<TopFile>,
    progress: &ScanProgress,
) -> Raw {
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.to_string_lossy().into_owned());
    let mut node = Raw { name, size: 0, is_dir: true, children: Vec::new() };

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return node, // permission denied etc. → empty dir, not fatal
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let meta = match entry.metadata() {
            // entry.metadata() does NOT traverse symlinks (unlike fs::metadata),
            // so a symlink is sized as the link itself and never followed.
            Ok(m) => m,
            Err(_) => continue,
        };
        // Don't cross filesystem boundaries (mounts inside the tree).
        if device_of(&meta) != root_dev {
            continue;
        }
        progress.items.fetch_add(1, Ordering::Relaxed);
        if meta.is_dir() {
            let child = walk(&path, depth + 1, root_dev, top, progress);
            node.size += child.size;
            node.children.push(child);
        } else {
            let sz = on_disk_size(&meta);
            node.size += sz;
            progress.bytes.fetch_add(sz, Ordering::Relaxed);
            let child_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            node.children.push(Raw { name: child_name, size: sz, is_dir: false, children: Vec::new() });
            consider_top(top, &path, sz);
        }
    }
    let _ = depth;
    node
}

/// Keep a bounded max-heap-ish list of the largest files (simple: push, sort,
/// truncate when it grows past 2×TOP_FILES so sorts stay cheap).
fn consider_top(top: &mut Vec<TopFile>, path: &Path, size: u64) {
    if size == 0 {
        return;
    }
    if top.len() < TOP_FILES {
        top.push(TopFile { path: path.to_string_lossy().into_owned(), size });
    } else if size > top.iter().map(|t| t.size).min().unwrap_or(0) {
        top.push(TopFile { path: path.to_string_lossy().into_owned(), size });
        if top.len() > TOP_FILES * 2 {
            top.sort_by_key(|t| Reverse(t.size));
            top.truncate(TOP_FILES);
        }
    }
}

/// Pure: full tree → bounded view. Sorts children by size desc, keeps the top
/// `MAX_CHILDREN` (folding the rest into `Other`), stops at `MAX_DEPTH`, and
/// respects a global node budget. Files below the fold in a dir vanish from
/// the chart but their size still counts (via `Other`).
fn prune(raw: Raw, depth: usize, budget: &mut usize) -> DiskNode {
    let child_count = raw.children.len();
    let mut out = DiskNode {
        name: raw.name,
        size: raw.size,
        is_dir: raw.is_dir,
        other: false,
        child_count,
        children: Vec::new(),
    };
    if depth >= MAX_DEPTH || *budget == 0 || raw.children.is_empty() {
        return out;
    }
    let mut kids = raw.children;
    kids.sort_by_key(|r| Reverse(r.size));
    let keep = fold_children(&kids, MAX_CHILDREN);
    let (shown, folded_size) = keep;
    for child in kids.into_iter().take(shown) {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        // Skip zero-size children — they'd be invisible slivers.
        if child.size == 0 {
            continue;
        }
        out.children.push(prune(child, depth + 1, budget));
    }
    if folded_size > 0 {
        out.children.push(DiskNode {
            name: "Sonstiges".into(),
            size: folded_size,
            is_dir: false,
            other: true,
            child_count: 0,
            children: Vec::new(),
        });
    }
    out
}

/// Pure: given size-sorted children, decide how many to show and the combined
/// size of the rest. Returns `(shown_count, folded_size)`. If only one would
/// be folded, show it instead (an "Other" of a single item is silly).
fn fold_children(sorted: &[Raw], cap: usize) -> (usize, u64) {
    if sorted.len() <= cap {
        return (sorted.len(), 0);
    }
    // Show cap-1 real segments + one "Other", UNLESS the remainder is a single
    // item (then just show all `cap`).
    if sorted.len() == cap + 1 {
        return (sorted.len(), 0);
    }
    let shown = cap - 1;
    let folded: u64 = sorted[shown..].iter().map(|r| r.size).sum();
    (shown, folded)
}

/// Match a scanned path to its containing volume among the mounted disks,
/// returning `(mount, total, free, is_mount_root)`. Pure over the disk list.
pub fn match_volume(
    path: &str,
    disks: &[(String, u64, u64)],
) -> Option<(String, u64, u64, bool)> {
    // Longest mount-point that is a prefix of `path` wins (so `/Users` beats
    // `/`). Compare on path boundaries to avoid `/Vol` matching `/Volumes`.
    let mut best: Option<&(String, u64, u64)> = None;
    for d in disks {
        let mount = &d.0;
        let is_prefix = path == mount
            || (path.starts_with(mount)
                && (mount.ends_with('/') || path.as_bytes().get(mount.len()) == Some(&b'/')));
        if is_prefix && best.is_none_or(|b| mount.len() > b.0.len()) {
            best = Some(d);
        }
    }
    best.map(|(mount, total, free)| (mount.clone(), *total, *free, path == mount))
}

/// Impure entry point: scan `root`, returning the bounded view + top files +
/// volume info. `disks` is `(mount, total, free)` from the caller (sysinfo),
/// passed in so this stays testable.
pub fn scan(
    root: &Path,
    disks: &[(String, u64, u64)],
    progress: &ScanProgress,
) -> Result<DiskScan, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("Pfad nicht lesbar: {e}"))?;
    let meta = std::fs::symlink_metadata(&root).map_err(|e| format!("Pfad nicht lesbar: {e}"))?;
    if !meta.is_dir() {
        return Err("Kein Ordner — bitte einen Ordner angeben.".into());
    }
    let root_dev = device_of(&meta);
    let mut top: Vec<TopFile> = Vec::new();
    let raw = walk(&root, 0, root_dev, &mut top, progress);
    top.sort_by_key(|t| Reverse(t.size));
    top.truncate(TOP_FILES);

    let total = raw.size;
    let root_name = if raw.name.is_empty() { "/".into() } else { raw.name.clone() };
    let items = progress.items.load(Ordering::Relaxed);
    let mut budget = MAX_NODES;
    let tree = prune(raw, 0, &mut budget);

    let root_str = root.to_string_lossy().into_owned();
    let (volume_mount, volume_total, volume_free, is_volume_root) =
        match_volume(&root_str, disks).unwrap_or_default();

    Ok(DiskScan {
        root_path: root_str,
        root_name,
        total,
        volume_mount,
        volume_total,
        volume_free,
        is_volume_root,
        tree,
        top_files: top,
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(name: &str, size: u64, dir: bool) -> Raw {
        Raw { name: name.into(), size, is_dir: dir, children: Vec::new() }
    }

    #[test]
    fn fold_children_keeps_all_when_within_cap() {
        let kids: Vec<Raw> = (0..5).map(|i| raw(&format!("f{i}"), 100, false)).collect();
        assert_eq!(fold_children(&kids, 24), (5, 0));
    }

    #[test]
    fn fold_children_aggregates_the_tail_but_not_a_lone_extra() {
        // cap+1 items → show all (an "Other" of one item is silly).
        let kids: Vec<Raw> = (0..25).map(|_| raw("x", 10, false)).collect();
        assert_eq!(fold_children(&kids, 24), (25, 0));
        // cap+2 → show cap-1 real + fold the rest.
        let kids: Vec<Raw> = (0..30).map(|_| raw("x", 10, false)).collect();
        let (shown, folded) = fold_children(&kids, 24);
        assert_eq!(shown, 23);
        assert_eq!(folded, (30 - 23) * 10);
    }

    #[test]
    fn prune_folds_tail_into_other_and_carries_full_size() {
        let mut root = raw("root", 0, true);
        for i in 0..40 {
            root.children.push(raw(&format!("d{i}"), (40 - i) * 1000, true));
        }
        root.size = root.children.iter().map(|c| c.size).sum();
        let mut budget = MAX_NODES;
        let pruned = prune(root, 0, &mut budget);
        // 23 real + 1 "Sonstiges".
        assert_eq!(pruned.children.len(), 24);
        assert!(pruned.children.last().unwrap().other);
        // The rings never lose total size: shown + other == parent.
        let sum: u64 = pruned.children.iter().map(|c| c.size).sum();
        assert_eq!(sum, pruned.size);
        assert_eq!(pruned.child_count, 40); // real count preserved for the readout
    }

    #[test]
    fn prune_stops_at_max_depth() {
        // Build a chain deeper than MAX_DEPTH.
        let mut node = raw("leaf", 100, true);
        for i in 0..(MAX_DEPTH + 3) {
            let mut parent = raw(&format!("l{i}"), 100, true);
            parent.children.push(node);
            node = parent;
        }
        let mut budget = MAX_NODES;
        let pruned = prune(node, 0, &mut budget);
        // Walk down; nothing should exist past MAX_DEPTH rings.
        let mut d = 0;
        let mut cur = &pruned;
        while let Some(next) = cur.children.first() {
            cur = next;
            d += 1;
        }
        assert!(d <= MAX_DEPTH, "depth {d} exceeded MAX_DEPTH {MAX_DEPTH}");
    }

    #[test]
    fn prune_drops_zero_size_slivers() {
        let mut root = raw("root", 500, true);
        root.children.push(raw("real", 500, false));
        root.children.push(raw("empty", 0, false));
        let mut budget = MAX_NODES;
        let pruned = prune(root, 0, &mut budget);
        assert_eq!(pruned.children.len(), 1);
        assert_eq!(pruned.children[0].name, "real");
    }

    #[test]
    fn volume_matching_prefers_the_longest_mount_on_path_boundaries() {
        let disks = vec![
            ("/".to_string(), 1000, 400),
            ("/Users".to_string(), 900, 300),
            ("/Volumes/Ext".to_string(), 2000, 1000),
        ];
        // /Users beats / for a home path.
        let m = match_volume("/Users/martin/claude", &disks).unwrap();
        assert_eq!(m.0, "/Users");
        assert!(!m.3); // not the mount root
        // Exact mount → is_volume_root.
        assert!(match_volume("/Users", &disks).unwrap().3);
        // Boundary: "/Vol" must NOT match "/Volumes/Ext".
        let root = match_volume("/Vol", &disks).unwrap();
        assert_eq!(root.0, "/");
        // The external volume matches its own tree.
        assert_eq!(match_volume("/Volumes/Ext/x", &disks).unwrap().0, "/Volumes/Ext");
        // No disks → None.
        assert!(match_volume("/x", &[]).is_none());
    }

    #[test]
    fn scan_a_real_temp_tree_sizes_and_finds_top_files() {
        let base = std::env::temp_dir().join(format!("ir-disk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("sub")).unwrap();
        std::fs::write(base.join("big.bin"), vec![0u8; 200 * 1024]).unwrap();
        std::fs::write(base.join("sub/small.txt"), b"hello").unwrap();
        let prog = ScanProgress::default();
        let scan = super::scan(&base, &[], &prog).unwrap();
        let _ = std::fs::remove_dir_all(&base);

        assert!(scan.total >= 200 * 1024, "total {} too small", scan.total);
        // big.bin is the largest file.
        assert!(scan.top_files[0].path.ends_with("big.bin"));
        // Tree has the two entries (dir + file), sorted size desc → file first.
        assert_eq!(scan.tree.child_count, 2);
        assert_eq!(scan.tree.children[0].name, "big.bin");
        assert!(scan.items >= 3);
    }

    #[test]
    fn scan_rejects_a_file_path() {
        let f = std::env::temp_dir().join(format!("ir-disk-file-{}.txt", std::process::id()));
        std::fs::write(&f, b"x").unwrap();
        let prog = ScanProgress::default();
        let err = super::scan(&f, &[], &prog).unwrap_err();
        let _ = std::fs::remove_file(&f);
        assert!(err.contains("Kein Ordner"));
    }
}
