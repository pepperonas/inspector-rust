//! macOS drag-to-snap: a global `CGEventTap` watches the left-mouse drag, the
//! Accessibility API moves/resizes the dragged (focused) window, and a
//! transparent Tauri overlay previews the target zone. All geometry math is the
//! pure, unit-tested core in `super`.
//!
//! Threading: the tap runs on a dedicated `CFRunLoop` thread (like
//! `input_lock`/`gestures`). Window/overlay ops are marshalled to the main
//! thread via `run_on_main_thread`. AX *reads* run inline on the tap thread;
//! the AX frame *setter* is ALSO bounced to the main thread — for other apps'
//! windows it is a thread-safe MIG call, but for a window of our OWN process
//! (dragged screenshot pin, popup) AX short-circuits in-process into AppKit,
//! which macOS 26 hard-asserts onto the main thread (silent EXC_BREAKPOINT).
//! Requires **Accessibility** (`expander::accessibility_granted`).

use super::{armed_zone, classify_zone, cocoa_rect_to_topleft, dwell_step, zone_rect, DwellState, Rect};
use objc2::encode::{Encode, Encoding};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use parking_lot::Mutex;
use std::ffi::{c_void, CString};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

const OVERLAY_LABEL: &str = "snap-overlay";

// ── CoreFoundation / CoreGraphics / AX FFI ───────────────────────────────────

type CFTypeRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFStringRef = *const c_void;
type CFRunLoopRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFMachPortRef = *mut c_void;
type CGEventRef = *mut c_void;
type CGEventTapProxy = *mut c_void;
type AXUIElementRef = *const c_void;
type AXValueRef = *const c_void;

const KCF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const KAX_VALUE_CGPOINT: u32 = 1;
const KAX_VALUE_CGSIZE: u32 = 2;

// CGEventType
const EVT_LEFT_MOUSE_DOWN: u32 = 1;
const EVT_LEFT_MOUSE_UP: u32 = 2;
const EVT_LEFT_MOUSE_DRAGGED: u32 = 6;
const EVT_KEY_DOWN: u32 = 10;
const EVT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const EVT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
// CGEventTapLocation / placement / options
const CG_SESSION_TAP: u32 = 1;
const CG_HEAD_INSERT: u32 = 0;
const CG_TAP_LISTEN_ONLY: u32 = 1; // never consume — the window must still drag
const CG_KEYBOARD_KEYCODE_FIELD: u32 = 9;
const ESC_KEYCODE: i64 = 53;

type CGEventTapCallBack = extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

/// NSRect / CGRect for `msg_send` struct returns (origin{x,y}, size{w,h}).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NsRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}
unsafe impl Encode for NsRect {
    const ENCODING: Encoding = Encoding::Struct(
        "CGRect",
        &[
            Encoding::Struct("CGPoint", &[Encoding::Double, Encoding::Double]),
            Encoding::Struct("CGSize", &[Encoding::Double, Encoding::Double]),
        ],
    );
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFAllocatorDefault: CFAllocatorRef;
    static kCFRunLoopCommonModes: CFStringRef;
    fn CFStringCreateWithCString(a: CFAllocatorRef, s: *const i8, enc: u32) -> CFStringRef;
    fn CFRelease(cf: CFTypeRef);
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    // Bounded run — the tap thread re-checks RUNNING between slices instead of
    // depending on a CFRunLoopStop landing in the window between the RUN_LOOP
    // publish and the loop actually running (the same publish race fixed in
    // gestures/macos.rs the same day; here a missed stop leaked a live tap
    // thread per snapping toggle instead of hanging).
    fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source_handled: bool)
        -> i32;
    static kCFRunLoopDefaultMode: CFStringRef;
    fn CFRunLoopStop(rl: CFRunLoopRef);
    fn CFMachPortCreateRunLoopSource(
        a: *mut c_void,
        port: CFMachPortRef,
        order: isize,
    ) -> CFRunLoopSourceRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, src: CFRunLoopSourceRef, mode: CFStringRef);
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;

    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        el: AXUIElementRef,
        attr: CFStringRef,
        out: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(el: AXUIElementRef, attr: CFStringRef, value: CFTypeRef) -> i32;
    fn AXValueCreate(value_type: u32, value_ptr: *const c_void) -> AXValueRef;
}

fn cfstr(s: &str) -> CFStringRef {
    let c = CString::new(s).unwrap();
    unsafe { CFStringCreateWithCString(kCFAllocatorDefault, c.as_ptr(), KCF_STRING_ENCODING_UTF8) }
}

// ── Screen geometry (NSScreen visibleFrame → top-left) ───────────────────────

/// Visible frames of every screen, in top-left coords. NSScreen reads are
/// thread-safe enough for geometry (no mutation); we snapshot at drag start.
pub(crate) fn screens_topleft() -> Vec<Rect> {
    unsafe {
        let cls = class!(NSScreen);
        let arr: *mut AnyObject = msg_send![cls, screens];
        if arr.is_null() {
            return Vec::new();
        }
        let n: usize = msg_send![arr, count];
        if n == 0 {
            return Vec::new();
        }
        // The primary screen (index 0, the menu-bar screen) defines the global
        // flip pivot.
        let s0: *mut AnyObject = msg_send![arr, objectAtIndex: 0usize];
        let f0: NsRect = msg_send![s0, frame];
        let primary_h = f0.h;
        (0..n)
            .map(|i| {
                let s: *mut AnyObject = msg_send![arr, objectAtIndex: i];
                let vf: NsRect = msg_send![s, visibleFrame];
                cocoa_rect_to_topleft(Rect::new(vf.x, vf.y, vf.w, vf.h), primary_h)
            })
            .collect()
    }
}

/// The screen (visible frame, top-left) whose bounds contain `cursor`, else the
/// first screen (best effort for a cursor between displays).
pub(crate) fn screen_for_cursor(cursor: (f64, f64), screens: &[Rect]) -> Option<Rect> {
    let (cx, cy) = cursor;
    screens
        .iter()
        .find(|r| cx >= r.x && cx < r.x + r.w && cy >= r.y && cy < r.y + r.h)
        .or_else(|| screens.first())
        .copied()
}

// ── Accessibility window move/resize ─────────────────────────────────────────

/// The focused window of the frontmost app (caller releases). At mouse-down of
/// a titlebar drag this is the window being dragged.
pub(crate) unsafe fn focused_window() -> Option<AXUIElementRef> {
    let sw = AXUIElementCreateSystemWide();
    if sw.is_null() {
        return None;
    }
    let app_attr = cfstr("AXFocusedApplication");
    let mut app: CFTypeRef = std::ptr::null();
    let r1 = AXUIElementCopyAttributeValue(sw, app_attr, &mut app);
    CFRelease(app_attr);
    CFRelease(sw as CFTypeRef);
    if r1 != 0 || app.is_null() {
        return None;
    }
    let win_attr = cfstr("AXFocusedWindow");
    let mut win: CFTypeRef = std::ptr::null();
    let r2 = AXUIElementCopyAttributeValue(app as AXUIElementRef, win_attr, &mut win);
    CFRelease(win_attr);
    CFRelease(app);
    if r2 != 0 || win.is_null() {
        return None;
    }
    Some(win as AXUIElementRef)
}

/// Move + resize `win` to `rect` (top-left coords). Position is set **before and
/// after** size so an app that clamps size still lands at the zone origin; a
/// fixed-size window simply ignores the size set (→ move-only). Caller retains
/// ownership of `win` (does not release it).
fn snap_window(win: AXUIElementRef, rect: Rect) {
    unsafe {
        let set_pos = |w: AXUIElementRef| {
            let p = CGPoint { x: rect.x, y: rect.y };
            let v = AXValueCreate(KAX_VALUE_CGPOINT, &p as *const _ as *const c_void);
            let a = cfstr("AXPosition");
            let rc = AXUIElementSetAttributeValue(w, a, v);
            if rc != 0 {
                tracing::debug!("window-snap: AXPosition set failed rc={rc}");
            }
            CFRelease(a);
            CFRelease(v as CFTypeRef);
        };
        set_pos(win);
        let s = CGSize {
            width: rect.w,
            height: rect.h,
        };
        let sv = AXValueCreate(KAX_VALUE_CGSIZE, &s as *const _ as *const c_void);
        let sa = cfstr("AXSize");
        let rc = AXUIElementSetAttributeValue(win, sa, sv);
        if rc != 0 {
            tracing::debug!("window-snap: AXSize set failed rc={rc}");
        }
        CFRelease(sa);
        CFRelease(sv as CFTypeRef);
        set_pos(win);
    }
}

/// Retain the focused AX window for this drag (window-palette `TARGET_WIN`
/// pattern). We must NOT re-resolve focus at mouse-up: hiding the full-screen
/// maximize overlay can steal/clear focus, so a fresh `AXFocusedWindow` then
/// returns None / the wrong window and the snap silently no-ops while the
/// preview looked correct.
fn capture_drag_win() {
    release_drag_win();
    unsafe {
        if let Some(win) = focused_window() {
            // `focused_window` already returns a +1 (Copy rule); store as-is.
            DRAG_WIN.store(win as isize, Ordering::SeqCst);
            tracing::debug!("window-snap: captured drag window");
        } else {
            tracing::debug!("window-snap: no focused window at drag start");
        }
    }
}

fn release_drag_win() {
    let prev = DRAG_WIN.swap(0, Ordering::SeqCst);
    if prev != 0 {
        unsafe { CFRelease(prev as CFTypeRef) };
    }
}

fn take_drag_win() -> Option<AXUIElementRef> {
    let w = DRAG_WIN.swap(0, Ordering::SeqCst);
    if w == 0 {
        None
    } else {
        Some(w as AXUIElementRef)
    }
}

// ── Drag state (read by the tap callback, which can't capture) ───────────────

static RUNNING: AtomicBool = AtomicBool::new(false);
static DRAGGING: AtomicBool = AtomicBool::new(false);
static CANCELLED: AtomicBool = AtomicBool::new(false);
static TAP_PORT: AtomicIsize = AtomicIsize::new(0);
static RUN_LOOP: AtomicIsize = AtomicIsize::new(0);
/// Retained `AXUIElementRef` of the window being dragged (as isize). Captured
/// at mouse-down; applied at mouse-up. Mirrors window_palette's `TARGET_WIN`.
static DRAG_WIN: AtomicIsize = AtomicIsize::new(0);
static SCREENS: Mutex<Vec<Rect>> = Mutex::new(Vec::new());
/// Dwell-arming state (v0.165.0) — the classified zone plus whether it has
/// dwelled long enough to be LIVE. Replaces the old bare `CURRENT_ZONE`:
/// preview + snap now hang off `armed_zone()`, never the raw classification.
static DWELL: Mutex<DwellState> = Mutex::new(DwellState {
    candidate: None,
    since_ms: 0,
    armed: false,
});
/// Configured dwell in ms (0 = instant). Atomic so a settings change applies
/// live to the running tap thread.
static DWELL_MS: AtomicU64 = AtomicU64::new(super::DEFAULT_DWELL_MS);
/// Last drag cursor — the between-slice tick needs a position to arm a
/// PERFECTLY still cursor (a resting hand produces no mouseDragged events).
static LAST_CURSOR: Mutex<Option<(f64, f64)>> = Mutex::new(None);
static CURRENT_TARGET: Mutex<Option<Rect>> = Mutex::new(None);
static APP: Mutex<Option<AppHandle>> = Mutex::new(None);

/// Monotonic ms for the dwell clock (process-relative; only differences count).
fn now_ms() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Push the configured dwell (called from `apply`, also while running).
pub(crate) fn set_dwell_ms(ms: u64) {
    DWELL_MS.store(ms.min(super::DWELL_MS_MAX), Ordering::Relaxed);
}

fn app_handle() -> Option<AppHandle> {
    APP.lock().clone()
}

/// Position + show (or hide) the preview overlay on the main thread.
fn update_overlay(target: Option<Rect>) {
    let Some(app) = app_handle() else { return };
    show_overlay(&app, target);
}

/// Show the shared `snap-overlay` window over `target` (top-left coords), or hide
/// it when `None`. Reusable from any thread (marshals to the main thread). The
/// window-palette feature reuses this for its live screen-outline preview.
pub(crate) fn show_overlay(app: &AppHandle, target: Option<Rect>) {
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || match target {
        Some(r) => {
            let win = match app.get_webview_window(OVERLAY_LABEL) {
                Some(w) => w,
                None => match build_overlay(&app) {
                    Some(w) => w,
                    None => return,
                },
            };
            // Logical (points) coords match AX / NSScreen point space.
            let _ = win.set_position(LogicalPosition::new(r.x, r.y));
            let _ = win.set_size(LogicalSize::new(r.w.max(1.0), r.h.max(1.0)));
            let _ = win.set_ignore_cursor_events(true);
            // Show via `orderFrontRegardless` — NOT Tauri `show()`, which is
            // `makeKeyAndOrderFront` and made this passive outline the KEY
            // window on every preview. That key-steal broke the window
            // palette's WKWebView hover tracking (pointerenter/leave stopped
            // firing → preset previews froze / flapped hidden) and would
            // likewise steal key from the app being drag-snapped.
            let mut ordered = false;
            if let Ok(ns) = win.ns_window() {
                let w = ns as *mut AnyObject;
                if !w.is_null() {
                    unsafe {
                        let _: () = msg_send![w, orderFrontRegardless];
                    }
                    ordered = true;
                }
            }
            if !ordered {
                let _ = win.show();
            }
            tracing::debug!("snap-overlay: show at {r:?}");
        }
        None => {
            if let Some(win) = app.get_webview_window(OVERLAY_LABEL) {
                let _ = win.hide();
                tracing::debug!("snap-overlay: hide");
            }
        }
    });
}

fn build_overlay(app: &AppHandle) -> Option<WebviewWindow> {
    match WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App("index.html".into()))
        .title("Snap")
        .inner_size(200.0, 200.0)
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
            let _ = w.set_ignore_cursor_events(true);
            Some(w)
        }
        Err(e) => {
            tracing::warn!("window-snap: overlay build failed: {e:#}");
            None
        }
    }
}

extern "C" fn tap_callback(
    _proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    _info: *mut c_void,
) -> CGEventRef {
    if !RUNNING.load(Ordering::Relaxed) {
        return event;
    }
    match event_type {
        EVT_TAP_DISABLED_BY_TIMEOUT | EVT_TAP_DISABLED_BY_USER_INPUT => {
            let port = TAP_PORT.load(Ordering::SeqCst) as CFMachPortRef;
            if !port.is_null() {
                unsafe { CGEventTapEnable(port, true) };
            }
        }
        EVT_LEFT_MOUSE_DOWN => {
            DRAGGING.store(true, Ordering::Relaxed);
            CANCELLED.store(false, Ordering::Relaxed);
            *DWELL.lock() = DwellState::default();
            *LAST_CURSOR.lock() = None;
            *CURRENT_TARGET.lock() = None;
            let s = screens_topleft();
            tracing::debug!("snap: mousedown, {} screen(s); first={:?}", s.len(), s.first());
            *SCREENS.lock() = s;
            // Pin the window NOW — at mouse-up focus may already be gone
            // (especially after hiding a full-screen maximize overlay).
            capture_drag_win();
        }
        EVT_LEFT_MOUSE_DRAGGED => {
            if !DRAGGING.load(Ordering::Relaxed) || CANCELLED.load(Ordering::Relaxed) {
                return event;
            }
            // Late capture if mousedown missed focus (rare; belt-and-braces).
            if DRAG_WIN.load(Ordering::SeqCst) == 0 {
                capture_drag_win();
            }
            let p = unsafe { CGEventGetLocation(event) };
            let cursor = (p.x, p.y);
            // Copy out the one matching Rect under the lock — cloning the
            // whole Vec allocated on every mouseDragged event (100+/s).
            let screen = {
                let screens = SCREENS.lock();
                screen_for_cursor(cursor, &screens)
            };
            let Some(screen) = screen else {
                return event;
            };
            *LAST_CURSOR.lock() = Some(cursor);
            let prev = *DWELL.lock();
            // Classification (with its hysteresis) is unchanged; dwell-arming
            // (v0.165.0) is a layer ON TOP: only an ARMED zone shows the
            // preview or snaps, so a quick place-near-the-edge stays inert.
            let zone = classify_zone(cursor, screen, prev.candidate);
            let next = dwell_step(prev, zone, now_ms(), DWELL_MS.load(Ordering::Relaxed));
            if next != prev {
                *DWELL.lock() = next;
            }
            if armed_zone(next) != armed_zone(prev) {
                let target = armed_zone(next).map(|z| zone_rect(z, screen));
                tracing::debug!(
                    "snap: armed {:?} -> {:?} cursor=({:.0},{:.0}) screen={screen:?}",
                    armed_zone(prev), armed_zone(next), cursor.0, cursor.1
                );
                *CURRENT_TARGET.lock() = target;
                update_overlay(target);
            }
        }
        EVT_LEFT_MOUSE_UP => {
            let was_dragging = DRAGGING.swap(false, Ordering::Relaxed);
            let cancelled = CANCELLED.swap(false, Ordering::Relaxed);
            let target = CURRENT_TARGET.lock().take();
            *DWELL.lock() = DwellState::default();
            *LAST_CURSOR.lock() = None;
            let win = take_drag_win();
            update_overlay(None);
            tracing::debug!(
                "snap: mouseup dragging={was_dragging} cancelled={cancelled} target={target:?} win={}",
                win.is_some()
            );
            if was_dragging && !cancelled {
                if let (Some(rect), Some(win)) = (target, win) {
                    // ⚠️ On the MAIN thread, not inline on this tap thread.
                    // For windows of OTHER apps the AX setter is a thread-safe
                    // out-of-process MIG call and inline was fine for years —
                    // but when the focused window belongs to OUR OWN process
                    // (a dragged screenshot pin, the popup), AX short-circuits
                    // in-process straight into AppKit, and macOS 26's
                    // WindowManagement hard-asserts the main thread:
                    // EXC_BREAKPOINT, no Rust panic, no crash.log — the app
                    // just vanishes. Field crash 2026-08-06, same family as
                    // the iris overlay-build crash (see iris.rs).
                    // Apply to the RETAINED window from mouse-down — do not
                    // re-query AXFocusedWindow (overlay hide can clear it).
                    // Carry the AX ref as isize so the closure is `Send`.
                    let win_ptr = win as isize;
                    if let Some(app) = app_handle() {
                        let _ = app.run_on_main_thread(move || {
                            let win = win_ptr as AXUIElementRef;
                            snap_window(win, rect);
                            unsafe { CFRelease(win as CFTypeRef) };
                        });
                    } else {
                        snap_window(win, rect);
                        unsafe { CFRelease(win as CFTypeRef) };
                    }
                } else if let Some(win) = win {
                    unsafe { CFRelease(win as CFTypeRef) };
                }
            } else if let Some(win) = win {
                unsafe { CFRelease(win as CFTypeRef) };
            }
        }
        // Esc during a drag → cancel the pending snap + hide the overlay.
        EVT_KEY_DOWN
            if DRAGGING.load(Ordering::Relaxed)
                && unsafe { CGEventGetIntegerValueField(event, CG_KEYBOARD_KEYCODE_FIELD) }
                    == ESC_KEYCODE =>
        {
            CANCELLED.store(true, Ordering::Relaxed);
            *DWELL.lock() = DwellState::default();
            *LAST_CURSOR.lock() = None;
            *CURRENT_TARGET.lock() = None;
            release_drag_win();
            update_overlay(None);
        }
        _ => {}
    }
    event
}

/// Between-slice dwell tick (see the run loop): advances arming for a cursor
/// that is resting inside a zone without generating drag events.
fn maybe_arm_still_cursor() {
    if !DRAGGING.load(Ordering::Relaxed) || CANCELLED.load(Ordering::Relaxed) {
        return;
    }
    let prev = *DWELL.lock();
    if prev.candidate.is_none() || prev.armed {
        return;
    }
    let Some(cursor) = *LAST_CURSOR.lock() else { return };
    let screen = {
        let screens = SCREENS.lock();
        screen_for_cursor(cursor, &screens)
    };
    let Some(screen) = screen else { return };
    let next = dwell_step(prev, prev.candidate, now_ms(), DWELL_MS.load(Ordering::Relaxed));
    if next != prev {
        *DWELL.lock() = next;
    }
    if armed_zone(next) != armed_zone(prev) {
        let target = armed_zone(next).map(|z| zone_rect(z, screen));
        tracing::debug!("snap: armed via still-cursor tick -> {:?}", armed_zone(next));
        *CURRENT_TARGET.lock() = target;
        update_overlay(target);
    }
}

fn install_tap_thread() {
    std::thread::Builder::new()
        .name("ir-window-snap".into())
        .spawn(|| unsafe {
            let mask = (1u64 << EVT_LEFT_MOUSE_DOWN)
                | (1u64 << EVT_LEFT_MOUSE_UP)
                | (1u64 << EVT_LEFT_MOUSE_DRAGGED)
                | (1u64 << EVT_KEY_DOWN);
            let tap = CGEventTapCreate(
                CG_SESSION_TAP,
                CG_HEAD_INSERT,
                CG_TAP_LISTEN_ONLY,
                mask,
                tap_callback,
                std::ptr::null_mut(),
            );
            if tap.is_null() {
                tracing::warn!(
                    "window-snap: event tap unavailable (grant Accessibility to System Settings → Privacy → Accessibility)"
                );
                RUNNING.store(false, Ordering::SeqCst);
                return;
            }
            TAP_PORT.store(tap as isize, Ordering::SeqCst);
            let src = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            RUN_LOOP.store(CFRunLoopGetCurrent() as isize, Ordering::SeqCst);
            tracing::info!("window-snap: drag monitor armed");
            while RUNNING.load(Ordering::SeqCst) {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, false);
                // A PERFECTLY still cursor produces no mouseDragged events,
                // so the dwell could never elapse from the callback alone —
                // this between-slice tick arms it (worst case dwell + 250 ms;
                // a real hand jitters and arms precisely via the callback).
                maybe_arm_still_cursor();
            }
            RUN_LOOP.store(0, Ordering::SeqCst);
        })
        .ok();
}

/// Enable/disable the drag-to-snap monitor.
pub fn set_active(app: &tauri::AppHandle, enabled: bool) {
    if enabled {
        if RUNNING.swap(true, Ordering::SeqCst) {
            return; // already running
        }
        *APP.lock() = Some(app.clone());
        if !crate::expander::accessibility_granted() {
            tracing::warn!("window-snap: Accessibility not granted — snapping won't fire until it is");
        }
        install_tap_thread();
    } else {
        RUNNING.store(false, Ordering::SeqCst);
        let port = TAP_PORT.swap(0, Ordering::SeqCst) as CFMachPortRef;
        if !port.is_null() {
            unsafe { CGEventTapEnable(port, false) };
        }
        let rl = RUN_LOOP.swap(0, Ordering::SeqCst);
        if rl != 0 {
            unsafe { CFRunLoopStop(rl as CFRunLoopRef) };
        }
        release_drag_win();
        update_overlay(None);
    }
}

#[cfg(test)]
mod tests {
    use super::{screen_for_cursor, Rect};

    const A: Rect = Rect { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 };
    // Secondary directly to the right of A.
    const B: Rect = Rect { x: 1440.0, y: 0.0, w: 1920.0, h: 1080.0 };

    #[test]
    fn picks_the_screen_containing_the_cursor() {
        assert_eq!(screen_for_cursor((100.0, 100.0), &[A, B]), Some(A));
        assert_eq!(screen_for_cursor((2000.0, 500.0), &[A, B]), Some(B));
    }

    #[test]
    fn shared_edge_belongs_to_the_right_screen_only() {
        // Half-open [x, x+w): x = 1440 is the right edge of A (excluded) and
        // the left edge of B (included) — adjacent screens never both claim it.
        assert_eq!(screen_for_cursor((1440.0, 200.0), &[A, B]), Some(B));
        // One px left → still A.
        assert_eq!(screen_for_cursor((1439.0, 200.0), &[A, B]), Some(A));
    }

    #[test]
    fn falls_back_to_the_first_screen_when_between_displays() {
        // A cursor in no screen's bounds (e.g. a gap or just off the bottom)
        // falls back to the first screen rather than returning None.
        assert_eq!(screen_for_cursor((100.0, 5000.0), &[A, B]), Some(A));
    }

    #[test]
    fn single_screen_is_always_returned() {
        assert_eq!(screen_for_cursor((-50.0, -50.0), &[A]), Some(A));
    }

    #[test]
    fn empty_screen_list_is_none() {
        assert_eq!(screen_for_cursor((0.0, 0.0), &[]), None);
    }
}
