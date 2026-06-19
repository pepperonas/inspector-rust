/**
 * Pure helpers for the live `uptime` command's animated odometer. Kept out of
 * the component so they're unit-testable.
 */

/** Whole-unit breakdown of an uptime in seconds. */
export function uptimeBreakdown(totalSeconds: number): {
  days: number;
  hours: number;
  minutes: number;
  seconds: number;
} {
  const t = Math.max(0, Math.floor(totalSeconds));
  return {
    days: Math.floor(t / 86400),
    hours: Math.floor((t % 86400) / 3600),
    minutes: Math.floor((t % 3600) / 60),
    seconds: t % 60,
  };
}

/**
 * The **continuous** value (in `[0, 10)`) of the base-10 digit at place `10^power`
 * for `totalSeconds`. Used to position each odometer strip every animation
 * frame: a fractional result means the strip sits *between* two digits, so the
 * odometer scrolls smoothly. `power = 0` is the seconds-units digit (one full
 * cycle per 10 s); `power = -6` is the microsecond digit (a blur).
 */
export function odometerValue(totalSeconds: number, power: number): number {
  const scaled = totalSeconds / Math.pow(10, power);
  const m = scaled % 10;
  return m < 0 ? m + 10 : m;
}

/** Number of base-10 integer digits in `n` (≥ 1; `0`/invalid → 1). */
export function integerDigitCount(n: number): number {
  if (!Number.isFinite(n) || n < 1) return 1;
  return Math.floor(Math.log10(n)) + 1;
}

/** The list of digit *powers* (high → low) for an odometer with `intDigits`
 * integer places and `fracDigits` fractional places. e.g. `(2, 3)` →
 * `[1, 0, -1, -2, -3]`. */
export function odometerPowers(intDigits: number, fracDigits: number): number[] {
  const out: number[] = [];
  for (let p = intDigits - 1; p >= 0; p--) out.push(p);
  for (let p = -1; p >= -fracDigits; p--) out.push(p);
  return out;
}
