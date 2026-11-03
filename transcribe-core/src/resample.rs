//! Any rate, any channel count → 16 kHz mono f32 (downmix = mean).

use rubato::{FftFixedIn, Resampler as _};

pub const OUT_RATE: u32 = 16_000;
const CHUNK: usize = 1024;

pub struct Resampler {
    channels: usize,
    inner: Option<FftFixedIn<f32>>,
    pending: Vec<f32>,
}

impl Resampler {
    pub fn new(in_rate: u32, channels: u16) -> Self {
        let inner = (in_rate != OUT_RATE).then(|| {
            FftFixedIn::<f32>::new(in_rate as usize, OUT_RATE as usize, CHUNK, 2, 1)
                .expect("valid sample rates")
        });
        Resampler { channels: usize::from(channels.max(1)), inner, pending: Vec::new() }
    }

    fn run(&mut self, mono: &[f32]) -> Vec<f32> {
        let Some(r) = self.inner.as_mut() else { return mono.to_vec() };
        self.pending.extend_from_slice(mono);
        let mut out = Vec::new();
        loop {
            let need = r.input_frames_next();
            if self.pending.len() < need {
                break;
            }
            let chunk: Vec<f32> = self.pending.drain(..need).collect();
            match r.process(&[chunk], None) {
                Ok(mut v) => out.append(&mut v[0]),
                Err(_) => break,
            }
        }
        out
    }

    /// Interleaved frames (`L R L R …`).
    pub fn push(&mut self, interleaved: &[f32]) -> Vec<f32> {
        let ch = self.channels;
        if ch == 1 {
            return self.run(interleaved);
        }
        let mono: Vec<f32> = interleaved
            .chunks_exact(ch)
            .map(|f| f.iter().sum::<f32>() / ch as f32)
            .collect();
        self.run(&mono)
    }

    /// One slice per channel (non-interleaved buffers, e.g. Core Audio taps).
    pub fn push_planar(&mut self, planes: &[&[f32]]) -> Vec<f32> {
        if planes.is_empty() {
            return Vec::new();
        }
        let n = planes.iter().map(|p| p.len()).min().unwrap_or(0);
        let mono: Vec<f32> = (0..n)
            .map(|i| planes.iter().map(|p| p[i]).sum::<f32>() / planes.len() as f32)
            .collect();
        self.run(&mono)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_crossings(s: &[f32]) -> usize {
        s.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count()
    }

    #[test]
    fn resampler_48k_stereo_to_16k_mono() {
        let n = 48_000;
        let mut inter = Vec::with_capacity(n * 2);
        for i in 0..n {
            let v = (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48_000.0).sin() * 0.5;
            inter.push(v);
            inter.push(v);
        }
        let mut r = Resampler::new(48_000, 2);
        let mut out = Vec::new();
        for c in inter.chunks(960) {
            out.extend(r.push(c));
        }
        let expected = 16_000.0;
        assert!((out.len() as f32 - expected).abs() / expected < 0.08, "{}", out.len());
        // 1 kHz → about 2000 zero crossings per second of output.
        let zc = zero_crossings(&out[2000..out.len() - 2000]) as f32 / ((out.len() - 4000) as f32 / 16_000.0);
        assert!((zc - 2000.0).abs() < 40.0, "{zc}");
    }

    #[test]
    fn sixteen_khz_mono_passes_through() {
        let mut r = Resampler::new(16_000, 1);
        assert_eq!(r.push(&[0.1, 0.2, 0.3]), vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn planar_downmix_averages_channels() {
        let mut r = Resampler::new(16_000, 2);
        assert_eq!(r.push_planar(&[&[1.0, 0.0], &[0.0, 0.0]]), vec![0.5, 0.0]);
    }
}
