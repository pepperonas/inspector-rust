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
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/rust-lib/assets/record");
    let mut s: Value =
        serde_json::from_str(&template.replace("<PROJECT>", &dir.display().to_string()).replace("<ASSETS>", assets)).unwrap();
    let mut clip = s["clips"][0].clone();
    clip["sourceStartSec"] = json!(0.0);
    clip["sourceEndSec"] = json!(dur_ms as f64 / 1000.0);
    clip["extraAudio"]["offsetSec"] = json!(mic_ms as f64 / 1000.0);
    clip["extraAudio"]["gainDb"] = json!(0.0);
    s["clips"] = json!([clip]);
    s["cropByClip"] = json!([null]);
    s["layout"]["layoutByClip"] = json!([s["layout"]["layoutByClip"][0].clone()]);
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
            // The app sets it at spawn (`RenderClient::spawn`); text draws in the shipped fonts.
            .env("OPENSCREEN_FONTS_DIR", concat!(env!("CARGO_MANIFEST_DIR"), "/../core/rust-lib/assets/record/fonts"))
            // A dev helper has no `Contents/Resources/` beside it (`render_client::dev_env`).
            .env("IR_SEGMENTATION_MODEL", concat!(env!("CARGO_MANIFEST_DIR"), "/../vendor/selfie-segmentation/selfie_segmentation_landscape.onnx"))
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
    assert!(matches!(h.event(Duration::from_secs(30)), Event::Ready { .. }));
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

/// Fix round 1, ruling 2: the app goes away mid-export (its end of stdout closed). The next
/// progress write fails, the export is cancelled, nothing is published or left staged, and the
/// helper exits non-zero instead of rendering to the end for nobody.
#[test]
#[ignore]
fn broken_output_cancels_export_and_exits() {
    let Some(dir) = project() else { return };
    let out = out_dir("broken");
    let mp4 = out.join("orphan.mp4");
    let mut child = Proc::new(HELPER)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start helper");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    write_command(
        &mut stdin,
        &Command::Export {
            scene: scene(&dir, &[], (1920, 1080)),
            kind: ExportKind::Mp4 { w: 1920, h: 1080, fps: 60, codec: Codec::Hevc },
            out_path: mp4.display().to_string(),
        },
    )
    .unwrap();
    // Wait for the export to run, then close our end of its output.
    let (tag, body) = read_frame(&mut stdout).expect("first progress");
    assert_eq!(tag, TAG_EVENT);
    assert!(matches!(decode_event(&body).unwrap(), Event::ExportProgress { .. }));
    drop(stdout);
    let t = Instant::now();
    let status = wait(&mut child, Duration::from_secs(30));
    assert!(!status.success(), "{status:?}");
    let left: Vec<_> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name()).collect();
    println!("broken output: exit {status:?} after {:?}, files left {left:?}", t.elapsed());
    assert!(left.is_empty(), "files left behind: {left:?}");
    drop(stdin);
    let _ = std::fs::remove_dir_all(&out);
}

/// The styled scene with this project's files: one clip, padding, radius, shadow,
/// wallpaper 01, pointer on, one zoom over t = 2 s.
fn styled_scene(dir: &Path) -> Value {
    let (dur_ms, mic_ms) = recording(dir);
    let template = include_str!("../../core/rust-lib/src/record/fixtures/scene-golden-styled.json");
    let assets = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/rust-lib/assets/record");
    let mut s: Value =
        serde_json::from_str(&template.replace("<PROJECT>", &dir.display().to_string()).replace("<ASSETS>", assets)).unwrap();
    let mut clip = s["clips"][0].clone();
    clip["sourceStartSec"] = json!(0.0);
    clip["sourceEndSec"] = json!(dur_ms as f64 / 1000.0);
    clip["extraAudio"]["offsetSec"] = json!(mic_ms as f64 / 1000.0);
    clip["extraAudio"]["gainDb"] = json!(0.0);
    s["clips"] = json!([clip]);
    s["cropByClip"] = json!([null]);
    s["layout"]["layoutByClip"] = json!([s["layout"]["layoutByClip"][0].clone()]);
    s["effects"]["padding"] = json!(0.5);
    s["effects"]["roundnessFrac"] = json!(40.0 / 1800.0);
    s["effects"]["shadow"] = json!(0.6);
    s["background"] = json!({"kind": "image", "path": format!("{assets}/wallpapers/01.jpg")});
    s["cursor"]["show"] = json!(true);
    s["cursor"]["autoHide"] = json!(false);
    let zoom = s["zoomRegions"][0].clone();
    s["zoomRegions"] = json!([zoom]); // clip 0, 1–3 s
    s["speedRegions"] = json!([]);
    s
}

/// RGB bytes of a JPEG (ffmpeg, a dev tool) and its size.
fn jpeg_rgb(path: &Path) -> (u32, u32, Vec<u8>) {
    let dims = ffprobe(path, "stream=width,height");
    let get = |k: &str| -> u32 {
        dims.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).unwrap().trim().parse().unwrap()
    };
    let (w, h) = (get("width"), get("height"));
    let o = Proc::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24", "-"])
        .output()
        .unwrap();
    assert_eq!(o.stdout.len(), (w * h * 3) as usize);
    (w, h, o.stdout)
}

/// Stage 3: the preview shows the export's effect values. A preview frame at t = 2 s and the
/// `Thumbs` still of the same frame (the export's compositor path) differ by less than 3/255
/// on average.
#[test]
#[ignore]
fn preview_matches_still_with_effects() {
    let Some(dir) = project() else { return };
    let out = out_dir("parity");
    let mut h = Helper::start();
    let scene = styled_scene(&dir);
    let fps = scene["output"]["fps"].as_u64().unwrap() as u32;
    let frame = 2 * fps as u64;

    h.send(&Command::Thumbs { scene: scene.clone(), out_fps: fps, frames: vec![frame], max_width: 960 });
    let jpeg = match h.next(Duration::from_secs(120)) {
        Msg::Thumb(i, jpeg) if i == frame => jpeg,
        _ => panic!("expected the still"),
    };
    assert_eq!(h.event(Duration::from_secs(10)), Event::ThumbsDone);
    let still_path = out.join("still.jpg");
    std::fs::write(&still_path, &jpeg).unwrap();
    let (w, hgt, still) = jpeg_rgb(&still_path);

    h.send(&Command::Open { scene, preview_w: w, preview_h: hgt });
    assert!(matches!(h.event(Duration::from_secs(30)), Event::Ready { .. }));
    h.send(&Command::SetTime { source_sec: 2.0 });
    // Let the render thread settle on the seek (cursor load, wallpaper, zoom), then take the
    // newest frame.
    let mut f = h.frame_after(0);
    let settle = Instant::now() + Duration::from_secs(3);
    while Instant::now() < settle {
        std::thread::sleep(Duration::from_millis(300));
        h.send(&Command::Frame { since_gen: f.gen });
        if let Msg::Preview(p) = h.next(Duration::from_secs(5)) {
            if !p.is_empty() {
                f = p;
            }
        }
    }
    assert_eq!((f.w, f.h), (w, hgt), "preview and still share a size");
    let mut sum = 0u64;
    for (px, rgb) in f.rgba.as_chunks::<4>().0.iter().zip(still.as_chunks::<3>().0) {
        for c in 0..3 {
            sum += (px[c] as i64 - rgb[c] as i64).unsigned_abs();
        }
    }
    let mae = sum as f64 / (w as f64 * hgt as f64 * 3.0);
    // Keep both images for a look.
    let raw = out.join("preview.rgba");
    std::fs::write(&raw, &f.rgba).unwrap();
    let _ = Proc::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{w}x{hgt}"), "-i"])
        .arg(&raw)
        .arg(out.join("preview.png"))
        .status();
    // The mean alone is blind to a local difference: without the effect values the pointer was
    // drawn at the LiveView's default size and the whole-frame mean still stayed under 3
    // (measured 1.58). The worst 32-px block catches it (measured 53.9 without, 8.1 with).
    let block = |bx: u32, by: u32| -> f64 {
        let mut s = 0u64;
        for y in by..by + 32 {
            for x in bx..bx + 32 {
                let (i, j) = (((y * w + x) * 4) as usize, ((y * w + x) * 3) as usize);
                for c in 0..3 {
                    s += (f.rgba[i + c] as i64 - still[j + c] as i64).unsigned_abs();
                }
            }
        }
        s as f64 / (32.0 * 32.0 * 3.0)
    };
    let worst = (0..=(hgt - 32) / 16)
        .flat_map(|by| (0..=(w - 32) / 16).map(move |bx| (bx * 16, by * 16)))
        .map(|(bx, by)| block(bx, by))
        .fold(0.0, f64::max);
    println!(
        "parity: {w}x{hgt}, preview gen {}, mean abs diff {mae:.2} / 255, worst 32-px block {worst:.2} (files in {})",
        f.gen,
        out.display()
    );
    assert!(mae < 3.0, "preview differs from the still: mean abs diff {mae:.2}");
    assert!(worst < 20.0, "a part of the preview differs from the still: worst block {worst:.2}");
    assert!(h.shutdown().success());
}

/// Stage 3 end to end (Task 7): exports the scene the app built for a real project
/// (`IR_E2E_SCENE`, written by `inspector-rust-core`'s `stage3_e2e_scene`) as MP4 and GIF into
/// `IR_E2E_OUT`. Wallpaper, padding, laptop frame, themed pointer, click sound and the planned
/// zooms all come from the app's scene builder; this only drives the helper.
#[test]
#[ignore]
fn export_scene_file() {
    let (Some(scene_path), Some(out)) = (std::env::var_os("IR_E2E_SCENE"), std::env::var_os("IR_E2E_OUT")) else {
        eprintln!("skipped: set IR_E2E_SCENE=<scene.json> IR_E2E_OUT=<dir>");
        return;
    };
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    let scene: Value = serde_json::from_slice(&std::fs::read(&scene_path).unwrap()).unwrap();
    let (w, h) = (scene["output"]["width"].as_u64().unwrap() as u32, scene["output"]["height"].as_u64().unwrap() as u32);
    let mut helper = Helper::start();

    let mp4 = out.join("stage3.mp4");
    let t = Instant::now();
    helper.send(&Command::Export {
        scene: scene.clone(),
        kind: ExportKind::Mp4 { w, h, fps: 30, codec: Codec::H264 },
        out_path: mp4.display().to_string(),
    });
    let (done, _, frames) = helper.export_result(Duration::from_secs(600));
    assert_eq!(done, Event::ExportDone { path: mp4.display().to_string() });
    println!(
        "mp4: {:?}, {} bytes, {frames} frames, duration {:.3} s\n{}",
        t.elapsed(),
        std::fs::metadata(&mp4).unwrap().len(),
        ffprobe_duration(&mp4),
        ffprobe(&mp4, "stream=codec_name,width,height,r_frame_rate,sample_rate,channels")
    );

    let gw = 640u32;
    let gh = ((gw as f64 * h as f64 / w as f64 / 2.0).round() as u32) * 2;
    let mut gif_scene = scene;
    gif_scene["output"] = json!({"width": gw, "height": gh, "fps": 15});
    let gif = out.join("stage3.gif");
    let t = Instant::now();
    helper.send(&Command::Export {
        scene: gif_scene,
        kind: ExportKind::Gif { w: gw, h: gh, fps: 15, loop_forever: true },
        out_path: gif.display().to_string(),
    });
    let (done, _, _) = helper.export_result(Duration::from_secs(600));
    assert_eq!(done, Event::ExportDone { path: gif.display().to_string() });
    println!(
        "gif: {:?}, {} bytes, duration {:.3} s\n{}",
        t.elapsed(),
        std::fs::metadata(&gif).unwrap().len(),
        ffprobe_duration(&gif),
        ffprobe(&gif, "stream=codec_name,width,height,r_frame_rate")
    );
    assert!(helper.shutdown().success());
}

fn project2() -> Option<PathBuf> {
    match std::env::var_os("IR_RECORD_PROJECT2") {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            eprintln!("skipped: set IR_RECORD_PROJECT2=<a second, different project dir>");
            None
        }
    }
}

/// Stage 4 (Task 2): a scene with two sources — clip 0 = 0–2 s of project A, clip 1 = all of
/// project B (each clip names its own files, like `scene_json` builds for an inserted take).
fn two_source_scene(a: &Path, b: &Path) -> Value {
    let mut s = scene(a, &[], (1280, 720));
    let mut first = s["clips"][0].clone();
    first["sourceEndSec"] = json!(2.0);
    let (b_ms, b_mic) = recording(b);
    let bp: Value = serde_json::from_str(&std::fs::read_to_string(b.join("project.json")).unwrap()).unwrap();
    let mut second = first.clone();
    second["screenPath"] = json!(b.join("screen.mp4").display().to_string());
    second["cursorPath"] = json!(b.join("cursor.json").display().to_string());
    second["sourceStartSec"] = json!(0.0);
    second["sourceEndSec"] = json!(b_ms as f64 / 1000.0);
    second["hasAudio"] = bp["recording"]["systemAudio"].clone();
    if bp["recording"]["mic"].is_null() {
        second.as_object_mut().unwrap().remove("extraAudio");
    } else {
        second["extraAudio"]["path"] = json!(b.join("mic.wav").display().to_string());
        second["extraAudio"]["offsetSec"] = json!(b_mic as f64 / 1000.0);
    }
    s["clips"] = json!([first, second]);
    s["cropByClip"] = json!([null, null]);
    let rect = s["layout"]["layoutByClip"][0].clone();
    s["layout"]["layoutByClip"] = json!([rect.clone(), rect]);
    s
}

fn still(h: &mut Helper, scene: &Value, frame: u64, out: &Path, name: &str) -> (u32, u32, Vec<u8>) {
    h.send(&Command::Thumbs { scene: scene.clone(), out_fps: 30, frames: vec![frame], max_width: 960 });
    let jpeg = match h.next(Duration::from_secs(120)) {
        Msg::Thumb(i, jpeg) if i == frame => jpeg,
        _ => panic!("expected the still"),
    };
    assert_eq!(h.event(Duration::from_secs(10)), Event::ThumbsDone);
    let p = out.join(name);
    std::fs::write(&p, &jpeg).unwrap();
    jpeg_rgb(&p)
}

/// The newest frame after letting the render thread settle for `secs`.
fn settled(h: &mut Helper, since: u64, secs: f64) -> PreviewFrame {
    let mut f = h.frame_after(since);
    let until = Instant::now() + Duration::from_secs_f64(secs);
    while Instant::now() < until {
        std::thread::sleep(Duration::from_millis(250));
        h.send(&Command::Frame { since_gen: f.gen });
        if let Msg::Preview(p) = h.next(Duration::from_secs(5)) {
            if !p.is_empty() {
                f = p;
            }
        }
    }
    f
}

/// (mean abs diff, worst 32-px block) between a preview frame (RGBA) and a still (RGB).
fn diff(f: &PreviewFrame, still: &[u8]) -> (f64, f64) {
    let (w, h) = (f.w, f.h);
    let mut sum = 0u64;
    for (px, rgb) in f.rgba.as_chunks::<4>().0.iter().zip(still.as_chunks::<3>().0) {
        for c in 0..3 {
            sum += (px[c] as i64 - rgb[c] as i64).unsigned_abs();
        }
    }
    let block = |bx: u32, by: u32| -> f64 {
        let mut s = 0u64;
        for y in by..by + 32 {
            for x in bx..bx + 32 {
                let (i, j) = (((y * w + x) * 4) as usize, ((y * w + x) * 3) as usize);
                for c in 0..3 {
                    s += (f.rgba[i + c] as i64 - still[j + c] as i64).unsigned_abs();
                }
            }
        }
        s as f64 / (32.0 * 32.0 * 3.0)
    };
    let worst = (0..=(h - 32) / 16)
        .flat_map(|by| (0..=(w - 32) / 16).map(move |bx| (bx * 16, by * 16)))
        .map(|(bx, by)| block(bx, by))
        .fold(0.0, f64::max);
    (sum as f64 / (w as f64 * h as f64 * 3.0), worst)
}

/// Stage 4 (Task 2): the preview plays across a clip boundary into ANOTHER recording, and a
/// still in the second clip shows the second recording (preview vs. the export's still path).
/// Needs `IR_RECORD_PROJECT` and `IR_RECORD_PROJECT2` (two different recordings).
#[test]
#[ignore]
fn two_sources_preview_crosses_into_second_take() {
    let (Some(a), Some(b)) = (project(), project2()) else { return };
    let out = out_dir("two-sources");
    let mut h = Helper::start();
    let scene = two_source_scene(&a, &b);
    // Programme: clip 0 = 0–2 s, clip 1 starts at 2 s. Stills at 1 s (A) and 3 s (B at 1 s).
    let (w, hgt, still_a) = still(&mut h, &scene, 30, &out, "still-a.jpg");
    let (_, _, still_b) = still(&mut h, &scene, 90, &out, "still-b.jpg");

    h.send(&Command::Open { scene, preview_w: w, preview_h: hgt });
    assert!(matches!(h.event(Duration::from_secs(30)), Event::Ready { .. }));
    let opened = settled(&mut h, 0, 1.0);

    // Jump from clip 0 (take A) straight into clip 1 (take B) at 1 s: a switch to another file,
    // the same frame as the still of programme 3 s.
    h.send(&Command::SetActiveClip { clip: 1, source_sec: 1.0 });
    let f = settled(&mut h, opened.gen, 3.0);
    assert_eq!((f.w, f.h), (w, hgt));
    let (mae, worst) = diff(&f, &still_b);
    let raw = out.join("preview-b.rgba");
    std::fs::write(&raw, &f.rgba).unwrap();
    let _ = Proc::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{w}x{hgt}"), "-i"])
        .arg(&raw)
        .arg(out.join("preview-b.png"))
        .status();
    println!("second take: {w}x{hgt}, mean abs diff {mae:.2} / 255, worst 32-px block {worst:.2} (files in {})", out.display());
    assert!(mae < 3.0, "preview in clip 1 differs from the still: {mae:.2}");
    assert!(worst < 20.0, "a part of the preview in clip 1 differs: worst block {worst:.2}");

    // Back into take A, then cross the boundary by playing: 0.4 s before clip 0's end, play
    // ~1.4 s, pause — the render thread advances into the other file by itself.
    h.send(&Command::SetActiveClip { clip: 0, source_sec: 1.6 });
    let before = settled(&mut h, f.gen, 1.5);
    let (back_a, _) = diff(&before, &still_a);
    h.send(&Command::SetPlaying { playing: true });
    std::thread::sleep(Duration::from_millis(1400));
    h.send(&Command::SetPlaying { playing: false });
    let crossed = settled(&mut h, before.gen, 1.0);
    let (to_a, _) = diff(&crossed, &still_a);
    let (to_b, _) = diff(&crossed, &still_b);
    println!("back in A: diff to A {back_a:.2}; after crossing: diff to A {to_a:.2}, to B {to_b:.2}");
    assert!(to_b < to_a, "after playing over the boundary the preview must show take B (A {to_a:.2}, B {to_b:.2})");
    assert!(h.shutdown().success());
}

/// Stage 4 (Task 3): annotations — a Caveat text, an arrow, a mosaic oval and a free-form blur
/// (vendor patch 13) — look the same in the preview and in the export's still (mean < 3,
/// worst 32-px block < 20). Keeps `still.jpg` / `preview.png` in the output folder for a look.
#[test]
#[ignore]
fn annotations_preview_matches_still() {
    let Some(dir) = project() else { return };
    let out = out_dir("annotations");
    let mut h = Helper::start();
    let mut scene = styled_scene(&dir);
    let end = recording(&dir).0 as f64 / 1000.0;
    let base = |id: &str, kind: &str, x: f64, y: f64, w: f64, hh: f64, z: i32| {
        json!({"id": id, "clipIndex": 0, "startSec": 0.0, "endSec": end, "kind": kind,
            "x": x, "y": y, "w": w, "h": hh, "zIndex": z})
    };
    let mut text = base("t", "text", 0.08, 0.06, 0.5, 0.18, 0);
    text["space"] = json!("frame");
    text["text"] = json!({"content": "Hallo Caveat", "color": "#ffffff", "backgroundColor": "rgba(0, 0, 0, 0.7)",
        "fontSizeRel": 64.0 / 1080.0, "fontFamily": "Caveat", "fontWeight": "bold", "fontStyle": "normal",
        "textDecoration": "none", "textAlign": "center", "animation": "none"});
    let mut arrow = base("a", "figure", 0.62, 0.08, 0.2, 0.2, 1);
    arrow["space"] = json!("frame");
    arrow["figure"] = json!({"direction": "down-left", "color": "#34B27B", "strokeWidth": 8});
    let mut oval = base("o", "blur", 0.1, 0.45, 0.35, 0.3, 2);
    oval["blur"] = json!({"style": "mosaic", "shape": "oval", "color": "white", "intensity": 12, "blockSize": 16});
    let mut free = base("f", "blur", 0.55, 0.45, 0.35, 0.35, 3);
    free["blur"] = json!({"style": "blur", "shape": "freehand", "color": "white", "intensity": 20, "blockSize": 12,
        "freehandPoints": [{"x": 0.5, "y": 0.0}, {"x": 1.0, "y": 0.4}, {"x": 0.8, "y": 1.0},
                           {"x": 0.2, "y": 1.0}, {"x": 0.0, "y": 0.4}]});
    scene["annotations"] = json!([text, arrow, oval, free]);
    // At rest (no zoom): the comparison is about the annotations, not the zoom ramp.
    scene["zoomRegions"] = json!([]);
    let frame = 2 * 30;
    let (w, hgt, still) = still(&mut h, &scene, frame, &out, "still.jpg");
    h.send(&Command::Open { scene, preview_w: w, preview_h: hgt });
    assert!(matches!(h.event(Duration::from_secs(30)), Event::Ready { .. }));
    h.send(&Command::SetTime { source_sec: 2.0 });
    let f = settled(&mut h, 0, 3.0);
    assert_eq!((f.w, f.h), (w, hgt), "preview and still share a size");
    let raw = out.join("preview.rgba");
    std::fs::write(&raw, &f.rgba).unwrap();
    let _ = Proc::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{w}x{hgt}"), "-i"])
        .arg(&raw)
        .arg(out.join("preview.png"))
        .status();
    let (mae, worst) = diff(&f, &still);
    println!("annotations: {w}x{hgt}, mean abs diff {mae:.2} / 255, worst 32-px block {worst:.2} (files in {})", out.display());
    assert!(mae < 3.0, "preview differs from the still: mean abs diff {mae:.2}");
    assert!(worst < 20.0, "a part of the preview differs from the still: worst block {worst:.2}");
    assert!(h.shutdown().success());
}

/// Stage 5 (Task 5): the camera styles — picture-in-picture, side by side, camera only and a
/// blurred background (segmentation on the workspace `ort`, patch 16) — look the same in the
/// preview and in the export's still, and a time past the end of the (6 s) camera holds its
/// last frame in both (patch 17). Scenes come from the app (`camera_live_scenes` in
/// `core/rust-lib/src/record/editor_ipc.rs`):
///
/// ```sh
/// IR_EDITOR_PROJECT=<project> IR_CAMERA_SCENES=<dir> cargo test -p inspector-rust-core --lib camera_live_scenes -- --ignored --nocapture
/// IR_CAMERA_SCENES=<dir> cargo test -p record-render --test helper_live camera_preview_matches_still -- --ignored --nocapture
/// ```
#[test]
#[ignore]
fn camera_preview_matches_still() {
    let Some(scenes) = std::env::var_os("IR_CAMERA_SCENES").map(PathBuf::from) else {
        eprintln!("skipped: set IR_CAMERA_SCENES=<dir> (see the doc comment)");
        return;
    };
    let out = out_dir("camera");
    // (scene, source second): 2 s inside the camera, 8 s after its last frame.
    let cases = [("pip", 2.0), ("side-by-side", 2.0), ("camera-only", 2.0), ("blur", 2.0), ("pip", 8.0), ("blur", 8.0)];
    let mut h = Helper::start();
    for (name, at) in cases {
        let scene: Value = serde_json::from_slice(&std::fs::read(scenes.join(format!("{name}.json"))).unwrap()).unwrap();
        let frame = (at * 30.0) as u64;
        let tag = format!("{name}-{at}");
        let (w, hgt, still) = still(&mut h, &scene, frame, &out, &format!("{tag}-still.jpg"));
        h.send(&Command::Open { scene, preview_w: w, preview_h: hgt });
        match h.event(Duration::from_secs(30)) {
            Event::Ready { segmentation_model } => assert!(segmentation_model.is_some(), "the helper reports its model"),
            other => panic!("expected Ready, got {other:?}"),
        }
        h.send(&Command::SetTime { source_sec: at });
        let f = settled(&mut h, 0, 3.0);
        assert_eq!((f.w, f.h), (w, hgt), "preview and still share a size");
        let raw = out.join(format!("{tag}-preview.rgba"));
        std::fs::write(&raw, &f.rgba).unwrap();
        let _ = Proc::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{w}x{hgt}"), "-i"])
            .arg(&raw)
            .arg(out.join(format!("{tag}-preview.png")))
            .status();
        let (mae, worst) = diff(&f, &still);
        println!("camera {tag}: {w}x{hgt}, mean abs diff {mae:.2} / 255, worst 32-px block {worst:.2}");
        assert!(mae < 3.0, "{tag}: preview differs from the still: mean abs diff {mae:.2}");
        assert!(worst < 20.0, "{tag}: a part of the preview differs from the still: worst block {worst:.2}");
    }
    println!("files in {}", out.display());
    assert!(h.shutdown().success());
}

/// A mono 16-bit 48 kHz WAV: `f(t)` per sample.
fn write_wav(path: &Path, secs: f64, f: impl Fn(f64) -> f64) {
    let rate = 48_000u32;
    let n = (secs * rate as f64).round() as u32;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + n * 2).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    for v in [16u32.to_le_bytes(), [1, 0, 1, 0], rate.to_le_bytes(), (rate * 2).to_le_bytes(), [2, 0, 16, 0]] {
        b.extend_from_slice(&v);
    }
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(n * 2).to_le_bytes());
    for i in 0..n {
        let v = f(i as f64 / rate as f64).clamp(-1.0, 1.0);
        b.extend_from_slice(&((v * 32767.0).round() as i16).to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

/// `mean_volume` (dB) of `[from, to]` s of the file's audio after `filter`.
fn mean_volume(path: &Path, from: f64, to: f64, filter: &str) -> f64 {
    let out = Proc::new("ffmpeg")
        .args(["-v", "info", "-nostats", "-i"])
        .arg(path)
        .args(["-af", &format!("atrim={from}:{to},{filter},volumedetect"), "-f", "null", "-"])
        .output()
        .expect("ffmpeg (dev tool) on PATH");
    let log = String::from_utf8_lossy(&out.stderr);
    let line = log.lines().find(|l| l.contains("mean_volume:")).expect("volumedetect output");
    line.split("mean_volume:").nth(1).unwrap().trim().trim_end_matches(" dB").trim().parse().unwrap()
}

/// Integrated loudness (LUFS) and true peak (dBTP) of the whole file (`ebur128`).
fn ebur128(path: &Path) -> (f64, f64) {
    let out = Proc::new("ffmpeg")
        .args(["-v", "info", "-nostats", "-i"])
        .arg(path)
        .args(["-af", "ebur128=peak=true", "-f", "null", "-"])
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&out.stderr);
    let summary = &log[log.rfind("Summary:").expect("ebur128 summary")..];
    let after = |key: &str| -> f64 {
        let rest = &summary[summary.find(key).unwrap() + key.len()..];
        rest.split_whitespace().find_map(|w| w.parse().ok()).unwrap()
    };
    (after("I:"), after("Peak:"))
}

/// Stage 4: music (loop, −18 dB, 1 s fades) and a voiceover under a programme without other
/// audio. Measures the export: music audible, lower under the voice (ducking), the voice
/// lifted by its loudness normalisation, true peak ≤ −2 dBTP. Builds a synthetic video when
/// `IR_RECORD_PROJECT` is not set (only the audio is measured).
#[test]
#[ignore]
fn audio_tracks_export_levels() {
    let work = out_dir("audio-tracks");
    let dir = match std::env::var_os("IR_RECORD_PROJECT") {
        Some(p) => PathBuf::from(p),
        None => {
            let d = work.join("project");
            std::fs::create_dir_all(&d).unwrap();
            let ok = Proc::new("ffmpeg")
                .args(["-v", "error", "-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=30:duration=12", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
                .arg(d.join("screen.mp4"))
                .status()
                .unwrap();
            assert!(ok.success(), "synthetic video");
            std::fs::write(d.join("project.json"), r#"{"recording":{"durationMs":12000}}"#).unwrap();
            d
        }
    };
    let (dur_ms, _) = recording(&dir);
    let dur = (dur_ms as f64 / 1000.0).min(12.0);
    // Music: 4 s of 220 Hz at −6 dBFS, looped. Voice: quiet 1 kHz (−26 dBFS), on 2 s, off 2 s.
    let music = work.join("music.wav");
    write_wav(&music, 4.0, |t| 0.5 * (2.0 * std::f64::consts::PI * 220.0 * t).sin());
    let voice = work.join("voice.wav");
    write_wav(&voice, 8.0, |t| if (t as u64) % 4 < 2 { 0.05 * (2.0 * std::f64::consts::PI * 1000.0 * t).sin() } else { 0.0 });
    let mut s = scene(&dir, &[], (1280, 720));
    s["clips"][0]["sourceEndSec"] = json!(dur);
    s["clips"][0]["hasAudio"] = json!(false);
    s["clips"][0].as_object_mut().unwrap().remove("extraAudio");
    // What `audio_tracks::project_tracks` sends for these two tracks (anchored at 0 s and 2 s).
    let mut tracks = Vec::new();
    let mut at = 0.0;
    while at < dur {
        let this = (dur - at).min(4.0);
        tracks.push(json!({"path": music.display().to_string(), "startSec": at, "gainDb": -18.0,
            "trimStartSec": 0.0, "trimEndSec": this,
            "fadeInSec": if at == 0.0 { 1.0 } else { 0.0 }, "fadeOutSec": if at + this >= dur { 1.0 } else { 0.0 },
            "kind": "music"}));
        at += 4.0;
    }
    tracks.push(json!({"path": voice.display().to_string(), "startSec": 2.0, "gainDb": 0.0,
        "trimStartSec": 0.0, "trimEndSec": 8.0, "fadeInSec": 0.0, "fadeOutSec": 0.0, "kind": "voiceover"}));
    s["audioTracks"] = json!(tracks);
    // The same, the music pushed to +12 dB: the limiter must hold the ceiling.
    let mut hot = s.clone();
    for t in hot["audioTracks"].as_array_mut().unwrap() {
        if t["kind"] == "music" {
            t["gainDb"] = json!(12.0);
        }
    }

    let mut h = Helper::start();
    let mp4 = work.join("audio.mp4");
    let hot_mp4 = work.join("audio-hot.mp4");
    for (scene, out) in [(s, &mp4), (hot, &hot_mp4)] {
        h.send(&Command::Export {
            scene,
            kind: ExportKind::Mp4 { w: 1280, h: 720, fps: 30, codec: Codec::H264 },
            out_path: out.display().to_string(),
        });
        let (done, _, _) = h.export_result(Duration::from_secs(300));
        assert_eq!(done, Event::ExportDone { path: out.display().to_string() });
    }
    assert!(h.shutdown().success());
    let (hot_lufs, hot_tp) = ebur128(&hot_mp4);
    println!("hot export (music +12 dB): integrated {hot_lufs:.1} LUFS, true peak {hot_tp:.2} dBTP");
    assert!(hot_tp <= -2.0 + 0.3, "true peak {hot_tp} dBTP over the −2 dBTP ceiling");

    let (lufs, tp) = ebur128(&mp4);
    // Voice 2–4 s and 6–8 s; music alone 4.0–6.0 (minus hold + release) and 10.0–11.0.
    let music_under_voice = mean_volume(&mp4, 2.8, 3.8, "lowpass=f=400,lowpass=f=400");
    let music_alone = mean_volume(&mp4, 5.3, 5.9, "lowpass=f=400,lowpass=f=400");
    let music_after = mean_volume(&mp4, 10.4, 10.9, "lowpass=f=400,lowpass=f=400");
    let voice_level = mean_volume(&mp4, 2.8, 3.8, "highpass=f=600,highpass=f=600");
    println!(
        "audio export: integrated {lufs:.1} LUFS, true peak {tp:.2} dBTP; music alone {music_alone:.1} / {music_after:.1} dB, \
         under the voice {music_under_voice:.1} dB (ducked {:.1} dB); voice {voice_level:.1} dB (file −29 dB mean)",
        music_alone - music_under_voice
    );
    assert!(tp <= -2.0 + 0.3, "true peak {tp} dBTP over the −2 dBTP ceiling");
    assert!(music_alone > -45.0 && music_after > -45.0, "music audible: {music_alone} / {music_after}");
    assert!(music_under_voice < music_alone - 6.0, "music ducked under the voice: {music_under_voice} vs {music_alone}");
    assert!(voice_level > -29.0 + 6.0, "the quiet voice is lifted: {voice_level}");
    if std::env::var_os("IR_RECORD_KEEP_EXPORTS").is_some() {
        println!("export kept in {}", work.display());
    } else {
        let _ = std::fs::remove_dir_all(&work);
    }
}
