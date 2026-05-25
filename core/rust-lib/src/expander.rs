//! Trigger-based text expander.
//!
//! Workflow when the user presses the configured hotkey from any focused
//! text field:
//!
//! 1. Save the current clipboard text (best-effort — image / file
//!    clipboards are *not* preserved across the expansion in this version).
//! 2. Synthesize the platform's "select previous word" shortcut
//!    (`Option+Shift+Left` on macOS; `Ctrl+Shift+Left` elsewhere) followed
//!    by the platform copy shortcut.
//! 3. Read the now-selected word back out of the clipboard.
//! 4. Look it up in the snippets table via
//!    [`snippets::find_by_exact_abbreviation`].
//! 5. **Hit:** write the snippet body to the clipboard, synthesize paste —
//!    overwrites the still-active selection in the source app.
//! 6. **Miss:** do nothing (the selection stays visible so the user
//!    notices the failed match).
//! 7. Restore the original clipboard text after a small delay.
//!
//! All of step 2-5 happens while the popup is hidden — the source app
//! retains key focus the whole time.

use anyhow::{anyhow, Result};
use clipboard_rs::{Clipboard, ClipboardContext};
use enigo::{
    Direction::{Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use serde::{Deserialize, Serialize};
use std::thread;
use std::time::Duration;

use crate::db::DbHandle;
use crate::snippets;
use crate::text_field::{default_field_access, native_path, CapturePath, ReplaceOutcome};

/// enigo `Settings` with `open_prompt_to_get_permissions = false` —
/// see paste.rs for the full rationale. Used once at cache-init time
/// in [`enigo_lock`]; every call after that re-uses the same instance.
fn enigo_settings() -> Settings {
    Settings {
        open_prompt_to_get_permissions: false,
        ..Settings::default()
    }
}

/// Send+Sync wrapper around `Enigo` for our singleton cache. Enigo
/// itself is `!Send` (it caches OS handles that aren't thread-safe
/// in the general case). We serialise every access through the
/// surrounding `Mutex` *and* always call from the main thread (via
/// Tauri's `run_on_main_thread` queue), so the unsafe impls are
/// sound for our access pattern. Same shape as `UiaCell` on Windows.
struct EnigoCell(Enigo);
unsafe impl Send for EnigoCell {}
unsafe impl Sync for EnigoCell {}

/// Cached `Enigo`. v0.34.x and earlier created a fresh `Enigo::new()`
/// per call (~3-4 per expansion); each construction does a small but
/// non-zero amount of work (event-source create on macOS, key-state
/// init on Windows). Cache once; serialise via Mutex.
fn enigo_lock() -> Result<parking_lot::MappedMutexGuard<'static, Enigo>> {
    use parking_lot::{Mutex, MutexGuard};
    use std::sync::OnceLock;
    static ENIGO: OnceLock<Mutex<EnigoCell>> = OnceLock::new();
    if let Some(m) = ENIGO.get() {
        return Ok(MutexGuard::map(m.lock(), |c| &mut c.0));
    }
    let e = Enigo::new(&enigo_settings())
        .map_err(|err| anyhow!("enigo init failed: {err:?}"))?;
    let _ = ENIGO.set(Mutex::new(EnigoCell(e)));
    let g = ENIGO.get().expect("just-set or race").lock();
    Ok(MutexGuard::map(g, |c| &mut c.0))
}

// Whether the OS has granted Inspector Rust permission to synthesize keyboard
// events (macOS Accessibility / "Privacy & Security" → Accessibility).
// `enigo` silently no-ops without it on macOS — the hotkey fires, the
// `expand_at_cursor` cycle runs, but `Cmd+Shift+←` / `Cmd+C` / `Cmd+V`
// never reach the source app, so the abbreviation never gets selected
// or replaced. Knowing this state up-front lets the UI surface it
// instead of leaving the user puzzled.
#[cfg(target_os = "macos")]
mod macos_ax {
    use std::ffi::c_void;

    type CFTypeRef = *const c_void;
    type CFAllocatorRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type CFIndex = isize;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        pub fn AXIsProcessTrusted() -> bool;
        pub fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
        pub static kAXTrustedCheckOptionPrompt: CFTypeRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        pub static kCFBooleanTrue: CFTypeRef;
        pub static kCFAllocatorDefault: CFAllocatorRef;
        pub static kCFTypeDictionaryKeyCallBacks: c_void;
        pub static kCFTypeDictionaryValueCallBacks: c_void;

        pub fn CFDictionaryCreate(
            allocator: CFAllocatorRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            num_values: CFIndex,
            key_call_backs: *const c_void,
            value_call_backs: *const c_void,
        ) -> CFDictionaryRef;

        pub fn CFRelease(cf: CFTypeRef);
    }
}

/// Whether the OS-level synthetic-input permission is active.
#[cfg(target_os = "macos")]
pub fn accessibility_granted() -> bool {
    unsafe { macos_ax::AXIsProcessTrusted() }
}

/// Whether the OS-level synthetic-input permission is active.
#[cfg(target_os = "linux")]
pub fn accessibility_granted() -> bool {
    // Always attempt AT-SPI first; fall back to the clipboard path when it
    // returns None (same policy as Windows UI Automation).
    true
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn accessibility_granted() -> bool {
    true
}

/// True if macOS has currently raised the "secure event input" flag
/// — typically because a password field (or a sudo prompt in Terminal)
/// is the keyboard responder. While this is on, **`CGEventPost` is
/// dropped at the HID layer** for the affected process, so any
/// expansion attempt would silently no-op. We check this before
/// trying so we can fail loudly + actionably instead of "hotkey did
/// nothing".
///
/// Returns `false` on non-macOS (no equivalent system flag).
#[cfg(target_os = "macos")]
pub fn secure_event_input_active() -> bool {
    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn IsSecureEventInputEnabled() -> u8;
    }
    unsafe { IsSecureEventInputEnabled() != 0 }
}

#[cfg(not(target_os = "macos"))]
pub fn secure_event_input_active() -> bool {
    false
}

/// Is the user *still* holding the Alt / Option modifier? Used by
/// [`expand_at_cursor`] to skip the 40 ms "wait for the hotkey's own
/// modifier to come up" sleep when the modifier is already released
/// — typically the dominant case for a fast typist (tap-release
/// before our handler queues onto the main thread). Saves a noticeable
/// 20-30 ms of pre-expansion latency.
///
/// macOS uses CGEventSourceKeyState with VK_Option (kVK_Option=0x3A).
/// Windows uses `GetAsyncKeyState(VK_MENU=0x12)`. Linux: no portable
/// fast probe → optimistic `false` (skip the sleep, accept the rare
/// stuck-modifier race; not a regression since v0.33.x didn't probe
/// either).
#[cfg(target_os = "macos")]
fn alt_currently_held() -> bool {
    // CGEventSourceKeyState(kCGEventSourceStateHIDSystemState=1, keyCode).
    // VK code 0x3A is left Option; 0x3D is right Option. Either counts.
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn CGEventSourceKeyState(state: u32, keycode: u16) -> u8;
    }
    const HID_SYSTEM_STATE: u32 = 1;
    const VK_OPTION_LEFT: u16 = 0x3A;
    const VK_OPTION_RIGHT: u16 = 0x3D;
    unsafe {
        CGEventSourceKeyState(HID_SYSTEM_STATE, VK_OPTION_LEFT) != 0
            || CGEventSourceKeyState(HID_SYSTEM_STATE, VK_OPTION_RIGHT) != 0
    }
}

#[cfg(target_os = "windows")]
fn alt_currently_held() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_MENU};
    // GetAsyncKeyState returns SHORT; high bit set = key is down right now.
    unsafe { (GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000) != 0 }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn alt_currently_held() -> bool {
    // X11 has XQueryKeymap, Wayland has nothing. Skip — the worst
    // case is a single stuck-Alt event in the source app, no different
    // from pre-v0.34.x behaviour. Future PR can wire X11 if measured.
    false
}

/// Best-effort check whether our own popup/editor window is the
/// frontmost app. If yes, pressing the expander hotkey would expand
/// into our own search bar — confusing + useless. We refuse the
/// expansion in that case (logged + sentinel returned).
fn inspector_rust_is_frontmost() -> bool {
    match crate::frontmost_app::name() {
        Some(name) => {
            // The bundle's display name is "InspectorRust"; the
            // process-name lookup via System Events typically returns
            // the LSDisplayName, which is also "InspectorRust". On
            // some macOS variants it returns "Inspector Rust" with a
            // space — match both case-insensitively, hyphenated and
            // not.
            let n = name.to_ascii_lowercase();
            n == "inspectorrust" || n == "inspector rust" || n == "inspector-rust"
        }
        // No lookup possible (non-macOS, automation denied) → assume
        // we're not frontmost. Failing open is the right move: the
        // worst case is "expansion fires into our own popup" which is
        // already mitigated by the popup-being-hidden invariant in
        // the hotkey handler.
        None => false,
    }
}

/// Best-effort check whether the frontmost app is a terminal. In
/// terminals the abbreviation expander **cannot** read the typed
/// text (no AX-exposed input line) AND the clipboard-cycle fallback
/// produces garbage (`Option+Shift+Left` becomes an ESC-sequence,
/// not a selection — see comments in `expand_at_cursor`). Without
/// this check the user would either get a silent no-op *or* — worse
/// — a wrong expansion based on stale clipboard contents that
/// happened to match an abbreviation.
///
/// Returns `true` for: Terminal.app, iTerm2, Warp, kitty, Alacritty,
/// Ghostty, WezTerm, Tabby, Hyper, kovid's terminal-related tools.
/// Case-insensitive substring + exact match where useful.
fn is_terminal_frontmost() -> bool {
    let Some(name) = crate::frontmost_app::name() else {
        // Can't probe (non-macOS, automation denied) → optimistic
        // false. Worst case: user hits clipboard cycle in a terminal
        // and gets the same silent no-op as before this fix.
        return false;
    };
    let n = name.to_ascii_lowercase();
    // Exact matches first (faster; covers the dominant cases).
    matches!(
        n.as_str(),
        "terminal" | "iterm" | "iterm2" | "warp" | "kitty" | "alacritty"
            | "ghostty" | "wezterm" | "tabby" | "hyper"
    )
        // Substring catch-all for less common spellings / forks
        // (e.g. "Apple Terminal" reported as just "Terminal" most of
        // the time, but defensive). `terminal` covers Apple Terminal
        // and many forks; we already matched the exact name above
        // so this branch handles weird display variants.
        || n.contains("terminal")
        || n.contains("iterm")
}

/// Trigger the macOS "would like to control this computer" dialog and
/// add Inspector Rust to **System Settings → Privacy & Security → Accessibility**
/// so the user can flip the toggle there. Returns the *current* trusted
/// status (which is almost always `false` immediately after the prompt
/// appears — the user still has to grant it).
///
/// On non-macOS this is a no-op that returns the same as
/// [`accessibility_granted`].
#[cfg(target_os = "macos")]
pub fn request_accessibility_grant() -> bool {
    use macos_ax::*;
    use std::ffi::c_void;

    unsafe {
        let key = kAXTrustedCheckOptionPrompt;
        let value = kCFBooleanTrue;
        let dict = CFDictionaryCreate(
            kCFAllocatorDefault,
            &key as *const _,
            &value as *const _,
            1,
            &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
            &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
        );
        let trusted = AXIsProcessTrustedWithOptions(dict);
        CFRelease(dict);
        trusted
    }
}

#[cfg(not(target_os = "macos"))]
pub fn request_accessibility_grant() -> bool {
    accessibility_granted()
}

/// Open **System Settings → Privacy & Security → Accessibility** at the
/// right pane via the macOS preference URL scheme. No-op on other OSes.
#[cfg(target_os = "macos")]
pub fn open_accessibility_settings() -> anyhow::Result<()> {
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn()
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("open System Settings: {e}"))
}

#[cfg(not(target_os = "macos"))]
pub fn open_accessibility_settings() -> anyhow::Result<()> {
    Ok(())
}

/// Wipe stale TCC grants for Inspector Rust (Accessibility + PostEvent), then
/// fire the standard "would like to control" prompt with the *current*
/// cdhash. Solves the common "toggle says on but Inspector Rust still sees
/// untrusted" state that occurs after every real source-change rebuild
/// — the previous toggle was for an older cdhash, the new binary needs
/// a fresh grant. Runs `tccutil reset` for our own bundle id, which
/// doesn't require sudo.
#[cfg(target_os = "macos")]
pub fn force_reset_and_request_grant() -> anyhow::Result<bool> {
    // 1) Wipe whatever stale entries exist. tccutil exits 0 even if
    //    there's nothing to reset, so we don't need to check.
    let _ = std::process::Command::new("tccutil")
        .args(["reset", "Accessibility", "io.celox.inspector-rust"])
        .status();
    let _ = std::process::Command::new("tccutil")
        .args(["reset", "PostEvent", "io.celox.inspector-rust"])
        .status();

    // 2) Fire the prompt. This re-adds Inspector Rust to System Settings →
    //    Accessibility with the current cdhash, ready to be toggled on.
    Ok(request_accessibility_grant())
}

#[cfg(not(target_os = "macos"))]
pub fn force_reset_and_request_grant() -> anyhow::Result<bool> {
    Ok(accessibility_granted())
}

/// Settings keys.
pub const KEY_HOTKEY: &str = "expander.hotkey";
pub const KEY_ENABLED: &str = "expander.enabled";
/// One-shot flag: set once the legacy `Alt+Backquote` default has been
/// migrated to [`DEFAULT_HOTKEY`]. Prevents re-migrating a value the user
/// deliberately set back to `Alt+Backquote` afterwards.
pub const KEY_HOTKEY_MIGRATED: &str = "expander.hotkey_migrated_v0_12";
/// One-shot flag: on Linux Wayland, migrate layout-sensitive hotkeys
/// (`Ctrl+Backquote`, German `<` key confusion) to [`DEFAULT_HOTKEY`].
pub const KEY_HOTKEY_MIGRATED_LINUX: &str = "expander.hotkey_migrated_linux_v1";

/// Default hotkey when no setting has ever been written. `Alt + Digit1`
/// (the `1` row key, **not** the numpad) is layout-stable on every
/// keyboard: it has a fixed `KeyboardEvent.code` everywhere, isn't a
/// dead key on any layout, and isn't reserved by macOS or Windows. The
/// previous default `Alt+Backquote` was unreachable on German ISO Macs
/// (the physical `^` key reports as `IntlBackslash`, not `Backquote`).
pub const DEFAULT_HOTKEY: &str = "Alt+Digit1";

/// The pre-0.12 default, kept only so [`migrate_legacy_default`] can
/// recognise an un-customised old install and bump it.
pub const LEGACY_DEFAULT_HOTKEY: &str = "Alt+Backquote";

/// Error string returned by [`expand_at_cursor`] / [`diagnose_at_cursor`]
/// when synthetic input is needed but the OS hasn't granted it (macOS
/// Accessibility). The hotkey handler turns this sentinel into a popup +
/// `expander-permission-needed` event so the user gets an actionable
/// banner instead of a silent no-op.
pub const ERR_NO_ACCESSIBILITY: &str = "ax.permission_denied";

/// macOS-only sentinel: secure event input is active (typically a
/// password field is keyboard responder). `CGEventPost` is dropped at
/// the HID layer in this state, so any synthesis would silently
/// no-op. We bail with this sentinel so the hotkey handler can show
/// a "expansion blocked — secure input active" toast instead.
pub const ERR_SECURE_INPUT: &str = "ax.secure_input_active";

/// Sentinel: we ourselves are the frontmost app, so expanding into
/// our own search bar would be a no-op at best, confusing at worst.
/// The hotkey handler ignores this silently (a debug log is enough —
/// the user just hit the hotkey while looking at our popup, no
/// "error" worth surfacing).
pub const ERR_INSPECTOR_FRONTMOST: &str = "ax.inspector_frontmost";

/// Sentinel: the focused element is a password / secure text field.
/// We refuse to expand into one — risk of leaking a configured
/// expansion (e.g. a signature snippet) into a password manager or
/// sudo prompt. Surfaced as a brief toast.
pub const ERR_PASSWORD_FIELD: &str = "ax.password_field";

/// Sentinel: the frontmost app is a terminal where the abbreviation
/// expander fundamentally can't work (no AX-exposed input line + the
/// keystroke-cycle fallback's `Option+Shift+Left` becomes an
/// ESC-sequence, not a selection). The hotkey handler opens the
/// popup as a workaround so the user can search + paste.
pub const ERR_TERMINAL_UNSUPPORTED: &str = "ax.terminal_unsupported";

/// Typed enum for expander pre-check rejections. The hotkey handler
/// pattern-matches on this instead of doing fragile string equality
/// on `e.to_string()` — fewer copy-paste typos, easier to spot in a
/// review. Round-trips through the error chain via `to_sentinel()`
/// so the existing string-based API stays stable for the diagnose
/// IPC + future plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    /// macOS Accessibility not granted (synthetic input would no-op).
    NoAccessibility,
    /// macOS secure event input flag is active (CGEventPost dropped).
    SecureInput,
    /// We are the frontmost app (expansion would land in our own popup).
    InspectorFrontmost,
    /// Focused field is a password / secure text field.
    PasswordField,
    /// Frontmost is a terminal — abbreviation expansion can't work
    /// there (see [`ERR_TERMINAL_UNSUPPORTED`]). Hotkey handler
    /// reacts by opening the popup as a fallback.
    TerminalUnsupported,
}

impl BlockReason {
    /// String sentinel for the IPC error / diagnose surface. Stable;
    /// changing one of these is a breaking-API change for the frontend.
    pub fn to_sentinel(self) -> &'static str {
        match self {
            BlockReason::NoAccessibility => ERR_NO_ACCESSIBILITY,
            BlockReason::SecureInput => ERR_SECURE_INPUT,
            BlockReason::InspectorFrontmost => ERR_INSPECTOR_FRONTMOST,
            BlockReason::PasswordField => ERR_PASSWORD_FIELD,
            BlockReason::TerminalUnsupported => ERR_TERMINAL_UNSUPPORTED,
        }
    }

    /// Inverse of [`to_sentinel`] — recover the typed reason from an
    /// `anyhow::Error` chain. Returns `None` for non-block errors.
    pub fn from_error(e: &anyhow::Error) -> Option<BlockReason> {
        let s = e.to_string();
        match s.as_str() {
            ERR_NO_ACCESSIBILITY => Some(BlockReason::NoAccessibility),
            ERR_SECURE_INPUT => Some(BlockReason::SecureInput),
            ERR_INSPECTOR_FRONTMOST => Some(BlockReason::InspectorFrontmost),
            ERR_PASSWORD_FIELD => Some(BlockReason::PasswordField),
            ERR_TERMINAL_UNSUPPORTED => Some(BlockReason::TerminalUnsupported),
            _ => None,
        }
    }
}

/// Settings key: the direct hotkey→snippet bindings, stored as a JSON
/// array of [`DirectSlot`]. Empty / missing → no direct slots.
pub const KEY_DIRECT_SLOTS: &str = "expander.direct_slots";

/// One "press a hotkey → paste this snippet's body" binding. Unlike the
/// abbreviation expander it reads nothing from the focused field — it just
/// writes the body to the clipboard and synthesizes paste — so it works in
/// **any** app, including terminals (Terminal.app, iTerm2, …) where the
/// abbreviation paths can't see the input line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectSlot {
    /// Tauri global-shortcut accelerator string, same format as the
    /// abbreviation hotkey (e.g. `"Alt+Digit2"`).
    pub hotkey: String,
    /// `snippets.id` of the snippet whose body gets pasted on press.
    pub snippet_id: i64,
}

/// Read the configured direct slots. Malformed JSON → empty list (the UI
/// will let the user re-add them) rather than a hard error.
pub fn get_direct_slots(db: &DbHandle) -> Result<Vec<DirectSlot>> {
    match crate::settings::get(db, KEY_DIRECT_SLOTS)? {
        Some(json) if !json.trim().is_empty() => {
            Ok(serde_json::from_str(&json).unwrap_or_default())
        }
        _ => Ok(Vec::new()),
    }
}

/// Persist the direct slots as JSON.
pub fn set_direct_slots(db: &DbHandle, slots: &[DirectSlot]) -> Result<()> {
    let json = serde_json::to_string(slots)?;
    crate::settings::set(db, KEY_DIRECT_SLOTS, &json)
}

/// Sweep direct slots: drop any whose `snippet_id` no longer exists
/// in the `snippets` table. Called once at startup from `lib.rs`
/// before [`hotkey::register_direct_slots`] arms the global shortcuts
/// — otherwise stale slots would silently no-op on every hotkey
/// press and leave a confusing log trail.
///
/// Returns the number of slots that were pruned (0 = nothing changed,
/// no settings write). Errors during the snippet-existence check are
/// treated as "don't prune" so a transient SQLite hiccup doesn't
/// destroy bindings.
pub fn prune_stale_direct_slots(db: &DbHandle) -> Result<usize> {
    let slots = get_direct_slots(db)?;
    if slots.is_empty() {
        return Ok(0);
    }
    let mut kept = Vec::with_capacity(slots.len());
    let mut pruned = 0usize;
    for slot in slots {
        match snippets::get_by_id(db, slot.snippet_id) {
            Ok(Some(_)) => kept.push(slot),
            Ok(None) => {
                tracing::info!(
                    "pruning stale direct slot: hotkey={} → deleted snippet_id={}",
                    slot.hotkey,
                    slot.snippet_id
                );
                pruned += 1;
            }
            Err(e) => {
                tracing::warn!(
                    "stale-slot check errored for snippet_id={} (keeping slot): {e:#}",
                    slot.snippet_id
                );
                kept.push(slot);
            }
        }
    }
    if pruned > 0 {
        set_direct_slots(db, &kept)?;
    }
    Ok(pruned)
}

/// Paste the body of snippet `snippet_id` at the current cursor — the
/// handler for a direct-slot global shortcut. No reading, no selection:
/// write body → synthesize paste → restore the clipboard. A no-op (logged)
/// if the snippet was deleted since the slot was bound. On macOS, returns
/// the [`ERR_NO_ACCESSIBILITY`] sentinel when synthetic input isn't
/// allowed (same gate as the abbreviation expander and the main paste).
///
/// Must run on the main thread on macOS — `enigo`'s `Cmd+V` synthesis
/// touches TSM, which asserts main-thread (see `hotkey.rs` / `docs/text-expander.md`).
pub fn paste_snippet_body(
    db: &DbHandle,
    snippet_id: i64,
    watcher: Option<&crate::clipboard_watcher::WatcherState>,
) -> Result<()> {
    let Some(snippet) = snippets::get_by_id(db, snippet_id)? else {
        tracing::warn!("direct slot points at deleted snippet id {snippet_id}; ignoring");
        return Ok(());
    };
    // Same safety gates as `expand_at_cursor`. Direct-slot is even more
    // dangerous in a password field — the slot has a known body and
    // would happily paste it into a sudo prompt or password manager.
    if inspector_rust_is_frontmost() {
        tracing::debug!("paste_snippet_body: Inspector Rust is frontmost — skipping");
        return Err(anyhow!(ERR_INSPECTOR_FRONTMOST));
    }
    #[cfg(target_os = "macos")]
    {
        if !accessibility_granted() {
            return Err(anyhow!(ERR_NO_ACCESSIBILITY));
        }
        if secure_event_input_active() {
            return Err(anyhow!(ERR_SECURE_INPUT));
        }
    }
    // Password-field guard. Direct slots are especially risky here
    // because they paste a *known* body — if it's a signature or
    // similar, that body would land in a password manager / sudo
    // prompt / system password dialog. Bail loudly.
    if matches!(
        default_field_access().is_focused_field_secure(),
        Ok(true)
    ) {
        tracing::warn!("paste_snippet_body: focused field is secure — refusing");
        return Err(anyhow!(ERR_PASSWORD_FIELD));
    }
    // If the user typed the snippet's abbreviation before pressing the
    // direct hotkey (the dominant flow — type `aiplan`, press hotkey,
    // expect the body to replace it), delete the typed abbreviation
    // first by synthesizing N Backspaces. We deliberately do not *read*
    // the field — that would break the "works everywhere including
    // terminals" promise. So this is blind: if the user pressed the
    // hotkey without typing the abbreviation, N chars before the cursor
    // get deleted. Character count, not byte length, so multibyte
    // abbreviations (umlauts, emoji) work correctly.
    let abbrev_chars = snippet.abbreviation.chars().count();
    if abbrev_chars > 0 {
        send_backspaces(abbrev_chars)?;
        // Same beat we use before paste — let the destination app
        // process the deletes before the clipboard write hits.
        thread::sleep(Duration::from_millis(40));
    }
    paste_over_selection(&snippet.body, watcher)
}

/// One-time settings migration: if the stored hotkey is still the pre-0.12
/// default `Alt+Backquote` (i.e. the user never changed it), rewrite it to
/// the new layout-stable [`DEFAULT_HOTKEY`]. A migration flag makes this
/// idempotent and prevents clobbering a value the user re-picks later.
/// Returns the hotkey string that should be used from here on.
pub fn migrate_legacy_default(db: &DbHandle) -> String {
    use crate::settings;

    let stored = settings::get_or(db, KEY_HOTKEY, DEFAULT_HOTKEY)
        .unwrap_or_else(|_| DEFAULT_HOTKEY.to_string());

    if settings::get_bool(db, KEY_HOTKEY_MIGRATED, false).unwrap_or(false) {
        return stored;
    }

    let upgraded = if stored == LEGACY_DEFAULT_HOTKEY {
        let _ = settings::set(db, KEY_HOTKEY, DEFAULT_HOTKEY);
        tracing::info!("migrated expander hotkey {LEGACY_DEFAULT_HOTKEY} → {DEFAULT_HOTKEY}");
        DEFAULT_HOTKEY.to_string()
    } else {
        stored
    };
    let _ = settings::set(db, KEY_HOTKEY_MIGRATED, "true");
    upgraded
}

/// Linux Wayland hotkeys are registered via GNOME gsettings, not Tauri.
/// Keys like `Ctrl+Backquote` map to `<Control>grave`, which is **not**
/// the German `<` key (`less`) — the shortcut looks armed but never fires.
/// Migrate known-broken combos once to the layout-stable default.
#[cfg(target_os = "linux")]
pub fn migrate_linux_wayland_hotkey(db: &DbHandle, stored: String) -> String {
    use crate::desktop_shortcuts::expander_hotkey_needs_gsettings;
    use crate::settings;

    if settings::get_bool(db, KEY_HOTKEY_MIGRATED_LINUX, false).unwrap_or(false) {
        return stored;
    }

    if !expander_hotkey_needs_gsettings() {
        let _ = settings::set(db, KEY_HOTKEY_MIGRATED_LINUX, "true");
        return stored;
    }

    const BROKEN: &[&str] = &[
        "Ctrl+Backquote",
        "Control+Backquote",
        "Alt+Backquote",
        "Ctrl+IntlBackslash",
        "Alt+IntlBackslash",
    ];

    let upgraded = if BROKEN.iter().any(|b| stored.eq_ignore_ascii_case(b)) {
        let _ = settings::set(db, KEY_HOTKEY, DEFAULT_HOTKEY);
        tracing::info!(
            "Linux Wayland: migrated expander hotkey {stored} → {DEFAULT_HOTKEY} \
             (Backquote/grave does not match the German < key in gsettings; use Alt+1)"
        );
        DEFAULT_HOTKEY.to_string()
    } else {
        stored
    };

    let _ = settings::set(db, KEY_HOTKEY_MIGRATED_LINUX, "true");
    upgraded
}

#[cfg(not(target_os = "linux"))]
pub fn migrate_linux_wayland_hotkey(_db: &DbHandle, stored: String) -> String {
    stored
}

/// Diagnostic outcome of an expand-cycle attempt: what got captured,
/// whether it matched a snippet, and a preview of what would be pasted.
/// Used by the Settings panel's "Test now" button so the user can see
/// the *exact* reason an expansion fails (no abbreviation matched, the
/// captured text was empty, …) instead of a silent no-op.
#[derive(Debug, Serialize)]
pub struct DiagnoseResult {
    /// The whitespace-trimmed text captured before the cursor. Empty
    /// string if nothing was selectable (no text before the cursor).
    pub captured: String,
    /// Set when `find_by_exact_abbreviation` returned a row.
    pub matched_abbreviation: Option<String>,
    /// First ~80 characters of the matched snippet body — gives the user
    /// confidence the right snippet would be pasted.
    pub paste_preview: Option<String>,
    /// Which capture mechanism actually succeeded:
    /// - `ax` — macOS Accessibility API (`AXUIElement`).
    /// - `uia` — Windows UI Automation (`IUIAutomation`).
    /// - `clipboard` — fell back to `Cmd/Ctrl+Shift+←` + `Cmd/Ctrl+C`.
    pub path: CapturePath,
}

/// Run the capture half of expansion (read the word before the cursor,
/// look it up) **without** pasting. The caller is responsible for hiding
/// the popup *before* this runs so the AX/UIA call targets the source
/// app, not Inspector Rust itself.
///
/// Capture-path policy:
/// 1. **Try AX (macOS) / UIA (Windows) first.** No keystroke synthesis,
///    no clipboard touch — the cleanest path. Works in any app that
///    exposes its focused field through accessibility.
/// 2. **Fall back to the clipboard roundtrip** (`Cmd/Ctrl+Shift+←` +
///    `Cmd/Ctrl+C`) only when AX/UIA returns `None`. The user's
///    clipboard is saved and restored around the operation.
pub fn diagnose_at_cursor(db: &DbHandle) -> Result<DiagnoseResult> {
    // 1) AX/UIA first — clean read, no side effects.
    // BUT: on macOS, calling AX functions on an *untrusted* process
    // triggers the system "would like to control" prompt as a side
    // effect — even when we just want to silently fall back to the
    // clipboard path. So short-circuit when we know the process isn't
    // trusted yet.
    let access = default_field_access();
    let access_ok = accessibility_granted();

    // On macOS, without the Accessibility grant we can neither read the
    // focused field (AX) nor synthesize the clipboard-roundtrip keystrokes
    // (enigo no-ops) — the diagnosis would just report an empty capture
    // and leave the user guessing. Surface the real reason instead.
    #[cfg(target_os = "macos")]
    if !access_ok {
        return Err(anyhow!(
            "Accessibility permission isn't granted — Inspector Rust can't read the \
             focused field or synthesize keystrokes without it. Grant it in the \
             section above, then relaunch Inspector Rust."
        ));
    }

    let (captured, path) = match if access_ok {
        access.read_word_before_cursor()
    } else {
        Ok(None)
    } {
        Ok(Some(word)) => (word, native_path()),
        Ok(None) | Err(_) => {
            // 2) Fall back to the clipboard roundtrip.
            let saved = read_clipboard_text();
            select_previous_word()?;
            thread::sleep(Duration::from_millis(30));
            send_copy()?;
            thread::sleep(Duration::from_millis(80));
            let captured_raw = read_clipboard_text().unwrap_or_default();
            restore_clipboard(saved.as_deref());
            (
                trim_abbreviation(&captured_raw).to_string(),
                CapturePath::Clipboard,
            )
        }
    };

    let mut result = DiagnoseResult {
        captured: captured.clone(),
        matched_abbreviation: None,
        paste_preview: None,
        path,
    };

    if !captured.is_empty() {
        if let Some(snippet) = snippets::find_by_exact_abbreviation(db, &captured)? {
            // First 80 chars of the body, single-line preview.
            let preview: String = snippet.body.replace('\n', " ").chars().take(80).collect();
            result.matched_abbreviation = Some(snippet.abbreviation);
            result.paste_preview = Some(preview);
        }
    }

    Ok(result)
}

/// Run a full expand-at-cursor cycle. Errors are returned for logging but
/// the orchestration layer (the hotkey handler) treats them as recoverable
/// — the next press starts a fresh attempt.
///
/// Tries AX (macOS) / UIA (Windows) **first** — the clean path with no
/// clipboard touch and no flickering selection. Falls back to the
/// keystroke + clipboard roundtrip only when the focused element doesn't
/// expose accessibility info.
pub fn expand_at_cursor(
    db: &DbHandle,
    watcher: Option<&crate::clipboard_watcher::WatcherState>,
) -> Result<()> {
    // ── Pre-flight safety checks (cheap; bail before any AX/keystroke work) ──
    // 1) Inspector Rust itself frontmost? Hotkey was bounced into our
    //    own popup; do nothing.
    if inspector_rust_is_frontmost() {
        tracing::debug!("expand_at_cursor: Inspector Rust is frontmost — skipping");
        return Err(anyhow!(ERR_INSPECTOR_FRONTMOST));
    }
    // 2) macOS secure event input → CGEventPost is dropped; bail
    //    actionably rather than fail silently.
    #[cfg(target_os = "macos")]
    if secure_event_input_active() {
        tracing::warn!("expand_at_cursor: secure event input active — refusing");
        return Err(anyhow!(ERR_SECURE_INPUT));
    }

    // The hotkey itself is `Alt+<key>` — when this runs (queued onto
    // the main thread, a few ms after key-down) the user may still be
    // physically holding that `Alt`. If we synthesize our own chords
    // while it's down, the source app sees `Alt+Cmd+Shift+←` instead
    // of `Cmd+Shift+←` and breaks. Pre-v0.35 we slept a flat 40 ms;
    // now we poll the modifier state and wait *only* if Alt is
    // actually still pressed. Fast typists (release-before-handler)
    // save the full 40 ms; slow ones pay the original cost. Cap at
    // 80 ms total so a key actually stuck in the OS doesn't hang us.
    {
        let start = std::time::Instant::now();
        while alt_currently_held() && start.elapsed() < Duration::from_millis(80) {
            thread::sleep(Duration::from_millis(8));
        }
    }

    // ── Path 1: native accessibility (AX / UIA) ────────────────────────────
    // Skip AX entirely when the process isn't trusted — calling AX from
    // an untrusted process fires the macOS permission prompt as a side
    // effect, which is exactly the noise we want to avoid for users in
    // the post-rebuild stale-cdhash state. Fall straight through to the
    // clipboard path; the SettingsPanel banner / Force re-grant button
    // is the right place to surface the underlying permission issue.
    let access = default_field_access();
    if accessibility_granted() {
        // 3) Password-field guard. Cheap AX/UIA query *before* we
        //    try to read text — refuse to expand into a credentials
        //    field. The AX/UIA path returns None for unsupported
        //    elements; treat true as "yes, password" and bail loudly.
        match access.is_focused_field_secure() {
            Ok(true) => {
                tracing::warn!("expand_at_cursor: focused field is secure — refusing");
                return Err(anyhow!(ERR_PASSWORD_FIELD));
            }
            Ok(false) => {} // proceed
            Err(e) => tracing::debug!("password-field probe failed (continuing): {e:#}"),
        }
        if let Ok(Some(word)) = access.read_word_before_cursor() {
            if let Some(snippet) = snippets::find_by_exact_abbreviation(db, &word)? {
                // Try the in-place replace via the same accessibility layer.
                match access.try_replace_word_before_cursor(&snippet.body) {
                    Ok(ReplaceOutcome::Replaced) => {
                        tracing::info!(
                            "expander: matched snippet {:?} via {:?}",
                            snippet.abbreviation,
                            native_path()
                        );
                        return Ok(());
                    }
                    Ok(ReplaceOutcome::SelectionActive) => {
                        // The AX layer *selected* the abbreviation but the
                        // in-place text set was a no-op — the typical
                        // Electron / Chromium / Mac-Catalyst case (WhatsApp,
                        // Slack, Discord, VS Code, …): those expose `AXValue`
                        // read-only and return success for the set anyway.
                        // The abbreviation is highlighted right now, so just
                        // paste the body over it — do NOT re-select.
                        tracing::debug!(
                            "AX selected the abbreviation but in-place replace \
                             was a no-op; pasting body over the live selection"
                        );
                        return paste_over_selection(&snippet.body, watcher);
                    }
                    Ok(ReplaceOutcome::Unsupported) => {
                        tracing::debug!(
                            "focused element exposes no settable text attrs; \
                             falling back to keystroke-select + paste"
                        );
                        // Reuse the abbreviation we already captured.
                        return expand_via_clipboard(db, Some(&word), Some(&snippet.body), watcher);
                    }
                    Err(e) => {
                        tracing::warn!("AX/UIA replace errored: {e:#}; falling back");
                        return expand_via_clipboard(db, Some(&word), Some(&snippet.body), watcher);
                    }
                }
            }
            // No snippet matched. We still want the user to *see* the
            // failure — leaving the field untouched is the right move;
            // no fallback needed for the no-match case.
            return Ok(());
        }
    }

    // 4) Terminal short-circuit. The Path 2 clipboard cycle below
    //    *cannot* work in terminals — `Option+Shift+Left` becomes an
    //    ESC-sequence in the shell, not a selection. The captured
    //    "abbreviation" would be empty or stale clipboard contents,
    //    which is at best a silent no-op and at worst a wrong-paste
    //    (if the old clipboard happened to match an abbreviation).
    //    Bail with the sentinel so the hotkey handler can open the
    //    popup as a fallback (loud + actionable instead of silent).
    if is_terminal_frontmost() {
        tracing::info!(
            "expand_at_cursor: terminal frontmost — clipboard cycle would mis-paste; opening popup"
        );
        return Err(anyhow!(ERR_TERMINAL_UNSUPPORTED));
    }

    // ── Path 2: clipboard roundtrip (legacy) ───────────────────────────────
    // This path synthesizes `Cmd/Ctrl+Shift+←` + `Cmd/Ctrl+C` + `Cmd/Ctrl+V`
    // via enigo. On macOS that needs the Accessibility (AXIsProcessTrusted)
    // grant — and if we got here on macOS, the `if accessibility_granted()`
    // above was false, so this would silently no-op. Bail with the
    // sentinel instead; the caller turns it into a "grant Accessibility"
    // banner. On Windows/Linux `accessibility_granted()` is always true,
    // so this never fires and the keystroke path runs normally.
    #[cfg(target_os = "macos")]
    if !accessibility_granted() {
        return Err(anyhow!(ERR_NO_ACCESSIBILITY));
    }

    expand_via_clipboard(db, None, None, watcher)
}

/// Pre-AX/UIA expand path. Used as a fallback when the focused element
/// doesn't expose accessibility, and (with `prefetched_*` set) when
/// AX/UIA gave us the abbreviation + body but couldn't perform the
/// replace itself — we still need keystroke synthesis to actually
/// replace the word.
fn expand_via_clipboard(
    db: &DbHandle,
    prefetched_word: Option<&str>,
    prefetched_body: Option<&str>,
    watcher: Option<&crate::clipboard_watcher::WatcherState>,
) -> Result<()> {
    let saved = read_clipboard_text();

    // Select-prev-word + copy is only needed when we don't already know
    // the abbreviation. With prefetched data the cursor is still right
    // after the abbreviation but no selection exists yet — re-select.
    select_previous_word()?;
    thread::sleep(Duration::from_millis(30));
    send_copy()?;
    thread::sleep(Duration::from_millis(80));

    let abbr_raw = read_clipboard_text().unwrap_or_default();

    // SAFETY GUARD: if our select+copy didn't change the clipboard at
    // all (the new `abbr_raw` equals the pre-cycle `saved`), it means
    // `Option/Ctrl+Shift+Left` produced no selection — typical for
    // any app where word-back-with-shift isn't a real keyboard
    // selection (terminals, some web text fields with custom key
    // handlers, …). Without this check we'd then look up the OLD
    // clipboard text as if it were the typed abbreviation — and if it
    // happens to match a configured snippet, we'd paste the WRONG
    // body into the cursor position. Bail loudly with the terminal
    // sentinel so the popup-fallback fires.
    if prefetched_word.is_none() && Some(abbr_raw.as_str()) == saved.as_deref() {
        tracing::warn!(
            "expand_via_clipboard: select+copy yielded same clipboard as before — \
             cycle had no effect (terminal? non-AX text view?). Bailing to popup."
        );
        return Err(anyhow!(ERR_TERMINAL_UNSUPPORTED));
    }

    let abbr = if let Some(w) = prefetched_word {
        // Prefer the AX-captured word — guards against the clipboard
        // capturing the wrong region in apps that mistreat Shift+Left.
        w
    } else {
        trim_abbreviation(&abbr_raw)
    };
    if abbr.is_empty() {
        tracing::info!(
            "expander: no abbreviation captured before cursor (clipboard path empty — \
             keystroke synthesis may not reach this app on Wayland, or nothing to select)"
        );
        restore_clipboard(saved.as_deref());
        return Ok(());
    }

    // 4) Look it up — unless the AX/UIA path already gave us the body.
    let body = if let Some(b) = prefetched_body {
        b.to_string()
    } else {
        let hit = snippets::find_by_exact_abbreviation(db, abbr)?;
        let Some(snippet) = hit else {
            tracing::info!("expander: captured {abbr:?} but no snippet matches that abbreviation");
            // Selection stays highlighted in the source app — visual cue
            // that nothing matched. Restore clipboard before bailing.
            restore_clipboard(saved.as_deref());
            return Ok(());
        };
        tracing::info!(
            "expander: matched snippet {:?} via clipboard path",
            snippet.abbreviation
        );
        snippet.body
    };

    // 5) Replace selection: write the body, paste over the highlight.
    //    Arm the watcher so the body + restored clipboard don't
    //    pollute history (pre-v0.33.0 bug: every expansion via this
    //    path added the snippet body as a "new clip").
    if let Some(w) = watcher {
        w.mark_self_write(crate::models::ContentType::Text, &body);
    }
    write_clipboard_text(&body)?;
    thread::sleep(Duration::from_millis(50));
    send_paste()?;
    tracing::info!(
        "expander: pasted snippet body ({body_len} chars)",
        body_len = body.len()
    );

    // 6) Background restore (v0.35.0+). Same shape as
    //    `paste_over_selection`: return immediately after paste, then
    //    a worker thread waits 120 ms and restores only if our body
    //    is still on the clipboard. Drops 180 ms from the user-
    //    perceived expansion latency.
    if let Some(text) = saved {
        let watcher_clone = watcher.cloned_handle();
        let body_owned = body.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            if matches!(read_clipboard_text(), Some(cur) if cur == body_owned) {
                if let Some(w) = watcher_clone.as_ref() {
                    w.mark_self_write(crate::models::ContentType::Text, &text);
                }
                let _ = write_clipboard_text(&text);
            }
        });
    }

    Ok(())
}

/// Paste `body` over whatever is currently selected in the focused app,
/// then restore the user's clipboard text. Used when the accessibility
/// layer managed to *select* the abbreviation but couldn't replace the
/// text in place — the typical Electron / Mac-Catalyst case. No
/// re-selection here: the selection is already on the abbreviation, so a
/// `Cmd/Ctrl+Shift+←` would only extend it onto the previous word.
fn paste_over_selection(
    body: &str,
    watcher: Option<&crate::clipboard_watcher::WatcherState>,
) -> Result<()> {
    let saved = read_clipboard_text();
    // Arm the watcher for BOTH writes — the body about to be pasted
    // *and* the (background-scheduled) restored clipboard. Without
    // this, every snippet expansion would pollute history with the
    // body. Pre-v0.33.0 bug: missing mark_self_write here meant
    // `aiplan` → body got stored in history *and* re-surfaced as a
    // "recent" clip.
    if let Some(w) = watcher {
        w.mark_self_write(crate::models::ContentType::Text, body);
        // Also pre-arm the restore so the watcher skips that too.
        // Re-arming overwrites the previous fuse, but both writes
        // happen sequentially below — the watcher checks each event
        // against the most-recent fuse, which is what we want.
        if let Some(text) = saved.as_deref() {
            // Defer arming the restore until restore time (otherwise
            // the body write would consume this fuse instead).
            let _ = text;
        }
    }
    write_clipboard_text(body)?;
    thread::sleep(Duration::from_millis(50));
    send_paste()?;

    // v0.35 — restore in a background thread. The expand call
    // returns immediately after paste; we no longer block 180 ms
    // for nothing. The restore-thread waits 120 ms (enough for
    // every app I tested to finish consuming the clipboard) then
    // checks: if the clipboard still equals our body, restore the
    // user's text. If it doesn't (the user or another app wrote
    // something new in those 120 ms), do nothing — don't clobber
    // their fresh clipboard content.
    if let Some(text) = saved {
        let watcher_clone = watcher.cloned_handle();
        let body_owned = body.to_string();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            if matches!(read_clipboard_text(), Some(cur) if cur == body_owned) {
                if let Some(w) = watcher_clone.as_ref() {
                    w.mark_self_write(crate::models::ContentType::Text, &text);
                }
                let _ = write_clipboard_text(&text);
            }
        });
    }
    Ok(())
}

/// Helper trait to clone a `Option<&WatcherState>` into a `Send`-able
/// owned handle the restore thread can take. The clipboard_watcher
/// already exposes Arc<...> internals; we re-use the existing public
/// snapshot via `WatcherState::handle()`.
trait WatcherClone {
    fn cloned_handle(self) -> Option<crate::clipboard_watcher::WatcherState>;
}
impl WatcherClone for Option<&crate::clipboard_watcher::WatcherState> {
    fn cloned_handle(self) -> Option<crate::clipboard_watcher::WatcherState> {
        self.map(|w| w.clone())
    }
}

/// Trim common boundary characters that the platform may include in the
/// "previous word" selection (trailing space the user typed after the
/// abbreviation, NBSP, newlines, …).
fn trim_abbreviation(raw: &str) -> &str {
    raw.trim_matches(|c: char| c.is_whitespace() || c == '\u{00A0}')
}

fn read_clipboard_text() -> Option<String> {
    let ctx = ClipboardContext::new().ok()?;
    ctx.get_text().ok()
}

fn write_clipboard_text(text: &str) -> Result<()> {
    let ctx = ClipboardContext::new().map_err(|e| anyhow!("clipboard ctx init failed: {e:?}"))?;
    ctx.set_text(text.to_string())
        .map_err(|e| anyhow!("set_text failed: {e:?}"))?;
    Ok(())
}

fn restore_clipboard(saved: Option<&str>) {
    if let Some(text) = saved {
        let _ = write_clipboard_text(text);
    }
}

#[cfg(target_os = "macos")]
fn word_modifier() -> Key {
    // On macOS Option == Alt, both physically and in enigo's mapping.
    Key::Alt
}
#[cfg(not(target_os = "macos"))]
fn word_modifier() -> Key {
    Key::Control
}

#[cfg(target_os = "macos")]
fn cmd_modifier() -> Key {
    Key::Meta
}
#[cfg(not(target_os = "macos"))]
fn cmd_modifier() -> Key {
    Key::Control
}

fn select_previous_word() -> Result<()> {
    let mut e = enigo_lock()?;
    let modifier = word_modifier();
    e.key(modifier, Press)
        .map_err(|err| anyhow!("modifier press: {err:?}"))?;
    e.key(Key::Shift, Press)
        .map_err(|err| anyhow!("shift press: {err:?}"))?;
    e.key(Key::LeftArrow, Press)
        .map_err(|err| anyhow!("left press: {err:?}"))?;
    e.key(Key::LeftArrow, Release)
        .map_err(|err| anyhow!("left release: {err:?}"))?;
    e.key(Key::Shift, Release)
        .map_err(|err| anyhow!("shift release: {err:?}"))?;
    e.key(modifier, Release)
        .map_err(|err| anyhow!("modifier release: {err:?}"))?;
    Ok(())
}

fn send_copy() -> Result<()> {
    send_modified_letter('c')
}

fn send_paste() -> Result<()> {
    send_modified_letter('v')
}

/// Synthesize `count` Backspace key presses. Used by `paste_snippet_body`
/// to clear the typed abbreviation before pasting the body — see that
/// function for the design trade-off (blind delete vs. AX read).
fn send_backspaces(count: usize) -> Result<()> {
    if count == 0 {
        return Ok(());
    }
    let mut e = enigo_lock()?;
    for i in 0..count {
        e.key(Key::Backspace, Press)
            .map_err(|err| anyhow!("backspace press: {err:?}"))?;
        e.key(Key::Backspace, Release)
            .map_err(|err| anyhow!("backspace release: {err:?}"))?;
        // Tiny pace gap between events. Without it some apps (notably
        // older Electron + IME-active terminals) coalesce or drop
        // consecutive Backspace presses, leaving a residual character
        // before the snippet body. 4 ms × 20 chars = 80 ms total — too
        // small to notice, big enough that the OS event loop drains
        // each Backspace before the next lands. Skip after the final
        // key so we don't add idle time before the subsequent paste.
        if i + 1 < count {
            thread::sleep(Duration::from_millis(4));
        }
    }
    Ok(())
}

fn send_modified_letter(letter: char) -> Result<()> {
    let mut e = enigo_lock()?;
    let m = cmd_modifier();
    e.key(m, Press)
        .map_err(|err| anyhow!("modifier press: {err:?}"))?;
    e.key(Key::Unicode(letter), Press)
        .map_err(|err| anyhow!("letter press: {err:?}"))?;
    e.key(Key::Unicode(letter), Release)
        .map_err(|err| anyhow!("letter release: {err:?}"))?;
    e.key(m, Release)
        .map_err(|err| anyhow!("modifier release: {err:?}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings;

    #[test]
    fn trim_abbreviation_strips_surrounding_whitespace() {
        assert_eq!(trim_abbreviation("  mfg  "), "mfg");
        assert_eq!(trim_abbreviation("mfg\n"), "mfg");
        assert_eq!(trim_abbreviation("\u{00A0}mfg"), "mfg");
        assert_eq!(trim_abbreviation("mfg"), "mfg");
        assert_eq!(trim_abbreviation(""), "");
        assert_eq!(trim_abbreviation("   "), "");
    }

    #[test]
    fn settings_constants_match_documented_keys() {
        // Sanity check — these strings are referenced from the frontend
        // settings UI, so they're effectively part of our public API.
        assert_eq!(KEY_HOTKEY, "expander.hotkey");
        assert_eq!(KEY_ENABLED, "expander.enabled");
        assert_eq!(DEFAULT_HOTKEY, "Alt+Digit1");
        assert_eq!(LEGACY_DEFAULT_HOTKEY, "Alt+Backquote");
        // The new default must be a string the shortcut parser accepts.
        crate::hotkey::parse_shortcut(DEFAULT_HOTKEY).expect("default hotkey must parse");
    }

    #[test]
    fn migrate_legacy_default_upgrades_untouched_install() {
        use crate::settings;
        use parking_lot::Mutex;
        use rusqlite::Connection;
        use std::sync::Arc;

        let db = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        settings::init_table(&db).unwrap();

        // Untouched old install: hotkey row holds the legacy default.
        settings::set(&db, KEY_HOTKEY, LEGACY_DEFAULT_HOTKEY).unwrap();
        assert_eq!(migrate_legacy_default(&db), DEFAULT_HOTKEY);
        assert_eq!(
            settings::get(&db, KEY_HOTKEY).unwrap().as_deref(),
            Some(DEFAULT_HOTKEY)
        );
        // Idempotent — and won't clobber a value re-picked afterwards.
        settings::set(&db, KEY_HOTKEY, LEGACY_DEFAULT_HOTKEY).unwrap();
        assert_eq!(migrate_legacy_default(&db), LEGACY_DEFAULT_HOTKEY);
    }

    #[test]
    fn migrate_legacy_default_leaves_custom_hotkey_alone() {
        use crate::settings;
        use parking_lot::Mutex;
        use rusqlite::Connection;
        use std::sync::Arc;

        let db = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        settings::init_table(&db).unwrap();
        settings::set(&db, KEY_HOTKEY, "Ctrl+Shift+E").unwrap();
        assert_eq!(migrate_legacy_default(&db), "Ctrl+Shift+E");
        assert_eq!(
            settings::get(&db, KEY_HOTKEY).unwrap().as_deref(),
            Some("Ctrl+Shift+E")
        );
    }

    #[test]
    fn error_sentinel_is_stable() {
        // The hotkey handler matches on these exact strings — bumping
        // any of them is a frontend-coordination break, so pin them.
        assert_eq!(ERR_NO_ACCESSIBILITY, "ax.permission_denied");
        assert_eq!(ERR_SECURE_INPUT, "ax.secure_input_active");
        assert_eq!(ERR_INSPECTOR_FRONTMOST, "ax.inspector_frontmost");
        assert_eq!(ERR_PASSWORD_FIELD, "ax.password_field");
        assert_eq!(ERR_TERMINAL_UNSUPPORTED, "ax.terminal_unsupported");
    }

    #[test]
    fn block_reason_round_trips_through_anyhow() {
        for r in [
            BlockReason::NoAccessibility,
            BlockReason::SecureInput,
            BlockReason::InspectorFrontmost,
            BlockReason::PasswordField,
            BlockReason::TerminalUnsupported,
        ] {
            let e = anyhow::anyhow!(r.to_sentinel());
            assert_eq!(BlockReason::from_error(&e), Some(r));
        }
        // Unrelated errors map to None.
        let e = anyhow::anyhow!("something else");
        assert_eq!(BlockReason::from_error(&e), None);
    }

    #[test]
    fn settings_module_round_trip_for_expander_keys() {
        // Belt-and-braces: ensure the keys we export are usable with the
        // settings store.
        use parking_lot::Mutex;
        use rusqlite::Connection;
        use std::sync::Arc;

        let conn = Connection::open_in_memory().unwrap();
        let db = Arc::new(Mutex::new(conn));
        settings::init_table(&db).unwrap();

        assert_eq!(
            settings::get_or(&db, KEY_HOTKEY, DEFAULT_HOTKEY).unwrap(),
            DEFAULT_HOTKEY
        );
        assert!(settings::get_bool(&db, KEY_ENABLED, false).unwrap() == false);

        settings::set(&db, KEY_HOTKEY, "Ctrl+Shift+E").unwrap();
        settings::set(&db, KEY_ENABLED, "true").unwrap();
        assert_eq!(
            settings::get_or(&db, KEY_HOTKEY, DEFAULT_HOTKEY).unwrap(),
            "Ctrl+Shift+E"
        );
        assert!(settings::get_bool(&db, KEY_ENABLED, false).unwrap());
    }
}
