//! Microphone capture edge: feeds [`WavSink`] on the shared record timeline.
use super::core::PauseShared;
use super::host_now_ns;
use super::wav::WavSink;
use crate::mic_capture::{self, MicStream};
use parking_lot::Mutex;
use std::sync::Arc;
use tauri::AppHandle;

/// Start capturing into `sink`. Chunks arriving while paused are dropped; the
/// chunk start on the timeline is `timeline(now) - chunk duration`. If the
/// input device disappears, `start_chunks` re-opens it later and the gap
/// becomes silence automatically (see `WavSink::push`).
pub fn start_mic(app: AppHandle, sink: Arc<Mutex<WavSink>>, clock: Arc<PauseShared>) -> MicStream {
    sink.lock().set_origin(clock.timeline_ns(host_now_ns()));
    mic_capture::start_chunks(app, move |chunk, rate| {
        if clock.is_paused() || chunk.is_empty() || rate == 0 {
            return;
        }
        let dur_ns = chunk.len() as i64 * 1_000_000_000 / rate as i64;
        let at = clock.timeline_ns(host_now_ns()) - dur_ns;
        let mut s = sink.lock();
        s.set_input_rate(rate);
        if let Err(e) = s.push(at.max(0), chunk) {
            tracing::warn!("record mic write failed: {e}");
        }
    })
}
