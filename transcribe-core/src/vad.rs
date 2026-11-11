//! Voice activity: per-512-sample-frame speech probabilities.

use crate::segment::VAD_FRAME;

pub trait Vad: Send {
    /// Probabilities for `samples` (one per `VAD_FRAME`; a trailing partial
    /// frame is kept for the next call).
    fn probs(&mut self, samples: &[f32]) -> Result<Vec<f32>, String>;
}

/// Energy VAD for tests and as a fallback: RMS above `threshold` is speech.
pub struct EnergyVad {
    pub threshold: f32,
    rest: Vec<f32>,
}

impl EnergyVad {
    pub fn new(threshold: f32) -> Self {
        EnergyVad { threshold, rest: Vec::new() }
    }
}

impl Vad for EnergyVad {
    fn probs(&mut self, samples: &[f32]) -> Result<Vec<f32>, String> {
        self.rest.extend_from_slice(samples);
        let n = self.rest.len() / VAD_FRAME;
        let out = self.rest[..n * VAD_FRAME]
            .chunks(VAD_FRAME)
            .map(|f| {
                let r = (f.iter().map(|x| x * x).sum::<f32>() / f.len() as f32).sqrt();
                if r > self.threshold { 1.0 } else { 0.0 }
            })
            .collect();
        self.rest.drain(..n * VAD_FRAME);
        Ok(out)
    }
}

/// A frame louder than this (about −44 dBFS) counts as content in the music
/// modes — a quiet room sits near −67 dBFS, a playing song far above.
pub const MUSIC_RMS: f32 = 0.006;

/// Song and room sessions: the speech VAD hears singing over instruments as
/// "no speech" most of the time and would cut a song into 1-s scraps with
/// long holes. Every frame that is audibly not silence counts there; the
/// VAD's own verdict still stands for quiet speech.
pub fn music_probs(frames: &[f32], mut probs: Vec<f32>) -> Vec<f32> {
    for (p, f) in probs.iter_mut().zip(frames.chunks(VAD_FRAME)) {
        let r = (f.iter().map(|x| x * x).sum::<f32>() / f.len().max(1) as f32).sqrt();
        if r > MUSIC_RMS {
            *p = 1.0;
        }
    }
    probs
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use silero::SileroVad;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod silero {
    use super::*;
    use whisper_rs::{WhisperVadContext, WhisperVadContextParams};

    /// Context the ggml Silero model sees before the new frames: its LSTM
    /// state is reset on every call, the history warms it up again.
    pub const HISTORY: usize = 16_000;

    pub struct SileroVad {
        ctx: WhisperVadContext,
        history: Vec<f32>,
        rest: Vec<f32>,
    }

    impl SileroVad {
        pub fn new() -> Result<SileroVad, String> {
            let path = crate::model::vad_model_path()?;
            let mut p = WhisperVadContextParams::new();
            p.set_n_threads(1);
            p.set_use_gpu(false);
            let ctx = WhisperVadContext::new(&path.to_string_lossy(), p).map_err(|e| format!("vad: {e}"))?;
            Ok(SileroVad { ctx, history: Vec::new(), rest: Vec::new() })
        }
    }

    impl Vad for SileroVad {
        fn probs(&mut self, samples: &[f32]) -> Result<Vec<f32>, String> {
            self.rest.extend_from_slice(samples);
            let n = self.rest.len() / VAD_FRAME;
            if n == 0 {
                return Ok(Vec::new());
            }
            let fresh: Vec<f32> = self.rest.drain(..n * VAD_FRAME).collect();
            let mut window = self.history.clone();
            let skip = window.len() / VAD_FRAME;
            window.extend_from_slice(&fresh);
            self.ctx.detect_speech(&window).map_err(|e| format!("vad: {e}"))?;
            let all = self.ctx.probabilities();
            let mut out: Vec<f32> = all.iter().skip(skip).take(n).copied().collect();
            out.resize(n, 0.0);
            self.history.extend_from_slice(&fresh);
            let keep = HISTORY / VAD_FRAME * VAD_FRAME;
            if self.history.len() > keep {
                let cut = self.history.len() - keep;
                self.history.drain(..cut);
            }
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_vad_keeps_partial_frames() {
        let mut v = EnergyVad::new(0.01);
        assert!(v.probs(&[0.5; 300]).unwrap().is_empty());
        assert_eq!(v.probs(&[0.5; 300]).unwrap(), vec![1.0]);
        assert_eq!(v.probs(&[0.0; 1024]).unwrap().len(), 2);
    }

    #[test]
    fn music_counts_every_audible_frame() {
        let mut frames = vec![0.3f32; VAD_FRAME];
        frames.extend(vec![0.001f32; VAD_FRAME * 2]);
        // Loud frame the VAD missed, quiet frame it missed, quiet frame it heard.
        assert_eq!(music_probs(&frames, vec![0.0, 0.1, 0.8]), vec![1.0, 0.1, 0.8]);
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    #[ignore = "loads whisper.cpp"]
    fn silero_marks_speech_frames() {
        let speech = crate::test_support::fixture("de.wav");
        let mut v = SileroVad::new().unwrap();
        let silence = vec![0.0f32; 16_000];
        let ps = v.probs(&silence).unwrap();
        assert!(ps.iter().all(|p| *p < 0.35), "{ps:?}");
        let mut probs = Vec::new();
        for c in speech.chunks(1600) {
            probs.extend(v.probs(c).unwrap());
        }
        let loud = probs.iter().filter(|p| **p > 0.5).count();
        eprintln!("{loud}/{} frames speech", probs.len());
        assert!(loud as f32 > probs.len() as f32 * 0.5);
    }
}
