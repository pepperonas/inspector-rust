//! Live tests of the built `ir-record-render` helper against a REAL stage-1 project.
//!
//! All `#[ignore]`: they need Metal, VideoToolbox, the ffmpeg dylibs and a recording.
//! `IR_RECORD_PROJECT=<project dir>` is required (skip otherwise); make one with
//! `IR_RECORD_KEEP=1 cargo test -p inspector-rust-core --lib session_live_display_5s -- --ignored --nocapture`
//! (display awake). Run:
//!
//! ```sh
//! IR_RECORD_PROJECT=<dir> cargo test -p record-render --test helper_live -- --ignored --nocapture --test-threads=1
//! ```
#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use record_render_protocol::{
    decode_event, decode_preview, decode_thumb, read_frame, write_command, write_frame, Codec, Command,
    Event, ExportKind, PreviewFrame, TAG_COMMAND, TAG_EVENT, TAG_PREVIEW, TAG_THUMB,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as Proc, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const HELPER: &str = env!("CARGO_BIN_EXE_ir-record-render");

fn project() -> Option<PathBuf> {
    match std::env::var_os("IR_RECORD_PROJECT") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("skipped: set IR_RECORD_PROJECT=<stage-1 project dir>");
            None
        }
    }
}

/// (duration_ms, mic offset ms) from the project's `project.json`.
fn recording(dir: &Path) -> (u64, i64) {
    let p: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("project.json")).unwrap()).unwrap();
    let r = &p["recording"];
    (r["durationMs"].as_u64().unwrap(), r["mic"]["offsetMs"].as_i64().unwrap_or(0))
}

/// The app's golden scene with this project's files, one clip over the whole recording.
fn scene(dir: &Path, speed: &[(f64, f64, f64)], out: (u32, u32)) -> Value {
    let (dur_ms, mic_ms) = recording(dir);
    let template = include_str!("../../core/rust-lib/src/record/fixtures/scene-golden.json");
    let mut s: Value = serde_json::from_str(&template.replace("<PROJECT>", &dir.display().to_string())).unwrap();
    let mut clip = s["clips"][0].clone();
    clip["sourceStartSec"] = json!(0.0);
    clip["sourceEndSec"] = json!(dur_ms as f64 / 1000.0);
    clip["extraAudio"]["offsetSec"] = json!(mic_ms as f64 / 1000.0);
    clip["extraAudio"]["gainDb"] = json!(0.0);
    s["clips"] = json!([clip]);
    s["cropByClip"] = json!([null]);
    s["speedRegions"] = json!(speed
        .iter()
        .map(|(a, b, v)| json!({"clipIndex": 0, "startSec": a, "endSec": b, "speed": v}))
        .collect::<Vec<_>>());
    s["output"] = json!({"width": out.0, "height": out.1, "fps": 30});
    s
}

enum Msg {
    Event(Event),
    Preview(PreviewFrame),
    Thumb(u64, Vec<u8>),
}

struct Helper {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<Msg>,
}

impl Helper {
    fn start() -> Helper {
        let mut child = Proc::new(HELPER)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start helper");
        let stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok((tag, body)) = read_frame(&mut stdout) {
                let msg = match tag {
                    TAG_EVENT => Msg::Event(decode_event(&body).expect("event json")),
                    TAG_PREVIEW => Msg::Preview(decode_preview(&body).expect("preview body")),
                    TAG_THUMB => {
                        let (i, jpeg) = decode_thumb(&body).unwrap();
                        Msg::Thumb(i, jpeg.to_vec())
                    }
                    other => panic!("unexpected tag {other}"),
                };
                if tx.send(msg).is_err() {
                    return;
                }
            }
        });
        Helper { child, stdin, rx }
    }

    fn send(&mut self, cmd: &Command) {
        write_command(&mut self.stdin, cmd).expect("send");
    }

    fn next(&self, within: Duration) -> Msg {
        match self.rx.recv_timeout(within) {
            Ok(m) => m,
            Err(RecvTimeoutError::Timeout) => panic!("helper silent for {within:?}"),
            Err(RecvTimeoutError::Disconnected) => panic!("helper closed its output"),
        }
    }

    fn event(&self, within: Duration) -> Event {
        match self.next(within) {
            Msg::Event(e) => e,
            _ => panic!("expected an event"),
        }
    }

    /// Waits for the export's final event, counting progress events.
    fn export_result(&self, within: Duration) -> (Event, u64, u64) {
        let deadline = Instant::now() + within;
        let (mut n, mut last) = (0, 0);
        loop {
            match self.event(deadline.saturating_duration_since(Instant::now())) {
                Event::ExportProgress { frames, total } => {
                    n += 1;
                    last = frames;
                    assert!(total > 0);
                }
                Event::Error { msg } => panic!("error during export: {msg}"),
                done => return (done, n, last),
            }
        }
    }

    /// Polls `Frame` until a preview newer than `since` arrives.
    fn frame_after(&mut self, since: u64) -> PreviewFrame {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            self.send(&Command::Frame { since_gen: since });
            match self.next(Duration::from_secs(5)) {
                Msg::Preview(p) if !p.is_empty() => return p,
                Msg::Preview(_) => std::thread::sleep(Duration::from_millis(30)),
                Msg::Event(e) => panic!("expected a frame, got {e:?}"),
                Msg::Thumb(..) => panic!("unexpected thumb"),
            }
        }
        panic!("no preview frame after gen {since}");
    }

    fn shutdown(mut self) -> std::process::ExitStatus {
        self.send(&Command::Shutdown);
        wait(&mut self.child, Duration::from_secs(10))
    }
}

fn wait(child: &mut Child, within: Duration) -> std::process::ExitStatus {
    let deadline = Instant::now() + within;
    loop {
        if let Some(s) = child.try_wait().unwrap() {
            return s;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("helper did not exit within {within:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn ffprobe(path: &Path, entries: &str) -> String {
    let out = Proc::new("ffprobe")
        .args(["-v", "error", "-show_entries", entries, "-of", "default=nw=1"])
        .arg(path)
        .output()
        .expect("ffprobe (dev tool) on PATH");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn ffprobe_duration(path: &Path) -> f64 {
    ffprobe(path, "format=duration")
        .trim()
        .trim_start_matches("duration=")
        .parse()
        .unwrap()
}

fn out_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ir-record-render-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
#[ignore]
fn helper_live_export() {
    let Some(dir) = project() else { return };
    let (dur_ms, _) = recording(&dir);
    let dur = dur_ms as f64 / 1000.0;
    let out = out_dir("export");
    let mut h = Helper::start();

    // Preview: Open + 3 frames, each the requested size and not empty.
    let preview_scene = scene(&dir, &[], (1920, 1080));
    let t = Instant::now();
    h.send(&Command::Open { scene: preview_scene.clone(), preview_w: 640, preview_h: 360 });
    assert_eq!(h.event(Duration::from_secs(30)), Event::Ready);
    let mut gen = 0;
    for (i, at) in [0.0, 1.5, 3.0].into_iter().enumerate() {
        if i > 0 {
            h.send(&Command::SetTime { source_sec: at });
        }
        let f = h.frame_after(gen);
        assert_eq!((f.w, f.h), (640, 360));
        assert_eq!(f.rgba.len(), 640 * 360 * 4);
        let lit = f.rgba.as_chunks::<4>().0.iter().filter(|p| p[0] > 8 || p[1] > 8 || p[2] > 8).count();
        assert!(lit > 640 * 360 / 10, "frame {i} is (nearly) black: {lit} lit pixels");
        println!("preview frame {i}: gen {} {}x{} at {:?}, {lit} lit px", f.gen, f.w, f.h, t.elapsed());
        gen = f.gen;
    }
    // Nothing newer while paused and untouched: an empty frame, not a stale copy.
    h.send(&Command::Frame { since_gen: u64::MAX });
    match h.next(Duration::from_secs(5)) {
        Msg::Preview(p) => assert!(p.is_empty()),
        _ => panic!("expected an (empty) preview frame"),
    }

    // Thumbnails: 3 JPEGs at the requested indices, then ThumbsDone.
    let t = Instant::now();
    h.send(&Command::Thumbs { scene: preview_scene, out_fps: 30, frames: vec![0, 45, 120], max_width: 320 });
    let mut thumbs = Vec::new();
    loop {
        match h.next(Duration::from_secs(60)) {
            Msg::Thumb(i, jpeg) => thumbs.push((i, jpeg)),
            Msg::Event(Event::ThumbsDone) => break,
            Msg::Event(e) => panic!("thumbs: {e:?}"),
            Msg::Preview(_) => panic!("unexpected preview"),
        }
    }
    assert_eq!(thumbs.iter().map(|t| t.0).collect::<Vec<_>>(), vec![0, 45, 120]);
    for (i, jpeg) in &thumbs {
        assert_eq!(&jpeg[..2], b"\xff\xd8", "thumb {i} is a JPEG");
    }
    println!("thumbs: {} in {:?}, sizes {:?}", thumbs.len(), t.elapsed(), thumbs.iter().map(|t| t.1.len()).collect::<Vec<_>>());
    std::fs::write(out.join("thumb-45.jpg"), &thumbs[1].1).unwrap();

    // Letterboxing: the middle half of the source (a portrait-ish crop) in a 16:9 frame must be
    // pillarboxed — black left/right edges, picture in the middle — never stretched.
    let mut cropped = scene(&dir, &[], (1920, 1080));
    cropped["cropByClip"] = json!([{"x": 0.25, "y": 0.0, "width": 0.5, "height": 1.0}]);
    h.send(&Command::Thumbs { scene: cropped, out_fps: 30, frames: vec![15], max_width: 960 });
    let crop_jpeg = match h.next(Duration::from_secs(60)) {
        Msg::Thumb(15, jpeg) => jpeg,
        _ => panic!("expected the cropped thumb"),
    };
    assert_eq!(h.event(Duration::from_secs(10)), Event::ThumbsDone);
    let crop_path = out.join("thumb-cropped.jpg");
    std::fs::write(&crop_path, &crop_jpeg).unwrap();
    // Mean colour of a 960×540 strip: x 0–100 (bar), 430–530 (picture).
    let strip = |x: u32| -> u32 {
        let o = Proc::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(&crop_path)
            .args(["-vf", &format!("crop=100:540:{x}:0,scale=1:1"), "-f", "rawvideo", "-pix_fmt", "gray", "-"])
            .output()
            .unwrap();
        o.stdout[0] as u32
    };
    let (edge, middle) = (strip(0), strip(430));
    println!("cropped thumb: edge luma {edge}, middle luma {middle}");
    assert!(edge < 10 && middle > 30, "edge {edge}, middle {middle}");

    // MP4 1080p30, with 1–3 s at ×2: the programme is one second shorter than the source.
    let mp4 = out.join("export.mp4");
    let t = Instant::now();
    h.send(&Command::Export {
        scene: scene(&dir, &[(1.0, 3.0, 2.0)], (1920, 1080)),
        kind: ExportKind::Mp4 { w: 1920, h: 1080, fps: 30, codec: Codec::H264 },
        out_path: mp4.display().to_string(),
    });
    let (done, progress, frames) = h.export_result(Duration::from_secs(300));
    assert_eq!(done, Event::ExportDone { path: mp4.display().to_string() });
    let got = ffprobe_duration(&mp4);
    println!(
        "mp4: {:?}, {} bytes, {progress} progress events (last {frames}), duration {got:.3} s (programme {:.3} s)\n{}",
        t.elapsed(),
        std::fs::metadata(&mp4).unwrap().len(),
        dur - 1.0,
        ffprobe(&mp4, "stream=codec_name,width,height,r_frame_rate,sample_rate,channels,duration")
    );
    assert!((got - (dur - 1.0)).abs() <= 0.2, "mp4 {got} vs programme {}", dur - 1.0);
    assert!(progress >= 1);

    // GIF 480 wide at 15 fps.
    let gif = out.join("export.gif");
    let t = Instant::now();
    h.send(&Command::Export {
        scene: scene(&dir, &[], (1920, 1080)),
        kind: ExportKind::Gif { w: 480, h: 270, fps: 15, loop_forever: true },
        out_path: gif.display().to_string(),
    });
    let (done, progress, _) = h.export_result(Duration::from_secs(300));
    assert_eq!(done, Event::ExportDone { path: gif.display().to_string() });
    let got = ffprobe_duration(&gif);
    println!(
        "gif: {:?}, {} bytes, {progress} progress events, duration {got:.3} s (programme {dur:.3} s)\n{}",
        t.elapsed(),
        std::fs::metadata(&gif).unwrap().len(),
        ffprobe(&gif, "stream=codec_name,width,height,r_frame_rate")
    );
    assert!((got - dur).abs() <= 0.2, "gif {got} vs programme {dur}");

    // The preview still works after the exports.
    h.send(&Command::SetTime { source_sec: 0.5 });
    let f = h.frame_after(gen);
    assert_eq!((f.w, f.h), (640, 360));

    assert!(h.shutdown().success());
    if std::env::var_os("IR_RECORD_KEEP_EXPORTS").is_some() {
        println!("exports kept in {}", out.display());
    } else {
        let _ = std::fs::remove_dir_all(&out);
    }
}

#[test]
#[ignore]
fn export_cancel_leaves_no_file() {
    let Some(dir) = project() else { return };
    let out = out_dir("cancel");
    let mp4 = out.join("cancelled.mp4");
    let mut h = Helper::start();
    h.send(&Command::Export {
        scene: scene(&dir, &[], (1920, 1080)),
        kind: ExportKind::Mp4 { w: 1920, h: 1080, fps: 60, codec: Codec::Hevc },
        out_path: mp4.display().to_string(),
    });
    // Cancel once the export is demonstrably running.
    match h.event(Duration::from_secs(60)) {
        Event::ExportProgress { .. } => {}
        e => panic!("expected progress first, got {e:?}"),
    }
    h.send(&Command::Cancel);
    let (done, ..) = h.export_result(Duration::from_secs(60));
    assert!(matches!(done, Event::ExportFailed { cancelled: true, .. }), "{done:?}");
    let left: Vec<_> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert!(left.is_empty(), "files left behind: {left:?}");
    assert!(h.shutdown().success());
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
#[ignore]
fn panic_is_reported_not_fatal() {
    // Name from the plan: a panic is REPORTED (`Fatal` event) instead of a silent death or a hang.
    let Some(_) = project() else { return };
    let mut h = Helper::start();
    // Raw JSON: the command only exists in debug builds of the helper.
    write_frame(&mut h.stdin, TAG_COMMAND, br#"{"cmd":"__test_panic"}"#).unwrap();
    match h.event(Duration::from_secs(10)) {
        Event::Fatal { msg } => assert!(msg.contains("test panic requested"), "{msg}"),
        e => panic!("expected Fatal, got {e:?}"),
    }
    let status = wait(&mut h.child, Duration::from_secs(10));
    assert!(!status.success(), "{status:?}");
    println!("panic -> Fatal, exit {status:?}");
}
