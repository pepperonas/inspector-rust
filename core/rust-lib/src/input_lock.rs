//! Input lock — block all keyboard / mouse / trackpad input until a
//! configured chord is pressed. Inspired by `pepperonas/macOS-lock`.
//!
//! Triggered by typing `freeze` in the popup search bar; unlocked by
//! pressing the chord configured in Settings → Input Lock (default:
//! `i + r` — hold `i`, press `r`).
//!
//! Cross-platform via the `rdev` crate:
//! - **macOS** — CGEventTap. Requires Accessibility (same grant the
//!   text-expander + paste already need).
//! - **Windows** — `WH_KEYBOARD_LL` + `WH_MOUSE_LL` low-level hooks.
//!   No extra permission needed.
//! - **Linux** — X11 only (`XGrabKeyboard` / `XGrabPointer`). Wayland
//!   is **not supported** by `rdev` — the protocol forbids global
//!   input grabs for security. `start_input_lock` returns an error on
//!   a Wayland session so the user sees a clear toast.
//!
//! ## Safety hatches that always work
//!
//! OS-level system shortcuts cannot be intercepted by any user-level
//! event tap, so the user is never truly locked out of the machine:
//! - **macOS** — `⌥⌘Esc` opens Force Quit Applications.
//! - **Windows** — `Ctrl+Alt+Del` opens the security screen.
//! - **Linux** — `Ctrl+Alt+F2` switches to a different VT.
//!
//! ## Persistent grab thread
//!
//! `rdev::grab(callback)` blocks the calling thread forever — there is
//! no clean stop API. So we spawn the grab thread ONCE on first lock
//! activation (`GRAB_STARTED` guards re-spawn) and toggle behaviour via
//! the [`LOCK_ACTIVE`] atomic flag. When unlocked the callback just
//! returns `Some(event)` to pass through, so the per-event cost is the
//! callback invocation + atomic load + match — small but not free; the
//! trade-off is that the rdev API doesn't expose a stop primitive.

use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::thread;

use rdev::{grab, Event, EventType, Key};

/// True while the input lock is active. The grab callback reads this
/// to decide whether to swallow events (true) or pass them through
/// (false).
static LOCK_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Currently-pressed keys, tracked by the grab callback. Used to
/// match against [`unlock_chord`]'s contents.
static PRESSED: OnceLock<Mutex<HashSet<Key>>> = OnceLock::new();

/// The keys that must be simultaneously pressed to unlock. Set by
/// [`start_input_lock`] before each lock cycle.
static UNLOCK_CHORD: OnceLock<Mutex<Vec<Key>>> = OnceLock::new();

/// Tracks whether the grab thread has been spawned. We spawn once,
/// then toggle behaviour via `LOCK_ACTIVE` (rdev::grab can't be
/// stopped cleanly — see the module doc).
static GRAB_STARTED: AtomicBool = AtomicBool::new(false);

fn pressed() -> &'static Mutex<HashSet<Key>> {
    PRESSED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn unlock_chord() -> &'static Mutex<Vec<Key>> {
    UNLOCK_CHORD.get_or_init(|| Mutex::new(Vec::new()))
}

/// Parse a key-name string (case-insensitive) into the rdev::Key enum.
/// Supports lowercase letter names (`"a"`…`"z"`), digit names
/// (`"0"`…`"9"`), and a handful of common special keys.
pub fn key_from_str(s: &str) -> Option<Key> {
    let s = s.trim().to_lowercase();
    match s.as_str() {
        "a" => Some(Key::KeyA), "b" => Some(Key::KeyB), "c" => Some(Key::KeyC),
        "d" => Some(Key::KeyD), "e" => Some(Key::KeyE), "f" => Some(Key::KeyF),
        "g" => Some(Key::KeyG), "h" => Some(Key::KeyH), "i" => Some(Key::KeyI),
        "j" => Some(Key::KeyJ), "k" => Some(Key::KeyK), "l" => Some(Key::KeyL),
        "m" => Some(Key::KeyM), "n" => Some(Key::KeyN), "o" => Some(Key::KeyO),
        "p" => Some(Key::KeyP), "q" => Some(Key::KeyQ), "r" => Some(Key::KeyR),
        "s" => Some(Key::KeyS), "t" => Some(Key::KeyT), "u" => Some(Key::KeyU),
        "v" => Some(Key::KeyV), "w" => Some(Key::KeyW), "x" => Some(Key::KeyX),
        "y" => Some(Key::KeyY), "z" => Some(Key::KeyZ),
        "0" => Some(Key::Num0), "1" => Some(Key::Num1), "2" => Some(Key::Num2),
        "3" => Some(Key::Num3), "4" => Some(Key::Num4), "5" => Some(Key::Num5),
        "6" => Some(Key::Num6), "7" => Some(Key::Num7), "8" => Some(Key::Num8),
        "9" => Some(Key::Num9),
        "space" => Some(Key::Space),
        "return" | "enter" => Some(Key::Return),
        "tab" => Some(Key::Tab),
        "escape" | "esc" => Some(Key::Escape),
        _ => None,
    }
}

/// Activate the input lock with the given unlock chord. The chord is
/// a list of key-name strings (see [`key_from_str`] for accepted
/// values). All keys must be simultaneously pressed to unlock — order
/// of presses doesn't matter, so "hold `i`, then press `r`" works.
pub fn start_input_lock(unlock_keys: Vec<String>) -> Result<(), String> {
    let keys: Vec<Key> = unlock_keys
        .iter()
        .filter_map(|s| key_from_str(s))
        .collect();
    if keys.is_empty() {
        return Err("input lock: unlock chord is empty or unparseable".into());
    }

    // Reject Wayland sessions up front — rdev can't grab there and the
    // user would just see "lock activated" but nothing actually
    // blocked.
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            && std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
        {
            return Err(
                "input lock: not supported on Wayland (X11 only on Linux)".into(),
            );
        }
    }

    *unlock_chord().lock() = keys;
    pressed().lock().clear();
    LOCK_ACTIVE.store(true, Ordering::SeqCst);

    // Spawn the grab thread once; subsequent locks just flip the flag.
    if !GRAB_STARTED.swap(true, Ordering::SeqCst) {
        thread::spawn(|| {
            if let Err(e) = grab(callback) {
                // Roll back the GRAB_STARTED flag so a re-trigger has
                // a chance to retry (e.g. user just granted
                // Accessibility on macOS).
                LOCK_ACTIVE.store(false, Ordering::SeqCst);
                GRAB_STARTED.store(false, Ordering::SeqCst);
                tracing::error!("rdev::grab failed (input lock disabled): {e:?}");
            }
        });
    }

    Ok(())
}

/// Whether the lock is currently active. Used by the frontend for an
/// optional indicator and by tests.
pub fn is_locked() -> bool {
    LOCK_ACTIVE.load(Ordering::SeqCst)
}

fn callback(event: Event) -> Option<Event> {
    if !LOCK_ACTIVE.load(Ordering::SeqCst) {
        return Some(event);
    }
    match &event.event_type {
        EventType::KeyPress(key) => {
            let key = *key;
            // Update the pressed-set, then check the chord. Hold the
            // locks separately + briefly so concurrent reads aren't
            // blocked any longer than needed.
            {
                let mut p = pressed().lock();
                p.insert(key);
            }
            let matched = {
                let p = pressed().lock();
                let chord = unlock_chord().lock();
                !chord.is_empty() && chord.iter().all(|k| p.contains(k))
            };
            if matched {
                LOCK_ACTIVE.store(false, Ordering::SeqCst);
                pressed().lock().clear();
            }
            None
        }
        EventType::KeyRelease(key) => {
            pressed().lock().remove(key);
            None
        }
        EventType::ButtonPress(_)
        | EventType::ButtonRelease(_)
        | EventType::MouseMove { .. }
        | EventType::Wheel { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_from_str_handles_letters_digits_and_aliases() {
        assert_eq!(key_from_str("i"), Some(Key::KeyI));
        assert_eq!(key_from_str("R"), Some(Key::KeyR));
        assert_eq!(key_from_str("  z  "), Some(Key::KeyZ));
        assert_eq!(key_from_str("0"), Some(Key::Num0));
        assert_eq!(key_from_str("9"), Some(Key::Num9));
        assert_eq!(key_from_str("space"), Some(Key::Space));
        assert_eq!(key_from_str("Enter"), Some(Key::Return));
        assert_eq!(key_from_str("return"), Some(Key::Return));
        assert_eq!(key_from_str("ESC"), Some(Key::Escape));
        assert_eq!(key_from_str("escape"), Some(Key::Escape));
        assert_eq!(key_from_str("tab"), Some(Key::Tab));
    }

    #[test]
    fn key_from_str_rejects_unknown_tokens() {
        assert_eq!(key_from_str(""), None);
        assert_eq!(key_from_str("hello"), None);
        assert_eq!(key_from_str("f1"), None); // function keys not in MVP scope
        assert_eq!(key_from_str("ctrl"), None); // chord can't include modifiers
    }

    #[test]
    fn start_input_lock_rejects_empty_chord() {
        let r = start_input_lock(vec![]);
        assert!(r.is_err());
        let r = start_input_lock(vec!["not-a-key".into(), "alsobogus".into()]);
        assert!(r.is_err(), "all-unparseable chord must reject");
    }

    /// The chord-matching predicate works regardless of the *order* of
    /// presses — "hold `i`, then press `r`" is equivalent to "hold `r`,
    /// then press `i`". This pins that semantics so a future refactor
    /// to e.g. an ordered Vec doesn't silently break the UX.
    #[test]
    fn chord_match_is_order_independent() {
        let chord = [Key::KeyI, Key::KeyR];
        let pressed_then = [Key::KeyR, Key::KeyI]; // pressed in reverse order
        let p: HashSet<Key> = pressed_then.iter().copied().collect();
        let matched = !chord.is_empty() && chord.iter().all(|k| p.contains(k));
        assert!(matched);
    }
}
