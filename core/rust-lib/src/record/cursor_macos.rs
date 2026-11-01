//! macOS cursor telemetry for `record`: position, clicks, cursor type and the
//! cursor image, written as OpenScreen-compatible sidecar data
//! ([`CursorRecording`]).
//!
//! One thread (`ir-record-cursor`) does everything:
//! * every 33 ms it reads the global cursor point (`CGEventGetLocation` of a
//!   fresh event, top-left points — same space as `bounds`), the current
//!   system cursor image and (with Accessibility) the AX cursor type;
//! * between ticks it pumps its own `CFRunLoop`, which hosts a **listen-only**
//!   `CGEventTap` for mouse down/up. Clicks are queued with their own time and
//!   position and each becomes its own sample, so a click between two ticks is
//!   never lost.
//!
//! Without Accessibility there is no tap and no AX lookup: the recording is
//! `provider = position-only` (positions + images, no clicks/types). NSCursor
//! needs no permission.
//!
//! The ordering/cap rules are the pure [`SampleSink`] (unit-tested below).

use super::core::{cursor_type_for_roles, normalize_point, CursorType, CursorVisibility};
use super::PauseShared;
use super::host_now_ns;
use super::project::{CursorAsset, CursorProvider, CursorRecording, CursorSample, Interaction};
use base64::Engine as _;
use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Sample period (OpenScreen's default `sampleIntervalMs`).
pub const TICK_MS: u64 = 33;
/// Hard cap on samples (~1 h at 30 Hz).
pub const MAX_SAMPLES: usize = 108_000;
/// AX messaging timeout per element, seconds.
const AX_TIMEOUT_S: f32 = 0.05;
/// An AX lookup slower than this pauses AX lookups for [`AX_BACKOFF`].
const AX_SLOW: Duration = Duration::from_millis(40);
const AX_BACKOFF: Duration = Duration::from_secs(1);

// ── pure core ────────────────────────────────────────────────────────────────

/// Appends samples with strictly increasing `time_ms` and the sample cap.
///
/// * A tick (`move`) sample whose time is not after the previous one is
///   dropped.
/// * An interaction (click/mouseup) is never dropped for ordering: if its
///   time is not after the previous sample it is placed 1 ms after it (the
///   tap queue can deliver an event a few ms "late" relative to a tick).
/// * Past [`MAX_SAMPLES`] nothing more is added; `capped` reports it once.
#[derive(Debug, Default)]
pub struct SampleSink {
    pub samples: Vec<CursorSample>,
    pub capped: bool,
}

impl SampleSink {
    /// Returns false if the sample was not added.
    pub fn push(&mut self, mut s: CursorSample) -> bool {
        if self.samples.len() >= MAX_SAMPLES {
            if !self.capped {
                self.capped = true;
                tracing::warn!("record cursor: sample cap {MAX_SAMPLES} reached, dropping further samples");
            }
            return false;
        }
        if let Some(last) = self.samples.last() {
            if s.time_ms <= last.time_ms {
                let is_move = matches!(s.interaction_type, None | Some(Interaction::Move));
                if is_move {
                    return false;
                }
                s.time_ms = last.time_ms + 1;
            }
        }
        self.samples.push(s);
        true
    }
}

/// Map a tap event (type, click count) to an interaction. `None` = ignore.
pub fn interaction_for_event(event_type: u32, click_count: i64) -> Option<Interaction> {
    match event_type {
        EVT_LEFT_DOWN => Some(if click_count >= 2 {
            Interaction::DoubleClick
        } else {
            Interaction::Click
        }),
        EVT_LEFT_UP => Some(Interaction::Mouseup),
        EVT_RIGHT_DOWN => Some(Interaction::RightClick),
        EVT_OTHER_DOWN => Some(Interaction::MiddleClick),
        _ => None,
    }
}

/// Video-time ms for a host time, or `None` before video time 0.
pub fn video_ms(timeline_ns: i64, origin_ns: i64) -> Option<u64> {
    let d = timeline_ns - origin_ns;
    (d >= 0).then_some((d / 1_000_000) as u64)
}

/// Lowercase hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in d {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

// ── FFI ──────────────────────────────────────────────────────────────────────

type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFRunLoopRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFMachPortRef = *mut c_void;
type CGEventRef = *mut c_void;
type CGEventTapProxy = *mut c_void;
type AXUIElementRef = *const c_void;

const KCF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const EVT_LEFT_DOWN: u32 = 1;
const EVT_LEFT_UP: u32 = 2;
const EVT_RIGHT_DOWN: u32 = 3;
const EVT_OTHER_DOWN: u32 = 25;
const EVT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const EVT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
const CG_SESSION_TAP: u32 = 1;
const CG_HEAD_INSERT: u32 = 0;
const CG_TAP_LISTEN_ONLY: u32 = 1;
const CG_MOUSE_CLICK_STATE: u32 = 1;
const NS_BITMAP_PNG: usize = 4;
const CG_WINDOW_LIST_ON_SCREEN_ONLY: u32 = 1;
const CG_WINDOW_LIST_EXCLUDE_DESKTOP: u32 = 16;

type CGEventTapCallBack = extern "C" fn(CGEventTapProxy, u32, CGEventRef, *mut c_void) -> CGEventRef;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGSize {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}
// SAFETY: layouts match CoreGraphics' CGPoint/CGSize/CGRect.
unsafe impl Encode for CGPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[Encoding::Double, Encoding::Double]);
}
unsafe impl Encode for CGSize {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[Encoding::Double, Encoding::Double]);
}
unsafe impl Encode for CGRect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[CGPoint::ENCODING, CGSize::ENCODING]);
}
unsafe impl RefEncode for CGRect {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
}

/// Opaque `CGImage` for typed `msg_send` arguments.
#[repr(C)]
struct CGImageOpaque {
    _p: [u8; 0],
}
// SAFETY: only ever used behind a pointer (`CGImageRef`).
unsafe impl RefEncode for CGImageOpaque {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGImage", &[]));
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopDefaultMode: CFStringRef;
    fn CFStringCreateWithCString(a: *const c_void, s: *const c_char, enc: u32) -> CFStringRef;
    fn CFRelease(cf: CFTypeRef);
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source_handled: bool) -> i32;
    fn CFMachPortCreateRunLoopSource(a: *mut c_void, port: CFMachPortRef, order: isize) -> CFRunLoopSourceRef;
    fn CFMachPortInvalidate(port: CFMachPortRef);
    fn CFRunLoopAddSource(rl: CFRunLoopRef, src: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRemoveSource(rl: CFRunLoopRef, src: CFRunLoopSourceRef, mode: CFStringRef);
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
    fn CGEventCreate(source: *mut c_void) -> *mut c_void;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;

    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFTypeRef;
    fn AXUIElementCopyElementAtPosition(app: AXUIElementRef, x: f32, y: f32, out: *mut CFTypeRef) -> i32;
    fn AXUIElementCopyAttributeValue(el: AXUIElementRef, attr: CFStringRef, out: *mut CFTypeRef) -> i32;
    fn AXUIElementSetMessagingTimeout(el: AXUIElementRef, timeout: f32) -> i32;
}

/// Owned CFString, released on drop.
struct CfStr(CFStringRef);
impl CfStr {
    fn new(s: &str) -> Self {
        let c = CString::new(s).expect("static attribute name");
        // SAFETY: valid NUL-terminated UTF-8; +1 result released in Drop.
        Self(unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), KCF_STRING_ENCODING_UTF8) })
    }
}
impl Drop for CfStr {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: we own the +1 reference.
            unsafe { CFRelease(self.0) }
        }
    }
}

fn global_cursor_point() -> Option<(f64, f64)> {
    // SAFETY: CGEventCreate(NULL) returns a +1 event (or NULL) we release.
    unsafe {
        let ev = CGEventCreate(std::ptr::null_mut());
        if ev.is_null() {
            return None;
        }
        let p = CGEventGetLocation(ev);
        CFRelease(ev as CFTypeRef);
        Some((p.x, p.y))
    }
}

// ── AX cursor type ───────────────────────────────────────────────────────────

struct AxAttrs {
    role: CfStr,
    subrole: CfStr,
    role_desc: CfStr,
    parent: CfStr,
}

/// String attribute of an AX element (CFString is toll-free bridged to
/// NSString, so it is read via `UTF8String` — no CFString FFI needed).
unsafe fn ax_string(el: AXUIElementRef, attr: &CfStr) -> Option<String> {
    let mut v: CFTypeRef = std::ptr::null();
    if AXUIElementCopyAttributeValue(el, attr.0, &mut v) != 0 || v.is_null() {
        return None;
    }
    let obj = v as *mut AnyObject;
    let is_str: bool = msg_send![obj, isKindOfClass: class!(NSString)];
    let out = if is_str {
        let p: *const c_char = msg_send![obj, UTF8String];
        (!p.is_null()).then(|| CStr::from_ptr(p).to_string_lossy().into_owned())
    } else {
        None
    };
    CFRelease(v);
    out
}

/// `NSString` for a dictionary key (autoreleased).
unsafe fn ns_str(s: &CStr) -> *mut AnyObject {
    msg_send![class!(NSString), stringWithUTF8String: s.as_ptr()]
}

unsafe fn ns_f64(dict: *mut AnyObject, key: *mut AnyObject) -> Option<f64> {
    let v: *mut AnyObject = msg_send![dict, objectForKey: key];
    if v.is_null() {
        return None;
    }
    let is_num: bool = msg_send![v, isKindOfClass: class!(NSNumber)];
    is_num.then(|| msg_send![v, doubleValue])
}

/// Pid owning the frontmost on-screen window that contains `pt` (global
/// top-left points), skipping our own process (in-process AX would hop into
/// AppKit off the main thread). Falls back to the frontmost application.
/// Must run inside an autorelease pool.
unsafe fn pid_at_point(pt: (f64, f64)) -> Option<i32> {
    let own = std::process::id() as i32;
    let list = CGWindowListCopyWindowInfo(
        CG_WINDOW_LIST_ON_SCREEN_ONLY | CG_WINDOW_LIST_EXCLUDE_DESKTOP,
        0,
    );
    let mut found = None;
    if !list.is_null() {
        let arr = list as *mut AnyObject; // CFArray, toll-free bridged
        let k_bounds = ns_str(c"kCGWindowBounds");
        let k_pid = ns_str(c"kCGWindowOwnerPID");
        let k_alpha = ns_str(c"kCGWindowAlpha");
        let (k_x, k_y, k_w, k_h) = (ns_str(c"X"), ns_str(c"Y"), ns_str(c"Width"), ns_str(c"Height"));
        let n: usize = msg_send![arr, count];
        for i in 0..n {
            let d: *mut AnyObject = msg_send![arr, objectAtIndex: i];
            let Some(pid) = ns_f64(d, k_pid).map(|p| p as i32) else { continue };
            if pid == own || ns_f64(d, k_alpha).is_some_and(|a| a <= 0.0) {
                continue;
            }
            let b: *mut AnyObject = msg_send![d, objectForKey: k_bounds];
            if b.is_null() {
                continue;
            }
            let (Some(x), Some(y), Some(w), Some(h)) = (ns_f64(b, k_x), ns_f64(b, k_y), ns_f64(b, k_w), ns_f64(b, k_h))
            else {
                continue;
            };
            if pt.0 >= x && pt.0 < x + w && pt.1 >= y && pt.1 < y + h {
                found = Some(pid);
                break;
            }
        }
        CFRelease(list);
    }
    found.or_else(|| {
        let ws: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        let app: *mut AnyObject = msg_send![ws, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let pid: i32 = msg_send![app, processIdentifier];
        (pid > 0 && pid != own).then_some(pid)
    })
}

/// Per-app AX element cache: the messaging timeout is set on the APP element
/// (never on the system-wide one, which would change the process-global
/// timeout other threads rely on).
#[derive(Default)]
struct AppElCache {
    pid: i32,
    el: Option<AXUIElementRef>,
}

impl AppElCache {
    unsafe fn get(&mut self, pid: i32) -> Option<AXUIElementRef> {
        if self.pid != pid || self.el.is_none() {
            self.clear();
            let el = AXUIElementCreateApplication(pid);
            if el.is_null() {
                return None;
            }
            AXUIElementSetMessagingTimeout(el, AX_TIMEOUT_S);
            self.pid = pid;
            self.el = Some(el);
        }
        self.el
    }
    fn clear(&mut self) {
        if let Some(el) = self.el.take() {
            // SAFETY: we own the +1 element from AXUIElementCreateApplication.
            unsafe { CFRelease(el) };
        }
    }
}

impl Drop for AppElCache {
    fn drop(&mut self) {
        self.clear();
    }
}

/// Hit-test the element under `pt` inside the app owning that point and
/// classify the innermost ≤ 5 ancestors. Every element queried carries the
/// 0.05 s per-element timeout. Must run inside an autorelease pool.
unsafe fn ax_cursor_type(pt: (f64, f64), a: &AxAttrs, cache: &mut AppElCache) -> Option<CursorType> {
    let pid = pid_at_point(pt)?;
    let app = cache.get(pid)?;
    let mut hit: CFTypeRef = std::ptr::null();
    let e = AXUIElementCopyElementAtPosition(app, pt.0 as f32, pt.1 as f32, &mut hit);
    if e != 0 || hit.is_null() {
        return None;
    }
    let mut roles = Vec::with_capacity(5);
    let mut cur = hit as AXUIElementRef; // owned (+1)
    for i in 0..5 {
        AXUIElementSetMessagingTimeout(cur, AX_TIMEOUT_S);
        roles.push((
            ax_string(cur, &a.role),
            ax_string(cur, &a.subrole),
            ax_string(cur, &a.role_desc).map(|d| d.to_lowercase()),
        ));
        if i == 4 {
            break;
        }
        let mut parent: CFTypeRef = std::ptr::null();
        let ok = AXUIElementCopyAttributeValue(cur, a.parent.0, &mut parent) == 0 && !parent.is_null();
        CFRelease(cur);
        if !ok {
            cur = std::ptr::null();
            break;
        }
        cur = parent as AXUIElementRef;
    }
    if !cur.is_null() {
        CFRelease(cur);
    }
    cursor_type_for_roles(&roles)
}

// ── cursor image ─────────────────────────────────────────────────────────────

type SeedFn = unsafe extern "C" fn() -> i32;

/// Private `CGSCurrentCursorSeed` — changes whenever the cursor image does.
/// Optional (dlsym); without it the image is re-rendered every tick.
fn cursor_seed_fn() -> Option<SeedFn> {
    // SAFETY: dlsym with RTLD_DEFAULT and a static NUL-terminated name; the
    // symbol has signature `int CGSCurrentCursorSeed(void)`.
    let p = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"CGSCurrentCursorSeed".as_ptr()) };
    (!p.is_null()).then(|| unsafe { std::mem::transmute::<*mut c_void, SeedFn>(p) })
}

/// Render the current system cursor to a PNG asset (OpenScreen's
/// `currentCursorAsset`). Must run inside an autorelease pool.
unsafe fn current_cursor_asset() -> Option<CursorAsset> {
    let mut cursor: *mut AnyObject = msg_send![class!(NSCursor), currentSystemCursor];
    if cursor.is_null() {
        cursor = msg_send![class!(NSCursor), currentCursor];
    }
    if cursor.is_null() {
        return None;
    }
    let image: *mut AnyObject = msg_send![cursor, image];
    if image.is_null() {
        return None;
    }
    let size: CGSize = msg_send![image, size];
    if !(size.width > 0.0 && size.height > 0.0) {
        return None;
    }
    let mut rect = CGRect { origin: CGPoint::default(), size };
    let cg: *mut CGImageOpaque = msg_send![
        image,
        CGImageForProposedRect: &mut rect as *mut CGRect,
        context: std::ptr::null_mut::<AnyObject>(),
        hints: std::ptr::null_mut::<AnyObject>()
    ];
    if cg.is_null() {
        return None;
    }
    let rep: *mut AnyObject = msg_send![class!(NSBitmapImageRep), alloc];
    let rep: *mut AnyObject = msg_send![rep, initWithCGImage: cg];
    if rep.is_null() {
        return None;
    }
    let props: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
    let data: *mut AnyObject = msg_send![rep, representationUsingType: NS_BITMAP_PNG, properties: props];
    let px_w: isize = msg_send![rep, pixelsWide];
    let px_h: isize = msg_send![rep, pixelsHigh];
    let png = if data.is_null() {
        None
    } else {
        let len: usize = msg_send![data, length];
        let bytes: *const u8 = msg_send![data, bytes];
        (!bytes.is_null() && len > 0).then(|| std::slice::from_raw_parts(bytes, len).to_vec())
    };
    let _: () = msg_send![rep, release];
    let png = png?;
    if px_w <= 0 || px_h <= 0 {
        return None;
    }
    let scale = px_w as f64 / size.width;
    let hot: CGPoint = msg_send![cursor, hotSpot];
    Some(CursorAsset {
        id: sha256_hex(&png),
        platform: "darwin".into(),
        image_data_url: format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&png)
        ),
        width: px_w as u32,
        height: px_h as u32,
        hotspot_x: hot.x * scale,
        hotspot_y: hot.y * scale,
        scale_factor: Some(scale),
        cursor_type: None,
    })
}

// ── recorder ─────────────────────────────────────────────────────────────────

struct TapEvent {
    at_ns: i64,
    x: f64,
    y: f64,
    kind: Interaction,
}

struct TapCtx {
    port: AtomicPtr<c_void>,
    clock: Arc<PauseShared>,
    queue: Mutex<Vec<TapEvent>>,
}

extern "C" fn tap_cb(_proxy: CGEventTapProxy, etype: u32, event: CGEventRef, user: *mut c_void) -> CGEventRef {
    // SAFETY: `user` is the `TapCtx` kept alive by the sampler thread until
    // after the tap is invalidated.
    let ctx = unsafe { &*(user as *const TapCtx) };
    if etype == EVT_TAP_DISABLED_BY_TIMEOUT || etype == EVT_TAP_DISABLED_BY_USER_INPUT {
        let port = ctx.port.load(Ordering::Acquire);
        if !port.is_null() {
            // SAFETY: port is the live tap created by the sampler thread.
            unsafe { CGEventTapEnable(port, true) };
        }
        return event;
    }
    if ctx.clock.is_paused() {
        return event;
    }
    // SAFETY: `event` is valid for the callback's duration.
    let (p, clicks) = unsafe { (CGEventGetLocation(event), CGEventGetIntegerValueField(event, CG_MOUSE_CLICK_STATE)) };
    if let Some(kind) = interaction_for_event(etype, clicks) {
        ctx.queue.lock().push(TapEvent {
            at_ns: host_now_ns(),
            x: p.x,
            y: p.y,
            kind,
        });
    }
    event
}

/// Timing stats of the sampler thread (for the live test / logging).
#[derive(Debug, Default, Clone, Copy)]
pub struct TickStats {
    pub ticks: u64,
    pub total_work: Duration,
    pub max_work: Duration,
    pub image_renders: u64,
    pub ax_calls: u64,
    pub ax_total: Duration,
}

struct Shared {
    bounds: Mutex<(f64, f64, f64, f64)>,
    stop: AtomicBool,
}

pub struct CursorRecorder {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<(CursorRecording, TickStats)>>,
}

impl CursorRecorder {
    /// Start sampling. `bounds_pt` = captured content in global top-left
    /// points (x, y, w, h); `origin_ns` = timeline ns of video time 0.
    pub fn start(bounds_pt: (f64, f64, f64, f64), clock: Arc<PauseShared>, origin_ns: i64) -> Self {
        let shared = Arc::new(Shared {
            bounds: Mutex::new(bounds_pt),
            stop: AtomicBool::new(false),
        });
        let sh = shared.clone();
        let thread = std::thread::Builder::new()
            .name("ir-record-cursor".into())
            .spawn(move || run(sh, clock, origin_ns))
            .ok();
        if thread.is_none() {
            tracing::warn!("record cursor: could not spawn sampler thread");
        }
        Self { shared, thread }
    }

    /// The captured content moved/resized (window recording).
    pub fn set_bounds(&self, bounds_pt: (f64, f64, f64, f64)) {
        *self.shared.bounds.lock() = bounds_pt;
    }

    pub fn stop(self) -> CursorRecording {
        self.stop_with_stats().0
    }

    pub fn stop_with_stats(mut self) -> (CursorRecording, TickStats) {
        self.shared.stop.store(true, Ordering::Release);
        let fallback = || CursorRecording {
            version: 1,
            provider: CursorProvider::None,
            samples: Vec::new(),
            assets: Vec::new(),
        };
        match self.thread.take().map(|t| t.join()) {
            Some(Ok(r)) => r,
            _ => (fallback(), TickStats::default()),
        }
    }
}

impl Drop for CursorRecorder {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
    }
}

fn run(shared: Arc<Shared>, clock: Arc<PauseShared>, origin_ns: i64) -> (CursorRecording, TickStats) {
    let native = crate::expander::accessibility_granted();
    let ctx = Arc::new(TapCtx {
        port: AtomicPtr::new(std::ptr::null_mut()),
        clock: clock.clone(),
        queue: Mutex::new(Vec::new()),
    });

    // Install the listen-only tap on this thread's run loop.
    let mut tap: Option<(CFMachPortRef, CFRunLoopSourceRef, CFRunLoopRef)> = None;
    if native {
        let mask: u64 = (1 << EVT_LEFT_DOWN) | (1 << EVT_LEFT_UP) | (1 << EVT_RIGHT_DOWN) | (1 << EVT_OTHER_DOWN);
        // SAFETY: standard CGEventTap setup; `ctx` outlives the tap (released
        // after CFMachPortInvalidate below).
        unsafe {
            let port = CGEventTapCreate(
                CG_SESSION_TAP,
                CG_HEAD_INSERT,
                CG_TAP_LISTEN_ONLY,
                mask,
                tap_cb,
                Arc::as_ptr(&ctx) as *mut c_void,
            );
            if port.is_null() {
                tracing::warn!("record cursor: CGEventTapCreate failed; clicks unavailable");
            } else {
                ctx.port.store(port, Ordering::Release);
                let src = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), port, 0);
                let rl = CFRunLoopGetCurrent();
                CFRunLoopAddSource(rl, src, kCFRunLoopDefaultMode);
                CGEventTapEnable(port, true);
                tap = Some((port, src, rl));
            }
        }
    }
    // Clicks AND cursor types need Accessibility + a working tap; without the
    // tap we cannot honestly claim `native` (no clicks), so the whole
    // recording is position-only (positions + images).
    if native && tap.is_none() {
        tracing::warn!("record cursor: Accessibility granted but no event tap; recording position-only");
    }
    let native = tap.is_some();
    let provider = if native { CursorProvider::Native } else { CursorProvider::PositionOnly };
    let mut app_cache = AppElCache::default();

    let attrs = AxAttrs {
        role: CfStr::new("AXRole"),
        subrole: CfStr::new("AXSubrole"),
        role_desc: CfStr::new("AXRoleDescription"),
        parent: CfStr::new("AXParent"),
    };
    let seed_fn = cursor_seed_fn();

    let mut sink = SampleSink::default();
    let mut assets: Vec<CursorAsset> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut visibility = CursorVisibility::new();
    let mut last_seed: Option<i32> = None;
    let mut asset_id: Option<String> = None;
    let mut cursor_type: Option<CursorType> = None;
    let mut ax_paused_until: Option<Instant> = None;
    let mut stats = TickStats::default();
    let tick = Duration::from_millis(TICK_MS);
    let mut next = Instant::now();

    let emit = |sink: &mut SampleSink,
                    vis: &mut CursorVisibility,
                    timeline_ns: i64,
                    p: (f64, f64),
                    kind: Option<Interaction>,
                    asset_id: &Option<String>,
                    cursor_type: Option<CursorType>| {
        let Some(time_ms) = video_ms(timeline_ns, origin_ns) else {
            return;
        };
        let bounds = *shared.bounds.lock();
        let (cx, cy, inside) = normalize_point(p.0, p.1, bounds);
        sink.push(CursorSample {
            time_ms,
            cx,
            cy,
            visible: Some(vis.observe(inside)),
            asset_id: asset_id.clone(),
            cursor_type: if native { cursor_type } else { None },
            interaction_type: Some(kind.unwrap_or(Interaction::Move)),
        });
    };

    while !shared.stop.load(Ordering::Acquire) {
        // Pump the run loop (tap events) until the next tick.
        let now = Instant::now();
        if next > now {
            let wait = (next - now).as_secs_f64();
            // SAFETY: running this thread's own run loop.
            unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, wait, false) };
            if Instant::now() < next {
                std::thread::sleep(next.saturating_duration_since(Instant::now()));
            }
        }
        next += tick;
        if Instant::now() > next + tick * 4 {
            next = Instant::now() + tick; // fell far behind (sleep): resync
        }
        if shared.stop.load(Ordering::Acquire) {
            break;
        }
        let work_start = Instant::now();
        // Drain every tick. Each click is retimed by its own CAPTURE time
        // (`timeline_of`, the rule the video uses): a click queued before a
        // pause keeps its pre-pause offset even when drained during or after
        // it, and one that falls inside a pause is dropped.
        let events: Vec<TapEvent> = std::mem::take(&mut *ctx.queue.lock());
        if clock.is_paused() {
            for ev in &events {
                if let Some(at) = clock.timeline_of(ev.at_ns) {
                    emit(&mut sink, &mut visibility, at, (ev.x, ev.y), Some(ev.kind), &asset_id, cursor_type);
                }
            }
            continue;
        }
        objc2::rc::autoreleasepool(|_| {
            // Cursor image: re-render only when the cursor changed.
            let seed = seed_fn.map(|f| unsafe { f() });
            if seed.is_none() || seed != last_seed || asset_id.is_none() {
                // SAFETY: AppKit reads inside an autorelease pool; any ObjC
                // exception is caught.
                let res = unsafe { objc2::exception::catch(std::panic::AssertUnwindSafe(|| current_cursor_asset())) };
                if let Ok(Some(asset)) = res {
                    stats.image_renders += 1;
                    if seen.insert(asset.id.clone()) {
                        asset_id = Some(asset.id.clone());
                        assets.push(asset);
                    } else {
                        asset_id = Some(asset.id);
                    }
                    last_seed = seed;
                }
            }
            let Some(pt) = global_cursor_point() else {
                return;
            };
            if native && ax_paused_until.is_none_or(|t| Instant::now() >= t) {
                let t0 = Instant::now();
                // SAFETY: AX reads with per-element timeouts.
                cursor_type = unsafe { ax_cursor_type(pt, &attrs, &mut app_cache) };
                stats.ax_calls += 1;
                stats.ax_total += t0.elapsed();
                ax_paused_until = (t0.elapsed() > AX_SLOW).then(|| Instant::now() + AX_BACKOFF);
            }
            for ev in &events {
                if let Some(at) = clock.timeline_of(ev.at_ns) {
                    emit(&mut sink, &mut visibility, at, (ev.x, ev.y), Some(ev.kind), &asset_id, cursor_type);
                }
            }
            emit(&mut sink, &mut visibility, clock.timeline_ns(host_now_ns()), pt, None, &asset_id, cursor_type);
        });
        let w = work_start.elapsed();
        stats.ticks += 1;
        stats.total_work += w;
        stats.max_work = stats.max_work.max(w);
    }

    if let Some((port, src, rl)) = tap {
        // SAFETY: tear down the tap we created on this thread.
        unsafe {
            ctx.port.store(std::ptr::null_mut(), Ordering::Release);
            CGEventTapEnable(port, false);
            CFRunLoopRemoveSource(rl, src, kCFRunLoopDefaultMode);
            CFMachPortInvalidate(port);
            CFRelease(src as CFTypeRef);
            CFRelease(port as CFTypeRef);
        }
    }
    if stats.ticks > 0 {
        tracing::info!(
            "record cursor: {} samples, {} assets, avg tick {:.2} ms, max {:.2} ms",
            sink.samples.len(),
            assets.len(),
            stats.total_work.as_secs_f64() * 1000.0 / stats.ticks as f64,
            stats.max_work.as_secs_f64() * 1000.0
        );
    }
    (
        CursorRecording {
            version: 1,
            provider,
            samples: sink.samples,
            assets,
        },
        stats,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: u64, kind: Interaction) -> CursorSample {
        CursorSample {
            time_ms: t,
            cx: 0.5,
            cy: 0.5,
            visible: Some(true),
            asset_id: None,
            cursor_type: None,
            interaction_type: Some(kind),
        }
    }

    #[test]
    fn sink_drops_non_increasing_moves() {
        let mut k = SampleSink::default();
        assert!(k.push(s(10, Interaction::Move)));
        assert!(!k.push(s(10, Interaction::Move)));
        assert!(!k.push(s(9, Interaction::Move)));
        assert!(k.push(s(11, Interaction::Move)));
        assert_eq!(k.samples.len(), 2);
    }

    #[test]
    fn sink_keeps_late_clicks_one_ms_after() {
        let mut k = SampleSink::default();
        k.push(s(40, Interaction::Move));
        assert!(k.push(s(38, Interaction::Click)));
        assert!(k.push(s(40, Interaction::Mouseup)));
        let t: Vec<u64> = k.samples.iter().map(|x| x.time_ms).collect();
        assert_eq!(t, vec![40, 41, 42]);
        assert_eq!(k.samples[1].interaction_type, Some(Interaction::Click));
        // a following tick at the same ms as the bumped click is dropped
        assert!(!k.push(s(42, Interaction::Move)));
    }

    #[test]
    fn sink_caps() {
        let mut k = SampleSink::default();
        for i in 0..MAX_SAMPLES as u64 {
            assert!(k.push(s(i, Interaction::Move)));
        }
        assert!(!k.push(s(MAX_SAMPLES as u64, Interaction::Click)));
        assert!(k.capped);
        assert_eq!(k.samples.len(), MAX_SAMPLES);
    }

    #[test]
    fn event_mapping() {
        assert_eq!(interaction_for_event(EVT_LEFT_DOWN, 1), Some(Interaction::Click));
        assert_eq!(interaction_for_event(EVT_LEFT_DOWN, 0), Some(Interaction::Click));
        assert_eq!(interaction_for_event(EVT_LEFT_DOWN, 2), Some(Interaction::DoubleClick));
        assert_eq!(interaction_for_event(EVT_LEFT_DOWN, 3), Some(Interaction::DoubleClick));
        assert_eq!(interaction_for_event(EVT_LEFT_UP, 1), Some(Interaction::Mouseup));
        assert_eq!(interaction_for_event(EVT_RIGHT_DOWN, 1), Some(Interaction::RightClick));
        assert_eq!(interaction_for_event(EVT_OTHER_DOWN, 1), Some(Interaction::MiddleClick));
        assert_eq!(interaction_for_event(4, 1), None); // right up
        assert_eq!(interaction_for_event(5, 0), None); // mouse moved
    }

    #[test]
    fn video_ms_relative_to_origin() {
        assert_eq!(video_ms(5_000_000, 5_000_000), Some(0));
        assert_eq!(video_ms(5_999_999 + 1_000_000, 5_000_000), Some(1));
        assert_eq!(video_ms(4_999_999, 5_000_000), None);
    }

    #[test]
    fn sha256_hex_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    #[ignore]
    fn cursor_recorder_live() {
        let clock = Arc::new(PauseShared::new());
        let origin = clock.timeline_ns(host_now_ns());
        let rec = CursorRecorder::start((0.0, 0.0, 1920.0, 1080.0), clock, origin);
        std::thread::sleep(Duration::from_secs(2));
        let (r, st) = rec.stop_with_stats();
        let avg = st.total_work.as_secs_f64() * 1000.0 / st.ticks.max(1) as f64;
        println!(
            "provider={:?} samples={} assets={} ticks={} avg_tick_ms={:.3} max_tick_ms={:.3} image_renders={} ax_calls={} avg_ax_ms={:.3} typed_samples={} first_asset={:?}",
            r.provider,
            r.samples.len(),
            r.assets.len(),
            st.ticks,
            avg,
            st.max_work.as_secs_f64() * 1000.0,
            st.image_renders,
            st.ax_calls,
            st.ax_total.as_secs_f64() * 1000.0 / st.ax_calls.max(1) as f64,
            r.samples.iter().filter(|s| s.cursor_type.is_some()).count(),
            r.assets.first().map(|a| (a.width, a.height, a.hotspot_x, a.hotspot_y, a.scale_factor, a.image_data_url.len())),
        );
        assert!(r.samples.len() >= 50, "too few samples: {}", r.samples.len());
        assert!(r.samples.windows(2).all(|w| w[0].time_ms < w[1].time_ms));
        assert!(!r.assets.is_empty());
        assert!(r.assets[0].image_data_url.starts_with("data:image/png;base64,"));
        assert!(avg < 10.0, "avg tick {avg:.3} ms");
    }
}
