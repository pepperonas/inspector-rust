//! Incremental 48 kHz mono 16-bit WAV writer with timeline alignment.
//!
//! Time model: the sink owns a timeline origin (set via [`WavSink::set_origin`],
//! or the first push's `at_ns`). `push(at_ns, ..)` takes the timeline time of
//! the chunk START (pause-adjusted). The expected position is the number of
//! samples written so far; if `at_ns` is more than 50 ms ahead of it, silence
//! is written up to `at_ns` first. If it is behind (jitter/overlap) the chunk
//! is simply appended -- samples are never dropped and the file never rewinds.

use std::fs::File;
use std::io::{self, BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

pub const OUT_RATE: u32 = 48_000;
const GAP_THRESHOLD_NS: i64 = 50_000_000;

pub fn wav_header(sample_rate: u32, channels: u16, data_bytes: u32) -> [u8; 44] {
    let mut h = [0u8; 44];
    let block_align = channels as u32 * 2;
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_bytes).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&channels.to_le_bytes());
    h[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    h[28..32].copy_from_slice(&(sample_rate * block_align).to_le_bytes());
    h[32..34].copy_from_slice(&(block_align as u16).to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    h
}

/// Linear resampler; output length = round(len * to / from).
pub fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if input.is_empty() || from == 0 || to == 0 {
        return Vec::new();
    }
    if from == to {
        return input.to_vec();
    }
    let out_len = ((input.len() as f64) * to as f64 / from as f64).round() as usize;
    let ratio = from as f64 / to as f64;
    let last = input.len() - 1;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let i0 = (pos.floor() as usize).min(last);
            let i1 = (i0 + 1).min(last);
            let frac = (pos - i0 as f64) as f32;
            input[i0] + (input[i1] - input[i0]) * frac
        })
        .collect()
}

fn to_i16(s: f32) -> i16 {
    (s.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

pub struct WavSink {
    w: BufWriter<File>,
    input_rate: u32,
    samples: u64,
    origin_ns: Option<i64>,
    /// Unconsumed source samples + fractional read position (drift-free resampling).
    carry: Vec<f32>,
    phase: f64,
}

impl WavSink {
    pub fn create(path: &Path, sample_rate: u32) -> io::Result<Self> {
        let mut w = BufWriter::new(File::create(path)?);
        w.write_all(&wav_header(OUT_RATE, 1, 0))?;
        Ok(Self {
            w,
            input_rate: sample_rate,
            samples: 0,
            origin_ns: None,
            carry: Vec::new(),
            phase: 0.0,
        })
    }

    /// Timeline time (ns) corresponding to sample 0.
    pub fn set_origin(&mut self, origin_ns: i64) {
        self.origin_ns = Some(origin_ns);
    }

    /// Update the input rate (device may change after a re-open).
    pub fn set_input_rate(&mut self, rate: u32) {
        if rate != self.input_rate {
            self.carry.clear();
            self.phase = 0.0;
        }
        self.input_rate = rate;
    }

    /// Stateful linear resample: the fractional source position carries across
    /// pushes so the long-run output count has no per-chunk rounding bias.
    fn resample_stateful(&mut self, input: &[f32]) -> Vec<f32> {
        if self.input_rate == OUT_RATE {
            return input.to_vec();
        }
        if self.input_rate == 0 {
            return Vec::new();
        }
        self.carry.extend_from_slice(input);
        let ratio = self.input_rate as f64 / OUT_RATE as f64;
        let mut out = Vec::with_capacity((input.len() as f64 / ratio) as usize + 2);
        let mut pos = self.phase;
        while (pos.floor() as usize) + 1 < self.carry.len() {
            let i0 = pos.floor() as usize;
            let frac = (pos - i0 as f64) as f32;
            out.push(self.carry[i0] + (self.carry[i0 + 1] - self.carry[i0]) * frac);
            pos += ratio;
        }
        let consumed = (pos.floor() as usize).min(self.carry.len());
        self.carry.drain(..consumed);
        self.phase = pos - consumed as f64;
        out
    }

    fn write_silence_to(&mut self, target_ns: i64) -> io::Result<()> {
        let target_samples = (target_ns as i128 * OUT_RATE as i128 / 1_000_000_000) as u64;
        let mut missing = target_samples.saturating_sub(self.samples);
        let zeros = [0u8; 4096];
        while missing > 0 {
            let n = missing.min((zeros.len() / 2) as u64);
            self.w.write_all(&zeros[..n as usize * 2])?;
            missing -= n;
            self.samples += n;
        }
        Ok(())
    }

    /// Write trailing silence up to timeline `at_ns` (no-op if already past).
    pub fn pad_to(&mut self, at_ns: i64) -> io::Result<()> {
        let origin = *self.origin_ns.get_or_insert(at_ns);
        self.write_silence_to(at_ns - origin)
    }

    /// Cut everything after timeline `at_ns` (no-op if the file is shorter):
    /// samples that arrived after the video ended.
    pub fn truncate_to(&mut self, at_ns: i64) -> io::Result<()> {
        let origin = *self.origin_ns.get_or_insert(at_ns);
        let target = ((at_ns - origin).max(0) as i128 * OUT_RATE as i128 / 1_000_000_000) as u64;
        if self.samples <= target {
            return Ok(());
        }
        self.w.flush()?;
        let f = self.w.get_mut();
        f.set_len(44 + target * 2)?;
        f.seek(SeekFrom::End(0))?;
        self.samples = target;
        self.carry.clear();
        self.phase = 0.0;
        Ok(())
    }

    pub fn push(&mut self, at_ns: i64, mono_f32: &[f32]) -> io::Result<()> {
        let origin = *self.origin_ns.get_or_insert(at_ns);
        let expected_ns = (self.samples as i64) * 1_000_000_000 / OUT_RATE as i64;
        let target_ns = at_ns - origin;
        if target_ns - expected_ns > GAP_THRESHOLD_NS {
            self.write_silence_to(target_ns)?;
        }
        let resampled = self.resample_stateful(mono_f32);
        let mut buf = Vec::with_capacity(resampled.len() * 2);
        for s in &resampled {
            buf.extend_from_slice(&to_i16(*s).to_le_bytes());
        }
        self.w.write_all(&buf)?;
        self.samples += resampled.len() as u64;
        Ok(())
    }

    /// Patch header sizes; returns duration in ms.
    pub fn finish(mut self) -> io::Result<u64> {
        self.w.flush()?;
        let bytes = (self.samples * 2).min(u32::MAX as u64) as u32;
        let f = self.w.get_mut();
        f.seek(SeekFrom::Start(0))?;
        f.write_all(&wav_header(OUT_RATE, 1, bytes))?;
        f.flush()?;
        Ok(self.samples / (OUT_RATE as u64 / 1000))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ir-wav-{}-{}", name, std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join("t.wav")
    }

    #[test]
    fn header_matches_canonical_layout() {
        let h = wav_header(48_000, 1, 1000);
        assert_eq!(&h[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(h[4..8].try_into().unwrap()), 1036);
        assert_eq!(&h[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(h[24..28].try_into().unwrap()), 48_000);
        assert_eq!(u32::from_le_bytes(h[28..32].try_into().unwrap()), 96_000);
        assert_eq!(u16::from_le_bytes(h[32..34].try_into().unwrap()), 2);
        assert_eq!(&h[36..40], b"data");
        assert_eq!(u32::from_le_bytes(h[40..44].try_into().unwrap()), 1000);
    }

    #[test]
    fn truncate_cuts_samples_after_the_end() {
        let p = tmp("trunc");
        let mut s = WavSink::create(&p, 48_000).unwrap();
        s.set_origin(1_000_000_000);
        s.push(1_000_000_000, &vec![0.2f32; 4_800]).unwrap(); // 100 ms
        s.truncate_to(1_060_000_000).unwrap(); // keep 60 ms
        s.truncate_to(1_090_000_000).unwrap(); // longer than file: no-op
        s.push(1_060_000_000, &vec![0.2f32; 480]).unwrap(); // appends after the cut
        let ms = s.finish().unwrap();
        assert_eq!(ms, 70);
        assert_eq!(std::fs::metadata(&p).unwrap().len(), 44 + 70 * 48 * 2);
    }

    #[test]
    fn gap_is_filled_with_silence() {
        let p = tmp("gap");
        let mut s = WavSink::create(&p, 48_000).unwrap();
        let chunk = vec![0.5f32; 480]; // 10 ms
        s.push(0, &chunk).unwrap();
        s.push(200_000_000, &chunk).unwrap();
        let ms = s.finish().unwrap();
        assert!((209..=211).contains(&ms), "ms={ms}");
        let len = std::fs::metadata(&p).unwrap().len();
        assert_eq!(len, 44 + (ms * 48) * 2);
    }

    #[test]
    fn small_jitter_does_not_insert_silence() {
        let p = tmp("jit");
        let mut s = WavSink::create(&p, 48_000).unwrap();
        let chunk = vec![0.1f32; 480];
        s.push(0, &chunk).unwrap();
        s.push(30_000_000, &chunk).unwrap(); // 20 ms gap < 50 ms
        assert_eq!(s.finish().unwrap(), 20);
    }

    #[test]
    fn finish_patches_sizes() {
        let p = tmp("sizes");
        let mut s = WavSink::create(&p, 48_000).unwrap();
        s.push(0, &vec![0.0f32; 4800]).unwrap();
        s.finish().unwrap();
        let b = std::fs::read(&p).unwrap();
        let riff = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
        let data = u32::from_le_bytes(b[40..44].try_into().unwrap()) as usize;
        assert_eq!(riff, b.len() - 8);
        assert_eq!(data, b.len() - 44);
    }

    #[test]
    fn resample_linear_preserves_duration() {
        let input: Vec<f32> = (0..441).map(|i| (i as f32 * 0.01).sin()).collect();
        assert_eq!(resample_linear(&input, 44_100, 48_000).len(), 480);
        assert_eq!(resample_linear(&input, 48_000, 48_000).len(), 441);
        assert!(resample_linear(&[], 44_100, 48_000).is_empty());
    }

    #[test]
    fn explicit_origin_yields_leading_silence() {
        let p = tmp("origin");
        let mut s = WavSink::create(&p, 48_000).unwrap();
        s.set_origin(0);
        s.push(300_000_000, &vec![0.2f32; 480]).unwrap();
        let ms = s.finish().unwrap();
        assert!((309..=311).contains(&ms), "ms={ms}");
    }

    #[test]
    fn no_resampling_drift_over_an_hour() {
        let p = tmp("drift");
        let mut s = WavSink::create(&p, 44_100).unwrap();
        s.set_origin(0);
        let chunk = vec![0.1f32; 1024];
        let mut n = 0u64;
        for _ in 0..3600 {
            let at = (n as i128 * 1_000_000_000 / 44_100) as i64;
            s.push(at, &chunk).unwrap();
            n += 1024;
        }
        let expect = 3600u64 * 1024 * 48_000 / 44_100;
        assert!(
            (s.samples as i64 - expect as i64).abs() <= 1100,
            "{} vs {}",
            s.samples,
            expect
        );
    }

    #[test]
    fn pad_to_extends_and_is_noop_when_past() {
        let p = tmp("pad");
        let mut s = WavSink::create(&p, 48_000).unwrap();
        s.set_origin(0);
        s.push(0, &vec![0.0f32; 4800]).unwrap(); // 100 ms
        s.pad_to(500_000_000).unwrap();
        s.pad_to(200_000_000).unwrap();
        assert_eq!(s.finish().unwrap(), 500);
    }

    #[test]
    fn clamps_to_i16() {
        assert_eq!(to_i16(2.0), 32767);
        assert_eq!(to_i16(-2.0), -32767);
    }
}
