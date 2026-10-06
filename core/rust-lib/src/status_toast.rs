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
//! fresh toggle), then clears itself and calls `hide_status_toast` when its
//! timer fires.
//!
//! ⚠️ Once built, the window is NEVER ordered out again (2026-09-28). A toast
//! window that had been hidden came back blank: the NSWindow was on screen
//! again (CGWindowList: on-screen, alpha 1) but its WKWebView stayed
//! suspended — no paint, no JS — so it never dismissed itself either. Field
//! symptom: "gestures change the volume but show no HUD", with an empty
//! click-through window parked on screen for hours; reproduced live (the
//! first toast of a process works, every later one is blank). So the toast
//! follows the iris-overlay recipe, which has always rendered reliably: a
//! permanently shown, transparent, click-through window with
//! `setCanHide:NO` (survives `app.hide()`); "hidden" = the frontend renders
//! nothing. Destroying + rebuilding per toast also worked but leaked a native
//! window per toast (7 → 12 after six toasts), so it was dropped.

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
                // The volume HUD can be dragged (v0.197.0): the first click on
                // this never-key window must reach the webview, not just
                // activate the window.
                .accept_first_mouse(true)
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

    // One short safety re-center after show, in case showing / Space-joining
    // perturbed the frame. The native macOS path is synchronous + reliable, so a
    // single quick re-apply is enough (a longer delay would risk the toast
    // "following" the cursor to another screen if the user moved meanwhile).
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(90));
        let app3 = app2.clone();
        let _ = app2.run_on_main_thread(move || {
            if let Some(w) = app3.get_webview_window(TOAST_LABEL) {
                if w.is_visible().unwrap_or(false) {
                    center_on_cursor_monitor(&w);
                }
            }
        });
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
        // Survive `app.hide()` (every popup close runs it): an ordered-out
        // toast webview never renders again (module doc).
        let _: () = objc2::msg_send![nswindow, setCanHide: false];
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

/// Announce `toast` while the popup STAYS OPEN (v0.129.1 — the footer's
/// dark-wake toggle): the app is active (the user just clicked inside the
/// popup), so the fresh toast window orders on-screen directly — no app-hide
/// settle dance needed. The toast is click-through + transient, so briefly
/// overlapping the popup is fine; `hide()`'s popup-visible guard keeps the
/// later toast-hide from `app.hide()`-ing the popup away.
pub fn announce_keeping_popup(app: &AppHandle, toast: StatusToast) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        let app3 = app2.clone();
        show(&app3, toast);
    });
}

/// The frontend has emptied the toast (it is now an invisible, click-through
/// surface). The window itself stays ordered in — see the module doc. On
/// macOS this also fires `app.hide()` to return key focus to whatever
/// app was frontmost before the popup opened — deferred to here (rather
/// than at toggle time) so the toast isn't swallowed by the app-hide while
/// it's still animating.
pub fn hide(app: &AppHandle) {
    // Deliberately NO `win.hide()`: an ordered-out toast webview comes back
    // suspended and blank (module doc). Non-macOS keeps hiding — the bug is
    // WebKit-on-macOS, and a hidden window there costs nothing.
    #[cfg(not(target_os = "macos"))]
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
        // Never `app.hide()` while the POPUP is on screen (v0.129.1): the
        // footer's dark-wake toggle announces its toast with the popup kept
        // open — an app-hide here would close it out from under the user.
        let popup_visible = app
            .get_webview_window(crate::hotkey::POPUP_LABEL)
            .map(|w| w.is_visible().unwrap_or(false))
            .unwrap_or(false);
        if !passive && !popup_visible {
            let _ = app.hide();
        }
    }
}

// ── Mouse gate for the volume HUD (v0.197.0) ────────────────────────────────
//
// The toast is click-through so it never eats a click meant for the app under
// it. The volume HUD's slider must be clickable, though — so while the HUD
// shows, the frontend reports its card rect and a ~30 Hz gate makes the window
// accept the mouse ONLY while the pointer is inside that rect. Anywhere else
// the window stays click-through, exactly as before.

/// A rect in window-local logical px (CSS px, origin top-left).
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct HitRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Pure: is the window-local point inside the rect (half-open)?
pub fn hit(px: f64, py: f64, r: &HitRect) -> bool {
    px >= r.x && px < r.x + r.width && py >= r.y && py < r.y + r.height
}

/// Pure: a Cocoa global point (origin bottom-left) → window-local px with the
/// origin at the window's TOP-left, given the window frame in Cocoa points.
pub fn cocoa_to_local(mx: f64, my: f64, fx: f64, fy: f64, fh: f64) -> (f64, f64) {
    (mx - fx, (fy + fh) - my)
}

static HIT_RECT: Mutex<Option<HitRect>> = Mutex::new(None);
static GATE_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const GATE_TICK_MS: u64 = 33;
/// Emitted on every enter/leave of the card; mirrors `HOVER_EVENT` in
/// `lib/volume-hud.ts`.
pub const HOVER_EVENT: &str = "status-toast-hover";

/// Set (or clear with `None`) the clickable rect. Starts the gate when needed;
/// clearing makes the window click-through again on the next tick.
pub fn set_hit_rect(app: &AppHandle, rect: Option<HitRect>) {
    *HIT_RECT.lock() = rect;
    if rect.is_none() {
        if let Some(w) = app.get_webview_window(TOAST_LABEL) {
            let _ = w.set_ignore_cursor_events(true);
        }
        return;
    }
    if GATE_RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return; // already polling — it reads the new rect
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let mut inside_last: Option<bool> = None;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(GATE_TICK_MS));
            let Some(rect) = *HIT_RECT.lock() else { break };
            let (tx, rx) = std::sync::mpsc::channel();
            let app2 = app.clone();
            let _ = app.run_on_main_thread(move || {
                let _ = tx.send(app2.get_webview_window(TOAST_LABEL).and_then(|w| pointer_local(&w)));
            });
            let Ok(Some((px, py))) = rx.recv_timeout(std::time::Duration::from_millis(500)) else { continue };
            let inside = hit(px, py, &rect);
            if inside_last != Some(inside) {
                inside_last = Some(inside);
                if let Some(w) = app.get_webview_window(TOAST_LABEL) {
                    let _ = w.set_ignore_cursor_events(!inside);
                }
                // The HUD stays while hovered (2026-10-06). The frontend can't
                // rely on DOM pointerleave: this gate makes the window
                // click-through the moment the pointer leaves the card.
                let _ = app.emit(HOVER_EVENT, inside);
            }
        }
        if let Some(w) = app.get_webview_window(TOAST_LABEL) {
            let _ = w.set_ignore_cursor_events(true);
        }
        if inside_last == Some(true) {
            let _ = app.emit(HOVER_EVENT, false);
        }
        GATE_RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        // A rect set while we were shutting down needs a fresh gate.
        if HIT_RECT.lock().is_some() {
            set_hit_rect(&app, *HIT_RECT.lock());
        }
    });
}

/// The pointer in window-local logical px. macOS reads AppKit directly
/// (`NSEvent.mouseLocation` + the window frame, both Cocoa points — no DPI
/// conversion to get wrong). Must run on the main thread.
#[cfg(target_os = "macos")]
fn pointer_local(win: &WebviewWindow) -> Option<(f64, f64)> {
    use objc2::encode::{Encode, Encoding};
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct P {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct S {
        w: f64,
        h: f64,
    }
    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct R {
        o: P,
        s: S,
    }
    unsafe impl Encode for P {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for S {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for R {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[P::ENCODING, S::ENCODING]);
    }
    let ns = win.ns_window().ok()? as *mut AnyObject;
    if ns.is_null() {
        return None;
    }
    let cls = AnyClass::get(c"NSEvent")?;
    unsafe {
        let m: P = msg_send![cls, mouseLocation];
        let f: R = msg_send![ns, frame];
        Some(cocoa_to_local(m.x, m.y, f.o.x, f.o.y, f.s.h))
    }
}

#[cfg(not(target_os = "macos"))]
fn pointer_local(win: &WebviewWindow) -> Option<(f64, f64)> {
    let c = win.app_handle().cursor_position().ok()?;
    let p = win.outer_position().ok()?;
    let sf = win.scale_factor().ok()?;
    Some(((c.x - p.x as f64) / sf, (c.y - p.y as f64) / sf))
}

/// After the user dragged the slider the app may have become active (the
/// click activated it). Hand focus back unless the popup is open.
pub fn release_focus(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let popup_visible = app
            .get_webview_window(crate::hotkey::POPUP_LABEL)
            .map(|w| w.is_visible().unwrap_or(false))
            .unwrap_or(false);
        if !popup_visible {
            let _ = app.hide();
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// Centre the toast on the monitor **under the cursor**, resolved via the global
/// cursor query (`pick_cursor_monitor_globally`) — NOT `win.current_monitor()`,
/// which returns a *stale* association (e.g. an external display that was just
/// unplugged) and parked the toast off-screen (v0.84.121). Falls back to the
/// primary monitor. `available_monitors()` always reflects the live set, so a
/// disconnected display can't be chosen.
fn center_on_cursor_monitor(win: &WebviewWindow) {
    // macOS: position natively via the NSWindow (Cocoa points, no Tauri
    // set_position/set_size scale-lag or coordinate conversion). This is the
    // reliable path; the Tauri path below is the fallback + Win/Linux.
    #[cfg(target_os = "macos")]
    {
        if center_native_macos(win) {
            return;
        }
    }
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

/// macOS: centre the toast on whichever **NSScreen** the cursor is on, natively
/// via `setFrame:display:` in Cocoa points. This sidesteps Tauri's
/// `set_position`/`set_size` (which convert via the window's *current* scale
/// factor — wrong/laggy right after the cursor crosses to a different-DPI
/// display) and the physical↔logical coordinate dance entirely: NSWindow works
/// in points and re-renders the webview at the destination screen's backing
/// scale automatically. Returns `false` if the AppKit objects aren't reachable
/// (caller falls back to the Tauri path). Must run on the main thread.
#[cfg(target_os = "macos")]
fn center_native_macos(win: &WebviewWindow) -> bool {
    use objc2::encode::{Encode, Encoding};
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct NSPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct NSSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct NSRect {
        origin: NSPoint,
        size: NSSize,
    }
    unsafe impl Encode for NSPoint {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for NSSize {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for NSRect {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[NSPoint::ENCODING, NSSize::ENCODING]);
    }

    let Ok(ptr) = win.ns_window() else {
        return false;
    };
    let nswindow = ptr as *mut AnyObject;
    if nswindow.is_null() {
        return false;
    }

    unsafe {
        let (Some(ns_event), Some(ns_screen)) =
            (AnyClass::get(c"NSEvent"), AnyClass::get(c"NSScreen"))
        else {
            return false;
        };
        // Global cursor in Cocoa coords (bottom-left origin, points) — same space
        // as NSScreen.frame, so containment is a direct compare.
        let cursor: NSPoint = msg_send![ns_event, mouseLocation];
        let screens: *mut AnyObject = msg_send![ns_screen, screens];
        if screens.is_null() {
            return false;
        }
        let count: usize = msg_send![screens, count];

        let mut target: *mut AnyObject = std::ptr::null_mut();
        for i in 0..count {
            let s: *mut AnyObject = msg_send![screens, objectAtIndex: i];
            if s.is_null() {
                continue;
            }
            let f: NSRect = msg_send![s, frame];
            if cursor.x >= f.origin.x
                && cursor.x < f.origin.x + f.size.width
                && cursor.y >= f.origin.y
                && cursor.y < f.origin.y + f.size.height
            {
                target = s;
                break;
            }
        }
        if target.is_null() {
            target = msg_send![ns_screen, mainScreen];
        }
        if target.is_null() {
            return false;
        }

        let sf: NSRect = msg_send![target, frame];
        // Centre a canonical WIN_W × WIN_H (points) window on the target screen.
        let w = WIN_W;
        let h = WIN_H;
        let x = sf.origin.x + (sf.size.width - w) / 2.0;
        let y = sf.origin.y + (sf.size.height - h) / 2.0;
        let frame = NSRect {
            origin: NSPoint { x, y },
            size: NSSize { width: w, height: h },
        };
        let _: () = msg_send![nswindow, setFrame: frame, display: true];
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression 2026-09-28: an ordered-out toast webview came back blank, so
    /// gestures showed no HUD. The macOS toast must survive `app.hide()` and
    /// must never be ordered out by our own hide path.
    #[test]
    fn hit_is_half_open_and_cocoa_flips_y() {
        let r = HitRect { x: 50.0, y: 60.0, width: 200.0, height: 80.0 };
        assert!(hit(50.0, 60.0, &r));
        assert!(hit(249.9, 139.9, &r));
        assert!(!hit(250.0, 100.0, &r));
        assert!(!hit(100.0, 140.0, &r));
        assert!(!hit(49.9, 100.0, &r));
        // Window frame origin (1000, 500) bottom-left, height 200: a pointer at
        // Cocoa y 680 is 20 px below the window's top edge (700).
        assert_eq!(cocoa_to_local(1100.0, 680.0, 1000.0, 500.0, 200.0), (100.0, 20.0));
    }

    #[test]
    fn macos_toast_is_never_ordered_out() {
        let src = include_str!("status_toast.rs");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        // Needle assembled at runtime so this test can't find its own text.
        let needle = format!("{}: false]", "setCanHide");
        assert!(code.contains(&needle), "toast must opt out of app.hide()");
        let hide = code.split("pub fn hide(").nth(1).unwrap();
        let hide = &hide[..hide.find("\n}\n").unwrap()];
        let before_cfg = hide.split("#[cfg(not(target_os = \"macos\"))]").next().unwrap();
        assert!(!before_cfg.contains("win.hide()"), "hide() must not order the toast out on macOS");
    }
}
