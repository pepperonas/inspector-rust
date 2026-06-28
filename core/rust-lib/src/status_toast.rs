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
    show_inner(app, toast, true);
}

/// Like [`show`] but **never steals focus** (`set_focus` is skipped) — for
/// passive overlays fired while the user works in another app (touchpad-gesture
/// volume/mute), where yanking focus on every swipe would be disruptive.
pub fn show_passive(app: &AppHandle, toast: StatusToast) {
    show_inner(app, toast, false);
}

fn show_inner(app: &AppHandle, toast: StatusToast, focus: bool) {
    tracing::debug!("status_toast: show kind={} on={} focus={focus}", toast.kind, toast.on);
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
    // Make the HUD show on the *current* Space and over fullscreen apps without
    // switching Spaces or stealing focus — essential for the passive (gesture)
    // toast, which otherwise stays on the Space where the window was first built
    // and is invisible while a fullscreen app (e.g. a video / Spotify
    // fullscreen) is frontmost.
    elevate_toast_window(&win);
    // Force it on-screen + frontmost. After `app.hide()` (run at toggle
    // time) the app is hidden, and an Accessory app's `show()` alone does
    // not reliably order a fresh window in — `set_focus` makes it key and
    // brings the overlay forward over whatever app is now frontmost. Skipped
    // for passive toasts so a gesture never pulls focus from the active app
    // (elevate_toast_window's orderFrontRegardless brings it forward instead).
    if focus {
        let _ = win.set_focus();
    }

    // Re-center after showing. `set_size(PhysicalSize)` converts via the window's
    // *current* scale factor, which lags a move to a different display (mixed-DPI
    // / right after the cursor crossed to another screen) — so the synchronous
    // placement above can land off-centre or on the previous screen. Re-apply at
    // two points so both a fast and a slow scale/Space settle are caught (each is
    // idempotent — re-centres to the same spot if already correct).
    let app2 = app.clone();
    std::thread::spawn(move || {
        for delay_ms in [110u64, 320] {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            let app3 = app2.clone();
            let _ = app2.run_on_main_thread(move || {
                if let Some(w) = app3.get_webview_window(TOAST_LABEL) {
                    if w.is_visible().unwrap_or(false) {
                        center_on_cursor_monitor(&w);
                    }
                }
            });
        }
    });
}

/// macOS: let the toast join all Spaces + fullscreen, and order it front
/// without making it key (no focus theft). No-op elsewhere.
#[cfg(target_os = "macos")]
fn elevate_toast_window(win: &WebviewWindow) {
    use objc2::runtime::AnyObject;
    let Ok(ptr) = win.ns_window() else { return };
    let nswindow = ptr as *mut AnyObject;
    if nswindow.is_null() {
        return;
    }
    // NSWindowCollectionBehavior: CanJoinAllSpaces (1<<0) | Stationary (1<<4) |
    // FullScreenAuxiliary (1<<8).
    let behavior: u64 = (1 << 0) | (1 << 4) | (1 << 8);
    unsafe {
        let _: () = objc2::msg_send![nswindow, setCollectionBehavior: behavior];
        let _: () = objc2::msg_send![nswindow, orderFrontRegardless];
    }
}

#[cfg(not(target_os = "macos"))]
fn elevate_toast_window(_win: &WebviewWindow) {}

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
    // Passive toasts (gesture volume/mute) never took focus, so don't
    // `app.hide()` — that would needlessly perturb the frontmost app / Spaces.
    #[cfg(target_os = "macos")]
    {
        let passive = app
            .try_state::<LatestToast>()
            .and_then(|s| s.0.lock().as_ref().map(|t| matches!(t.kind.as_str(), "volume" | "mute")))
            .unwrap_or(false);
        if !passive {
            let _ = app.hide();
        }
    }
}

/// Centre the toast on the monitor **under the cursor**, resolved via the global
/// cursor query (`pick_cursor_monitor_globally`) — NOT `win.current_monitor()`,
/// which returns a *stale* association (e.g. an external display that was just
/// unplugged) and parked the toast off-screen (v0.84.121). Falls back to the
/// primary monitor. `available_monitors()` always reflects the live set, so a
/// disconnected display can't be chosen.
fn center_on_cursor_monitor(win: &WebviewWindow) {
    let monitors = win.available_monitors().unwrap_or_default();
    let monitor = crate::screenshot_preview::pick_cursor_monitor_globally(&monitors)
        .or_else(|| win.primary_monitor().ok().flatten());
    let Some(m) = monitor else {
        tracing::warn!("status_toast: no monitor resolved ({} available)", monitors.len());
        return;
    };
    let mp = m.position();
    let ms = m.size();
    let scale = m.scale_factor();
    let w_px = (WIN_W * scale) as i32;
    let h_px = (WIN_H * scale) as i32;
    let x = mp.x + (ms.width as i32 - w_px) / 2;
    let y = mp.y + (ms.height as i32 - h_px) / 2;
    tracing::debug!(
        "status_toast: place at ({x},{y}) size {w_px}x{h_px} on monitor pos=({},{}) size={}x{} ({} avail)",
        mp.x, mp.y, ms.width, ms.height, monitors.len()
    );
    // Move onto the target display first (updates the window's scale), then size.
    let _ = win.set_position(PhysicalPosition::new(x, y));
    let _ = win.set_size(PhysicalSize::new(w_px.max(1) as u32, h_px.max(1) as u32));
    // Re-assert the position after the size/scale settles, in case set_size
    // shifted it (mixed-DPI: the move to a different-scale display lags).
    let _ = win.set_position(PhysicalPosition::new(x, y));
}
