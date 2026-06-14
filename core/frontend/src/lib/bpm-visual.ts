/**
 * Pure helpers for the BPM detector's AAA beat visualization
 * (`components/BpmDetector.tsx`). Kept separate + unit-tested; the
 * component owns only the imperative canvas drawing + animation state.
 */

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

/** Clamp to [0, 1]. */
export function clamp01(v: number): number {
  return v < 0 ? 0 : v > 1 ? 1 : v;
}

/** Linear interpolation. */
export function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

/** Cubic ease-out — fast start, gentle settle. Used for shockwave radius. */
export function easeOutCubic(t: number): number {
  const u = 1 - clamp01(t);
  return 1 - u * u * u;
}

/** Parse `#rgb` / `#rrggbb` (or `rgb(...)`) into an {r,g,b}. Returns null on
 *  anything it can't read, so callers can fall back to a default. */
export function parseColor(input: string): Rgb | null {
  const s = input.trim();
  const hex = s.match(/^#?([0-9a-f]{3}|[0-9a-f]{6})$/i);
  if (hex) {
    let h = hex[1];
    if (h.length === 3) {
      h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
    }
    return {
      r: parseInt(h.slice(0, 2), 16),
      g: parseInt(h.slice(2, 4), 16),
      b: parseInt(h.slice(4, 6), 16),
    };
  }
  const rgb = s.match(/rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/i);
  if (rgb) {
    return { r: +rgb[1], g: +rgb[2], b: +rgb[3] };
  }
  return null;
}

/** Blend two colors. `t=0` → a, `t=1` → b. */
export function mixRgb(a: Rgb, b: Rgb, t: number): Rgb {
  const u = clamp01(t);
  return {
    r: Math.round(lerp(a.r, b.r, u)),
    g: Math.round(lerp(a.g, b.g, u)),
    b: Math.round(lerp(a.b, b.b, u)),
  };
}

/** CSS `rgba()` string for an {r,g,b} + alpha (alpha clamped to [0,1]). */
export function rgba(c: Rgb, alpha: number): string {
  return `rgba(${c.r}, ${c.g}, ${c.b}, ${clamp01(alpha)})`;
}

/**
 * Collapse a raw FFT magnitude array (`AnalyserNode.getByteFrequencyData`,
 * 0..255 per bin) into `barCount` **log-spaced** bars normalized to [0, 1].
 *
 * Log spacing matters for music: linear bins put almost all the visible
 * action in the bottom octave. Exponentially-growing windows give the bass
 * fine resolution and average the sparse high end, which is what reads as a
 * "real" spectrum analyzer. The DC bin (0) is skipped. `maxBinRatio` trims the
 * upper bins (mostly empty air / hiss above ~16 kHz) so the bars use their
 * full dynamic range.
 */
export function spectrumBars(
  freq: Uint8Array,
  barCount: number,
  maxBinRatio = 0.7,
): Float32Array {
  const out = new Float32Array(Math.max(0, barCount));
  if (barCount <= 0 || freq.length === 0) return out;
  const usable = Math.max(2, Math.floor(freq.length * clamp01(maxBinRatio)));
  for (let i = 0; i < barCount; i++) {
    const lo = Math.floor(usable ** (i / barCount));
    let hi = Math.floor(usable ** ((i + 1) / barCount));
    if (hi <= lo) hi = lo + 1;
    let sum = 0;
    let n = 0;
    for (let b = lo; b < hi && b < freq.length; b++) {
      sum += freq[b];
      n++;
    }
    out[i] = n > 0 ? sum / n / 255 : 0;
  }
  return out;
}

/**
 * Attack/release smoothing for a bar array (in place on `prev`): a bar that
 * jumps up snaps most of the way immediately (punchy), a bar that drops eases
 * down slowly (satisfying fall-off). Frame-rate-corrected via `dt` (seconds).
 */
export function smoothBars(
  prev: Float32Array,
  next: Float32Array,
  dt: number,
  attack = 0.6,
  release = 4.5,
): void {
  const a = clamp01(attack);
  // exponential release independent of frame rate
  const rel = 1 - Math.exp(-release * Math.max(0, dt));
  for (let i = 0; i < prev.length && i < next.length; i++) {
    prev[i] = next[i] > prev[i] ? lerp(prev[i], next[i], a) : lerp(prev[i], next[i], rel);
  }
}

/** Confidence → an RGB on a rose→amber→emerald ramp (mirrors the old bar). */
export function confidenceColor(confidence: number): Rgb {
  const rose: Rgb = { r: 244, g: 63, b: 94 };
  const amber: Rgb = { r: 245, g: 158, b: 11 };
  const emerald: Rgb = { r: 16, g: 185, b: 129 };
  const c = clamp01(confidence);
  return c < 0.5 ? mixRgb(rose, amber, c / 0.5) : mixRgb(amber, emerald, (c - 0.5) / 0.5);
}
