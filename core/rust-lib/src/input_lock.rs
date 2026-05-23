//! Input lock — block all keyboard / mouse / trackpad input until a
//! configured chord is pressed. Same idea as `pepperonas/macOS-lock`.
//!
//! Trigger: type `freeze` in the popup search bar (or press Enter on
//! the `freeze` autocomplete row). Unlock: press the chord configured
//! in Settings → Input Lock (default: hold `i`, press `r`).
//!
//! ## Implementation
//!
//! Native `CGEventTap` via the `core-graphics` + `core-foundation`
//! crates — the Quartz Event Services API the Python `macOS-lock`
//! script uses through PyObjC.
//!
//! The tap is installed on **the main thread's run loop** (the one
//! Tauri's NSApp is already spinning), not a worker thread with its
//! own run loop. The latter compiled but didn't actually intercept
//! events on macOS Sonoma+ — Apple's docs don't promise it works, and
//! evidence said it didn't. Installing on the main loop is what
//! macOS-lock's Python equivalent does (it blocks main with
//! `CFRunLoopRun`), and what most real-world Cocoa apps that use
//! event taps do. The tap object itself is `std::mem::forget`-ed so
//! it stays alive past the IPC handler's return — Rust's Drop would
//! otherwise tear down the Mach port the moment the binding leaves
//! scope.
//!
//! Toggle behaviour via `LOCK_ACTIVE` atomic: when false the callback
//! returns the event unchanged (passes through); when true it tracks
//! the unlock chord and swallows the event.
//!
//! Requires Accessibility (the existing grant covers it). On Windows /
//! Linux `start_input_lock` returns "not implemented yet" so the
//! Settings UI + trigger stay platform-agnostic.
//!
//! ## Safety hatch
//!
//! `⌥⌘Esc` (Force Quit) is processed by WindowServer above any
//! user-level event tap and cannot be intercepted — you can always
//! recover even if you forget the chord.

use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;

// ── Key-name → macOS keycode table ───────────────────────────────────────
//
// Identical mapping to `macos-lock-cli.py`'s `KEYCODE_MAP`. Stored
// platform-agnostically so the chord-validation logic on Windows/Linux
// still works even though the tap there isn't wired up yet.

/// Parse a key-name string (case-insensitive) into the macOS keycode
/// the event tap compares against. Returns `None` for unknown names so
/// the caller can surface a clear "invalid chord" error.
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

/// True while the input lock is active. The tap callback reads this
/// on every event; when false the event is passed through unmodified.
static LOCK_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Set to true once the tap is installed for the app's lifetime. We
/// only install once — subsequent lock cycles just flip `LOCK_ACTIVE`.
static TAP_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Diagnostic counter — incremented on every callback invocation.
/// The first few invocations are logged at info level so we can verify
/// the tap is actually receiving events.
static CALLBACK_COUNT: AtomicU64 = AtomicU64::new(0);

/// Keycodes that must be simultaneously pressed to unlock. Set by
/// `start_input_lock` before each cycle.
static UNLOCK_CODES: OnceLock<Mutex<Vec<i64>>> = OnceLock::new();

/// Currently-pressed keycodes, tracked by the tap callback.
static PRESSED: OnceLock<Mutex<HashSet<i64>>> = OnceLock::new();

fn unlock_codes() -> &'static Mutex<Vec<i64>> {
    UNLOCK_CODES.get_or_init(|| Mutex::new(Vec::new()))
}
fn pressed() -> &'static Mutex<HashSet<i64>> {
    PRESSED.get_or_init(|| Mutex::new(HashSet::new()))
}

// ── Public API ───────────────────────────────────────────────────────────

/// Activate the input lock with the given unlock chord.
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

/// Whether the lock is currently active. Used by tests + possible
/// frontend indicators.
#[allow(dead_code)]
pub fn is_locked() -> bool {
    LOCK_ACTIVE.load(Ordering::SeqCst)
}

// ── macOS event-tap implementation ───────────────────────────────────────

#[cfg(target_os = "macos")]
mod macos_impl {
    use super::*;
    use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
    use core_graphics::event::{
        CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
        CGEventType, EventField,
    };

    /// Build the CGEventTap, install its run-loop source on the **main
    /// thread's** run loop, enable it, and `mem::forget` the tap so
    /// it outlives this function (Drop would tear down the Mach port).
    pub fn install_tap_on_main_runloop() -> Result<(), String> {
        let events_of_interest = vec![
            CGEventType::KeyDown,
            CGEventType::KeyUp,
            CGEventType::FlagsChanged,
            CGEventType::LeftMouseDown,
            CGEventType::LeftMouseUp,
            CGEventType::LeftMouseDragged,
            CGEventType::RightMouseDown,
            CGEventType::RightMouseUp,
            CGEventType::RightMouseDragged,
            CGEventType::OtherMouseDown,
            CGEventType::OtherMouseUp,
            CGEventType::OtherMouseDragged,
            CGEventType::MouseMoved,
            CGEventType::ScrollWheel,
        ];

        let tap = CGEventTap::new(
            CGEventTapLocation::Session,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default, // intercepting, not listen-only
            events_of_interest,
            callback,
        )
        .map_err(|_| {
            "CGEventTap::new failed. Grant Inspector Rust Accessibility access \
             (System Settings → Privacy & Security → Accessibility), then try \
             `freeze` again."
                .to_string()
        })?;

        unsafe {
            let loop_source = tap
                .mach_port
                .create_runloop_source(0)
                .map_err(|_| "create_runloop_source failed".to_string())?;
            // The MAIN run loop, not the current thread's. Tauri's
            // NSApp.run is already spinning the main run loop, so
            // adding a Mach port source there means callbacks fire
            // on the main thread — what macOS expects for an HID-
            // session event tap.
            CFRunLoop::get_main().add_source(&loop_source, kCFRunLoopCommonModes);
        }
        tap.enable();

        // Drop would CFRelease the Mach port and disconnect the tap.
        // Forget keeps the underlying objects alive for the app's
        // lifetime.
        std::mem::forget(tap);

        tracing::info!("input_lock: CGEventTap installed on main run loop");
        Ok(())
    }

    /// The CGEventTap callback — fires on the main thread (where the
    /// run loop hosting our source lives). Returns `None` to drop the
    /// event, `Some(event)` to pass it through.
    fn callback(
        _proxy: core_graphics::event::CGEventTapProxy,
        ty: CGEventType,
        event: &core_graphics::event::CGEvent,
    ) -> Option<core_graphics::event::CGEvent> {
        // Diagnostic: log the first handful of invocations so we can
        // verify the tap is actually receiving events. Cheap atomic
        // increment + bounded logging.
        let n = CALLBACK_COUNT.fetch_add(1, Ordering::SeqCst);
        if n < 8 {
            tracing::info!(
                "input_lock callback #{n}: type={:?}, lock_active={}",
                ty,
                LOCK_ACTIVE.load(Ordering::SeqCst)
            );
        }

        // Fast path — when not locked, pass every event through.
        if !LOCK_ACTIVE.load(Ordering::SeqCst) {
            return Some(event.clone());
        }

        match ty {
            CGEventType::KeyDown => {
                let kc = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
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
                None
            }
            CGEventType::KeyUp => {
                let kc = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                pressed().lock().remove(&kc);
                None
            }
            _ => None, // swallow mouse + scroll while locked
        }
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
