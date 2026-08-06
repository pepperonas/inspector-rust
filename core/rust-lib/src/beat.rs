//! Beat/onset detection for `iris` — a faithful Rust port of the field-proven
//! `lib/bpm.ts` `BpmAnalyzer` core (itself deployed three times: the `bpm`
//! detector, the disco engine, and the raspi5 disco-controller).
//!
//! Same algorithm, same constants: energy-onset detection on the kick band
//! (HP 30 Hz → LP 100 Hz, Q 1.5), 1024-sample analysis hops, a moving-average
//! baseline with the **calibration gate** (no onset for the first
//! `AVG_WINDOW_S` — without it the biased-low baseline fires a burst of false
//! onsets at startup, the historical "too fast for the first 20 seconds"
//! bug), the **SuperFlux lagged-peak gate** (Böck & Widmer — an onset must
//! rise above the peak of a *lagged* recent window, which is what suppresses
//! the kick's own decay/sustain/room-echo from reading as ghost beats between
//! real beats), a 300 ms refractory (max 200 BPM), and an IOI-median BPM with
//! a consistency confidence.
//!
//! Differences from the TS original, both deliberate:
//! * **Time comes from the sample count**, not a wall clock — deterministic
//!   under test, immune to callback jitter.
//! * The display-smoothing layer (octave snap, 4 s display mean, stale reset)
//!   is omitted: iris needs *onsets* to flash on, not a UI-stable readout.
//!   The raw median BPM + confidence ride along for scaling only.
//!
//! The biquad here is a minimal RBJ-cookbook implementation rather than a
//! reuse of `boom::Biquad` — boom's exposes shelves/peaking for its EQ, not
//! plain LP/HP, and a dependency between the system-audio EQ and the beat
//! detector would be a strange coupling for ~30 lines of math.

use std::collections::VecDeque;

/// Kick band, matching the `bpm` detector's Web-Audio graph.
pub const BAND_HP_HZ: f32 = 30.0;
pub const BAND_LP_HZ: f32 = 100.0;
pub const BAND_LP_Q: f32 = 1.5;
/// Analysis hop, matching the reference's 1024-sample frames.
pub const HOP: usize = 1024;
/// Moving-average baseline window + the calibration gate span.
pub const AVG_WINDOW_S: f64 = 3.0;
/// Hop energy must exceed `baseline × this` to count as an onset.
pub const ONSET_THRESHOLD: f32 = 1.4;
/// Minimum inter-onset gap (=> max 200 BPM; suppresses room echoes).
pub const ONSET_REFRACTORY_S: f64 = 0.3;
/// Sliding window of onsets used for the BPM median.
pub const IOI_WINDOW_S: f64 = 6.0;
pub const BPM_MIN: f32 = 60.0;
pub const BPM_MAX: f32 = 200.0;
/// Onsets needed in the window before the median is trusted.
pub const MIN_ONSETS_FOR_ESTIMATE: usize = 4;
/// SuperFlux gate: skip `LAG` frames back, reference = max of the next `WIN`
/// older frames, current energy must beat it by `MARGIN`.
pub const SUPERFLUX_LAG: usize = 4;
pub const SUPERFLUX_WIN: usize = 4;
pub const SUPERFLUX_MARGIN: f32 = 1.04;
/// An IOI within this fraction of the median counts as "consistent" for the
/// confidence figure.
const IOI_CONSISTENT_FRAC: f32 = 0.15;
/// Salience mapping: baseline-multiple 1.4× → strength 0, ≥6× → 1.
const STRENGTH_FULL_RATIO: f32 = 6.0;

/// One detected beat.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct Onset {
    /// 0..1 salience — how far the hop energy rose above the onset threshold.
    pub strength: f32,
    /// Median-IOI tempo folded into [`BPM_MIN`], [`BPM_MAX`]; `None` until
    /// enough onsets are in the window.
    pub bpm: Option<f32>,
    /// 0..1 — fraction of recent IOIs consistent with the median.
    pub confidence: f32,
}

// ── minimal RBJ biquad (transposed direct form II) ──────────────────────────

struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn new(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn lowpass(rate: f32, freq: f32, q: f32) -> Self {
        let w0 = std::f32::consts::TAU * freq / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        Self::new(
            (1.0 - cos) / 2.0,
            1.0 - cos,
            (1.0 - cos) / 2.0,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        )
    }

    fn highpass(rate: f32, freq: f32, q: f32) -> Self {
        let w0 = std::f32::consts::TAU * freq / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        Self::new(
            (1.0 + cos) / 2.0,
            -(1.0 + cos),
            (1.0 + cos) / 2.0,
            1.0 + alpha,
            -2.0 * cos,
            1.0 - alpha,
        )
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

// ── the detector ────────────────────────────────────────────────────────────

pub struct BeatDetector {
    rate: f32,
    hp: Biquad,
    lp: Biquad,
    /// Filtered samples waiting to fill the next hop.
    pending: Vec<f32>,
    /// Running time in seconds, derived from the processed sample count.
    now_s: f64,
    first_hop_s: Option<f64>,
    /// (time, energy) of recent hops for the moving-average baseline.
    energy_hist: VecDeque<(f64, f32)>,
    /// Last `SUPERFLUX_LAG + SUPERFLUX_WIN` hop energies, newest at the back.
    recent: VecDeque<f32>,
    last_onset_s: f64,
    /// Onset timestamps inside the IOI window.
    onsets: VecDeque<f64>,
}

impl BeatDetector {
    pub fn new(rate: u32) -> Self {
        let r = rate.max(8000) as f32;
        Self {
            rate: r,
            hp: Biquad::highpass(r, BAND_HP_HZ, std::f32::consts::FRAC_1_SQRT_2),
            lp: Biquad::lowpass(r, BAND_LP_HZ, BAND_LP_Q),
            pending: Vec::with_capacity(HOP),
            now_s: 0.0,
            first_hop_s: None,
            energy_hist: VecDeque::new(),
            recent: VecDeque::new(),
            last_onset_s: f64::NEG_INFINITY,
            onsets: VecDeque::new(),
        }
    }

    /// Feed one mono chunk; returns every onset that fired inside it.
    pub fn feed(&mut self, chunk: &[f32]) -> Vec<Onset> {
        let mut fired = Vec::new();
        for &s in chunk {
            let f = self.lp.process(self.hp.process(s));
            self.pending.push(f);
            if self.pending.len() == HOP {
                self.now_s += HOP as f64 / self.rate as f64;
                if let Some(o) = self.finish_hop() {
                    fired.push(o);
                }
                self.pending.clear();
            }
        }
        fired
    }

    fn finish_hop(&mut self) -> Option<Onset> {
        let energy = {
            let sum: f64 = self.pending.iter().map(|&s| (s as f64) * (s as f64)).sum();
            (sum / HOP as f64).sqrt() as f32
        };
        let t = self.now_s;
        let first = *self.first_hop_s.get_or_insert(t);

        self.energy_hist.push_back((t, energy));
        while self
            .energy_hist
            .front()
            .is_some_and(|&(ht, _)| t - ht > AVG_WINDOW_S)
        {
            self.energy_hist.pop_front();
        }

        // SuperFlux reference — computed BEFORE this hop enters the buffer:
        // the max of the WIN *oldest* entries, which sit LAG frames back,
        // just before this attack's smear.
        let sf_ref = if self.recent.len() >= SUPERFLUX_LAG + SUPERFLUX_WIN {
            self.recent
                .iter()
                .take(SUPERFLUX_WIN)
                .cloned()
                .fold(0.0f32, f32::max)
        } else {
            f32::INFINITY // not enough history → gate closed
        };
        self.recent.push_back(energy);
        if self.recent.len() > SUPERFLUX_LAG + SUPERFLUX_WIN {
            self.recent.pop_front();
        }

        // Calibration gate — elapsed since the FIRST hop, monotonic (the
        // oldest-buffered-age variant essentially never opened; diagnosed in
        // the disco-controller port).
        if t - first < AVG_WINDOW_S {
            return None;
        }
        if self.energy_hist.is_empty() {
            return None;
        }
        let avg =
            self.energy_hist.iter().map(|&(_, e)| e).sum::<f32>() / self.energy_hist.len() as f32;
        if avg <= 0.0 {
            return None;
        }

        let rising = energy > sf_ref * SUPERFLUX_MARGIN;
        let loud = energy > avg * ONSET_THRESHOLD;
        let free = t - self.last_onset_s >= ONSET_REFRACTORY_S;
        if !(rising && loud && free) {
            return None;
        }

        self.last_onset_s = t;
        self.onsets.push_back(t);
        while self
            .onsets
            .front()
            .is_some_and(|&ot| t - ot > IOI_WINDOW_S)
        {
            self.onsets.pop_front();
        }

        let ratio = energy / (avg * ONSET_THRESHOLD);
        let strength = ((ratio - 1.0) / (STRENGTH_FULL_RATIO - 1.0)).clamp(0.0, 1.0);
        let (bpm, confidence) = self.estimate();
        Some(Onset {
            strength,
            bpm,
            confidence,
        })
    }

    /// Median-IOI tempo + consistency confidence over the current window.
    fn estimate(&self) -> (Option<f32>, f32) {
        if self.onsets.len() < MIN_ONSETS_FOR_ESTIMATE {
            return (None, 0.0);
        }
        let times: Vec<f64> = self.onsets.iter().cloned().collect();
        let mut iois: Vec<f32> = times.windows(2).map(|w| (w[1] - w[0]) as f32).collect();
        if iois.is_empty() {
            return (None, 0.0);
        }
        iois.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = iois[iois.len() / 2];
        if median <= 0.0 {
            return (None, 0.0);
        }
        let consistent = iois
            .iter()
            .filter(|&&i| (i - median).abs() <= median * IOI_CONSISTENT_FRAC)
            .count();
        let confidence = consistent as f32 / iois.len() as f32;
        Some(fold_bpm(60.0 / median)).map(|b| (Some(b), confidence)).unwrap()
    }
}

/// Octave-fold a raw tempo into the trusted [`BPM_MIN`, `BPM_MAX`] band.
pub fn fold_bpm(mut bpm: f32) -> f32 {
    if !bpm.is_finite() || bpm <= 0.0 {
        return 0.0;
    }
    while bpm < BPM_MIN {
        bpm *= 2.0;
    }
    while bpm > BPM_MAX {
        bpm /= 2.0;
    }
    bpm
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48000;
    /// ~40 ms chunks, matching mic_capture's drain cadence.
    const CHUNK: usize = 1920;

    /// A synthetic track: a low noise floor with decaying 55 Hz kick bursts
    /// at the given beat interval. Deterministic (index-seeded "noise").
    fn kick_track(seconds: f64, beat_interval_s: f64, kick_amp: f32) -> Vec<f32> {
        let n = (seconds * RATE as f64) as usize;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f64 / RATE as f64;
            // Cheap deterministic noise floor (index hash), ±0.008.
            // u32 wrap is the point — on usize this never wraps and the
            // "noise" becomes a huge ramp with massive in-band energy (the
            // bug that made this fixture fire onsets between the kicks).
            let h = ((i as u32).wrapping_mul(2654435761) >> 16) as f32 / 65536.0;
            let mut s = (h - 0.5) * 0.016;
            let since_beat = t % beat_interval_s;
            if since_beat < 0.08 {
                let env = (1.0 - since_beat / 0.08) as f32;
                s += kick_amp
                    * env
                    * env
                    * (std::f64::consts::TAU * 55.0 * since_beat).sin() as f32;
            }
            out.push(s);
        }
        out
    }

    fn run(detector: &mut BeatDetector, samples: &[f32]) -> Vec<(usize, Onset)> {
        let mut fired = Vec::new();
        for (ci, chunk) in samples.chunks(CHUNK).enumerate() {
            for o in detector.feed(chunk) {
                fired.push((ci, o));
            }
        }
        fired
    }

    #[test]
    fn detects_a_120_bpm_kick_pattern_with_the_right_tempo() {
        let mut d = BeatDetector::new(RATE);
        let fired = run(&mut d, &kick_track(14.0, 0.5, 0.6));
        // 14 s at 2 beats/s, minus the 3 s calibration gate → ~22 beats.
        assert!(
            fired.len() >= 16 && fired.len() <= 26,
            "got {} onsets",
            fired.len()
        );
        let last = fired.last().unwrap().1;
        let bpm = last.bpm.expect("bpm should be locked by the end");
        assert!((bpm - 120.0).abs() < 4.0, "bpm read {bpm}");
        assert!(last.confidence > 0.6, "confidence {}", last.confidence);
    }

    #[test]
    fn a_slower_pattern_reads_slower() {
        let mut d = BeatDetector::new(RATE);
        let fired = run(&mut d, &kick_track(16.0, 0.75, 0.6)); // 80 BPM
        let last = fired.last().expect("onsets").1;
        let bpm = last.bpm.expect("bpm");
        assert!((bpm - 80.0).abs() < 4.0, "bpm read {bpm}");
    }

    #[test]
    fn silence_produces_no_onsets() {
        let mut d = BeatDetector::new(RATE);
        assert!(run(&mut d, &vec![0.0; RATE as usize * 8]).is_empty());
    }

    #[test]
    fn a_steady_tone_is_not_a_beat() {
        // Constant energy: never rises above its own baseline × threshold.
        let mut d = BeatDetector::new(RATE);
        let tone: Vec<f32> = (0..RATE as usize * 8)
            .map(|i| (std::f64::consts::TAU * 60.0 * i as f64 / RATE as f64).sin() as f32 * 0.5)
            .collect();
        let fired = run(&mut d, &tone);
        assert!(fired.len() <= 1, "steady tone fired {} onsets", fired.len());
    }

    #[test]
    fn nothing_fires_during_the_calibration_window() {
        let mut d = BeatDetector::new(RATE);
        // 2.5 s of kicks — entirely inside the 3 s gate.
        assert!(run(&mut d, &kick_track(2.5, 0.5, 0.8)).is_empty());
    }

    #[test]
    fn the_refractory_floor_limits_the_onset_rate() {
        let mut d = BeatDetector::new(RATE);
        // 375 ms kicks (160 BPM) — every one above the 300 ms refractory, so
        // the detected count must stay near the true beat count, never above.
        let fired = run(&mut d, &kick_track(12.0, 0.375, 0.6));
        let audible_beats = ((12.0 - AVG_WINDOW_S) / 0.375) as usize + 2;
        assert!(
            fired.len() <= audible_beats,
            "{} onsets for {} beats",
            fired.len(),
            audible_beats
        );
    }

    #[test]
    fn strength_is_normalised_and_scales_with_the_hit() {
        let mut d1 = BeatDetector::new(RATE);
        let soft = run(&mut d1, &kick_track(10.0, 0.5, 0.15));
        let mut d2 = BeatDetector::new(RATE);
        let hard = run(&mut d2, &kick_track(10.0, 0.5, 0.9));
        for (_, o) in soft.iter().chain(hard.iter()) {
            assert!((0.0..=1.0).contains(&o.strength), "strength {}", o.strength);
            assert!((0.0..=1.0).contains(&o.confidence));
        }
        let avg = |v: &[(usize, Onset)]| {
            v.iter().map(|(_, o)| o.strength).sum::<f32>() / v.len().max(1) as f32
        };
        assert!(
            avg(&hard) > avg(&soft),
            "hard {} vs soft {}",
            avg(&hard),
            avg(&soft)
        );
    }

    #[test]
    fn fold_bpm_brings_octaves_into_the_trusted_band() {
        assert_eq!(fold_bpm(120.0), 120.0);
        assert_eq!(fold_bpm(240.0), 120.0);
        assert_eq!(fold_bpm(30.0), 60.0);
        assert_eq!(fold_bpm(480.0), 120.0);
        assert_eq!(fold_bpm(0.0), 0.0);
        assert_eq!(fold_bpm(f32::NAN), 0.0);
    }

    #[test]
    fn bpm_is_none_until_enough_onsets_are_in_the_window() {
        let mut d = BeatDetector::new(RATE);
        // Just past calibration: the first few onsets can't have a median yet.
        let fired = run(&mut d, &kick_track(4.6, 0.5, 0.6));
        assert!(!fired.is_empty(), "expected at least one onset");
        assert!(
            fired[0].1.bpm.is_none(),
            "first onset must not claim a tempo"
        );
    }
}

