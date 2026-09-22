//! macOS cursor wrap-around: a listen-only `CGEventTap` on mouse moves reads
//! the cursor position + the raw movement delta, runs the pure [`wrap_target`]
//! core, and — when it says so — teleports the pointer with
//! `CGWarpMouseCursorPosition`.
//!
//! Threading mirrors `window_snap`/`gestures`: the tap lives on a dedicated
//! `CFRunLoop` thread; `RUNNING`/`TAP_PORT`/`RUN_LOOP` gate start/stop. This is
//! the impure FFI edge — the repo leaves it untested (needs a live machine +
//! Accessibility); the geometry it calls is exhaustively unit-tested in `mod.rs`.
//!
//! Coordinates are global top-left points throughout (`CGEventGetLocation`,
//! `CGDisplayBounds` and `CGWarpMouseCursorPosition` all agree), so nothing is
//! converted.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering};

use parking_lot::Mutex;

use super::{wrap_target, CursorWrapConfig, Rect};

type CFStringRef = *const c_void;
type CFRunLoopRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFMachPortRef = *mut c_void;
type CGEventRef = *mut c_void;
type CGEventTapProxy = *mut c_void;
type CGDirectDisplayID = u32;

// CGEventType
const EVT_MOUSE_MOVED: u32 = 5;
const EVT_LEFT_MOUSE_DRAGGED: u32 = 6;
const EVT_RIGHT_MOUSE_DRAGGED: u32 = 7;
const EVT_OTHER_MOUSE_DRAGGED: u32 = 27;
const EVT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const EVT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

// CGMouseEventField
const FIELD_DELTA_X: u32 = 4;
const FIELD_DELTA_Y: u32 = 5;

// CGEventTapLocation / placement / options
const CG_SESSION_TAP: u32 = 1;
const CG_HEAD_INSERT: u32 = 0;
const CG_TAP_LISTEN_ONLY: u32 = 1; // never consume — the cursor must move normally

/// Ignore edge triggers for this long after a warp, so the wrap can't loop or
/// jitter at the boundary.
const COOLDOWN_MS: u64 = 120;

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
#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopCommonModes: CFStringRef;
    static kCFRunLoopDefaultMode: CFStringRef;
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source_handled: bool)
        -> i32;
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
    fn CGWarpMouseCursorPosition(new_cursor_position: CGPoint) -> i32;
    fn CGAssociateMouseAndMouseCursorPosition(connected: bool) -> i32;
    fn CGGetActiveDisplayList(
        max_displays: u32,
        active_displays: *mut CGDirectDisplayID,
        display_count: *mut u32,
    ) -> i32;
    fn CGDisplayBounds(display: CGDirectDisplayID) -> CGRect;
}

// ── State ────────────────────────────────────────────────────────────────────

static RUNNING: AtomicBool = AtomicBool::new(false);
static TAP_PORT: AtomicIsize = AtomicIsize::new(0);
static RUN_LOOP: AtomicIsize = AtomicIsize::new(0);
static LAST_WARP_MS: AtomicU64 = AtomicU64::new(0);
/// Live config (edges / dead-zone / drag). Locked per mouse move — uncontended,
/// so a settings change applies to the running tap without a restart.
static CONFIG: Mutex<CursorWrapConfig> = Mutex::new(CursorWrapConfig {
    enabled: true,
    left: true,
    right: true,
    top: true,
    bottom: true,
    corner_deadzone_px: super::DEFAULT_CORNER_DEADZONE_PX,
    wrap_during_drag: false,
});
/// Cached display rects (top-left points). Refreshed each run-loop slice so a
/// hotplug is picked up within ~0.25 s without a syscall on every mouse move.
static SCREENS: Mutex<Vec<Rect>> = Mutex::new(Vec::new());

fn now_ms() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Push the live config (called from `apply`, also while running).
pub(crate) fn set_config(cfg: &CursorWrapConfig) {
    *CONFIG.lock() = *cfg;
}

/// Read every active display's bounds as top-left-point rects.
fn display_rects() -> Vec<Rect> {
    unsafe {
        let mut count: u32 = 0;
        if CGGetActiveDisplayList(0, std::ptr::null_mut(), &mut count) != 0 || count == 0 {
            return Vec::new();
        }
        let mut ids = vec![0u32; count as usize];
        if CGGetActiveDisplayList(count, ids.as_mut_ptr(), &mut count) != 0 {
            return Vec::new();
        }
        ids.iter()
            .take(count as usize)
            .map(|&id| {
                let b = CGDisplayBounds(id);
                Rect::new(b.origin.x, b.origin.y, b.size.width, b.size.height)
            })
            .collect()
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
            return event;
        }
        EVT_MOUSE_MOVED => {}
        EVT_LEFT_MOUSE_DRAGGED | EVT_RIGHT_MOUSE_DRAGGED | EVT_OTHER_MOUSE_DRAGGED => {
            // A dragged event means a button is held. Only wrap mid-drag when
            // explicitly enabled — otherwise a wrap would drop the drag.
            if !CONFIG.lock().wrap_during_drag {
                return event;
            }
        }
        _ => return event,
    }

    // Refractory window after a warp — prevents loops/jitter at the boundary.
    let now = now_ms();
    if now.saturating_sub(LAST_WARP_MS.load(Ordering::Relaxed)) < COOLDOWN_MS {
        return event;
    }

    let p = unsafe { CGEventGetLocation(event) };
    // Raw movement delta — nonzero even while the cursor is clamped at the
    // outer bound, which is exactly the "still pushing into the edge" signal.
    let dx = unsafe { CGEventGetIntegerValueField(event, FIELD_DELTA_X) } as f64;
    let dy = unsafe { CGEventGetIntegerValueField(event, FIELD_DELTA_Y) } as f64;
    if dx == 0.0 && dy == 0.0 {
        return event;
    }

    let cfg = *CONFIG.lock();
    let target = {
        let screens = SCREENS.lock();
        wrap_target((p.x, p.y), &screens, (dx, dy), &cfg)
    };
    if let Some((tx, ty)) = target {
        unsafe {
            CGWarpMouseCursorPosition(CGPoint { x: tx, y: ty });
            // Re-link mouse↔cursor immediately: a bare warp disassociates them
            // for ~0.25 s, which freezes motion right after every wrap.
            CGAssociateMouseAndMouseCursorPosition(true);
        }
        LAST_WARP_MS.store(now, Ordering::Relaxed);
        tracing::debug!("cursor-wrap: warp ({:.0},{:.0}) -> ({tx:.0},{ty:.0})", p.x, p.y);
    }
    event
}

fn install_tap_thread() {
    std::thread::Builder::new()
        .name("ir-cursor-wrap".into())
        .spawn(|| unsafe {
            let mask = (1u64 << EVT_MOUSE_MOVED)
                | (1u64 << EVT_LEFT_MOUSE_DRAGGED)
                | (1u64 << EVT_RIGHT_MOUSE_DRAGGED)
                | (1u64 << EVT_OTHER_MOUSE_DRAGGED);
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
                    "cursor-wrap: event tap unavailable (grant Accessibility in System Settings → Privacy → Accessibility)"
                );
                RUNNING.store(false, Ordering::SeqCst);
                return;
            }
            TAP_PORT.store(tap as isize, Ordering::SeqCst);
            let src = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            RUN_LOOP.store(CFRunLoopGetCurrent() as isize, Ordering::SeqCst);
            *SCREENS.lock() = display_rects();
            tracing::info!("cursor-wrap: monitor armed ({} display(s))", SCREENS.lock().len());
            while RUNNING.load(Ordering::SeqCst) {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, false);
                // Refresh the display cache between slices so a hotplug /
                // rearrange is reflected within ~0.25 s (rare + cheap).
                let s = display_rects();
                if !s.is_empty() {
                    *SCREENS.lock() = s;
                }
            }
            RUN_LOOP.store(0, Ordering::SeqCst);
        })
        .ok();
}

/// Enable/disable the wrap monitor (idempotent).
pub fn set_active(app: &tauri::AppHandle, enabled: bool) {
    let _ = app;
    if enabled {
        if RUNNING.swap(true, Ordering::SeqCst) {
            return; // already running
        }
        if !crate::expander::accessibility_granted() {
            tracing::warn!(
                "cursor-wrap: Accessibility not granted — wrapping won't fire until it is"
            );
        }
        install_tap_thread();
    } else {
        if !RUNNING.swap(false, Ordering::SeqCst) {
            return; // already stopped
        }
        let port = TAP_PORT.swap(0, Ordering::SeqCst) as CFMachPortRef;
        if !port.is_null() {
            unsafe { CGEventTapEnable(port, false) };
        }
        let rl = RUN_LOOP.swap(0, Ordering::SeqCst);
        if rl != 0 {
            unsafe { CFRunLoopStop(rl as CFRunLoopRef) };
        }
    }
}
