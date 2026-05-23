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

use rdev::Key;

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

/// Activate the input lock with the given unlock chord.
///
/// ## ⚠️ Currently disabled — returns an error
///
/// The previous implementation called `rdev::grab(...)` on a spawned
/// thread to install a CGEventTap (macOS) / WH_KEYBOARD_LL (Windows) /
/// X11 grab (Linux). On macOS the `rdev` crate's `unstable_grab`
/// feature triggers a process-level abort under conditions we couldn't
/// isolate quickly — typing `freeze` instantly killed Inspector Rust.
///
/// Until we replace it with a native CGEventTap (via `objc2`, parallel
/// to how OCR uses Vision), the `freeze` command surfaces this error.
/// The chord-validation logic + the settings UI + the trigger plumbing
/// stay intact so the replacement just drops in.
pub fn start_input_lock(unlock_keys: Vec<String>) -> Result<(), String> {
    // Still validate the chord — that's free and gives early feedback
    // if the user's stored chord is malformed for whatever reason.
    let keys: Vec<Key> = unlock_keys
        .iter()
        .filter_map(|s| key_from_str(s))
        .collect();
    if keys.is_empty() {
        return Err("input lock: unlock chord is empty or unparseable".into());
    }

    Err(
        "Input lock is temporarily disabled — the rdev event-tap \
         implementation crashed the app on macOS. A native CGEventTap \
         port is in progress. Chord setting + UI stay so it'll work \
         immediately when re-enabled."
            .into(),
    )
}

/// Whether the lock is currently active. Always `false` while
/// [`start_input_lock`] is disabled (above). Kept on the public
/// surface so a future re-enable doesn't need to thread a new
/// function through the frontend.
#[allow(dead_code)]
pub fn is_locked() -> bool {
    false
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
    fn start_input_lock_currently_returns_a_clear_error() {
        // Even with a valid chord — the feature is gated off pending a
        // native CGEventTap port. The error message points the user at
        // the situation rather than silently doing nothing.
        let r = start_input_lock(vec!["i".into(), "r".into()]);
        assert!(r.is_err());
        let msg = r.unwrap_err();
        assert!(msg.contains("temporarily disabled"));
    }

    #[test]
    fn start_input_lock_rejects_empty_chord_before_disabled_check() {
        // The empty-chord guard fires first so a future re-enable can
        // rely on the parsed chord being non-empty.
        let r = start_input_lock(vec![]);
        assert!(r.is_err());
        let r = start_input_lock(vec!["not-a-key".into(), "alsobogus".into()]);
        assert!(r.is_err(), "all-unparseable chord must reject");
    }
}
