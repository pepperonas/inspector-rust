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
/// tap could fire several tab switches ("apps jump around wildly").
pub const TIPTAP_EMIT_GAP_MS: u64 = 350;
/// The tap must land at a similar HEIGHT as the resting finger (|Δy|, 0..1).
/// Two fingertips sit side by side; a thumb anchored at the pad's bottom edge
/// vs. a pointing finger has a large Δy — that posture is normal cursor use,
/// never a tip-tap (it caused runaway tab switching in iTerm & co.).
pub const TIPTAP_MAX_DY_NORM: f64 = 0.22;
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
    // Tab switching: synthesize the (near-universal) Ctrl+Tab / Ctrl+Shift+Tab
    // directly — no toast, the visibly switching tab is the feedback. CGEventPost
    // is thread-safe, so this runs inline on the capture thread (fast path).
    if matches!(action, GestureAction::NextTab | GestureAction::PrevTab) {
        #[cfg(target_os = "macos")]
        send_tab_switch(matches!(action, GestureAction::NextTab));
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

/// Synthesize Ctrl+Tab (next) / Ctrl+Shift+Tab (previous) — the tab-navigation
/// shortcut virtually every tabbed macOS app understands (Safari, Chrome,
/// Firefox, VS Code, iTerm2, Terminal, Finder, …). Raw CGEvent FFI; posting to
/// the HID tap needs the Accessibility grant the gestures feature already uses.
#[cfg(target_os = "macos")]
fn send_tab_switch(next: bool) {
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
    const KEY_TAB: u16 = 48; // kVK_Tab
    const FLAG_SHIFT: u64 = 0x0002_0000; // kCGEventFlagMaskShift
    const FLAG_CONTROL: u64 = 0x0004_0000; // kCGEventFlagMaskControl
    const HID_TAP: u32 = 0; // kCGHIDEventTap
    let flags = FLAG_CONTROL | if next { 0 } else { FLAG_SHIFT };
    unsafe {
        for down in [true, false] {
            let ev = CGEventCreateKeyboardEvent(std::ptr::null_mut(), KEY_TAB, down);
            if ev.is_null() {
                return;
            }
            CGEventSetFlags(ev, flags);
            CGEventPost(HID_TAP, ev);
            CFRelease(ev as *const c_void);
        }
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
#[derive(Debug, Default)]
pub struct Recognizer {
    active: bool,
    start: (f64, f64, u64),
    peak: (f64, f64, u64), // last frame at max_contacts
    max_contacts: u8,
}

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
                    if !(TIPTAP_MIN_SEP_NORM..=TIPTAP_MAX_DX_NORM).contains(&dx)
                        || (t.y - r.y).abs() > TIPTAP_MAX_DY_NORM
                    {
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
                    // One finger lifted. If the REST remains → the tap finger
                    // tapped: emit (direction = where the tap landed vs. rest).
                    let remaining_is_rest = dist(c[0], rest) <= dist(c[0], tap);
                    if remaining_is_rest {
                        let dur = t_ms.saturating_sub(started);
                        let emit = (TIPTAP_TAP_MIN_MS..=TIPTAP_TAP_MAX_MS)
                            .contains(&dur)
                            .then_some(if tap_start_x > rest.x {
                                GestureKind::TipTapRight
                            } else {
                                GestureKind::TipTapLeft
                            });
                        // Rest stays down → back to Resting. The settle timer
                        // restarts (no backdating): chained taps at human speed
                        // (> ~80 ms apart) still work, but a bouncing lift
                        // contact can't immediately re-arm another tap.
                        (TtState::Resting { rest: c[0], since: t_ms }, emit)
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
        let app = app.clone();
        let sink: GestureSink = Box::new(move |ev| {
            if let Some(action) = map_action(&ev, &cfg) {
                perform(&app, action, step);
            }
        });
        match source.start(cfg, sink) {
            Ok(()) => *guard = Some(source),
            Err(e) => tracing::warn!("touchpad gestures: source failed to start: {e}"),
        }
    } else if let Some(mut source) = guard.take() {
        source.stop();
    }
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
