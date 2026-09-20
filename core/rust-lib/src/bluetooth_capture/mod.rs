//! Professional Bluetooth capture analyzer (`btsniff`).
//!
//! Stage 1 (this module tree) is the deterministic, fully cross-platform file
//! analyzer: parse a `.pklg` / btsnoop / pcapng capture into normalised
//! `BtPacket`s and decode HCI / ACL / L2CAP / ATT / GATT / SMP. Stage 2 will
//! add the CoreBluetooth live-GATT backend (`macos.rs`) behind a platform
//! abstraction — nothing here may assume macOS.
//!
//! The existing `bt` / `bluetooth` command (`crate::bluetooth`) is unrelated
//! and must not be touched.

pub mod capture;
pub mod decode;
pub mod diff;
pub mod export;
pub mod live;
pub mod models;
pub mod parser;

#[cfg(target_os = "macos")]
pub mod macos;

use std::path::Path;
use std::sync::Mutex;

use models::{BtPacket, BtPacketDetail, BtPacketSlim, CaptureState, CaptureStats};
use parser::CaptureFormat;

/// Reject a capture file larger than this before reading it into memory — a
/// real HCI session is a handful of MB; anything past this is almost certainly
/// a mis-selected file, and we must not let one exhaust memory (§29/§25).
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// The loaded analysis, held in Tauri-managed state. Stage 2 will extend this
/// with a live-capture backend; the state machine already models both.
#[derive(Default)]
pub struct CaptureSession {
    pub state: CaptureState,
    pub format: Option<CaptureFormat>,
    pub packets: Vec<BtPacket>,
    pub stats: CaptureStats,
    /// Display label for the source (basename only — never the full path).
    pub source_label: Option<String>,
}

/// Tauri-managed handle. `session` is the analyzer (file) session; `live` is
/// the Stage-2 live-capture session. The interior `Mutex`/`Arc` are what the
/// free functions take, so they stay unit-testable without Tauri.
pub struct BtSniffState {
    pub session: Mutex<CaptureSession>,
    pub live: std::sync::Arc<live::LiveShared>,
}

impl Default for BtSniffState {
    fn default() -> Self {
        Self {
            session: Mutex::new(CaptureSession::default()),
            live: std::sync::Arc::new(live::LiveShared::default()),
        }
    }
}

/// The parsed result before it's committed to the session — the return of the
/// heavy IO/parse work, computed with no `State` so it runs in `spawn_blocking`.
pub struct Loaded {
    pub format: CaptureFormat,
    pub packets: Vec<BtPacket>,
    pub stats: CaptureStats,
    pub label: String,
}

/// What `open_file` hands back to the frontend: the classified metadata + the
/// slim timeline. Heavy per-packet payloads are fetched on demand via `detail`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OpenResult {
    pub format: String,
    pub source_label: String,
    pub packets: Vec<BtPacketSlim>,
    pub stats: CaptureStats,
}

#[derive(Debug)]
pub enum SessionError {
    TooLarge(u64),
    Read(String),
    Parse(String),
    NoSession,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::TooLarge(n) => {
                write!(f, "capture file is too large ({n} bytes; max 512 MiB)")
            }
            SessionError::Read(e) => write!(f, "could not read capture file: {e}"),
            SessionError::Parse(e) => write!(f, "{e}"),
            SessionError::NoSession => write!(f, "no capture is loaded"),
        }
    }
}

impl std::error::Error for SessionError {}

fn format_tag(f: CaptureFormat) -> &'static str {
    match f {
        CaptureFormat::Pklg => "pklg",
        CaptureFormat::Btsnoop => "btsnoop",
        CaptureFormat::Pcapng => "pcapng",
        CaptureFormat::Unknown => "unknown",
    }
}

fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

/// Read + parse a capture file — the size-gated IO + parse, with NO session
/// state so a command can run it in `spawn_blocking` off the async runtime.
pub fn load(path: &str) -> Result<Loaded, SessionError> {
    let meta = std::fs::metadata(path).map_err(|e| SessionError::Read(e.to_string()))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(SessionError::TooLarge(meta.len()));
    }
    let bytes = std::fs::read(path).map_err(|e| SessionError::Read(e.to_string()))?;
    let format = parser::detect_format(&bytes);
    let packets = parser::parse(&bytes).map_err(|e| SessionError::Parse(e.to_string()))?;
    let stats = stats::compute(&packets);
    Ok(Loaded {
        format,
        packets,
        stats,
        label: basename(path),
    })
}

/// Commit a parsed capture into the session and build the slim view for the UI.
pub fn store(session: &Mutex<CaptureSession>, loaded: Loaded) -> OpenResult {
    let result = OpenResult {
        format: format_tag(loaded.format).to_string(),
        source_label: loaded.label.clone(),
        packets: loaded.packets.iter().map(BtPacket::slim).collect(),
        stats: loaded.stats.clone(),
    };
    let mut s = session.lock().expect("capture session poisoned");
    s.state = CaptureState::Stopped; // a loaded file is a completed capture
    s.format = Some(loaded.format);
    s.packets = loaded.packets;
    s.stats = loaded.stats;
    s.source_label = Some(loaded.label);
    result
}

/// Read + parse + commit in one call (the async command uses the `load`+`store`
/// split so the IO runs in `spawn_blocking`; this one-shot is the test seam).
#[cfg(test)]
pub fn open_file(session: &Mutex<CaptureSession>, path: &str) -> Result<OpenResult, SessionError> {
    Ok(store(session, load(path)?))
}

/// The inspector detail for one packet (bytes + decoded + byte-diff).
pub fn packet_detail(
    session: &Mutex<CaptureSession>,
    index: usize,
) -> Result<BtPacketDetail, SessionError> {
    let s = session.lock().expect("capture session poisoned");
    if s.packets.is_empty() {
        return Err(SessionError::NoSession);
    }
    diff::detail(&s.packets, index).ok_or(SessionError::NoSession)
}

pub fn stats(session: &Mutex<CaptureSession>) -> CaptureStats {
    session
        .lock()
        .expect("capture session poisoned")
        .stats
        .clone()
}

/// Serialise the loaded session to JSON.
pub fn export_json(session: &Mutex<CaptureSession>) -> Result<String, SessionError> {
    let s = session.lock().expect("capture session poisoned");
    if s.packets.is_empty() {
        return Err(SessionError::NoSession);
    }
    let fmt = s.format.map(format_tag).unwrap_or("unknown");
    Ok(export::build_json(fmt, &s.packets, &s.stats))
}

mod stats;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("ir-btsniff-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        let mut f = std::fs::File::create(&p).unwrap();
        f.write_all(bytes).unwrap();
        p
    }

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

    #[test]
    fn open_parses_and_stores_then_detail_and_export_work() {
        let mut buf = pklg_rec(0x00, 1, 0, &[0x0c, 0x20, 0x02, 0x01, 0x00]);
        buf.extend(pklg_rec(
            0x02,
            2,
            0,
            &[
                0x40, 0x00, 0x08, 0x00, 0x04, 0x00, 0x04, 0x00, 0x52, 0x25, 0x00, 0x61, 0x64, 0x00,
            ],
        ));
        let path = write_temp("session.pklg", &buf);

        let session = Mutex::new(CaptureSession::default());
        let res = open_file(&session, path.to_str().unwrap()).unwrap();
        assert_eq!(res.format, "pklg");
        assert_eq!(res.packets.len(), 2);
        assert_eq!(res.source_label, "session.pklg");
        assert_eq!(res.stats.packets, 2);
        assert_eq!(res.stats.att_writes, 1);
        // slim list carries no bytes
        let slim_json = serde_json::to_string(&res.packets).unwrap();
        assert!(!slim_json.contains("\"raw\""));

        // detail for the ATT write
        let d = packet_detail(&session, 1).unwrap();
        assert_eq!(d.att_handle, Some(0x0025));
        assert_eq!(
            d.raw,
            vec![
                0x40, 0x00, 0x08, 0x00, 0x04, 0x00, 0x04, 0x00, 0x52, 0x25, 0x00, 0x61, 0x64, 0x00
            ]
        );

        let json = export_json(&session).unwrap();
        assert!(json.contains("\"format\": \"pklg\""));
    }

    #[test]
    fn detail_and_export_without_session_error() {
        let session = Mutex::new(CaptureSession::default());
        assert!(matches!(
            packet_detail(&session, 0),
            Err(SessionError::NoSession)
        ));
        assert!(matches!(
            export_json(&session),
            Err(SessionError::NoSession)
        ));
    }

    #[test]
    fn missing_file_is_a_read_error() {
        let session = Mutex::new(CaptureSession::default());
        assert!(matches!(
            open_file(&session, "/no/such/capture.pklg"),
            Err(SessionError::Read(_))
        ));
    }

    #[test]
    fn state_defaults_to_idle() {
        assert_eq!(CaptureState::default(), CaptureState::Idle);
    }
}
