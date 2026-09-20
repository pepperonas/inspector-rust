//! HCI / ACL / L2CAP / ATT decoding.
//!
//! Turns one raw HCI packet (whose H4 kind + direction the parser already
//! knows) into a classified `Decoded`: protocol, connection handle, a human
//! opcode, ATT handle + UUID where present, a one-line summary, and a decoded
//! field list for the inspector.
//!
//! ⚠️ Every byte access is bounds-checked — capture files are untrusted input
//! (the prompt's §29 rule). A truncated PDU degrades to a best-effort summary,
//! it never panics and never invents fields.

use super::models::{BtDirection, BtProtocol};

/// The H4 packet kind, as the source told us (PacketLogger type byte, btsnoop
/// H4 prefix, or a live backend).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HciKind {
    Command,
    Event,
    Acl,
    Sco,
}

/// The classified result. `raw`/timestamps are added by the caller.
#[derive(Debug, Clone, Default)]
pub struct Decoded {
    pub protocol: BtProtocol,
    pub handle: Option<u16>,
    pub opcode: Option<String>,
    pub att_handle: Option<u16>,
    pub uuid: Option<String>,
    pub summary: String,
    pub fields: Vec<(String, String)>,
}

// ── ATT opcode names — the ONE source of truth, shared with stats.rs so the
// summary text and the summary counters can never disagree (the registry rule).
pub const ATT_WRITE_REQUEST: &str = "Write Request";
pub const ATT_WRITE_COMMAND: &str = "Write Command";
pub const ATT_READ_REQUEST: &str = "Read Request";
pub const ATT_READ_BLOB_REQUEST: &str = "Read Blob Request";
pub const ATT_NOTIFICATION: &str = "Handle Value Notification";
pub const ATT_INDICATION: &str = "Handle Value Indication";

/// How an ATT opcode counts in the session summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttClass {
    Write,
    Read,
    Notify,
    Indicate,
    Other,
}

/// Classify an ATT opcode name for the stats counters. Keyed on the shared
/// constants above, so a rename can't silently drop a counter.
pub fn att_class(opcode: &str) -> AttClass {
    match opcode {
        ATT_WRITE_REQUEST | ATT_WRITE_COMMAND => AttClass::Write,
        ATT_READ_REQUEST | ATT_READ_BLOB_REQUEST => AttClass::Read,
        ATT_NOTIFICATION => AttClass::Notify,
        ATT_INDICATION => AttClass::Indicate,
        _ => AttClass::Other,
    }
}

fn u16le(b: &[u8], i: usize) -> Option<u16> {
    if i + 1 < b.len() {
        Some(u16::from_le_bytes([b[i], b[i + 1]]))
    } else {
        None
    }
}

/// Format a UUID from its little-endian on-the-wire bytes. 2 bytes → `2A19`,
/// 16 bytes → the canonical dashed form. Any other length is not a UUID.
pub fn format_uuid(le: &[u8]) -> Option<String> {
    match le.len() {
        2 => Some(format!("{:04X}", u16::from_le_bytes([le[0], le[1]]))),
        4 => Some(format!(
            "{:08X}",
            u32::from_le_bytes([le[0], le[1], le[2], le[3]])
        )),
        16 => {
            // On the wire UUIDs are little-endian; the canonical string is
            // big-endian.
            let mut be = [0u8; 16];
            for (i, b) in le.iter().enumerate() {
                be[15 - i] = *b;
            }
            Some(format!(
                "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                be[0], be[1], be[2], be[3], be[4], be[5], be[6], be[7],
                be[8], be[9], be[10], be[11], be[12], be[13], be[14], be[15],
            ))
        }
        _ => None,
    }
}

/// The one entry point. `data` is the full HCI packet body for its kind
/// (Command: opcode+len+params; Event: code+len+params; ACL: handle+len+L2CAP).
pub fn decode(kind: HciKind, _dir: BtDirection, data: &[u8]) -> Decoded {
    match kind {
        HciKind::Command => decode_command(data),
        HciKind::Event => decode_event(data),
        HciKind::Acl => decode_acl(data),
        HciKind::Sco => Decoded {
            protocol: BtProtocol::Sco,
            summary: format!("SCO data ({} bytes)", data.len()),
            ..Default::default()
        },
    }
}

fn decode_command(data: &[u8]) -> Decoded {
    let mut d = Decoded {
        protocol: BtProtocol::HciCmd,
        ..Default::default()
    };
    let Some(opcode) = u16le(data, 0) else {
        d.summary = "HCI Command (truncated)".into();
        return d;
    };
    let ogf = opcode >> 10;
    let ocf = opcode & 0x03ff;
    let name = hci_command_name(opcode);
    let label = name.unwrap_or("HCI Command").to_string();
    d.opcode = Some(label.clone());
    d.summary = match name {
        Some(n) => n.to_string(),
        None => format!("HCI Command OGF 0x{ogf:02X} OCF 0x{ocf:03X}"),
    };
    d.fields.push(("Opcode".into(), format!("0x{opcode:04X}")));
    d.fields.push(("OGF".into(), format!("0x{ogf:02X}")));
    d.fields.push(("OCF".into(), format!("0x{ocf:03X}")));
    if let Some(&plen) = data.get(2) {
        d.fields.push(("Param length".into(), plen.to_string()));
    }
    d
}

fn decode_event(data: &[u8]) -> Decoded {
    let mut d = Decoded {
        protocol: BtProtocol::HciEvt,
        ..Default::default()
    };
    let Some(&code) = data.first() else {
        d.summary = "HCI Event (truncated)".into();
        return d;
    };
    let name = hci_event_name(code);
    d.fields
        .push(("Event code".into(), format!("0x{code:02X}")));
    // LE Meta events (0x3e) carry a subevent in the first parameter byte.
    if code == 0x3e {
        let sub = data.get(2).copied();
        let subname = sub.and_then(le_meta_name);
        d.opcode = Some(
            subname
                .map(|s| s.to_string())
                .unwrap_or_else(|| "LE Meta Event".into()),
        );
        d.summary = subname.unwrap_or("LE Meta Event").to_string();
        if let Some(s) = sub {
            d.fields.push(("Subevent".into(), format!("0x{s:02X}")));
        }
        return d;
    }
    let label = name.unwrap_or("HCI Event").to_string();
    d.opcode = Some(label);
    d.summary = match name {
        Some(n) => n.to_string(),
        None => format!("HCI Event 0x{code:02X}"),
    };
    d
}

fn decode_acl(data: &[u8]) -> Decoded {
    let mut d = Decoded {
        protocol: BtProtocol::Acl,
        ..Default::default()
    };
    let Some(hf) = u16le(data, 0) else {
        d.summary = "ACL (truncated)".into();
        return d;
    };
    let handle = hf & 0x0fff;
    d.handle = Some(handle);
    d.fields.push(("Handle".into(), format!("0x{handle:04X}")));
    // L2CAP starts at byte 4: [len(2), cid(2), payload...]
    let l2 = &data.get(4..).unwrap_or(&[]);
    let Some(cid) = u16le(l2, 2) else {
        d.summary = format!("ACL data, handle 0x{handle:04X} ({} bytes)", data.len());
        return d;
    };
    d.fields.push(("L2CAP CID".into(), format!("0x{cid:04X}")));
    let payload = l2.get(4..).unwrap_or(&[]);
    match cid {
        0x0004 => {
            // ATT (the connection handle is already recorded above)
            decode_att(&mut d, payload);
        }
        0x0006 => {
            d.protocol = BtProtocol::Smp;
            let op = payload.first().copied();
            d.opcode = op.map(|o| smp_name(o).to_string());
            d.summary = op
                .map(|o| format!("SMP {} (0x{o:02X})", smp_name(o)))
                .unwrap_or_else(|| "SMP".into());
        }
        0x0001 | 0x0005 => {
            d.protocol = BtProtocol::L2capSig;
            let op = payload.first().copied();
            d.opcode = op.map(|o| format!("L2CAP signaling 0x{o:02X}"));
            d.summary = "L2CAP signaling".into();
        }
        _ => {
            d.protocol = BtProtocol::L2cap;
            d.summary = format!("L2CAP CID 0x{cid:04X} ({} bytes)", payload.len());
        }
    }
    d
}

fn decode_att(d: &mut Decoded, p: &[u8]) {
    d.protocol = BtProtocol::Att;
    let Some(&op) = p.first() else {
        d.summary = "ATT (truncated)".into();
        return;
    };
    d.fields.push(("ATT Opcode".into(), format!("0x{op:02X}")));
    let name = att_name(op);
    d.opcode = Some(name.to_string());
    // Opcodes that carry an attribute handle at bytes [1..3].
    let carries_handle = matches!(op, 0x0a | 0x0c | 0x12 | 0x16 | 0x1b | 0x1d | 0x52 | 0xd2);
    if carries_handle {
        if let Some(h) = u16le(p, 1) {
            d.att_handle = Some(h);
            d.fields.push(("ATT Handle".into(), format!("0x{h:04X}")));
        }
    }
    // Read By Type / Read By Group Type requests carry a type UUID after the
    // start/end handle pair (bytes 5..).
    if op == 0x08 || op == 0x10 {
        if let (Some(s), Some(e)) = (u16le(p, 1), u16le(p, 3)) {
            d.fields.push(("Start handle".into(), format!("0x{s:04X}")));
            d.fields.push(("End handle".into(), format!("0x{e:04X}")));
        }
        if let Some(u) = format_uuid(p.get(5..).unwrap_or(&[])) {
            d.uuid = Some(u.clone());
            d.fields.push(("Type UUID".into(), u));
        }
    }
    // Value-bearing ops: expose the value length; the bytes live in `raw`.
    let value = match op {
        0x0b => p.get(1..),                      // Read Response
        0x12 | 0x52 | 0x1b | 0x1d => p.get(3..), // Write/Notify/Indicate value
        _ => None,
    };
    if let Some(v) = value {
        d.fields.push(("Value length".into(), v.len().to_string()));
    }
    // Summary: name + handle where we have one.
    d.summary = match d.att_handle {
        Some(h) => format!("{name} · handle 0x{h:04X}"),
        None => name.to_string(),
    };
}

// ── Name tables (common LE-centric subset; fallbacks keep the raw code). ──

fn hci_command_name(op: u16) -> Option<&'static str> {
    Some(match op {
        0x0401 => "Inquiry",
        0x0406 => "Disconnect",
        0x0c03 => "Reset",
        0x0c01 => "Set Event Mask",
        0x0c13 => "Write Local Name",
        0x0c14 => "Read Local Name",
        0x1001 => "Read Local Version Information",
        0x1002 => "Read Local Supported Commands",
        0x1009 => "Read BD_ADDR",
        0x2001 => "LE Set Event Mask",
        0x2002 => "LE Read Buffer Size",
        0x2005 => "LE Set Random Address",
        0x2006 => "LE Set Advertising Parameters",
        0x2008 => "LE Set Advertising Data",
        0x2009 => "LE Set Scan Response Data",
        0x200a => "LE Set Advertise Enable",
        0x200b => "LE Set Scan Parameters",
        0x200c => "LE Set Scan Enable",
        0x200d => "LE Create Connection",
        0x200e => "LE Create Connection Cancel",
        0x2013 => "LE Connection Update",
        0x2016 => "LE Read Remote Features",
        0x2018 => "LE Rand",
        0x2019 => "LE Start Encryption",
        0x201a => "LE Long Term Key Request Reply",
        0x2022 => "LE Set Data Length",
        _ => return None,
    })
}

fn hci_event_name(code: u8) -> Option<&'static str> {
    Some(match code {
        0x01 => "Inquiry Complete",
        0x03 => "Connection Complete",
        0x04 => "Connection Request",
        0x05 => "Disconnection Complete",
        0x08 => "Encryption Change",
        0x0e => "Command Complete",
        0x0f => "Command Status",
        0x13 => "Number Of Completed Packets",
        0x1b => "Max Slots Change",
        0x30 => "Encryption Key Refresh Complete",
        0x3e => "LE Meta Event",
        _ => return None,
    })
}

fn le_meta_name(sub: u8) -> Option<&'static str> {
    Some(match sub {
        0x01 => "LE Connection Complete",
        0x02 => "LE Advertising Report",
        0x03 => "LE Connection Update Complete",
        0x04 => "LE Read Remote Features Complete",
        0x05 => "LE Long Term Key Request",
        0x0a => "LE Enhanced Connection Complete",
        0x0b => "LE Directed Advertising Report",
        0x0d => "LE Extended Advertising Report",
        _ => return None,
    })
}

fn att_name(op: u8) -> &'static str {
    match op {
        0x01 => "Error Response",
        0x02 => "Exchange MTU Request",
        0x03 => "Exchange MTU Response",
        0x04 => "Find Information Request",
        0x05 => "Find Information Response",
        0x06 => "Find By Type Value Request",
        0x07 => "Find By Type Value Response",
        0x08 => "Read By Type Request",
        0x09 => "Read By Type Response",
        0x0a => ATT_READ_REQUEST,
        0x0b => "Read Response",
        0x0c => ATT_READ_BLOB_REQUEST,
        0x0d => "Read Blob Response",
        0x0e => "Read Multiple Request",
        0x0f => "Read Multiple Response",
        0x10 => "Read By Group Type Request",
        0x11 => "Read By Group Type Response",
        0x12 => ATT_WRITE_REQUEST,
        0x13 => "Write Response",
        0x16 => "Prepare Write Request",
        0x17 => "Prepare Write Response",
        0x18 => "Execute Write Request",
        0x19 => "Execute Write Response",
        0x1b => ATT_NOTIFICATION,
        0x1d => ATT_INDICATION,
        0x1e => "Handle Value Confirmation",
        0x52 => ATT_WRITE_COMMAND,
        0xd2 => "Signed Write Command",
        _ => "ATT PDU",
    }
}

fn smp_name(op: u8) -> &'static str {
    match op {
        0x01 => "Pairing Request",
        0x02 => "Pairing Response",
        0x03 => "Pairing Confirm",
        0x04 => "Pairing Random",
        0x05 => "Pairing Failed",
        0x06 => "Encryption Information",
        0x0b => "Pairing Public Key",
        _ => "PDU",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_le_set_scan_enable() {
        // opcode 0x200c LE, plen 2, enable 1 filter 0
        let d = decode(
            HciKind::Command,
            BtDirection::Tx,
            &[0x0c, 0x20, 0x02, 0x01, 0x00],
        );
        assert_eq!(d.protocol, BtProtocol::HciCmd);
        assert_eq!(d.opcode.as_deref(), Some("LE Set Scan Enable"));
        assert!(d.summary.contains("Scan Enable"));
    }

    #[test]
    fn unknown_command_keeps_raw_ogf_ocf() {
        let d = decode(HciKind::Command, BtDirection::Tx, &[0xff, 0xfc, 0x00]);
        assert_eq!(d.opcode.as_deref(), Some("HCI Command"));
        assert!(d.summary.contains("OGF") && d.summary.contains("OCF"));
    }

    #[test]
    fn event_command_complete_and_le_meta() {
        let cc = decode(
            HciKind::Event,
            BtDirection::Rx,
            &[0x0e, 0x04, 0x01, 0x03, 0x0c, 0x00],
        );
        assert_eq!(cc.opcode.as_deref(), Some("Command Complete"));
        let meta = decode(HciKind::Event, BtDirection::Rx, &[0x3e, 0x0c, 0x02]);
        assert_eq!(meta.opcode.as_deref(), Some("LE Advertising Report"));
    }

    #[test]
    fn acl_att_write_command_carries_handle_and_value_len() {
        // ACL: handle 0x0040, then L2CAP len 6, CID 0x0004 (ATT), then
        // ATT: 0x52 (Write Command) handle 0x0025, value 61 64 00
        let data = [
            0x40, 0x00, // handle 0x0040 (flags 0)
            0x08, 0x00, // ACL data total length
            0x04, 0x00, // L2CAP length
            0x04, 0x00, // L2CAP CID = ATT
            0x52, 0x25, 0x00, 0x61, 0x64, 0x00, // ATT Write Command
        ];
        let d = decode(HciKind::Acl, BtDirection::Tx, &data);
        assert_eq!(d.protocol, BtProtocol::Att);
        assert_eq!(d.handle, Some(0x0040));
        assert_eq!(d.att_handle, Some(0x0025));
        assert_eq!(d.opcode.as_deref(), Some(ATT_WRITE_COMMAND));
        assert_eq!(att_class(&d.opcode.unwrap()), AttClass::Write);
        assert!(d.summary.contains("0x0025"));
    }

    #[test]
    fn acl_att_notification_classifies_as_notify() {
        let data = [
            0x40, 0x00, 0x06, 0x00, 0x04, 0x00, 0x04, 0x00, // ACL+L2CAP(ATT)
            0x1b, 0x25, 0x00, 0x61, // Notification handle 0x0025
        ];
        let d = decode(HciKind::Acl, BtDirection::Rx, &data);
        assert_eq!(d.opcode.as_deref(), Some(ATT_NOTIFICATION));
        assert_eq!(att_class(ATT_NOTIFICATION), AttClass::Notify);
        assert_eq!(d.att_handle, Some(0x0025));
    }

    #[test]
    fn acl_smp_channel() {
        let data = [
            0x40, 0x00, 0x02, 0x00, 0x02, 0x00, 0x06, 0x00, // CID 0x0006 = SMP
            0x01, 0x03, // Pairing Request
        ];
        let d = decode(HciKind::Acl, BtDirection::Tx, &data);
        assert_eq!(d.protocol, BtProtocol::Smp);
        assert_eq!(d.opcode.as_deref(), Some("Pairing Request"));
    }

    #[test]
    fn read_by_group_type_extracts_uuid() {
        // ATT 0x10, start 0x0001 end 0xFFFF, type UUID 0x2800 (Primary Service)
        let data = [
            0x40, 0x00, 0x0b, 0x00, 0x07, 0x00, 0x04, 0x00, 0x10, 0x01, 0x00, 0xff, 0xff, 0x00,
            0x28,
        ];
        let d = decode(HciKind::Acl, BtDirection::Tx, &data);
        assert_eq!(d.uuid.as_deref(), Some("2800"));
    }

    #[test]
    fn format_uuid_16_and_128() {
        assert_eq!(format_uuid(&[0x19, 0x2a]).as_deref(), Some("2A19"));
        // 0000180f-0000-1000-8000-00805f9b34fb little-endian on the wire
        let le = [
            0xfb, 0x34, 0x9b, 0x5f, 0x80, 0x00, 0x00, 0x80, 0x00, 0x10, 0x00, 0x00, 0x0f, 0x18,
            0x00, 0x00,
        ];
        assert_eq!(
            format_uuid(&le).as_deref(),
            Some("0000180f-0000-1000-8000-00805f9b34fb")
        );
        assert_eq!(format_uuid(&[0x01]), None);
    }

    #[test]
    fn truncated_inputs_never_panic() {
        for len in 0..3 {
            let _ = decode(HciKind::Command, BtDirection::Tx, &vec![0u8; len]);
            let _ = decode(HciKind::Event, BtDirection::Rx, &vec![0u8; len]);
            let _ = decode(HciKind::Acl, BtDirection::Tx, &vec![0u8; len]);
        }
    }
}
