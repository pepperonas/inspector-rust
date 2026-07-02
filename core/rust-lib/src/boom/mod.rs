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
/// Soft-limiter output ceiling (linear). Full scale — output may use the whole
/// range; the soft knee below only shapes actual overs so the boost can never
/// hard-clip / blow the internal speakers.
pub const LIMITER_CEILING: f32 = 1.0;
/// The limiter is exactly transparent below `KNEE_FRACTION × ceiling`. This must
/// sit ABOVE normal music peaks: modern masters peak at ~0 dBFS, so the original
/// 0.8 knee (−2 dB) tanh-compressed the top 2 dB of every peak — flat-EQ boom
/// audibly dulled transients vs. bypass ("sounds worse with boom on"). At 0.95
/// (−0.45 dB) only genuine overs get shaped.
pub const KNEE_FRACTION: f32 = 0.95;

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

    /// Low-shelf (RBJ cookbook, slope S=1). `gain_db` 0 → passthrough. Boosts /
    /// cuts everything below `freq` — used by the **Bass** enhancement.
    pub fn set_lowshelf(&mut self, freq: f64, gain_db: f64, sample_rate: f64) {
        self.set_shelf(freq, gain_db, sample_rate, true);
    }

    /// High-shelf (RBJ cookbook, slope S=1). `gain_db` 0 → passthrough. Boosts /
    /// cuts everything above `freq` — used by the **Fidelity** ("air") effect.
    pub fn set_highshelf(&mut self, freq: f64, gain_db: f64, sample_rate: f64) {
        self.set_shelf(freq, gain_db, sample_rate, false);
    }

    fn set_shelf(&mut self, freq: f64, gain_db: f64, sample_rate: f64, low: bool) {
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
        let cos = w0.cos();
        // S = 1 → alpha = sin/2 · √2.
        let alpha = w0.sin() / 2.0 * std::f64::consts::SQRT_2;
        let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
        let (b0, b1, b2, a0, a1, a2) = if low {
            (
                a * ((a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha),
                (a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha,
            )
        } else {
            (
                a * ((a + 1.0) + (a - 1.0) * cos + two_sqrt_a_alpha),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - two_sqrt_a_alpha),
                (a + 1.0) - (a - 1.0) * cos + two_sqrt_a_alpha,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - two_sqrt_a_alpha,
            )
        };
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

/// Soft-knee limiter: **exactly transparent** below the knee
/// ([`KNEE_FRACTION`]·ceiling), then smoothly compresses everything above toward
/// `±ceiling` — so normal-level audio (incl. full-scale music peaks) is
/// untouched and only genuine overs are tamed (never exceeds `±ceiling`). A
/// plain `tanh(x)` would attenuate even quiet signals.
#[inline]
pub fn soft_limit(x: f32, ceiling: f32) -> f32 {
    if ceiling <= 0.0 {
        return 0.0;
    }
    let knee = ceiling * KNEE_FRACTION;
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

/// Perceptual volume taper for the system volume slider while boom is active:
/// amplitude = scalar² (the classic audio power taper, dB = 40·log₁₀ s —
/// 60 % ≈ −9 dB, 40 % ≈ −16 dB, 20 % ≈ −28 dB). The boom-Audio driver publishes
/// the volume control but no longer applies it (its stock curve was linear over
/// −64..0 dB: 40 % = −38 dB ≈ inaudible, 20 % = −51 dB — the "volume scaling
/// feels wrong" bug); the playback bridge applies this gain instead.
#[inline]
pub fn volume_gain(scalar: f32) -> f32 {
    let s = scalar.clamp(0.0, 1.0);
    s * s
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
    /// Enhancement-effect intensities (0..1).
    pub effects: Effects,
}

impl Default for DspParams {
    /// Flat / bypass-equivalent: 0 dB pre-amp, flat EQ, 100 % boost, effects off.
    fn default() -> Self {
        DspParams {
            preamp_db: 0.0,
            band_gains_db: vec![0.0; BANDS_10.len()],
            boost_pct: 100.0,
            effects: Effects::default(),
        }
    }
}

// ── Enhancement-effect tuning (one place) ────────────────────────────────────
const BASS_FREQ: f64 = 90.0; // low-shelf corner
const BASS_MAX_DB: f64 = 9.0; // at intensity 1.0
const CLARITY_FREQ: f64 = 3000.0; // presence peak
const CLARITY_Q: f64 = 0.9;
const CLARITY_MAX_DB: f64 = 6.0;
const FIDELITY_FREQ: f64 = 9000.0; // high-shelf "air"
const FIDELITY_MAX_DB: f64 = 6.0;
const AMBIENCE_MAX_WIDTH: f32 = 0.8; // width = 1 + ambience·0.8 (up to 1.8× side)
const NIGHT_THRESH_DB: f32 = -22.0;
const NIGHT_MAX_RATIO: f32 = 5.0; // up to 5:1 downward compression
const NIGHT_MAX_MAKEUP_DB: f32 = 6.0;
const NIGHT_ATTACK_MS: f32 = 5.0;
const NIGHT_RELEASE_MS: f32 = 150.0;

/// Soft-knee-free downward compressor for the **Night** effect (level the loud +
/// quiet so it's listenable at low volume). All precomputed; only `env` is state.
#[derive(Clone, Copy, Debug)]
struct Compressor {
    active: bool,
    thresh: f32,   // linear threshold
    exponent: f32, // 1 − 1/ratio
    makeup: f32,   // linear make-up gain
    atk: f32,      // envelope attack coefficient
    rel: f32,      // envelope release coefficient
    env: f32,      // running envelope (state)
}

impl Default for Compressor {
    fn default() -> Self {
        Compressor { active: false, thresh: 1.0, exponent: 0.0, makeup: 1.0, atk: 0.0, rel: 0.0, env: 0.0 }
    }
}

impl Compressor {
    /// Linked-stereo gain for the frame's `peak`, with attack/release smoothing.
    #[inline]
    fn gain(&mut self, peak: f32) -> f32 {
        let coeff = if peak > self.env { self.atk } else { self.rel };
        self.env = coeff * self.env + (1.0 - coeff) * peak;
        let g = if self.env > self.thresh {
            (self.thresh / self.env).powf(self.exponent)
        } else {
            1.0
        };
        g * self.makeup
    }
}

pub struct DspChain {
    sample_rate: f64,
    freqs: Vec<f32>,
    bands_l: Vec<Biquad>,
    bands_r: Vec<Biquad>,
    // Enhancement effects — per-channel shelf/peak filters.
    bass_l: Biquad,
    bass_r: Biquad,
    clarity_l: Biquad,
    clarity_r: Biquad,
    fidelity_l: Biquad,
    fidelity_r: Biquad,
    width: f32, // ambience stereo width; 1.0 = off
    comp: Compressor,
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
            bass_l: Biquad::default(),
            bass_r: Biquad::default(),
            clarity_l: Biquad::default(),
            clarity_r: Biquad::default(),
            fidelity_l: Biquad::default(),
            fidelity_r: Biquad::default(),
            width: 1.0,
            comp: Compressor::default(),
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

        // ── Enhancement effects (intensity 0..1; 0 → exact passthrough) ──────
        let fx = &params.effects;
        let bass_db = fx.bass.clamp(0.0, 1.0) as f64 * BASS_MAX_DB;
        self.bass_l.set_lowshelf(BASS_FREQ, bass_db, self.sample_rate);
        self.bass_r.set_lowshelf(BASS_FREQ, bass_db, self.sample_rate);

        let clarity_db = fx.clarity.clamp(0.0, 1.0) as f64 * CLARITY_MAX_DB;
        self.clarity_l.set_peaking(CLARITY_FREQ, CLARITY_Q, clarity_db, self.sample_rate);
        self.clarity_r.set_peaking(CLARITY_FREQ, CLARITY_Q, clarity_db, self.sample_rate);

        let fid_db = fx.fidelity.clamp(0.0, 1.0) as f64 * FIDELITY_MAX_DB;
        self.fidelity_l.set_highshelf(FIDELITY_FREQ, fid_db, self.sample_rate);
        self.fidelity_r.set_highshelf(FIDELITY_FREQ, fid_db, self.sample_rate);

        self.width = 1.0 + fx.ambience.clamp(0.0, 1.0) * AMBIENCE_MAX_WIDTH;

        let night = fx.night.clamp(0.0, 1.0);
        if night > 1e-4 {
            let ratio = 1.0 + night * (NIGHT_MAX_RATIO - 1.0);
            let fs = self.sample_rate as f32;
            self.comp.active = true;
            self.comp.thresh = db_to_linear(NIGHT_THRESH_DB);
            self.comp.exponent = 1.0 - 1.0 / ratio;
            self.comp.makeup = db_to_linear(night * NIGHT_MAX_MAKEUP_DB);
            self.comp.atk = (-1.0 / (fs * NIGHT_ATTACK_MS / 1000.0)).exp();
            self.comp.rel = (-1.0 / (fs * NIGHT_RELEASE_MS / 1000.0)).exp();
        } else {
            self.comp.active = false;
        }
    }

    pub fn reset(&mut self) {
        for b in self.bands_l.iter_mut().chain(self.bands_r.iter_mut()) {
            b.reset();
        }
        for b in [
            &mut self.bass_l,
            &mut self.bass_r,
            &mut self.clarity_l,
            &mut self.clarity_r,
            &mut self.fidelity_l,
            &mut self.fidelity_r,
        ] {
            b.reset();
        }
        self.comp.env = 0.0;
    }

    /// Process an interleaved buffer in place. Per frame: per-channel
    /// preamp→EQ→bass→clarity→fidelity, then ×boost, then the (linked) Night
    /// compressor, then Ambience stereo-widen, then the limiter. `channels` ≥ 1;
    /// channel 0 uses the L banks, channel 1 the R banks, further channels reuse L.
    pub fn process_interleaved(&mut self, buf: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        let pre = self.preamp;
        let boost = self.boost;
        let ceil = self.ceiling;
        let width = self.width;
        let comp_active = self.comp.active;

        for frame in buf.chunks_mut(channels) {
            // 1. per-channel filter chain + preamp + boost.
            for (ch, sample) in frame.iter_mut().enumerate() {
                let (bands, bass, clarity, fidelity) = if ch == 1 {
                    (&mut self.bands_r, &mut self.bass_r, &mut self.clarity_r, &mut self.fidelity_r)
                } else {
                    (&mut self.bands_l, &mut self.bass_l, &mut self.clarity_l, &mut self.fidelity_l)
                };
                let mut s = (*sample * pre) as f64;
                for b in bands.iter_mut() {
                    s = b.process(s);
                }
                s = bass.process(s);
                s = clarity.process(s);
                s = fidelity.process(s);
                *sample = s as f32 * boost;
            }

            // 2. Night compressor — linked across the frame's channels.
            if comp_active {
                let mut peak = 0.0f32;
                for &s in frame.iter() {
                    peak = peak.max(s.abs());
                }
                let g = self.comp.gain(peak);
                for s in frame.iter_mut() {
                    *s *= g;
                }
            }

            // 3. Ambience — mid/side stereo widening (needs L+R).
            if width != 1.0 && channels >= 2 {
                let (l, r) = (frame[0], frame[1]);
                let mid = 0.5 * (l + r);
                let side = 0.5 * (l - r) * width;
                frame[0] = mid + side;
                frame[1] = mid - side;
            }

            // 4. Limiter — last, so it tames any peaks the effects/boost added.
            for s in frame.iter_mut() {
                *s = soft_limit(*s, ceil);
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
            effects: self.effects.clone(),
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
    #[cfg(target_os = "windows")]
    {
        // The command surfaces on Windows; the panel shows an "install
        // Equalizer APO" card when the backend is missing (mirrors the macOS
        // driver-install card).
        true
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
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

#[cfg(target_os = "macos")]
pub(crate) mod macos;

/// (Re)sync the live audio engine to the saved config — start/stop the process
/// tap + push DSP params. Called after a config change + at startup. No-op on
/// non-macOS (and harmless if `enabled` is false). Phase 1b.
pub fn apply(db: &DbHandle) {
    let cfg = BoomConfig::load(db);
    #[cfg(target_os = "macos")]
    {
        // Clear a stale "boom Audio is the default output" state (after an unclean
        // exit) before (re)starting, so audio is never left silent. When boom is
        // toggled normally the default is already the user's real device, which
        // start_locked captures — so boom EQs whatever output they have selected.
        macos::reset_stale_default();
        macos::set_active(&cfg);
    }
    #[cfg(target_os = "windows")]
    {
        // Enabled or not, (re)write the include file — disabled renders as a
        // passthrough comment, so toggling boom off truly clears the EQ.
        if let Err(e) = windows::apply_config(&cfg) {
            tracing::warn!("boom(win): apply failed: {e}");
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = cfg;
    }
}

/// Live level-meter readout (input/output RMS + clip), for the UI.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct BoomLevels {
    pub input: f32,
    pub output: f32,
    pub clip: bool,
}

/// Current engine levels (zeros when not running / off macOS).
pub fn levels() -> BoomLevels {
    #[cfg(target_os = "macos")]
    {
        let (input, output, clip) = macos::levels();
        BoomLevels { input, output, clip }
    }
    #[cfg(not(target_os = "macos"))]
    {
        BoomLevels::default()
    }
}

// ── Windows backend — Equalizer APO config renderer (pure, cross-platform) ───
//
// On Windows boom drives **Equalizer APO** (the established user-mode APO that
// hooks the Windows audio engine) instead of a virtual driver: `apply` renders
// the saved config into an include file inside EqAPO's config directory and
// EqAPO hot-reloads it. The renderer mirrors the DspChain stage order
// (pre-amp → 10-band EQ → bass/clarity/fidelity shelves → ambience widen);
// the volume boost is folded into the preamp (20·log10), and — since EqAPO has
// no limiter — `controlled_boost` subtracts the largest positive gain as
// clipping headroom. The Night compressor has no EqAPO equivalent and is
// omitted (hidden in the Windows UI).

/// Name of the include file boom manages inside EqAPO's config directory.
pub const APO_INCLUDE_FILE: &str = "inspector-rust-boom.txt";
const APO_HEADER: &str = "# Managed by Inspector Rust (boom) — do not edit; rewritten on every change.";

/// Render the Equalizer APO include-file content for `cfg`. Pure + unit-tested.
pub fn apo_config_text(cfg: &BoomConfig) -> String {
    let mut out = String::new();
    out.push_str(APO_HEADER);
    out.push('\n');
    if !cfg.enabled {
        out.push_str("# boom is off — passthrough.\n");
        return out;
    }

    let e = &cfg.effects;
    let bass_db = (e.bass as f64 * BASS_MAX_DB) as f32;
    let clarity_db = (e.clarity as f64 * CLARITY_MAX_DB) as f32;
    let fidelity_db = (e.fidelity as f64 * FIDELITY_MAX_DB) as f32;

    // Boost folds into the preamp; controlled boost reserves headroom for the
    // largest positive gain anywhere in the chain (EqAPO has no limiter).
    let boost_db = if cfg.boost_pct > 0.0 { 20.0 * (cfg.boost_pct / 100.0).log10() } else { -60.0 };
    let max_pos_gain = cfg
        .band_gains_db
        .iter()
        .copied()
        .chain([cfg.preamp_db.max(0.0), bass_db, clarity_db, fidelity_db])
        .fold(0.0_f32, f32::max);
    let headroom = if cfg.controlled_boost { -max_pos_gain } else { 0.0 };
    let preamp = cfg.preamp_db + boost_db + headroom;
    out.push_str(&format!("Preamp: {preamp:.1} dB\n"));

    let mut n = 0;
    let mut filter = |out: &mut String, body: String| {
        n += 1;
        out.push_str(&format!("Filter {n}: {body}\n"));
    };
    for (freq, gain) in BANDS_10.iter().zip(cfg.band_gains_db.iter()) {
        if *gain != 0.0 {
            filter(&mut out, format!("ON PK Fc {freq:.0} Hz Gain {gain:.1} dB Q {BAND_Q}"));
        }
    }
    if bass_db > 0.0 {
        filter(&mut out, format!("ON LS Fc {BASS_FREQ:.0} Hz Gain {bass_db:.1} dB"));
    }
    if clarity_db > 0.0 {
        filter(
            &mut out,
            format!("ON PK Fc {CLARITY_FREQ:.0} Hz Gain {clarity_db:.1} dB Q {CLARITY_Q}"),
        );
    }
    if fidelity_db > 0.0 {
        filter(&mut out, format!("ON HS Fc {FIDELITY_FREQ:.0} Hz Gain {fidelity_db:.1} dB"));
    }
    // Ambience = mid/side widen, expressed as a simultaneous stereo mix
    // (EqAPO's Copy uses the pre-command channel values on the right side):
    //   L' = a·L + b·R,  R' = b·L + a·R  with a=(1+w)/2, b=(1-w)/2.
    if e.ambience > 0.0 {
        let w = 1.0 + e.ambience * AMBIENCE_MAX_WIDTH;
        let a = (1.0 + w) / 2.0;
        let b = (1.0 - w) / 2.0;
        out.push_str(&format!("Copy: L={a:.3}*L{b:+.3}*R R={b:+.3}*L+{a:.3}*R\n"));
    }
    out
}

/// Ensure `config.txt` contains the `Include:` line for our file — returns the
/// new content when an edit is needed, `None` when it's already present. Pure.
pub fn apo_ensure_include(config_txt: &str) -> Option<String> {
    let line = format!("Include: {APO_INCLUDE_FILE}");
    if config_txt.lines().any(|l| l.trim() == line) {
        return None;
    }
    let mut s = config_txt.to_string();
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    s.push_str(&line);
    s.push('\n');
    Some(s)
}

#[cfg(target_os = "windows")]
pub(crate) mod windows;

/// Tear the engine down (app quit) so the system output is never left altered.
pub fn shutdown() {
    #[cfg(target_os = "macos")]
    {
        macos::shutdown();
    }
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
    fn volume_gain_is_a_perceptual_power_taper() {
        assert_eq!(volume_gain(0.0), 0.0);
        assert_eq!(volume_gain(1.0), 1.0);
        assert!((volume_gain(0.5) - 0.25).abs() < 1e-6);
        // Monotonic + clamped.
        assert!(volume_gain(0.6) > volume_gain(0.4));
        assert_eq!(volume_gain(1.5), 1.0);
        assert_eq!(volume_gain(-0.2), 0.0);
        // Sanity of the dB feel: 40 % ≈ −16 dB (audible), NOT the old −38 dB.
        let db = 20.0 * volume_gain(0.4).log10();
        assert!(db > -18.0 && db < -14.0, "40% → {db} dB");
    }

    #[test]
    fn limiter_transparent_for_full_scale_music_peaks() {
        // Modern masters peak near 0 dBFS. The limiter must pass those bit-exact
        // (the old 0.8 knee compressed the top 2 dB of every peak → boom sounded
        // worse than bypass at flat settings). Only genuine overs are shaped.
        for x in [0.5f32, 0.85, 0.90, 0.94, -0.94] {
            assert_eq!(soft_limit(x, LIMITER_CEILING), x, "must be bit-transparent at {x}");
        }
        // A flat chain passes a loud music-level signal through unchanged.
        let mut chain = DspChain::new(48_000.0, &BANDS_10);
        chain.set_params(&DspParams::default());
        let mut buf = vec![0.94f32; 64];
        chain.process_interleaved(&mut buf, 1);
        assert!(buf.iter().all(|&s| (s - 0.94).abs() < 1e-4), "flat boom must be transparent");
        // Genuine overs are shaped, never exceed the ceiling.
        let over = soft_limit(1.4, LIMITER_CEILING);
        assert!(over > 0.95 && over <= LIMITER_CEILING + 1e-6);
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
        chain.set_params(&DspParams { preamp_db: 0.0, band_gains_db: vec![0.0; 10], boost_pct: 100.0, effects: Effects::default() });
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
        chain.set_params(&DspParams { preamp_db: 0.0, band_gains_db: vec![0.0; 10], boost_pct: 200.0, effects: Effects::default() });
        let mut buf = vec![0.05f32; 64];
        chain.process_interleaved(&mut buf, 1);
        // 200 % ≈ ×2 (well below the limiter knee for a 0.05 signal).
        assert!(buf.iter().all(|&s| (s - 0.1).abs() < 0.01));
    }

    #[test]
    fn boost_output_is_limited_for_loud_input() {
        let mut chain = DspChain::new(48000.0, &BANDS_10);
        chain.set_params(&DspParams { preamp_db: 0.0, band_gains_db: vec![0.0; 10], boost_pct: 300.0, effects: Effects::default() });
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

    // ── Enhancement effects ──────────────────────────────────────────────────

    fn fx_params(set: impl FnOnce(&mut Effects)) -> DspParams {
        let mut e = Effects::default();
        set(&mut e);
        DspParams { preamp_db: 0.0, band_gains_db: vec![0.0; 10], boost_pct: 100.0, effects: e }
    }

    fn process_mono(chain: &mut DspChain, input: &[f32]) -> Vec<f32> {
        let mut buf = input.to_vec();
        chain.process_interleaved(&mut buf, 1);
        buf
    }

    #[test]
    fn bass_effect_lifts_low_frequencies() {
        let mut chain = DspChain::new(48000.0, &BANDS_10);
        chain.set_params(&fx_params(|e| e.bass = 1.0));
        let input = sine(50.0, 48000.0, 4800, 0.15); // below the 90 Hz shelf corner
        let out = process_mono(&mut chain, &input);
        assert!(rms(&out[2400..]) > rms(&input[2400..]) * 1.5, "bass should boost 50 Hz");
    }

    #[test]
    fn fidelity_effect_lifts_high_frequencies() {
        let mut chain = DspChain::new(48000.0, &BANDS_10);
        chain.set_params(&fx_params(|e| e.fidelity = 1.0));
        let input = sine(14000.0, 48000.0, 4800, 0.15);
        let out = process_mono(&mut chain, &input);
        assert!(rms(&out[2400..]) > rms(&input[2400..]) * 1.3, "fidelity should boost highs");
    }

    #[test]
    fn clarity_effect_lifts_presence() {
        let mut chain = DspChain::new(48000.0, &BANDS_10);
        chain.set_params(&fx_params(|e| e.clarity = 1.0));
        let input = sine(3000.0, 48000.0, 4800, 0.15);
        let out = process_mono(&mut chain, &input);
        assert!(rms(&out[2400..]) > rms(&input[2400..]) * 1.2, "clarity should boost ~3 kHz");
    }

    #[test]
    fn ambience_effect_widens_stereo() {
        let mut chain = DspChain::new(48000.0, &BANDS_10);
        chain.set_params(&fx_params(|e| e.ambience = 1.0));
        // Constant stereo with L≠R → a non-zero side component to widen.
        let mut buf = Vec::new();
        for _ in 0..64 {
            buf.push(0.3);
            buf.push(0.1);
        }
        chain.process_interleaved(&mut buf, 2);
        let (l, r) = (buf[buf.len() - 2], buf[buf.len() - 1]);
        // mid 0.2 stays; side 0.1 × 1.8 = 0.18 → L≈0.38, R≈0.02.
        assert!(l > 0.30 && r < 0.10, "ambience widens L↔R (got L={l}, R={r})");
    }

    #[test]
    fn ambience_off_keeps_stereo_unchanged() {
        let mut chain = DspChain::new(48000.0, &BANDS_10);
        chain.set_params(&fx_params(|_| {})); // all effects 0
        let mut buf = vec![0.3f32, 0.1, 0.3, 0.1];
        chain.process_interleaved(&mut buf, 2);
        assert!((buf[0] - 0.3).abs() < 1e-3 && (buf[1] - 0.1).abs() < 1e-3, "no widen when ambience=0");
    }

    #[test]
    fn night_effect_compresses_loud_signal() {
        let mut chain = DspChain::new(48000.0, &BANDS_10);
        chain.set_params(&fx_params(|e| e.night = 1.0));
        // Loud, well above the -22 dB threshold → downward compression dominates.
        let mut buf = vec![0.5f32; 9600];
        chain.process_interleaved(&mut buf, 1);
        // After the envelope settles, the steady output is below the 0.5 input.
        assert!(buf[9599].abs() < 0.4, "night should compress a loud signal (got {})", buf[9599]);
    }

    // ── Equalizer APO renderer (Windows backend) ────────────────────────────

    #[test]
    fn apo_disabled_is_passthrough() {
        let cfg = BoomConfig::default();
        let t = apo_config_text(&cfg);
        assert!(t.contains("passthrough"));
        assert!(!t.contains("Preamp"));
        assert!(!t.contains("Filter"));
    }

    #[test]
    fn apo_flat_enabled_has_zero_preamp_and_no_filters() {
        let cfg = BoomConfig { enabled: true, ..BoomConfig::default() };
        let t = apo_config_text(&cfg);
        assert!(t.contains("Preamp: 0.0 dB"), "{t}");
        assert!(!t.contains("Filter"), "flat EQ must emit no filters: {t}");
        assert!(!t.contains("Copy:"));
    }

    #[test]
    fn apo_bands_render_as_peaking_filters() {
        let mut cfg = BoomConfig { enabled: true, ..BoomConfig::default() };
        cfg.band_gains_db[0] = 4.0; // 31 Hz
        cfg.band_gains_db[9] = -3.0; // 16 kHz
        cfg.controlled_boost = false;
        let t = apo_config_text(&cfg);
        assert!(t.contains("Filter 1: ON PK Fc 31 Hz Gain 4.0 dB Q 1.41"), "{t}");
        assert!(t.contains("Filter 2: ON PK Fc 16000 Hz Gain -3.0 dB Q 1.41"), "{t}");
    }

    #[test]
    fn apo_boost_folds_into_preamp_and_controlled_reserves_headroom() {
        let mut cfg = BoomConfig { enabled: true, ..BoomConfig::default() };
        cfg.boost_pct = 200.0; // +6.02 dB
        cfg.band_gains_db[3] = 5.0;
        cfg.controlled_boost = true;
        let t = apo_config_text(&cfg);
        // 0 preamp + 6.0 boost − 5.0 headroom ≈ 1.0 dB
        assert!(t.contains("Preamp: 1.0 dB"), "{t}");
        cfg.controlled_boost = false;
        let t = apo_config_text(&cfg);
        assert!(t.contains("Preamp: 6.0 dB"), "{t}");
    }

    #[test]
    fn apo_effects_map_to_shelves_peak_and_copy() {
        let mut cfg = BoomConfig { enabled: true, ..BoomConfig::default() };
        cfg.controlled_boost = false;
        cfg.effects = Effects { bass: 1.0, clarity: 0.5, fidelity: 1.0, ambience: 0.5, night: 1.0 };
        let t = apo_config_text(&cfg);
        assert!(t.contains("ON LS Fc 90 Hz Gain 9.0 dB"), "{t}");
        assert!(t.contains("ON PK Fc 3000 Hz Gain 3.0 dB Q 0.9"), "{t}");
        assert!(t.contains("ON HS Fc 9000 Hz Gain 6.0 dB"), "{t}");
        // ambience 0.5 → width 1.4 → a=1.2, b=−0.2
        assert!(t.contains("Copy: L=1.200*L-0.200*R R=-0.200*L+1.200*R"), "{t}");
        // night has no EqAPO equivalent — nothing rendered for it
        assert!(!t.to_lowercase().contains("night"));
    }

    #[test]
    fn apo_boost_zero_renders_deep_attenuation() {
        let cfg = BoomConfig { enabled: true, boost_pct: 0.0, ..BoomConfig::default() };
        let t = apo_config_text(&cfg);
        assert!(t.contains("Preamp: -60.0 dB"), "{t}");
    }

    #[test]
    fn apo_boost_below_100_attenuates() {
        let cfg = BoomConfig { enabled: true, boost_pct: 50.0, ..BoomConfig::default() };
        let t = apo_config_text(&cfg);
        assert!(t.contains("Preamp: -6.0 dB"), "{t}");
    }

    #[test]
    fn apo_negative_preamp_renders_signed() {
        let cfg = BoomConfig { enabled: true, preamp_db: -6.5, ..BoomConfig::default() };
        let t = apo_config_text(&cfg);
        // no positive gain anywhere → controlled boost reserves no headroom
        assert!(t.contains("Preamp: -6.5 dB"), "{t}");
    }

    #[test]
    fn apo_headroom_counts_effect_gains_too() {
        let mut cfg = BoomConfig { enabled: true, ..BoomConfig::default() };
        cfg.effects.bass = 1.0; // low-shelf +9 dB is the largest boost in the chain
        let t = apo_config_text(&cfg);
        assert!(t.contains("Preamp: -9.0 dB"), "{t}");
    }

    #[test]
    fn apo_extra_band_gains_are_ignored() {
        let mut cfg = BoomConfig { enabled: true, ..BoomConfig::default() };
        cfg.controlled_boost = false;
        cfg.band_gains_db = vec![3.0; 12]; // 2 more than the 10 EQ bands
        let t = apo_config_text(&cfg);
        assert_eq!(t.matches("ON PK").count(), 10, "{t}");
        assert!(!t.contains("Filter 11:"), "{t}");
    }

    #[test]
    fn apo_full_ambience_widen_matrix() {
        let mut cfg = BoomConfig { enabled: true, ..BoomConfig::default() };
        cfg.effects.ambience = 1.0; // width 1.8 → a = 1.4, b = −0.4
        let t = apo_config_text(&cfg);
        assert!(t.contains("Copy: L=1.400*L-0.400*R R=-0.400*L+1.400*R"), "{t}");
    }

    #[test]
    fn apo_commented_include_line_does_not_count() {
        let updated = apo_ensure_include("# Include: inspector-rust-boom.txt");
        assert!(updated.is_some(), "a commented-out include must not satisfy the check");
        assert!(updated.unwrap().ends_with("Include: inspector-rust-boom.txt\n"));
    }

    #[test]
    fn apo_ensure_include_appends_once() {
        let updated = apo_ensure_include("Preamp: -3 dB").unwrap();
        assert!(updated.ends_with("Include: inspector-rust-boom.txt\n"), "{updated}");
        assert!(updated.starts_with("Preamp: -3 dB\n"));
        // already present (any indentation) → no rewrite
        assert!(apo_ensure_include(&updated).is_none());
        assert!(apo_ensure_include("  Include: inspector-rust-boom.txt  ").is_none());
        // empty config.txt → just the include line
        assert_eq!(apo_ensure_include(""), Some("Include: inspector-rust-boom.txt\n".into()));
    }
}
