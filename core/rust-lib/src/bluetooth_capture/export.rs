//! JSON export of an analysed session + filename helpers.
//!
//! Privacy (§21): the default filename is timestamp-only, and any user-supplied
//! name is run through `sanitize_export_name`, which strips MAC-address runs so
//! a device address can never leak into a filename.

use chrono::{DateTime, Local};
use serde::Serialize;

use super::models::{BtDirection, BtPacket, CaptureStats};

#[derive(Serialize)]
struct ExportDoc<'a> {
    tool: &'static str,
    schema: u32,
    format: &'a str,
    packet_count: usize,
    stats: &'a CaptureStats,
    packets: Vec<ExportPacket>,
}

#[derive(Serialize)]
struct ExportPacket {
    index: u32,
    t_us: u64,
    abs_ms: u64,
    direction: BtDirection,
    protocol: &'static str,
    handle: Option<u16>,
    opcode: Option<String>,
    att_handle: Option<u16>,
    uuid: Option<String>,
    length: u32,
    summary: String,
    hex: String,
    decoded: Vec<(String, String)>,
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Serialise the whole session to pretty JSON. `format` is the source format
/// tag ("pklg" / "btsnoop" / "pcapng").
pub fn build_json(format: &str, packets: &[BtPacket], stats: &CaptureStats) -> String {
    let doc = ExportDoc {
        tool: "inspector-rust btsniff",
        schema: 1,
        format,
        packet_count: packets.len(),
        stats,
        packets: packets
            .iter()
            .map(|p| ExportPacket {
                index: p.index,
                t_us: p.t_us,
                abs_ms: p.abs_ms,
                direction: p.direction,
                protocol: p.protocol.tag(),
                handle: p.handle,
                opcode: p.opcode.clone(),
                att_handle: p.att_handle,
                uuid: p.uuid.clone(),
                length: p.length,
                summary: p.summary.clone(),
                hex: to_hex(&p.raw),
                decoded: p.decoded.clone(),
            })
            .collect(),
    };
    // Serialisation of our own owned data cannot fail; fall back defensively.
    serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string())
}

/// Default export filename: timestamp only, never any device data.
pub fn default_json_filename(now: DateTime<Local>) -> String {
    format!(
        "inspector-bluetooth-analysis-{}.json",
        now.format("%Y-%m-%d_%H-%M-%S")
    )
}

pub fn default_json_filename_now() -> String {
    default_json_filename(Local::now())
}

/// Remove any `HH:HH:HH:HH:HH:HH` / `HH-HH-...` MAC-address run from a string.
pub fn strip_mac_addresses(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if is_mac_at(&chars, i) {
            i += 17; // a MAC is exactly 6*2 hex + 5 separators
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn is_mac_at(c: &[char], i: usize) -> bool {
    // pattern: HH sep HH sep HH sep HH sep HH sep HH  (17 chars)
    if i + 17 > c.len() {
        return false;
    }
    let sep = c[i + 2];
    if sep != ':' && sep != '-' {
        return false;
    }
    for group in 0..6 {
        let base = i + group * 3;
        if !c[base].is_ascii_hexdigit() || !c[base + 1].is_ascii_hexdigit() {
            return false;
        }
        if group < 5 && c[base + 2] != sep {
            return false;
        }
    }
    true
}

/// Turn a user-supplied name into a safe `.json` basename (no path parts, no
/// MAC address, no control chars). Empty/degenerate input falls back to the
/// timestamp default.
pub fn sanitize_export_name(raw: &str, now: DateTime<Local>) -> String {
    let stripped = strip_mac_addresses(raw);
    // basename only — never let a path separator through
    let base = stripped.rsplit(['/', '\\']).next().unwrap_or("");
    let mut cleaned: String = base
        .chars()
        .map(|ch| match ch {
            c if c.is_alphanumeric() => c,
            '-' | '_' | '.' => ch,
            _ => '-',
        })
        .collect();
    // collapse repeated dashes, trim edges
    while cleaned.contains("--") {
        cleaned = cleaned.replace("--", "-");
    }
    let cleaned = cleaned.trim_matches(['-', '.', ' ']).to_string();
    let stem = cleaned
        .strip_suffix(".json")
        .unwrap_or(&cleaned)
        .to_string();
    if stem.is_empty() {
        return default_json_filename(now);
    }
    format!("{stem}.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth_capture::models::BtProtocol;
    use chrono::TimeZone;

    fn fixed_now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 3, 7, 14, 5, 9).unwrap()
    }

    fn sample_pkt() -> BtPacket {
        BtPacket {
            index: 0,
            t_us: 0,
            abs_ms: 1_700_000_000_000,
            direction: BtDirection::Tx,
            protocol: BtProtocol::Att,
            handle: Some(0x0040),
            opcode: Some("Write Command".into()),
            att_handle: Some(0x0025),
            uuid: Some("2A19".into()),
            length: 4,
            summary: "Write Command · handle 0x0025".into(),
            raw: vec![0x52, 0x25, 0x00, 0x61],
            decoded: vec![("ATT Opcode".into(), "0x52".into())],
        }
    }

    #[test]
    fn json_has_stats_and_hex_packet() {
        let pkts = vec![sample_pkt()];
        let stats = CaptureStats {
            packets: 1,
            att_writes: 1,
            ..Default::default()
        };
        let json = build_json("pklg", &pkts, &stats);
        assert!(json.contains("\"format\": \"pklg\""));
        assert!(json.contains("\"protocol\": \"ATT\""));
        assert!(json.contains("\"hex\": \"52250061\""));
        assert!(json.contains("\"att_writes\": 1"));
        assert!(json.contains("\"direction\": \"tx\""));
        // parses back
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["packet_count"], 1);
    }

    #[test]
    fn default_filename_is_timestamped() {
        assert_eq!(
            default_json_filename(fixed_now()),
            "inspector-bluetooth-analysis-2026-03-07_14-05-09.json"
        );
    }

    #[test]
    fn strips_mac_addresses() {
        assert_eq!(
            strip_mac_addresses("scooter AA:BB:CC:11:22:33 log"),
            "scooter  log"
        );
        assert_eq!(strip_mac_addresses("de-ad-be-ef-00-01"), "");
        // not a MAC — left intact
        assert_eq!(strip_mac_addresses("v1.2.3"), "v1.2.3");
        assert_eq!(strip_mac_addresses("AA:BB"), "AA:BB"); // too short
    }

    #[test]
    fn sanitize_name_drops_paths_macs_and_bad_chars() {
        let now = fixed_now();
        assert_eq!(
            sanitize_export_name("/tmp/../my scan.json", now),
            "my-scan.json"
        );
        assert_eq!(
            sanitize_export_name("ninebot AA:BB:CC:DD:EE:FF", now),
            "ninebot.json"
        );
        // empty after cleaning → timestamp default
        assert_eq!(sanitize_export_name("//", now), default_json_filename(now));
        assert_eq!(sanitize_export_name("", now), default_json_filename(now));
    }
}
