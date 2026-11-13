//! The microphone level in the recording HUD before a take: a short-lived capture on the
//! CHOSEN input whose newest level the HUD polls. Like the camera preview it stops when the
//! take starts and when nobody polls any more.

use crate::mic_capture::{self, MicStream};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};

/// Without a poll for this long the meter stops (the microphone is released).
pub const IDLE_STOP_MS: i64 = 3_000;
/// Meter range in dBFS: silence floor and full scale.
pub const FLOOR_DBFS: f32 = -60.0;

/// 0..1 for the meter bar from a level in dBFS (linear in dB between the floor and 0).
pub fn meter_level(dbfs: f32) -> f32 {
    if !dbfs.is_finite() {
        return 0.0;
    }
    ((dbfs - FLOOR_DBFS) / -FLOOR_DBFS).clamp(0.0, 1.0)
}

/// What the HUD gets per poll.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicLevel {
    /// 0..1, see [`meter_level`].
    pub level: f32,
    pub dbfs: f32,
    /// The chosen input is missing and the default input is measured instead.
    pub fell_back: bool,
    /// Samples have arrived since the meter started.
    pub live: bool,
}

struct Meter {
    device: String,
    stream: MicStream,
}

static CURRENT: Mutex<Option<Meter>> = Mutex::new(None);
static LEVEL_BITS: AtomicU32 = AtomicU32::new(0);
static LIVE: AtomicBool = AtomicBool::new(false);
static LAST_POLL_MS: AtomicI64 = AtomicI64::new(0);
static WATCHING: AtomicBool = AtomicBool::new(false);

fn now_ms() -> i64 {
    super::host_now_ns() / 1_000_000
}

/// Starts (or keeps) metering `device` (a `MicDevice::id`; empty = the default input).
pub fn start(device: &str) {
    let mut cur = CURRENT.lock();
    LAST_POLL_MS.store(now_ms(), Ordering::Relaxed);
    if cur.as_ref().is_some_and(|m| m.device == device) {
        return;
    }
    cur.take();
    LIVE.store(false, Ordering::Relaxed);
    LEVEL_BITS.store(FLOOR_DBFS.to_bits(), Ordering::Relaxed);
    let stream = mic_capture::start_chunks_on(None, Some(device.to_string()), |chunk, _rate| {
        LEVEL_BITS.store(mic_capture::chunk_dbfs(chunk).to_bits(), Ordering::Relaxed);
        LIVE.store(true, Ordering::Relaxed);
    });
    *cur = Some(Meter { device: device.to_string(), stream });
    drop(cur);
    if !WATCHING.swap(true, Ordering::SeqCst) {
        let _ = std::thread::Builder::new().name("ir-mic-meter-watch".into()).spawn(|| {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                if CURRENT.lock().is_none() {
                    break;
                }
                if now_ms() - LAST_POLL_MS.load(Ordering::Relaxed) > IDLE_STOP_MS {
                    stop();
                    break;
                }
            }
            WATCHING.store(false, Ordering::SeqCst);
        });
    }
}

/// Stops the meter and releases the microphone. Safe to call when none runs.
pub fn stop() {
    if let Some(m) = CURRENT.lock().take() {
        m.stream.stop();
    }
    LIVE.store(false, Ordering::Relaxed);
}

/// The HUD's poll (`None` when no meter runs).
pub fn level() -> Option<MicLevel> {
    let cur = CURRENT.lock();
    let m = cur.as_ref()?;
    LAST_POLL_MS.store(now_ms(), Ordering::Relaxed);
    let dbfs = f32::from_bits(LEVEL_BITS.load(Ordering::Relaxed));
    Some(MicLevel { level: meter_level(dbfs), dbfs, fell_back: m.stream.fell_back(), live: LIVE.load(Ordering::Relaxed) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_is_linear_in_decibels() {
        assert_eq!(meter_level(0.0), 1.0);
        assert_eq!(meter_level(-60.0), 0.0);
        assert!((meter_level(-30.0) - 0.5).abs() < 1e-6);
        assert!((meter_level(-12.0) - 0.8).abs() < 1e-6);
        // Silence (-120 from `chunk_dbfs`), clipping and garbage stay inside the bar.
        assert_eq!(meter_level(-120.0), 0.0);
        assert_eq!(meter_level(6.0), 1.0);
        assert_eq!(meter_level(f32::NAN), 0.0);
        assert_eq!(meter_level(f32::NEG_INFINITY), 0.0);
    }
}
