//! Live-capture backbone (Stage 2, the "B" of A+B): the platform-agnostic
//! backend abstraction, the explicit capture state machine (§19), a bounded
//! in-memory buffer with an explicit overflow strategy (§17), and the
//! deliver-cursor logic that drives batched streaming + `PausedView` (§9).
//!
//! ALL of this is pure and unit-tested. The platform backend (`macos.rs`) is
//! the thin, hardware-dependent shell that produces packets and pushes them in;
//! it must never contain logic that belongs here.
//!
//! ⚠️ Nothing here assumes macOS — a Windows/Linux backend feeds the same
//! `LiveStore` (§28).

use super::models::{BtDirection, BtPacket, BtProtocol, CaptureState, CaptureStats};
use super::stats;

/// What a CoreBluetooth `CBManagerState` means for us. Raw values are the
/// documented, ABI-stable ordinals (0 unknown, 1 resetting, 2 unsupported,
/// 3 unauthorized, 4 poweredOff, 5 poweredOn) so we don't depend on an enum
/// spelling. Transient states say "wait" (a newer state callback will follow),
/// hard states say "error" with an actionable message (§15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CbReadiness {
    Ready,
    Wait,
    Error(&'static str),
}

pub fn cb_readiness(raw: isize) -> CbReadiness {
    match raw {
        5 => CbReadiness::Ready,   // poweredOn
        0 | 1 => CbReadiness::Wait, // unknown / resetting (transient)
        4 => CbReadiness::Error(
            "Bluetooth is turned off — enable it in Control Center or System Settings, then retry.",
        ),
        3 => CbReadiness::Error(
            "Inspector Rust isn't allowed to use Bluetooth. Grant it under System Settings → Privacy & Security → Bluetooth, then retry.",
        ),
        2 => CbReadiness::Error("This Mac reports no Bluetooth Low Energy support."),
        _ => CbReadiness::Error("Bluetooth is unavailable on this Mac."),
    }
}

/// FNV-1a hash of a device identifier → a stable 12-bit synthetic "handle".
/// CoreBluetooth hides the hardware MAC and real ATT handles, so for
/// advertisement packets we derive a LOCAL per-device id purely to group a
/// device's successive advertisements for the byte-diff (§12). It is NOT a real
/// BT connection handle — the UI/docs say so.
pub fn synthetic_handle(identifier: &str) -> u16 {
    let mut h: u32 = 0x811c_9dc5;
    for b in identifier.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    // fold into the 12-bit handle space, never 0 (0 reads as "none")
    ((h & 0x0fff) as u16).max(1)
}

/// Build a normalised `BtPacket` from one BLE advertisement (§18). Pure so it
/// is unit-tested without CoreBluetooth; the FFI shell only extracts the values.
/// `index`/`t_us`/`length` are assigned later by `LiveStore::push`.
#[allow(clippy::too_many_arguments)]
pub fn advertisement_packet(
    name: Option<&str>,
    identifier: &str,
    rssi: i32,
    service_uuids: &[String],
    manufacturer_data: Vec<u8>,
    connectable: bool,
    abs_ms: u64,
) -> BtPacket {
    let label = name.filter(|n| !n.is_empty()).unwrap_or("(unnamed)");
    let mut summary = format!("{label} · {rssi} dBm");
    if let Some(u) = service_uuids.first() {
        summary.push_str(" · ");
        summary.push_str(u);
        if service_uuids.len() > 1 {
            summary.push_str(&format!(" +{}", service_uuids.len() - 1));
        }
    }

    let mut decoded: Vec<(String, String)> = vec![
        ("Device".into(), label.to_string()),
        ("Identifier".into(), identifier.to_string()),
        ("RSSI".into(), format!("{rssi} dBm")),
        (
            "Connectable".into(),
            if connectable { "yes" } else { "no" }.into(),
        ),
    ];
    if !service_uuids.is_empty() {
        decoded.push(("Service UUIDs".into(), service_uuids.join(", ")));
    }
    if !manufacturer_data.is_empty() {
        decoded.push((
            "Manufacturer data".into(),
            format!("{} bytes", manufacturer_data.len()),
        ));
    }

    BtPacket {
        index: 0,
        t_us: 0,
        abs_ms,
        direction: BtDirection::Rx, // advertisements are controller → host
        protocol: BtProtocol::HciEvt,
        handle: Some(synthetic_handle(identifier)),
        opcode: Some("LE Advertising Report".into()),
        att_handle: None,
        uuid: service_uuids.first().cloned(),
        length: manufacturer_data.len() as u32,
        summary,
        raw: manufacturer_data,
        decoded,
    }
}

/// In-memory packet cap for a live session (§17). A CoreBluetooth GATT session
/// is low-volume (tens–hundreds of ops), so this is a safety ceiling, not a
/// normal operating point. On overflow the OLDEST packets are dropped and
/// counted in `dropped` — never a silent loss.
pub const LIVE_MAX_PACKETS: usize = 200_000;
/// How many oldest packets to shed at once when the cap is hit (amortises the
/// `Vec` shift instead of paying it per packet).
const OVERFLOW_SHED: usize = 4_096;

/// Is `from → to` a legal capture transition (§19)? Everything not listed is
/// rejected, so the backend can never drive the session into a nonsense state.
pub fn can_transition(from: CaptureState, to: CaptureState) -> bool {
    use CaptureState::*;
    matches!(
        (from, to),
        // discovering the backend
        (Unavailable, Idle)
            | (Idle, Unavailable)
            // start flow
            | (Idle, Starting)
            | (Starting, Capturing)
            | (Starting, Error)
            | (Starting, Idle) // start aborted before any packet
            // pause is a VIEW freeze — the backend keeps running
            | (Capturing, PausedView)
            | (PausedView, Capturing)
            // stop flow (from either running state)
            | (Capturing, Stopping)
            | (PausedView, Stopping)
            | (Stopping, Stopped)
            // a stopped/loaded session can be cleared back to idle for a new run
            | (Stopped, Idle)
            | (Stopped, Starting)
            // errors can surface from any active state
            | (Capturing, Error)
            | (PausedView, Error)
            | (Stopping, Error)
            | (Error, Idle)
    )
}

/// A bounded, index-stable packet store shared between the backend (writer) and
/// the emitter/IPC (readers). Packet `index` is monotonic and never reused, so
/// the inspector/diff can address a packet by its stable id even after the
/// oldest rows are shed.
pub struct LiveStore {
    packets: Vec<BtPacket>,
    /// Total packets ever accepted (monotonic; also the next index to assign).
    produced: u32,
    /// Packets shed to honour the cap — surfaced to the UI, never hidden.
    dropped: u64,
    /// Highest `index` already delivered to the frontend.
    delivered: i64,
    cap: usize,
}

impl LiveStore {
    pub fn new(cap: usize) -> Self {
        Self {
            packets: Vec::new(),
            produced: 0,
            dropped: 0,
            delivered: -1,
            cap: cap.max(1),
        }
    }

    /// Accept a decoded packet from the backend. `index`/`t_us` are assigned
    /// here (session-relative) — the backend supplies everything else incl.
    /// `abs_ms`. Enforces the cap by shedding the oldest packets.
    pub fn push(&mut self, mut p: BtPacket) {
        p.index = self.produced;
        self.produced = self.produced.wrapping_add(1);
        // Relative time from the first packet's absolute clock (0 if unknown).
        let base = self.packets.first().map(|f| f.abs_ms).unwrap_or(p.abs_ms);
        p.t_us = if p.abs_ms >= base {
            (p.abs_ms - base) * 1000
        } else {
            0
        };
        p.length = p.raw.len() as u32;
        self.packets.push(p);
        if self.packets.len() > self.cap {
            let shed = OVERFLOW_SHED.min(self.packets.len() - self.cap + OVERFLOW_SHED);
            let shed = shed.min(self.packets.len());
            self.packets.drain(0..shed);
            self.dropped += shed as u64;
        }
    }

    #[cfg_attr(not(test), allow(dead_code))] // exercised by the buffer tests
    pub fn len(&self) -> usize {
        self.packets.len()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }

    pub fn produced(&self) -> u32 {
        self.produced
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn packets(&self) -> &[BtPacket] {
        &self.packets
    }

    /// Count of packets accepted but not yet delivered to the UI — the number
    /// shown as "N packets buffered" while `PausedView` (§9).
    pub fn buffered(&self) -> usize {
        self.packets
            .iter()
            .filter(|p| (p.index as i64) > self.delivered)
            .count()
    }

    /// The undelivered tail (monotonic indices ⇒ a contiguous slice).
    pub fn undelivered(&self) -> &[BtPacket] {
        let start = self
            .packets
            .iter()
            .position(|p| (p.index as i64) > self.delivered)
            .unwrap_or(self.packets.len());
        &self.packets[start..]
    }

    /// Mark everything up to and including `index` as delivered.
    pub fn mark_delivered(&mut self, index: u32) {
        self.delivered = self.delivered.max(index as i64);
    }

    /// Clear the display + buffer for a new run (§9 Clear).
    pub fn clear(&mut self) {
        self.packets.clear();
        self.produced = 0;
        self.dropped = 0;
        self.delivered = -1;
    }

    /// Look a packet up by its STABLE index (not vec position) — robust to
    /// oldest-shedding, unlike a positional lookup.
    pub fn position_of(&self, index: u32) -> Option<usize> {
        self.packets.iter().position(|p| p.index == index)
    }

    pub fn stats(&self) -> CaptureStats {
        stats::compute(&self.packets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth_capture::models::{BtDirection, BtProtocol};

    fn mk(abs_ms: u64, raw: &[u8]) -> BtPacket {
        BtPacket {
            index: 999, // overwritten by push()
            t_us: 999,
            abs_ms,
            direction: BtDirection::Rx,
            protocol: BtProtocol::Att,
            handle: Some(0x40),
            opcode: Some("Handle Value Notification".into()),
            att_handle: Some(0x25),
            uuid: Some("2A19".into()),
            length: 0,
            summary: "n".into(),
            raw: raw.to_vec(),
            decoded: vec![],
        }
    }

    #[test]
    fn legal_and_illegal_transitions() {
        use CaptureState::*;
        assert!(can_transition(Idle, Starting));
        assert!(can_transition(Starting, Capturing));
        assert!(can_transition(Capturing, PausedView));
        assert!(can_transition(PausedView, Capturing));
        assert!(can_transition(Capturing, Stopping));
        assert!(can_transition(PausedView, Stopping));
        assert!(can_transition(Stopping, Stopped));
        assert!(can_transition(Stopped, Idle));
        // illegal
        assert!(!can_transition(Idle, Capturing)); // must go through Starting
        assert!(!can_transition(PausedView, Stopped)); // must go through Stopping
        assert!(!can_transition(Stopped, Capturing));
        assert!(!can_transition(Idle, PausedView));
        assert!(!can_transition(Capturing, Idle));
    }

    #[test]
    fn push_assigns_monotonic_index_and_relative_time() {
        let mut s = LiveStore::new(100);
        s.push(mk(1_000, &[1, 2]));
        s.push(mk(1_500, &[3]));
        assert_eq!(s.packets()[0].index, 0);
        assert_eq!(s.packets()[1].index, 1);
        assert_eq!(s.packets()[0].t_us, 0); // first
        assert_eq!(s.packets()[1].t_us, 500_000); // 500 ms later
        assert_eq!(s.packets()[0].length, 2); // recomputed from raw
        assert_eq!(s.produced(), 2);
    }

    #[test]
    fn buffered_and_delivery_cursor_drive_pause() {
        let mut s = LiveStore::new(100);
        for i in 0..5 {
            s.push(mk(1_000 + i * 10, &[i as u8]));
        }
        assert_eq!(s.buffered(), 5);
        assert_eq!(s.undelivered().len(), 5);
        // deliver the first batch
        let last = s.undelivered().last().unwrap().index;
        s.mark_delivered(last);
        assert_eq!(s.buffered(), 0);
        // more arrive (as if during PausedView)
        s.push(mk(2_000, &[9]));
        s.push(mk(2_010, &[10]));
        assert_eq!(s.buffered(), 2);
        assert_eq!(
            s.undelivered().iter().map(|p| p.index).collect::<Vec<_>>(),
            vec![5, 6]
        );
    }

    #[test]
    fn overflow_sheds_oldest_and_counts_never_silent() {
        let cap = 10;
        let mut s = LiveStore::new(cap);
        for i in 0..(cap + OVERFLOW_SHED + 3) {
            s.push(mk(1_000 + i as u64, &[0]));
        }
        // never grows unbounded
        assert!(s.len() <= cap + OVERFLOW_SHED);
        // the shed count is exact and visible
        assert_eq!(s.dropped(), s.produced() as u64 - s.len() as u64);
        assert!(s.dropped() > 0);
        // index stays stable/monotonic — the newest survives with a high index
        let newest = s.packets().last().unwrap().index;
        assert_eq!(newest, s.produced() - 1);
    }

    #[test]
    fn position_of_survives_shedding() {
        let mut s = LiveStore::new(3);
        for i in 0..6 {
            s.push(mk(1_000 + i, &[0]));
        }
        // oldest indices are gone; a surviving stable index still resolves
        let surviving = s.packets()[0].index;
        assert_eq!(s.position_of(surviving), Some(0));
        assert_eq!(s.position_of(0), None); // shed
    }

    #[test]
    fn cb_readiness_maps_states() {
        assert_eq!(cb_readiness(5), CbReadiness::Ready);
        assert_eq!(cb_readiness(0), CbReadiness::Wait);
        assert_eq!(cb_readiness(1), CbReadiness::Wait);
        assert!(matches!(cb_readiness(4), CbReadiness::Error(m) if m.contains("turned off")));
        assert!(matches!(cb_readiness(3), CbReadiness::Error(m) if m.contains("allowed")));
        assert!(matches!(cb_readiness(2), CbReadiness::Error(_)));
    }

    #[test]
    fn synthetic_handle_is_stable_distinct_and_nonzero() {
        let a = synthetic_handle("11111111-2222-3333-4444-555555555555");
        let b = synthetic_handle("AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE");
        assert_eq!(a, synthetic_handle("11111111-2222-3333-4444-555555555555")); // stable
        assert_ne!(a, b); // distinct devices → distinct handle (overwhelmingly)
        assert_ne!(a, 0);
        assert!(a <= 0x0fff && b <= 0x0fff);
    }

    #[test]
    fn advertisement_packet_maps_fields() {
        let p = advertisement_packet(
            Some("Ninebot ZT3"),
            "DEAD-BEEF",
            -63,
            &["FE95".into(), "180F".into()],
            vec![0x61, 0x64, 0x00],
            true,
            1_700_000_000_000,
        );
        assert_eq!(p.protocol, BtProtocol::HciEvt);
        assert_eq!(p.direction, BtDirection::Rx);
        assert_eq!(p.opcode.as_deref(), Some("LE Advertising Report"));
        assert_eq!(p.uuid.as_deref(), Some("FE95"));
        assert_eq!(p.raw, vec![0x61, 0x64, 0x00]); // mfg data → diffable payload
        assert!(p.summary.contains("Ninebot ZT3"));
        assert!(p.summary.contains("-63 dBm"));
        assert!(p.summary.contains("FE95"));
        assert!(p.summary.contains("+1")); // extra service count
                                           // grouped by the synthetic per-device handle for byte-diff
        assert_eq!(p.handle, Some(synthetic_handle("DEAD-BEEF")));
        assert!(p.decoded.iter().any(|(k, _)| k == "Identifier"));
    }

    #[test]
    fn advertisement_packet_unnamed_and_empty() {
        let p = advertisement_packet(None, "X", -90, &[], vec![], false, 0);
        assert!(p.summary.contains("(unnamed)"));
        assert_eq!(p.uuid, None);
        assert!(p.raw.is_empty());
    }

    #[test]
    fn clear_resets_everything() {
        let mut s = LiveStore::new(100);
        s.push(mk(1_000, &[1]));
        s.mark_delivered(0);
        s.clear();
        assert!(s.is_empty());
        assert_eq!(s.produced(), 0);
        assert_eq!(s.dropped(), 0);
        assert_eq!(s.buffered(), 0);
        // indices restart
        s.push(mk(2_000, &[1]));
        assert_eq!(s.packets()[0].index, 0);
    }
}
