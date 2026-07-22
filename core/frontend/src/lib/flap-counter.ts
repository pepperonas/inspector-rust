// ── Split-flap number counter (airport departure board) ─────────────────────
// A raw rounded readout (dB / BPM) changes every animation frame → the number
// flickers and is hard to read. This steps the DISPLAYED integer one unit at a
// time toward the target; each step is a short vertical flip (like a flip-clock
// / airport departure board) followed by a readable dwell — a "stop". So the
// value mechanically flaps through the intermediate numbers and settles, and it
// stays legible the whole time. Pure logic (no DOM/canvas) → unit-testable.
import { easeOutCubic } from "./bpm-visual";

export interface Flap {
  /** the integer currently at rest (NaN until the first target arrives) */
  shown: number;
  /** flipping away from this value… */
  from: number;
  /** …toward this value */
  to: number;
  /** flip progress 0→1 (1 = settled, showing `shown`) */
  t: number;
  /** don't start the next flip until the clock ≥ this (the dwell / "stop") */
  hold: number;
}

export const newFlap = (): Flap => ({ shown: NaN, from: 0, to: 0, t: 1, hold: 0 });

/** duration of one digit flip, ms */
export const FLAP_FLIP_MS = 115;
/** pause after each flip, ms — the "stop" that keeps the number readable */
export const FLAP_DWELL_MS = 65;

/**
 * Advance a flap one frame toward `target` (NaN = blank, e.g. silence).
 * `now` and `dtMs` are milliseconds. `flipMs`/`dwellMs` are injectable so tests
 * stay deterministic.
 */
export function updateFlap(
  f: Flap,
  target: number,
  now: number,
  dtMs: number,
  flipMs: number = FLAP_FLIP_MS,
  dwellMs: number = FLAP_DWELL_MS,
): void {
  if (!Number.isFinite(target)) {
    f.shown = NaN;
    return;
  }
  if (!Number.isFinite(f.shown)) {
    // first real value: snap in, no flip from zero
    f.shown = f.from = f.to = target;
    f.t = 1;
    return;
  }
  if (f.t < 1) {
    f.t = Math.min(1, f.t + dtMs / flipMs);
    if (f.t >= 1) {
      f.shown = f.to;
      f.hold = now + dwellMs;
    }
  }
  if (f.t >= 1 && now >= f.hold && f.shown !== target) {
    f.from = f.shown;
    f.to = f.shown + Math.sign(target - f.shown);
    f.t = 0;
  }
}

/**
 * The integer to draw right now + its vertical scale (1 = flat card, 0 = edge-on
 * at the flip midpoint) — feed straight into a `ctx.scale(1, scaleY)`.
 */
export function flapView(f: Flap): { value: number; scaleY: number } {
  if (!Number.isFinite(f.shown)) return { value: NaN, scaleY: 1 };
  if (f.t >= 1) return { value: f.shown, scaleY: 1 };
  return f.t < 0.5
    ? { value: f.from, scaleY: 1 - easeOutCubic(f.t / 0.5) }
    : { value: f.to, scaleY: easeOutCubic((f.t - 0.5) / 0.5) };
}
