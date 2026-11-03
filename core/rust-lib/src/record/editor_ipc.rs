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

use super::assets::AssetRoot;
use super::editor::{self, EditDoc, EditorView, OutputTarget, TakeMetas};
use super::peaks::{self, Peaks, SourcePeaks, WaveSpan};
use super::project::{self, CursorRecording, Project, ProjectStatus, ProjectSummary, RecordingMeta, MIC_FILE, PROJECT_FILE, SCREEN_FILE};
use super::render_client::{FrameAnswer, Notice, PeaksError, RenderClient, Sink, PEAKS_TIMEOUT};
use super::click_cues::click_hits_by_source;
use super::scene_json::{has_cursor_data, load_takes, project_asset, read_cursor, scene_json_with, screen_boxes, take_metas, TakeSources};
use std::collections::{HashMap, HashSet};
use super::voiceover::{self, Take, VoiceoverState};
use super::zoom_plan::plan_auto_zooms;
use base64::Engine;
use record_render_protocol::{Codec, Command, ExportKind, PreviewFrame};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const EDITOR_LABEL: &str = super::EDITOR_WINDOW;
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
/// `{ sessionId }` after `open_editor` replaced the session: an editor window that already
/// shows another project reloads (it must never send that project's doc to the new one).
pub const EV_OPENED: &str = "record-editor-opened";
/// `{ sessionId, waveforms }`: peaks arrived; the waveforms for the document as it is now.
pub const EV_WAVEFORMS: &str = "record-editor-waveforms";
pub const EV_EXPORT_PROGRESS: &str = "record-export-progress";
pub const EV_EXPORT_DONE: &str = "record-export-done";
pub const EV_EXPORT_FAILED: &str = "record-export-failed";

pub const ERR_NO_SESSION: &str = "record.editor.no_session";
pub const ERR_UNREADABLE: &str = "record.editor.unreadable";
pub const ERR_NO_VIDEO: &str = "record.editor.no_video";
pub const ERR_BAD_EXPORT: &str = "record.editor.bad_export";
pub const ERR_NO_DOWNLOADS: &str = "record.editor.no_downloads";
/// Opening a project while an export runs: the helper's render thread is busy until the
/// export ends, an `Open` behind it would time out and kill the export.
pub const ERR_OPEN_EXPORTING: &str = "record.editor.open_exporting";
/// Deleting the project that is open in the editor.
pub const ERR_DELETE_OPEN: &str = "record.editor.delete_open";

/// An imported background image is not a PNG or JPEG.
pub const ERR_IMPORT_TYPE: &str = "record.editor.import_type";
/// An imported pointer is not a PNG (a pointer needs transparency).
pub const ERR_IMPORT_CURSOR_TYPE: &str = "record.editor.import_cursor_type";
pub const ERR_IMPORT_TOO_BIG: &str = "record.editor.import_too_big";
pub const ERR_IMPORT_FAILED: &str = "record.editor.import_failed";
/// Imported images and pointers are copied into the project up to this size.
pub const MAX_IMPORT_BYTES: u64 = 25 * 1024 * 1024;

// ── assets (inspector) ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct WallpaperAsset {
    pub id: String,
    /// Absolute path of the picker thumbnail (the editor shows it through the asset protocol).
    pub thumb: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThemeAsset {
    pub id: String,
    /// Absolute path of the theme's `arrow.png`.
    pub preview: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorAssets {
    pub wallpapers: Vec<WallpaperAsset>,
    pub themes: Vec<ThemeAsset>,
    /// False when the take has no pointer telemetry: the inspector shows no pointer settings.
    pub has_cursor_data: bool,
    /// The click sound's press / release WAVs (the preview plays the export's files).
    pub click_down: Option<String>,
    pub click_up: Option<String>,
}

/// What the inspector offers: the shipped wallpapers and pointer themes (only those whose
/// files exist) and whether the take can draw a pointer.
pub fn editor_assets_for(assets: &AssetRoot, cursor: Option<&CursorRecording>) -> EditorAssets {
    let wallpapers = AssetRoot::wallpaper_ids()
        .iter()
        .filter_map(|id| assets.wallpaper_thumb(id).map(|p| WallpaperAsset { id: (*id).to_string(), thumb: p.to_string_lossy().into_owned() }))
        .collect();
    let themes = assets
        .cursor_theme_ids()
        .into_iter()
        .filter_map(|id| {
            let t = assets.cursor_theme(&id)?;
            let arrow = t.sprites.into_iter().find(|s| s.0 == "arrow")?.1;
            arrow.is_file().then(|| ThemeAsset { id, preview: arrow.to_string_lossy().into_owned() })
        })
        .collect();
    let (down, up) = assets.click_sounds();
    let file = |p: PathBuf| p.is_file().then(|| p.to_string_lossy().into_owned());
    EditorAssets {
        wallpapers,
        themes,
        has_cursor_data: has_cursor_data(cursor),
        click_down: file(down),
        click_up: file(up),
    }
}

/// What a file is imported as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    Image,
    Cursor,
}

fn sniff_image(path: &Path) -> Option<&'static str> {
    use std::io::Read;
    let mut head = [0u8; 8];
    let mut f = std::fs::File::open(path).ok()?;
    let n = f.read(&mut head).ok()?;
    if n >= 8 && head == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A] {
        Some("png")
    } else if n >= 3 && head[..3] == [0xFF, 0xD8, 0xFF] {
        Some("jpeg")
    } else {
        None
    }
}

/// A fresh v4-shaped id for an imported file.
fn random_id() -> String {
    let mut b: [u8; 16] = rand::random();
    b[6] = (b[6] & 0x0F) | 0x40;
    b[8] = (b[8] & 0x3F) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..])
}

/// Copies `src` into `<project>/assets/<uuid>.<ext>` and returns the file name (the plain
/// name the scene builder accepts). Only PNG and JPEG (pointers: only PNG), checked by
/// extension AND content, at most [`MAX_IMPORT_BYTES`]. The project keeps its own copy, so it
/// stays complete when moved.
pub fn import_asset_into(project_dir: &Path, src: &Path, kind: AssetKind) -> Result<String, String> {
    let ext = src.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    let wrong = || match kind {
        AssetKind::Image => ERR_IMPORT_TYPE.to_string(),
        AssetKind::Cursor => ERR_IMPORT_CURSOR_TYPE.to_string(),
    };
    let allowed: &[&str] = match kind {
        AssetKind::Image => &["png", "jpg", "jpeg"],
        AssetKind::Cursor => &["png"],
    };
    if !allowed.contains(&ext.as_str()) {
        return Err(wrong());
    }
    let meta = std::fs::metadata(src).map_err(|_| ERR_IMPORT_FAILED.to_string())?;
    if !meta.is_file() {
        return Err(ERR_IMPORT_FAILED.into());
    }
    if meta.len() > MAX_IMPORT_BYTES {
        return Err(ERR_IMPORT_TOO_BIG.into());
    }
    let content_ok = matches!((sniff_image(src), ext.as_str()), (Some("png"), "png") | (Some("jpeg"), "jpg" | "jpeg"));
    if !content_ok {
        return Err(wrong());
    }
    let dir = project_dir.join(super::scene_json::PROJECT_ASSETS_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| {
        tracing::warn!(target: "record::editor", "import: {}: {e}", dir.display());
        ERR_IMPORT_FAILED.to_string()
    })?;
    let name = format!("{}.{ext}", random_id());
    std::fs::copy(src, dir.join(&name)).map_err(|e| {
        tracing::warn!(target: "record::editor", "import: {}: {e}", src.display());
        ERR_IMPORT_FAILED.to_string()
    })?;
    Ok(name)
}

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
    takes: &TakeMetas,
    doc: &EditDoc,
) -> Result<ExportPlan, String> {
    match opts.kind {
        ExportFormat::Mp4 => {
            if !MP4_FPS.contains(&opts.fps) {
                return Err(ERR_BAD_EXPORT.into());
            }
            let (w, h) = editor::output_size(meta, takes, doc, opts.target);
            Ok((ExportKind::Mp4 { w, h, fps: opts.fps, codec: opts.codec }, (w, h), opts.fps, "mp4"))
        }
        ExportFormat::Gif => {
            if !GIF_FPS.contains(&opts.fps) || !GIF_WIDTHS.contains(&opts.gif_width) {
                return Err(ERR_BAD_EXPORT.into());
            }
            let (sw, sh) = editor::output_size(meta, takes, doc, OutputTarget::Source);
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
    /// Where the playhead is in the recording (the audio preview cues from this).
    pub source: Option<SourcePos>,
    /// Playback speed at `source` (1 when there is none).
    pub speed: f64,
}

fn locate(doc: &EditDoc, prog_ms: u64) -> Option<SourcePos> {
    editor::programme_to_source(doc, prog_ms).map(|(clip, source_ms)| SourcePos { clip, source_ms })
}

/// Speed at a mapped position (1 without one).
fn speed_of(doc: &EditDoc, pos: Option<SourcePos>) -> f64 {
    pos.map_or(1.0, |p| editor::speed_at(doc, p.clip, p.source_ms))
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
    /// Document clip index → the scene's clip index (clips of a missing take are not in the
    /// scene). Empty = the same index.
    scene_index: Vec<Option<usize>>,
    /// Counts continuous playbacks: play, pause, seek and stopping at the end each start a new
    /// run (an edit or a re-aim does not — the clock stays continuous).
    run: u64,
    /// A voiceover take follows one run: its id, and the position where that run ended.
    watch: Option<(u64, Option<u64>)>,
}

/// Where a watched run (a voiceover take) stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    /// No take is watched.
    Unwatched,
    /// Still playing; the programme position now.
    Running(u64),
    /// Paused, seeked, restarted or at the end; the position where it stopped.
    Ended(u64),
}

impl Transport {
    pub fn new(doc: &EditDoc) -> Self {
        Self {
            playing: false,
            base_ms: 0,
            started_ms: 0,
            clip: Some(0),
            programme_ms: editor::programme_duration_ms(doc),
            scene_index: Vec::new(),
            run: 0,
            watch: None,
        }
    }

    /// The current run ends here (before the transport changes): a watched run records the
    /// position it stopped at.
    fn break_run(&mut self, now_ms: u64) {
        let pos = self.position(now_ms);
        if let Some((run, end @ None)) = &mut self.watch {
            if *run == self.run {
                *end = Some(pos);
            }
        }
        self.run += 1;
    }

    /// Follows the current run (a voiceover take started with it).
    pub fn watch(&mut self) {
        self.watch = Some((self.run, None));
    }

    pub fn unwatch(&mut self) {
        self.watch = None;
    }

    pub fn watched(&self, now_ms: u64) -> RunState {
        match self.watch {
            None => RunState::Unwatched,
            Some((_, Some(end))) => RunState::Ended(end),
            Some((run, None)) if run == self.run && self.playing => RunState::Running(self.position(now_ms)),
            // A run that ended without passing through `break_run` cannot exist; be safe.
            Some((_, None)) => RunState::Ended(self.position(now_ms)),
        }
    }

    /// See [`editor::scene_clip_indices`].
    pub fn set_scene_index(&mut self, index: Vec<Option<usize>>) {
        self.scene_index = index;
    }

    fn scene_clip(&self, clip: usize) -> usize {
        self.scene_index.get(clip).copied().flatten().unwrap_or(clip)
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
            Command::SetActiveClip { clip: self.scene_clip(p.clip), source_sec: sec(p.source_ms) }
        } else {
            Command::SetTime { source_sec: sec(p.source_ms) }
        };
        (vec![cmd], Some(p))
    }

    pub fn play(&mut self, doc: &EditDoc, now_ms: u64) -> Vec<Command> {
        if self.programme_ms == 0 {
            return Vec::new();
        }
        self.break_run(now_ms);
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
        self.break_run(now_ms);
        self.base_ms = self.position(now_ms);
        self.playing = false;
        vec![Command::SetPlaying { playing: false }]
    }

    pub fn seek(&mut self, doc: &EditDoc, now_ms: u64, prog_ms: u64) -> (Vec<Command>, Option<SourcePos>) {
        self.break_run(now_ms);
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
            self.break_run(now_ms);
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
    /// The session this view belongs to; every `editor_update` names it.
    pub session_id: u64,
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
    /// `doc` as it plays ([`editor::playable`]): clips of a missing take last no time. The
    /// transport, the programme mapping and the scene use this; `doc` is what is stored.
    play: EditDoc,
    /// Resolved once per session: the bundle's asset root and the take's cursor telemetry.
    assets: AssetRoot,
    cursor: Option<CursorRecording>,
    /// The inserted takes (`<project>/takes/<id>/`), read at open and after an import.
    takes: TakeSources,
    transport: Transport,
    preview: (u32, u32),
    /// Stall-watchdog anchor (`now_ms` clock), see [`stall_check`].
    last_frame_ms: u64,
    dirty: bool,
    save_pending: bool,
    peaks: PeakStore,
}

/// The session's waveform peaks, keyed by cache name (`peaks::cache_name_for`: path, size,
/// mtime — a changed file is a new key). A file that failed is not asked for again.
#[derive(Default)]
struct PeakStore {
    ready: HashMap<String, Arc<Peaks>>,
    pending: HashSet<String>,
    failed: HashSet<String>,
}

/// How one file's peaks ended.
enum PeakOutcome {
    Ready(Peaks),
    /// The file's fault (no audio, unreadable, decode timed out): not asked for again while
    /// this version of the file is in the session.
    Failed(String),
    /// The helper is gone: not the file's fault; asked for again after the next edit.
    Gone(String),
}

impl PeakStore {
    /// Records `key`'s outcome; `false` = the helper is gone, stop asking for now.
    fn finish(&mut self, key: &str, outcome: PeakOutcome) -> bool {
        self.pending.remove(key);
        match outcome {
            PeakOutcome::Ready(p) => {
                self.ready.insert(key.to_string(), Arc::new(p));
                true
            }
            PeakOutcome::Failed(_) => {
                self.failed.insert(key.to_string());
                true
            }
            PeakOutcome::Gone(_) => false,
        }
    }
}

/// One file whose peaks are wanted: `mic` = a recorder `mic.wav` (read here), else decoded by
/// the helper.
#[derive(Debug, Clone, PartialEq)]
struct PeakJob {
    file: PathBuf,
    key: String,
    mic: bool,
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
        editor::output_size(&self.project.recording, &self.metas(), &self.doc, target)
    }

    fn metas(&self) -> TakeMetas {
        take_metas(&self.takes)
    }

    /// Re-derives the playing document and the scene's clip indices after `doc` or `takes`
    /// changed.
    fn sync_play(&mut self) {
        let metas = self.metas();
        self.play = editor::playable(&self.doc, &metas);
        self.transport.set_scene_index(editor::scene_clip_indices(&self.doc, &metas));
    }

    fn scene(&self, out: (u32, u32), fps: u32) -> Value {
        scene_json_with(&self.dir, &self.project, &self.play, out, fps, &self.assets, self.cursor.as_ref(), &self.takes)
    }

    fn preview_scene(&self) -> Value {
        self.scene(self.output(OutputTarget::P1080), PREVIEW_FPS)
    }

    /// The timeline view plus the preview scene's output size, the clicks and screen boxes.
    fn view(&self) -> EditorView {
        // Geometry of the playing document, the stored one as `doc` (a missing take's clip is
        // a zero-width span the UI marks; its bounds must come back unchanged on the next edit).
        let v = session_view(&self.project.recording, &self.play, self.cursor.as_ref(), &self.takes, self.output(OutputTarget::P1080));
        let waveforms = self.waveforms(&v.tracks);
        EditorView { doc: self.doc.clone(), waveforms, ..v }
    }

    /// A recording's folder and metadata (`None` = the project's own take).
    fn recording(&self, source: Option<&str>) -> Option<(PathBuf, &RecordingMeta)> {
        match source {
            None => Some((self.dir.clone(), &self.project.recording)),
            Some(id) => self.takes.get(id).map(|t| (editor::take_dir(&self.dir, Some(id)), &t.meta)),
        }
    }

    fn ready_peaks(&self, file: &Path) -> Option<Arc<Peaks>> {
        self.peaks.ready.get(&peaks::cache_name_for(file)?).cloned()
    }

    fn waveforms(&self, tracks: &[super::audio_tracks::TrackSpan]) -> Vec<WaveSpan> {
        peaks::project_waveforms(
            &self.play,
            tracks,
            |source| {
                let Some((dir, meta)) = self.recording(source) else { return SourcePeaks::default() };
                SourcePeaks {
                    mic: meta.mic.as_ref().and_then(|m| Some((self.ready_peaks(&dir.join(MIC_FILE))?, m.offset_ms))),
                    system: meta.system_audio.then(|| self.ready_peaks(&dir.join(SCREEN_FILE))).flatten(),
                }
            },
            |i| self.ready_peaks(&project_asset(&self.dir, &self.doc.audio_tracks.get(i)?.file)?),
        )
    }

    /// Every audio file the timeline draws a waveform of: each playing recording's mic and
    /// system sound, every track file.
    fn audio_files(&self) -> Vec<(PathBuf, bool)> {
        let mut out = Vec::new();
        let sources = self.play.clips.iter().filter(|c| c.end_ms > c.start_ms).map(|c| c.source.as_deref());
        let mut seen = HashSet::new();
        for source in sources {
            if !seen.insert(source) {
                continue;
            }
            let Some((dir, meta)) = self.recording(source) else { continue };
            if meta.mic.is_some() {
                out.push((dir.join(MIC_FILE), true));
            }
            if meta.system_audio {
                out.push((dir.join(SCREEN_FILE), false));
            }
        }
        for t in &self.doc.audio_tracks {
            if let Some(p) = project_asset(&self.dir, &t.file) {
                out.push((p, false));
            }
        }
        out
    }

    /// The files whose peaks are neither here, nor coming, nor failed — marked as coming.
    /// Entries for files (or file versions) the session no longer draws are dropped.
    fn take_peak_jobs(&mut self) -> Vec<PeakJob> {
        let files: Vec<(PathBuf, bool, String)> =
            self.audio_files().into_iter().filter_map(|(f, mic)| peaks::cache_name_for(&f).map(|k| (f, mic, k))).collect();
        let wanted: HashSet<&String> = files.iter().map(|(_, _, k)| k).collect();
        self.peaks.ready.retain(|k, _| wanted.contains(k));
        self.peaks.failed.retain(|k| wanted.contains(k));
        let mut jobs = Vec::new();
        for (file, mic, key) in files.iter().cloned() {
            let store = &self.peaks;
            if store.ready.contains_key(&key) || store.pending.contains(&key) || store.failed.contains(&key) {
                continue;
            }
            self.peaks.pending.insert(key.clone());
            jobs.push(PeakJob { file, key, mic });
        }
        jobs
    }

    fn init(&self, helper_error: Option<String>) -> EditorInit {
        EditorInit {
            session_id: self.id,
            project: self.summary(),
            meta: self.project.recording.clone(),
            view: self.view(),
            helper_error,
        }
    }
}

/// [`editor::view`] plus what only the session knows: the output size, the recorded clicks
/// on the programme clock and each clip's screen box in the output.
pub fn session_view(
    meta: &RecordingMeta,
    doc: &EditDoc,
    cursor: Option<&CursorRecording>,
    takes: &TakeSources,
    out: (u32, u32),
) -> EditorView {
    let cursor_of = |source: Option<&str>| match source {
        None => cursor,
        Some(id) => takes.get(id).and_then(|t| t.cursor.as_ref()),
    };
    let click_hits = click_hits_by_source(doc, |s| cursor_of(s).filter(|r| has_cursor_data(Some(r))));
    let screen_rects = screen_boxes(meta, &take_metas(takes), doc, out).iter().map(|(r, _)| [r.x, r.y, r.width, r.height]).collect();
    EditorView { output_size: [out.0, out.1], click_hits, screen_rects, takes: take_metas(takes), ..editor::view(doc) }
}

/// The document with the planner's zooms appended (`plan_auto_zooms`: new auto regions only,
/// the existing ones — manual or auto — are reserved and stay), marked as planned, normalised.
/// `None` without pointer data: nothing to plan, nothing changes. `cursor` is the project's
/// own take; clips of inserted takes get no planned zooms (their clicks are not in it).
pub fn with_auto_zooms(doc: &EditDoc, cursor: Option<&CursorRecording>, duration_ms: u64, takes: &TakeMetas) -> Option<EditDoc> {
    let rec = cursor.filter(|_| has_cursor_data(cursor))?;
    let mut next = doc.clone();
    let planned = plan_auto_zooms(doc, rec)
        .into_iter()
        .filter(|z| doc.clips.get(z.clip).is_some_and(|c| c.source.is_none()));
    next.zooms.extend(planned);
    next.auto_zoom_done = true;
    Some(editor::normalize(next, duration_ms, takes))
}

/// The stored document for an open. The first time a v2 document is opened (`auto_zoom_done`
/// unset) it gets the planner's zooms once when `auto_zoom` (`record.auto_zoom`) is on and the
/// take has pointer data — and is marked done either way, so zooms are never added behind the
/// user's back later (deleting them all and reopening plans nothing). `true` = changed (save).
pub fn doc_for_open(project: &Project, cursor: Option<&CursorRecording>, auto_zoom: bool, takes: &TakeMetas) -> (EditDoc, bool) {
    let doc = editor::load(project, takes);
    if doc.auto_zoom_done {
        return (doc, false);
    }
    let duration = project.recording.duration_ms;
    let planned = if auto_zoom { with_auto_zooms(&doc, cursor, duration, takes) } else { None };
    let next = planned.unwrap_or(editor::EditDoc { auto_zoom_done: true, ..doc });
    (next, true)
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

/// The confirmation after an export: the file name (the Finder shows where).
pub fn export_done_toast(path: &str) -> crate::status_toast::StatusToast {
    let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
    crate::status_toast::StatusToast { kind: "record".into(), on: true, title: crate::i18n::t("rec2.toast.exported"), subtitle: name }
}

fn sink(app: &AppHandle) -> Sink {
    let app = app.clone();
    Arc::new(move |notice| match notice {
        Notice::ExportProgress { frames, total } => {
            let _ = app.emit(EV_EXPORT_PROGRESS, serde_json::json!({ "frames": frames, "total": total }));
        }
        Notice::ExportDone { path } => {
            crate::commands::reveal_in_file_manager(Path::new(&path));
            crate::status_toast::announce_keeping_popup(&app, export_done_toast(&path));
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
        let client = Arc::new(RenderClient::spawn(sink(app), &AssetRoot::resolve(app).fonts_dir())?);
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
                let cmds = s.transport.reaim(&s.play, now_ms());
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
    if let Some(s) = s.as_mut() {
        save_session(s);
    }
}

/// Writes the session's document into ITS project dir (never another one), if it changed.
fn save_session(s: &mut Session) {
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

/// `hotkey::hide_popup` ends in `app.hide()`, which hides EVERY window of the process —
/// dismissing the popup (Esc, click-away, Ctrl+Space to paste) would take the editor with it,
/// and an Accessory app has no Dock icon to bring it back. Same opt-out as the iris overlays.
#[cfg(target_os = "macos")]
fn keep_on_app_hide(win: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    if let Ok(ns) = win.ns_window() {
        let w = ns as *mut AnyObject;
        if !w.is_null() {
            // SAFETY: `w` is this window's live NSWindow; we are on the main thread.
            unsafe {
                let _: () = msg_send![w, setCanHide: false];
            }
        }
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
                keep_on_app_hide(&win);
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
    open_allowed()?;
    let project = project::read_project(&dir).map_err(|e| {
        tracing::warn!(target: "record::editor", "{}: {e}", dir.display());
        ERR_UNREADABLE.to_string()
    })?;
    if project.status == ProjectStatus::Failed || !dir.join(SCREEN_FILE).is_file() {
        return Err(ERR_NO_VIDEO.into());
    }
    let assets = AssetRoot::resolve(app);
    let cursor = read_cursor(&dir);
    let takes = load_takes(&dir);
    let auto_zoom = app
        .try_state::<crate::db::DbHandle>()
        .map(|db| super::RecordConfig::load(&db).auto_zoom)
        .unwrap_or(true);
    let (doc, planned) = doc_for_open(&project, cursor.as_ref(), auto_zoom, &take_metas(&takes));
    // A running take of the previous project ends unsaved: its session is replaced.
    voiceover::abort();
    *VOICEOVER_DONE.lock().unwrap_or_else(|p| p.into_inner()) = None;
    // Takes an interrupted run left behind (crash, kill): no track uses them.
    let used: HashSet<String> = doc.audio_tracks.iter().map(|t| t.file.clone()).collect();
    let swept = voiceover::sweep_orphans(&dir.join(super::scene_json::PROJECT_ASSETS_DIR), &used, std::time::SystemTime::now(), voiceover::ORPHAN_AGE);
    if swept > 0 {
        tracing::info!(target: "record::voiceover", "removed {swept} voiceover file(s) no track uses");
    }
    // A different project open before: its edits are saved first.
    flush_save();
    let (scene, preview, init) = {
        let mut s = session();
        let play = editor::playable(&doc, &take_metas(&takes));
        let mut new = Session {
            id: NEXT_SESSION.fetch_add(1, Ordering::SeqCst),
            transport: Transport::new(&play),
            play,
            preview: (0, 0),
            assets,
            cursor,
            takes,
            dir,
            project,
            doc,
            last_frame_ms: now_ms(),
            // The first open's planning (or its "done" mark) is written right away.
            dirty: planned,
            save_pending: false,
            peaks: PeakStore::default(),
        };
        new.sync_play();
        let preview = preview_size(new.output(OutputTarget::P1080));
        let new = Session { preview, ..new };
        let out = (new.preview_scene(), preview, new.init(None));
        // An edit that reached the old session after the flush above goes into ITS project.
        if let Some(old) = s.as_mut() {
            save_session(old);
        }
        *s = Some(new);
        if let Some(cur) = s.as_mut() {
            save_session(cur);
        }
        out
    };
    show_window(app);
    let helper_error = ensure_client(app, false)
        .and_then(|c| c.open(scene, preview.0, preview.1))
        .err();
    if let Some(e) = &helper_error {
        tracing::warn!(target: "record::editor", "preview did not start: {e}");
    }
    // A window that already existed still shows the previous project: it reloads on this.
    let _ = app.emit(EV_OPENED, serde_json::json!({ "sessionId": init.session_id }));
    ensure_peaks(app);
    Ok(EditorInit { helper_error, ..init })
}

/// Refuses an open while the helper exports (see [`ERR_OPEN_EXPORTING`]).
pub fn open_refusal(exporting: bool) -> Result<(), String> {
    if exporting { Err(ERR_OPEN_EXPORTING.into()) } else { Ok(()) }
}

/// [`open_refusal`] for the live helper — checked before anything (popup, session) changes.
pub fn open_allowed() -> Result<(), String> {
    open_refusal(current_client().is_some_and(|c| c.exporting()))
}

/// The project dir open in the editor, if any.
pub fn open_dir() -> Option<PathBuf> {
    session().as_ref().map(|s| s.dir.clone())
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

/// Applies `doc` to the session it was made for. A doc from another session (an old
/// window's in-flight edit after a project switch) is refused and changes nothing.
fn apply_update(
    slot: &mut Option<Session>,
    session_id: u64,
    doc: EditDoc,
    now: u64,
) -> Result<(EditorView, Value, Vec<Command>), String> {
    let s = slot.as_mut().filter(|s| s.id == session_id).ok_or(ERR_NO_SESSION)?;
    let before = s.output(OutputTarget::P1080);
    // A take folder removed (or back) while the editor is open: re-read, it is cheap.
    s.takes = load_takes(&s.dir);
    s.doc = editor::normalize(doc, s.project.recording.duration_ms, &s.metas());
    s.sync_play();
    let mut cmds = s.transport.set_doc(&s.play, now);
    // Under `Auto` padding, frame and crop change the output's shape, not only an aspect
    // switch: every shape change reshapes the preview (the same size `open_editor` picks), so
    // the helper never draws the new scene onto the old canvas. The UI re-sends its own panel
    // fit afterwards (`editor_resize`).
    let after = s.output(OutputTarget::P1080);
    if after != before {
        let (w, h) = preview_size(after);
        s.preview = (w, h);
        cmds.insert(0, Command::Resize { w, h });
    }
    s.last_frame_ms = now;
    s.dirty = true;
    Ok((s.view(), s.preview_scene(), cmds))
}

pub fn update(app: &AppHandle, session_id: u64, doc: EditDoc) -> Result<EditorView, String> {
    let (view, scene, cmds) = {
        let mut guard = session();
        let out = apply_update(&mut guard, session_id, doc, now_ms())?;
        if let Some(s) = guard.as_mut() {
            schedule_save(s);
        }
        out
    };
    let client = ensure_client(app, true)?;
    // Commands first: a `Resize` (put first by `apply_update`) must reach the helper before the
    // scene, so no frame shows the new scene on the old canvas size.
    send_all(&client, &cmds)?;
    client.send(&Command::SetScene { scene })?;
    // A new track file (or an inserted take) gets its waveform.
    ensure_peaks(app);
    Ok(view)
}

/// Computes the peaks the open session still lacks on a background thread, then emits
/// [`EV_WAVEFORMS`]. `mic.wav` is read here; the other files go to the helper one at a time.
fn ensure_peaks(app: &AppHandle) {
    {
        let guard = session();
        if guard.is_none() {
            return;
        }
        // One worker at a time: a running one picks up whatever is new before it stops (the
        // flag only changes under the session lock, so nothing is missed between the two).
        if PEAKS_WORKER.swap(true, Ordering::SeqCst) {
            return;
        }
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("ir-record-peaks".into()).spawn(move || peaks_worker(&app));
    if let Err(e) = spawned {
        tracing::warn!(target: "record::editor", "peaks thread did not start: {e}");
        PEAKS_WORKER.store(false, Ordering::SeqCst);
    }
}

/// Set while the `ir-record-peaks` worker runs.
static PEAKS_WORKER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The worker: takes the open session's missing peaks, computes them one file at a time
/// (`mic.wav` here, the rest on the helper's decode thread — the preview never waits), emits
/// [`EV_WAVEFORMS`], and repeats until nothing is missing. A gone helper stops it; the next
/// edit starts it again.
fn peaks_worker(app: &AppHandle) {
    loop {
        let (id, dir, jobs) = {
            let mut guard = session();
            let Some(s) = guard.as_mut() else {
                PEAKS_WORKER.store(false, Ordering::SeqCst);
                return;
            };
            let jobs = s.take_peak_jobs();
            if jobs.is_empty() {
                PEAKS_WORKER.store(false, Ordering::SeqCst);
                return;
            }
            (s.id, s.dir.clone(), jobs)
        };
        let mut stop = false;
        let mut jobs = jobs.into_iter();
        for job in jobs.by_ref() {
            let cache = dir.join(&job.key);
            let outcome = if job.mic {
                match peaks::wav_peaks(&job.file, &cache) {
                    Ok(p) => PeakOutcome::Ready(p),
                    Err(e) => PeakOutcome::Failed(e.to_string()),
                }
            } else {
                match current_client() {
                    Some(c) => match c.peaks(job.file.to_string_lossy().into_owned(), cache.to_string_lossy().into_owned(), PEAKS_TIMEOUT) {
                        Ok((_, duration_ms)) => match peaks::read_cache(&cache) {
                            Some(data) => PeakOutcome::Ready(Peaks { data, duration_ms }),
                            None => PeakOutcome::Failed("peaks cache unreadable".into()),
                        },
                        Err(PeaksError::Failed(m)) => PeakOutcome::Failed(m),
                        Err(PeaksError::TimedOut) => PeakOutcome::Failed("decode timed out".into()),
                        Err(PeaksError::Gone(m)) => PeakOutcome::Gone(m),
                    },
                    None => PeakOutcome::Gone("no render helper".into()),
                }
            };
            if matches!(outcome, PeakOutcome::Ready(_)) {
                peaks::remove_older_caches(&dir, &job.key);
            }
            if let PeakOutcome::Failed(e) | PeakOutcome::Gone(e) = &outcome {
                tracing::warn!(target: "record::editor", "no waveform for {}: {e}", job.file.display());
            }
            let mut guard = session();
            let Some(s) = guard.as_mut().filter(|s| s.id == id) else { break };
            if !s.peaks.finish(&job.key, outcome) {
                stop = true;
                break;
            }
        }
        if stop {
            let mut guard = session();
            if let Some(s) = guard.as_mut().filter(|s| s.id == id) {
                for job in jobs {
                    s.peaks.pending.remove(&job.key);
                }
            }
            PEAKS_WORKER.store(false, Ordering::SeqCst);
            drop(guard);
            return;
        }
        let waveforms = {
            let guard = session();
            guard.as_ref().filter(|s| s.id == id).map(|s| s.waveforms(&super::audio_tracks::track_entries(&s.play)))
        };
        if let Some(waveforms) = waveforms {
            let _ = app.emit(EV_WAVEFORMS, serde_json::json!({ "sessionId": id, "waveforms": waveforms }));
        }
    }
}

/// `editor_auto_zoom`: appends the planner's zooms to the open document (one undo step in the
/// UI, which records the returned doc). Without pointer data the view comes back unchanged.
pub fn auto_zoom(app: &AppHandle, session_id: u64) -> Result<EditorView, String> {
    let next = {
        let guard = session();
        let s = guard.as_ref().filter(|s| s.id == session_id).ok_or(ERR_NO_SESSION)?;
        match with_auto_zooms(&s.doc, s.cursor.as_ref(), s.project.recording.duration_ms, &s.metas()) {
            Some(doc) => doc,
            None => return Ok(s.view()),
        }
    };
    update(app, session_id, next)
}

/// `editor_move_clip`: clip `from` to position `to` with everything on it (one undo step in
/// the UI, which records the returned doc).
pub fn move_clip(app: &AppHandle, session_id: u64, from: usize, to: usize) -> Result<EditorView, String> {
    let next = {
        let guard = session();
        let s = guard.as_ref().filter(|s| s.id == session_id).ok_or(ERR_NO_SESSION)?;
        editor::move_clip(&s.doc, from, to)
    };
    update(app, session_id, next)
}

/// `editor_duplicate_clip`: a copy of clip `index` right behind it (one undo step in the UI).
pub fn duplicate_clip(app: &AppHandle, session_id: u64, index: usize) -> Result<EditorView, String> {
    let next = {
        let guard = session();
        let s = guard.as_ref().filter(|s| s.id == session_id).ok_or(ERR_NO_SESSION)?;
        editor::duplicate_clip(&s.doc, index)
    };
    update(app, session_id, next)
}

/// `editor_import_take`: copies the finished project at `src` into the open project
/// (`takes/<id>/`, see [`super::session::import_take_into`]) and appends one clip over all of
/// it. The copy runs without the session lock (it can take a moment); a session switched in
/// the meantime refuses the result (`ERR_NO_SESSION`), the copied folder stays unused.
pub fn import_take(app: &AppHandle, session_id: u64, src: &Path) -> Result<EditorView, String> {
    let dir = {
        let guard = session();
        guard.as_ref().filter(|s| s.id == session_id).ok_or(ERR_NO_SESSION)?.dir.clone()
    };
    let (id, meta) = super::session::import_take_into(&dir, src)?;
    let next = {
        let mut guard = session();
        let s = guard.as_mut().filter(|s| s.id == session_id).ok_or(ERR_NO_SESSION)?;
        s.takes = load_takes(&s.dir);
        if !s.takes.contains_key(&id) {
            return Err(super::session::ERR_TAKE_COPY.into());
        }
        editor::append_take(&s.doc, &id, &meta)
    };
    update(app, session_id, next)
}

pub fn seek(app: &AppHandle, programme_ms: u64) -> Result<Option<SourcePos>, String> {
    let (cmds, pos) = {
        let mut guard = session();
        let s = guard.as_mut().ok_or(ERR_NO_SESSION)?;
        s.last_frame_ms = now_ms();
        s.transport.seek(&s.play, now_ms(), programme_ms)
    };
    let client = ensure_client(app, true)?;
    send_all(&client, &cmds)?;
    Ok(pos)
}

/// Programme → source on the open document, without moving the playhead (trim drags).
pub fn to_source(programme_ms: u64) -> Result<Option<SourcePos>, String> {
    let guard = session();
    let s = guard.as_ref().ok_or(ERR_NO_SESSION)?;
    Ok(locate(&s.play, programme_ms))
}

/// Source → programme on the open document, without moving the playhead.
pub fn to_programme(clip: usize, source_ms: u64) -> Result<u64, String> {
    let guard = session();
    let s = guard.as_ref().ok_or(ERR_NO_SESSION)?;
    Ok(editor::source_to_programme(&s.play, clip, source_ms))
}

fn playback(s: &Session) -> PlaybackState {
    let client = HOST.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let position_ms = s.transport.position(now_ms());
    let source = locate(&s.play, position_ms);
    PlaybackState {
        playing: s.transport.playing(),
        position_ms,
        source,
        speed: speed_of(&s.play, source),
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
        if playing { s.transport.play(&s.play, now_ms()) } else { s.transport.pause(now_ms()) }
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
        let cmds = s.transport.tick(&s.play, now_ms());
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
        let frames = thumb_frames(editor::programme_duration_ms(&s.play), count, THUMB_FPS);
        (s.scene(out, THUMB_FPS), frames)
    };
    let client = ensure_client(app, true)?;
    let rendered = client.thumbs(scene, THUMB_FPS, frames.clone(), THUMB_WIDTH);
    // The helper paused its preview while rendering, the app clock ran on: put the preview
    // back where the playhead is now.
    let cmds = {
        let mut guard = session();
        guard.as_mut().filter(|s| s.transport.playing()).map(|s| {
            s.last_frame_ms = now_ms();
            s.transport.reaim(&s.play, now_ms())
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
        let (kind, size, fps, ext) = export_plan(&opts, &s.project.recording, &s.metas(), &s.doc)?;
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
        (s.scene(size, fps), kind, path)
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
    // A running take of the closing session has nowhere to go (an ended one was already laid
    // under the programme by itself).
    voiceover::abort();
    *VOICEOVER_DONE.lock().unwrap_or_else(|p| p.into_inner()) = None;
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

/// The inspector's wallpapers, pointer themes and whether the take has pointer data. The
/// asset folders are added to the asset-protocol scope (they live in the app bundle, or in
/// the source tree in dev builds), so the editor can show the thumbnails.
pub fn assets(app: &AppHandle) -> Result<EditorAssets, String> {
    let (out, root) = {
        let guard = session();
        let s = guard.as_ref().ok_or(ERR_NO_SESSION)?;
        (editor_assets_for(&s.assets, s.cursor.as_ref()), s.assets.dir.clone())
    };
    let scope = app.asset_protocol_scope();
    for (dir, recursive) in [
        (root.join("wallpapers").join("thumbs"), false),
        (root.join("cursors"), true),
        (root.join("sounds"), false),
    ] {
        if let Err(e) = scope.allow_directory(&dir, recursive) {
            tracing::warn!(target: "record::editor", "asset scope {}: {e}", dir.display());
        }
    }
    Ok(out)
}

/// Copies an image or pointer into the open project (see [`import_asset_into`]).
pub fn import_asset(session_id: u64, src: &Path, kind: AssetKind) -> Result<String, String> {
    let dir = {
        let guard = session();
        guard.as_ref().filter(|s| s.id == session_id).ok_or(ERR_NO_SESSION)?.dir.clone()
    };
    import_asset_into(&dir, src, kind)
}

// ── voiceover ──────────────────────────────────────────────────────────────

/// Serialises finishing a take: [`voiceover_stop`] and the level thread's auto-finish never
/// race over one take, and a stop that comes second finds the auto-finished result.
static VOICEOVER_FINISH: Mutex<()> = Mutex::new(());
/// A take that ended by itself and was laid under the programme (or failed to be): handed
/// out once by [`voiceover_stop`] for its session.
static VOICEOVER_DONE: Mutex<Option<(u64, Result<EditorView, String>)>> = Mutex::new(None);

/// Where session `session_id`'s watched run stands, in ms into a take started at `start_ms`.
fn take_progress(session_id: u64, start_ms: u64) -> voiceover::Progress {
    let guard = session();
    let Some(s) = guard.as_ref().filter(|s| s.id == session_id) else { return voiceover::Progress::Gone };
    match s.transport.watched(now_ms()) {
        RunState::Unwatched => voiceover::Progress::Waiting,
        RunState::Running(pos) => voiceover::Progress::Running(pos.saturating_sub(start_ms)),
        RunState::Ended(pos) => voiceover::Progress::Ended(pos.saturating_sub(start_ms)),
    }
}

/// Starts a voiceover take at `programme_ms` of session `session_id`: the microphone records
/// into `<project>/assets/voiceover-<ts>.wav` while the preview plays from there (started here,
/// together with the take's clock). The take follows that one run of the transport: a pause,
/// seek, stall or the programme's end ends it, and it is laid under the programme by itself
/// ([`voiceover::EV_FINISHED`]); the UI may also end it with [`voiceover_stop`].
pub fn voiceover_start(app: &AppHandle, session_id: u64, programme_ms: u64) -> Result<VoiceoverState, String> {
    if voiceover::ACTIVE.lock().is_some() {
        return Err(voiceover::ERR_BUSY.into());
    }
    *VOICEOVER_DONE.lock().unwrap_or_else(|p| p.into_inner()) = None;
    let (dir, plan) = {
        let guard = session();
        let s = guard.as_ref().filter(|s| s.id == session_id).ok_or(ERR_NO_SESSION)?;
        (s.dir.clone(), voiceover::plan(&s.play, programme_ms)?)
    };
    voiceover::check_access(voiceover::mic_access())?;
    let wall_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    let take = Take::create(session_id, &dir, plan, wall_ms)?;
    let handle = app.clone();
    let opened = take.open_mic(app.clone(), move || take_progress(session_id, programme_ms), move || {
        let _guard = VOICEOVER_FINISH.lock().unwrap_or_else(|p| p.into_inner());
        let Ok(take) = voiceover::take_for(&mut voiceover::ACTIVE.lock(), session_id) else { return };
        let result = finish_take(&handle, session_id, take);
        if let Err(e) = &result {
            tracing::warn!(target: "record::voiceover", "take ended without a track: {e}");
        }
        *VOICEOVER_DONE.lock().unwrap_or_else(|p| p.into_inner()) = Some((session_id, result));
        let _ = handle.emit(voiceover::EV_FINISHED, serde_json::json!({ "sessionId": session_id }));
    });
    if let Err(e) = opened {
        take.discard();
        return Err(e);
    }
    // No chunk at all: no input, or a refused permission. Never a silent take.
    if !take.wait_for_input(voiceover::INPUT_TIMEOUT) {
        take.discard();
        return Err(voiceover::silent_input_error(voiceover::mic_access()).into());
    }
    let arm = take.arm_handle();
    let state = voiceover::install(&mut voiceover::ACTIVE.lock(), take)?;
    let cmds = {
        let mut guard = session();
        match guard.as_mut().filter(|s| s.id == session_id) {
            None => None,
            Some(s) => {
                let now = now_ms();
                s.last_frame_ms = now;
                let (mut cmds, _) = s.transport.seek(&s.play, now, programme_ms);
                arm.arm();
                cmds.extend(s.transport.play(&s.play, now));
                s.transport.watch();
                Some(cmds)
            }
        }
    };
    let started = match cmds {
        None => Err(ERR_NO_SESSION.to_string()),
        Some(cmds) => ensure_client(app, true).and_then(|c| send_all(&c, &cmds)),
    };
    if let Err(e) = started {
        voiceover::abort();
        if let Some(s) = session().as_mut() {
            s.transport.unwatch();
        }
        return Err(e);
    }
    Ok(state)
}

/// Ends the take: at the transport's position now (or where its run already stopped), pauses
/// the preview and lays the take under the programme where it was recorded — a voiceover track
/// (0 dB, no fades, the recorded length) anchored at the picture of the take's start. The UI
/// records the answer as one undo step. A take that already ended by itself is handed out here.
pub fn voiceover_stop(app: &AppHandle, session_id: u64) -> Result<EditorView, String> {
    let _guard = VOICEOVER_FINISH.lock().unwrap_or_else(|p| p.into_inner());
    match voiceover::take_for(&mut voiceover::ACTIVE.lock(), session_id) {
        Ok(take) => finish_take(app, session_id, take),
        Err(e) => {
            let mut done = VOICEOVER_DONE.lock().unwrap_or_else(|p| p.into_inner());
            match done.take() {
                Some((id, result)) if id == session_id => result,
                other => {
                    *done = other;
                    Err(e)
                }
            }
        }
    }
}

/// Ends the running take without a track: the file is removed and the preview paused.
pub fn voiceover_cancel(_app: &AppHandle, session_id: u64) -> Result<(), String> {
    let _guard = VOICEOVER_FINISH.lock().unwrap_or_else(|p| p.into_inner());
    let take = voiceover::take_for(&mut voiceover::ACTIVE.lock(), session_id)?;
    take.discard();
    let cmds = end_run(session_id).1;
    if let Some(c) = current_client() {
        let _ = send_all(&c, &cmds);
    }
    Ok(())
}

/// Stops following the take's run: where it stopped (or the playhead now, which then pauses).
/// `None` = the session is gone.
fn end_run(session_id: u64) -> (Option<u64>, Vec<Command>) {
    let mut guard = session();
    let Some(s) = guard.as_mut().filter(|s| s.id == session_id) else { return (None, Vec::new()) };
    let now = now_ms();
    let (pos, cmds) = match s.transport.watched(now) {
        RunState::Running(pos) => (pos, s.transport.pause(now)),
        RunState::Ended(pos) => (pos, Vec::new()),
        RunState::Unwatched => (s.transport.position(now), Vec::new()),
    };
    s.transport.unwatch();
    (Some(pos), cmds)
}

/// Finishes `take` (see [`voiceover_stop`]); the file is removed whenever no track keeps it.
fn finish_take(app: &AppHandle, session_id: u64, take: Take) -> Result<EditorView, String> {
    let (pos, cmds) = end_run(session_id);
    if let Some(c) = current_client() {
        let _ = send_all(&c, &cmds);
    }
    let Some(pos) = pos else {
        take.discard();
        return Err(ERR_NO_SESSION.into());
    };
    // Permission refused while the take ran: what it holds is silence.
    if voiceover::mic_denied() {
        take.discard();
        return Err(voiceover::ERR_MIC_DENIED.into());
    }
    let plan = take.plan.clone();
    let path = take.path().to_path_buf();
    let (file, span) = take.finish_at(voiceover::span_from_run(pos, &plan))?;
    let next = {
        let guard = session();
        guard.as_ref().filter(|s| s.id == session_id).map(|s| doc_with_voiceover(&s.doc, &s.play, &plan, &file, span))
    };
    let result = match next {
        Some(next) => update(app, session_id, next),
        None => Err(ERR_NO_SESSION.into()),
    };
    if result.is_err() && !voiceover_kept(session_id, &file) {
        let _ = std::fs::remove_file(&path);
    }
    result
}

/// Does the session's document hold a track of `file` (an `update` that failed after the
/// document was applied keeps it)?
fn voiceover_kept(session_id: u64, file: &str) -> bool {
    session().as_ref().filter(|s| s.id == session_id).is_some_and(|s| s.doc.audio_tracks.iter().any(|t| t.file == file))
}

/// `doc` with the finished take added as a voiceover track (see [`voiceover::anchor`]).
fn doc_with_voiceover(doc: &EditDoc, play: &EditDoc, plan: &voiceover::Plan, file: &str, span_ms: u64) -> EditDoc {
    let mut next = doc.clone();
    if let Some((clip, source_ms)) = voiceover::anchor(doc, play, plan) {
        next.audio_tracks.push(voiceover::voiceover_track(random_id(), file, clip, source_ms, span_ms));
    }
    next
}

/// App exit (`RunEvent::Exit`): save and stop the helper.
pub fn shutdown(app: &AppHandle) {
    close(app, false, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_done_toast_names_the_file() {
        let t = export_done_toast("/Users/x/Downloads/Demo 2026-11-02 09.05.mp4");
        assert_eq!(t.kind, "record");
        assert!(t.on);
        assert_eq!(t.subtitle, "Demo 2026-11-02 09.05.mp4");
        assert!(!t.title.is_empty() && t.title != "rec2.toast.exported");
    }

    #[test]
    fn export_done_shows_a_toast_and_reveals_once() {
        let src = include_str!("editor_ipc.rs");
        let arm = src.split("Notice::ExportDone { path } =>").nth(1).unwrap().split("Notice::ExportFailed").next().unwrap();
        assert_eq!(arm.matches("reveal_in_file_manager").count(), 1);
        assert!(arm.contains("announce_keeping_popup(&app, export_done_toast(&path))"));
    }
    use crate::record::editor::{Aspect, EditClip, OutputShape, SpeedRegion, EDITOR_VERSION};
    use chrono::NaiveDate;

    fn import_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ir-editor-import-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const PNG_BYTES: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
    const JPEG_BYTES: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0];

    #[test]
    fn import_rejects_non_image() {
        let dir = import_dir("reject");
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let txt = dir.join("notes.txt");
        std::fs::write(&txt, b"hello").unwrap();
        assert_eq!(import_asset_into(&project, &txt, AssetKind::Image).unwrap_err(), ERR_IMPORT_TYPE);
        // A text file renamed to .png is still not an image.
        let fake = dir.join("fake.png");
        std::fs::write(&fake, b"not a png at all").unwrap();
        assert_eq!(import_asset_into(&project, &fake, AssetKind::Image).unwrap_err(), ERR_IMPORT_TYPE);
        // A pointer must be a PNG (it needs transparency).
        let jpg = dir.join("photo.jpg");
        std::fs::write(&jpg, JPEG_BYTES).unwrap();
        assert_eq!(import_asset_into(&project, &jpg, AssetKind::Cursor).unwrap_err(), ERR_IMPORT_CURSOR_TYPE);
        // Too big.
        let big = dir.join("big.png");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(MAX_IMPORT_BYTES + 1).unwrap();
        drop(f);
        let mut head = std::fs::OpenOptions::new().write(true).open(&big).unwrap();
        std::io::Write::write_all(&mut head, PNG_BYTES).unwrap();
        drop(head);
        assert_eq!(import_asset_into(&project, &big, AssetKind::Image).unwrap_err(), ERR_IMPORT_TOO_BIG);
        assert!(!project.join(crate::record::scene_json::PROJECT_ASSETS_DIR).exists(), "nothing was copied");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_copies_into_project_assets() {
        let dir = import_dir("copy");
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let jpg = dir.join("Holiday Photo.JPEG");
        std::fs::write(&jpg, JPEG_BYTES).unwrap();
        let name = import_asset_into(&project, &jpg, AssetKind::Image).unwrap();
        assert!(name.ends_with(".jpeg"), "{name}");
        assert!(!name.contains('/') && !name.starts_with('.'));
        let copied = crate::record::scene_json::project_asset(&project, &name).expect("a plain name the scene builder accepts");
        assert_eq!(std::fs::read(copied).unwrap(), JPEG_BYTES);
        let png = dir.join("arrow.png");
        std::fs::write(&png, PNG_BYTES).unwrap();
        let a = import_asset_into(&project, &png, AssetKind::Cursor).unwrap();
        let b = import_asset_into(&project, &png, AssetKind::Cursor).unwrap();
        assert_ne!(a, b, "every import gets its own file");
        assert!(a.ends_with(".png"));
        assert!(std::fs::read(&png).is_ok(), "the source stays where it was");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn editor_assets_list_wallpapers_themes_and_cursor_data() {
        let a = editor_assets_for(&AssetRoot::dev(), None);
        assert_eq!(a.wallpapers.len(), 8);
        assert_eq!(a.wallpapers[0].id, "01");
        assert!(a.wallpapers.iter().all(|w| Path::new(&w.thumb).is_file()), "{:?}", a.wallpapers);
        assert_eq!(a.themes.len(), 7);
        assert_eq!(a.themes[0].id, "default");
        assert!(a.themes.iter().all(|t| t.preview.ends_with("arrow.png") && Path::new(&t.preview).is_file()));
        assert!(!a.has_cursor_data);
        let json = serde_json::to_value(&a).unwrap();
        assert!(json.get("hasCursorData").is_some(), "{json}");
    }

    fn doc(clips: &[(u64, u64)]) -> EditDoc {
        EditDoc {
            version: EDITOR_VERSION,
            clips: clips.iter().map(|&(s, e)| EditClip::main(s, e)).collect(),
            speed: Vec::new(),
            crop: vec![None; clips.len()],
            output: OutputShape::default(),
            mic_gain_db: 0.0,
            appearance: crate::record::editor::Appearance::plain(),
            cursor: crate::record::editor::CursorStyle::plain(),
            zooms: Vec::new(),
            auto_zoom_done: true,
            annotations: Vec::new(),
            audio_tracks: Vec::new(),
            system_gain_db: 0.0,
        }
    }

    fn tmp_project(name: &str, id: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ir-editor-ipc-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = Project {
            schema: 1,
            id: id.into(),
            title: id.into(),
            created_at: 0,
            updated_at: 0,
            status: ProjectStatus::Ready,
            failure: None,
            recording: RecordingMeta { width: 1920, height: 1080, fps: 30, duration_ms: 10_000, ..Default::default() },
            editor: serde_json::json!({}),
        };
        project::write_atomic(&dir, PROJECT_FILE, &p).unwrap();
        dir
    }

    fn session_for(dir: &Path, id: u64) -> Session {
        let project = project::read_project(dir).unwrap();
        let doc = editor::load(&project, &TakeMetas::new());
        let mut s = Session {
            id,
            dir: dir.to_path_buf(),
            transport: Transport::new(&doc),
            play: doc.clone(),
            preview: (640, 360),
            assets: AssetRoot::dev(),
            cursor: None,
            takes: load_takes(dir),
            project,
            doc,
            last_frame_ms: 0,
            dirty: false,
            save_pending: false,
            peaks: PeakStore::default(),
        };
        s.sync_play();
        s
    }

    /// The session asks for each audio file's peaks once (mic read here, the rest from the
    /// helper), and the view's waveforms follow the peaks it has.
    #[test]
    fn session_asks_for_peaks_once_and_draws_what_it_has() {
        let dir = tmp_project("peaks", "take");
        let mut p = project::read_project(&dir).unwrap();
        p.recording.system_audio = true;
        p.recording.mic = Some(crate::record::project::MicMeta { offset_ms: 0, sample_rate: 48_000 });
        project::write_atomic(&dir, PROJECT_FILE, &p).unwrap();
        std::fs::write(dir.join(SCREEN_FILE), b"mp4").unwrap();
        let mut sink = crate::record::wav::WavSink::create(&dir.join(MIC_FILE), 48_000).unwrap();
        sink.set_origin(0);
        sink.push(0, &vec![0.5f32; 48_000]).unwrap();
        sink.finish().unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/song.m4a"), b"m4a").unwrap();
        let mut s = session_for(&dir, 1);
        let mut d = s.doc.clone();
        d.audio_tracks = vec![crate::record::editor::AudioTrack {
            id: "m".into(),
            kind: crate::record::editor::TrackKind::Music,
            file: "song.m4a".into(),
            clip: 0,
            start_ms: 0,
            duration_ms: 4_000,
            offset_ms: 0,
            span_ms: None,
            gain_db: -18.0,
            looped: false,
            fade_in_ms: 0,
            fade_out_ms: 0,
            muted: false,
            label: String::new(),
        }];
        s.doc = d;
        s.sync_play();
        let jobs = s.take_peak_jobs();
        let names: Vec<_> = jobs.iter().map(|j| (j.file.file_name().unwrap().to_string_lossy().into_owned(), j.mic)).collect();
        assert_eq!(names, [("mic.wav".to_string(), true), ("screen.mp4".to_string(), false), ("song.m4a".to_string(), false)]);
        assert!(s.take_peak_jobs().is_empty(), "pending files are not asked for twice");
        // Nothing ready yet: no waveforms, but the tracks are there.
        let v = s.view();
        assert!(v.waveforms.is_empty());
        assert_eq!(v.tracks.len(), 1);
        // The mic arrives (read here): its waveform covers the clip.
        let mic = &jobs[0];
        let peaks = peaks::wav_peaks(&mic.file, &dir.join(&mic.key)).unwrap();
        s.peaks.pending.remove(&mic.key);
        s.peaks.ready.insert(mic.key.clone(), Arc::new(peaks));
        let v = s.view();
        assert_eq!(v.waveforms.len(), 1);
        assert_eq!(v.waveforms[0].kind, peaks::WaveKind::Mic);
        assert_eq!((v.waveforms[0].prog_start_ms, v.waveforms[0].prog_end_ms), (0.0, 10_000.0));
        assert!((v.waveforms[0].peaks[1] - 0.5).abs() < 1e-3);
        // A failed file is not asked for again (review fix 3: a timeout counts as failed).
        assert!(s.peaks.finish(&jobs[1].key, PeakOutcome::Failed("decode timed out".into())));
        // A gone helper is not the file's fault: the worker stops, the file is asked for again.
        assert!(!s.peaks.finish(&jobs[2].key, PeakOutcome::Gone("helper died".into())));
        let again = s.take_peak_jobs();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].key, jobs[2].key);
        // A file that changes is a new version: the old entry is dropped, the new one asked for.
        let mic_key = jobs[0].key.clone();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut sink = crate::record::wav::WavSink::create(&dir.join(MIC_FILE), 48_000).unwrap();
        sink.set_origin(0);
        sink.push(0, &vec![0.25f32; 24_000]).unwrap();
        sink.finish().unwrap();
        let next = s.take_peak_jobs();
        assert_eq!(next.len(), 1);
        assert!(next[0].mic && next[0].key != mic_key);
        assert!(!s.peaks.ready.contains_key(&mic_key), "the stale version is dropped");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn sample(t: u64, cx: f64, cy: f64, kind: Option<crate::record::project::Interaction>) -> crate::record::project::CursorSample {
        crate::record::project::CursorSample {
            time_ms: t,
            cx,
            cy,
            visible: None,
            asset_id: None,
            cursor_type: None,
            interaction_type: kind,
        }
    }

    /// A 20-s take: pointer parked, clicks at 5 s and 12 s (two separate shots).
    fn clicks_take() -> CursorRecording {
        use crate::record::project::{CursorProvider, Interaction};
        let mut samples: Vec<_> = (0..200).map(|i| sample(i * 100, 0.3, 0.4, None)).collect();
        samples.push(sample(5_000, 0.3, 0.4, Some(Interaction::Click)));
        samples.push(sample(5_080, 0.3, 0.4, Some(Interaction::Mouseup)));
        samples.push(sample(12_000, 0.7, 0.6, Some(Interaction::Click)));
        samples.push(sample(12_080, 0.7, 0.6, Some(Interaction::Mouseup)));
        samples.sort_by_key(|s| s.time_ms);
        CursorRecording { version: 1, provider: CursorProvider::Native, samples, assets: vec![] }
    }

    fn manual_zoom(start: u64, end: u64) -> crate::record::editor::ZoomRegion {
        crate::record::editor::ZoomRegion {
            id: "mine".into(),
            clip: 0,
            start_ms: start,
            end_ms: end,
            depth: 2,
            custom_scale: None,
            focus_x: 0.5,
            focus_y: 0.5,
            focus_mode: crate::record::editor::FocusMode::Manual,
            origin: crate::record::editor::ZoomOrigin::Manual,
            hide_cursor: false,
        }
    }

    #[test]
    fn auto_zoom_appends_and_keeps_manual() {
        use crate::record::editor::ZoomOrigin;
        let mut d = doc(&[(0, 20_000)]);
        d.zooms = vec![manual_zoom(18_000, 19_500)];
        let rec = clicks_take();
        let next = with_auto_zooms(&d, Some(&rec), 20_000, &TakeMetas::new()).expect("pointer data: planned");
        assert!(next.auto_zoom_done);
        assert_eq!(next.zooms.iter().filter(|z| z.origin == ZoomOrigin::Manual).count(), 1);
        assert_eq!(next.zooms.iter().find(|z| z.id == "mine"), d.zooms.first(), "the manual zoom is untouched");
        let autos: Vec<_> = next.zooms.iter().filter(|z| z.origin == ZoomOrigin::Auto).collect();
        assert_eq!(autos.len(), 2, "one zoom per click: {autos:?}");
        assert!(autos[0].start_ms <= 5_000 && autos[0].end_ms > 5_000);
        assert!(autos[1].start_ms <= 12_000 && autos[1].end_ms > 12_000);
        // Pressing it again plans nothing new: every existing zoom is reserved.
        let again = with_auto_zooms(&next, Some(&rec), 20_000, &TakeMetas::new()).unwrap();
        assert_eq!(again.zooms, next.zooms);
    }

    #[test]
    fn auto_zoom_without_cursor_data_is_noop() {
        let d = doc(&[(0, 20_000)]);
        assert_eq!(with_auto_zooms(&d, None, 20_000, &TakeMetas::new()), None);
        let empty = CursorRecording { samples: vec![], ..clicks_take() };
        assert_eq!(with_auto_zooms(&d, Some(&empty), 20_000, &TakeMetas::new()), None);
        let none = CursorRecording { provider: crate::record::project::CursorProvider::None, ..clicks_take() };
        assert_eq!(with_auto_zooms(&d, Some(&none), 20_000, &TakeMetas::new()), None);
    }

    #[test]
    fn first_open_plans_once() {
        let dir = tmp_project("first-open", "f");
        let mut p = project::read_project(&dir).unwrap();
        p.recording.duration_ms = 20_000;
        let rec = clicks_take();
        // Off: no zooms, but the open still marks it done.
        let (off, changed) = doc_for_open(&p, Some(&rec), false, &TakeMetas::new());
        assert!(changed && off.auto_zoom_done && off.zooms.is_empty());
        // On: the first open plans.
        let (first, changed) = doc_for_open(&p, Some(&rec), true, &TakeMetas::new());
        assert!(changed && first.auto_zoom_done);
        assert_eq!(first.zooms.len(), 2);
        // Saved, every zoom deleted, reopened: nothing comes back.
        let cleared = EditDoc { zooms: vec![], ..first };
        editor::store(&mut p, &cleared);
        project::write_atomic(&dir, PROJECT_FILE, &p).unwrap();
        let p2 = project::read_project(&dir).unwrap();
        let (second, changed) = doc_for_open(&p2, Some(&rec), true, &TakeMetas::new());
        assert!(!changed);
        assert!(second.zooms.is_empty(), "a second open never plans again");
        // A v1 project never gets zooms either.
        let mut old = p2.clone();
        old.editor = serde_json::json!({"version": 1, "clips": [{"startMs": 0, "endMs": 20000}]});
        let (v1, changed) = doc_for_open(&old, Some(&rec), true, &TakeMetas::new());
        assert!(!changed && v1.zooms.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn click_hits_in_view() {
        let meta = RecordingMeta { width: 1920, height: 1080, fps: 30, duration_ms: 20_000, ..Default::default() };
        let d = doc(&[(0, 20_000)]);
        let rec = clicks_take();
        let v = session_view(&meta, &d, Some(&rec), &TakeSources::new(), (1920, 1080));
        let presses: Vec<u64> = v.click_hits.iter().filter(|h| !h.release).map(|h| h.prog_ms).collect();
        assert_eq!(presses, vec![5_000, 12_000]);
        assert_eq!(v.click_hits.len(), 4, "a press and a release each");
        assert_eq!(v.output_size, [1920, 1080]);
        assert_eq!(v.screen_rects.len(), 1);
        // Cut out the first click: only the second plays, on the programme clock.
        let cut = doc(&[(8_000, 20_000)]);
        let v = session_view(&meta, &cut, Some(&rec), &TakeSources::new(), (1920, 1080));
        assert_eq!(v.click_hits.iter().filter(|h| !h.release).map(|h| h.prog_ms).collect::<Vec<_>>(), vec![4_000]);
        // No pointer data: no clicks.
        assert!(session_view(&meta, &d, None, &TakeSources::new(), (1920, 1080)).click_hits.is_empty());
        let json = serde_json::to_value(&v).unwrap();
        assert!(json.get("clickHits").is_some() && json.get("screenRects").is_some() && json.get("zooms").is_some());
    }

    #[test]
    fn view_maps_zooms_to_programme_time() {
        let mut d = doc(&[(0, 4_000), (6_000, 10_000)]);
        d.zooms = vec![crate::record::editor::ZoomRegion { clip: 1, ..manual_zoom(7_000, 8_000) }];
        let v = editor::view(&d);
        assert_eq!(v.zooms.len(), 1);
        assert_eq!((v.zooms[0].prog_start_ms, v.zooms[0].prog_end_ms), (5_000, 6_000));
    }

    /// End-to-end fixture for the render helper (Task 7): the real project at `IR_E2E_PROJECT`
    /// opened like the editor does (first open → auto-zooms), styled (wallpaper 01, padding 50,
    /// laptop frame, `pop-coral` pointer, click sound on), its scene written to
    /// `IR_E2E_SCENE_OUT` (default `<project>/scene-e2e.json`) for `record-render`'s
    /// `export_scene_file`.
    #[test]
    #[ignore]
    fn stage3_e2e_scene() {
        use crate::record::editor::{Background, Frame};
        let Some(dir) = std::env::var_os("IR_E2E_PROJECT").map(PathBuf::from) else {
            eprintln!("skipped: set IR_E2E_PROJECT=<project dir>");
            return;
        };
        let project = project::read_project(&dir).expect("project.json");
        let cursor = read_cursor(&dir);
        println!("cursor samples: {}", cursor.as_ref().map_or(0, |c| c.samples.len()));
        let (mut doc, planned) = doc_for_open(&project, cursor.as_ref(), true, &TakeMetas::new());
        println!("first open planned={planned}, zooms:");
        for z in &doc.zooms {
            println!("  {} clip {} {}–{} ms depth {} focus ({:.2},{:.2}) {:?}", z.id, z.clip, z.start_ms, z.end_ms, z.depth, z.focus_x, z.focus_y, z.origin);
        }
        doc.appearance.background = Background::Wallpaper { id: "01".into() };
        doc.appearance.padding = 50.0;
        doc.appearance.frame = Frame::Laptop;
        doc.cursor.show = true;
        doc.cursor.theme = "pop-coral".into();
        doc.cursor.click_sound = true;
        let doc = editor::normalize(doc, project.recording.duration_ms, &TakeMetas::new());
        let out = editor::output_size(&project.recording, &TakeMetas::new(), &doc, OutputTarget::P1080);
        let v = session_view(&project.recording, &doc, cursor.as_ref(), &TakeSources::new(), out);
        println!("output {out:?}, click hits {}, screen rect {:?}", v.click_hits.len(), v.screen_rects.first());
        let scene = scene_json_with(&dir, &project, &doc, out, 30, &AssetRoot::dev(), cursor.as_ref(), &TakeSources::new());
        assert_eq!(scene.get("clickSound").is_some(), !v.click_hits.is_empty(), "click sound iff the take has clicks");
        // `IR_E2E_SCENE_OUT` keeps someone else's project untouched.
        let path = std::env::var_os("IR_E2E_SCENE_OUT").map(PathBuf::from).unwrap_or_else(|| dir.join("scene-e2e.json"));
        std::fs::write(&path, serde_json::to_vec_pretty(&scene).unwrap()).unwrap();
        println!("scene written: {}", path.display());
    }

    #[test]
    fn auto_zoom_ipc_is_registered() {
        let lib = include_str!("../lib.rs");
        assert!(lib.contains("commands::editor_auto_zoom,"));
        let ipc = include_str!("../../../frontend/src/lib/ipc.ts");
        assert!(ipc.contains("\"editor_auto_zoom\""));
    }

    #[test]
    fn an_update_after_a_project_switch_lands_only_in_the_new_project() {
        let a = tmp_project("switch-a", "a");
        let b = tmp_project("switch-b", "b");
        let a_before = std::fs::read(a.join(PROJECT_FILE)).unwrap();
        // The editor showed A (session 1), then B was opened (session 2).
        let mut slot = Some(session_for(&b, 2));
        let b_doc = slot.as_ref().unwrap().doc.clone();
        // An edit the old window made for A arrives late: refused, B untouched.
        let stale = doc(&[(0, 2_000), (6_000, 9_000)]);
        assert_eq!(apply_update(&mut slot, 1, stale.clone(), 0).unwrap_err(), ERR_NO_SESSION);
        let s = slot.as_mut().unwrap();
        assert_eq!(s.doc, b_doc);
        assert!(!s.dirty, "a refused update schedules no save");
        save_session(s);
        assert_eq!(project::read_project(&b).unwrap().editor, serde_json::json!({}), "B's project.json untouched");
        // B's own edit is stored into B — and only B.
        apply_update(&mut slot, 2, stale.clone(), 0).unwrap();
        save_session(slot.as_mut().unwrap());
        assert_eq!(editor::load(&project::read_project(&b).unwrap(), &TakeMetas::new()).clips, stale.clips);
        assert_eq!(std::fs::read(a.join(PROJECT_FILE)).unwrap(), a_before, "A's project.json untouched");
        // No session at all: refused too.
        assert_eq!(apply_update(&mut None, 2, stale, 0).unwrap_err(), ERR_NO_SESSION);
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    /// A clip whose take folder is gone stays in the stored document (no loss on the next
    /// save); it plays for no time, the scene leaves it out, and the transport aims at the
    /// scene's index of the clip behind it.
    #[test]
    fn a_missing_take_is_kept_and_skipped_while_playing() {
        let dir = tmp_project("missing-take", "m");
        let mut slot = Some(session_for(&dir, 4));
        let mut d = doc(&[(0, 4_000), (0, 6_000), (5_000, 9_000)]);
        d.clips[1].source = Some("gone".into());
        let (view, scene, _) = apply_update(&mut slot, 4, d.clone(), 0).unwrap();
        assert_eq!(view.doc.clips, d.clips, "the stored document keeps the clip");
        assert_eq!(view.clips[1].prog_start_ms, view.clips[1].prog_end_ms, "zero-width while missing");
        assert_eq!(view.programme_ms, 8_000);
        assert_eq!(scene["clips"].as_array().unwrap().len(), 2);
        let s = slot.as_mut().unwrap();
        assert_eq!(s.doc.clips, d.clips);
        // Seeking into the third clip names the scene's clip 1.
        let (cmds, pos) = s.transport.seek(&s.play, 0, 5_000);
        assert_eq!(pos, Some(SourcePos { clip: 2, source_ms: 6_000 }));
        assert!(matches!(cmds[..], [Command::SetActiveClip { clip: 1, .. }]), "{cmds:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Under `Auto` the output shape follows padding (and frame, crop): an edit that changes
    /// it must reshape the preview too, or the helper keeps rendering the old shape.
    #[test]
    fn an_update_that_changes_the_output_size_resizes_the_preview() {
        let dir = tmp_project("reshape", "r");
        let mut slot = Some(session_for(&dir, 3));
        let s = slot.as_ref().unwrap();
        assert_eq!(s.doc.output.aspect, Aspect::Auto);
        let before = s.output(OutputTarget::P1080);
        // Same shape: no resize.
        let same = s.doc.clone();
        let (view, _, cmds) = apply_update(&mut slot, 3, same, 0).unwrap();
        assert!(!cmds.iter().any(|c| matches!(c, Command::Resize { .. })), "{cmds:?}");
        assert_eq!(view.output_size, [before.0, before.1]);
        // More padding: another Auto shape → the preview follows it.
        let mut padded = slot.as_ref().unwrap().doc.clone();
        padded.appearance.padding = 100.0;
        let (view, _, cmds) = apply_update(&mut slot, 3, padded, 0).unwrap();
        let s = slot.as_ref().unwrap();
        let after = s.output(OutputTarget::P1080);
        assert_ne!(after, before, "the fixture must change the Auto shape");
        let want = preview_size(after);
        let resizes: Vec<_> = cmds.iter().filter(|c| matches!(c, Command::Resize { .. })).collect();
        assert_eq!(resizes.len(), 1, "{cmds:?}");
        assert!(matches!(resizes[0], Command::Resize { w, h } if (*w, *h) == want));
        assert_eq!(s.preview, want, "a restarted helper re-opens at the new shape");
        assert_eq!(view.output_size, [after.0, after.1]);
        // The init view carries it too (a window mounting later).
        assert_eq!(s.init(None).view.output_size, [after.0, after.1]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_opened_event_and_init_carry_the_session_id() {
        let b = tmp_project("init", "b");
        let s = session_for(&b, 7);
        let v = serde_json::to_value(s.init(None)).unwrap();
        assert_eq!(v["sessionId"], 7);
        let src = include_str!("editor_ipc.rs");
        let open = src.split("pub fn open_editor(").nth(1).unwrap().split("\n}\n").next().unwrap();
        assert!(open.contains("app.emit(EV_OPENED"), "open_editor announces the new session");
        let _ = std::fs::remove_dir_all(&b);
    }

    #[test]
    fn opening_is_refused_while_an_export_runs() {
        assert_eq!(open_refusal(true), Err(ERR_OPEN_EXPORTING.to_string()));
        assert_eq!(open_refusal(false), Ok(()));
        // The guard runs before the session is touched (the export must not lose its scene).
        let src = include_str!("editor_ipc.rs");
        let open = src.split("pub fn open_editor(").nth(1).unwrap().split("\n}\n").next().unwrap();
        let guard = open.find("open_allowed()?").expect("open_editor checks for a running export");
        assert!(guard < open.find("flush_save").unwrap() && guard < open.find("session()").unwrap());
    }

    #[test]
    fn the_editor_window_survives_app_hide() {
        let src = include_str!("editor_ipc.rs");
        let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        let body = |name: &str| code.split(&format!("fn {name}(")).nth(1).unwrap().split("\n}\n").next().unwrap().to_string();
        assert!(body("show_window").contains("keep_on_app_hide(&win)"), "every built editor window opts out");
        // Needle assembled at runtime so this test can't find its own text.
        let needle = format!("{}: false]", "setCanHide");
        assert!(body("keep_on_app_hide").contains(&needle), "the editor must opt out of app.hide() (popup dismissal)");
    }

    #[test]
    fn mapping_queries_and_speed_follow_the_document() {
        // Clip 0 source 0–10 s with 2× over 2–6 s (programme 8 s), clip 1 source 12–14 s.
        let mut d = doc(&[(0, 10_000), (12_000, 14_000)]);
        d.speed.push(SpeedRegion { clip: 0, start_ms: 2_000, end_ms: 6_000, speed: 2.0 });
        assert_eq!(locate(&d, 3_000), Some(SourcePos { clip: 0, source_ms: 4_000 }));
        assert_eq!(locate(&d, 9_000), Some(SourcePos { clip: 1, source_ms: 13_000 }));
        assert_eq!(editor::source_to_programme(&d, 0, 4_000), 3_000);
        assert_eq!(editor::source_to_programme(&d, 1, 13_000), 9_000);
        assert_eq!(speed_of(&d, locate(&d, 3_000)), 2.0);
        assert_eq!(speed_of(&d, locate(&d, 5_000)), 1.0);
        assert_eq!(speed_of(&d, locate(&d, 1_000)), 1.0);
        assert_eq!(speed_of(&d, None), 1.0);
        // Regions are half-open: their end already plays at the speed after them.
        assert_eq!(editor::speed_at(&d, 0, 6_000), 1.0);
        assert_eq!(editor::speed_at(&d, 0, 10_000), 1.0);
        assert_eq!(editor::speed_at(&d, 5, 0), 1.0);
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
        let mut d = doc(&[(0, 5_000)]);
        d.output.aspect = Aspect::W16H9;
        let (kind, size, fps, ext) = export_plan(&mp4, &meta, &TakeMetas::new(), &d).unwrap();
        assert_eq!(kind, ExportKind::Mp4 { w: 1280, h: 720, fps: 60, codec: Codec::Hevc });
        assert_eq!((size, fps, ext), ((1280, 720), 60, "mp4"));
        assert_eq!(export_plan(&ExportOptions { fps: 25, ..mp4.clone() }, &meta, &TakeMetas::new(), &d).unwrap_err(), ERR_BAD_EXPORT);

        let gif = ExportOptions { kind: ExportFormat::Gif, fps: 15, gif_width: 480, loop_forever: false, ..ExportOptions::default() };
        let (kind, size, _, ext) = export_plan(&gif, &meta, &TakeMetas::new(), &d).unwrap();
        assert_eq!(kind, ExportKind::Gif { w: 480, h: 270, fps: 15, loop_forever: false });
        assert_eq!((size, ext), ((480, 270), "gif"));
        assert!(export_plan(&ExportOptions { gif_width: 500, ..gif.clone() }, &meta, &TakeMetas::new(), &d).is_err());
        assert!(export_plan(&ExportOptions { fps: 60, ..gif }, &meta, &TakeMetas::new(), &d).is_err());
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
        let doc = editor::load(&project, &TakeMetas::new());
        let out = editor::output_size(&project.recording, &TakeMetas::new(), &doc, OutputTarget::P1080);
        let assets = AssetRoot::dev();
        let cursor = read_cursor(&dir);
        let scene = scene_json_with(&dir, &project, &doc, out, PREVIEW_FPS, &assets, cursor.as_ref(), &TakeSources::new());
        let client = RenderClient::spawn(Arc::new(|n| eprintln!("notice: {n:?}")), &assets.fonts_dir()).unwrap();
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
        let t = client.thumbs(scene_json_with(&dir, &project, &doc, (640, 360), THUMB_FPS, &assets, cursor.as_ref(), &TakeSources::new()), THUMB_FPS, frames.clone(), THUMB_WIDTH).unwrap();
        eprintln!("thumbs {:?}", t.iter().map(|(i, j)| (*i, j.len())).collect::<Vec<_>>());
        assert_eq!(t.len(), frames.len());
        let started = Instant::now();
        client.shutdown();
        eprintln!("shutdown in {:?}", started.elapsed());
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    /// A take follows one run of the transport: pause, seek and the programme's end end it
    /// where it was; an edit (continuous clock) or a re-aim does not.
    #[test]
    fn a_take_follows_one_run_of_the_transport() {
        let d = doc(&[(0, 10_000)]);
        let mut t = Transport::new(&d);
        assert_eq!(t.watched(0), RunState::Unwatched);
        t.seek(&d, 0, 1_000);
        t.play(&d, 0);
        t.watch();
        assert_eq!(t.watched(500), RunState::Running(1_500));
        t.set_doc(&d, 600);
        t.reaim(&d, 700);
        assert_eq!(t.watched(800), RunState::Running(1_800), "an edit or re-aim keeps the run");
        t.pause(1_500);
        assert_eq!(t.watched(9_000), RunState::Ended(2_500), "paused: ended where it paused");
        t.play(&d, 9_000);
        assert_eq!(t.watched(9_500), RunState::Ended(2_500), "playing again is another run");
        // A stall pause (the frame watchdog) is a pause, a seek ends the run where it was.
        let mut t = Transport::new(&d);
        t.play(&d, 0);
        t.watch();
        t.seek(&d, 2_000, 8_000);
        assert_eq!(t.watched(2_100), RunState::Ended(2_000));
        // The programme's end.
        let mut t = Transport::new(&d);
        t.seek(&d, 0, 9_000);
        t.play(&d, 0);
        t.watch();
        t.tick(&d, 5_000);
        assert_eq!(t.watched(5_000), RunState::Ended(10_000));
        // Play at the end (no tick in between) starts over at 0: that jump ends the run too.
        let mut t = Transport::new(&d);
        t.seek(&d, 0, 9_000);
        t.play(&d, 0);
        t.watch();
        t.play(&d, 5_000);
        assert_eq!(t.watched(5_100), RunState::Ended(10_000));
        // The take's length is the run's.
        let p = voiceover::Plan { programme_ms: 1_000, clip: 0, source: None, source_ms: 1_000, max_ms: 9_000 };
        assert_eq!(voiceover::span_from_run(2_500, &p), 1_500);
    }

    /// A finished take becomes a voiceover track at the clip/source time of the playhead it
    /// started from, and plays there on the programme clock.
    #[test]
    fn a_voiceover_lands_where_it_was_recorded() {
        let dir = tmp_project("voiceover", "v");
        let mut slot = Some(session_for(&dir, 6));
        let mut d = doc(&[(0, 4_000), (5_000, 9_000)]);
        d.speed = vec![editor::SpeedRegion { clip: 1, start_ms: 5_000, end_ms: 7_000, speed: 2.0 }];
        apply_update(&mut slot, 6, d, 0).unwrap();
        let s = slot.as_ref().unwrap();
        let plan = voiceover::plan(&s.play, 4_500).unwrap();
        assert_eq!((plan.clip, plan.source_ms, plan.max_ms), (1, 6_000, 2_500));
        let take = Take::create(6, &dir, plan.clone(), 42).unwrap();
        take.push_at(0, &[0.1; 4_800], 48_000);
        let (file, span) = take.finish_at(1_200).unwrap();
        let next = doc_with_voiceover(&s.doc, &s.play, &plan, &file, span);
        let (view, _, _) = apply_update(&mut slot, 6, next, 0).unwrap();
        let t = &view.doc.audio_tracks[0];
        assert_eq!((t.kind, t.file.as_str(), t.clip, t.start_ms, t.span_ms), (editor::TrackKind::Voiceover, "voiceover-42.wav", 1, 6_000, Some(1_200)));
        let span = view.tracks.iter().find(|e| e.index == 0).expect("the track plays");
        assert_eq!(span.prog_start_ms.round() as u64, 4_500, "it starts at the take's playhead");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
