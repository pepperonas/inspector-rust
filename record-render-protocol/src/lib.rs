//! Wire protocol between Inspector Rust and its `ir-record-render` helper process.
//!
//! Both directions are a stream of frames on the helper's stdin/stdout:
//! `tag: u8` + `len: u32 LE` + `len` bytes. Tags:
//!
//! | tag | direction | body |
//! |---|---|---|
//! | [`TAG_COMMAND`] `0x01` | app → helper | JSON [`Command`] |
//! | [`TAG_EVENT`] `0x02` | helper → app | JSON [`Event`] |
//! | [`TAG_PREVIEW`] `0x03` | helper → app | `gen u64 LE`, `w u32 LE`, `h u32 LE`, RGBA8 (`w·h·4`) |
//! | [`TAG_THUMB`] `0x04` | helper → app | `index u64 LE` + JPEG bytes |
//!
//! Replies: `Open` → `Ready`; `Frame` → exactly one `0x03` frame (an EMPTY one — `w = h = 0`,
//! `gen = since_gen`, no pixels — when nothing newer exists); `Thumbs` → one `0x04` per
//! rendered frame, then `ThumbsDone`; `Export` → `ExportProgress`… then `ExportDone` or
//! `ExportFailed`. The setters (`SetScene`, `SetActiveClip`, `SetPlaying`, `SetTime`, `Resize`)
//! answer only on failure (`Error`). `Cancel` is handled out of band (it must reach an export
//! that is blocking the helper's render thread). Any command may also produce `Error`; `Fatal`
//! means the helper is about to exit.
//!
//! Platform-free on purpose (serde/serde_json only): the app on every OS and the helper share it.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, Read, Write};

pub const TAG_COMMAND: u8 = 0x01;
pub const TAG_EVENT: u8 = 0x02;
pub const TAG_PREVIEW: u8 = 0x03;
pub const TAG_THUMB: u8 = 0x04;

/// Largest body one frame may carry. A 4K RGBA preview is 33 MB; anything near this limit is a
/// corrupted length, not a frame, and must not make the reader allocate gigabytes.
pub const MAX_FRAME_LEN: usize = 256 << 20;

pub fn write_frame(w: &mut impl Write, tag: u8, body: &[u8]) -> io::Result<()> {
    if body.len() > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("frame body of {} bytes exceeds {MAX_FRAME_LEN}", body.len()),
        ));
    }
    let mut header = [0u8; 5];
    header[0] = tag;
    header[1..].copy_from_slice(&(body.len() as u32).to_le_bytes());
    w.write_all(&header)?;
    w.write_all(body)?;
    w.flush()
}

/// Reads one frame. A stream that ends mid-frame (or before one starts) is
/// `ErrorKind::UnexpectedEof`; a length over [`MAX_FRAME_LEN`] is `InvalidData`.
pub fn read_frame(r: &mut impl Read) -> io::Result<(u8, Vec<u8>)> {
    let mut header = [0u8; 5];
    r.read_exact(&mut header)?;
    let len = u32::from_le_bytes([header[1], header[2], header[3], header[4]]) as usize;
    if len > MAX_FRAME_LEN {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame length {len} exceeds {MAX_FRAME_LEN}"),
        ));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok((header[0], body))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    H264,
    Hevc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExportKind {
    Mp4 { w: u32, h: u32, fps: u32, codec: Codec },
    Gif { w: u32, h: u32, fps: u32, loop_forever: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// `scene` is the compositor `Scene` JSON (built by the app's `scene_json`).
    Open { scene: Value, preview_w: u32, preview_h: u32 },
    SetScene { scene: Value },
    /// `source_sec` is source time of that clip, not programme time.
    SetActiveClip { clip: usize, source_sec: f64 },
    SetPlaying { playing: bool },
    SetTime { source_sec: f64 },
    Resize { w: u32, h: u32 },
    Frame { since_gen: u64 },
    /// `frames` are indices on the `out_fps` programme grid.
    Thumbs { scene: Value, out_fps: u32, frames: Vec<u64>, max_width: u32 },
    Export { scene: Value, kind: ExportKind, out_path: String },
    Cancel,
    Shutdown,
    /// Debug builds only: panics inside the render thread, to test the crash path.
    #[cfg(debug_assertions)]
    #[serde(rename = "__test_panic")]
    TestPanic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Ready,
    ThumbsDone,
    Error { msg: String },
    /// `total` is the expected frame count of the programme (an estimate: a clip the file
    /// cannot fill is shortened while exporting).
    ExportProgress { frames: u64, total: u64 },
    ExportDone { path: String },
    ExportFailed { msg: String, cancelled: bool },
    Fatal { msg: String },
}

pub fn write_command(w: &mut impl Write, cmd: &Command) -> io::Result<()> {
    write_json(w, TAG_COMMAND, cmd)
}

pub fn write_event(w: &mut impl Write, ev: &Event) -> io::Result<()> {
    write_json(w, TAG_EVENT, ev)
}

fn write_json(w: &mut impl Write, tag: u8, value: &impl Serialize) -> io::Result<()> {
    let body = serde_json::to_vec(value).map_err(io::Error::other)?;
    write_frame(w, tag, &body)
}

pub fn decode_command(body: &[u8]) -> io::Result<Command> {
    serde_json::from_slice(body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

pub fn decode_event(body: &[u8]) -> io::Result<Event> {
    serde_json::from_slice(body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// A decoded `0x03` body. `w == 0 && h == 0` means "nothing newer than `since_gen`".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewFrame {
    pub gen: u64,
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

impl PreviewFrame {
    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }
}

pub fn encode_preview(gen: u64, w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(16 + rgba.len());
    body.extend_from_slice(&gen.to_le_bytes());
    body.extend_from_slice(&w.to_le_bytes());
    body.extend_from_slice(&h.to_le_bytes());
    body.extend_from_slice(rgba);
    body
}

pub fn decode_preview(body: &[u8]) -> io::Result<PreviewFrame> {
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    if body.len() < 16 {
        return Err(bad("preview frame shorter than its 16-byte header"));
    }
    let gen = u64::from_le_bytes(body[0..8].try_into().unwrap());
    let w = u32::from_le_bytes(body[8..12].try_into().unwrap());
    let h = u32::from_le_bytes(body[12..16].try_into().unwrap());
    let want = (w as u64) * (h as u64) * 4;
    if (body.len() - 16) as u64 != want {
        return Err(bad("preview pixel count does not match its size"));
    }
    Ok(PreviewFrame { gen, w, h, rgba: body[16..].to_vec() })
}

pub fn encode_thumb(index: u64, jpeg: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(8 + jpeg.len());
    body.extend_from_slice(&index.to_le_bytes());
    body.extend_from_slice(jpeg);
    body
}

pub fn decode_thumb(body: &[u8]) -> io::Result<(u64, &[u8])> {
    if body.len() < 8 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "thumb frame shorter than 8 bytes"));
    }
    Ok((u64::from_le_bytes(body[0..8].try_into().unwrap()), &body[8..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Cursor;

    #[test]
    fn frame_roundtrip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, TAG_EVENT, b"hello").unwrap();
        write_frame(&mut buf, TAG_PREVIEW, &[]).unwrap();
        let big = vec![7u8; 70_000];
        write_frame(&mut buf, TAG_THUMB, &big).unwrap();
        assert_eq!(&buf[..5], &[TAG_EVENT, 5, 0, 0, 0], "tag + u32 LE length");
        let mut r = Cursor::new(buf);
        assert_eq!(read_frame(&mut r).unwrap(), (TAG_EVENT, b"hello".to_vec()));
        assert_eq!(read_frame(&mut r).unwrap(), (TAG_PREVIEW, Vec::new()));
        assert_eq!(read_frame(&mut r).unwrap(), (TAG_THUMB, big));
        // The stream is exhausted: the next read is an EOF, not a phantom frame.
        assert_eq!(read_frame(&mut r).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn preview_and_thumb_bodies_roundtrip() {
        let px = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let p = decode_preview(&encode_preview(42, 2, 1, &px)).unwrap();
        assert_eq!(p, PreviewFrame { gen: 42, w: 2, h: 1, rgba: px });
        let empty = decode_preview(&encode_preview(9, 0, 0, &[])).unwrap();
        assert!(empty.is_empty());
        assert_eq!(empty.gen, 9);
        // A size that does not match the pixels is refused.
        assert!(decode_preview(&encode_preview(1, 2, 2, &[0; 4])).is_err());
        let body = encode_thumb(7, b"\xff\xd8jpeg");
        assert_eq!(decode_thumb(&body).unwrap(), (7, &b"\xff\xd8jpeg"[..]));
        assert!(decode_thumb(&[1, 2]).is_err());
    }

    fn every_command() -> Vec<Command> {
        let scene = json!({"clips": [], "output": {"width": 1920, "height": 1080, "fps": 30}});
        let mut all = vec![
            Command::Open { scene: scene.clone(), preview_w: 640, preview_h: 360 },
            Command::SetScene { scene: scene.clone() },
            Command::SetActiveClip { clip: 2, source_sec: 1.25 },
            Command::SetPlaying { playing: true },
            Command::SetTime { source_sec: 3.5 },
            Command::Resize { w: 800, h: 450 },
            Command::Frame { since_gen: 17 },
            Command::Thumbs { scene: scene.clone(), out_fps: 30, frames: vec![0, 15, 90], max_width: 160 },
            Command::Export {
                scene: scene.clone(),
                kind: ExportKind::Mp4 { w: 1920, h: 1080, fps: 30, codec: Codec::H264 },
                out_path: "/tmp/a.mp4".into(),
            },
            Command::Export {
                scene: scene.clone(),
                kind: ExportKind::Mp4 { w: 1280, h: 720, fps: 60, codec: Codec::Hevc },
                out_path: "/tmp/b.mp4".into(),
            },
            Command::Export {
                scene,
                kind: ExportKind::Gif { w: 480, h: 270, fps: 15, loop_forever: false },
                out_path: "/tmp/c.gif".into(),
            },
            Command::Cancel,
            Command::Shutdown,
        ];
        #[cfg(debug_assertions)]
        all.push(Command::TestPanic);
        all
    }

    #[test]
    fn command_json_roundtrip() {
        for cmd in every_command() {
            let mut buf = Vec::new();
            write_command(&mut buf, &cmd).unwrap();
            let (tag, body) = read_frame(&mut Cursor::new(buf)).unwrap();
            assert_eq!(tag, TAG_COMMAND);
            assert_eq!(decode_command(&body).unwrap(), cmd);
        }
        // The wire names are part of the contract with the app.
        let v = serde_json::to_value(Command::SetActiveClip { clip: 1, source_sec: 0.5 }).unwrap();
        assert_eq!(v, json!({"cmd": "set_active_clip", "clip": 1, "source_sec": 0.5}));
        let v = serde_json::to_value(Command::Export {
            scene: json!({}),
            kind: ExportKind::Mp4 { w: 2, h: 2, fps: 30, codec: Codec::Hevc },
            out_path: "x".into(),
        })
        .unwrap();
        assert_eq!(v["kind"], json!({"type": "mp4", "w": 2, "h": 2, "fps": 30, "codec": "hevc"}));
        assert_eq!(serde_json::to_value(Command::Cancel).unwrap(), json!({"cmd": "cancel"}));
        // An unknown command is an error, not a silent no-op.
        assert!(decode_command(br#"{"cmd":"format_disk"}"#).is_err());
    }

    #[test]
    fn event_json_roundtrip() {
        let all = vec![
            Event::Ready,
            Event::ThumbsDone,
            Event::Error { msg: "no".into() },
            Event::ExportProgress { frames: 3, total: 90 },
            Event::ExportDone { path: "/x.mp4".into() },
            Event::ExportFailed { msg: "cancelled".into(), cancelled: true },
            Event::Fatal { msg: "boom".into() },
        ];
        for ev in all {
            let mut buf = Vec::new();
            write_event(&mut buf, &ev).unwrap();
            let (tag, body) = read_frame(&mut Cursor::new(buf)).unwrap();
            assert_eq!(tag, TAG_EVENT);
            assert_eq!(decode_event(&body).unwrap(), ev);
        }
        assert_eq!(serde_json::to_value(Event::Ready).unwrap(), json!({"event": "ready"}));
    }

    #[test]
    fn truncated_frame_is_error() {
        let mut buf = Vec::new();
        write_frame(&mut buf, TAG_COMMAND, b"{\"cmd\":\"cancel\"}").unwrap();
        for cut in [1, 4, 5, buf.len() - 1] {
            let err = read_frame(&mut Cursor::new(&buf[..cut])).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof, "cut at {cut}");
        }
        // A corrupt length is refused before anything is allocated.
        let mut huge = vec![TAG_PREVIEW];
        huge.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(read_frame(&mut Cursor::new(huge)).unwrap_err().kind(), io::ErrorKind::InvalidData);
        // And the writer refuses to produce one.
        let over = vec![0u8; MAX_FRAME_LEN + 1];
        assert!(write_frame(&mut Vec::new(), TAG_PREVIEW, &over).is_err());
    }
}
