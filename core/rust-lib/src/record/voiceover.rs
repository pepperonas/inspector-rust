//! Recording a voiceover in the editor: the microphone (native, [`crate::mic_capture`]) into
//! `<project>/assets/voiceover-<ts>.wav` while the preview plays from the playhead.
//!
//! Time model: the preview transport is the authority. A take follows ONE continuous run of
//! the transport (`Transport::watch` in `editor_ipc`): a pause, seek, stall or the programme's
//! end ends the run, and the take's length is where the run stopped minus where it started
//! ([`span_from_run`]). Inside the run, a chunk lands at `now - start - chunk length` on the
//! take's own clock, armed together with the transport's play, so voice and picture share one
//! origin ([`WavSink`] pads gaps with silence). Chunks before the start or past the
//! programme's end are dropped; the stop cuts the file to the run's span.
//!
//! One take at a time ([`ERR_BUSY`]); it lives in [`ACTIVE`] until stop or cancel. The pure
//! parts (plan, anchor, track, span) are tested here; the session glue is in `editor_ipc`.

use super::editor::{self, AudioTrack, EditDoc, TrackKind};
use super::scene_json::PROJECT_ASSETS_DIR;
use super::wav::{WavSink, OUT_RATE};
use crate::mic_capture::{self, MicStream};
use parking_lot::Mutex;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter};

pub const ERR_BUSY: &str = "record.editor.voiceover_busy";
pub const ERR_NO_MIC: &str = "record.editor.no_mic";
/// The microphone permission was refused (System Settings → Privacy → Microphone).
pub const ERR_MIC_DENIED: &str = "record.editor.mic_denied";
/// The playhead sits at (or past) the programme's end: there is no room to record into.
pub const ERR_AT_END: &str = "record.editor.voiceover_at_end";
/// Stop/cancel without a running take (or one of another session).
pub const ERR_NOT_RECORDING: &str = "record.editor.voiceover_none";
/// Nothing usable was recorded (shorter than [`MIN_SPAN_MS`]); the file is removed.
pub const ERR_EMPTY: &str = "record.editor.voiceover_empty";
pub const ERR_FAILED: &str = "record.editor.voiceover_failed";

/// `{ rms, elapsedMs, ended }` about 10× a second while a take runs.
pub const EV_LEVEL: &str = "record-voiceover-level";
/// `{ sessionId }`: a take that ended by itself (programme end, pause, seek, stall) was laid
/// under the programme; `editor_voiceover_stop` hands out the result once.
pub const EV_FINISHED: &str = "record-voiceover-finished";
/// How long the take waits for the microphone's first chunk before it gives up.
pub const INPUT_TIMEOUT: Duration = Duration::from_millis(2_500);
/// Voiceover files without a track older than this are removed when the editor opens.
pub const ORPHAN_AGE: Duration = Duration::from_secs(3_600);
pub const LEVEL_INTERVAL: Duration = Duration::from_millis(100);
/// A take shorter than this is a mis-click, not a voiceover.
pub const MIN_SPAN_MS: u64 = 100;

/// Where a take starts: the playhead, its clip (with that clip's recording) and source time
/// there, and how much programme is left after it (the most the take may last).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub programme_ms: u64,
    pub clip: usize,
    /// The clip's recording (`None` = the project's own take): finds the picture again after
    /// the clips were reordered during the take.
    pub source: Option<String>,
    pub source_ms: u64,
    pub max_ms: u64,
}

/// What the editor shows while a take runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceoverState {
    pub session_id: u64,
    /// File name under `<project>/assets/`.
    pub file: String,
    pub programme_ms: u64,
    /// The take ends by itself after this long (the programme's end).
    pub max_ms: u64,
    pub clip: usize,
    pub source_ms: u64,
}

/// The take's plan on the playing document `play` (Task-2 mapping: programme → clip/source).
pub fn plan(play: &EditDoc, programme_ms: u64) -> Result<Plan, String> {
    let total = editor::programme_duration_ms(play);
    let max_ms = total.saturating_sub(programme_ms);
    if max_ms == 0 {
        return Err(ERR_AT_END.into());
    }
    let (clip, source_ms) = editor::programme_to_source(play, programme_ms).ok_or(ERR_AT_END)?;
    let source = play.clips.get(clip).and_then(|c| c.source.clone());
    Ok(Plan { programme_ms, clip, source, source_ms, max_ms })
}

/// The take's length from the transport: where its run stopped (or the playhead now) minus
/// where it started, never past the programme's end.
pub fn span_from_run(position_ms: u64, plan: &Plan) -> u64 {
    position_ms.saturating_sub(plan.programme_ms).min(plan.max_ms)
}

/// Where the finished take is anchored in `doc`: the clip that shows the picture it was
/// recorded against — the plan's clip if it still holds that recording and source time, else
/// any clip that does (the clips were reordered or duplicated during the take), else the
/// plan's programme time on the document as it plays now.
pub fn anchor(doc: &EditDoc, play: &EditDoc, plan: &Plan) -> Option<(usize, u64)> {
    let shows = |c: &editor::EditClip| c.source == plan.source && (c.start_ms..=c.end_ms).contains(&plan.source_ms);
    if doc.clips.get(plan.clip).is_some_and(shows) {
        return Some((plan.clip, plan.source_ms));
    }
    if let Some(i) = doc.clips.iter().position(shows) {
        return Some((i, plan.source_ms));
    }
    editor::programme_to_source(play, plan.programme_ms)
}

/// The track for a take: voiceover, 0 dB, no fades, the recorded length as its span (the
/// same defaults as the editor's `addTrack`, pinned by a test that reads `editor.ts`).
pub fn voiceover_track(id: String, file: &str, clip: usize, source_ms: u64, span_ms: u64) -> AudioTrack {
    AudioTrack {
        id,
        kind: TrackKind::Voiceover,
        file: file.to_string(),
        clip,
        start_ms: source_ms,
        duration_ms: span_ms,
        offset_ms: 0,
        span_ms: Some(span_ms),
        gain_db: 0.0,
        looped: false,
        fade_in_ms: 0,
        fade_out_ms: 0,
        muted: false,
        label: String::new(),
    }
}

/// `voiceover-<ts>.wav`, or `voiceover-<ts>-<n>.wav` when that name is taken in `assets`.
pub fn free_file_name(assets: &Path, ts_ms: u64) -> String {
    // `taken`, not `!exists()`: a dangling planted link "doesn't exist" but is not free.
    use record_render_protocol::safe_fs::taken;
    let first = format!("voiceover-{ts_ms}.wav");
    if !taken(&assets.join(&first)) {
        return first;
    }
    (2..)
        .map(|n| format!("voiceover-{ts_ms}-{n}.wav"))
        .find(|name| !taken(&assets.join(name)))
        .unwrap_or(first)
}

/// Exactly the names [`free_file_name`] makes: `voiceover-<digits>[-<digits>].wav`.
pub fn is_take_name(name: &str) -> bool {
    let Some(stem) = name.strip_prefix("voiceover-").and_then(|r| r.strip_suffix(".wav")) else { return false };
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    match stem.split_once('-') {
        None => digits(stem),
        Some((ts, n)) => digits(ts) && digits(n),
    }
}

/// Removes take files in `assets` that no track uses and that are older than `max_age` (an
/// interrupted take: crash, kill). Only our own names are touched. Returns how many went.
pub fn sweep_orphans(assets: &Path, referenced: &HashSet<String>, now: SystemTime, max_age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(assets) else { return 0 };
    let mut removed = 0;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !is_take_name(&name) || referenced.contains(&name) {
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let old = meta.modified().ok().and_then(|m| now.duration_since(m).ok()).is_some_and(|age| age >= max_age);
        if old && std::fs::remove_file(e.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Microphone availability as the take sees it before it opens anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicAccess {
    Ready,
    /// The user has not been asked yet: opening the input asks.
    Undetermined,
    Denied,
    Missing,
}

/// [`MicAccess`] as an error, or `Ok` when the take may open the input.
pub fn check_access(access: MicAccess) -> Result<(), String> {
    match access {
        MicAccess::Ready | MicAccess::Undetermined => Ok(()),
        MicAccess::Denied => Err(ERR_MIC_DENIED.into()),
        MicAccess::Missing => Err(ERR_NO_MIC.into()),
    }
}

/// The error for a microphone that delivered nothing: refused (or still unanswered) permission
/// says so, anything else is a missing or broken input.
pub fn silent_input_error(access: MicAccess) -> &'static str {
    match access {
        MicAccess::Denied | MicAccess::Undetermined => ERR_MIC_DENIED,
        MicAccess::Ready | MicAccess::Missing => ERR_NO_MIC,
    }
}

/// Is there an input device, and may this app use it?
pub fn mic_access() -> MicAccess {
    use cpal::traits::HostTrait;
    if cpal::default_host().default_input_device().is_none() {
        return MicAccess::Missing;
    }
    permission()
}

/// The permission alone (refused while a take ran = the take recorded silence).
pub fn mic_denied() -> bool {
    permission() == MicAccess::Denied
}

#[cfg(target_os = "macos")]
fn permission() -> MicAccess {
    use objc2::msg_send;
    use objc2::runtime::AnyClass;
    let Some(cls) = AnyClass::get(c"AVCaptureDevice") else { return MicAccess::Undetermined };
    // SAFETY: `AVMediaTypeAudio` is a static NSString provided by AVFoundation (linked by the
    // recorder); `authorizationStatusForMediaType:` is a class method returning NSInteger.
    let Some(media) = (unsafe { objc2_av_foundation::AVMediaTypeAudio }) else { return MicAccess::Undetermined };
    let status: isize = unsafe { msg_send![cls, authorizationStatusForMediaType: media] };
    match status {
        3 => MicAccess::Ready,
        0 => MicAccess::Undetermined,
        _ => MicAccess::Denied, // 1 restricted, 2 denied
    }
}

#[cfg(not(target_os = "macos"))]
fn permission() -> MicAccess {
    MicAccess::Ready
}

/// Where the take's run stands, as the level thread reads it from the transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// The preview has not started yet.
    Waiting,
    /// Recording; ms into the take.
    Running(u64),
    /// The run stopped (pause, seek, stall, end) after this many ms.
    Ended(u64),
    /// The session is gone.
    Gone,
}

/// The parts the microphone callback shares with the take.
struct Shared {
    /// `None` once the take is finished or discarded: late chunks are dropped.
    sink: Mutex<Option<WavSink>>,
    /// When the preview started (set by [`Arm::arm`]); chunks before it are dropped.
    start: OnceLock<Instant>,
    max_ms: u64,
    /// Highest chunk RMS since the last level event, as `f32` bits (non-negative floats order
    /// like their bits).
    peak_rms: AtomicU32,
    /// Chunks the microphone delivered at all (also before the start).
    arrivals: AtomicU32,
    /// Chunks written into the file.
    chunks: AtomicU32,
}

impl Shared {
    /// One microphone chunk that ended now.
    fn push(&self, chunk: &[f32], rate: u32) {
        if chunk.is_empty() || rate == 0 {
            return;
        }
        self.arrivals.fetch_add(1, Ordering::Relaxed);
        let Some(start) = self.start.get() else { return };
        let dur_ns = chunk.len() as i64 * 1_000_000_000 / rate as i64;
        let at = start.elapsed().as_nanos() as i64 - dur_ns;
        self.push_at(at, chunk, rate);
    }

    /// `chunk` starting at `at_ns` on the take's clock. A chunk that starts before the take
    /// (the input warms up before the preview starts) or after its room is dropped.
    fn push_at(&self, at_ns: i64, chunk: &[f32], rate: u32) {
        if at_ns < 0 || at_ns >= self.max_ms as i64 * 1_000_000 {
            return;
        }
        let rms = mic_capture::chunk_rms(chunk);
        self.peak_rms.fetch_max(rms.max(0.0).to_bits(), Ordering::Relaxed);
        let mut guard = self.sink.lock();
        let Some(sink) = guard.as_mut() else { return };
        sink.set_input_rate(rate);
        match sink.push(at_ns, chunk) {
            Ok(()) => {
                self.chunks.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => tracing::warn!(target: "record::voiceover", "write failed: {e}"),
        }
    }
}

/// Starts a take's clock; taken before the take is installed, so arming never needs the slot.
pub struct Arm(Arc<Shared>);

impl Arm {
    /// Call together with the transport's play.
    pub fn arm(&self) {
        let _ = self.0.start.set(Instant::now());
    }
}

/// A running take.
pub struct Take {
    pub state: VoiceoverState,
    pub plan: Plan,
    path: PathBuf,
    shared: Arc<Shared>,
    /// Released when the run ends (level thread), or by finish/discard.
    mic: Arc<Mutex<Option<MicStream>>>,
    running: Arc<AtomicBool>,
}

impl Take {
    /// Creates the WAV for `plan` in `<project_dir>/assets/` (the take records nothing before
    /// it is armed).
    pub fn create(session_id: u64, project_dir: &Path, plan: Plan, ts_ms: u64) -> Result<Self, String> {
        // A symlinked `assets/` is refused; the WAV itself is created new (WavSink, O_NOFOLLOW).
        let assets = record_render_protocol::safe_fs::plain_dir(project_dir, PROJECT_ASSETS_DIR)
            .map_err(|e| format!("{ERR_FAILED}:{e}"))?;
        let file = free_file_name(&assets, ts_ms);
        let path = assets.join(&file);
        let mut sink = WavSink::create(&path, OUT_RATE).map_err(|e| format!("{ERR_FAILED}:{e}"))?;
        sink.set_origin(0);
        Ok(Take {
            state: VoiceoverState {
                session_id,
                file,
                programme_ms: plan.programme_ms,
                max_ms: plan.max_ms,
                clip: plan.clip,
                source_ms: plan.source_ms,
            },
            path,
            shared: Arc::new(Shared {
                sink: Mutex::new(Some(sink)),
                start: OnceLock::new(),
                max_ms: plan.max_ms,
                peak_rms: AtomicU32::new(0),
                arrivals: AtomicU32::new(0),
                chunks: AtomicU32::new(0),
            }),
            plan,
            mic: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(true)),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn arm_handle(&self) -> Arm {
        Arm(self.shared.clone())
    }

    /// Opens the microphone and starts the level thread: [`EV_LEVEL`] every
    /// [`LEVEL_INTERVAL`] from `progress` (the transport); when the run ends it releases the
    /// microphone, sends `ended: true` once and calls `on_end` (which lays the take under the
    /// programme). Open before arming, so the input is warm when the take starts.
    pub fn open_mic(
        &self,
        app: AppHandle,
        progress: impl Fn() -> Progress + Send + 'static,
        on_end: impl FnOnce() + Send + 'static,
    ) -> Result<(), String> {
        let shared = self.shared.clone();
        *self.mic.lock() = Some(mic_capture::start_chunks(app.clone(), move |c: &[f32], r: u32| shared.push(c, r)));
        let (shared, mic, running) = (self.shared.clone(), self.mic.clone(), self.running.clone());
        std::thread::Builder::new()
            .name("ir-record-voiceover".into())
            .spawn(move || level_loop(app, shared, mic, running, progress, on_end))
            .map(|_| ())
            .map_err(|e| {
                self.release();
                format!("{ERR_FAILED}:{e}")
            })
    }

    /// [`open_mic`](Self::open_mic) without an app or level thread (live test).
    #[cfg(test)]
    fn open_mic_headless(&self) {
        let shared = self.shared.clone();
        *self.mic.lock() = Some(mic_capture::start_chunks_headless(move |c: &[f32], r: u32| shared.push(c, r)));
    }

    /// Waits until the microphone delivered its first chunk; `false` after `timeout`.
    pub fn wait_for_input(&self, timeout: Duration) -> bool {
        let until = Instant::now() + timeout;
        while self.shared.arrivals.load(Ordering::Relaxed) == 0 {
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
    }

    /// Chunks written so far.
    pub fn chunks(&self) -> u32 {
        self.shared.chunks.load(Ordering::Relaxed)
    }

    fn release(&self) -> Option<WavSink> {
        self.running.store(false, Ordering::SeqCst);
        if let Some(m) = self.mic.lock().take() {
            m.stop();
        }
        self.shared.sink.lock().take()
    }

    fn remove_file(&self) {
        if let Err(e) = std::fs::remove_file(&self.path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(target: "record::voiceover", "{}: {e}", self.path.display());
            }
        }
    }

    /// Ends the take at `span_ms` (from [`span_from_run`]): the file is cut (or padded) to it
    /// and closed. Returns `(file name, span ms)`. On any failure the file is removed: a take
    /// shorter than [`MIN_SPAN_MS`] ([`ERR_EMPTY`]), one the microphone delivered nothing to
    /// ([`ERR_NO_MIC`] — no silent take), a write error ([`ERR_FAILED`]).
    pub fn finish_at(self, span_ms: u64) -> Result<(String, u64), String> {
        let span = span_ms.min(self.plan.max_ms);
        let Some(mut sink) = self.release() else {
            self.remove_file();
            return Err(ERR_FAILED.into());
        };
        let end_ns = span as i64 * 1_000_000;
        let written = sink.pad_to(end_ns).and_then(|_| sink.truncate_to(end_ns)).and_then(|_| sink.finish());
        let fail = match written {
            Err(e) => Some(format!("{ERR_FAILED}:{e}")),
            Ok(ms) if ms < MIN_SPAN_MS => Some(ERR_EMPTY.to_string()),
            Ok(_) if self.chunks() == 0 => Some(ERR_NO_MIC.to_string()),
            Ok(ms) => return Ok((self.state.file.clone(), ms)),
        };
        self.remove_file();
        Err(fail.unwrap_or_else(|| ERR_FAILED.into()))
    }

    /// Stops the microphone and removes the file.
    pub fn discard(self) {
        drop(self.release());
        self.remove_file();
    }

    #[cfg(test)]
    pub(crate) fn push_at(&self, at_ns: i64, chunk: &[f32], rate: u32) {
        self.shared.push_at(at_ns, chunk, rate);
    }
}

fn level_loop(
    app: AppHandle,
    shared: Arc<Shared>,
    mic: Arc<Mutex<Option<MicStream>>>,
    running: Arc<AtomicBool>,
    progress: impl Fn() -> Progress,
    on_end: impl FnOnce(),
) {
    while running.load(Ordering::SeqCst) {
        std::thread::sleep(LEVEL_INTERVAL);
        if !running.load(Ordering::SeqCst) {
            return;
        }
        let rms = f32::from_bits(shared.peak_rms.swap(0, Ordering::Relaxed));
        let (elapsed, ended) = match progress() {
            Progress::Waiting => (0, false),
            Progress::Running(ms) => (ms, false),
            Progress::Ended(ms) => (ms, true),
            Progress::Gone => return,
        };
        let _ = app.emit(EV_LEVEL, serde_json::json!({ "rms": rms, "elapsedMs": elapsed.min(shared.max_ms), "ended": ended }));
        if ended {
            if let Some(m) = mic.lock().take() {
                m.stop();
            }
            if running.load(Ordering::SeqCst) {
                on_end();
            }
            return;
        }
    }
}

/// The running take, if any.
pub static ACTIVE: Mutex<Option<Take>> = Mutex::new(None);

/// Puts `take` into `slot` unless one is running there already (then `take` is discarded —
/// its file was just created — and the running one is untouched).
pub fn install(slot: &mut Option<Take>, take: Take) -> Result<VoiceoverState, String> {
    if slot.is_some() {
        take.discard();
        return Err(ERR_BUSY.into());
    }
    let state = take.state.clone();
    *slot = Some(take);
    Ok(state)
}

/// The running take of `session_id`, taken out of `slot`.
pub fn take_for(slot: &mut Option<Take>, session_id: u64) -> Result<Take, String> {
    match slot {
        Some(t) if t.state.session_id == session_id => Ok(slot.take().expect("checked")),
        _ => Err(ERR_NOT_RECORDING.into()),
    }
}

/// Discards whatever take runs (the editor closed or switched projects mid-take).
pub fn abort() {
    let take = ACTIVE.lock().take();
    if let Some(t) = take {
        t.discard();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::editor::{EditClip, SpeedRegion, EDITOR_VERSION};
    use crate::record::wav::wav_info;

    fn doc(clips: &[(u64, u64)]) -> EditDoc {
        let mut d: EditDoc = serde_json::from_value(serde_json::json!({ "version": EDITOR_VERSION, "clips": [] })).unwrap();
        d.clips = clips.iter().map(|&(s, e)| EditClip::main(s, e)).collect();
        d.crop = vec![None; clips.len()];
        d
    }

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ir-voiceover-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn plan_of(max_ms: u64) -> Plan {
        Plan { programme_ms: 0, clip: 0, source: None, source_ms: 0, max_ms }
    }

    fn tone(ms: u64) -> Vec<f32> {
        (0..(48 * ms)).map(|i| ((i as f32) * 0.05).sin() * 0.3).collect()
    }

    #[test]
    fn voiceover_stops_at_programme_end() {
        let p = Plan { programme_ms: 3_000, ..plan_of(1_000) };
        assert_eq!(span_from_run(5_000, &p), 1_000, "the run went past the room: cut at the end");
        assert_eq!(span_from_run(3_500, &p), 500, "paused early: as far as the run went");
        assert_eq!(span_from_run(2_000, &p), 0, "a run that ended before the start");
        // 2 s of sound into a take with 1 s of room: the file is exactly 1 s.
        let dir = tmp("end");
        let take = Take::create(1, &dir, plan_of(1_000), 7).unwrap();
        let path = take.path().to_path_buf();
        for i in 0..20 {
            take.push_at(i * 100_000_000, &tone(100), 48_000);
        }
        let (file, span) = take.finish_at(span_from_run(2_000, &plan_of(1_000))).unwrap();
        assert_eq!((file.as_str(), span), ("voiceover-7.wav", 1_000));
        assert_eq!(wav_info(&path).unwrap().frames, 48_000);
        // Stopped early: the file is as long as the take ran (padded when the input lagged).
        let take = Take::create(1, &dir, plan_of(5_000), 8).unwrap();
        let path = take.path().to_path_buf();
        take.push_at(0, &tone(200), 48_000);
        assert_eq!(take.finish_at(700).unwrap().1, 700);
        assert_eq!(wav_info(&path).unwrap().frames, 48 * 700);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn chunks_outside_the_take_are_dropped() {
        let dir = tmp("outside");
        let take = Take::create(1, &dir, plan_of(1_000), 1).unwrap();
        take.push_at(-50_000_000, &tone(100), 48_000);
        take.push_at(1_000_000_000, &tone(100), 48_000);
        assert_eq!(take.chunks(), 0);
        take.push_at(0, &tone(100), 48_000);
        assert_eq!(take.chunks(), 1);
        take.discard();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn planted_links_are_never_written_through() {
        let dir = tmp("links");
        let outside = tmp("links-outside");
        let victim = outside.join("victim.wav");
        std::fs::write(&victim, b"keep me").unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        // A dangling link at the name the take would pick, and a live one at the next name.
        std::os::unix::fs::symlink(outside.join("would-be-created.wav"), dir.join("assets/voiceover-7.wav")).unwrap();
        std::os::unix::fs::symlink(&victim, dir.join("assets/voiceover-7-2.wav")).unwrap();
        let take = Take::create(1, &dir, plan_of(4_000), 7).unwrap();
        assert_eq!(take.path(), dir.join("assets/voiceover-7-3.wav"), "taken names are skipped, dangling ones too");
        take.discard();
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep me");
        assert!(!outside.join("would-be-created.wav").exists());
        // A symlinked assets/ is refused outright.
        let linked = tmp("links-dir");
        std::os::unix::fs::symlink(&outside, linked.join("assets")).unwrap();
        assert!(Take::create(1, &linked, plan_of(4_000), 8).is_err());
        assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 1, "nothing written outside");
        for d in [dir, outside, linked] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn voiceover_cancel_discards() {
        let dir = tmp("cancel");
        let mut slot = None;
        let take = Take::create(3, &dir, plan_of(4_000), 9).unwrap();
        let path = take.path().to_path_buf();
        assert_eq!(path, dir.join("assets/voiceover-9.wav"));
        install(&mut slot, take).unwrap();
        slot.as_ref().unwrap().push_at(0, &tone(300), 48_000);
        assert!(path.is_file());
        assert_eq!(take_for(&mut slot, 4).err().as_deref(), Some(ERR_NOT_RECORDING), "another session's take");
        take_for(&mut slot, 3).unwrap().discard();
        assert!(slot.is_none());
        assert!(!path.exists(), "the file is gone");
        assert!(std::fs::read_dir(dir.join("assets")).unwrap().next().is_none(), "nothing left behind");
        // A take that recorded nothing usable is removed on stop too.
        let take = Take::create(3, &dir, plan_of(4_000), 10).unwrap();
        let path = take.path().to_path_buf();
        assert_eq!(take.finish_at(40).err().as_deref(), Some(ERR_EMPTY));
        assert!(!path.exists());
        // Nothing from the microphone at all: no silent take, the file goes too.
        let take = Take::create(3, &dir, plan_of(4_000), 11).unwrap();
        let path = take.path().to_path_buf();
        assert_eq!(take.finish_at(2_000).err().as_deref(), Some(ERR_NO_MIC));
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn voiceover_anchor_from_programme_ms() {
        // Clip 0 plays source 0–4 s with 2–4 s at 2× (3 s programme), clip 1 plays 10–14 s.
        let mut d = doc(&[(0, 4_000), (10_000, 14_000)]);
        d.speed = vec![SpeedRegion { clip: 0, start_ms: 2_000, end_ms: 4_000, speed: 2.0 }];
        let d = editor::normalize(d, 20_000, &Default::default());
        let total = editor::programme_duration_ms(&d);
        assert_eq!(total, 7_000);
        let p = plan(&d, 2_500).unwrap();
        assert_eq!((p.clip, p.source_ms, p.max_ms), (0, 3_000, 4_500), "inside the 2× region");
        let p = plan(&d, 5_000).unwrap();
        assert_eq!((p.clip, p.source_ms, p.max_ms), (1, 12_000, 2_000));
        assert_eq!(Some((p.clip, p.source_ms)), editor::programme_to_source(&d, 5_000), "the Task-2 mapping");
        assert_eq!(plan(&d, total).err().as_deref(), Some(ERR_AT_END));
        assert_eq!(plan(&d, total + 1).err().as_deref(), Some(ERR_AT_END));
        // The track: voiceover, 0 dB, no fades, the span as recorded.
        let t = voiceover_track("id".into(), "voiceover-1.wav", p.clip, p.source_ms, 1_234);
        assert_eq!((t.kind, t.gain_db, t.fade_in_ms, t.fade_out_ms), (TrackKind::Voiceover, 0.0, 0, 0));
        assert_eq!((t.clip, t.start_ms, t.duration_ms, t.span_ms, t.offset_ms), (1, 12_000, 1_234, Some(1_234), 0));
        // The anchor keeps the plan while its clip exists, else maps the playhead again.
        assert_eq!(anchor(&d, &d, &p), Some((1, 12_000)));
        // Clip 0 shortened during the take: the playhead now maps elsewhere, the take keeps
        // the picture it was recorded against.
        let shorter = editor::normalize(doc(&[(0, 2_000), (10_000, 14_000)]), 20_000, &Default::default());
        assert_eq!(editor::programme_to_source(&shorter, 5_000), Some((1, 13_000)));
        assert_eq!(anchor(&shorter, &shorter, &p), Some((1, 12_000)));
        // Reordered during the take: the clip that shows the recorded picture is found again.
        let swapped = editor::normalize(doc(&[(10_000, 14_000), (0, 4_000)]), 20_000, &Default::default());
        assert_eq!(anchor(&swapped, &swapped, &p), Some((0, 12_000)));
        // No clip shows it any more: the playhead's mapping now.
        let one = editor::normalize(doc(&[(0, 8_000)]), 20_000, &Default::default());
        assert_eq!(anchor(&one, &one, &p), Some((0, 5_000)));
    }

    #[test]
    fn only_our_unused_old_takes_are_swept() {
        for ok in ["voiceover-1.wav", "voiceover-1793000000000.wav", "voiceover-17-2.wav"] {
            assert!(is_take_name(ok), "{ok}");
        }
        for bad in ["voiceover-.wav", "voiceover-x.wav", "voiceover-1-2-3.wav", "voiceover-1.mp3", "song.wav", "voiceover-1-.wav", "xvoiceover-1.wav"] {
            assert!(!is_take_name(bad), "{bad}");
        }
        let dir = tmp("sweep");
        let assets = dir.join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        for f in ["voiceover-1.wav", "voiceover-2.wav", "voiceover-3-2.wav", "music.wav", "voiceover-x.wav"] {
            std::fs::write(assets.join(f), b"x").unwrap();
        }
        let used: HashSet<String> = ["voiceover-2.wav".to_string()].into();
        assert_eq!(sweep_orphans(&assets, &used, SystemTime::now(), ORPHAN_AGE), 0, "young files stay (a take may be running)");
        let later = SystemTime::now() + Duration::from_secs(2 * 3_600);
        assert_eq!(sweep_orphans(&assets, &used, later, ORPHAN_AGE), 2);
        let mut left: Vec<String> = std::fs::read_dir(&assets).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["music.wav", "voiceover-2.wav", "voiceover-x.wav"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The track defaults are the editor's `addTrack(voiceover)` defaults.
    #[test]
    fn track_defaults_match_the_editor() {
        let ts = include_str!("../../../frontend/src/lib/editor.ts");
        let body = &ts[ts.find("export function addTrack(").expect("addTrack")..];
        let body = &body[..body.find("\n}\n").expect("end of addTrack")];
        for line in ["offsetMs: 0,", "spanMs: music ? null : duration,", "gainDb: music ? MUSIC_DEFAULT_GAIN_DB : 0,", "looped: false,", "fadeInMs: music ? MUSIC_DEFAULT_FADE_MS : 0,", "fadeOutMs: music ? MUSIC_DEFAULT_FADE_MS : 0,", "muted: false,", "label: \"\","] {
            assert!(body.contains(line), "editor.ts addTrack no longer has `{line}`");
        }
        let t = voiceover_track("i".into(), "f.wav", 0, 0, 900);
        assert_eq!((t.offset_ms, t.span_ms, t.gain_db, t.looped, t.fade_in_ms, t.fade_out_ms, t.muted, t.label.as_str()), (0, Some(900), 0.0, false, 0, 0, false, ""));
        assert_eq!(t.duration_ms, 900, "durationMs = the file's length = the recorded span");
    }

    #[test]
    fn silent_input_names_the_cause() {
        assert_eq!(silent_input_error(MicAccess::Denied), ERR_MIC_DENIED);
        assert_eq!(silent_input_error(MicAccess::Undetermined), ERR_MIC_DENIED, "the prompt is still open");
        assert_eq!(silent_input_error(MicAccess::Ready), ERR_NO_MIC);
    }

    #[test]
    fn second_start_is_busy() {
        let dir = tmp("busy");
        let mut slot = None;
        let first = Take::create(1, &dir, plan_of(4_000), 1).unwrap();
        let first_path = first.path().to_path_buf();
        install(&mut slot, first).unwrap();
        let second = Take::create(1, &dir, plan_of(4_000), 1).unwrap();
        let second_path = second.path().to_path_buf();
        assert_ne!(first_path, second_path, "a taken name is not reused");
        assert_eq!(install(&mut slot, second).err().as_deref(), Some(ERR_BUSY));
        assert!(!second_path.exists(), "the refused take leaves no file");
        assert!(first_path.is_file(), "the running take is untouched");
        assert_eq!(slot.as_ref().unwrap().state.file, "voiceover-1.wav");
        take_for(&mut slot, 1).unwrap().discard();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_or_refused_microphone_is_a_sentinel() {
        assert_eq!(check_access(MicAccess::Missing).err().as_deref(), Some(ERR_NO_MIC));
        assert_eq!(check_access(MicAccess::Denied).err().as_deref(), Some(ERR_MIC_DENIED));
        assert!(check_access(MicAccess::Ready).is_ok());
        assert!(check_access(MicAccess::Undetermined).is_ok(), "opening the input asks");
    }

    /// Records 3 s from the real microphone. Needs an input device and the Microphone grant
    /// for the test binary; without one it reports and passes nothing off as a recording.
    #[test]
    #[ignore]
    fn voiceover_live_records_three_seconds() {
        let access = mic_access();
        eprintln!("microphone access: {access:?}");
        if check_access(access).is_err() {
            eprintln!("no usable microphone: skipped");
            return;
        }
        let dir = tmp("live");
        let take = Take::create(1, &dir, plan_of(60_000), 1).unwrap();
        let path = take.path().to_path_buf();
        take.open_mic_headless();
        assert!(take.wait_for_input(INPUT_TIMEOUT), "the microphone delivered nothing");
        let started = Instant::now();
        take.arm_handle().arm();
        std::thread::sleep(Duration::from_secs(3));
        let elapsed = started.elapsed().as_millis() as u64;
        let chunks = take.chunks();
        let peak = f32::from_bits(take.shared.peak_rms.load(Ordering::Relaxed));
        let (_, span) = take.finish_at(elapsed).unwrap();
        eprintln!("peak rms {peak:.5}");
        let frames = wav_info(&path).unwrap().frames;
        eprintln!("chunks {chunks}, span {span} ms, frames {frames} ({:.3} s)", frames as f64 / 48_000.0);
        assert!((2_950..=3_150).contains(&span), "{span}");
        assert_eq!(frames, span * 48);
        assert!(chunks > 0, "the microphone delivered nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
