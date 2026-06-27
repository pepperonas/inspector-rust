//! `boom` — system-wide audio enhancement (macOS, driverless via Core-Audio
//! process taps). **Phase 1a (this module): the pure, unit-tested DSP engine +
//! presets + config.** The realtime Core-Audio tap / aggregate-device / IOProc
//! routing that feeds system audio through [`DspChain`] lands in `boom/macos.rs`
//! (phase 1b) — until then this is a fully-tested engine with no live audio.
//!
//! The DSP signal chain (per audio block, exactly what the IOProc will run):
//! **pre-amp → graphic EQ (peaking biquads) → volume boost → soft limiter**.
//! Everything here is allocation-free in the hot path (`process_*`) and pure, so
//! it's deterministically unit-testable without any audio hardware.

#![allow(dead_code)] // the realtime engine consuming this lands in phase 1b

use crate::db::DbHandle;
use serde::{Deserialize, Serialize};

/// Standard 10-band graphic-EQ centre frequencies (ISO octave bands, Hz).
pub const BANDS_10: [f32; 10] = [
    31.0, 62.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

/// Default per-band Q for a graphic EQ (≈ octave-wide peaking filters).
const BAND_Q: f64 = 1.41;
/// Soft-limiter output ceiling (linear). Keeps the signal just under full scale
/// so the boost can never hard-clip / blow the internal speakers.
pub const LIMITER_CEILING: f32 = 0.985;

// ── Biquad (RBJ peaking EQ, transposed direct-form II) ───────────────────────

#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Default for Biquad {
    fn default() -> Self {
        // Identity (passthrough) until configured.
        Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, z1: 0.0, z2: 0.0 }
    }
}

impl Biquad {
    /// Set peaking-EQ coefficients (RBJ audio-EQ cookbook). `gain_db` 0 → exact
    /// passthrough. State (`z1`/`z2`) is preserved so live tweaks don't click.
    pub fn set_peaking(&mut self, freq: f64, q: f64, gain_db: f64, sample_rate: f64) {
        if gain_db.abs() < 1e-6 || sample_rate <= 0.0 {
            self.b0 = 1.0;
            self.b1 = 0.0;
            self.b2 = 0.0;
            self.a1 = 0.0;
            self.a2 = 0.0;
            return;
        }
        let a = 10f64.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * (freq / sample_rate);
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q);

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a;

        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    pub fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// Soft-knee limiter: **exactly transparent** below the knee (0.8·ceiling), then
/// smoothly compresses everything above toward `±ceiling` — so normal-level
/// audio is untouched and only peaks near clipping are tamed (never exceeds
/// `±ceiling`). A plain `tanh(x)` would attenuate even quiet signals.
#[inline]
pub fn soft_limit(x: f32, ceiling: f32) -> f32 {
    if ceiling <= 0.0 {
        return 0.0;
    }
    let knee = ceiling * 0.8;
    let a = x.abs();
    if a <= knee {
        return x;
    }
    let range = ceiling - knee;
    let shaped = knee + range * ((a - knee) / range).tanh();
    x.signum() * shaped
}

/// Convert a decibel value to a linear gain factor.
#[inline]
pub fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

// ── DSP chain (pre-amp → EQ → boost → limiter) ───────────────────────────────

/// Live parameters pushed to the chain (the realtime engine will pass these
/// lock-free; here they're plain values).
#[derive(Clone, Debug)]
pub struct DspParams {
    pub preamp_db: f32,
    pub band_gains_db: Vec<f32>,
    /// Volume boost in percent (100 = unity, 200 = +6 dB make-up).
    pub boost_pct: f32,
}

pub struct DspChain {
    sample_rate: f64,
    freqs: Vec<f32>,
    bands_l: Vec<Biquad>,
    bands_r: Vec<Biquad>,
    preamp: f32,
    boost: f32,
    ceiling: f32,
}

impl DspChain {
    pub fn new(sample_rate: f64, freqs: &[f32]) -> Self {
        DspChain {
            sample_rate,
            freqs: freqs.to_vec(),
            bands_l: vec![Biquad::default(); freqs.len()],
            bands_r: vec![Biquad::default(); freqs.len()],
            preamp: 1.0,
            boost: 1.0,
            ceiling: LIMITER_CEILING,
        }
    }

    /// Recompute coefficients + gains from `params`. Not realtime-safe (does the
    /// trig); the engine calls this off the audio thread + swaps the chain in.
    pub fn set_params(&mut self, params: &DspParams) {
        self.preamp = db_to_linear(params.preamp_db);
        self.boost = (params.boost_pct.max(0.0)) / 100.0;
        for (i, &f) in self.freqs.iter().enumerate() {
            let g = params.band_gains_db.get(i).copied().unwrap_or(0.0) as f64;
            self.bands_l[i].set_peaking(f as f64, BAND_Q, g, self.sample_rate);
            self.bands_r[i].set_peaking(f as f64, BAND_Q, g, self.sample_rate);
        }
    }

    pub fn reset(&mut self) {
        for b in self.bands_l.iter_mut().chain(self.bands_r.iter_mut()) {
            b.reset();
        }
    }

    #[inline]
    fn process_sample(bands: &mut [Biquad], pre: f32, boost: f32, ceiling: f32, x: f32) -> f32 {
        let mut s = (x * pre) as f64;
        for b in bands.iter_mut() {
            s = b.process(s);
        }
        soft_limit(s as f32 * boost, ceiling)
    }

    /// Process an interleaved buffer in place. `channels` ≥ 1; channel 0 uses the
    /// L bank, channel 1 the R bank, any further channels reuse the L bank.
    pub fn process_interleaved(&mut self, buf: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        let (pre, boost, ceil) = (self.preamp, self.boost, self.ceiling);
        for frame in buf.chunks_mut(channels) {
            for (ch, sample) in frame.iter_mut().enumerate() {
                let bank = if ch == 1 { &mut self.bands_r } else { &mut self.bands_l };
                *sample = Self::process_sample(bank, pre, boost, ceil, *sample);
            }
        }
    }
}

// ── Presets ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize)]
pub struct Preset {
    pub name: String,
    pub group: &'static str,
    /// 10 band gains in dB (aligned to [`BANDS_10`]).
    pub gains: [f32; 10],
}

/// All built-in presets: genre EQ curves + per-device speaker-correction curves.
/// Returned to the UI; `gains_for` looks one up by name.
pub fn presets() -> Vec<Preset> {
    macro_rules! p {
        ($name:literal, $group:literal, [$($g:expr),*]) => {
            Preset { name: $name.to_string(), group: $group, gains: [$($g as f32),*] }
        };
    }
    vec![
        // group, name, 31 62 125 250 500 1k 2k 4k 8k 16k
        p!("Flat", "Genre", [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        p!("Bass Boost", "Genre", [7.0, 6.0, 4.5, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        p!("Treble Boost", "Genre", [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.5, 4.0, 5.5, 6.0]),
        p!("Vocal", "Genre", [-2.0, -1.5, 0.0, 1.5, 3.0, 4.0, 3.5, 2.0, 0.5, -1.0]),
        p!("Rock", "Genre", [4.0, 3.0, 1.5, -1.0, -1.5, 0.5, 2.0, 3.5, 4.0, 4.5]),
        p!("Pop", "Genre", [-1.0, 0.0, 1.5, 3.0, 3.5, 2.5, 0.5, -0.5, 1.0, 2.0]),
        p!("Electronic", "Genre", [5.0, 4.0, 1.5, 0.0, -1.0, 0.5, 1.5, 2.5, 4.0, 5.0]),
        p!("Hip-Hop", "Genre", [6.0, 5.0, 3.0, 1.0, -0.5, 0.5, 1.0, 1.5, 2.5, 3.0]),
        p!("Jazz", "Genre", [3.0, 2.0, 1.0, 1.5, -0.5, -0.5, 0.0, 1.5, 2.5, 3.0]),
        p!("Classical", "Genre", [3.5, 3.0, 2.0, 0.0, -1.0, -1.0, 0.0, 1.5, 2.5, 3.5]),
        p!("Acoustic", "Genre", [3.0, 2.5, 1.5, 0.5, 1.0, 1.5, 2.0, 2.5, 2.0, 1.5]),
        p!("Podcast", "Genre", [-4.0, -3.0, -1.0, 1.5, 3.5, 4.0, 3.5, 2.0, 0.0, -2.0]),
        p!("Movie", "Genre", [4.5, 3.5, 1.5, 0.5, 1.5, 2.0, 2.0, 2.5, 3.0, 3.5]),
        p!("Gaming", "Genre", [4.0, 3.0, 1.0, 0.5, 1.5, 2.5, 3.0, 3.5, 3.0, 2.5]),
        p!("Loudness", "Genre", [6.0, 4.5, 2.0, 0.0, -1.0, 0.0, 1.0, 3.0, 5.0, 6.5]),
        // Device / speaker correction.
        p!("MacBook Air Speaker", "Device", [6.5, 5.5, 3.0, 0.5, -0.5, 0.5, 1.5, 2.0, 2.5, 2.0]),
        p!("MacBook Pro Speaker", "Device", [3.5, 2.5, 1.0, 0.0, 0.0, 0.5, 1.0, 1.5, 2.0, 1.5]),
        p!("In-Ear", "Device", [2.5, 2.0, 1.0, 0.0, 0.5, 1.0, 1.0, 0.5, 1.5, 2.5]),
        p!("Over-Ear", "Device", [3.0, 2.0, 0.5, 0.0, -0.5, 0.0, 0.5, 1.5, 2.5, 2.0]),
        p!("Bluetooth", "Device", [3.5, 2.5, 1.0, 0.0, 0.0, 0.5, 1.5, 2.0, 1.5, 1.0]),
    ]
}

/// Band gains for a named preset (None if unknown / "Custom").
pub fn gains_for(name: &str) -> Option<[f32; 10]> {
    presets().into_iter().find(|p| p.name == name).map(|p| p.gains)
}

// ── Config ───────────────────────────────────────────────────────────────────

const KEY_CONFIG: &str = "boom.config";

/// Per-effect enhancement intensities (0..1). Stored now; consumed by the
/// realtime engine in phase 1b.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Effects {
    pub bass: f32,
    pub fidelity: f32,
    pub ambience: f32,
    pub clarity: f32,
    pub night: f32,
}

impl Default for Effects {
    fn default() -> Self {
        Effects { bass: 0.0, fidelity: 0.0, ambience: 0.0, clarity: 0.0, night: 0.0 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoomConfig {
    pub enabled: bool,
    pub preset: String,
    pub preamp_db: f32,
    #[serde(default)]
    pub band_gains_db: Vec<f32>,
    /// Volume boost percent (100 = system normal; > 100 = boost).
    pub boost_pct: f32,
    /// More aggressive limiting for distortion-free high boost.
    pub controlled_boost: bool,
    #[serde(default)]
    pub effects: Effects,
}

impl Default for BoomConfig {
    fn default() -> Self {
        BoomConfig {
            enabled: false,
            preset: "Flat".to_string(),
            preamp_db: 0.0,
            band_gains_db: vec![0.0; 10],
            boost_pct: 100.0,
            controlled_boost: true,
            effects: Effects::default(),
        }
    }
}

impl BoomConfig {
    pub fn load(db: &DbHandle) -> BoomConfig {
        crate::settings::get(db, KEY_CONFIG)
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<BoomConfig>(&s).ok())
            .map(|mut c| {
                c.normalize();
                c
            })
            .unwrap_or_default()
    }

    pub fn save(&self, db: &DbHandle) -> anyhow::Result<()> {
        let mut c = self.clone();
        c.normalize();
        let json = serde_json::to_string(&c)?;
        crate::settings::set(db, KEY_CONFIG, &json)?;
        Ok(())
    }

    /// Clamp to sane ranges + ensure 10 band gains.
    pub fn normalize(&mut self) {
        if self.band_gains_db.len() != 10 {
            self.band_gains_db.resize(10, 0.0);
        }
        for g in self.band_gains_db.iter_mut() {
            *g = g.clamp(-12.0, 12.0);
        }
        self.preamp_db = self.preamp_db.clamp(-12.0, 12.0);
        self.boost_pct = self.boost_pct.clamp(0.0, 300.0);
        let e = &mut self.effects;
        for v in [&mut e.bass, &mut e.fidelity, &mut e.ambience, &mut e.clarity, &mut e.night] {
            *v = v.clamp(0.0, 1.0);
        }
    }

    /// The DSP params this config feeds the chain.
    pub fn dsp_params(&self) -> DspParams {
        DspParams {
            preamp_db: self.preamp_db,
            band_gains_db: self.band_gains_db.clone(),
            boost_pct: self.boost_pct,
        }
    }
}

// ── Availability (macOS 14.2+ for the Process-Tap API) ───────────────────────

/// `true` if the host supports the Core-Audio process-tap engine (macOS 14.2+).
pub fn is_supported() -> bool {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|v| version_ge_14_2(v.trim()))
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Pure version comparison against 14.2 (e.g. "26.5.1" → true, "14.1" → false).
pub fn version_ge_14_2(v: &str) -> bool {
    let parts: Vec<u32> = v.split('.').filter_map(|p| p.parse().ok()).collect();
    let major = parts.first().copied().unwrap_or(0);
    let minor = parts.get(1).copied().unwrap_or(0);
    major > 14 || (major == 14 && minor >= 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_gate_matches_14_2_minimum() {
        assert!(version_ge_14_2("14.2"));
        assert!(version_ge_14_2("14.6.1"));
        assert!(version_ge_14_2("15.0"));
        assert!(version_ge_14_2("26.5.1"));
        assert!(!version_ge_14_2("14.1"));
        assert!(!version_ge_14_2("14.0"));
        assert!(!version_ge_14_2("13.6"));
        assert!(!version_ge_14_2("garbage"));
    }

    fn rms(buf: &[f32]) -> f32 {
        (buf.iter().map(|s| s * s).sum::<f32>() / buf.len() as f32).sqrt()
    }

    fn sine(freq: f32, sr: f32, n: usize, amp: f32) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / sr).sin())
            .collect()
    }

    #[test]
    fn biquad_zero_gain_is_passthrough() {
        let mut b = Biquad::default();
        b.set_peaking(1000.0, 1.41, 0.0, 48000.0);
        for x in [-0.7, -0.1, 0.0, 0.3, 0.95] {
            assert!((b.process(x) - x).abs() < 1e-9, "0 dB band must pass through");
        }
    }

    #[test]
    fn biquad_boost_increases_energy_at_center() {
        let sr = 48000.0;
        let input = sine(1000.0, sr, 4800, 0.2);
        let mut b = Biquad::default();
        b.set_peaking(1000.0, 1.41, 6.0, sr as f64);
        // Warm up + measure the steady-state tail.
        let out: Vec<f32> = input.iter().map(|&x| b.process(x as f64) as f32).collect();
        let tail_in = &input[2400..];
        let tail_out = &out[2400..];
        assert!(rms(tail_out) > rms(tail_in) * 1.3, "+6 dB at 1k should lift a 1k tone");
    }

    #[test]
    fn biquad_cut_reduces_energy_at_center() {
        let sr = 48000.0;
        let input = sine(1000.0, sr, 4800, 0.2);
        let mut b = Biquad::default();
        b.set_peaking(1000.0, 1.41, -6.0, sr as f64);
        let out: Vec<f32> = input.iter().map(|&x| b.process(x as f64) as f32).collect();
        assert!(rms(&out[2400..]) < rms(&input[2400..]) * 0.8);
    }

    #[test]
    fn soft_limit_never_exceeds_ceiling() {
        for x in [-100.0, -2.0, -0.5, 0.0, 0.5, 2.0, 100.0] {
            assert!(soft_limit(x, 0.985).abs() <= 0.985 + 1e-6);
        }
        // ~linear for small signals.
        assert!((soft_limit(0.01, 0.985) - 0.01).abs() < 1e-3);
    }

    #[test]
    fn db_to_linear_reference_points() {
        assert!((db_to_linear(0.0) - 1.0).abs() < 1e-6);
        assert!((db_to_linear(6.0) - 1.9953).abs() < 1e-3);
        assert!((db_to_linear(-6.0) - 0.5012).abs() < 1e-3);
    }

    #[test]
    fn flat_chain_is_near_passthrough() {
        let mut chain = DspChain::new(48000.0, &BANDS_10);
        chain.set_params(&DspParams { preamp_db: 0.0, band_gains_db: vec![0.0; 10], boost_pct: 100.0 });
        let mut buf = sine(440.0, 48000.0, 2048, 0.2);
        let orig = buf.clone();
        chain.process_interleaved(&mut buf, 2);
        for (a, b) in orig.iter().zip(&buf) {
            assert!((a - b).abs() < 1e-3, "flat chain should ~pass through");
        }
    }

    #[test]
    fn boost_amplifies_small_signals_before_the_limiter() {
        let mut chain = DspChain::new(48000.0, &BANDS_10);
        chain.set_params(&DspParams { preamp_db: 0.0, band_gains_db: vec![0.0; 10], boost_pct: 200.0 });
        let mut buf = vec![0.05f32; 64];
        chain.process_interleaved(&mut buf, 1);
        // 200 % ≈ ×2 (well below the limiter knee for a 0.05 signal).
        assert!(buf.iter().all(|&s| (s - 0.1).abs() < 0.01));
    }

    #[test]
    fn boost_output_is_limited_for_loud_input() {
        let mut chain = DspChain::new(48000.0, &BANDS_10);
        chain.set_params(&DspParams { preamp_db: 0.0, band_gains_db: vec![0.0; 10], boost_pct: 300.0 });
        let mut buf = vec![0.9f32; 64];
        chain.process_interleaved(&mut buf, 1);
        assert!(buf.iter().all(|&s| s.abs() <= LIMITER_CEILING + 1e-6));
    }

    #[test]
    fn presets_are_complete_and_flat_is_zero() {
        let ps = presets();
        assert_eq!(ps.len(), 20, "15 genre + 5 device presets");
        let flat = ps.iter().find(|p| p.name == "Flat").unwrap();
        assert!(flat.gains.iter().all(|&g| g == 0.0));
        assert_eq!(gains_for("Bass Boost").unwrap().len(), 10);
        assert!(gains_for("Nope").is_none());
        // Bass Boost lifts the low bands, not the highs.
        let bb = gains_for("Bass Boost").unwrap();
        assert!(bb[0] > 4.0 && bb[9] <= 0.5);
    }

    #[test]
    fn config_normalize_clamps_and_fills() {
        let mut c = BoomConfig {
            band_gains_db: vec![99.0, -99.0], // wrong length + out of range
            preamp_db: 50.0,
            boost_pct: 999.0,
            ..Default::default()
        };
        c.normalize();
        assert_eq!(c.band_gains_db.len(), 10);
        assert_eq!(c.band_gains_db[0], 12.0);
        assert_eq!(c.band_gains_db[1], -12.0);
        assert_eq!(c.preamp_db, 12.0);
        assert_eq!(c.boost_pct, 300.0);
    }
}
