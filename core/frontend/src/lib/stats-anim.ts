/**
 * Pure math + formatting for the stats panel's value animation and heat glow
 * (v0.115.0). Kept out of the component so it's unit-testable — the component
 * owns only the imperative DOM writes (shared rAF tween engine).
 */

/** Tween duration per value change. MUST stay below the panel's `POLL_MS`
 *  (1500) so the number comes to rest between polls. */
export const STAT_TWEEN_MS = 600;

/** Heat thresholds — derived from the panel's existing `loadColor` scale
 *  (≥70 amber, ≥90 red). The glow ramps in across the amber band and reaches
 *  full "ausgelastet" heat exactly where the bar turns red. */
export const HEAT_START = 70;
export const HEAT_FULL = 90;

/** MD3-ish decelerate — fast start, soft landing. Matches the house tween
 *  curve (raspi-monitor `animate-number`, dashboard `animateStat`). */
export function easeOutCubic(t: number): number {
  const c = Math.max(0, Math.min(1, t));
  return 1 - Math.pow(1 - c, 3);
}

/** Eased interpolation between two values at progress `t` (0..1). */
export function tweenAt(from: number, to: number, t: number): number {
  return from + (to - from) * easeOutCubic(t);
}

/**
 * Heat intensity 0..1 for a load percentage: 0 below `HEAT_START`, linear ramp
 * across the amber band, capped at 1 from `HEAT_FULL` ("ausgelastet" — the
 * breathing/beam layer arms at exactly 1).
 */
export function heatLevel(pct: number): number {
  if (!Number.isFinite(pct)) return 0;
  return Math.max(0, Math.min(1, (pct - HEAT_START) / (HEAT_FULL - HEAT_START)));
}

/** "Ausgelastet": the bar sits in `loadColor`'s red zone. */
export function isHot(pct: number): boolean {
  return heatLevel(pct) >= 1;
}

/**
 * Unit-stable byte formatting for tweens: the UNIT and DECIMALS come from the
 * TARGET value (same 1024 ladder + digit rule as `humanBytes`), the NUMBER
 * from the interpolated value — so "1.4 GB → 2.1 GB" never flickers through
 * "1433 MB" mid-flight and the string width stays put (the raspi-monitor
 * lesson: precision follows the TARGET for the whole run).
 */
export function bytesFormatterFor(target: number): (v: number) => string {
  if (!Number.isFinite(target) || target <= 0) {
    // Degenerate target — fall back to formatting each value on its own
    // ladder; the tween toward 0 is short-lived anyway.
    return (v) => (v <= 0 ? "0 B" : bytesFormatterFor(v)(v));
  }
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let scaled = target;
  let i = 0;
  while (scaled >= 1024 && i < units.length - 1) {
    scaled /= 1024;
    i++;
  }
  const digits = i === 0 ? 0 : scaled < 10 ? 1 : 0;
  const div = Math.pow(1024, i);
  return (v) => `${Math.max(0, v / div).toFixed(digits)} ${units[i]}`;
}

/** Rate variant: `bytesFormatterFor` + "/s" (mirrors `humanRate`). */
export function rateFormatterFor(target: number): (v: number) => string {
  const f = bytesFormatterFor(target);
  return (v) => `${f(v)}/s`;
}
