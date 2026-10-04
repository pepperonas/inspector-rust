//! Live view for the `gestures` panel (gesture guard Phase 4): the most recent
//! touch frame with each contact's classification, and the last
//! [`LOG_LEN`] decisions of the pipeline.
//!
//! Cost discipline: the decision log is fed on every decision (rare — a few
//! per gesture), but the per-frame snapshot is only built while the panel is
//! actually polling ([`watched`]); with the panel closed a frame pays one
//! atomic load. Nothing here is persisted and nothing leaves the process.

use super::guard::{Decision, Level, TouchClass, Via};
use super::trace::Touch;
use super::{GestureAction, GestureKind};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// Decisions kept for the panel.
pub const LOG_LEN: usize = 20;
/// Frame snapshots are taken this long after the panel's last poll.
pub const WATCH_MS: u64 = 1_500;

/// One contact as the panel draws it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveTouch {
    pub id: i32,
    pub x: f64,
    pub y: f64,
    pub major: f32,
    pub minor: f32,
    pub angle: f32,
    pub size: f32,
    pub class: TouchClass,
    /// Landed in an edge zone (level 2).
    pub in_edge: bool,
}

/// The latest frame of one device.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveFrame {
    /// [`clock`] time the frame was taken.
    pub at_ms: u64,
    pub device: u32,
    pub touches: Vec<LiveTouch>,
}

/// One line of the decision log.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogEntry {
    pub seq: u64,
    /// [`clock`] time the decision was logged.
    pub at_ms: u64,
    pub device: u32,
    pub level: Level,
    /// [`super::guard::Reason::code`] — the same code the replay view labels.
    pub verdict: String,
    pub accepted: bool,
    pub kind: Option<GestureKind>,
    pub fingers: Option<u8>,
    pub action: Option<GestureAction>,
    pub via: Option<Via>,
    /// Set for a contact decision (palm / thumb / edge zone).
    pub touch_id: Option<i32>,
}

/// A contact's state as the pipeline sees it, merged into a [`LiveTouch`].
pub fn live_touch(t: &Touch, class: TouchClass, in_edge: bool) -> LiveTouch {
    LiveTouch {
        id: t.id,
        x: t.x,
        y: t.y,
        major: t.major,
        minor: t.minor,
        angle: t.angle,
        size: t.size,
        class,
        in_edge,
    }
}

/// The ring of recent decisions plus the latest frame. Pure state; the
/// process-wide instance lives behind [`LIVE`].
#[derive(Debug, Default)]
pub struct Live {
    log: VecDeque<LogEntry>,
    seq: u64,
    frame: Option<LiveFrame>,
}

impl Live {
    /// Log a decision (oldest dropped past [`LOG_LEN`]).
    pub fn push(&mut self, d: &Decision, at_ms: u64) {
        self.seq += 1;
        if self.log.len() == LOG_LEN {
            self.log.pop_front();
        }
        self.log.push_back(LogEntry {
            seq: self.seq,
            at_ms,
            device: d.device,
            level: d.level,
            verdict: d.reason.code(),
            accepted: d.accepted,
            kind: d.event.map(|e| e.kind),
            fingers: d.event.map(|e| e.fingers),
            action: d.action,
            via: d.via,
            touch_id: d.touch_id,
        });
    }

    pub fn set_frame(&mut self, f: LiveFrame) {
        self.frame = Some(f);
    }

    /// Newest first.
    pub fn log(&self) -> Vec<LogEntry> {
        self.log.iter().rev().cloned().collect()
    }

    pub fn frame(&self) -> Option<LiveFrame> {
        self.frame.clone()
    }

    pub fn clear(&mut self) {
        self.log.clear();
        self.frame = None;
    }
}

static LIVE: Mutex<Live> = Mutex::new(Live { log: VecDeque::new(), seq: 0, frame: None });
static CLOCK: OnceLock<Instant> = OnceLock::new();
/// [`clock`] time of the panel's last poll, `0` = never.
static LAST_POLL_MS: AtomicU64 = AtomicU64::new(0);

/// Milliseconds since the first use of the live view — one clock for every
/// platform, so "how long ago" is consistent in the panel.
pub fn clock() -> u64 {
    CLOCK.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

/// Pure: is the panel watching (polled within [`WATCH_MS`])?
pub fn is_watched(last_poll: u64, now: u64) -> bool {
    last_poll != 0 && now.saturating_sub(last_poll) <= WATCH_MS
}

/// Should the platform build a frame snapshot right now?
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn watched() -> bool {
    is_watched(LAST_POLL_MS.load(Ordering::Relaxed), clock())
}

/// Log a decision (every platform, from the common sink).
pub(crate) fn record(d: &Decision) {
    let at = clock();
    LIVE.lock().push(d, at);
}

/// Store the latest frame (only called while [`watched`]).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn set_frame(device: u32, touches: Vec<LiveTouch>) {
    let f = LiveFrame { at_ms: clock(), device, touches };
    LIVE.lock().set_frame(f);
}

/// The panel polled: arm the frame snapshots and hand back the state.
pub(crate) fn poll() -> (u64, Option<LiveFrame>, Vec<LogEntry>) {
    let now = clock();
    LAST_POLL_MS.store(now, Ordering::Relaxed);
    let live = LIVE.lock();
    (now, live.frame(), live.log())
}

/// The decisions of the last `window_ms`, oldest first (misfire reports).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn recent(window_ms: u64) -> Vec<LogEntry> {
    let from = clock().saturating_sub(window_ms);
    LIVE.lock().log.iter().filter(|e| e.at_ms >= from).cloned().collect()
}

/// Forget the log and the frame (panel's "clear" button).
pub(crate) fn clear() {
    LIVE.lock().clear();
}

#[cfg(test)]
mod tests {
    use super::super::guard::Reason;
    use super::super::GestureEvent;
    use super::*;

    fn decision(reason: Reason, accepted: bool) -> Decision {
        Decision {
            t_ms: 10,
            device: 0,
            level: Level::Typing,
            reason,
            accepted,
            event: Some(GestureEvent { kind: GestureKind::Tap, fingers: 3 }),
            via: Some(Via::Tick),
            action: Some(GestureAction::MuteToggle),
            binding: None,
            touch_id: None,
        }
    }

    #[test]
    fn the_log_keeps_only_the_newest_twenty_newest_first() {
        let mut l = Live::default();
        for i in 0..25 {
            l.push(&decision(Reason::Typing, false), i);
        }
        let log = l.log();
        assert_eq!(log.len(), LOG_LEN);
        assert_eq!(log[0].seq, 25, "newest first");
        assert_eq!(log[LOG_LEN - 1].seq, 6, "the five oldest are gone");
    }

    #[test]
    fn a_log_entry_carries_the_reason_code_the_ui_labels() {
        let mut l = Live::default();
        l.push(&decision(Reason::Typing, false), 1);
        l.push(&decision(Reason::Accepted, true), 2);
        let log = l.log();
        assert_eq!(log[0].verdict, "dispatched");
        assert!(log[0].accepted);
        assert_eq!(log[1].verdict, "typing_guard");
        assert_eq!(log[1].kind, Some(GestureKind::Tap));
        assert_eq!(log[1].fingers, Some(3));
    }

    #[test]
    fn a_contact_decision_has_no_gesture() {
        let mut l = Live::default();
        let d = Decision {
            event: None,
            via: None,
            action: None,
            binding: None,
            touch_id: Some(7),
            reason: Reason::Palm,
            level: Level::Classify,
            ..decision(Reason::Palm, false)
        };
        l.push(&d, 1);
        let e = &l.log()[0];
        assert_eq!((e.kind, e.fingers, e.touch_id), (None, None, Some(7)));
        assert_eq!(e.verdict, "palm");
    }

    #[test]
    fn snapshots_only_while_the_panel_polls() {
        assert!(!is_watched(0, 100), "never polled");
        assert!(is_watched(1_000, 1_000 + WATCH_MS), "inside the window");
        assert!(!is_watched(1_000, 1_001 + WATCH_MS), "panel closed");
    }

    #[test]
    fn clear_empties_log_and_frame() {
        let mut l = Live::default();
        l.push(&decision(Reason::Accepted, true), 1);
        l.set_frame(LiveFrame { at_ms: 1, device: 0, touches: Vec::new() });
        l.clear();
        assert!(l.log().is_empty());
        assert!(l.frame().is_none());
    }
}
