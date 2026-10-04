//! One shared window source for the focus mode AND stage 2 (no double
//! queries): the focused window comes from Accessibility, event-driven via an
//! `AXObserver` on the frontmost app (focus change, move, resize) on its own
//! run-loop thread. The window list for the retro frames comes from
//! `CGWindowListCopyWindowInfo` (front-to-back, bounds + titles) and is
//! refreshed on the same AX events, at most every 250 ms.
//!
//! Not frame-rate polling: the thread sleeps in `CFRunLoopRunInMode` and wakes
//! on AX notifications; the only timer is a 0.5 s check for a new frontmost
//! app (that switch has no AX notification of its own).
//!
//! Without the Accessibility grant the focus stays unknown — the overlay
//! renders everything with the background cell size; the panel says why.

use std::ffi::{c_void, CStr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_core_foundation::{CGPoint, CGSize};
use objc2_foundation::NSString;

use super::frame::WinInfo;
use super::Rect;

type CFTypeRef = *const c_void;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    // Signatures match the declarations in text_field / tracking (same symbols).
    fn AXUIElementCreateApplication(pid: i32) -> *const c_void;
    fn AXUIElementCopyAttributeValue(e: *const c_void, attr: CFTypeRef, out: *mut CFTypeRef) -> i32;
    fn AXObserverCreate(pid: i32, cb: AxCallback, out: *mut *mut c_void) -> i32;
    fn AXObserverAddNotification(obs: *mut c_void, e: *const c_void, name: CFTypeRef, refcon: *mut c_void) -> i32;
    fn AXObserverGetRunLoopSource(obs: *mut c_void) -> *mut c_void;
    fn AXValueGetValue(v: CFTypeRef, ty: u32, out: *mut c_void) -> u8;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, src: *mut c_void, mode: CFTypeRef);
    fn CFRunLoopRemoveSource(rl: *mut c_void, src: *mut c_void, mode: CFTypeRef);
    fn CFRunLoopRunInMode(mode: CFTypeRef, secs: f64, return_after_source: bool) -> i32;
    fn CFRelease(v: CFTypeRef);
    static kCFRunLoopDefaultMode: CFTypeRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> CFTypeRef;
}

type AxCallback = extern "C" fn(*mut c_void, *mut c_void, CFTypeRef, *mut c_void);

/// `kCFRunLoopRunFinished` — the run loop had no sources to wait on.
const RUN_FINISHED: i32 = 1;
const AX_POINT: u32 = 1;
const AX_SIZE: u32 = 2;

static RUN: AtomicBool = AtomicBool::new(false);
static DIRTY: AtomicBool = AtomicBool::new(false);
static THREAD: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

extern "C" fn on_ax(_o: *mut c_void, _e: *mut c_void, _n: CFTypeRef, _r: *mut c_void) {
    DIRTY.store(true, Ordering::SeqCst);
}

pub fn accessibility_granted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

fn cf(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

fn cfp(s: &NSString) -> CFTypeRef {
    s as *const NSString as CFTypeRef
}

unsafe fn copy_attr(e: *const c_void, name: &str) -> Option<CFTypeRef> {
    let key = cf(name);
    let mut out: CFTypeRef = std::ptr::null();
    (AXUIElementCopyAttributeValue(e, cfp(&key), &mut out) == 0 && !out.is_null()).then_some(out)
}

unsafe fn frontmost_pid() -> i32 {
    let Some(ws) = AnyClass::get(c"NSWorkspace") else { return -1 };
    let shared: *mut AnyObject = msg_send![ws, sharedWorkspace];
    let app: *mut AnyObject = msg_send![shared, frontmostApplication];
    if app.is_null() {
        return -1;
    }
    msg_send![app, processIdentifier]
}

/// Focused window of `app` in global top-left points + full-screen flag.
unsafe fn focused_window(app: *const c_void) -> Option<(Rect, bool)> {
    let win = copy_attr(app, "AXFocusedWindow")?;
    let mut pos = CGPoint { x: 0.0, y: 0.0 };
    let mut size = CGSize { width: 0.0, height: 0.0 };
    let ok = (|| {
        let p = copy_attr(win, "AXPosition")?;
        let got_p = AXValueGetValue(p, AX_POINT, &mut pos as *mut _ as *mut c_void) != 0;
        CFRelease(p);
        let s = copy_attr(win, "AXSize")?;
        let got_s = AXValueGetValue(s, AX_SIZE, &mut size as *mut _ as *mut c_void) != 0;
        CFRelease(s);
        (got_p && got_s).then_some(())
    })();
    let full = copy_attr(win, "AXFullScreen")
        .map(|v| {
            let b: bool = msg_send![v as *const AnyObject, boolValue];
            CFRelease(v);
            b
        })
        .unwrap_or(false);
    CFRelease(win);
    ok?;
    Some((Rect { x: pos.x as f32, y: pos.y as f32, w: size.width as f32, h: size.height as f32 }, full))
}

unsafe fn num(dict: *mut AnyObject, key: &str) -> Option<f64> {
    let k = cf(key);
    let v: *mut AnyObject = msg_send![dict, objectForKey: &*k];
    (!v.is_null()).then(|| msg_send![v, doubleValue])
}

/// On-screen normal windows of other apps, front to back.
unsafe fn window_list(focused_pid: i32) -> Vec<WinInfo> {
    // kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements
    let arr = CGWindowListCopyWindowInfo(1 | 16, 0) as *mut AnyObject;
    if arr.is_null() {
        return Vec::new();
    }
    let me = std::process::id() as f64;
    let n: usize = msg_send![arr, count];
    let mut out = Vec::new();
    let mut first_of_focused = true;
    for i in 0..n {
        let d: *mut AnyObject = msg_send![arr, objectAtIndex: i];
        if num(d, "kCGWindowLayer") != Some(0.0) || num(d, "kCGWindowOwnerPID") == Some(me) {
            continue;
        }
        if num(d, "kCGWindowAlpha").unwrap_or(1.0) <= 0.01 {
            continue;
        }
        let bk = cf("kCGWindowBounds");
        let b: *mut AnyObject = msg_send![d, objectForKey: &*bk];
        if b.is_null() {
            continue;
        }
        let (Some(x), Some(y), Some(w), Some(h)) = (num(b, "X"), num(b, "Y"), num(b, "Width"), num(b, "Height")) else {
            continue;
        };
        if w < 40.0 || h < 30.0 {
            continue;
        }
        let nk = cf("kCGWindowName");
        let name: *mut AnyObject = msg_send![d, objectForKey: &*nk];
        let title = if name.is_null() {
            String::new()
        } else {
            let c: *const std::os::raw::c_char = msg_send![name, UTF8String];
            if c.is_null() { String::new() } else { CStr::from_ptr(c).to_string_lossy().into_owned() }
        };
        let pid = num(d, "kCGWindowOwnerPID").unwrap_or(-1.0) as i32;
        let focused = pid == focused_pid && first_of_focused;
        if focused {
            first_of_focused = false;
        }
        out.push(WinInfo { rect: Rect { x: x as f32, y: y as f32, w: w as f32, h: h as f32 }, title, focused });
        if out.len() >= super::frame::MAX_WINDOWS {
            break;
        }
    }
    CFRelease(arr as CFTypeRef);
    out
}

pub fn start() {
    if RUN.swap(true, Ordering::SeqCst) {
        return;
    }
    let h = std::thread::Builder::new()
        .name("ir-retro-focus".into())
        .spawn(|| unsafe { run() })
        .ok();
    *THREAD.lock().unwrap_or_else(|e| e.into_inner()) = h;
}

pub fn stop() {
    RUN.store(false, Ordering::SeqCst);
    if let Some(h) = THREAD.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = h.join();
    }
}

unsafe fn run() {
    let rl = CFRunLoopGetCurrent();
    let mut cur_pid = -1;
    let mut app: *const c_void = std::ptr::null();
    let mut observer: *mut c_void = std::ptr::null_mut();
    let mut source: *mut c_void = std::ptr::null_mut();
    let mut last_focus: Option<(Rect, bool)> = None;
    let mut last_list = Instant::now() - Duration::from_secs(10);
    let mut windows: Vec<WinInfo> = Vec::new();
    let me = std::process::id() as i32;
    while RUN.load(Ordering::SeqCst) {
        let (want_focus, want_frames) = super::macos::wants_focus_data();
        let pid = frontmost_pid();
        // Our own popup in front: keep the last focus (don't jump to us).
        if pid > 0 && pid != me && pid != cur_pid {
            if !source.is_null() {
                CFRunLoopRemoveSource(rl, source, kCFRunLoopDefaultMode);
            }
            if !observer.is_null() {
                CFRelease(observer);
                observer = std::ptr::null_mut();
            }
            if !app.is_null() {
                CFRelease(app);
            }
            cur_pid = pid;
            app = AXUIElementCreateApplication(pid);
            source = std::ptr::null_mut();
            if accessibility_granted() && AXObserverCreate(pid, on_ax, &mut observer) == 0 && !observer.is_null() {
                for n in ["AXFocusedWindowChanged", "AXMainWindowChanged", "AXWindowMoved", "AXWindowResized", "AXMoved", "AXResized"] {
                    let s = cf(n);
                    AXObserverAddNotification(observer, app, cfp(&s), std::ptr::null_mut());
                }
                source = AXObserverGetRunLoopSource(observer);
                if !source.is_null() {
                    CFRunLoopAddSource(rl, source, kCFRunLoopDefaultMode);
                }
            }
            DIRTY.store(true, Ordering::SeqCst);
        }
        // AX events mark the state dirty; the window list (titles, moves of
        // background windows) is additionally refreshed once a second.
        let periodic = want_frames && last_list.elapsed() >= Duration::from_secs(1);
        if DIRTY.swap(false, Ordering::SeqCst) || periodic {
            if want_focus && accessibility_granted() && !app.is_null() {
                last_focus = focused_window(app);
            } else if !want_focus {
                last_focus = None;
            }
            if want_frames && last_list.elapsed() >= Duration::from_millis(250) {
                last_list = Instant::now();
                windows = window_list(cur_pid);
            } else if !want_frames {
                windows.clear();
            }
            let (focus, full) = match last_focus {
                Some((r, f)) => (Some(r), f),
                None => (None, false),
            };
            super::macos::set_live(focus, full, windows.clone());
        }
        // ⚠️ Without an AX observer the run loop has no source and
        // RunInMode returns kCFRunLoopRunFinished (1) IMMEDIATELY — that was
        // a busy loop at 100 % CPU (found by sampling the live app).
        if CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.5, true) == RUN_FINISHED {
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    if !source.is_null() {
        CFRunLoopRemoveSource(rl, source, kCFRunLoopDefaultMode);
    }
    if !observer.is_null() {
        CFRelease(observer);
    }
    if !app.is_null() {
        CFRelease(app);
    }
}
