//! Touchpad gestures (BetterTouchTool-style) → existing volume / mute actions.
//!
//! | Gesture            | Action                          |
//! |--------------------|---------------------------------|
//! | 3-finger swipe up  | volume up   (`adjust_system_volume(+step)`) |
//! | 3-finger swipe down| volume down (`adjust_system_volume(-step)`) |
//! | 3-finger tap       | mute toggle (`toggle_system_mute()`)        |
//! | tip-tap right (2 fingers resting, a 3rd taps to their RIGHT) | next tab (Ctrl+Tab) |
//! | tip-tap left  (2 fingers resting, a 3rd taps to their LEFT)  | previous tab (Ctrl+Shift+Tab) |
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
/// Default volume step per swipe (matches the `Shift+↑/↓` popup step). Was 6
/// until v0.84.268 — an odd step that never aligned with the 5-% grid people
/// expect from a percent readout; `migrate_volume_step_default` bumps stored
/// un-customised installs.
pub const DEFAULT_VOLUME_STEP: i32 = 5;

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
/// A tap cluster is FINALISED (emitted) once the pad has had no non-palm contact
/// for this long. It's the coalescing window that turns a light multi-finger tap
/// arriving as sequential single-finger touches into ONE N-finger tap — and the
/// mute's latency, so kept short.
pub const TAP_SETTLE_MS: u64 = 160;
/// All fingers of a multi-finger tap must touch within this overall span; longer
/// is a hold / two separate taps, not one gesture.
pub const TAP_CLUSTER_MAX_MS: u64 = 700;
/// No single finger of a tap may stay down longer than this — a HELD 3-finger
/// chord (fingers resting a while) is not a tap even if the cluster span fits.
/// (A quick tap's fingers are each down well under this, even when staggered.)
pub const TAP_HOLD_MAX_MS: u64 = 350;
/// In a tap cluster each finger may travel at most this far (its own path). More
/// is a drag/scroll — generous vs `TAP_MAX_MOVE_NORM` to tolerate the small
/// movement of the make/break contact phases + a slightly rolling tap.
pub const TAP_FINGER_MAX_MOVE_NORM: f64 = 0.12;

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

/// Directional coherence a multi-finger movement needs to count as a **swipe**
/// (v0.99.0). Coherence = |Σ vᵢ| / Σ |vᵢ| over the moving fingers' travel
/// vectors — `1.0` when all fingers travel the same way, `0` when they cancel.
/// A real 2-/3-finger swipe is near-parallel (≈ 0.9+); a pinch/rotate/spread
/// cancels out (< 0.3). This is the KEY tap-vs-swipe discriminator: it lets the
/// swipe threshold drop to `SWIPE_FINGER_MIN_MOVE_NORM` (so a *weak* 3-finger
/// swipe of ~0.08 is volume, not a mis-fired tap → the accidental-mute bug)
/// while still rejecting divergent movement. A tap where ONE finger drifts has
/// only 1 mover, so it never reaches the ≥2-coherent-movers swipe test and
/// stays a tap.
pub const SWIPE_COHERENCE_MIN: f64 = 0.6;

/// Early swipe emission (v0.110.0): a swipe used to emit only when a finger
/// LIFTED — the whole remainder of the motion after the threshold was pure
/// wait. A swipe whose ≥ 3 movers have each travelled this far (2× the lift
/// bar) with full coherence is unambiguous mid-flight, so it emits while the
/// fingers are still down and the rest of the gesture is consumed. The margin
/// is deliberate: at exactly the lift bar a pinch could still be developing,
/// and 2-finger geometries are excluded entirely (a scroll that later gains a
/// 3rd finger must keep its at-lift decision). Weak swipes (travel between the
/// two bars) keep the old emit-on-lift path unchanged.
pub const EARLY_SWIPE_MIN_MOVE_NORM: f64 = 2.0 * SWIPE_FINGER_MIN_MOVE_NORM;

// Tip-tap (v0.91.6): ONE finger rests on the pad, a SECOND finger taps briefly
// to its left/right → previous/next tab. (This is the one-finger posture the
// user asked to return to; v0.84.266 had switched to a two-finger rest to dodge
// thumb-anchored-cursor false positives — that risk is back with one rest
// finger, mitigated by the settle gate + movement/duration/height guards.)
/// The resting finger must be down at least this long before the tap lands —
/// two fingers landing together are a scroll/swipe, never a tip-tap.
pub const TIPTAP_REST_MIN_MS: u64 = 80;
/// The tapping finger must lift within this window (else it's a two-finger rest).
pub const TIPTAP_TAP_MAX_MS: u64 = 300;
/// …and must be down at least this long. 40 ms also filters MT state flicker
/// (a lightly-resting finger can bounce between touching/hover states frame to
/// frame, which looks like machine-gun micro-taps).
pub const TIPTAP_TAP_MIN_MS: u64 = 40;
/// Max movement (normalized) a resting finger may make during the tap — more is
/// a scroll/swipe, and a drifting rest finger re-arms its settle timer.
pub const TIPTAP_MAX_MOVE_NORM: f64 = 0.05;
/// The tap must land at least this far (normalized) beyond the resting finger
/// for the direction decision to be reliable (a tap right on top of the rest
/// finger is ambiguous → rejected).
pub const TIPTAP_MIN_SEP_NORM: f64 = 0.03;
/// Refractory period between two tip-tap emits. A physical tap's lift can
/// "bounce" (the contact re-appears for a frame or two) — without this gap one
/// tap could fire several tab switches ("apps jump around wildly"). Bounce is
/// primarily blocked by the deferred lift-confirmation; this gap is
/// belt-and-braces, so it's kept short — 200 ms still allows ~5 deliberate
/// chained taps/s (350 ms swallowed rapid taps and read as "laggy").
pub const TIPTAP_EMIT_GAP_MS: u64 = 200;
/// The tap must land at a roughly similar HEIGHT as the resting finger
/// (|Δy|, 0..1). Generous — strongly angled hands are fine.
pub const TIPTAP_MAX_DY_NORM: f64 = 0.55;
/// …and not implausibly far past the edge sideways (an adjacent fingertip, not
/// a wide reach across the pad).
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
const KEY_TYPING_GUARD: &str = "gestures.typing_guard";
const KEY_VOLUME: &str = "gestures.volume";
const KEY_MUTE: &str = "gestures.mute";

fn default_false() -> bool {
    false
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct GestureConfig {
    pub enabled: bool,
    pub fingers: u8,
    pub volume_step: i32,
    /// Tip-tap tab switching: rest TWO fingers, tap a third to their
    /// left/right (v0.84.266). **Opt-in (default off)**. The two-finger rest is
    /// a deliberate posture — unlike the old one-finger version it doesn't
    /// collide with thumb-anchored cursor use — but it stays opt-in so it never
    /// fires for people who don't want it.
    #[serde(default = "default_false")]
    pub tiptap: bool,
    /// Typing guard / disable-while-typing (v0.109.0, default ON): volume and
    /// mute gestures are suppressed for a short window after a real keystroke —
    /// palms brushing the pad WHILE typing were the top misfire source (field
    /// data: 40 of 48 isolated dispatches in one week were volume swipes).
    /// libinput's DWT is the model; tab switching is exempt because it
    /// synthesizes keystrokes itself and is used in rapid bursts.
    #[serde(default = "default_true")]
    pub typing_guard: bool,
    /// Per-gesture switches (v0.109.0, default ON) — the honest escape hatch:
    /// whoever is bitten by ONE gesture can turn exactly that one off.
    #[serde(default = "default_true")]
    pub volume: bool,
    #[serde(default = "default_true")]
    pub mute: bool,
}

fn default_true() -> bool {
    true
}

impl Default for GestureConfig {
    fn default() -> Self {
        GestureConfig {
            enabled: false, // opt-in
            fingers: DEFAULT_FINGERS,
            volume_step: DEFAULT_VOLUME_STEP,
            tiptap: false, // opt-in (see the field doc — accidental-trigger risk)
            typing_guard: true,
            volume: true,
            mute: true,
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
            typing_guard: crate::settings::get_bool(db, KEY_TYPING_GUARD, d.typing_guard)
                .unwrap_or(d.typing_guard),
            volume: crate::settings::get_bool(db, KEY_VOLUME, d.volume).unwrap_or(d.volume),
            mute: crate::settings::get_bool(db, KEY_MUTE, d.mute).unwrap_or(d.mute),
        }
    }

    pub fn save(&self, db: &DbHandle) -> anyhow::Result<()> {
        crate::settings::set(db, KEY_ENABLED, if self.enabled { "true" } else { "false" })?;
        crate::settings::set(db, KEY_VOLUME_STEP, &self.volume_step.to_string())?;
        crate::settings::set(db, KEY_TIPTAP, if self.tiptap { "true" } else { "false" })?;
        crate::settings::set(db, KEY_TYPING_GUARD, if self.typing_guard { "true" } else { "false" })?;
        crate::settings::set(db, KEY_VOLUME, if self.volume { "true" } else { "false" })?;
        crate::settings::set(db, KEY_MUTE, if self.mute { "true" } else { "false" })?;
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

/// Typing-guard window: a volume/mute gesture whose fingers LANDED, or whose
/// touch was still down, less than this long after a real (non-modifier)
/// keystroke is suppressed. libinput uses 200 ms after a lone keypress and
/// 500 ms inside a typing burst, keyed to the TOUCH start — and so is this
/// since v0.166.1 (see [`TypingTrace`]).
pub const TYPING_GUARD_S: f64 = 0.5;

/// What the typing guard knows about a gesture, in seconds against the last
/// hardware key-down. `before_touch_s` was sampled when the fingers LANDED
/// (libinput's disable-while-typing question: "was the user typing when the
/// touch began?"); `before_lift_s` is derived at dispatch — seconds between
/// that key-down and the moment the fingers LIFTED, so it is NEGATIVE when the
/// last key came after the lift. That negative case is what the v0.109.0 guard
/// got wrong: it measured against the DISPATCH, which for a tap sits ≥ 160 ms
/// past the lift (settle + tick), so a key pressed right after a deliberate
/// 3-finger tap — typing resumed — read as "typing 0.01 s ago" and vetoed the
/// tap. `f64::INFINITY` = no key-down known (never vetoes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypingTrace {
    pub before_touch_s: f64,
    pub before_lift_s: f64,
}

/// Whether the typing guard suppresses `action`. Pure: the platform queries
/// stay at the call site. Two exemptions are load-bearing:
///
/// * Tab actions are NEVER suppressed — tip-tap both emits keystrokes itself
///   (it would self-suppress its own bursts) and is guarded by its own
///   rest-quiet rule instead.
/// * An UNMUTE (`unmuting`: a mute toggle while the output is muted) is NEVER
///   suppressed. The guard exists against palm misfires, and an accidental
///   unmute costs nothing — you hear sound — while a vetoed unmute strands the
///   user muted with no gesture that can undo it (the 2026-09-05 report:
///   "I keep having to unmute in System Settings"). Only the MUTING direction
///   can be an accident worth blocking.
///
/// A key pressed AFTER the fingers lifted (`before_lift_s < 0`) never counts:
/// it cannot have caused the touch. Pure modifier presses don't count as
/// key-downs at the source (macOS reports them as flags-changed), so
/// shift-clicks and hotkey chords don't arm the guard — libinput's exemption.
pub fn typing_guard_suppresses(action: GestureAction, trace: TypingTrace, unmuting: bool) -> bool {
    match action {
        GestureAction::NextTab | GestureAction::PrevTab => false,
        GestureAction::MuteToggle if unmuting => false,
        GestureAction::VolumeUp | GestureAction::VolumeDown | GestureAction::MuteToggle => {
            let typed_before_touch = trace.before_touch_s < TYPING_GUARD_S;
            // ≥ 0: the key preceded the lift (during the touch, or the window
            // before it). < 0 is "typing resumed after the gesture" — allowed.
            let typed_before_lift = (0.0..TYPING_GUARD_S).contains(&trace.before_lift_s);
            typed_before_touch || typed_before_lift
        }
    }
}

/// Handoff from the platform emit site to the dispatch sink, both of which run
/// on the emitting thread with the sink called synchronously right after these
/// are set. `u64::MAX` = not sampled. Kept as plain atomics rather than fields
/// on `GestureEvent` so the event stays the small value type the recogniser
/// tests construct by the dozen.
static EMIT_LIFT_AGE_MS: AtomicU64 = AtomicU64::new(0);
static TOUCH_TYPED_BEFORE_MS: AtomicU64 = AtomicU64::new(u64::MAX);

/// Platform hook: the pad went from no contact to at least one — sample how
/// long ago the last key-down was, so the guard can ask its question about the
/// TOUCH START rather than about whenever the recogniser finished.
pub(crate) fn note_touch_start() {
    let s = seconds_since_last_keydown();
    let ms = if s.is_finite() { (s * 1000.0) as u64 } else { u64::MAX };
    TOUCH_TYPED_BEFORE_MS.store(ms, Ordering::Relaxed);
}

/// Platform hook, called right before the sink: how long ago the emitted
/// gesture's fingers lifted (0 for an in-flight / at-lift emit, the settle
/// delay for a deferred tap).
pub(crate) fn note_emit_lift_age_ms(ms: u64) {
    EMIT_LIFT_AGE_MS.store(ms, Ordering::Relaxed);
}

/// Assemble the guard's view of the gesture being dispatched (see the hooks).
fn typing_trace_now() -> TypingTrace {
    let since_keydown = seconds_since_last_keydown();
    let lift_age_s = EMIT_LIFT_AGE_MS.load(Ordering::Relaxed) as f64 / 1000.0;
    let before_touch_s = match TOUCH_TYPED_BEFORE_MS.load(Ordering::Relaxed) {
        u64::MAX => f64::INFINITY,
        ms => ms as f64 / 1000.0,
    };
    TypingTrace { before_touch_s, before_lift_s: since_keydown - lift_age_s }
}

/// Is the default output currently muted? `None` = unknown (no control, no
/// macOS) — the guard then treats the toggle as a MUTE, i.e. still vetoable.
#[cfg(target_os = "macos")]
fn output_muted_now() -> Option<bool> {
    crate::system_commands::ca_volume::read_mute()
}
#[cfg(not(target_os = "macos"))]
fn output_muted_now() -> Option<bool> {
    None
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
        // A tap mutes on AT LEAST the configured finger count: a coalesced tap
        // cluster can over-count slightly if a finger re-touches (a new contact
        // id), so `>=` keeps a genuine 3-finger tap muting rather than being
        // dropped as a "4-finger" event. A 1/2-finger tap is still ignored.
        GestureKind::Tap => {
            (cfg.mute && ev.fingers >= cfg.fingers).then_some(GestureAction::MuteToggle)
        }
        // Swipes need EXACTLY the configured count (a 2-finger scroll must not
        // change volume).
        GestureKind::SwipeUp if cfg.volume && ev.fingers == cfg.fingers => {
            Some(GestureAction::VolumeUp)
        }
        GestureKind::SwipeDown if cfg.volume && ev.fingers == cfg.fingers => {
            Some(GestureAction::VolumeDown)
        }
        _ => None,
    }
}

/// Why a gesture that WOULD dispatch under all-on switches was dropped by
/// THIS config — `None` when it wouldn't dispatch anyway (1-finger taps are
/// constant noise) or when config isn't what dropped it. Powers the
/// dropped-by-config log line at the dispatch chokepoint: exactly this hole
/// cost a field debugging round (2026-09-04 — `gestures.mute` sat `false` in
/// the settings DB for weeks and every recognised 3-finger tap vanished
/// WITHOUT A TRACE between "recognised Tap" and "gesture dispatch"; the
/// typing guard logs its vetoes, the config gate did not).
pub fn config_drop_reason(ev: &GestureEvent, cfg: &GestureConfig) -> Option<&'static str> {
    if map_action(ev, cfg).is_some() {
        return None; // it dispatched — nothing was dropped
    }
    let all_on = GestureConfig { volume: true, mute: true, tiptap: true, ..*cfg };
    map_action(ev, &all_on)?; // wouldn't dispatch even with every switch on
    Some(match ev.kind {
        GestureKind::Tap => "gestures.mute is off",
        GestureKind::SwipeUp | GestureKind::SwipeDown => "gestures.volume is off",
        GestureKind::TipTapLeft | GestureKind::TipTapRight => "gestures.tiptap is off",
        _ => return None,
    })
}

/// Seconds since the HID system last saw `event_type`. `f64::INFINITY` on
/// error / off macOS, i.e. "as long ago as possible" — every caller treats that
/// as "no recent activity", so a failure can never fabricate a signal.
#[cfg(target_os = "macos")]
fn seconds_since_hid_event(event_type: u32) -> f64 {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: u32, event_type: u32) -> f64;
    }
    // kCGEventSourceStateHIDSystemState = 1
    let s = unsafe { CGEventSourceSecondsSinceLastEventType(1, event_type) };
    if s.is_finite() && s >= 0.0 {
        s
    } else {
        f64::INFINITY
    }
}

/// Seconds since the last hardware key-down (macOS). Pure modifier presses are
/// flags-changed events, not key-downs, so they do not reset this clock.
/// `f64::INFINITY` elsewhere / on error = the guard never fires.
#[cfg(target_os = "macos")]
fn seconds_since_last_keydown() -> f64 {
    seconds_since_hid_event(10) // kCGEventKeyDown
}
#[cfg(not(target_os = "macos"))]
fn seconds_since_last_keydown() -> f64 {
    f64::INFINITY
}

/// Seconds since the pointer last did anything (moved, dragged, scrolled).
/// Deliberately NOT keyboard: this answers "is a pointing device in use right
/// now", which on a MacBook is the trackpad — and a trackpad in use MUST be
/// producing multitouch frames. See [`liveness_should_rebuild`].
#[cfg(target_os = "macos")]
fn seconds_since_pointer_activity() -> f64 {
    // kCGEventMouseMoved = 5, kCGEventLeftMouseDragged = 6, kCGEventScrollWheel = 22
    [5u32, 6, 22]
        .iter()
        .map(|t| seconds_since_hid_event(*t))
        .fold(f64::INFINITY, f64::min)
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
                #[cfg(target_os = "macos")]
                let has_control = crate::system_commands::ca_volume::has_volume_control();
                #[cfg(not(target_os = "macos"))]
                let has_control = true;
                // ⚠️ Say so when there is nothing to set. Showing a direction
                // arrow for an output WITHOUT a volume control (aggregate /
                // Multi-Output device, HDMI, some DACs) makes a no-op look like
                // it worked — the user is then left guessing for days.
                match crate::system_commands::volume_failure_reason(level, has_control) {
                    Some(reason) => {
                        tracing::warn!("volume gesture: {reason}");
                        crate::status_toast::StatusToast {
                            kind: "volume".into(),
                            on: false,
                            // A non-numeric title draws no bar — the existing
                            // renderer already handles that for the arrow case.
                            title: "—".into(),
                            subtitle: reason.into(),
                        }
                    }
                    None => crate::status_toast::StatusToast {
                        kind: "volume".into(),
                        on: level.map(|l| l > 0).unwrap_or(true),
                        // Title carries the level so the frontend can draw the bar;
                        // falls back to a direction arrow when the OS gives no read-back.
                        title: level
                            .map(|l| format!("{l}%"))
                            .unwrap_or_else(|| if delta > 0 { "+".into() } else { "−".into() }),
                        subtitle: "Volume".into(),
                    },
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
    /// Position of MAXIMUM displacement from `start` seen so far. Swipe
    /// direction/magnitude are read from this (not `last`), so a fast swipe
    /// whose finger lifts at/just-past its peak — with the last tracked frame
    /// possibly stale under sparse frame delivery — is still measured at its
    /// true travel. `travel()` ≥ `disp()` always, so this only ever makes a
    /// swipe MORE reliably detected, never less.
    peak: (f64, f64),
    present: bool,
    /// Sticky size-based palm flag (once a palm, always a palm until lift).
    palm: bool,
}

impl PTrack {
    fn disp(&self) -> f64 {
        (self.last.0 - self.start.0).hypot(self.last.1 - self.start.1)
    }
    /// Distance from `start` to the peak-displacement position (the swipe's true
    /// travel, robust to a stale lift frame).
    fn travel(&self) -> f64 {
        (self.peak.0 - self.start.0).hypot(self.peak.1 - self.start.1)
    }
    /// Travel vector (start → peak), for the coherence/direction test.
    fn travel_vec(&self) -> (f64, f64) {
        (self.peak.0 - self.start.0, self.peak.1 - self.start.1)
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
/// 3. **Per-finger movement / duration at finalise time**: only contacts that
///    moved ≥ `SWIPE_FINGER_MIN_MOVE_NORM` count as swipe fingers; a tap's
///    fingers must have moved < `TAP_FINGER_MAX_MOVE_NORM` and stayed down
///    ≤ `TAP_HOLD_MAX_MS` — so palm + 2-finger scroll yields `fingers == 2`,
///    which `map_action` ignores, and a held chord is not a tap.
///
/// **Tap vs. swipe by COHERENCE, not just magnitude (v0.99.0 — the accidental-
/// mute fix).** A pure magnitude threshold can't cleanly split the two (a weak
/// swipe and a tap-with-drift overlap): the OLD code let a tap's finger drift up
/// to `TAP_FINGER_MAX_MOVE_NORM` (0.12) — the same as the swipe threshold — so a
/// gentle 3-finger swipe of ~0.08–0.11 (just under the bar) fell through to the
/// tap path → an unwanted mute. The real discriminator is **how many fingers
/// travelled, and how coherently**: a genuine tap is essentially stationary (at
/// most ONE finger drifts on lift); a swipe is **≥ 2 fingers travelling
/// coherently** (`|Σvᵢ|/Σ|vᵢ| ≥ SWIPE_COHERENCE_MIN`). So `decide_swipe` fires on
/// ≥ 2 coherent movers each ≥ `SWIPE_FINGER_MIN_MOVE_NORM` (catching the weak
/// swipe as volume) while a divergent pinch/spread cancels out and is rejected;
/// and `finalize_tap` VETOES any ≥ 2-mover gesture (`swipe_geometry` = Some) so
/// it can never mute. Travel is measured to each finger's PEAK (`PTrack.peak`),
/// robust to a stale lift frame. The one-drifting-finger tap (< 2 movers) is
/// preserved exactly.
///
/// **Swipe vs. tap separation (the mute-double-toggle fix).** A SWIPE emits
/// immediately when a real finger LIFTS with ≥2 fingers having moved coherently.
/// A TAP is DEFERRED into an open *cluster* and finalised by [`tick`](Self::tick)
/// once the pad has been quiet for `TAP_SETTLE_MS`. This coalescing is essential:
/// a light multi-finger tap is often reported by the trackpad as SEQUENTIAL
/// single-finger touches (`0→1→0` per finger, ~25 ms apart) rather than a
/// simultaneous `0→3→0` — without the settle window each sub-touch would emit its
/// own tap (the observed unmute-then-remute). Only an actual lift (present →
/// absent) feeds the cluster; a contact merely going *resting* never opens one,
/// and finalising KEEPS still-present contacts so a parked heel's rest-timer
/// isn't reset.
#[derive(Debug, Default)]
pub struct PalmAwareRecognizer {
    tracks: Vec<PTrack>,
    prev_active: usize,
    /// Last frame time (ms) any non-palm contact was present — drives the tap
    /// settle in [`PalmAwareRecognizer::tick`].
    last_contact_ms: u64,
    /// A tap cluster is accumulating: its emit is DEFERRED to `tick`. A light
    /// multi-finger tap can arrive as SEQUENTIAL single-finger touches
    /// (`0→1→0` per finger, ~25 ms apart) rather than a simultaneous `0→3`;
    /// deferring lets the cluster's distinct contacts be counted as ONE
    /// N-finger tap once the pad goes quiet. Swipes are NOT deferred (they emit
    /// immediately, so volume stays responsive).
    cluster_open: bool,
    /// This gesture already emitted an EARLY swipe (v0.110.0) — every later
    /// frame of it (including the final lifts, which would re-qualify in
    /// `decide_swipe`) is consumed. Cleared when the last non-palm contact
    /// leaves the pad.
    swipe_emitted: bool,
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

    /// Feed one frame (every currently-touching contact). A SWIPE is returned
    /// immediately (on all-lift); a TAP is DEFERRED — the cluster stays open and
    /// [`tick`](Self::tick) emits it once the pad settles, coalescing sequential
    /// single-finger touches of a light multi-finger tap into one N-finger tap.
    pub fn feed(&mut self, t_ms: u64, contacts: &[RawContact]) -> Option<GestureEvent> {
        // If a genuinely NEW gesture begins (an unseen contact id) while the
        // previous tap cluster has already settled but the async platform tick
        // hasn't finalised it yet, finalise it FIRST so its lifted tracks don't
        // merge into the fresh gesture. Its tap is returned; the new gesture
        // emits on its own settle/lift. (The 40 ms tick usually wins this race —
        // this is belt-and-braces.)
        let mut carried = None;
        if self.cluster_open
            && t_ms.saturating_sub(self.last_contact_ms) >= TAP_SETTLE_MS
            && !self.tracks.iter().any(|t| t.active(t_ms))
            && contacts
                .iter()
                .any(|c| !self.tracks.iter().any(|t| t.id == c.id))
        {
            carried = self.finalize_tap(t_ms);
            self.reset_keep_present();
        }

        // Record which non-palm tracks were present before this frame, so we can
        // tell an actual finger LIFT (present → absent) from a mere rest
        // transition (a still-present contact whose 600 ms parking makes it
        // "inactive"). Only a real lift feeds the tap cluster.
        let was_present: Vec<i32> = self
            .tracks
            .iter()
            .filter(|t| t.present && !t.palm)
            .map(|t| t.id)
            .collect();

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
                // Track the furthest point from `start` (the swipe's true travel).
                let d_new = (c.x - t.start.0).hypot(c.y - t.start.1);
                if d_new > t.travel() {
                    t.peak = (c.x, c.y);
                }
            } else {
                self.tracks.push(PTrack {
                    id: c.id,
                    start: (c.x, c.y),
                    t_down: t_ms,
                    last: (c.x, c.y),
                    t_last: t_ms,
                    peak: (c.x, c.y),
                    present: true,
                    palm,
                });
            }
        }

        // A non-palm finger LIFTED this frame if it was present last frame and is
        // absent now. (A contact that merely went resting is still `present`, so
        // it is NOT a lift — that was the bug where a parked heel opened a phantom
        // cluster and the tick reset its rest-timer.)
        let lifted = was_present
            .iter()
            .any(|id| !self.tracks.iter().any(|t| t.id == *id && t.present));

        // Active (present, non-palm, non-resting) finger count. Any keeps the
        // settle timer from advancing; a parked palm never does. `active_fingers`
        // (scroll-consume arming) reads this. A LIFT also counts as pad activity
        // — the settle window must run from the lift frame, not from the last
        // frame the finger was still touching (with sparse/erratic frame
        // delivery those can be far apart, and the tick would finalise early,
        // breaking the sequential-sub-touch coalescing).
        self.prev_active = self.tracks.iter().filter(|t| t.active(t_ms)).count();
        if self.prev_active > 0 || lifted {
            self.last_contact_ms = t_ms;
        }

        // EARLY swipe (v0.110.0): a decisive ≥ 3-finger coherent motion emits
        // while the fingers are still DOWN — the action fires mid-swipe instead
        // of after the lift. The rest of this gesture is consumed via
        // `swipe_emitted`. Never while a tap cluster is accumulating (a lifted
        // tap finger means this is not a clean in-flight swipe — the at-lift
        // decision below stays the authority there).
        if !lifted && !self.swipe_emitted && !self.cluster_open {
            if let Some(swipe) = self.decide_swipe_at(t_ms, EARLY_SWIPE_MIN_MOVE_NORM, 3) {
                self.swipe_emitted = true;
                return Some(swipe);
            }
        }

        // SWIPE emits immediately on lift, whenever ≥2 non-palm fingers moved
        // coherently — even while a palm stays resting on the pad.
        if lifted {
            if self.swipe_emitted {
                // Already emitted mid-flight — the end-of-gesture lifts (which
                // would re-qualify in `decide_swipe`) are consumed. When the
                // last non-palm contact leaves, the gesture is over: reset so
                // the next one starts clean.
                if !self.tracks.iter().any(|t| t.present && !t.palm) {
                    self.reset_keep_present();
                    self.swipe_emitted = false;
                }
                return carried;
            }
            if let Some(swipe) = self.decide_swipe(t_ms) {
                self.reset_keep_present();
                return Some(swipe);
            }
            // Diagnostics (v0.113.1): a THREE-plus-contact gesture that produced
            // no swipe is exactly the shape of a "3-finger swipe stopped
            // working" report — and it is invisible in the log otherwise, since
            // only emitted events are logged. One line per such gesture; 1- and
            // 2-finger use (incl. scrolls and pinches) never reaches it, so this
            // cannot become noise. A genuine 3-finger TAP also lands here, and
            // its numbers (movers 0, travel ~0) say so at a glance.
            let contacts = self.tracks.iter().filter(|t| !t.palm).count();
            if contacts >= 3 {
                let (movers, coherence) = self
                    .swipe_geometry(t_ms)
                    .map(|(_, _, n, c)| (n, c))
                    .unwrap_or((0, 0.0));
                let travel = self
                    .tracks
                    .iter()
                    .filter(|t| !t.palm)
                    .map(|t| t.travel())
                    .fold(0.0f64, f64::max);
                let palms = self.tracks.len() - contacts;
                tracing::info!(
                    "gestures: {contacts}-contact gesture ended without a swipe                      (movers={movers}, coherence={coherence:.2} [need {SWIPE_COHERENCE_MIN}],                      max travel={travel:.3} [need {SWIPE_FINGER_MIN_MOVE_NORM}], palms={palms})"
                );
            }
            // No coherent movement → the lifted finger is a tap sub-segment. Keep
            // the cluster open so `tick` coalesces sequential single touches of a
            // light multi-finger tap into ONE N-finger tap.
            self.cluster_open = true;
        }
        carried
    }

    /// Whether the settle ticker has any work: a tap cluster is open and waiting
    /// to be finalised. The platform ticker PARKS while this is false
    /// (PERFORMANCE-PLAN A2) — before v0.166.0 it woke every 24 ms around the
    /// clock, ~42 wakeups/s with no finger on the pad, the bulk of the app's
    /// idle CPU.
    pub fn needs_tick(&self) -> bool {
        self.cluster_open
    }

    /// Milliseconds since the last contact left the pad — for a deferred tap,
    /// how long the emit trails the lift (the typing guard measures against
    /// the LIFT, not the dispatch).
    pub fn since_last_contact_ms(&self, now: u64) -> u64 {
        now.saturating_sub(self.last_contact_ms)
    }

    /// Emit a deferred TAP once the pad has settled. Call periodically (the
    /// platform layer runs a ~40 ms tick); pure + testable. Returns the coalesced
    /// N-finger tap (N = distinct non-palm contacts in the cluster) exactly once,
    /// then resets — keeping any still-resting contact so its rest-timer survives.
    pub fn tick(&mut self, now: u64) -> Option<GestureEvent> {
        if !self.cluster_open || now.saturating_sub(self.last_contact_ms) < TAP_SETTLE_MS {
            return None;
        }
        // Don't finalise while an active finger is still down (a slow-landing tap
        // participant); wait for the pad to actually quiet.
        if self.tracks.iter().any(|t| t.active(now)) {
            return None;
        }
        let ev = self.finalize_tap(now);
        self.reset_keep_present();
        ev
    }

    /// Reset the cluster after emitting, but KEEP contacts that are still on the
    /// pad (a resting palm/thumb) with their original timing — dropping only the
    /// lifted tap/swipe fingers. Resetting them would restart their rest-timer and
    /// make a parked heel spuriously read as an active finger again.
    fn reset_keep_present(&mut self) {
        self.tracks.retain(|t| t.present);
        self.cluster_open = false;
    }

    /// Geometry of the just-ended movement, over the non-palm, non-resting
    /// fingers that each **travelled** ≥ `SWIPE_FINGER_MIN_MOVE_NORM` (measured
    /// to their peak, not their possibly-stale last frame). Returns the resultant
    /// travel `(rx, ry)`, the number of movers `n`, and the directional coherence
    /// `|Σvᵢ| / Σ|vᵢ|` — or `None` when fewer than two fingers moved. Shared by
    /// the swipe decision and the tap veto so they agree on what "a swipe" is.
    fn swipe_geometry(&self, now: u64) -> Option<(f64, f64, usize, f64)> {
        let (mut rx, mut ry, mut sum_mag, mut n) = (0.0f64, 0.0f64, 0.0f64, 0usize);
        for t in &self.tracks {
            if t.palm || (t.present && t.resting(now)) {
                continue;
            }
            let m = t.travel();
            if m >= SWIPE_FINGER_MIN_MOVE_NORM {
                let (vx, vy) = t.travel_vec();
                rx += vx;
                ry += vy;
                sum_mag += m;
                n += 1;
            }
        }
        if n < 2 {
            return None;
        }
        let rmag = rx.hypot(ry);
        let coherence = if sum_mag > 0.0 { rmag / sum_mag } else { 0.0 };
        Some((rx, ry, n, coherence))
    }

    /// Swipe decision for the just-ended segment: **≥ 2 fingers travelling
    /// coherently** in one direction (`coherence ≥ SWIPE_COHERENCE_MIN`). Because
    /// coherence — not a big magnitude — is what proves swipe intent, the per-
    /// finger floor is just `SWIPE_FINGER_MIN_MOVE_NORM`, so a *weak* 3-finger
    /// swipe (~0.08) is caught as volume instead of mis-firing as a tap (the
    /// accidental-mute fix); a pinch/rotate/spread cancels out (low coherence) and
    /// is rejected → no false volume. One drifting finger during a tap gives only
    /// 1 mover, so it never triggers this. `fingers = n`, so a palm + 2-finger
    /// scroll reads as 2 (→ ignored by `map_action`), never 3.
    fn decide_swipe(&self, now: u64) -> Option<GestureEvent> {
        self.decide_swipe_at(now, SWIPE_FINGER_MIN_MOVE_NORM, 2)
    }

    /// Parameterised swipe decision shared by the at-lift path (`min_move` =
    /// `SWIPE_FINGER_MIN_MOVE_NORM`, ≥ 2 movers) and the early in-flight path
    /// (`EARLY_SWIPE_MIN_MOVE_NORM`, ≥ 3 movers — see the constant's doc).
    fn decide_swipe_at(&self, now: u64, min_move: f64, min_fingers: usize) -> Option<GestureEvent> {
        let (rx, ry, n, coherence) = self.swipe_geometry(now)?;
        if n < min_fingers || coherence < SWIPE_COHERENCE_MIN {
            return None;
        }
        let avg = n as f64;
        classify_swipe(rx / avg, ry / avg, min_move)
            .map(|kind| GestureEvent { kind, fingers: n as u8 })
    }

    /// Finalise the open tap cluster: count the DISTINCT non-palm contacts that
    /// touched during it (they may have landed sequentially), and emit one Tap
    /// with that count — provided the whole cluster was brief and low-movement.
    fn finalize_tap(&self, now: u64) -> Option<GestureEvent> {
        // Veto: a genuine tap has its fingers essentially stationary — at most
        // ONE finger drifts on lift. So if ≥ 2 fingers actually TRAVELLED
        // (`swipe_geometry` = Some), this was NOT a tap and must never mute: a
        // *coherent* one is a (weak) swipe `decide_swipe` normally already fired
        // on lift; an *incoherent* one (a pinch/spread) is nothing. Either way,
        // no mute. The one-drifting-finger tap (< 2 movers → None) is preserved.
        if self.swipe_geometry(now).is_some() {
            return None;
        }
        let mut n = 0usize;
        let mut first_down = u64::MAX;
        let mut last_up = 0u64;
        let mut max_disp = 0.0f64;
        let mut max_finger_dur = 0u64;
        for t in &self.tracks {
            // A size-palm or a STILL-resting parked contact isn't a tap finger.
            if t.palm || (t.present && t.resting(now)) {
                continue;
            }
            n += 1;
            first_down = first_down.min(t.t_down);
            last_up = last_up.max(t.t_last);
            max_disp = max_disp.max(t.disp());
            max_finger_dur = max_finger_dur.max(t.t_last.saturating_sub(t.t_down));
        }
        if n == 0 {
            return None;
        }
        let cluster_dur = last_up.saturating_sub(first_down);
        // A tap: fingers touched within a brief cluster, none stayed down long
        // (rejects a HELD chord), none travelled far. `cluster_dur` tolerates
        // sequential landing/lifting — unlike a strict all-fingers-overlap
        // window, which sequential touches never satisfy.
        (cluster_dur <= TAP_CLUSTER_MAX_MS
            && max_finger_dur <= TAP_HOLD_MAX_MS
            && max_disp <= TAP_FINGER_MAX_MOVE_NORM)
            .then_some(GestureEvent { kind: GestureKind::Tap, fingers: n as u8 })
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
    /// One finger resting/settling. `since` = when this rest was established
    /// (reset on movement, so a tap right after a scroll can't fire).
    /// `anchor` = the rest's position when the CURRENT stillness window began:
    /// drift is measured cumulatively against it, not frame-to-frame — a
    /// slowly-mousing thumb moves < `TIPTAP_MAX_MOVE_NORM` per frame while
    /// covering half the pad, and the per-frame check read that as "settled
    /// rest", so a palm graze mid-mousing fired a tab switch (v0.109.0 fix).
    Rest1 { rest: Contact, anchor: Contact, since: u64 },
    /// Rest + one tap finger down. `dir` is locked at tap-land time from the
    /// tap's position relative to the rest finger; `started` = when the tap
    /// landed; `rest_since` carries the rest's settle time so chained taps
    /// don't need to re-settle.
    TapDown { rest: Contact, rest_since: u64, tap: Contact, dir: GestureKind, started: u64 },
    /// The tap finger lifted (the rest finger remains) — emit is DEFERRED one
    /// frame to confirm the lift. If the tap finger re-appears next frame it was
    /// a mid-hold contact flicker (not a real lift) → back to `TapDown`, no emit;
    /// this stops one physical tap from firing twice (double-jump bug). Only a
    /// *confirmed* lift emits.
    TapReleasing { rest: Contact, rest_since: u64, dir: GestureKind, started: u64, lift_t: u64 },
    /// Disqualified (scroll/swipe/too-many fingers) — wait for the finger count
    /// to fall back to a resting posture, then re-settle.
    Poisoned,
}

/// Split two contacts into the resting finger + the tap. The rest is the
/// contact **closest** to the tracked rest position; the tap is the newcomer.
fn split_rest_tap(c: &[Contact], rest: Contact) -> (Contact, Contact) {
    if dist(c[0], rest) <= dist(c[1], rest) {
        (c[0], c[1])
    } else {
        (c[1], c[0])
    }
}

/// Which way a tap points relative to the resting finger, or `None` when it
/// lands on top of it (ambiguous), too far away, or at a wildly different
/// height. Pure — the direction heart of the recogniser.
pub fn tiptap_direction(rest: Contact, tap: Contact) -> Option<GestureKind> {
    if (tap.y - rest.y).abs() > TIPTAP_MAX_DY_NORM {
        return None;
    }
    let dx = tap.x - rest.x; // > 0 = tap is to the right of the rest finger
    if (TIPTAP_MIN_SEP_NORM..=TIPTAP_MAX_DX_NORM).contains(&dx) {
        Some(GestureKind::TipTapRight)
    } else if (TIPTAP_MIN_SEP_NORM..=TIPTAP_MAX_DX_NORM).contains(&(-dx)) {
        Some(GestureKind::TipTapLeft)
    } else {
        None
    }
}

/// **TipTap** recogniser (one-finger-rest, v0.91.6): one finger rests on the
/// pad, a second taps briefly to its left/right → [`GestureKind::TipTapLeft`]/
/// [`TipTapRight`]. Pure + unit-tested; fed per-frame with every contact's
/// position (unordered). Guards: the rest finger must be down ≥
/// `TIPTAP_REST_MIN_MS` first (kills the two-finger scroll/swipe, whose fingers
/// land together), the tap must lift within `TIPTAP_TAP_MAX_MS`, and movement >
/// `TIPTAP_MAX_MOVE_NORM` by the rest finger disqualifies the attempt. Taps
/// chain: the rest finger stays down and a second taps again.
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

    /// Duration-valid tap → its locked direction.
    fn tap_emit(dir: GestureKind, started: u64, lift_t: u64) -> Option<GestureKind> {
        let dur = lift_t.saturating_sub(started);
        (TIPTAP_TAP_MIN_MS..=TIPTAP_TAP_MAX_MS).contains(&dur).then_some(dir)
    }

    fn step(state: TtState, t_ms: u64, c: &[Contact]) -> (TtState, Option<GestureKind>) {
        match state {
            // Idle / Poisoned funnel finger-count changes toward a fresh rest.
            TtState::Idle | TtState::Poisoned => match c.len() {
                0 => (TtState::Idle, None),
                1 => (TtState::Rest1 { rest: c[0], anchor: c[0], since: t_ms }, None),
                _ => (TtState::Poisoned, None), // 2+ landing at once = scroll/swipe
            },
            TtState::Rest1 { rest, anchor, since } => match c.len() {
                0 => (TtState::Idle, None),
                1 => {
                    // Track the rest finger; CUMULATIVE movement since the
                    // stillness window began (scroll, slow mousing) re-arms the
                    // settle timer so a tap right after motion can't fire. A
                    // frame-to-frame check is blind to slow steady motion.
                    let drifted = dist(c[0], anchor) > TIPTAP_MAX_MOVE_NORM;
                    (
                        TtState::Rest1 {
                            rest: c[0],
                            anchor: if drifted { c[0] } else { anchor },
                            since: if drifted { t_ms } else { since },
                        },
                        None,
                    )
                }
                2 => {
                    if t_ms.saturating_sub(since) < TIPTAP_REST_MIN_MS {
                        // The second finger landed before the rest settled → a
                        // two-finger scroll/swipe, not a tip-tap.
                        return (TtState::Poisoned, None);
                    }
                    let (r, tap) = split_rest_tap(c, rest);
                    match tiptap_direction(r, tap) {
                        Some(dir) => (
                            TtState::TapDown { rest: r, rest_since: since, tap, dir, started: t_ms },
                            None,
                        ),
                        // Ambiguous / implausible tap position → not a tip-tap.
                        None => (TtState::Poisoned, None),
                    }
                }
                _ => (TtState::Poisoned, None),
            },
            TtState::TapDown { rest, rest_since, tap, dir, started } => match c.len() {
                // Everything lifted fast — the tap plus the rest went up
                // near-together. Emit if the tap duration was valid.
                0 => (TtState::Idle, Self::tap_emit(dir, started, t_ms)),
                1 => {
                    // One finger lifted. Did the TAP lift (the remaining matches
                    // the rest) or did the rest lift (the tap is still here)?
                    if dist(c[0], rest) <= dist(c[0], tap) {
                        // Rest remains → tap lifted → DEFER one frame to confirm.
                        (TtState::TapReleasing { rest: c[0], rest_since, dir, started, lift_t: t_ms }, None)
                    } else {
                        // The rest finger lifted, tap still down → ambiguous.
                        (TtState::Poisoned, None)
                    }
                }
                2 => {
                    // Still holding. Movement of the rest finger or overstaying
                    // the tap window means it's a scroll/hold, not a tap.
                    let (r, t) = split_rest_tap(c, rest);
                    if dist(r, rest) > TIPTAP_MAX_MOVE_NORM
                        || t_ms.saturating_sub(started) > TIPTAP_TAP_MAX_MS
                    {
                        return (TtState::Poisoned, None);
                    }
                    (TtState::TapDown { rest: r, rest_since, tap: t, dir, started }, None)
                }
                _ => (TtState::Poisoned, None),
            },
            TtState::TapReleasing { rest, rest_since, dir, started, lift_t } => match c.len() {
                // Lift confirmed (tap stayed gone) → emit once.
                0 => (TtState::Idle, Self::tap_emit(dir, started, lift_t)),
                1 => (
                    // The rest finger stays down — chaining: go back to a settled
                    // Rest1 (carry `rest_since`) so a second tap can fire at once.
                    // The anchor restarts at the current position: the rest was
                    // still through the whole tap (TapDown's movement guard).
                    TtState::Rest1 { rest: c[0], anchor: c[0], since: rest_since },
                    Self::tap_emit(dir, started, lift_t),
                ),
                2 => {
                    // The tap finger re-appeared: it was a mid-hold flicker, NOT
                    // a real lift → resume the SAME tap (keep `started`, `dir`),
                    // no emit. This is the double-fire fix.
                    let (r, t) = split_rest_tap(c, rest);
                    if dist(r, rest) > TIPTAP_MAX_MOVE_NORM
                        || t_ms.saturating_sub(started) > TIPTAP_TAP_MAX_MS
                    {
                        return (TtState::Poisoned, None);
                    }
                    (TtState::TapDown { rest: r, rest_since, tap: t, dir, started }, None)
                }
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

/// One-shot migration (v0.84.268): the volume step's default used to be **6**,
/// which `set_gesture_config` persisted verbatim — so installs carry a stored
/// `6` nobody chose (the Settings UI never exposed the field). Reset exactly
/// that historical default to the new 5; any other stored value is a deliberate
/// customisation and is left alone.
pub fn migrate_volume_step_default(db: &DbHandle) {
    const FLAG: &str = "gestures.volume_step_migrated_v0_84_268";
    if crate::settings::get_bool(db, FLAG, false).unwrap_or(false) {
        return;
    }
    if let Ok(stored) = crate::settings::get_or(db, KEY_VOLUME_STEP, "") {
        if stored.trim() == "6" {
            let _ = crate::settings::set(db, KEY_VOLUME_STEP, &DEFAULT_VOLUME_STEP.to_string());
        }
    }
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
                // Typing guard (v0.109.0): a dispatch-layer veto, so the
                // recogniser is untouched — the failure mode of a wrong veto is
                // one swallowed gesture, never broken detection. Each veto logs
                // its own line so the suppression rate is measurable against
                // the isolated-dispatch misfire metric.
                if cfg.typing_guard {
                    let trace = typing_trace_now();
                    let unmuting =
                        action == GestureAction::MuteToggle && output_muted_now() == Some(true);
                    if typing_guard_suppresses(action, trace, unmuting) {
                        tracing::info!(
                            "gesture suppressed (typing {:.2}s before touch, {:.2}s before lift): {:?} ({} fingers) → {:?}",
                            trace.before_touch_s, trace.before_lift_s, ev.kind, ev.fingers, action
                        );
                        return;
                    }
                }
                // One low-volume line per dispatched gesture action (kind +
                // finger count) — the single chokepoint, handy for diagnosing
                // any future gesture misfire without turning on debug.
                tracing::info!("gesture dispatch: {:?} ({} fingers) → {:?}", ev.kind, ev.fingers, action);
                perform(&app_sink, action, step);
            } else if let Some(reason) = config_drop_reason(&ev, &cfg) {
                // A config-gated drop must never be silent (the 2026-09-04
                // lesson): the recogniser said "recognised Tap (3 fingers)",
                // and without this line the trail simply ended.
                tracing::info!(
                    "gesture dropped by config: {:?} ({} fingers) — {}",
                    ev.kind, ev.fingers, reason
                );
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

// ── Liveness watchdog (v0.113.1) ───────────────────────────────────────────
//
// The sleep watchdog below only catches a >60 s wall/monotonic drift. The
// MultitouchSupport registration also goes stale WITHOUT that signal — a short
// nap, a display sleep, a lid cycle, or the trackpad being re-enumerated leave
// the run loop happily spinning on a device that never calls back again. The
// symptom is indistinguishable from "gestures are off" and, until now, only an
// app restart fixed it (observed twice; the second time the process had been
// silent for 40 min while the machine was in active use).
//
// So instead of guessing at every cause, watch the EFFECT: no touch frames for
// a while *even though a pointing device is being used*. On a MacBook the
// pointing device is the trackpad, and a trackpad in use MUST produce frames —
// that combination means the registration is dead, not the user idle.

/// No multitouch frame for this long → the registration is suspect.
pub const LIVENESS_STALE_MS: u64 = 45_000;
/// … but only when the pointer moved this recently. An idle Mac legitimately
/// produces no frames for hours; rebuilding then would be pure churn.
pub const LIVENESS_POINTER_ACTIVE_S: f64 = 12.0;
/// Base gap between rebuild attempts.
pub const LIVENESS_COOLDOWN_S: u64 = 60;
/// The gap doubles after each attempt that doesn't bring frames back, capped
/// here. This bounds the one false positive the design accepts: someone using
/// an EXTERNAL mouse and never touching the trackpad looks exactly like a dead
/// registration. A rebuild is cheap (~30 ms, no user-visible effect), so the
/// trade is a few wasted re-registrations against gestures silently dying.
pub const LIVENESS_MAX_COOLDOWN_S: u64 = 900;

/// Pure: should the capture be rebuilt right now?
///
/// `since_frame_ms = None` means no frame has EVER arrived — the device may
/// simply not exist (no trackpad), so we never rebuild on that; only a source
/// that once worked and then went quiet is treated as stale.
pub fn liveness_should_rebuild(
    since_frame_ms: Option<u64>,
    pointer_idle_s: f64,
    since_rebuild_s: u64,
    cooldown_s: u64,
) -> bool {
    let Some(since_frame_ms) = since_frame_ms else {
        return false;
    };
    since_frame_ms >= LIVENESS_STALE_MS
        && pointer_idle_s <= LIVENESS_POINTER_ACTIVE_S
        && since_rebuild_s >= cooldown_s
}

/// Backoff step after an attempt that didn't restore frames.
pub fn next_liveness_cooldown_s(current: u64) -> u64 {
    current.saturating_mul(2).min(LIVENESS_MAX_COOLDOWN_S)
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
            // 15 s, not 30: the tick is also the liveness cadence, and a dead
            // capture should heal in about a minute, not two.
            const TICK: std::time::Duration = std::time::Duration::from_secs(15);
            const SLEPT_SLACK: std::time::Duration = std::time::Duration::from_secs(60);
            let rebuild = |reason: &str| {
                let (Some(db), Some(state)) = (
                    app.try_state::<DbHandle>(),
                    app.try_state::<GestureState>(),
                ) else {
                    return false;
                };
                tracing::info!("gestures: {reason} — rebuilding the touch capture");
                apply(&app, &db, &state);
                true
            };
            let mut cooldown_s = LIVENESS_COOLDOWN_S;
            let mut last_rebuild: Option<std::time::Instant> = None;
            loop {
                let mono = std::time::Instant::now();
                let wall = std::time::SystemTime::now();
                std::thread::sleep(TICK);
                let mono_elapsed = mono.elapsed();
                let wall_elapsed = wall.elapsed().unwrap_or(mono_elapsed);
                if wall_elapsed > mono_elapsed + SLEPT_SLACK {
                    let slept = (wall_elapsed - mono_elapsed).as_secs();
                    if rebuild(&format!("system slept ~{slept}s")) {
                        // A fresh rebuild deserves a fresh chance: reset the
                        // liveness backoff so a post-wake failure is retried
                        // promptly rather than at whatever gap we'd escalated to.
                        last_rebuild = Some(std::time::Instant::now());
                        cooldown_s = LIVENESS_COOLDOWN_S;
                    }
                    continue;
                }

                // Liveness: frames stopped although a pointing device is in use.
                #[cfg(target_os = "macos")]
                {
                    if !macos::is_running() {
                        continue; // gestures off — nothing to heal
                    }
                    let since_frame = macos::ms_since_last_frame();
                    if since_frame.is_some_and(|ms| ms < LIVENESS_STALE_MS) {
                        cooldown_s = LIVENESS_COOLDOWN_S; // healthy → drop the backoff
                        continue;
                    }
                    let since_rebuild_s = last_rebuild
                        .map(|t| t.elapsed().as_secs())
                        .unwrap_or(u64::MAX);
                    let idle = seconds_since_pointer_activity();
                    if liveness_should_rebuild(since_frame, idle, since_rebuild_s, cooldown_s) {
                        let secs = since_frame.unwrap_or(0) / 1000;
                        if rebuild(&format!(
                            "no touch frames for {secs}s while the pointer was active {idle:.0}s ago (stale registration; next check in {cooldown_s}s)"
                        )) {
                            last_rebuild = Some(std::time::Instant::now());
                            cooldown_s = next_liveness_cooldown_s(cooldown_s);
                        }
                    }
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Liveness watchdog (v0.113.1) ─────────────────────────────────────

    #[test]
    fn liveness_rebuilds_only_when_frames_stopped_while_the_pointer_was_active() {
        let stale = LIVENESS_STALE_MS + 1;
        // The whole point: silent capture + a pointing device in use.
        assert!(liveness_should_rebuild(Some(stale), 1.0, u64::MAX, LIVENESS_COOLDOWN_S));
        // Frames still flowing → nothing wrong.
        assert!(!liveness_should_rebuild(Some(1_000), 1.0, u64::MAX, LIVENESS_COOLDOWN_S));
        // Nobody at the machine → no frames is the CORRECT state, not a fault.
        assert!(!liveness_should_rebuild(Some(stale), 600.0, u64::MAX, LIVENESS_COOLDOWN_S));
        // Just rebuilt → wait out the cooldown instead of hammering.
        assert!(!liveness_should_rebuild(Some(stale), 1.0, 5, LIVENESS_COOLDOWN_S));
    }

    #[test]
    fn liveness_never_fires_before_the_device_has_ever_delivered_a_frame() {
        // A machine with no trackpad would otherwise be rebuilt forever.
        assert!(!liveness_should_rebuild(None, 0.0, u64::MAX, LIVENESS_COOLDOWN_S));
    }

    #[test]
    fn liveness_is_exactly_at_the_boundaries_it_documents() {
        // Pinning the comparisons themselves: >= stale, <= active window.
        assert!(liveness_should_rebuild(Some(LIVENESS_STALE_MS), LIVENESS_POINTER_ACTIVE_S, u64::MAX, 0));
        assert!(!liveness_should_rebuild(Some(LIVENESS_STALE_MS - 1), 0.0, u64::MAX, 0));
        let just_idle = LIVENESS_POINTER_ACTIVE_S + 0.1;
        assert!(!liveness_should_rebuild(Some(LIVENESS_STALE_MS), just_idle, u64::MAX, 0));
    }

    #[test]
    fn the_cooldown_backs_off_and_is_capped() {
        // Bounds the churn for the accepted false positive (external mouse,
        // trackpad legitimately untouched): a handful of retries per hour, not
        // one per minute forever.
        let mut c = LIVENESS_COOLDOWN_S;
        for _ in 0..20 {
            c = next_liveness_cooldown_s(c);
        }
        assert_eq!(c, LIVENESS_MAX_COOLDOWN_S);
        assert_eq!(next_liveness_cooldown_s(LIVENESS_COOLDOWN_S), LIVENESS_COOLDOWN_S * 2);
        assert_eq!(next_liveness_cooldown_s(u64::MAX), LIVENESS_MAX_COOLDOWN_S); // no overflow
    }

    fn test_db() -> DbHandle {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let db = std::sync::Arc::new(parking_lot::Mutex::new(conn));
        crate::settings::init_table(&db).unwrap();
        db
    }

    #[test]
    fn volume_step_migration_resets_only_the_old_default() {
        // A stored 6 (the pre-v0.84.268 default, persisted by save()) → 5.
        let db = test_db();
        crate::settings::set(&db, KEY_VOLUME_STEP, "6").unwrap();
        migrate_volume_step_default(&db);
        assert_eq!(GestureConfig::load(&db).volume_step, 5);
        // One-shot: a later 6 (now a deliberate choice) is left alone.
        crate::settings::set(&db, KEY_VOLUME_STEP, "6").unwrap();
        migrate_volume_step_default(&db);
        assert_eq!(GestureConfig::load(&db).volume_step, 6);
    }

    #[test]
    fn volume_step_migration_keeps_custom_values_and_absent_keys() {
        let db = test_db();
        crate::settings::set(&db, KEY_VOLUME_STEP, "10").unwrap();
        migrate_volume_step_default(&db);
        assert_eq!(GestureConfig::load(&db).volume_step, 10);

        let db2 = test_db();
        migrate_volume_step_default(&db2); // no stored value → default applies
        assert_eq!(GestureConfig::load(&db2).volume_step, DEFAULT_VOLUME_STEP);
    }

    #[test]
    fn config_load_rejects_invalid_volume_steps() {
        // The load filter (`0 < v <= 50`) must catch every garbage shape a
        // hand-edited / corrupted settings row can take — a 0 step would make
        // the swipe a no-op, a negative one would invert it.
        for bad in ["0", "-5", "51", "1000", "abc", "", "5.5"] {
            let db = test_db();
            crate::settings::set(&db, KEY_VOLUME_STEP, bad).unwrap();
            assert_eq!(
                GestureConfig::load(&db).volume_step,
                DEFAULT_VOLUME_STEP,
                "stored {bad:?} should fall back to the default"
            );
        }
    }

    #[test]
    fn config_load_accepts_the_valid_step_range() {
        for good in [1, 2, 5, 10, 25, 50] {
            let db = test_db();
            crate::settings::set(&db, KEY_VOLUME_STEP, &good.to_string()).unwrap();
            assert_eq!(GestureConfig::load(&db).volume_step, good);
        }
    }

    #[test]
    fn config_save_load_round_trip() {
        let db = test_db();
        let cfg = GestureConfig { enabled: true, volume_step: 7, tiptap: true, ..Default::default() };
        cfg.save(&db).unwrap();
        let loaded = GestureConfig::load(&db);
        assert!(loaded.enabled);
        assert_eq!(loaded.volume_step, 7);
        assert!(loaded.tiptap);
    }

    #[test]
    fn fresh_db_loads_pure_defaults() {
        let db = test_db();
        let cfg = GestureConfig::load(&db);
        assert!(!cfg.enabled, "gestures are opt-in");
        assert!(!cfg.tiptap, "tip-tap is opt-in");
        assert_eq!(cfg.volume_step, DEFAULT_VOLUME_STEP);
        assert_eq!(cfg.fingers, DEFAULT_FINGERS);
    }

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
        let mut out = Vec::new();
        let mut last_t = 0u64;
        for (t, cs) in frames {
            // Tick BEFORE feed (like the platform ticker running between frames):
            // a prior tap cluster that has settled is finalised before the next
            // gesture's contacts arrive.
            if let Some(e) = r.tick(*t) {
                out.push(e);
            }
            if let Some(e) = r.feed(*t, cs) {
                out.push(e); // swipes emit immediately; a carried tap too
            }
            last_t = *t;
        }
        // Flush any still-open tap cluster (the platform tick would fire once the
        // pad stays quiet past TAP_SETTLE_MS).
        if let Some(e) = r.tick(last_t + TAP_SETTLE_MS + 10) {
            out.push(e);
        }
        out
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
    fn decisive_swipe_emits_early_while_fingers_are_still_down() {
        // v0.110.0: the volume action must fire MID-SWIPE. Feed only the
        // motion frames (no lift) — the event must already be out.
        let mut r = PalmAwareRecognizer::new();
        let mut early = None;
        for (i, step) in [0.0, 0.25, 0.5, 0.75, 1.0].iter().enumerate() {
            let y = 0.8 + (0.3 - 0.8) * step;
            let cs = vec![rc(1, 0.4, y), rc(2, 0.5, y), rc(3, 0.6, y)];
            if let Some(e) = r.feed(i as u64 * 50, &cs) {
                early = Some((i, e));
                break;
            }
        }
        let (frame, ev) = early.expect("no early emission before any lift");
        assert_eq!(ev, GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 });
        assert!(frame < 4, "emitted only at the end of the motion (frame {frame})");
    }

    #[test]
    fn early_swipe_never_double_emits_and_resets_for_the_next_gesture() {
        // Full gesture: motion (early emit) → more motion → lifts. Exactly ONE
        // event; afterwards a fresh 3-finger tap must still work (state reset).
        let mut frames = swipe_frames(0, 0.8, 0.3, &[]);
        // A fresh 3-finger tap well after the swipe (distinct contact ids).
        frames.push((1000, vec![rc(7, 0.4, 0.5), rc(8, 0.5, 0.5), rc(9, 0.6, 0.5)]));
        frames.push((1060, vec![]));
        let mut r = PalmAwareRecognizer::new();
        let mut out = Vec::new();
        for (t, cs) in &frames {
            if let Some(e) = r.tick(*t) {
                out.push(e);
            }
            if let Some(e) = r.feed(*t, cs) {
                out.push(e);
            }
        }
        if let Some(e) = r.tick(1060 + TAP_SETTLE_MS + 10) {
            out.push(e);
        }
        assert_eq!(
            out,
            vec![
                GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 },
                GestureEvent { kind: GestureKind::Tap, fingers: 3 },
            ],
            "expected exactly one swipe (no lift-phase double emit) + the follow-up tap"
        );
    }

    #[test]
    fn weak_swipe_stays_on_the_at_lift_path() {
        // Travel between the two bars (0.09: ≥ lift bar 0.06, < early bar
        // 0.12): nothing mid-flight, the swipe emits on lift exactly as before.
        let mut r = PalmAwareRecognizer::new();
        for (i, step) in [0.0, 0.5, 1.0].iter().enumerate() {
            let y = 0.55 + (0.46 - 0.55) * step;
            let cs = vec![rc(1, 0.4, y), rc(2, 0.5, y), rc(3, 0.6, y)];
            assert_eq!(r.feed(i as u64 * 50, &cs), None, "weak swipe must not emit early");
        }
        assert_eq!(
            r.feed(150, &[]),
            Some(GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 })
        );
    }

    #[test]
    fn two_finger_motion_never_emits_early() {
        // A 2-finger scroll travelling far: the early path needs ≥ 3 movers
        // (a scroll that later gains a 3rd finger must keep its options open).
        let mut r = PalmAwareRecognizer::new();
        for (i, step) in [0.0, 0.25, 0.5, 0.75, 1.0].iter().enumerate() {
            let y = 0.8 + (0.3 - 0.8) * step;
            let cs = vec![rc(1, 0.45, y), rc(2, 0.55, y)];
            assert_eq!(r.feed(i as u64 * 50, &cs), None, "2-finger motion emitted early");
        }
        // At lift it still yields the (map_action-ignored) 2-finger event.
        assert_eq!(
            r.feed(260, &[]),
            Some(GestureEvent { kind: GestureKind::SwipeUp, fingers: 2 })
        );
    }

    #[test]
    fn early_swipe_fires_with_a_parked_palm_resting() {
        // The parked heel must neither block the early emission nor count into
        // the finger count.
        let heel = rc_palm(9, 0.1, 0.9);
        let mut r = PalmAwareRecognizer::new();
        let mut got = None;
        for (i, step) in [0.0, 0.25, 0.5, 0.75, 1.0].iter().enumerate() {
            let y = 0.8 + (0.3 - 0.8) * step;
            let cs = vec![rc(1, 0.4, y), rc(2, 0.5, y), rc(3, 0.6, y), heel];
            if let Some(e) = r.feed(i as u64 * 50, &cs) {
                got = Some(e);
                break;
            }
        }
        assert_eq!(got, Some(GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 }));
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

    /// The "audio jumps around" fix: a 3-finger tap where ONE finger drifts on
    /// lift must still be a 3-finger TAP (mute) — not dropped, not a SwipeUp.
    /// The drift is swept across the whole range that used to break it: the
    /// dead-zone (0.03–0.06), and above the swipe-finger threshold (≥ 0.06).
    #[test]
    fn three_finger_tap_survives_one_drifting_finger() {
        for drift in [0.04_f64, 0.08, 0.11] {
            // f1 stays, f2 stays, f3 drifts up by `drift` on the way off.
            let evs = palm_events(&[
                (0, vec![rc(1, 0.4, 0.5), rc(2, 0.5, 0.5), rc(3, 0.6, 0.5)]),
                (60, vec![rc(1, 0.4, 0.5), rc(2, 0.5, 0.5), rc(3, 0.6, 0.5 - drift)]),
                (100, vec![]),
            ]);
            assert_eq!(
                evs,
                vec![GestureEvent { kind: GestureKind::Tap, fingers: 3 }],
                "one finger drifting {drift} must stay a 3-finger tap, not a swipe/None",
            );
        }
    }

    /// THE accidental-mute fix: a WEAK 3-finger swipe (~0.09 travel, below the
    /// old 0.12 magnitude threshold) must register as volume (SwipeUp) — NOT
    /// mis-fire as a 3-finger tap/mute. Coherent ≥ 2-finger travel = a swipe,
    /// however gentle.
    #[test]
    fn weak_three_finger_swipe_is_volume_not_a_mute() {
        for (y0, y1, dir) in [
            (0.55, 0.46, GestureKind::SwipeUp),   // 0.09 up
            (0.46, 0.55, GestureKind::SwipeDown), // 0.09 down
        ] {
            let evs = palm_events(&swipe_frames(0, y0, y1, &[]));
            assert_eq!(
                evs,
                vec![GestureEvent { kind: dir, fingers: 3 }],
                "a weak coherent 3-finger swipe ({y0}->{y1}) must be volume, not a mute",
            );
        }
    }

    /// Three fingers moving APART (a pinch/spread) is incoherent — it must be
    /// neither a swipe (no false volume) NOR a tap (no false mute): no event.
    #[test]
    fn divergent_three_finger_spread_is_neither_swipe_nor_mute() {
        let evs = palm_events(&[
            (0, vec![rc(1, 0.4, 0.5), rc(2, 0.5, 0.5), rc(3, 0.6, 0.5)]),
            // f1 travels up, f3 travels down by the same amount → resultant ≈ 0.
            (50, vec![rc(1, 0.4, 0.40), rc(2, 0.5, 0.5), rc(3, 0.6, 0.60)]),
            (100, vec![]),
        ]);
        assert!(
            evs.is_empty(),
            "an incoherent spread must not mute or change volume: {evs:?}",
        );
    }

    /// A light 2-finger tap where both fingers micro-drift (< the swipe floor)
    /// stays a 2-finger tap (which `map_action` ignores) — the ≥2-movers veto
    /// only fires for fingers that actually TRAVELLED ≥ `SWIPE_FINGER_MIN_MOVE_NORM`.
    #[test]
    fn two_finger_micro_drift_stays_a_tap() {
        let evs = palm_events(&[
            (0, vec![rc(1, 0.5, 0.5), rc(2, 0.6, 0.5)]),
            (60, vec![rc(1, 0.5, 0.53), rc(2, 0.6, 0.53)]), // both drift 0.03 (< 0.06)
            (100, vec![]),
        ]);
        assert_eq!(evs, vec![GestureEvent { kind: GestureKind::Tap, fingers: 2 }]);
    }

    /// THE core fix: a light 3-finger tap that the trackpad reports as three
    /// SEQUENTIAL single-finger touches (0→1→0 each, ~25 ms apart — never 3 at
    /// once) must still coalesce into ONE 3-finger tap (→ mute), via the cluster
    /// + settle tick. This is exactly the field-logged failure.
    #[test]
    fn needs_tick_is_true_only_while_a_tap_cluster_waits_for_its_settle() {
        // The ticker's park condition (A2): idle pad → no work; a lifted tap →
        // work until the settle finalises it → no work again. A ticker that
        // parked while a cluster waited would never finalise the tap (mute
        // would die); one that never parked is the old 42-wakeups/s.
        let mut r = PalmAwareRecognizer::new();
        assert!(!r.needs_tick(), "fresh recogniser has nothing to tick");
        let c = |id: i32, x: f64| RawContact { id, x, y: 0.5, size: 1.0 };
        r.feed(0, &[c(1, 0.4), c(2, 0.5), c(3, 0.6)]);
        assert!(!r.needs_tick(), "fingers still down: nothing deferred yet");
        r.feed(60, &[]); // lift → the tap cluster opens, deferred to the tick
        assert!(r.needs_tick(), "a lifted tap must keep the ticker awake");
        assert!(r.tick(100).is_none(), "not settled yet (TAP_SETTLE_MS)");
        assert!(r.needs_tick());
        let ev = r.tick(60 + TAP_SETTLE_MS + 1);
        assert_eq!(ev.map(|e| (e.kind, e.fingers)), Some((GestureKind::Tap, 3)));
        assert!(!r.needs_tick(), "finalised → the ticker may park again");
    }

    #[test]
    fn sequential_single_touches_coalesce_into_one_three_finger_tap() {
        let evs = palm_events(&[
            (0, vec![rc(1, 0.40, 0.5)]),   // finger A lands
            (50, vec![]),                  // A lifts
            (75, vec![rc(2, 0.50, 0.5)]),  // B lands (25 ms later — same cluster)
            (130, vec![]),                 // B lifts
            (155, vec![rc(3, 0.60, 0.5)]), // C lands
            (210, vec![]),                 // C lifts
        ]);
        assert_eq!(
            evs,
            vec![GestureEvent { kind: GestureKind::Tap, fingers: 3 }],
            "three sequential single touches must coalesce into ONE 3-finger tap",
        );
    }

    /// Two distinct 1-finger taps far apart do NOT coalesce into a 2-finger tap
    /// (a real pause resets the cluster) — each is its own (ignored) 1-finger tap.
    #[test]
    fn distinct_taps_after_a_pause_do_not_coalesce() {
        let evs = palm_events(&[
            (0, vec![rc(1, 0.4, 0.5)]),
            (50, vec![]),
            (900, vec![rc(2, 0.6, 0.5)]), // ≫ TAP_SETTLE_MS later → separate cluster
            (950, vec![]),
        ]);
        assert_eq!(
            evs,
            vec![
                GestureEvent { kind: GestureKind::Tap, fingers: 1 },
                GestureEvent { kind: GestureKind::Tap, fingers: 1 },
            ],
        );
    }

    /// A held 3-finger chord is NOT a tap — its overlap window is the hold.
    #[test]
    fn palm_rec_held_chord_is_not_a_tap() {
        let f = |t: u64| (t, vec![rc(1, 0.4, 0.5), rc(2, 0.5, 0.5), rc(3, 0.6, 0.5)]);
        let evs = palm_events(&[f(0), f(200), f(400), (450, vec![])]);
        assert!(evs.is_empty(), "a 400 ms chord must not read as a tap: {evs:?}");
    }

    /// A single physical tap whose contacts FLICKER on lift (touching → gone →
    /// touching → gone within a few frames) must fire the mute toggle ONCE, not
    /// twice — the reported "unmutes then instantly re-mutes" bug.
    #[test]
    fn palm_rec_flickering_lift_taps_once_not_twice() {
        let all = || vec![rc(1, 0.4, 0.5), rc(2, 0.5, 0.5), rc(3, 0.6, 0.5)];
        let evs = palm_events(&[
            (0, all()),
            (100, all()),
            (150, vec![]),  // lift → Tap #1
            (170, all()),   // flicker: contacts re-register …
            (185, all()),   // … for > TAP_MIN_MS (a valid second tap on its own)
            (200, vec![]),  // lift again → suppressed by the refractory
        ]);
        assert_eq!(
            evs,
            vec![GestureEvent { kind: GestureKind::Tap, fingers: 3 }],
            "a flickering lift must toggle mute exactly once",
        );
    }

    /// Two GENUINE 3-finger taps far enough apart both fire — the settle
    /// coalescing collapses one tap's sub-touches, not two distinct taps.
    #[test]
    fn palm_rec_two_genuine_taps_both_fire() {
        let all = || vec![rc(1, 0.4, 0.5), rc(2, 0.5, 0.5), rc(3, 0.6, 0.5)];
        let evs = palm_events(&[
            (0, all()),
            (100, all()),
            (150, vec![]), // Tap #1
            (1100, all()), // a clearly separate tap (deliberate re-taps are ≥ ~1.1 s)
            (1200, all()),
            (1250, vec![]), // Tap #2 — its own cluster (≫ TAP_SETTLE_MS after #1)
        ]);
        assert_eq!(
            evs,
            vec![
                GestureEvent { kind: GestureKind::Tap, fingers: 3 },
                GestureEvent { kind: GestureKind::Tap, fingers: 3 },
            ],
        );
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

    #[test]
    fn four_finger_tap_reads_four_and_still_mutes() {
        // map_action fires on fingers >= cfg (a sloppy 4th finger must not
        // silently disable mute).
        let evs = palm_events(&[
            (0, vec![rc(1, 0.3, 0.5), rc(2, 0.4, 0.5), rc(3, 0.5, 0.5), rc(4, 0.6, 0.5)]),
            (80, vec![]),
        ]);
        assert_eq!(evs, vec![GestureEvent { kind: GestureKind::Tap, fingers: 4 }]);
        let cfg = GestureConfig { enabled: true, ..Default::default() };
        assert_eq!(map_action(&evs[0], &cfg), Some(GestureAction::MuteToggle));
    }

    #[test]
    fn one_finger_dragging_far_is_neither_tap_nor_swipe() {
        // A single finger travelling far: swipe needs ≥2 coherent movers, and
        // its drift exceeds the tap tolerance → the gesture is silently dropped
        // (normal pointer/drag use must never emit anything).
        let evs = palm_events(&[
            (0, vec![rc(1, 0.5, 0.8)]),
            (60, vec![rc(1, 0.5, 0.5)]),
            (120, vec![rc(1, 0.5, 0.2)]),
            (150, vec![]),
        ]);
        assert_eq!(evs, vec![]);
    }

    #[test]
    fn carried_tap_emits_from_feed_when_the_tick_missed_its_slot() {
        // Belt-and-braces path: the cluster has settled but the async ticker
        // hasn't fired — a NEW contact id arriving via feed must finalise the
        // old cluster (returning its tap) instead of merging into it.
        let mut r = PalmAwareRecognizer::new();
        r.feed(0, &[rc(1, 0.4, 0.5), rc(2, 0.5, 0.5), rc(3, 0.6, 0.5)]);
        r.feed(80, &[]); // tap lifts → cluster opens
        // No tick. A fresh gesture starts well past the settle window:
        let carried = r.feed(80 + TAP_SETTLE_MS + 50, &[rc(9, 0.2, 0.2)]);
        assert_eq!(carried, Some(GestureEvent { kind: GestureKind::Tap, fingers: 3 }));
        // The old tracks are gone — the new touch settles as its own 1-finger tap.
        r.feed(300, &[]);
        let next = r.tick(300 + TAP_SETTLE_MS + 10);
        assert_eq!(next, Some(GestureEvent { kind: GestureKind::Tap, fingers: 1 }));
    }

    #[test]
    fn tick_respects_the_settle_window_and_fires_exactly_once() {
        let mut r = PalmAwareRecognizer::new();
        r.feed(0, &[rc(1, 0.4, 0.5), rc(2, 0.5, 0.5), rc(3, 0.6, 0.5)]);
        r.feed(80, &[]);
        // Inside the settle window → not yet.
        assert_eq!(r.tick(80 + TAP_SETTLE_MS - 1), None);
        // Past it → the coalesced tap, exactly once.
        assert_eq!(
            r.tick(80 + TAP_SETTLE_MS),
            Some(GestureEvent { kind: GestureKind::Tap, fingers: 3 })
        );
        assert_eq!(r.tick(80 + TAP_SETTLE_MS + 200), None);
    }

    #[test]
    fn decisive_swipe_emits_in_flight_and_the_lift_is_consumed() {
        // v0.110.0 INVERTS the old "emit only at lift" pin: a decisive
        // ≥ 3-finger motion emits while still touching (that's the latency
        // win), and the lift frame — which would re-qualify in decide_swipe —
        // must then emit NOTHING (the double-emit guard).
        let mut r = PalmAwareRecognizer::new();
        assert_eq!(r.feed(0, &[rc(1, 0.4, 0.8), rc(2, 0.5, 0.8), rc(3, 0.6, 0.8)]), None);
        // Fingers moved well past the early bar and are still touching → emit NOW.
        assert_eq!(
            r.feed(100, &[rc(1, 0.4, 0.4), rc(2, 0.5, 0.4), rc(3, 0.6, 0.4)]),
            Some(GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 })
        );
        // The lift is consumed — no second event.
        assert_eq!(r.feed(150, &[]), None);
    }

    #[test]
    fn swipe_down_with_resting_palm_reads_three_fingers() {
        // Mirror of the SwipeUp+palm case for the down direction (volume down).
        let palm = rc(9, 0.08, 0.9);
        let mut frames: Vec<(u64, Vec<RawContact>)> =
            vec![(0, vec![palm]), (800, vec![palm])];
        frames.extend(swipe_frames(1000, 0.3, 0.8, &[palm]));
        let evs = palm_events(&frames);
        assert_eq!(evs, vec![GestureEvent { kind: GestureKind::SwipeDown, fingers: 3 }]);
    }

    // ── Tip-tap ──────────────────────────────────────────────────────────

    #[allow(dead_code)]
    fn c(x: f64) -> Contact {
        Contact { x, y: 0.5 }
    }

    fn tiptap_events(frames: &[(u64, Vec<Contact>)]) -> Vec<GestureKind> {
        let mut r = TipTapRecognizer::new();
        frames.iter().filter_map(|(t, cs)| r.feed(*t, cs)).collect()
    }

    // One resting finger at a given height.
    fn rest_one() -> Vec<Contact> {
        vec![Contact { x: 0.48, y: 0.5 }]
    }
    // Rest finger + a tap finger at `tap_x`.
    fn rest_tap(tap_x: f64) -> Vec<Contact> {
        vec![Contact { x: 0.48, y: 0.5 }, Contact { x: tap_x, y: 0.5 }]
    }

    #[test]
    fn tiptap_rest_then_tap_right() {
        // One finger rests; a second taps to its RIGHT (0.68) for ~60 ms → next.
        let evs = tiptap_events(&[
            (0, rest_one()),
            (100, rest_one()), // settled ≥ REST_MIN_MS
            (150, rest_tap(0.68)), // TapDown
            (210, rest_one()),     // tap lifted (rest remains) → TapReleasing
            (300, rest_one()),     // confirmed → emit
            (400, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapRight]);
    }

    #[test]
    fn tiptap_rest_then_tap_left() {
        // A second finger taps to the LEFT of the rest (0.28) → previous.
        let evs = tiptap_events(&[
            (0, rest_one()),
            (100, rest_one()),
            (150, rest_tap(0.28)),
            (210, rest_one()),
            (300, rest_one()),
            (400, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapLeft]);
    }

    #[test]
    fn tiptap_tap_on_top_of_the_rest_is_ambiguous() {
        // A tap landing right on the rest finger has no clear direction.
        let evs = tiptap_events(&[
            (0, rest_one()),
            (100, rest_one()),
            (150, rest_tap(0.49)), // dx 0.01 < MIN_SEP
            (210, rest_one()),
            (300, rest_one()),
            (400, vec![]),
        ]);
        assert!(evs.is_empty(), "an on-top tap must not fire");
    }

    #[test]
    fn tiptap_chains_while_the_rest_stays_down() {
        // Human-speed chaining: the rest finger stays down and a second taps
        // again. The confirm is a 1-contact frame (not a full lift), and the
        // carried rest-settle time lets the next tap fire without re-settling.
        let evs = tiptap_events(&[
            (0, rest_one()),
            (100, rest_one()),
            (150, rest_tap(0.68)), // TapDown
            (210, rest_one()),     // TapReleasing
            (260, rest_one()),     // confirmed → emit #1 (Right)
            (520, rest_tap(0.28)), // TapDown (rest still settled)
            (580, rest_one()),     // TapReleasing
            (640, rest_one()),     // emit #2 (Left) — > EMIT_GAP_MS after #1
            (800, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapRight, GestureKind::TipTapLeft]);
    }

    #[test]
    fn tiptap_rapid_deliberate_chaining_is_not_swallowed() {
        // Two deliberate taps ~250 ms apart must BOTH fire.
        let evs = tiptap_events(&[
            (0, rest_one()),
            (100, rest_one()),
            (150, rest_tap(0.68)),
            (210, rest_one()),
            (250, rest_one()), // emit #1
            (410, rest_tap(0.68)),
            (470, rest_one()),
            (500, rest_one()), // emit #2 — 250 ms after #1
            (600, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapRight, GestureKind::TipTapRight]);
    }

    #[test]
    fn tiptap_midhold_flicker_does_not_double_fire() {
        // The tap contact briefly drops out mid-hold (a MultitouchSupport state
        // flicker) then returns before the REAL lift. The deferred lift-confirm
        // fires exactly ONCE — no jump-two-tabs.
        let evs = tiptap_events(&[
            (0, rest_one()),
            (100, rest_one()),
            (150, rest_tap(0.68)), // TapDown
            (200, rest_one()),     // flicker: tap gone one frame → TapReleasing
            (210, rest_tap(0.68)), // tap back → flicker, not a lift → TapDown
            (420, rest_one()),     // the real lift → TapReleasing
            (500, rest_one()),     // confirmed → single emit
            (600, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapRight]);
    }

    #[test]
    fn tiptap_bounce_fires_at_most_once() {
        // The tap's lift "bounces": the contact re-appears for a frame right
        // after emitting. The emit refractory must swallow it.
        let evs = tiptap_events(&[
            (0, rest_one()),
            (100, rest_one()),
            (150, rest_tap(0.68)),
            (210, rest_one()),     // TapReleasing
            (250, rest_one()),     // emit
            (270, rest_tap(0.68)), // bounce re-contact 20 ms later → TapDown
            (300, rest_one()),     // its lift is within the 200 ms refractory
            (400, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapRight]);
    }

    #[test]
    fn tiptap_recovers_after_a_rejected_attempt_without_full_lift() {
        // A two-finger scroll/swipe poisons; falling back to the resting finger
        // and re-settling must re-arm (no need to lift everything).
        let evs = tiptap_events(&[
            (0, rest_one()),
            (10, rest_tap(0.68)), // 2nd landed within settle → poisoned
            (200, rest_tap(0.68)),
            (250, rest_one()), // back to the rest → recovering
            (400, rest_one()), // settled ≥ REST_MIN_MS
            (450, rest_tap(0.68)),
            (510, rest_one()),
            (560, rest_one()), // valid tap → emit
            (700, vec![]),
        ]);
        assert_eq!(evs, vec![GestureKind::TipTapRight]);
    }

    #[test]
    fn tiptap_tolerates_an_angled_hand() {
        // The tap lands noticeably HIGHER than the rest finger (Δy = 0.28) — a
        // natural angled-hand tip-tap; must fire.
        let ok = tiptap_events(&[
            (0, rest_one()),
            (100, rest_one()),
            (150, vec![
                Contact { x: 0.48, y: 0.5 },
                Contact { x: 0.68, y: 0.22 }, // Δy 0.28 from the rest
            ]),
            (210, rest_one()),
            (300, rest_one()),
            (400, vec![]),
        ]);
        assert_eq!(ok, vec![GestureKind::TipTapRight]);
        // …but a wildly different height (Δy = 0.70) is no tip-tap posture.
        let too_high = tiptap_events(&[
            (0, rest_one()),
            (100, rest_one()),
            (150, vec![
                Contact { x: 0.48, y: 0.5 },
                Contact { x: 0.68, y: 1.20 }, // Δy 0.70 from the rest
            ]),
            (210, rest_one()),
            (300, rest_one()),
            (400, vec![]),
        ]);
        assert!(too_high.is_empty());
    }

    #[test]
    fn tiptap_rejects_two_fingers_landing_together_scroll_guard() {
        // The second finger lands before the rest settled → a scroll, not a tap.
        let evs = tiptap_events(&[
            (0, rest_one()),
            (30, rest_tap(0.68)), // 30 ms < REST_MIN_MS
            (90, rest_one()),
            (150, vec![]),
        ]);
        assert!(evs.is_empty());
        // Two fingers landing at once (a scroll/swipe start) never fires either.
        let evs = tiptap_events(&[
            (0, rest_tap(0.68)),
            (60, rest_tap(0.68)),
            (120, vec![]),
        ]);
        assert!(evs.is_empty());
    }

    #[test]
    fn tiptap_rejects_movement_and_overstay() {
        // Both glide (a two-finger scroll/swipe) → the rest finger moves →
        // poisoned.
        let scroll = tiptap_events(&[
            (0, rest_one()),
            (100, rest_one()),
            (150, rest_tap(0.68)),
            (200, vec![
                Contact { x: 0.48, y: 0.30 }, // rest moved up 0.20
                Contact { x: 0.68, y: 0.30 },
            ]),
            (260, rest_one()),
            (300, vec![]),
        ]);
        assert!(scroll.is_empty());
        // The tap finger overstays the tap window (a two-finger rest).
        let hold = tiptap_events(&[
            (0, rest_one()),
            (100, rest_one()),
            (150, rest_tap(0.68)),
            (600, rest_tap(0.68)), // > TAP_MAX_MS
            (700, rest_one()),
            (800, vec![]),
        ]);
        assert!(hold.is_empty());
    }

    #[test]
    fn tiptap_no_emit_when_the_rest_finger_lifts_instead() {
        // During the hold the REST finger lifts (only the tap remains) →
        // ambiguous, nothing fires.
        let evs = tiptap_events(&[
            (0, rest_one()),
            (100, rest_one()),
            (150, rest_tap(0.68)),
            (210, vec![Contact { x: 0.68, y: 0.5 }]), // the rest is gone, tap remains
            (300, vec![]),
        ]);
        assert!(evs.is_empty());
    }

    #[test]
    fn tiptap_single_finger_rest_alone_never_fires() {
        // Just one resting finger with no tap — normal cursor use must never be
        // read as a tip-tap.
        let evs = tiptap_events(&[
            (0, rest_one()),
            (100, rest_one()),
            (200, rest_one()),
            (260, vec![]),
        ]);
        assert!(evs.is_empty());
    }

    #[test]
    fn tiptap_direction_edges() {
        let rest = Contact { x: 0.48, y: 0.5 };
        assert_eq!(
            tiptap_direction(rest, Contact { x: 0.70, y: 0.5 }),
            Some(GestureKind::TipTapRight)
        );
        assert_eq!(
            tiptap_direction(rest, Contact { x: 0.26, y: 0.5 }),
            Some(GestureKind::TipTapLeft)
        );
        // On top of the rest finger → None.
        assert_eq!(tiptap_direction(rest, Contact { x: 0.49, y: 0.5 }), None);
        // Too far away → None.
        assert_eq!(tiptap_direction(rest, Contact { x: 0.98, y: 0.5 }), None);
        // Too high (Δy 0.60 > the 0.55 limit) → None.
        assert_eq!(tiptap_direction(rest, Contact { x: 0.70, y: 1.10 }), None);
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
        let on = GestureConfig { enabled: true, tiptap: true, ..Default::default() };
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

    #[test]
    fn map_action_per_gesture_switches() {
        // Each switch kills exactly its own gesture family, nothing else.
        let base = GestureConfig { enabled: true, tiptap: true, ..Default::default() };
        let up = GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 };
        let down = GestureEvent { kind: GestureKind::SwipeDown, fingers: 3 };
        let tap = GestureEvent { kind: GestureKind::Tap, fingers: 3 };
        let right = GestureEvent { kind: GestureKind::TipTapRight, fingers: 2 };

        let no_volume = GestureConfig { volume: false, ..base };
        assert_eq!(map_action(&up, &no_volume), None);
        assert_eq!(map_action(&down, &no_volume), None);
        assert_eq!(map_action(&tap, &no_volume), Some(GestureAction::MuteToggle));
        assert_eq!(map_action(&right, &no_volume), Some(GestureAction::NextTab));

        let no_mute = GestureConfig { mute: false, ..base };
        assert_eq!(map_action(&tap, &no_mute), None);
        assert_eq!(map_action(&up, &no_mute), Some(GestureAction::VolumeUp));
        assert_eq!(map_action(&right, &no_mute), Some(GestureAction::NextTab));
    }

    #[test]
    fn a_config_gated_drop_is_named_never_silent() {
        // The 2026-09-04 field lesson: `gestures.mute` sat false in the DB
        // and every recognised 3-finger tap vanished between "recognised"
        // and "dispatch" with NO log line — the typing guard logs its
        // vetoes, the config gate didn't. `config_drop_reason` names the
        // responsible switch, and ONLY for gestures that would otherwise
        // have dispatched (a 1-finger tap is constant noise, not a drop).
        let base = GestureConfig { enabled: true, tiptap: true, ..Default::default() };
        let tap3 = GestureEvent { kind: GestureKind::Tap, fingers: 3 };
        let tap1 = GestureEvent { kind: GestureKind::Tap, fingers: 1 };
        let up = GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 };
        let right = GestureEvent { kind: GestureKind::TipTapRight, fingers: 2 };

        let no_mute = GestureConfig { mute: false, ..base };
        assert_eq!(config_drop_reason(&tap3, &no_mute), Some("gestures.mute is off"));
        // Would never dispatch anyway → not a config drop, no log noise.
        assert_eq!(config_drop_reason(&tap1, &no_mute), None);
        // Dispatched fine → nothing dropped.
        assert_eq!(config_drop_reason(&tap3, &base), None);
        assert_eq!(config_drop_reason(&up, &no_mute), None);

        let no_volume = GestureConfig { volume: false, ..base };
        assert_eq!(config_drop_reason(&up, &no_volume), Some("gestures.volume is off"));
        let no_tiptap = GestureConfig { tiptap: false, ..base };
        assert_eq!(config_drop_reason(&right, &no_tiptap), Some("gestures.tiptap is off"));
        // Master switch off = the whole source never runs; the reason helper
        // stays quiet rather than blaming an individual gesture switch.
        let disabled = GestureConfig { enabled: false, ..base };
        assert_eq!(config_drop_reason(&tap3, &disabled), None);
    }

    fn trace(before_touch_s: f64, before_lift_s: f64) -> TypingTrace {
        TypingTrace { before_touch_s, before_lift_s }
    }

    #[test]
    fn typing_guard_suppresses_volume_and_mute_only_within_window() {
        use GestureAction::*;
        // Typing right before the touch: volume + mute suppressed, tabs never.
        for a in [VolumeUp, VolumeDown, MuteToggle] {
            assert!(typing_guard_suppresses(a, trace(0.0, 0.0), false));
            assert!(typing_guard_suppresses(a, trace(TYPING_GUARD_S - 0.01, f64::INFINITY), false));
            // At/after the boundary: allowed again.
            assert!(!typing_guard_suppresses(a, trace(TYPING_GUARD_S, TYPING_GUARD_S), false));
            assert!(!typing_guard_suppresses(a, trace(3600.0, 3600.0), false));
        }
        for a in [NextTab, PrevTab] {
            assert!(!typing_guard_suppresses(a, trace(0.0, 0.0), false), "tab actions are exempt: {a:?}");
        }
        // The error sentinel (no data) must never suppress.
        assert!(!typing_guard_suppresses(VolumeUp, trace(f64::INFINITY, f64::INFINITY), false));
    }

    #[test]
    fn a_key_pressed_after_the_lift_never_vetoes() {
        use GestureAction::*;
        // The 2026-09-05 log: "typing 0.01 s ago" at a dispatch that trailed
        // the lift by ≥ 160 ms — the key came AFTER the tap. Not a palm.
        for a in [VolumeUp, VolumeDown, MuteToggle] {
            assert!(!typing_guard_suppresses(a, trace(f64::INFINITY, -0.01), false), "{a:?}");
            assert!(!typing_guard_suppresses(a, trace(f64::INFINITY, -0.4), false), "{a:?}");
        }
        // …but a key DURING the touch (before the lift) still vetoes.
        assert!(typing_guard_suppresses(MuteToggle, trace(f64::INFINITY, 0.2), false));
    }

    #[test]
    fn typing_right_before_the_touch_vetoes_even_when_keys_continue_after() {
        // Fingers landed 0.3 s after a key, typing resumed after the lift:
        // the touch-start sample is the palm signature and wins.
        assert!(typing_guard_suppresses(GestureAction::VolumeUp, trace(0.3, -0.2), false));
    }

    #[test]
    fn an_unmute_is_never_vetoed() {
        // Same trace, same action: only the direction differs. A vetoed unmute
        // strands the user muted; an accidental unmute is harmless.
        let t = trace(0.0, 0.0);
        assert!(typing_guard_suppresses(GestureAction::MuteToggle, t, false));
        assert!(!typing_guard_suppresses(GestureAction::MuteToggle, t, true));
        // The exemption is mute-specific: volume ignores the flag.
        assert!(typing_guard_suppresses(GestureAction::VolumeUp, t, true));
    }

    #[test]
    fn gesture_config_missing_new_fields_defaults_on() {
        // A pre-v0.109.0 frontend payload (no typing_guard/volume/mute keys)
        // must deserialise with all three ON — never silently disable guards.
        let cfg: GestureConfig = serde_json::from_str(
            r#"{"enabled":true,"fingers":3,"volume_step":5,"tiptap":false}"#,
        )
        .unwrap();
        assert!(cfg.typing_guard);
        assert!(cfg.volume);
        assert!(cfg.mute);
    }

    #[test]
    fn tiptap_slow_drift_then_tap_does_not_fire() {
        // The v0.109.0 misfire: a thumb mousing SLOWLY (0.02/frame — under the
        // per-frame movement bar) covers 0.18 of the pad, then a second finger
        // grazes the pad. Frame-to-frame checks read the mover as a settled
        // rest and fired a tab switch; the cumulative anchor re-arms the
        // settle timer, so the graze right after motion must NOT emit.
        let drift: Vec<(u64, Vec<Contact>)> = (0..10)
            .map(|i| (i * 20, vec![Contact { x: 0.30 + 0.02 * i as f64, y: 0.5 }]))
            .collect();
        let mut frames = drift;
        frames.push((200, vec![Contact { x: 0.48, y: 0.5 }, Contact { x: 0.68, y: 0.5 }]));
        frames.push((260, vec![Contact { x: 0.48, y: 0.5 }]));
        frames.push((300, vec![Contact { x: 0.48, y: 0.5 }]));
        frames.push((340, vec![]));
        assert_eq!(tiptap_events(&frames), vec![], "tap right after slow motion must not fire");
    }

    #[test]
    fn tiptap_fires_once_the_drifting_rest_settles() {
        // Same slow drift, but the rest then holds STILL past REST_MIN before
        // the tap lands — a deliberate tip-tap after moving the cursor must
        // still work (the guard blocks motion, not the user).
        let mut frames: Vec<(u64, Vec<Contact>)> = (0..10)
            .map(|i| (i * 20, vec![Contact { x: 0.30 + 0.02 * i as f64, y: 0.5 }]))
            .collect();
        frames.push((300, vec![Contact { x: 0.48, y: 0.5 }])); // still ≥ REST_MIN
        frames.push((320, vec![Contact { x: 0.48, y: 0.5 }, Contact { x: 0.68, y: 0.5 }]));
        frames.push((380, vec![Contact { x: 0.48, y: 0.5 }]));
        frames.push((420, vec![Contact { x: 0.48, y: 0.5 }]));
        frames.push((500, vec![]));
        assert_eq!(tiptap_events(&frames), vec![GestureKind::TipTapRight]);
    }
}
