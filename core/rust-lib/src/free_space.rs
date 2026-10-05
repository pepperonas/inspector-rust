//! Free space on the system disk, for the popup footer (v0.195.0).
//!
//! The figure comes from `sysinfo`, which on macOS reads
//! `VolumeAvailableCapacityForImportantUsage` — the same value System Settings
//! shows under "Storage" ("8,3 GB available of 494,38 GB"). It includes space
//! macOS can free on demand (purgeable caches, local snapshots), so it is
//! larger than `df`'s free column. Windows/Linux report plain free space.
//!
//! Which disk: the volume that holds the HOME folder. On macOS `/Users/…`
//! matches `/` (the firmlinked data volume is mounted elsewhere), and every
//! APFS volume in the container reports the same shared free space, so the
//! number is the one System Settings shows.

use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct FreeSpace {
    /// Volume name as the OS shows it ("Macintosh HD", "Local Disk").
    pub name: String,
    pub mount: String,
    /// Bytes available to the user right now.
    pub available: u64,
    pub total: u64,
}

/// One mounted disk: `(name, mount, total, available)`.
pub type DiskRow = (String, String, u64, u64);

/// Normalise a path for prefix matching: Windows separators become `/`, and
/// drive letters compare case-insensitively.
fn norm(p: &str) -> String {
    let s = p.replace('\\', "/");
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        s.to_ascii_lowercase()
    } else {
        s
    }
}

/// Pick the disk that holds `home`: the longest mount point that is a prefix
/// of it on a path boundary (so `/Vol` never matches `/Volumes`). Falls back to
/// the root mount, then to the first disk — a footer that shows *some* real
/// number beats one that shows nothing. Disks reporting a total of 0 are
/// ignored (virtual/pseudo filesystems). Pure.
pub fn pick(home: Option<&str>, disks: &[DiskRow]) -> Option<FreeSpace> {
    let real: Vec<&DiskRow> = disks.iter().filter(|d| d.2 > 0).collect();
    let mut best: Option<&DiskRow> = None;
    if let Some(home) = home {
        let h = norm(home);
        for d in &real {
            let m = norm(&d.1);
            let is_prefix = h == m
                || (h.starts_with(&m) && (m.ends_with('/') || h.as_bytes().get(m.len()) == Some(&b'/')));
            if is_prefix && best.is_none_or(|b| m.len() > norm(&b.1).len()) {
                best = Some(d);
            }
        }
    }
    let chosen = best
        .or_else(|| real.iter().copied().find(|d| d.1 == "/"))
        .or_else(|| real.first().copied())?;
    Some(FreeSpace {
        name: chosen.0.clone(),
        mount: chosen.1.clone(),
        total: chosen.2,
        // Never claim more free than the disk holds (a stale/odd reading
        // would render as ">100 %" in the tooltip).
        available: chosen.3.min(chosen.2),
    })
}

/// Impure entry point: list the mounted disks and pick the home volume.
pub fn current() -> Option<FreeSpace> {
    let disks: Vec<DiskRow> = sysinfo::Disks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|d| {
            (
                d.name().to_string_lossy().into_owned(),
                d.mount_point().to_string_lossy().into_owned(),
                d.total_space(),
                d.available_space(),
            )
        })
        .collect();
    let home = dirs::home_dir().map(|p| p.to_string_lossy().into_owned());
    pick(home.as_deref(), &disks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, mount: &str, total: u64, avail: u64) -> DiskRow {
        (name.into(), mount.into(), total, avail)
    }

    /// The real list sysinfo reported on the maintainer's Mac (2026-10-05).
    fn mac() -> Vec<DiskRow> {
        vec![
            row("Macintosh HD", "/", 494_384_795_648, 7_915_089_920),
            row("Macintosh HD", "/System/Volumes/Data", 494_384_795_648, 7_915_089_920),
            row("Untitled", "/Volumes/Untitled", 494_384_795_648, 7_915_089_920),
            row("Samsung SSD", "/Volumes/Samsung SSD", 499_898_105_856, 411_634_127_675),
        ]
    }

    #[test]
    fn mac_home_matches_the_system_disk() {
        let f = pick(Some("/Users/martin"), &mac()).unwrap();
        assert_eq!(f.mount, "/");
        assert_eq!(f.name, "Macintosh HD");
        assert_eq!(f.available, 7_915_089_920);
        assert_eq!(f.total, 494_384_795_648);
    }

    #[test]
    fn longest_mount_wins_and_never_on_a_partial_segment() {
        let disks = vec![
            row("root", "/", 100, 10),
            row("data", "/System/Volumes/Data", 200, 20),
            row("vol", "/Vol", 300, 30),
        ];
        assert_eq!(pick(Some("/System/Volumes/Data/Users/x"), &disks).unwrap().name, "data");
        // `/Vol` is a string prefix of `/Volumes/…` but not a path prefix.
        assert_eq!(pick(Some("/Volumes/Ext/x"), &disks).unwrap().name, "root");
    }

    #[test]
    fn external_disk_holding_home_is_chosen() {
        let f = pick(Some("/Volumes/Samsung SSD/home"), &mac()).unwrap();
        assert_eq!(f.name, "Samsung SSD");
    }

    #[test]
    fn windows_drive_letters_match_case_insensitively() {
        let disks = vec![row("Local Disk", "C:\\", 500, 50), row("Data", "D:\\", 900, 90)];
        assert_eq!(pick(Some("c:\\Users\\martin"), &disks).unwrap().name, "Local Disk");
        assert_eq!(pick(Some("D:\\Work"), &disks).unwrap().name, "Data");
    }

    #[test]
    fn falls_back_to_root_then_first_disk() {
        let disks = vec![row("ext", "/mnt/ext", 50, 5), row("root", "/", 100, 10)];
        assert_eq!(pick(None, &disks).unwrap().name, "root");
        let no_root = vec![row("ext", "/mnt/ext", 50, 5)];
        assert_eq!(pick(Some("/home/x"), &no_root).unwrap().name, "ext");
        assert!(pick(Some("/home/x"), &[]).is_none());
    }

    #[test]
    fn pseudo_filesystems_with_zero_size_are_ignored() {
        let disks = vec![row("devfs", "/", 0, 0), row("real", "/data", 100, 10)];
        assert_eq!(pick(Some("/home"), &disks).unwrap().name, "real");
    }

    #[test]
    fn available_never_exceeds_total() {
        let f = pick(Some("/"), &[row("odd", "/", 100, 150)]).unwrap();
        assert_eq!(f.available, 100);
    }

    /// Live: prints what the footer would show. `-- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn free_space_live() {
        println!("{:?}", current());
    }
}
