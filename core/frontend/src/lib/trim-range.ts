/**
 * Pure maths for the `rz`-style trim bar under the download buttons.
 *
 * The interaction model is QuickTime Player's: a range with two draggable
 * handles, everything OUTSIDE the range is discarded, and the range can be
 * typed in numerically (the YouTube Studio affordance). All of it is pure so
 * the bar, the keyboard path and the download hand-off compute the same range
 * from the same input.
 */
export interface Range {
  start: number;
  end: number;
}

/** Shortest range the UI allows, in seconds. Below this a drag is a mis-click
 *  rather than an intent, and a sub-second cut is not a useful clip. */
export const MIN_LEN = 1;

/** `m:ss`, or `h:mm:ss` once the duration warrants it. */
export function fmtClock(sec: number, withHours = false): string {
  const t = Math.max(0, Math.floor(sec));
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const s = t % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 || withHours ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

/**
 * Parse `1:23`, `1:02:03`, `83` or `1:23.5` to seconds.
 *
 * ⚠️ Returns null for anything it cannot read, so a half-typed field never
 * silently jumps the handle to 0 while the user is still typing.
 */
export function parseClock(text: string): number | null {
  const s = text.trim();
  if (!s || !/^\d{1,2}(:\d{1,2}){0,2}(\.\d+)?$/.test(s)) return null;
  const parts = s.split(":").map(Number);
  if (parts.some((n) => Number.isNaN(n))) return null;
  const secs = parts.reduce((acc, n) => acc * 60 + n, 0);
  return Number.isFinite(secs) ? secs : null;
}

/** Clamp a value into `[0, duration]`. */
export function clampTime(t: number, duration: number): number {
  if (!(duration > 0)) return 0;
  return t < 0 ? 0 : t > duration ? duration : t;
}

/**
 * Move one handle and keep the range valid.
 *
 * ⚠️ The handles PUSH rather than swap: dragging the start past the end leaves
 * a `MIN_LEN` range instead of flipping which handle you are holding — a swap
 * mid-drag makes the bar feel like it fights you.
 */
export function moveHandle(
  range: Range,
  which: "start" | "end",
  to: number,
  duration: number,
  minLen = MIN_LEN,
): Range {
  const t = clampTime(to, duration);
  if (which === "start") {
    const start = Math.min(t, Math.max(0, range.end - minLen));
    return { start, end: Math.max(range.end, start + minLen) };
  }
  const end = Math.max(t, Math.min(duration, range.start + minLen));
  return { start: Math.min(range.start, end - minLen), end };
}

/** Slide the whole range without changing its length (dragging the middle). */
export function moveRange(range: Range, deltaS: number, duration: number): Range {
  const len = range.end - range.start;
  let start = range.start + deltaS;
  if (start < 0) start = 0;
  if (start + len > duration) start = Math.max(0, duration - len);
  return { start, end: start + len };
}

/** A whole-video range — the state a freshly opened bar starts in. */
export function fullRange(duration: number): Range {
  return { start: 0, end: Math.max(MIN_LEN, duration) };
}

/** Is this range still the untouched whole video? */
export function isFullRange(range: Range, duration: number, eps = 0.05): boolean {
  return range.start <= eps && Math.abs(range.end - duration) <= eps;
}

/**
 * What to hand the download IPC.
 *
 * ⚠️ Returns `undefined` unless the user actually narrowed the range — an
 * untouched bar must produce the exact download it produced before this feature
 * existed, and the Rust side pins that the absent section leaves the argv alone.
 */
export function sectionFor(
  range: Range,
  duration: number,
  enabled: boolean,
): [number, number] | undefined {
  if (!enabled || !(duration > 0)) return undefined;
  if (isFullRange(range, duration)) return undefined;
  const start = clampTime(range.start, duration);
  const end = clampTime(range.end, duration);
  if (!(end - start >= MIN_LEN)) return undefined;
  return [round3(start), round3(end)];
}

function round3(v: number): number {
  return Math.round(v * 1000) / 1000;
}

/** Pointer x within a track → time. */
export function timeAtX(x: number, trackLeft: number, trackWidth: number, duration: number): number {
  if (!(trackWidth > 0) || !(duration > 0)) return 0;
  const f = (x - trackLeft) / trackWidth;
  return clampTime(f * duration, duration);
}

/** Time → percentage across the track, for CSS. */
export function pctAt(t: number, duration: number): number {
  if (!(duration > 0)) return 0;
  const p = (clampTime(t, duration) / duration) * 100;
  return Math.round(p * 1000) / 1000;
}

/**
 * Keyboard step for a handle. Coarse by default, fine with Shift — the
 * precision affordance QuickTime gives by letting you hold a handle.
 */
export function nudgeStep(duration: number, fine: boolean): number {
  if (fine) return 0.1;
  return duration > 600 ? 5 : 1;
}
