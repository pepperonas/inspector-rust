//! Windows touchpad capture via **Raw Input + HID Precision Touchpad**, run
//! through the same gesture guard pipeline as macOS (gesture guard Phase 6).
//!
//! There is no Win32 API that hands you "3-finger swipe" directly (`WM_GESTURE`
//! is touchscreen legacy). Raw Input is registered on the Precision-Touchpad
//! HID (Usage Page `0x0D`, Usage `0x05`) with `RIDEV_INPUTSINK` (frames arrive
//! without focus); each report is parsed PER CONTACT against the device's
//! preparsed data (`HidP_*`): contact id, tip switch, position, the
//! **confidence bit** and — when the touchpad reports them — width and height.
//! [`super::ptp`] (pure, tested on every platform) assembles hybrid reports
//! into frames and turns them into [`Touch`]es; [`Pipeline`] then runs all
//! four levels exactly as on macOS: a contact the touchpad marks as
//! not-a-finger is a palm until it lifts, edge zones apply to where a contact
//! lands, the typing level gets every key press from the expander's keyboard
//! hook (times only), and the plausibility rules see the real finger count.
//! The settle ticker for deferred taps is a `WM_TIMER` on the message window,
//! armed only while a tap is pending.
//!
//! **CAVEATS (also in docs/gestures.md):**
//! 1. Works only with genuine **Precision Touchpads**. Old Synaptics/ELAN with a
//!    proprietary driver expose no PTP collection → no frames, no gestures.
//! 2. Raw Input is **read-only**: Windows keeps firing its own 3-/4-finger
//!    gestures in parallel. Set *Settings → Touchpad → Three/Four-finger
//!    gestures* to **Nothing** to avoid double-triggering.
//! 3. PTP reports no contact SIZE comparable to macOS: the size-based palm and
//!    thumb rules stay inactive; the confidence bit carries level 1.
//!
//! **Runtime-unverified** (written + compile-validated against `windows` 0.61;
//! the maintainer has no Windows box) — consistent with the rest of the repo's
//! Windows code.

use super::guard::Pipeline;
use super::ptp::{physical_mm, PtpAssembler, PtpContact, PtpGeometry, PtpTracker};
use super::trace::{DeviceInfo, Frame, KeyDown};
use super::{GestureConfig, GestureSink, GestureSource};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use windows::Win32::Devices::HumanInterfaceDevice::{
    HidP_GetButtonCaps, HidP_GetCaps, HidP_GetUsageValue, HidP_GetUsages, HidP_GetValueCaps, HidP_Input,
    HIDP_BUTTON_CAPS, HIDP_CAPS, HIDP_STATUS_SUCCESS, HIDP_VALUE_CAPS, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
    RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK, RIDI_PREPARSEDDATA, RIM_TYPEHID,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, KillTimer, PostMessageW,
    RegisterClassW, SetTimer, TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_DESTROY, WM_INPUT, WM_TIMER, WNDCLASSW,
};

// HID usage pages / usages for a Precision Touchpad digitizer.
const PAGE_GENERIC: u16 = 0x01;
const USAGE_X: u16 = 0x30;
const USAGE_Y: u16 = 0x31;
const PAGE_DIGITIZER: u16 = 0x0D;
const USAGE_TIP_SWITCH: u16 = 0x42;
const USAGE_CONFIDENCE: u16 = 0x47;
const USAGE_WIDTH: u16 = 0x48;
const USAGE_HEIGHT: u16 = 0x49;
const USAGE_CONTACT_ID: u16 = 0x51;
const USAGE_CONTACT_COUNT: u16 = 0x54;

/// Posted to the message window to ask its loop to quit (on `stop()`).
const WM_GESTURE_QUIT: u32 = WM_APP + 1;
/// The settle ticker (`SetTimer` id) and its period — mirrors macOS `TICK_MS`.
const TICK_TIMER: usize = 1;
const TICK_MS: u32 = 24;

// ── Shared state the WndProc reads (it can't capture) ────────────────────────

static RUNNING: AtomicBool = AtomicBool::new(false);
static SINK: Mutex<Option<GestureSink>> = Mutex::new(None);
static PIPELINE: Mutex<Option<Pipeline>> = Mutex::new(None);
static START: OnceLock<Instant> = OnceLock::new();
static THREAD_HWND: AtomicIsize = AtomicIsize::new(0);
static TICKING: AtomicBool = AtomicBool::new(false);
/// Device facts in first-seen order (= the frames' device index), for the
/// live view and the edge profiles.
static DEVICE_CACHE: Mutex<Vec<DeviceInfo>> = Mutex::new(Vec::new());

/// Per-device state: the preparsed data, the geometry, and the frame
/// assembler + contact tracker. Keyed by the Raw Input `HANDLE` as isize.
struct Device {
    preparsed: Vec<u8>,
    collections: u16,
    has_confidence: bool,
    geometry: PtpGeometry,
    index: u32,
    assembler: PtpAssembler,
    tracker: PtpTracker,
}

fn devices() -> &'static Mutex<HashMap<isize, Device>> {
    static D: OnceLock<Mutex<HashMap<isize, Device>>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(HashMap::new()))
}

#[inline]
fn pp(d: &Device) -> PHIDP_PREPARSED_DATA {
    PHIDP_PREPARSED_DATA(d.preparsed.as_ptr() as isize)
}

/// Milliseconds on the capture clock; `None` before the first start.
pub(crate) fn now_ms() -> Option<u64> {
    START.get().map(|s| s.elapsed().as_millis() as u64)
}

pub(crate) fn is_running() -> bool {
    RUNNING.load(Ordering::Relaxed)
}

pub(crate) fn cached_device_infos() -> Vec<DeviceInfo> {
    DEVICE_CACHE.lock().clone()
}

/// A key-down from the expander's keyboard hook (time + shortcut flag only).
pub(crate) fn note_key(k: KeyDown) {
    if let Some(p) = PIPELINE.lock().as_mut() {
        p.set_complete_keys(true);
        p.key(k);
    }
}

/// New guard thresholds from the panel, applied in place. `false` = no capture.
pub(crate) fn update_guard(g: super::guard::GuardConfig) -> bool {
    match PIPELINE.lock().as_mut() {
        Some(p) => {
            p.set_guard(g);
            true
        }
        None => false,
    }
}

/// Typing level right now; `None` = no capture running.
pub(crate) fn typing_state() -> Option<(bool, bool)> {
    let now = now_ms()?;
    let guard = PIPELINE.lock();
    Some(guard.as_ref()?.typing_state(now))
}

// ── Per-device caps resolution ───────────────────────────────────────────────

unsafe fn resolve_device(hdevice: HANDLE, index: u32) -> Option<Device> {
    let mut size: u32 = 0;
    GetRawInputDeviceInfoW(Some(hdevice), RIDI_PREPARSEDDATA, None, &mut size);
    if size == 0 {
        return None;
    }
    let mut preparsed = vec![0u8; size as usize];
    let got = GetRawInputDeviceInfoW(
        Some(hdevice),
        RIDI_PREPARSEDDATA,
        Some(preparsed.as_mut_ptr() as *mut c_void),
        &mut size,
    );
    if got == u32::MAX || got == 0 {
        return None;
    }
    let pd = PHIDP_PREPARSED_DATA(preparsed.as_ptr() as isize);

    let mut caps = HIDP_CAPS::default();
    if HidP_GetCaps(pd, &mut caps) != HIDP_STATUS_SUCCESS || caps.NumberInputValueCaps == 0 {
        return None;
    }
    let mut value_caps = vec![HIDP_VALUE_CAPS::default(); caps.NumberInputValueCaps as usize];
    let mut len = caps.NumberInputValueCaps;
    if HidP_GetValueCaps(HidP_Input, value_caps.as_mut_ptr(), &mut len, pd) != HIDP_STATUS_SUCCESS {
        return None;
    }
    // X/Y logical ranges for normalising, physical extents for the pad size.
    let mut x: Option<&HIDP_VALUE_CAPS> = None;
    let mut y: Option<&HIDP_VALUE_CAPS> = None;
    for vc in value_caps.iter().take(len as usize) {
        if vc.UsagePage != PAGE_GENERIC || vc.IsRange {
            continue;
        }
        match vc.Anonymous.NotRange.Usage {
            USAGE_X if x.is_none() => x = Some(vc),
            USAGE_Y if y.is_none() => y = Some(vc),
            _ => {}
        }
    }
    let (x, y) = (x?, y?);
    if x.LogicalMax <= x.LogicalMin || y.LogicalMax <= y.LogicalMin {
        return None; // not a usable absolute digitizer
    }
    let geometry = PtpGeometry {
        x_min: x.LogicalMin,
        x_max: x.LogicalMax,
        y_min: y.LogicalMin,
        y_max: y.LogicalMax,
        width_mm: physical_mm(x.PhysicalMin, x.PhysicalMax, x.Units, x.UnitsExp),
        height_mm: physical_mm(y.PhysicalMin, y.PhysicalMax, y.Units, y.UnitsExp),
    };

    // Does the touchpad report the confidence bit at all? Without the usage
    // a contact gets no verdict (rather than "always doubted").
    let mut has_confidence = false;
    if caps.NumberInputButtonCaps > 0 {
        let mut button_caps = vec![HIDP_BUTTON_CAPS::default(); caps.NumberInputButtonCaps as usize];
        let mut blen = caps.NumberInputButtonCaps;
        if HidP_GetButtonCaps(HidP_Input, button_caps.as_mut_ptr(), &mut blen, pd) == HIDP_STATUS_SUCCESS {
            has_confidence = button_caps.iter().take(blen as usize).any(|bc| {
                bc.UsagePage == PAGE_DIGITIZER
                    && if bc.IsRange {
                        (bc.Anonymous.Range.UsageMin..=bc.Anonymous.Range.UsageMax).contains(&USAGE_CONFIDENCE)
                    } else {
                        bc.Anonymous.NotRange.Usage == USAGE_CONFIDENCE
                    }
            });
        }
    }

    Some(Device {
        preparsed,
        collections: caps.NumberLinkCollectionNodes,
        has_confidence,
        geometry,
        index,
        assembler: PtpAssembler::default(),
        tracker: PtpTracker::default(),
    })
}

// ── HID report → contact slots ───────────────────────────────────────────────

/// Read one value usage of a link collection; `None` when the report lacks it.
unsafe fn value(d: &Device, page: u16, coll: u16, usage: u16, report: &[u8]) -> Option<u32> {
    let mut v: u32 = 0;
    (HidP_GetUsageValue(HidP_Input, page, Some(coll), usage, &mut v, pp(d), report) == HIDP_STATUS_SUCCESS)
        .then_some(v)
}

/// Parse one HID report into its contact slots (in collection order, padding
/// included — the assembler drops it) and its Contact Count.
/// Takes a raw ptr/len: `HidP_GetUsages` wants `&mut [u8]`, the value reads
/// `&[u8]`; the HID functions only read the report.
unsafe fn parse_report(d: &Device, report_ptr: *const u8, report_len: usize) -> (Vec<PtpContact>, u32) {
    let report = std::slice::from_raw_parts(report_ptr, report_len);
    let count = value(d, PAGE_DIGITIZER, 0, USAGE_CONTACT_COUNT, report).unwrap_or(0);
    let mut slots = Vec::with_capacity(d.collections as usize);
    for coll in 1..=d.collections {
        let mut usages = [0u16; 8];
        let mut ulen: u32 = usages.len() as u32;
        let report_mut = std::slice::from_raw_parts_mut(report_ptr as *mut u8, report_len);
        let st = HidP_GetUsages(HidP_Input, PAGE_DIGITIZER, Some(coll), usages.as_mut_ptr(), &mut ulen, pp(d), report_mut);
        if st != HIDP_STATUS_SUCCESS {
            continue;
        }
        let down = &usages[..ulen as usize];
        let (Some(x), Some(y)) = (value(d, PAGE_GENERIC, coll, USAGE_X, report), value(d, PAGE_GENERIC, coll, USAGE_Y, report))
        else {
            continue; // not a finger collection
        };
        slots.push(PtpContact {
            id: value(d, PAGE_DIGITIZER, coll, USAGE_CONTACT_ID, report).unwrap_or(coll as u32),
            x: x as i32,
            y: y as i32,
            tip: down.contains(&USAGE_TIP_SWITCH),
            confidence: d.has_confidence.then(|| down.contains(&USAGE_CONFIDENCE)),
            width: value(d, PAGE_DIGITIZER, coll, USAGE_WIDTH, report).map(|v| v as i32),
            height: value(d, PAGE_DIGITIZER, coll, USAGE_HEIGHT, report).map(|v| v as i32),
        });
    }
    (slots, count)
}

// ── WM_INPUT / WM_TIMER handling ─────────────────────────────────────────────

/// Hand decisions to the sink (outside the pipeline lock).
fn deliver(decisions: Vec<super::guard::Decision>) {
    if decisions.is_empty() {
        return;
    }
    if let Some(sink) = SINK.lock().as_ref() {
        for d in decisions {
            sink(d);
        }
    }
}

/// Arm the settle ticker when a tap is pending; the timer disarms itself.
unsafe fn ensure_ticking(hwnd: HWND) {
    if !TICKING.swap(true, Ordering::SeqCst) {
        SetTimer(Some(hwnd), TICK_TIMER, TICK_MS, None);
    }
}

unsafe fn on_tick(hwnd: HWND) {
    let Some(now) = now_ms() else { return };
    let (decisions, pending) = {
        let mut guard = PIPELINE.lock();
        let Some(p) = guard.as_mut() else { return };
        (p.tick(now), p.needs_tick())
    };
    if !pending {
        let _ = KillTimer(Some(hwnd), TICK_TIMER);
        TICKING.store(false, Ordering::SeqCst);
    }
    deliver(decisions);
}

/// One complete frame of one device through the pipeline.
fn feed_frame(index: u32, touches: Vec<super::trace::Touch>, hwnd: HWND) {
    let Some(t_ms) = now_ms() else { return };
    super::record_frame(t_ms, || index, &touches);
    let frame = Frame { t_ms, device: index, touches };
    let (decisions, pending) = {
        let mut guard = PIPELINE.lock();
        let Some(p) = guard.as_mut() else { return };
        p.set_complete_keys(crate::auto_expand::tap_live());
        let d = p.feed(&frame);
        if super::live::watched() {
            super::live::set_frame(index, p.live_touches(&frame));
        }
        (d, p.needs_tick())
    };
    if pending {
        unsafe { ensure_ticking(hwnd) };
    }
    for d in &decisions {
        if let Some(ev) = d.event {
            tracing::info!("gestures(win): recognised {:?} ({} finger(s)) t_ms={t_ms}", ev.kind, ev.fingers);
        }
    }
    deliver(decisions);
}

unsafe fn handle_input(hwnd: HWND, lparam: LPARAM) {
    if !RUNNING.load(Ordering::Relaxed) {
        return;
    }
    let hraw = HRAWINPUT(lparam.0 as *mut c_void);
    let header_size = std::mem::size_of::<RAWINPUTHEADER>() as u32;
    let mut size: u32 = 0;
    if GetRawInputData(hraw, RID_INPUT, None, &mut size, header_size) == u32::MAX || size == 0 {
        return;
    }
    let mut buf = vec![0u8; size as usize];
    let read = GetRawInputData(hraw, RID_INPUT, Some(buf.as_mut_ptr() as *mut c_void), &mut size, header_size);
    if read == u32::MAX || read == 0 {
        return;
    }
    let raw = &*(buf.as_ptr() as *const RAWINPUT);
    if raw.header.dwType != RIM_TYPEHID.0 {
        return;
    }
    let hdevice = raw.header.hDevice;
    let dev_key = hdevice.0 as isize;
    let hid = &raw.data.hid;
    let report_size = hid.dwSizeHid as usize;
    let count = hid.dwCount as usize;
    if report_size == 0 || count == 0 {
        return;
    }
    let base = hid.bRawData.as_ptr();

    // Parse and assemble under the device lock; feed the pipeline outside it.
    let mut frames = Vec::new();
    {
        let mut map = devices().lock();
        if !map.contains_key(&dev_key) {
            let index = map.len() as u32;
            let Some(d) = resolve_device(hdevice, index) else { return };
            let info = DeviceInfo { builtin: None, width_mm: d.geometry.width_mm, height_mm: d.geometry.height_mm };
            tracing::info!("gestures(win): touchpad {index}: {info:?}, confidence bit: {}", d.has_confidence);
            let mut cache = DEVICE_CACHE.lock();
            cache.push(info);
            if let Some(p) = PIPELINE.lock().as_mut() {
                p.set_devices(cache.clone());
            }
            map.insert(dev_key, d);
        }
        let Some(d) = map.get_mut(&dev_key) else { return };
        let t_ms = now_ms().unwrap_or(0);
        for i in 0..count {
            let (slots, n) = parse_report(d, base.add(i * report_size), report_size);
            if let Some(contacts) = d.assembler.push(&slots, n) {
                let geometry = d.geometry;
                frames.push((d.index, d.tracker.frame(t_ms, &contacts, &geometry)));
            }
        }
    }
    for (index, touches) in frames {
        feed_frame(index, touches, hwnd);
    }
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_INPUT => {
            handle_input(hwnd, lparam);
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TICK_TIMER => {
            on_tick(hwnd);
            LRESULT(0)
        }
        WM_GESTURE_QUIT => {
            let _ = KillTimer(Some(hwnd), TICK_TIMER);
            TICKING.store(false, Ordering::SeqCst);
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            windows::Win32::UI::WindowsAndMessaging::PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ── Message-loop thread ──────────────────────────────────────────────────────

unsafe fn run_message_loop() -> windows::core::Result<()> {
    let hinstance = GetModuleHandleW(None)?;
    let class_name = windows::core::w!("InspectorRustGestureWnd");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance.into(),
        lpszClassName: class_name,
        ..Default::default()
    };
    RegisterClassW(&wc);

    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class_name,
        windows::core::w!("InspectorRustGesture"),
        WINDOW_STYLE(0),
        0,
        0,
        0,
        0,
        Some(HWND_MESSAGE),
        None,
        Some(hinstance.into()),
        None,
    )?;
    THREAD_HWND.store(hwnd.0 as isize, Ordering::SeqCst);

    // Raw Input for the Precision-Touchpad digitizer, INPUTSINK so frames
    // arrive without focus, targeted at our message window.
    let rid = RAWINPUTDEVICE {
        usUsagePage: PAGE_DIGITIZER,
        usUsage: 0x05, // Touch Pad
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    };
    RegisterRawInputDevices(&[rid], std::mem::size_of::<RAWINPUTDEVICE>() as u32)?;

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
    THREAD_HWND.store(0, Ordering::SeqCst);
    Ok(())
}

// ── Source ───────────────────────────────────────────────────────────────────

pub struct WindowsGestureSource;

impl WindowsGestureSource {
    pub fn new() -> Self {
        WindowsGestureSource
    }
}

impl GestureSource for WindowsGestureSource {
    fn start(&mut self, cfg: GestureConfig, sink: GestureSink) -> Result<(), String> {
        *SINK.lock() = Some(sink);
        if RUNNING.swap(true, Ordering::SeqCst) {
            return Ok(()); // already running — the sink above was swapped in place
        }
        let _ = START.set(Instant::now());
        devices().lock().clear();
        DEVICE_CACHE.lock().clear();
        *PIPELINE.lock() = Some(
            Pipeline::new(cfg, Vec::new(), false)
                .with_mute(false, Some(super::output_muted_now))
                .with_live_bindings(Some(super::frontmost_app_id)),
        );

        // The Raw Input window needs its own thread with a message loop.
        std::thread::spawn(|| {
            if let Err(e) = unsafe { run_message_loop() } {
                tracing::warn!("gesture raw-input loop failed: {e}");
                RUNNING.store(false, Ordering::SeqCst);
            }
        });
        Ok(())
    }

    fn stop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
        let hwnd = THREAD_HWND.load(Ordering::SeqCst);
        if hwnd != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut c_void)), WM_GESTURE_QUIT, WPARAM(0), LPARAM(0));
            }
        }
        *SINK.lock() = None;
        *PIPELINE.lock() = None;
        devices().lock().clear();
        DEVICE_CACHE.lock().clear();
    }
}
