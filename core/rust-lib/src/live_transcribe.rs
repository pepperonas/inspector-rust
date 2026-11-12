//! Live dictation: `transcribe --start` records the microphone and shows the
//! text while you speak; `transcribe --stop` (or Enter / the stop button) ends
//! it and the text goes to the clipboard + history. `--song` transcribes what
//! this Mac plays, `--room` a song heard through the microphone.
//!
//! The audio, the voice detection and Whisper run in the `ir-transcribe`
//! helper — the same engine as `meeting`, in `Mode::Dictation` / `Song` /
//! `Room` — with a private cache folder as the session root that is deleted
//! afterwards. This module keeps the [`LiveJob`] the panel renders, the song
//! recognition (Shazam windows come from the helper via `Command::Window`)
//! and the optional ducking of the music.
//!
//! The session lives in the backend, so closing the popup never stops it; the
//! footer shows a REC light and typing `transcribe` reconnects via `status()`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use transcribe_protocol::{Command, DedupeMode, Event, Mode, SessionConfig, Source, SourceStatus};

/// Tauri event with the full [`LiveJob`] snapshot after every change.
pub const LIVE_EVENT: &str = "transcribe-live";
/// Tauri event fired once when a session ends ([`LiveDone`]).
pub const LIVE_DONE_EVENT: &str = "transcribe-live-done";
/// Error sentinel: a dictation (or a file job) is already running.
pub const ERR_BUSY: &str = "transcribe.busy";

/// Sample rate of everything below.
pub const RATE: usize = 16_000;
/// Hard stop after an hour.
pub const MAX_SECONDS: u64 = 3600;
/// 20 ms analysis frames (song windows).
pub const FRAME: usize = RATE / 50;
/// Below this frame RMS (−40 dBFS) a frame is silence, whatever is playing.
pub const MIN_THRESHOLD: f32 = 0.01;
/// A microphone that sends no audio this long after the start is broken.
pub const NO_AUDIO_AFTER: Duration = Duration::from_secs(5);
/// How long the helper may take to decode what was open at the stop.
pub const STOP_WAIT: Duration = Duration::from_secs(120);

fn frame_rms(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum: f64 = frame.iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum();
    (sum / frame.len() as f64).sqrt() as f32
}

/// The helper's `Window` answer as samples.
pub fn decode_window(pcm_b64: &str) -> Vec<i16> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(pcm_b64)
        .map(|b| b.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect())
        .unwrap_or_default()
}

/// The session the helper runs for a dictation or song. Nothing of it is
/// kept: `root` is a private folder deleted at the end. Pure.
pub fn session_config(source: LiveSource, lang: transcribe_protocol::Lang, cfg: &crate::meeting::MeetingConfig, root: PathBuf) -> SessionConfig {
    let (mode, sources) = source.session();
    SessionConfig {
        sources,
        mic_device: cfg.mic_device.clone(),
        model: cfg.model.clone(),
        lang,
        root,
        keep_audio: false,
        dedupe: DedupeMode::Never,
        dedupe_threshold: cfg.dedupe_threshold,
        mode,
    }
}

// ── State ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveState {
    /// The helper is loading the model; the session starts once it is ready.
    Starting,
    Recording,
    /// Stopped; the helper decodes what was still open.
    Finalizing,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveSource {
    /// Dictation: the default microphone.
    #[default]
    Mic,
    /// `--song`: everything this Mac plays, captured digitally — no speaker,
    /// room or microphone in between, and nothing is turned down.
    System,
    /// `--room` (`--song --mic`): a song playing on another device, heard
    /// through the microphone.
    Room,
}

impl LiveSource {
    /// A song (not a dictation): longer pieces, recognised against Shazam.
    pub fn is_song(self) -> bool {
        matches!(self, LiveSource::System | LiveSource::Room)
    }

    /// Records the microphone (a dead microphone ends the session).
    pub fn uses_mic(self) -> bool {
        matches!(self, LiveSource::Mic | LiveSource::Room)
    }

    /// The engine mode and the sources it records.
    pub fn session(self) -> (Mode, Vec<Source>) {
        match self {
            LiveSource::Mic => (Mode::Dictation, vec![Source::Mic]),
            LiveSource::System => (Mode::Song, vec![Source::Sys]),
            LiveSource::Room => (Mode::Room, vec![Source::Mic]),
        }
    }

    fn capture(self) -> Source {
        if self == LiveSource::System { Source::Sys } else { Source::Mic }
    }
}

/// The song recognised in a song session, with its lyrics from the catalogue.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct SongInfo {
    pub title: String,
    pub artist: String,
    pub cover_url: String,
    /// The catalogue lyrics (lrclib); None = the song has none there.
    pub lyrics: Option<String>,
    /// The lyrics as the panel's button copies them ("Title – Artist", blank
    /// line, lyrics).
    pub lyrics_clip: Option<String>,
    /// The catalogue is still being asked.
    pub lyrics_loading: bool,
}

impl SongInfo {
    /// Put the catalogue's answer in. Blank lyrics count as none. Pure.
    pub fn with_lyrics(mut self, lyrics: Option<String>) -> Self {
        let lyrics = lyrics.filter(|l| !l.trim().is_empty());
        self.lyrics_clip = lyrics.as_deref().map(|l| crate::shazam::lyrics_clip_text(&self.title, &self.artist, l));
        self.lyrics = lyrics;
        self.lyrics_loading = false;
        self
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LiveJob {
    pub id: u64,
    pub state: LiveState,
    pub language: Option<String>,
    pub started_ms: u64,
    pub elapsed_ms: u64,
    /// 0..1 input level for the meter.
    pub level: f32,
    /// Text of closed segments — kept.
    pub committed: String,
    /// Text of the segment still being spoken — replaced on every update.
    pub partial: String,
    /// Kept for the panel's wire format; the engine's finals are the result.
    pub final_text: Option<String>,
    /// The model the helper is loading right now.
    pub loading_model: Option<String>,
    pub copied: bool,
    pub error: Option<String>,
    /// Output volume before it was lowered for the recording (percent).
    pub ducked_from: Option<i32>,
    /// Where the audio comes from: the microphone, or what this Mac plays.
    pub source: LiveSource,
    /// The song recognised during a song session (Shazam + lyrics catalogue).
    pub song: Option<SongInfo>,
    /// How many recognition attempts were made (song sessions).
    pub recognize_tries: u32,
    /// The text the session ended with (what was copied) — set at the end, so
    /// the panel shows exactly that without re-deriving it.
    pub result: Option<String>,
    #[serde(skip)]
    last_final_seq: u64,
    #[serde(skip)]
    partial_seq: u64,
}

/// What Whisper writes for silence or noise. Kept short: the segmenter already
/// keeps silence away from the model, this only catches what slips through.
pub fn is_noise_text(text: &str) -> bool {
    let t = text.trim();
    if t.chars().all(|c| !c.is_alphanumeric()) {
        return true;
    }
    let lower = t.to_lowercase();
    [
        "untertitel im auftrag des zdf",
        "untertitel der amara.org-community",
        "untertitelung des zdf",
        "subtitles by the amara.org community",
        "thank you for watching",
        "vielen dank für's zuschauen",
        "vielen dank fürs zuschauen",
    ]
    .iter()
    .any(|p| lower.contains(p))
}

fn join(a: &str, b: &str) -> String {
    match (a.trim().is_empty(), b.trim().is_empty()) {
        (true, _) => b.trim().to_string(),
        (_, true) => a.to_string(),
        _ => format!("{} {}", a.trim_end(), b.trim()),
    }
}

impl LiveJob {
    pub fn new(id: u64, language: Option<String>, now: u64) -> Self {
        LiveJob {
            id,
            state: LiveState::Starting,
            language,
            started_ms: now,
            elapsed_ms: 0,
            level: 0.0,
            committed: String::new(),
            partial: String::new(),
            final_text: None,
            loading_model: None,
            copied: false,
            error: None,
            ducked_from: None,
            source: LiveSource::Mic,
            song: None,
            recognize_tries: 0,
            result: None,
            last_final_seq: 0,
            partial_seq: 0,
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self.state, LiveState::Starting | LiveState::Recording | LiveState::Finalizing)
    }

    /// Fold one helper event in. Pure.
    pub fn apply(&mut self, ev: &Event) {
        match ev {
            Event::Started { .. } => {
                self.loading_model = None;
                if self.state == LiveState::Starting {
                    self.state = LiveState::Recording;
                }
            }
            Event::Hypothesis { seq, stable, pending, .. } => {
                if *seq > self.last_final_seq {
                    let text = join(stable, pending);
                    self.partial = if is_noise_text(&text) { String::new() } else { text };
                    self.partial_seq = *seq;
                }
            }
            Event::Final { seq, text, .. } => {
                if !is_noise_text(text) {
                    self.committed = join(&self.committed, text);
                }
                self.last_final_seq = self.last_final_seq.max(*seq);
                // The partial was a preview of this (or an older) segment.
                if self.partial_seq <= *seq {
                    self.partial.clear();
                }
            }
            Event::Dropped { seq, .. } => {
                self.last_final_seq = self.last_final_seq.max(*seq);
                if self.partial_seq <= *seq {
                    self.partial.clear();
                }
            }
            Event::Level { rms, .. } => self.level = meter_level(*rms),
            _ => {}
        }
    }

    /// What was heard: the kept finals and the segment still open.
    pub fn transcript_text(&self) -> String {
        join(&self.committed, &self.partial)
    }

    /// What ends up in the clipboard when the session stops: what was
    /// heard. A recognised song's catalogue lyrics never replace it — the
    /// panel offers them on a button (`SongInfo::lyrics_clip`).
    pub fn result_text(&self) -> String {
        self.transcript_text()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LiveDone {
    pub id: u64,
    pub ok: bool,
    pub cancelled: bool,
    pub text: String,
    pub error: Option<String>,
}

// ── Song recognition ────────────────────────────────────────────────────────

/// Audio sent to Shazam per attempt, at most.
pub const RECOGNIZE_WINDOW: usize = RATE * 10;
/// The first attempts go out earlier, with what there is.
pub const RECOGNIZE_MIN: usize = RATE * 5;
/// Pause between attempts. Through the microphone one window in three
/// matched in the 2026-10-22 measurement, so a miss is normal — try again.
pub const RECOGNIZE_EVERY_MS: u64 = 5_000;
/// Give up after this many attempts (~2 minutes of trying).
pub const RECOGNIZE_MAX_TRIES: u32 = 24;

/// Seconds of audio to ask the helper for: everything so far, up to the
/// full window. Pure.
pub fn recognize_secs(recorded: usize) -> u32 {
    (recorded.min(RECOGNIZE_WINDOW) / RATE) as u32
}

/// Whether to send the next window to Shazam now. Pure.
pub fn recognize_due(
    source: LiveSource,
    recorded: usize,
    since_last_ms: Option<u64>,
    tries: u32,
    in_flight: bool,
    matched: bool,
) -> bool {
    source.is_song()
        && !matched
        && !in_flight
        && tries < RECOGNIZE_MAX_TRIES
        && recorded >= RECOGNIZE_MIN
        && since_last_ms.is_none_or(|ms| ms >= RECOGNIZE_EVERY_MS)
}

/// A window worth sending: something is playing in it (silence never matches
/// and only spends an attempt).
pub fn window_has_sound(samples: &[i16]) -> bool {
    let loud = samples.chunks(FRAME).filter(|f| frame_rms(f) > MIN_THRESHOLD).count();
    loud * FRAME * 2 >= samples.len()
}

/// 0..1 meter value from a linear RMS (−60 dBFS … 0 dBFS).
pub fn meter_level(rms: f32) -> f32 {
    if rms <= 1e-6 {
        return 0.0;
    }
    ((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}


// ── Session ─────────────────────────────────────────────────────────────────

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static JOB: parking_lot::Mutex<Option<LiveJob>> = parking_lot::Mutex::new(None);
static STOP: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The current (or last) dictation.
pub fn status() -> Option<LiveJob> {
    JOB.lock().clone()
}

/// True while a dictation records or finalizes.
pub fn is_active() -> bool {
    JOB.lock().as_ref().is_some_and(LiveJob::is_active)
}

fn publish(app: &tauri::AppHandle) {
    use tauri::Emitter;
    if let Some(job) = JOB.lock().clone() {
        let _ = app.emit(LIVE_EVENT, job);
    }
}

fn update(f: impl FnOnce(&mut LiveJob)) {
    if let Some(job) = JOB.lock().as_mut() {
        f(job);
    }
}

/// Start dictating. Errors with [`ERR_BUSY`] while one (or a file job, or a
/// meeting) runs, and with a readable sentence when the engine can't start.
pub fn start(app: tauri::AppHandle, language: Option<String>, source: LiveSource) -> Result<LiveJob, String> {
    if !crate::transcribe_client::supported() {
        return Err(crate::transcribe::user_error(crate::transcribe_client::ERR_UNSUPPORTED));
    }
    let lang = crate::transcribe::lang_of(language.as_deref())?;
    let cfg = {
        use tauri::Manager;
        app.try_state::<crate::db::DbHandle>()
            .map(|db| crate::meeting::MeetingConfig::load(&db))
            .unwrap_or_default()
    };
    crate::meeting::model_ready(&cfg.model).map_err(|e| crate::transcribe::user_error(&e))?;
    let job = {
        let mut guard = JOB.lock();
        if guard.as_ref().is_some_and(LiveJob::is_active)
            || crate::transcribe::status().is_some_and(|j| j.is_active())
            || crate::meeting::is_running()
            || crate::transcribe_client::is_busy()
            || crate::record::editor_is_transcribing()
        {
            return Err(ERR_BUSY.into());
        }
        let mut job = LiveJob::new(NEXT_ID.fetch_add(1, Ordering::SeqCst), language.clone(), now_ms());
        job.source = source;
        job.loading_model = Some(cfg.model.clone());
        *guard = Some(job.clone());
        job
    };
    STOP.store(false, Ordering::SeqCst);
    CANCEL.store(false, Ordering::SeqCst);
    publish(&app);
    let id = job.id;
    std::thread::Builder::new()
        .name("ir-transcribe-live".into())
        .spawn(move || run(app, id, lang, source, cfg))
        .map_err(|e| format!("transcribe live: {e}"))?;
    Ok(job)
}

/// Change the language of a running dictation from the next segment on.
pub fn set_lang(language: Option<String>) -> Result<Option<LiveJob>, String> {
    let lang = crate::transcribe::lang_of(language.as_deref())?;
    if is_active() {
        crate::transcribe_client::send(&Command::SetLang { lang })?;
        update(|j| j.language = language.clone().filter(|l| !l.trim().is_empty()));
    }
    Ok(status())
}

/// End the recording; the session then finalizes and copies the text.
pub fn stop() -> Option<LiveJob> {
    if is_active() {
        STOP.store(true, Ordering::SeqCst);
    }
    status()
}

/// Throw the dictation away (nothing is copied).
pub fn cancel() -> bool {
    if !is_active() {
        return false;
    }
    CANCEL.store(true, Ordering::SeqCst);
    true
}

fn private_dir(id: u64) -> Result<PathBuf, String> {
    let dir = dirs::cache_dir()
        .ok_or("no cache folder")?
        .join("InspectorRust")
        .join("transcribe-live")
        .join(id.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| format!("cache: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(dir)
}

// --- Lowering the music while dictating -------------------------------------
//
// Field case 2026-10-20: Spotify played through the built-in speakers at 100 %,
// the microphone heard the music louder than the voice (chunk RMS 0.29) and
// every decode came back empty — Whisper's confident segments were all
// lyrics/instruments, the voice was masked. No filter can recover a voice the
// microphone never separated, so while recording the system output is turned
// down (what dedicated dictation tools do) and put back when recording stops.

/// Settings key; opt-in — the music is lowered only when set to "true" (v0.223.1).
pub const SETTING_DUCK: &str = "transcribe.duck_audio";
/// Output volume while recording (percent).
pub const DUCK_TO: i32 = 10;
/// Read-back tolerance: a virtual output (boom Audio) reads a set volume back
/// a few percent off.
const DUCK_TOLERANCE: i32 = 3;

/// The level to duck to, or `None` when the output is already quiet enough.
pub fn duck_target(current: i32) -> Option<i32> {
    (current > DUCK_TO).then_some(DUCK_TO)
}

/// Restore only if nobody changed the volume meanwhile — a level the user set
/// during the recording is theirs and must not be overwritten.
pub fn should_restore(now: Option<i32>, ducked_to: i32) -> bool {
    match now {
        Some(v) => (v - ducked_to).abs() <= DUCK_TOLERANCE,
        None => true,
    }
}

/// Active ducking: the level to go back to and the level we set.
static DUCKED: parking_lot::Mutex<Option<(i32, i32)>> = parking_lot::Mutex::new(None);

fn duck_setting(app: &tauri::AppHandle) -> Option<String> {
    use tauri::Manager;
    let db = app.try_state::<crate::db::DbHandle>()?;
    crate::settings::get(&db, SETTING_DUCK).ok().flatten()
}

/// Lower the music while dictating? Only when asked for (`transcribe.duck_audio
/// = true`) and only for the microphone — the music is never touched by
/// default, and in song mode it IS what is being transcribed. Pure.
pub fn duck_wanted(setting: Option<&str>, source: LiveSource) -> bool {
    source == LiveSource::Mic && setting.is_some_and(|v| v.trim() == "true")
}

fn duck_output(_app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        use crate::system_commands::ca_volume;
        let Some(current) = ca_volume::read_volume() else { return };
        let Some(target) = duck_target(current) else { return };
        if ca_volume::write_volume(target) {
            *DUCKED.lock() = Some((current, target));
            update(|j| j.ducked_from = Some(current));
            tracing::info!("transcribe live: output lowered {current}% -> {target}%");
        }
    }
}

/// Put the output volume back (idempotent; also called on app exit).
pub fn restore_output() {
    let Some((before, target)) = DUCKED.lock().take() else { return };
    update(|j| j.ducked_from = None);
    #[cfg(target_os = "macos")]
    {
        use crate::system_commands::ca_volume;
        let now = ca_volume::read_volume();
        if should_restore(now, target) {
            ca_volume::write_volume(before);
            tracing::info!("transcribe live: output restored to {before}%");
        } else {
            tracing::info!("transcribe live: volume changed during dictation ({now:?}%) — left as is");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (before, target);
}
/// One Shazam attempt on a window of the recording; on a match the lyrics are
/// fetched and both land in the job (and the Shazam history).
fn spawn_recognize(app: tauri::AppHandle, id: u64, window: Vec<i16>, flag: Arc<AtomicBool>) {
    flag.store(true, Ordering::SeqCst);
    let thread_flag = flag.clone();
    let spawned = std::thread::Builder::new().name("ir-transcribe-song".into()).spawn(move || {
        let flag = thread_flag;
        let result = crate::shazam::recognize(&window);
        match result {
            Ok(Some(m)) => {
                {
                    use tauri::Manager;
                    if let Some(db) = app.try_state::<crate::db::DbHandle>() {
                        let _ = crate::shazam::history_insert(&db, &m);
                    }
                }
                // The song shows at once; its lyrics follow when the catalogue answers.
                let info = SongInfo { title: m.title, artist: m.artist, cover_url: m.cover_url, lyrics_loading: true, ..Default::default() };
                if let Some(job) = JOB.lock().as_mut().filter(|j| j.id == id) {
                    job.song = Some(info.clone());
                }
                publish(&app);
                let lyrics = crate::shazam::fetch_lyrics(&info.artist, &info.title).ok().flatten();
                tracing::info!("transcribe song: recognised (lyrics: {})", lyrics.is_some());
                if let Some(job) = JOB.lock().as_mut().filter(|j| j.id == id) {
                    job.song = Some(info.with_lyrics(lyrics));
                }
                publish(&app);
            }
            Ok(None) => tracing::info!("transcribe song: no match yet"),
            Err(e) => tracing::warn!("transcribe song: recognition failed: {e}"),
        }
        flag.store(false, Ordering::SeqCst);
    });
    if spawned.is_err() {
        flag.store(false, Ordering::SeqCst);
    }
}

/// What the session loop does with one helper event. Pure.
#[derive(Debug, PartialEq)]
pub enum Step {
    Apply,
    /// The helper refused or lost the session.
    Fail(String),
    /// The session is over (after a stop).
    Ended(Option<String>),
    /// A song window to try against Shazam.
    Window(Vec<i16>),
    Ignore,
}

pub fn step(ev: &Event, source: LiveSource, started: bool) -> Step {
    match ev {
        Event::Stopped { error, .. } => Step::Ended(error.clone()),
        Event::Error { error } if !started => Step::Fail(error.clone()),
        Event::SourceState { source: s, state: SourceStatus::Lost, detail } if *s == source.capture() && source.uses_mic() => {
            Step::Fail(detail.clone().unwrap_or_else(|| crate::i18n::t("trx.live_no_mic")))
        }
        Event::Window { source: s, pcm_b64 } if *s == source.capture() => Step::Window(decode_window(pcm_b64)),
        Event::Warning { code, detail } => {
            tracing::warn!("transcribe live: {code}: {detail}");
            Step::Ignore
        }
        Event::Started { .. }
        | Event::Hypothesis { .. }
        | Event::Final { .. }
        | Event::Dropped { .. }
        | Event::Level { .. } => Step::Apply,
        _ => Step::Ignore,
    }
}

fn run(app: tauri::AppHandle, id: u64, lang: transcribe_protocol::Lang, source: LiveSource, cfg: crate::meeting::MeetingConfig) {
    let dir = match private_dir(id) {
        Ok(d) => d,
        Err(e) => return finish(&app, Some(e), None),
    };
    let (tx, rx) = std::sync::mpsc::channel::<Event>();
    let tx = parking_lot::Mutex::new(tx);
    let sub = crate::transcribe_client::subscribe(move |ev| {
        if !matches!(
            ev,
            Event::ModelProgress { .. } | Event::ModelReady { .. } | Event::ModelError { .. } | Event::Devices { .. }
                | Event::FileProgress { .. } | Event::FileDone { .. } | Event::FileWords { .. } | Event::FileError { .. }
        ) {
            let _ = tx.lock().send(ev.clone());
        }
    });
    let outcome = session(&app, id, lang, source, &cfg, &dir, &rx);
    crate::transcribe_client::unsubscribe(sub);
    restore_output();
    if CANCEL.load(Ordering::SeqCst) {
        update(|j| j.state = LiveState::Cancelled);
        return finish(&app, None, Some(&dir));
    }
    finish(&app, outcome.err(), Some(&dir));
}

/// Run one helper session until it is stopped and fully decoded.
fn session(
    app: &tauri::AppHandle,
    id: u64,
    lang: transcribe_protocol::Lang,
    source: LiveSource,
    cfg: &crate::meeting::MeetingConfig,
    dir: &Path,
    rx: &std::sync::mpsc::Receiver<Event>,
) -> Result<(), String> {
    if duck_wanted(duck_setting(app).as_deref(), source) {
        duck_output(app);
    }
    crate::transcribe_client::send(&Command::Start(session_config(source, lang, cfg, dir.to_path_buf())))
        .map_err(|e| crate::transcribe::user_error(&e))?;
    tracing::info!("transcribe live: source {source:?}");

    let mut started: Option<Instant> = None;
    let mut heard = false;
    let recognizing = Arc::new(AtomicBool::new(false));
    let mut window_asked = false;
    let mut last_recognize: Option<Instant> = None;
    let mut stop_sent: Option<Instant> = None;
    let begun = Instant::now();

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(ev) => match step(&ev, source, started.is_some()) {
                Step::Apply => {
                    if matches!(ev, Event::Started { .. }) {
                        started = Some(Instant::now());
                    }
                    if matches!(ev, Event::Level { .. }) {
                        heard = true;
                    }
                    update(|j| j.apply(&ev));
                }
                Step::Fail(e) => {
                    let _ = crate::transcribe_client::send(&Command::Stop);
                    let _ = wait_stopped(rx, Duration::from_secs(10));
                    return Err(crate::transcribe::user_error(&e));
                }
                Step::Ended(error) => return error.map_or(Ok(()), |e| Err(crate::transcribe::user_error(&e))),
                Step::Window(window) => {
                    window_asked = false;
                    if window_has_sound(&window) {
                        last_recognize = Some(Instant::now());
                        update(|j| j.recognize_tries += 1);
                        spawn_recognize(app.clone(), id, window, recognizing.clone());
                    }
                }
                Step::Ignore => {}
            },
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Err(crate::transcribe_client::ERR_HELPER_DIED.into()),
        }
        if let Some(t0) = started {
            let elapsed = t0.elapsed();
            update(|j| j.elapsed_ms = elapsed.as_millis() as u64);
            // Nothing playing yet is fine for a song (press play after
            // starting); a microphone that delivers nothing is broken.
            if stop_sent.is_none() && source.uses_mic() && !heard && elapsed > NO_AUDIO_AFTER {
                let _ = crate::transcribe_client::send(&Command::Stop);
                let _ = wait_stopped(rx, Duration::from_secs(10));
                return Err(crate::i18n::t("trx.live_no_mic"));
            }
            let (tries, matched) = status().map(|j| (j.recognize_tries, j.song.is_some())).unwrap_or((0, true));
            let recorded = elapsed.as_millis() as usize * RATE / 1000;
            if stop_sent.is_none()
                && !window_asked
                && recognize_due(
                    source,
                    recorded,
                    last_recognize.map(|t| t.elapsed().as_millis() as u64),
                    tries,
                    recognizing.load(Ordering::SeqCst),
                    matched,
                )
            {
                window_asked = true;
                last_recognize = Some(Instant::now());
                let secs = recognize_secs(recorded);
                let _ = crate::transcribe_client::send(&Command::Window { source: source.capture(), secs });
            }
        } else if begun.elapsed() > Duration::from_secs(120) && stop_sent.is_none() {
            // The queued Start can still come up (slow model load): stop it, or the
            // helper records with nobody listening and stays busy until the app quits.
            let _ = crate::transcribe_client::send(&Command::Stop);
            let _ = wait_stopped(rx, Duration::from_secs(10));
            return Err(crate::transcribe::user_error("transcribe.timeout"));
        }
        publish(app);
        let want_stop = CANCEL.load(Ordering::SeqCst)
            || STOP.load(Ordering::SeqCst)
            || started.is_some_and(|t| t.elapsed() >= Duration::from_secs(MAX_SECONDS));
        if want_stop && stop_sent.is_none() {
            stop_sent = Some(Instant::now());
            if !CANCEL.load(Ordering::SeqCst) {
                update(|j| j.state = LiveState::Finalizing);
                publish(app);
            }
            let _ = crate::transcribe_client::send(&Command::Stop);
        }
        if stop_sent.is_some_and(|t| t.elapsed() > STOP_WAIT) {
            return Err(crate::transcribe::user_error("transcribe.timeout"));
        }
    }
}

/// Swallow events until the helper reports the session stopped.
fn wait_stopped(rx: &std::sync::mpsc::Receiver<Event>, max: Duration) -> bool {
    let end = Instant::now() + max;
    while Instant::now() < end {
        if let Ok(Event::Stopped { .. }) = rx.recv_timeout(Duration::from_millis(100)) {
            return true;
        }
    }
    false
}

/// End of a session: copy the text (unless cancelled), settle the state,
/// clean up, announce. `error` = the session failed (the text so far is
/// still copied — dictated words are never lost to a crash).
fn finish(app: &tauri::AppHandle, error: Option<String>, dir: Option<&Path>) {
    tracing::info!("transcribe live: finish (error: {error:?})");
    restore_output();
    if let Some(d) = dir {
        let _ = std::fs::remove_dir_all(d);
    }
    let (cancelled, text) = {
        let guard = JOB.lock();
        let job = guard.as_ref();
        (
            job.is_some_and(|j| j.state == LiveState::Cancelled),
            job.map(LiveJob::result_text).unwrap_or_default(),
        )
    };
    let copied = !cancelled
        && !text.trim().is_empty()
        && crate::commands::commit_transformed_text(app.clone(), text.clone(), None, None).is_ok();
    update(|j| {
        j.copied = copied;
        j.level = 0.0;
        j.loading_model = None;
        j.committed = j.transcript_text();
        j.partial.clear();
        j.result = (!cancelled).then(|| text.clone());
        if !cancelled {
            j.state = if error.is_some() { LiveState::Failed } else { LiveState::Done };
            j.error = error.clone();
        }
    });
    publish(app);
    use tauri::Emitter;
    let id = status().map(|j| j.id).unwrap_or(0);
    let _ = app.emit(
        LIVE_DONE_EVENT,
        LiveDone { id, ok: error.is_none() && !cancelled, cancelled, text, error },
    );
}

#[cfg(test)]
mod duck_tests {
    use super::*;

    #[test]
    fn music_is_only_lowered_on_request_and_never_in_song_mode() {
        assert!(!duck_wanted(None, LiveSource::Mic));
        assert!(!duck_wanted(Some("false"), LiveSource::Mic));
        assert!(duck_wanted(Some(" true "), LiveSource::Mic));
        assert!(!duck_wanted(Some("true"), LiveSource::System));
    }

    #[test]
    fn loud_output_is_lowered_quiet_output_left_alone() {
        assert_eq!(duck_target(100), Some(DUCK_TO));
        assert_eq!(duck_target(DUCK_TO + 1), Some(DUCK_TO));
        assert_eq!(duck_target(DUCK_TO), None);
        assert_eq!(duck_target(0), None);
    }

    #[test]
    fn restore_only_when_the_user_did_not_touch_the_volume() {
        assert!(should_restore(Some(DUCK_TO), DUCK_TO));
        assert!(should_restore(Some(DUCK_TO + 2), DUCK_TO), "read-back jitter");
        assert!(!should_restore(Some(60), DUCK_TO), "user turned it up");
        assert!(!should_restore(Some(0), DUCK_TO), "user turned it down");
        assert!(should_restore(None, DUCK_TO));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_that_never_comes_is_stopped_before_giving_up() {
        let src = include_str!("live_transcribe.rs");
        let code = &src[..src.find("#[cfg(test)]\nmod tests").unwrap()];
        let at = code.find("begun.elapsed() > Duration::from_secs(120)").unwrap();
        let arm = &code[at..at + 500];
        let stop = arm.find("Command::Stop").expect("sends Stop");
        assert!(stop < arm.find("transcribe.timeout").unwrap(), "before returning the error");
    }

    fn silence(ms: usize) -> Vec<i16> {
        vec![0; RATE * ms / 1000]
    }

    fn music(ms: usize) -> Vec<i16> {
        (0..RATE * ms / 1000)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let beat = if (t * 2.0).fract() < 0.1 { 1.0 } else { 0.55 };
                (((t * 220.0 * std::f32::consts::TAU).sin() + (t * 277.0 * std::f32::consts::TAU).sin())
                    * 3500.0
                    * beat) as i16
            })
            .collect()
    }

    fn fin(seq: u64, text: &str) -> Event {
        Event::Final { source: Source::Mic, seq, start_ms: 0, end_ms: 0, lang: "de".into(), text: text.into(), confidence: 0.9 }
    }

    fn hyp(seq: u64, stable: &str, pending: &str) -> Event {
        Event::Hypothesis { source: Source::Mic, seq, stable: stable.into(), pending: pending.into() }
    }

    #[test]
    fn sources_map_to_engine_modes() {
        assert!(!LiveSource::Mic.is_song() && LiveSource::Mic.uses_mic());
        assert!(LiveSource::System.is_song() && !LiveSource::System.uses_mic());
        assert!(LiveSource::Room.is_song() && LiveSource::Room.uses_mic());
        assert_eq!(LiveSource::Mic.session(), (Mode::Dictation, vec![Source::Mic]));
        assert_eq!(LiveSource::System.session(), (Mode::Song, vec![Source::Sys]));
        assert_eq!(LiveSource::Room.session(), (Mode::Room, vec![Source::Mic]));
        let cfg = crate::meeting::MeetingConfig::default();
        let c = session_config(LiveSource::Mic, transcribe_protocol::Lang::De, &cfg, "/tmp/x".into());
        assert!(!c.keep_audio, "a dictation never keeps audio");
        assert_eq!(c.dedupe, DedupeMode::Never);
        assert_eq!(c.root, PathBuf::from("/tmp/x"));
        assert_eq!(c.model, cfg.model);
    }

    #[test]
    fn recognition_is_tried_for_songs_only_and_paced() {
        let w = RECOGNIZE_WINDOW;
        assert!(recognize_due(LiveSource::Room, w, None, 0, false, false));
        assert!(recognize_due(LiveSource::System, w, None, 0, false, false));
        // A dictation is never sent to Shazam.
        assert!(!recognize_due(LiveSource::Mic, w * 10, None, 0, false, false));
        // Early attempts go out with what there is, but not with less than the minimum.
        assert!(recognize_due(LiveSource::Room, RECOGNIZE_MIN, None, 0, false, false));
        assert!(!recognize_due(LiveSource::Room, RECOGNIZE_MIN - 1, None, 0, false, false));
        assert_eq!(recognize_secs(RECOGNIZE_MIN), 5);
        assert_eq!(recognize_secs(RATE * 7 + 100), 7);
        assert_eq!(recognize_secs(RATE * 60), 10);
        // Paced, one at a time, stops once matched or out of tries.
        assert!(!recognize_due(LiveSource::Room, w, Some(RECOGNIZE_EVERY_MS - 1), 1, false, false));
        assert!(recognize_due(LiveSource::Room, w, Some(RECOGNIZE_EVERY_MS), 1, false, false));
        assert!(!recognize_due(LiveSource::Room, w, None, 0, true, false));
        assert!(!recognize_due(LiveSource::Room, w, None, 0, false, true));
        assert!(!recognize_due(LiveSource::Room, w, None, RECOGNIZE_MAX_TRIES, false, false));
    }

    #[test]
    fn silence_is_not_worth_an_attempt() {
        assert!(!window_has_sound(&silence(10_000)));
        assert!(window_has_sound(&music(10_000)));
        // Mostly quiet with a little sound: not enough.
        let mut half = silence(8_000);
        half.extend(music(2_000));
        assert!(!window_has_sound(&half));
    }

    #[test]
    fn helper_windows_decode_to_samples() {
        use base64::Engine as _;
        let pcm: Vec<u8> = [1i16, -2, 300].iter().flat_map(|s| s.to_le_bytes()).collect();
        let b64 = base64::engine::general_purpose::STANDARD.encode(pcm);
        assert_eq!(decode_window(&b64), vec![1, -2, 300]);
        assert_eq!(decode_window("not base64!"), Vec::<i16>::new());
    }

    #[test]
    fn stopping_copies_what_was_heard_never_the_catalogue_lyrics() {
        let mut j = LiveJob::new(1, None, 0);
        j.apply(&fin(1, "something heard"));
        assert_eq!(j.result_text(), "something heard");
        let song = SongInfo { title: "T".into(), artist: "A".into(), lyrics_loading: true, ..Default::default() };
        j.song = Some(song.clone().with_lyrics(Some("line one\nline two".into())));
        // The transcript is the result; the lyrics wait on the button.
        assert_eq!(j.result_text(), "something heard");
        let s = j.song.as_ref().unwrap();
        assert_eq!(s.lyrics_clip.as_deref(), Some(crate::shazam::lyrics_clip_text("T", "A", "line one\nline two").as_str()));
        assert!(!s.lyrics_loading);
        // Blank or missing lyrics: nothing to copy, and the search is over.
        for none in [Some("  ".to_string()), None] {
            let s = song.clone().with_lyrics(none);
            assert_eq!((s.lyrics, s.lyrics_clip, s.lyrics_loading), (None, None, false));
        }
    }

    #[test]
    fn finals_append_and_hypotheses_preview_the_open_segment() {
        let mut j = LiveJob::new(1, None, 0);
        j.apply(&Event::Started { session_dir: "/x".into(), sources: vec![Source::Mic] });
        assert_eq!(j.state, LiveState::Recording);
        j.apply(&hyp(1, "Hallo", "Welt"));
        assert_eq!(j.partial, "Hallo Welt");
        j.apply(&fin(1, "Hallo Welt."));
        assert_eq!((j.committed.as_str(), j.partial.as_str()), ("Hallo Welt.", ""));
        j.apply(&hyp(2, "Wie", "geht"));
        j.apply(&fin(2, "Wie geht es?"));
        assert_eq!(j.committed, "Hallo Welt. Wie geht es?");
        assert_eq!(j.result_text(), "Hallo Welt. Wie geht es?");
    }

    #[test]
    fn a_stale_hypothesis_never_shows_after_its_final() {
        let mut j = LiveJob::new(1, None, 0);
        j.apply(&fin(5, "Erster Satz."));
        j.apply(&hyp(4, "Erster", ""));
        assert_eq!(j.partial, "", "a preview older than the kept text is dropped");
        j.apply(&hyp(6, "Zweiter", ""));
        assert_eq!(j.partial, "Zweiter");
        // A dropped segment clears its preview too.
        j.apply(&Event::Dropped { source: Source::Mic, seq: 6, reason: "empty".into() });
        assert_eq!(j.partial, "");
        // Words still open at the stop are not lost.
        j.apply(&hyp(7, "und das", ""));
        assert_eq!(j.result_text(), "Erster Satz. und das");
    }

    #[test]
    fn whisper_silence_phrases_are_dropped() {
        assert!(is_noise_text("Untertitel im Auftrag des ZDF, 2020"));
        assert!(is_noise_text(" ... "));
        assert!(!is_noise_text("Das ZDF berichtet heute."));
        let mut j = LiveJob::new(1, None, 0);
        j.apply(&fin(1, "Untertitel der Amara.org-Community"));
        assert_eq!(j.committed, "");
    }

    #[test]
    fn the_loop_fails_on_refusal_and_a_lost_microphone_only() {
        let refused = Event::Error { error: "model.missing".into() };
        assert_eq!(step(&refused, LiveSource::Mic, false), Step::Fail("model.missing".into()));
        // An unrelated error during a running session does not end it.
        assert_eq!(step(&refused, LiveSource::Mic, true), Step::Ignore);
        let lost = |s| Event::SourceState { source: s, state: SourceStatus::Lost, detail: Some("weg".into()) };
        assert_eq!(step(&lost(Source::Mic), LiveSource::Mic, true), Step::Fail("weg".into()));
        // Song mode records the system audio: a lost microphone is not ours.
        assert_eq!(step(&lost(Source::Mic), LiveSource::System, true), Step::Ignore);
        // The system tap retries on its own; a song keeps going.
        assert_eq!(step(&lost(Source::Sys), LiveSource::System, true), Step::Ignore);
        assert_eq!(
            step(&Event::Stopped { session_dir: None, error: Some("x".into()) }, LiveSource::Mic, true),
            Step::Ended(Some("x".into()))
        );
        assert_eq!(step(&fin(1, "a"), LiveSource::Mic, true), Step::Apply);
    }

    #[test]
    fn meter_level_maps_dbfs() {
        assert_eq!(meter_level(0.0), 0.0);
        assert_eq!(meter_level(1.0), 1.0);
        assert!((meter_level(0.031_622_8) - 0.5).abs() < 0.01);
        let mut j = LiveJob::new(1, None, 0);
        j.apply(&Event::Level { source: Source::Mic, rms: 1.0 });
        assert_eq!(j.level, 1.0);
    }
}
