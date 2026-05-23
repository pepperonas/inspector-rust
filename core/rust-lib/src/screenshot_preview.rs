//! CleanShot-X-style floating screenshot preview.
//!
//! After `commands::run_screenshot_pipeline` captures a PNG, it stashes
//! the on-disk path in [`PendingScreenshot`] state and calls
//! [`show_preview`] to spawn (or reuse) a small frameless transparent
//! window in the bottom-left corner of the monitor where the OS cursor
//! lives. The window's React side (`components/ScreenshotPreview.tsx`)
//! displays the thumbnail and offers three actions — Save / Discard /
//! Edit — each routed back to one of the IPC commands at the bottom of
//! this module.
//!
//! Until the user picks one, NO side effects happen: the PNG is just a
//! temp file in `~/Library/Caches/InspectorRust/` (or the equivalent
//! per-OS cache dir). Discard deletes the temp, Save moves it to
//! ~/Downloads + clipboard + history, Edit moves it to ~/Downloads and
//! hands the path to the OS default image viewer (`open` / `start` /
//! `xdg-open`).

use parking_lot::Mutex;
use std::path::PathBuf;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

/// Shared state holding the on-disk path of the most recently captured
/// (still-pending) screenshot, written by `run_screenshot_pipeline` and
/// read by the Save / Discard / Edit IPCs.
#[derive(Default)]
pub struct PendingScreenshot(pub Mutex<Option<PathBuf>>);

pub const PREVIEW_LABEL: &str = "screenshot-preview";

/// Inner-window dimensions for the floating preview. ~16:10 aspect so
/// landscape thumbnails read well.
const WIN_W: f64 = 340.0;
const WIN_H: f64 = 220.0;
/// Margin from the screen edge. Bottom margin needs to clear the
/// macOS Dock — default Dock is ~78 px high, "Magnification: Large"
/// goes ~128 px. 110 px gives breathing room for both. Side margin
/// can stay tight.
const EDGE_MARGIN_X: i32 = 24;
const EDGE_MARGIN_Y_BOTTOM: i32 = 110;

// ── macOS global cursor position (raw FFI) ─────────────────────────────
//
// `WebviewWindow::cursor_position()` returns coords that only refresh
// when the window receives a mouse event — so polling it from an
// inactive preview window kept giving the stale position from the last
// click. We need the GLOBAL cursor instead: macOS exposes that via
// `CGEventGetLocation` on an `event` synthesised from the null event
// source. Coords come back in POINTS with top-left origin (Carbon
// coords) — convert to physical pixels per monitor via its scale
// factor.

#[cfg(target_os = "macos")]
mod cg_cursor {
    use std::ffi::c_void;
    #[repr(C)]
    pub struct CGPoint {
        pub x: f64,
        pub y: f64,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn CGEventCreate(source: *mut c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *mut c_void);
    }

    /// Global cursor position in POINTS (top-left origin), or `None`
    /// if the system call failed.
    pub fn position_in_points() -> Option<(f64, f64)> {
        unsafe {
            let ev = CGEventCreate(std::ptr::null_mut());
            if ev.is_null() {
                return None;
            }
            let p = CGEventGetLocation(ev);
            CFRelease(ev);
            Some((p.x, p.y))
        }
    }
}

/// Compute the preview's target position + size for whichever monitor
/// the OS cursor currently lives on. Bottom-left of that monitor plus
/// a Dock-clearing margin. Returns `None` if no monitor is reachable.
fn cursor_monitor_target(win: &WebviewWindow) -> Option<(PhysicalPosition<i32>, PhysicalSize<u32>)> {
    let monitors = win.available_monitors().unwrap_or_default();
    let monitor = pick_cursor_monitor_globally(&monitors)
        .or_else(|| win.primary_monitor().ok().flatten())?;

    let mp = monitor.position();
    let ms = monitor.size();
    let scale = monitor.scale_factor();
    // Window size + margins in physical pixels (`set_position` /
    // `set_size` take physical units).
    let win_w_px = (WIN_W * scale) as i32;
    let win_h_px = (WIN_H * scale) as i32;
    let margin_x_px = ((EDGE_MARGIN_X as f64) * scale) as i32;
    let margin_y_px = ((EDGE_MARGIN_Y_BOTTOM as f64) * scale) as i32;
    let x = mp.x + margin_x_px;
    let y = mp.y + ms.height as i32 - win_h_px - margin_y_px;
    Some((
        PhysicalPosition::new(x, y),
        PhysicalSize::new(win_w_px as u32, win_h_px as u32),
    ))
}

/// Find the monitor containing the OS cursor using the **global**
/// cursor query (`CGEventGetLocation`) rather than
/// `WebviewWindow::cursor_position()` — the latter only refreshes when
/// the preview window itself receives a mouse event, so polling it
/// returned a stale position until the user clicked on a new monitor.
/// Returns `None` if the cursor query fails or no monitor contains
/// the cursor.
fn pick_cursor_monitor_globally(monitors: &[tauri::Monitor]) -> Option<tauri::Monitor> {
    #[cfg(target_os = "macos")]
    {
        let (cx_pt, cy_pt) = cg_cursor::position_in_points()?;
        // Bounds-check in POINTS — convert each monitor's physical
        // pixel bounds via its own scale factor (handles mixed-scale
        // multi-monitor setups correctly).
        monitors
            .iter()
            .find(|m| {
                let scale = m.scale_factor();
                let mp = m.position();
                let ms = m.size();
                let pt_x = mp.x as f64 / scale;
                let pt_y = mp.y as f64 / scale;
                let pt_w = ms.width as f64 / scale;
                let pt_h = ms.height as f64 / scale;
                cx_pt >= pt_x
                    && cx_pt < pt_x + pt_w
                    && cy_pt >= pt_y
                    && cy_pt < pt_y + pt_h
            })
            .cloned()
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Non-macOS fallback: we don't have a portable global-cursor
        // query yet. None here makes the caller fall back to
        // `primary_monitor`.
        let _ = monitors;
        None
    }
}

/// Create (or re-show) the preview window on the monitor that currently
/// holds the cursor, positioned at its bottom-left corner with a
/// margin. The React side reads the pending screenshot via
/// `get_pending_screenshot_path` and listens for `screenshot-pending`
/// events for subsequent captures while the window is already open.
pub fn show_preview(app: &AppHandle) -> tauri::Result<()> {
    let win = if let Some(existing) = app.get_webview_window(PREVIEW_LABEL) {
        existing
    } else {
        WebviewWindowBuilder::new(app, PREVIEW_LABEL, WebviewUrl::App("index.html".into()))
            .title("Screenshot")
            .inner_size(WIN_W, WIN_H)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .visible(false)
            .focused(false)
            .build()?
    };

    // Place + size for the current cursor monitor. The continuous
    // monitor-follow happens via the `reposition_preview_to_cursor`
    // IPC, called every 200 ms from the React side of the preview
    // window (the React polling is *much* more reliable than a
    // std::thread spawned on the Rust side — Tauri's IPC layer
    // marshals set_position onto the main thread cleanly, while a
    // bare std::thread doing the same has been flaky on macOS).
    if let Some((target_pos, target_size)) = cursor_monitor_target(&win) {
        let _ = win.set_position(target_pos);
        let _ = win.set_size(target_size);
    }

    // Notify the React side that there's a fresh capture to show. If
    // the window's just been built it'll pick the path up via the
    // `get_pending_screenshot_path` IPC on mount; the event covers
    // the case where the window is already open from a previous shot.
    let _ = win.emit("screenshot-pending", ());
    let _ = win.show();
    Ok(())
}

/// Tear down the preview window once the user has acted on the capture
/// (or the React auto-hide timer fires). Called from each of the three
/// action IPCs.
fn close_preview(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(PREVIEW_LABEL) {
        let _ = win.close();
    }
}

/// Cross-platform "open this file with the OS default app" — used by
/// the Edit action to hand the PNG to Preview.app / Photos / whatever
/// the user has registered for `.png`.
fn open_with_default(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/open").arg(path).spawn().map(|_| ())
    }
    #[cfg(target_os = "windows")]
    {
        // `cmd /c start "" <path>` — the empty quoted "" is the title,
        // required when the first arg contains spaces.
        std::process::Command::new("cmd")
            .args(["/c", "start", ""])
            .arg(path)
            .spawn()
            .map(|_| ())
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(path).spawn().map(|_| ())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = path;
        Err(std::io::Error::other("open not implemented on this platform"))
    }
}

/// Move the pending capture from the cache temp into ~/Downloads under
/// a friendly name. Returns the final destination path.
fn promote_to_downloads(temp: &std::path::Path) -> std::io::Result<PathBuf> {
    let dir = dirs::download_dir()
        .or_else(dirs::picture_dir)
        .ok_or_else(|| std::io::Error::other("no Downloads or Pictures dir"))?;
    let stem = format!(
        "inspector-rust-screenshot-{}.png",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let dest = dir.join(stem);
    // rename across the same volume is atomic; falls back to copy+remove
    // if temp + downloads are on different mounts.
    if std::fs::rename(temp, &dest).is_err() {
        std::fs::copy(temp, &dest)?;
        let _ = std::fs::remove_file(temp);
    }
    Ok(dest)
}

// ── IPC ─────────────────────────────────────────────────────────────────────

use tauri::State;

use crate::clipboard_watcher::WatcherState;
use crate::db::{self, DbHandle};

/// Read the on-disk path of the currently-pending screenshot. The
/// preview React component calls this on mount to know which PNG to
/// load. Returns `None` when there's nothing pending (e.g. window was
/// reopened from a previous session).
#[tauri::command]
pub fn get_pending_screenshot_path(state: State<'_, PendingScreenshot>) -> Option<String> {
    state.inner().0.lock().clone().map(|p| p.to_string_lossy().into_owned())
}

/// Save action — promote the temp PNG to ~/Downloads, push to clipboard,
/// add a history entry, close the preview window. Mirrors what the old
/// (auto-save) pipeline did, but only now that the user opted in.
#[tauri::command]
pub fn screenshot_preview_save(
    app: AppHandle,
    pending: State<'_, PendingScreenshot>,
) -> Result<(), String> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use clipboard_rs::{common::RustImage, Clipboard, ClipboardContext, RustImageData};

    let temp = pending.inner().0.lock().take().ok_or_else(|| "nothing pending".to_string())?;
    let bytes = std::fs::read(&temp).map_err(|e| format!("read pending {}: {e}", temp.display()))?;

    // Move to ~/Downloads first so a clipboard or history failure
    // doesn't leave the user without the file.
    let dest = promote_to_downloads(&temp).map_err(|e| format!("promote to Downloads: {e}"))?;

    // Clipboard.
    let b64 = B64.encode(&bytes);
    if let Some(watcher) = app.try_state::<WatcherState>() {
        watcher.mark_self_write(crate::models::ContentType::Image, &b64);
    }
    let ctx = ClipboardContext::new().map_err(|e| format!("clipboard ctx: {e:?}"))?;
    let img = RustImageData::from_bytes(&bytes).map_err(|e| format!("decode png: {e:?}"))?;
    ctx.set_image(img).map_err(|e| format!("set_image: {e:?}"))?;

    // History.
    if let Some(handle) = app.try_state::<DbHandle>() {
        let _ = db::upsert_clip(
            &handle,
            &crate::models::NewClip {
                content_type: crate::models::ContentType::Image,
                content_text: format!("[screenshot · {} B]", bytes.len()),
                content_data: b64,
                byte_size: bytes.len() as i64,
            },
        );
        let _ = app.emit("clipboard-changed", ());
    }

    let _ = app.emit("screenshot-saved", dest.to_string_lossy().to_string());
    close_preview(&app);
    Ok(())
}

/// Discard action — delete the temp file, close the preview. No
/// clipboard, no Downloads, no history. The default-on-auto-hide too.
#[tauri::command]
pub fn screenshot_preview_discard(
    app: AppHandle,
    pending: State<'_, PendingScreenshot>,
) -> Result<(), String> {
    if let Some(temp) = pending.inner().0.lock().take() {
        let _ = std::fs::remove_file(&temp);
    }
    close_preview(&app);
    Ok(())
}

/// Frontend-driven cursor-follow: the preview React component polls
/// this every 200 ms. If the cursor has crossed to a different monitor
/// since the last call, the preview window is re-positioned to the
/// bottom-left of the new monitor.
///
/// IPC instead of a pure Rust background thread because Tauri marshals
/// window operations onto the main thread inside its IPC layer — much
/// more reliable than spawning a std::thread that calls `set_position`
/// from a worker (which was flaky on macOS).
#[tauri::command]
pub fn reposition_preview_to_cursor(app: AppHandle) -> Result<(), String> {
    let Some(win) = app.get_webview_window(PREVIEW_LABEL) else {
        return Ok(()); // window already closed — no-op
    };
    let Some((target_pos, target_size)) = cursor_monitor_target(&win) else {
        return Ok(());
    };
    let current = match win.outer_position() {
        Ok(p) => p,
        Err(_) => return Ok(()),
    };
    // Reposition only on a monitor change. Within a single monitor the
    // target stays fixed (we always anchor to the same bottom-left)
    // so the diff is 0 and we don't churn set_position every tick.
    // 50 px tolerance absorbs window-decoration rounding etc.
    if (current.x - target_pos.x).abs() > 50 || (current.y - target_pos.y).abs() > 50 {
        let _ = win.set_position(target_pos);
        let _ = win.set_size(target_size);
    }
    Ok(())
}

/// Edit action — move the temp PNG to ~/Downloads and hand it to the
/// OS-default image viewer (Preview.app on macOS). The file persists
/// after editing so the user can save changes in place.
#[tauri::command]
pub fn screenshot_preview_edit(
    app: AppHandle,
    pending: State<'_, PendingScreenshot>,
) -> Result<(), String> {
    let temp = pending.inner().0.lock().take().ok_or_else(|| "nothing pending".to_string())?;
    let dest = promote_to_downloads(&temp).map_err(|e| format!("promote: {e}"))?;
    if let Err(e) = open_with_default(&dest) {
        // Don't surface as fatal — the file is on disk, the user can
        // open it themselves. Just log.
        tracing::warn!("open {} with default app: {e}", dest.display());
    }
    close_preview(&app);
    Ok(())
}
