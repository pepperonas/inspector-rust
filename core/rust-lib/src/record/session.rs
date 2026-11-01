//! Recording session lifecycle: pure phase machine + project-root rules
//! (tested), and on macOS the session that ties capture, microphone and
//! cursor sampler together around one [`PauseShared`] timeline.
//!
//! Start order (one origin for everything): project dir + `project.json`
//! (`recording`) → optional countdown → `Capture::start` →
//! `wait_for_first_frame` gives the timeline ns of video time 0 → microphone
//! and cursor sampler start on that origin → control bar → sound.
//!
//! Stop order: capture first (its `end_ns` is the file's end) → mic padded to
//! `end_ns` and finished → cursor → `cursor.json` → `project.json` (`ready`,
//! or `failed` + reason).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::project::CursorMode;

// ---------- pure: phases ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Idle,
    Picking,
    Countdown,
    Recording,
    Paused,
    Finishing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Show the system picker.
    Pick,
    /// The picker answered (pick or cancel).
    PickDone,
    Start,
    /// Capture is running (countdown over, first frame written).
    Begin,
    /// Start failed before `Begin`.
    Fail,
    Pause,
    Resume,
    Stop,
    Cancel,
    /// Files are finalised.
    Finished,
}

pub const ERR_BUSY: &str = "record.busy";
pub const ERR_NOT_RECORDING: &str = "record.not_recording";
pub const ERR_INVALID: &str = "record.invalid_state";
pub const ERR_NO_SOURCE: &str = "record.no_source";
pub const ERR_DISK_FULL: &str = "record.disk_full";
pub const ERR_UNSUPPORTED: &str = "record.unsupported";
pub const ERR_START_FAILED: &str = "record.start_failed";
pub const ERR_CANCELLED: &str = "record.cancelled";
pub const ERR_OUTSIDE_ROOT: &str = "record.outside_root";

/// The only legal phase changes; everything else is an error sentinel.
pub fn next_phase(phase: Phase, action: Action) -> Result<Phase, &'static str> {
    use Action as A;
    use Phase as P;
    match (phase, action) {
        (P::Idle | P::Picking, A::Pick) => Ok(P::Picking),
        (P::Picking, A::PickDone) => Ok(P::Idle),
        // A pick arriving while something else runs changes nothing.
        (p, A::PickDone) => Ok(p),
        (P::Idle | P::Picking, A::Start) => Ok(P::Countdown),
        (_, A::Start) => Err(ERR_BUSY),
        (P::Countdown, A::Begin) => Ok(P::Recording),
        (P::Countdown, A::Fail | A::Cancel) => Ok(P::Idle),
        (P::Recording, A::Pause) => Ok(P::Paused),
        (P::Paused, A::Pause) => Ok(P::Paused),
        (P::Paused, A::Resume) => Ok(P::Recording),
        (P::Recording, A::Resume) => Ok(P::Recording),
        (P::Recording | P::Paused, A::Stop | A::Cancel) => Ok(P::Finishing),
        (P::Finishing, A::Finished) => Ok(P::Idle),
        (P::Idle | P::Picking, A::Pause | A::Resume | A::Stop | A::Cancel) => Err(ERR_NOT_RECORDING),
        _ => Err(ERR_INVALID),
    }
}

// ---------- pure: project root ----------

/// `dir` must be an existing direct child of `root` (after resolving `..`
/// and symlinks); the root itself and anything outside are refused.
pub fn project_dir_inside_root(root: &Path, dir: &Path) -> Result<PathBuf, &'static str> {
    let root = std::fs::canonicalize(root).map_err(|_| ERR_OUTSIDE_ROOT)?;
    let dir = std::fs::canonicalize(dir).map_err(|_| ERR_OUTSIDE_ROOT)?;
    if dir == root || dir.parent() != Some(root.as_path()) || !dir.is_dir() {
        return Err(ERR_OUTSIDE_ROOT);
    }
    Ok(dir)
}

/// The on-screen notice for a take that went wrong — at start (the HUD is
/// already closed by then, so there is nowhere else to say it), mid-take
/// (the capture died and the take was stopped) or at stop (the writer
/// failed). `None` when the take ended fine: a normal stop says nothing.
pub fn failure_toast(at_start: bool, failed: bool, reason: Option<&str>) -> Option<crate::status_toast::StatusToast> {
    if !failed {
        return None;
    }
    let title = if at_start { "rec2.toast.start_failed" } else { "rec2.toast.failed" };
    let subtitle = match reason.map(str::trim).filter(|r| !r.is_empty()) {
        Some(r) => r.to_string(),
        None => crate::i18n::t("rec2.toast.no_reason"),
    };
    Some(crate::status_toast::StatusToast {
        kind: "record".into(),
        on: false,
        title: crate::i18n::t(title),
        subtitle,
    })
}

/// A deletable project: inside the root (see [`project_dir_inside_root`]) and
/// carrying a `project.json`. Any other folder there is the user's, so the
/// panel must never be able to trash it.
pub fn project_dir_for_delete(root: &Path, dir: &Path) -> Result<PathBuf, &'static str> {
    let dir = project_dir_inside_root(root, dir)?;
    if !super::project::is_project_dir(&dir) {
        return Err(ERR_OUTSIDE_ROOT);
    }
    Ok(dir)
}

/// Pauses in video time: `(position_ms, paused_ms)` — where in the file the
/// cut sits and how long the pause lasted. `intervals` are host-time
/// `[pause, resume)` pairs in order; `origin_ns` is the timeline ns of video
/// time 0.
pub fn pause_marks(intervals: &[(i64, i64)], origin_ns: i64) -> Vec<(u64, u64)> {
    let mut before = 0i64;
    let mut out = Vec::with_capacity(intervals.len());
    for &(p, r) in intervals {
        let dur = (r - p).max(0);
        let at = ((p - before) - origin_ns).max(0) / 1_000_000;
        out.push((at as u64, (dur / 1_000_000) as u64));
        before += dur;
    }
    out
}

/// Elapsed recording time in ms for the status line.
pub fn elapsed_ms(timeline_now_ns: i64, origin_ns: Option<i64>) -> u64 {
    match origin_ns {
        Some(o) => ((timeline_now_ns - o).max(0) / 1_000_000) as u64,
        None => 0,
    }
}

// ---------- config + IPC types ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RecordConfig {
    /// Absolute folder; empty = `~/Movies/Inspector Rust`.
    pub project_dir: String,
    pub countdown: bool,
    pub cursor_mode: CursorMode,
    pub system_audio: bool,
    pub mic: bool,
}

impl Default for RecordConfig {
    fn default() -> Self {
        Self {
            project_dir: String::new(),
            countdown: true,
            cursor_mode: CursorMode::EditableOverlay,
            system_audio: true,
            mic: false,
        }
    }
}

const K_DIR: &str = "record.project_dir";
const K_COUNTDOWN: &str = "record.countdown";
const K_CURSOR: &str = "record.cursor_mode";
const K_SYSTEM_AUDIO: &str = "record.system_audio";
const K_MIC: &str = "record.mic";

fn parse_bool(v: Option<String>, default: bool) -> bool {
    match v.as_deref().map(str::trim) {
        Some("true") | Some("1") => true,
        Some("false") | Some("0") => false,
        _ => default,
    }
}

pub fn cursor_mode_from_str(s: &str) -> CursorMode {
    if s.trim() == "system" {
        CursorMode::System
    } else {
        CursorMode::EditableOverlay
    }
}

fn cursor_mode_str(m: CursorMode) -> &'static str {
    match m {
        CursorMode::System => "system",
        CursorMode::EditableOverlay => "editable-overlay",
    }
}

impl RecordConfig {
    pub fn load(db: &crate::db::DbHandle) -> Self {
        let d = Self::default();
        let get = |k: &str| crate::settings::get(db, k).ok().flatten();
        Self {
            project_dir: get(K_DIR).unwrap_or_default().trim().to_string(),
            countdown: parse_bool(get(K_COUNTDOWN), d.countdown),
            cursor_mode: get(K_CURSOR).map(|s| cursor_mode_from_str(&s)).unwrap_or(d.cursor_mode),
            system_audio: parse_bool(get(K_SYSTEM_AUDIO), d.system_audio),
            mic: parse_bool(get(K_MIC), d.mic),
        }
    }

    pub fn save(&self, db: &crate::db::DbHandle) -> anyhow::Result<()> {
        crate::settings::set(db, K_DIR, self.project_dir.trim())?;
        crate::settings::set(db, K_COUNTDOWN, if self.countdown { "true" } else { "false" })?;
        crate::settings::set(db, K_CURSOR, cursor_mode_str(self.cursor_mode))?;
        crate::settings::set(db, K_SYSTEM_AUDIO, if self.system_audio { "true" } else { "false" })?;
        crate::settings::set(db, K_MIC, if self.mic { "true" } else { "false" })?;
        Ok(())
    }

    /// The folder projects live in.
    pub fn root(&self) -> PathBuf {
        resolve_root(&self.project_dir)
    }
}

pub fn resolve_root(setting: &str) -> PathBuf {
    let s = setting.trim();
    if !s.is_empty() {
        let home = dirs::home_dir();
        return crate::path_arg::expand_user(s, home.as_deref());
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join("Movies").join("Inspector Rust")
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartOptions {
    pub system_audio: bool,
    pub mic: bool,
    pub cursor_mode: CursorMode,
    pub countdown: bool,
    #[serde(default)]
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RecordStatus {
    pub supported: bool,
    pub reason: Option<String>,
    pub phase: Phase,
    pub elapsed_ms: u64,
    pub project_dir: Option<String>,
    /// Source picked in the system picker (label), if any.
    pub source: Option<String>,
    /// Minutes of disk reserve left when it is low (< 15 min).
    pub low_disk_minutes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectListItem {
    #[serde(flatten)]
    pub summary: super::project::ProjectSummary,
    /// The project currently being recorded (its files are not final yet).
    pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FinishedEvent {
    pub dir: String,
    /// "ready" | "failed" | "cancelled"
    pub status: String,
    pub failure: Option<String>,
}

/// Mark leftovers of a crash/quit (`status: recording`) as `incomplete`.
pub fn mark_interrupted(root: &Path) {
    use super::project::{read_project, write_atomic, ProjectStatus, PROJECT_FILE};
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for e in rd.flatten() {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        if let Ok(mut p) = read_project(&dir) {
            if p.status == ProjectStatus::Recording {
                p.status = ProjectStatus::Incomplete;
                p.failure = Some(crate::i18n::t("rec2.interrupted"));
                p.updated_at = chrono::Local::now().timestamp_millis();
                if let Err(err) = write_atomic(&dir, PROJECT_FILE, &p) {
                    tracing::warn!("record: cannot mark {} incomplete: {err}", dir.display());
                }
            }
        }
    }
}

/// List projects, marking (and not degrading) the one being recorded now.
pub fn list_items(root: &Path, active: Option<&Path>) -> Vec<ProjectListItem> {
    let active = active.and_then(|a| std::fs::canonicalize(a).ok());
    super::project::list_projects(root)
        .into_iter()
        .map(|mut summary| {
            let is_active = active.is_some() && std::fs::canonicalize(&summary.dir).ok() == active;
            if is_active {
                summary.status = super::project::ProjectStatus::Recording;
                summary.failure = None;
            }
            ProjectListItem { summary, active: is_active }
        })
        .collect()
}

/// Is recording possible on this build/OS at all? `Err(reason key)` if not.
pub fn support_reason() -> Result<(), &'static str> {
    #[cfg(not(target_os = "macos"))]
    {
        Err("rec2.unsupported.platform")
    }
    #[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
    {
        Err("rec2.unsupported.intel")
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        if super::picker_macos::picker_available() {
            Ok(())
        } else {
            Err("rec2.unsupported.macos")
        }
    }
}

pub fn record_supported() -> bool {
    support_reason().is_ok()
}

#[cfg(target_os = "macos")]
pub use mac::*;

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use crate::record::capture_macos::{self, Capture, CaptureOptions};
    use crate::record::core::{capture_output_size, default_video_bitrate, disk_space_verdict, project_dir_name, DiskVerdict};
    use crate::record::cursor_macos::CursorRecorder;
    use crate::record::mic::start_mic;
    use crate::record::picker_macos::{self, Picked};
    use crate::record::project::{
        write_atomic, MicMeta, Project, ProjectStatus, RecordingMeta, SourceKind, CURSOR_FILE, MIC_FILE,
        PROJECT_FILE, PROJECT_SCHEMA, SCREEN_FILE,
    };
    use crate::record::wav::{WavSink, OUT_RATE};
    use crate::record::{host_now_ns, PauseShared};
    use crate::mic_capture::MicStream;
    use parking_lot::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tauri::{AppHandle, Emitter, Manager};

    pub const HUD_LABEL: &str = "record-hud";
    pub const COUNTDOWN_LABEL: &str = "record-countdown";
    pub const BAR_LABEL: &str = "record-bar";
    pub const NOTES_LABEL: &str = "record-notes";
    const FPS: u32 = 60;
    const COUNTDOWN: Duration = Duration::from_secs(3);
    const FIRST_FRAME: Duration = Duration::from_secs(3);

    struct Active {
        dir: PathBuf,
        project: Project,
        clock: Arc<PauseShared>,
        capture: Capture,
        origin_ns: i64,
        mic: Option<(MicStream, Arc<Mutex<WavSink>>)>,
        cursor: CursorRecorder,
        assertion: Option<u32>,
        ticker_stop: Arc<AtomicBool>,
    }

    #[derive(Default)]
    struct Inner {
        phase: Phase,
        gen: u64,
        active: Option<Active>,
        /// Dir of the project being started/recorded (also during countdown).
        dir: Option<PathBuf>,
        low_disk_minutes: Option<u64>,
    }

    #[derive(Default)]
    pub struct RecordState {
        inner: Mutex<Inner>,
    }

    impl RecordState {
        pub fn phase(&self) -> Phase {
            self.inner.lock().phase
        }

        fn transition(&self, action: Action) -> Result<Phase, String> {
            let mut g = self.inner.lock();
            let p = next_phase(g.phase, action).map_err(str::to_string)?;
            g.phase = p;
            Ok(p)
        }

        pub fn status(&self) -> RecordStatus {
            let g = self.inner.lock();
            let reason = support_reason().err().map(crate::i18n::t);
            let elapsed = g
                .active
                .as_ref()
                .map(|a| elapsed_ms(a.clock.timeline_ns(host_now_ns()), Some(a.origin_ns)))
                .unwrap_or(0);
            RecordStatus {
                supported: reason.is_none(),
                reason,
                phase: g.phase,
                elapsed_ms: elapsed,
                project_dir: g.dir.as_ref().map(|d| d.to_string_lossy().into_owned()),
                source: picker_macos::current().map(|p| p.label),
                low_disk_minutes: g.low_disk_minutes,
            }
        }

        pub fn active_dir(&self) -> Option<PathBuf> {
            self.inner.lock().dir.clone()
        }
    }

    /// Show a failure toast passively (no focus steal, popup untouched) —
    /// the user is usually in another app while a take runs.
    fn announce_failure(app: &AppHandle, toast: Option<crate::status_toast::StatusToast>) {
        let Some(toast) = toast else { return };
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || crate::status_toast::show_passive(&app2, toast));
    }

    fn emit_status(app: &AppHandle) {
        if let Some(st) = app.try_state::<RecordState>() {
            let _ = app.emit("record-status", st.status());
        }
    }

    // ---------- display sleep ----------

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: *const objc2_core_foundation::CFString,
            level: u32,
            name: *const objc2_core_foundation::CFString,
            id: *mut u32,
        ) -> i32;
        fn IOPMAssertionRelease(id: u32) -> i32;
    }

    fn prevent_display_sleep() -> Option<u32> {
        let ty = objc2_core_foundation::CFString::from_str("PreventUserIdleDisplaySleep");
        let name = objc2_core_foundation::CFString::from_str("Inspector Rust recording");
        let mut id = 0u32;
        // SAFETY: both CFStrings live for the call; `id` is a valid out-pointer.
        // 255 = kIOPMAssertionLevelOn.
        let rc = unsafe { IOPMAssertionCreateWithName(&*ty, 255, &*name, &mut id) };
        (rc == 0).then_some(id)
    }

    fn release_display_sleep(id: Option<u32>) {
        if let Some(id) = id {
            // SAFETY: `id` came from IOPMAssertionCreateWithName.
            unsafe { IOPMAssertionRelease(id) };
        }
    }

    // ---------- disk ----------

    fn free_bytes(dir: &Path) -> Option<u64> {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        // SAFETY: `c` is NUL-terminated, `st` a valid out-pointer.
        if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
            return None;
        }
        Some(st.f_bavail as u64 * st.f_frsize as u64)
    }

    /// Video (bitrate for the picked source at 60 fps) + system audio + mic.
    fn est_bytes_per_min(picked: &Picked, opts: &StartOptions) -> u64 {
        let (w, h) = capture_output_size(
            picked.content_w_pt,
            picked.content_h_pt,
            picked.scale,
            capture_macos::MAX_W,
            capture_macos::MAX_H,
        );
        let mut bits = default_video_bitrate(w, h, FPS) as u64;
        if opts.system_audio {
            bits += capture_macos::AUDIO_BITRATE as u64;
        }
        let mut bytes = bits * 60 / 8;
        if opts.mic {
            bytes += OUT_RATE as u64 * 2 * 60;
        }
        bytes
    }

    // ---------- start ----------

    struct Prepared {
        dir: PathBuf,
        project: Project,
    }

    fn prepare_project(root: &Path, picked: &Picked, opts: &StartOptions) -> Result<(Prepared, DiskVerdict), String> {
        std::fs::create_dir_all(root).map_err(|e| format!("{ERR_START_FAILED}:{e}"))?;
        let verdict = disk_space_verdict(free_bytes(root).unwrap_or(u64::MAX), est_bytes_per_min(picked, opts));
        if verdict == DiskVerdict::Refuse {
            return Err(ERR_DISK_FULL.into());
        }
        let name = project_dir_name(chrono::Local::now(), &opts.title, &|n| root.join(n).exists());
        let dir = root.join(&name);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{ERR_START_FAILED}:{e}"))?;
        let now = chrono::Local::now().timestamp_millis();
        let title = if opts.title.trim().is_empty() { name.clone() } else { opts.title.trim().to_string() };
        let project = Project {
            schema: PROJECT_SCHEMA,
            id: format!("{:016x}{:016x}", rand::random::<u64>(), rand::random::<u64>()),
            title,
            created_at: now,
            updated_at: now,
            status: ProjectStatus::Recording,
            failure: None,
            recording: RecordingMeta {
                source: picked.kind,
                source_label: picked.label.clone(),
                fps: FPS,
                system_audio: opts.system_audio,
                cursor_mode: opts.cursor_mode,
                ..Default::default()
            },
            editor: serde_json::json!({}),
        };
        write_atomic(&dir, PROJECT_FILE, &project).map_err(|e| format!("{ERR_START_FAILED}:{e}"))?;
        Ok((Prepared { dir, project }, verdict))
    }

    fn fail_project(dir: &Path, mut project: Project, reason: &str) {
        project.status = ProjectStatus::Failed;
        project.failure = Some(reason.to_string());
        project.updated_at = chrono::Local::now().timestamp_millis();
        let _ = write_atomic(dir, PROJECT_FILE, &project);
    }

    /// Start capture + mic + cursor on one origin. No UI.
    fn begin_capture(
        app: Option<AppHandle>,
        picked: &Picked,
        opts: &StartOptions,
        prep: Prepared,
    ) -> Result<Active, Box<(Prepared, String)>> {
        let clock = Arc::new(PauseShared::new());
        let filter = picked.filter_for_capture();
        let capture = match Capture::start(
            filter,
            CaptureOptions {
                system_audio: opts.system_audio,
                show_system_cursor: opts.cursor_mode == CursorMode::System,
                fps: FPS,
            },
            &prep.dir.join(SCREEN_FILE),
            clock.clone(),
        ) {
            Ok(c) => c,
            Err(e) => return Err(Box::new((prep, e))),
        };
        let Some(origin_ns) = capture.wait_for_first_frame(FIRST_FRAME) else {
            let _ = capture.stop();
            return Err(Box::new((prep, crate::i18n::t("rec2.capture_no_frame"))));
        };
        let mic = if opts.mic {
            match WavSink::create(&prep.dir.join(MIC_FILE), OUT_RATE) {
                Ok(sink) => {
                    let sink = Arc::new(Mutex::new(sink));
                    Some((start_mic(app, sink.clone(), clock.clone(), origin_ns), sink))
                }
                Err(e) => {
                    tracing::warn!("record: cannot create mic.wav: {e}");
                    None
                }
            }
        } else {
            None
        };
        let cursor = CursorRecorder::start(capture.content_bounds_pt(), clock.clone(), origin_ns);
        let assertion = prevent_display_sleep();
        Ok(Active {
            dir: prep.dir,
            project: prep.project,
            clock,
            capture,
            origin_ns,
            mic,
            cursor,
            assertion,
            ticker_stop: Arc::new(AtomicBool::new(false)),
        })
    }

    /// What `finish` produced.
    pub struct Outcome {
        pub dir: PathBuf,
        pub status: ProjectStatus,
        pub failure: Option<String>,
        pub video_ms: u64,
        pub mic_ms: Option<u64>,
    }

    /// Stop everything and write the final files. No UI.
    fn finish(a: Active) -> Outcome {
        a.ticker_stop.store(true, Ordering::SeqCst);
        let cap = a.capture.stop();
        let host_end = host_now_ns();
        let end_ns = match &cap {
            Ok(r) => r.end_ns,
            Err(_) => a.clock.timeline_ns(host_end),
        };

        let mut mic_meta = None;
        let mut mic_ms = None;
        let mut failure: Option<String> = None;
        if let Some((stream, sink)) = a.mic {
            stream.stop();
            drop(stream);
            // The capture thread holds a clone until its loop exits (≤ ~0.2 s).
            let mut sink = sink;
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            let owned = loop {
                match Arc::try_unwrap(sink) {
                    Ok(m) => break Some(m.into_inner()),
                    Err(s) if std::time::Instant::now() < deadline => {
                        sink = s;
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(s) => {
                        // Still referenced: pad what we can and leave the header as is.
                        let _ = s.lock().pad_to(end_ns);
                        break None;
                    }
                }
            };
            match owned {
                Some(mut w) => {
                    // Exactly the video's length: silence up to the end,
                    // chunks that arrived after it cut off.
                    let res = w.pad_to(end_ns).and_then(|_| w.truncate_to(end_ns)).and_then(|_| w.finish());
                    match res {
                        Ok(ms) => {
                            mic_ms = Some(ms);
                            mic_meta = Some(MicMeta { offset_ms: 0, sample_rate: OUT_RATE });
                        }
                        Err(e) => tracing::warn!("record: mic.wav finish failed: {e}"),
                    }
                }
                None => tracing::warn!("record: mic capture did not release mic.wav in time"),
            }
        }

        let cursor = a.cursor.stop();
        if let Err(e) = write_atomic(&a.dir, CURSOR_FILE, &cursor) {
            tracing::warn!("record: cannot write cursor.json: {e}");
        }
        release_display_sleep(a.assertion);

        let mut project = a.project;
        let pauses = pause_marks(&a.clock.pauses_until(host_end), a.origin_ns);
        let mut video_ms = 0;
        let status = match cap {
            Ok(r) => {
                video_ms = r.duration_ms;
                project.recording.width = r.width;
                project.recording.height = r.height;
                project.recording.duration_ms = r.duration_ms;
                if r.dropped_frames > 0 || r.rejected_frames > 0 {
                    tracing::info!(
                        "record: {} frames dropped (encoder busy), {} rejected (timestamps)",
                        r.dropped_frames,
                        r.rejected_frames
                    );
                }
                match r.stream_error {
                    Some(e) => {
                        failure = Some(e);
                        ProjectStatus::Failed
                    }
                    None => ProjectStatus::Ready,
                }
            }
            Err(e) => {
                failure = Some(e);
                ProjectStatus::Failed
            }
        };
        project.recording.mic = mic_meta;
        project.recording.pauses = pauses;
        project.status = status;
        project.failure = failure.clone();
        project.updated_at = chrono::Local::now().timestamp_millis();
        if let Err(e) = write_atomic(&a.dir, PROJECT_FILE, &project) {
            tracing::warn!("record: cannot write project.json: {e}");
        }
        Outcome { dir: a.dir, status, failure, video_ms, mic_ms }
    }

    fn status_str(s: ProjectStatus) -> &'static str {
        match s {
            ProjectStatus::Ready => "ready",
            ProjectStatus::Failed => "failed",
            ProjectStatus::Incomplete => "incomplete",
            ProjectStatus::Recording => "recording",
        }
    }

    // ---------- windows ----------

    /// Keep a record window out of every capture and alive across
    /// `NSApp.hide` (main thread).
    fn harden_window(app: &AppHandle, label: &'static str, passive: bool) {
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            let Some(w) = app2.get_webview_window(label) else { return };
            let Ok(ns) = w.ns_window() else { return };
            let ns = ns as *mut objc2::runtime::AnyObject;
            if ns.is_null() {
                return;
            }
            // SAFETY: main thread, `ns` is this window's live NSWindow.
            unsafe {
                let _: () = objc2::msg_send![ns, setSharingType: 0u64];
                let _: () = objc2::msg_send![ns, setCanHide: false];
                if passive {
                    let _: () = objc2::msg_send![ns, orderFrontRegardless];
                }
            }
        });
    }

    fn cursor_monitor(app: &AppHandle) -> Option<tauri::Monitor> {
        let mons = app.available_monitors().ok()?;
        crate::screenshot_preview::pick_cursor_monitor_globally(&mons).or_else(|| app.primary_monitor().ok().flatten())
    }

    fn build_window(app: &AppHandle, label: &'static str) -> Result<(), String> {
        use tauri::{WebviewUrl, WebviewWindowBuilder};
        // Idempotent: a window with this label is reused (close() is async,
        // rebuilding right after it fails with "label already exists").
        if let Some(w) = app.get_webview_window(label) {
            if label == HUD_LABEL || label == NOTES_LABEL {
                let _ = w.show();
                let _ = w.set_focus();
                harden_window(app, label, false);
            } else {
                harden_window(app, label, true);
            }
            return Ok(());
        }
        let mon = cursor_monitor(app);
        let b = WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .visible(false);
        let (w, h, focused) = match label {
            HUD_LABEL => (440.0, 320.0, true),
            BAR_LABEL => (312.0, 54.0, false),
            NOTES_LABEL => (360.0, 420.0, true),
            _ => (0.0, 0.0, false),
        };
        let win = match label {
            COUNTDOWN_LABEL => b.title("Countdown").focused(false).build(),
            _ => b.title("Recording").inner_size(w, h).focused(focused).shadow(true).accept_first_mouse(true).build(),
        }
        .map_err(|e| format!("build {label}: {e}"))?;
        if let Some(m) = mon {
            let (pos, size, scale) = (m.position(), m.size(), m.scale_factor());
            match label {
                COUNTDOWN_LABEL => {
                    let _ = win.set_position(tauri::PhysicalPosition::new(pos.x, pos.y));
                    let _ = win.set_size(tauri::PhysicalSize::new(size.width, size.height));
                    let _ = win.set_ignore_cursor_events(true);
                }
                NOTES_LABEL => {
                    let area = m.work_area();
                    let ww = (w * scale) as i32;
                    let x = area.position.x + area.size.width as i32 - ww - (24.0 * scale) as i32;
                    let y = area.position.y + (48.0 * scale) as i32;
                    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
                }
                HUD_LABEL => {
                    let ww = (w * scale) as i32;
                    let hh = (h * scale) as i32;
                    let x = pos.x + (size.width as i32 - ww) / 2;
                    let y = pos.y + size.height as i32 - hh - (96.0 * scale) as i32;
                    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
                }
                _ => {
                    let area = m.work_area();
                    let ww = (w * scale) as i32;
                    let x = area.position.x + (area.size.width as i32 - ww) / 2;
                    let y = area.position.y + (12.0 * scale) as i32;
                    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
                }
            }
        }
        if label == HUD_LABEL || label == NOTES_LABEL {
            let _ = win.show();
            let _ = win.set_focus();
            harden_window(app, label, false);
        } else {
            harden_window(app, label, true);
        }
        Ok(())
    }

    /// Build a window from a worker thread (never from a command's thread).
    fn open_window(app: &AppHandle, label: &'static str) {
        let app = app.clone();
        std::thread::spawn(move || {
            if let Err(e) = build_window(&app, label) {
                tracing::warn!("record: {e}");
            }
        });
    }

    fn close_window(app: &AppHandle, label: &str) {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.close();
        }
    }

    fn hide_popup_window(app: &AppHandle) {
        if let Some(p) = app.get_webview_window(crate::hotkey::POPUP_LABEL) {
            let _ = p.hide();
        }
        let _ = app.emit("popup-hidden", ());
    }

    // ---------- ticker (status 1/s + window bounds 2/s) ----------

    fn spawn_ticker(app: AppHandle, stop: Arc<AtomicBool>, window_id: Option<u32>) {
        std::thread::Builder::new()
            .name("ir-record-ticker".into())
            .spawn(move || {
                let mut n = 0u64;
                while !stop.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(500));
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Some(id) = window_id {
                        follow_window(&app, id);
                    }
                    if let Some(reason) = capture_failure(&app) {
                        tracing::warn!("record: capture failed mid-take: {reason}");
                        let app2 = app.clone();
                        std::thread::spawn(move || {
                            if let Err(e) = crate::record::session::stop(&app2) {
                                tracing::warn!("record: stopping after a failure: {e}");
                            }
                        });
                        break;
                    }
                    n += 1;
                    if n.is_multiple_of(2) {
                        emit_status(&app);
                    }
                }
            })
            .ok();
    }

    fn capture_failure(app: &AppHandle) -> Option<String> {
        let st = app.try_state::<RecordState>()?;
        let g = st.inner.lock();
        g.active.as_ref().and_then(|a| a.capture.failure())
    }

    /// Keep the cursor sampler's bounds on a moving window.
    fn follow_window(app: &AppHandle, window_id: u32) {
        let Some(b) = capture_macos::cg_window_bounds(window_id) else { return };
        if let Some(st) = app.try_state::<RecordState>() {
            if let Some(a) = st.inner.lock().active.as_ref() {
                a.cursor.set_bounds(b);
            }
        }
    }

    // ---------- public entry points ----------

    pub fn open_hud(app: &AppHandle) -> Result<(), String> {
        support_reason().map_err(|_| ERR_UNSUPPORTED.to_string())?;
        if let Some(st) = app.try_state::<RecordState>() {
            if !matches!(st.phase(), Phase::Idle | Phase::Picking) {
                return Err(ERR_BUSY.into());
            }
        }
        hide_popup_window(app);
        open_window(app, HUD_LABEL);
        Ok(())
    }

    /// Teleprompter notes window (never captured, survives `NSApp.hide`).
    pub fn open_notes(app: &AppHandle) {
        open_window(app, NOTES_LABEL);
    }

    pub fn close_notes(app: &AppHandle) {
        close_window(app, NOTES_LABEL);
    }

    pub fn pick_source(app: &AppHandle, kind: SourceKind) -> Result<(), String> {
        support_reason().map_err(|_| ERR_UNSUPPORTED.to_string())?;
        let st = app.state::<RecordState>();
        st.transition(Action::Pick)?;
        emit_status(app);
        picker_macos::present(app, kind)
    }

    /// Called by the picker observer after a pick or a cancel.
    pub fn pick_finished(app: &AppHandle) {
        if let Some(st) = app.try_state::<RecordState>() {
            let _ = st.transition(Action::PickDone);
        }
        emit_status(app);
    }

    pub fn start(app: &AppHandle, opts: StartOptions) -> Result<RecordStatus, String> {
        support_reason().map_err(|_| ERR_UNSUPPORTED.to_string())?;
        let st = app.state::<RecordState>();
        let my_gen = {
            let mut g = st.inner.lock();
            g.phase = next_phase(g.phase, Action::Start).map_err(str::to_string)?;
            g.gen += 1;
            g.low_disk_minutes = None;
            g.gen
        };
        let fail_early = |e: String| {
            let _ = st.transition(Action::Fail);
            emit_status(app);
            e
        };
        let Some(picked) = picker_macos::current() else {
            return Err(fail_early(ERR_NO_SOURCE.into()));
        };
        let root = app
            .try_state::<crate::db::DbHandle>()
            .map(|db| RecordConfig::load(&db).root())
            .unwrap_or_else(|| resolve_root(""));
        let (prep, verdict) = prepare_project(&root, &picked, &opts).map_err(fail_early)?;
        {
            let mut g = st.inner.lock();
            g.dir = Some(prep.dir.clone());
            if let DiskVerdict::Low { minutes } = verdict {
                g.low_disk_minutes = Some(minutes);
            }
        }
        emit_status(app);
        close_window(app, HUD_LABEL);

        if opts.countdown {
            open_window(app, COUNTDOWN_LABEL);
            std::thread::sleep(COUNTDOWN);
            close_window(app, COUNTDOWN_LABEL);
        }
        let still_ours = |st: &RecordState| {
            let g = st.inner.lock();
            g.gen == my_gen && g.phase == Phase::Countdown
        };
        if !still_ours(&st) {
            discard_dir(&prep.dir);
            clear_dir(&st, my_gen);
            emit_status(app);
            return Err(ERR_CANCELLED.into());
        }

        let active = match begin_capture(Some(app.clone()), &picked, &opts, prep) {
            Ok(a) => a,
            Err(boxed) => {
                let (prep, reason) = *boxed;
                fail_project(&prep.dir, prep.project, &reason);
                {
                    let mut g = st.inner.lock();
                    if g.gen == my_gen {
                        g.phase = next_phase(g.phase, Action::Fail).unwrap_or(Phase::Idle);
                        g.dir = None;
                    }
                }
                let _ = app.emit(
                    "record-finished",
                    FinishedEvent {
                        dir: prep.dir.to_string_lossy().into_owned(),
                        status: "failed".into(),
                        failure: Some(reason.clone()),
                    },
                );
                emit_status(app);
                announce_failure(app, failure_toast(true, true, Some(&reason)));
                return Err(format!("{ERR_START_FAILED}:{reason}"));
            }
        };

        // A cancel during capture start turned the phase back to idle.
        let ticker_stop = active.ticker_stop.clone();
        let leftover = {
            let mut g = st.inner.lock();
            if g.gen == my_gen && g.phase == Phase::Countdown {
                g.phase = Phase::Recording;
                g.active = Some(active);
                None
            } else {
                Some(active)
            }
        };
        if let Some(a) = leftover {
            let out = finish(a);
            discard_dir(&out.dir);
            clear_dir(&st, my_gen);
            emit_status(app);
            return Err(ERR_CANCELLED.into());
        }
        spawn_ticker(app.clone(), ticker_stop, picked.window_id);
        open_window(app, BAR_LABEL);
        crate::sound::play(crate::sound::Sound::RecordStart);
        emit_status(app);
        Ok(st.status())
    }

    fn clear_dir(st: &RecordState, gen: u64) {
        let mut g = st.inner.lock();
        if g.gen == gen {
            g.dir = None;
        }
    }

    fn discard_dir(dir: &Path) {
        if trash::delete(dir).is_err() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    pub fn pause(app: &AppHandle) -> Result<RecordStatus, String> {
        set_paused(app, true)
    }

    pub fn resume(app: &AppHandle) -> Result<RecordStatus, String> {
        set_paused(app, false)
    }

    fn set_paused(app: &AppHandle, paused: bool) -> Result<RecordStatus, String> {
        let st = app.state::<RecordState>();
        {
            let mut g = st.inner.lock();
            let action = if paused { Action::Pause } else { Action::Resume };
            g.phase = next_phase(g.phase, action).map_err(str::to_string)?;
            if let Some(a) = g.active.as_ref() {
                if paused {
                    a.clock.pause(host_now_ns());
                } else {
                    a.clock.resume(host_now_ns());
                }
            }
        }
        emit_status(app);
        Ok(st.status())
    }

    /// Take the running session out (phase → finishing).
    fn take_active(st: &RecordState, action: Action) -> Result<Active, String> {
        let mut g = st.inner.lock();
        let p = next_phase(g.phase, action).map_err(str::to_string)?;
        if p != Phase::Finishing {
            // Cancel during the countdown: the start thread cleans up.
            g.phase = p;
            return Err(ERR_CANCELLED.into());
        }
        let a = g.active.take().ok_or_else(|| ERR_INVALID.to_string())?;
        g.phase = p;
        Ok(a)
    }

    fn after_finish(app: &AppHandle, st: &RecordState) {
        {
            let mut g = st.inner.lock();
            g.phase = next_phase(g.phase, Action::Finished).unwrap_or(Phase::Idle);
            g.dir = None;
            g.low_disk_minutes = None;
        }
        close_window(app, BAR_LABEL);
        crate::sound::play(crate::sound::Sound::RecordStop);
        emit_status(app);
    }

    pub fn stop(app: &AppHandle) -> Result<String, String> {
        let st = app.state::<RecordState>();
        let a = take_active(&st, Action::Stop)?;
        emit_status(app);
        let out = finish(a);
        after_finish(app, &st);
        crate::commands::reveal_in_file_manager(&out.dir);
        let dir = out.dir.to_string_lossy().into_owned();
        announce_failure(app, failure_toast(false, out.status == ProjectStatus::Failed, out.failure.as_deref()));
        let _ = app.emit(
            "record-finished",
            FinishedEvent { dir: dir.clone(), status: status_str(out.status).into(), failure: out.failure },
        );
        Ok(dir)
    }

    pub fn cancel(app: &AppHandle) -> Result<(), String> {
        let st = app.state::<RecordState>();
        let a = match take_active(&st, Action::Cancel) {
            Ok(a) => a,
            Err(e) if e == ERR_CANCELLED => {
                close_window(app, COUNTDOWN_LABEL);
                emit_status(app);
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        emit_status(app);
        let out = finish(a);
        discard_dir(&out.dir);
        after_finish(app, &st);
        let _ = app.emit(
            "record-finished",
            FinishedEvent { dir: out.dir.to_string_lossy().into_owned(), status: "cancelled".into(), failure: None },
        );
        Ok(())
    }

    /// App exit: finalise a running take, no UI.
    pub fn shutdown(app: &AppHandle) {
        let Some(st) = app.try_state::<RecordState>() else { return };
        let a = {
            let mut g = st.inner.lock();
            g.phase = Phase::Idle;
            g.active.take()
        };
        if let Some(a) = a {
            tracing::info!("record: app exits during a take — finalising");
            let _ = finish(a);
        }
    }

    pub fn delete_project(app: &AppHandle, dir: &Path) -> Result<(), String> {
        let st = app.state::<RecordState>();
        let root = app
            .try_state::<crate::db::DbHandle>()
            .map(|db| RecordConfig::load(&db).root())
            .unwrap_or_else(|| resolve_root(""));
        let dir = project_dir_for_delete(&root, dir).map_err(str::to_string)?;
        if let Some(active) = st.active_dir() {
            if std::fs::canonicalize(active).ok().as_deref() == Some(dir.as_path()) {
                return Err(ERR_BUSY.into());
            }
        }
        trash::delete(&dir).map_err(|e| format!("{ERR_INVALID}:{e}"))
    }

    pub fn reveal_project(app: &AppHandle, dir: &Path) -> Result<(), String> {
        let root = app
            .try_state::<crate::db::DbHandle>()
            .map(|db| RecordConfig::load(&db).root())
            .unwrap_or_else(|| resolve_root(""));
        let dir = project_dir_inside_root(&root, dir).map_err(str::to_string)?;
        crate::commands::reveal_in_file_manager(&dir);
        Ok(())
    }

    // ---------- headless entry (live test) ----------

    /// Record `filter` for `secs`, pausing once in the middle. No windows,
    /// no picker, no AppHandle. Returns the finished outcome.
    #[cfg(test)]
    pub(crate) fn record_headless(root: &Path, picked: Picked, opts: StartOptions, secs: f64) -> Outcome {
        let (prep, _) = prepare_project(root, &picked, &opts).expect("prepare");
        let a = begin_capture(None, &picked, &opts, prep).map_err(|b| b.1).expect("begin");
        let third = Duration::from_secs_f64(secs / 3.0);
        std::thread::sleep(third);
        a.clock.pause(host_now_ns());
        std::thread::sleep(Duration::from_millis(700));
        a.clock.resume(host_now_ns());
        std::thread::sleep(third * 2);
        finish(a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_transitions_are_guarded() {
        assert_eq!(next_phase(Phase::Recording, Action::Start), Err(ERR_BUSY), "already running");
        assert_eq!(next_phase(Phase::Paused, Action::Start), Err(ERR_BUSY));
        assert_eq!(next_phase(Phase::Countdown, Action::Start), Err(ERR_BUSY));
        assert_eq!(next_phase(Phase::Finishing, Action::Start), Err(ERR_BUSY));
        assert_eq!(next_phase(Phase::Idle, Action::Pause), Err(ERR_NOT_RECORDING));
        assert_eq!(next_phase(Phase::Idle, Action::Stop), Err(ERR_NOT_RECORDING));
        assert_eq!(next_phase(Phase::Paused, Action::Stop), Ok(Phase::Finishing));
        assert_eq!(next_phase(Phase::Recording, Action::Stop), Ok(Phase::Finishing));
        assert_eq!(next_phase(Phase::Idle, Action::Start), Ok(Phase::Countdown));
        assert_eq!(next_phase(Phase::Picking, Action::Start), Ok(Phase::Countdown));
        assert_eq!(next_phase(Phase::Countdown, Action::Begin), Ok(Phase::Recording));
        assert_eq!(next_phase(Phase::Countdown, Action::Cancel), Ok(Phase::Idle));
        assert_eq!(next_phase(Phase::Recording, Action::Pause), Ok(Phase::Paused));
        assert_eq!(next_phase(Phase::Paused, Action::Resume), Ok(Phase::Recording));
        assert_eq!(next_phase(Phase::Finishing, Action::Finished), Ok(Phase::Idle));
        assert_eq!(next_phase(Phase::Finishing, Action::Pause), Err(ERR_INVALID));
        assert_eq!(next_phase(Phase::Recording, Action::Pick), Err(ERR_INVALID));
        assert_eq!(next_phase(Phase::Recording, Action::PickDone), Ok(Phase::Recording));
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ir-rec-sess-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn failures_get_a_toast_and_a_clean_stop_does_not() {
        assert!(failure_toast(false, false, None).is_none(), "a normal stop says nothing");
        assert!(failure_toast(false, false, Some("ignored")).is_none());
        let mid = failure_toast(false, true, Some("The video file could not be written (x).")).unwrap();
        assert_eq!(mid.kind, "record");
        assert_eq!(mid.subtitle, "The video file could not be written (x).");
        let start = failure_toast(true, true, Some("no frame")).unwrap();
        assert_ne!(start.title, mid.title, "start and mid-take failures read differently");
        assert!(!start.title.starts_with("rec2."), "title is translated, not a raw key");
        let blank = failure_toast(false, true, Some("  ")).unwrap();
        assert!(!blank.subtitle.trim().is_empty() && !blank.subtitle.starts_with("rec2."), "fallback reason");
    }

    #[test]
    fn delete_refuses_folders_without_project_json() {
        let base = tmp("del-noproj");
        let root = base.join("root");
        let user = root.join("My holiday clips");
        let broken = root.join("2026-11-01 10.00.00 Broken");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join(super::super::project::PROJECT_FILE), "{ not json").unwrap();
        assert!(project_dir_inside_root(&root, &user).is_ok(), "inside the root");
        assert_eq!(project_dir_for_delete(&root, &user), Err(ERR_OUTSIDE_ROOT), "no project.json");
        assert!(project_dir_for_delete(&root, &broken).is_ok(), "an unparseable project is still a project");
        assert_eq!(project_dir_for_delete(&root, &base.join("x")), Err(ERR_OUTSIDE_ROOT));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn delete_refuses_paths_outside_root() {
        let base = tmp("del");
        let root = base.join("root");
        let proj = root.join("2026-11-01 10.00.00 Take");
        let other = base.join("other");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        assert!(project_dir_inside_root(&root, &proj).is_ok());
        assert_eq!(project_dir_inside_root(&root, &root.join("../other")), Err(ERR_OUTSIDE_ROOT));
        assert_eq!(project_dir_inside_root(&root, &proj.join("..").join("..").join("other")), Err(ERR_OUTSIDE_ROOT));
        assert_eq!(project_dir_inside_root(&root, &other), Err(ERR_OUTSIDE_ROOT), "foreign absolute path");
        assert_eq!(project_dir_inside_root(&root, &root), Err(ERR_OUTSIDE_ROOT), "the root itself");
        assert_eq!(project_dir_inside_root(&root, &root.join(".")), Err(ERR_OUTSIDE_ROOT));
        assert_eq!(project_dir_inside_root(&root, &root.join("missing")), Err(ERR_OUTSIDE_ROOT));
        // Nested deeper than one level is not a project folder.
        let nested = proj.join("sub");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(project_dir_inside_root(&root, &nested), Err(ERR_OUTSIDE_ROOT));
        #[cfg(unix)]
        {
            let link = root.join("link");
            std::os::unix::fs::symlink(&other, &link).unwrap();
            assert_eq!(project_dir_inside_root(&root, &link), Err(ERR_OUTSIDE_ROOT), "symlink out");
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn pause_marks_sit_on_the_cut_in_video_time() {
        let s = 1_000_000_000i64;
        // origin at 10 s; pauses 12–13 s and 15–17 s (host time).
        let m = pause_marks(&[(12 * s, 13 * s), (15 * s, 17 * s)], 10 * s);
        assert_eq!(m, vec![(2_000, 1_000), (4_000, 2_000)]);
        assert!(pause_marks(&[], 0).is_empty());
    }

    #[test]
    fn elapsed_never_negative() {
        assert_eq!(elapsed_ms(5_000_000_000, Some(2_000_000_000)), 3_000);
        assert_eq!(elapsed_ms(1, Some(5)), 0);
        assert_eq!(elapsed_ms(9, None), 0);
    }

    #[test]
    fn active_project_is_not_listed_as_incomplete() {
        use super::super::project::{write_atomic, Project, ProjectStatus, PROJECT_FILE};
        let root = tmp("list");
        let mk = |name: &str| {
            let d = root.join(name);
            std::fs::create_dir_all(&d).unwrap();
            let p = Project {
                schema: 1,
                id: name.into(),
                title: name.into(),
                created_at: 1,
                updated_at: 1,
                status: ProjectStatus::Recording,
                failure: None,
                recording: Default::default(),
                editor: serde_json::json!({}),
            };
            write_atomic(&d, PROJECT_FILE, &p).unwrap();
            d
        };
        let live = mk("live");
        let dead = mk("dead");
        let items = list_items(&root, Some(&live));
        let get = |d: &Path| items.iter().find(|i| i.summary.dir == d).unwrap();
        assert!(get(&live).active);
        assert_eq!(get(&live).summary.status, ProjectStatus::Recording);
        assert!(!get(&dead).active);
        assert_eq!(get(&dead).summary.status, ProjectStatus::Incomplete);
        mark_interrupted(&root);
        let p = super::super::project::read_project(&dead).unwrap();
        assert_eq!(p.status, ProjectStatus::Incomplete);
        assert!(p.failure.is_some());
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// `caffeinate -u -t 20 & cargo test -p inspector-rust-core --lib session_live_display_5s -- --ignored --nocapture`
/// Needs Screen Recording + Microphone for the test binary's responsible
/// process and an awake display.
#[cfg(all(test, target_os = "macos"))]
mod live {
    use super::*;
    use crate::record::capture_macos::main_display_id;
    use crate::record::picker_macos::Picked;
    use crate::record::project::{read_project, ProjectStatus, CURSOR_FILE, MIC_FILE, SCREEN_FILE};

    #[test]
    #[ignore]
    fn session_live_display_5s() {
        let root = std::env::temp_dir().join(format!("ir-rec-session-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let picked = Picked::display(main_display_id()).expect("display filter");
        let opts = StartOptions {
            system_audio: true,
            mic: true,
            cursor_mode: CursorMode::EditableOverlay,
            countdown: false,
            title: "live".into(),
        };
        let out = record_headless(&root, picked, opts, 5.0);
        println!("dir {}", out.dir.display());
        println!("status {:?}, failure {:?}, video_ms {}, mic_ms {:?}", out.status, out.failure, out.video_ms, out.mic_ms);
        assert_eq!(out.status, ProjectStatus::Ready, "failure: {:?}", out.failure);
        for f in [PROJECT_FILE_NAME, SCREEN_FILE, MIC_FILE, CURSOR_FILE] {
            let len = std::fs::metadata(out.dir.join(f)).map(|m| m.len()).unwrap_or(0);
            println!("{f}: {len} bytes");
            assert!(len > 0, "{f} missing or empty");
        }
        let p = read_project(&out.dir).expect("project.json");
        println!("project.json status {:?}, pauses {:?}, {}x{}", p.status, p.recording.pauses, p.recording.width, p.recording.height);
        assert_eq!(p.status, ProjectStatus::Ready);
        assert_eq!(p.recording.pauses.len(), 1, "one pause");
        // WAV header: data chunk size at byte 40, 48 kHz mono i16.
        let wav = std::fs::read(out.dir.join(MIC_FILE)).unwrap();
        let data = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as f64;
        let wav_s = data / 2.0 / 48_000.0;
        let video_s = out.video_ms as f64 / 1000.0;
        println!("mic.wav {wav_s:.3} s, video {video_s:.3} s, diff {:.3} s", wav_s - video_s);
        assert!((wav_s - video_s).abs() <= 0.05, "mic {wav_s} vs video {video_s}");
        // 5 s of recording; the 0.7 s pause is cut out (wall clock ≈ 5.7 s).
        assert!((video_s - 5.0).abs() <= 0.5, "video {video_s}");
        // `IR_RECORD_KEEP=1` keeps the project (stage 2 uses it as a real fixture).
        if std::env::var_os("IR_RECORD_KEEP").is_none() {
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    const PROJECT_FILE_NAME: &str = crate::record::project::PROJECT_FILE;
}
