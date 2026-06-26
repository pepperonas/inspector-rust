//! macOS window-palette implementation: a global mouse-move monitor + a dwell
//! timer detect a hover over a window's green **zoom button** (AX subrole
//! `AXZoomButton`), then anchor a Tauri overlay (`window-palette`) under it.
//! Picking a preset / dragging the hex grid sends a 0..1 fraction back, which is
//! mapped to an absolute rect on the button's screen and applied to that window
//! via the Accessibility API. Reuses `window_snap`'s screen helpers + `Rect`.
//!
//! Hover detection is a `CGEventTap` on `mouseMoved` (cheap when idle) plus a
//! generation-guarded **dwell timer** so a *still* cursor over the button still
//! triggers (mouse-moved alone never fires while the cursor is stationary). AX
//! calls use a short messaging timeout so an unresponsive app can't hang us.

use super::{fraction_to_rect, PaletteContext, WindowPaletteConfig};
use crate::window_snap::macos::{screen_for_cursor, screens_topleft};
use crate::window_snap::Rect;
use parking_lot::Mutex;
use std::ffi::{c_void, CString};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};

const PALETTE_LABEL: &str = "window-palette";
const PAL_W: f64 = 300.0;
const PAL_H: f64 = 236.0;
const GAP: f64 = 8.0;
const HOVER_DELAY_MS: u64 = 180;
const GRACE_MS: u128 = 260;
const HITTEST_THROTTLE: Duration = Duration::from_millis(50);
const EDGE_MARGIN: f64 = 6.0;

// ── FFI types ────────────────────────────────────────────────────────────────

type CFTypeRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFStringRef = *const c_void;
type CFRunLoopRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFMachPortRef = *mut c_void;
type CFIndex = isize;
type CGEventRef = *mut c_void;
type CGEventTapProxy = *mut c_void;
type AXUIElementRef = *const c_void;
type AXValueRef = *const c_void;

const KCF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const KAX_VALUE_CGPOINT: u32 = 1;
const KAX_VALUE_CGSIZE: u32 = 2;

const EVT_MOUSE_MOVED: u32 = 5;
const EVT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const EVT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
const CG_SESSION_TAP: u32 = 1;
const CG_HEAD_INSERT: u32 = 0;
const CG_TAP_LISTEN_ONLY: u32 = 1;

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

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFAllocatorDefault: CFAllocatorRef;
    static kCFRunLoopCommonModes: CFStringRef;
    fn CFStringCreateWithCString(a: CFAllocatorRef, s: *const i8, enc: u32) -> CFStringRef;
    fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> u8;
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn CFRelease(cf: CFTypeRef);
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopRun();
    fn CFRunLoopStop(rl: CFRunLoopRef);
    fn CFMachPortCreateRunLoopSource(a: *mut c_void, port: CFMachPortRef, order: isize) -> CFRunLoopSourceRef;
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

    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyElementAtPosition(
        app: AXUIElementRef,
        x: f32,
        y: f32,
        out: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementCopyAttributeValue(el: AXUIElementRef, attr: CFStringRef, out: *mut CFTypeRef) -> i32;
    fn AXUIElementSetAttributeValue(el: AXUIElementRef, attr: CFStringRef, value: CFTypeRef) -> i32;
    fn AXUIElementSetMessagingTimeout(el: AXUIElementRef, timeout: f32) -> i32;
    fn AXValueCreate(value_type: u32, value_ptr: *const c_void) -> AXValueRef;
    fn AXValueGetValue(value: AXValueRef, value_type: u32, value_ptr: *mut c_void) -> u8;
}

fn cfstr(s: &str) -> CFStringRef {
    let c = CString::new(s).unwrap();
    unsafe { CFStringCreateWithCString(kCFAllocatorDefault, c.as_ptr(), KCF_STRING_ENCODING_UTF8) }
}

/// Whether the element's string attribute `attr` equals `expected` — compared
/// via `CFEqual` against a freshly-built CFString (no buffer read-out, so no
/// `CFStringGetCString` FFI). `CFEqual` safely returns false for a non-string.
unsafe fn attr_equals(el: AXUIElementRef, attr: &str, expected: &str) -> bool {
    let a = cfstr(attr);
    let mut v: CFTypeRef = std::ptr::null();
    let e = AXUIElementCopyAttributeValue(el, a, &mut v);
    CFRelease(a);
    if e != 0 || v.is_null() {
        return false;
    }
    let want = cfstr(expected);
    let eq = CFEqual(v, want) != 0;
    CFRelease(want);
    CFRelease(v);
    eq
}

unsafe fn axvalue_point(el: AXUIElementRef, attr: &str) -> Option<(f64, f64)> {
    let a = cfstr(attr);
    let mut v: CFTypeRef = std::ptr::null();
    let e = AXUIElementCopyAttributeValue(el, a, &mut v);
    CFRelease(a);
    if e != 0 || v.is_null() {
        return None;
    }
    let mut p = CGPoint { x: 0.0, y: 0.0 };
    let ok = AXValueGetValue(v as AXValueRef, KAX_VALUE_CGPOINT, &mut p as *mut _ as *mut c_void);
    CFRelease(v);
    (ok != 0).then_some((p.x, p.y))
}

unsafe fn axvalue_size(el: AXUIElementRef, attr: &str) -> Option<(f64, f64)> {
    let a = cfstr(attr);
    let mut v: CFTypeRef = std::ptr::null();
    let e = AXUIElementCopyAttributeValue(el, a, &mut v);
    CFRelease(a);
    if e != 0 || v.is_null() {
        return None;
    }
    let mut s = CGSize { width: 0.0, height: 0.0 };
    let ok = AXValueGetValue(v as AXValueRef, KAX_VALUE_CGSIZE, &mut s as *mut _ as *mut c_void);
    CFRelease(v);
    (ok != 0).then_some((s.width, s.height))
}

unsafe fn element_frame(el: AXUIElementRef) -> Option<Rect> {
    let (px, py) = axvalue_point(el, "AXPosition")?;
    let (w, h) = axvalue_size(el, "AXSize")?;
    Some(Rect::new(px, py, w, h))
}

/// Walk up `kAXParent` from `el` to the enclosing `AXWindow`; returns it
/// **retained** (caller releases), or None.
unsafe fn window_of(el: AXUIElementRef) -> Option<AXUIElementRef> {
    let mut cur = el;
    let mut owned: Option<AXUIElementRef> = None; // intermediate parent we own
    for _ in 0..8 {
        if attr_equals(cur, "AXRole", "AXWindow") {
            CFRetain(cur);
            if let Some(o) = owned {
                CFRelease(o);
            }
            return Some(cur);
        }
        let a = cfstr("AXParent");
        let mut p: CFTypeRef = std::ptr::null();
        let e = AXUIElementCopyAttributeValue(cur, a, &mut p);
        CFRelease(a);
        if let Some(o) = owned.take() {
            CFRelease(o);
        }
        if e != 0 || p.is_null() {
            return None;
        }
        cur = p as AXUIElementRef; // +1 (Copy rule) — now ours
        owned = Some(cur);
    }
    if let Some(o) = owned {
        CFRelease(o);
    }
    None
}

/// Hit-test the cursor: if it's over a window's zoom button, return its frame
/// (top-left) + the enclosing window (retained). AX timeouts guard against hung
/// apps.
unsafe fn zoom_hit(cursor: (f64, f64)) -> Option<(Rect, AXUIElementRef)> {
    let sw = AXUIElementCreateSystemWide();
    if sw.is_null() {
        return None;
    }
    AXUIElementSetMessagingTimeout(sw, 0.2);
    let mut el: CFTypeRef = std::ptr::null();
    let e = AXUIElementCopyElementAtPosition(sw, cursor.0 as f32, cursor.1 as f32, &mut el);
    CFRelease(sw as CFTypeRef);
    if e != 0 || el.is_null() {
        return None;
    }
    let el = el as AXUIElementRef;
    let is_zoom = attr_equals(el, "AXSubrole", "AXZoomButton");
    if !is_zoom {
        CFRelease(el as CFTypeRef);
        return None;
    }
    let frame = element_frame(el);
    let win = window_of(el);
    CFRelease(el as CFTypeRef);
    match (frame, win) {
        (Some(f), Some(w)) => Some((f, w)),
        (None, Some(w)) => {
            CFRelease(w as CFTypeRef);
            None
        }
        _ => None,
    }
}

/// Set a specific window's frame (top-left coords): position, then size, then
/// position again — a fixed-size window simply keeps its size (move-only).
unsafe fn set_window_frame(win: AXUIElementRef, rect: Rect) {
    let set_pos = |w: AXUIElementRef| {
        let p = CGPoint { x: rect.x, y: rect.y };
        let v = AXValueCreate(KAX_VALUE_CGPOINT, &p as *const _ as *const c_void);
        let a = cfstr("AXPosition");
        AXUIElementSetAttributeValue(w, a, v);
        CFRelease(a);
        CFRelease(v as CFTypeRef);
    };
    set_pos(win);
    let s = CGSize { width: rect.w, height: rect.h };
    let sv = AXValueCreate(KAX_VALUE_CGSIZE, &s as *const _ as *const c_void);
    let sa = cfstr("AXSize");
    AXUIElementSetAttributeValue(win, sa, sv);
    CFRelease(sa);
    CFRelease(sv as CFTypeRef);
    set_pos(win);
}

fn rect_contains(r: Rect, p: (f64, f64), margin: f64) -> bool {
    p.0 >= r.x - margin
        && p.0 <= r.x + r.w + margin
        && p.1 >= r.y - margin
        && p.1 <= r.y + r.h + margin
}

// ── State ────────────────────────────────────────────────────────────────────

static RUNNING: AtomicBool = AtomicBool::new(false);
static PALETTE_SHOWN: AtomicBool = AtomicBool::new(false);
static TAP_PORT: AtomicIsize = AtomicIsize::new(0);
static RUN_LOOP: AtomicIsize = AtomicIsize::new(0);
static TARGET_WIN: AtomicIsize = AtomicIsize::new(0); // retained AXUIElementRef as isize
static HOVER_GEN: AtomicU64 = AtomicU64::new(0);
static LAST_CURSOR: Mutex<(f64, f64)> = Mutex::new((0.0, 0.0));
static HOVER_BTN_FRAME: Mutex<Option<Rect>> = Mutex::new(None);
static PALETTE_RECT: Mutex<Option<Rect>> = Mutex::new(None);
static TARGET_VF: Mutex<Option<Rect>> = Mutex::new(None);
static OUT_SINCE: Mutex<Option<Instant>> = Mutex::new(None);
static LAST_HITTEST: Mutex<Option<Instant>> = Mutex::new(None);
static CONFIG: Mutex<(u32, u32)> = Mutex::new((super::DEFAULT_COLS, super::DEFAULT_ROWS));
static APP: Mutex<Option<AppHandle>> = Mutex::new(None);

fn app_handle() -> Option<AppHandle> {
    APP.lock().clone()
}

fn release_target() {
    let prev = TARGET_WIN.swap(0, Ordering::SeqCst);
    if prev != 0 {
        unsafe { CFRelease(prev as CFTypeRef) };
    }
}

// ── Show / hide ──────────────────────────────────────────────────────────────

unsafe fn try_show_if(gen: u64) {
    if HOVER_GEN.load(Ordering::SeqCst) != gen || PALETTE_SHOWN.load(Ordering::SeqCst) {
        return;
    }
    let cursor = *LAST_CURSOR.lock();
    let frame = match *HOVER_BTN_FRAME.lock() {
        Some(f) => f,
        None => return,
    };
    if !rect_contains(frame, cursor, 2.0) {
        return; // cursor left the button before the dwell elapsed
    }
    // Re-hit to get a fresh retained window element + current frame.
    let Some((frame, win)) = zoom_hit(cursor) else { return };
    let screens = screens_topleft();
    let center = (frame.x + frame.w / 2.0, frame.y + frame.h / 2.0);
    let Some(vf) = screen_for_cursor(center, &screens) else {
        CFRelease(win as CFTypeRef);
        return;
    };
    let px = (frame.x + frame.w / 2.0 - PAL_W / 2.0).clamp(vf.x + 4.0, (vf.x + vf.w - PAL_W - 4.0).max(vf.x + 4.0));
    let py = (frame.y + frame.h + GAP).clamp(vf.y + 4.0, (vf.y + vf.h - PAL_H - 4.0).max(vf.y + 4.0));

    release_target();
    TARGET_WIN.store(win as isize, Ordering::SeqCst);
    *TARGET_VF.lock() = Some(vf);
    *PALETTE_RECT.lock() = Some(Rect::new(px, py, PAL_W, PAL_H));
    *HOVER_BTN_FRAME.lock() = Some(frame);
    *OUT_SINCE.lock() = None;
    PALETTE_SHOWN.store(true, Ordering::SeqCst);

    if let Some(app) = app_handle() {
        let a = app.clone();
        let _ = app.run_on_main_thread(move || show_window(&a, px, py));
    }
}

fn show_window(app: &AppHandle, x: f64, y: f64) {
    let win = match app.get_webview_window(PALETTE_LABEL) {
        Some(w) => w,
        None => {
            match WebviewWindowBuilder::new(app, PALETTE_LABEL, WebviewUrl::App("index.html".into()))
                .title("Window palette")
                .inner_size(PAL_W, PAL_H)
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
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("window-palette: build failed: {e:#}");
                    return;
                }
            }
        }
    };
    let _ = win.set_position(LogicalPosition::new(x, y));
    let _ = win.set_size(LogicalSize::new(PAL_W, PAL_H));
    let _ = win.show();
    // Tell the (reused) webview to (re)load context + reset its drag state.
    let _ = app.emit("window-palette-shown", ());
}

fn hide_palette() {
    if !PALETTE_SHOWN.swap(false, Ordering::SeqCst)
        && PALETTE_RECT.lock().is_none()
        && TARGET_WIN.load(Ordering::SeqCst) == 0
    {
        return;
    }
    release_target();
    *PALETTE_RECT.lock() = None;
    *HOVER_BTN_FRAME.lock() = None;
    *OUT_SINCE.lock() = None;
    *TARGET_VF.lock() = None;
    if let Some(app) = app_handle() {
        let a = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(w) = a.get_webview_window(PALETTE_LABEL) {
                let _ = w.hide();
            }
        });
    }
}

// ── Move handling ────────────────────────────────────────────────────────────

unsafe fn on_grace(cursor: (f64, f64)) {
    let over_pal = PALETTE_RECT.lock().map(|r| rect_contains(r, cursor, EDGE_MARGIN)).unwrap_or(false);
    let over_btn = HOVER_BTN_FRAME.lock().map(|r| rect_contains(r, cursor, EDGE_MARGIN)).unwrap_or(false);
    if over_pal || over_btn {
        *OUT_SINCE.lock() = None;
        return;
    }
    let mut out = OUT_SINCE.lock();
    match *out {
        None => *out = Some(Instant::now()),
        Some(t) => {
            if t.elapsed().as_millis() > GRACE_MS {
                drop(out);
                hide_palette();
            }
        }
    }
}

unsafe fn maybe_hittest(cursor: (f64, f64)) {
    {
        let mut l = LAST_HITTEST.lock();
        let now = Instant::now();
        if let Some(t) = *l {
            if now.duration_since(t) < HITTEST_THROTTLE {
                return;
            }
        }
        *l = Some(now);
    }
    match zoom_hit(cursor) {
        Some((frame, win)) => {
            CFRelease(win as CFTypeRef); // re-hit at show time
            let was_over = HOVER_BTN_FRAME.lock().replace(frame).is_some();
            if !was_over {
                let gen = HOVER_GEN.fetch_add(1, Ordering::SeqCst) + 1;
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(HOVER_DELAY_MS));
                    unsafe { try_show_if(gen) };
                });
            }
        }
        None => {
            if HOVER_BTN_FRAME.lock().take().is_some() {
                HOVER_GEN.fetch_add(1, Ordering::SeqCst); // invalidate any pending dwell
            }
        }
    }
}

extern "C" fn on_event(_proxy: CGEventTapProxy, ty: u32, event: CGEventRef, _info: *mut c_void) -> CGEventRef {
    if !RUNNING.load(Ordering::Relaxed) {
        return event;
    }
    match ty {
        EVT_TAP_DISABLED_BY_TIMEOUT | EVT_TAP_DISABLED_BY_USER_INPUT => {
            let port = TAP_PORT.load(Ordering::SeqCst) as CFMachPortRef;
            if !port.is_null() {
                unsafe { CGEventTapEnable(port, true) };
            }
        }
        EVT_MOUSE_MOVED => {
            let p = unsafe { CGEventGetLocation(event) };
            let cursor = (p.x, p.y);
            *LAST_CURSOR.lock() = cursor;
            unsafe {
                if PALETTE_SHOWN.load(Ordering::SeqCst) {
                    on_grace(cursor);
                } else {
                    maybe_hittest(cursor);
                }
            }
        }
        _ => {}
    }
    event
}

fn install_tap_thread() {
    std::thread::Builder::new()
        .name("ir-window-palette".into())
        .spawn(|| unsafe {
            let mask = 1u64 << EVT_MOUSE_MOVED;
            let tap = CGEventTapCreate(CG_SESSION_TAP, CG_HEAD_INSERT, CG_TAP_LISTEN_ONLY, mask, on_event, std::ptr::null_mut());
            if tap.is_null() {
                tracing::warn!("window-palette: event tap unavailable (grant Accessibility)");
                RUNNING.store(false, Ordering::SeqCst);
                return;
            }
            TAP_PORT.store(tap as isize, Ordering::SeqCst);
            let src = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            RUN_LOOP.store(CFRunLoopGetCurrent() as isize, Ordering::SeqCst);
            tracing::info!("window-palette: hover monitor armed");
            CFRunLoopRun();
            RUN_LOOP.store(0, Ordering::SeqCst);
        })
        .ok();
}

// ── Public (called from `super::apply` + the IPC commands) ────────────────────

pub(crate) fn set_active(app: &AppHandle, _db: &crate::db::DbHandle, cfg: WindowPaletteConfig) {
    *APP.lock() = Some(app.clone());
    *CONFIG.lock() = (cfg.cols, cfg.rows);
    if cfg.enabled {
        if RUNNING.swap(true, Ordering::SeqCst) {
            return; // already running (config updated above)
        }
        if !crate::expander::accessibility_granted() {
            tracing::warn!("window-palette: Accessibility not granted — palette won't appear until it is");
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
        hide_palette();
    }
}

pub(crate) fn context() -> PaletteContext {
    let vf = TARGET_VF.lock().unwrap_or(Rect::new(0.0, 0.0, 1440.0, 900.0));
    let (cols, rows) = *CONFIG.lock();
    PaletteContext { cols, rows, screen_w: vf.w, screen_h: vf.h }
}

pub(crate) fn apply_fraction(fx: f64, fy: f64, fw: f64, fh: f64) {
    let win = TARGET_WIN.swap(0, Ordering::SeqCst);
    let vf = *TARGET_VF.lock();
    if win != 0 {
        if let Some(vf) = vf {
            let rect = fraction_to_rect(fx, fy, fw, fh, vf);
            unsafe { set_window_frame(win as AXUIElementRef, rect) };
        }
        unsafe { CFRelease(win as CFTypeRef) };
    }
    hide_palette();
}

pub(crate) fn cancel() {
    hide_palette();
}
