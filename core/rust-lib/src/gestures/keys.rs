//! Key-down classification for the typing level on Windows and Linux (gesture
//! guard Phase 6). Pure, so it is tested everywhere.
//!
//! Same rules as the macOS keyboard tap: only the TIME of a key press is used,
//! never which key; a pure modifier press never counts (macOS doesn't even
//! deliver those as key-downs, Windows and Linux do), and a key pressed while
//! Ctrl or the Windows/Super key is held is a shortcut, not typing.

// Windows uses only the VK helpers, Linux only the evdev tracker.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

/// Windows virtual-key codes of the modifier keys (Shift, Ctrl, Alt in their
/// generic and left/right forms, both Windows keys, Caps Lock).
pub fn windows_vk_is_modifier(vk: u16) -> bool {
    matches!(vk, 0x10..=0x12 | 0x14 | 0x5B | 0x5C | 0xA0..=0xA5)
}

/// Windows: is a key pressed with these modifiers a shortcut (not typing)?
/// Ctrl or a Windows key makes one — except Ctrl+Alt, which is how AltGr
/// arrives and types `@`, `[`, `{` on German layouts.
pub fn windows_is_shortcut(ctrl: bool, alt: bool, win: bool) -> bool {
    win || (ctrl && !alt)
}

// Linux evdev key codes (linux/input-event-codes.h).
const KEY_LEFTCTRL: u32 = 29;
const KEY_LEFTSHIFT: u32 = 42;
const KEY_RIGHTSHIFT: u32 = 54;
const KEY_LEFTALT: u32 = 56;
const KEY_CAPSLOCK: u32 = 58;
const KEY_RIGHTCTRL: u32 = 97;
const KEY_RIGHTALT: u32 = 100;
const KEY_LEFTMETA: u32 = 125;
const KEY_RIGHTMETA: u32 = 126;

fn evdev_is_modifier(code: u32) -> bool {
    matches!(
        code,
        KEY_LEFTCTRL
            | KEY_RIGHTCTRL
            | KEY_LEFTSHIFT
            | KEY_RIGHTSHIFT
            | KEY_LEFTALT
            | KEY_RIGHTALT
            | KEY_LEFTMETA
            | KEY_RIGHTMETA
            | KEY_CAPSLOCK
    )
}

fn evdev_makes_shortcut(code: u32) -> bool {
    matches!(code, KEY_LEFTCTRL | KEY_RIGHTCTRL | KEY_LEFTMETA | KEY_RIGHTMETA)
}

/// Tracks which shortcut modifiers are held on Linux (libinput delivers the
/// raw presses and releases of every keyboard on the seat).
#[derive(Debug, Default)]
pub struct EvdevKeys {
    held: Vec<u32>,
}

impl EvdevKeys {
    /// A key event. Returns `Some(shortcut)` for a press that the typing level
    /// should see — `shortcut` = Ctrl or Super was held — and `None` for
    /// releases and modifier presses.
    pub fn on_key(&mut self, code: u32, pressed: bool) -> Option<bool> {
        if evdev_makes_shortcut(code) {
            if pressed {
                if !self.held.contains(&code) {
                    self.held.push(code);
                }
            } else {
                self.held.retain(|c| *c != code);
            }
        }
        if !pressed || evdev_is_modifier(code) {
            return None;
        }
        Some(!self.held.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_modifiers_never_count_and_letters_do() {
        for vk in [0x10, 0x11, 0x12, 0x14, 0x5B, 0x5C, 0xA0, 0xA3, 0xA5] {
            assert!(windows_vk_is_modifier(vk), "{vk:#x}");
        }
        for vk in [0x41, 0x20, 0x0D, 0x30, 0xA6, 0x13] {
            assert!(!windows_vk_is_modifier(vk), "{vk:#x}");
        }
    }

    #[test]
    fn windows_altgr_is_typing_but_ctrl_and_win_are_shortcuts() {
        assert!(windows_is_shortcut(true, false, false), "Ctrl+C");
        assert!(windows_is_shortcut(false, false, true), "Win+E");
        assert!(!windows_is_shortcut(true, true, false), "AltGr+Q = @");
        assert!(windows_is_shortcut(true, true, true), "Win wins");
        assert!(!windows_is_shortcut(false, true, false), "Alt alone");
        assert!(!windows_is_shortcut(false, false, false));
    }

    #[test]
    fn linux_typing_counts_and_modifiers_do_not() {
        let mut k = EvdevKeys::default();
        assert_eq!(k.on_key(30, true), Some(false), "KEY_A is typing");
        assert_eq!(k.on_key(30, false), None, "a release is nothing");
        assert_eq!(k.on_key(KEY_LEFTSHIFT, true), None);
        assert_eq!(k.on_key(30, true), Some(false), "Shift+A is still typing");
        assert_eq!(k.on_key(KEY_CAPSLOCK, true), None);
    }

    #[test]
    fn linux_ctrl_and_super_make_a_shortcut_until_released() {
        let mut k = EvdevKeys::default();
        assert_eq!(k.on_key(KEY_LEFTCTRL, true), None);
        assert_eq!(k.on_key(46, true), Some(true), "Ctrl+C");
        assert_eq!(k.on_key(KEY_LEFTMETA, true), None);
        assert_eq!(k.on_key(KEY_LEFTCTRL, false), None);
        assert_eq!(k.on_key(46, true), Some(true), "Super still held");
        assert_eq!(k.on_key(KEY_LEFTMETA, false), None);
        assert_eq!(k.on_key(46, true), Some(false), "both released");
        assert_eq!(k.on_key(KEY_LEFTALT, true), None);
        assert_eq!(k.on_key(46, true), Some(false), "Alt alone is not a shortcut");
    }
}
