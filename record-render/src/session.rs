//! The helper's session: one render thread owns every compositor object, a reader thread feeds
//! it commands.
//!
//! Threading (RECON §8): `Gpu`/`Compositor` hold `RefCell`s and are confined to the thread that
//! runs [`run`]. The reader thread only parses frames, hands commands over a channel and handles
//! `Cancel` out of band — an export blocks the render thread for minutes, so a cancel queued
//! behind it would arrive after the fact. Every command runs inside `catch_unwind` and an
//! autorelease pool (metal-rs returns autoreleased command buffers; without a pool they pile up
//! on this long-lived thread). A panic becomes a `Fatal` event and the process exits non-zero
//! WITHOUT running destructors — a `LiveView` drop joins its render thread, and a poisoned
//! session is not the moment to wait on it.

use anyhow::{anyhow, bail, Context, Result};
use openscreen_compositor::compositor::Compositor;
use openscreen_compositor::config::{self, Cfg};
use openscreen_compositor::d3d::Gpu;
use openscreen_compositor::export_control::{ExportCancelled, ExportControl};
use openscreen_compositor::frame_geometry::live_params_from_scene;
use openscreen_compositor::gif_export::{export_gif_cancellable, GifExportParams};
use openscreen_compositor::live::LiveView;
use openscreen_compositor::pipeline::{self, ClipSource, ExportCodec, ExportParams};
use openscreen_compositor::regions::speed_segments_for_window;
use openscreen_compositor::scene::{Scene, SceneSpeedRegion};
use openscreen_compositor::timeline_walk;
use record_render_protocol::{
    decode_command, encode_preview, encode_thumb, read_frame, write_event, write_frame, Codec,
    Command, Event, ExportKind, TAG_COMMAND, TAG_PREVIEW, TAG_THUMB,
};
use serde_json::Value;
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::FromRawFd;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Exit code after a `Fatal` (panic or broken output channel).
const EXIT_FATAL: i32 = 2;
/// How many compositors (one per output size) stay cached. Each compiles the 4k-line MSL
/// uber-shader on construction, so reuse matters; each also holds render targets.
const MAX_COMPOSITORS: usize = 3;
/// Progress events at most this often (plus the first frame).
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// The protocol channel. Cloned into the reader thread for its own `Error` replies.
type Out = Arc<Mutex<File>>;

fn send_event(out: &Out, ev: &Event) -> io::Result<()> {
    let mut f = out.lock().unwrap_or_else(|p| p.into_inner());
    write_event(&mut *f, ev)
}

fn send_frame(out: &Out, tag: u8, body: &[u8]) -> io::Result<()> {
    let mut f = out.lock().unwrap_or_else(|p| p.into_inner());
    write_frame(&mut *f, tag, body)
}

extern "C" {
    fn dup(fd: i32) -> i32;
    fn dup2(from: i32, to: i32) -> i32;
}

/// Moves the protocol to a private descriptor and points fd 1 at stderr, so a stray
/// `println!`, an ffmpeg log or any library writing to stdout can never corrupt a frame.
fn take_protocol_stdout() -> io::Result<File> {
    io::stdout().flush()?;
    // SAFETY: plain descriptor calls on fds 1 and 2, which every process has; the new
    // descriptor is owned by the returned File and closed exactly once by it.
    unsafe {
        let fd = dup(1);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        if dup2(2, 1) < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(File::from_raw_fd(fd))
    }
}

/// Runs the helper until `Shutdown`, end of input or a fatal error; returns the exit code.
pub fn run() -> i32 {
    let out: Out = match take_protocol_stdout() {
        Ok(f) => Arc::new(Mutex::new(f)),
        Err(e) => {
            eprintln!("ir-record-render: cannot set up the protocol channel: {e}");
            return EXIT_FATAL;
        }
    };
    let exports = ExportSlot::default();
    let (tx, rx) = mpsc::channel::<Incoming>();
    {
        let out = out.clone();
        let exports = exports.clone();
        std::thread::Builder::new()
            .name("ir-render-stdin".into())
            .spawn(move || read_commands(io::stdin().lock(), tx, out, exports))
            .expect("spawn stdin reader");
    }

    let mut state = State::new(out.clone(), exports);
    while let Ok(incoming) = rx.recv() {
        let shutdown = matches!(incoming.cmd, Command::Shutdown);
        let result = catch_unwind(AssertUnwindSafe(|| {
            objc::rc::autoreleasepool(|| state.handle(incoming))
        }));
        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                // The output channel itself is gone: nobody is listening any more.
                eprintln!("ir-record-render: cannot write to the app: {e}");
                exit_now(EXIT_FATAL);
            }
            Err(panic) => {
                let msg = panic_message(&*panic);
                let _ = send_event(&out, &Event::Fatal { msg: format!("render helper crashed: {msg}") });
                exit_now(EXIT_FATAL);
            }
        }
        if shutdown {
            break;
        }
    }
    // Shutdown or end of input: leave without joining the preview's render thread.
    exit_now(0)
}

fn exit_now(code: i32) -> ! {
    let _ = io::stderr().flush();
    std::process::exit(code)
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// A command plus, for `Export`, the control the reader thread already registered — so a
/// `Cancel` sent right after an `Export` reaches it even before the render thread picks it up.
struct Incoming {
    cmd: Command,
    control: Option<ExportControl>,
}

/// The export currently queued or running, cancellable from the reader thread.
#[derive(Clone, Default)]
struct ExportSlot(Arc<Mutex<Option<(u64, ExportControl)>>>);

impl ExportSlot {
    fn begin(&self, id: u64) -> ExportControl {
        let control = ExportControl::default();
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Some((id, control.clone()));
        control
    }

    fn cancel(&self) {
        if let Some((_, c)) = &*self.0.lock().unwrap_or_else(|p| p.into_inner()) {
            c.cancel();
        }
    }

    fn finish(&self, id: u64) {
        let mut slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if slot.as_ref().is_some_and(|(i, _)| *i == id) {
            *slot = None;
        }
    }
}

fn read_commands(mut input: impl io::Read, tx: mpsc::Sender<Incoming>, out: Out, exports: ExportSlot) {
    let mut next_export = 0u64;
    loop {
        let (tag, body) = match read_frame(&mut input) {
            Ok(frame) => frame,
            Err(e) => {
                if e.kind() != io::ErrorKind::UnexpectedEof {
                    eprintln!("ir-record-render: reading commands failed: {e}");
                }
                // The app is gone (or sent garbage framing): stop like a Shutdown.
                exports.cancel();
                let _ = tx.send(Incoming { cmd: Command::Shutdown, control: None });
                return;
            }
        };
        if tag != TAG_COMMAND {
            let _ = send_event(&out, &Event::Error { msg: format!("unexpected frame tag {tag:#04x}") });
            continue;
        }
        let cmd = match decode_command(&body) {
            Ok(cmd) => cmd,
            Err(e) => {
                let _ = send_event(&out, &Event::Error { msg: format!("bad command: {e}") });
                continue;
            }
        };
        let control = match &cmd {
            Command::Cancel => {
                exports.cancel();
                continue;
            }
            Command::Shutdown => {
                exports.cancel();
                None
            }
            Command::Export { .. } => {
                next_export += 1;
                Some(exports.begin(next_export))
            }
            _ => None,
        };
        let last = matches!(cmd, Command::Shutdown);
        if tx.send(Incoming { cmd, control }).is_err() || last {
            return;
        }
    }
}

struct State {
    out: Out,
    exports: ExportSlot,
    export_id: u64,
    live: Option<LiveView>,
    /// The preview's scene (for `SetActiveClip`, which needs the clip's file).
    scene: Option<Scene>,
    gpu: Option<Gpu>,
    /// Compositors by output size, most recently used last.
    comps: Vec<((u32, u32), Compositor)>,
}

impl State {
    fn new(out: Out, exports: ExportSlot) -> Self {
        Self { out, exports, export_id: 0, live: None, scene: None, gpu: None, comps: Vec::new() }
    }

    fn event(&self, ev: Event) -> io::Result<()> {
        send_event(&self.out, &ev)
    }

    fn error(&self, msg: impl Into<String>) -> io::Result<()> {
        self.event(Event::Error { msg: msg.into() })
    }

    /// Runs one command. `Err` only when the output channel is broken.
    fn handle(&mut self, incoming: Incoming) -> io::Result<()> {
        match incoming.cmd {
            Command::Open { scene, preview_w, preview_h } => match self.open(&scene, preview_w, preview_h) {
                Ok(()) => self.event(Event::Ready),
                Err(e) => self.error(format!("{e:#}")),
            },
            Command::SetScene { scene } => match parse_scene(&scene) {
                Ok(parsed) => {
                    if let Some(live) = &self.live {
                        live.set_scene(&scene.to_string());
                    }
                    self.scene = Some(parsed);
                    Ok(())
                }
                Err(e) => self.error(format!("{e:#}")),
            },
            Command::SetActiveClip { clip, source_sec } => {
                let (Some(live), Some(scene)) = (&self.live, &self.scene) else {
                    return self.error("no preview open");
                };
                let Some(c) = scene.clips.get(clip) else {
                    return self.error(format!("clip {clip} does not exist ({} clips)", scene.clips.len()));
                };
                live.set_active_clip(&c.screen_path, &c.webcam_path, c.webcam_offset_sec, clip, source_sec);
                Ok(())
            }
            Command::SetPlaying { playing } => self.with_live(|l| l.set_playing(playing)),
            Command::SetTime { source_sec } => self.with_live(|l| l.set_time(source_sec)),
            Command::Resize { w, h } => self.with_live(|l| l.set_rect(w, h)),
            Command::Frame { since_gen } => self.frame(since_gen),
            Command::Thumbs { scene, out_fps, frames, max_width } => {
                match self.paused(|s| s.thumbs(&scene, out_fps, &frames, max_width)) {
                    Ok(()) => self.event(Event::ThumbsDone),
                    Err(e) => match e.downcast::<io::Error>() {
                        Ok(io) => Err(io),
                        Err(e) => self.error(format!("{e:#}")),
                    },
                }
            }
            Command::Export { scene, kind, out_path } => {
                self.export_id += 1;
                let id = self.export_id;
                let control = incoming.control.unwrap_or_default();
                let result = self.paused(|s| s.export(&scene, &kind, &out_path, &control));
                self.exports.finish(id);
                match result {
                    Ok(()) => self.event(Event::ExportDone { path: out_path }),
                    Err(e) => {
                        let cancelled = e.is::<ExportCancelled>();
                        let msg = if cancelled { "export cancelled".to_string() } else { format!("{e:#}") };
                        self.event(Event::ExportFailed { msg, cancelled })
                    }
                }
            }
            // Handled by the reader thread; never queued.
            Command::Cancel => Ok(()),
            Command::Shutdown => {
                // Let the preview's render thread wind down before the process exits.
                self.live = None;
                Ok(())
            }
            #[cfg(debug_assertions)]
            Command::TestPanic => panic!("test panic requested"),
        }
    }

    fn with_live(&self, f: impl FnOnce(&LiveView)) -> io::Result<()> {
        match &self.live {
            Some(l) => {
                f(l);
                Ok(())
            }
            None => self.error("no preview open"),
        }
    }

    fn open(&mut self, scene_json: &Value, w: u32, h: u32) -> Result<()> {
        let scene = parse_scene(scene_json)?;
        let first = scene.clips.first().ok_or_else(|| anyhow!("the scene has no clips"))?;
        // A previous preview first: its render thread holds a GPU context and decoders.
        self.live = None;
        let live = LiveView::create(w.max(1), h.max(1), &first.screen_path, &first.webcam_path, "")
            .context("starting the preview")?;
        live.set_scene(&scene_json.to_string());
        live.set_playing(false);
        live.set_active_clip(&first.screen_path, &first.webcam_path, first.webcam_offset_sec, 0, first.source_start_sec);
        self.live = Some(live);
        self.scene = Some(scene);
        Ok(())
    }

    fn frame(&self, since_gen: u64) -> io::Result<()> {
        let Some(live) = &self.live else {
            return self.error("no preview open");
        };
        if let Some(fatal) = live.fatal_error() {
            return self.error(format!("preview stopped: {fatal}"));
        }
        match live.latest_frame_since(since_gen) {
            Some((gen, w, h, rgba, ..)) => send_frame(&self.out, TAG_PREVIEW, &encode_preview(gen, w, h, &rgba)),
            None => send_frame(&self.out, TAG_PREVIEW, &encode_preview(since_gen, 0, 0, &[])),
        }
    }

    /// Runs `f` with the preview paused (frees the GPU's 3D engine for an export, RECON §8)
    /// and gives it back the transport it had — also when `f` fails.
    fn paused<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let was_playing = self.live.as_ref().map(|l| l.playing());
        if let Some(l) = &self.live {
            l.set_playing(false);
        }
        let result = f(self);
        if let (Some(l), Some(p)) = (&self.live, was_playing) {
            l.set_playing(p);
        }
        result
    }

    fn gpu(&mut self) -> Result<&Gpu> {
        if self.gpu.is_none() {
            self.gpu = Some(Gpu::create_auto(false).context("no Metal device")?);
        }
        Ok(self.gpu.as_ref().expect("just set"))
    }

    /// The compositor for `size`, set up for `scene`. One per size is kept (shader compile).
    fn compositor(&mut self, size: (u32, u32), scene: &Scene) -> Result<(&Gpu, &Compositor)> {
        self.gpu()?;
        let gpu = self.gpu.as_ref().expect("gpu");
        if let Some(i) = self.comps.iter().position(|(s, _)| *s == size) {
            let entry = self.comps.remove(i);
            self.comps.push(entry);
        } else {
            let comp = Compositor::new_sized(gpu, size.0, size.1).context("creating the compositor")?;
            if self.comps.len() >= MAX_COMPOSITORS {
                self.comps.remove(0);
            }
            self.comps.push((size, comp));
        }
        let comp = &self.comps.last().expect("just pushed").1;
        comp.set_live_params(live_params_from_scene(scene));
        comp.set_scene(Some(scene.clone()));
        Ok((gpu, comp))
    }

    fn thumbs(&mut self, scene_json: &Value, out_fps: u32, frames: &[u64], max_width: u32) -> Result<()> {
        let scene = parse_scene(scene_json)?;
        let clips = clip_sources(&scene);
        let cfg = export_cfg(&scene);
        let size = (even(scene.output.width), even(scene.output.height));
        let (gpu, comp) = self.compositor(size, &scene)?;
        // SAFETY: gpu and comp are this thread's own objects, alive for the call.
        let rendered = unsafe { timeline_walk::render_frames(&clips, gpu, comp, &cfg, out_fps.max(1) as i32, frames) }?;
        for frame in rendered {
            let index = frame.index;
            let (_, _, jpeg) = frame.to_jpeg(max_width)?;
            send_frame(&self.out, TAG_THUMB, &encode_thumb(index, &jpeg))?;
        }
        Ok(())
    }

    fn export(&mut self, scene_json: &Value, kind: &ExportKind, out_path: &str, control: &ExportControl) -> Result<()> {
        control.check()?;
        let scene = parse_scene(scene_json)?;
        let clips = clip_sources(&scene);
        let cfg = export_cfg(&scene);
        let (w, h, fps) = match kind {
            ExportKind::Mp4 { w, h, fps, .. } | ExportKind::Gif { w, h, fps, .. } => (*w, *h, *fps),
        };
        if fps == 0 {
            bail!("export fps must be positive");
        }
        let size = export_size((scene.output.width, scene.output.height), (w, h)).map_err(|e| anyhow!(e))?;
        let total = expected_frames(&scene, fps);
        let out = self.out.clone();
        let mut last_sent: Option<Instant> = None;
        let write_failed = std::cell::Cell::new(false);
        let mut progress = |frames: u64| {
            let now = Instant::now();
            if frames <= 1 || last_sent.is_none_or(|t| now.duration_since(t) >= PROGRESS_EVERY) {
                last_sent = Some(now);
                if send_event(&out, &Event::ExportProgress { frames, total }).is_err() {
                    write_failed.set(true);
                }
            }
        };
        let (gpu, comp) = self.compositor(size, &scene)?;
        match kind {
            ExportKind::Mp4 { codec, .. } => {
                let params = ExportParams {
                    width: size.0,
                    height: size.1,
                    fps: Some(fps),
                    codec: match codec {
                        Codec::H264 => ExportCodec::H264,
                        Codec::Hevc => ExportCodec::H265,
                    },
                    bit_rate: None,
                };
                pipeline::run_composited_multi_cancellable(&clips, out_path, gpu, comp, &cfg, &params, &mut progress, control)?;
            }
            ExportKind::Gif { loop_forever, .. } => {
                let params = GifExportParams {
                    width: Some(size.0),
                    height: Some(size.1),
                    fps: Some(fps),
                    loop_count: if *loop_forever { None } else { Some(1) },
                    dither: true,
                };
                export_gif_cancellable(&clips, Path::new(out_path), gpu, comp, &cfg, &params, &mut progress, control)?;
            }
        }
        if write_failed.get() {
            bail!("the app stopped listening during the export");
        }
        Ok(())
    }
}

fn parse_scene(scene: &Value) -> Result<Scene> {
    Scene::from_json(&scene.to_string()).context("invalid scene")
}

/// The scene's clips as the export walk wants them. The crate has no such conversion
/// (upstream did it in its napi layer); `extra_audio` (the mic) must travel with each clip.
fn clip_sources(scene: &Scene) -> Vec<ClipSource> {
    scene
        .clips
        .iter()
        .map(|c| ClipSource {
            screen: c.screen_path.clone(),
            webcam: c.webcam_path.clone(),
            source_start_sec: c.source_start_sec,
            source_end_sec: c.source_end_sec,
            webcam_offset_sec: c.webcam_offset_sec,
            has_audio: c.has_audio,
            extra_audio: c.extra_audio.clone(),
        })
        .collect()
}

/// Upstream's export config (napi `export_cfg_and_scene`): C8 with a static layout, plus what
/// the scene sets.
fn export_cfg(scene: &Scene) -> Cfg {
    let mut cfg = config::all().pop().expect("at least one config");
    cfg.zoom = false;
    cfg.layout_anim = false;
    cfg.mblur_n = 1;
    cfg.bg_blur = scene.effects.blur;
    cfg.cursor = scene.cursor.show;
    cfg
}

/// Even, at least 2 (NV12 4:2:0).
fn even(v: u32) -> u32 {
    v.max(2) & !1
}

/// The export's pixel size. Its SHAPE belongs to the scene (`scene.output`, which the preview
/// is sized from): another shape would lay the frame out on a different canvas and the file
/// would disagree with the preview, so it is refused (port of upstream's
/// `resolve_output_size`; 3 % absorbs even-pixel snapping on small GIF sizes).
fn export_size(output: (u32, u32), want: (u32, u32)) -> Result<(u32, u32), String> {
    let (ow, oh) = (output.0.max(1) as u64, output.1.max(1) as u64);
    let (w, h) = (even(want.0), even(want.1));
    if (w as u64 * oh).abs_diff(h as u64 * ow) * 100 > 3 * h as u64 * ow {
        return Err(format!("export size {w}x{h} does not match the scene's output frame {ow}x{oh}"));
    }
    Ok((w, h))
}

/// Frames the export walk will produce at `fps`: the same speed segments it builds, clip by
/// clip (regions filtered like `Scene::for_clip_window`). An estimate only in that a clip
/// longer than its file is shortened while exporting.
fn expected_frames(scene: &Scene, fps: u32) -> u64 {
    let mut programme_sec = 0.0;
    let mut total = 0;
    for (i, clip) in scene.clips.iter().enumerate() {
        let regions: Vec<SceneSpeedRegion> = scene
            .speed_regions
            .iter()
            .filter(|r| {
                r.clip_index
                    .map(|c| c == i)
                    .unwrap_or(r.end_sec > clip.source_start_sec && r.start_sec < clip.source_end_sec)
            })
            .cloned()
            .collect();
        let segments =
            speed_segments_for_window(&regions, clip.source_start_sec, clip.source_end_sec, programme_sec, fps as f64);
        if let Some(last) = segments.last() {
            programme_sec = last.output_end_sec();
        }
        total += segments.iter().map(|s| s.frame_count).sum::<u64>();
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ruling R9: the app's golden scene (built by `record::scene_json`) is a valid compositor
    /// `Scene` — both sides of the contract agree. Paths are placeholders; this parses only.
    #[test]
    fn golden_scene_fixture_parses_into_the_compositor_scene() {
        let json = include_str!("../../core/rust-lib/src/record/fixtures/scene-golden.json");
        let scene = Scene::from_json(json).expect("golden fixture parses");
        assert_eq!(scene.clips.len(), 2);
        assert_eq!((scene.output.width, scene.output.height), (1920, 1080));
        assert_eq!(scene.speed_regions.len(), 2);
        assert_eq!(scene.crop_by_clip.len(), 2);
        let clips = clip_sources(&scene);
        let mic = clips[1].extra_audio.as_ref().expect("the mic travels with the clip");
        assert_eq!(mic.path, "<PROJECT>/mic.wav");
        assert!((mic.offset_sec - 0.12).abs() < 1e-9 && (mic.gain_db + 6.0).abs() < 1e-9);
        assert_eq!(clips[1].source_start_sec, 6.5);
        assert!(clips.iter().all(|c| c.has_audio));
    }

    #[test]
    fn expected_frames_follows_speed() {
        let json = include_str!("../../core/rust-lib/src/record/fixtures/scene-golden.json");
        let scene = Scene::from_json(json).unwrap();
        // Clip 0: 0–4 s with 1–3 s at ×2 → 3 s. Clip 1: 6.5–10 s with 7–9.25 s at ×0.5 → 5.75 s.
        let frames = expected_frames(&scene, 30);
        assert!((frames as i64 - (8.75 * 30.0) as i64).abs() <= 1, "{frames}");
    }

    #[test]
    fn export_size_keeps_the_scene_shape() {
        assert_eq!(export_size((1920, 1080), (1920, 1080)), Ok((1920, 1080)));
        assert_eq!(export_size((1920, 1080), (1280, 720)), Ok((1280, 720)));
        // 480 × 270 is the 16:9 GIF size; odd input is snapped to even.
        assert_eq!(export_size((1920, 1080), (481, 271)), Ok((480, 270)));
        assert!(export_size((1080, 1920), (1920, 1080)).is_err());
        assert!(export_size((1920, 1080), (1080, 1080)).is_err());
    }
}
