//! Deterministic session summary (§14). Computed from the packet vector on
//! demand — never accumulated incrementally, so it can't drift from the data.

use std::collections::HashSet;

use super::decode::{att_class, AttClass};
use super::models::{BtDirection, BtPacket, BtProtocol, CaptureStats};

pub fn compute(packets: &[BtPacket]) -> CaptureStats {
    let mut s = CaptureStats::default();
    let mut handles: HashSet<u16> = HashSet::new();
    let mut att_handles: HashSet<u16> = HashSet::new();
    let mut uuids: HashSet<String> = HashSet::new();

    let mut min_t: Option<u64> = None;
    let mut max_t: Option<u64> = None;

    for p in packets {
        s.packets += 1;
        s.bytes += p.length as u64;
        match p.direction {
            BtDirection::Tx => {
                s.tx_packets += 1;
                s.tx_bytes += p.length as u64;
            }
            BtDirection::Rx => {
                s.rx_packets += 1;
                s.rx_bytes += p.length as u64;
            }
            BtDirection::Unknown => {}
        }
        if let Some(h) = p.handle {
            handles.insert(h);
        }
        if let Some(h) = p.att_handle {
            att_handles.insert(h);
        }
        if let Some(u) = &p.uuid {
            uuids.insert(u.clone());
        }
        if p.protocol == BtProtocol::Att {
            if let Some(op) = &p.opcode {
                match att_class(op) {
                    AttClass::Write => s.att_writes += 1,
                    AttClass::Read => s.att_reads += 1,
                    AttClass::Notify => s.notifications += 1,
                    AttClass::Indicate => s.indications += 1,
                    AttClass::Other => {}
                }
            }
        }
        // Only count timestamps we actually have (t_us is 0 for the first packet
        // and for records with no clock; min/max over the real span).
        min_t = Some(min_t.map_or(p.t_us, |m| m.min(p.t_us)));
        max_t = Some(max_t.map_or(p.t_us, |m| m.max(p.t_us)));
    }

    s.duration_us = match (min_t, max_t) {
        (Some(a), Some(b)) => b.saturating_sub(a),
        _ => 0,
    };
    s.unique_handles = handles.len() as u32;
    s.unique_att_handles = att_handles.len() as u32;
    s.unique_uuids = uuids.len() as u32;
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth_capture::decode::{ATT_NOTIFICATION, ATT_READ_REQUEST, ATT_WRITE_COMMAND};

    #[allow(clippy::too_many_arguments)]
    fn pkt(
        index: u32,
        t_us: u64,
        dir: BtDirection,
        proto: BtProtocol,
        handle: Option<u16>,
        att: Option<u16>,
        uuid: Option<&str>,
        opcode: Option<&str>,
        length: u32,
    ) -> BtPacket {
        BtPacket {
            index,
            t_us,
            abs_ms: 0,
            direction: dir,
            protocol: proto,
            handle,
            opcode: opcode.map(String::from),
            att_handle: att,
            uuid: uuid.map(String::from),
            length,
            summary: String::new(),
            raw: vec![0u8; length as usize],
            decoded: Vec::new(),
        }
    }

    #[test]
    fn empty_session_is_all_zero() {
        let s = compute(&[]);
        assert_eq!(s.packets, 0);
        assert_eq!(s.duration_us, 0);
        assert_eq!(s.unique_handles, 0);
    }

    #[test]
    fn counts_direction_bytes_and_att_ops() {
        let pkts = vec![
            pkt(
                0,
                0,
                BtDirection::Tx,
                BtProtocol::HciCmd,
                None,
                None,
                None,
                Some("Reset"),
                4,
            ),
            pkt(
                1,
                1_000,
                BtDirection::Tx,
                BtProtocol::Att,
                Some(0x40),
                Some(0x25),
                Some("2A19"),
                Some(ATT_WRITE_COMMAND),
                6,
            ),
            pkt(
                2,
                3_000,
                BtDirection::Rx,
                BtProtocol::Att,
                Some(0x40),
                Some(0x25),
                Some("2A19"),
                Some(ATT_NOTIFICATION),
                5,
            ),
            pkt(
                3,
                5_000,
                BtDirection::Tx,
                BtProtocol::Att,
                Some(0x41),
                Some(0x2A),
                Some("2A00"),
                Some(ATT_READ_REQUEST),
                3,
            ),
        ];
        let s = compute(&pkts);
        assert_eq!(s.packets, 4);
        assert_eq!(s.tx_packets, 3);
        assert_eq!(s.rx_packets, 1);
        assert_eq!(s.bytes, 4 + 6 + 5 + 3);
        assert_eq!(s.tx_bytes, 4 + 6 + 3);
        assert_eq!(s.rx_bytes, 5);
        assert_eq!(s.att_writes, 1);
        assert_eq!(s.att_reads, 1);
        assert_eq!(s.notifications, 1);
        assert_eq!(s.indications, 0);
        assert_eq!(s.duration_us, 5_000); // 5000 - 0
        assert_eq!(s.unique_handles, 2); // 0x40, 0x41
        assert_eq!(s.unique_att_handles, 2); // 0x25, 0x2A
        assert_eq!(s.unique_uuids, 2); // 2A19, 2A00
    }

    #[test]
    fn non_att_opcodes_do_not_count_as_att_ops() {
        // An HCI event named similarly must not be miscounted.
        let pkts = vec![pkt(
            0,
            0,
            BtDirection::Rx,
            BtProtocol::HciEvt,
            None,
            None,
            None,
            Some("Command Complete"),
            5,
        )];
        let s = compute(&pkts);
        assert_eq!(s.att_writes, 0);
        assert_eq!(s.att_reads, 0);
        assert_eq!(s.notifications, 0);
    }
}
