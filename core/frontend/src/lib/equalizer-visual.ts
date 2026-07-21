/**
 * Pure helpers for the `equalizer` spectrum visualizer
 * (`components/EqualizerVisualizer.tsx`). Kept separate + unit-tested; the
 * component owns only the imperative canvas drawing + animation state.
 *
 * Colour/mix/clamp helpers are reused from `bpm-visual.ts` — not duplicated —
 * since the equalizer shares the BPM detector's visual language (the `bpm`
 * feature is the blueprint). The band bucketing (`spectrumBars`) and
 * attack/release smoothing (`smoothBars`) also come from there.
 */

import { clamp01 } from "./bpm-visual";

/**
 * Peak-hold with slow fall-off (in place on `peaks`). A peak marker jumps
 * **instantly** to a bar that rose above it (so the marker sits on the crest),
 * otherwise it **falls slowly** at `fallPerSec` units/second — the classic
 * spectrum-analyzer peak dot. Frame-rate-corrected via `dt` (seconds). Each
 * peak is clamped to [0, 1] and never drops below its bar's current value.
 */
export function peakDecay(
  peaks: Float32Array,
  bars: Float32Array,
  dt: number,
  fallPerSec = 0.55,
): void {
  const fall = fallPerSec * Math.max(0, dt);
  const n = Math.min(peaks.length, bars.length);
  for (let i = 0; i < n; i++) {
    const bar = clamp01(bars[i]);
    if (bar >= peaks[i]) {
      peaks[i] = bar; // rising edge → snap to the crest
    } else {
      peaks[i] = Math.max(bar, peaks[i] - fall); // fall, but never below the bar
    }
  }
}

/** Geometry for a row of `count` evenly-spaced vertical bars across `width`. */
export interface BarGeometry {
  /** Width of one bar (px). */
  barW: number;
  /** Centre-to-centre distance between adjacent bars (px). */
  step: number;
  /** Left edge (px) of bar `i`. */
  x: (i: number) => number;
}

/**
 * Lay `count` bars across `width` with `gapRatio` of each slot left as the gap
 * between bars (0 = touching, 0.5 = half-gap). Deterministic + pure so the
 * component never recomputes layout in a way that thrashes; returns safe zeros
 * for a non-positive `count`/`width`.
 */
export function barGeometry(width: number, count: number, gapRatio = 0.34): BarGeometry {
  if (count <= 0 || width <= 0) {
    return { barW: 0, step: 0, x: () => 0 };
  }
  const step = width / count;
  const gap = clamp01(gapRatio);
  const barW = step * (1 - gap);
  const pad = (step - barW) / 2; // centre each bar in its slot
  return { barW, step, x: (i: number) => i * step + pad };
}
