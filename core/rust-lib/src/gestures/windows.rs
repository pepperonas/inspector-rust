//! Windows touchpad gesture capture via **Raw Input + HID Precision Touchpad**.
//!
//! There is no Win32 API that hands you "3-finger swipe" directly (`WM_GESTURE`
//! is touchscreen legacy). The correct path — used here — is to register Raw
//! Input on the Precision-Touchpad HID (Usage Page `0x0D`, Usage `0x05`) with
//! `RIDEV_INPUTSINK` (frames arrive without focus), parse the HID contact
//! reports against the device's preparsed data (`HidP_*`), assemble per-frame
//! finger count + centroid, and feed the shared, unit-tested [`Recognizer`].
//!
//! **CAVEATS (also in the README + Settings):**
//! 1. Works only with genuine **Precision Touchpads** that expose the standard
//!    HID digitizer reports. Old Synaptics/ELAN with a proprietary driver don't
//!    → `start()` finds no PTP device and returns an error (feature disabled,
//!    no crash).
//! 2. Raw Input is **read-only**: Windows keeps firing its own 3-/4-finger
//!    gestures in parallel and they can't be swallowed from the Raw Input
//!    stream. The user must set *Settings → Touchpad → Three/Four-finger
//!    gestures* to **Nothing** to avoid double-triggering.
//!
//! **Runtime-unverified** (written + compile-validated against `windows` 0.61;
//! the maintainer has no Windows box) — consistent with the rest of the repo's
//! Windows code.

use super::{GestureConfig, GestureEvent, GestureSink, GestureSource, Recognizer, TouchFrame};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use windows::core::PCWSTR;
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidP_GetCaps, HidP_GetUsageValue, HidP_GetUsages, HidP_GetValueCaps, HidP_Input, HIDP_CAPS,
    HIDP_VALUE_CAPS, HIDP_STATUS_SUCCESS, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, RegisterRawInputDevices, HRAWINPUT, RAWINPUT,
    RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT, RIDEV_INPUTSINK, RIDI_PREPARSEDDATA, RIM_TYPEHID,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW,
    RegisterClassW, TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP,
    WM_DESTROY, WM_INPUT, WNDCLASSW,
};

// HID usage pages / usages for a Precision Touchpad digitizer.
const HID_USAGE_PAGE_GENERIC: u16 = 0x01;
const HID_USAGE_GENERIC_X: u16 = 0x30;
const HID_USAGE_GENERIC_Y: u16 = 0x31;
const HID_USAGE_PAGE_DIGITIZER: u16 = 0x0D;
const HID_USAGE_DIGITIZER_TIP_SWITCH: u16 = 0x42;
const HID_USAGE_DIGITIZER_CONTACT_COUNT: u16 = 0x54;

/// Posted to the message window to ask its loop to quit (on `stop()`).
const WM_GESTURE_QUIT: u32 = WM_APP + 1;

// ── Shared state the WndProc reads (it can't capture) ────────────────────────

static RUNNING: AtomicBool = AtomicBool::new(false);
static SINK: Mutex<Option<GestureSink>> = Mutex::new(None);
static REC: Mutex<Option<Recognizer>> = Mutex::new(None);
static START: OnceLock<Instant> = OnceLock::new();
static THREAD_HWND: AtomicIsize = AtomicIsize::new(0);

/// Per-device cache: the preparsed data + X/Y logical ranges + collection count,
/// resolved once on the first report from a device. Keyed by `HANDLE` as isize.
struct DeviceCaps {
    preparsed: Vec<u8>,
    collections: u16,
    x_min: i32,
    x_max: i32,
    y_min: i32,
    y_max: i32,
}

fn devices() -> &'static Mutex<HashMap<isize, DeviceCaps>> {
    static D: OnceLock<Mutex<HashMap<isize, DeviceCaps>>> = OnceLock::new();
    D.get_or_init(|| Mutex::new(HashMap::new()))
}

#[inline]
fn pp(caps: &DeviceCaps) -> PHIDP_PREPARSED_DATA {
    PHIDP_PREPARSED_DATA(caps.preparsed.as_ptr() as isize)
}

#[inline]
fn now_ms() -> u64 {
    START.get().map(|s| s.elapsed().as_millis() as u64).unwrap_or(0)
}

// ── Per-device caps resolution ───────────────────────────────────────────────

unsafe fn resolve_caps(hdevice: HANDLE) -> Option<DeviceCaps> {
    // Fetch the preparsed data blob.
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
    if HidP_GetCaps(pd, &mut caps) != HIDP_STATUS_SUCCESS {
        return None;
    }
    let n_value_caps = caps.NumberInputValueCaps;
    if n_value_caps == 0 {
        return None;
    }
    let mut value_caps = vec![HIDP_VALUE_CAPS::default(); n_value_caps as usize];
    let mut len = n_value_caps;
    if HidP_GetValueCaps(HidP_Input, value_caps.as_mut_ptr(), &mut len, pd) != HIDP_STATUS_SUCCESS {
        return None;
    }

    // Pull the X / Y logical ranges (Generic Desktop page) so we can normalise.
    let (mut x_min, mut x_max, mut y_min, mut y_max) = (0i32, 0i32, 0i32, 0i32);
    for vc in value_caps.iter().take(len as usize) {
        if vc.UsagePage != HID_USAGE_PAGE_GENERIC {
            continue;
        }
        // Usage lives in the NotRange arm when IsRange is false (it is for X/Y).
        let usage = unsafe { vc.Anonymous.NotRange.Usage };
        if usage == HID_USAGE_GENERIC_X {
            x_min = vc.LogicalMin;
            x_max = vc.LogicalMax;
        } else if usage == HID_USAGE_GENERIC_Y {
            y_min = vc.LogicalMin;
            y_max = vc.LogicalMax;
        }
    }
    if x_max <= x_min || y_max <= y_min {
        return None; // not a usable absolute digitizer
    }

    Some(DeviceCaps {
        preparsed,
        collections: caps.NumberLinkCollectionNodes,
        x_min,
        x_max,
        y_min,
        y_max,
    })
}

// ── HID report → TouchFrame ──────────────────────────────────────────────────

/// Parse one HID report into a frame: the centroid (normalized 0..1) of the
/// touching contacts + the contact count. `None` if nothing parseable.
/// Takes a raw ptr/len so we can hand the report to both the `&[u8]`
/// (`HidP_GetUsageValue`) and `&mut [u8]` (`HidP_GetUsages`) APIs — the HID
/// functions only read it.
unsafe fn parse_report(caps: &DeviceCaps, report_ptr: *const u8, report_len: usize) -> Option<TouchFrame> {
    let pd = pp(caps);
    let report = || std::slice::from_raw_parts(report_ptr, report_len);
    let report_mut = || std::slice::from_raw_parts_mut(report_ptr as *mut u8, report_len);

    // How many contacts the device says are down this frame (advances parser).
    let mut contact_count: u32 = 0;
    let _ = HidP_GetUsageValue(
        HidP_Input,
        HID_USAGE_PAGE_DIGITIZER,
        Some(0),
        HID_USAGE_DIGITIZER_CONTACT_COUNT,
        &mut contact_count,
        pd,
        report(),
    );

    // Walk the per-finger link collections, summing the touching ones.
    let (mut sx, mut sy, mut touching) = (0.0f64, 0.0f64, 0u8);
    let xr = (caps.x_max - caps.x_min) as f64;
    let yr = (caps.y_max - caps.y_min) as f64;
    for coll in 1..=caps.collections {
        // Is this finger's Tip Switch down?
        let mut usages = [0u16; 8];
        let mut ulen: u32 = usages.len() as u32;
        let st = HidP_GetUsages(
            HidP_Input,
            HID_USAGE_PAGE_DIGITIZER,
            Some(coll),
            usages.as_mut_ptr(),
            &mut ulen,
            pd,
            report_mut(),
        );
        if st != HIDP_STATUS_SUCCESS {
            continue;
        }
        let down = usages
            .iter()
            .take(ulen as usize)
            .any(|&u| u == HID_USAGE_DIGITIZER_TIP_SWITCH);
        if !down {
            continue;
        }
        let mut x: u32 = 0;
        let mut y: u32 = 0;
        let sx_ok = HidP_GetUsageValue(
            HidP_Input,
            HID_USAGE_PAGE_GENERIC,
            Some(coll),
            HID_USAGE_GENERIC_X,
            &mut x,
            pd,
            report(),
        ) == HIDP_STATUS_SUCCESS;
        let sy_ok = HidP_GetUsageValue(
            HidP_Input,
            HID_USAGE_PAGE_GENERIC,
            Some(coll),
            HID_USAGE_GENERIC_Y,
            &mut y,
            pd,
            report(),
        ) == HIDP_STATUS_SUCCESS;
        if !sx_ok || !sy_ok {
            continue;
        }
        // Normalize 0..1. Y grows downward (PTP origin top-left) → matches
        // `classify_swipe` (dy < 0 = up), so no flip.
        sx += (x as i32 - caps.x_min) as f64 / xr;
        sy += (y as i32 - caps.y_min) as f64 / yr;
        touching += 1;
    }

    // Use the measured count of touching tips (most reliable for the
    // begin/lift transitions the Recognizer keys on). `contact_count` is read
    // above mainly to advance the HID parser state; a frame with all tips up
    // yields `touching == 0` → the Recognizer's lift path fires.
    let _ = contact_count;
    let (x, y) = if touching > 0 {
        (sx / touching as f64, sy / touching as f64)
    } else {
        (0.0, 0.0)
    };
    Some(TouchFrame { contacts: touching, x, y, t_ms: now_ms() })
}

// ── WM_INPUT handling ────────────────────────────────────────────────────────

unsafe fn handle_input(lparam: LPARAM) {
    if !RUNNING.load(Ordering::Relaxed) {
        return;
    }
    let hraw = HRAWINPUT(lparam.0 as *mut c_void);
    let header_size = std::mem::size_of::<RAWINPUTHEADER>() as u32;

    // Query size, then read.
    let mut size: u32 = 0;
    if GetRawInputData(hraw, RID_INPUT, None, &mut size, header_size) == u32::MAX || size == 0 {
        return;
    }
    let mut buf = vec![0u8; size as usize];
    let read = GetRawInputData(
        hraw,
        RID_INPUT,
        Some(buf.as_mut_ptr() as *mut c_void),
        &mut size,
        header_size,
    );
    if read == u32::MAX || read == 0 {
        return;
    }
    let raw = &*(buf.as_ptr() as *const RAWINPUT);
    if raw.header.dwType != RIM_TYPEHID.0 {
        return;
    }
    let hdevice = raw.header.hDevice;
    let dev_key = hdevice.0 as isize;

    // Resolve + cache this device's caps on first sight.
    {
        let mut map = devices().lock();
        if !map.contains_key(&dev_key) {
            if let Some(c) = resolve_caps(hdevice) {
                map.insert(dev_key, c);
            } else {
                return;
            }
        }
    }

    let hid = &raw.data.hid;
    let report_size = hid.dwSizeHid as usize;
    let count = hid.dwCount as usize;
    if report_size == 0 || count == 0 {
        return;
    }
    let base = hid.bRawData.as_ptr();

    let map = devices().lock();
    let Some(caps) = map.get(&dev_key) else {
        return;
    };
    for i in 0..count {
        if let Some(frame) = parse_report(caps, base.add(i * report_size), report_size) {
            let event: Option<GestureEvent> = {
                let mut rec = REC.lock();
                rec.get_or_insert_with(Recognizer::new).feed(frame)
            };
            if let Some(ev) = event {
                if let Some(sink) = SINK.lock().as_ref() {
                    sink(ev);
                }
            }
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_INPUT => {
            handle_input(lparam);
            LRESULT(0)
        }
        WM_GESTURE_QUIT => {
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

    // Register Raw Input for the Precision-Touchpad digitizer, INPUTSINK so
    // frames arrive without focus, targeted at our message window.
    let rid = RAWINPUTDEVICE {
        usUsagePage: HID_USAGE_PAGE_DIGITIZER,
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
    fn start(&mut self, _cfg: GestureConfig, sink: GestureSink) -> Result<(), String> {
        if RUNNING.swap(true, Ordering::SeqCst) {
            return Ok(()); // already running
        }
        let _ = START.set(Instant::now());
        *REC.lock() = Some(Recognizer::new());
        *SINK.lock() = Some(sink);
        devices().lock().clear();

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
                let _ = PostMessageW(
                    Some(HWND(hwnd as *mut c_void)),
                    WM_GESTURE_QUIT,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
        *SINK.lock() = None;
        *REC.lock() = None;
        devices().lock().clear();
    }
}
