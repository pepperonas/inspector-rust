//! Style presets: the look of a recording (background, frame, pointer, aspect) saved as a
//! shareable file `<recordings folder>/Presets/<name>.irpreset`.
//!
//! A preset is plain JSON (2-space indent, trailing newline, written atomically). Own images
//! and pointers it refers to live beside it under `Presets/assets/<content hash>.<ext>`, so a
//! preset survives the project it was saved from. Applying copies those files into the
//! project's `assets/`; a file that went missing falls back to the standard look, never to an
//! error. Applying is one document change (one undo step in the editor); new projects take the
//! default preset's look but never its aspect (as OpenScreen).
#![cfg_attr(not(all(target_os = "macos", target_arch = "aarch64")), allow(dead_code))]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::editor::{Appearance, Aspect, Background, CursorStyle, EditDoc};
use super::scene_json::{project_asset, PROJECT_ASSETS_DIR};

pub const PRESET_FORMAT: &str = "inspector-rust-record-preset";
pub const PRESET_VERSION: u32 = 1;
/// Folder (inside the recordings folder) presets live in.
pub const PRESETS_DIR: &str = "Presets";
pub const PRESET_EXT: &str = "irpreset";
/// Longest preset name, in characters.
pub const MAX_NAME_CHARS: usize = 80;

pub const ERR_NAME_EMPTY: &str = "record.editor.preset_name_empty";
pub const ERR_NAME_INVALID: &str = "record.editor.preset_name_invalid";
pub const ERR_NAME_TOO_LONG: &str = "record.editor.preset_name_too_long";
pub const ERR_NAME_TAKEN: &str = "record.editor.preset_name_taken";
pub const ERR_NOT_FOUND: &str = "record.editor.preset_not_found";
pub const ERR_WRITE: &str = "record.editor.preset_write";
/// Listing reasons for a preset that cannot be applied.
pub const ERR_UNREADABLE: &str = "record.editor.preset_unreadable";
pub const ERR_FORMAT: &str = "record.editor.preset_format";
pub const ERR_VERSION: &str = "record.editor.preset_version";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub appearance: Appearance,
    pub cursor: CursorStyle,
    pub aspect: Aspect,
}

/// One file of the presets folder. A broken file is listed (`ok: false`, `error` = reason key)
/// so it can be deleted or revealed, but never applied.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetEntry {
    pub name: String,
    /// File name inside the presets folder (the handle the IPC takes).
    pub file: String,
    pub ok: bool,
    pub error: Option<String>,
}

pub fn presets_dir(root: &Path) -> PathBuf {
    root.join(PRESETS_DIR)
}

/// The name as stored: trimmed; refused when empty, longer than [`MAX_NAME_CHARS`], or when
/// it could leave the presets folder or make a bad file name (`/`, `\`, `:`, control
/// characters, a leading `.`).
pub fn clean_name(name: &str) -> Result<String, &'static str> {
    let n = name.trim();
    if n.is_empty() {
        return Err(ERR_NAME_EMPTY);
    }
    if n.chars().count() > MAX_NAME_CHARS {
        return Err(ERR_NAME_TOO_LONG);
    }
    if n.starts_with('.') || n.chars().any(|c| matches!(c, '/' | '\\' | ':') || c.is_control()) {
        return Err(ERR_NAME_INVALID);
    }
    Ok(n.to_string())
}

fn file_for(name: &str) -> String {
    format!("{name}.{PRESET_EXT}")
}

/// A regular file and not a symlink (a link in the presets folder could pull in any file).
fn plain_file(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_file())
}

fn has_preset_ext(file: &str) -> bool {
    Path::new(file).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(PRESET_EXT))
}

/// `<presets>/<file>` for a plain `*.irpreset` file name that exists.
pub fn preset_path(root: &Path, file: &str) -> Result<PathBuf, &'static str> {
    let plain = !file.is_empty() && !file.starts_with('.') && !file.contains(['/', '\\', '\0']) && has_preset_ext(file);
    if !plain {
        return Err(ERR_NOT_FOUND);
    }
    let p = presets_dir(root).join(file);
    if plain_file(&p) { Ok(p) } else { Err(ERR_NOT_FOUND) }
}

fn stem(file: &str) -> String {
    Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Parses a preset file; `Err` = the reason key it is listed with.
pub fn parse_preset(bytes: &[u8]) -> Result<Preset, &'static str> {
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| ERR_UNREADABLE)?;
    if v.get("format").and_then(|f| f.as_str()) != Some(PRESET_FORMAT) {
        return Err(ERR_FORMAT);
    }
    if v.get("version").and_then(|f| f.as_u64()) != Some(PRESET_VERSION as u64) {
        return Err(ERR_VERSION);
    }
    serde_json::from_value::<Preset>(v).map_err(|_| ERR_UNREADABLE)
}

pub fn read_preset(root: &Path, file: &str) -> Result<Preset, String> {
    let path = preset_path(root, file)?;
    let bytes = std::fs::read(&path).map_err(|_| ERR_UNREADABLE.to_string())?;
    let mut p = parse_preset(&bytes).map_err(str::to_string)?;
    // A hand-edited or empty name reads as the file's own name.
    if clean_name(&p.name).is_err() {
        p.name = stem(file);
    }
    Ok(p)
}

/// Every `*.irpreset` file of the presets folder, by name (case-insensitive).
pub fn list_presets(root: &Path) -> Vec<PresetEntry> {
    let Ok(rd) = std::fs::read_dir(presets_dir(root)) else { return Vec::new() };
    let mut out: Vec<PresetEntry> = rd
        .flatten()
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            (!file.starts_with('.') && has_preset_ext(&file) && plain_file(&e.path())).then_some(file)
        })
        .map(|file| match read_preset(root, &file) {
            Ok(p) => PresetEntry { name: p.name, file, ok: true, error: None },
            Err(e) => PresetEntry { name: stem(&file), file, ok: false, error: Some(e) },
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.file.cmp(&b.file)));
    out
}

/// Is `name` already used by another preset file (case-insensitive; `except` = the file being
/// renamed)? Compared against the file names: they are the identity on a case-insensitive disk.
fn name_taken(root: &Path, name: &str, except: Option<&str>) -> bool {
    let want = name.to_lowercase();
    let Ok(rd) = std::fs::read_dir(presets_dir(root)) else { return false };
    rd.flatten().any(|e| {
        let file = e.file_name().to_string_lossy().into_owned();
        has_preset_ext(&file) && Some(file.as_str()) != except && stem(&file).to_lowercase() == want
    })
}

/// `bytes` to `dir/name` through a temporary file and a rename (never a half-written preset).
/// Never through a planted link (`safe_fs`): this also writes into a project's `assets/`.
fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    record_render_protocol::safe_fs::write_atomic(&dir.join(name), bytes)
}

/// The file's text: 2-space JSON and a trailing newline.
pub fn to_text(p: &Preset) -> String {
    // Plain data; serialisation cannot fail.
    let mut s = serde_json::to_string_pretty(p).unwrap_or_default();
    s.push('\n');
    s
}

fn write_preset(root: &Path, file: &str, p: &Preset) -> Result<(), String> {
    write_atomic(&presets_dir(root), file, to_text(p).as_bytes()).map_err(|e| {
        tracing::warn!(target: "record::presets", "{file}: {e}");
        ERR_WRITE.to_string()
    })
}

/// Writes a NEW preset file: the content goes to a temporary file that is then hard-linked
/// to `file`, which fails when the name exists in any spelling the disk treats as the same
/// (case, Unicode normalisation) or appeared meanwhile (a racing save) — `ERR_NAME_TAKEN`,
/// never an overwrite. On a disk without hard links (exFAT/FAT, many network shares) the
/// file is created directly with O_EXCL + O_NOFOLLOW instead, which still never overwrites.
pub fn write_new_preset(root: &Path, file: &str, p: &Preset) -> Result<(), String> {
    write_new_with(root, file, p, |a, b| std::fs::hard_link(a, b))
}

fn write_new_with(
    root: &Path,
    file: &str,
    p: &Preset,
    link: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), String> {
    use record_render_protocol::safe_fs::{create_temp, open_new_nofollow};
    use std::io::Write;
    let dir = presets_dir(root);
    let fail = |e: std::io::Error| {
        tracing::warn!(target: "record::presets", "{file}: {e}");
        ERR_WRITE.to_string()
    };
    let taken = |e: std::io::Error| {
        if e.kind() == std::io::ErrorKind::AlreadyExists { ERR_NAME_TAKEN.to_string() } else { fail(e) }
    };
    std::fs::create_dir_all(&dir).map_err(fail)?;
    let bytes = to_text(p);
    let target = dir.join(file);
    // A unique temp name per call (pid + time + counter), created with O_EXCL + O_NOFOLLOW.
    let (tmp, mut f) = create_temp(&target).map_err(fail)?;
    let written = (|| {
        f.write_all(bytes.as_bytes())?;
        f.sync_all()
    })();
    drop(f);
    let linked = written.and_then(|_| link(&tmp, &target));
    let _ = std::fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(ERR_NAME_TAKEN.into()),
        Err(e) => {
            // No hard links on this disk: create the final name exclusively and write it.
            tracing::info!(target: "record::presets", "{file}: hard link unavailable ({e}), writing directly");
            let mut out = open_new_nofollow(&target).map_err(taken)?;
            let res = out.write_all(bytes.as_bytes()).and_then(|_| out.sync_all());
            drop(out);
            res.map_err(|e| {
                // Never leave a half-written preset behind under the real name.
                let _ = std::fs::remove_file(&target);
                fail(e)
            })
        }
    }
}

/// Copies `src` into `dest_assets` as `<sha256 prefix>.<ext>` (kept when the same content is
/// already there); returns that name.
fn copy_hashed(src: &Path, dest_assets: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(src)?;
    let ext = src.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_else(|| "bin".into());
    let ext: String = ext.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
    let hash: String = Sha256::digest(&bytes).iter().take(16).map(|b| format!("{b:02x}")).collect();
    let name = format!("{hash}.{ext}");
    let dest = dest_assets.join(&name);
    // `plain_file`: a link planted under that name is replaced, never trusted.
    if !plain_file(&dest) {
        std::fs::create_dir_all(dest_assets)?;
        write_atomic(dest_assets, &name, &bytes)?;
    }
    Ok(name)
}

/// Appearance and pointer with their own files moved from `from_dir/assets` to `to_dir/assets`
/// (content-hashed names). A file that is missing (or cannot be copied) gives the standard
/// background / no own pointer.
fn relocate_files(appearance: &Appearance, cursor: &CursorStyle, from_dir: &Path, to_dir: &Path) -> (Appearance, CursorStyle) {
    // A symlinked `assets/` (planted in a project opened from elsewhere) is refused: nothing
    // is copied and the standard look applies.
    let mv = |file: &str| -> Option<String> {
        let src = project_asset(from_dir, file).filter(|p| plain_file(p))?;
        // Only now (something to copy): the folder is made, or refused when it is a link.
        let to_assets = std::fs::create_dir_all(to_dir)
            .and_then(|_| record_render_protocol::safe_fs::plain_dir(to_dir, PROJECT_ASSETS_DIR))
            .map_err(|e| tracing::warn!(target: "record::presets", "{}: {e}", to_dir.display()))
            .ok()?;
        copy_hashed(&src, &to_assets)
            .map_err(|e| tracing::warn!(target: "record::presets", "{}: {e}", src.display()))
            .ok()
    };
    let mut a = appearance.clone();
    if let Background::Image { file } = &appearance.background {
        a.background = match mv(file) {
            Some(file) => Background::Image { file },
            None => Appearance::default().background,
        };
    }
    let mut c = cursor.clone();
    if let Some(custom) = &cursor.custom {
        c.custom = mv(&custom.file).map(|file| super::editor::CustomCursor { file, ..custom.clone() });
    }
    (a, c)
}

/// Saves the open document's look under `name` (unique, case-insensitive). Own images and
/// pointers of `project_dir` are copied next to the preset.
pub fn save_preset(root: &Path, name: &str, doc: &EditDoc, project_dir: &Path) -> Result<PresetEntry, String> {
    let name = clean_name(name)?;
    if name_taken(root, &name, None) {
        return Err(ERR_NAME_TAKEN.into());
    }
    let dir = presets_dir(root);
    let (appearance, cursor) = relocate_files(&doc.appearance, &doc.cursor, project_dir, &dir);
    let preset = Preset {
        format: PRESET_FORMAT.into(),
        version: PRESET_VERSION,
        name: name.clone(),
        appearance,
        cursor,
        aspect: doc.output.aspect,
    };
    let file = file_for(&name);
    write_new_preset(root, &file, &preset)?;
    Ok(PresetEntry { name, file, ok: true, error: None })
}

/// The preset's look with its files copied into `project_dir/assets` (missing → standard).
pub fn resolve_for_project(preset: &Preset, root: &Path, project_dir: &Path) -> Preset {
    let (appearance, cursor) = relocate_files(&preset.appearance, &preset.cursor, &presets_dir(root), project_dir);
    Preset { appearance, cursor, ..preset.clone() }
}

/// `doc` with the preset's look and aspect (files copied into the project first).
pub fn apply_preset(doc: &EditDoc, preset: &Preset, root: &Path, project_dir: &Path) -> EditDoc {
    let r = resolve_for_project(preset, root, project_dir);
    let mut next = doc.clone();
    next.appearance = r.appearance;
    next.cursor = r.cursor;
    next.output.aspect = r.aspect;
    next
}

/// Renames a preset (name inside the file and the file name). Changing only the case of a
/// name is a rename too; the files of other presets count as taken.
pub fn rename_preset(root: &Path, file: &str, new_name: &str) -> Result<PresetEntry, String> {
    let name = clean_name(new_name)?;
    let old_path = preset_path(root, file)?;
    let mut preset = read_preset(root, file)?;
    if name_taken(root, &name, Some(file)) {
        return Err(ERR_NAME_TAKEN.into());
    }
    preset.name = name.clone();
    let new_file = file_for(&name);
    if new_file.to_lowercase() == file.to_lowercase() {
        // Same file on a case-insensitive disk: rewrite in place, then fix the name's case.
        write_preset(root, file, &preset)?;
        if new_file != file {
            std::fs::rename(&old_path, presets_dir(root).join(&new_file)).map_err(|_| ERR_WRITE.to_string())?;
        }
    } else {
        write_new_preset(root, &new_file, &preset)?;
        let _ = std::fs::remove_file(&old_path);
    }
    Ok(PresetEntry { name, file: new_file, ok: true, error: None })
}

/// The listed file that `stored` (the `record.default_preset` value) names, compared without
/// case — the disk does not tell them apart either.
pub fn match_file<'a>(presets: &'a [PresetEntry], stored: &str) -> Option<&'a PresetEntry> {
    presets.iter().find(|p| p.file.to_lowercase() == stored.to_lowercase())
}

/// Removes a preset file (its copied assets stay: other presets may share them).
pub fn delete_preset(root: &Path, file: &str) -> Result<(), String> {
    let path = preset_path(root, file)?;
    std::fs::remove_file(path).map_err(|_| ERR_WRITE.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::editor::{default_doc, CustomCursor, Frame};
    use crate::record::project::RecordingMeta;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ir-presets-{tag}-{}-{}", std::process::id(), rand_suffix()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn rand_suffix() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::SeqCst)
    }

    fn doc() -> EditDoc {
        default_doc(&RecordingMeta { duration_ms: 10_000, ..Default::default() }, None)
    }

    fn styled() -> EditDoc {
        let mut d = doc();
        d.appearance.padding = 12.0;
        d.appearance.frame = Frame::Laptop;
        d.appearance.background = Background::Color { color: "#123456".into() };
        d.cursor.size = 5.0;
        d.output.aspect = Aspect::W9H16;
        d
    }

    #[test]
    fn save_and_apply_roundtrip() {
        let root = tmp("rt");
        let project = tmp("rt-proj");
        let entry = save_preset(&root, "  Clean demo ", &styled(), &project).unwrap();
        assert_eq!(entry, PresetEntry { name: "Clean demo".into(), file: "Clean demo.irpreset".into(), ok: true, error: None });
        let text = std::fs::read_to_string(presets_dir(&root).join(&entry.file)).unwrap();
        assert!(text.ends_with("}\n") && text.contains("\n  \"format\": \"inspector-rust-record-preset\""), "{text}");
        assert_eq!(list_presets(&root), vec![entry.clone()]);

        let preset = read_preset(&root, &entry.file).unwrap();
        let applied = apply_preset(&doc(), &preset, &root, &project);
        let want = styled();
        assert_eq!(applied.appearance, want.appearance);
        assert_eq!(applied.cursor, want.cursor);
        assert_eq!(applied.output.aspect, Aspect::W9H16);
        // Only the look changes.
        assert_eq!(applied.clips, doc().clips);
    }

    #[test]
    fn apply_copies_preset_assets() {
        let root = tmp("assets");
        let src = tmp("assets-src");
        let dest = tmp("assets-dest");
        std::fs::create_dir_all(src.join("assets")).unwrap();
        std::fs::write(src.join("assets/bg.png"), b"\x89PNG background").unwrap();
        std::fs::write(src.join("assets/arrow.png"), b"\x89PNG pointer").unwrap();
        let mut d = doc();
        d.appearance.background = Background::Image { file: "bg.png".into() };
        d.cursor.custom = Some(CustomCursor { file: "arrow.png".into(), hotspot_x: 0.25, hotspot_y: 0.5 });
        let entry = save_preset(&root, "Own", &d, &src).unwrap();

        let preset = read_preset(&root, &entry.file).unwrap();
        let Background::Image { file: bg } = &preset.appearance.background else { panic!("image kept") };
        assert!(presets_dir(&root).join("assets").join(bg).is_file(), "copied next to the preset");

        let applied = apply_preset(&doc(), &preset, &root, &dest);
        let Background::Image { file } = &applied.appearance.background else { panic!("image applied") };
        assert_eq!(std::fs::read(dest.join("assets").join(file)).unwrap(), b"\x89PNG background");
        let custom = applied.cursor.custom.expect("pointer applied");
        assert_eq!((custom.hotspot_x, custom.hotspot_y), (0.25, 0.5));
        assert_eq!(std::fs::read(dest.join("assets").join(&custom.file)).unwrap(), b"\x89PNG pointer");

        // A preset file whose image went missing falls back to the standard look.
        std::fs::remove_file(presets_dir(&root).join("assets").join(bg)).unwrap();
        let gone = apply_preset(&doc(), &preset, &root, &tmp("assets-dest2"));
        assert_eq!(gone.appearance.background, Appearance::default().background);
    }

    #[test]
    fn broken_preset_is_listed_not_applied() {
        let root = tmp("broken");
        std::fs::create_dir_all(presets_dir(&root)).unwrap();
        std::fs::write(presets_dir(&root).join("Half.irpreset"), b"{\"format\": \"inspector-rust-rec").unwrap();
        std::fs::write(presets_dir(&root).join("notes.txt"), b"not a preset").unwrap();
        let list = list_presets(&root);
        assert_eq!(list, vec![PresetEntry { name: "Half".into(), file: "Half.irpreset".into(), ok: false, error: Some(ERR_UNREADABLE.into()) }]);
        assert_eq!(read_preset(&root, "Half.irpreset"), Err(ERR_UNREADABLE.into()));
    }

    #[test]
    fn wrong_version_rejected() {
        let root = tmp("version");
        save_preset(&root, "Future", &styled(), &tmp("version-p")).unwrap();
        let path = presets_dir(&root).join("Future.irpreset");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("\"version\": 1", "\"version\": 2")).unwrap();
        assert_eq!(read_preset(&root, "Future.irpreset"), Err(ERR_VERSION.into()));
        assert_eq!(list_presets(&root)[0].error.as_deref(), Some(ERR_VERSION));
        std::fs::write(&path, text.replace(PRESET_FORMAT, "something-else")).unwrap();
        assert_eq!(read_preset(&root, "Future.irpreset"), Err(ERR_FORMAT.into()));
    }

    #[test]
    fn names_unique_case_insensitive() {
        let root = tmp("unique");
        let p = tmp("unique-p");
        let a = save_preset(&root, "Dark", &styled(), &p).unwrap();
        assert_eq!(save_preset(&root, "dark", &styled(), &p), Err(ERR_NAME_TAKEN.into()));
        let b = save_preset(&root, "Light", &styled(), &p).unwrap();
        assert_eq!(rename_preset(&root, &b.file, "DARK"), Err(ERR_NAME_TAKEN.into()));
        // A change of case of its own name is fine.
        let renamed = rename_preset(&root, &a.file, "DARK").unwrap();
        assert_eq!(renamed.file, "DARK.irpreset");
        let names: Vec<String> = list_presets(&root).into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["DARK", "Light"]);
        delete_preset(&root, &renamed.file).unwrap();
        assert_eq!(list_presets(&root).len(), 1);
    }

    #[test]
    fn default_preset_used_for_new_doc_without_aspect() {
        let root = tmp("default");
        let entry = save_preset(&root, "Default", &styled(), &tmp("default-p")).unwrap();
        let preset = read_preset(&root, &entry.file).unwrap();
        let meta = RecordingMeta { duration_ms: 10_000, ..Default::default() };
        let d = default_doc(&meta, Some(&preset));
        assert_eq!(d.appearance, styled().appearance);
        assert_eq!(d.cursor, styled().cursor);
        assert_eq!(d.output.aspect, Aspect::Auto, "the aspect never carries over to new projects");
        assert_eq!(default_doc(&meta, None).appearance, Appearance::default());
    }

    #[test]
    fn preset_path_traversal_refused() {
        let root = tmp("traversal");
        let p = tmp("traversal-p");
        for bad in ["../x", "a/b", "..\\x", ".hidden", "x\u{0}y", "a:b"] {
            assert_eq!(save_preset(&root, bad, &styled(), &p), Err(ERR_NAME_INVALID.into()), "{bad:?}");
        }
        assert_eq!(save_preset(&root, "   ", &styled(), &p), Err(ERR_NAME_EMPTY.into()));
        assert_eq!(save_preset(&root, &"x".repeat(81), &styled(), &p), Err(ERR_NAME_TOO_LONG.into()));
        assert!(save_preset(&root, &"ä".repeat(80), &styled(), &p).is_ok(), "80 characters, not bytes");
        assert!(!root.parent().unwrap().join("x.irpreset").exists());
        for bad in ["../x.irpreset", "/etc/x.irpreset", "x.json"] {
            assert_eq!(read_preset(&root, bad), Err(ERR_NOT_FOUND.into()), "{bad}");
            assert_eq!(delete_preset(&root, bad), Err(ERR_NOT_FOUND.into()), "{bad}");
        }
    }

    #[test]
    fn preset_asset_names_are_plain() {
        // A crafted preset pointing outside its assets folder is treated as missing.
        let root = tmp("crafted");
        let dest = tmp("crafted-dest");
        let mut preset = read_back(&root);
        preset.appearance.background = Background::Image { file: "../../secret.png".into() };
        let applied = apply_preset(&doc(), &preset, &root, &dest);
        assert_eq!(applied.appearance.background, Appearance::default().background);
        assert!(!dest.join("assets").exists());
    }

    #[test]
    fn save_never_overwrites_an_existing_file() {
        let root = tmp("exclusive");
        let first = save_preset(&root, "Café", &styled(), &tmp("exclusive-p")).unwrap();
        let before = std::fs::read(presets_dir(&root).join(&first.file)).unwrap();
        // A racing save of the same file (past the name check) fails as taken.
        let mut other = read_preset(&root, &first.file).unwrap();
        other.appearance.padding = 99.0;
        assert_eq!(write_new_preset(&root, &first.file, &other), Err(ERR_NAME_TAKEN.into()));
        // The decomposed spelling: one file on a normalisation-insensitive disk (APFS) → taken;
        // two files elsewhere. Either way the first preset is untouched.
        let nfd = "Cafe\u{301}";
        let _ = save_preset(&root, nfd, &doc(), &tmp("exclusive-q"));
        assert_eq!(std::fs::read(presets_dir(&root).join(&first.file)).unwrap(), before);
        assert!(std::fs::read_dir(presets_dir(&root)).unwrap().flatten().all(|e| { let n = e.file_name().to_string_lossy().into_owned(); !n.ends_with(".new") && !n.ends_with(".tmp") }));
    }

    #[test]
    fn save_falls_back_when_the_disk_has_no_hard_links() {
        let root = tmp("nolink");
        let unsupported = |_: &Path, _: &Path| Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "no links"));
        let p = read_back(&root);
        write_new_with(&root, "fallback.irpreset", &p, unsupported).unwrap();
        assert_eq!(read_preset(&root, "fallback.irpreset").unwrap().appearance, p.appearance);
        // The fallback still never overwrites an existing preset.
        let before = std::fs::read(presets_dir(&root).join("fallback.irpreset")).unwrap();
        let mut other = p.clone();
        other.appearance.padding = 77.0;
        assert_eq!(write_new_with(&root, "fallback.irpreset", &other, unsupported), Err(ERR_NAME_TAKEN.into()));
        assert_eq!(std::fs::read(presets_dir(&root).join("fallback.irpreset")).unwrap(), before);
        // No temp files left behind.
        assert!(std::fs::read_dir(presets_dir(&root)).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".tmp")));
    }

    #[cfg(unix)]
    #[test]
    fn apply_refuses_a_symlinked_project_assets_folder() {
        let root = tmp("ln-assets");
        let src = tmp("ln-assets-src");
        std::fs::create_dir_all(src.join("assets")).unwrap();
        std::fs::write(src.join("assets/bg.png"), b"\x89PNG background").unwrap();
        let mut d = doc();
        d.appearance.background = Background::Image { file: "bg.png".into() };
        let entry = save_preset(&root, "Own", &d, &src).unwrap();
        let preset = read_preset(&root, &entry.file).unwrap();
        let dest = tmp("ln-assets-dest");
        let outside = tmp("ln-assets-outside");
        std::os::unix::fs::symlink(&outside, dest.join("assets")).unwrap();
        let applied = apply_preset(&doc(), &preset, &root, &dest);
        assert_eq!(applied.appearance.background, Appearance::default().background, "standard look instead");
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    }

    #[test]
    #[cfg(unix)]
    fn symlinks_in_the_presets_folder_are_not_followed() {
        let root = tmp("links");
        let elsewhere = tmp("links-out");
        let real = save_preset(&root, "Real", &styled(), &tmp("links-p")).unwrap();
        let outside = elsewhere.join("x.irpreset");
        std::fs::copy(presets_dir(&root).join(&real.file), &outside).unwrap();
        std::os::unix::fs::symlink(&outside, presets_dir(&root).join("Link.irpreset")).unwrap();
        assert_eq!(list_presets(&root).len(), 1, "the link is not listed");
        assert_eq!(read_preset(&root, "Link.irpreset"), Err(ERR_NOT_FOUND.into()));
        // A linked asset counts as missing.
        std::fs::write(elsewhere.join("secret.png"), b"secret").unwrap();
        std::fs::create_dir_all(presets_dir(&root).join("assets")).unwrap();
        std::os::unix::fs::symlink(elsewhere.join("secret.png"), presets_dir(&root).join("assets/bg.png")).unwrap();
        let mut p = read_preset(&root, &real.file).unwrap();
        p.appearance.background = Background::Image { file: "bg.png".into() };
        let dest = tmp("links-dest");
        let applied = apply_preset(&doc(), &p, &root, &dest);
        assert_eq!(applied.appearance.background, Appearance::default().background);
        assert!(!dest.join("assets").exists());
    }

    #[test]
    fn default_matches_without_case() {
        let list = vec![PresetEntry { name: "Dark".into(), file: "Dark.irpreset".into(), ok: false, error: Some(ERR_VERSION.into()) }];
        assert_eq!(match_file(&list, "dark.IRPRESET").map(|p| p.file.as_str()), Some("Dark.irpreset"));
        assert!(match_file(&list, "Light.irpreset").is_none());
    }

    fn read_back(root: &Path) -> Preset {
        let e = save_preset(root, "Base", &styled(), &tmp("rb")).unwrap();
        read_preset(root, &e.file).unwrap()
    }
}
