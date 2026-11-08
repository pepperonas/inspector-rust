//! Reopen a saved screenshot with its elements still editable.
//!
//! When the editor saves, the ORIGINAL capture and the document (annotations,
//! crop, orientation, backdrop — as JSON from the frontend) are stored under a
//! key derived from the exported PNG's bytes. Reopening any image whose bytes
//! match (the history clip, or the PNG the editor wrote to Downloads) finds
//! that project again; anything else opens as a plain picture.
//!
//! Projects live in `<data dir>/InspectorRust/editor-projects/<key>/` as
//! `source.png` + `doc.json`; only the newest [`KEEP`] are kept.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

/// Projects kept on disk (older ones are removed after each save).
pub const KEEP: usize = 40;
/// A document bigger than this is not stored (pictures inside it make it big).
pub const MAX_DOC_BYTES: usize = 64 * 1024 * 1024;
/// Largest picture file accepted for opening or inserting.
pub const MAX_IMAGE_BYTES: u64 = 50 * 1024 * 1024;
/// Longest side of an inserted picture; larger ones are scaled down.
pub const MAX_INSERT_SIDE: u32 = 4000;

const SOURCE: &str = "source.png";
const DOC: &str = "doc.json";

/// Project key of an exported PNG.
pub fn key_of(bytes: &[u8]) -> String {
    let h = Sha256::digest(bytes);
    h.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

pub fn projects_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("InspectorRust").join("editor-projects"))
}

/// Store the project for `exported`. Writes into a temp folder first and
/// renames it into place, so a half-written project is never found.
pub fn store(dir: &Path, exported: &[u8], source: &Path, doc_json: &str) -> std::io::Result<()> {
    if doc_json.len() > MAX_DOC_BYTES {
        return Err(std::io::Error::other("document too large to keep"));
    }
    std::fs::create_dir_all(dir)?;
    let key = key_of(exported);
    let tmp = dir.join(format!(".tmp-{key}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let res = (|| {
        std::fs::copy(source, tmp.join(SOURCE))?;
        std::fs::write(tmp.join(DOC), doc_json)?;
        let dest = dir.join(&key);
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::rename(&tmp, &dest)
    })();
    if res.is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    res?;
    prune(dir, KEEP);
    Ok(())
}

/// The project whose exported PNG had exactly these bytes:
/// `(original capture path, document JSON)`.
pub fn find(dir: &Path, bytes: &[u8]) -> Option<(PathBuf, String)> {
    let p = dir.join(key_of(bytes));
    let source = p.join(SOURCE);
    if !source.is_file() {
        return None;
    }
    let doc = std::fs::read_to_string(p.join(DOC)).ok()?;
    Some((source, doc))
}

/// Keep the `keep` most recently written projects, remove the rest.
pub fn prune(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut items: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            let p = e.path();
            let t = std::fs::metadata(p.join(DOC)).and_then(|m| m.modified()).ok()?;
            Some((t, p))
        })
        .collect();
    items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    for (_, p) in items.into_iter().skip(keep) {
        let _ = std::fs::remove_dir_all(p);
    }
}

/// A project waiting to be picked up by the editor, tied to the pending
/// capture it belongs to (so a later, unrelated capture never gets it).
#[derive(Default)]
pub struct ReopenState(pub Mutex<Option<(PathBuf, String)>>);

/// Hand out the waiting document if it belongs to `pending`. Clears it.
pub fn take_for(slot: &Mutex<Option<(PathBuf, String)>>, pending: Option<&Path>) -> Option<String> {
    let mut g = slot.lock();
    let matches = matches!((g.as_ref(), pending), (Some((p, _)), Some(cur)) if p == cur);
    if matches {
        g.take().map(|(_, d)| d)
    } else {
        None
    }
}

/// Picture bytes → PNG bytes (PNG input passes through unchanged).
pub fn to_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(bytes.to_vec());
    }
    let img = image::load_from_memory(bytes).map_err(|e| format!("not a picture: {e}"))?;
    encode_png(&img)
}

fn encode_png(img: &image::DynamicImage) -> Result<Vec<u8>, String> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| format!("encode png: {e}"))?;
    Ok(out.into_inner())
}

/// A picture for placing on the canvas, as a PNG data URL, scaled so its
/// longer side is at most [`MAX_INSERT_SIDE`].
pub fn insert_data_url(bytes: &[u8]) -> Result<String, String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("not a picture: {e}"))?;
    let img = if img.width().max(img.height()) > MAX_INSERT_SIDE {
        img.resize(MAX_INSERT_SIDE, MAX_INSERT_SIDE, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    Ok(format!("data:image/png;base64,{}", B64.encode(encode_png(&img)?)))
}

fn read_capped(path: &Path) -> Result<Vec<u8>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if meta.len() > MAX_IMAGE_BYTES {
        return Err("picture is larger than 50 MB".into());
    }
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Make `bytes` the pending capture and open the editor on it — with its
/// saved elements when this picture is a result the editor wrote.
fn open_bytes(app: &AppHandle, bytes: &[u8]) -> Result<(), String> {
    let found = projects_dir().and_then(|d| find(&d, bytes));
    let png = match &found {
        Some((source, _)) => std::fs::read(source).map_err(|e| format!("read project: {e}"))?,
        None => to_png(bytes)?,
    };
    let cache = dirs::cache_dir()
        .map(|d| d.join("InspectorRust"))
        .ok_or_else(|| "no cache dir on this system".to_string())?;
    std::fs::create_dir_all(&cache).map_err(|e| format!("create cache dir: {e}"))?;
    let path = cache.join(format!(
        "screenshot-pending-{}-reopen.png",
        chrono::Local::now().format("%Y%m%d-%H%M%S%3f")
    ));
    std::fs::write(&path, &png).map_err(|e| format!("write {}: {e}", path.display()))?;

    if let Some(state) = app.try_state::<ReopenState>() {
        *state.0.lock() = found.map(|(_, doc)| (path.clone(), doc));
    }
    let pending = app.state::<crate::screenshot_preview::PendingScreenshot>();
    let old = pending.inner().current.lock().replace(crate::screenshot_preview::Pending {
        path,
        app_name: None,
        saved: false,
    });
    crate::screenshot_preview::discard_superseded(old);

    crate::hotkey::hide_popup(app);
    if let Some(win) = app.get_webview_window(crate::screenshot_preview::PREVIEW_LABEL) {
        let _ = win.hide();
    }
    crate::screenshot_editor::open_editor(app).map_err(|e| format!("open editor: {e:#}"))?;
    let _ = app.emit("editor-screenshot-changed", ());
    Ok(())
}

/// Open a history picture in the editor.
#[tauri::command]
pub async fn editor_open_clip(app: AppHandle, id: i64) -> Result<(), String> {
    let db = app.state::<crate::db::DbHandle>().inner().clone();
    let entry = tauri::async_runtime::spawn_blocking(move || crate::db::get(&db, id))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "clip not found".to_string())?;
    if entry.content_type != crate::models::ContentType::Image {
        return Err("not a picture".into());
    }
    let bytes = B64.decode(entry.content_data.as_bytes()).map_err(|e| format!("decode clip: {e}"))?;
    open_bytes(&app, &bytes)
}

/// Open a picture file in the editor.
#[tauri::command]
pub async fn editor_open_file(app: AppHandle, path: String) -> Result<(), String> {
    let path = PathBuf::from(crate::path_arg::expand_user_str(&path, dirs::home_dir().as_deref()));
    let bytes = tauri::async_runtime::spawn_blocking(move || read_capped(&path))
        .await
        .map_err(|e| e.to_string())??;
    open_bytes(&app, &bytes)
}

/// The saved elements for the capture the editor is showing, if any. One-shot.
#[tauri::command]
pub fn editor_take_project(
    reopen: tauri::State<'_, ReopenState>,
    pending: tauri::State<'_, crate::screenshot_preview::PendingScreenshot>,
) -> Option<String> {
    let cur = pending.inner().current.lock().as_ref().map(|p| p.path.clone());
    take_for(&reopen.0, cur.as_deref())
}

/// A picture file to place on the canvas, as a PNG data URL.
#[tauri::command]
pub async fn editor_insert_image_file(path: String) -> Result<String, String> {
    let path = PathBuf::from(crate::path_arg::expand_user_str(&path, dirs::home_dir().as_deref()));
    tauri::async_runtime::spawn_blocking(move || insert_data_url(&read_capped(&path)?))
        .await
        .map_err(|e| e.to_string())?
}

/// The picture on the clipboard as a PNG data URL, or `None`.
#[tauri::command]
pub async fn editor_clipboard_image() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        use clipboard_rs::{common::RustImage, Clipboard, ClipboardContext};
        let ctx = ClipboardContext::new().map_err(|e| format!("clipboard: {e:?}"))?;
        let Ok(img) = ctx.get_image() else { return Ok(None) };
        let png = img.to_png().map_err(|e| format!("clipboard png: {e:?}"))?;
        insert_data_url(png.get_bytes()).map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ir-proj-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn rand_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }
    fn png(w: u32, h: u32) -> Vec<u8> {
        encode_png(&image::DynamicImage::new_rgba8(w, h)).unwrap()
    }

    #[test]
    fn a_stored_project_is_found_by_the_exported_bytes() {
        let root = tmp();
        let src = root.join("orig.png");
        std::fs::write(&src, b"original").unwrap();
        let dir = root.join("projects");
        store(&dir, b"exported", &src, "{\"doc\":1}").unwrap();
        let (source, doc) = find(&dir, b"exported").expect("found");
        assert_eq!(std::fs::read(source).unwrap(), b"original");
        assert_eq!(doc, "{\"doc\":1}");
        assert!(find(&dir, b"something else").is_none());
        // the source may be deleted after saving — the project keeps its copy
        std::fs::remove_file(&src).unwrap();
        assert!(find(&dir, b"exported").is_some());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn storing_again_replaces_and_leaves_no_temp_folder() {
        let root = tmp();
        let src = root.join("orig.png");
        std::fs::write(&src, b"a").unwrap();
        let dir = root.join("p");
        store(&dir, b"x", &src, "1").unwrap();
        store(&dir, b"x", &src, "2").unwrap();
        assert_eq!(find(&dir, b"x").unwrap().1, "2");
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(names.len(), 1);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn a_missing_source_stores_nothing() {
        let root = tmp();
        let dir = root.join("p");
        assert!(store(&dir, b"x", &root.join("gone.png"), "1").is_err());
        assert!(find(&dir, b"x").is_none());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn an_oversized_document_is_refused() {
        let root = tmp();
        let src = root.join("o.png");
        std::fs::write(&src, b"a").unwrap();
        let big = "x".repeat(MAX_DOC_BYTES + 1);
        assert!(store(&root.join("p"), b"x", &src, &big).is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn prune_keeps_the_newest() {
        let root = tmp();
        let src = root.join("o.png");
        std::fs::write(&src, b"a").unwrap();
        let dir = root.join("p");
        for i in 0..5u8 {
            store(&dir, &[i], &src, "d").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        prune(&dir, 2);
        assert!(find(&dir, &[4]).is_some());
        assert!(find(&dir, &[3]).is_some());
        assert!(find(&dir, &[0]).is_none());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn the_waiting_document_only_goes_to_its_own_capture() {
        let slot = Mutex::new(Some((PathBuf::from("/a.png"), "doc".to_string())));
        assert_eq!(take_for(&slot, Some(Path::new("/b.png"))), None);
        assert_eq!(take_for(&slot, None), None);
        assert_eq!(take_for(&slot, Some(Path::new("/a.png"))), Some("doc".into()));
        assert_eq!(take_for(&slot, Some(Path::new("/a.png"))), None);
    }

    #[test]
    fn pictures_become_png_and_big_ones_shrink() {
        let p = png(10, 5);
        assert_eq!(to_png(&p).unwrap(), p);
        let mut jpg = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(8, 8).write_to(&mut jpg, image::ImageFormat::Jpeg).unwrap();
        assert!(to_png(jpg.get_ref()).unwrap().starts_with(b"\x89PNG"));
        assert!(to_png(b"not a picture").is_err());

        let url = insert_data_url(&png(MAX_INSERT_SIDE + 200, 100)).unwrap();
        let bytes = B64.decode(url.strip_prefix("data:image/png;base64,").unwrap()).unwrap();
        let img = image::load_from_memory(&bytes).unwrap();
        assert_eq!(img.width(), MAX_INSERT_SIDE);
        assert!(insert_data_url(b"nope").is_err());
    }
}
