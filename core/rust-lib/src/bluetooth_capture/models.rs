//! Shared data model for the Bluetooth capture analyzer (`btsniff`).
//!
//! One normalised packet type feeds every surface — the file analyzer
//! (`.pklg` / btsnoop / pcapng import, see `parser.rs`) and, later, the
//! CoreBluetooth live-GATT backend (`macos.rs`). Parsing + decoding live in
//! Rust (deterministic, exhaustively testable); the frontend receives
//! structured, already-classified packets.
//!
//! ⚠️ Nothing here is macOS-specific — a Windows/Linux backend must be able to
//! produce the same `BtPacket`s (the prompt's cross-platform requirement).

#![allow(dead_code)] // the live backend + some helpers are behind cfg / staged

use serde::{Deserialize, Serialize};

/// Transport direction. `tx` = host → controller (shown `→`), `rx` =
/// controller → host (shown `←`). `unknown` when the source can't say — never
/// guessed (btsnoop H1, malformed records).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BtDirection {
    Tx,
    Rx,
    Unknown,
}

/// Wire protocol / layer the packet was classified as. `gatt` is NOT a wire
/// protocol — GATT operations ride on `Att`; the UI's GATT filter matches
/// `Att` packets whose opcode is a recognised GATT op. `l2cap_sig` = the
/// L2CAP signaling channel (CID 0x0001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BtProtocol {
    HciCmd,
    HciEvt,
    Acl,
    Sco,
    L2cap,
    L2capSig,
    Att,
    Smp,
    /// A recognised record whose inner layer we don't decode (or a
    /// PacketLogger note/vendor record). Fields stay empty rather than invented.
    Other,
    #[default]
    Unknown,
}

impl BtProtocol {
    /// Short uppercase tag for the timeline TYPE column.
    pub fn tag(self) -> &'static str {
        match self {
            BtProtocol::HciCmd => "CMD",
            BtProtocol::HciEvt => "EVT",
            BtProtocol::Acl => "ACL",
            BtProtocol::Sco => "SCO",
            BtProtocol::L2cap => "L2CAP",
            BtProtocol::L2capSig => "L2CAP",
            BtProtocol::Att => "ATT",
            BtProtocol::Smp => "SMP",
            BtProtocol::Other => "—",
            BtProtocol::Unknown => "?",
        }
    }
}

/// One fully-parsed packet. The internal record; the timeline gets a `slim`
/// projection (no bytes), the inspector a `detail` (bytes + decoded + diff) —
/// mirroring the app's `db::list_slim` / `db::get` split so a 100k-packet
/// session doesn't ship every payload on every list refresh.
#[derive(Debug, Clone)]
pub struct BtPacket {
    /// 0-based position in the session (stable — the diff/detail IPC keys off it).
    pub index: u32,
    /// Relative microseconds from the first packet (epoch cancels — robust).
    pub t_us: u64,
    /// Absolute unix milliseconds, `0` = unknown (never a wrong guess).
    pub abs_ms: u64,
    pub direction: BtDirection,
    pub protocol: BtProtocol,
    /// HCI connection handle (12 bits), when the layer carries one.
    pub handle: Option<u16>,
    /// Human opcode / operation, e.g. `LE Set Scan Enable`, `Write Command`.
    pub opcode: Option<String>,
    /// ATT attribute handle, when this is an ATT PDU that carries one.
    pub att_handle: Option<u16>,
    /// Resolved 16-/128-bit UUID string, when present in the PDU.
    pub uuid: Option<String>,
    /// Payload byte length (the record payload we hold in `raw`).
    pub length: u32,
    /// One-line description for the timeline INFO column.
    pub summary: String,
    /// Full record payload bytes (HCI packet incl. its type-specific header).
    pub raw: Vec<u8>,
    /// Decoded field list `(label, value)` for the inspector. Only fields we
    /// actually parsed — unknown payloads add none (no speculation).
    pub decoded: Vec<(String, String)>,
}

impl BtPacket {
    pub fn slim(&self) -> BtPacketSlim {
        BtPacketSlim {
            index: self.index,
            t_us: self.t_us,
            abs_ms: self.abs_ms,
            direction: self.direction,
            protocol: self.protocol,
            handle: self.handle,
            opcode: self.opcode.clone(),
            att_handle: self.att_handle,
            uuid: self.uuid.clone(),
            length: self.length,
            summary: self.summary.clone(),
        }
    }
}

/// Timeline row — everything except the raw bytes + decoded fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BtPacketSlim {
    pub index: u32,
    pub t_us: u64,
    pub abs_ms: u64,
    pub direction: BtDirection,
    pub protocol: BtProtocol,
    pub handle: Option<u16>,
    pub opcode: Option<String>,
    pub att_handle: Option<u16>,
    pub uuid: Option<String>,
    pub length: u32,
    pub summary: String,
}

/// Inspector detail — the heavy payload, fetched one packet at a time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BtPacketDetail {
    pub index: u32,
    pub direction: BtDirection,
    pub protocol: BtProtocol,
    pub handle: Option<u16>,
    pub opcode: Option<String>,
    pub att_handle: Option<u16>,
    pub uuid: Option<String>,
    pub t_us: u64,
    pub abs_ms: u64,
    /// Raw payload bytes (rendered as a hex dump in the UI).
    pub raw: Vec<u8>,
    pub decoded: Vec<(String, String)>,
    /// Previous packet index sharing this ATT handle (or connection handle,
    /// when there's no ATT handle) — the "Diff with previous" target.
    pub prev_index: Option<u32>,
    /// Per-byte change mask vs `prev_index`'s payload (`true` = byte differs).
    /// Length = `raw.len()`; `None` when there's no comparable previous packet.
    pub diff: Option<Vec<bool>>,
}

/// The lifecycle state of a capture, mirrored to the frontend so the UI can
/// only ever offer valid transitions. `PausedView` freezes the UI ONLY — the
/// backend keeps capturing (spec §9/§19).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureState {
    Unavailable,
    #[default]
    Idle,
    Starting,
    Capturing,
    PausedView,
    Stopping,
    Stopped,
    Error,
}

/// Deterministic session summary (spec §14) — computed from the packets, never
/// stored incrementally so it can't drift.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaptureStats {
    pub packets: u32,
    pub tx_packets: u32,
    pub rx_packets: u32,
    pub bytes: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub att_writes: u32,
    pub att_reads: u32,
    pub notifications: u32,
    pub indications: u32,
    pub duration_us: u64,
    pub unique_handles: u32,
    pub unique_att_handles: u32,
    pub unique_uuids: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_and_protocol_serialize_snake_case() {
        assert_eq!(serde_json::to_string(&BtDirection::Tx).unwrap(), "\"tx\"");
        assert_eq!(
            serde_json::to_string(&BtProtocol::HciCmd).unwrap(),
            "\"hci_cmd\""
        );
        assert_eq!(
            serde_json::to_string(&BtProtocol::L2capSig).unwrap(),
            "\"l2cap_sig\""
        );
        assert_eq!(
            serde_json::to_string(&CaptureState::PausedView).unwrap(),
            "\"paused_view\""
        );
    }

    #[test]
    fn protocol_tags_are_stable() {
        assert_eq!(BtProtocol::Att.tag(), "ATT");
        assert_eq!(BtProtocol::HciCmd.tag(), "CMD");
        assert_eq!(BtProtocol::L2capSig.tag(), "L2CAP");
        assert_eq!(BtProtocol::Unknown.tag(), "?");
    }

    #[test]
    fn slim_drops_the_heavy_fields_but_keeps_metadata() {
        let p = BtPacket {
            index: 3,
            t_us: 1234,
            abs_ms: 0,
            direction: BtDirection::Rx,
            protocol: BtProtocol::Att,
            handle: Some(0x0040),
            opcode: Some("Handle Value Notification".into()),
            att_handle: Some(0x0025),
            uuid: Some("2A19".into()),
            length: 4,
            summary: "Notification handle 0x0025".into(),
            raw: vec![0x1b, 0x25, 0x00, 0x61],
            decoded: vec![("ATT Opcode".into(), "0x1b".into())],
        };
        let s = p.slim();
        assert_eq!(s.index, 3);
        assert_eq!(s.att_handle, Some(0x0025));
        // The slim JSON must not contain the raw bytes.
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("\"raw\""));
        assert!(!json.contains("\"decoded\""));
    }
}
