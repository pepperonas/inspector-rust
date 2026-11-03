//! Live transcription engine, free of Tauri. See
//! `docs/superpowers/specs/2026-11-03-live-transcription-design.md`.
pub use transcribe_protocol as protocol;
pub mod text;
pub mod writer;
pub mod segment;
