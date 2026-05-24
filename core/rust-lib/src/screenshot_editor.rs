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

/// Editor window dimensions. Large enough to comfortably edit a
/// region-screenshot at 1× — the canvas inside auto-scales the source
/// PNG to fit while preserving aspect ratio.
const EDITOR_W: f64 = 900.0;
const EDITOR_H: f64 = 640.0;

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

    WebviewWindowBuilder::new(app, EDITOR_LABEL, WebviewUrl::App("index.html".into()))
        .title("Edit screenshot")
        .inner_size(EDITOR_W, EDITOR_H)
        .min_inner_size(640.0, 480.0)
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
