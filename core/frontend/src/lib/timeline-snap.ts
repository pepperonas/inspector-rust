import type { RecordEditorView } from "./ipc";

/**
 * Timeline snapping (stage 4). Everything here is programme ms: a drag snaps on the programme
 * clock and the caller maps the snapped position to source through Rust (R12) — this module
 * never touches source time.
 */

/** A dragged edge closer than this many px to a target snaps onto it. */
export const SNAP_THRESHOLD_PX = 8;

/** What a drag snaps to: the targets and the timeline's scale. */
export interface Snapping {
  targets: readonly number[];
  /** px per programme ms. */
  pxPerMs: number;
}

/** `ms` onto the nearest target within `thresholdPx` (inclusive); `target` is `null` when none is. */
export function snap(
  ms: number,
  targets: readonly number[],
  pxPerMs: number,
  thresholdPx = SNAP_THRESHOLD_PX,
): { ms: number; target: number | null } {
  if (!(pxPerMs > 0) || !Number.isFinite(ms)) return { ms, target: null };
  let best: number | null = null;
  let bestPx = Infinity;
  for (const t of targets) {
    const d = Math.abs(t - ms) * pxPerMs;
    if (d <= thresholdPx && d < bestPx) {
      best = t;
      bestPx = d;
    }
  }
  return best == null ? { ms, target: null } : { ms: best, target: best };
}

/**
 * The snap targets: the programme's start and end and every clip edge (Rust's spans). Not the
 * playhead and not other regions.
 */
export function snapTargets(view: Pick<RecordEditorView, "programmeMs" | "clips">): number[] {
  const set = new Set<number>([0, Math.max(0, view.programmeMs)]);
  for (const c of view.clips) {
    set.add(c.progStartMs);
    set.add(c.progEndMs);
  }
  return [...set].sort((a, b) => a - b);
}

/**
 * The shift that puts whichever of `edges` lies closest to a target onto it (a moved block
 * snaps with its start or its end, whichever is nearer).
 */
export function snapShift(
  edges: readonly number[],
  targets: readonly number[],
  pxPerMs: number,
  thresholdPx = SNAP_THRESHOLD_PX,
): { shiftMs: number; target: number | null } {
  let out = { shiftMs: 0, target: null as number | null };
  let bestPx = Infinity;
  for (const e of edges) {
    const s = snap(e, targets, pxPerMs, thresholdPx);
    if (s.target == null) continue;
    const d = Math.abs(s.ms - e) * pxPerMs;
    if (d < bestPx) {
      bestPx = d;
      out = { shiftMs: s.ms - e, target: s.target };
    }
  }
  return out;
}

/** The target one of `edges` sits on (within half a ms): where the guide line is drawn. */
export function snapGuide(edges: readonly number[], targets: readonly number[]): number | null {
  for (const e of edges) for (const t of targets) if (Math.abs(e - t) < 0.5) return t;
  return null;
}
