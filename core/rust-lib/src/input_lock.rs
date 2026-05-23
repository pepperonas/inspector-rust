//! Input lock — block all keyboard / mouse / trackpad input until a
//! configured chord is pressed. Same idea as `pepperonas/macOS-lock`.
//!
//! Trigger: type `freeze` in the popup search bar (or press Enter on
//! the `freeze` autocomplete row). Unlock: press the chord configured
//! in Settings → Input Lock (default: hold `i`, press `r`).
//!
//! ## macOS implementation
//!
//! **Raw FFI** to `CGEventTapCreate` + `CFRunLoop*` via `#[link]` —
//! we previously tried the `core-graphics` crate's `CGEventTap::new`
//! wrapper but it returned `None` from the closure without actually
//! dropping events on macOS Sonoma (the callback fired with
//! `lock_active=true` but events still reached apps anyway). Raw FFI
//! gives us a real C-ABI `extern "C" fn` whose return value is
//! unambiguously the `CGEventRef` passed back to the OS (or `NULL`
//! to drop) — identical semantics to what `macos-lock.py` uses via
//! PyObjC.
//!
//! The tap is installed on **the main thread's** run loop (the one
//! Tauri's `NSApp.run` is already spinning), not a worker thread —
//! Apple's HID/Session taps need that for reliable event delivery.
//!
//! Requires Accessibility (the existing grant for the text-expander
//! covers it).
//!
//! ## Safety hatch
//!
//! `⌥⌘Esc` (Force Quit) is processed by WindowServer above any
//! user-level event tap — you can always recover.

use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;

// ── Key-name → macOS keycode table ───────────────────────────────────────

/// Parse a key-name string (case-insensitive) into the macOS keycode
/// the event tap compares against. Returns `None` for unknown names.
pub fn key_from_str(name: &str) -> Option<i64> {
    let s = name.trim().to_lowercase();
    match s.as_str() {
        "a" => Some(0), "s" => Some(1), "d" => Some(2), "f" => Some(3),
        "h" => Some(4), "g" => Some(5), "z" => Some(6), "x" => Some(7),
        "c" => Some(8), "v" => Some(9), "b" => Some(11), "q" => Some(12),
        "w" => Some(13), "e" => Some(14), "r" => Some(15), "y" => Some(16),
        "t" => Some(17),
        "0" => Some(29), "1" => Some(18), "2" => Some(19), "3" => Some(20),
        "4" => Some(21), "5" => Some(23), "6" => Some(22), "7" => Some(26),
        "8" => Some(28), "9" => Some(25),
        "o" => Some(31), "u" => Some(32), "i" => Some(34), "p" => Some(35),
        "l" => Some(37), "j" => Some(38), "k" => Some(40),
        "n" => Some(45), "m" => Some(46),
        "space" => Some(49),
        "return" | "enter" => Some(36),
        "tab" => Some(48),
        "escape" | "esc" => Some(53),
        "delete" => Some(51),
        _ => None,
    }
}

// ── Shared state ─────────────────────────────────────────────────────────

static LOCK_ACTIVE: AtomicBool = AtomicBool::new(false);
static TAP_INSTALLED: AtomicBool = AtomicBool::new(false);
static CALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);
static UNLOCK_CODES: OnceLock<Mutex<Vec<i64>>> = OnceLock::new();
static PRESSED: OnceLock<Mutex<HashSet<i64>>> = OnceLock::new();

fn unlock_codes() -> &'static Mutex<Vec<i64>> {
    UNLOCK_CODES.get_or_init(|| Mutex::new(Vec::new()))
}
fn pressed() -> &'static Mutex<HashSet<i64>> {
    PRESSED.get_or_init(|| Mutex::new(HashSet::new()))
}

// ── Public API ───────────────────────────────────────────────────────────

pub fn start_input_lock(unlock_keys: Vec<String>) -> Result<(), String> {
    let codes: Vec<i64> = unlock_keys.iter().filter_map(|s| key_from_str(s)).collect();
    if codes.is_empty() {
        return Err("input lock: unlock chord is empty or unparseable".into());
    }
    *unlock_codes().lock() = codes;
    pressed().lock().clear();

    #[cfg(target_os = "macos")]
    {
        if !TAP_INSTALLED.load(Ordering::SeqCst) {
            macos_impl::install_tap_on_main_runloop()?;
            TAP_INSTALLED.store(true, Ordering::SeqCst);
        }
        LOCK_ACTIVE.store(true, Ordering::SeqCst);
        tracing::info!(
            "input_lock: activated (callback_count_so_far={})",
            CALLBACK_COUNT.load(Ordering::SeqCst)
        );
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        Err(
            "Input lock is only implemented on macOS at the moment. \
             A native Windows (WH_KEYBOARD_LL) / Linux (X11) port is planned."
                .into(),
        )
    }
}

#[allow(dead_code)]
pub fn is_locked() -> bool {
    LOCK_ACTIVE.load(Ordering::SeqCst)
}

// ── macOS event-tap (raw FFI) ────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod macos_impl {
    use super::*;
    use std::ffi::c_void;

    // ── Opaque CF / CG types as void pointers ─────────────────────
    type CGEventRef = *mut c_void;
    type CGEventTapProxy = *mut c_void;
    type CFMachPortRef = *mut c_void;
    type CFAllocatorRef = *mut c_void;
    type CFRunLoopRef = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFStringRef = *const c_void;
    type CGEventMask = u64;
    type CGEventTapCallBack = extern "C" fn(
        proxy: CGEventTapProxy,
        event_type: u32,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef;

    // ── Constants from <CoreGraphics/CGEventTypes.h> ──────────────
    #[allow(non_upper_case_globals)]
    mod c {
        // CGEventTapLocation
        pub const SESSION_EVENT_TAP: u32 = 1;
        // CGEventTapPlacement
        pub const HEAD_INSERT_EVENT_TAP: u32 = 0;
        // CGEventTapOptions
        pub const EVENT_TAP_OPTION_DEFAULT: u32 = 0;
        // CGEventField
        pub const KEYBOARD_EVENT_KEYCODE: u32 = 9;
        // CGEventType — values from CGEventTypes.h
        pub const EVT_LEFT_MOUSE_DOWN: u32 = 1;
        pub const EVT_LEFT_MOUSE_UP: u32 = 2;
        pub const EVT_RIGHT_MOUSE_DOWN: u32 = 3;
        pub const EVT_RIGHT_MOUSE_UP: u32 = 4;
        pub const EVT_MOUSE_MOVED: u32 = 5;
        pub const EVT_LEFT_MOUSE_DRAGGED: u32 = 6;
        pub const EVT_RIGHT_MOUSE_DRAGGED: u32 = 7;
        pub const EVT_KEY_DOWN: u32 = 10;
        pub const EVT_KEY_UP: u32 = 11;
        pub const EVT_FLAGS_CHANGED: u32 = 12;
        pub const EVT_SCROLL_WHEEL: u32 = 22;
        pub const EVT_OTHER_MOUSE_DOWN: u32 = 25;
        pub const EVT_OTHER_MOUSE_UP: u32 = 26;
        pub const EVT_OTHER_MOUSE_DRAGGED: u32 = 27;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: CGEventMask,
            callback: CGEventTapCallBack,
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
        fn CFRunLoopGetMain() -> CFRunLoopRef;
        static kCFRunLoopCommonModes: CFStringRef;
    }

    /// The raw C-ABI tap callback. Returning `event` passes the event
    /// through unchanged; returning `null_mut()` drops it from the
    /// queue. Single source of truth — no Rust wrapper between us and
    /// the OS, so there's no chance of an ABI-level mismatch.
    extern "C" fn tap_callback(
        _proxy: CGEventTapProxy,
        event_type: u32,
        event: CGEventRef,
        _user_info: *mut c_void,
    ) -> CGEventRef {
        let n = CALLBACK_COUNT.fetch_add(1, Ordering::SeqCst);
        if n < 8 {
            tracing::info!(
                "input_lock callback #{n}: type={event_type}, lock_active={}",
                LOCK_ACTIVE.load(Ordering::SeqCst)
            );
        }

        if !LOCK_ACTIVE.load(Ordering::SeqCst) {
            return event; // pass through unchanged
        }

        match event_type {
            ty if ty == c::EVT_KEY_DOWN => {
                let kc = unsafe {
                    CGEventGetIntegerValueField(event, c::KEYBOARD_EVENT_KEYCODE)
                };
                let matched = {
                    let mut p = pressed().lock();
                    p.insert(kc);
                    let chord = unlock_codes().lock();
                    !chord.is_empty() && chord.iter().all(|k| p.contains(k))
                };
                if matched {
                    LOCK_ACTIVE.store(false, Ordering::SeqCst);
                    pressed().lock().clear();
                    tracing::info!("input_lock: unlock chord matched, releasing");
                }
                std::ptr::null_mut() // drop
            }
            ty if ty == c::EVT_KEY_UP => {
                let kc = unsafe {
                    CGEventGetIntegerValueField(event, c::KEYBOARD_EVENT_KEYCODE)
                };
                pressed().lock().remove(&kc);
                std::ptr::null_mut()
            }
            _ => std::ptr::null_mut(), // drop mouse / scroll / flags while locked
        }
    }

    pub fn install_tap_on_main_runloop() -> Result<(), String> {
        let mask: CGEventMask = (1u64 << c::EVT_KEY_DOWN)
            | (1u64 << c::EVT_KEY_UP)
            | (1u64 << c::EVT_FLAGS_CHANGED)
            | (1u64 << c::EVT_LEFT_MOUSE_DOWN)
            | (1u64 << c::EVT_LEFT_MOUSE_UP)
            | (1u64 << c::EVT_LEFT_MOUSE_DRAGGED)
            | (1u64 << c::EVT_RIGHT_MOUSE_DOWN)
            | (1u64 << c::EVT_RIGHT_MOUSE_UP)
            | (1u64 << c::EVT_RIGHT_MOUSE_DRAGGED)
            | (1u64 << c::EVT_OTHER_MOUSE_DOWN)
            | (1u64 << c::EVT_OTHER_MOUSE_UP)
            | (1u64 << c::EVT_OTHER_MOUSE_DRAGGED)
            | (1u64 << c::EVT_MOUSE_MOVED)
            | (1u64 << c::EVT_SCROLL_WHEEL);

        let tap_port = unsafe {
            CGEventTapCreate(
                c::SESSION_EVENT_TAP,
                c::HEAD_INSERT_EVENT_TAP,
                c::EVENT_TAP_OPTION_DEFAULT,
                mask,
                tap_callback,
                std::ptr::null_mut(),
            )
        };
        if tap_port.is_null() {
            return Err(
                "CGEventTapCreate returned NULL. Grant Inspector Rust \
                 Accessibility access (System Settings → Privacy & Security → \
                 Accessibility), then try `freeze` again."
                    .into(),
            );
        }

        unsafe {
            let loop_source =
                CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap_port, 0);
            if loop_source.is_null() {
                return Err("CFMachPortCreateRunLoopSource returned NULL".into());
            }
            let main_loop = CFRunLoopGetMain();
            CFRunLoopAddSource(main_loop, loop_source, kCFRunLoopCommonModes);
            CGEventTapEnable(tap_port, true);
        }

        tracing::info!(
            "input_lock: CGEventTap installed on main run loop (raw FFI)"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_from_str_handles_letters_digits_and_aliases() {
        assert_eq!(key_from_str("a"), Some(0));
        assert_eq!(key_from_str("i"), Some(34));
        assert_eq!(key_from_str("r"), Some(15));
        assert_eq!(key_from_str("Z"), Some(6));
        assert_eq!(key_from_str("  z  "), Some(6));
        assert_eq!(key_from_str("0"), Some(29));
        assert_eq!(key_from_str("9"), Some(25));
        assert_eq!(key_from_str("space"), Some(49));
        assert_eq!(key_from_str("Enter"), Some(36));
        assert_eq!(key_from_str("return"), Some(36));
        assert_eq!(key_from_str("ESC"), Some(53));
        assert_eq!(key_from_str("escape"), Some(53));
        assert_eq!(key_from_str("tab"), Some(48));
        assert_eq!(key_from_str("delete"), Some(51));
    }

    #[test]
    fn key_from_str_rejects_unknown_tokens() {
        assert_eq!(key_from_str(""), None);
        assert_eq!(key_from_str("hello"), None);
        assert_eq!(key_from_str("f1"), None);
        assert_eq!(key_from_str("ctrl"), None);
    }

    #[test]
    fn start_input_lock_rejects_empty_or_all_unparseable_chord() {
        let r = start_input_lock(vec![]);
        assert!(r.is_err());
        let r = start_input_lock(vec!["not-a-key".into(), "alsobogus".into()]);
        assert!(r.is_err());
    }
}
