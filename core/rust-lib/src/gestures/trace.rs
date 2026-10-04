//! Platform-neutral touch model, trace format and deterministic replay.
//!
//! Phase 1 of the gesture-guard programme (see `docs/gestures.md`). Three
//! jobs, all pure — no OS calls, so everything here is unit-testable:
//!
//! * [`Touch`] / [`Frame`] / [`Trace`]: what a platform layer reports, in one
//!   shape for every OS. A trace is the JSON form of a recording — a fixture
//!   for replay tests and the artefact of `gestures record`. It holds touch
//!   data and the TIMES of key-downs, never which key: a trace must not be a
//!   keylogger.
//! * [`contact_feeds`]: the per-frame filtering the macOS frame callback used
//!   to do inline (which states count as "on the pad", which contacts the
//!   tip-tap recogniser sees). Moved here so the live path and the replay run
//!   the SAME code — a replay that re-implemented it would test a copy.
//! * [`replay`]: drives the unchanged recognisers + the dispatch decisions
//!   (config gate, typing guard) through a trace exactly the way the live
//!   macOS path does, including the 24 ms settle ticker, and reports every
//!   outcome. Golden fixtures pin today's behaviour with it before any
//!   recogniser is touched.

use super::guard::{Decision, Level, Pipeline, Reason, Via};
use super::{GestureAction, GestureConfig, GestureEvent, GestureKind};
use serde::{Deserialize, Serialize};

/// Trace format version. Bump when a field changes meaning; readers reject a
/// newer version instead of misreading it.
pub const TRACE_VERSION: u32 = 1;

/// Cadence of the platform settle ticker the replay emulates (ms). Mirrors
/// `TICK_MS` in `macos.rs`.
pub const REPLAY_TICK_MS: u64 = 24;

/// Upper bound on a recording, so a forgotten recorder can't grow without end.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub const MAX_RECORD_SECS: u64 = 120;

/// Lifecycle of one contact, normalised over the platforms. macOS reports
/// these as numbered states (1/2 in range, 3 make, 4 touching, 5 break, 6/7
/// leaving); Windows and Linux map onto the same set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchPhase {
    /// Near the surface, not touching.
    Hover,
    /// Landing.
    Make,
    /// Down.
    Touching,
    /// Lifting.
    Break,
    /// Gone or going.
    Leaving,
}

impl TouchPhase {
    /// Physical contact with the pad. MAKE and BREAK count: a lightly resting
    /// finger flickers between them and TOUCHING frame to frame, and field
    /// logs showed a light 3-finger tap arriving as fingers taking turns in
    /// TOUCHING — filtering to TOUCHING alone serialised the tap (v0.85.6).
    pub fn on_pad(self) -> bool {
        matches!(self, TouchPhase::Make | TouchPhase::Touching | TouchPhase::Break)
    }

    /// macOS MultitouchSupport state number → phase.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn from_mt_state(state: i32) -> TouchPhase {
        match state {
            1 | 2 => TouchPhase::Hover,
            3 => TouchPhase::Make,
            4 => TouchPhase::Touching,
            5 => TouchPhase::Break,
            _ => TouchPhase::Leaving,
        }
    }
}

/// One contact in one frame. Positions are normalised 0..1 with y growing
/// DOWNWARDS (screen convention, so "up" is decreasing y, as everywhere in
/// the recognisers). Fields a platform can't report stay 0.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Touch {
    /// Driver-stable contact id (unique per device while the contact lives).
    pub id: i32,
    pub x: f64,
    pub y: f64,
    /// Velocity in normalised units per second as the driver reports it.
    #[serde(default)]
    pub vx: f64,
    #[serde(default)]
    pub vy: f64,
    /// Ellipse axes in the driver's units (macOS: millimetre-ish).
    #[serde(default)]
    pub major: f32,
    #[serde(default)]
    pub minor: f32,
    /// Ellipse angle in radians.
    #[serde(default)]
    pub angle: f32,
    /// Contact size / area as the driver reports it. The palm guard keys on
    /// this today (`PALM_SIZE`).
    #[serde(default)]
    pub size: f32,
    pub phase: TouchPhase,
    /// The driver itself says this contact is not a finger — the Windows
    /// Precision Touchpad's confidence bit was 0. Level 1 treats it as a palm
    /// until it lifts. macOS has no such flag (always `false`); omitted from
    /// the JSON when `false`, so older traces and fixtures read unchanged.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub palm: bool,
}

/// Everything a device reported at one instant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// Milliseconds since the start of the trace.
    pub t_ms: u64,
    /// Index into [`Trace::devices`].
    #[serde(default)]
    pub device: u32,
    pub touches: Vec<Touch>,
}

/// A key-down. ONLY the time and whether it was a pure modifier — never the
/// key itself.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct KeyDown {
    pub t_ms: u64,
    #[serde(default)]
    pub modifier: bool,
}

/// A touch surface the trace saw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// `None` = the platform couldn't say.
    #[serde(default)]
    pub builtin: Option<bool>,
    /// Physical surface size in millimetres, when the platform reports it.
    #[serde(default)]
    pub width_mm: Option<f64>,
    #[serde(default)]
    pub height_mm: Option<f64>,
}

/// One expected dispatcher outcome, as stored in a fixture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpectedOutcome {
    pub kind: GestureKind,
    pub fingers: u8,
    /// `dispatched`, `typing_guard`, `config:<reason>` or `unmapped`.
    pub verdict: String,
    /// The action for a dispatched or vetoed gesture.
    #[serde(default)]
    pub action: Option<GestureAction>,
}

/// A recording or a fixture.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trace {
    pub version: u32,
    /// `macos-multitouch`, `synthetic`, …
    pub source: String,
    /// Free text: what the scenario is, what today's code does with it.
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub devices: Vec<DeviceInfo>,
    /// Output was muted when the trace began (the typing guard never vetoes
    /// an UNMUTE, so this changes the verdict of a mute tap).
    #[serde(default)]
    pub muted_at_start: bool,
    pub frames: Vec<Frame>,
    #[serde(default)]
    pub keys: Vec<KeyDown>,
    /// What replay must produce. `None` for a raw recording.
    #[serde(default)]
    pub expect: Option<Vec<ExpectedOutcome>>,
}

impl Trace {
    /// Parse and version-check a trace.
    pub fn from_json(json: &str) -> Result<Trace, String> {
        let t: Trace = serde_json::from_str(json).map_err(|e| format!("trace: {e}"))?;
        if t.version > TRACE_VERSION {
            return Err(format!(
                "trace: version {} is newer than this build understands ({TRACE_VERSION})",
                t.version
            ));
        }
        Ok(t)
    }
}

// ── Replay ───────────────────────────────────────────────────────────────────

/// One recognised gesture and what the guard decided about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub t_ms: u64,
    pub via: Via,
    pub event: GestureEvent,
    pub action: Option<GestureAction>,
    pub level: Level,
    pub reason: Reason,
}

/// One replay outcome in a shape the UI can render.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReplayRow {
    pub t_ms: u64,
    pub via: Via,
    pub level: Level,
    #[serde(flatten)]
    pub outcome: ExpectedOutcome,
}

impl Outcome {
    fn from_decision(d: &Decision) -> Option<Outcome> {
        Some(Outcome {
            t_ms: d.t_ms,
            via: d.via?,
            event: d.event?,
            action: d.action,
            level: d.level,
            reason: d.reason,
        })
    }

    pub fn row(&self) -> ReplayRow {
        ReplayRow { t_ms: self.t_ms, via: self.via, level: self.level, outcome: self.expected() }
    }

    pub fn expected(&self) -> ExpectedOutcome {
        ExpectedOutcome {
            kind: self.event.kind,
            fingers: self.event.fingers,
            verdict: self.reason.code(),
            action: self.action,
        }
    }
}

/// Every decision the guard pipeline makes over a trace, the way the live
/// macOS path makes them: key-downs are revealed only up to the current time,
/// frames are fed in order, and a deferred tap is finalised by a ticker that
/// wakes [`REPLAY_TICK_MS`] after a frame left work and keeps ticking at that
/// cadence while work remains. Deterministic.
#[cfg(test)]
pub fn replay_decisions(trace: &Trace, cfg: &GestureConfig) -> Vec<Decision> {
    // A trace carries the complete key history (synthetic, or a future
    // keyboard-tap recording) — unlike today's live macOS source.
    let p = Pipeline::new(*cfg, trace.devices.clone(), true).with_mute(trace.muted_at_start, None);
    run_replay(trace, p)
}

/// [`replay`] against a binding list (the user's own setup) instead of the
/// historic switches.
pub fn replay_with_bindings(trace: &Trace, cfg: &GestureConfig, list: Vec<super::bindings::GestureBinding>) -> Vec<Outcome> {
    let p = Pipeline::new(*cfg, trace.devices.clone(), true)
        .with_mute(trace.muted_at_start, None)
        .with_bindings(list);
    run_replay(trace, p).iter().filter_map(Outcome::from_decision).collect()
}

fn run_replay(trace: &Trace, mut p: Pipeline) -> Vec<Decision> {
    let mut keys = trace.keys.clone();
    keys.sort_by_key(|k| k.t_ms);
    let mut ki = 0;
    let mut out = Vec::new();
    let mut next_tick: Option<u64> = None;

    let reveal = |p: &mut Pipeline, ki: &mut usize, upto: u64| {
        while *ki < keys.len() && keys[*ki].t_ms <= upto {
            p.key(keys[*ki]);
            *ki += 1;
        }
    };

    for frame in &trace.frames {
        while let Some(t) = next_tick.filter(|&t| t < frame.t_ms) {
            reveal(&mut p, &mut ki, t);
            out.extend(p.tick(t));
            next_tick = p.needs_tick().then_some(t + REPLAY_TICK_MS);
        }
        reveal(&mut p, &mut ki, frame.t_ms);
        out.extend(p.feed(frame));
        if p.needs_tick() && next_tick.is_none() {
            next_tick = Some(frame.t_ms + REPLAY_TICK_MS);
        }
    }
    // Let a pending tap settle after the last frame (bounded).
    let end = trace.frames.last().map(|f| f.t_ms).unwrap_or(0) + 5_000;
    while let Some(t) = next_tick.filter(|&t| t <= end) {
        reveal(&mut p, &mut ki, t);
        out.extend(p.tick(t));
        next_tick = p.needs_tick().then_some(t + REPLAY_TICK_MS);
    }
    out
}

/// The gesture decisions of a replay (contact decisions dropped).
#[cfg(test)]
pub fn replay(trace: &Trace, cfg: &GestureConfig) -> Vec<Outcome> {
    replay_decisions(trace, cfg).iter().filter_map(Outcome::from_decision).collect()
}

// ── Recorder buffer ──────────────────────────────────────────────────────────

/// Accumulates a live recording. The platform pushes frames (and derived
/// key-down times); [`Recorder::finish`] turns it into a [`Trace`] with times
/// relative to the first frame. Pure — the platform owns the clock.
#[derive(Debug)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct Recorder {
    source: String,
    devices: Vec<DeviceInfo>,
    started_ms: u64,
    until_ms: u64,
    frames: Vec<Frame>,
    keys: Vec<KeyDown>,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl Recorder {
    /// Start recording at `now_ms` for `secs` seconds (clamped to
    /// 1..=[`MAX_RECORD_SECS`]).
    pub fn new(source: &str, devices: Vec<DeviceInfo>, now_ms: u64, secs: u64) -> Recorder {
        let secs = secs.clamp(1, MAX_RECORD_SECS);
        Recorder {
            source: source.into(),
            devices,
            started_ms: now_ms,
            until_ms: now_ms + secs * 1000,
            frames: Vec::new(),
            keys: Vec::new(),
        }
    }

    pub fn is_done(&self, now_ms: u64) -> bool {
        now_ms >= self.until_ms
    }

    /// Absolute time the recording ends.
    pub fn until_ms(&self) -> u64 {
        self.until_ms
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Record a frame (absolute time). Ignored outside the window.
    pub fn push_frame(&mut self, now_ms: u64, device: u32, touches: Vec<Touch>) {
        if now_ms < self.started_ms || now_ms >= self.until_ms {
            return;
        }
        self.frames.push(Frame { t_ms: now_ms - self.started_ms, device, touches });
    }

    /// Record that the most recent key-down happened at absolute time
    /// `key_ms`. The platform can usually only see "the last key-down", so it
    /// reports it repeatedly; the same instant (±5 ms of rounding) is kept once.
    pub fn note_key(&mut self, key_ms: u64) {
        if key_ms < self.started_ms || key_ms >= self.until_ms {
            return;
        }
        let t = key_ms - self.started_ms;
        if self.keys.last().is_some_and(|k| t.abs_diff(k.t_ms) <= 5) {
            return;
        }
        self.keys.push(KeyDown { t_ms: t, modifier: false });
    }

    /// Record a key-down reported directly by a keyboard tap (exact time,
    /// shortcut flag) — the complete-history counterpart of [`Recorder::note_key`].
    pub fn push_key(&mut self, key_ms: u64, modifier: bool) {
        if key_ms < self.started_ms || key_ms >= self.until_ms {
            return;
        }
        self.keys.push(KeyDown { t_ms: key_ms - self.started_ms, modifier });
    }

    pub fn finish(self) -> Trace {
        let mut keys = self.keys;
        keys.sort_by_key(|k| k.t_ms);
        Trace {
            version: TRACE_VERSION,
            source: self.source,
            note: String::new(),
            devices: self.devices,
            muted_at_start: false,
            frames: self.frames,
            keys,
            expect: None,
        }
    }
}

// ── Rolling window for "that was unintended" ────────────────────────────────

/// How far back the "unintended" hotkey reaches.
pub const RECENT_WINDOW_MS: u64 = 3_000;

/// The last [`RECENT_WINDOW_MS`] of frames and key-down times, kept in memory
/// only so the "that was unintended" hotkey can save what just happened.
/// Nothing here is written anywhere until the hotkey is pressed. Pure — the
/// platform owns the clock.
#[derive(Debug)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct RecentFrames {
    frames: std::collections::VecDeque<Frame>,
    keys: std::collections::VecDeque<KeyDown>,
}

impl Default for RecentFrames {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
impl RecentFrames {
    pub const fn new() -> RecentFrames {
        RecentFrames { frames: std::collections::VecDeque::new(), keys: std::collections::VecDeque::new() }
    }

    /// Frame at absolute time `now_ms`; anything older than the window goes.
    pub fn push_frame(&mut self, now_ms: u64, device: u32, touches: Vec<Touch>) {
        self.frames.push_back(Frame { t_ms: now_ms, device, touches });
        self.trim(now_ms);
    }

    /// Key-down at absolute time `key_ms` (same ±5 ms dedupe as the recorder:
    /// the derived path reports the same key repeatedly).
    pub fn push_key(&mut self, key_ms: u64, modifier: bool) {
        if self.keys.back().is_some_and(|k| key_ms.abs_diff(k.t_ms) <= 5) {
            return;
        }
        self.keys.push_back(KeyDown { t_ms: key_ms, modifier });
    }

    fn trim(&mut self, now_ms: u64) {
        let from = now_ms.saturating_sub(RECENT_WINDOW_MS);
        while self.frames.front().is_some_and(|f| f.t_ms < from) {
            self.frames.pop_front();
        }
        while self.keys.front().is_some_and(|k| k.t_ms < from) {
            self.keys.pop_front();
        }
    }

    pub fn clear(&mut self) {
        self.frames.clear();
        self.keys.clear();
    }

    /// Frames held — the buffer must stay bounded however long the pad is used.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.frames.len()
    }

    /// The window ending at `now_ms` as a trace (times relative to its first
    /// frame), marked as a misfire: `expect` is empty — nothing should have
    /// fired. `None` when the pad was untouched for the whole window.
    pub fn snapshot(&self, now_ms: u64, source: &str, devices: Vec<DeviceInfo>, note: String) -> Option<Trace> {
        let from = now_ms.saturating_sub(RECENT_WINDOW_MS);
        let frames: Vec<&Frame> = self.frames.iter().filter(|f| f.t_ms >= from && f.t_ms <= now_ms).collect();
        let start = frames.first()?.t_ms;
        Some(Trace {
            version: TRACE_VERSION,
            source: source.into(),
            note,
            devices,
            muted_at_start: false,
            frames: frames.iter().map(|f| Frame { t_ms: f.t_ms - start, ..(*f).clone() }).collect(),
            keys: self
                .keys
                .iter()
                .filter(|k| k.t_ms >= start && k.t_ms <= now_ms)
                .map(|k| KeyDown { t_ms: k.t_ms - start, modifier: k.modifier })
                .collect(),
            expect: Some(Vec::new()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(id: i32, x: f64, y: f64, size: f32, phase: TouchPhase) -> Touch {
        Touch { id, x, y, vx: 0.0, vy: 0.0, major: 0.0, minor: 0.0, angle: 0.0, size, phase, palm: false }
    }

    #[test]
    fn mt_states_map_onto_phases_and_only_contact_counts() {
        assert_eq!(TouchPhase::from_mt_state(2), TouchPhase::Hover);
        assert_eq!(TouchPhase::from_mt_state(3), TouchPhase::Make);
        assert_eq!(TouchPhase::from_mt_state(4), TouchPhase::Touching);
        assert_eq!(TouchPhase::from_mt_state(5), TouchPhase::Break);
        assert_eq!(TouchPhase::from_mt_state(7), TouchPhase::Leaving);
        assert_eq!(TouchPhase::from_mt_state(0), TouchPhase::Leaving);
        let on: Vec<bool> = (1..=7).map(|s| TouchPhase::from_mt_state(s).on_pad()).collect();
        assert_eq!(on, [false, false, true, true, true, false, false]);
    }

    #[test]
    fn feeds_drop_hover_and_keep_excluded_contacts_only_for_the_palm_recogniser() {
        use super::super::guard::contact_feeds;
        let ts = [
            touch(1, 0.2, 0.5, 1.0, TouchPhase::Touching),
            touch(2, 0.4, 0.5, 1.0, TouchPhase::Hover),
            touch(3, 0.6, 0.5, 2.5, TouchPhase::Make),
            touch(4, 0.8, 0.5, 1.0, TouchPhase::Break),
        ];
        let (palm, tiptap) = contact_feeds(&ts, |t| t.size >= 2.0);
        assert_eq!(palm.iter().map(|c| c.id).collect::<Vec<_>>(), [1, 3, 4]);
        assert_eq!(palm.iter().map(|c| c.excluded).collect::<Vec<_>>(), [false, true, false]);
        assert_eq!(tiptap.len(), 2, "hover and the excluded contact are not tip-tap contacts");
    }

    #[test]
    fn tiptap_feed_is_capped_so_a_crowd_still_reads_as_too_many() {
        use super::super::guard::{contact_feeds, TIPTAP_FEED_MAX};
        let ts: Vec<Touch> =
            (0..5).map(|i| touch(i, 0.1 + i as f64 * 0.1, 0.5, 1.0, TouchPhase::Touching)).collect();
        let (palm, tiptap) = contact_feeds(&ts, |_| false);
        assert_eq!(palm.len(), 5);
        assert_eq!(tiptap.len(), TIPTAP_FEED_MAX);
    }

    #[test]
    fn json_round_trip_and_version_gate() {
        let t = Trace {
            version: TRACE_VERSION,
            source: "synthetic".into(),
            note: String::new(),
            devices: vec![DeviceInfo { builtin: Some(true), width_mm: None, height_mm: None }],
            muted_at_start: false,
            frames: vec![Frame { t_ms: 0, device: 0, touches: vec![touch(1, 0.5, 0.5, 1.0, TouchPhase::Touching)] }],
            keys: vec![KeyDown { t_ms: 3, modifier: false }],
            expect: None,
        };
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(Trace::from_json(&json).unwrap(), t);
        let newer = json.replacen(&format!("\"version\":{TRACE_VERSION}"), "\"version\":999", 1);
        assert!(Trace::from_json(&newer).unwrap_err().contains("newer"));
    }

    #[test]
    fn recorder_keeps_the_window_rebases_time_and_dedups_keys() {
        let mut r = Recorder::new("test", vec![], 1_000, 2);
        r.push_frame(999, 0, vec![]); // before
        r.push_frame(1_010, 0, vec![]);
        r.push_frame(3_000, 0, vec![]); // after (window is [1000, 3000))
        r.note_key(1_200);
        r.note_key(1_203); // same instant, re-reported
        r.note_key(1_500);
        assert!(r.is_done(3_000));
        let t = r.finish();
        assert_eq!(t.frames.iter().map(|f| f.t_ms).collect::<Vec<_>>(), [10]);
        assert_eq!(t.keys.iter().map(|k| k.t_ms).collect::<Vec<_>>(), [200, 500]);
        assert!(t.expect.is_none());
    }

    #[test]
    fn recorder_duration_is_clamped() {
        assert_eq!(Recorder::new("t", vec![], 0, 0).until_ms(), 1_000);
        assert_eq!(Recorder::new("t", vec![], 0, 9_999).until_ms(), MAX_RECORD_SECS * 1000);
    }
    #[test]
    fn the_recent_window_keeps_only_the_last_three_seconds() {
        let mut r = RecentFrames::new();
        for t in (0..=6_000).step_by(500) {
            r.push_frame(t, 0, vec![touch(1, 0.5, 0.5, 1.0, TouchPhase::Touching)]);
        }
        r.push_key(1_000, false); // before the window: trimmed with the next frame
        r.push_frame(6_100, 0, vec![]);
        r.push_key(5_000, true);
        r.push_key(5_003, true); // same instant reported twice
        assert_eq!(r.len(), 7, "older frames leave memory, not just the snapshot");
        let t = r.snapshot(6_100, "test", vec![], "n".into()).expect("frames in the window");
        assert_eq!(t.frames.first().map(|f| f.t_ms), Some(0), "relative to the first kept frame");
        // 3_500 .. 6_100 → 3500, 4000, …, 6000, 6100
        assert_eq!(t.frames.len(), 7);
        assert_eq!(t.frames.last().map(|f| f.t_ms), Some(2_600));
        assert_eq!(t.keys, vec![KeyDown { t_ms: 1_500, modifier: true }]);
        assert_eq!(t.expect, Some(vec![]), "a misfire expects nothing to fire");
        assert_eq!(t.note, "n");
    }

    #[test]
    fn an_untouched_window_has_no_snapshot() {
        let mut r = RecentFrames::new();
        r.push_frame(1_000, 0, vec![]);
        assert!(r.snapshot(9_000, "t", vec![], String::new()).is_none(), "the only frame is long gone");
        r.clear();
        assert!(r.snapshot(0, "t", vec![], String::new()).is_none());
    }

}
