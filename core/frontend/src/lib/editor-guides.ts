//! Alignment guides while moving elements in the screenshot editor: the
//! moved selection's edges and centre snap to those of other elements and
//! of the screenshot itself, and the lines it snapped to are reported for
//! drawing. Resizing snaps the dragged edges (or line end) the same way. Pure.

import { boundsOf, type Annotation, type Handle, type Rect } from "./editor-geometry";

/** A full-length guide line: vertical at x (`axis: "x"`) or horizontal at y. */
export type Guide = { axis: "x" | "y"; at: number };

const xs = (r: Rect) => [r.x, r.x + r.w / 2, r.x + r.w];
const ys = (r: Rect) => [r.y, r.y + r.h / 2, r.y + r.h];

/** Best correction on one axis: the smallest `target - own` within `tol`. */
function bestShift(own: number[], targets: number[], tol: number): number | null {
  let best: number | null = null;
  for (const o of own) {
    for (const t of targets) {
      const d = t - o;
      if (Math.abs(d) <= tol && (best === null || Math.abs(d) < Math.abs(best))) best = d;
    }
  }
  return best;
}

/** Snap a move of `moving` by (dx,dy) to the lines of `targets`. `free`
 *  says which axes may be corrected (a ⇧-locked axis stays as it is). */
export function snapMove(
  moving: Rect,
  targets: ReadonlyArray<Rect>,
  dx: number,
  dy: number,
  tol: number,
  free: { x: boolean; y: boolean } = { x: true, y: true },
): { dx: number; dy: number; guides: Guide[] } {
  const tx = targets.flatMap(xs);
  const ty = targets.flatMap(ys);
  const moved = { ...moving, x: moving.x + dx, y: moving.y + dy };
  const sx = free.x ? bestShift(xs(moved), tx, tol) : null;
  const sy = free.y ? bestShift(ys(moved), ty, tol) : null;
  const outDx = dx + (sx ?? 0);
  const outDy = dy + (sy ?? 0);
  const final = { ...moving, x: moving.x + outDx, y: moving.y + outDy };
  const guides: Guide[] = [];
  const near = (a: number, b: number) => Math.abs(a - b) < 0.5;
  if (sx !== null) {
    for (const v of xs(final)) if (tx.some((t) => near(t, v)) && !guides.some((g) => g.axis === "x" && near(g.at, v))) guides.push({ axis: "x", at: v });
  }
  if (sy !== null) {
    for (const v of ys(final)) if (ty.some((t) => near(t, v)) && !guides.some((g) => g.axis === "y" && near(g.at, v))) guides.push({ axis: "y", at: v });
  }
  return { dx: outDx, dy: outDy, guides };
}

/** The positions a resize handle moves, in the element's current geometry:
 *  the dragged box edges, or the dragged end of a line/arrow. `null` when the
 *  handle has no edge to align — text/step/emoji size, an arrow's bend, and a
 *  picture corner (its aspect lock would move the edge off the guide again). */
export function resizeAnchors(a: Annotation, handle: Handle): { xs: number[]; ys: number[] } | null {
  if (a.type === "arrow" || a.type === "line") {
    if (handle === "start") return { xs: [a.x1], ys: [a.y1] };
    if (handle === "end") return { xs: [a.x2], ys: [a.y2] };
    return null;
  }
  if (a.type === "text" || a.type === "step" || a.type === "emoji") return null;
  if (a.type === "image" && handle.length === 2) return null;
  if (handle === "start" || handle === "end" || handle === "mid") return null;
  const b = boundsOf(a);
  const xs: number[] = [];
  const ys: number[] = [];
  if (handle.includes("w")) xs.push(b.x);
  if (handle.includes("e")) xs.push(b.x + b.w);
  if (handle.includes("n")) ys.push(b.y);
  if (handle.includes("s")) ys.push(b.y + b.h);
  return { xs, ys };
}

/** Snap a resize drag of (dx,dy): the moving anchors (see `resizeAnchors`)
 *  snap to the edges and centres of `targets`. */
export function snapResize(
  anchors: { xs: number[]; ys: number[] },
  targets: ReadonlyArray<Rect>,
  dx: number,
  dy: number,
  tol: number,
): { dx: number; dy: number; guides: Guide[] } {
  const sx = anchors.xs.length ? bestShift(anchors.xs.map((v) => v + dx), targets.flatMap(xs), tol) : null;
  const sy = anchors.ys.length ? bestShift(anchors.ys.map((v) => v + dy), targets.flatMap(ys), tol) : null;
  const outDx = dx + (sx ?? 0);
  const outDy = dy + (sy ?? 0);
  const guides: Guide[] = [];
  const tx = targets.flatMap(xs);
  const ty = targets.flatMap(ys);
  const near = (a: number, b: number) => Math.abs(a - b) < 0.5;
  if (sx !== null) for (const v of anchors.xs.map((x) => x + outDx)) if (tx.some((t) => near(t, v)) && !guides.some((g) => g.axis === "x" && near(g.at, v))) guides.push({ axis: "x", at: v });
  if (sy !== null) for (const v of anchors.ys.map((y) => y + outDy)) if (ty.some((t) => near(t, v)) && !guides.some((g) => g.axis === "y" && near(g.at, v))) guides.push({ axis: "y", at: v });
  return { dx: outDx, dy: outDy, guides };
}
