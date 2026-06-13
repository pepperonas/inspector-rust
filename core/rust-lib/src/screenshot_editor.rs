//! Screenshot annotation editor — a separate Tauri webview window that
//! loads the currently-pending screenshot and lets the user draw
//! arrows, text, rectangles, highlights, and pixelate blurs on top.
//!
//! Routed from the preview window's Pencil button via
//! `screenshot_editor::open_editor(app)`. The editor's React side reads
//! the pending PNG via `get_pending_screenshot_info`, renders it onto
//! a canvas, layers annotations, and on Save sends the baked PNG bytes
//! back via [`editor_save`]. The backend then writes the result to
//! `~/Downloads/<App>-<ts>-edited.png` (keeping the app-name prefix
//! used by the regular Save path) and refreshes the pending entry so
//! the preview shows the edited version.

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::screenshot_preview::{Pending, PendingScreenshot, PREVIEW_LABEL};

/// Tauri window label for the editor. Singleton — opening "another"
/// editor just re-focuses the existing window.
pub const EDITOR_LABEL: &str = "screenshot-editor";

/// Default editor window dimensions (used the first time, before the user
/// has resized it). The canvas inside auto-scales the source PNG to fit.
const EDITOR_W: f64 = 900.0;
const EDITOR_H: f64 = 640.0;
const EDITOR_MIN_W: f64 = 640.0;
const EDITOR_MIN_H: f64 = 480.0;

/// Settings key: the last editor window size, stored as `"WIDTHxHEIGHT"`
/// (logical pixels), so it's restored on the next open (v0.66.0).
const KEY_EDITOR_SIZE: &str = "screenshot.editor_size";

/// Parse a `"WIDTHxHEIGHT"` size string into `(w, h)`. Pure + unit-tested.
/// Returns `None` for anything malformed.
fn parse_size(s: &str) -> Option<(f64, f64)> {
    let (w, h) = s.split_once('x')?;
    let w = w.trim().parse::<f64>().ok()?;
    let h = h.trim().parse::<f64>().ok()?;
    if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 {
        Some((w, h))
    } else {
        None
    }
}

/// Read the saved editor size from settings, clamped to the minimum.
/// Falls back to the default when unset / unparsable / below the minimum.
fn saved_editor_size(app: &AppHandle) -> (f64, f64) {
    let Some(db) = app.try_state::<crate::db::DbHandle>() else {
        return (EDITOR_W, EDITOR_H);
    };
    let parsed = crate::settings::get(&db, KEY_EDITOR_SIZE)
        .ok()
        .flatten()
        .and_then(|s| parse_size(&s));
    match parsed {
        Some((w, h)) if w >= EDITOR_MIN_W && h >= EDITOR_MIN_H => (w, h),
        _ => (EDITOR_W, EDITOR_H),
    }
}

/// Persist the editor window size (logical px). Called from the editor's
/// resize listener (debounced) so the next open restores it.
#[tauri::command]
pub fn set_editor_size(
    db: tauri::State<'_, crate::db::DbHandle>,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let w = width.round().max(EDITOR_MIN_W) as i64;
    let h = height.round().max(EDITOR_MIN_H) as i64;
    crate::settings::set(&db, KEY_EDITOR_SIZE, &format!("{w}x{h}"))
        .map_err(|e| e.to_string())
}

/// Open (or refocus) the editor window. The React side picks the
/// current pending screenshot via the existing IPCs.
pub fn open_editor(app: &AppHandle) -> Result<()> {
    if let Some(existing) = app.get_webview_window(EDITOR_LABEL) {
        existing.show().ok();
        existing.set_focus().ok();
        // Tell the editor to reload — it may have stale state from a
        // previous session if it was just hidden, not destroyed.
        let _ = app.emit("editor-screenshot-changed", ());
        return Ok(());
    }

    let (w, h) = saved_editor_size(app);
    WebviewWindowBuilder::new(app, EDITOR_LABEL, WebviewUrl::App("index.html".into()))
        .title("Edit screenshot")
        .inner_size(w, h)
        .min_inner_size(EDITOR_MIN_W, EDITOR_MIN_H)
        .resizable(true)
        .decorations(true)
        .visible(true)
        .focused(true)
        .center()
        .build()
        .context("create editor webview window")?;
    Ok(())
}

/// Save the edited PNG (passed as base64 from the canvas
/// `toDataURL('image/png')` call), write it to `~/Downloads` with the
/// app-name prefix + `-edited` suffix, push it to clipboard, add it
/// to history, replace the pending entry, and re-show the preview
/// window so the user sees the result.
#[tauri::command]
pub fn editor_save(
    app: AppHandle,
    pending: tauri::State<'_, PendingScreenshot>,
    png_b64: String,
) -> Result<String, String> {
    use clipboard_rs::{common::RustImage, Clipboard, ClipboardContext, RustImageData};

    // Strip the optional `data:image/png;base64,` prefix.
    let b64 = png_b64
        .strip_prefix("data:image/png;base64,")
        .unwrap_or(&png_b64);
    let bytes = B64
        .decode(b64)
        .map_err(|e| format!("decode editor PNG: {e}"))?;

    // Filename: re-use the captured app name + a fresh timestamp +
    // `-edited`. Keeps the alphabetical grouping in Finder consistent
    // with the unedited save path.
    let app_name = pending
        .inner()
        .current
        .lock()
        .as_ref()
        .and_then(|p| p.app_name.clone());
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let stem = match app_name.as_deref() {
        Some(a) if !a.is_empty() => format!("{a}-{ts}-edited.png"),
        _ => format!("Screenshot-{ts}-edited.png"),
    };
    let dir = dirs::download_dir()
        .or_else(dirs::picture_dir)
        .ok_or_else(|| "no Downloads/Pictures dir".to_string())?;
    let dest = dir.join(stem);
    std::fs::write(&dest, &bytes)
        .map_err(|e| format!("write {}: {e}", dest.display()))?;

    // Clipboard + history (mirror screenshot_preview_save's behaviour
    // so an edited screenshot is the new "current" everywhere).
    if let Some(watcher) = app.try_state::<crate::clipboard_watcher::WatcherState>() {
        watcher.mark_self_write(crate::models::ContentType::Image, &B64.encode(&bytes));
    }
    if let Ok(ctx) = ClipboardContext::new() {
        if let Ok(img) = RustImageData::from_bytes(&bytes) {
            let _ = ctx.set_image(img);
        }
    }
    if let Some(handle) = app.try_state::<crate::db::DbHandle>() {
        let _ = crate::db::upsert_clip(
            &handle,
            &crate::models::NewClip {
                content_type: crate::models::ContentType::Image,
                content_text: format!("[screenshot · edited · {} B]", bytes.len()),
                content_data: B64.encode(&bytes),
                byte_size: bytes.len() as i64,
            },
        );
        let _ = app.emit("clipboard-changed", ());
    }

    // Replace the pending entry with the edited file so the preview
    // (when re-shown) shows the new version.
    {
        let mut cur = pending.inner().current.lock();
        *cur = Some(Pending {
            path: dest.clone(),
            app_name,
            // Already written to ~/Downloads — must survive a later discard.
            saved: true,
        });
    }

    // Close the editor + re-show the preview so the user sees the
    // result with the same Copy / Save / Edit affordances.
    if let Some(win) = app.get_webview_window(EDITOR_LABEL) {
        let _ = win.close();
    }
    if let Err(e) = crate::screenshot_preview::show_preview(&app) {
        tracing::warn!("re-show preview after edit: {e:#}");
    }

    let _ = app.emit("screenshot-saved", dest.to_string_lossy().to_string());
    Ok(dest.to_string_lossy().into_owned())
}

/// Copy the *edited* canvas (base64 PNG from `toDataURL`) straight to the
/// clipboard — no file, no history mutation beyond the clip, no window close.
/// Bound to Cmd/Ctrl+C in the editor so the user can grab the annotated image
/// and keep editing. Returns the PNG byte size for a toast.
#[tauri::command]
pub fn editor_copy(app: AppHandle, png_b64: String) -> Result<usize, String> {
    use clipboard_rs::{common::RustImage, Clipboard, ClipboardContext, RustImageData};

    let b64 = png_b64
        .strip_prefix("data:image/png;base64,")
        .unwrap_or(&png_b64);
    let bytes = B64
        .decode(b64)
        .map_err(|e| format!("decode editor PNG: {e}"))?;

    if let Some(watcher) = app.try_state::<crate::clipboard_watcher::WatcherState>() {
        watcher.mark_self_write(crate::models::ContentType::Image, &B64.encode(&bytes));
    }
    let ctx = ClipboardContext::new().map_err(|e| format!("clipboard init: {e:?}"))?;
    let img = RustImageData::from_bytes(&bytes).map_err(|e| format!("decode png: {e:?}"))?;
    ctx.set_image(img).map_err(|e| format!("set_image: {e:?}"))?;
    Ok(bytes.len())
}

/// Cancel — close the editor without saving. The pending entry is
/// untouched, so the preview can re-open showing the original capture.
#[tauri::command]
pub fn editor_cancel(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(EDITOR_LABEL) {
        let _ = win.close();
    }
    // Re-show the preview unchanged so the user can still hit Save /
    // Discard on the original capture.
    if let Some(win) = app.get_webview_window(PREVIEW_LABEL) {
        let _ = win.show();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_size;

    #[test]
    fn parses_valid_size_strings() {
        assert_eq!(parse_size("900x640"), Some((900.0, 640.0)));
        assert_eq!(parse_size("1280x720"), Some((1280.0, 720.0)));
        // Whitespace around the components is tolerated.
        assert_eq!(parse_size(" 800 x 600 "), Some((800.0, 600.0)));
        // Fractional (logical px after scale division) is fine.
        assert_eq!(parse_size("960.5x540.25"), Some((960.5, 540.25)));
    }

    #[test]
    fn rejects_malformed_strings() {
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("900"), None); // no separator
        assert_eq!(parse_size("900x"), None); // missing height
        assert_eq!(parse_size("x640"), None); // missing width
        assert_eq!(parse_size("axb"), None); // non-numeric
        assert_eq!(parse_size("900*640"), None); // wrong separator
    }

    #[test]
    fn rejects_non_positive_and_non_finite() {
        assert_eq!(parse_size("0x640"), None);
        assert_eq!(parse_size("900x0"), None);
        assert_eq!(parse_size("-900x640"), None);
        assert_eq!(parse_size("infx640"), None);
        assert_eq!(parse_size("NaNxNaN"), None);
    }
}
