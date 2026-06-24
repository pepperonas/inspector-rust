//! macOS touchpad gesture capture via the **private** `MultitouchSupport`
//! framework — the same mechanism BetterTouchTool / MiddleClick / Jitouch use.
//! It's the only way to get a global raw-finger stream (count + normalized
//! positions) without a focused window, which the public NSEvent API can't do
//! (the system consumes 3-finger swipes and there's no global 3-finger tap).
//!
//! **Loaded at runtime via `dlopen`**, not hard-linked: if the private framework
//! is missing or its symbols moved, `start()` fails and the feature degrades
//! gracefully (gestures just don't fire) instead of breaking app launch.
//!
//! **Run loop:** `MTDeviceStart(dev, 0)` delivers the contact callback via the
//! *calling thread's* CFRunLoop (mode `0` = default, not `MTRunModeNoRunLoop`).
//! So we start the devices on a dedicated thread and run `CFRunLoopRun()` there
//! to keep callbacks flowing — calling `MTDeviceStart` on a thread that then
//! returns (no live run loop) is why an earlier build never fired (v0.84.115).
//!
//! **PRIVATE-API CAVEAT:** the `Finger` struct layout + symbol names are
//! reverse-engineered (the long-standing community `mt.c` layout). A future
//! macOS could change them; the code is fully guarded so it never crashes — at
//! worst gestures stop until the layout is updated.

use super::{GestureConfig, GestureEvent, GestureSink, GestureSource, Recognizer, TouchFrame};
use parking_lot::Mutex;
use std::ffi::c_void;
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

// ── Private MultitouchSupport FFI ────────────────────────────────────────────

type MTDeviceRef = *mut c_void;
type CFArrayRef = *const c_void;
type CFRunLoopRef = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct MTPoint {
    x: f32,
    y: f32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct MTVector {
    pos: MTPoint,
    vel: MTPoint,
}

/// One touch contact. Long-standing community layout (`mt.c`); `#[repr(C)]`
/// inserts the 4-byte pad after `frame` before the 8-aligned `timestamp`.
#[repr(C)]
#[derive(Clone, Copy)]
struct Finger {
    frame: c_int,
    timestamp: f64,
    identifier: c_int,
    state: c_int,
    foo3: c_int,
    foo4: c_int,
    normalized: MTVector,
    size: f32,
    zero1: c_int,
    angle: f32,
    major_axis: f32,
    minor_axis: f32,
    mm: MTVector,
    zero2: [c_int; 2],
    z_density: f32,
}

type MTContactCallback =
    extern "C" fn(MTDeviceRef, *mut Finger, c_int, f64, c_int) -> c_int;

// dlopen / dlsym live in libSystem (always linked on macOS).
extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}
const RTLD_LAZY: c_int = 1;

// CoreFoundation is public + always linked; used to iterate the device list and
// run / stop the capture thread's run loop + add the scroll-tap source.
type CFMachPortRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFStringRef = *const c_void;
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFArrayGetCount(arr: CFArrayRef) -> isize;
    fn CFArrayGetValueAtIndex(arr: CFArrayRef, idx: isize) -> *const c_void;
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopRun();
    fn CFRunLoopStop(rl: CFRunLoopRef);
    fn CFMachPortCreateRunLoopSource(
        allocator: *mut c_void,
        port: CFMachPortRef,
        order: isize,
    ) -> CFRunLoopSourceRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    static kCFRunLoopCommonModes: CFStringRef;
}

// CGEventTap (active) to *consume* the trackpad scroll a 3-finger swipe would
// otherwise cause in the app underneath. Raw FFI, same pattern as `input_lock`.
type CGEventRef = *mut c_void;
type CGEventTapProxy = *mut c_void;
type CGEventTapCallBack =
    extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;
const CG_SESSION_EVENT_TAP: u32 = 1;
const CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
const CG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;
const CG_EVT_SCROLL_WHEEL: u32 = 22;
const CG_EVT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const CG_EVT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
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
}

/// Resolved MultitouchSupport entry points.
#[derive(Clone, Copy)]
struct Mt {
    create_list: unsafe extern "C" fn() -> CFArrayRef,
    create_default: Option<unsafe extern "C" fn() -> MTDeviceRef>,
    register_cb: unsafe extern "C" fn(MTDeviceRef, MTContactCallback),
    start: unsafe extern "C" fn(MTDeviceRef, c_int) -> c_int,
    stop: unsafe extern "C" fn(MTDeviceRef) -> c_int,
}

unsafe fn load_mt() -> Option<Mt> {
    let path = b"/System/Library/PrivateFrameworks/MultitouchSupport.framework/MultitouchSupport\0";
    let handle = dlopen(path.as_ptr() as *const c_char, RTLD_LAZY);
    if handle.is_null() {
        return None;
    }
    let sym = |name: &[u8]| dlsym(handle, name.as_ptr() as *const c_char);
    let create_list = sym(b"MTDeviceCreateList\0");
    let create_default = sym(b"MTDeviceCreateDefault\0");
    let register_cb = sym(b"MTRegisterContactFrameCallback\0");
    let start = sym(b"MTDeviceStart\0");
    let stop = sym(b"MTDeviceStop\0");
    if create_list.is_null() || register_cb.is_null() || start.is_null() || stop.is_null() {
        return None;
    }
    Some(Mt {
        create_list: std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> CFArrayRef>(create_list),
        create_default: if create_default.is_null() {
            None
        } else {
            Some(std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> MTDeviceRef>(create_default))
        },
        register_cb: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(MTDeviceRef, MTContactCallback)>(register_cb),
        start: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(MTDeviceRef, c_int) -> c_int>(start),
        stop: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(MTDeviceRef) -> c_int>(stop),
    })
}

// ── Shared state the C callback / stop() read (the callback can't capture) ───

static RUNNING: AtomicBool = AtomicBool::new(false);
static SINK: Mutex<Option<GestureSink>> = Mutex::new(None);
static REC: Mutex<Option<Recognizer>> = Mutex::new(None);
static START: OnceLock<Instant> = OnceLock::new();
static RUN_LOOP: AtomicIsize = AtomicIsize::new(0);
static FIRST_FRAME_LOGGED: AtomicBool = AtomicBool::new(false);
static LAST_COUNT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
/// True while a 3-finger gesture is in progress (≥3 fingers seen, not yet fully
/// lifted) — the scroll tap swallows scroll-wheel events while it's set so the
/// app underneath doesn't also scroll. Set/cleared by the multitouch callback.
static GESTURE_ARMED: AtomicBool = AtomicBool::new(false);
static SCROLL_TAP_PORT: AtomicIsize = AtomicIsize::new(0);
/// Finger count at/above which the scroll-consume arms (matches DEFAULT_FINGERS).
const ARM_FINGERS: usize = 3;
/// MTDeviceRefs + the loaded API, so `stop()` (other thread) can finalise.
static MT_API: Mutex<Option<Mt>> = Mutex::new(None);
static MT_DEVICES: Mutex<Vec<isize>> = Mutex::new(Vec::new());

extern "C" fn frame_callback(
    _device: MTDeviceRef,
    data: *mut Finger,
    n_fingers: c_int,
    _timestamp: f64,
    _frame: c_int,
) -> c_int {
    if !RUNNING.load(Ordering::Relaxed) {
        return 0;
    }
    let n = n_fingers.max(0) as usize;
    let fingers: &[Finger] = if n == 0 || data.is_null() {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(data, n) }
    };
    if !FIRST_FRAME_LOGGED.swap(true, Ordering::Relaxed) {
        tracing::info!("gestures(mac): first multitouch frame received ({n} finger(s)) — capture is live");
    }
    // DIAGNOSTIC: log the finger-count transitions so a real 3-finger swipe's
    // shape (0→3→…→0) is visible in the log. Only fires on a change, so ~a few
    // lines per gesture.
    let prev = LAST_COUNT.swap(n as i32, Ordering::Relaxed);
    if prev != n as i32 {
        tracing::debug!("gestures(mac): contacts {prev} -> {n}");
    }
    // Arm the scroll-consume once 3 fingers are down; keep it armed through the
    // lift (3→2→1→0) so the trailing 1-/2-finger frames don't scroll either.
    // Disarm only on full lift.
    if n >= ARM_FINGERS {
        GESTURE_ARMED.store(true, Ordering::Relaxed);
    } else if n == 0 {
        GESTURE_ARMED.store(false, Ordering::Relaxed);
    }
    // Centroid of the active contacts; flip y so "up" = decreasing y (screen
    // convention), matching `classify_swipe` (dy < 0 = up).
    let (mut sx, mut sy) = (0.0f64, 0.0f64);
    for f in fingers {
        sx += f.normalized.pos.x as f64;
        sy += 1.0 - f.normalized.pos.y as f64;
    }
    let count = fingers.len();
    let (x, y) = if count > 0 {
        (sx / count as f64, sy / count as f64)
    } else {
        (0.0, 0.0)
    };
    let t_ms = START.get().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
    let frame = TouchFrame { contacts: count as u8, x, y, t_ms };

    let event: Option<GestureEvent> = {
        let mut rec = REC.lock();
        rec.get_or_insert_with(Recognizer::new).feed(frame)
    };
    if let Some(ev) = event {
        tracing::debug!("gestures(mac): recognised {:?} ({} finger(s))", ev.kind, ev.fingers);
        if let Some(sink) = SINK.lock().as_ref() {
            sink(ev);
        }
    }
    0
}

/// Active CGEventTap callback: swallow scroll-wheel events while a 3-finger
/// gesture is armed, so the swipe that changes volume doesn't also scroll the
/// app underneath. Everything else passes through unchanged.
extern "C" fn scroll_tap_callback(
    _proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    _info: *mut c_void,
) -> CGEventRef {
    match event_type {
        // The OS disables the tap on timeout / heavy input — re-enable it.
        CG_EVT_TAP_DISABLED_BY_TIMEOUT | CG_EVT_TAP_DISABLED_BY_USER_INPUT => {
            let port = SCROLL_TAP_PORT.load(Ordering::SeqCst) as CFMachPortRef;
            if !port.is_null() {
                unsafe { CGEventTapEnable(port, true) };
            }
            event
        }
        CG_EVT_SCROLL_WHEEL if GESTURE_ARMED.load(Ordering::Relaxed) => {
            std::ptr::null_mut() // consume — drop the scroll
        }
        _ => event,
    }
}

/// Install the scroll-consume tap on the current run loop. Returns whether it
/// was installed (false = no Accessibility grant → gestures still work, but the
/// swipe won't be consumed). The tap's run-loop source also keeps this thread's
/// run loop alive.
unsafe fn install_scroll_tap() -> bool {
    let mask = 1u64 << CG_EVT_SCROLL_WHEEL;
    let tap = CGEventTapCreate(
        CG_SESSION_EVENT_TAP,
        CG_HEAD_INSERT_EVENT_TAP,
        CG_EVENT_TAP_OPTION_DEFAULT,
        mask,
        scroll_tap_callback,
        std::ptr::null_mut(),
    );
    if tap.is_null() {
        tracing::warn!(
            "gestures(mac): scroll-consume tap unavailable (grant Accessibility to stop the \
             underlying scroll); volume still changes"
        );
        return false;
    }
    SCROLL_TAP_PORT.store(tap as isize, Ordering::SeqCst);
    let source = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
    CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
    CGEventTapEnable(tap, true);
    tracing::info!("gestures(mac): scroll-consume tap installed");
    true
}

/// Runs on a dedicated thread: start the devices, then pump a CFRunLoop so the
/// contact callback is delivered. Blocks until `stop()` calls `CFRunLoopStop`.
fn capture_thread() {
    let Some(mt) = (unsafe { load_mt() }) else {
        tracing::warn!("gestures(mac): MultitouchSupport not loadable — gestures disabled");
        RUNNING.store(false, Ordering::SeqCst);
        return;
    };

    // Enumerate devices (fall back to the default device if the list is empty).
    let mut devices: Vec<MTDeviceRef> = Vec::new();
    unsafe {
        let list = (mt.create_list)();
        if !list.is_null() {
            let count = CFArrayGetCount(list);
            for i in 0..count {
                let dev = CFArrayGetValueAtIndex(list, i) as MTDeviceRef;
                if !dev.is_null() {
                    devices.push(dev);
                }
            }
        }
        if devices.is_empty() {
            if let Some(create_default) = mt.create_default {
                let dev = create_default();
                if !dev.is_null() {
                    devices.push(dev);
                }
            }
        }
    }
    if devices.is_empty() {
        tracing::warn!("gestures(mac): no multitouch devices found");
        RUNNING.store(false, Ordering::SeqCst);
        return;
    }
    tracing::info!("gestures(mac): {} multitouch device(s), starting capture", devices.len());

    unsafe {
        for &dev in &devices {
            (mt.register_cb)(dev, frame_callback);
            (mt.start)(dev, 0);
        }
        RUN_LOOP.store(CFRunLoopGetCurrent() as isize, Ordering::SeqCst);
        // Active tap to consume the scroll a 3-finger swipe would otherwise
        // cause underneath. Also keeps this thread's run loop alive (MT itself
        // delivers on its own internal thread, so without a source the loop
        // would exit immediately — harmless, but the tap needs it running).
        install_scroll_tap();
    }
    *MT_API.lock() = Some(mt);
    *MT_DEVICES.lock() = devices.iter().map(|&d| d as isize).collect();

    tracing::info!("gestures(mac): entering run loop (waiting for finger frames)");
    // Blocks here delivering callbacks until CFRunLoopStop (from stop()).
    unsafe { CFRunLoopRun() };
    tracing::info!("gestures(mac): run loop exited");
    RUN_LOOP.store(0, Ordering::SeqCst);
}

// ── Source ───────────────────────────────────────────────────────────────────

pub struct MacGestureSource;

impl MacGestureSource {
    pub fn new() -> Self {
        MacGestureSource
    }
}

impl GestureSource for MacGestureSource {
    fn start(&mut self, _cfg: GestureConfig, sink: GestureSink) -> Result<(), String> {
        if RUNNING.swap(true, Ordering::SeqCst) {
            return Ok(()); // already running
        }
        let _ = START.set(Instant::now());
        FIRST_FRAME_LOGGED.store(false, Ordering::SeqCst);
        *REC.lock() = Some(Recognizer::new());
        *SINK.lock() = Some(sink);
        // The run loop must live on its own thread (a sync IPC command thread
        // returns immediately → no run loop → no callbacks).
        std::thread::Builder::new()
            .name("ir-gestures-mac".into())
            .spawn(capture_thread)
            .map_err(|e| format!("spawn gesture thread: {e}"))?;
        Ok(())
    }

    fn stop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
        GESTURE_ARMED.store(false, Ordering::SeqCst);
        let tap = SCROLL_TAP_PORT.swap(0, Ordering::SeqCst) as CFMachPortRef;
        if !tap.is_null() {
            unsafe { CGEventTapEnable(tap, false) };
        }
        if let Some(mt) = *MT_API.lock() {
            for &dev in MT_DEVICES.lock().iter() {
                unsafe { (mt.stop)(dev as MTDeviceRef) };
            }
        }
        let rl = RUN_LOOP.swap(0, Ordering::SeqCst);
        if rl != 0 {
            unsafe { CFRunLoopStop(rl as CFRunLoopRef) };
        }
        MT_DEVICES.lock().clear();
        *MT_API.lock() = None;
        *SINK.lock() = None;
        *REC.lock() = None;
    }
}
