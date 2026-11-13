//! IOKit HID transport for [`super`]: one thread (`ir-logi`) owns the HID
//! manager and the mouse. Everything that talks to the device runs on it —
//! requests from other threads arrive as closures over a channel and the
//! thread's run loop is stopped to wake it.
//!
//! The thread sleeps in `CFRunLoopRunInMode` and wakes only for a report from
//! the mouse, a (dis)connect or a queued request; with no Logitech mouse it
//! never wakes.

use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use objc2_foundation::{NSDictionary, NSNumber, NSString};
use tauri::{Emitter, Manager};

use super::*;

type Ref = *mut c_void;
type CFStringRef = *const c_void;
type CFRunLoopRef = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

type DeviceCallback = extern "C" fn(context: *mut c_void, result: i32, sender: *mut c_void, device: Ref);
type ReportCallback = extern "C" fn(
    context: *mut c_void,
    result: i32,
    sender: *mut c_void,
    report_type: u32,
    report_id: u32,
    report: *mut u8,
    report_length: isize,
);

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDManagerCreate(allocator: *const c_void, options: u32) -> Ref;
    fn IOHIDManagerSetDeviceMatching(manager: Ref, matching: *const c_void);
    fn IOHIDManagerRegisterDeviceMatchingCallback(manager: Ref, callback: DeviceCallback, context: *mut c_void);
    fn IOHIDManagerRegisterDeviceRemovalCallback(manager: Ref, callback: DeviceCallback, context: *mut c_void);
    fn IOHIDManagerScheduleWithRunLoop(manager: Ref, run_loop: CFRunLoopRef, mode: CFStringRef);
    fn IOHIDManagerOpen(manager: Ref, options: u32) -> i32;
    fn IOHIDDeviceSetReport(device: Ref, report_type: u32, report_id: isize, report: *const u8, length: isize) -> i32;
    fn IOHIDDeviceRegisterInputReportCallback(
        device: Ref,
        report: *mut u8,
        report_length: isize,
        callback: ReportCallback,
        context: *mut c_void,
    );
    fn IOHIDDeviceGetProperty(device: Ref, key: CFStringRef) -> *const c_void;
    fn IOHIDRequestAccess(request_type: u32) -> bool;
}

/// `CFRunLoopSourceContext` (version 0). Only `perform` is used.
#[repr(C)]
struct SourceContext {
    version: isize,
    info: *mut c_void,
    unused: [*const c_void; 7],
    perform: extern "C" fn(*mut c_void),
}

extern "C" fn woken(_: *mut c_void) {}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopRunInMode(mode: CFStringRef, seconds: f64, return_after_source_handled: bool) -> i32;
    fn CFRunLoopSourceCreate(allocator: *const c_void, order: isize, context: *mut SourceContext) -> *mut c_void;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: *mut c_void, mode: CFStringRef);
    fn CFRunLoopSourceSignal(source: *mut c_void);
    fn CFRunLoopWakeUp(rl: CFRunLoopRef);
    fn CFRelease(cf: *const c_void);
    static kCFRunLoopDefaultMode: CFStringRef;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGEventCreate(source: *mut c_void) -> *mut c_void;
    fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
    fn CGWarpMouseCursorPosition(new_cursor_position: CGPoint) -> i32;
    fn CGAssociateMouseAndMouseCursorPosition(connected: bool) -> i32;
}

const VENDOR_LOGITECH: i32 = 0x046d;
/// The HID++ collection of a Bluetooth device.
const USAGE_PAGE_HIDPP_BLE: i32 = 0xFF43;
const USAGE_HIDPP_LONG: i32 = 0x0202;
const REPORT_TYPE_OUTPUT: u32 = 1;
const REQUEST_LISTEN_EVENT: u32 = 1;
const REPLY_TIMEOUT: Duration = Duration::from_millis(1500);
/// How long a caller on another thread waits for the device thread.
const JOB_TIMEOUT: Duration = Duration::from_secs(8);

type Job = Box<dyn FnOnce(&mut Worker) + Send>;

static JOBS: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
static RUN_LOOP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
/// Signalled to wake the device thread; a signal sent while the thread is
/// busy is kept until it next waits (a bare `CFRunLoopStop` would be lost).
static WAKE_SOURCE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static STATUS: Mutex<Option<LogiStatus>> = Mutex::new(None);
/// Filled by the C callbacks (they run on the device thread).
static INBOX: Mutex<Inbox> = Mutex::new(Inbox { reports: VecDeque::new(), attached: Vec::new(), removed: Vec::new() });

struct Inbox {
    reports: VecDeque<Vec<u8>>,
    attached: Vec<usize>,
    removed: Vec<usize>,
}

extern "C" fn on_attached(_: *mut c_void, _: i32, _: *mut c_void, device: Ref) {
    INBOX.lock().unwrap().attached.push(device as usize);
}

extern "C" fn on_removed(_: *mut c_void, _: i32, _: *mut c_void, device: Ref) {
    INBOX.lock().unwrap().removed.push(device as usize);
}

extern "C" fn on_report(_: *mut c_void, _: i32, _: *mut c_void, _: u32, _: u32, report: *mut u8, len: isize) {
    if report.is_null() || len <= 0 || len > 64 {
        return;
    }
    // SAFETY: IOKit hands us `len` valid bytes of the buffer we registered.
    let bytes = unsafe { std::slice::from_raw_parts(report, len as usize) }.to_vec();
    let mut inbox = INBOX.lock().unwrap();
    if inbox.reports.len() < 256 {
        inbox.reports.push_back(bytes);
    }
}

struct Held {
    start: CGPoint,
}

struct Worker {
    app: tauri::AppHandle,
    device: Option<Ref>,
    /// The report buffer IOKit writes into; lives as long as the thread.
    buffer: Box<[u8; 64]>,
    features: HashMap<u16, u8>,
    /// Device events seen while waiting for a reply.
    events: VecDeque<Vec<u8>>,
    pressed: Vec<u16>,
    held: HashMap<u16, Held>,
    config: LogiConfig,
    status: LogiStatus,
}

fn cursor() -> CGPoint {
    // SAFETY: a null source is the documented way to read the current state;
    // the event is released right after.
    unsafe {
        let ev = CGEventCreate(std::ptr::null_mut());
        if ev.is_null() {
            return CGPoint { x: 0.0, y: 0.0 };
        }
        let p = CGEventGetLocation(ev);
        CFRelease(ev as *const c_void);
        p
    }
}

fn options_running() -> bool {
    std::process::Command::new("/usr/bin/pgrep")
        .args(["-fq", "logioptionsplus_agent"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

impl Worker {
    fn publish(&mut self) {
        let mut slot = STATUS.lock().unwrap();
        if slot.as_ref() != Some(&self.status) {
            *slot = Some(self.status.clone());
            drop(slot);
            let _ = self.app.emit(EVENT, ());
        }
    }

    fn pump(&mut self, seconds: f64) {
        // SAFETY: plain run-loop call on this thread.
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, true) };
    }

    /// Send one request and wait for its reply; other reports seen meanwhile
    /// are kept as events.
    fn call_index(&mut self, index: u8, function: u8, params: &[u8]) -> Result<Vec<u8>, String> {
        let device = self.device.ok_or("no device")?;
        let msg = request(index, function, params);
        // SAFETY: `device` is the live IOHIDDevice from the matching callback
        // (cleared on removal, on this same thread); the buffer is 20 bytes.
        let r = unsafe { IOHIDDeviceSetReport(device, REPORT_TYPE_OUTPUT, REPORT_LONG as isize, msg.as_ptr(), REPORT_LEN as isize) };
        if r != 0 {
            return Err(format!("send failed (0x{:x})", r as u32));
        }
        let until = Instant::now() + REPLY_TIMEOUT;
        loop {
            let reports: Vec<Vec<u8>> = INBOX.lock().unwrap().reports.drain(..).collect();
            let mut answer = None;
            for rep in reports {
                if answer.is_some() {
                    self.events.push_back(rep);
                    continue;
                }
                match classify_incoming(&rep, index, function) {
                    Incoming::Reply(p) => answer = Some(Ok(p.to_vec())),
                    Incoming::Error(c) => answer = Some(Err(format!("device: {}", error_text(c)))),
                    Incoming::Other => self.events.push_back(rep),
                }
            }
            if let Some(a) = answer {
                return a;
            }
            if Instant::now() >= until {
                return Err("no answer from the mouse".into());
            }
            self.pump(0.05);
        }
    }

    fn call(&mut self, feature: u16, function: u8, params: &[u8]) -> Result<Vec<u8>, String> {
        let index = *self.features.get(&feature).ok_or("feature missing")?;
        self.call_index(index, function, params)
    }

    fn has(&self, feature: u16) -> bool {
        self.features.contains_key(&feature)
    }

    fn attach(&mut self, device: Ref) {
        if self.device.is_some() {
            return;
        }
        self.device = Some(device);
        self.features.clear();
        self.pressed.clear();
        self.held.clear();
        // SAFETY: the buffer outlives the device (it lives in the worker,
        // which lives as long as the thread); the callback only copies.
        unsafe {
            IOHIDDeviceRegisterInputReportCallback(device, self.buffer.as_mut_ptr(), 64, on_report, std::ptr::null_mut());
        }
        for f in [F_NAME, F_BATTERY, F_REPROG, F_SMARTSHIFT, F_HIRES_WHEEL, F_DPI] {
            let [hi, lo] = f.to_be_bytes();
            match self.call_index(0, 0, &[hi, lo]) {
                Ok(p) if p.first().is_some_and(|i| *i != 0) => {
                    self.features.insert(f, p[0]);
                }
                Ok(_) => {}
                Err(e) => {
                    // The first request failing means the device does not
                    // speak to us at all — give up on it.
                    tracing::warn!("logi: feature 0x{f:04x} lookup failed: {e}");
                    if self.features.is_empty() && f == F_NAME {
                        self.device = None;
                        return;
                    }
                }
            }
        }
        self.status = LogiStatus { supported: true, connected: true, name: self.product_name(device), ..Default::default() };
        self.read_controls();
        self.apply_config();
        self.read_state();
        tracing::info!("logi: {} connected ({} features)", self.status.name, self.features.len());
        self.publish();
    }

    fn product_name(&self, device: Ref) -> String {
        let key = NSString::from_str("Product");
        // SAFETY: NSString is toll-free bridged to CFString; the property is
        // owned by the device (get rule) and copied into a Rust string here.
        unsafe {
            let v = IOHIDDeviceGetProperty(device, &*key as *const NSString as CFStringRef);
            if v.is_null() {
                return "Logitech".into();
            }
            (*(v as *const NSString)).to_string()
        }
    }

    fn detach(&mut self) {
        self.device = None;
        self.features.clear();
        self.pressed.clear();
        self.held.clear();
        self.events.clear();
        self.status = LogiStatus { supported: true, ..Default::default() };
        tracing::info!("logi: mouse disconnected");
        self.publish();
    }

    fn read_controls(&mut self) {
        let mut out = Vec::new();
        if let Ok(p) = self.call(F_REPROG, 0, &[]) {
            for i in 0..p.first().copied().unwrap_or(0).min(32) {
                if let Some(c) = self.call(F_REPROG, 1, &[i]).ok().and_then(|p| parse_control(&p)) {
                    out.push(c);
                }
            }
        }
        self.status.controls = out;
    }

    /// Battery and the three settings, as the mouse reports them now.
    fn read_state(&mut self) {
        if self.device.is_none() {
            return;
        }
        self.status.battery = self.call(F_BATTERY, 0, &[]).ok().and_then(|p| parse_battery(&p));
        self.status.dpi = self.read_dpi();
        self.status.wheel = self.call(F_SMARTSHIFT, 0, &[]).ok().and_then(|p| parse_smartshift(&p));
        self.status.invert = self.read_invert();
        self.status.options_running = options_running();
    }

    fn read_dpi(&mut self) -> Option<Dpi> {
        let list = self.call(F_DPI, 1, &[0]).ok()?;
        let options = parse_dpi_list(list.get(1..)?);
        let cur = self.call(F_DPI, 2, &[0]).ok()?;
        if cur.len() < 5 || options.is_empty() {
            return None;
        }
        Some(Dpi {
            current: u16::from_be_bytes([cur[1], cur[2]]),
            default: u16::from_be_bytes([cur[3], cur[4]]),
            options,
        })
    }

    fn read_invert(&mut self) -> Option<bool> {
        let cap = self.call(F_HIRES_WHEEL, 0, &[]).ok()?;
        if cap.get(1)? & WHEEL_CAP_INVERT == 0 {
            return None;
        }
        let mode = self.call(F_HIRES_WHEEL, 1, &[]).ok()?;
        Some(mode.first()? & WHEEL_INVERT_BIT != 0)
    }

    /// Write the stored configuration to the mouse. Each part is independent:
    /// one refused setting does not stop the others.
    fn apply_config(&mut self) {
        if self.device.is_none() {
            return;
        }
        let cfg = self.config.clone();
        if let (Some(want), true) = (cfg.dpi, self.has(F_DPI)) {
            let options = self.call(F_DPI, 1, &[0]).ok().map(|l| parse_dpi_list(l.get(1..).unwrap_or(&[]))).unwrap_or_default();
            if let Some(dpi) = nearest_dpi(&options, want) {
                let [hi, lo] = dpi.to_be_bytes();
                if let Err(e) = self.call(F_DPI, 3, &[0, hi, lo]) {
                    tracing::warn!("logi: DPI not set: {e}");
                }
            }
        }
        if let (Some(w), true) = (cfg.wheel, self.has(F_SMARTSHIFT)) {
            if let Err(e) = self.call(F_SMARTSHIFT, 1, &smartshift_bytes(w)) {
                tracing::warn!("logi: wheel mode not set: {e}");
            }
        }
        if let (Some(inv), true) = (cfg.invert, self.has(F_HIRES_WHEEL)) {
            if let Ok(mode) = self.call(F_HIRES_WHEEL, 1, &[]) {
                let now = mode.first().copied().unwrap_or(0);
                let next = with_invert(now, inv);
                if next != now {
                    if let Err(e) = self.call(F_HIRES_WHEEL, 2, &[next]) {
                        tracing::warn!("logi: scroll direction not set: {e}");
                    }
                }
            }
        }
        if self.has(F_REPROG) {
            for c in self.status.controls.clone() {
                if !c.divertable {
                    continue;
                }
                let ours = cfg.button(c.cid).is_some();
                if let Err(e) = self.call(F_REPROG, 3, &divert_bytes(c.cid, ours)) {
                    tracing::warn!("logi: button 0x{:x} not {}: {e}", c.cid, if ours { "taken" } else { "released" });
                }
            }
        }
        self.pressed.clear();
        self.held.clear();
    }

    fn handle_events(&mut self) {
        let fresh: Vec<Vec<u8>> = INBOX.lock().unwrap().reports.drain(..).collect();
        self.events.extend(fresh);
        while let Some(rep) = self.events.pop_front() {
            let Some((index, function, payload)) = as_event(&rep) else { continue };
            if Some(&index) == self.features.get(&F_REPROG) && function == 0 {
                let now = parse_pressed(payload);
                self.buttons_changed(now);
            } else if Some(&index) == self.features.get(&F_BATTERY) && function == 0 {
                if let Some(b) = parse_battery(payload) {
                    self.status.battery = Some(b);
                    self.publish();
                }
            }
        }
    }

    fn buttons_changed(&mut self, now: Vec<u16>) {
        let (down, up) = press_edges(&self.pressed, &now);
        self.pressed = now;
        for cid in down {
            let Some(b) = self.config.button(cid).cloned() else { continue };
            if fires_on_press(&b) {
                if let Some(a) = &b.press {
                    self.perform(a);
                }
            } else {
                self.held.insert(cid, Held { start: cursor() });
            }
        }
        for cid in up {
            let (Some(h), Some(b)) = (self.held.remove(&cid), self.config.button(cid).cloned()) else { continue };
            let p = cursor();
            if let Some((action, dir)) = release_action(&b, p.x - h.start.x, p.y - h.start.y) {
                if dir.is_some() {
                    // The move was the gesture, not pointing: put the pointer
                    // back. The second call re-attaches mouse and cursor — a
                    // bare warp freezes the pointer for a quarter second.
                    // SAFETY: plain CoreGraphics calls with a valid point.
                    unsafe {
                        CGWarpMouseCursorPosition(h.start);
                        CGAssociateMouseAndMouseCursorPosition(true);
                    }
                }
                let action = action.clone();
                self.perform(&action);
            }
        }
    }

    fn perform(&self, action: &BindingAction) {
        tracing::info!("logi: button action {:?}", action.kind());
        crate::gestures::perform_action(&self.app, action);
    }
}

fn run(app: tauri::AppHandle, jobs: Receiver<Job>) {
    let db = app.state::<DbHandle>().inner().clone();
    let mut w = Worker {
        app,
        device: None,
        buffer: Box::new([0u8; 64]),
        features: HashMap::new(),
        events: VecDeque::new(),
        pressed: Vec::new(),
        held: HashMap::new(),
        config: load(&db),
        status: LogiStatus { supported: true, ..Default::default() },
    };
    let keys = [NSString::from_str("VendorID"), NSString::from_str("DeviceUsagePage"), NSString::from_str("DeviceUsage")];
    let vals = [NSNumber::new_i32(VENDOR_LOGITECH), NSNumber::new_i32(USAGE_PAGE_HIDPP_BLE), NSNumber::new_i32(USAGE_HIDPP_LONG)];
    let matching = NSDictionary::from_retained_objects(&[&*keys[0], &*keys[1], &*keys[2]], &vals);
    // SAFETY: the manager is created, configured and scheduled on this thread
    // and never released (it lives as long as the app). NSDictionary is
    // toll-free bridged to CFDictionary; the manager retains it.
    let opened = unsafe {
        let rl = CFRunLoopGetCurrent();
        let mut ctx = SourceContext { version: 0, info: std::ptr::null_mut(), unused: [std::ptr::null(); 7], perform: woken };
        let source = CFRunLoopSourceCreate(std::ptr::null(), 0, &mut ctx);
        CFRunLoopAddSource(rl, source, kCFRunLoopDefaultMode);
        WAKE_SOURCE.store(source, Ordering::SeqCst);
        RUN_LOOP.store(rl, Ordering::SeqCst);
        let mgr = IOHIDManagerCreate(std::ptr::null(), 0);
        IOHIDManagerSetDeviceMatching(mgr, &*matching as *const _ as *const c_void);
        IOHIDManagerRegisterDeviceMatchingCallback(mgr, on_attached, std::ptr::null_mut());
        IOHIDManagerRegisterDeviceRemovalCallback(mgr, on_removed, std::ptr::null_mut());
        IOHIDManagerScheduleWithRunLoop(mgr, rl, kCFRunLoopDefaultMode);
        IOHIDManagerOpen(mgr, 0)
    };
    if opened != 0 {
        tracing::warn!("logi: HID manager not opened (0x{:x}) — Input Monitoring?", opened as u32);
        w.status.permission_denied = true;
    }
    w.publish();
    loop {
        // Sleeps until a report, a (dis)connect or `wake()`.
        w.pump(3600.0);
        let (attached, removed) = {
            let mut inbox = INBOX.lock().unwrap();
            (std::mem::take(&mut inbox.attached), std::mem::take(&mut inbox.removed))
        };
        for d in removed {
            if w.device.map(|x| x as usize) == Some(d) {
                w.detach();
            }
        }
        for d in attached {
            w.attach(d as Ref);
        }
        while let Ok(job) = jobs.try_recv() {
            job(&mut w);
        }
        w.handle_events();
    }
}

fn wake() {
    let (rl, source) = (RUN_LOOP.load(Ordering::SeqCst), WAKE_SOURCE.load(Ordering::SeqCst));
    if !rl.is_null() && !source.is_null() {
        // SAFETY: both calls are thread-safe; loop and source belong to a
        // thread that never exits and are never released.
        unsafe {
            CFRunLoopSourceSignal(source);
            CFRunLoopWakeUp(rl);
        }
    }
}

/// Run `f` on the device thread and wait for it.
fn on_device_thread(f: impl FnOnce(&mut Worker) + Send + 'static) {
    let Some(tx) = JOBS.get() else { return };
    let (done_tx, done_rx) = channel::<()>();
    let job: Job = Box::new(move |w| {
        f(w);
        let _ = done_tx.send(());
    });
    if tx.lock().unwrap().send(job).is_err() {
        return;
    }
    wake();
    let _ = done_rx.recv_timeout(JOB_TIMEOUT);
}

pub fn start(app: tauri::AppHandle) {
    let (tx, rx) = channel::<Job>();
    if JOBS.set(Mutex::new(tx)).is_err() {
        return;
    }
    let _ = std::thread::Builder::new().name("ir-logi".into()).spawn(move || run(app, rx));
}

pub fn status() -> LogiStatus {
    STATUS.lock().unwrap().clone().unwrap_or(LogiStatus { supported: true, ..Default::default() })
}

pub fn refresh() {
    on_device_thread(|w| {
        w.read_state();
        w.publish();
    });
}

pub fn apply(cfg: LogiConfig) {
    on_device_thread(move |w| {
        w.config = cfg;
        w.apply_config();
        w.read_state();
        w.publish();
    });
}

pub fn request_access() -> bool {
    // SAFETY: plain IOKit call; shows the system prompt at most once.
    unsafe { IOHIDRequestAccess(REQUEST_LISTEN_EVENT) }
}
