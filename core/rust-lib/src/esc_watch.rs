//! Unfocused-popup Escape watcher (macOS, v0.88.1).
//!
//! When the popup is visible on screen but another app holds keyboard focus,
//! the webview receives no key events, so Esc couldn't close it. This module
//! arms an active `CGEventTap` that watches for the Escape keycode, hides the
//! popup, and **consumes** that dismissing Esc (v0.94.0) so it never leaks to
//! the focused app underneath — it was meant to close the overlay, not to also
//! cancel a dialog / exit an editor mode / deselect in whatever app is behind
//! it. Because the arm is **one-shot** (it disarms after the first Esc), only
//! that dismissing Esc is swallowed; every later Esc flows to the focused app
//! as usual.
//!
//! Armed while: popup **visible + unfocused**, in BOTH `close_on_blur` states
//! (v0.94.0 — the guarantee "if IR is on screen, Esc kills IR, not the app
//! behind it" must not depend on the setting). With `close_on_blur` ON the
//! popup usually also auto-hides on blur (which disarms this again via
//! `hide_popup`), so the watcher only bites while the popup LINGERS visible —
//! the post-show grace window, or a modal keeping it open. With `close_on_blur`
//! OFF the popup stays open unfocused, which is its whole point. Disarmed on
//! focus-regain and on hide, so it's NEVER armed while the popup is focused
//! (the webview keeps its own Esc semantics then). The tap thread is created
//! lazily on the first arm and stays alive, gated by an `ARMED` atomic — arming
//! after that is a store, never an FFI call.
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
/// Active (default) tap: we can SWALLOW the dismissing Esc by returning null
/// from the callback, so it never reaches the app underneath. Every other
/// event is passed through unchanged.
const TAP_OPTION_DEFAULT: u32 = 0;
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
                // Consume this Escape: it dismissed OUR popup, so the focused
                // app underneath must not also receive it. Returning null drops
                // the event. (One-shot arm ⇒ every later Esc still flows on.)
                return std::ptr::null_mut();
            }
        }
        _ => {}
    }
    // Everything we don't act on flows through unchanged.
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
                TAP_OPTION_DEFAULT,
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
