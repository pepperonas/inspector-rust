//! The command loop of `ir-transcribe`: one JSON command per stdin line, one
//! JSON event per stdout line. The model is loaded on the first session and
//! kept until the process ends; one session (or file job) at a time.

use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use transcribe_core::protocol::{encode, decode_command, Command, Event, SessionConfig};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const ERR_BUSY: &str = "transcribe.busy";
/// A file or words job whose thread panicked (the job still ends).
pub const JOB_PANICKED: &str = "transcribe.job_crashed";

pub type Emit = Arc<dyn Fn(Event) + Send + Sync>;

/// A file transcription running on its own thread.
pub struct FileJob {
    pub cancel: Arc<AtomicBool>,
    pub done: Arc<AtomicBool>,
}

impl FileJob {
    pub fn running(&self) -> bool {
        !self.done.load(Ordering::SeqCst)
    }
}

/// Write events from any thread, one line each, in order.
pub fn event_writer(out: impl Write + Send + 'static) -> (Emit, std::thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<Event>();
    let h = std::thread::Builder::new()
        .name("ir-transcribe-out".into())
        .spawn(move || {
            let mut out = out;
            for ev in rx {
                if out.write_all(encode(&ev).as_bytes()).and_then(|_| out.flush()).is_err() {
                    return;
                }
            }
        })
        .expect("spawn output thread");
    let tx = Mutex::new(tx);
    (Arc::new(move |e| {
        let _ = tx.lock().unwrap_or_else(|p| p.into_inner()).send(e);
    }), h)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod engine_side {
    use super::*;
    use transcribe_core::engine::{Engine, WhisperTranscriber};
    use transcribe_core::model;
    use transcribe_core::pipeline::{Deps, Pipeline};

    pub struct Session {
        pub pipeline: Pipeline,
        _activity: Option<transcribe_core::activity_macos::Activity>,
    }

    #[derive(Default)]
    pub struct Engines {
        loaded: Option<(String, Arc<Engine>)>,
    }

    impl Engines {
        pub fn get(&mut self, id: &str) -> Result<Arc<Engine>, String> {
            if let Some((loaded_id, e)) = &self.loaded {
                if loaded_id == id {
                    return Ok(e.clone());
                }
            }
            let info = model::find(id).ok_or(model::ERR_UNKNOWN)?;
            if !model::is_installed(info) {
                return Err(model::ERR_MISSING.into());
            }
            self.loaded = None; // free the old model before loading the next
            let e = Arc::new(Engine::load(&model::path_of(info))?);
            self.loaded = Some((id.to_string(), e.clone()));
            Ok(e)
        }
    }

    pub fn fallback_root() -> std::path::PathBuf {
        dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("InspectorRust").join("Transkripte")
    }

    pub fn start(cfg: SessionConfig, engines: &mut Engines, emit: Emit) -> Result<Session, String> {
        let engine = engines.get(&cfg.model)?;
        let deps = Deps {
            transcriber: Box::new(WhisperTranscriber::new(engine)),
            vad: Arc::new(|| {
                transcribe_core::vad::SileroVad::new().map(|v| Box::new(v) as Box<dyn transcribe_core::vad::Vad>)
            }),
            capture: Arc::new(transcribe_core::capture::SystemCapture),
            fallback_root: fallback_root(),
            threads: 4,
            headphones: transcribe_core::capture::tap_macos::headphones_hint(),
            timing: transcribe_core::pipeline::Timing::default(),
        };
        let pipeline = Pipeline::start(cfg, emit, deps)?;
        Ok(Session { pipeline, _activity: transcribe_core::activity_macos::Activity::begin("Transkription läuft") })
    }

    pub fn devices() -> Vec<String> {
        transcribe_core::capture::mic::list_input_devices()
    }

    /// Transcribe one file on its own thread; `FileProgress`… then `FileDone`
    /// or `FileError`.
    pub fn transcribe_file(
        path: std::path::PathBuf,
        lang: transcribe_core::protocol::Lang,
        model_id: &str,
        engines: &mut Engines,
        emit: Emit,
    ) -> Result<FileJob, String> {
        use transcribe_core::file;
        let engine = engines.get(model_id)?;
        let cancel = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let (c2, d2) = (cancel.clone(), done.clone());
        std::thread::Builder::new()
            .name("ir-transcribe-file".into())
            .spawn(move || {
                let _activity = transcribe_core::activity_macos::Activity::begin("Datei wird transkribiert");
                let run = || -> Result<(std::path::PathBuf, Option<String>), String> {
                    let mut src = file::open_audio(&path)?;
                    let mut vad = transcribe_core::vad::SileroVad::new()?;
                    let mut tr = WhisperTranscriber::new(engine);
                    let e2 = emit.clone();
                    emit(Event::FileProgress { done_ms: 0, total_ms: src.total_ms() });
                    let text = file::transcribe_stream(src.as_mut(), &mut vad, &mut tr, lang, 4, &c2, move |d, t| {
                        e2(Event::FileProgress { done_ms: d, total_ms: t })
                    })?;
                    Ok((file::write_txt(&path, &text.text)?, text.lang))
                };
                // A panic must still end the job: `done` unset left every later
                // Start / TranscribeFile answering "busy" until the app quit.
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|_| Err(JOB_PANICKED.into())) {
                    Ok((txt, lang)) => emit(Event::FileDone { txt, lang }),
                    Err(error) => emit(Event::FileError { error }),
                }
                d2.store(true, Ordering::SeqCst);
            })
            .map_err(|e| e.to_string())?;
        Ok(FileJob { cancel, done })
    }

    /// Words with times for one file on its own thread: `FileProgress`… then
    /// `FileWords` or `FileError`. A DTW-capable context is loaded for the
    /// job (flash attention off) and freed when it ends.
    pub fn transcribe_words(
        path: std::path::PathBuf,
        lang: transcribe_core::protocol::Lang,
        model_id: &str,
        emit: Emit,
    ) -> Result<FileJob, String> {
        use transcribe_core::file;
        let info = model::find(model_id).ok_or(model::ERR_UNKNOWN)?;
        let preset = transcribe_core::words::dtw_preset_for(info.id).ok_or(transcribe_core::words::ERR_NO_DTW)?;
        if !model::is_installed(info) {
            return Err(model::ERR_MISSING.into());
        }
        let model_path = model::path_of(info);
        let cancel = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let (c2, d2) = (cancel.clone(), done.clone());
        std::thread::Builder::new()
            .name("ir-transcribe-words".into())
            .spawn(move || {
                let _activity = transcribe_core::activity_macos::Activity::begin("Wortzeiten werden ermittelt");
                let run = || -> Result<file::FileWords, String> {
                    let mut src = file::open_audio(&path)?;
                    emit(Event::FileProgress { done_ms: 0, total_ms: src.total_ms() });
                    let engine = Engine::load_dtw(&model_path, preset)?;
                    let mut stream = engine.new_stream()?;
                    let mut vad = transcribe_core::vad::SileroVad::new()?;
                    let e2 = emit.clone();
                    file::transcribe_words_stream(src.as_mut(), &mut vad, &mut stream, lang, 4, &c2, move |d, t| {
                        e2(Event::FileProgress { done_ms: d, total_ms: t })
                    })
                };
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|_| Err(JOB_PANICKED.into())) {
                    Ok(r) => emit(Event::FileWords { words: r.words, lang: r.lang }),
                    Err(error) => emit(Event::FileError { error }),
                }
                d2.store(true, Ordering::SeqCst);
            })
            .map_err(|e| e.to_string())?;
        Ok(FileJob { cancel, done })
    }

    pub fn download(id: &str, cancel: Arc<AtomicBool>, emit: Emit) {
        let Some(info) = model::find(id) else {
            emit(Event::ModelError { model: id.into(), error: model::ERR_UNKNOWN.into() });
            return;
        };
        let id = id.to_string();
        std::thread::spawn(move || {
            let e2 = emit.clone();
            let idc = id.clone();
            match model::download(info, &cancel, move |done, total| {
                e2(Event::ModelProgress { model: idc.clone(), done, total })
            }) {
                Ok(_) => emit(Event::ModelReady { model: id }),
                Err(e) => emit(Event::ModelError { model: id, error: e }),
            }
        });
    }
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
mod engine_side {
    use super::*;
    pub struct Session {
        pub pipeline: transcribe_core::pipeline::Pipeline,
    }
    #[derive(Default)]
    pub struct Engines;
    pub fn start(_: SessionConfig, _: &mut Engines, _: Emit) -> Result<Session, String> {
        Err("transcribe.unsupported".into())
    }
    pub fn devices() -> Vec<String> {
        Vec::new()
    }
    pub fn download(id: &str, _: Arc<AtomicBool>, emit: Emit) {
        emit(Event::ModelError { model: id.into(), error: "transcribe.unsupported".into() });
    }
    pub fn transcribe_file(
        _: std::path::PathBuf,
        _: transcribe_core::protocol::Lang,
        _: &str,
        _: &mut Engines,
        _: Emit,
    ) -> Result<FileJob, String> {
        Err("transcribe.unsupported".into())
    }
    pub fn transcribe_words(
        _: std::path::PathBuf,
        _: transcribe_core::protocol::Lang,
        _: &str,
        _: Emit,
    ) -> Result<FileJob, String> {
        Err("transcribe.unsupported".into())
    }
}

pub use engine_side::*;

fn b64_i16(samples: &[i16]) -> String {
    use base64::Engine as _;
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn stop_session(session: &mut Option<Session>) {
    if let Some(s) = session.take() {
        let _ = s.pipeline.stop();
    }
}

/// Run until stdin closes or `Shutdown`.
pub fn serve(input: impl BufRead, emit: Emit) {
    emit(Event::Ready { version: VERSION.into() });
    let mut engines = Engines::default();
    let mut session: Option<Session> = None;
    let mut file: Option<FileJob> = None;
    let download_cancel = Arc::new(AtomicBool::new(false));

    for line in input.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let cmd = match decode_command(&line) {
            Ok(c) => c,
            Err(e) => {
                emit(Event::Error { error: format!("bad command: {e}") });
                continue;
            }
        };
        // A session that stopped on its own (fatal writer error) is gone.
        if session.as_ref().is_some_and(|s| !s.pipeline.is_running()) {
            stop_session(&mut session);
        }
        if file.as_ref().is_some_and(|f| !f.running()) {
            file = None;
        }
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match cmd {
            Command::Start(cfg) => {
                if session.is_some() || file.is_some() {
                    emit(Event::Error { error: ERR_BUSY.into() });
                    return true;
                }
                match start(cfg, &mut engines, emit.clone()) {
                    Ok(s) => session = Some(s),
                    Err(e) => emit(Event::Error { error: e }),
                }
                true
            }
            Command::Stop => {
                if session.is_none() {
                    emit(Event::Stopped { session_dir: None, error: None });
                }
                stop_session(&mut session);
                true
            }
            Command::SetLang { lang } => {
                if let Some(s) = &session {
                    s.pipeline.set_lang(lang);
                }
                true
            }
            Command::SetSource { source, on } => {
                if let Some(s) = &session {
                    s.pipeline.set_source(source, on);
                }
                true
            }
            Command::Window { source, secs } => {
                let pcm = session.as_ref().map(|s| s.pipeline.window(source, secs)).unwrap_or_default();
                emit(Event::Window { source, pcm_b64: b64_i16(&pcm) });
                true
            }
            Command::DownloadModel { model } => {
                download_cancel.store(false, Ordering::SeqCst);
                download(&model, download_cancel.clone(), emit.clone());
                true
            }
            Command::CancelDownload => {
                download_cancel.store(true, Ordering::SeqCst);
                true
            }
            Command::TranscribeFile { path, lang, model } => {
                if session.is_some() || file.is_some() {
                    emit(Event::FileError { error: ERR_BUSY.into() });
                    return true;
                }
                match transcribe_file(path, lang, &model, &mut engines, emit.clone()) {
                    Ok(j) => file = Some(j),
                    Err(error) => emit(Event::FileError { error }),
                }
                true
            }
            Command::TranscribeWords { path, lang, model } => {
                if session.is_some() || file.is_some() {
                    emit(Event::FileError { error: ERR_BUSY.into() });
                    return true;
                }
                match transcribe_words(path, lang, &model, emit.clone()) {
                    Ok(j) => file = Some(j),
                    Err(error) => emit(Event::FileError { error }),
                }
                true
            }
            Command::CancelFile => {
                if let Some(f) = &file {
                    f.cancel.store(true, Ordering::SeqCst);
                }
                true
            }
            Command::ListDevices => {
                emit(Event::Devices { mic: devices() });
                true
            }
            Command::Shutdown => false,
        }));
        match outcome {
            Ok(true) => {}
            Ok(false) => break,
            Err(_) => emit(Event::Error { error: "transcribe.internal".into() }),
        }
    }
    download_cancel.store(true, Ordering::SeqCst);
    if let Some(f) = &file {
        f.cancel.store(true, Ordering::SeqCst);
    }
    stop_session(&mut session);
}
