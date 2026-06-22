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
//! **PRIVATE-API CAVEAT:** the `Finger` struct layout + symbol names are
//! reverse-engineered and version-sensitive (this is the long-standing
//! community layout used by the tools above). A future macOS could change them;
//! the code is fully guarded so it can never crash — at worst gestures stop
//! working until the layout is updated.

use super::{GestureConfig, GestureEvent, GestureSink, GestureSource, Recognizer, TouchFrame};
use parking_lot::Mutex;
use std::ffi::c_void;
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

// ── Private MultitouchSupport FFI ────────────────────────────────────────────

type MTDeviceRef = *mut c_void;
type CFArrayRef = *const c_void;

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

// dlopen / dlsym live in libSystem (always linked on macOS) — declare directly
// to avoid pulling in the `libc` crate.
extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}
const RTLD_LAZY: c_int = 1;

// CoreFoundation is public + always linked; used to iterate the device list.
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFArrayGetCount(arr: CFArrayRef) -> isize;
    fn CFArrayGetValueAtIndex(arr: CFArrayRef, idx: isize) -> *const c_void;
}

/// Resolved MultitouchSupport entry points.
#[derive(Clone, Copy)]
struct Mt {
    create_list: unsafe extern "C" fn() -> CFArrayRef,
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
    let register_cb = sym(b"MTRegisterContactFrameCallback\0");
    let start = sym(b"MTDeviceStart\0");
    let stop = sym(b"MTDeviceStop\0");
    if create_list.is_null() || register_cb.is_null() || start.is_null() || stop.is_null() {
        return None;
    }
    Some(Mt {
        create_list: std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> CFArrayRef>(create_list),
        register_cb: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(MTDeviceRef, MTContactCallback)>(register_cb),
        start: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(MTDeviceRef, c_int) -> c_int>(start),
        stop: std::mem::transmute::<*mut c_void, unsafe extern "C" fn(MTDeviceRef) -> c_int>(stop),
    })
}

// ── Shared state the C callback reads (it can't capture) ─────────────────────

static RUNNING: AtomicBool = AtomicBool::new(false);
static MT_SINK: Mutex<Option<GestureSink>> = Mutex::new(None);
static MT_REC: Mutex<Option<Recognizer>> = Mutex::new(None);
static START: OnceLock<Instant> = OnceLock::new();

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
        let mut rec = MT_REC.lock();
        rec.get_or_insert_with(Recognizer::new).feed(frame)
    };
    if let Some(ev) = event {
        if let Some(sink) = MT_SINK.lock().as_ref() {
            sink(ev);
        }
    }
    0
}

// ── Source ───────────────────────────────────────────────────────────────────

pub struct MacGestureSource {
    mt: Option<Mt>,
    devices: Vec<MTDeviceRef>,
}

impl MacGestureSource {
    pub fn new() -> Self {
        MacGestureSource { mt: None, devices: Vec::new() }
    }
}

// MTDeviceRef is an opaque pointer we only hand back to the framework on the
// same thread set; the framework owns the worker thread. Safe to move the
// handle list across threads for start/stop.
unsafe impl Send for MacGestureSource {}

impl GestureSource for MacGestureSource {
    fn start(&mut self, _cfg: GestureConfig, sink: GestureSink) -> Result<(), String> {
        let mt = unsafe { load_mt() }.ok_or_else(|| {
            "MultitouchSupport unavailable (private framework not loadable)".to_string()
        })?;
        let _ = START.set(Instant::now());
        *MT_REC.lock() = Some(Recognizer::new());
        *MT_SINK.lock() = Some(sink);
        RUNNING.store(true, Ordering::SeqCst);

        let list = unsafe { (mt.create_list)() };
        if list.is_null() {
            RUNNING.store(false, Ordering::SeqCst);
            return Err("no multitouch devices".into());
        }
        let count = unsafe { CFArrayGetCount(list) };
        let mut devices = Vec::new();
        for i in 0..count {
            let dev = unsafe { CFArrayGetValueAtIndex(list, i) } as MTDeviceRef;
            if dev.is_null() {
                continue;
            }
            unsafe {
                (mt.register_cb)(dev, frame_callback);
                (mt.start)(dev, 0);
            }
            devices.push(dev);
        }
        if devices.is_empty() {
            RUNNING.store(false, Ordering::SeqCst);
            return Err("no startable multitouch devices".into());
        }
        self.mt = Some(mt);
        self.devices = devices;
        Ok(())
    }

    fn stop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
        if let Some(mt) = self.mt {
            for &dev in &self.devices {
                unsafe { (mt.stop)(dev) };
            }
        }
        self.devices.clear();
        self.mt = None;
        *MT_SINK.lock() = None;
        *MT_REC.lock() = None;
    }
}
