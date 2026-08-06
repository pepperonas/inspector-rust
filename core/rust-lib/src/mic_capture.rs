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

/// Callback for [`Sink::Chunks`]: one drained mono chunk + the device rate.
pub type ChunkFn = Box<dyn Fn(&[f32], u32) + Send>;

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

/// What a capture does with each drained ~40 ms chunk.
pub enum Sink {
    /// Emit base64 LE-`i16` mono PCM as `{rate, b64}` on this Tauri event —
    /// the BPM detector / disco path, which needs the samples themselves.
    Pcm(&'static str),
    /// Call back with each drained mono chunk + the device sample rate.
    /// Used by `iris`, which needs the samples themselves (RMS for the
    /// threshold + the beat detector): shipping base64 PCM 25×/s over IPC
    /// just to analyse it in a webview would be pure waste, and the
    /// decisions belong in one place (Rust) anyway.
    Chunks(ChunkFn),
}

impl Sink {
    fn label(&self) -> &'static str {
        match self {
            Sink::Pcm(ev) => ev,
            Sink::Chunks(_) => "chunks",
        }
    }
}

/// Start capturing the default input device and emitting `event` (~25×/s).
pub fn start(app: AppHandle, event: &'static str) -> MicStream {
    start_with(app, Sink::Pcm(event))
}

/// Start an analysis capture: `on_chunk` is called ~25×/s with each drained
/// mono chunk and the device sample rate. No IPC, no base64 — see
/// [`Sink::Chunks`].
pub fn start_chunks(
    app: AppHandle,
    on_chunk: impl Fn(&[f32], u32) + Send + 'static,
) -> MicStream {
    start_with(app, Sink::Chunks(Box::new(on_chunk)))
}

/// Shared entry point: spawns the worker thread that owns the cpal stream
/// (`cpal::Stream` isn't `Send`, so it must never leave that thread).
pub fn start_with(app: AppHandle, sink: Sink) -> MicStream {
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    std::thread::spawn(move || {
        let label = sink.label();
        if let Err(e) = run(app, sink, stop2) {
            tracing::warn!("mic_capture[{label}]: {e}");
        }
    });
    MicStream { stop }
}

/// Root-mean-square of a mono chunk (linear amplitude, 0..1).
pub(crate) fn chunk_rms(chunk: &[f32]) -> f32 {
    if chunk.is_empty() {
        return 0.0;
    }
    let sum: f64 = chunk.iter().map(|&s| (s as f64) * (s as f64)).sum();
    (sum / chunk.len() as f64).sqrt() as f32
}

fn run(app: AppHandle, sink: Sink, stop: Arc<AtomicBool>) -> Result<(), String> {
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
        match &sink {
            Sink::Chunks(on_chunk) => on_chunk(&chunk, rate),
            Sink::Pcm(event) => {
                let mut bytes = Vec::with_capacity(chunk.len() * 2);
                for &s in &chunk {
                    let v =
                        (s.clamp(-1.0, 1.0) as f64 * if s < 0.0 { 32768.0 } else { 32767.0 }) as i16;
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let _ = app.emit(event, serde_json::json!({ "rate": rate, "b64": b64 }));
            }
        }
    }
    drop(stream); // stop + release the input device
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // `chunk_rms` is the entry point of the whole `iris` chain (RMS → dBFS →
    // SPL → lit/dark), so its numbers have to be right rather than merely
    // plausible. The cpal stream itself needs a live machine and stays
    // untested, per the house rule.

    #[test]
    fn an_empty_chunk_is_silent_not_nan() {
        // Division by zero here would poison every downstream dB conversion.
        assert_eq!(chunk_rms(&[]), 0.0);
        assert!(chunk_rms(&[]).is_finite());
    }

    #[test]
    fn digital_silence_is_zero() {
        assert_eq!(chunk_rms(&[0.0; 512]), 0.0);
    }

    #[test]
    fn a_full_scale_square_wave_is_one() {
        let buf: Vec<f32> = (0..256).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        assert!((chunk_rms(&buf) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_full_scale_sine_is_one_over_root_two() {
        let n = 4096;
        let buf: Vec<f32> = (0..n)
            .map(|i| (std::f32::consts::TAU * 8.0 * i as f32 / n as f32).sin())
            .collect();
        let expected = 1.0 / 2f32.sqrt();
        assert!(
            (chunk_rms(&buf) - expected).abs() < 1e-3,
            "got {}, want {expected}",
            chunk_rms(&buf)
        );
    }

    #[test]
    fn rms_scales_linearly_with_amplitude() {
        let base: Vec<f32> = (0..1024)
            .map(|i| (std::f32::consts::TAU * 4.0 * i as f32 / 1024.0).sin())
            .collect();
        let quiet: Vec<f32> = base.iter().map(|s| s * 0.25).collect();
        let full = chunk_rms(&base);
        assert!((chunk_rms(&quiet) - full * 0.25).abs() < 1e-4);
    }

    #[test]
    fn a_dc_offset_counts_as_level() {
        // RMS is not mean-removed: a stuck-at-0.5 input reads 0.5, not 0. Worth
        // pinning — a DC-biased interface would otherwise look silent if this
        // ever "helpfully" subtracted the mean.
        assert!((chunk_rms(&[0.5; 128]) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn sign_does_not_matter() {
        assert!((chunk_rms(&[0.3; 64]) - chunk_rms(&[-0.3; 64])).abs() < 1e-6);
    }

    #[test]
    fn a_single_loud_sample_barely_moves_a_quiet_chunk() {
        // One spike in 1024 samples must not read as a loud chunk — this is
        // what keeps a click from tripping the iris threshold on its own.
        let mut buf = vec![0.0f32; 1024];
        buf[0] = 1.0;
        assert!(chunk_rms(&buf) < 0.032, "{}", chunk_rms(&buf));
    }

    #[test]
    fn the_result_is_never_negative_or_nan() {
        for buf in [vec![0.0; 8], vec![-1.0; 8], vec![0.7, -0.2, 0.1, -0.9]] {
            let r = chunk_rms(&buf);
            assert!(r >= 0.0 && r.is_finite(), "{r}");
        }
    }

    #[test]
    fn sink_labels_identify_the_mode_in_logs() {
        assert_eq!(Sink::Pcm("mic-audio").label(), "mic-audio");
        assert_eq!(Sink::Chunks(Box::new(|_, _| {})).label(), "chunks");
    }
}
