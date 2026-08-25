//! Unfocused-popup Escape watcher (macOS, v0.88.1).
//!
//! When the popup is visible on screen but another app holds keyboard focus,
//! the webview receives no key events, so Esc couldn't close it. This module
//! arms an active `CGEventTap` that watches for the Escape keycode, dismisses
//! the popup **via the frontend** (`popup-dismiss` event → the `hidePopup`
//! wrapper, so the CRT power-off animation plays like every other close —
//! v0.126.1; the old direct `hide_popup` snapped the overlay away without it;
//! a 1.2 s native fallback still hides a wedged webview), and **consumes**
//! that dismissing Esc (v0.94.0) so it never leaks to
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
//! **Enter-to-paste while lingering unfocused (v0.98.1).** In `close_on_blur`
//! OFF mode the popup stays visible but unfocused on purpose — so the webview
//! also can't see the **Enter** that would paste the selected clip/snippet. When
//! armed with `watch_enter = true` (only in that mode) the same tap also catches
//! Return / keypad-Enter, emits `popup-activate-selected` to the frontend (which
//! activates the highlighted row — pasting a clip/snippet, which itself hides the
//! popup), and **consumes** the key so it doesn't also land in the app behind.
//! Like the Esc path it's one-shot per arm; because a clip/snippet paste closes
//! the popup, the very next keystroke sees a dismissed overlay, so a stray Enter
//! can't be hijacked repeatedly. In `close_on_blur` ON mode `watch_enter` is
//! false (the popup auto-hides on blur — there's no lingering state to serve, and
//! Enter during the brief post-show grace must never be swallowed).
//!
//! Uses the same raw-FFI pattern as `input_lock.rs` / `gestures`; needs the
//! Accessibility grant the expander already holds. Without the grant the tap
//! creation fails silently → the feature degrades (popup closes via the
//! toggle hotkey), never breaks anything else.

#![cfg(target_os = "macos")]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::OnceLock;

use tauri::{AppHandle, Emitter, Manager};

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
const KEYCODE_RETURN: i64 = 36;
const KEYCODE_KEYPAD_ENTER: i64 = 76;

static ARMED: AtomicBool = AtomicBool::new(false);
/// Whether the armed tap should ALSO treat Enter as "activate the selected row"
/// (only in `close_on_blur` OFF mode — see the module doc).
static WATCH_ENTER: AtomicBool = AtomicBool::new(false);
static THREAD_STARTED: AtomicBool = AtomicBool::new(false);
static TAP_PORT: AtomicIsize = AtomicIsize::new(0);
static APP: OnceLock<AppHandle> = OnceLock::new();

/// What an armed tap should do with a key-down (pure decision, unit-tested).
#[derive(Debug, PartialEq, Eq)]
enum KeyAction {
    /// Pass the key through unchanged.
    Ignore,
    /// Escape → hide the popup + consume the key.
    Dismiss,
    /// Enter (linger mode) → activate the selected row + consume the key.
    ActivateSelected,
}

/// Decide what to do with a key-down given the arm state. Escape always
/// dismisses while armed; Enter/keypad-Enter only activates when `watch_enter`
/// (close_on_blur OFF); everything else passes through.
fn classify_key(code: i64, armed: bool, watch_enter: bool) -> KeyAction {
    if !armed {
        return KeyAction::Ignore;
    }
    if code == KEYCODE_ESCAPE {
        return KeyAction::Dismiss;
    }
    if watch_enter && (code == KEYCODE_RETURN || code == KEYCODE_KEYPAD_ENTER) {
        return KeyAction::ActivateSelected;
    }
    KeyAction::Ignore
}

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
            match classify_key(code, true, WATCH_ENTER.load(Ordering::Relaxed)) {
                KeyAction::Dismiss => {
                    // One shot per arm; act off the tap thread (keep the
                    // callback cheap — a slow callback gets the tap disabled).
                    ARMED.store(false, Ordering::SeqCst);
                    std::thread::spawn(|| {
                        if let Some(app) = APP.get() {
                            tracing::info!(
                                "esc-watch: Escape while unfocused — dismissing via frontend"
                            );
                            // Route through the frontend (like the Enter path
                            // below) so the dismiss plays the CRT power-off —
                            // the old direct `hide_popup` snapped the overlay
                            // away without the signature animation. WAAPI runs
                            // fine in a visible-but-unfocused webview.
                            let _ = app.emit("popup-dismiss", ());
                            // Fallback: a wedged webview must not leave a
                            // zombie overlay. The CRT power-off maxes out well
                            // under a second (900 ms cap × 0.76 + margin), so
                            // if the popup is still visible after 1.2 s the
                            // frontend never acted — hide natively.
                            std::thread::sleep(std::time::Duration::from_millis(1200));
                            let still_visible = app
                                .get_webview_window(crate::hotkey::POPUP_LABEL)
                                .is_some_and(|w| w.is_visible().unwrap_or(false));
                            if still_visible {
                                tracing::warn!(
                                    "esc-watch: frontend dismiss didn't hide — native fallback"
                                );
                                crate::hotkey::hide_popup(app);
                            }
                        }
                    });
                    // Consume this Escape: it dismissed OUR popup, so the focused
                    // app underneath must not also receive it. Returning null drops
                    // the event. (One-shot arm ⇒ every later Esc still flows on.)
                    return std::ptr::null_mut();
                }
                KeyAction::ActivateSelected => {
                    // Enter while the popup lingers unfocused (close_on_blur OFF):
                    // paste the SELECTED clip/snippet. One-shot like Esc; emit off
                    // the tap thread so the callback stays cheap. The frontend's
                    // `activate(selected)` pastes via the normal path, which hides
                    // the popup — so the overlay closes like Enter-while-focused.
                    ARMED.store(false, Ordering::SeqCst);
                    std::thread::spawn(|| {
                        if let Some(app) = APP.get() {
                            tracing::info!(
                                "esc-watch: Enter while unfocused — activating selected row"
                            );
                            let _ = app.emit("popup-activate-selected", ());
                        }
                    });
                    // Consume it: the user meant to act on OUR overlay, not to
                    // type a newline in the app behind it.
                    return std::ptr::null_mut();
                }
                KeyAction::Ignore => {}
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

/// Arm the watcher: popup is visible but lost focus. Lazily starts the tap
/// thread on first use. `watch_enter` additionally makes Return / keypad-Enter
/// paste the selected row (only in `close_on_blur` OFF mode — see the module
/// doc); pass `false` and only Esc is consumed.
pub fn arm(app: &AppHandle, watch_enter: bool) {
    let _ = APP.set(app.clone());
    ensure_thread();
    WATCH_ENTER.store(watch_enter, Ordering::SeqCst);
    ARMED.store(true, Ordering::SeqCst);
}

/// Disarm (focus regained / popup hidden). The tap stays installed but inert.
pub fn disarm() {
    ARMED.store(false, Ordering::SeqCst);
    WATCH_ENTER.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disarmed_ignores_every_key() {
        for &code in &[KEYCODE_ESCAPE, KEYCODE_RETURN, KEYCODE_KEYPAD_ENTER, 0, 42] {
            assert_eq!(classify_key(code, false, true), KeyAction::Ignore);
            assert_eq!(classify_key(code, false, false), KeyAction::Ignore);
        }
    }

    #[test]
    fn escape_dismisses_while_armed_regardless_of_watch_enter() {
        assert_eq!(classify_key(KEYCODE_ESCAPE, true, false), KeyAction::Dismiss);
        assert_eq!(classify_key(KEYCODE_ESCAPE, true, true), KeyAction::Dismiss);
    }

    #[test]
    fn enter_activates_only_when_watch_enter_is_on() {
        // Return + keypad Enter both activate when watch_enter is on.
        assert_eq!(
            classify_key(KEYCODE_RETURN, true, true),
            KeyAction::ActivateSelected
        );
        assert_eq!(
            classify_key(KEYCODE_KEYPAD_ENTER, true, true),
            KeyAction::ActivateSelected
        );
        // …and are ignored (pass through) when watch_enter is off — so Enter is
        // never swallowed in close_on_blur ON mode / during the post-show grace.
        assert_eq!(classify_key(KEYCODE_RETURN, true, false), KeyAction::Ignore);
        assert_eq!(
            classify_key(KEYCODE_KEYPAD_ENTER, true, false),
            KeyAction::Ignore
        );
    }

    #[test]
    fn other_keys_pass_through_even_when_fully_armed() {
        for &code in &[0, 4, 42, 49 /* space */, 48 /* tab */] {
            assert_eq!(classify_key(code, true, true), KeyAction::Ignore);
        }
    }
}
