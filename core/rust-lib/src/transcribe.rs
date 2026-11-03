//! Local audio/video → sibling TXT, selected in Finder or passed explicitly.
//! The files are transcribed one after another by the `ir-transcribe` helper
//! (whisper.cpp on Metal, the model chosen for `meeting`); this module keeps
//! the per-file job the panel shows.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use transcribe_protocol::{Event, Lang};

/// The helper's error when a file job was cancelled.
pub const ERR_FILE_CANCELLED: &str = "transcribe.file_cancelled";
const EXTENSIONS: &[&str] = &[
    "mp4", "mp3", "m4a", "wav", "aac", "flac", "ogg", "opus", "webm", "mov", "mkv", "avi",
];

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub path: Option<String>,
    pub language: Option<String>,
    /// `--start` / `--stop`: live dictation from the microphone instead of a file.
    pub live: Option<LiveAction>,
    /// `--song` (`--lyrics`, `--system`): listen to what this Mac plays
    /// instead of the microphone — lyrics from a song. Implies `--start`.
    pub song: bool,
    /// `--room` (`--raum`, or `--song --mic`): a song playing on ANOTHER
    /// device, heard through the microphone. Implies `--song`.
    pub room: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveAction {
    Start,
    Stop,
}

/// Preserve literal path spaces/backslashes. Only a quote at a token's start
/// opens a quoted token (an apostrophe in `martin's video.mp4` is literal).
pub fn parse_args(arg: &str) -> Result<Args, String> {
    let mut tokens = Vec::new();
    let mut chars = arg.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if ch.is_whitespace() {
            continue;
        }
        let quoted = ch == '"' || ch == '\'';
        let end = if quoted {
            let mut closing = None;
            for (i, c) in chars.by_ref() {
                if c == ch {
                    closing = Some(i + c.len_utf8());
                    break;
                }
            }
            let end = closing.ok_or("Nicht geschlossenes Anführungszeichen im Pfad")?;
            if chars.peek().is_some_and(|(_, c)| !c.is_whitespace()) {
                return Err("Nach einem zitierten Pfad muss ein Leerzeichen stehen".into());
            }
            end
        } else {
            let mut end = start + ch.len_utf8();
            while let Some(&(i, c)) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                chars.next();
                end = i + c.len_utf8();
            }
            end
        };
        tokens.push((start, end, quoted));
    }
    let mut result = Args::default();
    let mut path_tokens = Vec::new();
    let mut language_seen = false;
    let mut flags_done = false;
    let mut mic = false;
    let mut i = 0;
    while i < tokens.len() {
        let (start, end, quoted) = tokens[i];
        let token = &arg[start..end];
        if !quoted && !flags_done && token == "--" {
            flags_done = true;
        } else if !quoted
            && !flags_done
            && (token == "--language" || token == "-l" || token.starts_with("--language="))
        {
            if language_seen {
                return Err(crate::i18n::t("trx.lang_once"));
            }
            language_seen = true;
            let value = if let Some(value) = token.strip_prefix("--language=") {
                value
            } else {
                i += 1;
                let &(s, e, _) = tokens.get(i).ok_or_else(|| crate::i18n::t("trx.lang_missing"))?;
                &arg[s..e]
            };
            let value = value.trim_matches(['"', '\'']).to_lowercase();
            if value.is_empty() || !value.chars().all(|c| c.is_ascii_alphabetic()) {
                return Err(crate::i18n::t("trx.lang_invalid"));
            }
            result.language = (value != "auto").then_some(value);
        } else if !quoted && !flags_done && (token == "--start" || token == "--stop") {
            let action = if token == "--start" { LiveAction::Start } else { LiveAction::Stop };
            if result.live.is_some_and(|a| a != action) {
                return Err(crate::i18n::t("trx.live_both"));
            }
            result.live = Some(action);
        } else if !quoted && !flags_done && matches!(token, "--song" | "--lyrics" | "--system") {
            result.song = true;
        } else if !quoted && !flags_done && matches!(token, "--room" | "--raum") {
            result.song = true;
            result.room = true;
        } else if !quoted && !flags_done && token == "--mic" {
            mic = true;
        } else if !quoted && !flags_done && token.starts_with('-') {
            return Err(crate::i18n::tf("trx.unknown_param", &[("token", token)]));
        } else {
            path_tokens.push(i);
        }
        i += 1;
    }
    if let (Some(&first), Some(&last)) = (path_tokens.first(), path_tokens.last()) {
        if path_tokens.len() != last - first + 1 {
            return Err(crate::i18n::t("trx.lang_pos"));
        }
        if path_tokens.len() > 1 && path_tokens.iter().any(|&i| tokens[i].2) {
            return Err(crate::i18n::t("trx.one_path"));
        }
        let path = arg[tokens[first].0..tokens[last].1].to_string();
        if path.trim_matches(['"', '\'']).is_empty() {
            return Err(crate::i18n::t("trx.empty_path"));
        }
        result.path = Some(path);
    }
    // `--mic` only means something together with `--song`: the microphone is
    // already the source of a plain dictation.
    if result.song && mic {
        result.room = true;
    }
    if result.song {
        if result.live == Some(LiveAction::Stop) {
            return Err(crate::i18n::t("trx.song_with_stop"));
        }
        result.live = Some(LiveAction::Start);
    }
    if result.live.is_some() && result.path.is_some() {
        return Err(crate::i18n::t("trx.live_with_path"));
    }
    if result.live == Some(LiveAction::Stop) && result.language.is_some() {
        return Err(crate::i18n::t("trx.live_stop_lang"));
    }
    Ok(result)
}

fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

pub fn choose_inputs(
    path: Option<PathBuf>,
    selection: Result<Vec<PathBuf>, String>,
    is_file: impl Fn(&Path) -> bool,
) -> Result<Vec<PathBuf>, String> {
    if let Some(path) = path {
        if !supported(&path) {
            return Err(crate::i18n::tf("trx.unsupported", &[("path", &path.display().to_string())]));
        }
        if !is_file(&path) {
            return Err(crate::i18n::tf("trx.not_found", &[("path", &path.display().to_string())]));
        }
        return Ok(vec![path]);
    }
    let files: Vec<_> = selection?
        .into_iter()
        .filter(|p| supported(p) && is_file(p))
        .collect();
    if files.is_empty() {
        return Err(crate::i18n::t("trx.no_selection"));
    }
    Ok(files)
}

/// The languages the engine transcribes: German, English or automatic.
pub fn lang_of(language: Option<&str>) -> Result<Lang, String> {
    match language.map(|l| l.trim().to_ascii_lowercase()).as_deref() {
        None | Some("") | Some("auto") => Ok(Lang::Auto),
        Some("de" | "german" | "deutsch" | "ger" | "deu") => Ok(Lang::De),
        Some("en" | "english" | "englisch" | "eng") => Ok(Lang::En),
        Some(_) => Err(crate::i18n::t("trx.lang_de_en")),
    }
}

// ---------------------------------------------------------------------------
// Background job. The worker prints one JSON event per line (model / start /
// progress / done / failed); the state below folds them into a `Job` that the
// preview panel renders per file. The job lives in the backend, so closing the
// overlay never stops it — reopening `transcribe` reconnects via `status()`.
// ---------------------------------------------------------------------------

/// Tauri event carrying the full `Job` snapshot after every change.
pub const PROGRESS_EVENT: &str = "transcribe-progress";
/// Tauri event fired once when a job ends (`DoneEvent`).
pub const DONE_EVENT: &str = "transcribe-done";
/// Error sentinel: a job is already running (the panel just reconnects).
pub const ERR_BUSY: &str = "transcribe.busy";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileState {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileJob {
    pub path: String,
    pub name: String,
    pub state: FileState,
    /// 0..=1 — frames decoded / frames total, as reported by Whisper.
    pub progress: f64,
    pub output: Option<String>,
    pub language: Option<String>,
    pub error: Option<String>,
    pub started_ms: Option<u64>,
    pub finished_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Worker spawned, Whisper still importing.
    Starting,
    LoadingModel,
    Running,
    Finished,
    Cancelled,
    /// The worker itself failed (Whisper missing, ffmpeg missing, no input…).
    Failed,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Job {
    pub id: u64,
    pub phase: Phase,
    pub language: Option<String>,
    pub files: Vec<FileJob>,
    pub error: Option<String>,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DoneEvent {
    pub id: u64,
    pub ok: bool,
    pub message: String,
}

/// One line of worker output.
#[derive(Debug, serde::Deserialize, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WorkerEvent {
    Model,
    Start {
        index: usize,
    },
    Progress {
        index: usize,
        done: u64,
        total: u64,
    },
    Done {
        index: usize,
        output: String,
        #[serde(default)]
        language: Option<String>,
    },
    Failed {
        index: usize,
        error: String,
    },
}

/// How the worker process ended.
#[derive(Debug, PartialEq)]
pub enum Exit {
    /// Every file was sent and answered.
    Ok,
    /// The user cancelled.
    Cancelled,
}

impl Job {
    pub fn new(id: u64, paths: &[PathBuf], language: Option<String>, now: u64) -> Self {
        Self {
            id,
            phase: Phase::Starting,
            language,
            files: paths
                .iter()
                .map(|p| FileJob {
                    path: p.to_string_lossy().into_owned(),
                    name: p
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| p.to_string_lossy().into_owned()),
                    state: FileState::Queued,
                    progress: 0.0,
                    output: None,
                    language: None,
                    error: None,
                    started_ms: None,
                    finished_ms: None,
                })
                .collect(),
            error: None,
            started_ms: now,
            finished_ms: None,
        }
    }

    /// A job that never got going (no input, ffmpeg missing, …).
    pub fn failed(id: u64, error: String, now: u64) -> Self {
        let mut job = Self::new(id, &[], None, now);
        job.phase = Phase::Failed;
        job.error = Some(error);
        job.finished_ms = Some(now);
        job
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.phase,
            Phase::Starting | Phase::LoadingModel | Phase::Running
        )
    }

    /// Fold one worker event in. Out-of-range indices are ignored (a worker
    /// bug must never panic the backend).
    pub fn apply(&mut self, event: WorkerEvent, now: u64) {
        match event {
            WorkerEvent::Model => {
                if self.phase == Phase::Starting {
                    self.phase = Phase::LoadingModel;
                }
            }
            WorkerEvent::Start { index } => {
                self.phase = Phase::Running;
                if let Some(f) = self.files.get_mut(index) {
                    f.state = FileState::Running;
                    f.progress = 0.0;
                    f.started_ms = Some(now);
                }
            }
            WorkerEvent::Progress { index, done, total } => {
                if let Some(f) = self.files.get_mut(index) {
                    if total > 0 {
                        // Never let a stray event move a bar backwards.
                        f.progress = f.progress.max((done as f64 / total as f64).clamp(0.0, 1.0));
                    }
                }
            }
            WorkerEvent::Done {
                index,
                output,
                language,
            } => {
                if let Some(f) = self.files.get_mut(index) {
                    f.state = FileState::Done;
                    f.progress = 1.0;
                    f.output = Some(output);
                    f.language = language;
                    f.finished_ms = Some(now);
                }
            }
            WorkerEvent::Failed { index, error } => {
                if let Some(f) = self.files.get_mut(index) {
                    f.state = FileState::Failed;
                    f.error = Some(user_error(&error));
                    f.finished_ms = Some(now);
                }
            }
        }
    }

    /// Close the job once the worker process is gone. Files the worker never
    /// reported on are marked so no row stays "running" forever.
    pub fn finish(&mut self, exit: Exit, now: u64) {
        let (phase, leftover, reason) = match exit {
            Exit::Cancelled => (Phase::Cancelled, FileState::Cancelled, None),
            Exit::Ok => (
                Phase::Finished,
                FileState::Failed,
                Some(crate::i18n::t("trx.worker_silent")),
            ),
        };
        for f in &mut self.files {
            if matches!(f.state, FileState::Queued | FileState::Running) {
                f.state = leftover;
                if leftover == FileState::Failed {
                    f.error = reason.clone();
                }
                f.finished_ms = Some(now);
            }
        }
        if phase == Phase::Failed {
            self.error = reason;
        }
        self.phase = phase;
        self.finished_ms = Some(now);
    }

    pub fn summary(&self) -> Summary {
        let mut summary = Summary::default();
        for f in &self.files {
            match f.state {
                FileState::Done => summary
                    .outputs
                    .push(PathBuf::from(f.output.clone().unwrap_or_default())),
                FileState::Failed => summary
                    .failed
                    .push((PathBuf::from(&f.path), f.error.clone().unwrap_or_default())),
                _ => {}
            }
        }
        summary
    }

    /// The one-line result for the toast / notification.
    pub fn message(&self) -> String {
        match self.phase {
            Phase::Failed if self.files.iter().all(|f| f.state != FileState::Done) => self
                .error
                .clone()
                .unwrap_or_else(|| crate::i18n::t("trx.failed")),
            Phase::Cancelled => {
                let done = self
                    .files
                    .iter()
                    .filter(|f| f.state == FileState::Done)
                    .count();
                crate::i18n::tf("trx.cancelled", &[("done", &done.to_string()), ("total", &self.files.len().to_string())])
            }
            _ => self.summary().message(),
        }
    }

    pub fn ok(&self) -> bool {
        self.phase == Phase::Finished && self.files.iter().all(|f| f.state == FileState::Done)
    }
}

/// A helper event for the file at `index`. `None` = not about files (or the
/// cancel answer, which `finish(Cancelled)` handles). Pure.
pub fn file_event(index: usize, ev: &Event) -> Option<WorkerEvent> {
    match ev {
        Event::FileProgress { done_ms, total_ms } => Some(WorkerEvent::Progress { index, done: *done_ms, total: *total_ms }),
        Event::FileDone { txt, lang } => Some(WorkerEvent::Done {
            index,
            output: txt.to_string_lossy().into_owned(),
            language: lang.clone(),
        }),
        Event::FileError { error } if error != ERR_FILE_CANCELLED => Some(WorkerEvent::Failed { index, error: error.clone() }),
        _ => None,
    }
}

/// The helper's answer that ends the file: done, failed or cancelled.
pub fn ends_file(ev: &Event) -> bool {
    matches!(ev, Event::FileDone { .. } | Event::FileError { .. })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static JOB: parking_lot::Mutex<Option<Job>> = parking_lot::Mutex::new(None);
static CANCELLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The current (or last) job — for a panel that opens after the start.
pub fn status() -> Option<Job> {
    JOB.lock().clone()
}

fn publish(app: &tauri::AppHandle, job: &Job) {
    use tauri::Emitter;
    let _ = app.emit(PROGRESS_EVENT, job);
}

fn update(app: &tauri::AppHandle, f: impl FnOnce(&mut Job)) {
    let snapshot = {
        let mut guard = JOB.lock();
        let Some(job) = guard.as_mut() else { return };
        f(job);
        job.clone()
    };
    publish(app, &snapshot);
}

/// Record a job that failed before it started, so the panel shows the reason
/// in the same place as every other outcome. Never replaces a running job.
pub fn record_failure(app: &tauri::AppHandle, error: String) -> Job {
    let mut guard = JOB.lock();
    if let Some(job) = guard.as_ref().filter(|j| j.is_active()) {
        return job.clone();
    }
    let job = Job::failed(
        NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        error,
        now_ms(),
    );
    *guard = Some(job.clone());
    drop(guard);
    publish(app, &job);
    job
}

/// Start a job in the background. Errors with `ERR_BUSY` while one runs.
pub fn start(
    app: tauri::AppHandle,
    paths: Vec<PathBuf>,
    language: Option<String>,
    model: String,
) -> Result<Job, String> {
    let job = {
        let mut guard = JOB.lock();
        if guard.as_ref().is_some_and(Job::is_active) {
            return Err(ERR_BUSY.into());
        }
        let job = Job::new(
            NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
            &paths,
            language.clone(),
            now_ms(),
        );
        *guard = Some(job.clone());
        job
    };
    CANCELLED.store(false, std::sync::atomic::Ordering::SeqCst);
    publish(&app, &job);
    let lang = lang_of(language.as_deref()).unwrap_or(Lang::Auto);
    std::thread::Builder::new()
        .name("ir-transcribe".into())
        .spawn(move || run_job(app, paths, lang, model))
        .map_err(|e| format!("transcribe: {e}"))?;
    Ok(job)
}

/// Stop the running job. Returns false when nothing was running.
pub fn cancel() -> bool {
    if !JOB.lock().as_ref().is_some_and(Job::is_active) {
        return false;
    }
    CANCELLED.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = crate::transcribe_client::send(&transcribe_protocol::Command::CancelFile);
    true
}

/// Longest silence from the helper before a file counts as lost (model
/// load and a long segment on a slow Mac stay far below this).
const FILE_SILENCE: std::time::Duration = std::time::Duration::from_secs(600);

/// Transcribe one file through the helper, folding its events into the job.
/// Returns false when the file can't continue (cancelled).
fn run_file(app: &tauri::AppHandle, index: usize, path: &Path, lang: Lang, model: &str) -> bool {
    use std::sync::atomic::Ordering;
    let now = now_ms();
    update(app, |job| job.apply(WorkerEvent::Start { index }, now));
    let (tx, rx) = std::sync::mpsc::channel::<Event>();
    let tx = parking_lot::Mutex::new(tx);
    let sub = crate::transcribe_client::subscribe(move |ev| {
        if matches!(ev, Event::FileProgress { .. } | Event::FileDone { .. } | Event::FileError { .. }) {
            let _ = tx.lock().send(ev.clone());
        }
    });
    let cmd = transcribe_protocol::Command::TranscribeFile { path: path.to_path_buf(), lang, model: model.to_string() };
    let mut go_on = true;
    if let Err(error) = crate::transcribe_client::send(&cmd) {
        let now = now_ms();
        update(app, |job| job.apply(WorkerEvent::Failed { index, error }, now));
    } else {
        loop {
            match rx.recv_timeout(FILE_SILENCE) {
                Ok(ev) => {
                    if let Some(we) = file_event(index, &ev) {
                        let now = now_ms();
                        update(app, |job| job.apply(we, now));
                    }
                    if ends_file(&ev) {
                        break;
                    }
                }
                Err(_) => {
                    let now = now_ms();
                    let error = crate::transcribe_client::ERR_HELPER_DIED.to_string();
                    update(app, |job| job.apply(WorkerEvent::Failed { index, error }, now));
                    let _ = crate::transcribe_client::send(&transcribe_protocol::Command::CancelFile);
                    break;
                }
            }
        }
    }
    crate::transcribe_client::unsubscribe(sub);
    if CANCELLED.load(Ordering::SeqCst) {
        go_on = false;
    }
    go_on
}

fn run_job(app: tauri::AppHandle, paths: Vec<PathBuf>, lang: Lang, model: String) {
    use std::sync::atomic::Ordering;
    update(&app, |job| job.apply(WorkerEvent::Model, now_ms()));
    for (index, path) in paths.iter().enumerate() {
        if CANCELLED.load(Ordering::SeqCst) || !run_file(&app, index, path, lang, &model) {
            break;
        }
    }
    let exit = if CANCELLED.load(Ordering::SeqCst) { Exit::Cancelled } else { Exit::Ok };
    let now = now_ms();
    update(&app, |job| job.finish(exit, now));
    if let Some(job) = status() {
        for f in job.files.iter().filter(|f| f.state == FileState::Failed) {
            tracing::warn!(
                "transcribe: {}: {}",
                f.path,
                f.error.clone().unwrap_or_default()
            );
        }
        let message = job.message();
        notify_completion(&message);
        use tauri::Emitter;
        let _ = app.emit(
            DONE_EVENT,
            DoneEvent {
                id: job.id,
                ok: job.ok(),
                message,
            },
        );
    }
}

#[derive(Debug, Default)]
pub struct Summary {
    pub outputs: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
}

impl Summary {
    pub fn message(&self) -> String {
        let success = match self.outputs.as_slice() {
            [path] => crate::i18n::tf(
                "trx.created_one",
                &[("name", &path.file_name().unwrap_or_default().to_string_lossy())],
            ),
            outputs => crate::i18n::tf("trx.created_n", &[("n", &outputs.len().to_string())]),
        };
        if let Some((path, reason)) = self.failed.first() {
            crate::i18n::tf(
                "trx.failed_detail",
                &[
                    ("success", &success),
                    ("n", &self.failed.len().to_string()),
                    ("file", &path.file_name().unwrap_or_default().to_string_lossy()),
                    ("reason", &user_error(reason)),
                ],
            )
        } else {
            success
        }
    }
}

pub fn user_error(error: &str) -> String {
    // A sentinel from the engine (`model.missing`, `transcribe.file_decode` …)
    // becomes the sentence the meeting panel shows for it too.
    let e = error.trim();
    if !e.is_empty() && !e.contains(char::is_whitespace) && e.contains('.') {
        let key = format!("meeting.err.{}", e.replace('.', "_"));
        let s = crate::i18n::t(&key);
        if s != key {
            return s;
        }
    }
    match error
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
    {
        Some(line) => line.chars().take(220).collect(),
        None => crate::i18n::t("trx.failed"),
    }
}

pub fn notify_completion(message: &str) {
    #[cfg(target_os = "macos")]
    {
        // Message is argv data, including quotes/newlines in filenames.
        let _ = Command::new("/usr/bin/osascript")
            .args(["-e", "on run argv\ndisplay notification (item 1 of argv) with title \"Inspector Rust\" subtitle \"Audio/Video → Text\"\nend run", message])
            .stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = message;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_and_language_only() {
        assert_eq!(parse_args("").unwrap(), Args::default());
        assert_eq!(
            parse_args("--language DE").unwrap(),
            Args {
                language: Some("de".into()),
                path: None,
                live: None,
                song: false,
                room: false,
            }
        );
        assert_eq!(parse_args("-l auto").unwrap(), Args::default());
        assert_eq!(
            parse_args("--language=english")
                .unwrap()
                .language
                .as_deref(),
            Some("english")
        );
    }

    #[test]
    fn preserves_spaces_unicode_and_windows_paths() {
        for path in [
            "~/Videos/Grüße  heute.mp4",
            r"C:\My Videos\clip.MP4",
            "/tmp/martin's video.mp4",
        ] {
            let args = parse_args(&format!("{path} --language de")).unwrap();
            assert_eq!(args.path.as_deref(), Some(path));
            assert_eq!(args.language.as_deref(), Some("de"));
        }
        let args = parse_args("-l en \"~/video --language de.mp4\"").unwrap();
        assert_eq!(args.path.as_deref(), Some("\"~/video --language de.mp4\""));
        assert_eq!(args.language.as_deref(), Some("en"));
        assert_eq!(
            parse_args("-- -clip.mp4").unwrap().path.as_deref(),
            Some("-clip.mp4")
        );
    }

    #[test]
    fn live_flags_start_and_stop_dictation() {
        let a = parse_args("--start").unwrap();
        assert_eq!((a.live, a.path, a.language), (Some(LiveAction::Start), None, None));
        let a = parse_args("--start --language de").unwrap();
        assert_eq!((a.live, a.language.as_deref()), (Some(LiveAction::Start), Some("de")));
        let a = parse_args("-l en --start").unwrap();
        assert_eq!((a.live, a.language.as_deref()), (Some(LiveAction::Start), Some("en")));
        assert_eq!(parse_args("--stop").unwrap().live, Some(LiveAction::Stop));
        // Repeating the same flag is harmless; the two together are not.
        assert_eq!(parse_args("--start --start").unwrap().live, Some(LiveAction::Start));
        assert!(parse_args("--start --stop").is_err());
        // A file and the microphone at once makes no sense.
        assert!(parse_args("--start ~/a.mp3").is_err());
        assert!(parse_args("--stop --language de").is_err());
        let a = parse_args("--start --song").unwrap();
        assert_eq!((a.live, a.song), (Some(LiveAction::Start), true));
        let a = parse_args("--lyrics -l en").unwrap();
        assert_eq!((a.live, a.song, a.language.as_deref()), (Some(LiveAction::Start), true, Some("en")));
        assert!(parse_args("--system").unwrap().song);
        assert!(!parse_args("--start").unwrap().song);
        assert!(parse_args("--stop --song").is_err());
        assert!(parse_args("--song ~/a.mp3").is_err());
        // Another device through the microphone.
        for arg in ["--room", "--raum", "--song --mic", "--mic --song", "--start --room -l de"] {
            let a = parse_args(arg).unwrap();
            assert_eq!((a.live, a.song, a.room), (Some(LiveAction::Start), true, true), "{arg}");
        }
        assert!(!parse_args("--song").unwrap().room);
        let plain = parse_args("--start --mic").unwrap();
        assert_eq!((plain.song, plain.room), (false, false));
        assert!(parse_args("--stop --room").is_err());
        // A file run is unchanged.
        assert_eq!(parse_args("a.mp3").unwrap().live, None);
    }

    #[test]
    fn rejects_malformed_flags_and_multiple_paths() {
        for arg in [
            "--language",
            "--language=",
            "-l --bad",
            "--foo de",
            "-l de --language en",
            "-l auto -l de",
            "\"unfinished",
            "\"\"",
            "\"a.mp4\" \"b.mp4\"",
            "my -l de video.mp4",
        ] {
            assert!(parse_args(arg).is_err(), "{arg}");
        }
    }

    #[test]
    fn explicit_input_wins_and_errors_do_not_fall_back() {
        let video = PathBuf::from("/v.MP4");
        assert_eq!(
            choose_inputs(Some(video.clone()), Err("denied".into()), |_| true).unwrap(),
            vec![video]
        );
        assert!(choose_inputs(
            Some("/missing.mp4".into()),
            Ok(vec!["/other.mp4".into()]),
            |_| false
        )
        .unwrap_err()
        .contains("not found"));
        assert!(choose_inputs(Some("/notes.md".into()), Ok(vec![]), |_| true).is_err());
    }

    #[test]
    fn selection_filters_media_and_propagates_permission_errors() {
        let selection = ["a.mp4", "b.mp3", "c.MOV", "d.txt", "gone.wav"];
        assert_eq!(
            choose_inputs(
                None,
                Ok(selection.iter().map(PathBuf::from).collect()),
                |p| p != Path::new("gone.wav")
            )
            .unwrap(),
            vec![PathBuf::from("a.mp4"), "b.mp3".into(), "c.MOV".into()]
        );
        assert_eq!(
            choose_inputs(None, Err("denied".into()), |_| true).unwrap_err(),
            "denied"
        );
        assert!(choose_inputs(None, Ok(vec![]), |_| true)
            .unwrap_err()
            .contains("transcribe <path>"));
    }

    #[test]
    fn summary_names_outputs_and_partial_failures() {
        let mut summary = Summary {
            outputs: vec!["/clip 2.txt".into()],
            failed: vec![],
        };
        assert_eq!(summary.message(), "clip 2.txt created");
        summary
            .failed
            .push(("/broken.mp4".into(), "Keine Tonspur".into()));
        assert!(summary
            .message()
            .contains("1 failed: broken.mp4 — Keine Tonspur"));
    }

    fn batch() -> Job {
        Job::new(
            7,
            &[PathBuf::from("/a/eins.mp4"), PathBuf::from("/a/zwei.mp3")],
            None,
            1000,
        )
    }

    #[test]
    fn file_job_maps_progress_and_done_events() {
        let mut job = batch();
        job.apply(WorkerEvent::Start { index: 1 }, 10);
        for ev in [
            Event::FileProgress { done_ms: 30_000, total_ms: 120_000 },
            Event::Level { source: transcribe_protocol::Source::Mic, rms: 0.1 },
        ] {
            if let Some(we) = file_event(1, &ev) {
                job.apply(we, 20);
            }
        }
        assert_eq!(job.files[1].progress, 0.25);
        assert_eq!(job.files[0].state, FileState::Queued, "events land on their own file");
        let done = Event::FileDone { txt: "/a/zwei.txt".into(), lang: Some("de".into()) };
        assert!(ends_file(&done));
        job.apply(file_event(1, &done).unwrap(), 30);
        assert_eq!(job.files[1].state, FileState::Done);
        assert_eq!(job.files[1].output.as_deref(), Some("/a/zwei.txt"));
        assert_eq!(job.files[1].language.as_deref(), Some("de"));

        let failed = Event::FileError { error: "transcribe.file_decode".into() };
        job.apply(file_event(0, &failed).unwrap(), 40);
        assert_eq!(job.files[0].state, FileState::Failed);
        assert_ne!(job.files[0].error.as_deref(), Some("transcribe.file_decode"), "a sentinel becomes a sentence");

        // The answer to a cancel ends the file but is no failure.
        let cancelled = Event::FileError { error: ERR_FILE_CANCELLED.into() };
        assert!(ends_file(&cancelled));
        assert_eq!(file_event(0, &cancelled), None);
    }

    #[test]
    fn languages_are_german_english_or_auto() {
        assert_eq!(lang_of(None).unwrap(), Lang::Auto);
        assert_eq!(lang_of(Some("auto")).unwrap(), Lang::Auto);
        assert_eq!(lang_of(Some("DE")).unwrap(), Lang::De);
        assert_eq!(lang_of(Some("english")).unwrap(), Lang::En);
        assert!(lang_of(Some("fr")).is_err());
    }

    #[test]
    fn a_batch_reports_every_file_on_its_own() {
        let mut job = batch();
        assert_eq!(job.phase, Phase::Starting);
        assert!(job.is_active());
        job.apply(WorkerEvent::Model, 1100);
        assert_eq!(job.phase, Phase::LoadingModel);
        job.apply(WorkerEvent::Start { index: 0 }, 1200);
        assert_eq!(job.phase, Phase::Running);
        assert_eq!(job.files[0].state, FileState::Running);
        assert_eq!(job.files[1].state, FileState::Queued);
        job.apply(
            WorkerEvent::Progress {
                index: 0,
                done: 30,
                total: 120,
            },
            1300,
        );
        assert!((job.files[0].progress - 0.25).abs() < 1e-9);
        assert_eq!(job.files[1].progress, 0.0);
        job.apply(
            WorkerEvent::Done {
                index: 0,
                output: "/a/eins.txt".into(),
                language: Some("de".into()),
            },
            1400,
        );
        job.apply(WorkerEvent::Start { index: 1 }, 1400);
        job.apply(
            WorkerEvent::Failed {
                index: 1,
                error: "trace\nKeine Tonspur".into(),
            },
            1500,
        );
        job.finish(Exit::Ok, 1500);
        assert_eq!(job.phase, Phase::Finished);
        assert!(!job.is_active());
        assert_eq!(job.files[0].state, FileState::Done);
        assert_eq!(job.files[0].progress, 1.0);
        assert_eq!(job.files[0].language.as_deref(), Some("de"));
        assert_eq!(job.files[1].error.as_deref(), Some("Keine Tonspur"));
        assert!(!job.ok());
        assert!(job
            .message()
            .contains("1 failed: zwei.mp3 — Keine Tonspur"));
    }

    #[test]
    fn progress_never_runs_backwards_and_bad_indices_are_ignored() {
        let mut job = batch();
        job.apply(
            WorkerEvent::Progress {
                index: 0,
                done: 80,
                total: 100,
            },
            0,
        );
        job.apply(
            WorkerEvent::Progress {
                index: 0,
                done: 10,
                total: 100,
            },
            0,
        );
        assert!((job.files[0].progress - 0.8).abs() < 1e-9);
        job.apply(
            WorkerEvent::Progress {
                index: 0,
                done: 500,
                total: 100,
            },
            0,
        );
        assert_eq!(job.files[0].progress, 1.0);
        job.apply(
            WorkerEvent::Progress {
                index: 1,
                done: 5,
                total: 0,
            },
            0,
        );
        assert_eq!(job.files[1].progress, 0.0);
        job.apply(WorkerEvent::Start { index: 9 }, 0);
        job.apply(
            WorkerEvent::Failed {
                index: 9,
                error: "x".into(),
            },
            0,
        );
        assert!(job.files.iter().all(|f| f.state == FileState::Queued));
    }

    #[test]
    fn cancel_keeps_finished_files_and_stops_the_rest() {
        let mut job = batch();
        job.apply(WorkerEvent::Start { index: 0 }, 0);
        job.apply(
            WorkerEvent::Done {
                index: 0,
                output: "/a/eins.txt".into(),
                language: None,
            },
            0,
        );
        job.apply(WorkerEvent::Start { index: 1 }, 0);
        job.finish(Exit::Cancelled, 9);
        assert_eq!(job.phase, Phase::Cancelled);
        assert_eq!(job.files[0].state, FileState::Done);
        assert_eq!(job.files[1].state, FileState::Cancelled);
        assert_eq!(job.files[1].error, None);
        assert_eq!(job.message(), "Cancelled — 1 of 2 done");
    }

    #[test]
    fn a_skipped_file_never_leaves_a_row_running() {
        // A clean end that skipped a file still closes that row.
        let mut job = batch();
        job.apply(WorkerEvent::Start { index: 0 }, 0);
        job.apply(
            WorkerEvent::Done {
                index: 0,
                output: "/a/eins.txt".into(),
                language: None,
            },
            0,
        );
        job.finish(Exit::Ok, 1);
        assert_eq!(job.files[1].state, FileState::Failed);
        assert_eq!(job.phase, Phase::Finished);
    }

    #[test]
    fn all_done_is_ok_and_input_errors_become_a_failed_job() {
        let mut job = batch();
        for i in 0..2 {
            job.apply(WorkerEvent::Start { index: i }, 0);
            job.apply(
                WorkerEvent::Done {
                    index: i,
                    output: format!("/a/{i}.txt"),
                    language: None,
                },
                0,
            );
        }
        job.finish(Exit::Ok, 1);
        assert!(job.ok());
        assert_eq!(job.message(), "2 transcripts created");
        let failed = Job::failed(3, "Keine Audio-/Videodatei im Finder ausgewählt".into(), 4);
        assert_eq!(failed.phase, Phase::Failed);
        assert!(failed.files.is_empty());
        assert!(!failed.is_active());
        assert_eq!(
            failed.message(),
            "Keine Audio-/Videodatei im Finder ausgewählt"
        );
    }

    #[test]
    fn hud_errors_keep_last_line_and_cap_unicode_safely() {
        assert_eq!(
            user_error("decoder details\nNo audio track\n"),
            "No audio track"
        );
        assert_eq!(user_error(&"ü".repeat(300)).chars().count(), 220);
        assert_eq!(user_error("\n"), "Transcription failed");
    }
}
