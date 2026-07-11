//! Streaming native microphone capture (`cpal`) for the real-time audio
//! features (BPM detector; disco later). Emits mono PCM chunks to the frontend
//! as Tauri events, so those features never call `getUserMedia` — which makes
//! WKWebView reconfigure the shared CoreAudio device and briefly pauses other
//! apps' playback at mic-open (the same glitch fixed natively for Shazam).
//!
//! The frontend feeds the streamed PCM into its existing Web-Audio analysis
//! graph via a ScriptProcessor source, so the detection/visualizer code is
//! unchanged — only the audio *source* moves from the mic to this stream.
//!
//! Each event payload is `{ rate: u32, b64: String }` where `b64` is base64 of
//! little-endian `i16` mono samples.

use base64::Engine;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

/// A running capture; drop or `stop()` to end it (the worker thread owns the
/// cpal stream — `cpal::Stream` isn't `Send`, so it never leaves that thread).
pub struct MicStream {
    stop: Arc<AtomicBool>,
}

impl MicStream {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for MicStream {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Start capturing the default input device and emitting `event` (~25×/s).
pub fn start(app: AppHandle, event: &'static str) -> MicStream {
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    std::thread::spawn(move || {
        if let Err(e) = run(app, event, stop2) {
            tracing::warn!("mic_capture[{event}]: {e}");
        }
    });
    MicStream { stop }
}

fn run(app: AppHandle, event: &'static str, stop: Arc<AtomicBool>) -> Result<(), String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("no microphone found")?;
    let supported = device
        .default_input_config()
        .map_err(|e| format!("mic config error: {e}"))?;
    let rate = supported.sample_rate().0;
    let ch = supported.config().channels.max(1) as usize;
    let fmt = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let err_fn = |e| tracing::warn!("mic_capture: cpal stream error: {e}");

    fn push_mono(buf: &Mutex<Vec<f32>>, samples: impl Iterator<Item = f32>, ch: usize) {
        let mut b = buf.lock();
        let mut acc = 0.0f32;
        let mut c = 0usize;
        for s in samples {
            acc += s;
            c += 1;
            if c == ch {
                b.push(acc / ch as f32);
                acc = 0.0;
                c = 0;
            }
        }
    }

    let b1 = buf.clone();
    let b2 = buf.clone();
    let b3 = buf.clone();
    let stream = match fmt {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _: &_| push_mono(&b1, data.iter().copied(), ch),
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _: &_| push_mono(&b2, data.iter().map(|&s| s as f32 / 32768.0), ch),
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _: &_| {
                push_mono(&b3, data.iter().map(|&s| (s as f32 - 32768.0) / 32768.0), ch)
            },
            err_fn,
            None,
        ),
        other => return Err(format!("unsupported mic sample format: {other:?}")),
    }
    .map_err(|e| format!("mic stream build failed: {e}"))?;

    stream.play().map_err(|e| format!("mic start failed: {e}"))?;

    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(std::time::Duration::from_millis(40));
        let chunk = {
            let mut b = buf.lock();
            std::mem::take(&mut *b)
        };
        if chunk.is_empty() {
            continue;
        }
        let mut bytes = Vec::with_capacity(chunk.len() * 2);
        for &s in &chunk {
            let v = (s.clamp(-1.0, 1.0) as f64 * if s < 0.0 { 32768.0 } else { 32767.0 }) as i16;
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let _ = app.emit(event, serde_json::json!({ "rate": rate, "b64": b64 }));
    }
    drop(stream); // stop + release the input device
    Ok(())
}
