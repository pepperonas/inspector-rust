//! Writes into a recording project folder that never follow a planted symlink.
//!
//! Project folders can be opened from anywhere ("Open project…"), so a file or a folder name
//! inside one may be a symlink someone else put there. Every write the app or the render
//! helper makes into a project goes through here:
//!
//! - [`open_new_nofollow`]: a NEW file only (`create_new` = O_EXCL, which fails on anything
//!   already at the path — a link included, dangling or not) and O_NOFOLLOW on Unix.
//! - [`write_atomic`]: a uniquely named temp file next to the target through
//!   [`open_new_nofollow`], fsync, rename. A rename replaces a link AT the target, it never
//!   writes through it.
//! - [`plain_dir`]: a subfolder (`assets/`, `takes/`, `takes/<id>`) that is a real directory —
//!   a symlinked one is refused, a missing one is created.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Refusal: a path in the project is a symlink (or not what it should be).
pub const ERR_LINKED: &str = "a project path is a symlink or not a plain directory";

/// Creates `path` as a new file; anything already there (a symlink included) is an error.
pub fn open_new_nofollow(path: &Path) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(libc::O_NOFOLLOW).mode(0o644);
    }
    opts.open(path)
}

fn unique_suffix() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("{:x}-{:x}-{:x}", std::process::id(), nanos, NEXT.fetch_add(1, Ordering::Relaxed))
}

/// A fresh temp file next to `target` (same folder, so the rename is atomic).
pub fn create_temp(target: &Path) -> io::Result<(PathBuf, File)> {
    let dir = target.parent().unwrap_or(Path::new("."));
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut last = None;
    for _ in 0..16 {
        let tmp = dir.join(format!(".{name}.{}.tmp", unique_suffix()));
        match open_new_nofollow(&tmp) {
            Ok(f) => return Ok((tmp, f)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| io::Error::other("no free temp name")))
}

/// Writes `bytes` to `target` atomically: temp file, fsync, rename.
pub fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let (tmp, mut f) = create_temp(target)?;
    let res = (|| {
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, target)
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// `parent/name` as a real directory: created when missing, refused when it is a symlink or
/// anything other than a directory.
pub fn plain_dir(parent: &Path, name: &str) -> io::Result<PathBuf> {
    let p = parent.join(name);
    match std::fs::symlink_metadata(&p) {
        Ok(m) if m.file_type().is_dir() => return Ok(p),
        Ok(_) => return Err(io::Error::new(io::ErrorKind::PermissionDenied, ERR_LINKED)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    match std::fs::create_dir(&p) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    // Re-check: something may have been put there between the look and the create.
    match std::fs::symlink_metadata(&p) {
        Ok(m) if m.file_type().is_dir() => Ok(p),
        _ => Err(io::Error::new(io::ErrorKind::PermissionDenied, ERR_LINKED)),
    }
}

/// `true` when `path` exists as anything, a dangling symlink included (`Path::exists` says
/// `false` for a dangling link, so it must never decide whether a name is free).
pub fn taken(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ir-safe-fs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_planted_link_is_never_written_through() {
        let d = tmp("link");
        let outside = tmp("link-outside");
        let victim = outside.join("victim.txt");
        std::fs::write(&victim, b"keep me").unwrap();
        // A link at the exact path: refused, existing target or dangling.
        symlink(&victim, d.join("at")).unwrap();
        assert!(open_new_nofollow(&d.join("at")).is_err());
        let fresh = outside.join("would-be-created");
        symlink(&fresh, d.join("dangling")).unwrap();
        assert!(open_new_nofollow(&d.join("dangling")).is_err());
        assert!(!fresh.exists(), "a dangling link does not create its target");
        assert!(taken(&d.join("dangling")) && !d.join("dangling").exists());
        // A link AS the target of an atomic write is replaced, not followed.
        symlink(&victim, d.join("target.json")).unwrap();
        write_atomic(&d.join("target.json"), b"new").unwrap();
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
        assert!(!std::fs::symlink_metadata(d.join("target.json")).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(d.join("target.json")).unwrap(), b"new");
        let left: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp")).collect();
        assert!(left.is_empty(), "{left:?}");
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn a_symlinked_folder_is_refused() {
        let d = tmp("dir");
        let outside = tmp("dir-outside");
        symlink(&outside, d.join("assets")).unwrap();
        assert!(plain_dir(&d, "assets").is_err());
        std::fs::write(d.join("takes"), b"x").unwrap();
        assert!(plain_dir(&d, "takes").is_err(), "a file is not a folder");
        let p = plain_dir(&d, "new").unwrap();
        assert!(p.is_dir());
        assert_eq!(plain_dir(&d, "new").unwrap(), p, "an existing real folder is fine");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
