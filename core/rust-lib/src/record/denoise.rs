//! Noise suppression for a recording's microphone — never destructive: `mic.wav` stays as
//! recorded, the cleaned-up sound is a cache file next to it (`mic.denoised-<percent>.wav`)
//! that the scene and the preview use while the editor document asks for it.

use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use nnnoiseless::DenoiseState;
use record_render_protocol::safe_fs;

use super::project::MIC_FILE;
use super::wav::{read_mono, wav_header, wav_info, OUT_RATE};

/// The strengths the editor offers (percent of the cleaned signal in the mix); 0 = off.
pub const LEVELS: [u8; 3] = [50, 75, 100];
/// RNNoise works on 10-ms frames at 48 kHz.
const FRAME: usize = DenoiseState::FRAME_SIZE;
const CACHE_PREFIX: &str = "mic.denoised-";

pub const ERR_RATE: &str = "record.editor.denoise_rate";
pub const ERR_CANCELLED: &str = "record.editor.denoise_cancelled";

/// The nearest offered strength; 0 (and anything below the first step's half) is off.
pub fn normalize_level(level: u8) -> u8 {
    if level < LEVELS[0] / 2 {
        return 0;
    }
    *LEVELS.iter().min_by_key(|l| l.abs_diff(level)).unwrap_or(&0)
}

pub fn cache_name(level: u8) -> String {
    format!("{CACHE_PREFIX}{level}.wav")
}

/// The cleaned microphone file of the recording in `dir` for `level`, when it is there (a
/// plain file — a link is not followed).
pub fn cached(dir: &Path, level: u8) -> Option<PathBuf> {
    if level == 0 {
        return None;
    }
    let p = dir.join(cache_name(level));
    std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_file()).then_some(p)
}

/// Removes the cache files of other strengths (one per recording is kept). Best effort.
fn remove_others(dir: &Path, keep: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name != keep && name.starts_with(CACHE_PREFIX) && name.ends_with(".wav") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Streaming denoiser: `push` samples (−1..1), cleaned samples come out through `out`, the
/// same number as went in after `finish` — sample `n` out belongs to sample `n` in. RNNoise
/// answers one frame late (its overlap window; measured in the tests), so a frame's cleaned
/// version arrives with the next frame and is mixed with the input kept from before.
pub struct Denoiser {
    state: Box<DenoiseState<'static>>,
    mix: f32,
    input: Vec<f32>,
    /// The frame whose cleaned version the next `process_frame` returns.
    prev: Option<Vec<f32>>,
}

impl Denoiser {
    pub fn new(level: u8) -> Self {
        Self { state: DenoiseState::new(), mix: level.min(100) as f32 / 100.0, input: Vec::with_capacity(FRAME), prev: None }
    }

    fn frame(&mut self, out: &mut dyn FnMut(f32)) {
        let mut scaled = [0f32; FRAME];
        for (s, v) in scaled.iter_mut().zip(&self.input) {
            *s = v * 32768.0;
        }
        let mut wet = [0f32; FRAME];
        self.state.process_frame(&mut wet, &scaled);
        if let Some(prev) = self.prev.take() {
            for (dry, wet) in prev.iter().zip(&wet) {
                out(dry + (wet / 32768.0 - dry) * self.mix);
            }
        }
        self.prev = Some(std::mem::replace(&mut self.input, Vec::with_capacity(FRAME)));
    }

    pub fn push(&mut self, sample: f32, out: &mut dyn FnMut(f32)) {
        self.input.push(sample);
        if self.input.len() == FRAME {
            self.frame(out);
        }
    }

    /// The last (partial) frame, and the one still inside the filter.
    pub fn finish(mut self, out: &mut dyn FnMut(f32)) {
        if !self.input.is_empty() {
            self.frame(out);
        }
        // One silent frame pushes the last real one out (`prev` keeps its true length).
        if self.prev.is_some() {
            self.frame(out);
        }
    }
}

/// Writes the cleaned copy of `src` (48 kHz 16-bit WAV) to `dst`: a temp file next to it,
/// renamed when complete — `dst` exists only whole. `cancel` stops it (temp removed).
pub fn denoise_file(src: &Path, dst: &Path, level: u8, cancel: &AtomicBool) -> Result<(), String> {
    let info = wav_info(src).map_err(|e| e.to_string())?;
    if info.sample_rate != OUT_RATE {
        return Err(ERR_RATE.into());
    }
    let (tmp, file) = safe_fs::create_temp(dst).map_err(|e| e.to_string())?;
    let res = (|| -> io::Result<bool> {
        let mut w = BufWriter::new(file);
        w.write_all(&wav_header(OUT_RATE, 1, 0))?;
        let mut dn = Denoiser::new(level);
        let mut written = 0u64;
        let mut failed: Option<io::Error> = None;
        let mut count = 0u32;
        let mut stopped = false;
        let mut put = |v: f32| {
            if failed.is_none() {
                match w.write_all(&((v.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes()) {
                    Ok(()) => written += 1,
                    Err(e) => failed = Some(e),
                }
            }
        };
        read_mono(src, |s| {
            if stopped {
                return;
            }
            count = count.wrapping_add(1);
            if count.is_multiple_of(48_000) && cancel.load(Ordering::Relaxed) {
                stopped = true;
                return;
            }
            dn.push(s, &mut put);
        })?;
        if stopped {
            return Ok(false);
        }
        dn.finish(&mut put);
        if let Some(e) = failed {
            return Err(e);
        }
        let bytes = u32::try_from(written * 2).map_err(|_| io::Error::other("recording too long for a WAV"))?;
        w.seek(SeekFrom::Start(0))?;
        w.write_all(&wav_header(OUT_RATE, 1, bytes))?;
        let f = w.into_inner().map_err(|e| e.into_error())?;
        f.sync_all()?;
        Ok(true)
    })();
    match res {
        Ok(true) => std::fs::rename(&tmp, dst).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        }),
        Ok(false) => {
            let _ = std::fs::remove_file(&tmp);
            Err(ERR_CANCELLED.into())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e.to_string())
        }
    }
}

/// Makes sure the recording in `dir` has its cleaned microphone file for `level`; other
/// strengths' files are removed. Returns the file.
pub fn ensure(dir: &Path, level: u8, cancel: &AtomicBool) -> Result<PathBuf, String> {
    if let Some(p) = cached(dir, level) {
        return Ok(p);
    }
    let name = cache_name(level);
    let dst = dir.join(&name);
    // Something that is not a plain file sits at the name (a link): never write through it.
    if safe_fs::taken(&dst) {
        std::fs::remove_file(&dst).map_err(|e| e.to_string())?;
    }
    denoise_file(&dir.join(MIC_FILE), &dst, level, cancel)?;
    remove_others(dir, &name);
    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::wav::WavSink;

    fn noise(i: usize) -> f32 {
        let x = (i as u32).wrapping_mul(2_654_435_761).rotate_left(13).wrapping_mul(0x9E37_79B9);
        (x >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0
    }

    /// A voice-like tone that never repeats: harmonics of a gliding pitch (110 → 220 Hz)
    /// with an irregular syllable rhythm.
    fn voice(i: usize) -> f32 {
        let t = i as f32 / 48_000.0;
        let tau = 2.0 * std::f32::consts::PI;
        let phase = tau * (110.0 * t + 27.5 * t * t);
        let env = (0.5 + 0.5 * (tau * 2.3 * t).sin()) * (0.6 + 0.4 * (tau * 0.7 * t + 1.0).sin());
        let mut v = 0.0;
        for h in 1..=8 {
            v += (phase * h as f32).sin() / h as f32;
        }
        v * env * 0.2
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|s| s * s).sum::<f32>() / v.len().max(1) as f32).sqrt()
    }

    fn run(input: &[f32], level: u8) -> Vec<f32> {
        let mut out = Vec::with_capacity(input.len());
        let mut dn = Denoiser::new(level);
        for &s in input {
            dn.push(s, &mut |v| out.push(v));
        }
        dn.finish(&mut |v| out.push(v));
        out
    }

    #[test]
    fn levels_snap_to_the_offered_steps() {
        assert_eq!([0, 10, 24, 25, 60, 63, 80, 100, 255].map(normalize_level), [0, 0, 0, 50, 50, 75, 75, 100, 100]);
        assert_eq!(cache_name(75), "mic.denoised-75.wav");
    }

    #[test]
    fn output_has_the_length_of_the_input() {
        for n in [0usize, 1, 479, 480, 481, 4_801] {
            let input: Vec<f32> = (0..n).map(|i| noise(i) * 0.05).collect();
            assert_eq!(run(&input, 100).len(), n);
        }
    }

    #[test]
    fn steady_noise_gets_much_quieter_and_full_strength_removes_more() {
        let input: Vec<f32> = (0..96_000).map(|i| noise(i) * 0.03).collect();
        let tail = |v: &[f32]| rms(&v[48_000..]);
        let (half, full) = (run(&input, 50), run(&input, 100));
        assert!(tail(&full) < tail(&input) * 0.25, "noise {} → {}", tail(&input), tail(&full));
        assert!(tail(&half) > tail(&full) && tail(&half) < tail(&input) * 0.8);
    }

    /// Input and output are aligned sample for sample (measured, not assumed): the best
    /// match of a voice-like signal with its cleaned copy is at lag 0 — without the one-frame
    /// compensation it is at 480.
    #[test]
    fn the_cleaned_signal_is_not_shifted() {
        let input: Vec<f32> = (0..96_000).map(|i| voice(i) + noise(i) * 0.005).collect();
        let out = run(&input, 100);
        let corr = |lag: i64| -> f32 { (24_000i64..72_000).map(|i| input[i as usize] * out[(i + lag) as usize]).sum() };
        let best = (-960i64..=960).max_by(|a, b| corr(*a).total_cmp(&corr(*b))).unwrap();
        assert!(best.abs() <= 2, "the cleaned signal is off by {best} samples");
        assert!(rms(&out[24_000..72_000]) > rms(&input[24_000..72_000]) * 0.3, "the voice survives");
    }

    /// Speed reference: `cargo test --release -p inspector-rust-core --lib denoise_bench -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn denoise_bench() {
        let input: Vec<f32> = (0..48_000 * 60).map(|i| voice(i) + noise(i) * 0.02).collect();
        let t = std::time::Instant::now();
        let out = run(&input, 100);
        let el = t.elapsed();
        println!("60 s of audio in {el:?} ({:.0}x real time), {} samples", 60.0 / el.as_secs_f64(), out.len());
    }

    fn take_dir(name: &str, samples: &[f32]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ir-denoise-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut sink = WavSink::create(&dir.join(MIC_FILE), 48_000).unwrap();
        sink.set_origin(0);
        sink.push(0, samples).unwrap();
        sink.finish().unwrap();
        dir
    }

    #[test]
    fn ensure_writes_one_cache_and_keeps_the_recording() {
        let samples: Vec<f32> = (0..24_000).map(|i| noise(i) * 0.03).collect();
        let dir = take_dir("ensure", &samples);
        let before = std::fs::read(dir.join(MIC_FILE)).unwrap();
        let never = AtomicBool::new(false);
        assert_eq!(cached(&dir, 100), None);
        let p = ensure(&dir, 100, &never).unwrap();
        assert_eq!(cached(&dir, 100), Some(p.clone()));
        let (src, dst) = (wav_info(&dir.join(MIC_FILE)).unwrap(), wav_info(&p).unwrap());
        assert_eq!((dst.frames, dst.sample_rate, dst.channels), (src.frames, 48_000, 1));
        assert_eq!(std::fs::read(dir.join(MIC_FILE)).unwrap(), before, "the recording is untouched");
        // Another strength replaces the file; nothing else piles up.
        ensure(&dir, 50, &never).unwrap();
        assert_eq!((cached(&dir, 100), cached(&dir, 50).is_some()), (None, true));
        let names: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert_eq!(cached(&dir, 0), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancelled_run_leaves_nothing_behind() {
        let samples: Vec<f32> = (0..100_000).map(|i| noise(i) * 0.03).collect();
        let dir = take_dir("cancel", &samples);
        let stop = AtomicBool::new(true);
        assert_eq!(ensure(&dir, 100, &stop), Err(ERR_CANCELLED.to_string()));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "only mic.wav");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_at_the_cache_name_is_replaced_never_followed() {
        let samples: Vec<f32> = (0..4_800).map(|i| noise(i) * 0.03).collect();
        let dir = take_dir("link", &samples);
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, b"keep").unwrap();
        std::os::unix::fs::symlink(&victim, dir.join(cache_name(100))).unwrap();
        assert_eq!(cached(&dir, 100), None, "a link is not a cache");
        ensure(&dir, 100, &AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
        assert!(cached(&dir, 100).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
