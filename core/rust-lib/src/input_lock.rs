//! Input lock — block all keyboard / mouse / trackpad input until a
//! configured chord is pressed. Same idea as `pepperonas/macOS-lock`.
//!
//! Trigger: type `freeze` in the popup search bar (or press Enter on
//! the `freeze` autocomplete row). Unlock: press the chord configured
//! in Settings → Input Lock (default: hold `i`, press `r`).
//!
//! ## Implementation
//!
//! - **macOS** — native `CGEventTap` (the same Quartz Event Services
//!   API the Python `macOS-lock` script uses via PyObjC). The tap is
//!   installed at HID-session level + tap placement HeadInsert, so it
//!   sees every event before any other process. Runs on a dedicated
//!   thread with its own `CFRunLoopRun`; toggle behaviour via
//!   `LOCK_ACTIVE` atomic (the tap stays alive for the rest of the
//!   app lifetime so subsequent lock cycles don't pay re-creation
//!   cost). Requires Accessibility (same grant the text-expander +
//!   paste already use). **Replaced the v0.28.0 `rdev::grab`
//!   implementation, which used `unstable_grab` and triggered a
//!   process-level abort on macOS — going native avoids that.**
//!
//! - **Windows / Linux** — currently `start_input_lock` returns a
//!   clear "not implemented yet" error so the UI surfaces a toast
//!   instead of silently doing nothing. The chord storage + Settings
//!   UI are platform-agnostic and stay in place; only the platform
//!   tap is missing.
//!
//! ## Safety hatch
//!
//! OS-level shortcuts cannot be intercepted by user-level event taps:
//! - macOS: `⌥⌘Esc` → Force Quit. The lock can't block this — so the
//!   user can always recover even if they forget the chord.

use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

// ── Key-name → macOS keycode table ───────────────────────────────────────
//
// Identical mapping to `macos-lock-cli.py`'s `KEYCODE_MAP`. Stored
// platform-agnostically (just integers) so the chord-validation logic
// on Windows/Linux still works even though the tap there isn't wired
// up yet.

/// Parse a key-name string (case-insensitive) into the macOS keycode
/// the event tap compares against. Returns `None` for unknown names so
/// the caller surfaces a clear "invalid chord" error.
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

/// Set to true once the platform tap thread has been spawned for the
/// app's lifetime. We only spawn once — subsequent lock cycles just
/// flip `LOCK_ACTIVE`.
static TAP_STARTED: AtomicBool = AtomicBool::new(false);

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
        // If the tap thread is already up from a previous lock cycle,
        // just flip the flag — no need to re-create.
        if TAP_STARTED.load(Ordering::SeqCst) {
            LOCK_ACTIVE.store(true, Ordering::SeqCst);
            return Ok(());
        }

        // First-time setup. Spawn the tap thread, wait briefly for it
        // to report whether `CGEventTap::new` succeeded — so a missing
        // Accessibility permission (or any other failure mode) surfaces
        // back to the IPC caller as a clear error instead of silently
        // failing on a background thread.
        use std::sync::mpsc;
        let (tx, rx) = mpsc::channel::<Result<(), String>>();
        std::thread::Builder::new()
            .name("input-lock-tap".into())
            .spawn(move || macos_impl::run_event_tap(tx))
            .map_err(|e| format!("spawn tap thread: {e}"))?;

        match rx.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(Ok(())) => {
                TAP_STARTED.store(true, Ordering::SeqCst);
                LOCK_ACTIVE.store(true, Ordering::SeqCst);
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(
                "input lock: tap thread didn't report within 2 s — likely \
                 stuck waiting on Accessibility prompt. Grant Inspector Rust \
                 Accessibility access and try again."
                    .into(),
            ),
        }
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

    pub fn run_event_tap(init_tx: std::sync::mpsc::Sender<Result<(), String>>) {
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

        let tap = match CGEventTap::new(
            CGEventTapLocation::Session,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default, // not listen-only → can intercept
            events_of_interest,
            |_proxy, ty, event| {
                // Fast path — when not locked, pass every event through.
                if !LOCK_ACTIVE.load(Ordering::SeqCst) {
                    return Some(event.clone());
                }

                // Track keypresses so we can detect the unlock chord.
                // Mouse events are simply dropped while locked.
                match ty {
                    CGEventType::KeyDown => {
                        let kc = event.get_integer_value_field(
                            EventField::KEYBOARD_EVENT_KEYCODE,
                        );
                        let matched = {
                            let mut p = pressed().lock();
                            p.insert(kc);
                            let chord = unlock_codes().lock();
                            !chord.is_empty() && chord.iter().all(|k| p.contains(k))
                        };
                        if matched {
                            LOCK_ACTIVE.store(false, Ordering::SeqCst);
                            pressed().lock().clear();
                        }
                        None
                    }
                    CGEventType::KeyUp => {
                        let kc = event.get_integer_value_field(
                            EventField::KEYBOARD_EVENT_KEYCODE,
                        );
                        pressed().lock().remove(&kc);
                        None
                    }
                    _ => None, // swallow everything else while locked
                }
            },
        ) {
            Ok(t) => t,
            Err(_) => {
                // Creation failed — almost always missing Accessibility.
                let msg = "CGEventTap::new failed. \
                           Grant Inspector Rust Accessibility access \
                           (System Settings → Privacy → Accessibility), \
                           then try `freeze` again.".to_string();
                tracing::error!("{msg}");
                let _ = init_tx.send(Err(msg));
                return;
            }
        };

        // Tap created OK — report success so the IPC unblocks.
        let _ = init_tx.send(Ok(()));

        // Install on this thread's run loop, enable, and block.
        unsafe {
            let loop_source = match tap.mach_port.create_runloop_source(0) {
                Ok(s) => s,
                Err(_) => {
                    tracing::error!("create_runloop_source failed after tap init");
                    return;
                }
            };
            CFRunLoop::get_current().add_source(&loop_source, kCFRunLoopCommonModes);
        }
        tap.enable();
        tracing::info!("input-lock CGEventTap installed; running run loop");
        CFRunLoop::run_current();
        tracing::info!("input-lock run loop exited");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_from_str_handles_letters_digits_and_aliases() {
        // Letters → macOS keycodes from the canonical map.
        assert_eq!(key_from_str("a"), Some(0));
        assert_eq!(key_from_str("i"), Some(34));
        assert_eq!(key_from_str("r"), Some(15));
        assert_eq!(key_from_str("Z"), Some(6));
        assert_eq!(key_from_str("  z  "), Some(6));
        // Digits + specials.
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
        assert_eq!(key_from_str("f1"), None); // function keys not in scope yet
        assert_eq!(key_from_str("ctrl"), None); // chord can't include modifiers
        assert_eq!(key_from_str("\u{0000}"), None);
    }

    #[test]
    fn start_input_lock_rejects_empty_or_all_unparseable_chord() {
        let r = start_input_lock(vec![]);
        assert!(r.is_err());
        let r = start_input_lock(vec!["not-a-key".into(), "alsobogus".into()]);
        assert!(r.is_err(), "all-unparseable chord must reject");
    }
}
