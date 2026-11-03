import type { RecordTrackSpan } from "./ipc";

/**
 * The editor preview's mix (pure parts). The export mixes in the compositor; the preview
 * follows the same rules closely enough to judge levels: a track entry's level × its linear
 * fade envelope (`V/src/audio.rs` `fade_envelope`, fades that do not fit share the length in
 * proportion — `resolve_fade_samples`), and a limiter at the compositor's ceiling.
 */

/** The compositor's limiter ceiling (`V/src/audio.rs:91`), dBTP. */
export const LIMITER_CEILING_DB = -2;
/** The limiter starts bending this far below its ceiling. */
export const LIMITER_KNEE_DB = 3;
/** While a voiceover take runs the preview plays this much quieter (less bleed into the mic). */
export const VOICEOVER_DUCK_DB = -12;

/** dB → linear gain, unclamped (a WebAudio gain may boost above 0 dB). */
export function dbToGain(db: number): number {
  return Number.isFinite(db) ? 10 ** (db / 20) : 0;
}

/**
 * Linear fade factor at `t` ms into an entry `len` ms long: ramps up over `fadeIn`, down over
 * `fadeOut`; fades longer than the entry together are scaled down in proportion.
 */
export function fadeEnvelope(t: number, len: number, fadeIn: number, fadeOut: number): number {
  if (!(len > 0) || t < 0 || t > len) return 0;
  let fi = Math.max(0, fadeIn);
  let fo = Math.max(0, fadeOut);
  if (fi + fo > len) {
    const k = len / (fi + fo);
    fi *= k;
    fo *= k;
  }
  let v = 1;
  if (fi > 0 && t < fi) v = Math.min(v, t / fi);
  if (fo > 0 && len - t <= fo) v = Math.min(v, (len - t) / fo);
  return Math.max(0, v);
}

/** Linear gain of one track entry at programme `programmeMs` (0 outside it, or muted). */
export function gainAt(track: RecordTrackSpan, programmeMs: number): number {
  if (track.muted) return 0;
  const len = track.progEndMs - track.progStartMs;
  const t = programmeMs - track.progStartMs;
  if (t < 0 || t >= len) return 0;
  return dbToGain(track.gainDb) * fadeEnvelope(t, len, track.fadeInMs, track.fadeOutMs);
}

/** The entry of track `index` playing at `programmeMs` (start inclusive, end exclusive). */
export function entryAt(spans: readonly RecordTrackSpan[], index: number, programmeMs: number): RecordTrackSpan | null {
  return spans.find((s) => s.index === index && programmeMs >= s.progStartMs && programmeMs < s.progEndMs) ?? null;
}

/** Where in its file an entry is at `programmeMs` (entries play at 1×). */
export function fileMsAt(entry: RecordTrackSpan, programmeMs: number): number {
  return entry.fileStartMs + Math.max(0, programmeMs - entry.progStartMs);
}

/**
 * A `WaveShaperNode` curve: transparent up to `LIMITER_KNEE_DB` below the ceiling, then bent
 * smoothly (tanh) so no sample ever goes above `ceilingDb`. Inputs beyond ±1 are clamped by
 * the node itself to the curve's ends, which stay under the ceiling too.
 */
export function limiterCurve(ceilingDb = LIMITER_CEILING_DB, samples = 2048): Float32Array {
  const c = dbToGain(ceilingDb);
  const k = c * dbToGain(-LIMITER_KNEE_DB);
  const n = Math.max(2, Math.floor(samples));
  const out = new Float32Array(n);
  for (let i = 0; i < n; i += 1) {
    const x = (i * 2) / (n - 1) - 1;
    const a = Math.abs(x);
    const y = a <= k ? a : k + (c - k) * Math.tanh((a - k) / (c - k));
    out[i] = Math.sign(x) * Math.min(c, y);
  }
  return out;
}
