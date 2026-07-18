//! Meme picker (v0.70.0) — browse a folder of GIFs/images from the search
//! bar with `meme [fuzzy query]`, preview the selected one (animated), and
//! copy it to the clipboard on Enter.
//!
//! The library lives in a folder of category sub-folders
//! (`<dir>/<category>/<name>.gif`); [`scan`] walks it recursively and returns
//! one [`MemeEntry`] per image file, carrying the file stem (`name`), the
//! immediate parent folder (`category`), and the absolute `path`. Fuzzy
//! ranking happens in the frontend (`lib/meme.ts`).
//!
//! **Copy** puts the actual *file* on the clipboard (macOS: an `NSURL`
//! file-URL on `NSPasteboard`) so pasting into a chat app / Finder keeps the
//! animation, rather than a flattened still frame.

use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::db::DbHandle;

/// Settings key for a user-overridable meme directory.
pub const KEY_MEME_DIR: &str = "meme.dir";

/// Default meme library location, resolved relative to the user's home so it
/// works on every OS and user (macOS `/Users/<u>/My Drive/media/memes`,
/// Windows `C:\Users\<u>\My Drive\media\memes`). Google-Drive-for-Desktop in
/// *streaming* mode mounts under a drive letter instead (e.g. `G:\My Drive`),
/// so the directory is **overridable** via the `meme.dir` setting
/// (Settings → Meme library). Falls back to the literal `My Drive/...` if the
/// home dir can't be resolved.
pub fn default_meme_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join("My Drive")
        .join("media")
        .join("memes")
}

/// Image extensions we treat as memes (lower-cased compare).
const MEME_EXTS: &[&str] = &["gif", "png", "jpg", "jpeg", "webp", "bmp", "apng"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MemeEntry {
    /// File stem (filename without extension) — the primary display + match key.
    pub name: String,
    /// Immediate parent folder name relative to the library root (the
    /// "category"), or "" when the file sits directly in the root.
    pub category: String,
    /// Absolute path to the file.
    pub path: String,
}

fn has_meme_ext(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| MEME_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Recursively collect meme files under `root`. Pure (filesystem only) so it
/// can be unit-tested against a temp fixture. Hidden files (dot-prefixed) and
/// non-image files are skipped; symlinks are not followed. Results are sorted
/// by category then name for a stable list.
pub fn scan(root: &Path) -> Vec<MemeEntry> {
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort_by(|a, b| a.category.cmp(&b.category).then_with(|| a.name.cmp(&b.name)));
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<MemeEntry>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if name_str.starts_with('.') {
            continue; // skip .DS_Store, dot-dirs, etc.
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            walk(root, &path, out);
        } else if meta.is_file() && has_meme_ext(&path) {
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| name_str.to_string());
            // Category = the file's immediate parent folder name, unless that
            // parent IS the root (then "").
            let category = path
                .parent()
                .filter(|p| *p != root)
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            out.push(MemeEntry {
                name,
                category,
                path: path.to_string_lossy().to_string(),
            });
        }
    }
}

/// The configured meme directory (the `meme.dir` setting, or the default).
/// A blank/whitespace-only setting collapses to the default.
pub fn meme_dir(db: &DbHandle) -> PathBuf {
    match crate::settings::get(db, KEY_MEME_DIR) {
        Ok(Some(s)) if !s.trim().is_empty() => PathBuf::from(s.trim()),
        _ => default_meme_dir(),
    }
}

/// Scan the configured library.
pub fn list(db: &DbHandle) -> Vec<MemeEntry> {
    scan(&meme_dir(db))
}

/// Copy the meme file at `path` to the clipboard so it can be pasted
/// elsewhere (animation preserved). macOS writes a file-URL to `NSPasteboard`;
/// other platforms fall back to copying the decoded image (a still frame).
pub fn copy_to_clipboard(path: &str) -> Result<(), String> {
    if !Path::new(path).is_file() {
        return Err(format!("meme not found: {path}"));
    }
    #[cfg(target_os = "macos")]
    {
        macos_copy_file(path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        use clipboard_rs::{common::RustImage, Clipboard, ClipboardContext, RustImageData};
        let bytes = std::fs::read(path).map_err(|e| format!("read meme: {e}"))?;
        let ctx = ClipboardContext::new().map_err(|e| format!("clipboard: {e:?}"))?;
        let img = RustImageData::from_bytes(&bytes).map_err(|e| format!("decode: {e:?}"))?;
        ctx.set_image(img).map_err(|e| format!("set_image: {e:?}"))?;
        Ok(())
    }
}

/// Put the file on the macOS pasteboard as an `NSURL` file-URL (the canonical
/// "copy a file" — pasting into Finder / chat apps attaches the actual GIF).
#[cfg(target_os = "macos")]
fn macos_copy_file(path: &str) -> Result<(), String> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use std::ffi::CString;

    let cpath = CString::new(path).map_err(|_| "path has interior NUL".to_string())?;
    unsafe {
        let pb_cls = AnyClass::get(c"NSPasteboard").ok_or("NSPasteboard class missing")?;
        let str_cls = AnyClass::get(c"NSString").ok_or("NSString class missing")?;
        let url_cls = AnyClass::get(c"NSURL").ok_or("NSURL class missing")?;
        let arr_cls = AnyClass::get(c"NSArray").ok_or("NSArray class missing")?;

        let pb: *mut AnyObject = msg_send![pb_cls, generalPasteboard];
        if pb.is_null() {
            return Err("generalPasteboard returned nil".into());
        }
        let _: i64 = msg_send![pb, clearContents];
        let nsstr: *mut AnyObject = msg_send![str_cls, stringWithUTF8String: cpath.as_ptr()];
        let url: *mut AnyObject = msg_send![url_cls, fileURLWithPath: nsstr];
        if url.is_null() {
            return Err("fileURLWithPath returned nil".into());
        }
        let arr: *mut AnyObject = msg_send![arr_cls, arrayWithObject: url];
        let ok: bool = msg_send![pb, writeObjects: arr];
        if ok {
            Ok(())
        } else {
            Err("NSPasteboard writeObjects failed".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp() -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "ir-meme-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    #[test]
    fn scans_nested_categories_and_extracts_name_plus_category() {
        let root = tmp();
        fs::create_dir_all(root.join("cats")).unwrap();
        fs::create_dir_all(root.join("dogs")).unwrap();
        fs::write(root.join("cats/grumpy.gif"), b"x").unwrap();
        fs::write(root.join("dogs/doge.png"), b"x").unwrap();
        fs::write(root.join("top-level.gif"), b"x").unwrap();
        let memes = scan(&root);
        assert_eq!(memes.len(), 3);
        // sorted by (category, name): "" (top-level) < "cats" < "dogs"
        assert_eq!(memes[0].name, "top-level");
        assert_eq!(memes[0].category, "");
        assert_eq!(memes[1].name, "grumpy");
        assert_eq!(memes[1].category, "cats");
        assert_eq!(memes[2].name, "doge");
        assert_eq!(memes[2].category, "dogs");
    }

    #[test]
    fn skips_non_images_hidden_and_keeps_known_exts() {
        let root = tmp();
        fs::write(root.join("a.gif"), b"x").unwrap();
        fs::write(root.join("b.JPG"), b"x").unwrap(); // case-insensitive ext
        fs::write(root.join("c.webp"), b"x").unwrap();
        fs::write(root.join("notes.txt"), b"x").unwrap(); // skipped
        fs::write(root.join(".DS_Store"), b"x").unwrap(); // skipped (hidden)
        let names: Vec<_> = scan(&root).into_iter().map(|m| m.name).collect();
        assert!(names.contains(&"a".to_string()));
        assert!(names.contains(&"b".to_string()));
        assert!(names.contains(&"c".to_string()));
        assert!(!names.iter().any(|n| n == "notes"));
        assert_eq!(scan(&root).len(), 3);
    }

    #[test]
    fn missing_dir_yields_empty_not_error() {
        let root = tmp().join("does-not-exist");
        assert!(scan(&root).is_empty());
    }

    #[test]
    fn has_meme_ext_is_case_insensitive() {
        assert!(has_meme_ext(Path::new("x.GIF")));
        assert!(has_meme_ext(Path::new("x.WebP")));
        assert!(!has_meme_ext(Path::new("x.txt")));
        assert!(!has_meme_ext(Path::new("x")));
    }

    #[test]
    fn has_meme_ext_covers_apng_and_bmp_but_not_lookalikes() {
        assert!(has_meme_ext(Path::new("x.apng")));
        assert!(has_meme_ext(Path::new("x.bmp")));
        // Similar-but-unsupported formats stay out.
        assert!(!has_meme_ext(Path::new("x.svg")));
        assert!(!has_meme_ext(Path::new("x.gifv")));
        assert!(!has_meme_ext(Path::new("x.gif.txt")), "only the final ext counts");
    }

    #[test]
    fn category_is_the_immediate_parent_not_the_full_relative_path() {
        let root = tmp();
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::write(root.join("a/b/deep.gif"), b"x").unwrap();
        let memes = scan(&root);
        assert_eq!(memes.len(), 1);
        assert_eq!(memes[0].category, "b", "not \"a/b\" and not \"a\"");
        assert_eq!(memes[0].name, "deep");
    }

    #[test]
    fn unicode_names_and_categories_survive() {
        let root = tmp();
        fs::create_dir_all(root.join("Tiere & Co")).unwrap();
        fs::write(root.join("Tiere & Co/Grüße 🎉.gif"), b"x").unwrap();
        let memes = scan(&root);
        assert_eq!(memes.len(), 1);
        assert_eq!(memes[0].name, "Grüße 🎉");
        assert_eq!(memes[0].category, "Tiere & Co");
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        let root = tmp();
        let outside = tmp();
        fs::write(outside.join("real.gif"), b"x").unwrap();
        // A symlinked file AND a symlinked directory — neither may be scanned
        // (a link cycle or an escape out of the library must be impossible).
        std::os::unix::fs::symlink(outside.join("real.gif"), root.join("link.gif")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("linkdir")).unwrap();
        assert!(scan(&root).is_empty());
    }

    #[test]
    fn hidden_directories_are_skipped_entirely() {
        let root = tmp();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".git/icon.png"), b"x").unwrap();
        fs::write(root.join("ok.png"), b"x").unwrap();
        let memes = scan(&root);
        assert_eq!(memes.len(), 1);
        assert_eq!(memes[0].name, "ok");
    }

    fn mem_db() -> DbHandle {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let h = std::sync::Arc::new(parking_lot::Mutex::new(conn));
        crate::settings::init_table(&h).unwrap();
        h
    }

    #[test]
    fn meme_dir_setting_overrides_blank_falls_back_and_trims() {
        let db = mem_db();
        // No setting → default.
        assert_eq!(meme_dir(&db), default_meme_dir());
        // Explicit dir wins.
        crate::settings::set(&db, KEY_MEME_DIR, "/custom/memes").unwrap();
        assert_eq!(meme_dir(&db), PathBuf::from("/custom/memes"));
        // Surrounding whitespace is trimmed (a pasted path with a stray space).
        crate::settings::set(&db, KEY_MEME_DIR, "  /other/dir  ").unwrap();
        assert_eq!(meme_dir(&db), PathBuf::from("/other/dir"));
        // Blank / whitespace-only collapses back to the default.
        crate::settings::set(&db, KEY_MEME_DIR, "   ").unwrap();
        assert_eq!(meme_dir(&db), default_meme_dir());
    }

    #[test]
    fn root_pointing_at_a_file_yields_empty_not_error() {
        // A user pasting a FILE path into Settings → Meme library must not
        // crash the scan — read_dir fails, the list is simply empty.
        let root = tmp();
        let file = root.join("not-a-dir.gif");
        fs::write(&file, b"x").unwrap();
        assert!(scan(&file).is_empty());
    }

    #[test]
    fn names_sort_alphabetically_within_a_category() {
        let root = tmp();
        fs::create_dir_all(root.join("cats")).unwrap();
        fs::write(root.join("cats/zebra.gif"), b"x").unwrap();
        fs::write(root.join("cats/apple.gif"), b"x").unwrap();
        fs::write(root.join("cats/mango.gif"), b"x").unwrap();
        let names: Vec<_> = scan(&root).into_iter().map(|m| m.name).collect();
        assert_eq!(names, vec!["apple", "mango", "zebra"]);
    }

    #[test]
    fn multi_dot_filenames_keep_everything_before_the_final_ext_as_name() {
        let root = tmp();
        fs::write(root.join("v2.final.REALLY-final.gif"), b"x").unwrap();
        let memes = scan(&root);
        assert_eq!(memes.len(), 1);
        assert_eq!(memes[0].name, "v2.final.REALLY-final");
    }

    #[test]
    fn empty_subdirectories_contribute_nothing() {
        let root = tmp();
        fs::create_dir_all(root.join("empty/also-empty")).unwrap();
        fs::write(root.join("ok.gif"), b"x").unwrap();
        let memes = scan(&root);
        assert_eq!(memes.len(), 1);
        assert_eq!(memes[0].name, "ok");
    }

    #[test]
    fn copy_to_clipboard_missing_file_errors_before_touching_the_clipboard() {
        let missing = tmp().join("nope.gif");
        let err = copy_to_clipboard(&missing.to_string_lossy()).unwrap_err();
        assert!(err.contains("meme not found"), "got: {err}");
    }
}
