//! Byte-diff highlighting for reverse-engineering (§13) + the "previous
//! comparable packet" lookup that drives it.
//!
//! When inspecting a packet, the natural comparison is the previous packet on
//! the SAME attribute — an app that pokes one GATT characteristic repeatedly
//! writes a series of similar payloads, and the changing bytes are the payload.
//! So the "previous" is the closest earlier packet sharing the ATT handle;
//! failing that, the connection handle; failing that, none.

use super::models::{BtPacket, BtPacketDetail};

/// Per-byte change mask of `cur` against `prev` (`true` = differs). Length
/// always equals `cur.len()`; bytes past the end of `prev` count as changed.
pub fn byte_diff(prev: &[u8], cur: &[u8]) -> Vec<bool> {
    cur.iter()
        .enumerate()
        .map(|(i, &b)| prev.get(i) != Some(&b))
        .collect()
}

/// Index of the closest earlier packet comparable to `packets[index]`:
/// same ATT handle if the target has one, else same connection handle.
pub fn find_prev(packets: &[BtPacket], index: usize) -> Option<u32> {
    let cur = packets.get(index)?;
    let by_att = cur.att_handle;
    let by_conn = cur.handle;
    packets[..index].iter().rev().find_map(|p| {
        let match_att = by_att.is_some() && p.att_handle == by_att;
        let match_conn = by_att.is_none() && by_conn.is_some() && p.handle == by_conn;
        (match_att || match_conn).then_some(p.index)
    })
}

/// Build the inspector detail for a packet, resolving its comparison target and
/// computing the byte-diff mask against it.
pub fn detail(packets: &[BtPacket], index: usize) -> Option<BtPacketDetail> {
    let p = packets.get(index)?;
    let prev_index = find_prev(packets, index);
    let diff = prev_index.and_then(|pi| {
        packets
            .iter()
            .find(|q| q.index == pi)
            .map(|prev| byte_diff(&prev.raw, &p.raw))
    });
    Some(BtPacketDetail {
        index: p.index,
        direction: p.direction,
        protocol: p.protocol,
        handle: p.handle,
        opcode: p.opcode.clone(),
        att_handle: p.att_handle,
        uuid: p.uuid.clone(),
        t_us: p.t_us,
        abs_ms: p.abs_ms,
        raw: p.raw.clone(),
        decoded: p.decoded.clone(),
        prev_index,
        diff,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth_capture::models::{BtDirection, BtProtocol};

    fn att_pkt(index: u32, handle: u16, att: Option<u16>, raw: &[u8]) -> BtPacket {
        BtPacket {
            index,
            t_us: index as u64 * 1000,
            abs_ms: 0,
            direction: BtDirection::Tx,
            protocol: BtProtocol::Att,
            handle: Some(handle),
            opcode: Some("Write Command".into()),
            att_handle: att,
            uuid: None,
            length: raw.len() as u32,
            summary: String::new(),
            raw: raw.to_vec(),
            decoded: Vec::new(),
        }
    }

    #[test]
    fn byte_diff_marks_changed_and_extra_bytes() {
        assert_eq!(byte_diff(&[1, 2, 3], &[1, 9, 3]), vec![false, true, false]);
        // cur longer than prev → the extra byte is "changed"
        assert_eq!(byte_diff(&[1, 2], &[1, 2, 5]), vec![false, false, true]);
        // cur shorter → mask matches cur length
        assert_eq!(byte_diff(&[1, 2, 3], &[1, 2]), vec![false, false]);
        assert_eq!(byte_diff(&[], &[1]), vec![true]);
    }

    #[test]
    fn find_prev_prefers_same_att_handle() {
        let pkts = vec![
            att_pkt(0, 0x40, Some(0x25), &[0x52, 0x25, 0x00, 0x01]),
            att_pkt(1, 0x40, Some(0x2A), &[0x52, 0x2A, 0x00, 0x09]), // different att
            att_pkt(2, 0x40, Some(0x25), &[0x52, 0x25, 0x00, 0x07]), // same att as #0
        ];
        // #2's previous-on-same-att is #0, not #1.
        assert_eq!(find_prev(&pkts, 2), Some(0));
    }

    #[test]
    fn find_prev_falls_back_to_connection_handle() {
        let pkts = vec![
            att_pkt(0, 0x40, None, &[0xaa]),
            att_pkt(1, 0x41, None, &[0xbb]), // different conn handle
            att_pkt(2, 0x40, None, &[0xcc]), // same conn as #0
        ];
        assert_eq!(find_prev(&pkts, 2), Some(0));
        assert_eq!(find_prev(&pkts, 0), None); // nothing before it
    }

    #[test]
    fn detail_carries_prev_and_diff() {
        let pkts = vec![
            att_pkt(0, 0x40, Some(0x25), &[0x52, 0x25, 0x00, 0x01]),
            att_pkt(1, 0x40, Some(0x25), &[0x52, 0x25, 0x00, 0x02]),
        ];
        let d = detail(&pkts, 1).unwrap();
        assert_eq!(d.prev_index, Some(0));
        assert_eq!(d.diff, Some(vec![false, false, false, true])); // only last byte changed
    }

    #[test]
    fn detail_without_a_previous_has_no_diff() {
        let pkts = vec![att_pkt(0, 0x40, Some(0x25), &[0x52])];
        let d = detail(&pkts, 0).unwrap();
        assert_eq!(d.prev_index, None);
        assert_eq!(d.diff, None);
    }

    #[test]
    fn out_of_range_index_is_none() {
        let pkts = vec![att_pkt(0, 0x40, Some(0x25), &[0x52])];
        assert!(detail(&pkts, 5).is_none());
        assert!(find_prev(&pkts, 5).is_none());
    }
}
