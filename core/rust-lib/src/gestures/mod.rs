//! Touchpad gestures (BetterTouchTool-style) → existing volume / mute actions.
//!
//! | Gesture            | Action                          |
//! |--------------------|---------------------------------|
//! | 3-finger swipe up  | volume up   (`adjust_system_volume(+step)`) |
//! | 3-finger swipe down| volume down (`adjust_system_volume(-step)`) |
//! | 3-finger tap       | mute toggle (`toggle_system_mute()`)        |
//! | tip-tap right (1 finger resting, a 2nd taps to its RIGHT) | next tab (Ctrl+Tab) |
//! | tip-tap left  (1 finger resting, a 2nd taps to its LEFT)  | previous tab (Ctrl+Shift+Tab) |
//!
//! Design: this module is **platform-independent** and holds the normalized
//! event type, the config, the gesture→action dispatcher, and the pure
//! recognition logic (`classify_swipe` + `Recognizer` state machine) that the
//! self-built Windows HID path feeds raw touch frames into. The OS-specific
//! capture lives behind `#[cfg]` submodules (`linux` = libinput, `windows` =
//! Raw Input HID), each implementing [`GestureSource`]. Gestures are **opt-in**
//! (off by default) and the daemon runs as a tray-resident background thread —
//! no window, no focus needed — mirroring `auto_expand`/`input_lock`.
//
use crate::db::DbHandle;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::Manager;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

// ── Tuning constants (one place, clearly named) ──────────────────────────────

/// Default finger count the gestures fire on.
pub const DEFAULT_FINGERS: u8 = 3;
/// Default volume step per swipe (matches the `Shift+↑/↓` popup step).
pub const DEFAULT_VOLUME_STEP: i32 = 6;

/// A swipe must move at least this fraction of the touchpad (0..1, normalized)
/// to count — used by the self-built Windows recogniser.
pub const SWIPE_THRESHOLD_NORM: f64 = 0.12;
/// A tap may move at most this fraction of the pad (else it's a swipe/drag).
/// Kept tight so a short, quick 3-finger *swipe* (a volume flick) is never
/// mis-read as a tap → spurious mute.
pub const TAP_MAX_MOVE_NORM: f64 = 0.03;
/// A tap must lift within this many milliseconds (else it's a hold/rest).
pub const TAP_MAX_MS: u64 = 250;
/// …and must last at least this long. A single-frame contact (start == peak →
/// `dur == 0`) is a sensor glitch from the private MultitouchSupport feed, not a
/// real tap — without this floor those glitches muted "by themselves".
pub const TAP_MIN_MS: u64 = 10;

// Palm rejection (macOS per-contact recogniser). A palm/heel resting on one
// side of the pad must never count toward the 3-finger gestures — without
// this, palm + 2-finger scroll read as a 3-finger swipe (spurious volume) and
// palm + 2-finger tap as mute. Layered like libinput's palm detection:
/// A contact whose driver-reported `size` reaches this is a palm, immediately
/// and stickily (fingertips ~0.5–1.5 on Apple pads; this is the same
/// MultitouchSupport `Finger.size` field + default threshold Karabiner-Elements
/// ships for its palm rejection). Devices reporting 0 sizes degrade gracefully
/// to the rest/movement rules below.
pub const PALM_SIZE: f32 = 2.0;
/// A contact parked (nearly) motionless at least this long is "resting" (palm
/// heel / anchored thumb) and doesn't count as an active gesture finger. Longer
/// than `TAP_MAX_MS`, so a slow tap can never rest-out mid-gesture.
pub const PALM_REST_MIN_MS: u64 = 600;
/// "Motionless" = total displacement since touch-down below this (a resting
/// palm wobbles slightly; a scrolling finger travels far past it).
pub const PALM_REST_EPS_NORM: f64 = 0.03;
/// At decision time a swipe finger must itself have moved at least this much —
/// in a real 3-finger swipe every finger travels about the centroid distance
/// (≥ `SWIPE_THRESHOLD_NORM`), while a resting palm moves ~0. Half the swipe
/// threshold leaves slack for the outer fingers of a slightly rolling hand.
pub const SWIPE_FINGER_MIN_MOVE_NORM: f64 = 0.06;

// Tip-tap (BetterTouchTool-style "TipTap, 1 finger fix"): one finger rests, a
// second taps briefly to its left/right → previous/next tab.
/// The resting finger must be down alone at least this long before the tap
/// lands — two fingers landing together are a scroll/click, never a tip-tap.
pub const TIPTAP_REST_MIN_MS: u64 = 80;
/// The tapping finger must lift within this window (else it's a two-finger rest).
pub const TIPTAP_TAP_MAX_MS: u64 = 300;
/// …and must be down at least this long. 40 ms also filters MT state flicker
/// (a lightly-resting finger can bounce between touching/hover states frame to
/// frame, which looks like machine-gun micro-taps).
pub const TIPTAP_TAP_MIN_MS: u64 = 40;
/// Max movement (normalized) either finger may make during the tap — more is a
/// scroll/pinch, and a drifting rest finger re-arms its settle timer.
pub const TIPTAP_MAX_MOVE_NORM: f64 = 0.05;
/// The tap must land at least this far (|Δx|, normalized) from the resting
/// finger for the left/right decision to be reliable.
pub const TIPTAP_MIN_SEP_NORM: f64 = 0.03;
/// Refractory period between two tip-tap emits. A physical tap's lift can
/// "bounce" (the contact re-appears for a frame or two) — without this gap one
/// tap could fire several tab switches ("apps jump around wildly"). Bounce is
/// primarily blocked by the post-emit settle (`TIPTAP_REST_MIN_MS`); this gap
/// is belt-and-braces, so it's kept short — 200 ms still allows ~5 deliberate
/// chained taps/s (350 ms swallowed rapid taps and read as "laggy").
pub const TIPTAP_EMIT_GAP_MS: u64 = 200;
/// The tap must land at a roughly similar HEIGHT as the resting finger
/// (|Δy|, 0..1). Generous — strongly angled hands are fine (the runaway
/// thumb-anchor posture is caught by the bottom-edge zone below, so mid-pad
/// height differences carry little false-positive risk). 0.22 and even 0.35
/// rejected real tip-taps whenever one finger sat higher on the pad.
pub const TIPTAP_MAX_DY_NORM: f64 = 0.55;
/// STRICTER Δy when either contact sits in the pad's bottom-edge **thumb
/// zone** — a thumb anchored there while the index finger points is normal
/// cursor use, never a tip-tap (the runaway-tab-switching posture). Deliberate
/// tip-taps done low on the pad have both fingers in the zone → small Δy → OK.
pub const TIPTAP_THUMB_ZONE_Y: f64 = 0.80;
pub const TIPTAP_THUMB_DY_NORM: f64 = 0.18;
/// …and not implausibly far sideways (adjacent fingertips, not a wide pinch).
pub const TIPTAP_MAX_DX_NORM: f64 = 0.40;

// ── Normalized gesture event ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureKind {
    SwipeUp,
    SwipeDown,
    SwipeLeft,
    SwipeRight,
    Tap,
    /// One finger resting, a second tapped to its LEFT (→ previous tab).
    TipTapLeft,
    /// One finger resting, a second tapped to its RIGHT (→ next tab).
    TipTapRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GestureEvent {
    pub kind: GestureKind,
    pub fingers: u8,
}

// ── Config ───────────────────────────────────────────────────────────────────

const KEY_ENABLED: &str = "gestures.enabled";
const KEY_VOLUME_STEP: &str = "gestures.volume_step";
const KEY_TIPTAP: &str = "gestures.tiptap";

fn default_false() -> bool {
    false
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct GestureConfig {
    pub enabled: bool,
    pub fingers: u8,
    pub volume_step: i32,
    /// Tip-tap tab switching (rest one finger, tap another left/right).
    /// **Opt-in (default off)**: normal thumb-anchored trackpad use (thumb
    /// resting low while the index finger points) reads as rest+tap and caused
    /// runaway tab switching for users who never wanted the gesture.
    #[serde(default = "default_false")]
    pub tiptap: bool,
}

impl Default for GestureConfig {
    fn default() -> Self {
        GestureConfig {
            enabled: false, // opt-in
            fingers: DEFAULT_FINGERS,
            volume_step: DEFAULT_VOLUME_STEP,
            tiptap: false, // opt-in (see the field doc — accidental-trigger risk)
        }
    }
}

impl GestureConfig {
    pub fn load(db: &DbHandle) -> GestureConfig {
        let d = GestureConfig::default();
        GestureConfig {
            enabled: crate::settings::get_bool(db, KEY_ENABLED, d.enabled).unwrap_or(d.enabled),
            fingers: d.fingers,
            volume_step: crate::settings::get_or(db, KEY_VOLUME_STEP, &d.volume_step.to_string())
                .ok()
                .and_then(|s| s.parse().ok())
                .filter(|v: &i32| *v > 0 && *v <= 50)
                .unwrap_or(d.volume_step),
            tiptap: crate::settings::get_bool(db, KEY_TIPTAP, d.tiptap).unwrap_or(d.tiptap),
        }
    }

    pub fn save(&self, db: &DbHandle) -> anyhow::Result<()> {
        crate::settings::set(db, KEY_ENABLED, if self.enabled { "true" } else { "false" })?;
        crate::settings::set(db, KEY_VOLUME_STEP, &self.volume_step.to_string())?;
        crate::settings::set(db, KEY_TIPTAP, if self.tiptap { "true" } else { "false" })?;
        Ok(())
    }
}

// ── Dispatcher: gesture → action ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureAction {
    VolumeUp,
    VolumeDown,
    MuteToggle,
    NextTab,
    PrevTab,
}

/// Pure mapping (config-gated). Fixed bindings for now, but isolated here so a
/// future remap UI only has to change this function. Returns `None` when
/// gestures are off, the finger count doesn't match, or the gesture is unbound.
pub fn map_action(ev: &GestureEvent, cfg: &GestureConfig) -> Option<GestureAction> {
    if !cfg.enabled {
        return None;
    }
    // Tip-taps are inherently two-finger; the `fingers` count only gates the
    // swipe/tap family.
    match ev.kind {
        GestureKind::TipTapLeft => cfg.tiptap.then_some(GestureAction::PrevTab),
        GestureKind::TipTapRight => cfg.tiptap.then_some(GestureAction::NextTab),
        _ if ev.fingers != cfg.fingers => None,
        GestureKind::SwipeUp => Some(GestureAction::VolumeUp),
        GestureKind::SwipeDown => Some(GestureAction::VolumeDown),
        GestureKind::Tap => Some(GestureAction::MuteToggle),
        GestureKind::SwipeLeft | GestureKind::SwipeRight => None,
    }
}

/// Perform an action via the existing volume / mute pipeline + show a passive,
/// centred on-screen toast (the gesture's only visible feedback, since macOS
/// shows no HUD for programmatic volume changes). Runs on a worker thread so the
/// capture callback never blocks; the toast is shown on the main thread (window
/// op). Rapid re-triggers reuse the same toast (it updates in place — see
/// `StatusToast.tsx`), they don't re-pop.
fn perform(app: &tauri::AppHandle, action: GestureAction, step: i32) {
    tracing::debug!("gesture action: {action:?} (step {step})");
    // Tab switching: send the frontmost app's OWN tab-nav shortcut (data-driven
    // per-app map). No toast — the visibly switching tab is the feedback. Runs
    // INLINE on the capture thread for minimal latency; layout-dependent chars
    // come from the prewarmed cache (see `prewarm_tab_keys`).
    if matches!(action, GestureAction::NextTab | GestureAction::PrevTab) {
        #[cfg(target_os = "macos")]
        dispatch_tab_switch(app, matches!(action, GestureAction::NextTab));
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let toast = match action {
            GestureAction::VolumeUp | GestureAction::VolumeDown => {
                let delta = if matches!(action, GestureAction::VolumeUp) { step } else { -step };
                let level = crate::system_commands::nudge_volume(delta);
                crate::status_toast::StatusToast {
                    kind: "volume".into(),
                    on: level.map(|l| l > 0).unwrap_or(true),
                    // Title carries the level so the frontend can draw the bar;
                    // falls back to a direction arrow when the OS gives no read-back.
                    title: level
                        .map(|l| format!("{l}%"))
                        .unwrap_or_else(|| if delta > 0 { "+".into() } else { "−".into() }),
                    subtitle: "Volume".into(),
                }
            }
            GestureAction::MuteToggle => {
                let muted = crate::system_commands::toggle_system_mute().unwrap_or(true);
                crate::status_toast::StatusToast {
                    kind: "mute".into(),
                    on: muted,
                    title: if muted { "Muted".into() } else { "Unmuted".into() },
                    subtitle: "Volume".into(),
                }
            }
            // Handled above (early return) — kept for match exhaustiveness.
            GestureAction::NextTab | GestureAction::PrevTab => return,
        };
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            // If the main popup is open, keep it open: the toast briefly takes key
            // focus, which would otherwise trip the popup's focus-loss auto-hide.
            keep_popup_open_during_toast(&app2);
            crate::status_toast::show_passive(&app2, toast);
        });
    });
}

/// One synthesizable key chord (from `assets/tab_shortcuts.json` or the user
/// override). `key` is `"tab"` / `"left"` / `"right"` or a single character —
/// characters are resolved for the CURRENT keyboard layout at gesture time
/// (German: `]` = physical „6" + ⌥), named keys use fixed virtual keycodes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct KeyChord {
    pub key: String,
    #[serde(default)]
    pub mods: Vec<String>,
}

/// A per-app entry: bundle-id prefix → the app's own next/prev-tab chords.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TabAppEntry {
    pub prefix: String,
    pub next: KeyChord,
    pub prev: KeyChord,
}

/// The whole map: bundled defaults (`assets/tab_shortcuts.json`), optionally
/// overlaid by a user file of the same shape at `<data-dir>/tab-shortcuts.json`
/// (its entries are checked FIRST; its `default` replaces the bundled one).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TabShortcutMap {
    #[serde(default)]
    pub apps: Vec<TabAppEntry>,
    pub default: TabDefault,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TabDefault {
    pub next: KeyChord,
    pub prev: KeyChord,
}

/// The bundled map — a compile-time asset so a bad edit fails the build/tests,
/// not the user. Unlisted apps get the system-standard Ctrl+Tab default.
pub const TAB_SHORTCUTS_JSON: &str = include_str!("../../assets/tab_shortcuts.json");

pub fn parse_tab_map(json: &str) -> Result<TabShortcutMap, serde_json::Error> {
    serde_json::from_str(json)
}

/// Merge `user` over `built_in`: user entries match first, user default wins.
pub fn merge_tab_maps(built_in: TabShortcutMap, user: Option<TabShortcutMap>) -> TabShortcutMap {
    match user {
        None => built_in,
        Some(u) => TabShortcutMap {
            apps: u.apps.into_iter().chain(built_in.apps).collect(),
            default: u.default,
        },
    }
}

/// The chords for `bundle_id` (first prefix match wins, else the default).
pub fn tab_chords_for<'a>(map: &'a TabShortcutMap, bundle_id: &str) -> (&'a KeyChord, &'a KeyChord) {
    for e in &map.apps {
        if bundle_id.starts_with(&e.prefix) {
            return (&e.next, &e.prev);
        }
    }
    (&map.default.next, &map.default.prev)
}

/// The effective map: bundled + user override, loaded once per app run (edit
/// the override file → restart to apply). Never fails — the bundled JSON is
/// validated by a unit test, and a broken user file is logged + ignored.
#[cfg(target_os = "macos")]
fn effective_tab_map() -> &'static TabShortcutMap {
    use std::sync::OnceLock;
    static MAP: OnceLock<TabShortcutMap> = OnceLock::new();
    MAP.get_or_init(|| {
        let built_in = parse_tab_map(TAB_SHORTCUTS_JSON).unwrap_or_else(|e| {
            tracing::error!("gestures: bundled tab_shortcuts.json invalid: {e}");
            TabShortcutMap {
                apps: Vec::new(),
                default: TabDefault {
                    next: KeyChord { key: "tab".into(), mods: vec!["ctrl".into()] },
                    prev: KeyChord { key: "tab".into(), mods: vec!["ctrl".into(), "shift".into()] },
                },
            }
        });
        let user = crate::db::default_db_path()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("tab-shortcuts.json")))
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|txt| match parse_tab_map(&txt) {
                Ok(m) => {
                    tracing::info!("gestures: user tab-shortcuts.json loaded ({} entries)", m.apps.len());
                    Some(m)
                }
                Err(e) => {
                    tracing::warn!("gestures: user tab-shortcuts.json invalid — ignored: {e}");
                    None
                }
            });
        merge_tab_maps(built_in, user)
    })
}

// ── Tab-switch key synthesis (macOS) ─────────────────────────────────────────
// Latency-critical: a tip-tap should feel instant. Everything is resolved
// inline on the capture thread — the map is a OnceLock, the frontmost bundle a
// snapshot NSWorkspace read, and layout-dependent characters come from a cache
// that `prewarm_tab_keys` fills ON THE MAIN THREAD at gesture-source start (TIS
// is a main-thread API). Only an uncached char (e.g. after a user-override edit)
// takes the one-time main-thread hop; CGEventPost itself is thread-safe.

#[cfg(target_os = "macos")]
mod tab_keys {
    use std::ffi::c_void;
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreateKeyboardEvent(
            source: *mut c_void,
            keycode: u16,
            keydown: bool,
        ) -> *mut c_void;
        fn CGEventSetFlags(event: *mut c_void, flags: u64);
        fn CGEventPost(tap: u32, event: *mut c_void);
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        // Signature matches audio.rs's declaration (clashing_extern_declarations).
        fn CFRelease(cf: *const c_void);
    }
    pub const KEY_TAB: u16 = 48;
    pub const KEY_LEFT: u16 = 123;
    pub const KEY_RIGHT: u16 = 124;
    pub const FLAG_SHIFT: u64 = 0x0002_0000;
    pub const FLAG_CONTROL: u64 = 0x0004_0000;
    pub const FLAG_ALT: u64 = 0x0008_0000;
    pub const FLAG_CMD: u64 = 0x0010_0000;
    pub const FLAG_NUMPAD: u64 = 0x0020_0000; // real arrow keys carry NumericPad…
    pub const FLAG_FN: u64 = 0x0080_0000; // …and Fn — some apps (iTerm2) match exactly
    const HID_TAP: u32 = 0;

    /// Post one keydown+keyup chord. Thread-safe (raw CGEventPost).
    pub fn post(keycode: u16, flags: u64) {
        unsafe {
            for down in [true, false] {
                let ev = CGEventCreateKeyboardEvent(std::ptr::null_mut(), keycode, down);
                if ev.is_null() {
                    return;
                }
                CGEventSetFlags(ev, flags);
                CGEventPost(HID_TAP, ev);
                CFRelease(ev as *const c_void);
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn chord_mod_flags(mods: &[String]) -> u64 {
    use tab_keys::*;
    mods.iter()
        .map(|m| match m.as_str() {
            "cmd" => FLAG_CMD,
            "shift" => FLAG_SHIFT,
            "ctrl" => FLAG_CONTROL,
            "alt" => FLAG_ALT,
            _ => 0,
        })
        .fold(0, |a, b| a | b)
}

/// Layout-resolved character keys, filled on the main thread (TIS). Keyed by
/// char; value = (virtual keycode, the char's own extra modifier flags).
/// The keyboard layout rarely changes mid-session; after a layout switch the
/// cache refreshes on the next prewarm (source restart) — documented trade-off.
#[cfg(target_os = "macos")]
static LAYOUT_KEY_CACHE: parking_lot::Mutex<Vec<(char, (u16, u64))>> =
    parking_lot::Mutex::new(Vec::new());

/// Resolve every single-char key in the effective map ON THE MAIN THREAD and
/// fill [`LAYOUT_KEY_CACHE`], so gestures never pay the TIS cost (or a
/// main-thread hop) at tap time. Called from `apply` right after the source
/// starts. Also forces the map OnceLock (disk read for the user override) so
/// the first gesture is as fast as every other.
#[cfg(target_os = "macos")]
fn prewarm_tab_keys(app: &tauri::AppHandle) {
    let _ = app.run_on_main_thread(|| {
        let map = effective_tab_map();
        let mut chars: Vec<char> = Vec::new();
        for e in &map.apps {
            for c in [&e.next, &e.prev] {
                if !matches!(c.key.as_str(), "tab" | "left" | "right") {
                    if let Some(ch) = c.key.chars().next() {
                        if !chars.contains(&ch) {
                            chars.push(ch);
                        }
                    }
                }
            }
        }
        let mut cache = LAYOUT_KEY_CACHE.lock();
        cache.clear();
        for ch in chars {
            if let Some(resolved) = key_for_char(ch) {
                cache.push((ch, resolved));
            }
        }
        tracing::debug!("gestures: prewarmed {} layout key(s) for tab chords", cache.len());
    });
}

/// Send the right tab-switch shortcut **for the frontmost app**. Runs INLINE on
/// the capture thread (fast path — no main-thread hop): map + bundle lookup are
/// snapshot reads, chars come from the prewarmed cache. Only an uncached char
/// (user-override edited mid-session) falls back to a one-time main-thread
/// resolve. Needs the Accessibility grant gestures already use.
#[cfg(target_os = "macos")]
fn dispatch_tab_switch(app: &tauri::AppHandle, next: bool) {
    use tab_keys::*;
    let map = effective_tab_map();
    let bundle = frontmost_bundle_id().unwrap_or_default();
    let (n, p) = tab_chords_for(map, &bundle);
    let chord = if next { n } else { p };
    let flags = chord_mod_flags(&chord.mods);
    match chord.key.as_str() {
        "tab" => tab_keys::post(KEY_TAB, flags),
        "left" => tab_keys::post(KEY_LEFT, flags | FLAG_NUMPAD | FLAG_FN),
        "right" => tab_keys::post(KEY_RIGHT, flags | FLAG_NUMPAD | FLAG_FN),
        other => {
            let Some(target) = other.chars().next().filter(|_| other.chars().count() == 1) else {
                tracing::warn!("gestures: bad tab chord key {other:?} — falling back to Ctrl+Tab");
                tab_keys::post(KEY_TAB, FLAG_CONTROL | if next { 0 } else { FLAG_SHIFT });
                return;
            };
            // Fast path: prewarmed layout cache → inline post.
            if let Some((code, extra)) =
                LAYOUT_KEY_CACHE.lock().iter().find(|(c, _)| *c == target).map(|(_, r)| *r)
            {
                tab_keys::post(code, flags | extra);
                return;
            }
            // Miss (edited override mid-session): resolve once on the main
            // thread (TIS requirement), cache, post.
            let app = app.clone();
            let _ = app.run_on_main_thread(move || match key_for_char(target) {
                Some((code, extra)) => {
                    LAYOUT_KEY_CACHE.lock().push((target, (code, extra)));
                    tab_keys::post(code, flags | extra);
                }
                None => tab_keys::post(KEY_TAB, FLAG_CONTROL | if next { 0 } else { FLAG_SHIFT }),
            });
        }
    }
}

/// Bundle id of the frontmost app via `NSWorkspace` (no permission needed).
#[cfg(target_os = "macos")]
fn frontmost_bundle_id() -> Option<String> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    unsafe {
        let ws_cls = AnyClass::get(c"NSWorkspace")?;
        let ws: *mut AnyObject = msg_send![ws_cls, sharedWorkspace];
        if ws.is_null() {
            return None;
        }
        let app: *mut AnyObject = msg_send![ws, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let bid: *mut AnyObject = msg_send![app, bundleIdentifier];
        if bid.is_null() {
            return None;
        }
        let utf8: *const std::os::raw::c_char = msg_send![bid, UTF8String];
        if utf8.is_null() {
            return None;
        }
        Some(std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }
}

/// Find `(virtual keycode, extra CGEvent modifier flags)` that produce `target`
/// under the **current keyboard layout** (TIS + `UCKeyTranslate`, scanning
/// keycodes 0–127 × {∅, ⇧, ⌥, ⇧⌥}). German layout: `]` → `(22 /*"6"*/, ⌥)`.
/// `None` when the layout data is unavailable (IMEs) → caller falls back.
#[cfg(target_os = "macos")]
fn key_for_char(target: char) -> Option<(u16, u64)> {
    use std::ffi::c_void;
    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut c_void;
        fn TISGetInputSourceProperty(src: *mut c_void, key: *const c_void) -> *mut c_void;
        static kTISPropertyUnicodeKeyLayoutData: *const c_void;
        fn CFDataGetBytePtr(data: *mut c_void) -> *const u8;
        fn LMGetKbdType() -> u8;
        fn UCKeyTranslate(
            layout: *const c_void,
            vkey: u16,
            action: u16,
            modifier_key_state: u32,
            kbd_type: u32,
            options: u32,
            dead_key_state: *mut u32,
            max_len: usize,
            actual_len: *mut usize,
            unicode_string: *mut u16,
        ) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
    }
    const FLAG_SHIFT: u64 = 0x0002_0000;
    const FLAG_ALT: u64 = 0x0008_0000;
    // (Carbon modifier-key-state bits >> 8: shift = 2, option = 8.)
    const COMBOS: [(u32, u64); 4] = [
        (0, 0),
        (2, FLAG_SHIFT),
        (8, FLAG_ALT),
        (10, FLAG_SHIFT | FLAG_ALT),
    ];
    unsafe {
        let src = TISCopyCurrentKeyboardLayoutInputSource();
        if src.is_null() {
            return None;
        }
        let data = TISGetInputSourceProperty(src, kTISPropertyUnicodeKeyLayoutData);
        if data.is_null() {
            CFRelease(src as *const c_void);
            return None;
        }
        let layout = CFDataGetBytePtr(data) as *const c_void;
        let kbd_type = LMGetKbdType() as u32;
        let mut found: Option<(u16, u64)> = None;
        'outer: for (mod_state, extra_flags) in COMBOS {
            for vkey in 0u16..128 {
                let mut dead: u32 = 0;
                let mut len: usize = 0;
                let mut chars = [0u16; 4];
                // action 0 = key down; options bit 0 = no dead keys.
                let err = UCKeyTranslate(
                    layout, vkey, 0, mod_state, kbd_type, 1, &mut dead, 4, &mut len, chars.as_mut_ptr(),
                );
                if err == 0 && len == 1 && chars[0] as u32 == target as u32 {
                    found = Some((vkey, extra_flags));
                    break 'outer;
                }
            }
        }
        CFRelease(src as *const c_void);
        found
    }
}

static SUPPRESS_GEN: AtomicU64 = AtomicU64::new(0);

/// When the main popup is open, briefly suppress its focus-loss auto-hide so a
/// volume/mute gesture (whose passive toast momentarily takes key focus) doesn't
/// close it. No-op when the popup isn't open, and it never stomps a suppression
/// another feature (native dialog, pinned detector) already holds.
fn keep_popup_open_during_toast(app: &tauri::AppHandle) {
    let visible = app
        .get_webview_window(crate::hotkey::POPUP_LABEL)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    if !visible {
        return;
    }
    let Some(ui) = app.try_state::<crate::ui_state::UiState>() else {
        return;
    };
    // Only manage the flag if we're the one turning it on (false → true).
    if ui.suppress_hide.swap(true, Ordering::SeqCst) {
        return;
    }
    // Clear after the focus bounce settles — gen-guarded so a rapid burst of
    // gestures only clears once, after the last one.
    let generation = SUPPRESS_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(600));
        if SUPPRESS_GEN.load(Ordering::SeqCst) == generation {
            if let Some(ui) = app.try_state::<crate::ui_state::UiState>() {
                ui.suppress_hide.store(false, Ordering::SeqCst);
            }
        }
    });
}

// ── Source abstraction ───────────────────────────────────────────────────────

/// Callback the platform source invokes for every recognised gesture.
pub type GestureSink = Box<dyn Fn(GestureEvent) + Send + Sync + 'static>;

/// A platform-specific gesture capture backend (libinput / Raw Input HID / …).
pub trait GestureSource: Send {
    /// Begin capturing; deliver recognised gestures to `sink`. Returns an error
    /// if the backend can't initialise (e.g. no Precision Touchpad / no
    /// `/dev/input` access) so the caller can degrade gracefully.
    fn start(&mut self, cfg: GestureConfig, sink: GestureSink) -> Result<(), String>;
    fn stop(&mut self);
}

/// The OS gesture source for this platform, or `None` where unsupported.
fn platform_source() -> Option<Box<dyn GestureSource>> {
    #[cfg(target_os = "linux")]
    {
        return Some(Box::new(linux::LinuxGestureSource::new()));
    }
    #[cfg(target_os = "macos")]
    {
        return Some(Box::new(macos::MacGestureSource::new()));
    }
    #[cfg(target_os = "windows")]
    {
        return Some(Box::new(windows::WindowsGestureSource::new()));
    }
    #[allow(unreachable_code)]
    None
}

// ── Pure recognition ─────────────────────────────────────────────────────────

/// Classify a swipe from its total displacement. `dy < 0` is **up** (HID/screen
/// convention: y grows downward). Returns `None` below `threshold`. Pure.
pub fn classify_swipe(dx: f64, dy: f64, threshold: f64) -> Option<GestureKind> {
    if dx.abs().max(dy.abs()) < threshold {
        return None;
    }
    if dy.abs() >= dx.abs() {
        Some(if dy < 0.0 { GestureKind::SwipeUp } else { GestureKind::SwipeDown })
    } else {
        Some(if dx < 0.0 { GestureKind::SwipeLeft } else { GestureKind::SwipeRight })
    }
}

/// One touchpad frame (HID): contact count + the contacts' centroid, normalized
/// to 0..1 over the pad's logical range, with a millisecond timestamp.
/// Only constructed by the Windows Raw-Input path (+ tests) — macOS moved to
/// the per-contact [`PalmAwareRecognizer`].
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Debug, Clone, Copy)]
pub struct TouchFrame {
    pub contacts: u8,
    pub x: f64,
    pub y: f64,
    pub t_ms: u64,
}

/// Self-built recogniser for the Windows Raw-Input path: fed a stream of touch
/// frames, it emits a [`GestureEvent`] when all fingers lift. To avoid the
/// centroid skewing as fingers are released, the end position is taken from the
/// last frame at the gesture's **peak** finger count, not the lift frame.
/// (Windows-only at runtime; macOS uses [`PalmAwareRecognizer`].)
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Debug, Default)]
pub struct Recognizer {
    active: bool,
    start: (f64, f64, u64),
    peak: (f64, f64, u64), // last frame at max_contacts
    max_contacts: u8,
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
impl Recognizer {
    pub fn new() -> Self {
        Recognizer::default()
    }

    /// Feed one frame. Returns `Some(event)` exactly once, on the lift that ends
    /// a gesture (`contacts` back to 0).
    pub fn feed(&mut self, f: TouchFrame) -> Option<GestureEvent> {
        if f.contacts > 0 {
            if !self.active {
                self.active = true;
                self.start = (f.x, f.y, f.t_ms);
                self.peak = self.start;
                self.max_contacts = f.contacts;
            } else if f.contacts > self.max_contacts {
                // A finger joined → the centroid jumps as it averages one more
                // contact. Re-baseline `start` (and `peak`) to here so the swipe
                // is measured only across the stable peak-finger-count phase and
                // the join-jump doesn't corrupt the direction. (Fingers rarely
                // land simultaneously — real frames go 0→2→3.)
                self.max_contacts = f.contacts;
                self.start = (f.x, f.y, f.t_ms);
                self.peak = self.start;
            }
            if f.contacts >= self.max_contacts {
                self.peak = (f.x, f.y, f.t_ms);
            }
            None
        } else {
            if !self.active {
                return None;
            }
            self.active = false;
            let dx = self.peak.0 - self.start.0;
            let dy = self.peak.1 - self.start.1;
            let dur = self.peak.2.saturating_sub(self.start.2);
            let fingers = self.max_contacts;
            let moved = dx.hypot(dy);
            if (TAP_MIN_MS..=TAP_MAX_MS).contains(&dur) && moved <= TAP_MAX_MOVE_NORM {
                return Some(GestureEvent { kind: GestureKind::Tap, fingers });
            }
            classify_swipe(dx, dy, SWIPE_THRESHOLD_NORM)
                .map(|kind| GestureEvent { kind, fingers })
        }
    }
}

// ── Palm-aware per-contact recognition (pure; the macOS path) ────────────────

/// One raw per-contact sample from the platform layer: the driver's stable
/// contact id, normalized position (y already flipped to screen convention,
/// like [`TouchFrame`]), and the driver-reported contact `size`.
#[derive(Debug, Clone, Copy)]
pub struct RawContact {
    pub id: i32,
    pub x: f64,
    pub y: f64,
    pub size: f32,
}

#[derive(Debug, Clone, Copy)]
struct PTrack {
    id: i32,
    start: (f64, f64),
    t_down: u64,
    last: (f64, f64),
    t_last: u64,
    present: bool,
    /// Sticky size-based palm flag (once a palm, always a palm until lift).
    palm: bool,
}

impl PTrack {
    fn disp(&self) -> f64 {
        (self.last.0 - self.start.0).hypot(self.last.1 - self.start.1)
    }
    /// Parked motionless long enough to be a resting palm heel / anchored thumb.
    fn resting(&self, now: u64) -> bool {
        now.saturating_sub(self.t_down) >= PALM_REST_MIN_MS && self.disp() < PALM_REST_EPS_NORM
    }
    fn active(&self, now: u64) -> bool {
        self.present && !self.palm && !self.resting(now)
    }
}

/// Per-contact recogniser with **palm rejection** — replaces the centroid
/// [`Recognizer`] on macOS, where the private MultitouchSupport feed gives us
/// per-contact ids + sizes. (Windows keeps `Recognizer`: its Raw-Input path only
/// has the centroid, and Precision-Touchpad firmware already rejects palms.)
///
/// Three layered guards, mirroring libinput / Karabiner-Elements:
/// 1. **Size**: a contact with `size ≥ PALM_SIZE` never counts (sticky).
/// 2. **Rest**: a contact parked ≥ `PALM_REST_MIN_MS` with < `PALM_REST_EPS_NORM`
///    total movement is a parked palm/thumb — it neither counts toward the
///    active-finger count nor blocks a gesture from *other* fingers (a real
///    3-finger swipe fires even while the palm stays down).
/// 3. **Per-finger movement at decision time**: only contacts that themselves
///    moved ≥ `SWIPE_FINGER_MIN_MOVE_NORM` count as swipe fingers, and only
///    contacts down ≤ `TAP_MAX_MS` count as tap fingers — so palm + 2-finger
///    scroll yields `fingers == 2`, which `map_action` ignores.
///
/// A gesture is decided when the active-finger count falls back to 0 — which,
/// unlike the all-lift rule, also happens while a palm remains resting.
#[derive(Debug, Default)]
pub struct PalmAwareRecognizer {
    tracks: Vec<PTrack>,
    prev_active: usize,
}

impl PalmAwareRecognizer {
    pub fn new() -> Self {
        PalmAwareRecognizer::default()
    }

    /// Active (present, non-palm, non-resting) contact count as of the last
    /// `feed` — the platform layer arms its scroll-consume window on this, so a
    /// parked palm + 2-finger scroll no longer swallows the legitimate scroll.
    pub fn active_fingers(&self) -> usize {
        self.prev_active
    }

    /// Feed one frame (every currently-touching contact). Returns `Some(event)`
    /// exactly once per gesture, when the last active finger lifts (or parks).
    pub fn feed(&mut self, t_ms: u64, contacts: &[RawContact]) -> Option<GestureEvent> {
        for t in &mut self.tracks {
            t.present = false;
        }
        for c in contacts {
            let palm = c.size >= PALM_SIZE;
            if let Some(t) = self.tracks.iter_mut().find(|t| t.id == c.id) {
                t.last = (c.x, c.y);
                t.t_last = t_ms;
                t.present = true;
                t.palm |= palm;
            } else {
                self.tracks.push(PTrack {
                    id: c.id,
                    start: (c.x, c.y),
                    t_down: t_ms,
                    last: (c.x, c.y),
                    t_last: t_ms,
                    present: true,
                    palm,
                });
            }
        }
        let active = self.tracks.iter().filter(|t| t.active(t_ms)).count();
        let ev = if self.prev_active > 0 && active == 0 { self.decide(t_ms) } else { None };
        self.prev_active = active;
        if active == 0 {
            // Gesture over (or none in progress): drop lifted tracks — the
            // decision above already consumed them. Parked palms stay tracked.
            self.tracks.retain(|t| t.present);
        }
        ev
    }

    fn decide(&self, now: u64) -> Option<GestureEvent> {
        let mut moved_n = 0usize;
        let (mut dx, mut dy) = (0.0f64, 0.0f64);
        let mut taps = 0usize;
        let mut latest_down = 0u64;
        let mut earliest_up = u64::MAX;
        for t in &self.tracks {
            // Skip size-palms and STILL-PRESENT resters (a palm that lifted
            // together with the fingers passes this gate, but its near-zero
            // movement / long down-time disqualifies it below anyway).
            if t.palm || (t.present && t.resting(now)) {
                continue;
            }
            if t.disp() >= SWIPE_FINGER_MIN_MOVE_NORM {
                moved_n += 1;
                dx += t.last.0 - t.start.0;
                dy += t.last.1 - t.start.1;
            } else if t.disp() <= TAP_MAX_MOVE_NORM {
                taps += 1;
                latest_down = latest_down.max(t.t_down);
                earliest_up = earliest_up.min(t.t_last);
            }
        }
        if moved_n > 0 {
            let n = moved_n as f64;
            return classify_swipe(dx / n, dy / n, SWIPE_THRESHOLD_NORM)
                .map(|kind| GestureEvent { kind, fingers: moved_n as u8 });
        }
        if taps == 0 {
            return None;
        }
        // Tap: judged over the ALL-FINGERS-DOWN overlap window — the same
        // phase the old centroid recogniser measured (its re-baselined
        // max-contact span). Per-finger total durations are deliberately NOT
        // the gate (the v0.84.245 fix): real 3-finger taps land + lift
        // staggered, so requiring every finger individually inside
        // [TAP_MIN..TAP_MAX] kept dropping one finger (a one-frame ghost at
        // the low end, a lazy >250 ms contact at the high end) — the event
        // then read as a 2-finger tap and the 3-finger mute never fired. A
        // held chord still can't tap: its overlap window is the full hold.
        let overlap = earliest_up.saturating_sub(latest_down);
        ((TAP_MIN_MS..=TAP_MAX_MS).contains(&overlap))
            .then_some(GestureEvent { kind: GestureKind::Tap, fingers: taps as u8 })
    }
}

// ── Tip-tap recognition (pure) ───────────────────────────────────────────────

/// One touch contact, normalized to 0..1 over the pad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact {
    pub x: f64,
    pub y: f64,
}

fn dist(a: Contact, b: Contact) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

#[derive(Debug, Clone, Copy)]
enum TtState {
    Idle,
    /// One finger down, settling/resting.
    Resting { rest: Contact, since: u64 },
    /// Rest + a second (tap) finger down.
    TapDown { rest: Contact, tap: Contact, tap_start_x: f64, started: u64 },
    /// The tap finger just lifted (rest still down) — emit is DEFERRED one frame
    /// to confirm the lift. If the tap finger re-appears next frame it was a
    /// mid-hold contact flicker (not a real lift) → back to `TapDown`, no emit;
    /// this stops one physical tap from firing twice (jump-to-the-tab-after-next
    /// bug, v0.84.257). Only a *confirmed* lift emits.
    TapReleasing { rest: Contact, tap_start_x: f64, started: u64, lift_t: u64 },
    /// Disqualified (scroll/pinch/3+ fingers) — wait for all-lift.
    Poisoned,
}

/// BetterTouchTool-style **TipTap (1 finger fix)** recogniser: one finger rests
/// on the pad, a second taps briefly to its left/right → [`GestureKind::
/// TipTapLeft`]/[`TipTapRight`]. Pure + unit-tested; fed per-frame with every
/// contact's position (unordered). Guards: the rest finger must be down alone
/// ≥ `TIPTAP_REST_MIN_MS` first (kills two-finger scroll/click, whose fingers
/// land together), the tap must lift within `TIPTAP_TAP_MAX_MS`, and any
/// movement > `TIPTAP_MAX_MOVE_NORM` by either finger disqualifies the attempt.
/// Taps chain: the rest finger can stay down and tap repeatedly.
#[derive(Debug, Default)]
pub struct TipTapRecognizer {
    state: Option<TtState>,
    /// Timestamp of the last emit (refractory guard against contact bounce).
    last_emit: Option<u64>,
}

impl TipTapRecognizer {
    pub fn new() -> Self {
        TipTapRecognizer::default()
    }

    pub fn feed(&mut self, t_ms: u64, contacts: &[Contact]) -> Option<GestureKind> {
        let state = self.state.take().unwrap_or(TtState::Idle);
        let (next, emit) = Self::step(state, t_ms, contacts);
        self.state = Some(next);
        // Refractory: a tap's lift can bounce (contact re-appears for a frame),
        // which would re-run the whole tap cycle — cap the emit rate instead of
        // trusting every cycle.
        if emit.is_some() {
            if let Some(last) = self.last_emit {
                if t_ms.saturating_sub(last) < TIPTAP_EMIT_GAP_MS {
                    return None;
                }
            }
            self.last_emit = Some(t_ms);
        }
        emit
    }

    fn step(state: TtState, t_ms: u64, c: &[Contact]) -> (TtState, Option<GestureKind>) {
        match state {
            TtState::Idle => match c.len() {
                0 => (TtState::Idle, None),
                1 => (TtState::Resting { rest: c[0], since: t_ms }, None),
                _ => (TtState::Poisoned, None),
            },
            TtState::Resting { rest, since } => match c.len() {
                0 => (TtState::Idle, None),
                1 => {
                    // Track the rest finger; big movement (cursor drag) re-arms
                    // the settle timer so a tap right after a drag can't fire.
                    let moved = dist(c[0], rest) > TIPTAP_MAX_MOVE_NORM;
                    (
                        TtState::Resting { rest: c[0], since: if moved { t_ms } else { since } },
                        None,
                    )
                }
                2 => {
                    if t_ms.saturating_sub(since) < TIPTAP_REST_MIN_MS {
                        // Both fingers landed ~together → scroll/click, not tip-tap.
                        return (TtState::Poisoned, None);
                    }
                    // The contact closer to the tracked rest position IS the rest.
                    let (r, t) = if dist(c[0], rest) <= dist(c[1], rest) {
                        (c[0], c[1])
                    } else {
                        (c[1], c[0])
                    };
                    let dx = (t.x - r.x).abs();
                    let dy = (t.y - r.y).abs();
                    // Height guard, posture-aware: tolerant for an angled hand
                    // mid-pad, strict when the bottom-edge thumb zone is
                    // involved (thumb anchor + pointing finger = cursor use).
                    let in_thumb_zone = r.y.max(t.y) > TIPTAP_THUMB_ZONE_Y;
                    let dy_limit = if in_thumb_zone { TIPTAP_THUMB_DY_NORM } else { TIPTAP_MAX_DY_NORM };
                    if !(TIPTAP_MIN_SEP_NORM..=TIPTAP_MAX_DX_NORM).contains(&dx) || dy > dy_limit {
                        // Too close to call left/right, implausibly wide, or a
                        // thumb-vs-finger height mismatch (normal cursor use).
                        return (TtState::Poisoned, None);
                    }
                    (TtState::TapDown { rest: r, tap: t, tap_start_x: t.x, started: t_ms }, None)
                }
                _ => (TtState::Poisoned, None),
            },
            TtState::TapDown { rest, tap, tap_start_x, started } => match c.len() {
                0 => (TtState::Idle, None), // both lifted → a two-finger tap, not tip-tap
                1 => {
                    // One finger lifted. If the REST remains → the tap finger is
                    // releasing: DEFER the emit one frame to confirm the lift
                    // (a mid-hold flicker re-appears and must not fire a tap).
                    let remaining_is_rest = dist(c[0], rest) <= dist(c[0], tap);
                    if remaining_is_rest {
                        (
                            TtState::TapReleasing {
                                rest: c[0],
                                tap_start_x,
                                started,
                                lift_t: t_ms,
                            },
                            None,
                        )
                    } else {
                        // The rest lifted; the former tap finger becomes a fresh rest.
                        (TtState::Resting { rest: c[0], since: t_ms }, None)
                    }
                }
                2 => {
                    // Match contacts to rest/tap by proximity; movement by either
                    // disqualifies (that's a scroll/pinch), as does overstaying.
                    let (r, t) = if dist(c[0], rest) + dist(c[1], tap)
                        <= dist(c[1], rest) + dist(c[0], tap)
                    {
                        (c[0], c[1])
                    } else {
                        (c[1], c[0])
                    };
                    if dist(r, rest) > TIPTAP_MAX_MOVE_NORM
                        || dist(t, tap) > TIPTAP_MAX_MOVE_NORM
                        || t_ms.saturating_sub(started) > TIPTAP_TAP_MAX_MS
                    {
                        return (TtState::Poisoned, None);
                    }
                    (TtState::TapDown { rest: r, tap: t, tap_start_x, started }, None)
                }
                _ => (TtState::Poisoned, None),
            },
            TtState::TapReleasing { rest, tap_start_x, started, lift_t } => {
                // Emit direction, computed once from the tap's landing position.
                let dir = if tap_start_x > rest.x {
                    GestureKind::TipTapRight
                } else {
                    GestureKind::TipTapLeft
                };
                match c.len() {
                    0 => {
                        // Both lifted → the tap really happened then the rest lifted.
                        let dur = lift_t.saturating_sub(started);
                        let emit = (TIPTAP_TAP_MIN_MS..=TIPTAP_TAP_MAX_MS).contains(&dur).then_some(dir);
                        (TtState::Idle, emit)
                    }
                    1 => {
                        // Lift CONFIRMED (tap finger stayed gone, only the rest is
                        // here) → emit now (a real tap), back to Resting.
                        let dur = lift_t.saturating_sub(started);
                        let emit = (TIPTAP_TAP_MIN_MS..=TIPTAP_TAP_MAX_MS).contains(&dur).then_some(dir);
                        (TtState::Resting { rest: c[0], since: t_ms }, emit)
                    }
                    2 => {
                        // The tap finger re-appeared: it was a mid-hold flicker,
                        // NOT a real lift → resume the same tap (keep `started`),
                        // no emit. This is the double-fire fix.
                        let (r, t) = if dist(c[0], rest) <= dist(c[1], rest) {
                            (c[0], c[1])
                        } else {
                            (c[1], c[0])
                        };
                        if dist(r, rest) > TIPTAP_MAX_MOVE_NORM
                            || t_ms.saturating_sub(started) > TIPTAP_TAP_MAX_MS
                        {
                            return (TtState::Poisoned, None);
                        }
                        (TtState::TapDown { rest: r, tap: t, tap_start_x, started }, None)
                    }
                    _ => (TtState::Poisoned, None),
                }
            }
            TtState::Poisoned => match c.len() {
                // Recover as soon as at most the resting finger remains — a
                // rejected tap (moved / overstayed / ghost contact) must NOT
                // require lifting everything before tip-taps work again ("works
                // 5-6 times, then I have to lift"). The fresh settle timer still
                // blocks any scroll/pinch continuation from firing.
                0 => (TtState::Idle, None),
                1 => (TtState::Resting { rest: c[0], since: t_ms }, None),
                _ => (TtState::Poisoned, None),
            },
        }
    }
}

// ── Runtime state + lifecycle ────────────────────────────────────────────────

use parking_lot::Mutex;

/// Managed Tauri state: holds the running platform source (if any).
#[derive(Default)]
pub struct GestureState(pub Mutex<Option<Box<dyn GestureSource>>>);

/// One-shot migration (v0.84.209): tip-tap shipped default-ON in v0.84.206-208
/// and misfired badly during normal thumb-anchored trackpad use (runaway tab
/// switching). It is opt-in now — and any stored `true` from that window is
/// reset once, since it was never an explicit user choice. Users who want it
/// re-enable it consciously in Settings.
pub fn migrate_tiptap_optin(db: &DbHandle) {
    const FLAG: &str = "gestures.tiptap_optin_migrated_v0_84_209";
    if crate::settings::get_bool(db, FLAG, false).unwrap_or(false) {
        return;
    }
    let _ = crate::settings::set(db, KEY_TIPTAP, "false");
    let _ = crate::settings::set(db, FLAG, "true");
}

/// Start/stop the gesture daemon to match the saved config. Called at startup
/// and after a settings change (idempotent). Mirrors `auto_expand::apply`.
pub fn apply(app: &tauri::AppHandle, db: &DbHandle, state: &GestureState) {
    let cfg = GestureConfig::load(db);
    let mut guard = state.0.lock();
    if cfg.enabled {
        // Restart if already running — the sink closure captures the config, so
        // a settings change (tip-tap toggle, volume step) needs a fresh start.
        if let Some(mut running) = guard.take() {
            running.stop();
        }
        let Some(mut source) = platform_source() else {
            return; // unsupported platform → no-op
        };
        let step = cfg.volume_step;
        let app_sink = app.clone();
        let sink: GestureSink = Box::new(move |ev| {
            if let Some(action) = map_action(&ev, &cfg) {
                perform(&app_sink, action, step);
            }
        });
        match source.start(cfg, sink) {
            Ok(()) => {
                *guard = Some(source);
                // Pre-resolve the tab map + layout-dependent chord keys so the
                // first gesture is as fast as every other (no disk/TIS at tap
                // time).
                #[cfg(target_os = "macos")]
                prewarm_tab_keys(app);
            }
            Err(e) => tracing::warn!("touchpad gestures: source failed to start: {e}"),
        }
    } else if let Some(mut source) = guard.take() {
        source.stop();
    }
}

/// Rebuild the gesture capture after system sleep. The private
/// MultitouchSupport registration goes stale across sleep/wake — the run loop
/// keeps spinning but the device delivers no (or late/erratic) frames, so
/// gestures "stop working" until an app restart. Sleep is detected without any
/// AppKit observer: `Instant` (mach_absolute_time) does NOT advance while the
/// Mac sleeps but `SystemTime` does, so a wall-clock jump far beyond the
/// monotonic sleep interval means we slept. On detection the source is
/// restarted via `apply` (a no-op when gestures are disabled). Spawned once at
/// startup.
pub fn spawn_wake_watchdog(app: &tauri::AppHandle) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static SPAWNED: AtomicBool = AtomicBool::new(false);
    if SPAWNED.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::Builder::new()
        .name("ir-gestures-wake".into())
        .spawn(move || {
            use tauri::Manager as _;
            const TICK: std::time::Duration = std::time::Duration::from_secs(30);
            const SLEPT_SLACK: std::time::Duration = std::time::Duration::from_secs(60);
            loop {
                let mono = std::time::Instant::now();
                let wall = std::time::SystemTime::now();
                std::thread::sleep(TICK);
                let mono_elapsed = mono.elapsed();
                let wall_elapsed = wall.elapsed().unwrap_or(mono_elapsed);
                if wall_elapsed > mono_elapsed + SLEPT_SLACK {
                    tracing::info!(
                        "gestures: system slept ~{}s — rebuilding the touch capture",
                        (wall_elapsed - mono_elapsed).as_secs()
                    );
                    let (Some(db), Some(state)) = (
                        app.try_state::<DbHandle>(),
                        app.try_state::<GestureState>(),
                    ) else {
                        continue;
                    };
                    apply(&app, &db, &state);
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames_to_event(frames: &[TouchFrame]) -> Option<GestureEvent> {
        let mut r = Recognizer::new();
        let mut out = None;
        for &f in frames {
            if let Some(ev) = r.feed(f) {
                out = Some(ev);
            }
        }
        out
    }

    // ── Palm-aware recogniser ────────────────────────────────────────────

    fn rc(id: i32, x: f64, y: f64) -> RawContact {
        RawContact { id, x, y, size: 1.0 } // ordinary fingertip
    }
    fn rc_palm(id: i32, x: f64, y: f64) -> RawContact {
        RawContact { id, x, y, size: 3.0 } // heel-of-hand-sized contact
    }

    fn palm_events(frames: &[(u64, Vec<RawContact>)]) -> Vec<GestureEvent> {
        let mut r = PalmAwareRecognizer::new();
        frames.iter().filter_map(|(t, cs)| r.feed(*t, cs)).collect()
    }

    /// Three fingers swiping up from y0 to y1, palm-free — the baseline.
    fn swipe_frames(t0: u64, y0: f64, y1: f64, extra: &[RawContact]) -> Vec<(u64, Vec<RawContact>)> {
        let mut out = Vec::new();
        for (i, step) in [0.0, 0.25, 0.5, 0.75, 1.0].iter().enumerate() {
            let y = y0 + (y1 - y0) * step;
            let mut cs = vec![rc(1, 0.4, y), rc(2, 0.5, y), rc(3, 0.6, y)];
            cs.extend_from_slice(extra);
            out.push((t0 + i as u64 * 50, cs));
        }
        out.push((t0 + 260, extra.to_vec())); // fingers lift, extras remain
        out
    }

    #[test]
    fn palm_rec_three_finger_swipe_up() {
        let evs = palm_events(&swipe_frames(0, 0.8, 0.3, &[]));
        assert_eq!(evs, vec![GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 }]);
    }

    #[test]
    fn size_palm_plus_two_finger_scroll_is_not_a_three_finger_swipe() {
        // A big (size ≥ PALM_SIZE) heel rests on the left while two fingers
        // scroll — previously read as a 3-finger swipe → spurious volume.
        let palm = rc_palm(9, 0.08, 0.9);
        let frames: Vec<(u64, Vec<RawContact>)> = vec![
            (0, vec![palm]),
            (100, vec![palm, rc(1, 0.6, 0.8), rc(2, 0.7, 0.8)]),
            (150, vec![palm, rc(1, 0.6, 0.6), rc(2, 0.7, 0.6)]),
            (200, vec![palm, rc(1, 0.6, 0.4), rc(2, 0.7, 0.4)]),
            (250, vec![palm]), // fingers lift, palm stays
        ];
        let evs = palm_events(&frames);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].fingers, 2); // 2-finger event → map_action ignores it
    }

    #[test]
    fn resting_normal_size_palm_plus_two_finger_scroll_is_not_three_fingers() {
        // Same posture but the heel reports a fingertip-like size (some pads
        // do) — the REST rule (parked ≥ PALM_REST_MIN_MS, no movement) and the
        // per-finger-movement rule still keep it out of the count. The palm
        // lifts TOGETHER with the fingers (the original misfire report).
        let palm = rc(9, 0.08, 0.9);
        let frames: Vec<(u64, Vec<RawContact>)> = vec![
            (0, vec![palm]),
            (700, vec![palm]), // parked past PALM_REST_MIN_MS
            (1000, vec![palm, rc(1, 0.6, 0.8), rc(2, 0.7, 0.8)]),
            (1050, vec![palm, rc(1, 0.6, 0.6), rc(2, 0.7, 0.6)]),
            (1100, vec![palm, rc(1, 0.6, 0.4), rc(2, 0.7, 0.4)]),
            (1150, vec![]), // everything lifts at once
        ];
        let evs = palm_events(&frames);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].fingers, 2);
    }

    #[test]
    fn swipe_fires_while_palm_stays_resting() {
        // A real 3-finger swipe must still work with a heel parked on the pad —
        // and the event fires even though the palm never lifts.
        let palm = rc(9, 0.08, 0.9);
        let mut frames: Vec<(u64, Vec<RawContact>)> = vec![(0, vec![palm]), (800, vec![palm])];
        frames.extend(swipe_frames(1000, 0.8, 0.3, &[palm]));
        let evs = palm_events(&frames);
        assert_eq!(evs, vec![GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 }]);
    }

    #[test]
    fn palm_plus_two_finger_tap_is_not_a_three_finger_tap() {
        // Parked heel + a quick 2-finger tap: the heel's long down-time
        // disqualifies it as a tap finger → fingers = 2, no mute.
        let palm = rc(9, 0.08, 0.9);
        let frames: Vec<(u64, Vec<RawContact>)> = vec![
            (0, vec![palm]),
            (800, vec![palm]),
            (1000, vec![palm, rc(1, 0.5, 0.5), rc(2, 0.6, 0.5)]),
            (1100, vec![palm, rc(1, 0.5, 0.5), rc(2, 0.6, 0.5)]),
            (1150, vec![palm]),
        ];
        let evs = palm_events(&frames);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0], GestureEvent { kind: GestureKind::Tap, fingers: 2 });
    }

    #[test]
    fn three_finger_tap_with_parked_palm_still_mutes() {
        let palm = rc(9, 0.08, 0.9);
        let frames: Vec<(u64, Vec<RawContact>)> = vec![
            (0, vec![palm]),
            (800, vec![palm]),
            (1000, vec![palm, rc(1, 0.4, 0.5), rc(2, 0.5, 0.5), rc(3, 0.6, 0.5)]),
            (1100, vec![palm, rc(1, 0.4, 0.5), rc(2, 0.5, 0.5), rc(3, 0.6, 0.5)]),
            (1150, vec![palm]),
        ];
        let evs = palm_events(&frames);
        assert_eq!(evs, vec![GestureEvent { kind: GestureKind::Tap, fingers: 3 }]);
    }

    #[test]
    fn fingers_that_rest_out_then_swipe_still_recognised() {
        // 3 fingers land, sit motionless past PALM_REST_MIN_MS (they "rest
        // out" → a silent no-event decision), then swipe: the swipe must still
        // be recognised on lift.
        let mut frames: Vec<(u64, Vec<RawContact>)> = vec![
            (0, vec![rc(1, 0.4, 0.8), rc(2, 0.5, 0.8), rc(3, 0.6, 0.8)]),
            (700, vec![rc(1, 0.4, 0.8), rc(2, 0.5, 0.8), rc(3, 0.6, 0.8)]),
        ];
        frames.push((800, vec![rc(1, 0.4, 0.6), rc(2, 0.5, 0.6), rc(3, 0.6, 0.6)]));
        frames.push((900, vec![rc(1, 0.4, 0.3), rc(2, 0.5, 0.3), rc(3, 0.6, 0.3)]));
        frames.push((950, vec![]));
        let evs = palm_events(&frames);
        assert_eq!(evs, vec![GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 }]);
    }

    /// Regression (v0.84.245): real 3-finger taps land + lift STAGGERED — one
    /// finger's individual contact time easily falls outside [TAP_MIN..TAP_MAX]
    /// (a lazy 280 ms contact here). The tap must be judged over the
    /// all-fingers-down overlap window, not per finger — otherwise the event
    /// reads as a 2-finger tap and the 3-finger mute never fires.
    #[test]
    fn palm_rec_staggered_tap_counts_all_three_fingers() {
        let f1 = rc(1, 0.4, 0.5);
        let f2 = rc(2, 0.5, 0.5);
        let f3 = rc(3, 0.6, 0.5);
        let evs = palm_events(&[
            (0, vec![f1]),                 // finger 1 lands first…
            (40, vec![f1, f2, f3]),        // …the rest join (overlap starts)
            (180, vec![f1, f2, f3]),       // overlap ends here (140 ms)
            (200, vec![f1]),               // fingers 2+3 lift
            (280, vec![]),                 // finger 1 lifts last: 280 ms total
        ]);
        assert_eq!(evs, vec![GestureEvent { kind: GestureKind::Tap, fingers: 3 }]);
    }

    /// A held 3-finger chord is NOT a tap — its overlap window is the hold.
    #[test]
    fn palm_rec_held_chord_is_not_a_tap() {
        let f = |t: u64| (t, vec![rc(1, 0.4, 0.5), rc(2, 0.5, 0.5), rc(3, 0.6, 0.5)]);
        let evs = palm_events(&[f(0), f(200), f(400), (450, vec![])]);
        assert!(evs.is_empty(), "a 400 ms chord must not read as a tap: {evs:?}");
    }

    #[test]
    fn active_fingers_excludes_parked_palm() {
        // The scroll-consume window arms on ACTIVE fingers: heel + 2 fingers
        // must not reach 3 (it would swallow the user's legitimate scroll).
        let mut r = PalmAwareRecognizer::new();
        let palm = rc(9, 0.08, 0.9);
        r.feed(0, &[palm]);
        r.feed(800, &[palm]);
        r.feed(1000, &[palm, rc(1, 0.6, 0.8), rc(2, 0.7, 0.8)]);
        assert_eq!(r.active_fingers(), 2);
        // …while 3 real fingers (fresh, moving) do arm.
        let mut r2 = PalmAwareRecognizer::new();
        r2.feed(0, &[rc(1, 0.4, 0.8), rc(2, 0.5, 0.8), rc(3, 0.6, 0.8)]);
        assert_eq!(r2.active_fingers(), 3);
    }

    // ── Tip-tap ──────────────────────────────────────────────────────────

    fn c(x: f64) -> Contact {
        Contact { x, y: 0.5 }
    }

    fn tiptap_events(frames: &[(u64, Vec<Contact>)]) -> Vec<GestureKind> {
        let mut r = TipTapRecognizer::new();
        frames.iter().filter_map(|(t, cs)| r.feed(*t, cs)).collect()
    }

    #[test]
    fn tiptap_right_rest_then_tap_right_of_it() {
        // Index rests at x=0.40; middle taps at 0.55 for ~60 ms → next tab.
        let evs = tiptap_events(&[
            (0, vec![c(0.40)]),
            (100, vec![c(0.40)]),
            (150, vec![c(0.40), c(0.55)]),
            (210, vec![c(0.40)]), // tap lifted, rest stays
            (300, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapRight]);
    }

    #[test]
    fn tiptap_left_rest_then_tap_left_of_it() {
        // Middle rests at 0.55; index taps at 0.40 → previous tab.
        let evs = tiptap_events(&[
            (0, vec![c(0.55)]),
            (100, vec![c(0.55)]),
            (150, vec![c(0.55), c(0.40)]),
            (210, vec![c(0.55)]),
            (300, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapLeft]);
    }

    #[test]
    fn tiptap_chains_while_rest_stays_down() {
        // Human-speed chaining: each tap re-settles ≥ TIPTAP_REST_MIN_MS and the
        // emits are > TIPTAP_EMIT_GAP_MS apart.
        let evs = tiptap_events(&[
            (0, vec![c(0.45)]),
            (100, vec![c(0.45)]),
            (120, vec![c(0.45), c(0.60)]),
            (180, vec![c(0.45)]), // emit #1 (Right)
            (400, vec![c(0.45)]),
            (560, vec![c(0.45), c(0.30)]),
            (620, vec![c(0.45)]), // emit #2 (Left) — > TIPTAP_EMIT_GAP_MS later
            (800, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapRight, GestureKind::TipTapLeft]);
    }

    #[test]
    fn tiptap_rapid_deliberate_chaining_is_not_swallowed() {
        // Two deliberate taps ~250 ms apart must BOTH fire — the old 350 ms
        // emit gap swallowed the second one, which read as "laggy".
        let evs = tiptap_events(&[
            (0, vec![c(0.45)]),
            (100, vec![c(0.45)]),
            (120, vec![c(0.45), c(0.60)]),
            (180, vec![c(0.45)]), // emit #1
            (280, vec![c(0.45)]), // settle done (100 ms after emit)
            (370, vec![c(0.45), c(0.60)]),
            (430, vec![c(0.45)]), // emit #2 — 250 ms after #1
            (600, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapRight, GestureKind::TipTapRight]);
    }

    #[test]
    fn tiptap_midhold_flicker_does_not_double_fire() {
        // The tap contact briefly drops out mid-hold (a MultitouchSupport state
        // flicker) then returns before the REAL lift. The old code emitted on
        // the flicker AND the real lift — more than the 200 ms refractory apart
        // — so the browser jumped TWO tabs. The deferred lift-confirmation fires
        // exactly ONCE (v0.84.257).
        let evs = tiptap_events(&[
            (0, vec![c(0.40)]),
            (100, vec![c(0.40)]),
            (150, vec![c(0.40), c(0.55)]), // TapDown
            (200, vec![c(0.40)]),          // flicker: tap gone for one frame
            (210, vec![c(0.40), c(0.55)]), // tap back — a flicker, not a lift
            (420, vec![c(0.40)]),          // the real lift
            (500, vec![c(0.40)]),          // lift confirmed → single emit
            (600, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapRight]);
    }

    #[test]
    fn tiptap_recovers_after_a_rejected_attempt_without_full_lift() {
        // A two-finger scroll poisons; lifting back to ONE finger + re-settling
        // must re-arm (the old all-lift requirement made tip-taps die after a
        // few uses until the user lifted everything).
        let evs = tiptap_events(&[
            (0, vec![c(0.40)]),
            (10, vec![c(0.40), c(0.55)]), // landed together → poisoned
            (200, vec![c(0.40), c(0.55)]),
            (250, vec![c(0.40)]), // one lifted → recovering rest
            (400, vec![c(0.40)]), // settled ≥ 80 ms
            (420, vec![c(0.40), c(0.55)]),
            (480, vec![c(0.40)]), // valid tap → emit
            (600, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapRight]);
    }

    #[test]
    fn tiptap_tolerates_an_angled_hand_mid_pad() {
        // Index resting mid-pad, middle finger tapping noticeably HIGHER
        // (Δy = 0.28) — a natural angled-hand tip-tap; must fire. The old
        // 0.22 limit rejected this ("not recognised when one finger sits
        // higher on the pad").
        let evs = tiptap_events(&[
            (0, vec![Contact { x: 0.40, y: 0.62 }]),
            (100, vec![Contact { x: 0.40, y: 0.62 }]),
            (150, vec![Contact { x: 0.40, y: 0.62 }, Contact { x: 0.55, y: 0.34 }]),
            (210, vec![Contact { x: 0.40, y: 0.62 }]),
            (400, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapRight]);
        // A strongly angled hand (Δy = 0.45, outside the thumb zone) fires too.
        let evs = tiptap_events(&[
            (0, vec![Contact { x: 0.40, y: 0.70 }]),
            (100, vec![Contact { x: 0.40, y: 0.70 }]),
            (150, vec![Contact { x: 0.40, y: 0.70 }, Contact { x: 0.55, y: 0.25 }]),
            (210, vec![Contact { x: 0.40, y: 0.70 }]),
            (400, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapRight]);
        // …but beyond the generous limit (Δy = 0.62) it's no tip-tap posture.
        let evs = tiptap_events(&[
            (0, vec![Contact { x: 0.40, y: 0.72 }]),
            (100, vec![Contact { x: 0.40, y: 0.72 }]),
            (150, vec![Contact { x: 0.40, y: 0.72 }, Contact { x: 0.55, y: 0.10 }]),
            (210, vec![Contact { x: 0.40, y: 0.72 }]),
            (400, vec![]),
        ]);
        assert!(evs.is_empty());
    }

    #[test]
    fn tiptap_rejects_thumb_anchor_posture() {
        // Thumb resting at the pad's bottom while the index finger touches
        // higher up — normal cursor use, large Δy → must NEVER fire (this
        // posture caused runaway tab switching).
        let evs = tiptap_events(&[
            (0, vec![Contact { x: 0.45, y: 0.92 }]), // thumb, bottom edge
            (200, vec![Contact { x: 0.45, y: 0.92 }]),
            (250, vec![Contact { x: 0.45, y: 0.92 }, Contact { x: 0.55, y: 0.35 }]),
            (330, vec![Contact { x: 0.45, y: 0.92 }]),
            (500, vec![]),
        ]);
        assert!(evs.is_empty(), "thumb+finger height mismatch must not fire");
    }

    #[test]
    fn tiptap_bounce_fires_at_most_once() {
        // The tap's lift "bounces": the contact re-appears for a frame right
        // after lifting. The settle timer + emit refractory must swallow it.
        let evs = tiptap_events(&[
            (0, vec![c(0.40)]),
            (100, vec![c(0.40)]),
            (150, vec![c(0.40), c(0.55)]),
            (210, vec![c(0.40)]), // emit
            (230, vec![c(0.40), c(0.55)]), // bounce re-contact (20 ms later)
            (260, vec![c(0.40)]),
            (400, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapRight]);
    }

    #[test]
    fn tiptap_rejects_fingers_landing_together_scroll_guard() {
        // Two fingers within the settle window (a 2-finger scroll/click).
        let evs = tiptap_events(&[
            (0, vec![c(0.40)]),
            (30, vec![c(0.40), c(0.55)]), // 30 ms < TIPTAP_REST_MIN_MS
            (90, vec![c(0.40)]),
            (150, vec![]),
        ]);
        assert!(evs.is_empty());
    }

    #[test]
    fn tiptap_rejects_movement_and_overstay() {
        // Both fingers glide (scroll) → poisoned, nothing fires.
        let scroll = tiptap_events(&[
            (0, vec![c(0.40)]),
            (100, vec![c(0.40)]),
            (150, vec![c(0.40), c(0.55)]),
            (200, vec![Contact { x: 0.40, y: 0.30 }, Contact { x: 0.55, y: 0.30 }]),
            (260, vec![c(0.40)]),
            (300, vec![]),
        ]);
        assert!(scroll.is_empty());
        // Second finger overstays (a two-finger rest, not a tap).
        let hold = tiptap_events(&[
            (0, vec![c(0.40)]),
            (100, vec![c(0.40)]),
            (150, vec![c(0.40), c(0.55)]),
            (600, vec![c(0.40), c(0.55)]), // > TIPTAP_TAP_MAX_MS
            (700, vec![c(0.40)]),
            (800, vec![]),
        ]);
        assert!(hold.is_empty());
    }

    #[test]
    fn tiptap_no_emit_when_the_rest_finger_lifts_instead() {
        let evs = tiptap_events(&[
            (0, vec![c(0.40)]),
            (100, vec![c(0.40)]),
            (150, vec![c(0.40), c(0.55)]),
            (210, vec![c(0.55)]), // the REST lifted; tap finger stays
            (300, vec![]),
        ]);
        assert!(evs.is_empty());
    }

    #[test]
    fn bundled_tab_shortcuts_json_parses_and_routes() {
        let map = parse_tab_map(TAB_SHORTCUTS_JSON).expect("bundled tab_shortcuts.json must parse");
        assert!(!map.apps.is_empty());
        // Every entry has sane chords.
        for e in &map.apps {
            for c in [&e.next, &e.prev] {
                let named = matches!(c.key.as_str(), "tab" | "left" | "right");
                assert!(named || c.key.chars().count() == 1, "bad key {:?} for {}", c.key, e.prefix);
                assert!(!c.mods.is_empty(), "chord without modifiers for {}", e.prefix);
                for m in &c.mods {
                    assert!(matches!(m.as_str(), "cmd" | "shift" | "ctrl" | "alt"), "bad mod {m}");
                }
            }
        }
        let chord = |b: &str, next: bool| {
            let (n, p) = tab_chords_for(&map, b);
            if next { n.clone() } else { p.clone() }
        };
        // iTerm2: Ctrl+Tab is its MRU cycle → the map must use ⌘→/⌘← instead.
        assert_eq!(chord("com.googlecode.iterm2", true).key, "right");
        assert_eq!(chord("com.googlecode.iterm2", true).mods, vec!["cmd"]);
        assert_eq!(chord("com.googlecode.iterm2", false).key, "left");
        // VS Code family → ⌘⌥ arrows.
        for b in ["com.microsoft.VSCode", "com.todesktop.230313mzl4w4u92", "com.sublimetext.4"] {
            let c = chord(b, true);
            assert_eq!(c.key, "right", "{b}");
            assert!(c.mods.contains(&"alt".to_string()), "{b}");
        }
        // JetBrains family + Xcode → layout-aware brackets.
        for b in ["com.jetbrains.intellij", "com.jetbrains.pycharm", "com.google.android.studio", "com.apple.dt.Xcode"] {
            assert_eq!(chord(b, true).key, "]", "{b}");
            assert_eq!(chord(b, false).key, "[", "{b}");
        }
        // Unknown apps → the system-standard Ctrl+Tab default.
        let (n, p) = tab_chords_for(&map, "com.unknown.app");
        assert_eq!((n.key.as_str(), p.key.as_str()), ("tab", "tab"));
        assert!(n.mods.contains(&"ctrl".to_string()) && p.mods.contains(&"shift".to_string()));
    }

    #[test]
    fn user_tab_map_overrides_built_in() {
        let built_in = parse_tab_map(TAB_SHORTCUTS_JSON).unwrap();
        let user = parse_tab_map(
            r#"{ "apps": [ { "prefix": "com.googlecode.iterm2",
                 "next": { "key": "]", "mods": ["cmd", "shift"] },
                 "prev": { "key": "[", "mods": ["cmd", "shift"] } } ],
                 "default": { "next": { "key": "tab", "mods": ["ctrl"] },
                              "prev": { "key": "tab", "mods": ["ctrl", "shift"] } } }"#,
        )
        .unwrap();
        let merged = merge_tab_maps(built_in, Some(user));
        // The user's iTerm2 entry is found FIRST (overrides the bundled one).
        let (n, _) = tab_chords_for(&merged, "com.googlecode.iterm2");
        assert_eq!(n.key, "]");
        // Bundled entries still resolve for other apps.
        let (n2, _) = tab_chords_for(&merged, "com.jetbrains.idea");
        assert_eq!(n2.key, "]");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn key_for_char_resolves_brackets_on_the_current_layout() {
        // Live layout query (TIS) — headless CI may yield None; on a real
        // session both brackets must resolve to a sane keycode. On a German
        // layout ] is ⌥6 → the extra flags must include Alt.
        for ch in [']', '['] {
            if let Some((code, flags)) = key_for_char(ch) {
                assert!(code < 128, "keycode for {ch:?} out of range: {code}");
                eprintln!("key_for_char({ch:?}) → keycode {code}, extra flags {flags:#x}");
            } else {
                eprintln!("key_for_char({ch:?}) → None");
            }
        }
    }

    #[test]
    fn tiptap_map_action_gated_by_config() {
        let mut cfg = GestureConfig { enabled: true, tiptap: true, ..Default::default() };
        let left = GestureEvent { kind: GestureKind::TipTapLeft, fingers: 2 };
        let right = GestureEvent { kind: GestureKind::TipTapRight, fingers: 2 };
        assert_eq!(map_action(&left, &cfg), Some(GestureAction::PrevTab));
        assert_eq!(map_action(&right, &cfg), Some(GestureAction::NextTab));
        cfg.tiptap = false;
        assert_eq!(map_action(&left, &cfg), None);
        cfg.tiptap = true;
        cfg.enabled = false;
        assert_eq!(map_action(&right, &cfg), None);
    }

    #[test]
    fn classify_swipe_directions_and_threshold() {
        assert_eq!(classify_swipe(0.0, -0.3, 0.12), Some(GestureKind::SwipeUp));
        assert_eq!(classify_swipe(0.0, 0.3, 0.12), Some(GestureKind::SwipeDown));
        assert_eq!(classify_swipe(-0.3, 0.0, 0.12), Some(GestureKind::SwipeLeft));
        assert_eq!(classify_swipe(0.3, 0.0, 0.12), Some(GestureKind::SwipeRight));
        // dominant axis wins
        assert_eq!(classify_swipe(0.1, -0.3, 0.12), Some(GestureKind::SwipeUp));
        // below threshold → nothing
        assert_eq!(classify_swipe(0.05, 0.05, 0.12), None);
    }

    #[test]
    fn recognizer_three_finger_swipe_up() {
        // 3 fingers down at y=0.6, glide up to 0.2, then lift.
        let ev = frames_to_event(&[
            TouchFrame { contacts: 3, x: 0.5, y: 0.6, t_ms: 0 },
            TouchFrame { contacts: 3, x: 0.5, y: 0.4, t_ms: 40 },
            TouchFrame { contacts: 3, x: 0.5, y: 0.2, t_ms: 80 },
            TouchFrame { contacts: 0, x: 0.5, y: 0.2, t_ms: 120 },
        ]);
        assert_eq!(ev, Some(GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 }));
    }

    #[test]
    fn recognizer_three_finger_swipe_down() {
        let ev = frames_to_event(&[
            TouchFrame { contacts: 3, x: 0.5, y: 0.2, t_ms: 0 },
            TouchFrame { contacts: 3, x: 0.5, y: 0.6, t_ms: 80 },
            TouchFrame { contacts: 0, x: 0.5, y: 0.6, t_ms: 110 },
        ]);
        assert_eq!(ev, Some(GestureEvent { kind: GestureKind::SwipeDown, fingers: 3 }));
    }

    #[test]
    fn recognizer_three_finger_tap() {
        // 3 fingers, tiny movement, quick lift → Tap.
        let ev = frames_to_event(&[
            TouchFrame { contacts: 3, x: 0.5, y: 0.5, t_ms: 0 },
            TouchFrame { contacts: 3, x: 0.51, y: 0.5, t_ms: 60 },
            TouchFrame { contacts: 0, x: 0.51, y: 0.5, t_ms: 120 },
        ]);
        assert_eq!(ev, Some(GestureEvent { kind: GestureKind::Tap, fingers: 3 }));
    }

    #[test]
    fn recognizer_single_frame_glitch_is_not_a_tap() {
        // 3 fingers appear + vanish in one frame (dur == 0) — a sensor glitch,
        // not a real tap. Must NOT fire (this caused mute "by itself").
        let ev = frames_to_event(&[
            TouchFrame { contacts: 3, x: 0.5, y: 0.5, t_ms: 40 },
            TouchFrame { contacts: 0, x: 0.5, y: 0.5, t_ms: 40 },
        ]);
        assert_eq!(ev, None);
    }

    #[test]
    fn recognizer_short_quick_swipe_is_not_a_tap() {
        // A quick 3-finger flick that moves 0.04 of the pad (between the tightened
        // tap-move limit 0.03 and the swipe threshold 0.12) must NOT register as a
        // tap → no spurious mute. It's an ambiguous flick → no action.
        let ev = frames_to_event(&[
            TouchFrame { contacts: 3, x: 0.50, y: 0.50, t_ms: 0 },
            TouchFrame { contacts: 3, x: 0.50, y: 0.46, t_ms: 60 },
            TouchFrame { contacts: 0, x: 0.50, y: 0.46, t_ms: 110 },
        ]);
        assert_eq!(ev, None);
    }

    #[test]
    fn recognizer_rebaselines_when_finger_count_grows() {
        // Real trackpads land fingers one at a time: 0→2→3. The 2-finger frame
        // sits to one side; when the 3rd joins the centroid jumps. The swipe
        // must be measured from the first 3-finger frame, not the 2-finger one,
        // so a clean upward 3-finger glide reads as SwipeUp (not a false
        // direction from the join-jump).
        let ev = frames_to_event(&[
            TouchFrame { contacts: 2, x: 0.30, y: 0.60, t_ms: 0 }, // 2 fingers land left
            TouchFrame { contacts: 3, x: 0.50, y: 0.60, t_ms: 20 }, // 3rd joins → centroid jumps right
            TouchFrame { contacts: 3, x: 0.50, y: 0.40, t_ms: 60 }, // glide up
            TouchFrame { contacts: 3, x: 0.50, y: 0.20, t_ms: 100 },
            TouchFrame { contacts: 0, x: 0.50, y: 0.20, t_ms: 140 },
        ]);
        // dx from the join-jump (0.30→0.50) must NOT win → vertical up.
        assert_eq!(ev, Some(GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 }));
    }

    #[test]
    fn recognizer_lift_skew_uses_peak_position() {
        // Fingers glide up to 0.2 (3 down), then one lifts and the remaining
        // centroid jumps to 0.7 before full lift. The peak (0.2) must win, not
        // the lift-frame centroid.
        let ev = frames_to_event(&[
            TouchFrame { contacts: 3, x: 0.5, y: 0.6, t_ms: 0 },
            TouchFrame { contacts: 3, x: 0.5, y: 0.2, t_ms: 80 },
            TouchFrame { contacts: 1, x: 0.5, y: 0.7, t_ms: 95 },
            TouchFrame { contacts: 0, x: 0.5, y: 0.7, t_ms: 110 },
        ]);
        assert_eq!(ev, Some(GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 }));
    }

    #[test]
    fn recognizer_long_rest_no_move_is_not_a_tap() {
        // 3 fingers resting >TAP_MAX_MS without moving → neither tap nor swipe.
        let ev = frames_to_event(&[
            TouchFrame { contacts: 3, x: 0.5, y: 0.5, t_ms: 0 },
            TouchFrame { contacts: 3, x: 0.5, y: 0.5, t_ms: 600 },
            TouchFrame { contacts: 0, x: 0.5, y: 0.5, t_ms: 650 },
        ]);
        assert_eq!(ev, None);
    }

    #[test]
    fn map_action_respects_config_and_fingers() {
        let on = GestureConfig { enabled: true, fingers: 3, volume_step: 6, tiptap: true };
        let off = GestureConfig { enabled: false, ..on };
        let up = GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 };
        let tap = GestureEvent { kind: GestureKind::Tap, fingers: 3 };
        let two = GestureEvent { kind: GestureKind::SwipeUp, fingers: 2 };
        let left = GestureEvent { kind: GestureKind::SwipeLeft, fingers: 3 };
        assert_eq!(map_action(&up, &on), Some(GestureAction::VolumeUp));
        assert_eq!(map_action(&tap, &on), Some(GestureAction::MuteToggle));
        assert_eq!(map_action(&up, &off), None); // disabled
        assert_eq!(map_action(&two, &on), None); // wrong finger count
        assert_eq!(map_action(&left, &on), None); // unbound direction
    }
}
