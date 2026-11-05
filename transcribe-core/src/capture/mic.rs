//! The microphone through cpal (the same path the app's `mic_capture` uses:
//! any sample format, downmixed, here also resampled to 16 kHz).

use super::{AudioInput, Chunk, OnError};
use crate::resample::Resampler;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

pub struct MicInput {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AudioInput for MicInput {
    fn stop(mut self: Box<Self>) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Names of the input devices, default first.
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let default = host.default_input_device().and_then(|d| d.name().ok());
    let mut names: Vec<String> = host
        .input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    names.sort();
    names.dedup();
    if let Some(d) = default {
        names.retain(|n| n != &d);
        names.insert(0, d);
    }
    names
}

fn pick(device: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    if let Some(name) = device {
        if let Some(d) = host
            .input_devices()
            .ok()
            .and_then(|mut it| it.find(|d| d.name().ok().as_deref() == Some(name)))
        {
            return Ok(d);
        }
        tracing::warn!("mic: device {name:?} not found — using the default input");
    }
    host.default_input_device().ok_or_else(|| "mic.none".to_string())
}

pub fn start(device: Option<&str>, mut on_chunk: Chunk, mut on_error: OnError) -> Result<Box<dyn AudioInput>, String> {
    let device = pick(device)?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let thread = std::thread::Builder::new()
        .name("ir-transcribe-mic-capture".into())
        .spawn(move || {
            let supported = match device.default_input_config() {
                Ok(c) => c,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("mic config: {e}")));
                    return;
                }
            };
            let rate = supported.sample_rate().0;
            let ch = supported.config().channels.max(1) as usize;
            let fmt = supported.sample_format();
            let config: cpal::StreamConfig = supported.into();
            let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
            let dead = Arc::new(AtomicBool::new(false));
            let dead2 = dead.clone();
            let err_fn = move |e| {
                tracing::warn!("mic: stream error: {e}");
                dead2.store(true, Ordering::SeqCst);
            };
            fn push(buf: &Mutex<Vec<f32>>, it: impl Iterator<Item = f32>, ch: usize) {
                let mut b = buf.lock().unwrap_or_else(|e| e.into_inner());
                let (mut acc, mut c) = (0.0f32, 0usize);
                for s in it {
                    acc += s;
                    c += 1;
                    if c == ch {
                        b.push(acc / ch as f32);
                        acc = 0.0;
                        c = 0;
                    }
                }
            }
            let (b1, b2, b3) = (buf.clone(), buf.clone(), buf.clone());
            let stream = match fmt {
                cpal::SampleFormat::F32 => device.build_input_stream(
                    &config,
                    move |d: &[f32], _: &_| push(&b1, d.iter().copied(), ch),
                    err_fn,
                    None,
                ),
                cpal::SampleFormat::I16 => device.build_input_stream(
                    &config,
                    move |d: &[i16], _: &_| push(&b2, d.iter().map(|&s| s as f32 / 32768.0), ch),
                    err_fn,
                    None,
                ),
                cpal::SampleFormat::U16 => device.build_input_stream(
                    &config,
                    move |d: &[u16], _: &_| push(&b3, d.iter().map(|&s| (s as f32 - 32768.0) / 32768.0), ch),
                    err_fn,
                    None,
                ),
                other => {
                    let _ = ready_tx.send(Err(format!("mic: unsupported sample format {other:?}")));
                    return;
                }
            };
            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(format!("mic: {e}")));
                    return;
                }
            };
            if let Err(e) = stream.play() {
                let _ = ready_tx.send(Err(format!("mic: {e}")));
                return;
            }
            let _ = ready_tx.send(Ok(()));
            let mut rs = Resampler::new(rate, 1);
            while !stop2.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(40));
                if dead.load(Ordering::SeqCst) {
                    on_error("mic.lost".into());
                    return;
                }
                let chunk = std::mem::take(&mut *buf.lock().unwrap_or_else(|e| e.into_inner()));
                if !chunk.is_empty() {
                    let out = rs.push(&chunk);
                    if !out.is_empty() {
                        on_chunk(&out);
                    }
                }
            }
            drop(stream);
        })
        .map_err(|e| e.to_string())?;
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => Ok(Box::new(MicInput { stop, thread: Some(thread) })),
        Ok(Err(e)) => Err(e),
        Err(_) => {
            // The thread may still come up after we gave up: tell it to stop at once,
            // or it records forever next to the retry's capture (mic held open, two
            // interleaved chunk streams).
            stop.store(true, Ordering::SeqCst);
            Err("mic: start timed out".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_start_timeout_stops_the_capture_thread() {
        let src = include_str!("mic.rs");
        let code = &src[..src.find("#[cfg(test)]").unwrap()];
        let arm = &code[code.find("Err(_) => {").expect("timeout arm")..];
        let arm = &arm[..arm.find("start timed out").unwrap()];
        assert!(arm.contains("stop.store(true"), "the late thread must see stop");
    }

    #[test]
    #[ignore = "needs a microphone and its permission"]
    fn mic_live_produces_samples() {
        let got = Arc::new(Mutex::new(Vec::<f32>::new()));
        let g = got.clone();
        let h = start(None, Box::new(move |s| g.lock().unwrap().extend_from_slice(s)), Box::new(|e| panic!("{e}"))).unwrap();
        std::thread::sleep(Duration::from_secs(3));
        h.stop();
        let s = got.lock().unwrap();
        eprintln!("{} samples, devices {:?}", s.len(), list_input_devices());
        assert!(s.len() > 16_000 * 2);
    }
}
