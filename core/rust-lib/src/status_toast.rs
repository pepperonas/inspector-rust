//! `status_toast` — a brief, on-screen status flourish (v0.51.0+).
//!
//! A small frameless, transparent, click-through, always-on-top window
//! centred on the cursor's monitor that plays a short animation and then
//! auto-dismisses itself. Used after the popup hides to confirm a state
//! change in a way that's visible *over* whatever app the user just
//! returned to — currently the **wakelock** on/off toggle, styled like
//! the hidden-game intro flourishes.
//!
//! Lifecycle mirrors `screenshot_preview`: the React side pulls the
//! current payload via `get_status_toast` on mount and re-animates on
//! each `"status-toast-changed"` event (so a reused window picks up a
//! fresh toggle), then calls `hide_status_toast` when its timer fires.
//! The window is **hidden**, not closed, between toasts so re-showing is
//! instant.

use parking_lot::Mutex;
use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

pub const TOAST_LABEL: &str = "status-toast";

/// Logical (point) size of the toast window. Big enough for a large icon
/// + a line of title text with breathing room for the pop animation.
const WIN_W: f64 = 360.0;
const WIN_H: f64 = 200.0;

/// The payload the React side renders. Generic so future one-shot status
/// confirmations (freeze, timer, …) can reuse the same window.
#[derive(Debug, Clone, Serialize, Default)]
pub struct StatusToast {
    /// Logical kind — drives icon/colour selection on the frontend.
    pub kind: String,
    /// On/off sense of the state (e.g. wakelock enabled vs disabled).
    pub on: bool,
    /// Headline, e.g. `"Wakelock On"`.
    pub title: String,
    /// Sub-line, e.g. `"Your Mac stays awake"`.
    pub subtitle: String,
}

/// Tauri-managed holder for the most recent toast payload. The frontend
/// pulls it via `get_status_toast` rather than relying on event-delivery
/// timing (same pull model as the screenshot preview).
#[derive(Default)]
pub struct LatestToast(pub Mutex<Option<StatusToast>>);

/// Store `toast` as the current payload, then show (building once,
/// reusing thereafter) the toast window centred on the cursor's monitor
/// and notify the frontend to (re)play its animation.
pub fn show(app: &AppHandle, toast: StatusToast) {
    tracing::debug!("status_toast: show kind={} on={}", toast.kind, toast.on);
    if let Some(state) = app.try_state::<LatestToast>() {
        *state.0.lock() = Some(toast);
    } else {
        tracing::warn!("status_toast: LatestToast state missing — payload not stored");
    }

    let win = match app.get_webview_window(TOAST_LABEL) {
        Some(existing) => existing,
        None => {
            match WebviewWindowBuilder::new(app, TOAST_LABEL, WebviewUrl::App("index.html".into()))
                .title("Status")
                .inner_size(WIN_W, WIN_H)
                .resizable(false)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .shadow(false)
                .visible(false)
                .focused(false)
                .build()
            {
                Ok(w) => {
                    // Passive overlay — never intercept clicks so the user
                    // can keep working under it during the ~1.5 s it shows.
                    let _ = w.set_ignore_cursor_events(true);
                    w
                }
                Err(e) => {
                    tracing::warn!("status_toast: window build failed: {e:#}");
                    return;
                }
            }
        }
    };

    center_on_cursor_monitor(&win);
    // Re-assert click-through on reuse (cheap; harmless if already set).
    let _ = win.set_ignore_cursor_events(true);
    // A freshly-built window picks the payload up via `get_status_toast`
    // on mount; the event covers the already-open (reused) window.
    let _ = win.emit("status-toast-changed", ());
    let _ = win.show();
    // Force it on-screen + frontmost. After `app.hide()` (run at toggle
    // time) the app is hidden, and an Accessory app's `show()` alone does
    // not reliably order a fresh window in — `set_focus` makes it key and
    // brings the overlay forward over whatever app is now frontmost.
    let _ = win.set_focus();
}

/// Close the popup and announce `toast` on-screen — the shared flow used
/// by wakelock, timer, alarm, …: hide the popup the normal way (on macOS
/// that also `app.hide()`s so focus returns to the prior app), then show
/// the toast a beat LATER on the main thread. The delay lets the app-hide
/// settle so the fresh Accessory-app window reliably orders on-screen
/// (mirrors the screenshot-preview flow).
pub fn announce(app: &AppHandle, toast: StatusToast) {
    crate::hotkey::hide_popup(app);
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(90));
        let app3 = app2.clone();
        let _ = app2.run_on_main_thread(move || {
            show(&app3, toast);
        });
    });
}

/// Hide (not close) the toast window so the next toast re-shows instantly.
/// On macOS this also fires `app.hide()` to return key focus to whatever
/// app was frontmost before the popup opened — deferred to here (rather
/// than at toggle time) so the toast isn't swallowed by the app-hide while
/// it's still animating.
pub fn hide(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(TOAST_LABEL) {
        let _ = win.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.hide();
}

/// Park the window onto the cursor's monitor, then centre it there. Park
/// first so `current_monitor()` resolves to the right display before we
/// read its geometry (mirrors `hotkey::show_and_position`).
fn center_on_cursor_monitor(win: &WebviewWindow) {
    crate::hotkey::park_on_cursor_monitor(win);
    let monitor = win
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| win.primary_monitor().ok().flatten());
    let Some(m) = monitor else { return };
    let mp = m.position();
    let ms = m.size();
    let scale = m.scale_factor();
    let w_px = (WIN_W * scale) as i32;
    let h_px = (WIN_H * scale) as i32;
    let x = mp.x + (ms.width as i32 - w_px) / 2;
    let y = mp.y + (ms.height as i32 - h_px) / 2;
    let _ = win.set_position(PhysicalPosition::new(x, y));
    let _ = win.set_size(PhysicalSize::new(w_px.max(1) as u32, h_px.max(1) as u32));
}
