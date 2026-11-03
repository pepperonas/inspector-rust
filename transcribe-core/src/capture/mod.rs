//! Audio sources. A capture delivers 16 kHz mono f32 chunks to a callback;
//! the pipeline owns the handle on the thread that started it (cpal streams
//! are not `Send`).

use crate::protocol::Source;

pub type Chunk = Box<dyn FnMut(&[f32]) + Send>;
pub type OnError = Box<dyn FnMut(String) + Send>;

pub trait AudioInput {
    fn stop(self: Box<Self>);
    /// Something the user should know about how this source runs (e.g. the
    /// ScreenCaptureKit fallback).
    fn warning(&self) -> Option<String> {
        None
    }
}

pub trait CaptureFactory: Send + Sync {
    fn start(&self, source: Source, device: Option<&str>, on_chunk: Chunk, on_error: OnError)
        -> Result<Box<dyn AudioInput>, String>;
}

/// The real sources: cpal microphone; system audio via a Core Audio process
/// tap, falling back to ScreenCaptureKit.
pub struct SystemCapture;

// Platform sources arrive with Task 10.
impl CaptureFactory for SystemCapture {
    fn start(&self, _: Source, _: Option<&str>, _: Chunk, _: OnError) -> Result<Box<dyn AudioInput>, String> {
        Err("transcribe.unsupported".into())
    }
}
