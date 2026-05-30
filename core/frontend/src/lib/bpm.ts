/**
 * Real-time BPM detection from a stream of audio chunks.
 *
 * ## Algorithm
 *
 * Energy-based onset detection + inter-onset-interval (IOI) clustering.
 * Classic approach from Patin (2003) / Levitin chapter 6 / used by
 * Mixxx, RealtimeBPMAnalyzer.js, Spotify's web player. Battle-tested
 * for 4/4-time popular music — typical accuracy 85-95 % on tracks
 * with a clear kick drum.
 *
 *  1. Caller feeds chunks of **lowpass-filtered** audio (bass band
 *     20-150 Hz, isolating kick / bass-drum transients). The filter
 *     belongs in the audio graph (Web Audio `BiquadFilterNode`); this
 *     module assumes pre-filtered samples.
 *  2. Per chunk, compute RMS energy.
 *  3. Maintain a sliding-window moving average of energy.
 *  4. Onset = chunk-energy exceeds moving-average × threshold AND
 *     enough time has passed since the last onset (refractory period
 *     to suppress double-triggers).
 *  5. Record onset timestamps in a sliding window.
 *  6. Inter-onset intervals → median → BPM = 60 000 / median_ms.
 *  7. Octave-correction: fold into [60, 200] BPM by doubling /
 *     halving (so a half-speed reading on a fast track gets pulled up).
 *  8. Output is smoothed with an exponential moving average so the
 *     display doesn't flicker between adjacent integers.
 *
 *  Confidence is computed from IOI consistency:
 *    `1 - (stddev / median)`, clamped to [0, 1]. A track with steady
 *    quarter notes scores ~0.9; speech / noise scores ~0.1.
 *
 * ## Why not autocorrelation / spectral flux?
 *
 * Both are more accurate on syncopated material but 3-5× the CPU and
 * substantially more code. For the "listening to music nearby"
 * use-case this is overkill — the user mostly wants to know whether
 * the track is at 120 vs 140 vs 170, not whether it's at 138.4 vs
 * 138.6. Energy onsets give us that.
 */

export interface BpmEstimate {
  /** Smoothed BPM, or 0 if not enough data yet. */
  bpm: number;
  /** 0..1 — based on IOI consistency in the current window. */
  confidence: number;
  /** True only for the single `estimate()` call that immediately
   *  follows a `push()` which produced a beat. Drives the visual
   *  pulse animation in the UI. */
  beatJustFired: boolean;
}

/** Tuning knobs. Exported for tests + documentation; not user-facing. */
export const BPM_CONFIG = {
  /** How long to keep energy chunks for the moving-average baseline.
   *  3 s is short enough to adapt to a tempo change, long enough to
   *  not be fooled by a single loud chunk. */
  AVG_WINDOW_MS: 3000,
  /** Chunk energy must exceed `avg × threshold` to count as an onset.
   *  1.4 is a good balance: catches genuine kicks, rejects most
   *  background swells. */
  ONSET_THRESHOLD: 1.4,
  /** Minimum gap between successive onsets. v0.45.1: raised
   *  250 → 300 ms (max detectable 200 BPM, was 240) to better
   *  suppress room / BT-speaker echoes that arrive ~150-280 ms
   *  after the kick and otherwise show up as ghost onsets between
   *  real beats. Pop / rock / dance / metal all sit ≤ 200 BPM so
   *  the cap doesn't bite real music. */
  ONSET_REFRACTORY_MS: 300,
  /** Sliding window of onsets used to compute BPM. 6 s = enough beats
   *  for a stable median, short enough to adapt to a tempo change. */
  IOI_WINDOW_MS: 6000,
  /** BPM range we trust. Outside → octave-corrected (doubled / halved). */
  BPM_MIN: 60,
  BPM_MAX: 200,
  /** EMA weight for new BPM samples. v0.45.1: 0.20 → 0.12 — less
   *  twitchy display when echoes / BT artifacts inject occasional
   *  off-tempo IOIs. Lock-in still happens in ~6-8 s, the display
   *  just stops flickering ±3 BPM around the true value. */
  SMOOTHING_ALPHA: 0.12,
  /** Minimum number of onsets in the window before we trust the
   *  median. Below this, return BPM=0 / confidence=0. */
  MIN_ONSETS_FOR_ESTIMATE: 4,
  /** v0.45.1: after this long without enough onsets for an estimate,
   *  reset the displayed BPM to 0 — "music stopped, display the
   *  truth, don't show a stale value forever". 4 s is short enough
   *  to react to a pause + long enough that brief mic dropouts
   *  don't kill the lock. */
  STALE_RESET_MS: 4000,
  /** v0.45.1: when a new raw BPM estimate is within this many BPM
   *  of *half or double* the currently-locked smoothed BPM, snap to
   *  the existing octave instead of taking the multiplicative jump.
   *  Prevents the classic "120 ↔ 240" oscillation when irregular
   *  onsets push the median IOI across an octave boundary. */
  OCTAVE_SNAP_TOLERANCE_BPM: 8,
} as const;

interface EnergyChunk {
  time: number;
  energy: number;
}

export class BpmAnalyzer {
  private onsets: number[] = [];
  private energyHistory: EnergyChunk[] = [];
  private lastOnset = -Infinity;
  private smoothedBpm = 0;
  private justFiredBeat = false;
  /** Wall-clock of the last `estimate()` that actually produced a
   *  fresh raw BPM (had ≥ MIN_ONSETS_FOR_ESTIMATE onsets in window).
   *  Used to decay `smoothedBpm → 0` after STALE_RESET_MS of silence
   *  so the display drops back to "—" when the music stops, instead
   *  of misleadingly showing the last detected tempo forever. */
  private lastValidEstimateAt = -Infinity;

  /** Feed one chunk of (lowpass-filtered) audio samples.
   *  `samples` are float in [-1, 1]; `nowMs` is monotonically
   *  increasing milliseconds (typically `performance.now()`). */
  push(samples: Float32Array, nowMs: number): void {
    this.justFiredBeat = false;

    const energy = rms(samples);
    this.energyHistory.push({ time: nowMs, energy });
    // Drop chunks older than the moving-average window.
    while (
      this.energyHistory.length > 0 &&
      nowMs - this.energyHistory[0].time > BPM_CONFIG.AVG_WINDOW_MS
    ) {
      this.energyHistory.shift();
    }

    // Need at least a few chunks before the moving average is meaningful.
    if (this.energyHistory.length < 4) return;

    const avg =
      this.energyHistory.reduce((s, e) => s + e.energy, 0) /
      this.energyHistory.length;

    const triggered =
      energy > avg * BPM_CONFIG.ONSET_THRESHOLD &&
      nowMs - this.lastOnset >= BPM_CONFIG.ONSET_REFRACTORY_MS;

    if (triggered) {
      this.lastOnset = nowMs;
      this.onsets.push(nowMs);
      this.justFiredBeat = true;
      // Drop expired onsets.
      while (
        this.onsets.length > 0 &&
        nowMs - this.onsets[0] > BPM_CONFIG.IOI_WINDOW_MS
      ) {
        this.onsets.shift();
      }
    }
  }

  /** Current BPM estimate. Call after each `push()` or at the UI
   *  framerate (~60 fps) — cheap (no FFT, just a sort over <100
   *  intervals). */
  estimate(nowMs: number): BpmEstimate {
    // Drop expired onsets even if `push` wasn't called for a while.
    while (
      this.onsets.length > 0 &&
      nowMs - this.onsets[0] > BPM_CONFIG.IOI_WINDOW_MS
    ) {
      this.onsets.shift();
    }

    const beatJustFired = this.justFiredBeat;
    this.justFiredBeat = false; // consume the edge

    if (this.onsets.length < BPM_CONFIG.MIN_ONSETS_FOR_ESTIMATE) {
      // Sticky-value vs honest-reset balance: keep showing the last
      // smoothed value through brief onset droughts (so a momentary
      // mic dropout doesn't blank the display), BUT after
      // STALE_RESET_MS without any valid estimate, drop the display
      // to 0 — at that point the music has clearly stopped + a stale
      // number would be a lie.
      if (
        this.smoothedBpm > 0 &&
        nowMs - this.lastValidEstimateAt > BPM_CONFIG.STALE_RESET_MS
      ) {
        this.smoothedBpm = 0;
      }
      return { bpm: this.smoothedBpm, confidence: 0, beatJustFired };
    }

    // Build IOIs.
    const intervals: number[] = [];
    for (let i = 1; i < this.onsets.length; i++) {
      intervals.push(this.onsets[i] - this.onsets[i - 1]);
    }

    const sorted = [...intervals].sort((a, b) => a - b);
    const median = sorted[Math.floor(sorted.length / 2)];
    if (median <= 0) {
      return { bpm: this.smoothedBpm, confidence: 0, beatJustFired };
    }

    // Octave correction — pull a half-time or double-time reading
    // into the trusted BPM range.
    let rawBpm = 60000 / median;
    while (rawBpm < BPM_CONFIG.BPM_MIN) rawBpm *= 2;
    while (rawBpm > BPM_CONFIG.BPM_MAX) rawBpm /= 2;

    // **Octave snap.** If the new raw estimate sits suspiciously close
    // to half / double the currently-locked smoothed value, snap it
    // back to the locked octave. Without this, a few ghost onsets
    // injected between real beats can push the median IOI across an
    // octave boundary and the display flips 120 → 240 → 120 → …
    // The tolerance is symmetric: works for both "raw doubled too
    // high" (jumped up an octave) and "raw halved too low".
    if (this.smoothedBpm > 0) {
      const tol = BPM_CONFIG.OCTAVE_SNAP_TOLERANCE_BPM;
      if (Math.abs(rawBpm * 2 - this.smoothedBpm) < tol) {
        // raw is ~half of locked — promote raw to locked octave.
        rawBpm *= 2;
      } else if (Math.abs(rawBpm / 2 - this.smoothedBpm) < tol) {
        // raw is ~double of locked — demote raw to locked octave.
        rawBpm /= 2;
      }
    }

    // Smooth the visible BPM so the number doesn't flicker ±1 every
    // beat. Seed with the raw value on the first non-zero estimate.
    if (this.smoothedBpm === 0) {
      this.smoothedBpm = rawBpm;
    } else {
      this.smoothedBpm =
        this.smoothedBpm * (1 - BPM_CONFIG.SMOOTHING_ALPHA) +
        rawBpm * BPM_CONFIG.SMOOTHING_ALPHA;
    }
    this.lastValidEstimateAt = nowMs;

    // Confidence: tight IOI distribution → high confidence.
    const variance =
      intervals.reduce((s, i) => s + (i - median) ** 2, 0) / intervals.length;
    const stddev = Math.sqrt(variance);
    const confidence = Math.max(0, Math.min(1, 1 - stddev / median));

    return { bpm: this.smoothedBpm, confidence, beatJustFired };
  }

  /** Drop all state — useful when restarting the audio source. */
  reset(): void {
    this.onsets = [];
    this.energyHistory = [];
    this.lastOnset = -Infinity;
    this.smoothedBpm = 0;
    this.justFiredBeat = false;
    this.lastValidEstimateAt = -Infinity;
  }

  /** Current chunk RMS energy (the value compared against the avg).
   *  Exposed for the UI input-meter; doesn't affect detection. */
  currentEnergy(): number {
    return this.energyHistory.length > 0
      ? this.energyHistory[this.energyHistory.length - 1].energy
      : 0;
  }
}

function rms(samples: Float32Array): number {
  let sum = 0;
  for (let i = 0; i < samples.length; i++) {
    sum += samples[i] * samples[i];
  }
  return Math.sqrt(sum / samples.length);
}
