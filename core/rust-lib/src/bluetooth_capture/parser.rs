//! Capture-file parsers: PacketLogger `.pklg`, `btsnoop` (Android/BlueZ), and
//! `pcapng` (Wireshark). Each turns an untrusted byte buffer into `Vec<BtPacket>`.
//!
//! ⚠️ SECURITY (§29): capture files are untrusted input. Every length field is
//! bounds-checked, absurd sizes are rejected, a truncated file yields the
//! records read so far, and NOTHING here can panic on malformed bytes — the
//! `garbage_never_panics` test drives random buffers through all three.
//!
//! Direction handling: an HCI Command is ALWAYS host→controller (Tx) and an
//! Event ALWAYS controller→host (Rx), so we derive those from the packet type
//! and only trust the source's direction bit for ACL/SCO — which sidesteps the
//! btsnoop flags-bit ambiguity for the two cases that would be most visible.

use super::decode::{self, HciKind};
use super::models::{BtDirection, BtPacket, BtProtocol};

/// Reject a single record payload larger than this — a real HCI ACL packet is
/// ≤ ~64 KiB; a length field claiming more is corruption, not data.
const MAX_RECORD_LEN: usize = 128 * 1024;
/// Hard cap on packets from one file (defensive; a real session is far less).
const MAX_PACKETS: usize = 2_000_000;

/// btsnoop timestamp epoch: microseconds from 0000-01-01 to the Unix epoch.
/// (Wireshark's constant; verified against real Android `btsnoop_hci.log`.)
const BTSNOOP_EPOCH_DELTA_US: u64 = 0x00dc_ddb3_0f2f_8000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureFormat {
    Pklg,
    Btsnoop,
    Pcapng,
    Unknown,
}

#[derive(Debug)]
pub enum ParseError {
    /// Not one of the supported formats (magic bytes unrecognised).
    UnknownFormat,
    /// Recognised format but the header itself is unusable.
    BadHeader(&'static str),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::UnknownFormat => write!(
                f,
                "unrecognised capture format (expected .pklg, btsnoop or pcapng)"
            ),
            ParseError::BadHeader(w) => write!(f, "malformed capture header: {w}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Sniff the format from the leading bytes. Cheap; never reads past the magic.
pub fn detect_format(data: &[u8]) -> CaptureFormat {
    if data.starts_with(b"btsnoop\0") {
        return CaptureFormat::Btsnoop;
    }
    // pcapng Section Header Block type is 0x0A0D0D0A (byte order independent).
    if data.len() >= 4 && data[0..4] == [0x0a, 0x0d, 0x0d, 0x0a] {
        return CaptureFormat::Pcapng;
    }
    // PacketLogger has no magic. Heuristic: the first record's length field
    // (u32be) is a small, plausible record size and its type byte is known.
    if data.len() >= 13 {
        let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let typ = data[8];
        if (9..=MAX_RECORD_LEN).contains(&len) && is_known_pklg_type(typ) {
            return CaptureFormat::Pklg;
        }
    }
    CaptureFormat::Unknown
}

/// Parse any supported format into normalised packets.
pub fn parse(data: &[u8]) -> Result<Vec<BtPacket>, ParseError> {
    match detect_format(data) {
        CaptureFormat::Pklg => Ok(finalize(parse_pklg(data))),
        CaptureFormat::Btsnoop => Ok(finalize(parse_btsnoop(data)?)),
        CaptureFormat::Pcapng => Ok(finalize(parse_pcapng(data)?)),
        CaptureFormat::Unknown => Err(ParseError::UnknownFormat),
    }
}

// ── intermediate record before timestamps are made relative ──
struct RawRec {
    kind: Option<HciKind>,
    dir: BtDirection,
    /// Monotonic source timestamp (µs) for relative timing; epoch-agnostic.
    raw_us: Option<u64>,
    /// Unix timestamp (µs) for the absolute clock, when derivable.
    unix_us: Option<u64>,
    body: Vec<u8>,
    /// Set only for records we don't decode (system/vendor notes).
    note: Option<String>,
}

fn direction_for(kind: Option<HciKind>, source: BtDirection) -> BtDirection {
    match kind {
        Some(HciKind::Command) => BtDirection::Tx,
        Some(HciKind::Event) => BtDirection::Rx,
        _ => source,
    }
}

fn finalize(recs: Vec<RawRec>) -> Vec<BtPacket> {
    let base = recs.iter().find_map(|r| r.raw_us);
    recs.into_iter()
        .take(MAX_PACKETS)
        .enumerate()
        .map(|(i, r)| {
            let t_us = match (r.raw_us, base) {
                (Some(t), Some(b)) => t.saturating_sub(b),
                _ => 0,
            };
            let abs_ms = r.unix_us.map(|u| u / 1000).unwrap_or(0);
            let length = r.body.len() as u32;
            match r.kind {
                Some(kind) => {
                    let d = decode::decode(kind, r.dir, &r.body);
                    BtPacket {
                        index: i as u32,
                        t_us,
                        abs_ms,
                        direction: r.dir,
                        protocol: d.protocol,
                        handle: d.handle,
                        opcode: d.opcode,
                        att_handle: d.att_handle,
                        uuid: d.uuid,
                        length,
                        summary: d.summary,
                        raw: r.body,
                        decoded: d.fields,
                    }
                }
                None => BtPacket {
                    index: i as u32,
                    t_us,
                    abs_ms,
                    direction: r.dir,
                    protocol: BtProtocol::Other,
                    handle: None,
                    opcode: None,
                    att_handle: None,
                    uuid: None,
                    length,
                    summary: r
                        .note
                        .unwrap_or_else(|| format!("Vendor/system record ({length} bytes)")),
                    raw: r.body,
                    decoded: Vec::new(),
                },
            }
        })
        .collect()
}

// ── PacketLogger .pklg ──

fn is_known_pklg_type(t: u8) -> bool {
    matches!(t, 0x00 | 0x01 | 0x02 | 0x03 | 0x08 | 0x09 | 0xfc | 0xff)
}

fn pklg_kind_dir(t: u8) -> (Option<HciKind>, BtDirection) {
    match t {
        0x00 => (Some(HciKind::Command), BtDirection::Tx),
        0x01 => (Some(HciKind::Event), BtDirection::Rx),
        0x02 => (Some(HciKind::Acl), BtDirection::Tx),
        0x03 => (Some(HciKind::Acl), BtDirection::Rx),
        0x08 => (Some(HciKind::Sco), BtDirection::Tx),
        0x09 => (Some(HciKind::Sco), BtDirection::Rx),
        _ => (None, BtDirection::Unknown), // 0xfc note / 0xff / vendor
    }
}

fn parse_pklg(data: &[u8]) -> Vec<RawRec> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 4 <= data.len() && out.len() < MAX_PACKETS {
        let len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        // `len` counts the 8-byte timestamp + 1 type byte + payload.
        if !(9..=MAX_RECORD_LEN).contains(&len) {
            break; // corrupt / not actually pklg — stop, keep what we have
        }
        let rec_start = pos + 4;
        let rec_end = match rec_start.checked_add(len) {
            Some(e) if e <= data.len() => e,
            _ => break, // truncated final record
        };
        let rec = &data[rec_start..rec_end];
        let secs = u32::from_be_bytes([rec[0], rec[1], rec[2], rec[3]]) as u64;
        let usecs = u32::from_be_bytes([rec[4], rec[5], rec[6], rec[7]]) as u64;
        let typ = rec[8];
        let body = rec[9..].to_vec();
        let (kind, dir) = pklg_kind_dir(typ);
        let abs = secs * 1_000_000 + usecs.min(999_999);
        out.push(RawRec {
            kind,
            dir,
            raw_us: Some(abs),
            unix_us: Some(abs),
            body,
            note: (kind.is_none()).then(|| format!("PacketLogger record (type 0x{typ:02X})")),
        });
        pos = rec_end;
    }
    out
}

// ── btsnoop (Android / BlueZ hcidump) ──

fn parse_btsnoop(data: &[u8]) -> Result<Vec<RawRec>, ParseError> {
    // 8 magic + u32 version + u32 datalink = 16-byte header.
    if data.len() < 16 {
        return Err(ParseError::BadHeader("file shorter than btsnoop header"));
    }
    let datalink = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
    let mut out = Vec::new();
    let mut pos = 16usize;
    while pos + 24 <= data.len() && out.len() < MAX_PACKETS {
        let incl = u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
            as usize;
        let flags =
            u32::from_be_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]);
        let ts = i64::from_be_bytes([
            data[pos + 16],
            data[pos + 17],
            data[pos + 18],
            data[pos + 19],
            data[pos + 20],
            data[pos + 21],
            data[pos + 22],
            data[pos + 23],
        ]);
        if incl > MAX_RECORD_LEN {
            break;
        }
        let data_start = pos + 24;
        let data_end = match data_start.checked_add(incl) {
            Some(e) if e <= data.len() => e,
            _ => break,
        };
        let packet = &data[data_start..data_end];
        // flags bit0: direction (0 = sent/Tx, 1 = received/Rx). Only trusted for
        // ACL/SCO — cmd/evt direction is derived in `direction_for`.
        let src_dir = if flags & 0x01 != 0 {
            BtDirection::Rx
        } else {
            BtDirection::Tx
        };
        let (kind, dir, body) = decode_btsnoop_packet(datalink, flags, src_dir, packet);
        let (raw_us, unix_us) = btsnoop_times(ts);
        out.push(RawRec {
            kind,
            dir,
            raw_us,
            unix_us,
            body,
            note: (kind.is_none()).then(|| "Unsupported btsnoop datalink".to_string()),
        });
        pos = data_end;
    }
    Ok(out)
}

fn btsnoop_times(ts: i64) -> (Option<u64>, Option<u64>) {
    if ts <= 0 {
        return (None, None);
    }
    let raw = ts as u64;
    let unix = raw.checked_sub(BTSNOOP_EPOCH_DELTA_US);
    (Some(raw), unix)
}

fn decode_btsnoop_packet(
    datalink: u32,
    flags: u32,
    src_dir: BtDirection,
    packet: &[u8],
) -> (Option<HciKind>, BtDirection, Vec<u8>) {
    match datalink {
        // 1002 = HCI UART (H4): first byte is the H4 packet-type indicator.
        1002 => match packet.split_first() {
            Some((&h4, body)) => {
                let kind = h4_kind(h4);
                (kind, direction_for(kind, src_dir), body.to_vec())
            }
            None => (None, src_dir, Vec::new()),
        },
        // 1001 = unencapsulated HCI (H1): no prefix; type from flags bit1.
        1001 => {
            let is_cmd_evt = flags & 0x02 != 0;
            let kind = if is_cmd_evt {
                match src_dir {
                    BtDirection::Tx => Some(HciKind::Command),
                    _ => Some(HciKind::Event),
                }
            } else {
                Some(HciKind::Acl)
            };
            (kind, direction_for(kind, src_dir), packet.to_vec())
        }
        // 1003/1004 (BCSP/H5) and anything else: framing differs — keep the
        // bytes as an undecoded record rather than mis-parsing them.
        _ => (None, BtDirection::Unknown, packet.to_vec()),
    }
}

fn h4_kind(h4: u8) -> Option<HciKind> {
    match h4 {
        0x01 => Some(HciKind::Command),
        0x02 => Some(HciKind::Acl),
        0x03 => Some(HciKind::Sco),
        0x04 => Some(HciKind::Event),
        _ => None,
    }
}

// ── pcapng (Wireshark) ──

/// Cursor with the byte order fixed by the Section Header Block.
struct Pcapng<'a> {
    data: &'a [u8],
    le: bool,
}

impl<'a> Pcapng<'a> {
    fn u16(&self, o: usize) -> Option<u16> {
        let b = self.data.get(o..o + 2)?;
        Some(if self.le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    }
    fn u32(&self, o: usize) -> Option<u32> {
        let b = self.data.get(o..o + 4)?;
        Some(if self.le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        })
    }
}

fn parse_pcapng(data: &[u8]) -> Result<Vec<RawRec>, ParseError> {
    if data.len() < 12 {
        return Err(ParseError::BadHeader("file shorter than pcapng SHB"));
    }
    // The SHB byte-order magic at offset 8 tells us LE vs BE.
    let magic = &data[8..12];
    let le = match magic {
        [0x4d, 0x3c, 0x2b, 0x1a] => true,
        [0x1a, 0x2b, 0x3c, 0x4d] => false,
        _ => return Err(ParseError::BadHeader("bad pcapng byte-order magic")),
    };
    let pc = Pcapng { data, le };

    // Per-interface link type + timestamp resolution, in IDB order.
    let mut if_link: Vec<u16> = Vec::new();
    let mut if_tsresol: Vec<u8> = Vec::new();
    let mut out = Vec::new();

    let mut pos = 0usize;
    while pos + 8 <= data.len() && out.len() < MAX_PACKETS {
        let block_type = match pc.u32(pos) {
            Some(v) => v,
            None => break,
        };
        let total_len = match pc.u32(pos + 4) {
            Some(v) => v as usize,
            None => break,
        };
        // total_len includes the type + both length fields; must be ≥12 and
        // 32-bit aligned, and must fit.
        if total_len < 12 || total_len % 4 != 0 {
            break;
        }
        let end = match pos.checked_add(total_len) {
            Some(e) if e <= data.len() => e,
            _ => break,
        };
        let body = &data[pos + 8..end - 4]; // block body (between the length fields)

        match block_type {
            0x0000_0001 => {
                // Interface Description Block: u16 linktype, u16 reserved, u32 snaplen, options
                if let Some(lt) = pc.u16(pos + 8) {
                    if_link.push(lt);
                    // default tsresol = 6 (microseconds); parse the option if present.
                    let resol = pcapng_tsresol(&pc, pos + 8 + 8, end - 4).unwrap_or(6);
                    if_tsresol.push(resol);
                }
            }
            0x0000_0006 => {
                // Enhanced Packet Block: if_id, ts_high, ts_low, caplen, origlen, data
                if body.len() >= 20 {
                    let if_id = pc.u32(pos + 8).unwrap_or(0) as usize;
                    let ts_hi = pc.u32(pos + 12).unwrap_or(0) as u64;
                    let ts_lo = pc.u32(pos + 16).unwrap_or(0) as u64;
                    let caplen = pc.u32(pos + 20).unwrap_or(0) as usize;
                    let pkt_start = pos + 28;
                    let pkt_end = pkt_start.saturating_add(caplen);
                    if caplen <= MAX_RECORD_LEN && pkt_end <= end - 4 {
                        let packet = &data[pkt_start..pkt_end];
                        let lt = if_link.get(if_id).copied().unwrap_or(0);
                        let resol = if_tsresol.get(if_id).copied().unwrap_or(6);
                        let unix_us = pcapng_unix_us((ts_hi << 32) | ts_lo, resol);
                        push_pcapng_packet(&mut out, lt, packet, unix_us);
                    }
                }
            }
            // Simple Packet Block: origlen, data (uses interface 0, no timestamp)
            0x0000_0003 if body.len() >= 4 => {
                let pkt_start = pos + 12;
                let avail = (end - 4).saturating_sub(pkt_start);
                let take = avail.min(MAX_RECORD_LEN);
                let packet = &data[pkt_start..pkt_start + take];
                let lt = if_link.first().copied().unwrap_or(0);
                push_pcapng_packet(&mut out, lt, packet, None);
            }
            _ => {} // SHB (already handled), name-resolution, stats, custom → skip
        }
        pos = end;
    }
    Ok(out)
}

/// Read the `if_tsresol` option (code 9) from an IDB's options area.
fn pcapng_tsresol(pc: &Pcapng, mut o: usize, end: usize) -> Option<u8> {
    while o + 4 <= end {
        let code = pc.u16(o)?;
        let len = pc.u16(o + 2)? as usize;
        if code == 0 {
            break; // opt_endofopt
        }
        if code == 9 && len >= 1 {
            return pc.data.get(o + 4).copied();
        }
        // options are padded to 32 bits
        let advance = 4 + ((len + 3) & !3);
        o = o.checked_add(advance)?;
    }
    None
}

/// Convert a pcapng timestamp (in units of `resol`) to Unix microseconds.
fn pcapng_unix_us(ticks: u64, resol: u8) -> Option<u64> {
    if resol & 0x80 != 0 {
        // binary: 2^-(resol & 0x7f) seconds per tick
        let shift = (resol & 0x7f) as i32;
        let secs_per_tick = 2f64.powi(-shift);
        Some((ticks as f64 * secs_per_tick * 1e6) as u64)
    } else {
        // decimal: 10^-resol seconds per tick
        match resol as i32 - 6 {
            0 => Some(ticks),
            d if d > 0 => Some(ticks / 10u64.pow(d as u32)),
            d => ticks.checked_mul(10u64.pow((-d) as u32)),
        }
    }
}

fn push_pcapng_packet(out: &mut Vec<RawRec>, linktype: u16, packet: &[u8], unix_us: Option<u64>) {
    // LINKTYPE_BLUETOOTH_HCI_H4 = 187, ..._WITH_PHDR = 201.
    let (kind, dir, body) = match linktype {
        187 => match packet.split_first() {
            Some((&h4, body)) => {
                let k = h4_kind(h4);
                (k, direction_for(k, BtDirection::Unknown), body.to_vec())
            }
            None => (None, BtDirection::Unknown, Vec::new()),
        },
        201 => {
            // 4-byte big-endian direction pseudo-header (0=sent, 1=recv), then H4.
            if packet.len() >= 5 {
                let sent = u32::from_be_bytes([packet[0], packet[1], packet[2], packet[3]]) == 0;
                let src = if sent {
                    BtDirection::Tx
                } else {
                    BtDirection::Rx
                };
                let h4 = packet[4];
                let k = h4_kind(h4);
                (k, direction_for(k, src), packet[5..].to_vec())
            } else {
                (None, BtDirection::Unknown, packet.to_vec())
            }
        }
        _ => (None, BtDirection::Unknown, packet.to_vec()),
    };
    out.push(RawRec {
        kind,
        dir,
        raw_us: unix_us,
        unix_us,
        body,
        note: (kind.is_none()).then(|| format!("pcapng linktype {linktype}")),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth_capture::decode::ATT_WRITE_COMMAND;

    // ── builders ──

    fn pklg_rec(typ: u8, secs: u32, usecs: u32, body: &[u8]) -> Vec<u8> {
        let len = (9 + body.len()) as u32;
        let mut v = Vec::new();
        v.extend_from_slice(&len.to_be_bytes());
        v.extend_from_slice(&secs.to_be_bytes());
        v.extend_from_slice(&usecs.to_be_bytes());
        v.push(typ);
        v.extend_from_slice(body);
        v
    }

    fn btsnoop_header(datalink: u32) -> Vec<u8> {
        let mut v = b"btsnoop\0".to_vec();
        v.extend_from_slice(&1u32.to_be_bytes());
        v.extend_from_slice(&datalink.to_be_bytes());
        v
    }

    fn btsnoop_rec(flags: u32, ts: i64, packet: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        let n = packet.len() as u32;
        v.extend_from_slice(&n.to_be_bytes()); // orig len
        v.extend_from_slice(&n.to_be_bytes()); // incl len
        v.extend_from_slice(&flags.to_be_bytes());
        v.extend_from_slice(&0u32.to_be_bytes()); // drops
        v.extend_from_slice(&ts.to_be_bytes());
        v.extend_from_slice(packet);
        v
    }

    // ATT Write Command over ACL (handle 0x0040, att handle 0x0025).
    fn acl_att_write() -> Vec<u8> {
        vec![
            0x40, 0x00, 0x08, 0x00, 0x04, 0x00, 0x04, 0x00, 0x52, 0x25, 0x00, 0x61, 0x64, 0x00,
        ]
    }

    // ── detection ──

    #[test]
    fn detects_each_format() {
        let pklg = pklg_rec(0x00, 1, 0, &[0x0c, 0x20, 0x02, 0x01, 0x00]);
        assert_eq!(detect_format(&pklg), CaptureFormat::Pklg);
        assert_eq!(detect_format(&btsnoop_header(1002)), CaptureFormat::Btsnoop);
        assert_eq!(
            detect_format(&[0x0a, 0x0d, 0x0d, 0x0a, 0, 0, 0, 0]),
            CaptureFormat::Pcapng
        );
        assert_eq!(
            detect_format(b"\x00\x01\x02\x03random"),
            CaptureFormat::Unknown
        );
    }

    // ── pklg ──

    #[test]
    fn pklg_command_and_acl_write() {
        let mut buf = pklg_rec(0x00, 10, 500_000, &[0x0c, 0x20, 0x02, 0x01, 0x00]);
        buf.extend(pklg_rec(0x02, 11, 0, &acl_att_write()));
        let pkts = parse(&buf).unwrap();
        assert_eq!(pkts.len(), 2);

        assert_eq!(pkts[0].protocol, BtProtocol::HciCmd);
        assert_eq!(pkts[0].direction, BtDirection::Tx);
        assert_eq!(pkts[0].opcode.as_deref(), Some("LE Set Scan Enable"));
        assert_eq!(pkts[0].t_us, 0); // first packet
        assert_eq!(pkts[0].abs_ms, 10_500);

        assert_eq!(pkts[1].protocol, BtProtocol::Att);
        assert_eq!(pkts[1].direction, BtDirection::Tx); // pklg type 0x02 = sent
        assert_eq!(pkts[1].att_handle, Some(0x0025));
        assert_eq!(pkts[1].opcode.as_deref(), Some(ATT_WRITE_COMMAND));
        // 11.0s - 10.5s = 0.5s relative
        assert_eq!(pkts[1].t_us, 500_000);
    }

    #[test]
    fn pklg_note_record_is_kept_as_other() {
        let buf = pklg_rec(0xfc, 1, 0, b"system started");
        let pkts = parse(&buf).unwrap();
        assert_eq!(pkts.len(), 1);
        assert_eq!(pkts[0].protocol, BtProtocol::Other);
        assert!(pkts[0].summary.contains("type 0xFC"));
    }

    #[test]
    fn pklg_truncated_final_record_keeps_the_rest() {
        let mut buf = pklg_rec(0x01, 1, 0, &[0x0e, 0x00]); // one good event
                                                           // a length claiming more bytes than remain
        buf.extend_from_slice(&999u32.to_be_bytes());
        buf.extend_from_slice(&[0u8; 4]);
        let pkts = parse(&buf).unwrap();
        assert_eq!(pkts.len(), 1);
    }

    // ── btsnoop ──

    #[test]
    fn btsnoop_h4_derives_command_and_event_direction() {
        let mut buf = btsnoop_header(1002);
        // A COMMAND record whose flag bit0 lies (=1, "received"); we must still
        // report Tx because a command is always host→controller.
        let cmd = [0x01u8, 0x0c, 0x20, 0x02, 0x01, 0x00]; // H4 0x01 + LE Set Scan Enable
        buf.extend(btsnoop_rec(
            0x03,
            BTSNOOP_EPOCH_DELTA_US as i64 + 1_000_000,
            &cmd,
        ));
        // An EVENT record with flag bit0=0 ("sent"); must still be Rx.
        let evt = [0x04u8, 0x0e, 0x04, 0x01, 0x0c, 0x20, 0x00]; // H4 0x04 + Command Complete
        buf.extend(btsnoop_rec(
            0x02,
            BTSNOOP_EPOCH_DELTA_US as i64 + 2_000_000,
            &evt,
        ));

        let pkts = parse(&buf).unwrap();
        assert_eq!(pkts.len(), 2);
        assert_eq!(pkts[0].protocol, BtProtocol::HciCmd);
        assert_eq!(pkts[0].direction, BtDirection::Tx);
        assert_eq!(pkts[0].abs_ms, 1_000); // epoch delta subtracted
        assert_eq!(pkts[1].protocol, BtProtocol::HciEvt);
        assert_eq!(pkts[1].direction, BtDirection::Rx);
        assert_eq!(pkts[1].t_us, 1_000_000);
    }

    #[test]
    fn btsnoop_h4_acl_direction_from_flag() {
        let mut buf = btsnoop_header(1002);
        let mut acl = vec![0x02u8]; // H4 ACL
        acl.extend(acl_att_write());
        buf.extend(btsnoop_rec(0x01, BTSNOOP_EPOCH_DELTA_US as i64 + 5, &acl)); // bit0=1 → Rx
        let pkts = parse(&buf).unwrap();
        assert_eq!(pkts[0].protocol, BtProtocol::Att);
        assert_eq!(pkts[0].direction, BtDirection::Rx);
    }

    #[test]
    fn btsnoop_h1_unencapsulated_uses_flags() {
        let mut buf = btsnoop_header(1001);
        // data (bit1=0), sent (bit0=0) → ACL Tx
        buf.extend(btsnoop_rec(0x00, 1, &acl_att_write()));
        // cmd/evt (bit1=1), received (bit0=1) → Event
        buf.extend(btsnoop_rec(0x03, 2, &[0x0e, 0x04, 0x01, 0x0c, 0x20, 0x00]));
        let pkts = parse(&buf).unwrap();
        assert_eq!(pkts[0].protocol, BtProtocol::Att);
        assert_eq!(pkts[0].direction, BtDirection::Tx);
        assert_eq!(pkts[1].protocol, BtProtocol::HciEvt);
        assert_eq!(pkts[1].direction, BtDirection::Rx);
    }

    #[test]
    fn btsnoop_short_header_errors_cleanly() {
        assert!(
            matches!(parse(b"btsnoop\0short"), Err(ParseError::UnknownFormat))
                || matches!(parse(b"btsnoop\0short"), Err(ParseError::BadHeader(_)))
        );
    }

    // ── pcapng ──

    fn pcapng_le_h4() -> Vec<u8> {
        let mut v = Vec::new();
        // SHB
        let mut shb_body = Vec::new();
        shb_body.extend_from_slice(&0x1a2b3c4du32.to_le_bytes()); // BOM
        shb_body.extend_from_slice(&1u16.to_le_bytes()); // major
        shb_body.extend_from_slice(&0u16.to_le_bytes()); // minor
        shb_body.extend_from_slice(&(-1i64).to_le_bytes()); // section length unknown
        let shb_total = 12 + shb_body.len() as u32;
        v.extend_from_slice(&0x0a0d0d0au32.to_le_bytes());
        v.extend_from_slice(&shb_total.to_le_bytes());
        v.extend_from_slice(&shb_body);
        v.extend_from_slice(&shb_total.to_le_bytes());
        // IDB linktype 187 (H4)
        let mut idb_body = Vec::new();
        idb_body.extend_from_slice(&187u16.to_le_bytes());
        idb_body.extend_from_slice(&0u16.to_le_bytes()); // reserved
        idb_body.extend_from_slice(&0u32.to_le_bytes()); // snaplen
        let idb_total = 12 + idb_body.len() as u32;
        v.extend_from_slice(&1u32.to_le_bytes());
        v.extend_from_slice(&idb_total.to_le_bytes());
        v.extend_from_slice(&idb_body);
        v.extend_from_slice(&idb_total.to_le_bytes());
        // EPB with an H4 command packet
        let packet = [0x01u8, 0x0c, 0x20, 0x02, 0x01, 0x00];
        let mut epb_body = Vec::new();
        epb_body.extend_from_slice(&0u32.to_le_bytes()); // interface id
        epb_body.extend_from_slice(&0u32.to_le_bytes()); // ts high
        epb_body.extend_from_slice(&3_000_000u32.to_le_bytes()); // ts low (µs)
        epb_body.extend_from_slice(&(packet.len() as u32).to_le_bytes()); // caplen
        epb_body.extend_from_slice(&(packet.len() as u32).to_le_bytes()); // origlen
        epb_body.extend_from_slice(&packet);
        while epb_body.len() % 4 != 0 {
            epb_body.push(0);
        }
        let epb_total = 12 + epb_body.len() as u32;
        v.extend_from_slice(&6u32.to_le_bytes());
        v.extend_from_slice(&epb_total.to_le_bytes());
        v.extend_from_slice(&epb_body);
        v.extend_from_slice(&epb_total.to_le_bytes());
        v
    }

    #[test]
    fn pcapng_h4_enhanced_packet_block() {
        let buf = pcapng_le_h4();
        assert_eq!(detect_format(&buf), CaptureFormat::Pcapng);
        let pkts = parse(&buf).unwrap();
        assert_eq!(pkts.len(), 1);
        assert_eq!(pkts[0].protocol, BtProtocol::HciCmd);
        assert_eq!(pkts[0].direction, BtDirection::Tx);
        assert_eq!(pkts[0].abs_ms, 3_000);
    }

    #[test]
    fn pcapng_tsresol_converts() {
        // nanoseconds (resol 9) → µs divide by 1000
        assert_eq!(pcapng_unix_us(3_000_000_000, 9), Some(3_000_000));
        // milliseconds (resol 3) → µs multiply by 1000
        assert_eq!(pcapng_unix_us(3_000, 3), Some(3_000_000));
        // default µs
        assert_eq!(pcapng_unix_us(3_000_000, 6), Some(3_000_000));
    }

    // ── fuzz / safety ──

    #[test]
    fn garbage_never_panics() {
        // Prefixes that route to each parser, plus pure noise.
        let prefixes: &[&[u8]] = &[
            b"btsnoop\0\x00\x00\x00\x01\x00\x00\x03\xea",
            &[0x0a, 0x0d, 0x0d, 0x0a],
            &[0x00, 0x00, 0x00, 0x0c, 0, 0, 0, 0, 0x00],
        ];
        for pre in prefixes {
            for seed in 0u32..300 {
                let mut buf = pre.to_vec();
                let mut x = seed.wrapping_mul(2654435761);
                for _ in 0..(seed % 200) {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    buf.push((x & 0xff) as u8);
                }
                let _ = parse(&buf); // must not panic
            }
        }
    }

    #[test]
    fn empty_and_tiny_inputs() {
        assert!(parse(&[]).is_err());
        let _ = parse(&[0x0a, 0x0d, 0x0d, 0x0a]); // pcapng magic, nothing else
        let _ = parse(b"btsnoop\0"); // magic only
    }
}
