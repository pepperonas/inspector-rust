/**
 * Pure helpers for the Stats-panel history charts — building SVG line/area
 * paths from time-stamped series. No DOM; unit-tested. The component owns the
 * `<svg>` + styling.
 */

export interface SeriesPoint {
  /** Unix seconds (x). */
  t: number;
  /** Metric value (y). */
  v: number;
}

/**
 * Min/max of `values` with a small symmetric padding so the line doesn't touch
 * the chart edges. A flat series is padded around its single value so it renders
 * mid-height; an empty series returns `[0, 1]`.
 */
export function seriesExtent(values: number[], padFrac = 0.08): [number, number] {
  if (values.length === 0) return [0, 1];
  let min = values[0];
  let max = values[0];
  for (const v of values) {
    if (v < min) min = v;
    if (v > max) max = v;
  }
  if (min === max) {
    const pad = Math.abs(min) * 0.1 || 1;
    return [min - pad, max + pad];
  }
  const pad = (max - min) * padFrac;
  return [min - pad, max + pad];
}

function clamp(n: number, lo: number, hi: number): number {
  return n < lo ? lo : n > hi ? hi : n;
}

/**
 * SVG path `d` for a polyline mapping each point's `t` over `[tMin,tMax]` to x
 * and `v` over `[vMin,vMax]` to y (inverted: larger value = higher = smaller y)
 * inside a `w × h` box. Empty input → `""`.
 */
export function linePath(
  pts: SeriesPoint[],
  tMin: number,
  tMax: number,
  w: number,
  h: number,
  vMin: number,
  vMax: number,
): string {
  if (pts.length === 0) return "";
  const tSpan = tMax - tMin || 1;
  const vSpan = vMax - vMin || 1;
  const x = (t: number) => clamp(((t - tMin) / tSpan) * w, 0, w);
  const y = (v: number) => clamp(h - ((v - vMin) / vSpan) * h, 0, h);
  return pts
    .map((p, i) => `${i === 0 ? "M" : "L"}${x(p.t).toFixed(2)} ${y(p.v).toFixed(2)}`)
    .join(" ");
}

/**
 * Closed area path (the line, then down to the baseline and back) for a subtle
 * fill under the line. Empty input → `""`.
 */
export function areaPath(
  pts: SeriesPoint[],
  tMin: number,
  tMax: number,
  w: number,
  h: number,
  vMin: number,
  vMax: number,
): string {
  if (pts.length === 0) return "";
  const line = linePath(pts, tMin, tMax, w, h, vMin, vMax);
  const tSpan = tMax - tMin || 1;
  const x = (t: number) => clamp(((t - tMin) / tSpan) * w, 0, w);
  const x0 = x(pts[0].t);
  const x1 = x(pts[pts.length - 1].t);
  return `${line} L${x1.toFixed(2)} ${h.toFixed(2)} L${x0.toFixed(2)} ${h.toFixed(2)} Z`;
}
