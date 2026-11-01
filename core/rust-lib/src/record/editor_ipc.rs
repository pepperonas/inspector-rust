//! The `record` editor's app side: one editor session (project, document, transport) and one
//! lazily started render helper, driven by the `editor_*` IPC commands in `commands.rs`.
//!
//! Rules this module keeps (stage-2 plan, rulings R2/R4/R5/R11):
//! * The UI never maps programme ↔ source time itself: every command answers with the
//!   [`editor::view`] / [`SourcePos`] computed here from `editor.rs`.
//! * The app owns the playhead clock ([`Transport`]); the helper's preview only plays the
//!   active clip, so the app sends `SetActiveClip` at clip boundaries and stops at the end.
//! * Every blocking call runs off the main thread (the IPC wrappers use `spawn_blocking`); the
//!   window is built on the main thread via `run_on_main_thread`, never from a sync command.
//! * A dead helper never takes the app down: the UI gets `record-editor-fatal`, the next
//!   mutating command (or `editor_restart`) starts a new helper and re-opens the preview.
//! * The document is saved into `project.json` debounced (≤ [`SAVE_DEBOUNCE`]); close flushes.
//!
//! Errors returned to the UI are `record.editor.*` sentinels (en+de locale keys) or the
//! helper's own message.

use super::editor::{self, Aspect, EditDoc, EditorView, OutputTarget};
use super::project::{self, Project, ProjectStatus, ProjectSummary, RecordingMeta, PROJECT_FILE, SCREEN_FILE};
use super::render_client::{FrameAnswer, Notice, RenderClient, Sink};
use super::scene_json::scene_json;
use base64::Engine;
use record_render_protocol::{Codec, Command, ExportKind, PreviewFrame};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const EDITOR_LABEL: &str = "record-editor";
/// Document changes reach `project.json` at most this long after the last edit.
pub const SAVE_DEBOUNCE: Duration = Duration::from_millis(800);
/// While playing, no new preview frame for this long means the helper is stalled.
pub const PLAYBACK_STALL: Duration = Duration::from_secs(3);
/// The preview scene's fps (the export picks its own).
pub const PREVIEW_FPS: u32 = 30;
/// The preview is rendered at most this large (the window scales it).
pub const PREVIEW_MAX: (u32, u32) = (1280, 720);
pub const THUMB_FPS: u32 = 10;
pub const THUMB_WIDTH: u32 = 160;
pub const MAX_THUMBS: u32 = 60;
pub const MP4_FPS: [u32; 3] = [24, 30, 60];
pub const GIF_FPS: std::ops::RangeInclusive<u32> = 15..=30;
pub const GIF_WIDTHS: [u32; 4] = [480, 640, 854, 1080];

pub const EV_FATAL: &str = "record-editor-fatal";
pub const EV_EXPORT_PROGRESS: &str = "record-export-progress";
pub const EV_EXPORT_DONE: &str = "record-export-done";
pub const EV_EXPORT_FAILED: &str = "record-export-failed";

pub const ERR_NO_SESSION: &str = "record.editor.no_session";
pub const ERR_UNREADABLE: &str = "record.editor.unreadable";
pub const ERR_NO_VIDEO: &str = "record.editor.no_video";
pub const ERR_BAD_EXPORT: &str = "record.editor.bad_export";
pub const ERR_NO_DOWNLOADS: &str = "record.editor.no_downloads";

// ── pure parts ──────────────────────────────────────────────────────────────

/// `editor_frame`'s raw response: `gen u64 LE`, `w u32 LE`, `h u32 LE`, RGBA — or nothing at
/// all when no newer frame exists.
pub fn frame_response(frame: Option<&PreviewFrame>) -> Vec<u8> {
    match frame {
        Some(f) if !f.is_empty() => record_render_protocol::encode_preview(f.gen, f.w, f.h, &f.rgba),
        _ => Vec::new(),
    }
}

/// A file name the Finder accepts: no `/` or `:`, no control characters, not empty.
fn clean_title(title: &str) -> String {
    let t: String = title
        .chars()
        .map(|c| if c == '/' || c == ':' || c.is_control() { ' ' } else { c })
        .collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = t.trim_start_matches('.').trim().to_string();
    if t.is_empty() { "Recording".into() } else { t }
}

/// `<dir>/<title> <YYYY-MM-DD HH.MM>.<ext>`; taken names get ` 2`, ` 3`, …
pub fn export_path(
    dir: &Path,
    title: &str,
    now: chrono::NaiveDateTime,
    ext: &str,
    exists: impl Fn(&Path) -> bool,
) -> PathBuf {
    let stem = format!("{} {}", clean_title(title), now.format("%Y-%m-%d %H.%M"));
    let first = dir.join(format!("{stem}.{ext}"));
    if !exists(&first) {
        return first;
    }
    (2u32..)
        .map(|n| dir.join(format!("{stem} {n}.{ext}")))
        .find(|p| !exists(p))
        .expect("an unbounded range finds a free name")
}

/// What one frame poll tells the stall watchdog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramePoll {
    /// A new frame arrived.
    Frame,
    /// The helper answered "nothing newer".
    NoneNewer,
    /// No request was possible (thumbnails, a re-open, an export hold the helper).
    Busy,
}

/// The stall watchdog. `anchor_ms` is the last moment the preview was known healthy; only
/// time spent playing with the helper free to answer counts. Returns the new anchor and
/// whether the helper is stalled (no frame for more than [`PLAYBACK_STALL`]).
pub fn stall_check(playing: bool, poll: FramePoll, anchor_ms: u64, now_ms: u64) -> (u64, bool) {
    match poll {
        FramePoll::Frame | FramePoll::Busy => (now_ms, false),
        FramePoll::NoneNewer if !playing => (now_ms, false),
        FramePoll::NoneNewer => {
            (anchor_ms, now_ms.saturating_sub(anchor_ms) > PLAYBACK_STALL.as_millis() as u64)
        }
    }
}

/// Programme frame indices on the `fps` grid for `count` evenly spaced thumbnails (centred in
/// their slots), deduplicated, never past the last frame.
pub fn thumb_frames(programme_ms: u64, count: u32, fps: u32) -> Vec<u64> {
    let count = count.clamp(1, MAX_THUMBS) as u64;
    let fps = fps.max(1) as u64;
    let last = (programme_ms * fps / 1000).saturating_sub(1);
    let mut out: Vec<u64> = Vec::new();
    for i in 0..count {
        let t_ms = (2 * i + 1) * programme_ms / (2 * count);
        let idx = (t_ms * fps / 1000).min(last);
        if out.last() != Some(&idx) {
            out.push(idx);
        }
    }
    out
}

/// Fits `out` into [`PREVIEW_MAX`] keeping its shape (even sides, at least 2).
pub fn preview_size(out: (u32, u32)) -> (u32, u32) {
    let (w, h) = (out.0.max(1) as f64, out.1.max(1) as f64);
    let scale = (PREVIEW_MAX.0 as f64 / w).min(PREVIEW_MAX.1 as f64 / h).min(1.0);
    let even = |v: f64| (((v / 2.0).round() as u32) * 2).max(2);
    (even(w * scale), even(h * scale))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    #[default]
    Mp4,
    Gif,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportOptions {
    pub kind: ExportFormat,
    pub target: OutputTarget,
    pub fps: u32,
    pub codec: Codec,
    pub gif_width: u32,
    pub loop_forever: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            kind: ExportFormat::Mp4,
            target: OutputTarget::P1080,
            fps: 30,
            codec: Codec::H264,
            gif_width: 640,
            loop_forever: true,
        }
    }
}

/// Export kind, scene output size, fps and file extension.
pub type ExportPlan = (ExportKind, (u32, u32), u32, &'static str);

/// What the helper gets for `opts`: the export kind, the scene's output size and fps, and the
/// file extension. Values outside the offered presets are refused (`ERR_BAD_EXPORT`).
pub fn export_plan(
    opts: &ExportOptions,
    meta: &RecordingMeta,
    aspect: Aspect,
) -> Result<ExportPlan, String> {
    match opts.kind {
        ExportFormat::Mp4 => {
            if !MP4_FPS.contains(&opts.fps) {
                return Err(ERR_BAD_EXPORT.into());
            }
            let (w, h) = editor::output_size(meta, aspect, opts.target);
            Ok((ExportKind::Mp4 { w, h, fps: opts.fps, codec: opts.codec }, (w, h), opts.fps, "mp4"))
        }
        ExportFormat::Gif => {
            if !GIF_FPS.contains(&opts.fps) || !GIF_WIDTHS.contains(&opts.gif_width) {
                return Err(ERR_BAD_EXPORT.into());
            }
            let (sw, sh) = editor::output_size(meta, aspect, OutputTarget::Source);
            let w = opts.gif_width;
            let h = ((((w as f64 * sh as f64 / sw.max(1) as f64) / 2.0).round() as u32) * 2).max(2);
            Ok((ExportKind::Gif { w, h, fps: opts.fps, loop_forever: opts.loop_forever }, (w, h), opts.fps, "gif"))
        }
    }
}

/// A programme position mapped into the recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcePos {
    pub clip: usize,
    pub source_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackState {
    pub playing: bool,
    pub position_ms: u64,
    pub programme_ms: u64,
    pub exporting: bool,
    /// The helper's death reason while it is dead (until the next command restarts it).
    pub helper_error: Option<String>,
}

fn locate(doc: &EditDoc, prog_ms: u64) -> Option<SourcePos> {
    editor::programme_to_source(doc, prog_ms).map(|(clip, source_ms)| SourcePos { clip, source_ms })
}

fn sec(ms: u64) -> f64 {
    ms as f64 / 1000.0
}

/// The app's playhead. Programme time runs at wall-clock speed while playing; `now_ms` is any
/// monotonic millisecond clock. Every method returns the commands the helper needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Transport {
    playing: bool,
    /// Position when the transport last started/seeked (or the frozen position when paused).
    base_ms: u64,
    started_ms: u64,
    clip: Option<usize>,
    programme_ms: u64,
}

impl Transport {
    pub fn new(doc: &EditDoc) -> Self {
        Self { playing: false, base_ms: 0, started_ms: 0, clip: Some(0), programme_ms: editor::programme_duration_ms(doc) }
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    pub fn programme_ms(&self) -> u64 {
        self.programme_ms
    }

    pub fn position(&self, now_ms: u64) -> u64 {
        if self.playing {
            (self.base_ms + now_ms.saturating_sub(self.started_ms)).min(self.programme_ms)
        } else {
            self.base_ms.min(self.programme_ms)
        }
    }

    /// Points the preview at `pos`: a clip change is `SetActiveClip`, else `SetTime`
    /// (`force` always sends `SetActiveClip` — after a scene change the clip may differ).
    fn aim(&mut self, doc: &EditDoc, pos: u64, force: bool) -> (Vec<Command>, Option<SourcePos>) {
        let Some(p) = locate(doc, pos) else { return (Vec::new(), None) };
        let cmd = if force || self.clip != Some(p.clip) {
            self.clip = Some(p.clip);
            Command::SetActiveClip { clip: p.clip, source_sec: sec(p.source_ms) }
        } else {
            Command::SetTime { source_sec: sec(p.source_ms) }
        };
        (vec![cmd], Some(p))
    }

    pub fn play(&mut self, doc: &EditDoc, now_ms: u64) -> Vec<Command> {
        if self.programme_ms == 0 {
            return Vec::new();
        }
        let mut pos = self.position(now_ms);
        if pos >= self.programme_ms {
            pos = 0; // play from the end starts over
        }
        self.base_ms = pos;
        self.started_ms = now_ms;
        self.playing = true;
        let (mut cmds, _) = self.aim(doc, pos, true);
        cmds.push(Command::SetPlaying { playing: true });
        cmds
    }

    pub fn pause(&mut self, now_ms: u64) -> Vec<Command> {
        self.base_ms = self.position(now_ms);
        self.playing = false;
        vec![Command::SetPlaying { playing: false }]
    }

    pub fn seek(&mut self, doc: &EditDoc, now_ms: u64, prog_ms: u64) -> (Vec<Command>, Option<SourcePos>) {
        self.base_ms = prog_ms.min(self.programme_ms);
        self.started_ms = now_ms;
        let pos = self.base_ms;
        self.aim(doc, pos, false)
    }

    /// Advances the clock: a clip boundary sends `SetActiveClip`, the end stops playback.
    pub fn tick(&mut self, doc: &EditDoc, now_ms: u64) -> Vec<Command> {
        if !self.playing {
            return Vec::new();
        }
        let pos = self.position(now_ms);
        if pos >= self.programme_ms {
            self.base_ms = self.programme_ms;
            self.playing = false;
            return vec![Command::SetPlaying { playing: false }];
        }
        match locate(doc, pos) {
            Some(p) if self.clip != Some(p.clip) => {
                self.clip = Some(p.clip);
                vec![Command::SetActiveClip { clip: p.clip, source_sec: sec(p.source_ms) }]
            }
            _ => Vec::new(),
        }
    }

    /// Re-aims a fresh preview (a restarted helper starts on clip 0) at the playhead: always a
    /// `SetActiveClip`, never a `SetTime` into whatever clip the helper happens to show.
    pub fn reaim(&mut self, doc: &EditDoc, now_ms: u64) -> Vec<Command> {
        let pos = self.position(now_ms);
        self.base_ms = pos;
        self.started_ms = now_ms;
        self.aim(doc, pos, true).0
    }

    /// The document changed: keep the playhead (clamped into the new programme) and re-aim.
    pub fn set_doc(&mut self, doc: &EditDoc, now_ms: u64) -> Vec<Command> {
        let pos = self.position(now_ms);
        self.programme_ms = editor::programme_duration_ms(doc);
        self.base_ms = pos.min(self.programme_ms);
        self.started_ms = now_ms;
        let pos = self.base_ms;
        self.aim(doc, pos, true).0
    }
}

// ── session ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorInit {
    pub project: ProjectSummary,
    pub meta: RecordingMeta,
    pub view: EditorView,
    /// Set when the preview could not be started; the editor shows it and offers a restart.
    pub helper_error: Option<String>,
}

struct Session {
    /// Unique per `open_editor`: an old window's close must not end a newer session.
    id: u64,
    dir: PathBuf,
    project: Project,
    doc: EditDoc,
    transport: Transport,
    preview: (u32, u32),
    /// Stall-watchdog anchor (`now_ms` clock), see [`stall_check`].
    last_frame_ms: u64,
    dirty: bool,
    save_pending: bool,
}

impl Session {
    fn summary(&self) -> ProjectSummary {
        ProjectSummary {
            dir: self.dir.clone(),
            title: self.project.title.clone(),
            created_at: self.project.created_at,
            duration_ms: self.project.recording.duration_ms,
            status: self.project.status,
            failure: self.project.failure.clone(),
        }
    }

    fn output(&self, target: OutputTarget) -> (u32, u32) {
        editor::output_size(&self.project.recording, self.doc.output.aspect, target)
    }

    fn preview_scene(&self) -> Value {
        scene_json(&self.dir, &self.project, &self.doc, self.output(OutputTarget::P1080), PREVIEW_FPS)
    }

    fn init(&self, helper_error: Option<String>) -> EditorInit {
        EditorInit {
            project: self.summary(),
            meta: self.project.recording.clone(),
            view: editor::view(&self.doc),
            helper_error,
        }
    }
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
/// Bumped for every editor window built; a destroyed window that is no longer the current
/// one never touches the session.
static WINDOW_GEN: AtomicU64 = AtomicU64::new(0);
static HOST: Mutex<Option<Arc<RenderClient>>> = Mutex::new(None);

fn session() -> std::sync::MutexGuard<'static, Option<Session>> {
    SESSION.lock().unwrap_or_else(|p| p.into_inner())
}

fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

fn sink(app: &AppHandle) -> Sink {
    let app = app.clone();
    Arc::new(move |notice| match notice {
        Notice::ExportProgress { frames, total } => {
            let _ = app.emit(EV_EXPORT_PROGRESS, serde_json::json!({ "frames": frames, "total": total }));
        }
        Notice::ExportDone { path } => {
            crate::commands::reveal_in_file_manager(Path::new(&path));
            let _ = app.emit(EV_EXPORT_DONE, serde_json::json!({ "path": path }));
        }
        Notice::ExportFailed { reason, cancelled } => {
            let _ = app.emit(EV_EXPORT_FAILED, serde_json::json!({ "reason": reason, "cancelled": cancelled }));
        }
        Notice::Dead { reason } => {
            // Keep the edits: the next save would otherwise wait for another edit or the close.
            let _ = std::thread::Builder::new().name("ir-editor-save".into()).spawn(flush_save);
            let _ = app.emit(EV_FATAL, serde_json::json!({ "reason": reason }));
        }
    })
}

/// The live client, or none (never started / dead). Does not start one.
fn current_client() -> Option<Arc<RenderClient>> {
    HOST.lock().unwrap_or_else(|p| p.into_inner()).clone().filter(|c| !c.is_dead())
}

/// The live client; starts a new helper when there is none or it died. A fresh helper gets
/// the current session's preview re-opened (`reopen`) and the playhead re-aimed.
fn ensure_client(app: &AppHandle, reopen: bool) -> Result<Arc<RenderClient>, String> {
    let fresh = {
        let mut host = HOST.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(c) = host.as_ref().filter(|c| !c.is_dead()) {
            return Ok(c.clone());
        }
        let client = Arc::new(RenderClient::spawn(sink(app))?);
        *host = Some(client.clone());
        client
    };
    if reopen {
        let opened = {
            let mut s = session();
            s.as_mut().map(|s| {
                // The fresh helper's preview is paused on clip 0; stop the clock with it and
                // aim at the playhead with a SetActiveClip (Transport::reaim).
                s.transport.pause(now_ms());
                let cmds = s.transport.reaim(&s.doc, now_ms());
                s.last_frame_ms = now_ms();
                (s.preview_scene(), s.preview, cmds)
            })
        };
        if let Some((scene, (w, h), cmds)) = opened {
            fresh.open(scene, w, h)?;
            for c in &cmds {
                fresh.send(c)?;
            }
        }
    }
    Ok(fresh)
}

fn send_all(client: &RenderClient, cmds: &[Command]) -> Result<(), String> {
    cmds.iter().try_for_each(|c| client.send(c))
}

fn flush_save() {
    let mut s = session();
    let Some(s) = s.as_mut() else { return };
    s.save_pending = false;
    if !s.dirty {
        return;
    }
    s.dirty = false;
    editor::store(&mut s.project, &s.doc);
    s.project.updated_at = chrono::Utc::now().timestamp_millis();
    if let Err(e) = project::write_atomic(&s.dir, PROJECT_FILE, &s.project) {
        tracing::warn!(target: "record::editor", "saving {} failed: {e}", s.dir.display());
    }
}

fn schedule_save(s: &mut Session) {
    s.dirty = true;
    if s.save_pending {
        return;
    }
    s.save_pending = true;
    let spawned = std::thread::Builder::new().name("ir-editor-save".into()).spawn(|| {
        std::thread::sleep(SAVE_DEBOUNCE);
        flush_save();
    });
    if spawned.is_err() {
        s.save_pending = false;
    }
}

/// Builds (or re-shows) the editor window on the main thread. Safe from any thread.
fn show_window(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = handle.get_webview_window(EDITOR_LABEL) {
            let _ = w.show();
            let _ = w.set_focus();
            return;
        }
        let built = tauri::WebviewWindowBuilder::new(&handle, EDITOR_LABEL, tauri::WebviewUrl::App("index.html".into()))
            .title(crate::i18n::t("record.editor.window_title"))
            .inner_size(1120.0, 760.0)
            .min_inner_size(760.0, 520.0)
            .resizable(true)
            .decorations(true)
            .visible(true)
            .focused(true)
            .center()
            .build();
        match built {
            Ok(win) => {
                let app = handle.clone();
                let gen = WINDOW_GEN.fetch_add(1, Ordering::SeqCst) + 1;
                win.on_window_event(move |ev| {
                    if !matches!(ev, tauri::WindowEvent::Destroyed) || WINDOW_GEN.load(Ordering::SeqCst) != gen {
                        return;
                    }
                    // The session this window showed when it went away — read now, not on the
                    // thread, so an editor opened meanwhile is never the one closed.
                    let Some(id) = session().as_ref().map(|s| s.id) else { return };
                    let app = app.clone();
                    std::thread::spawn(move || close(&app, false, Some(id)));
                });
            }
            Err(e) => tracing::warn!(target: "record::editor", "editor window: {e}"),
        }
    });
}

/// Opens the project at `dir` in the editor: loads it, shows the window, opens the preview.
/// Blocking (the helper's `Open` takes a moment) — call it from a worker thread. Used by the
/// `editor_open` IPC and by the recording session after a take (Task 7).
pub fn open_editor(app: &AppHandle, dir: PathBuf) -> Result<EditorInit, String> {
    let project = project::read_project(&dir).map_err(|e| {
        tracing::warn!(target: "record::editor", "{}: {e}", dir.display());
        ERR_UNREADABLE.to_string()
    })?;
    if project.status == ProjectStatus::Failed || !dir.join(SCREEN_FILE).is_file() {
        return Err(ERR_NO_VIDEO.into());
    }
    let doc = editor::load(&project);
    // A different project open before: its edits are saved first.
    flush_save();
    let (scene, preview, init) = {
        let mut s = session();
        let new = Session {
            id: NEXT_SESSION.fetch_add(1, Ordering::SeqCst),
            transport: Transport::new(&doc),
            preview: (0, 0),
            dir,
            project,
            doc,
            last_frame_ms: now_ms(),
            dirty: false,
            save_pending: false,
        };
        let preview = preview_size(new.output(OutputTarget::P1080));
        let new = Session { preview, ..new };
        let out = (new.preview_scene(), preview, new.init(None));
        *s = Some(new);
        out
    };
    show_window(app);
    let helper_error = ensure_client(app, false)
        .and_then(|c| c.open(scene, preview.0, preview.1))
        .err();
    if let Some(e) = &helper_error {
        tracing::warn!(target: "record::editor", "preview did not start: {e}");
    }
    Ok(EditorInit { helper_error, ..init })
}

/// The open session, for a window that mounts after `open_editor` ran.
pub fn current() -> Option<EditorInit> {
    let helper_error = HOST
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .and_then(|c| match c.state() {
            super::render_client::ClientState::Dead { reason } => Some(reason),
            _ => None,
        });
    session().as_ref().map(|s| s.init(helper_error))
}

pub fn update(app: &AppHandle, doc: EditDoc) -> Result<EditorView, String> {
    let (view, scene, cmds) = {
        let mut guard = session();
        let s = guard.as_mut().ok_or(ERR_NO_SESSION)?;
        s.doc = editor::normalize(doc, s.project.recording.duration_ms);
        let cmds = s.transport.set_doc(&s.doc, now_ms());
        s.last_frame_ms = now_ms();
        schedule_save(s);
        (editor::view(&s.doc), s.preview_scene(), cmds)
    };
    let client = ensure_client(app, true)?;
    client.send(&Command::SetScene { scene })?;
    send_all(&client, &cmds)?;
    Ok(view)
}

pub fn seek(app: &AppHandle, programme_ms: u64) -> Result<Option<SourcePos>, String> {
    let (cmds, pos) = {
        let mut guard = session();
        let s = guard.as_mut().ok_or(ERR_NO_SESSION)?;
        s.last_frame_ms = now_ms();
        s.transport.seek(&s.doc, now_ms(), programme_ms)
    };
    let client = ensure_client(app, true)?;
    send_all(&client, &cmds)?;
    Ok(pos)
}

fn playback(s: &Session) -> PlaybackState {
    let client = HOST.lock().unwrap_or_else(|p| p.into_inner()).clone();
    PlaybackState {
        playing: s.transport.playing(),
        position_ms: s.transport.position(now_ms()),
        programme_ms: s.transport.programme_ms(),
        exporting: client.as_ref().is_some_and(|c| c.exporting()),
        helper_error: client.and_then(|c| match c.state() {
            super::render_client::ClientState::Dead { reason } => Some(reason),
            _ => None,
        }),
    }
}

pub fn play(app: &AppHandle, playing: bool) -> Result<PlaybackState, String> {
    let cmds = {
        let mut guard = session();
        let s = guard.as_mut().ok_or(ERR_NO_SESSION)?;
        s.last_frame_ms = now_ms();
        if playing { s.transport.play(&s.doc, now_ms()) } else { s.transport.pause(now_ms()) }
    };
    let client = ensure_client(app, true)?;
    send_all(&client, &cmds)?;
    let guard = session();
    Ok(playback(guard.as_ref().ok_or(ERR_NO_SESSION)?))
}

/// Advances the playhead (clip switches, stop at the end). Never starts a helper.
fn tick() -> Option<PlaybackState> {
    let (cmds, state) = {
        let mut guard = session();
        let s = guard.as_mut()?;
        let cmds = s.transport.tick(&s.doc, now_ms());
        (cmds, playback(s))
    };
    if let Some(c) = current_client() {
        let _ = send_all(&c, &cmds);
    }
    Some(state)
}

pub fn position() -> Option<PlaybackState> {
    tick()
}

/// The newest preview frame after `since_gen` in the raw layout of [`frame_response`].
/// Also the stall watchdog: while playing, no frame for [`PLAYBACK_STALL`] kills the helper.
pub fn frame(since_gen: u64) -> Vec<u8> {
    let Some(state) = tick() else { return Vec::new() };
    let Some(client) = current_client() else { return Vec::new() };
    let (poll, body) = match client.frame(since_gen) {
        Ok(FrameAnswer::Frame(f)) => (FramePoll::Frame, frame_response(Some(&f))),
        Ok(FrameAnswer::NoneNewer) => (FramePoll::NoneNewer, Vec::new()),
        Ok(FrameAnswer::Busy) => (FramePoll::Busy, Vec::new()),
        Err(_) => return Vec::new(),
    };
    let stalled = {
        let mut guard = session();
        let Some(s) = guard.as_mut() else { return body };
        let (anchor, stalled) = stall_check(state.playing, poll, s.last_frame_ms, now_ms());
        s.last_frame_ms = anchor;
        if stalled {
            s.transport.pause(now_ms());
        }
        stalled
    };
    if stalled {
        client.abandon(format!("render helper stalled: no preview frame for {} s", PLAYBACK_STALL.as_secs()));
    }
    body
}

pub fn resize(app: &AppHandle, w: u32, h: u32) -> Result<(), String> {
    let (w, h) = (w.clamp(16, 4096), h.clamp(16, 4096));
    {
        let mut guard = session();
        let s = guard.as_mut().ok_or(ERR_NO_SESSION)?;
        s.preview = (w, h);
    }
    ensure_client(app, true)?.send(&Command::Resize { w, h })
}

pub fn thumbs(app: &AppHandle, count: u32) -> Result<Vec<String>, String> {
    let (scene, frames) = {
        let guard = session();
        let s = guard.as_ref().ok_or(ERR_NO_SESSION)?;
        let out = s.output(OutputTarget::P720);
        let frames = thumb_frames(editor::programme_duration_ms(&s.doc), count, THUMB_FPS);
        (scene_json(&s.dir, &s.project, &s.doc, out, THUMB_FPS), frames)
    };
    let client = ensure_client(app, true)?;
    let rendered = client.thumbs(scene, THUMB_FPS, frames.clone(), THUMB_WIDTH);
    // The helper paused its preview while rendering, the app clock ran on: put the preview
    // back where the playhead is now.
    let cmds = {
        let mut guard = session();
        guard.as_mut().filter(|s| s.transport.playing()).map(|s| {
            s.last_frame_ms = now_ms();
            s.transport.reaim(&s.doc, now_ms())
        })
    };
    if let Some(cmds) = cmds {
        let _ = send_all(&client, &cmds);
    }
    let rendered = rendered?;
    let b64 = base64::engine::general_purpose::STANDARD;
    Ok(frames
        .iter()
        .filter_map(|f| rendered.iter().find(|(i, _)| i == f))
        .map(|(_, jpeg)| format!("data:image/jpeg;base64,{}", b64.encode(jpeg)))
        .collect())
}

pub fn export(app: &AppHandle, opts: ExportOptions) -> Result<String, String> {
    let (scene, kind, path) = {
        let guard = session();
        let s = guard.as_ref().ok_or(ERR_NO_SESSION)?;
        let (kind, size, fps, ext) = export_plan(&opts, &s.project.recording, s.doc.output.aspect)?;
        let downloads = dirs::download_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join("Downloads")))
            .filter(|d| d.is_dir())
            .ok_or(ERR_NO_DOWNLOADS)?;
        let title = if s.project.title.trim().is_empty() {
            s.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        } else {
            s.project.title.clone()
        };
        let path = export_path(&downloads, &title, chrono::Local::now().naive_local(), ext, |p| p.exists());
        (scene_json(&s.dir, &s.project, &s.doc, size, fps), kind, path)
    };
    let client = ensure_client(app, true)?;
    let out = path.to_string_lossy().into_owned();
    client.export(scene, kind, out.clone())?;
    Ok(out)
}

pub fn cancel_export() -> Result<(), String> {
    match current_client() {
        Some(c) => c.cancel_export(),
        None => Ok(()),
    }
}

/// Starts a new helper after a death and re-opens the preview.
pub fn restart(app: &AppHandle) -> Result<PlaybackState, String> {
    ensure_client(app, true)?;
    let guard = session();
    Ok(playback(guard.as_ref().ok_or(ERR_NO_SESSION)?))
}

/// Ends the session: saves, shuts the helper down (≤ 2 s, then killed; a running export ends
/// as cancelled and its partial file is removed), closes the window (unless it is already
/// going away). With `expected`, only that session is ended — a newer one is left alone.
pub fn close(app: &AppHandle, close_window: bool, expected: Option<u64>) {
    if !should_close(session().as_ref().map(|s| s.id), expected) {
        return;
    }
    flush_save();
    {
        let mut guard = session();
        if !should_close(guard.as_ref().map(|s| s.id), expected) {
            return;
        }
        guard.take();
    }
    let client = HOST.lock().unwrap_or_else(|p| p.into_inner()).take();
    if let Some(c) = client {
        c.shutdown();
    }
    if close_window {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(w) = handle.get_webview_window(EDITOR_LABEL) {
                let _ = w.close();
            }
        });
    }
}

/// `close` for `expected`: no expectation closes whatever is open; otherwise only the
/// expected session (an already-closed one leaves nothing to do).
pub fn should_close(current: Option<u64>, expected: Option<u64>) -> bool {
    match expected {
        None => true,
        Some(id) => current == Some(id),
    }
}

/// App exit (`RunEvent::Exit`): save and stop the helper.
pub fn shutdown(app: &AppHandle) {
    close(app, false, None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::editor::{EditClip, OutputShape, EDITOR_VERSION};
    use chrono::NaiveDate;

    fn doc(clips: &[(u64, u64)]) -> EditDoc {
        EditDoc {
            version: EDITOR_VERSION,
            clips: clips.iter().map(|&(s, e)| EditClip { start_ms: s, end_ms: e }).collect(),
            speed: Vec::new(),
            crop: vec![None; clips.len()],
            output: OutputShape::default(),
            mic_gain_db: 0.0,
        }
    }

    #[test]
    fn frame_response_layout() {
        let f = PreviewFrame { gen: 0x0102_0304_0506_0708, w: 2, h: 1, rgba: vec![9, 8, 7, 6, 5, 4, 3, 2] };
        let r = frame_response(Some(&f));
        assert_eq!(r.len(), 8 + 4 + 4 + 8);
        assert_eq!(&r[0..8], &0x0102_0304_0506_0708u64.to_le_bytes(), "gen u64 LE");
        assert_eq!(&r[8..12], &2u32.to_le_bytes(), "w u32 LE");
        assert_eq!(&r[12..16], &1u32.to_le_bytes(), "h u32 LE");
        assert_eq!(&r[16..], &[9, 8, 7, 6, 5, 4, 3, 2], "RGBA as is");
        // Nothing newer: an empty body, not a header with zeros.
        assert!(frame_response(None).is_empty());
        let empty = PreviewFrame { gen: 5, w: 0, h: 0, rgba: Vec::new() };
        assert!(frame_response(Some(&empty)).is_empty());
    }

    #[test]
    fn export_name_is_unique() {
        let dir = Path::new("/Users/x/Downloads");
        let now = NaiveDate::from_ymd_opt(2026, 11, 2).unwrap().and_hms_opt(9, 5, 33).unwrap();
        let p = export_path(dir, "Demo", now, "mp4", |_| false);
        assert_eq!(p, dir.join("Demo 2026-11-02 09.05.mp4"));
        let taken = [dir.join("Demo 2026-11-02 09.05.mp4"), dir.join("Demo 2026-11-02 09.05 2.mp4")];
        let p = export_path(dir, "Demo", now, "mp4", |p| taken.iter().any(|t| t == p));
        assert_eq!(p, dir.join("Demo 2026-11-02 09.05 3.mp4"));
        // Characters the Finder refuses never reach the name; an empty title still names it.
        assert_eq!(export_path(dir, "a/b:c\n", now, "gif", |_| false), dir.join("a b c 2026-11-02 09.05.gif"));
        assert_eq!(export_path(dir, "  ", now, "gif", |_| false), dir.join("Recording 2026-11-02 09.05.gif"));
        assert_eq!(export_path(dir, "..hidden", now, "gif", |_| false), dir.join("hidden 2026-11-02 09.05.gif"));
    }

    #[test]
    fn playhead_switches_clip_at_boundary() {
        // Clip 0 plays programme 0–2 s, clip 1 (source 5–8 s) programme 2–5 s.
        let d = doc(&[(0, 2_000), (5_000, 8_000)]);
        let mut t = Transport::new(&d);
        let start = t.play(&d, 1_000);
        assert_eq!(
            start,
            vec![Command::SetActiveClip { clip: 0, source_sec: 0.0 }, Command::SetPlaying { playing: true }]
        );
        let mut switches = Vec::new();
        let mut stops = 0;
        for now in (1_050..=6_500).step_by(50) {
            for c in t.tick(&d, now) {
                match c {
                    Command::SetActiveClip { .. } => switches.push((now, c)),
                    Command::SetPlaying { playing: false } => stops += 1,
                    other => panic!("unexpected {other:?}"),
                }
            }
        }
        assert_eq!(switches.len(), 1, "exactly one clip switch: {switches:?}");
        let (at, cmd) = &switches[0];
        assert_eq!(*at, 3_000, "at programme 2 s");
        assert_eq!(*cmd, Command::SetActiveClip { clip: 1, source_sec: 5.0 });
        assert_eq!(stops, 1, "stops once at the end");
        assert!(!t.playing());
        assert_eq!(t.position(9_999), 5_000);
        // Play at the end starts over at clip 0.
        assert_eq!(t.play(&d, 10_000)[0], Command::SetActiveClip { clip: 0, source_sec: 0.0 });
    }

    #[test]
    fn seek_within_a_clip_is_set_time() {
        let d = doc(&[(0, 2_000), (5_000, 8_000)]);
        let mut t = Transport::new(&d);
        let (cmds, pos) = t.seek(&d, 0, 500);
        assert_eq!(cmds, vec![Command::SetTime { source_sec: 0.5 }]);
        assert_eq!(pos, Some(SourcePos { clip: 0, source_ms: 500 }));
        let (cmds, pos) = t.seek(&d, 0, 2_500);
        assert_eq!(cmds, vec![Command::SetActiveClip { clip: 1, source_sec: 5.5 }]);
        assert_eq!(pos, Some(SourcePos { clip: 1, source_ms: 5_500 }));
        // Past the end clamps to the end.
        let (_, pos) = t.seek(&d, 0, 99_000);
        assert_eq!(pos, Some(SourcePos { clip: 1, source_ms: 8_000 }));
        // A shorter document clamps a paused playhead and re-aims with SetActiveClip.
        let short = doc(&[(0, 1_000)]);
        assert_eq!(t.set_doc(&short, 0), vec![Command::SetActiveClip { clip: 0, source_sec: 1.0 }]);
        assert_eq!(t.position(0), 1_000);
    }

    #[test]
    fn stall_counts_only_free_playing_time() {
        let stall = PLAYBACK_STALL.as_millis() as u64;
        // Helper answers "nothing newer" for longer than the limit while playing → stalled.
        assert_eq!(stall_check(true, FramePoll::NoneNewer, 0, stall), (0, false), "exactly the limit is fine");
        assert_eq!(stall_check(true, FramePoll::NoneNewer, 0, stall + 1), (0, true));
        // Thumbnails / a re-open / an export hold the helper for a long time: never a stall, and
        // the clock restarts afterwards.
        let mut anchor = 0;
        for now in (0..30_000).step_by(100) {
            let (a, stalled) = stall_check(true, FramePoll::Busy, anchor, now);
            assert!(!stalled, "busy at {now}");
            anchor = a;
        }
        // The busy time does not count: right after it, "nothing newer" is not a stall yet …
        assert_eq!(anchor, 29_900, "every busy poll moves the anchor");
        assert_eq!(stall_check(true, FramePoll::NoneNewer, anchor, 30_400), (anchor, false));
        // … only a full stall window of free playing time is.
        assert!(stall_check(true, FramePoll::NoneNewer, anchor, anchor + stall + 1).1);
        // Paused: no frames are expected.
        assert_eq!(stall_check(false, FramePoll::NoneNewer, 0, 60_000), (60_000, false));
        // A frame resets the anchor.
        assert_eq!(stall_check(true, FramePoll::Frame, 0, 9_000), (9_000, false));
    }

    #[test]
    fn restart_reaims_with_set_active_clip() {
        let d = doc(&[(0, 2_000), (5_000, 8_000)]);
        let mut t = Transport::new(&d);
        let _ = t.seek(&d, 0, 3_000); // playhead in clip 1
        // A fresh helper shows clip 0: the reopen must switch clips, not SetTime into clip 0.
        assert_eq!(t.reaim(&d, 0), vec![Command::SetActiveClip { clip: 1, source_sec: 6.0 }]);
        // Same while playing (the clock is stopped first, as the reopen does).
        let _ = t.play(&d, 0);
        let _ = t.pause(500);
        assert_eq!(t.reaim(&d, 500), vec![Command::SetActiveClip { clip: 1, source_sec: 6.5 }]);
    }

    #[test]
    fn close_only_ends_the_expected_session() {
        assert!(should_close(Some(3), None), "editor_close / app exit end whatever is open");
        assert!(should_close(Some(3), Some(3)));
        assert!(!should_close(Some(4), Some(3)), "an old window's close never ends a newer session");
        assert!(!should_close(None, Some(3)), "already closed");
    }

    #[test]
    fn thumb_frames_spread_and_stay_inside() {
        assert_eq!(thumb_frames(10_000, 5, 10), vec![10, 30, 50, 70, 90]);
        // Short programme: duplicates collapse, last index never exceeded.
        let f = thumb_frames(250, 10, 10);
        assert!(f.windows(2).all(|w| w[0] < w[1]), "{f:?}");
        assert!(*f.last().unwrap() <= 1);
        assert_eq!(thumb_frames(10_000, 0, 10).len(), 1, "count clamps to at least one");
        assert_eq!(thumb_frames(600_000, 500, 10).len(), MAX_THUMBS as usize);
    }

    #[test]
    fn export_plan_presets() {
        let meta = RecordingMeta { width: 3342, height: 2160, fps: 60, duration_ms: 5_000, ..RecordingMeta::default() };
        let mp4 = ExportOptions { target: OutputTarget::P720, fps: 60, codec: Codec::Hevc, ..ExportOptions::default() };
        let (kind, size, fps, ext) = export_plan(&mp4, &meta, Aspect::W16H9).unwrap();
        assert_eq!(kind, ExportKind::Mp4 { w: 1280, h: 720, fps: 60, codec: Codec::Hevc });
        assert_eq!((size, fps, ext), ((1280, 720), 60, "mp4"));
        assert_eq!(export_plan(&ExportOptions { fps: 25, ..mp4.clone() }, &meta, Aspect::W16H9).unwrap_err(), ERR_BAD_EXPORT);

        let gif = ExportOptions { kind: ExportFormat::Gif, fps: 15, gif_width: 480, loop_forever: false, ..ExportOptions::default() };
        let (kind, size, _, ext) = export_plan(&gif, &meta, Aspect::W16H9).unwrap();
        assert_eq!(kind, ExportKind::Gif { w: 480, h: 270, fps: 15, loop_forever: false });
        assert_eq!((size, ext), ((480, 270), "gif"));
        assert!(export_plan(&ExportOptions { gif_width: 500, ..gif.clone() }, &meta, Aspect::W16H9).is_err());
        assert!(export_plan(&ExportOptions { fps: 60, ..gif }, &meta, Aspect::W16H9).is_err());
        // Wire shape the UI sends.
        let o: ExportOptions =
            serde_json::from_str(r#"{"kind":"gif","target":"p1080","fps":20,"codec":"h264","gifWidth":854,"loopForever":true}"#)
                .unwrap();
        assert_eq!((o.kind, o.gif_width, o.fps), (ExportFormat::Gif, 854, 20));
    }

    #[test]
    fn preview_size_fits_and_keeps_shape() {
        assert_eq!(preview_size((1920, 1080)), (1280, 720));
        assert_eq!(preview_size((1080, 1920)), (406, 720));
        assert_eq!(preview_size((640, 360)), (640, 360), "never upscaled");
    }

    #[test]
    fn source_pos_is_camel_case() {
        let v = serde_json::to_value(SourcePos { clip: 1, source_ms: 2 }).unwrap();
        assert_eq!(v, serde_json::json!({"clip": 1, "sourceMs": 2}));
    }

    /// Live: the real helper against the test project. `MAC_FFMPEG_DIR=… cargo build -p record-render`
    /// then `IR_EDITOR_PROJECT=<dir> cargo test -p inspector-rust-core --lib editor_ipc_live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn editor_ipc_live() {
        let dir = PathBuf::from(std::env::var("IR_EDITOR_PROJECT").expect("IR_EDITOR_PROJECT"));
        let project = project::read_project(&dir).unwrap();
        let doc = editor::load(&project);
        let out = editor::output_size(&project.recording, doc.output.aspect, OutputTarget::P1080);
        let scene = scene_json(&dir, &project, &doc, out, PREVIEW_FPS);
        let client = RenderClient::spawn(Arc::new(|n| eprintln!("notice: {n:?}"))).unwrap();
        let (w, h) = preview_size(out);
        client.open(scene.clone(), w, h).unwrap();
        let mut got = None;
        for _ in 0..100 {
            if let FrameAnswer::Frame(f) = client.frame(0).unwrap() {
                got = Some(f);
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let f = got.expect("a preview frame");
        eprintln!("frame gen {} {}x{}", f.gen, f.w, f.h);
        let frames = thumb_frames(editor::programme_duration_ms(&doc), 4, THUMB_FPS);
        let t = client.thumbs(scene_json(&dir, &project, &doc, (640, 360), THUMB_FPS), THUMB_FPS, frames.clone(), THUMB_WIDTH).unwrap();
        eprintln!("thumbs {:?}", t.iter().map(|(i, j)| (*i, j.len())).collect::<Vec<_>>());
        assert_eq!(t.len(), frames.len());
        let started = Instant::now();
        client.shutdown();
        eprintln!("shutdown in {:?}", started.elapsed());
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
