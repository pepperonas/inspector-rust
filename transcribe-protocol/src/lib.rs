//! Wire protocol between Inspector Rust and its `ir-transcribe` helper, plus
//! the session folder format (`session`), which the app reads without
//! starting the helper.
//!
//! One JSON object per line in both directions: commands carry a `"cmd"` tag,
//! events an `"event"` tag. `encode` never emits a line break inside the
//! object (serde_json escapes `\n` in strings), so a line is always one
//! message.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub mod models;
pub mod session;

/// Where the audio comes from. Kept separate end to end — never mixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The microphone: "me".
    Mic,
    /// Everything this Mac plays: "the other side".
    Sys,
}

impl Source {
    /// The label in transcripts and the ticker.
    pub fn label(self) -> &'static str {
        match self {
            Source::Mic => "MIC",
            Source::Sys => "SYS",
        }
    }

    /// Lower-case name, used in file names (`audio/000001-mic.wav`).
    pub fn slug(self) -> &'static str {
        match self {
            Source::Mic => "mic",
            Source::Sys => "sys",
        }
    }
}

/// Recognition language. `Auto` only ever chooses between German and English.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    Auto,
    De,
    En,
}

impl Lang {
    /// The forced whisper language code, `None` for auto.
    pub fn forced(self) -> Option<&'static str> {
        match self {
            Lang::Auto => None,
            Lang::De => Some("de"),
            Lang::En => Some("en"),
        }
    }
}

/// When the echo gate (drop MIC lines that repeat what SYS just said) runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DedupeMode {
    /// On, unless the output looks like headphones.
    #[default]
    Auto,
    Always,
    Never,
}

/// What a session is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Live transcription of both sources into a session folder.
    #[default]
    Meeting,
    /// `transcribe --start`: microphone only, result to the clipboard.
    Dictation,
    /// `transcribe --song`: system audio only.
    Song,
    /// `transcribe --room`: microphone, song rules (longer segments).
    Room,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionConfig {
    pub sources: Vec<Source>,
    pub mic_device: Option<String>,
    pub model: String,
    pub lang: Lang,
    /// Folder the session folder is created in.
    pub root: PathBuf,
    pub keep_audio: bool,
    pub dedupe: DedupeMode,
    pub dedupe_threshold: f32,
    pub mode: Mode,
}

/// App → helper.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    Start(SessionConfig),
    Stop,
    SetSource { source: Source, on: bool },
    /// The last `secs` seconds of a source as 16 kHz i16 (song recognition).
    Window { source: Source, secs: u32 },
    DownloadModel { model: String },
    CancelDownload,
    TranscribeFile { path: PathBuf, lang: Lang, model: String },
    CancelFile,
    ListDevices,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceStatus {
    Starting,
    Running,
    Lost,
    Off,
}

/// Helper → app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Ready { version: String },
    Started { session_dir: PathBuf, sources: Vec<Source> },
    /// The open segment as it is heard: `stable` agreed by two passes, `pending` not yet.
    Hypothesis { source: Source, seq: u64, stable: String, pending: String },
    Final {
        source: Source,
        seq: u64,
        start_ms: u64,
        end_ms: u64,
        lang: String,
        text: String,
        confidence: f32,
    },
    Dropped { source: Source, seq: u64, reason: String },
    Level { source: Source, rms: f32 },
    SourceState { source: Source, state: SourceStatus, detail: Option<String> },
    Warning { code: String, detail: String },
    Stopped { session_dir: Option<PathBuf>, error: Option<String> },
    Window { source: Source, pcm_b64: String },
    ModelProgress { model: String, done: u64, total: u64 },
    ModelReady { model: String },
    ModelError { model: String, error: String },
    FileProgress { done_ms: u64, total_ms: u64 },
    FileDone {
        txt: PathBuf,
        /// The language most of the speech was in.
        #[serde(default)]
        lang: Option<String>,
    },
    FileError { error: String },
    Devices { mic: Vec<String> },
    Error { error: String },
}

/// One message as a single `\n`-terminated line.
pub fn encode<T: Serialize>(v: &T) -> String {
    let mut s = serde_json::to_string(v).unwrap_or_else(|e| {
        serde_json::to_string(&Event::Error { error: format!("encode: {e}") }).unwrap_or_default()
    });
    s.push('\n');
    s
}

pub fn decode_command(line: &str) -> Result<Command, String> {
    serde_json::from_str(line.trim()).map_err(|e| e.to_string())
}

pub fn decode_event(line: &str) -> Result<Event, String> {
    serde_json::from_str(line.trim()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests;
