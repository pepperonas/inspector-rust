//! Live dictation: `transcribe --start` records the microphone and shows the
//! text while you speak; `transcribe --stop` (or Enter / the stop button) ends
//! it, a more accurate pass runs over the whole recording, and the result goes
//! to the clipboard + history.
//!
//! Pieces:
//! * capture — the shared native cpal path (`mic_capture::start_chunks`), each
//!   chunk resampled to 16 kHz mono `i16` (`shazam::resample_to_16k_i16`) and
//!   appended to one buffer that holds the whole recording;
//! * [`Segmenter`] (pure) — cuts that stream at pauses into segments;
//! * a Python worker (`assets/transcribe/live.py`, same venv as the file mode)
//!   that decodes a closed segment as `final` (its text is kept) and the open
//!   one as `partial` (shown greyed, replaced on the next update);
//! * [`LiveJob`] (pure `apply`) — the state the preview panel renders.
//!
//! The session lives in the backend, so closing the popup never stops it; the
//! footer shows a REC light and typing `transcribe` reconnects via `status()`.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const WORKER: &str = include_str!("../assets/transcribe/live.py");

/// Tauri event with the full [`LiveJob`] snapshot after every change.
pub const LIVE_EVENT: &str = "transcribe-live";
/// Tauri event fired once when a session ends ([`LiveDone`]).
pub const LIVE_DONE_EVENT: &str = "transcribe-live-done";
/// Error sentinel: a dictation (or a file job) is already running.
pub const ERR_BUSY: &str = "transcribe.busy";

/// Sample rate of everything below.
pub const RATE: usize = 16_000;
/// Hard stop after an hour (the whole recording stays in memory: ~115 MB).
pub const MAX_SECONDS: u64 = 3600;

// ── Segmenter ───────────────────────────────────────────────────────────────

/// 20 ms analysis frames.
pub const FRAME: usize = RATE / 50;
/// Audio kept before the first loud frame — word onsets are quiet.
pub const PRE_ROLL: usize = RATE / 5;
/// Audio kept after the last loud frame — word endings are quiet too.
pub const TAIL: usize = RATE / 5;
/// A pause this long closes a segment once enough speech is in it …
pub const PAUSE_SHORT: usize = RATE * 6 / 10;
/// … and this much speech is "enough".
pub const SPEECH_FOR_SHORT_PAUSE: usize = RATE * 3 / 2;
/// A pause this long always ends the segment.
pub const PAUSE_LONG: usize = RATE * 3 / 2;
/// Less speech than this is a cough or a click, not words — dropped.
pub const MIN_SPEECH: usize = RATE * 3 / 10;
/// Whisper's window is 30 s; cut before it.
pub const MAX_SEGMENT: usize = RATE * 25;
/// Speech = frame RMS above the larger of this and FLOOR_FACTOR × the noise floor.
pub const MIN_THRESHOLD: f32 = 0.01;
pub const FLOOR_FACTOR: f32 = 3.0;

/// A closed segment: absolute sample indices `[start, end)` of the recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seg {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy)]
struct Open {
    start: usize,
    speech: usize,
    silence: usize,
    last_speech_end: usize,
}

/// Cuts a sample stream at pauses. Pure: feed samples, get closed segments.
#[derive(Debug)]
pub struct Segmenter {
    /// Absolute index of the first sample in `pending`.
    pos: usize,
    pending: Vec<i16>,
    open: Option<Open>,
    last_end: usize,
    floor: f32,
}

impl Default for Segmenter {
    fn default() -> Self {
        Self::new()
    }
}

impl Segmenter {
    pub fn new() -> Self {
        Segmenter { pos: 0, pending: Vec::new(), open: None, last_end: 0, floor: 0.002 }
    }

    /// Feed the next samples; returns the segments that closed.
    pub fn push(&mut self, samples: &[i16]) -> Vec<Seg> {
        self.pending.extend_from_slice(samples);
        let mut out = Vec::new();
        let mut used = 0;
        while self.pending.len() - used >= FRAME {
            let start = self.pos + used;
            let rms = frame_rms(&self.pending[used..used + FRAME]);
            if let Some(seg) = self.frame(rms, start) {
                out.push(seg);
            }
            used += FRAME;
        }
        self.pending.drain(..used);
        self.pos += used;
        out
    }

    /// The segment still being spoken: `(start, speech samples so far)`.
    pub fn open(&self) -> Option<(usize, usize)> {
        self.open.map(|o| (o.start, o.speech))
    }

    /// End of input: close the open segment if it holds real speech.
    pub fn flush(&mut self) -> Option<Seg> {
        let o = self.open.take()?;
        let end = (o.last_speech_end + TAIL).min(self.pos + self.pending.len());
        (o.speech >= MIN_SPEECH).then(|| self.close(o.start, end))
    }

    fn close(&mut self, start: usize, end: usize) -> Seg {
        self.last_end = end;
        Seg { start, end }
    }

    fn frame(&mut self, rms: f32, start: usize) -> Option<Seg> {
        let end = start + FRAME;
        let speech = rms > MIN_THRESHOLD.max(self.floor * FLOOR_FACTOR);
        // Noise floor = a slow-rising, fast-falling minimum. Speech dips between
        // syllables pull it down at once; a steady sound (a fan, traffic) lifts
        // it within ~1 s, so the room never reads as one endless sentence.
        if rms < self.floor {
            self.floor += (rms - self.floor) * 0.1;
        } else {
            self.floor += (rms - self.floor) * 0.01;
        }
        let Some(mut o) = self.open else {
            if speech {
                self.open = Some(Open {
                    start: start.saturating_sub(PRE_ROLL).max(self.last_end),
                    speech: FRAME,
                    silence: 0,
                    last_speech_end: end,
                });
            }
            return None;
        };
        if speech {
            o.speech += FRAME;
            o.silence = 0;
            o.last_speech_end = end;
        } else {
            o.silence += FRAME;
        }
        let tail_end = (o.last_speech_end + TAIL).min(end);
        if end - o.start >= MAX_SEGMENT {
            self.open = None;
            return Some(self.close(o.start, end));
        }
        if o.silence >= PAUSE_SHORT && o.speech >= SPEECH_FOR_SHORT_PAUSE {
            self.open = None;
            return Some(self.close(o.start, tail_end));
        }
        if o.silence >= PAUSE_LONG {
            self.open = None;
            return (o.speech >= MIN_SPEECH).then(|| self.close(o.start, tail_end));
        }
        self.open = Some(o);
        None
    }
}

fn frame_rms(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return 0.0;
    }
    let sum: f64 = frame.iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum();
    (sum / frame.len() as f64).sqrt() as f32
}

// ── State ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveState {
    /// Worker loading the model; the microphone is already recording.
    Starting,
    Recording,
    /// Stopped; the accurate model runs over the whole recording.
    Finalizing,
    Done,
    Failed,
    Cancelled,
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
    /// The accurate pass over the whole recording, once it is done.
    pub final_text: Option<String>,
    /// The model the worker is loading right now (first use downloads it).
    pub loading_model: Option<String>,
    pub copied: bool,
    pub error: Option<String>,
    #[serde(skip)]
    last_final_seq: u64,
    #[serde(skip)]
    partial_seq: u64,
}

/// One line from the worker.
#[derive(Debug, serde::Deserialize, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LiveEvent {
    Model { name: String },
    Ready,
    Text { seq: u64, kind: String, text: String },
    Finalizing { name: String },
    Final { text: String },
    Error {
        #[serde(default)]
        seq: Option<u64>,
        error: String,
    },
}

pub fn parse_event(line: &str) -> Option<LiveEvent> {
    serde_json::from_str(line.trim()).ok()
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
            last_final_seq: 0,
            partial_seq: 0,
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self.state, LiveState::Starting | LiveState::Recording | LiveState::Finalizing)
    }

    pub fn apply(&mut self, event: LiveEvent) {
        match event {
            LiveEvent::Model { name } | LiveEvent::Finalizing { name } => {
                self.loading_model = Some(name)
            }
            LiveEvent::Ready => {
                self.loading_model = None;
                if self.state == LiveState::Starting {
                    self.state = LiveState::Recording;
                }
            }
            LiveEvent::Text { seq, kind, text } => {
                let text = if is_noise_text(&text) { String::new() } else { text };
                if kind == "final" {
                    self.committed = join(&self.committed, &text);
                    self.last_final_seq = self.last_final_seq.max(seq);
                    // The partial was a preview of this (or an older) segment.
                    if self.partial_seq <= seq {
                        self.partial.clear();
                    }
                } else if seq > self.last_final_seq {
                    self.partial = text;
                    self.partial_seq = seq;
                }
            }
            LiveEvent::Final { text } => {
                self.loading_model = None;
                if !is_noise_text(&text) {
                    self.final_text = Some(text.trim().to_string());
                }
            }
            LiveEvent::Error { error, .. } => {
                tracing::warn!("transcribe live worker: {error}");
            }
        }
    }

    /// What ends up in the clipboard: the accurate pass, else the live text.
    /// A failed accurate pass never loses the words already shown.
    pub fn result_text(&self) -> String {
        match &self.final_text {
            Some(t) if !t.trim().is_empty() => t.clone(),
            _ => join(&self.committed, &self.partial),
        }
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

/// 0..1 meter value from a linear RMS (−60 dBFS … 0 dBFS).
pub fn meter_level(rms: f32) -> f32 {
    if rms <= 1e-6 {
        return 0.0;
    }
    ((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// Prompt for the next decode: the end of what was already said, so names and
/// spelling stay consistent across segments.
pub fn prompt_tail(committed: &str, max_chars: usize) -> String {
    let n = committed.chars().count();
    committed.chars().skip(n.saturating_sub(max_chars)).collect()
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

/// Start dictating. Errors with [`ERR_BUSY`] while one (or a file job) runs.
pub fn start(app: tauri::AppHandle, language: Option<String>) -> Result<LiveJob, String> {
    let job = {
        let mut guard = JOB.lock();
        if guard.as_ref().is_some_and(LiveJob::is_active)
            || crate::transcribe::status().is_some_and(|j| j.is_active())
        {
            return Err(ERR_BUSY.into());
        }
        let job = LiveJob::new(NEXT_ID.fetch_add(1, Ordering::SeqCst), language.clone(), now_ms());
        *guard = Some(job.clone());
        job
    };
    STOP.store(false, Ordering::SeqCst);
    CANCEL.store(false, Ordering::SeqCst);
    publish(&app);
    let id = job.id;
    std::thread::Builder::new()
        .name("ir-transcribe-live".into())
        .spawn(move || run(app, id, language))
        .map_err(|e| format!("transcribe live: {e}"))?;
    Ok(job)
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

fn write_pcm(path: &Path, samples: &[i16]) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, bytes).map_err(|e| format!("pcm: {e}"))
}

struct Worker {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    lines: std::sync::mpsc::Receiver<LiveEvent>,
    stderr: std::thread::JoinHandle<String>,
}

impl Worker {
    fn spawn(language: Option<&str>) -> Result<Worker, String> {
        let mut cmd = Command::new(crate::transcribe::python());
        cmd.args(["-c", WORKER])
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUNBUFFERED", "1")
            .env("IR_TRANSCRIBE_LANGUAGE", language.unwrap_or(""))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn().map_err(|e| {
            format!(
                "Python/Whisper nicht verfügbar: {e}. Einmal python3 scripts/setup-transcribe.py ausführen; alternativ IR_WHISPER_PYTHON setzen."
            )
        })?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or("worker stdout")?;
        let stderr = child.stderr.take();
        let (tx, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some(ev) = parse_event(&line) {
                    if tx.send(ev).is_err() {
                        break;
                    }
                }
            }
        });
        let stderr = std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut s) = stderr {
                let _ = s.read_to_string(&mut text);
            }
            text
        });
        Ok(Worker { child, stdin, lines, stderr })
    }

    fn send(&mut self, req: serde_json::Value) -> bool {
        match self.stdin.as_mut() {
            Some(w) => writeln!(w, "{req}").and_then(|_| w.flush()).is_ok(),
            None => false,
        }
    }

    fn crashed(mut self) -> String {
        drop(self.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
        crate::transcribe::user_error(&self.stderr.join().unwrap_or_default())
    }

    fn close(mut self) {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Pump every waiting worker line into the job. `Err` = the worker is gone.
fn drain(worker: &Worker, busy: &mut bool) -> Result<(), ()> {
    loop {
        match worker.lines.try_recv() {
            Ok(ev) => {
                // Every reply to a request frees the worker; the model notices don't.
                if !matches!(ev, LiveEvent::Model { .. } | LiveEvent::Ready | LiveEvent::Finalizing { .. }) {
                    *busy = false;
                }
                update(|j| j.apply(ev));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(()),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => return Err(()),
        }
    }
}

fn run(app: tauri::AppHandle, id: u64, language: Option<String>) {
    let dir = match private_dir(id) {
        Ok(d) => d,
        Err(e) => return finish(&app, Some(e), None),
    };
    let mut worker = match Worker::spawn(language.as_deref()) {
        Ok(w) => w,
        Err(e) => return finish(&app, Some(e), Some(&dir)),
    };

    let buffer: Arc<parking_lot::Mutex<Vec<i16>>> = Arc::default();
    let level = Arc::new(AtomicU32::new(0));
    let mic = {
        let buffer = buffer.clone();
        let level = level.clone();
        crate::mic_capture::start_chunks(app.clone(), move |chunk, rate| {
            let pcm = crate::shazam::resample_to_16k_i16(chunk, rate);
            level.store(crate::mic_capture::chunk_rms(chunk).to_bits(), Ordering::Relaxed);
            buffer.lock().extend_from_slice(&pcm);
        })
    };

    let started = Instant::now();
    let mut seg = Segmenter::new();
    let mut processed = 0usize;
    let mut finals: VecDeque<Seg> = VecDeque::new();
    let mut busy = false;
    let mut ready = false;
    let mut seq = 0u64;
    let mut last_partial = Instant::now();
    let mut last_partial_len = 0usize;
    let mut crashed: Option<String> = None;

    let send_decode = |worker: &mut Worker, seq: &mut u64, samples: &[i16], kind: &str| -> bool {
        *seq += 1;
        let path = dir.join(format!("{}-{kind}.pcm", *seq));
        if write_pcm(&path, samples).is_err() {
            return false;
        }
        let prompt = status().map(|j| prompt_tail(&j.committed, 200)).unwrap_or_default();
        worker.send(serde_json::json!({
            "op": "decode", "seq": *seq, "kind": kind,
            "path": path.to_string_lossy(), "prompt": prompt,
        }))
    };

    // ── Recording ──
    loop {
        std::thread::sleep(Duration::from_millis(100));
        if drain(&worker, &mut busy).is_err() {
            crashed = Some(String::new());
            break;
        }
        if !ready && status().is_some_and(|j| j.state == LiveState::Recording) {
            ready = true;
        }
        let total = {
            let b = buffer.lock();
            let new = b[processed..].to_vec();
            processed = b.len();
            drop(b);
            finals.extend(seg.push(&new));
            processed
        };
        let elapsed = started.elapsed();
        update(|j| {
            j.elapsed_ms = elapsed.as_millis() as u64;
            j.level = meter_level(f32::from_bits(level.load(Ordering::Relaxed)));
        });
        if total == 0 && elapsed > Duration::from_secs(5) {
            crashed = Some(crate::i18n::t("trx.live_no_mic"));
            break;
        }
        if ready && !busy {
            if let Some(s) = finals.pop_front() {
                let samples = buffer.lock()[s.start..s.end].to_vec();
                busy = send_decode(&mut worker, &mut seq, &samples, "final");
            } else if let Some((start, speech)) = seg.open() {
                if speech >= RATE / 2
                    && last_partial.elapsed() >= Duration::from_millis(1500)
                    && total > last_partial_len
                {
                    let samples = buffer.lock()[start..total].to_vec();
                    busy = send_decode(&mut worker, &mut seq, &samples, "partial");
                    last_partial = Instant::now();
                    last_partial_len = total;
                }
            }
        }
        publish(&app);
        if CANCEL.load(Ordering::SeqCst)
            || STOP.load(Ordering::SeqCst)
            || elapsed >= Duration::from_secs(MAX_SECONDS)
        {
            break;
        }
    }
    mic.stop();
    drop(mic);

    if CANCEL.load(Ordering::SeqCst) {
        worker.close();
        update(|j| j.state = LiveState::Cancelled);
        return finish(&app, None, Some(&dir));
    }
    if let Some(reason) = crashed {
        let error = if reason.is_empty() { worker.crashed() } else {
            worker.close();
            reason
        };
        return finish(&app, Some(error), Some(&dir));
    }

    // ── Finalizing ──
    update(|j| j.state = LiveState::Finalizing);
    publish(&app);
    let samples = std::mem::take(&mut *buffer.lock());
    finals.extend(seg.push(&samples[processed..]));
    finals.extend(seg.flush());
    let deadline = Instant::now() + Duration::from_secs(120);
    // A model still loading means nothing was decoded yet; wait for it too.
    while (!finals.is_empty() || busy || !ready) && Instant::now() < deadline {
        if CANCEL.load(Ordering::SeqCst) {
            break;
        }
        if drain(&worker, &mut busy).is_err() {
            break;
        }
        if !ready && status().is_some_and(|j| j.state != LiveState::Starting) {
            ready = true;
        }
        if ready && !busy {
            match finals.pop_front() {
                Some(s) => busy = send_decode(&mut worker, &mut seq, &samples[s.start..s.end], "final"),
                None => break,
            }
        }
        std::thread::sleep(Duration::from_millis(50));
        publish(&app);
    }
    if CANCEL.load(Ordering::SeqCst) {
        worker.close();
        update(|j| j.state = LiveState::Cancelled);
        return finish(&app, None, Some(&dir));
    }
    // The accurate pass over everything — only if there was speech at all.
    let spoke = status().is_some_and(|j| !j.result_text().trim().is_empty());
    if spoke {
        let all = dir.join("all.pcm");
        if write_pcm(&all, &samples).is_ok()
            && worker.send(serde_json::json!({ "op": "finalize", "path": all.to_string_lossy() }))
        {
            let secs = (samples.len() / RATE) as u64;
            let deadline = Instant::now() + Duration::from_secs(90 + secs * 3);
            let mut done = false;
            while !done && Instant::now() < deadline && !CANCEL.load(Ordering::SeqCst) {
                match worker.lines.recv_timeout(Duration::from_millis(200)) {
                    Ok(ev) => {
                        done = matches!(ev, LiveEvent::Final { .. } | LiveEvent::Error { seq: None, .. });
                        update(|j| j.apply(ev));
                        publish(&app);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        }
    }
    worker.close();
    if CANCEL.load(Ordering::SeqCst) {
        update(|j| j.state = LiveState::Cancelled);
        return finish(&app, None, Some(&dir));
    }
    finish(&app, None, Some(&dir));
}

/// End of a session: copy the text (unless cancelled), settle the state,
/// clean up, announce. `error` = the session failed (the text so far is
/// still copied — dictated words are never lost to a crash).
fn finish(app: &tauri::AppHandle, error: Option<String>, dir: Option<&Path>) {
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
        // What is shown is exactly what was copied.
        if j.final_text.is_none() {
            j.committed = text.clone();
        }
        j.partial.clear();
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
mod tests {
    use super::*;

    /// `ms` milliseconds of speech-like sound: 300 ms "syllables" with 80 ms
    /// gaps. Real speech dips between words; a gapless tone is a fan, and the
    /// segmenter rightly learns it as room noise.
    fn tone(ms: usize) -> Vec<i16> {
        (0..RATE * ms / 1000)
            .map(|i| {
                let in_word = (i % (RATE * 380 / 1000)) < RATE * 300 / 1000;
                if in_word { ((i as f32 * 0.2).sin() * 8000.0) as i16 } else { 0 }
            })
            .collect()
    }
    fn silence(ms: usize) -> Vec<i16> {
        vec![0; RATE * ms / 1000]
    }
    fn feed(seg: &mut Segmenter, parts: &[Vec<i16>]) -> Vec<Seg> {
        // Odd chunk sizes, like the microphone delivers them.
        let all: Vec<i16> = parts.concat();
        all.chunks(733).flat_map(|c| seg.push(c)).collect()
    }

    #[test]
    fn a_pause_after_enough_speech_closes_a_segment() {
        let mut s = Segmenter::new();
        let out = feed(&mut s, &[silence(500), tone(2000), silence(800)]);
        assert_eq!(out.len(), 1, "{out:?}");
        let seg = out[0];
        // Starts a pre-roll before the speech, ends a tail after it.
        assert_eq!(seg.start, RATE / 2 - PRE_ROLL);
        assert_eq!(seg.end, RATE / 2 + 2 * RATE + TAIL);
        assert!(s.open().is_none());
    }

    #[test]
    fn a_short_pause_does_not_cut_a_short_phrase() {
        let mut s = Segmenter::new();
        // 0.8 s of speech, 0.7 s pause, more speech: one sentence, not two.
        let out = feed(&mut s, &[tone(800), silence(700), tone(800)]);
        assert!(out.is_empty(), "{out:?}");
        assert!(s.open().is_some());
        // ~1.3 s of words in total: under the short-pause bar, so it waits for
        // the long pause — and then closes as ONE segment.
        assert!(feed(&mut s, &[silence(800)]).is_empty());
        let out = feed(&mut s, &[silence(800)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].start, 0);
    }

    #[test]
    fn a_long_pause_closes_even_a_short_phrase_and_drops_clicks() {
        let mut s = Segmenter::new();
        let out = feed(&mut s, &[tone(500), silence(1600)]);
        assert_eq!(out.len(), 1, "half a second of speech is still words");
        let mut s = Segmenter::new();
        let out = feed(&mut s, &[tone(100), silence(2000)]);
        assert!(out.is_empty(), "a click is not a segment: {out:?}");
        assert!(s.open().is_none());
    }

    #[test]
    fn silence_alone_produces_nothing() {
        let mut s = Segmenter::new();
        assert!(feed(&mut s, &[silence(5000)]).is_empty());
        assert!(s.flush().is_none());
    }

    #[test]
    fn non_stop_speech_is_cut_before_whispers_window() {
        let mut s = Segmenter::new();
        let out = feed(&mut s, &[tone(60_000)]);
        assert!(out.len() >= 2, "{out:?}");
        for seg in &out {
            assert!(seg.end - seg.start <= MAX_SEGMENT, "{seg:?}");
        }
        // Back to back: nothing lost between two cuts.
        assert_eq!(out[0].end, out[1].start);
    }

    #[test]
    fn segments_never_overlap_the_previous_one() {
        let mut s = Segmenter::new();
        // The pre-roll of the second phrase must not reach into the first.
        let out = feed(&mut s, &[tone(2000), silence(650), tone(2000), silence(900)]);
        assert_eq!(out.len(), 2, "{out:?}");
        assert!(out[1].start >= out[0].end);
    }

    #[test]
    fn flush_keeps_the_last_words_when_stopped_mid_sentence() {
        let mut s = Segmenter::new();
        assert!(feed(&mut s, &[tone(1200)]).is_empty());
        let seg = s.flush().expect("speech at the stop is kept");
        assert_eq!(seg.start, 0);
        assert_eq!(seg.end, (RATE * 12 / 10 / FRAME) * FRAME);
    }

    #[test]
    fn steady_background_noise_raises_the_bar() {
        let mut s = Segmenter::new();
        // A fan at ~-34 dBFS: above the absolute threshold, but it is the room.
        let hum: Vec<i16> = (0..RATE * 6).map(|i| ((i as f32 * 0.05).sin() * 900.0) as i16).collect();
        let out = feed(&mut s, std::slice::from_ref(&hum));
        let flushed = s.flush();
        // At most the first moments read as speech before the floor adapts.
        let total: usize = out.iter().chain(flushed.iter()).map(|g| g.end - g.start).sum();
        assert!(total < RATE * 2, "the room became one long sentence: {out:?} {flushed:?}");
    }

    fn text(seq: u64, kind: &str, t: &str) -> LiveEvent {
        LiveEvent::Text { seq, kind: kind.into(), text: t.into() }
    }

    #[test]
    fn finals_append_and_partials_preview_the_open_segment() {
        let mut j = LiveJob::new(1, None, 0);
        j.apply(LiveEvent::Ready);
        assert_eq!(j.state, LiveState::Recording);
        j.apply(text(1, "partial", "Hallo"));
        assert_eq!(j.partial, "Hallo");
        j.apply(text(2, "final", "Hallo Welt."));
        assert_eq!((j.committed.as_str(), j.partial.as_str()), ("Hallo Welt.", ""));
        j.apply(text(3, "partial", "Wie geht"));
        j.apply(text(4, "final", "Wie geht es?"));
        assert_eq!(j.committed, "Hallo Welt. Wie geht es?");
        assert_eq!(j.result_text(), "Hallo Welt. Wie geht es?");
    }

    #[test]
    fn a_stale_partial_never_shows_after_its_final() {
        let mut j = LiveJob::new(1, None, 0);
        j.apply(text(5, "final", "Erster Satz."));
        j.apply(text(4, "partial", "Erster"));
        assert_eq!(j.partial, "", "a preview older than the kept text is dropped");
        j.apply(text(6, "partial", "Zweiter"));
        assert_eq!(j.partial, "Zweiter");
    }

    #[test]
    fn the_accurate_pass_replaces_the_live_text() {
        let mut j = LiveJob::new(1, None, 0);
        j.apply(text(1, "final", "hallo welt"));
        j.apply(LiveEvent::Final { text: " Hallo, Welt! ".into() });
        assert_eq!(j.result_text(), "Hallo, Welt!");
    }

    #[test]
    fn a_failed_accurate_pass_keeps_the_live_text() {
        let mut j = LiveJob::new(1, None, 0);
        j.apply(text(1, "final", "Das bleibt."));
        j.apply(text(2, "partial", "und das"));
        j.apply(LiveEvent::Error { seq: None, error: "boom".into() });
        assert_eq!(j.result_text(), "Das bleibt. und das");
        j.apply(LiveEvent::Final { text: "  ".into() });
        assert_eq!(j.result_text(), "Das bleibt. und das");
        // Even an empty accurate text that got past `apply` never wins.
        j.final_text = Some(String::new());
        assert_eq!(j.result_text(), "Das bleibt. und das");
    }

    #[test]
    fn whisper_silence_phrases_are_dropped() {
        assert!(is_noise_text("Untertitel im Auftrag des ZDF, 2020"));
        assert!(is_noise_text(" ... "));
        assert!(!is_noise_text("Das ZDF berichtet heute."));
        let mut j = LiveJob::new(1, None, 0);
        j.apply(text(1, "final", "Untertitel der Amara.org-Community"));
        assert_eq!(j.committed, "");
    }

    #[test]
    fn worker_lines_parse_and_noise_is_ignored() {
        assert_eq!(parse_event(r#"{"event":"ready"}"#), Some(LiveEvent::Ready));
        assert_eq!(
            parse_event(r#"{"event":"text","seq":3,"kind":"final","text":"Hi"}"#),
            Some(text(3, "final", "Hi"))
        );
        assert_eq!(
            parse_event(r#"{"event":"error","seq":null,"error":"x"}"#),
            Some(LiveEvent::Error { seq: None, error: "x".into() })
        );
        assert_eq!(parse_event("Downloading model 10%"), None);
    }


    /// Real end-to-end check against the installed Whisper venv (macOS `say`
    /// supplies the speech): segmenter → worker → kept text, then the accurate
    /// pass. `cargo test -p inspector-rust-core --lib transcribe_live_live -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[cfg(target_os = "macos")]
    fn transcribe_live_live() {
        let dir = std::env::temp_dir().join(format!("ir-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("speech.wav");
        let ok = Command::new("say")
            .args(["-v", "Anna", "-o"])
            .arg(&wav)
            .args([
                "--data-format=LEI16@16000",
                "Guten Morgen. Heute testen wir das Diktat. [[slnc 1200]] Der zweite Satz kommt nach einer Pause.",
            ])
            .status()
            .unwrap();
        assert!(ok.success());
        let bytes = std::fs::read(&wav).unwrap();
        let data = bytes.windows(4).position(|w| w == b"data").unwrap() + 8;
        let mut pcm: Vec<i16> = bytes[data..].as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b)).collect();
        pcm.extend(vec![0i16; RATE * 2]);

        let mut worker = Worker::spawn(Some("de")).expect("worker");
        let mut seg = Segmenter::new();
        let mut segs: Vec<Seg> = pcm.chunks(640).flat_map(|c| seg.push(c)).collect();
        segs.extend(seg.flush());
        println!("segments: {segs:?}");
        assert!(segs.len() >= 2, "the pause should split the dictation: {segs:?}");

        let mut job = LiveJob::new(1, Some("de".into()), 0);
        let wait = |worker: &mut Worker, job: &mut LiveJob, until: fn(&LiveEvent) -> bool| loop {
            let ev = worker.lines.recv_timeout(Duration::from_secs(300)).expect("worker reply");
            let stop = until(&ev);
            job.apply(ev);
            if stop {
                break;
            }
        };
        wait(&mut worker, &mut job, |e| matches!(e, LiveEvent::Ready));
        for (i, sg) in segs.iter().enumerate() {
            let path = dir.join(format!("{i}.pcm"));
            write_pcm(&path, &pcm[sg.start..sg.end]).unwrap();
            let t0 = Instant::now();
            assert!(worker.send(serde_json::json!({"op":"decode","seq":i as u64 + 1,"kind":"final","path":path.to_string_lossy(),"prompt":""})));
            wait(&mut worker, &mut job, |e| matches!(e, LiveEvent::Text { .. } | LiveEvent::Error { .. }));
            println!("segment {i}: {:?} in {:?}", job.committed, t0.elapsed());
        }
        let live = job.committed.to_lowercase();
        assert!(live.contains("morgen") && live.contains("satz"), "{live}");
        let all = dir.join("all.pcm");
        write_pcm(&all, &pcm).unwrap();
        let t0 = Instant::now();
        assert!(worker.send(serde_json::json!({"op":"finalize","path":all.to_string_lossy()})));
        wait(&mut worker, &mut job, |e| matches!(e, LiveEvent::Final { .. } | LiveEvent::Error { seq: None, .. }));
        println!("final in {:?}: {:?}", t0.elapsed(), job.final_text);
        assert!(job.final_text.as_deref().is_some_and(|t| t.to_lowercase().contains("pause")));
        worker.close();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn meter_and_prompt_helpers() {
        assert_eq!(meter_level(0.0), 0.0);
        assert_eq!(meter_level(1.0), 1.0);
        assert!((meter_level(0.031_622_8) - 0.5).abs() < 0.01);
        assert_eq!(prompt_tail("abcdef", 3), "def");
        assert_eq!(prompt_tail("äöü", 10), "äöü");
    }
}
