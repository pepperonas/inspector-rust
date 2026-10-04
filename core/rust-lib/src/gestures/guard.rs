//! Gesture guard: the filter pipeline between raw touches and dispatched
//! actions (Phase 2 of the programme in `docs/gestures.md`).
//!
//! Four levels, all pure (no OS calls; the platform layer only feeds
//! [`Frame`]s, key-down times and clock ticks):
//!
//! 1. **Classification per contact** — finger / thumb / palm / unclear.
//!    Palm and thumb are sticky until the contact lifts; a contact is a
//!    finger only after `settle_ms` (until then "unclear", which still counts
//!    as a finger, so landing taps aren't delayed).
//! 2. **Spatial** — edge zones (per device profile: built-in vs. external)
//!    exclude contacts that LAND there; a finger that slides in from the
//!    middle stays valid. Optionally, a palm on the pad blocks every gesture.
//! 3. **Typing** — a key-down shortly before or during a touch blocks volume
//!    and mute (single key → `typing_single_ms`, inside a burst →
//!    `typing_burst_ms`). Modifiers never count, tab switching and UNMUTE are
//!    never blocked, a key after the lift never counts. Optionally the block
//!    holds until a touch starts in the centre area.
//! 4. **Plausibility** — the recognisers' own rules (coherence, minimum
//!    travel, tap window/hold/move — their thresholds now come from here), plus
//!    a constant finger count for swipes, an optional minimum speed and
//!    evenness, and a cooldown after every accepted gesture.
//!
//! Every decision — accepted or not, with level and reason — is returned as a
//! [`Decision`], and a contact excluded at level 1/2 produces one decision of
//! its own.

use super::trace::{DeviceInfo, Frame, KeyDown, Touch};
use super::{
    config_drop_reason, map_action, Contact, GestureAction, GestureConfig, GestureEvent,
    GestureKind, PalmAwareRecognizer, RawContact, RecParams, SwipeStats, TipTapRecognizer,
    EARLY_SWIPE_MIN_MOVE_NORM, PALM_SIZE, SWIPE_COHERENCE_MIN, SWIPE_FINGER_MIN_MOVE_NORM,
    TAP_CLUSTER_MAX_MS, TAP_FINGER_MAX_MOVE_NORM, TAP_HOLD_MAX_MS,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

// ── Config ───────────────────────────────────────────────────────────────────

/// Edge zones as fractions of the pad (0.05 = 5 %).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EdgeZones {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

impl EdgeZones {
    pub const DEFAULT: EdgeZones = EdgeZones { left: 0.03, right: 0.03, top: 0.05, bottom: 0.05 };

    /// The point lies in one of the zones (y grows downwards).
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x < self.left || x > 1.0 - self.right || y < self.top || y > 1.0 - self.bottom
    }

    fn clamped(self) -> EdgeZones {
        let c = |v: f64| if v.is_finite() { v.clamp(0.0, MAX_EDGE) } else { 0.0 };
        EdgeZones { left: c(self.left), right: c(self.right), top: c(self.top), bottom: c(self.bottom) }
    }
}

/// Largest edge zone: a quarter of the pad per side.
pub const MAX_EDGE: f64 = 0.25;

/// Every threshold of the four levels. `#[serde(default)]` on the struct: a
/// stored config missing a newer field gets that field's default instead of
/// failing to load (the `gestures.mute` lesson — a missing field must never
/// silently switch something off).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GuardConfig {
    // Level 1
    /// A contact is a finger only after this long (ms); before, "unclear".
    pub settle_ms: u64,
    /// Palm: contact size at or above this (the historic rule).
    pub palm_size: f32,
    /// Palm: ellipse major axis at or above this; 0 = off.
    pub palm_major: f32,
    /// Thumb: major/minor ratio at or above this; 0 = off.
    pub thumb_ratio: f32,
    /// Thumb: and at least this size.
    pub thumb_min_size: f32,
    /// Thumb: and landing at or below this height (0 = top, 1 = bottom).
    pub thumb_zone: f64,
    // Level 2
    pub edges_builtin: EdgeZones,
    pub edges_external: EdgeZones,
    /// While a palm rests on the pad, no gesture fires at all.
    pub palm_blocks_all: bool,
    // Level 3
    pub typing_single_ms: u64,
    pub typing_burst_ms: u64,
    /// Two key-downs at most this far apart make a burst.
    pub burst_gap_ms: u64,
    /// After typing, gestures stay blocked until a touch starts in the centre.
    pub release_by_center: bool,
    /// Centre area as a fraction of the pad, centred (0.5 = middle half).
    pub center_size: f64,
    // Level 4
    pub constant_count: bool,
    pub coherence_min: f64,
    pub swipe_min_move: f64,
    pub early_min_move: f64,
    /// Mean finger travel per second for a swipe; 0 = off.
    pub min_speed: f64,
    /// Shortest / longest finger travel of a swipe; 0 = off.
    pub evenness_min: f64,
    pub tap_window_ms: u64,
    pub tap_hold_max_ms: u64,
    pub tap_max_move: f64,
    pub cooldown_ms: u64,
}

impl Default for GuardConfig {
    fn default() -> Self {
        GuardConfig {
            settle_ms: 30,
            palm_size: PALM_SIZE,
            palm_major: 0.0,
            thumb_ratio: 1.7,
            thumb_min_size: 1.3,
            thumb_zone: 0.6,
            edges_builtin: EdgeZones::DEFAULT,
            edges_external: EdgeZones::DEFAULT,
            palm_blocks_all: false,
            typing_single_ms: 250,
            typing_burst_ms: 600,
            burst_gap_ms: 500,
            release_by_center: false,
            center_size: 0.5,
            constant_count: true,
            coherence_min: SWIPE_COHERENCE_MIN,
            swipe_min_move: SWIPE_FINGER_MIN_MOVE_NORM,
            early_min_move: EARLY_SWIPE_MIN_MOVE_NORM,
            min_speed: 0.0,
            evenness_min: 0.0,
            tap_window_ms: TAP_CLUSTER_MAX_MS,
            tap_hold_max_ms: TAP_HOLD_MAX_MS,
            tap_max_move: TAP_FINGER_MAX_MOVE_NORM,
            cooldown_ms: 150,
        }
    }
}

impl GuardConfig {
    /// Every value forced into a sane range — a hand-edited or old stored
    /// config can't wedge the recognisers (NaN, negative, absurd).
    pub fn normalized(self) -> GuardConfig {
        let d = GuardConfig::default();
        let f = |v: f64, lo: f64, hi: f64, def: f64| if v.is_finite() { v.clamp(lo, hi) } else { def };
        let f32c = |v: f32, lo: f32, hi: f32, def: f32| if v.is_finite() { v.clamp(lo, hi) } else { def };
        GuardConfig {
            settle_ms: self.settle_ms.min(300),
            palm_size: f32c(self.palm_size, 0.5, 10.0, d.palm_size),
            palm_major: f32c(self.palm_major, 0.0, 100.0, 0.0),
            thumb_ratio: f32c(self.thumb_ratio, 0.0, 5.0, d.thumb_ratio),
            thumb_min_size: f32c(self.thumb_min_size, 0.0, 10.0, d.thumb_min_size),
            thumb_zone: f(self.thumb_zone, 0.0, 1.0, d.thumb_zone),
            edges_builtin: self.edges_builtin.clamped(),
            edges_external: self.edges_external.clamped(),
            palm_blocks_all: self.palm_blocks_all,
            typing_single_ms: self.typing_single_ms.min(3_000),
            typing_burst_ms: self.typing_burst_ms.min(5_000),
            burst_gap_ms: self.burst_gap_ms.clamp(50, 2_000),
            release_by_center: self.release_by_center,
            center_size: f(self.center_size, 0.1, 1.0, d.center_size),
            constant_count: self.constant_count,
            coherence_min: f(self.coherence_min, 0.0, 1.0, d.coherence_min),
            swipe_min_move: f(self.swipe_min_move, 0.01, 0.5, d.swipe_min_move),
            early_min_move: f(self.early_min_move, 0.01, 1.0, d.early_min_move),
            min_speed: f(self.min_speed, 0.0, 20.0, 0.0),
            evenness_min: f(self.evenness_min, 0.0, 1.0, 0.0),
            tap_window_ms: self.tap_window_ms.clamp(50, 3_000),
            tap_hold_max_ms: self.tap_hold_max_ms.clamp(50, 3_000),
            tap_max_move: f(self.tap_max_move, 0.005, 0.5, d.tap_max_move),
            cooldown_ms: self.cooldown_ms.min(2_000),
        }
    }

    pub fn rec_params(&self) -> RecParams {
        RecParams {
            swipe_min_move: self.swipe_min_move,
            early_min_move: self.early_min_move,
            coherence_min: self.coherence_min,
            tap_cluster_max_ms: self.tap_window_ms,
            tap_hold_max_ms: self.tap_hold_max_ms,
            tap_max_move: self.tap_max_move,
        }
    }

    /// Edge zones for a device: the external profile only when the platform
    /// says it is NOT built in; unknown → built-in.
    pub fn edges_for(&self, device: Option<&DeviceInfo>) -> EdgeZones {
        match device.and_then(|d| d.builtin) {
            Some(false) => self.edges_external,
            _ => self.edges_builtin,
        }
    }

    fn in_center(&self, x: f64, y: f64) -> bool {
        let half = self.center_size / 2.0;
        (x - 0.5).abs() <= half && (y - 0.5).abs() <= half
    }
}

// ── Decisions ────────────────────────────────────────────────────────────────

/// Which level made a decision. `Config` = the user's per-gesture switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Classify,
    Spatial,
    Typing,
    Plausibility,
    Config,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchClass {
    Unclear,
    Finger,
    Thumb,
    Palm,
}

/// Why a decision went the way it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Accepted,
    /// Contact classified as a palm (level 1).
    Palm,
    /// Contact classified as a thumb (level 1).
    Thumb,
    /// Contact landed in an edge zone (level 2).
    EdgeZone,
    /// A palm lay on the pad and `palm_blocks_all` is on (level 2).
    PalmOnPad,
    /// More contacts were down than took part in the swipe (level 4).
    FingerCountChanged,
    TooSlow,
    UnevenFingers,
    Cooldown,
    /// Key-down shortly before or during the touch (level 3).
    Typing,
    /// Still blocked after typing until a touch starts in the centre (level 3).
    TypingAwaitCenter,
    /// A per-gesture switch is off.
    Config(&'static str),
    /// Recognised, but nothing is bound to it.
    Unmapped,
}

impl Reason {
    /// Stable code for logs, fixtures and the UI.
    pub fn code(&self) -> String {
        match self {
            Reason::Accepted => "dispatched".into(),
            Reason::Palm => "palm".into(),
            Reason::Thumb => "thumb".into(),
            Reason::EdgeZone => "edge_zone".into(),
            Reason::PalmOnPad => "palm_on_pad".into(),
            Reason::FingerCountChanged => "finger_count_changed".into(),
            Reason::TooSlow => "too_slow".into(),
            Reason::UnevenFingers => "uneven_fingers".into(),
            Reason::Cooldown => "cooldown".into(),
            Reason::Typing => "typing_guard".into(),
            Reason::TypingAwaitCenter => "typing_await_center".into(),
            Reason::Config(r) => format!("config:{r}"),
            Reason::Unmapped => "unmapped".into(),
        }
    }
}

/// Which recogniser path produced a gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Via {
    /// Palm-aware recogniser, from a frame (swipes).
    Frame,
    /// Palm-aware recogniser, from the settle ticker (deferred taps).
    Tick,
    TipTap,
    /// A platform that recognises gestures itself (Windows, Linux).
    External,
}

/// One decision of the pipeline.
#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub t_ms: u64,
    pub device: u32,
    pub level: Level,
    pub reason: Reason,
    pub accepted: bool,
    /// The gesture, for gesture decisions; `None` for a contact decision.
    pub event: Option<GestureEvent>,
    pub via: Option<Via>,
    /// The bound action, when there is one (also on a rejected gesture).
    pub action: Option<GestureAction>,
    /// The contact, for a contact decision.
    pub touch_id: Option<i32>,
}

// ── Level 1 + 2: per-contact state ───────────────────────────────────────────

#[derive(Debug, Clone)]
struct TouchMeta {
    first_ms: u64,
    class: TouchClass,
    in_edge: bool,
    max_size: f32,
    max_major: f32,
    max_ratio: f32,
    start_y: f64,
    /// The driver flagged it as not a finger at some point (sticky).
    driver_palm: bool,
}

impl TouchMeta {
    fn excluded(&self) -> bool {
        self.in_edge || matches!(self.class, TouchClass::Palm | TouchClass::Thumb)
    }
}

/// Classify one contact frame (level 1). Palm and thumb are sticky: once set,
/// a smaller contact area later on doesn't turn the contact back into a
/// finger. The stickiness comes from judging the RUNNING MAXIMA of size, axis
/// and ratio (and the fixed landing height) — no separate latch, which the
/// mutation probe showed would be dead code.
fn classify(meta: &mut TouchMeta, t: &Touch, now: u64, g: &GuardConfig) {
    meta.max_size = meta.max_size.max(t.size);
    meta.driver_palm |= t.palm;
    meta.max_major = meta.max_major.max(t.major);
    if t.minor > 0.0 {
        meta.max_ratio = meta.max_ratio.max(t.major / t.minor);
    }
    let palm = meta.driver_palm
        || meta.max_size >= g.palm_size
        || (g.palm_major > 0.0 && meta.max_major >= g.palm_major);
    let thumb = g.thumb_ratio > 0.0
        && meta.max_ratio >= g.thumb_ratio
        && meta.max_size >= g.thumb_min_size
        && meta.start_y >= g.thumb_zone;
    meta.class = if palm {
        TouchClass::Palm
    } else if thumb {
        TouchClass::Thumb
    } else if now.saturating_sub(meta.first_ms) >= g.settle_ms {
        TouchClass::Finger
    } else {
        TouchClass::Unclear
    };
}

/// Contacts the palm-aware recogniser sees (capacity of the live buffer).
pub const PALM_FEED_MAX: usize = 11;
/// Contacts the tip-tap recogniser sees: one rest + one tap, and a third so it
/// can tell "too many" and poison.
pub const TIPTAP_FEED_MAX: usize = 3;

/// Split one frame into the two recogniser feeds. Only on-pad contacts
/// (MAKE/TOUCHING/BREAK) take part; the palm-aware feed keeps excluded
/// contacts (flagged) so a resting palm's timing survives, the tip-tap feed
/// drops them. Shared by the live path and the replay.
pub fn contact_feeds(touches: &[Touch], excluded: impl Fn(&Touch) -> bool) -> (Vec<RawContact>, Vec<Contact>) {
    let mut palm = Vec::with_capacity(touches.len().min(PALM_FEED_MAX));
    let mut tiptap = Vec::with_capacity(TIPTAP_FEED_MAX);
    for t in touches.iter().filter(|t| t.phase.on_pad()) {
        let ex = excluded(t);
        if palm.len() < PALM_FEED_MAX {
            palm.push(RawContact { id: t.id, x: t.x, y: t.y, excluded: ex });
        }
        if !ex && tiptap.len() < TIPTAP_FEED_MAX {
            tiptap.push(Contact { x: t.x, y: t.y });
        }
    }
    (palm, tiptap)
}

struct DevState {
    palm: PalmAwareRecognizer,
    tiptap: TipTapRecognizer,
    meta: HashMap<i32, TouchMeta>,
    prev_count: usize,
    touch_start_ms: Option<u64>,
    palm_in_gesture: bool,
}

// ── Level 3: typing ──────────────────────────────────────────────────────────

/// Key-downs the pipeline knows about. `complete` = every key-down is
/// reported (a keyboard tap); otherwise only "the last key-down" is visible
/// and bursts can't be told apart, so every key gets the burst window.
#[derive(Debug, Default)]
struct Typing {
    keys: VecDeque<u64>,
    complete: bool,
    /// Typed since the last touch that started in the centre.
    await_center: bool,
}

const KEY_HISTORY: usize = 64;

impl Typing {
    fn push(&mut self, t: u64) {
        if self.keys.back().is_some_and(|&k| t.abs_diff(k) <= 5) {
            return;
        }
        if self.keys.len() == KEY_HISTORY {
            self.keys.pop_front();
        }
        self.keys.push_back(t);
        self.await_center = true;
    }

    fn window(&self, i: usize, g: &GuardConfig) -> u64 {
        let k = self.keys[i];
        let burst = !self.complete || (i > 0 && k - self.keys[i - 1] <= g.burst_gap_ms);
        if burst {
            g.typing_burst_ms
        } else {
            g.typing_single_ms
        }
    }

    /// A key-down inside `[touch_start − window, lift]` blocks.
    fn blocks(&self, touch_start: u64, lift: u64, g: &GuardConfig) -> bool {
        (0..self.keys.len()).any(|i| {
            let k = self.keys[i];
            k <= lift && k + self.window(i, g) >= touch_start
        })
    }
}

// ── The pipeline ─────────────────────────────────────────────────────────────

/// Where the pipeline gets "is the output muted?" from: live, the system;
/// in a replay, its own tracked state.
pub type MuteProbe = fn() -> Option<bool>;

pub struct Pipeline {
    cfg: GestureConfig,
    g: GuardConfig,
    devices: Vec<DeviceInfo>,
    dev: HashMap<u32, DevState>,
    typing: Typing,
    last_accept_ms: Option<u64>,
    muted: bool,
    mute_probe: Option<MuteProbe>,
}

impl Pipeline {
    /// `complete_keys`: every key-down will be reported (see [`Typing`]).
    pub fn new(cfg: GestureConfig, devices: Vec<DeviceInfo>, complete_keys: bool) -> Pipeline {
        Pipeline {
            g: cfg.guard.normalized(),
            cfg,
            devices,
            dev: HashMap::new(),
            typing: Typing { complete: complete_keys, ..Typing::default() },
            last_accept_ms: None,
            muted: false,
            mute_probe: None,
        }
    }

    pub fn with_mute(mut self, muted: bool, probe: Option<MuteProbe>) -> Pipeline {
        self.muted = muted;
        self.mute_probe = probe;
        self
    }

    /// Whether every key-down is being reported (a keyboard tap is live).
    /// Switching it on doesn't rewrite keys already known; it only changes
    /// how new ones are judged (single vs. burst instead of "always burst").
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn set_complete_keys(&mut self, complete: bool) {
        self.typing.complete = complete;
    }

    /// Device facts (built-in or not) the edge profiles key on, in the order
    /// of [`Frame::device`].
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn set_devices(&mut self, devices: Vec<DeviceInfo>) {
        self.devices = devices;
    }

    /// A key-down at `t_ms`. Modifiers never count (macOS doesn't even report
    /// them as key-downs; synthetic traces mark them).
    pub fn key(&mut self, k: KeyDown) {
        if !k.modifier {
            self.typing.push(k.t_ms);
        }
    }

    fn state(&mut self, device: u32) -> &mut DevState {
        let p = self.g.rec_params();
        self.dev.entry(device).or_insert_with(|| DevState {
            palm: PalmAwareRecognizer::with_params(p),
            tiptap: TipTapRecognizer::new(),
            meta: HashMap::new(),
            prev_count: 0,
            touch_start_ms: None,
            palm_in_gesture: false,
        })
    }

    /// Active (gesture-relevant) contacts on a device — the platform arms
    /// its scroll swallowing on this.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn active_fingers(&self, device: u32) -> usize {
        self.dev.get(&device).map(|d| d.palm.active_fingers()).unwrap_or(0)
    }

    /// Replace the guard thresholds without a restart (the panel's sliders).
    /// The per-device recogniser state is dropped — it was built with the old
    /// thresholds; contacts still down are re-classified from scratch.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn set_guard(&mut self, g: GuardConfig) {
        self.cfg.guard = g;
        self.g = g.normalized();
        self.dev.clear();
    }

    /// The contacts of `frame` with their current classification, for the
    /// live view. Call after [`Pipeline::feed`] with the same frame.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn live_touches(&self, frame: &Frame) -> Vec<super::live::LiveTouch> {
        let meta = self.dev.get(&frame.device).map(|d| &d.meta);
        frame
            .touches
            .iter()
            .filter(|t| t.phase.on_pad())
            .map(|t| {
                let m = meta.and_then(|m| m.get(&t.id));
                super::live::live_touch(
                    t,
                    m.map(|m| m.class).unwrap_or(TouchClass::Unclear),
                    m.is_some_and(|m| m.in_edge),
                )
            })
            .collect()
    }

    /// Typing level right now: would a touch starting at `now` be blocked by
    /// a recent key-down, and is the "release by a centre touch" still owed?
    pub fn typing_state(&self, now: u64) -> (bool, bool) {
        (
            self.cfg.typing_guard && self.typing.blocks(now, now, &self.g),
            self.cfg.typing_guard && self.g.release_by_center && self.typing.await_center,
        )
    }

    /// A deferred tap is waiting for [`Pipeline::tick`].
    pub fn needs_tick(&self) -> bool {
        self.dev.values().any(|d| d.palm.needs_tick())
    }

    /// Feed one frame of one device.
    pub fn feed(&mut self, frame: &Frame) -> Vec<Decision> {
        let g = self.g;
        let edges = g.edges_for(self.devices.get(frame.device as usize));
        let t = frame.t_ms;
        let device = frame.device;
        let mut out = Vec::new();

        let ds = self.state(device);
        let count = frame.touches.len();
        if count > 0 && ds.prev_count == 0 {
            ds.touch_start_ms = Some(t);
            ds.palm_in_gesture = false;
        }
        ds.prev_count = count;

        // Level 1 + 2 per contact.
        let on_pad: Vec<&Touch> = frame.touches.iter().filter(|x| x.phase.on_pad()).collect();
        ds.meta.retain(|id, _| on_pad.iter().any(|x| x.id == *id));
        let mut started_in_center = false;
        for touch in &on_pad {
            let fresh = !ds.meta.contains_key(&touch.id);
            let meta = ds.meta.entry(touch.id).or_insert_with(|| TouchMeta {
                first_ms: t,
                class: TouchClass::Unclear,
                in_edge: edges.contains(touch.x, touch.y),
                max_size: 0.0,
                max_major: 0.0,
                max_ratio: 0.0,
                start_y: touch.y,
                driver_palm: false,
            });
            if fresh && g.in_center(touch.x, touch.y) {
                started_in_center = true;
            }
            // A contact that lands already excluded (edge zone) must still
            // get its own decision — it was never "not excluded".
            let was_excluded = !fresh && meta.excluded();
            classify(meta, touch, t, &g);
            if meta.class == TouchClass::Palm {
                ds.palm_in_gesture = true;
            }
            if meta.excluded() && !was_excluded {
                let (level, reason) = if meta.in_edge {
                    (Level::Spatial, Reason::EdgeZone)
                } else if meta.class == TouchClass::Palm {
                    (Level::Classify, Reason::Palm)
                } else {
                    (Level::Classify, Reason::Thumb)
                };
                out.push(Decision {
                    t_ms: t,
                    device,
                    level,
                    reason,
                    accepted: false,
                    event: None,
                    via: None,
                    action: None,
                    touch_id: Some(touch.id),
                });
            }
        }
        if started_in_center {
            self.typing.await_center = false;
        }

        let ds = self.dev.get_mut(&device).expect("state exists");
        let meta = &ds.meta;
        let (raw, contacts) =
            contact_feeds(&frame.touches, |x| meta.get(&x.id).is_some_and(TouchMeta::excluded));
        let palm_ev = ds.palm.feed(t, &raw);
        let stats = ds.palm.last_swipe();
        let tiptap_ev = ds.tiptap.feed(t, &contacts);
        let touch_start = ds.touch_start_ms.unwrap_or(t);
        let palm_present = ds.palm_in_gesture;

        if let Some(ev) = palm_ev {
            out.push(self.judge(t, device, ev, Via::Frame, stats, touch_start, t, palm_present));
        }
        if let Some(kind) = tiptap_ev {
            let ev = GestureEvent { kind, fingers: 3 };
            out.push(self.judge(t, device, ev, Via::TipTap, None, touch_start, t, palm_present));
        }
        out
    }

    /// Finalise deferred taps (call every ~24 ms while [`Pipeline::needs_tick`]).
    pub fn tick(&mut self, now: u64) -> Vec<Decision> {
        let mut found = Vec::new();
        for (&device, ds) in self.dev.iter_mut() {
            if let Some(ev) = ds.palm.tick(now) {
                let lift = now.saturating_sub(ds.palm.since_last_contact_ms(now));
                found.push((device, ev, ds.touch_start_ms.unwrap_or(lift), lift, ds.palm_in_gesture));
            }
        }
        found
            .into_iter()
            .map(|(device, ev, start, lift, palm)| self.judge(now, device, ev, Via::Tick, None, start, lift, palm))
            .collect()
    }

    /// A gesture a platform recognised itself (Windows/Linux): levels 3 and 4
    /// and the config still apply; there is no touch start, so the event time
    /// stands in for it.
    pub fn external(&mut self, t_ms: u64, ev: GestureEvent) -> Decision {
        self.judge(t_ms, 0, ev, Via::External, None, t_ms, t_ms, false)
    }

    /// A gesture the platform recognised and then CANCELLED because the number
    /// of fingers changed mid-gesture (libinput does this). With the
    /// constant-count rule on it is rejected on level 4 like a recognised
    /// swipe whose finger count changed; otherwise it is judged normally.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub fn external_count_changed(&mut self, t_ms: u64, ev: GestureEvent) -> Decision {
        let stats = SwipeStats { peak_active: ev.fingers as usize + 1, ..SwipeStats::default() };
        self.judge(t_ms, 0, ev, Via::External, Some(stats), t_ms, t_ms, false)
    }

    #[allow(clippy::too_many_arguments)]
    fn judge(
        &mut self,
        t: u64,
        device: u32,
        ev: GestureEvent,
        via: Via,
        stats: Option<SwipeStats>,
        touch_start: u64,
        lift: u64,
        palm_present: bool,
    ) -> Decision {
        let action = map_action(&ev, &self.cfg);
        let reject = |level: Level, reason: Reason| Decision {
            t_ms: t,
            device,
            level,
            reason,
            accepted: false,
            event: Some(ev),
            via: Some(via),
            action,
            touch_id: None,
        };
        let is_swipe = matches!(
            ev.kind,
            GestureKind::SwipeUp | GestureKind::SwipeDown | GestureKind::SwipeLeft | GestureKind::SwipeRight
        );

        if self.g.palm_blocks_all && palm_present {
            return reject(Level::Spatial, Reason::PalmOnPad);
        }
        if let (true, Some(s)) = (is_swipe, stats) {
            if self.g.constant_count && s.peak_active > ev.fingers as usize {
                return reject(Level::Plausibility, Reason::FingerCountChanged);
            }
            if self.g.min_speed > 0.0 && s.duration_ms > 0 {
                let speed = s.mean_travel / (s.duration_ms as f64 / 1000.0);
                if speed < self.g.min_speed {
                    return reject(Level::Plausibility, Reason::TooSlow);
                }
            }
            if self.g.evenness_min > 0.0 && s.max_travel > 0.0 && s.min_travel / s.max_travel < self.g.evenness_min {
                return reject(Level::Plausibility, Reason::UnevenFingers);
            }
        }
        let Some(act) = action else {
            let reason = match config_drop_reason(&ev, &self.cfg) {
                Some(r) => Reason::Config(r),
                None => Reason::Unmapped,
            };
            return reject(Level::Config, reason);
        };

        let tab = matches!(act, GestureAction::NextTab | GestureAction::PrevTab);
        if self.cfg.typing_guard && !tab {
            let muted = self.mute_probe.and_then(|p| p()).unwrap_or(self.muted);
            let unmuting = act == GestureAction::MuteToggle && muted;
            if !unmuting {
                if self.typing.blocks(touch_start, lift, &self.g) {
                    return reject(Level::Typing, Reason::Typing);
                }
                if self.g.release_by_center && self.typing.await_center {
                    return reject(Level::Typing, Reason::TypingAwaitCenter);
                }
            }
        }
        if self.last_accept_ms.is_some_and(|l| t.saturating_sub(l) < self.g.cooldown_ms) {
            return reject(Level::Plausibility, Reason::Cooldown);
        }

        self.last_accept_ms = Some(t);
        if act == GestureAction::MuteToggle {
            self.muted = !self.muted;
        }
        Decision {
            t_ms: t,
            device,
            level: Level::Plausibility,
            reason: Reason::Accepted,
            accepted: true,
            event: Some(ev),
            via: Some(via),
            action: Some(act),
            touch_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::trace::TouchPhase;
    use super::*;

    fn cfg() -> GestureConfig {
        GestureConfig { enabled: true, tiptap: true, ..GestureConfig::default() }
    }

    fn touch(id: i32, x: f64, y: f64) -> Touch {
        Touch { id, x, y, vx: 0.0, vy: 0.0, major: 8.0, minor: 7.0, angle: 0.0, size: 1.0, phase: TouchPhase::Touching, palm: false }
    }

    fn frame(t: u64, touches: Vec<Touch>) -> Frame {
        Frame { t_ms: t, device: 0, touches }
    }

    fn g() -> GuardConfig {
        GuardConfig::default()
    }

    // Level 1

    #[test]
    fn a_contact_is_unclear_until_it_settles_then_a_finger() {
        let mut m = TouchMeta { first_ms: 0, class: TouchClass::Unclear, in_edge: false, max_size: 0.0, max_major: 0.0, max_ratio: 0.0, start_y: 0.5, driver_palm: false };
        classify(&mut m, &touch(1, 0.5, 0.5), 10, &g());
        assert_eq!(m.class, TouchClass::Unclear);
        classify(&mut m, &touch(1, 0.5, 0.5), 30, &g());
        assert_eq!(m.class, TouchClass::Finger);
    }

    #[test]
    fn palm_and_thumb_are_sticky() {
        let mut m = TouchMeta { first_ms: 0, class: TouchClass::Unclear, in_edge: false, max_size: 0.0, max_major: 0.0, max_ratio: 0.0, start_y: 0.5, driver_palm: false };
        let big = Touch { size: 2.5, ..touch(1, 0.5, 0.5) };
        classify(&mut m, &big, 5, &g());
        assert_eq!(m.class, TouchClass::Palm);
        classify(&mut m, &touch(1, 0.5, 0.5), 200, &g()); // area shrinks
        assert_eq!(m.class, TouchClass::Palm, "a palm never turns back into a finger");

        let mut t = TouchMeta { first_ms: 0, class: TouchClass::Unclear, in_edge: false, max_size: 0.0, max_major: 0.0, max_ratio: 0.0, start_y: 0.8, driver_palm: false };
        let flat = Touch { size: 1.5, major: 14.0, minor: 7.0, ..touch(2, 0.5, 0.8) };
        classify(&mut t, &flat, 40, &g());
        assert_eq!(t.class, TouchClass::Thumb);
        classify(&mut t, &touch(2, 0.5, 0.8), 300, &g());
        assert_eq!(t.class, TouchClass::Thumb);
    }

    #[test]
    fn the_major_axis_palm_rule_is_off_by_default_and_works_when_set() {
        let long = Touch { size: 1.2, major: 25.0, minor: 20.0, ..touch(1, 0.5, 0.3) };
        let fresh = || TouchMeta { first_ms: 0, class: TouchClass::Unclear, in_edge: false, max_size: 0.0, max_major: 0.0, max_ratio: 0.0, start_y: 0.3, driver_palm: false };
        let mut m = fresh();
        classify(&mut m, &long, 40, &g());
        assert_eq!(m.class, TouchClass::Finger);
        let mut m = fresh();
        classify(&mut m, &long, 40, &GuardConfig { palm_major: 20.0, ..g() });
        assert_eq!(m.class, TouchClass::Palm);
    }

    #[test]
    fn a_flat_contact_high_on_the_pad_is_not_a_thumb() {
        let mut m = TouchMeta { first_ms: 0, class: TouchClass::Unclear, in_edge: false, max_size: 0.0, max_major: 0.0, max_ratio: 0.0, start_y: 0.3, driver_palm: false };
        let flat = Touch { size: 1.5, major: 14.0, minor: 7.0, ..touch(1, 0.5, 0.3) };
        classify(&mut m, &flat, 40, &g());
        assert_eq!(m.class, TouchClass::Finger);
    }

    #[test]
    fn thumb_rule_can_be_switched_off() {
        let gg = GuardConfig { thumb_ratio: 0.0, ..g() };
        let mut m = TouchMeta { first_ms: 0, class: TouchClass::Unclear, in_edge: false, max_size: 0.0, max_major: 0.0, max_ratio: 0.0, start_y: 0.8, driver_palm: false };
        classify(&mut m, &Touch { size: 1.5, major: 14.0, minor: 7.0, ..touch(1, 0.5, 0.8) }, 40, &gg);
        assert_eq!(m.class, TouchClass::Finger);
    }

    // Level 2

    #[test]
    fn edge_zone_only_catches_contacts_that_land_there() {
        let mut p = Pipeline::new(cfg(), vec![], true);
        let d = p.feed(&frame(0, vec![touch(1, 0.5, 0.02)]));
        assert_eq!(d.iter().map(|d| d.reason).collect::<Vec<_>>(), [Reason::EdgeZone]);
        // A contact landing in the middle and sliding INTO the zone stays valid.
        let mut p = Pipeline::new(cfg(), vec![], true);
        p.feed(&frame(0, vec![touch(2, 0.5, 0.5)]));
        let d = p.feed(&frame(10, vec![touch(2, 0.5, 0.01)]));
        assert!(d.is_empty(), "{d:?}");
    }

    #[test]
    fn edge_zone_test_is_exclusive_of_the_inner_border() {
        let z = EdgeZones::DEFAULT;
        assert!(z.contains(0.02, 0.5) && z.contains(0.5, 0.97) && !z.contains(0.5, 0.5));
        assert!(!z.contains(0.03, 0.5), "exactly on the border is inside the pad");
    }

    #[test]
    fn external_trackpads_use_their_own_profile() {
        let gg = GuardConfig { edges_external: EdgeZones { left: 0.2, ..EdgeZones::DEFAULT }, ..g() };
        let ext = DeviceInfo { builtin: Some(false), width_mm: None, height_mm: None };
        let built = DeviceInfo { builtin: Some(true), width_mm: None, height_mm: None };
        assert_eq!(gg.edges_for(Some(&ext)).left, 0.2);
        assert_eq!(gg.edges_for(Some(&built)).left, EdgeZones::DEFAULT.left);
        assert_eq!(gg.edges_for(None).left, EdgeZones::DEFAULT.left, "unknown device → built-in");
    }

    // Level 3

    #[test]
    fn single_key_and_burst_get_their_own_window() {
        let gg = g();
        let mut ty = Typing { complete: true, ..Typing::default() };
        ty.push(1_000);
        assert!(ty.blocks(1_200, 1_300, &gg), "200 ms after a single key");
        assert!(!ty.blocks(1_300, 1_400, &gg), "300 ms after a single key is past 250");
        ty.push(1_300); // second key 300 ms later → burst
        assert!(ty.blocks(1_850, 1_950, &gg), "550 ms after a burst key is inside 600");
        assert!(!ty.blocks(1_950, 2_000, &gg));
    }

    #[test]
    fn a_key_after_the_lift_never_blocks() {
        let mut ty = Typing { complete: true, ..Typing::default() };
        ty.push(500);
        assert!(!ty.blocks(200, 400, &g()));
    }

    #[test]
    fn incomplete_history_uses_the_burst_window() {
        let mut ty = Typing::default(); // complete = false
        ty.push(1_000);
        assert!(ty.blocks(1_500, 1_550, &g()), "can't tell single from burst → 600 ms");
    }

    #[test]
    fn a_live_keyboard_tap_switches_from_the_burst_window_to_single() {
        let mut p = Pipeline::new(cfg(), vec![], false);
        p.key(KeyDown { t_ms: 1_000, modifier: false });
        assert!(p.typing.blocks(1_400, 1_450, &g()), "incomplete: 600 ms");
        p.set_complete_keys(true);
        assert!(p.typing.complete);
        assert!(!p.typing.blocks(1_400, 1_450, &g()), "complete + single key: 250 ms");
    }

    #[test]
    fn modifiers_never_arm_the_guard() {
        let mut p = Pipeline::new(cfg(), vec![], true);
        p.key(KeyDown { t_ms: 10, modifier: true });
        assert!(p.typing.keys.is_empty() && !p.typing.await_center);
    }

    fn tap3(p: &mut Pipeline, t0: u64) -> Vec<Decision> {
        tap3_at(p, t0, 0.5)
    }

    fn tap3_at(p: &mut Pipeline, t0: u64, y: f64) -> Vec<Decision> {
        let ts = || vec![touch(1, 0.4, y), touch(2, 0.5, y), touch(3, 0.6, y)];
        let mut out = Vec::new();
        for t in (t0..t0 + 90).step_by(10) {
            out.extend(p.feed(&frame(t, ts())));
        }
        out.extend(p.feed(&frame(t0 + 90, vec![])));
        let mut t = t0 + 90;
        while p.needs_tick() && t < t0 + 2_000 {
            t += 24;
            out.extend(p.tick(t));
        }
        out.into_iter().filter(|d| d.event.is_some()).collect()
    }

    #[test]
    fn center_release_holds_the_block_until_a_centre_touch() {
        let c = GestureConfig { guard: GuardConfig { release_by_center: true, center_size: 0.2, ..g() }, ..cfg() };
        let mut p = Pipeline::new(c, vec![], true);
        p.key(KeyDown { t_ms: 0, modifier: false });
        // Long after the timeout, but no centre touch yet (y 0.3 lies outside the 0.2 box).
        let d = tap3_at(&mut p, 5_000, 0.3);
        assert_eq!(d[0].reason, Reason::TypingAwaitCenter);
        // A touch that starts in the centre releases it.
        p.feed(&frame(8_000, vec![touch(9, 0.5, 0.5)]));
        p.feed(&frame(8_020, vec![]));
        let d = tap3_at(&mut p, 9_000, 0.3);
        // The centre touch was itself a 1-finger tap (unmapped); the 3-finger
        // tap after it goes through.
        let three = d.iter().find(|d| d.event.is_some_and(|e| e.fingers == 3)).unwrap();
        assert_eq!(three.reason, Reason::Accepted);
    }

    #[test]
    fn unmute_and_tabs_are_never_blocked_by_typing() {
        let mut p = Pipeline::new(cfg(), vec![], true).with_mute(true, None);
        p.key(KeyDown { t_ms: 990, modifier: false });
        assert_eq!(tap3(&mut p, 1_000)[0].reason, Reason::Accepted, "unmute");
        p.key(KeyDown { t_ms: 2_990, modifier: false });
        let tab = p.judge(3_000, 0, GestureEvent { kind: GestureKind::TipTapRight, fingers: 3 }, Via::TipTap, None, 2_995, 3_000, false);
        assert!(tab.accepted, "{tab:?}");
    }

    // Level 4

    #[test]
    fn a_platform_cancelled_swipe_is_a_changed_finger_count() {
        let up = GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 };
        let mut p = Pipeline::new(cfg(), vec![], true);
        let d = p.external_count_changed(1_000, up);
        assert!(!d.accepted);
        assert_eq!((d.level, d.reason), (Level::Plausibility, Reason::FingerCountChanged));
        // Rule off: the cancelled swipe is judged like any other.
        let loose = GestureConfig { guard: GuardConfig { constant_count: false, ..g() }, ..cfg() };
        let mut p = Pipeline::new(loose, vec![], true);
        assert!(p.external_count_changed(1_000, up).accepted);
    }

    #[test]
    fn cooldown_rejects_a_second_gesture_right_after_the_first() {
        let mut p = Pipeline::new(cfg(), vec![], true);
        let up = GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 };
        assert!(p.external(1_000, up).accepted);
        assert_eq!(p.external(1_100, up).reason, Reason::Cooldown);
        assert!(p.external(1_200, up).accepted, "150 ms later it fires again");
    }

    #[test]
    fn a_joining_contact_rejects_the_swipe_but_only_when_constant_count_is_on() {
        let stats = SwipeStats { peak_active: 4, duration_ms: 200, min_travel: 0.3, max_travel: 0.3, mean_travel: 0.3 };
        let up = GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 };
        let mut p = Pipeline::new(cfg(), vec![], true);
        assert_eq!(p.judge(0, 0, up, Via::Frame, Some(stats), 0, 0, false).reason, Reason::FingerCountChanged);
        let off = GestureConfig { guard: GuardConfig { constant_count: false, ..g() }, ..cfg() };
        let mut p = Pipeline::new(off, vec![], true);
        assert!(p.judge(0, 0, up, Via::Frame, Some(stats), 0, 0, false).accepted);
    }

    #[test]
    fn speed_and_evenness_are_off_by_default_and_work_when_set() {
        let slow = SwipeStats { peak_active: 3, duration_ms: 2_000, min_travel: 0.05, max_travel: 0.3, mean_travel: 0.15 };
        let up = GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 };
        let mut p = Pipeline::new(cfg(), vec![], true);
        assert!(p.judge(0, 0, up, Via::Frame, Some(slow), 0, 0, false).accepted);
        let strict = GestureConfig { guard: GuardConfig { min_speed: 0.5, ..g() }, ..cfg() };
        let mut p = Pipeline::new(strict, vec![], true);
        assert_eq!(p.judge(0, 0, up, Via::Frame, Some(slow), 0, 0, false).reason, Reason::TooSlow);
        let even = GestureConfig { guard: GuardConfig { evenness_min: 0.5, ..g() }, ..cfg() };
        let mut p = Pipeline::new(even, vec![], true);
        assert_eq!(p.judge(0, 0, up, Via::Frame, Some(slow), 0, 0, false).reason, Reason::UnevenFingers);
    }

    #[test]
    fn palm_on_pad_blocks_only_when_switched_on() {
        let up = GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 };
        let mut p = Pipeline::new(cfg(), vec![], true);
        assert!(p.judge(0, 0, up, Via::Frame, None, 0, 0, true).accepted);
        let on = GestureConfig { guard: GuardConfig { palm_blocks_all: true, ..g() }, ..cfg() };
        let mut p = Pipeline::new(on, vec![], true);
        assert_eq!(p.judge(0, 0, up, Via::Frame, None, 0, 0, true).reason, Reason::PalmOnPad);
    }

    /// The per-frame budget: < 0.2 ms. Run in release:
    /// `cargo test --release -p inspector-rust-core --lib guard_frame_budget -- --ignored --nocapture`
    // Live view (Phase 4)

    #[test]
    fn live_touches_carry_the_pipelines_classification() {
        let mut p = Pipeline::new(cfg(), vec![], true);
        let mut palm = touch(2, 0.8, 0.5);
        palm.size = 3.0;
        let f = frame(100, vec![touch(1, 0.4, 0.5), palm, touch(3, 0.5, 0.01)]);
        p.feed(&f);
        let live = p.live_touches(&f);
        let by = |id| live.iter().find(|t| t.id == id).unwrap().clone();
        assert_eq!(by(1).class, TouchClass::Unclear, "not settled yet");
        assert_eq!(by(2).class, TouchClass::Palm);
        assert!(by(3).in_edge && !by(1).in_edge);
        let f2 = frame(200, vec![touch(1, 0.4, 0.5)]);
        p.feed(&f2);
        assert_eq!(p.live_touches(&f2)[0].class, TouchClass::Finger);
    }

    #[test]
    fn live_touches_skip_contacts_that_are_not_on_the_pad() {
        let mut p = Pipeline::new(cfg(), vec![], true);
        let mut hover = touch(5, 0.5, 0.5);
        hover.phase = TouchPhase::Hover;
        let f = frame(100, vec![touch(1, 0.4, 0.5), hover]);
        p.feed(&f);
        assert_eq!(p.live_touches(&f).len(), 1);
    }

    #[test]
    fn typing_state_follows_the_key_window() {
        let mut p = Pipeline::new(cfg(), vec![], true);
        assert_eq!(p.typing_state(1_000), (false, false));
        p.key(KeyDown { t_ms: 1_000, modifier: false });
        assert!(p.typing_state(1_100).0, "inside the single-key window");
        assert!(!p.typing_state(1_000 + g().typing_single_ms + 1).0, "past it");
        // With the typing guard off nothing is ever reported as blocked.
        let mut off = Pipeline::new(GestureConfig { typing_guard: false, ..cfg() }, vec![], true);
        off.key(KeyDown { t_ms: 1_000, modifier: false });
        assert_eq!(off.typing_state(1_100), (false, false));
    }

    #[test]
    fn set_guard_takes_effect_without_a_restart() {
        let mut p = Pipeline::new(cfg(), vec![], true);
        let up = GestureEvent { kind: GestureKind::SwipeUp, fingers: 3 };
        assert!(p.external(1_000, up).accepted);
        p.set_guard(GuardConfig { cooldown_ms: 1_000, ..g() });
        assert_eq!(p.external(1_500, up).reason, Reason::Cooldown, "the new, longer cooldown applies");
        // Garbage is normalised like a stored config.
        p.set_guard(GuardConfig { cooldown_ms: 99_999, ..g() });
        assert_eq!(p.g.cooldown_ms, 2_000);
    }

    #[test]
    fn set_guard_hands_the_new_thresholds_to_the_recognisers() {
        // The recognisers bake their thresholds in at creation; without
        // rebuilding them a slider for tap/swipe limits would do nothing.
        let mut p = Pipeline::new(cfg(), vec![], true);
        assert_eq!(tap3(&mut p, 1_000)[0].reason, Reason::Accepted, "a 90 ms tap");
        p.set_guard(GuardConfig { tap_hold_max_ms: 50, ..g() });
        assert!(tap3(&mut p, 5_000).is_empty(), "held 90 ms > 50 ms: no tap any more");
    }

    #[test]
    fn set_guard_reclassifies_contacts_with_the_new_thresholds() {
        let mut p = Pipeline::new(cfg(), vec![], true);
        let mut big = touch(1, 0.5, 0.5);
        big.size = 1.5;
        let f = frame(100, vec![big]);
        p.feed(&f);
        p.feed(&frame(200, vec![big]));
        assert_eq!(p.live_touches(&f)[0].class, TouchClass::Finger);
        p.set_guard(GuardConfig { palm_size: 1.2, ..g() });
        let f3 = frame(300, vec![big]);
        p.feed(&f3);
        assert_eq!(p.live_touches(&f3)[0].class, TouchClass::Palm);
    }

    #[test]
    #[ignore = "timing — run by hand in release"]
    fn guard_frame_budget() {
        let mut p = Pipeline::new(cfg(), vec![], true);
        for k in 0..40 {
            p.key(KeyDown { t_ms: k * 100, modifier: false });
        }
        let n = 20_000u64;
        let start = std::time::Instant::now();
        for i in 0..n {
            let y = 0.7 - (i % 30) as f64 * 0.01;
            let ts = vec![touch(1, 0.4, y), touch(2, 0.5, y), touch(3, 0.6, y), Touch { size: 2.6, ..touch(4, 0.1, 0.9) }];
            let ts = if i % 30 == 29 { Vec::new() } else { ts };
            let f = frame(i * 8, ts);
            let _ = p.feed(&f);
            // Worst case: the `gestures` panel is open and snapshots every frame.
            std::hint::black_box(p.live_touches(&f));
            if p.needs_tick() {
                let _ = p.tick(i * 8 + 4);
            }
        }
        let per = start.elapsed().as_secs_f64() * 1000.0 / n as f64;
        eprintln!("guard: {per:.4} ms per frame");
        assert!(per < 0.2, "{per} ms per frame");
    }

    // Config

    #[test]
    fn defaults_reproduce_the_historic_recogniser_thresholds() {
        assert_eq!(g().rec_params(), RecParams::default());
        assert_eq!(g().palm_size, PALM_SIZE);
    }

    #[test]
    fn normalize_tames_garbage_and_keeps_good_values() {
        let bad = GuardConfig {
            palm_size: f32::NAN,
            coherence_min: 7.0,
            edges_builtin: EdgeZones { left: -1.0, right: 0.9, top: f64::NAN, bottom: 0.05 },
            settle_ms: 99_999,
            ..g()
        }
        .normalized();
        assert_eq!(bad.palm_size, PALM_SIZE);
        assert_eq!(bad.coherence_min, 1.0);
        assert_eq!(bad.edges_builtin, EdgeZones { left: 0.0, right: MAX_EDGE, top: 0.0, bottom: 0.05 });
        assert_eq!(bad.settle_ms, 300);
        assert_eq!(g().normalized(), g());
    }

    #[test]
    fn an_old_stored_config_missing_fields_gets_defaults() {
        let partial: GuardConfig = serde_json::from_str(r#"{"cooldown_ms": 300}"#).unwrap();
        assert_eq!(partial.cooldown_ms, 300);
        assert_eq!(partial.typing_single_ms, 250);
        assert!(partial.constant_count);
    }

    /// Every reason a GESTURE can be rejected with has a human label in the
    /// frontend's replay view — a new reason without one would show its raw
    /// code there.
    #[test]
    fn every_gesture_reason_has_a_frontend_label() {
        let ts = include_str!("../../../frontend/src/lib/gesture-trace.ts");
        for r in [
            Reason::Accepted, Reason::Palm, Reason::Thumb, Reason::EdgeZone, Reason::PalmOnPad,
            Reason::FingerCountChanged, Reason::TooSlow, Reason::UnevenFingers, Reason::Cooldown,
            Reason::Typing, Reason::TypingAwaitCenter, Reason::Unmapped,
        ] {
            let code = r.code();
            assert!(ts.contains(&format!("  {code}: ")), "no label for {code} in gesture-trace.ts");
        }
    }

    #[test]
    fn every_reason_has_a_distinct_code() {
        let all = [
            Reason::Accepted, Reason::Palm, Reason::Thumb, Reason::EdgeZone, Reason::PalmOnPad,
            Reason::FingerCountChanged, Reason::TooSlow, Reason::UnevenFingers, Reason::Cooldown,
            Reason::Typing, Reason::TypingAwaitCenter, Reason::Config("x"), Reason::Unmapped,
        ];
        let codes: std::collections::HashSet<String> = all.iter().map(|r| r.code()).collect();
        assert_eq!(codes.len(), all.len());
    }

    #[test]
    fn a_contact_the_driver_rejects_is_a_palm_until_it_lifts() {
        // Windows PTP confidence bit 0, even for a finger-sized contact.
        let g = GuardConfig::default();
        let mut m = TouchMeta { first_ms: 0, class: TouchClass::Unclear, in_edge: false, max_size: 0.0, max_major: 0.0, max_ratio: 0.0, start_y: 0.5, driver_palm: false };
        classify(&mut m, &Touch { palm: true, ..touch(1, 0.5, 0.5) }, 10, &g);
        assert_eq!(m.class, TouchClass::Palm);
        classify(&mut m, &touch(1, 0.5, 0.5), 200, &g);
        assert_eq!(m.class, TouchClass::Palm, "the driver regaining confidence doesn't clear it");
        let mut fresh = TouchMeta { driver_palm: false, ..m };
        fresh.class = TouchClass::Unclear;
        classify(&mut fresh, &touch(2, 0.5, 0.5), 200, &g);
        assert_eq!(fresh.class, TouchClass::Finger, "an untouched flag changes nothing");
    }
}
