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

use super::{
    Contact, GestureConfig, GestureEvent, GestureSink, GestureSource, PalmAwareRecognizer,
    RawContact, TipTapRecognizer, PALM_SIZE,
};
use parking_lot::Mutex;
use std::ffi::c_void;
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering};
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
    // (No `CFRunLoopRun` — the capture thread deliberately uses the BOUNDED
    // `CFRunLoopRunInMode` below so its exit can't depend on a `CFRunLoopStop`
    // landing in the right window. See `capture_thread`.)
    fn CFRunLoopStop(rl: CFRunLoopRef);
    /// Bounded run — returns after `seconds` even with no input source firing.
    /// This is what makes the capture thread's exit independent of a
    /// `CFRunLoopStop` landing at exactly the right moment (see `capture_thread`).
    fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source_handled: bool)
        -> i32;
    static kCFRunLoopDefaultMode: CFStringRef;
    fn CFMachPortCreateRunLoopSource(
        allocator: *mut c_void,
        port: CFMachPortRef,
        order: isize,
    ) -> CFRunLoopSourceRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFMachPortInvalidate(port: CFMachPortRef);
    fn CFRelease(cf: *const std::ffi::c_void);
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
static REC: Mutex<Option<PalmAwareRecognizer>> = Mutex::new(None);
static TIPTAP_REC: Mutex<Option<TipTapRecognizer>> = Mutex::new(None);
static START: OnceLock<Instant> = OnceLock::new();
static RUN_LOOP: AtomicIsize = AtomicIsize::new(0);
static FIRST_FRAME_LOGGED: AtomicBool = AtomicBool::new(false);
/// Time (ms since `START`) of the most recent multitouch frame; `0` = none ever.
/// Read by the liveness watchdog to notice a registration that went silent —
/// see `gestures::liveness_should_rebuild`. `START` is an `Instant`, which does
/// NOT advance while the Mac sleeps, so a wake can never look "stale" here.
static LAST_FRAME_MS: AtomicU64 = AtomicU64::new(0);
static LAST_COUNT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
/// The scroll tap swallows scroll-wheel events until this timestamp (ms since
/// `START`). Each ≥3-finger multitouch frame pushes it to `now + GRACE_MS`, so
/// it stays set for the whole 3-finger phase **and** a short grace afterwards —
/// the grace eats the lift-phase frames + any momentum scroll, which a hard
/// "armed-while-≥3" flag would leak. `0` = not swallowing.
static SWALLOW_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
static SCROLL_TAP_PORT: AtomicIsize = AtomicIsize::new(0);
/// The tap's run-loop source (+1 from Create) — kept so `stop()` can release
/// it. Losing this reference leaked a mach port + source per stop/start
/// cycle, and the wake watchdog restarts the source after EVERY sleep/wake.
static SCROLL_TAP_SOURCE: AtomicIsize = AtomicIsize::new(0);
/// Diagnostics: scroll events dropped vs let through during a gesture window.
static SCROLL_SWALLOWED: AtomicU32 = AtomicU32::new(0);
static SCROLL_PASSED: AtomicU32 = AtomicU32::new(0);
/// Finger count at/above which the scroll-consume arms (matches DEFAULT_FINGERS).
const ARM_FINGERS: usize = 3;
/// Keep swallowing this long after the last ≥3-finger frame (lift + momentum).
const GRACE_MS: u64 = 350;
/// MTDeviceRefs + the loaded API, so `stop()` (other thread) can finalise.
static MT_API: Mutex<Option<Mt>> = Mutex::new(None);
static MT_DEVICES: Mutex<Vec<isize>> = Mutex::new(Vec::new());
/// The running capture thread's join handle, so `stop()` can WAIT for it to
/// fully exit before a restart. Without this, a sleep/wake rebuild's `apply`
/// (`stop()` then `start()`) could spawn a new thread while the old run loop was
/// still draining buffered frames — the stale thread then replayed the same tap
/// through the shared recogniser ~1 s later, double-toggling mute.
static CAPTURE_THREAD: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
/// The tap-settle ticker's join handle. A light multi-finger tap arrives as
/// SEQUENTIAL single-finger touches; the recogniser DEFERS the tap and needs a
/// periodic `tick` to finalise it once the pad quiets — but frames STOP arriving
/// the instant the fingers lift, so the frame callback can't drive it. This
/// thread ticks the recogniser every `TICK_MS` and dispatches any coalesced tap.
static TICK_THREAD: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
/// Tap-settle tick cadence (ms). Must be well under `TAP_SETTLE_MS` (160) so a
/// settled cluster finalises within ~one cadence of going quiet.
const TICK_MS: u64 = 24; // was 40 — trims the average post-settle tap latency (v0.110.0)

/// Milliseconds since the last multitouch frame, or `None` when none has EVER
/// arrived (no trackpad, or never started) — the liveness watchdog must not
/// "heal" a device that never existed. `START` is an `Instant`, so this does not
/// inflate across system sleep.
pub(crate) fn ms_since_last_frame() -> Option<u64> {
    let last = LAST_FRAME_MS.load(Ordering::Relaxed);
    if last == 0 {
        return None;
    }
    let now = START.get()?.elapsed().as_millis() as u64;
    Some(now.saturating_sub(last))
}

/// Whether the capture is currently armed — the liveness watchdog must never
/// resurrect a source the user deliberately switched off.
pub(crate) fn is_running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

/// Park/wake gate for the settle ticker (A2). The frame callback notifies
/// whenever the recogniser reports work (`needs_tick`), `stop()` notifies so a
/// parked ticker can exit. The 1 s timeout is a safety net only — it costs one
/// wakeup per second instead of ~42, and guarantees a missed notify can never
/// strand a pending tap.
static TICK_GATE: (Mutex<()>, parking_lot::Condvar) = (Mutex::new(()), parking_lot::Condvar::new());

fn wake_ticker() {
    TICK_GATE.1.notify_all();
}

/// The recogniser tick loop (own thread). Finalises a deferred tap cluster once
/// the pad has been quiet past the settle window and dispatches it through the
/// sink — the piece the frame callback can't do (no frames arrive after lift).
/// PARKS while the recogniser has nothing deferred (PERFORMANCE-PLAN A2): the
/// pre-v0.166 loop slept 24 ms unconditionally, ~42 wakeups/s around the
/// clock with no finger on the pad.
fn tick_thread() {
    while RUNNING.load(Ordering::Relaxed) {
        let pending = REC.lock().as_ref().map(|r| r.needs_tick()).unwrap_or(false);
        if !pending {
            let mut guard = TICK_GATE.0.lock();
            TICK_GATE.1.wait_for(&mut guard, std::time::Duration::from_secs(1));
            continue;
        }
        std::thread::sleep(std::time::Duration::from_millis(TICK_MS));
        if !RUNNING.load(Ordering::Relaxed) {
            break;
        }
        let now = START.get().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
        let ev = {
            let mut rec = REC.lock();
            rec.as_mut().and_then(|r| r.tick(now))
        };
        if let Some(ev) = ev {
            tracing::info!(
                "gestures(mac): recognised {:?} ({} finger(s)) via settle tick",
                ev.kind, ev.fingers
            );
            if let Some(sink) = SINK.lock().as_ref() {
                sink(ev);
            }
        }
    }
}

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
    // Log the finger-count transitions (debug) so a real gesture's shape
    // (0→3→…→0) is visible when diagnosing. Only fires on a change, so ~a few
    // lines per gesture. Sizes included for palm-threshold field tuning
    // (fingertip ~0.5–1.5, palm heel larger — see PALM_SIZE).
    let now = START.get().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
    LAST_FRAME_MS.store(now.max(1), Ordering::Relaxed); // 1 = "a frame arrived"
    let prev = LAST_COUNT.swap(n as i32, Ordering::Relaxed);
    if prev != n as i32 && tracing::enabled!(tracing::Level::DEBUG) {
        let sizes = fingers
            .iter()
            .map(|f| format!("{:.2}", f.size))
            .collect::<Vec<_>>()
            .join(" ");
        tracing::debug!("gestures(mac): contacts {prev} -> {n} (sizes: {sizes})");
    }
    // Per-contact feed for the palm-aware recogniser: stable id + position
    // (y flipped so "up" = decreasing y, matching `classify_swipe`) + the
    // driver's contact `size` (palm rejection).
    //
    // Count a finger as ON THE PAD across the MAKE-TOUCH (3), TOUCHING (4) and
    // BREAK-TOUCH (5) states — not TOUCHING alone. Field logs showed a light,
    // quick 3-finger TAP arriving as three SEPARATE 1-finger touches
    // (0→1→0 × 3, ~25 ms apart): its fingers take turns in state 4 while the
    // others sit in make/break, so filtering to 4 alone SERIALISED a simultaneous
    // tap and the 3-finger count (→ mute) was almost never reached — the gesture
    // then did random things / collided with swipe. Make + break are still
    // physical contact (size > 0); only HOVER (2) and LINGER/OUT (6/7) — the true
    // leaving states — stay excluded.
    const MT_STATE_MAKE: c_int = 3;
    const MT_STATE_TOUCHING: c_int = 4;
    const MT_STATE_BREAK: c_int = 5;
    let t_ms = START.get().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
    let mut raw: [RawContact; 11] = [RawContact { id: 0, x: 0.0, y: 0.0, size: 0.0 }; 11];
    let mut rn = 0usize;
    for f in fingers.iter() {
        let on_pad = matches!(f.state, MT_STATE_MAKE | MT_STATE_TOUCHING | MT_STATE_BREAK);
        if !on_pad || rn >= raw.len() {
            continue;
        }
        raw[rn] = RawContact {
            id: f.identifier,
            x: f.normalized.pos.x as f64,
            y: 1.0 - f.normalized.pos.y as f64,
            size: f.size,
        };
        rn += 1;
    }

    let (event, prev_active, active, pending) = {
        let mut rec = REC.lock();
        let rec = rec.get_or_insert_with(PalmAwareRecognizer::new);
        let prev_active = rec.active_fingers();
        let ev = rec.feed(t_ms, &raw[..rn]);
        (ev, prev_active, rec.active_fingers(), rec.needs_tick())
    };
    if pending {
        wake_ticker(); // a deferred tap needs the settle ticker (A2)
    }

    // Scroll-consume window: while ≥3 ACTIVE fingers are down (parked palms /
    // resting thumbs excluded — a palm + 2-finger scroll must keep scrolling),
    // keep pushing the swallow deadline to now+GRACE — so it covers the whole
    // gesture plus a grace tail (lift frames + momentum). On full lift, log the
    // leak counters.
    if active >= ARM_FINGERS {
        // On the leading edge of a gesture, re-assert the tap is enabled — a
        // display reconfiguration (monitor unplug) can silently disable a
        // CGEventTap, after which all scroll leaks. Cheap, idempotent, once per
        // gesture (only when crossing into ≥3 active fingers).
        if prev_active < ARM_FINGERS {
            let tap = SCROLL_TAP_PORT.load(Ordering::Relaxed) as CFMachPortRef;
            if !tap.is_null() {
                unsafe { CGEventTapEnable(tap, true) };
            }
        }
        SWALLOW_UNTIL_MS.store(now + GRACE_MS, Ordering::Relaxed);
    } else if n == 0 && prev > 0 {
        let sw = SCROLL_SWALLOWED.swap(0, Ordering::Relaxed);
        let ps = SCROLL_PASSED.swap(0, Ordering::Relaxed);
        if sw + ps > 0 {
            tracing::debug!("gestures(mac): scroll window: swallowed={sw} passed={ps}");
        }
    }

    if let Some(ev) = event {
        // INFO (one line per recogniser emit, incl. events map_action rejects):
        // lets a mis-classified tap (e.g. a stray SwipeUp with 2 fingers) be seen
        // in the log without enabling debug. Low volume — only on a real gesture.
        tracing::info!(
            "gestures(mac): recognised {:?} ({} finger(s)) t_ms={} active {}->{}",
            ev.kind, ev.fingers, t_ms, prev_active, active
        );
        if let Some(sink) = SINK.lock().as_ref() {
            sink(ev);
        }
    }

    // Tip-tap runs on per-contact positions (the centroid can't tell which
    // finger tapped). Same y-flip + on-pad state filter as above. It needs
    // exactly 2 contacts (1 rest + 1 tap) and poisons at ≥ 3, so a 3-slot buffer
    // is enough to both see the tap and detect "too many". Size-palms are
    // skipped so a resting palm heel doesn't poison every tip-tap attempt.
    let mut contacts: [Contact; 3] = [Contact { x: 0.0, y: 0.0 }; 3];
    let mut cn = 0usize;
    for f in fingers.iter() {
        // Use the SAME on-pad state set as the palm-aware feed above
        // (MAKE|TOUCHING|BREAK), NOT TOUCHING alone. A lightly-resting finger
        // bounces between TOUCHING and MAKE/BREAK frame-to-frame; a
        // TOUCHING-only filter dropped it on those frames, so the two resting
        // fingers never stayed "settled" together long enough and the tip-tap
        // never fired (v0.85.6 widened the palm feed but forgot this one — the
        // "tab switch stopped working" regression). Only HOVER/LINGER/OUT (the
        // true leaving states) and genuine size-palms are excluded.
        let on_pad = matches!(f.state, MT_STATE_MAKE | MT_STATE_TOUCHING | MT_STATE_BREAK);
        if !on_pad || f.size >= PALM_SIZE {
            continue;
        }
        if cn >= contacts.len() {
            cn = contacts.len() + 1; // >4 real contacts → let the recogniser poison
            break;
        }
        contacts[cn] = Contact {
            x: f.normalized.pos.x as f64,
            y: 1.0 - f.normalized.pos.y as f64,
        };
        cn += 1;
    }
    let cn = cn.min(contacts.len());
    let tt_kind = {
        let mut rec = TIPTAP_REC.lock();
        rec.get_or_insert_with(TipTapRecognizer::new).feed(t_ms, &contacts[..cn])
    };
    if let Some(kind) = tt_kind {
        tracing::debug!("gestures(mac): recognised {kind:?} (tip-tap)");
        if let Some(sink) = SINK.lock().as_ref() {
            sink(GestureEvent { kind, fingers: 3 });
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
        CG_EVT_SCROLL_WHEEL => {
            let now = START.get().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0);
            if now <= SWALLOW_UNTIL_MS.load(Ordering::Relaxed) {
                SCROLL_SWALLOWED.fetch_add(1, Ordering::Relaxed);
                std::ptr::null_mut() // consume — drop the scroll
            } else {
                SCROLL_PASSED.fetch_add(1, Ordering::Relaxed);
                event
            }
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
    SCROLL_TAP_SOURCE.store(source as isize, Ordering::SeqCst);
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
    // BOUNDED run loop, deliberately (fixed 2026-08-16). The old `CFRunLoopRun()`
    // blocked until a `CFRunLoopStop` from `stop()` — but `stop()` can read
    // `RUN_LOOP` *before* it is published above, or fire `CFRunLoopStop` in the
    // window before the loop is actually running. Either way the stop is lost,
    // the loop then runs forever, and `stop()`'s unconditional `join()` hangs —
    // on the main thread, because `set_gesture_config` reached it. Waking every
    // `RL_SLICE_S` to re-check `RUNNING` removes the dependency on that signal
    // entirely: a missed stop now costs at most one slice instead of the app.
    const RL_SLICE_S: f64 = 0.25;
    while RUNNING.load(Ordering::SeqCst) {
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, RL_SLICE_S, false) };
    }
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
        // Install the fresh sink FIRST — the old early return dropped it when
        // RUNNING was still true, leaving a live capture that recognised
        // gestures but dispatched nothing (the frame callback lazily revives
        // the recognisers after a stop, so a stop/start race could keep the
        // capture alive sink-less).
        *SINK.lock() = Some(sink);
        if RUNNING.swap(true, Ordering::SeqCst) {
            return Ok(()); // already capturing — the sink above was swapped in place
        }
        let _ = START.set(Instant::now());
        FIRST_FRAME_LOGGED.store(false, Ordering::SeqCst);
        *REC.lock() = Some(PalmAwareRecognizer::new());
        *TIPTAP_REC.lock() = Some(TipTapRecognizer::new());
        // The run loop must live on its own thread (a sync IPC command thread
        // returns immediately → no run loop → no callbacks).
        let handle = std::thread::Builder::new()
            .name("ir-gestures-mac".into())
            .spawn(capture_thread)
            .map_err(|e| format!("spawn gesture thread: {e}"))?;
        *CAPTURE_THREAD.lock() = Some(handle);
        // The settle ticker finalises deferred taps (sequential single-touch
        // clusters) — frames stop on lift, so this drives the coalescing.
        let tick_handle = std::thread::Builder::new()
            .name("ir-gestures-tick".into())
            .spawn(tick_thread)
            .map_err(|e| format!("spawn gesture tick thread: {e}"))?;
        *TICK_THREAD.lock() = Some(tick_handle);
        Ok(())
    }

    fn stop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
        wake_ticker(); // a parked settle ticker must see RUNNING=false and exit
        SWALLOW_UNTIL_MS.store(0, Ordering::SeqCst);
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
        // Wait for the capture thread to fully exit (CFRunLoopRun returns right
        // after CFRunLoopStop), so it can't keep delivering/replaying frames
        // after a restart. Joined on the caller thread (apply/watchdog), which
        // never holds the capture thread's locks → no deadlock.
        if let Some(h) = CAPTURE_THREAD.lock().take() {
            let _ = h.join();
        }
        // Join the settle ticker too (it polls RUNNING every TICK_MS), so a
        // restart can't leave a stale ticker dispatching through the shared
        // recogniser.
        if let Some(h) = TICK_THREAD.lock().take() {
            let _ = h.join();
        }
        // Tear the tap down for real (fixed 2026-08-16): disabling is not
        // releasing. The +1 mach port and its run-loop source leaked on every
        // stop — and `apply()` restarts the source per settings change while
        // the wake watchdog restarts it after every sleep/wake, so a laptop
        // accumulated dead ports and WindowServer tap registrations daily.
        // Released only AFTER the joins above: the callback may still fire
        // between Enable(false) and thread exit, and it dereferences the port.
        let source = SCROLL_TAP_SOURCE.swap(0, Ordering::SeqCst) as CFRunLoopSourceRef;
        if !source.is_null() {
            unsafe { CFRelease(source as *const std::ffi::c_void) };
        }
        if !tap.is_null() {
            unsafe {
                CFMachPortInvalidate(tap);
                CFRelease(tap as *const std::ffi::c_void);
            }
        }
        MT_DEVICES.lock().clear();
        *MT_API.lock() = None;
        *SINK.lock() = None;
        *REC.lock() = None;
    }
}
