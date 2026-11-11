//! The live pipeline: per source capture → VAD → segmenter; one inference
//! thread (finals before previews) → LocalAgreement / echo gate → writer.
//!
//! Threads: one per source (owns its capture handle — cpal streams are not
//! `Send`), one for inference, one for the writer. A lost source restarts
//! every 2 s without touching the other; a fatal write error stops the
//! session with an error instead of recording into nothing.

use crate::capture::{AudioInput, CaptureFactory};
use crate::decode::{DecodeOpts, Decoded};
use crate::dedupe::{EchoGate, FinalSeg, Release};
use crate::protocol::session::{Line, SessionMeta, SessionStatus, META_VERSION};
use crate::protocol::{DedupeMode, Event, Mode, SessionConfig, Source, SourceStatus};
use crate::queue::{Job, JobKind, JobQueue};
use crate::segment::{Segmenter, MAX_SEG, RATE, VAD_FRAME};
use crate::text::Agreement;
use crate::vad::Vad;
use crate::writer::{spawn_writer, Writer, WriterMsg, WriterReport};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

pub trait Transcriber: Send {
    fn decode(&mut self, source: Source, samples: &[f32], opts: &DecodeOpts) -> Result<Decoded, String>;
}

pub type Sink = Arc<dyn Fn(Event) + Send + Sync>;
pub type VadFactory = Arc<dyn Fn() -> Result<Box<dyn Vad>, String> + Send + Sync>;

pub struct Deps {
    pub transcriber: Box<dyn Transcriber>,
    pub vad: VadFactory,
    pub capture: Arc<dyn CaptureFactory>,
    pub fallback_root: PathBuf,
    pub threads: usize,
    /// Headphones on the output (no echo possible) — only read for `DedupeMode::Auto`.
    pub headphones: Option<bool>,
    pub timing: Timing,
}

/// How a lost source is brought back.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Pause between quick restart attempts.
    pub retry_every: Duration,
    /// Quick attempts in a row before slowing down.
    pub max_retries: u32,
    /// Pause between attempts once the quick ones are used up — a source is
    /// never given up for the rest of the session (a USB mic plugged back in
    /// an hour later still comes back).
    pub slow_retry: Duration,
    /// A running capture that delivers nothing this long is restarted
    /// (coreaudiod restart, wake from sleep, a dead aggregate device). `None`
    /// for captures that only deliver while something plays.
    pub no_data: Option<Duration>,
}

impl Default for Timing {
    fn default() -> Self {
        Timing { retry_every: RETRY_EVERY, max_retries: MAX_RETRIES, slow_retry: SLOW_RETRY, no_data: Some(NO_DATA) }
    }
}

/// Hypotheses for the open segment, at most this often.
pub const HYPOTHESIS_EVERY: Duration = Duration::from_secs(1);
/// … over at most its last seconds.
pub const HYPOTHESIS_WINDOW: usize = RATE * 10;
/// … and only once it holds this much.
pub const HYPOTHESIS_MIN: usize = RATE / 2;
pub const LEVEL_EVERY: Duration = Duration::from_millis(100);
pub const RETRY_EVERY: Duration = Duration::from_secs(2);
pub const MAX_RETRIES: u32 = 30;
pub const SLOW_RETRY: Duration = Duration::from_secs(30);
pub const NO_DATA: Duration = Duration::from_secs(5);
/// Attempts to create the VAD before a source is reported lost.
pub const VAD_TRIES: u32 = 5;
/// Audio kept per source for song recognition (`Command::Window`).
pub const WINDOW_KEEP: usize = RATE * 30;
pub const PROMPT_CHARS: usize = 200;

pub fn max_seg_for(mode: Mode) -> usize {
    match mode {
        Mode::Room | Mode::Song => RATE * 20,
        _ => MAX_SEG,
    }
}

/// Song and room sessions transcribe music, not speech.
pub fn is_music(mode: Mode) -> bool {
    matches!(mode, Mode::Room | Mode::Song)
}

/// The text handed to a decode as context. Previews get none. Music gets
/// none either: on a stretch it cannot make out, Whisper answers with the
/// prompt itself, and the repeat then feeds the next prompt.
pub fn prompt_for(music: bool, kind: JobKind, tail: &str) -> String {
    if music || kind != JobKind::Final {
        String::new()
    } else {
        tail.to_string()
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn rms(s: &[f32]) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
}

fn idx(s: Source) -> usize {
    match s {
        Source::Mic => 0,
        Source::Sys => 1,
    }
}

enum SourceMsg {
    Chunk(Vec<f32>),
    Error(String),
    On,
    Off,
    Stop,
}

struct Shared {
    queue: Mutex<JobQueue>,
    cv: Condvar,
    stop: AtomicBool,
    sources_alive: AtomicUsize,
    windows: [Mutex<VecDeque<f32>>; 2],
    /// A source has an open segment (someone is talking right now).
    speaking: [AtomicBool; 2],
    dir: Mutex<Option<PathBuf>>,
    /// The session language; can change while it runs (`set_lang`).
    lang: Mutex<crate::protocol::Lang>,
}

impl Shared {
    fn push(&self, job: Job) {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).push(job);
        self.cv.notify_one();
    }
}

pub struct Pipeline {
    shared: Arc<Shared>,
    senders: [mpsc::Sender<SourceMsg>; 2],
    sources: Vec<JoinHandle<()>>,
    inference: Option<JoinHandle<()>>,
}

impl Pipeline {
    pub fn start(cfg: SessionConfig, sink: Sink, deps: Deps) -> Result<Pipeline, String> {
        let meta = SessionMeta {
            version: META_VERSION,
            started_ms: now_ms(),
            ended_ms: None,
            status: SessionStatus::Running,
            model: cfg.model.clone(),
            lang: cfg.lang,
            sources: cfg.sources.clone(),
            mode: cfg.mode,
            fallback_from: None,
            note: None,
        };
        let writer = Writer::open(&cfg.root, &deps.fallback_root, meta, cfg.keep_audio).map_err(|e| e.to_string())?;
        let dir = writer.dir().to_path_buf();
        let (wtx, wrx, whandle) = spawn_writer(writer);

        let shared = Arc::new(Shared {
            queue: Mutex::new(JobQueue::new()),
            cv: Condvar::new(),
            stop: AtomicBool::new(false),
            sources_alive: AtomicUsize::new(2),
            windows: [Mutex::new(VecDeque::new()), Mutex::new(VecDeque::new())],
            speaking: [AtomicBool::new(false), AtomicBool::new(false)],
            dir: Mutex::new(Some(dir.clone())),
            lang: Mutex::new(cfg.lang),
        });
        sink(Event::Started { session_dir: dir, sources: cfg.sources.clone() });

        let max_seg = max_seg_for(cfg.mode);
        let mut senders = Vec::new();
        let mut sources = Vec::new();
        for src in [Source::Mic, Source::Sys] {
            let (tx, rx) = mpsc::channel();
            let ctx = SourceCtx {
                source: src,
                on: cfg.sources.contains(&src),
                device: if src == Source::Mic { cfg.mic_device.clone() } else { None },
                rx,
                tx: tx.clone(),
                shared: shared.clone(),
                capture: deps.capture.clone(),
                vad: deps.vad.clone(),
                sink: sink.clone(),
                max_seg,
                music: is_music(cfg.mode),
                timing: deps.timing,
            };
            senders.push(tx);
            sources.push(
                std::thread::Builder::new()
                    .name(format!("ir-transcribe-{}", src.slug()))
                    .spawn(move || source_loop(ctx))
                    .map_err(|e| e.to_string())?,
            );
        }

        let both = cfg.sources.contains(&Source::Mic) && cfg.sources.contains(&Source::Sys);
        let gate_on = both
            && match cfg.dedupe {
                DedupeMode::Always => true,
                DedupeMode::Never => false,
                DedupeMode::Auto => !deps.headphones.unwrap_or(false),
            };
        let threshold = cfg.dedupe_threshold.clamp(0.5, 0.95);
        let inf = InferenceCtx {
            shared: shared.clone(),
            sink,
            transcriber: deps.transcriber,
            threads: deps.threads,
            music: is_music(cfg.mode),
            gate: EchoGate::new(threshold, gate_on),
            wtx,
            wrx,
            whandle: Some(whandle),
        };
        let inference = std::thread::Builder::new()
            .name("ir-transcribe-infer".into())
            .spawn(move || inference_loop(inf))
            .map_err(|e| e.to_string())?;

        let senders: [mpsc::Sender<SourceMsg>; 2] = [senders.remove(0), senders.remove(0)];
        Ok(Pipeline { shared, senders, sources, inference: Some(inference) })
    }

    pub fn dir(&self) -> Option<PathBuf> {
        self.shared.dir.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn is_running(&self) -> bool {
        !self.shared.stop.load(Ordering::SeqCst)
    }

    /// Switch the language without stopping: segments decoded from now on
    /// use it (the open segment included), finals already written keep theirs.
    pub fn set_lang(&self, lang: crate::protocol::Lang) {
        *self.shared.lang.lock().unwrap_or_else(|e| e.into_inner()) = lang;
    }

    pub fn set_source(&self, source: Source, on: bool) {
        let _ = self.senders[idx(source)].send(if on { SourceMsg::On } else { SourceMsg::Off });
    }

    /// The last `secs` seconds of a source as 16 kHz i16.
    pub fn window(&self, source: Source, secs: u32) -> Vec<i16> {
        let w = self.shared.windows[idx(source)].lock().unwrap_or_else(|e| e.into_inner());
        let n = (secs as usize * RATE).min(w.len());
        w.iter().skip(w.len() - n).map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).collect()
    }

    /// Stop capturing, transcribe what is left, close the session. Returns
    /// its folder.
    pub fn stop(mut self) -> Option<PathBuf> {
        for s in &self.senders {
            let _ = s.send(SourceMsg::Stop);
        }
        for h in self.sources.drain(..) {
            let _ = h.join();
        }
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.cv.notify_all();
        if let Some(h) = self.inference.take() {
            let _ = h.join();
        }
        self.dir()
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        if self.inference.is_some() {
            for s in &self.senders {
                let _ = s.send(SourceMsg::Stop);
            }
            self.shared.stop.store(true, Ordering::SeqCst);
            self.shared.cv.notify_all();
        }
    }
}

struct SourceCtx {
    source: Source,
    on: bool,
    device: Option<String>,
    rx: mpsc::Receiver<SourceMsg>,
    tx: mpsc::Sender<SourceMsg>,
    shared: Arc<Shared>,
    capture: Arc<dyn CaptureFactory>,
    vad: VadFactory,
    sink: Sink,
    max_seg: usize,
    music: bool,
    timing: Timing,
}

struct Stream {
    seg: Segmenter,
    vad: Box<dyn Vad>,
    pending: Vec<f32>,
    origin_ms: Option<u64>,
    seq: u64,
    last_hyp: Instant,
    last_level: Instant,
}

fn source_loop(c: SourceCtx) {
    let src = c.source;
    let t = c.timing;
    let state = |s: SourceStatus, d: Option<String>| (c.sink)(Event::SourceState { source: src, state: s, detail: d });
    // Two sources create their VAD at the same moment; a transient failure
    // (e.g. the model file being written on first use) is tried again.
    let mut vad = Err(String::new());
    for i in 0..VAD_TRIES {
        vad = (c.vad)();
        if vad.is_ok() || c.shared.stop.load(Ordering::SeqCst) {
            break;
        }
        std::thread::sleep(t.retry_every * (i + 1));
    }
    let mut stream = match vad {
        Ok(vad) => Stream {
            seg: Segmenter::new(c.max_seg),
            vad,
            pending: Vec::new(),
            origin_ms: None,
            seq: 0,
            last_hyp: Instant::now(),
            last_level: Instant::now(),
        },
        Err(e) => {
            state(SourceStatus::Lost, Some(e));
            c.shared.sources_alive.fetch_sub(1, Ordering::SeqCst);
            c.shared.cv.notify_all();
            return;
        }
    };
    let mut on = c.on;
    let mut input: Option<Box<dyn AudioInput>> = None;
    let mut retry_at: Option<Instant> = None;
    let mut retries = 0u32;
    let mut last_data = Instant::now();
    let mut slowed = false;

    let start = |input: &mut Option<Box<dyn AudioInput>>| -> Result<(), String> {
        let tx1 = c.tx.clone();
        let tx2 = c.tx.clone();
        let h = c.capture.start(
            src,
            c.device.as_deref(),
            Box::new(move |s: &[f32]| {
                let _ = tx1.send(SourceMsg::Chunk(s.to_vec()));
            }),
            Box::new(move |e| {
                let _ = tx2.send(SourceMsg::Error(e));
            }),
        )?;
        if let Some(w) = h.warning() {
            (c.sink)(Event::Warning { code: format!("{}.capture", src.slug()), detail: w });
        }
        *input = Some(h);
        Ok(())
    };

    if on {
        state(SourceStatus::Starting, None);
        match start(&mut input) {
            Ok(()) => {
                last_data = Instant::now();
                state(SourceStatus::Running, None)
            }
            Err(e) => {
                state(SourceStatus::Lost, Some(e));
                retry_at = Some(Instant::now() + t.retry_every);
            }
        }
    } else {
        state(SourceStatus::Off, None);
    }

    loop {
        if c.shared.stop.load(Ordering::SeqCst) {
            break;
        }
        // Wake for the next restart attempt, not only every 200 ms.
        let wait = retry_at
            .map(|at| at.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_millis(200))
            .clamp(Duration::from_millis(1), Duration::from_millis(200));
        match c.rx.recv_timeout(wait) {
            Ok(SourceMsg::Chunk(chunk)) => {
                last_data = Instant::now();
                if on {
                    process_chunk(&c, &mut stream, &chunk);
                }
            }
            Ok(SourceMsg::Error(e)) => {
                if let Some(h) = input.take() {
                    h.stop();
                }
                flush(&c, &mut stream);
                state(SourceStatus::Lost, Some(e));
                retry_at = Some(Instant::now() + t.retry_every);
            }
            Ok(SourceMsg::On) if !on => {
                on = true;
                retries = 0;
                state(SourceStatus::Starting, None);
                reset(&mut stream, c.max_seg);
                match start(&mut input) {
                    Ok(()) => {
                        last_data = Instant::now();
                        state(SourceStatus::Running, None)
                    }
                    Err(e) => {
                        state(SourceStatus::Lost, Some(e));
                        retry_at = Some(Instant::now() + t.retry_every);
                    }
                }
            }
            Ok(SourceMsg::Off) if on => {
                on = false;
                retry_at = None;
                if let Some(h) = input.take() {
                    h.stop();
                }
                flush(&c, &mut stream);
                state(SourceStatus::Off, None);
            }
            Ok(SourceMsg::On) | Ok(SourceMsg::Off) => {}
            Ok(SourceMsg::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        // A capture that is "running" but delivers nothing is as good as lost.
        if on
            && input.as_ref().is_some_and(|h| h.continuous())
            && t.no_data.is_some_and(|d| last_data.elapsed() >= d)
        {
            if let Some(h) = input.take() {
                h.stop();
            }
            flush(&c, &mut stream);
            state(SourceStatus::Lost, Some("no audio — restarting".into()));
            retry_at = Some(Instant::now() + t.retry_every);
        }
        if on && input.is_none() && retry_at.is_some_and(|at| Instant::now() >= at) {
            retries += 1;
            reset(&mut stream, c.max_seg);
            match start(&mut input) {
                Ok(()) => {
                    retry_at = None;
                    retries = 0;
                    slowed = false;
                    last_data = Instant::now();
                    state(SourceStatus::Running, None);
                }
                Err(e) if retries >= t.max_retries => {
                    retry_at = Some(Instant::now() + t.slow_retry);
                    if !slowed {
                        slowed = true;
                        state(SourceStatus::Lost, Some(format!("gave up quick retries, trying every {:?}: {e}", t.slow_retry)));
                    }
                }
                Err(_) => retry_at = Some(Instant::now() + t.retry_every),
            }
        }
        if on {
            maybe_hypothesis(&c, &mut stream);
        }
    }
    if let Some(h) = input.take() {
        h.stop();
    }
    // Messages still queued (chunks that arrived before the stop) count.
    while let Ok(SourceMsg::Chunk(chunk)) = c.rx.try_recv() {
        if on {
            process_chunk(&c, &mut stream, &chunk);
        }
    }
    flush(&c, &mut stream);
    c.shared.sources_alive.fetch_sub(1, Ordering::SeqCst);
    c.shared.cv.notify_all();
}

fn reset(s: &mut Stream, max_seg: usize) {
    s.seg = Segmenter::new(max_seg);
    s.pending.clear();
    s.origin_ms = None;
}

fn to_ms(origin: u64, sample: u64) -> u64 {
    origin + sample * 1000 / RATE as u64
}

fn process_chunk(c: &SourceCtx, s: &mut Stream, chunk: &[f32]) {
    if s.origin_ms.is_none() {
        s.origin_ms = Some(now_ms().saturating_sub(chunk.len() as u64 * 1000 / RATE as u64));
    }
    {
        let mut w = c.shared.windows[idx(c.source)].lock().unwrap_or_else(|e| e.into_inner());
        w.extend(chunk.iter().copied());
        while w.len() > WINDOW_KEEP {
            w.pop_front();
        }
    }
    if s.last_level.elapsed() >= LEVEL_EVERY {
        s.last_level = Instant::now();
        (c.sink)(Event::Level { source: c.source, rms: rms(chunk) });
    }
    s.pending.extend_from_slice(chunk);
    let n = s.pending.len() / VAD_FRAME * VAD_FRAME;
    if n == 0 {
        return;
    }
    let frames: Vec<f32> = s.pending.drain(..n).collect();
    let probs = match s.vad.probs(&frames) {
        Ok(p) => p,
        Err(e) => {
            (c.sink)(Event::Warning { code: format!("{}.vad", c.source.slug()), detail: e });
            vec![0.0; n / VAD_FRAME]
        }
    };
    let probs = if c.music { crate::vad::music_probs(&frames, probs) } else { probs };
    let origin = s.origin_ms.unwrap_or(0);
    for (seg, samples) in s.seg.push(&frames, &probs) {
        s.seq += 1;
        c.shared.push(Job {
            kind: JobKind::Final,
            source: c.source,
            seq: s.seq,
            start_ms: to_ms(origin, seg.start),
            samples: Arc::new(samples),
            prompt: String::new(),
            lang: None,
        });
        s.last_hyp = Instant::now();
    }
    c.shared.speaking[idx(c.source)].store(s.seg.open().is_some(), Ordering::SeqCst);
}

fn maybe_hypothesis(c: &SourceCtx, s: &mut Stream) {
    if s.last_hyp.elapsed() < HYPOTHESIS_EVERY {
        return;
    }
    let Some((open, samples)) = s.seg.open() else { return };
    if samples.len() < HYPOTHESIS_MIN {
        return;
    }
    s.last_hyp = Instant::now();
    let from = samples.len().saturating_sub(HYPOTHESIS_WINDOW);
    c.shared.push(Job {
        kind: JobKind::Hypothesis,
        source: c.source,
        seq: s.seq + 1,
        start_ms: to_ms(s.origin_ms.unwrap_or(0), open.start + from as u64),
        samples: Arc::new(samples[from..].to_vec()),
        prompt: String::new(),
        lang: None,
    });
}

fn flush(c: &SourceCtx, s: &mut Stream) {
    c.shared.speaking[idx(c.source)].store(false, Ordering::SeqCst);
    if let Some((seg, samples)) = s.seg.flush() {
        s.seq += 1;
        c.shared.push(Job {
            kind: JobKind::Final,
            source: c.source,
            seq: s.seq,
            start_ms: to_ms(s.origin_ms.unwrap_or(0), seg.start),
            samples: Arc::new(samples),
            prompt: String::new(),
            lang: None,
        });
    }
}

struct InferenceCtx {
    shared: Arc<Shared>,
    sink: Sink,
    transcriber: Box<dyn Transcriber>,
    threads: usize,
    music: bool,
    gate: EchoGate,
    wtx: mpsc::Sender<WriterMsg>,
    wrx: mpsc::Receiver<WriterReport>,
    whandle: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct SrcState {
    agreement: Agreement,
    last_lang: Option<String>,
    tail: String,
    last_final: u64,
}

fn tail_of(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

fn inference_loop(mut c: InferenceCtx) {
    let mut st: [SrcState; 2] = Default::default();
    let mut audio: HashMap<(Source, u64), Arc<Vec<f32>>> = HashMap::new();
    let mut fatal: Option<String> = None;

    loop {
        let job = {
            let mut q = c.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            if q.is_empty() {
                let done = c.shared.stop.load(Ordering::SeqCst)
                    && c.shared.sources_alive.load(Ordering::SeqCst) == 0;
                if done {
                    break;
                }
                q = c.shared.cv.wait_timeout(q, Duration::from_millis(250)).map(|r| r.0).unwrap_or_else(|e| e.into_inner().0);
            }
            q.pop()
        };
        if let Some(job) = job {
            run_job(&mut c, &mut st, &mut audio, job);
        }
        // Nobody is talking on SYS and none of its finals wait: a held MIC
        // line cannot be an echo of anything still to come — release it now
        // instead of after the full hold time.
        let sys_idle = !c.shared.speaking[idx(Source::Sys)].load(Ordering::SeqCst)
            && !c.shared.queue.lock().unwrap_or_else(|e| e.into_inner()).has_final(Source::Sys);
        c.gate.set_sys_busy(!sys_idle);
        let mut rel = c.gate.tick(now_ms());
        if sys_idle {
            rel.extend(c.gate.drain());
        }
        release(&mut c, &mut st, &mut audio, rel);
        if let Some(e) = drain_reports(&c) {
            fatal = Some(e);
            c.shared.stop.store(true, Ordering::SeqCst);
            break;
        }
    }
    if fatal.is_none() {
        let rel = c.gate.drain();
        release(&mut c, &mut st, &mut audio, rel);
        let _ = c.wtx.send(WriterMsg::Finish(SessionStatus::Completed));
    }
    drop(c.wtx);
    let mut dir = None;
    for r in c.wrx.iter() {
        match r {
            WriterReport::Finished(d) => dir = Some(d),
            WriterReport::Fatal(e) => fatal = fatal.or(Some(e)),
            WriterReport::Warning(w) => (c.sink)(Event::Warning { code: "writer.fallback".into(), detail: w }),
            WriterReport::Written => {}
        }
    }
    if let Some(h) = c.whandle.take() {
        let _ = h.join();
    }
    if let Some(d) = &dir {
        *c.shared.dir.lock().unwrap_or_else(|e| e.into_inner()) = Some(d.clone());
    }
    (c.sink)(Event::Stopped { session_dir: dir, error: fatal });
}

fn drain_reports(c: &InferenceCtx) -> Option<String> {
    while let Ok(r) = c.wrx.try_recv() {
        match r {
            WriterReport::Warning(w) => {
                if let Some(d) = w.rsplit("continuing in ").next() {
                    *c.shared.dir.lock().unwrap_or_else(|e| e.into_inner()) = Some(PathBuf::from(d));
                }
                (c.sink)(Event::Warning { code: "writer.fallback".into(), detail: w });
            }
            WriterReport::Fatal(e) => return Some(e),
            WriterReport::Written | WriterReport::Finished(_) => {}
        }
    }
    None
}

fn run_job(c: &mut InferenceCtx, st: &mut [SrcState; 2], audio: &mut HashMap<(Source, u64), Arc<Vec<f32>>>, job: Job) {
    let s = &mut st[idx(job.source)];
    if job.kind == JobKind::Hypothesis && job.seq <= s.last_final {
        return;
    }
    let opts = DecodeOpts {
        kind: job.kind,
        lang: *c.shared.lang.lock().unwrap_or_else(|e| e.into_inner()),
        last_lang: s.last_lang.clone(),
        prompt: prompt_for(c.music, job.kind, &s.tail),
        threads: c.threads,
    };
    let decoded = match c.transcriber.decode(job.source, &job.samples, &opts) {
        Ok(d) => d,
        Err(e) => {
            (c.sink)(Event::Warning { code: "decode".into(), detail: e });
            if job.kind == JobKind::Final {
                s.last_final = s.last_final.max(job.seq);
                s.agreement.reset();
                (c.sink)(Event::Dropped { source: job.source, seq: job.seq, reason: "decode_error".into() });
            }
            return;
        }
    };
    match job.kind {
        JobKind::Hypothesis => {
            if decoded.text.is_empty() {
                return;
            }
            let (stable, pending) = s.agreement.update(&decoded.text);
            (c.sink)(Event::Hypothesis { source: job.source, seq: job.seq, stable, pending });
        }
        JobKind::Final => {
            s.last_final = s.last_final.max(job.seq);
            s.agreement.reset();
            if decoded.text.is_empty() {
                let reason = if decoded.rejected.is_empty() { "empty".to_string() } else { decoded.rejected.join(",") };
                (c.sink)(Event::Dropped { source: job.source, seq: job.seq, reason });
                return;
            }
            s.last_lang = Some(decoded.lang.clone());
            s.tail = tail_of(&format!("{} {}", s.tail, decoded.text), PROMPT_CHARS);
            let end_ms = job.start_ms + job.samples.len() as u64 * 1000 / RATE as u64;
            let seg = FinalSeg {
                source: job.source,
                seq: job.seq,
                start_ms: job.start_ms,
                end_ms,
                text: decoded.text,
                lang: decoded.lang,
                confidence: decoded.confidence,
            };
            audio.insert((job.source, job.seq), job.samples.clone());
            let rel = c.gate.offer(seg, now_ms());
            release(c, st, audio, rel);
        }
    }
}

fn release(c: &mut InferenceCtx, _st: &mut [SrcState; 2], audio: &mut HashMap<(Source, u64), Arc<Vec<f32>>>, rel: Vec<Release>) {
    for r in rel {
        match r {
            Release::Keep(f) => {
                let samples = audio.remove(&(f.source, f.seq));
                let line = Line {
                    start_ms: f.start_ms,
                    end_ms: f.end_ms,
                    source: f.source,
                    lang: f.lang.clone(),
                    confidence: f.confidence,
                    text: f.text.clone(),
                };
                let _ = c.wtx.send(WriterMsg::Line(line, samples.map(|a| a.to_vec())));
                (c.sink)(Event::Final {
                    source: f.source,
                    seq: f.seq,
                    start_ms: f.start_ms,
                    end_ms: f.end_ms,
                    lang: f.lang,
                    text: f.text,
                    confidence: f.confidence,
                });
            }
            Release::Drop(f, reason) => {
                audio.remove(&(f.source, f.seq));
                (c.sink)(Event::Dropped { source: f.source, seq: f.seq, reason });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{Chunk, OnError};
    use crate::protocol::session::read_lines;
    use crate::protocol::Lang;
    use crate::vad::EnergyVad;

    /// Speech for `secs`, then 1.5 s of silence (closes the segment).
    fn utterance(secs: f32) -> Vec<f32> {
        let mut v: Vec<f32> = (0..(secs * RATE as f32) as usize).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        v.extend(vec![0.0; RATE * 3 / 2]);
        v
    }

    struct FakeInput;
    impl AudioInput for FakeInput {
        fn stop(self: Box<Self>) {}
    }

    struct FakeCapture {
        data: HashMap<Source, Vec<f32>>,
        fail: Option<Source>,
    }

    impl CaptureFactory for FakeCapture {
        fn start(&self, source: Source, _: Option<&str>, mut chunk: Chunk, mut err: OnError) -> Result<Box<dyn AudioInput>, String> {
            if let Some(d) = self.data.get(&source) {
                if source == Source::Mic && self.data.contains_key(&Source::Sys) {
                    // Real speaker echo reaches the mic as the system plays it;
                    // the fake threads would otherwise race at start.
                    let d = d.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(150));
                        for c in d.chunks(1600) {
                            chunk(c);
                        }
                    });
                } else {
                    for c in d.chunks(1600) {
                        chunk(c);
                    }
                }
            }
            if self.fail == Some(source) {
                err("unplugged".into());
            }
            Ok(Box::new(FakeInput))
        }
    }

    struct FakeTranscriber {
        text: Arc<dyn Fn(Source, JobKind) -> String + Send + Sync>,
        delay: Duration,
    }

    impl Transcriber for FakeTranscriber {
        fn decode(&mut self, source: Source, _: &[f32], opts: &DecodeOpts) -> Result<Decoded, String> {
            std::thread::sleep(self.delay);
            Ok(Decoded {
                text: (self.text)(source, opts.kind),
                lang: "de".into(),
                confidence: 0.9,
                mean_logprob: -0.1,
                no_speech: 0.0,
                rejected: vec![],
            })
        }
    }

    struct Run {
        events: Arc<Mutex<Vec<Event>>>,
        _tmp: tempfile::TempDir,
        root: PathBuf,
    }

    fn cfg(root: PathBuf) -> SessionConfig {
        SessionConfig {
            sources: vec![Source::Mic, Source::Sys],
            mic_device: None,
            model: "fake".into(),
            lang: Lang::Auto,
            root,
            keep_audio: false,
            dedupe: DedupeMode::Auto,
            dedupe_threshold: 0.75,
            mode: Mode::Meeting,
        }
    }

    /// A capture that answers each start from a script: `Err`, `Ok` that
    /// reports a failure right away, `Ok` that stays silent, or `Ok` with audio.
    #[derive(Clone, Copy, Debug)]
    enum Act {
        Fail,
        Drop,
        Silent,
        Speak,
    }

    struct Scripted {
        acts: Vec<Act>,
        calls: Arc<AtomicUsize>,
        audio: Vec<f32>,
    }

    impl CaptureFactory for Scripted {
        fn start(&self, source: Source, _: Option<&str>, mut chunk: Chunk, mut err: OnError) -> Result<Box<dyn AudioInput>, String> {
            assert_eq!(source, Source::Mic);
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            match self.acts.get(n).copied().unwrap_or(Act::Speak) {
                Act::Fail => Err(format!("start {n} failed")),
                Act::Drop => {
                    err(format!("dropped {n}"));
                    Ok(Box::new(FakeInput))
                }
                Act::Silent => Ok(Box::new(FakeInput)),
                Act::Speak => {
                    for c in self.audio.chunks(1600) {
                        chunk(c);
                    }
                    Ok(Box::new(FakeInput))
                }
            }
        }
    }

    fn fast() -> Timing {
        Timing {
            retry_every: Duration::from_millis(10),
            max_retries: 2,
            slow_retry: Duration::from_millis(40),
            no_data: Some(Duration::from_millis(150)),
        }
    }

    fn run_scripted(acts: Vec<Act>, timing: Timing, vad_fails: usize, wait: Duration) -> (Vec<Event>, usize) {
        let tmp = tempfile::tempdir().unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = events.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let vad_calls = Arc::new(AtomicUsize::new(0));
        let deps = Deps {
            transcriber: Box::new(FakeTranscriber { text: Arc::new(|_, _| "Hallo.".into()), delay: Duration::ZERO }),
            vad: Arc::new(move || {
                if vad_calls.fetch_add(1, Ordering::SeqCst) < vad_fails {
                    Err("vad busy".into())
                } else {
                    Ok(Box::new(EnergyVad::new(0.01)) as Box<dyn Vad>)
                }
            }),
            capture: Arc::new(Scripted { acts, calls: calls.clone(), audio: utterance(1.0) }),
            fallback_root: tmp.path().join("fallback"),
            threads: 1,
            headphones: None,
            timing,
        };
        let mut c = cfg(tmp.path().join("root"));
        c.sources = vec![Source::Mic];
        let p = Pipeline::start(c, Arc::new(move |e| ev.lock().unwrap().push(e)), deps).unwrap();
        // Every script ends in `Speak`: wait at least `wait`, then until its
        // transcript arrived (a loaded test machine needs longer than 600 ms
        // for several retries — a fixed sleep made these tests flaky).
        let started = std::time::Instant::now();
        let deadline = wait * 8;
        loop {
            std::thread::sleep(Duration::from_millis(20));
            let done = events.lock().unwrap().iter().any(|e| matches!(e, Event::Final { .. }));
            let elapsed = started.elapsed();
            if (done && elapsed >= wait) || elapsed >= deadline {
                break;
            }
        }
        p.stop();
        let out = events.lock().unwrap().clone();
        (out, calls.load(Ordering::SeqCst))
    }

    /// The last state, ignoring the "no audio" restarts that follow once a
    /// script's audio is used up (their timing races the test's stop).
    fn settled(st: &[(SourceStatus, Option<String>)]) -> SourceStatus {
        st.iter()
            .rev()
            .find(|(_, d)| !d.as_deref().unwrap_or("").contains("no audio"))
            .map(|(s, _)| *s)
            .expect("at least one state")
    }

    fn mic_states(ev: &[Event]) -> Vec<(SourceStatus, Option<String>)> {
        ev.iter()
            .filter_map(|e| match e {
                Event::SourceState { source: Source::Mic, state, detail } => Some((*state, detail.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_language_can_change_while_the_session_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let seen = Arc::new(Mutex::new(Vec::<Lang>::new()));
        let s2 = seen.clone();
        struct Rec(Arc<Mutex<Vec<Lang>>>);
        impl Transcriber for Rec {
            fn decode(&mut self, _: Source, _: &[f32], o: &DecodeOpts) -> Result<Decoded, String> {
                if o.kind == JobKind::Final {
                    self.0.lock().unwrap().push(o.lang);
                }
                Ok(Decoded { text: "Satz".into(), lang: "de".into(), confidence: 0.9, mean_logprob: -0.1, no_speech: 0.0, rejected: vec![] })
            }
        }
        // Two utterances, 2 s apart, delivered by a thread so the switch can
        // land between them.
        struct Later(Vec<f32>);
        impl CaptureFactory for Later {
            fn start(&self, _: Source, _: Option<&str>, mut chunk: Chunk, _: OnError) -> Result<Box<dyn AudioInput>, String> {
                let d = self.0.clone();
                std::thread::spawn(move || {
                    for (i, c) in d.chunks(1600).enumerate() {
                        chunk(c);
                        if i % 10 == 9 {
                            std::thread::sleep(Duration::from_millis(30));
                        }
                    }
                });
                Ok(Box::new(FakeInput))
            }
        }
        let mut audio = utterance(1.0);
        audio.extend(vec![0.0; RATE * 2]);
        audio.extend(utterance(1.0));
        let deps = Deps {
            transcriber: Box::new(Rec(s2)),
            vad: Arc::new(|| Ok(Box::new(EnergyVad::new(0.01)) as Box<dyn Vad>)),
            capture: Arc::new(Later(audio)),
            fallback_root: tmp.path().join("fallback"),
            threads: 1,
            headphones: None,
            timing: Timing { no_data: None, ..Timing::default() },
        };
        let mut c = cfg(tmp.path().join("root"));
        c.sources = vec![Source::Mic];
        c.lang = Lang::De;
        let p = Pipeline::start(c, Arc::new(|_| {}), deps).unwrap();
        let t0 = Instant::now();
        while seen.lock().unwrap().is_empty() && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        p.set_lang(Lang::En);
        let t0 = Instant::now();
        while seen.lock().unwrap().len() < 2 && t0.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = p.stop();
        let got = seen.lock().unwrap().clone();
        assert_eq!(got.first(), Some(&Lang::De), "{got:?}");
        assert_eq!(got.last(), Some(&Lang::En), "the switch applies to the next segment: {got:?}");
    }

    #[test]
    fn retries_start_over_after_every_successful_restart() {
        use Act::*;
        // Three short outages, one failed attempt each: three in total, over a
        // budget of two — fine only if a successful restart resets the count.
        let acts = vec![Drop, Fail, Drop, Fail, Drop, Fail, Speak];
        let (ev, calls) = run_scripted(acts, Timing { slow_retry: Duration::from_secs(60), no_data: None, ..fast() }, 0, Duration::from_millis(600));
        let st = mic_states(&ev);
        assert!(st.iter().all(|(_, d)| !d.as_deref().unwrap_or("").contains("gave up")), "{st:?}");
        assert_eq!(settled(&st), SourceStatus::Running, "{st:?}");
        assert_eq!(calls, 7);
    }

    #[test]
    fn a_source_that_gave_up_still_comes_back() {
        use Act::*;
        // The mic is unplugged for a long time, then plugged in again.
        let acts = vec![Fail, Fail, Fail, Fail, Fail, Speak];
        let (ev, _) = run_scripted(acts, fast(), 0, Duration::from_millis(600));
        let st = mic_states(&ev);
        assert!(st.iter().any(|(_, d)| d.as_deref().unwrap_or("").contains("gave up")), "{st:?}");
        assert_eq!(settled(&st), SourceStatus::Running, "{st:?}");
    }

    #[test]
    fn a_capture_that_goes_silent_is_restarted() {
        use Act::*;
        let (ev, calls) = run_scripted(vec![Silent, Speak], fast(), 0, Duration::from_millis(600));
        let st = mic_states(&ev);
        assert!(st.iter().any(|(s, d)| *s == SourceStatus::Lost && d.as_deref().unwrap_or("").contains("no audio")), "{st:?}");
        assert!(calls >= 2);
        assert!(ev.iter().any(|e| matches!(e, Event::Final { .. })), "the restarted capture is transcribed");
    }

    #[test]
    fn a_vad_that_fails_at_first_is_tried_again() {
        let (ev, _) = run_scripted(vec![Act::Speak], fast(), 2, Duration::from_millis(600));
        let st = mic_states(&ev);
        assert_eq!(settled(&st), SourceStatus::Running, "{st:?}");
        assert!(ev.iter().any(|e| matches!(e, Event::Final { .. })));
    }

    fn start(
        data: HashMap<Source, Vec<f32>>,
        fail: Option<Source>,
        text: Arc<dyn Fn(Source, JobKind) -> String + Send + Sync>,
        delay: Duration,
    ) -> (Pipeline, Run) {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("root");
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = events.clone();
        let deps = Deps {
            transcriber: Box::new(FakeTranscriber { text, delay }),
            vad: Arc::new(|| Ok(Box::new(EnergyVad::new(0.01)) as Box<dyn Vad>)),
            capture: Arc::new(FakeCapture { data, fail }),
            fallback_root: tmp.path().join("fallback"),
            threads: 1,
            headphones: None,
            timing: Timing::default(),
        };
        let p = Pipeline::start(cfg(root.clone()), Arc::new(move |e| ev.lock().unwrap().push(e)), deps).unwrap();
        (p, Run { events, _tmp: tmp, root })
    }

    fn finals(r: &Run) -> Vec<(Source, String)> {
        r.events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                Event::Final { source, text, .. } => Some((*source, text.clone())),
                _ => None,
            })
            .collect()
    }

    fn wait_for(r: &Run, f: impl Fn(&[Event]) -> bool) {
        for _ in 0..100 {
            if f(&r.events.lock().unwrap()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    /// A VAD that never hears speech — what Silero does on most of a song.
    struct DeafVad;
    impl Vad for DeafVad {
        fn probs(&mut self, samples: &[f32]) -> Result<Vec<f32>, String> {
            Ok(vec![0.0; samples.len() / crate::segment::VAD_FRAME])
        }
    }

    fn finals_with_deaf_vad(mode: Mode) -> usize {
        let tmp = tempfile::tempdir().unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = events.clone();
        let text: Arc<dyn Fn(Source, JobKind) -> String + Send + Sync> = Arc::new(|_, _| "la la la".into());
        let deps = Deps {
            transcriber: Box::new(FakeTranscriber { text, delay: Duration::ZERO }),
            vad: Arc::new(|| Ok(Box::new(DeafVad) as Box<dyn Vad>)),
            capture: Arc::new(FakeCapture { data: HashMap::from([(Source::Mic, utterance(2.0))]), fail: None }),
            fallback_root: tmp.path().join("fallback"),
            threads: 1,
            headphones: None,
            timing: Timing::default(),
        };
        let mut c = cfg(tmp.path().join("root"));
        c.sources = vec![Source::Mic];
        c.mode = mode;
        let p = Pipeline::start(c, Arc::new(move |e| ev.lock().unwrap().push(e)), deps).unwrap();
        std::thread::sleep(Duration::from_millis(500));
        p.stop();
        let n = events.lock().unwrap().iter().filter(|e| matches!(e, Event::Final { .. })).count();
        n
    }

    #[test]
    fn music_is_transcribed_even_when_the_vad_hears_no_speech() {
        assert_eq!(finals_with_deaf_vad(Mode::Room), 1);
        assert_eq!(finals_with_deaf_vad(Mode::Song), 1);
        assert_eq!(finals_with_deaf_vad(Mode::Meeting), 0, "speech modes still trust the VAD");
    }

    #[test]
    fn music_decodes_get_no_prompt() {
        assert_eq!(prompt_for(false, JobKind::Final, "so far"), "so far");
        assert_eq!(prompt_for(false, JobKind::Hypothesis, "so far"), "");
        assert_eq!(prompt_for(true, JobKind::Final, "so far"), "");
        assert!(is_music(Mode::Room) && is_music(Mode::Song) && !is_music(Mode::Dictation));
    }

    #[test]
    fn two_sources_are_labelled_separately() {
        let data = HashMap::from([(Source::Mic, utterance(1.0)), (Source::Sys, utterance(1.0))]);
        let text: Arc<dyn Fn(Source, JobKind) -> String + Send + Sync> = Arc::new(|s, _| match s {
            Source::Mic => "Ich habe eine Frage".into(),
            Source::Sys => "Bitte sehr, fragen Sie".into(),
        });
        let (p, r) = start(data, None, text, Duration::ZERO);
        wait_for(&r, |e| e.iter().filter(|e| matches!(e, Event::Final { .. })).count() >= 2);
        let dir = p.stop().unwrap();
        let mut f = finals(&r);
        f.sort_by_key(|x| x.0);
        assert_eq!(f, vec![(Source::Mic, "Ich habe eine Frage".into()), (Source::Sys, "Bitte sehr, fragen Sie".into())]);
        let lines = read_lines(&dir);
        assert_eq!(lines.len(), 2);
        assert!(dir.starts_with(&r.root));
        assert_eq!(crate::protocol::session::read_meta(&dir).unwrap().status, SessionStatus::Completed);
    }

    #[test]
    fn echo_is_not_written_twice() {
        let data = HashMap::from([(Source::Mic, utterance(1.0)), (Source::Sys, utterance(1.0))]);
        let text: Arc<dyn Fn(Source, JobKind) -> String + Send + Sync> =
            Arc::new(|_, _| "Wir treffen uns um drei".into());
        let (p, r) = start(data, None, text, Duration::ZERO);
        wait_for(&r, |e| e.iter().any(|e| matches!(e, Event::Dropped { source: Source::Mic, .. })));
        let dir = p.stop().unwrap();
        assert_eq!(finals(&r), vec![(Source::Sys, "Wir treffen uns um drei".into())]);
        assert_eq!(read_lines(&dir).len(), 1);
        assert!(r.events.lock().unwrap().iter().any(|e| matches!(e, Event::Dropped { source: Source::Mic, .. })));
    }

    #[test]
    fn hypotheses_never_reach_the_file() {
        // A long utterance yields previews while it is open.
        let mut long = utterance(3.0);
        long.truncate(RATE * 3);
        let data = HashMap::from([(Source::Mic, long)]);
        let text: Arc<dyn Fn(Source, JobKind) -> String + Send + Sync> = Arc::new(|_, k| match k {
            JobKind::Hypothesis => "vorläufig".into(),
            JobKind::Final => "endgültig".into(),
        });
        let (p, r) = start(data, None, text, Duration::ZERO);
        wait_for(&r, |e| e.iter().any(|e| matches!(e, Event::Hypothesis { .. })));
        let dir = p.stop().unwrap();
        assert!(r.events.lock().unwrap().iter().any(|e| matches!(e, Event::Hypothesis { .. })));
        let lines = read_lines(&dir);
        assert_eq!(lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), vec!["endgültig"]);
    }

    #[test]
    fn lost_source_does_not_stop_the_other() {
        let data = HashMap::from([(Source::Sys, utterance(1.0))]);
        let text: Arc<dyn Fn(Source, JobKind) -> String + Send + Sync> = Arc::new(|_, _| "System spricht".into());
        let (p, r) = start(data, Some(Source::Mic), text, Duration::ZERO);
        wait_for(&r, |e| e.iter().any(|e| matches!(e, Event::Final { .. })));
        p.stop();
        let ev = r.events.lock().unwrap();
        assert!(ev.iter().any(|e| matches!(e, Event::SourceState { source: Source::Mic, state: SourceStatus::Lost, .. })));
        drop(ev);
        assert_eq!(finals(&r), vec![(Source::Sys, "System spricht".into())]);
    }

    #[test]
    fn stop_flushes_open_segment_and_completes_session() {
        let mut open = utterance(2.0);
        open.truncate(RATE * 2); // no closing silence
        let data = HashMap::from([(Source::Mic, open)]);
        let text: Arc<dyn Fn(Source, JobKind) -> String + Send + Sync> = Arc::new(|_, _| "letzter Satz".into());
        let (p, r) = start(data, None, text, Duration::ZERO);
        std::thread::sleep(Duration::from_millis(100));
        let dir = p.stop().unwrap();
        assert_eq!(finals(&r), vec![(Source::Mic, "letzter Satz".into())]);
        assert!(matches!(r.events.lock().unwrap().last(), Some(Event::Stopped { error: None, .. })));
        assert_eq!(crate::protocol::session::read_meta(&dir).unwrap().status, SessionStatus::Completed);
    }

    #[test]
    fn mic_line_is_not_delayed_while_sys_is_silent() {
        let data = HashMap::from([(Source::Mic, utterance(1.0))]);
        let text: Arc<dyn Fn(Source, JobKind) -> String + Send + Sync> = Arc::new(|_, _| "Nur ich".into());
        let (p, r) = start(data, None, text, Duration::ZERO);
        let t = Instant::now();
        wait_for(&r, |e| e.iter().any(|e| matches!(e, Event::Final { .. })));
        assert!(t.elapsed() < Duration::from_millis(1500), "{:?}", t.elapsed());
        assert_eq!(finals(&r), vec![(Source::Mic, "Nur ich".into())]);
        p.stop();
    }

    #[test]
    fn fatal_writer_error_stops_the_session() {
        let data = HashMap::from([(Source::Mic, utterance(1.0))]);
        let text: Arc<dyn Fn(Source, JobKind) -> String + Send + Sync> = Arc::new(|_, _| "Satz".into());
        let (p, r) = start(data, None, text, Duration::from_millis(300));
        // Break both the session folder and the fallback root.
        std::fs::remove_dir_all(p.dir().unwrap()).unwrap();
        std::fs::write(r._tmp.path().join("fallback"), b"not a folder").unwrap();
        wait_for(&r, |e| e.iter().any(|e| matches!(e, Event::Stopped { .. })));
        assert!(!p.is_running());
        p.stop();
        let ev = r.events.lock().unwrap();
        assert!(ev.iter().any(|e| matches!(e, Event::Stopped { error: Some(_), .. })), "{ev:?}");
    }
}
