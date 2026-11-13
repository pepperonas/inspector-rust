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
use openscreen_compositor::audio::{audio_file_duration_sec, decode_clip_audio_unity, AUDIO_OUTPUT_SAMPLE_RATE};
use openscreen_compositor::audio_jobs::ClipAudioLevels;
use openscreen_compositor::compositor::Compositor;
use openscreen_compositor::config::{self, Cfg};
use openscreen_compositor::d3d::Gpu;
use openscreen_compositor::export_control::{ExportCancelled, ExportControl};
use openscreen_compositor::frame_geometry::live_params_from_scene;
use openscreen_compositor::gif_export::{export_gif_cancellable, GifExportParams};
use openscreen_compositor::live::LiveView;
use openscreen_compositor::pipeline::{self, ClipSource, ExportCodec, ExportParams};
use openscreen_compositor::regions::speed_segments_for_window;
use openscreen_compositor::scene::{Scene, SceneBackground, SceneSpeedRegion};
use openscreen_compositor::timeline_walk;
use record_render_protocol::peaks::{encode_peaks, peak_block_count, PeakBuilder};
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
/// `ExportFailed.msg` for an `Export` sent while another one is queued or running.
pub(crate) const EXPORT_BUSY: &str = "export already running";
/// Progress events at most this often (plus the first frame).
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

/// The protocol channel. Cloned into the reader thread for its own `Error` replies.
type Out = Arc<Mutex<Box<dyn Write + Send>>>;

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
        Ok(f) => Arc::new(Mutex::new(Box::new(f) as Box<dyn Write + Send>)),
        Err(e) => {
            eprintln!("ir-record-render: cannot set up the protocol channel: {e}");
            return EXIT_FATAL;
        }
    };
    let exports = ExportSlot::default();
    let (tx, rx) = mpsc::channel::<Incoming>();
    let peaks = spawn_peaks_worker(out.clone());
    {
        let out = out.clone();
        let exports = exports.clone();
        std::thread::Builder::new()
            .name("ir-render-stdin".into())
            .spawn(move || read_commands(io::stdin().lock(), tx, out, exports, peaks))
            .expect("spawn stdin reader");
    }

    let mut state = State::new(out.clone(), exports);
    if let Some(model) = state.model.clone() {
        spawn_warm_up(model);
    }
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
    /// Registers export `id`, or `None` while another one is queued or running: there is ONE
    /// export at a time, and the running one stays the one `Cancel`/EOF reach.
    fn begin(&self, id: u64) -> Option<ExportControl> {
        let mut slot = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if slot.is_some() {
            return None;
        }
        let control = ExportControl::default();
        *slot = Some((id, control.clone()));
        Some(control)
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

/// One `Peaks` request for the decode thread.
struct PeaksJob {
    id: u64,
    path: String,
    cache: String,
}

/// The decode thread: one `Peaks` at a time, off the render thread (a long decode must never
/// freeze the preview or hold back edits). A failure or a panic answers `PeaksFailed`; it
/// never ends the helper.
fn spawn_peaks_worker(out: Out) -> mpsc::Sender<PeaksJob> {
    let (tx, rx) = mpsc::channel::<PeaksJob>();
    std::thread::Builder::new()
        .name("ir-render-peaks".into())
        .spawn(move || {
            while let Ok(job) = rx.recv() {
                let ev = peaks_event(&job);
                if send_event(&out, &ev).is_err() {
                    return;
                }
            }
        })
        .expect("spawn peaks worker");
    tx
}

fn peaks_event(job: &PeaksJob) -> Event {
    let result = catch_unwind(AssertUnwindSafe(|| peaks_into_cache(&job.path, Path::new(&job.cache))));
    match result {
        Ok(Ok((blocks, duration_ms))) => Event::Peaks { id: job.id, blocks, duration_ms },
        Ok(Err(e)) => Event::PeaksFailed { id: job.id, msg: format!("{e:#}") },
        Err(panic) => Event::PeaksFailed { id: job.id, msg: format!("decoder crashed: {}", panic_message(&*panic)) },
    }
}

fn read_commands(
    mut input: impl io::Read,
    tx: mpsc::Sender<Incoming>,
    out: Out,
    exports: ExportSlot,
    peaks: mpsc::Sender<PeaksJob>,
) {
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
            Command::Peaks { id, path, cache } => {
                let job = PeaksJob { id: *id, path: path.clone(), cache: cache.clone() };
                if peaks.send(job).is_err() {
                    let _ = send_event(&out, &Event::PeaksFailed { id: *id, msg: "peaks worker gone".into() });
                }
                continue;
            }
            Command::Shutdown => {
                exports.cancel();
                None
            }
            Command::Export { .. } => {
                next_export += 1;
                match exports.begin(next_export) {
                    Some(control) => Some(control),
                    None => {
                        let _ = send_event(
                            &out,
                            &Event::ExportFailed { msg: EXPORT_BUSY.to_string(), cancelled: false },
                        );
                        continue;
                    }
                }
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
    /// The segmentation model (`segmentation_model`), resolved once at start.
    model: Option<std::path::PathBuf>,
    export_id: u64,
    live: Option<LiveView>,
    /// The preview's scene (for `SetActiveClip`, which needs the clip's file).
    scene: Option<Scene>,
    /// The same scene as sent, re-pushed when the panel is resized (`preview_scene` depends on
    /// the panel size).
    scene_json: Option<Value>,
    /// The preview panel's size (`Open`, `Resize`).
    preview: (u32, u32),
    gpu: Option<Gpu>,
    /// Compositors by output size, most recently used last.
    comps: Vec<((u32, u32), Compositor)>,
}

impl State {
    fn new(out: Out, exports: ExportSlot) -> Self {
        Self { out, exports, model: segmentation_model(), export_id: 0, live: None, scene: None, scene_json: None, preview: (0, 0), gpu: None, comps: Vec::new() }
    }

    fn event(&self, ev: Event) -> io::Result<()> {
        send_event(&self.out, &ev)
    }

    fn error(&self, msg: impl Into<String>) -> io::Result<()> {
        self.event(Event::Error { msg: msg.into() })
    }

    /// Runs one command. `Err` only when the output channel is broken.
    fn handle(&mut self, mut incoming: Incoming) -> io::Result<()> {
        // Every scene names the helper's own model before it reaches the compositor, whatever
        // it arrived with (see `fill_model_path`).
        if let Command::Open { scene, .. } | Command::SetScene { scene } | Command::Thumbs { scene, .. } | Command::Export { scene, .. } =
            &mut incoming.cmd
        {
            fill_model_path(scene, self.model.as_deref());
        }
        match incoming.cmd {
            Command::Open { scene, preview_w, preview_h } => match self.open(&scene, preview_w, preview_h) {
                Ok(()) => self.event(Event::Ready {
                    segmentation_model: self.model.as_ref().map(|m| m.to_string_lossy().into_owned()),
                }),
                Err(e) => self.error(format!("{e:#}")),
            },
            Command::SetScene { scene } => match parse_scene(&scene) {
                Ok(parsed) => {
                    if let Some(live) = &self.live {
                        push_scene(live, &scene, self.preview);
                    }
                    self.scene = Some(parsed);
                    self.scene_json = Some(scene);
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
            Command::Resize { w, h } => {
                self.preview = (w, h);
                let scene = self.scene_json.as_ref();
                self.with_live(|l| {
                    l.set_rect(w, h);
                    // Blur sizes are in output px: a new panel size changes the preview's share.
                    if let Some(scene) = scene.filter(|s| has_blur(s)) {
                        push_scene(l, scene, (w, h));
                    }
                })
            }
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
                    Err(e) if e.is::<io::Error>() => Err(e.downcast::<io::Error>().expect("checked")),
                    Err(e) => {
                        let cancelled = e.is::<ExportCancelled>();
                        let msg = if cancelled { "export cancelled".to_string() } else { format!("{e:#}") };
                        self.event(Event::ExportFailed { msg, cancelled })
                    }
                }
            }
            // Handled by the reader thread (Peaks by its decode thread); never queued.
            Command::Cancel | Command::Peaks { .. } => Ok(()),
            Command::Shutdown => {
                // May block if the preview thread is wedged; the app kills the helper after a timeout (Task 5).
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
        // The clip's own telemetry; `None` lets the LiveView fall back to the sidecar next to
        // the video, as the export does (`scene::cursor_sidecar`).
        let cursor = first.cursor_path.as_deref().unwrap_or("");
        let live = LiveView::create(w.max(1), h.max(1), &first.screen_path, &first.webcam_path, cursor)
            .context("starting the preview")?;
        push_scene(&live, scene_json, (w, h));
        live.set_playing(false);
        live.set_active_clip(&first.screen_path, &first.webcam_path, first.webcam_offset_sec, 0, first.source_start_sec);
        self.live = Some(live);
        self.scene = Some(scene);
        self.scene_json = Some(scene_json.clone());
        self.preview = (w, h);
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
                    // Nobody is listening: stop rendering, the staged file is removed and
                    // nothing is published.
                    write_failed.set(true);
                    control.cancel();
                }
            }
        };
        let (gpu, comp) = self.compositor(size, &scene)?;
        let result: Result<()> = match kind {
            ExportKind::Mp4 { codec, bit_rate, .. } => {
                let params = ExportParams {
                    width: size.0,
                    height: size.1,
                    fps: Some(fps),
                    codec: match codec {
                        Codec::H264 => ExportCodec::H264,
                        Codec::Hevc => ExportCodec::H265,
                    },
                    bit_rate: bit_rate.and_then(|b| i64::try_from(b).ok()),
                };
                pipeline::run_composited_multi_cancellable(&clips, out_path, gpu, comp, &cfg, &params, &mut progress, control).map(|_| ())
            }
            ExportKind::Gif { loop_forever, .. } => {
                let params = GifExportParams {
                    width: Some(size.0),
                    height: Some(size.1),
                    fps: Some(fps),
                    loop_count: if *loop_forever { None } else { Some(1) },
                    dither: true,
                };
                export_gif_cancellable(&clips, Path::new(out_path), gpu, comp, &cfg, &params, &mut progress, control).map(|_| ())
            }
        };
        if write_failed.get() {
            // An io::Error: `handle` treats it as a dead channel and the helper exits.
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "the app stopped listening during the export").into());
        }
        result
    }
}

// ── the preview's effect values ──────────────────────────────────────────────

/// Effect values the LiveView keeps apart from its scene (`set_param_*`). Its render thread
/// builds the compositor's `LiveParams` from them — not from the scene — so without these
/// the preview showed the LiveView's defaults while the export drew the scene's values.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct LiveParamSet {
    pub nums: Vec<(&'static str, f64)>,
    pub bools: Vec<(&'static str, bool)>,
    pub strs: Vec<(&'static str, String)>,
}

/// `LiveParams::default().bg_color` as `#rrggbb` (the colour the export falls back to).
fn default_background_hex(s: &Scene) -> String {
    let c = live_params_from_scene(s).bg_color;
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}

/// The LiveView parameters that give the preview the values the export takes from `scene`
/// (`frame_geometry::live_params_from_scene`, the export `Cfg` and its cursor smoothing).
/// Keys and units are those of `LiveView::set_param_*`. An unparsable scene gives nothing.
pub(crate) fn live_params(scene: &Value) -> LiveParamSet {
    let Ok(s) = parse_scene(scene) else {
        return LiveParamSet::default();
    };
    let f = |v: f32| v as f64;
    let mut strs = vec![("webcamShape", s.layout.webcam_shape.clone())];
    // The compositor falls back to this colour for an unreadable gradient stop or a corrupt
    // image, in the export (`live_params_from_scene`) and in the LiveView. A colour
    // background pushes its own; any other kind pushes the compositor's default, so a stale
    // colour from an earlier colour background never lingers.
    let bg = match &s.background {
        SceneBackground::Color { color } => color.clone(),
        _ => default_background_hex(&s),
    };
    strs.push(("backgroundColor", bg));
    LiveParamSet {
        nums: vec![
            ("shadow", f(s.effects.shadow)),
            // The scene's corner radius reaches the compositor as `effects.roundnessFrac`
            // (read in compose_frame) in both paths; `live_params_from_scene` leaves
            // `radius_scale` at its default 1.0, so the LiveView's multiplier must be 1 too —
            // anything else would scale a radius the export does not scale.
            ("roundness", 1.0),
            ("motionBlur", f(s.effects.motion_blur)),
            ("padding", f(s.effects.padding)),
            ("backgroundBlur", f(s.effects.blur)),
            ("cursorSize", f(s.cursor.size)),
            ("cursorClickBounce", f(s.cursor.click_bounce)),
            ("cursorSmoothing", f(s.cursor.smoothing)),
            ("cursorMotionBlur", f(s.cursor.motion_blur)),
            ("webcamSize", f(s.layout.webcam_size)),
        ],
        bools: vec![
            ("cursorShow", s.cursor.show),
            ("cursorAutoHide", s.cursor.auto_hide),
            ("cursorModel3d", s.cursor.model3d),
            ("webcamMirror", s.layout.webcam_mirror),
        ],
        strs,
    }
}

/// What the session hands a preview. A trait so the "every scene push re-applies every
/// effect value" rule is testable without a GPU.
pub(crate) trait PreviewTarget {
    fn set_scene(&self, json: &str);
    fn set_param_num(&self, key: &str, value: f64);
    fn set_param_bool(&self, key: &str, value: bool);
    fn set_param_str(&self, key: &str, value: &str);
}

impl PreviewTarget for LiveView {
    fn set_scene(&self, json: &str) {
        LiveView::set_scene(self, json)
    }
    fn set_param_num(&self, key: &str, value: f64) {
        LiveView::set_param_num(self, key, value)
    }
    fn set_param_bool(&self, key: &str, value: bool) {
        LiveView::set_param_bool(self, key, value)
    }
    fn set_param_str(&self, key: &str, value: &str) {
        LiveView::set_param_str(self, key, value)
    }
}

/// The ONLY way a scene reaches the preview (Open, SetScene, Resize, a restarted helper's
/// Open): the scene as the preview draws it (`preview_scene`), then every effect value —
/// after, because `set_scene` itself resets the cursor switches.
pub(crate) fn push_scene(view: &impl PreviewTarget, scene: &Value, preview: (u32, u32)) {
    view.set_scene(&preview_scene(scene, preview).to_string());
    let p = live_params(scene);
    for (k, v) in &p.nums {
        view.set_param_num(k, *v);
    }
    for (k, v) in &p.bools {
        view.set_param_bool(k, *v);
    }
    for (k, v) in &p.strs {
        view.set_param_str(k, v);
    }
}

/// Whether the scene has a blur annotation (the only part `preview_scene` changes).
fn has_blur(scene: &Value) -> bool {
    scene["annotations"].as_array().is_some_and(|a| a.iter().any(|a| a["kind"] == "blur"))
}

/// The scene as the preview draws it. A blur annotation's `intensity` and `blockSize` are
/// output pixels, and the compositor applies them as pixels of whatever it renders — the
/// preview renders smaller than the export (`live.rs` `preview_render_size`: the output,
/// contained in the panel, never larger), so the same 12-px mosaic would look twice as coarse
/// in a half-size preview. Scaled by that ratio here, the preview shows the export's blur.
/// Everything else in the scene is relative already.
pub(crate) fn preview_scene(scene: &Value, (pw, ph): (u32, u32)) -> Value {
    let (ow, oh) = (scene["output"]["width"].as_f64().unwrap_or(0.0), scene["output"]["height"].as_f64().unwrap_or(0.0));
    if !has_blur(scene) || ow <= 0.0 || oh <= 0.0 || pw == 0 || ph == 0 {
        return scene.clone();
    }
    let k = (pw.max(2) as f64 / ow).min(ph.max(2) as f64 / oh).min(1.0);
    let mut out = scene.clone();
    for a in out["annotations"].as_array_mut().into_iter().flatten() {
        if a["kind"] != "blur" {
            continue;
        }
        for key in ["intensity", "blockSize"] {
            if let Some(v) = a["blur"][key].as_f64() {
                a["blur"][key] = serde_json::json!(v * k);
            }
        }
    }
    out
}

/// Environment variable naming the segmentation model (tests, dev runs outside a bundle).
pub(crate) const MODEL_ENV: &str = "IR_SEGMENTATION_MODEL";
/// The model's file name in the bundle's `Contents/Resources/`.
pub(crate) const MODEL_FILE: &str = "selfie_segmentation_landscape.onnx";

/// Where the segmentation model is: `IR_SEGMENTATION_MODEL` when set (a set variable that names
/// no file means "no model", never the bundle's), else `Contents/Resources/` next to the
/// helper's `Contents/MacOS/`. Only a regular file counts.
fn resolve_model(env: Option<std::ffi::OsString>, exe: Option<&Path>) -> Option<std::path::PathBuf> {
    let path = match env.filter(|v| !v.is_empty()) {
        Some(v) => std::path::PathBuf::from(v),
        None => exe?.parent()?.join("../Resources").join(MODEL_FILE),
    };
    path.is_file().then_some(path)
}

/// The model this helper can run, if any: the runtime is linked in (`segmentation` feature)
/// and the file is there. Loading it stays lazy (the compositor loads it on the first frame
/// with an effect).
fn segmentation_model() -> Option<std::path::PathBuf> {
    if !openscreen_compositor::segmentation::runtime_available() {
        return None;
    }
    let exe = std::env::current_exe().ok();
    resolve_model(std::env::var_os(MODEL_ENV), exe.as_deref())
}

/// Loads the model and runs one inference on a blank 256×144 frame: pays ONNX Runtime's
/// one-time costs (environment, file cache, first-run allocation) so the compositor's own lazy
/// load on the first frame with an effect is fast. Returns (load, first inference) times.
fn warm_up(model: &Path) -> Result<(Duration, Duration)> {
    use openscreen_compositor::segmentation::{Segmenter, MODEL_HEIGHT, MODEL_WIDTH};
    let t0 = Instant::now();
    let mut seg = Segmenter::load(model)?;
    let loaded = t0.elapsed();
    let frame = vec![0u8; (MODEL_WIDTH * MODEL_HEIGHT * 3) as usize];
    let t1 = Instant::now();
    seg.run(&frame)?;
    Ok((loaded, t1.elapsed()))
}

/// [`warm_up`] on its own thread: never the render thread, never in the way of `Open`/`Ready`.
/// One log line; a failure is only logged — the compositor's own load still decides.
fn spawn_warm_up(model: std::path::PathBuf) {
    let _ = std::thread::Builder::new().name("ir-render-seg-warmup".into()).spawn(move || {
        match catch_unwind(AssertUnwindSafe(|| warm_up(&model))) {
            Ok(Ok((load, run))) => eprintln!(
                "ir-record-render: segmentation warmed up (load {} ms, first inference {} ms)",
                load.as_millis(),
                run.as_millis()
            ),
            Ok(Err(e)) => eprintln!("ir-record-render: segmentation warm-up skipped: {e:#}"),
            Err(_) => eprintln!("ir-record-render: segmentation warm-up panicked"),
        }
    });
}

/// Sets a scene effect's `modelPath` to this helper's own model, or removes it when the helper
/// has none. A path that arrives in a scene is never honoured: the compositor would load
/// whatever file it names. Only the model `segmentation_model` resolved may reach it.
fn fill_model_path(scene: &mut Value, model: Option<&Path>) {
    let Some(effect) = scene.get_mut("webcamEffect").and_then(Value::as_object_mut) else {
        return;
    };
    match model {
        Some(m) => {
            effect.insert("modelPath".into(), Value::String(m.to_string_lossy().into_owned()));
        }
        None => {
            effect.remove("modelPath");
        }
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
            cursor_path: c.cursor_path.clone(),
            audio_levels: ClipAudioLevels { normalise: c.normalise, system_gain_db: c.system_gain_db },
        })
        .collect()
}

/// One decode window of [`peaks_of`]: a minute of PCM at a time, whatever the file's length.
const PEAKS_WINDOW_SEC: f64 = 60.0;

/// The waveform of `path` (`record_render_protocol::peaks`): the unity decode (a mono file
/// keeps its level, stereo is averaged) through `PeakBuilder` (16 kHz, `[min, max]` blocks).
/// Returns the peaks and the audio's length in ms (from the container).
fn peaks_of(path: &str) -> Result<(Vec<f32>, u64)> {
    let duration = audio_file_duration_sec(path).ok_or_else(|| anyhow!("{path}: no readable duration"))?;
    let mut builder = PeakBuilder::new(duration, AUDIO_OUTPUT_SAMPLE_RATE as u32);
    let mut start = 0.0;
    while start < duration {
        let end = (start + PEAKS_WINDOW_SEC).min(duration);
        let pcm = decode_clip_audio_unity(path, start, end)?.ok_or_else(|| anyhow!("{path}: no audio stream"))?;
        let len = pcm.first().map_or(0, Vec::len);
        for i in 0..len {
            builder.push(pcm.iter().map(|p| p.get(i).copied().unwrap_or(0.0)).sum::<f32>() / pcm.len() as f32);
        }
        start = end;
    }
    Ok((builder.finish(), (duration * 1000.0).round() as u64))
}

/// [`peaks_of`] through the cache file: one of the right size is reused, otherwise the peaks
/// are written next to it and renamed into place (a half-written cache is never read).
/// A peaks cache is reused only when it is a plain file (never a planted link — not even to a
/// file of the right length) holding exactly `blocks` min/max pairs.
fn cache_reusable(cache: &Path, blocks: u32) -> bool {
    std::fs::symlink_metadata(cache).is_ok_and(|m| m.file_type().is_file() && m.len() == blocks as u64 * 8)
}

fn peaks_into_cache(path: &str, cache: &Path) -> Result<(u32, u64)> {
    if let Some(duration) = audio_file_duration_sec(path) {
        let blocks = peak_block_count(duration);
        if cache_reusable(cache, blocks) {
            return Ok((blocks, (duration * 1000.0).round() as u64));
        }
    }
    let (peaks, duration_ms) = peaks_of(path)?;
    // The cache lives in the project folder: never written through a planted link.
    record_render_protocol::safe_fs::write_atomic(cache, &encode_peaks(&peaks))
        .with_context(|| format!("writing {}", cache.display()))?;
    Ok(((peaks.len() / 2) as u32, duration_ms))
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

    #[test]
    fn the_model_is_found_from_the_env_or_next_to_the_helper() {
        use super::{resolve_model, MODEL_FILE};
        let d = std::env::temp_dir().join(format!("ir-render-model-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("App.app/Contents/MacOS")).unwrap();
        std::fs::create_dir_all(d.join("App.app/Contents/Resources")).unwrap();
        let exe = d.join("App.app/Contents/MacOS/ir-record-render");
        let bundled = d.join("App.app/Contents/Resources").join(MODEL_FILE);
        let other = d.join("other.onnx");
        std::fs::write(&other, b"x").unwrap();
        // Nothing in the bundle, no variable: no model.
        assert_eq!(resolve_model(None, Some(&exe)), None);
        std::fs::write(&bundled, b"x").unwrap();
        assert_eq!(resolve_model(None, Some(&exe)), Some(d.join("App.app/Contents/MacOS/../Resources").join(MODEL_FILE)));
        // The variable wins, and an empty one counts as unset.
        assert_eq!(resolve_model(Some(other.clone().into()), Some(&exe)), Some(other.clone()));
        assert!(resolve_model(Some("".into()), Some(&exe)).is_some(), "an empty variable is unset");
        // A variable naming a missing file is not silently replaced by the bundle's model.
        assert_eq!(resolve_model(Some(d.join("nope.onnx").into()), Some(&exe)), None);
        // A directory is not a model.
        assert_eq!(resolve_model(Some(d.clone().into()), None), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The warm-up loads the shipped model and runs it; a missing model is an error the caller
    /// only logs. Prints the cold and the warm times (the warm ones are what the compositor's
    /// lazy load pays after a warm-up).
    #[test]
    fn warm_up_loads_and_runs_the_model() {
        let model = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../vendor/selfie-segmentation/selfie_segmentation_landscape.onnx");
        let (load, run) = super::warm_up(&model).expect("warm-up on the shipped model");
        let (load2, run2) = super::warm_up(&model).expect("second load");
        eprintln!("cold: load {load:?} + run {run:?}; after warm-up: load {load2:?} + run {run2:?}");
        assert!(super::warm_up(std::path::Path::new("/nonexistent/m.onnx")).is_err());
    }

    /// The compositor loads whatever `modelPath` names, so only the helper's own, resolved
    /// model may reach it: a path that arrives in a scene (from the app or anyone writing to
    /// the pipe) is replaced, and without a model of its own the helper removes it.
    #[test]
    fn scene_effect_carries_model_path() {
        use super::fill_model_path;
        use serde_json::json;
        let m = std::path::Path::new("/R/selfie.onnx");
        let mut plain = json!({"clips": []});
        fill_model_path(&mut plain, Some(m));
        assert_eq!(plain, json!({"clips": []}), "no effect, nothing added");
        for sent in [json!({"mode": "blur"}), json!({"mode": "blur", "modelPath": null}), json!({"mode": "blur", "modelPath": ""})] {
            let mut s = json!({"webcamEffect": sent});
            fill_model_path(&mut s, Some(m));
            assert_eq!(s["webcamEffect"]["modelPath"], "/R/selfie.onnx");
        }
        // A foreign path is replaced, never honoured.
        let mut s = json!({"webcamEffect": {"mode": "blur", "modelPath": "/tmp/evil.onnx"}});
        fill_model_path(&mut s, Some(m));
        assert_eq!(s["webcamEffect"]["modelPath"], "/R/selfie.onnx");
        // Without a model of its own, any sent path is removed.
        let mut s = json!({"webcamEffect": {"mode": "blur", "modelPath": "/tmp/evil.onnx"}});
        fill_model_path(&mut s, None);
        assert!(s["webcamEffect"].get("modelPath").is_none(), "{s}");
        assert_eq!(s["webcamEffect"]["mode"], "blur", "the effect itself stays");
    }

    #[test]
    fn a_peaks_cache_is_reused_only_as_a_plain_file_of_the_right_size() {
        let d = std::env::temp_dir().join(format!("ir-render-peaks-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let good = d.join("good.peaks");
        std::fs::write(&good, vec![0u8; 16]).unwrap();
        assert!(cache_reusable(&good, 2));
        assert!(!cache_reusable(&good, 3), "wrong size");
        assert!(!cache_reusable(&d.join("missing.peaks"), 2));
        #[cfg(unix)]
        {
            let link = d.join("link.peaks");
            std::os::unix::fs::symlink(&good, &link).unwrap();
            assert!(!cache_reusable(&link, 2), "a link is never trusted, even to a fitting file");
        }
        let _ = std::fs::remove_dir_all(&d);
    }
    use super::*;
    use openscreen_compositor::frame_geometry::{webcam_shape_code, LiveParams};

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
        // Patch 14: the app marks the mic as the voice; the system level travels with it.
        assert_eq!(clips[1].audio_levels.normalise, openscreen_compositor::scene::AudioNormalise::Voice);
        assert_eq!(clips[1].audio_levels.system_gain_db, 0.0);
        assert!(clips.iter().all(|c| c.has_audio));
    }

    /// Stage 3: the styled golden (every effect, cursor sprites, zooms, click sound) parses too.
    #[test]
    fn styled_golden_scene_parses_into_the_compositor_scene() {
        use openscreen_compositor::scene::{SceneBackground, SceneFrame, WallpaperMotion};
        let json = include_str!("../../core/rust-lib/src/record/fixtures/scene-golden-styled.json");
        let scene = Scene::from_json(json).expect("styled golden parses");
        assert!(matches!(scene.effects.frame, SceneFrame::Window));
        assert!((scene.effects.padding - 0.4).abs() < 1e-6);
        match &scene.background {
            SceneBackground::Image { path, motion } => {
                assert!(path.ends_with("wallpapers/03.jpg"), "{path}");
                assert_eq!(*motion, WallpaperMotion::Drift);
            }
            _ => panic!("expected an image background"),
        }
        assert_eq!(scene.zoom_regions.len(), 2);
        assert_eq!(scene.zoom_regions[1].clip_index, Some(1));
        assert!(scene.zoom_regions[1].hide_cursor);
        assert!(scene.cursor.show && scene.cursor.auto_hide && scene.cursor.click_impact);
        assert_eq!(scene.cursor.cursor_sprites.len(), 16);
        assert!(!scene.click_sound.hits.is_empty() && !scene.click_sound.down_path.is_empty());
        assert!(scene.layout.screen_rect.is_some());
        assert_eq!(scene.layout.layout_by_clip.len(), 2);
        assert_eq!(scene.clips[0].cursor_path.as_deref(), Some("<PROJECT>/cursor.json"));
    }

    #[test]
    fn expected_frames_follows_speed() {
        let json = include_str!("../../core/rust-lib/src/record/fixtures/scene-golden.json");
        let scene = Scene::from_json(json).unwrap();
        // Clip 0: 0–4 s with 1–3 s at ×2 → 3 s. Clip 1: 6.5–10 s with 7–9.25 s at ×0.5 → 5.75 s.
        let frames = expected_frames(&scene, 30);
        assert!((frames as i64 - (8.75 * 30.0) as i64).abs() <= 1, "{frames}");
    }

    /// Output sink shared with the reader thread.
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Sink {
        fn events(&self) -> Vec<Event> {
            let bytes = self.0.lock().unwrap().clone();
            let mut r = io::Cursor::new(bytes);
            let mut out = Vec::new();
            while let Ok((tag, body)) = read_frame(&mut r) {
                assert_eq!(tag, record_render_protocol::TAG_EVENT);
                out.push(record_render_protocol::decode_event(&body).unwrap());
            }
            out
        }
    }

    /// stdin that serves chunks as the test hands them over and reports EOF once the test
    /// drops its sender — so the reader can be observed BEFORE end of input.
    struct Feed {
        rx: mpsc::Receiver<Vec<u8>>,
        buf: io::Cursor<Vec<u8>>,
    }
    impl io::Read for Feed {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            loop {
                let n = io::Read::read(&mut self.buf, out)?;
                if n > 0 {
                    return Ok(n);
                }
                match self.rx.recv() {
                    Ok(chunk) => self.buf = io::Cursor::new(chunk),
                    Err(_) => return Ok(0),
                }
            }
        }
    }

    fn encoded(cmd: &Command) -> Vec<u8> {
        let mut b = Vec::new();
        record_render_protocol::write_command(&mut b, cmd).unwrap();
        b
    }

    fn export_cmd(path: &str) -> Command {
        Command::Export {
            scene: serde_json::json!({}),
            kind: ExportKind::Gif { w: 2, h: 2, fps: 1, loop_forever: true },
            out_path: path.into(),
        }
    }

    /// Ruling 1 (fix round 1): one export at a time. A second `Export` while one runs is
    /// answered `ExportFailed{"export already running"}`, never queued, and the running export
    /// stays the one `Cancel` reaches.
    #[test]
    fn second_export_is_rejected_and_the_running_one_stays_cancellable() {
        let sink = Sink::default();
        let out: Out = Arc::new(Mutex::new(Box::new(sink.clone())));
        let slot = ExportSlot::default();
        let (feed_tx, feed_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        let (peaks_tx, _peaks_rx) = mpsc::channel();
        let reader = {
            let (out, slot) = (out.clone(), slot.clone());
            std::thread::spawn(move || {
                read_commands(Feed { rx: feed_rx, buf: io::Cursor::new(Vec::new()) }, tx, out, slot, peaks_tx)
            })
        };
        let t = Duration::from_secs(5);

        feed_tx.send(encoded(&export_cmd("/a.gif"))).unwrap();
        let first = rx.recv_timeout(t).expect("first export forwarded");
        assert!(matches!(&first.cmd, Command::Export { out_path, .. } if out_path == "/a.gif"));
        let running = first.control.expect("the export carries its control");

        feed_tx.send(encoded(&export_cmd("/b.gif"))).unwrap();
        feed_tx.send(encoded(&Command::SetPlaying { playing: true })).unwrap();
        // The command after the rejected one arrives; the rejected Export never does.
        let next = rx.recv_timeout(t).unwrap();
        assert!(matches!(next.cmd, Command::SetPlaying { playing: true }), "{:?}", next.cmd);
        assert_eq!(
            sink.events(),
            vec![Event::ExportFailed { msg: EXPORT_BUSY.into(), cancelled: false }]
        );
        assert!(running.check().is_ok(), "the rejected Export must not touch the running one");

        feed_tx.send(encoded(&Command::Cancel)).unwrap();
        feed_tx.send(encoded(&Command::SetPlaying { playing: false })).unwrap();
        rx.recv_timeout(t).unwrap();
        assert!(running.check().is_err(), "Cancel reaches the running export");

        // Once it is finished, the next Export is accepted again.
        slot.finish(1);
        feed_tx.send(encoded(&export_cmd("/c.gif"))).unwrap();
        let third = rx.recv_timeout(t).unwrap();
        assert!(matches!(&third.cmd, Command::Export { out_path, .. } if out_path == "/c.gif"));
        let third_control = third.control.unwrap();

        // End of input (the app is gone): the running export is cancelled, Shutdown queued.
        drop(feed_tx);
        assert!(matches!(rx.recv_timeout(t).unwrap().cmd, Command::Shutdown));
        assert!(third_control.check().is_err());
        reader.join().unwrap();
    }

    /// `Peaks` never reaches the render queue: the reader hands it to the decode thread, so a
    /// long decode cannot hold back frames or edits (review fix 2).
    #[test]
    fn peaks_go_to_the_decode_thread_not_the_render_queue() {
        let sink = Sink::default();
        let out: Out = Arc::new(Mutex::new(Box::new(sink.clone())));
        let (feed_tx, feed_rx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        let (peaks_tx, peaks_rx) = mpsc::channel::<PeaksJob>();
        let reader = std::thread::spawn(move || {
            read_commands(Feed { rx: feed_rx, buf: io::Cursor::new(Vec::new()) }, tx, out, ExportSlot::default(), peaks_tx)
        });
        let t = Duration::from_secs(5);
        feed_tx.send(encoded(&Command::Peaks { id: 7, path: "/a.wav".into(), cache: "/p.f32".into() })).unwrap();
        feed_tx.send(encoded(&Command::SetPlaying { playing: true })).unwrap();
        assert!(matches!(rx.recv_timeout(t).unwrap().cmd, Command::SetPlaying { playing: true }));
        let job = peaks_rx.recv_timeout(t).unwrap();
        assert_eq!((job.id, job.path.as_str(), job.cache.as_str()), (7, "/a.wav", "/p.f32"));
        drop(feed_tx);
        reader.join().unwrap();
    }

    /// The decode thread answers each job by id, a failure as `PeaksFailed` — and keeps going.
    #[test]
    fn peaks_worker_answers_by_id_and_survives_failures() {
        let dir = std::env::temp_dir().join(format!("ir-peaks-worker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("a.wav");
        write_sine_wav(&wav, 1.0, 0.5);
        let sink = Sink::default();
        let jobs = spawn_peaks_worker(Arc::new(Mutex::new(Box::new(sink.clone()))));
        jobs.send(PeaksJob { id: 1, path: "/no/such.wav".into(), cache: dir.join("x.f32").display().to_string() }).unwrap();
        jobs.send(PeaksJob { id: 2, path: wav.display().to_string(), cache: dir.join("a.f32").display().to_string() }).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while sink.events().len() < 2 {
            assert!(Instant::now() < deadline, "worker did not answer");
            std::thread::sleep(Duration::from_millis(10));
        }
        let ev = sink.events();
        assert!(matches!(&ev[0], Event::PeaksFailed { id: 1, .. }), "{:?}", ev[0]);
        assert!(matches!(ev[1], Event::Peaks { id: 2, blocks: 200, .. }), "{:?}", ev[1]);
        let _ = std::fs::remove_dir_all(&dir);
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

    // ── stage 3: the preview takes the export's effect values ───────────────

    fn styled() -> Value {
        serde_json::from_str(include_str!("../../core/rust-lib/src/record/fixtures/scene-golden-styled.json")).unwrap()
    }

    fn num(set: &LiveParamSet, key: &str) -> f64 {
        set.nums.iter().find(|(k, _)| *k == key).unwrap_or_else(|| panic!("no {key}")).1
    }
    fn flag(set: &LiveParamSet, key: &str) -> bool {
        set.bools.iter().find(|(k, _)| *k == key).unwrap_or_else(|| panic!("no {key}")).1
    }
    fn text<'a>(set: &'a LiveParamSet, key: &str) -> Option<&'a str> {
        set.strs.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str())
    }

    /// Every effect set away from its default arrives under the LiveView's key.
    #[test]
    fn live_params_map_every_effect() {
        let mut v = styled();
        v["effects"]["shadow"] = 0.6.into();
        v["effects"]["padding"] = 0.5.into();
        v["effects"]["blur"] = 0.7.into();
        v["effects"]["motionBlur"] = 0.4.into();
        v["effects"]["roundnessFrac"] = 0.04.into();
        v["cursor"]["size"] = 2.5.into();
        v["cursor"]["clickBounce"] = 1.8.into();
        v["cursor"]["smoothing"] = 0.45.into();
        v["cursor"]["motionBlur"] = 0.25.into();
        v["cursor"]["show"] = false.into();
        v["cursor"]["autoHide"] = true.into();
        v["cursor"]["model3d"] = true.into();
        v["layout"]["webcamSize"] = 1.4.into();
        v["layout"]["webcamMirror"] = true.into();
        v["layout"]["webcamShape"] = "circle".into();
        v["background"] = serde_json::json!({ "kind": "color", "color": "#123456" });
        let p = live_params(&v);
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
        assert!(close(num(&p, "shadow"), 0.6));
        assert!(close(num(&p, "padding"), 0.5));
        assert!(close(num(&p, "backgroundBlur"), 0.7));
        assert!(close(num(&p, "motionBlur"), 0.4));
        // The scene's corner radius acts through `roundnessFrac` in both paths; the export's
        // `radius_scale` stays at its default (frame_geometry::live_params_from_scene).
        assert!(close(num(&p, "roundness"), 1.0));
        assert!(close(num(&p, "cursorSize"), 2.5));
        assert!(close(num(&p, "cursorClickBounce"), 1.8));
        assert!(close(num(&p, "cursorSmoothing"), 0.45));
        assert!(close(num(&p, "cursorMotionBlur"), 0.25));
        assert!(close(num(&p, "webcamSize"), 1.4));
        assert!(!flag(&p, "cursorShow"));
        assert!(flag(&p, "cursorAutoHide"));
        assert!(flag(&p, "cursorModel3d"));
        assert!(flag(&p, "webcamMirror"));
        assert_eq!(text(&p, "backgroundColor"), Some("#123456"));
        assert_eq!(text(&p, "webcamShape"), Some("circle"));
        // Not a colour background: the compositor's default is pushed, replacing a stale colour.
        let binding = live_params(&styled());
        let d = text(&binding, "backgroundColor").expect("a default colour");
        assert!(d.starts_with('#') && d.len() == 7);
        let want = live_params_from_scene(&parse_scene(&styled()).unwrap()).bg_color;
        let q = |c: [f32; 4]| c.map(|v| (v * 255.0).round() as u8);
        assert_eq!(q(hex(d).unwrap())[..3], q(want)[..3]);
    }

    /// Preview equals export for the 3D cursor: a model3d scene pushes `cursorModel3d = true`,
    /// a flat one pushes `false` (so a stale value never lingers).
    #[test]
    fn a_model3d_scene_pushes_cursor_model3d_to_the_preview() {
        let mut v = styled();
        v["cursor"]["model3d"] = true.into();
        assert!(live_params(&v).bools.contains(&("cursorModel3d", true)));
        v["cursor"]["model3d"] = false.into();
        assert!(live_params(&v).bools.contains(&("cursorModel3d", false)));
    }

    /// The names the app's `scene_json::sculpt_name` / `glass_name` emit (SCULPTED_THEMES x
    /// SCULPTED_STATES, mirrored here because this crate does not link the core) are all known
    /// to the compositor; an unknown one would silently fall back to the extruded sprite.
    #[test]
    fn every_sculpt_name_is_known_to_the_compositor() {
        // The lists core emits, read from its source (this crate cannot link the core).
        let src = include_str!("../../core/rust-lib/src/record/scene_json.rs");
        let list = |name: &str| -> Vec<String> {
            let start = src.find(&format!("pub const {name}:")).unwrap_or_else(|| panic!("{name} not found"));
            let body = &src[start..];
            let body = &body[body.find("= [").expect("array") + 3..body.find("];").expect("end")];
            body.split(',').map(|t| t.trim().trim_matches('"').to_string()).filter(|t| !t.is_empty()).collect()
        };
        let (themes, states) = (list("SCULPTED_THEMES"), list("SCULPTED_STATES"));
        assert_eq!((themes.len(), states.len()), (6, 2), "{themes:?} {states:?}");
        for t in &themes {
            for st in &states {
                assert!(openscreen_compositor::sculpt::sculpted_shape(&format!("{t}/{st}")).is_some(), "{t}/{st}");
            }
        }
        assert!(openscreen_compositor::sculpt::sculpted_shape("studio-ink/text").is_none());
    }

    /// Test mirror of the LiveView's inspector state: `set_param_*` (live.rs:1131-1181) and
    /// the `InspectorParams → LiveParams` mapping of its render thread (live.rs:1797-1811).
    /// The compositor has no public way to read either back.
    struct Mirror {
        lp: LiveParams,
        bg_blur: f32,
        mblur_taps: u32,
        smoothing: f32,
        show: bool,
    }

    impl Mirror {
        fn after(scene: &Scene, set: &LiveParamSet) -> Self {
            let mut m = Mirror { lp: LiveParams::default(), bg_blur: 0.0, mblur_taps: 8, smoothing: 0.0, show: true };
            // `set_scene` itself copies these two.
            m.show = scene.cursor.show;
            m.lp.cursor_auto_hide = scene.cursor.auto_hide;
            for (k, v) in &set.nums {
                let f = *v as f32;
                match *k {
                    "shadow" => m.lp.shadow_scale = f.max(0.0),
                    "roundness" => m.lp.radius_scale = f.max(0.0),
                    "motionBlur" => m.mblur_taps = (1.0 + v.clamp(0.0, 1.0) * 15.0).round() as u32,
                    "padding" => m.lp.padding = f.clamp(0.0, 1.0),
                    "backgroundBlur" => m.bg_blur = f.clamp(0.0, 1.0),
                    "webcamSize" => m.lp.webcam_size_scale = f.max(0.05),
                    "cursorSize" => m.lp.cursor_size_scale = f.max(0.0),
                    "cursorClickBounce" => m.lp.cursor_bounce_scale = f.max(0.0),
                    "cursorSmoothing" => m.smoothing = f.clamp(0.0, 1.0),
                    "cursorMotionBlur" => m.lp.cursor_motion_blur = f.clamp(0.0, 1.0),
                    other => panic!("the LiveView has no number key {other}"),
                }
            }
            for (k, v) in &set.bools {
                match *k {
                    "webcamMirror" => m.lp.webcam_mirror = *v,
                    "cursorShow" => m.show = *v,
                    "cursorAutoHide" => m.lp.cursor_auto_hide = *v,
                    "cursorModel3d" => m.lp.cursor_model3d = *v,
                    other => panic!("the LiveView has no bool key {other}"),
                }
            }
            for (k, v) in &set.strs {
                match *k {
                    "backgroundColor" => m.lp.bg_color = hex(v).expect("a hex colour"),
                    "webcamShape" => m.lp.webcam_shape = webcam_shape_code(v),
                    other => panic!("the LiveView has no string key {other}"),
                }
            }
            m
        }
    }

    fn hex(s: &str) -> Option<[f32; 4]> {
        let h = s.trim_start_matches('#');
        let c = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok().map(|b| b as f32 / 255.0);
        Some([c(0)?, c(2)?, c(4)?, 1.0])
    }

    /// The LiveView ends up with the values the export takes from the scene.
    #[test]
    fn live_params_match_export_params() {
        let mut colour = styled();
        colour["background"] = serde_json::json!({ "kind": "color", "color": "#203040" });
        colour["layout"]["webcamMirror"] = true.into();
        colour["layout"]["webcamShape"] = "square".into();
        colour["layout"]["webcamSize"] = 0.8.into();
        colour["cursor"]["model3d"] = true.into();
        let golden: Value =
            serde_json::from_str(include_str!("../../core/rust-lib/src/record/fixtures/scene-golden.json")).unwrap();
        for (name, v) in [("golden", golden), ("styled", styled()), ("colour", colour)] {
            let scene = parse_scene(&v).unwrap();
            let m = Mirror::after(&scene, &live_params(&v));
            let e = live_params_from_scene(&scene);
            let same = |a: f32, b: f32, what: &str| assert!((a - b).abs() < 1e-6, "{name}: {what} {a} vs {b}");
            same(m.lp.shadow_scale, e.shadow_scale, "shadow");
            same(m.lp.radius_scale, e.radius_scale, "radius");
            same(m.lp.padding, e.padding, "padding");
            same(m.lp.webcam_size_scale, e.webcam_size_scale, "webcam size");
            same(m.lp.cursor_size_scale, e.cursor_size_scale, "cursor size");
            same(m.lp.cursor_bounce_scale, e.cursor_bounce_scale, "click bounce");
            same(m.lp.cursor_motion_blur, e.cursor_motion_blur, "cursor motion blur");
            assert_eq!(m.lp.webcam_mirror, e.webcam_mirror, "{name}: mirror");
            assert_eq!(m.lp.webcam_shape, e.webcam_shape, "{name}: shape");
            assert_eq!(m.lp.cursor_model3d, e.cursor_model3d, "{name}: model3d");
            assert_eq!(m.lp.cursor_auto_hide, e.cursor_auto_hide, "{name}: auto hide");
            // Outside LiveParams: what the export's Cfg and cursor track take from the scene.
            assert_eq!(m.show, scene.cursor.show, "{name}: cursor show");
            same(m.smoothing, scene.cursor.smoothing.clamp(0.0, 1.0), "smoothing");
            same(m.bg_blur, scene.effects.blur.clamp(0.0, 1.0), "background blur");
            assert_eq!(m.mblur_taps, (1.0 + scene.effects.motion_blur.clamp(0.0, 1.0) * 15.0).round() as u32);
            // A colour background is drawn from the scene's own colour; the LiveView's fallback
            // colour must agree with it.
            if let SceneBackground::Color { color } = &scene.background {
                assert_eq!(Some(m.lp.bg_color), hex(color), "{name}: background colour");
            }
        }
    }

    /// Records what reaches the preview, in order.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<String>>);

    impl PreviewTarget for Recorder {
        fn set_scene(&self, _json: &str) {
            self.0.lock().unwrap().push("scene".into());
        }
        fn set_param_num(&self, key: &str, _v: f64) {
            self.0.lock().unwrap().push(key.into());
        }
        fn set_param_bool(&self, key: &str, _v: bool) {
            self.0.lock().unwrap().push(key.into());
        }
        fn set_param_str(&self, key: &str, _v: &str) {
            self.0.lock().unwrap().push(key.into());
        }
    }

    /// Every scene push is followed by every effect value — a second push re-applies all of
    /// them (an edit, a restarted helper's `Open`).
    #[test]
    fn params_reapplied_after_set_scene() {
        let rec = Recorder::default();
        let v = styled();
        let expected = live_params(&v);
        let keys = expected.nums.len() + expected.bools.len() + expected.strs.len();
        push_scene(&rec, &v, (960, 540));
        push_scene(&rec, &v, (960, 540));
        let calls = rec.0.lock().unwrap().clone();
        assert_eq!(calls.len(), 2 * (1 + keys), "{calls:?}");
        for half in calls.chunks(1 + keys) {
            assert_eq!(half[0], "scene", "params follow the scene, never precede it");
            for (k, _) in &expected.nums {
                assert!(half.contains(&k.to_string()), "{k} missing in {half:?}");
            }
            for (k, _) in &expected.bools {
                assert!(half.contains(&k.to_string()), "{k} missing in {half:?}");
            }
        }
        // Every place that hands the LiveView a scene goes through `push_scene`.
        let src = include_str!("session.rs");
        let code = &src[..src.find("#[cfg(test)]").unwrap()];
        assert_eq!(code.matches(".set_scene(&").count(), 1, "only push_scene calls LiveView::set_scene");
    }

    /// Blur sizes follow the preview's share of the output; the rest stays as sent.
    #[test]
    fn preview_scene_scales_blur_to_the_preview() {
        let mut v = styled();
        v["output"] = serde_json::json!({"width": 1920, "height": 1080, "fps": 30});
        v["annotations"] = serde_json::json!([
            {"kind": "blur", "blur": {"intensity": 12.0, "blockSize": 16.0, "style": "mosaic"}},
            {"kind": "text", "text": {"fontSizeRel": 0.03}},
        ]);
        let p = preview_scene(&v, (960, 600));
        assert_eq!(p["annotations"][0]["blur"]["intensity"], 6.0);
        assert_eq!(p["annotations"][0]["blur"]["blockSize"], 8.0);
        assert_eq!(p["annotations"][1], v["annotations"][1]);
        // Never larger than the output, and a scene without blur goes through untouched.
        assert_eq!(preview_scene(&v, (4000, 3000))["annotations"][0]["blur"]["intensity"], 12.0);
        let plain = styled();
        assert_eq!(preview_scene(&plain, (960, 540)), plain);
    }

    /// A mono 16-bit WAV of `secs` with a 440 Hz sine of `amp`, at 48 kHz.
    fn write_sine_wav(path: &Path, secs: f64, amp: f32) {
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
            let v = amp * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin();
            b.extend_from_slice(&((v * 32767.0).round() as i16).to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }

    /// `Peaks`: 200 blocks per second, the mono file's level kept, the cache reused.
    #[test]
    fn peaks_are_folded_cached_and_reused() {
        let dir = std::env::temp_dir().join(format!("ir-peaks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("a.wav");
        write_sine_wav(&wav, 2.5, 0.5);
        let cache = dir.join("peaks-a.f32");
        let _ = std::fs::remove_file(&cache);
        let (blocks, ms) = peaks_into_cache(wav.to_str().unwrap(), &cache).unwrap();
        assert_eq!(blocks, 500);
        assert!(ms.abs_diff(2500) <= 5, "{ms}");
        let peaks = record_render_protocol::peaks::decode_peaks(&std::fs::read(&cache).unwrap()).unwrap();
        assert_eq!(peaks.len(), 1000);
        let max = peaks.iter().fold(0.0f32, |m, v| m.max(*v));
        let min = peaks.iter().fold(0.0f32, |m, v| m.min(*v));
        // Averaging three 48 kHz samples of a 440 Hz sine loses well under 1 %.
        assert!((max - 0.5).abs() < 0.01 && (min + 0.5).abs() < 0.01, "{min}..{max}");
        // A cache of the right size is answered without decoding again.
        std::fs::write(&cache, vec![0u8; 8 * 500]).unwrap();
        assert_eq!(peaks_into_cache(wav.to_str().unwrap(), &cache).unwrap(), (500, ms));
        assert_eq!(std::fs::read(&cache).unwrap(), vec![0u8; 8 * 500]);
        // A file without audio is an error, not an empty waveform.
        assert!(peaks_into_cache("/no/such.wav", &dir.join("x.f32")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
