//! Live-capture session state shared between the platform backend (writer),
//! the batched emitter (reader) and the IPC layer. Send+Sync so it can live in
//! Tauri managed state; the backend pushes packets through `LiveStore`.
//!
//! The state machine itself is in `capture.rs` (pure). This module holds the
//! concurrent session container + the small pure helpers the emitter needs.

use std::sync::Mutex;

use serde::Serialize;

use super::capture::{self, LiveStore, LIVE_MAX_PACKETS};
use super::models::{BtPacketSlim, CaptureState, CaptureStats};

/// Whether the platform even has a live backend, decided before any start (§16).
/// (`UnsupportedPlatform` is only constructed on non-macOS builds.)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum BackendAvailability {
    /// A live backend exists on this OS.
    Available,
    /// This OS has no live backend yet (§28 — clean message, no crash).
    UnsupportedPlatform,
}

/// Wall-clock milliseconds since the Unix epoch (session clock + packet stamps).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The non-macOS setup report (§28): the command works but reports cleanly that
/// live capture is macOS-only — no crash. (Called only on non-macOS builds.)
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub fn unsupported_setup() -> SetupStatus {
    SetupStatus {
        availability: BackendAvailability::UnsupportedPlatform,
        message:
            "Live Bluetooth capture is currently available on macOS only. \
                  On this platform you can still import and analyse .pklg / btsnoop / pcapng files."
                .into(),
        source_label: "—".into(),
    }
}

/// Setup/availability report for the idle panel (§16).
#[derive(Debug, Clone, Serialize)]
pub struct SetupStatus {
    pub availability: BackendAvailability,
    /// Human, actionable one-liner (never a generic "something went wrong").
    pub message: String,
    /// Short backend label for the source dropdown ("System Bluetooth (BLE)").
    pub source_label: String,
}

/// Live session metadata (everything except the packet buffer).
pub struct LiveMeta {
    pub state: CaptureState,
    pub error: Option<String>,
    /// Wall-clock ms when the current capture started (for the duration clock).
    pub started_at_ms: u64,
    /// Set true while `PausedView` — the backend keeps writing, the emitter holds.
    pub view_paused: bool,
}

impl Default for LiveMeta {
    fn default() -> Self {
        Self {
            state: CaptureState::Idle,
            error: None,
            started_at_ms: 0,
            view_paused: false,
        }
    }
}

/// The concurrent live session. `store` is written by the backend and read by
/// the emitter/IPC; `meta` carries the state machine + clock.
pub struct LiveShared {
    pub store: Mutex<LiveStore>,
    pub meta: Mutex<LiveMeta>,
}

impl Default for LiveShared {
    fn default() -> Self {
        Self {
            store: Mutex::new(LiveStore::new(LIVE_MAX_PACKETS)),
            meta: Mutex::new(LiveMeta::default()),
        }
    }
}

/// A state read for the frontend (`btsniff_live_status` + the `btsniff-state`
/// event payload).
#[derive(Debug, Clone, Serialize)]
pub struct LiveStatus {
    pub state: CaptureState,
    pub error: Option<String>,
    pub started_at_ms: u64,
    pub view_paused: bool,
    pub packets: u32,
    pub buffered: u32,
    pub dropped: u64,
    pub stats: CaptureStats,
}

impl LiveShared {
    /// Attempt a state transition, rejecting illegal ones (§19). Returns the
    /// resulting state (unchanged on rejection).
    pub fn transition(&self, to: CaptureState) -> Result<CaptureState, String> {
        let mut meta = self.meta.lock().expect("live meta poisoned");
        if meta.state == to {
            return Ok(to);
        }
        if !capture::can_transition(meta.state, to) {
            return Err(format!(
                "illegal capture transition {:?} → {:?}",
                meta.state, to
            ));
        }
        meta.state = to;
        if to != CaptureState::Error {
            meta.error = None;
        }
        Ok(to)
    }

    pub fn set_error(&self, message: impl Into<String>) {
        let mut meta = self.meta.lock().expect("live meta poisoned");
        meta.state = CaptureState::Error;
        meta.error = Some(message.into());
    }

    pub fn state(&self) -> CaptureState {
        self.meta.lock().expect("live meta poisoned").state
    }

    pub fn status(&self) -> LiveStatus {
        let meta = self.meta.lock().expect("live meta poisoned");
        let store = self.store.lock().expect("live store poisoned");
        LiveStatus {
            state: meta.state,
            error: meta.error.clone(),
            started_at_ms: meta.started_at_ms,
            view_paused: meta.view_paused,
            packets: store.produced(),
            buffered: store.buffered() as u32,
            dropped: store.dropped(),
            stats: store.stats(),
        }
    }

    /// Drain the undelivered packets for the emitter, UNLESS the view is paused
    /// (then hold them so the buffered count grows — §9). Returns the slim rows
    /// to emit and marks them delivered.
    pub fn drain_for_emit(&self) -> Vec<BtPacketSlim> {
        let meta_paused = self.meta.lock().expect("live meta poisoned").view_paused;
        if meta_paused {
            return Vec::new();
        }
        let mut store = self.store.lock().expect("live store poisoned");
        let batch: Vec<BtPacketSlim> = store.undelivered().iter().map(|p| p.slim()).collect();
        if let Some(last) = store.undelivered().last().map(|p| p.index) {
            store.mark_delivered(last);
        }
        batch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adv(id: &str, abs_ms: u64) -> super::super::models::BtPacket {
        capture::advertisement_packet(Some("dev"), id, -50, &[], vec![1], true, abs_ms)
    }

    #[test]
    fn transitions_go_through_the_state_machine() {
        let s = LiveShared::default();
        assert_eq!(s.state(), CaptureState::Idle);
        assert!(s.transition(CaptureState::Starting).is_ok());
        assert!(s.transition(CaptureState::Capturing).is_ok());
        // illegal jump is rejected and leaves the state intact
        assert!(s.transition(CaptureState::Stopped).is_err());
        assert_eq!(s.state(), CaptureState::Capturing);
        assert!(s.transition(CaptureState::PausedView).is_ok());
        assert!(s.transition(CaptureState::Capturing).is_ok());
        assert!(s.transition(CaptureState::Stopping).is_ok());
        assert!(s.transition(CaptureState::Stopped).is_ok());
    }

    #[test]
    fn set_error_forces_error_state_with_message() {
        let s = LiveShared::default();
        s.transition(CaptureState::Starting).unwrap();
        s.set_error("Bluetooth is powered off");
        assert_eq!(s.state(), CaptureState::Error);
        assert_eq!(
            s.status().error.as_deref(),
            Some("Bluetooth is powered off")
        );
    }

    #[test]
    fn drain_holds_while_paused_and_flushes_on_resume() {
        let s = LiveShared::default();
        s.transition(CaptureState::Starting).unwrap();
        s.transition(CaptureState::Capturing).unwrap();
        {
            let mut store = s.store.lock().unwrap();
            store.push(adv("a", 1000));
            store.push(adv("b", 1010));
        }
        // capturing → emitter drains
        assert_eq!(s.drain_for_emit().len(), 2);
        assert_eq!(s.drain_for_emit().len(), 0); // nothing new

        // pause the view; backend keeps writing
        s.transition(CaptureState::PausedView).unwrap();
        {
            s.meta.lock().unwrap().view_paused = true;
        }
        {
            let mut store = s.store.lock().unwrap();
            store.push(adv("c", 1020));
            store.push(adv("d", 1030));
        }
        // emitter holds while paused
        assert_eq!(s.drain_for_emit().len(), 0);
        assert_eq!(s.status().buffered, 2);

        // resume → the held packets flush at once
        {
            s.meta.lock().unwrap().view_paused = false;
        }
        s.transition(CaptureState::Capturing).unwrap();
        assert_eq!(s.drain_for_emit().len(), 2);
    }

    #[test]
    fn status_reflects_counts() {
        let s = LiveShared::default();
        s.transition(CaptureState::Starting).unwrap();
        s.transition(CaptureState::Capturing).unwrap();
        {
            let mut store = s.store.lock().unwrap();
            store.push(adv("a", 1000));
        }
        let st = s.status();
        assert_eq!(st.packets, 1);
        assert_eq!(st.state, CaptureState::Capturing);
    }
}
