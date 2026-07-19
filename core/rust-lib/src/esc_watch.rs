//! Listen-only Escape watcher (macOS, v0.88.1).
//!
//! When the user disables "click outside closes the popup"
//! (`popup.close_on_blur = false`), the overlay stays open while focus lives
//! elsewhere — and the webview then receives no key events, so Esc couldn't
//! close it. This module arms a **listen-only** `CGEventTap` (it never
//! consumes the keypress — the focused app still gets its Esc) that watches
//! for the Escape keycode and hides the popup.
//!
//! Armed ONLY while: popup visible + unfocused + close-on-blur disabled.
//! Disarmed on focus-regain and on hide. The tap thread is created lazily on
//! the first arm and stays alive, gated by an `ARMED` atomic — arming after
//! that is a store, never an FFI call (the global-shortcut-mutex deadlock
//! lesson from the record overlay doesn't apply here, but cheap is cheap).
//!
//! Uses the same raw-FFI pattern as `input_lock.rs` / `gestures`; needs the
//! Accessibility grant the expander already holds. Without the grant the tap
//! creation fails silently → the feature degrades (popup closes via the
//! toggle hotkey), never breaks anything else.

#![cfg(target_os = "macos")]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::OnceLock;

use tauri::AppHandle;

type CGEventRef = *mut c_void;
type CFMachPortRef = *mut c_void;
type CFAllocatorRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFRunLoopRef = *mut c_void;
type CFStringRef = *const c_void;
type CGEventMask = u64;
type CGEventTapProxy = *mut c_void;

type TapCallback = extern "C" fn(
    proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: CGEventMask,
        callback: TapCallback,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFMachPortCreateRunLoopSource(
        allocator: CFAllocatorRef,
        port: CFMachPortRef,
        order: isize,
    ) -> CFRunLoopSourceRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopRun();
    static kCFRunLoopCommonModes: CFStringRef;
}

const SESSION_EVENT_TAP: u32 = 1;
const HEAD_INSERT_EVENT_TAP: u32 = 0;
/// Listen-only: we OBSERVE Esc, we never swallow it.
const TAP_OPTION_LISTEN_ONLY: u32 = 1;
const EVT_KEY_DOWN: u32 = 10;
const EVT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const EVT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
const KEYBOARD_EVENT_KEYCODE: u32 = 9;
const KEYCODE_ESCAPE: i64 = 53;

static ARMED: AtomicBool = AtomicBool::new(false);
static THREAD_STARTED: AtomicBool = AtomicBool::new(false);
static TAP_PORT: AtomicIsize = AtomicIsize::new(0);
static APP: OnceLock<AppHandle> = OnceLock::new();

extern "C" fn tap_callback(
    _proxy: CGEventTapProxy,
    event_type: u32,
    event: CGEventRef,
    _user_info: *mut c_void,
) -> CGEventRef {
    match event_type {
        // The OS disables a tap on timeout/heavy input — re-enable, or the
        // watcher silently dies for the rest of the session.
        EVT_TAP_DISABLED_BY_TIMEOUT | EVT_TAP_DISABLED_BY_USER_INPUT => {
            let port = TAP_PORT.load(Ordering::SeqCst) as CFMachPortRef;
            if !port.is_null() {
                unsafe { CGEventTapEnable(port, true) };
            }
        }
        EVT_KEY_DOWN if ARMED.load(Ordering::Relaxed) => {
            let code = unsafe { CGEventGetIntegerValueField(event, KEYBOARD_EVENT_KEYCODE) };
            if code == KEYCODE_ESCAPE {
                // One shot per arm; hide off the tap thread (keep the
                // callback cheap — a slow callback gets the tap disabled).
                ARMED.store(false, Ordering::SeqCst);
                std::thread::spawn(|| {
                    if let Some(app) = APP.get() {
                        tracing::info!("esc-watch: Escape while unfocused — hiding popup");
                        crate::hotkey::hide_popup(app);
                    }
                });
            }
        }
        _ => {}
    }
    // Listen-only tap: the return value is ignored; the event flows on.
    event
}

fn ensure_thread() {
    if THREAD_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("ir-esc-watch".into())
        .spawn(|| unsafe {
            let mask: CGEventMask = 1u64 << EVT_KEY_DOWN;
            let tap = CGEventTapCreate(
                SESSION_EVENT_TAP,
                HEAD_INSERT_EVENT_TAP,
                TAP_OPTION_LISTEN_ONLY,
                mask,
                tap_callback,
                std::ptr::null_mut(),
            );
            if tap.is_null() {
                tracing::warn!(
                    "esc-watch: CGEventTapCreate failed (Accessibility not granted?) — \
                     Esc-while-unfocused disabled; the popup hotkey still closes"
                );
                THREAD_STARTED.store(false, Ordering::SeqCst);
                return;
            }
            TAP_PORT.store(tap as isize, Ordering::SeqCst);
            let source = CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap, true);
            tracing::info!("esc-watch: listen-only Escape tap installed");
            CFRunLoopRun();
        })
        .ok();
}

/// Arm the watcher: popup is visible but lost focus while click-outside-close
/// is disabled. Lazily starts the tap thread on first use.
pub fn arm(app: &AppHandle) {
    let _ = APP.set(app.clone());
    ensure_thread();
    ARMED.store(true, Ordering::SeqCst);
}

/// Disarm (focus regained / popup hidden). The tap stays installed but inert.
pub fn disarm() {
    ARMED.store(false, Ordering::SeqCst);
}
