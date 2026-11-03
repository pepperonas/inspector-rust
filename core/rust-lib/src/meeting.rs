//! Live transcription of microphone + system audio (`meeting` command):
//! config, the state the panel and ticker render, and the Tauri glue.
//! The work itself runs in the `ir-transcribe` helper (`transcribe_client`).

use crate::db::DbHandle;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use transcribe_protocol::models;
use transcribe_protocol::session::{self, SessionSummary};
use transcribe_protocol::{Command, DedupeMode, Event, Lang, Mode, SessionConfig, Source, SourceStatus};

pub const EVENT_CHANGED: &str = "meeting-changed";
pub const EVENT_LINE: &str = "meeting-line";
pub const EVENT_HYPOTHESIS: &str = "meeting-hypothesis";
pub const EVENT_DROPPED: &str = "meeting-dropped";
pub const EVENT_TICKER_CONFIG: &str = "meeting-ticker-config";
pub const ERR_BUSY: &str = "transcribe.busy";
pub const ERR_MIC_DENIED: &str = "transcribe.mic_denied";
pub const MAX_LINES: usize = 200;

const KEY_CONFIG: &str = "meeting.config";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TickerConfig {
    pub visible: bool,
    /// `"top"` or `"bottom"`.
    pub position: String,
    /// Monitor name; `None` = the one under the cursor.
    pub display: Option<String>,
    pub width_pct: u8,
    pub font_px: u8,
    pub opacity: f32,
    pub speed_px_s: u16,
}

impl Default for TickerConfig {
    fn default() -> Self {
        TickerConfig {
            visible: true,
            position: "bottom".into(),
            display: None,
            width_pct: 70,
            font_px: 15,
            opacity: 0.85,
            speed_px_s: 60,
        }
    }
}

impl TickerConfig {
    pub fn normalized(mut self) -> Self {
        if self.position != "top" {
            self.position = "bottom".into();
        }
        self.width_pct = self.width_pct.clamp(30, 100);
        self.font_px = self.font_px.clamp(11, 28);
        self.opacity = if self.opacity.is_finite() { self.opacity.clamp(0.3, 1.0) } else { 0.85 };
        self.speed_px_s = self.speed_px_s.clamp(20, 200);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeetingConfig {
    pub mic: bool,
    pub sys: bool,
    pub mic_device: Option<String>,
    pub model: String,
    pub lang: Lang,
    /// Folder the session folders go into; `None` = `~/Documents/Transkripte`.
    pub root: Option<PathBuf>,
    pub keep_audio: bool,
    pub audio_days: u32,
    pub dedupe: DedupeMode,
    pub dedupe_threshold: f32,
    pub ticker: TickerConfig,
}

impl Default for MeetingConfig {
    fn default() -> Self {
        MeetingConfig {
            mic: true,
            sys: true,
            mic_device: None,
            model: models::DEFAULT_MODEL.into(),
            lang: Lang::Auto,
            root: None,
            keep_audio: true,
            audio_days: 7,
            dedupe: DedupeMode::Auto,
            dedupe_threshold: 0.75,
            ticker: TickerConfig::default(),
        }
    }
}

pub fn default_root() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("Transkripte")
}

pub fn fallback_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("InspectorRust")
        .join("Transkripte")
}

impl MeetingConfig {
    pub fn normalized(mut self) -> Self {
        if models::find(&self.model).is_none() {
            self.model = models::DEFAULT_MODEL.into();
        }
        self.dedupe_threshold = if self.dedupe_threshold.is_finite() { self.dedupe_threshold.clamp(0.5, 0.95) } else { 0.75 };
        self.audio_days = self.audio_days.clamp(1, 365);
        if self.root.as_ref().is_some_and(|p| p.as_os_str().is_empty()) {
            self.root = None;
        }
        self.mic_device = self.mic_device.filter(|d| !d.trim().is_empty());
        self.ticker = self.ticker.normalized();
        self
    }

    pub fn root(&self) -> PathBuf {
        self.root.clone().unwrap_or_else(default_root)
    }

    pub fn sources(&self) -> Vec<Source> {
        let mut v = Vec::new();
        if self.mic {
            v.push(Source::Mic);
        }
        if self.sys {
            v.push(Source::Sys);
        }
        v
    }

    pub fn load(db: &DbHandle) -> Self {
        crate::settings::get(db, KEY_CONFIG)
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<MeetingConfig>(&s).ok())
            .unwrap_or_default()
            .normalized()
    }

    pub fn save(&self, db: &DbHandle) -> Result<(), String> {
        let s = serde_json::to_string(self).map_err(|e| e.to_string())?;
        crate::settings::set(db, KEY_CONFIG, &s).map_err(|e| e.to_string())
    }

    pub fn session_config(&self, mode: Mode, sources: Vec<Source>) -> SessionConfig {
        SessionConfig {
            sources,
            mic_device: self.mic_device.clone(),
            model: self.model.clone(),
            lang: self.lang,
            root: self.root(),
            keep_audio: self.keep_audio,
            dedupe: self.dedupe,
            dedupe_threshold: self.dedupe_threshold,
            mode,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LineView {
    pub source: Source,
    pub seq: u64,
    pub start_ms: u64,
    pub lang: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceView {
    pub state: SourceStatus,
    pub detail: Option<String>,
    pub level: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hypothesis {
    pub stable: String,
    pub pending: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DownloadView {
    pub model: String,
    pub done: u64,
    pub total: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct MeetingState {
    pub running: bool,
    pub starting: bool,
    pub dir: Option<PathBuf>,
    pub started_ms: Option<u64>,
    pub sources: BTreeMap<Source, SourceView>,
    pub lines: VecDeque<LineView>,
    pub hypotheses: BTreeMap<Source, Hypothesis>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    pub line_count: u64,
    pub download: Option<DownloadView>,
    pub recovered: Vec<PathBuf>,
}

impl MeetingState {
    /// Fold one helper event into the state. Pure.
    pub fn apply(&mut self, ev: &Event, now_ms: u64) {
        match ev {
            Event::Started { session_dir, sources } => {
                self.running = true;
                self.starting = false;
                self.dir = Some(session_dir.clone());
                self.started_ms = Some(now_ms);
                self.lines.clear();
                self.hypotheses.clear();
                self.warnings.clear();
                self.error = None;
                self.line_count = 0;
                self.sources.clear();
                for s in sources {
                    self.sources.insert(*s, SourceView { state: SourceStatus::Starting, detail: None, level: 0.0 });
                }
            }
            Event::SourceState { source, state, detail } => {
                let v = self.sources.entry(*source).or_insert(SourceView { state: *state, detail: None, level: 0.0 });
                v.state = *state;
                v.detail = detail.clone();
                if *state != SourceStatus::Running {
                    v.level = 0.0;
                }
            }
            Event::Level { source, rms } => {
                if let Some(v) = self.sources.get_mut(source) {
                    v.level = *rms;
                }
            }
            Event::Hypothesis { source, stable, pending, .. } => {
                self.hypotheses.insert(*source, Hypothesis { stable: stable.clone(), pending: pending.clone() });
            }
            Event::Final { source, seq, start_ms, lang, text, .. } => {
                self.hypotheses.remove(source);
                self.line_count += 1;
                self.lines.push_back(LineView { source: *source, seq: *seq, start_ms: *start_ms, lang: lang.clone(), text: text.clone() });
                while self.lines.len() > MAX_LINES {
                    self.lines.pop_front();
                }
            }
            Event::Dropped { source, .. } => {
                self.hypotheses.remove(source);
            }
            Event::Warning { detail, .. } => {
                if !self.warnings.contains(detail) {
                    self.warnings.push(detail.clone());
                }
            }
            Event::Stopped { session_dir, error } => {
                self.running = false;
                self.starting = false;
                if session_dir.is_some() {
                    self.dir = session_dir.clone();
                }
                self.error = error.clone();
                self.hypotheses.clear();
                for v in self.sources.values_mut() {
                    v.level = 0.0;
                }
            }
            Event::Error { error } if self.starting => {
                self.starting = false;
                self.error = Some(error.clone());
            }
            Event::ModelProgress { model, done, total } => {
                self.download = Some(DownloadView { model: model.clone(), done: *done, total: *total, error: None });
            }
            Event::ModelReady { .. } => self.download = None,
            Event::ModelError { model, error } => {
                self.download = Some(DownloadView { model: model.clone(), done: 0, total: 0, error: Some(error.clone()) });
            }
            _ => {}
        }
    }
}

static STATE: Mutex<Option<MeetingState>> = Mutex::new(None);
static APP: OnceLock<AppHandle> = OnceLock::new();
static SUBSCRIBED: OnceLock<u64> = OnceLock::new();
static LAST_EMIT: Mutex<Option<Instant>> = Mutex::new(None);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn status() -> MeetingState {
    STATE.lock().clone().unwrap_or_default()
}

pub fn is_running() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.running || s.starting)
}

fn update(f: impl FnOnce(&mut MeetingState)) -> MeetingState {
    let mut g = STATE.lock();
    let s = g.get_or_insert_with(MeetingState::default);
    f(s);
    s.clone()
}

fn emit_changed(app: &AppHandle, state: &MeetingState, force: bool) {
    let mut last = LAST_EMIT.lock();
    if !force && last.is_some_and(|t| t.elapsed() < Duration::from_millis(100)) {
        return;
    }
    *last = Some(Instant::now());
    let _ = app.emit(EVENT_CHANGED, state);
}

fn on_event(ev: &Event) {
    // Only events of a meeting session (dictation mode subscribes on its own).
    let ours = is_running() || matches!(ev, Event::ModelProgress { .. } | Event::ModelReady { .. } | Event::ModelError { .. });
    if !ours {
        return;
    }
    let was_running = is_running();
    let state = update(|s| s.apply(ev, now_ms()));
    let Some(app) = APP.get() else { return };
    match ev {
        Event::Final { .. } => {
            let _ = app.emit(EVENT_LINE, ev);
        }
        Event::Hypothesis { .. } => {
            let _ = app.emit(EVENT_HYPOTHESIS, ev);
        }
        Event::Dropped { .. } => {
            let _ = app.emit(EVENT_DROPPED, ev);
        }
        _ => {}
    }
    let force = !matches!(ev, Event::Level { .. });
    emit_changed(app, &state, force);
    if was_running && !state.running {
        crate::meeting_ui::on_session_end(app, &state);
    }
    if matches!(ev, Event::Started { .. }) {
        crate::meeting_ui::on_session_start(app, &state);
    }
    if matches!(ev, Event::Warning { .. }) {
        crate::meeting_ui::on_warning(app, &state);
    }
}

fn ensure_subscribed() {
    SUBSCRIBED.get_or_init(|| crate::transcribe_client::subscribe(on_event));
}

/// `AVCaptureDevice` authorization for audio: 0 undetermined, 1 restricted,
/// 2 denied, 3 authorized.
#[cfg(target_os = "macos")]
pub fn mic_authorization() -> i64 {
    use objc2::{class, msg_send};
    let media = objc2_foundation::NSString::from_str("soun");
    // SAFETY: documented AVFoundation class method.
    unsafe {
        msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: &*media]
    }
}

#[cfg(not(target_os = "macos"))]
pub fn mic_authorization() -> i64 {
    3
}

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    ensure_subscribed();
}

/// Mark sessions of a previous run that never ended (`kill -9`, crash) as
/// aborted and tell the user. Run once at start, off the main thread.
pub fn recover_at_start(app: &AppHandle, db: &DbHandle) {
    let cfg = MeetingConfig::load(db);
    let mut found = Vec::new();
    for root in [cfg.root(), fallback_root()] {
        found.extend(session::recover(&root));
    }
    if found.is_empty() {
        return;
    }
    update(|s| s.recovered = found.iter().map(|r| r.dir.clone()).collect());
    let until = found
        .iter()
        .filter_map(|r| r.ended_ms)
        .max()
        .map(|ms| {
            chrono::DateTime::from_timestamp_millis(ms as i64)
                .unwrap_or_default()
                .with_timezone(&chrono::Local)
                .format("%H:%M")
                .to_string()
        })
        .unwrap_or_default();
    let n = found.len().to_string();
    crate::status_toast::show_passive(
        app,
        crate::status_toast::StatusToast {
            kind: "transcribe".into(),
            on: false,
            title: crate::i18n::tf("meeting.recovered_title", &[("n", &n)]),
            subtitle: crate::i18n::tf("meeting.recovered_sub", &[("time", &until)]),
        },
    );
}

pub fn start(db: &DbHandle, sources: Option<Vec<Source>>) -> Result<MeetingState, String> {
    if !crate::transcribe_client::supported() {
        return Err(crate::transcribe_client::ERR_UNSUPPORTED.into());
    }
    if is_running()
        || crate::transcribe_client::is_busy()
        || crate::live_transcribe::is_active()
        || crate::transcribe::status().is_some_and(|j| j.is_active())
    {
        return Err(ERR_BUSY.into());
    }
    let cfg = MeetingConfig::load(db);
    let sources = sources.unwrap_or_else(|| cfg.sources());
    if sources.is_empty() {
        return Err("meeting.no_source".into());
    }
    if sources.contains(&Source::Mic) && mic_authorization() == 2 {
        return Err(ERR_MIC_DENIED.into());
    }
    model_ready(&cfg.model)?;
    ensure_subscribed();
    let state = update(|s| {
        s.starting = true;
        s.error = None;
    });
    if let Err(e) = crate::transcribe_client::send(&Command::Start(cfg.session_config(Mode::Meeting, sources))) {
        update(|s| {
            s.starting = false;
            s.error = Some(e.clone());
        });
        return Err(e);
    }
    // Old raw audio goes on each start; transcripts are never pruned.
    let days = cfg.audio_days;
    let roots = [cfg.root(), fallback_root()];
    std::thread::spawn(move || {
        for r in roots {
            prune_audio(&r, days);
        }
    });
    Ok(state)
}

/// The model is known and downloaded (sentinels `model.unknown` / `model.missing`).
pub fn model_ready(id: &str) -> Result<(), String> {
    let info = models::find(id).ok_or(models::ERR_UNKNOWN)?;
    if !models::is_installed_at(&model_path(info), info) {
        return Err(models::ERR_MISSING.into());
    }
    Ok(())
}

/// Raw audio older than `days` (only `*/audio/*.wav`).
pub fn prune_audio(root: &std::path::Path, days: u32) {
    let limit = Duration::from_secs(u64::from(days) * 86_400);
    let Ok(sessions) = std::fs::read_dir(root) else { return };
    for s in sessions.flatten() {
        let Ok(files) = std::fs::read_dir(s.path().join(session::AUDIO_DIR)) else { continue };
        for f in files.flatten() {
            let p = f.path();
            let old = p.extension().is_some_and(|e| e == "wav")
                && f.metadata().and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).is_some_and(|a| a > limit);
            if old {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

pub fn model_path(info: &models::ModelInfo) -> PathBuf {
    models::models_dir().join(info.file)
}

pub fn stop() -> Result<(), String> {
    if !is_running() {
        return Ok(());
    }
    crate::transcribe_client::send(&Command::Stop)
}

pub fn set_source(source: Source, on: bool) -> Result<(), String> {
    update(|s| {
        if let Some(v) = s.sources.get_mut(&source) {
            v.state = if on { SourceStatus::Starting } else { SourceStatus::Off };
        }
    });
    if is_running() {
        crate::transcribe_client::send(&Command::SetSource { source, on })?;
    }
    Ok(())
}

pub fn sessions(db: &DbHandle) -> Vec<SessionSummary> {
    let cfg = MeetingConfig::load(db);
    let mut roots = vec![cfg.root(), fallback_root()];
    roots.dedup();
    session::list_sessions(&roots)
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub id: &'static str,
    pub file: &'static str,
    pub bytes: u64,
    pub installed: bool,
}

pub fn model_status() -> Vec<ModelStatus> {
    models::MODELS
        .iter()
        .map(|m| ModelStatus { id: m.id, file: m.file, bytes: m.bytes, installed: models::is_installed_at(&model_path(m), m) })
        .collect()
}

pub fn download_model(model: &str) -> Result<(), String> {
    models::find(model).ok_or(models::ERR_UNKNOWN)?;
    ensure_subscribed();
    update(|s| s.download = Some(DownloadView { model: model.into(), done: 0, total: 0, error: None }));
    crate::transcribe_client::send(&Command::DownloadModel { model: model.into() })
}

pub fn cancel_download() -> Result<(), String> {
    update(|s| s.download = None);
    crate::transcribe_client::send(&Command::CancelDownload)
}

pub fn input_devices() -> Vec<String> {
    crate::transcribe_client::request(&Command::ListDevices, Duration::from_secs(5), |e| match e {
        Event::Devices { mic } => Some(mic.clone()),
        _ => None,
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn final_ev(source: Source, seq: u64, text: &str) -> Event {
        Event::Final { source, seq, start_ms: 0, end_ms: 1, lang: "de".into(), text: text.into(), confidence: 0.9 }
    }

    #[test]
    fn apply_keeps_last_200_lines() {
        let mut s = MeetingState::default();
        s.apply(&Event::Started { session_dir: "/s".into(), sources: vec![Source::Mic] }, 0);
        for i in 0..250 {
            s.apply(&final_ev(Source::Mic, i, "x"), 0);
        }
        assert_eq!(s.lines.len(), MAX_LINES);
        assert_eq!(s.line_count, 250);
        assert_eq!(s.lines.front().unwrap().seq, 50);
    }

    #[test]
    fn hypothesis_is_replaced_by_final_of_same_source() {
        let mut s = MeetingState::default();
        s.apply(&Event::Started { session_dir: "/s".into(), sources: vec![Source::Mic, Source::Sys] }, 0);
        s.apply(&Event::Hypothesis { source: Source::Mic, seq: 1, stable: "a".into(), pending: "b".into() }, 0);
        s.apply(&Event::Hypothesis { source: Source::Sys, seq: 1, stable: "c".into(), pending: "".into() }, 0);
        s.apply(&final_ev(Source::Mic, 1, "a b"), 0);
        assert!(!s.hypotheses.contains_key(&Source::Mic));
        assert!(s.hypotheses.contains_key(&Source::Sys));
        s.apply(&Event::Dropped { source: Source::Sys, seq: 1, reason: "echo".into() }, 0);
        assert!(s.hypotheses.is_empty());
    }

    #[test]
    fn stopped_with_error_is_kept() {
        let mut s = MeetingState::default();
        s.apply(&Event::Started { session_dir: "/s".into(), sources: vec![Source::Mic] }, 0);
        s.apply(&Event::Level { source: Source::Mic, rms: 0.5 }, 0);
        s.apply(&Event::Stopped { session_dir: None, error: Some("disk".into()) }, 0);
        assert!(!s.running);
        assert_eq!(s.error.as_deref(), Some("disk"));
        assert_eq!(s.dir, Some("/s".into()));
        assert_eq!(s.sources[&Source::Mic].level, 0.0);
    }

    #[test]
    fn config_normalizes_out_of_range_values() {
        let c = MeetingConfig {
            model: "nope".into(),
            dedupe_threshold: 3.0,
            audio_days: 0,
            root: Some(PathBuf::new()),
            mic_device: Some("  ".into()),
            ticker: TickerConfig { position: "left".into(), width_pct: 5, font_px: 99, opacity: f32::NAN, speed_px_s: 0, ..Default::default() },
            ..Default::default()
        }
        .normalized();
        assert_eq!(c.model, models::DEFAULT_MODEL);
        assert_eq!(c.dedupe_threshold, 0.95);
        assert_eq!(c.audio_days, 1);
        assert_eq!(c.root, None);
        assert_eq!(c.mic_device, None);
        assert_eq!(c.ticker.position, "bottom");
        assert_eq!((c.ticker.width_pct, c.ticker.font_px, c.ticker.speed_px_s), (30, 28, 20));
        assert_eq!(c.ticker.opacity, 0.85);
    }

    #[test]
    fn config_defaults_when_unset_or_partial() {
        let c: MeetingConfig = serde_json::from_str(r#"{"sys": false}"#).unwrap();
        assert!(c.mic && !c.sys);
        assert_eq!(c.sources(), vec![Source::Mic]);
        assert_eq!(c.ticker, TickerConfig::default());
        assert!(c.root().ends_with("Transkripte"));
    }
}
