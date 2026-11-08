//! Selection logic for the screenshot editor (pure): which element a click
//! means, click-cycling through stacked elements, marquee, layer order,
//! duplicate/paste/remove. The component wires pointer and keys to these.

import { distToArrow } from "./editor-arrows";
import {
  boundsOf,
  distToSegment,
  moveAnnotation,
  reflowSteps,
  type Annotation,
  type Point,
  type Rect,
} from "./editor-geometry";

export type HitCandidate = { id: string; dist: number; area: number; index: number };

/** Pointer distance (px) within which a second click counts as "the same
 *  spot" and cycles to the next element underneath. */
const CYCLE_RADIUS = 4;

function insideRect(p: Point, r: Rect, tol: number): boolean {
  return p.x >= r.x - tol && p.x <= r.x + r.w + tol && p.y >= r.y - tol && p.y <= r.y + r.h + tol;
}

/** Distance from `p` to a rectangle's outline. */
function distToRectEdge(p: Point, r: Rect): number {
  const inside = p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h;
  if (inside) return Math.min(p.x - r.x, r.x + r.w - p.x, p.y - r.y, r.y + r.h - p.y);
  const dx = Math.max(r.x - p.x, 0, p.x - (r.x + r.w));
  const dy = Math.max(r.y - p.y, 0, p.y - (r.y + r.h));
  return Math.hypot(dx, dy);
}

/** Radial approximation of the distance to an ellipse's outline; also
 *  reports whether `p` is inside. */
function ellipseRim(p: Point, r: Rect): { dist: number; inside: boolean } {
  const rx = r.w / 2;
  const ry = r.h / 2;
  const cx = r.x + rx;
  const cy = r.y + ry;
  if (rx <= 0 || ry <= 0) return { dist: Math.hypot(p.x - cx, p.y - cy), inside: false };
  const k = Math.hypot((p.x - cx) / rx, (p.y - cy) / ry);
  const d = Math.hypot(p.x - cx, p.y - cy);
  if (k === 0) return { dist: Math.min(rx, ry), inside: true };
  return { dist: Math.abs(d - d / k), inside: k <= 1 };
}

/** How far `p` is from `a` (0 = on it), or `null` if `p` misses it. Strokes
 *  count as 0 anywhere on their painted width. */
function distanceTo(a: Annotation, p: Point, tol: number): number | null {
  const stroke = (d: number, width: number) => (d <= tol + width / 2 ? Math.max(0, d - width / 2) : null);
  switch (a.type) {
    case "arrow":
      return stroke(distToArrow(a, p), a.width);
    case "line":
      return stroke(distToSegment(p, { x: a.x1, y: a.y1 }, { x: a.x2, y: a.y2 }), a.width);
    case "pen": {
      let best = Infinity;
      if (a.points.length === 1) best = Math.hypot(p.x - a.points[0].x, p.y - a.points[0].y);
      for (let i = 1; i < a.points.length; i++) best = Math.min(best, distToSegment(p, a.points[i - 1], a.points[i]));
      return stroke(best, a.width);
    }
    case "rect": {
      const r = boundsOf(a);
      if (a.fill === "filled") return insideRect(p, r, tol) ? 0 : null;
      return stroke(distToRectEdge(p, r), a.width);
    }
    case "ellipse": {
      const r = boundsOf(a);
      const rim = ellipseRim(p, r);
      if (a.fill === "filled") return rim.inside || rim.dist <= tol ? 0 : null;
      return stroke(rim.dist, a.width);
    }
    case "step":
      return Math.hypot(p.x - a.x, p.y - a.y) <= a.size + tol ? 0 : null;
    default:
      return insideRect(p, boundsOf(a), tol) ? 0 : null;
  }
}

/** Everything under `p`, best first: closest, then smallest, then newest. */
export function hitCandidates(anns: ReadonlyArray<Annotation>, p: Point, tol = 6): HitCandidate[] {
  const out: HitCandidate[] = [];
  anns.forEach((a, index) => {
    const dist = distanceTo(a, p, tol);
    if (dist === null) return;
    const b = boundsOf(a);
    out.push({ id: a.id, dist, area: Math.max(1, b.w * b.h), index });
  });
  return out.sort((x, y) => x.dist - y.dist || x.area - y.area || y.index - x.index);
}

/** The element a click at `p` means. A click within a few px of the previous
 *  pick (`prev`) moves on to the next element underneath, wrapping around. */
export function pickAt(
  anns: ReadonlyArray<Annotation>,
  p: Point,
  prev: { id: string; at: Point } | null,
  tol = 6,
): string | null {
  const c = hitCandidates(anns, p, tol);
  if (c.length === 0) return null;
  if (prev && Math.hypot(prev.at.x - p.x, prev.at.y - p.y) <= CYCLE_RADIUS) {
    const i = c.findIndex((h) => h.id === prev.id);
    if (i >= 0) return c[(i + 1) % c.length].id;
  }
  return c[0].id;
}

/** Ids whose bounds intersect `r`, in document order. */
export function marqueeIds(anns: ReadonlyArray<Annotation>, r: Rect): string[] {
  return anns
    .filter((a) => {
      const b = boundsOf(a);
      return b.x <= r.x + r.w && b.x + b.w >= r.x && b.y <= r.y + r.h && b.y + b.h >= r.y;
    })
    .map((a) => a.id);
}

/** Change the layer order of `ids` (relative order among them kept). */
export function reorder(
  anns: ReadonlyArray<Annotation>,
  ids: ReadonlyArray<string>,
  op: "forward" | "backward" | "front" | "back",
): Annotation[] {
  const set = new Set(ids);
  const moved = anns.filter((a) => set.has(a.id));
  const rest = anns.filter((a) => !set.has(a.id));
  if (op === "front") return [...rest, ...moved];
  if (op === "back") return [...moved, ...rest];
  const out = [...anns];
  if (op === "forward") {
    for (let i = out.length - 2; i >= 0; i--) {
      if (set.has(out[i].id) && !set.has(out[i + 1].id)) [out[i], out[i + 1]] = [out[i + 1], out[i]];
    }
  } else {
    for (let i = 1; i < out.length; i++) {
      if (set.has(out[i].id) && !set.has(out[i - 1].id)) [out[i], out[i - 1]] = [out[i - 1], out[i]];
    }
  }
  return out;
}

/** Append offset copies of `clip` on top with fresh ids; steps renumbered. */
export function pasteClip(
  anns: ReadonlyArray<Annotation>,
  clip: ReadonlyArray<Annotation>,
  offset: number,
  newId: () => string,
): { annotations: Annotation[]; ids: string[] } {
  const copies = clip.map((a) => ({ ...moveAnnotation(a, offset, offset), id: newId() }) as Annotation);
  return { annotations: reflowSteps([...anns, ...copies]), ids: copies.map((a) => a.id) };
}

/** Duplicate the elements `ids` (offset copies on top). */
export function duplicate(
  anns: ReadonlyArray<Annotation>,
  ids: ReadonlyArray<string>,
  offset: number,
  newId: () => string,
): { annotations: Annotation[]; ids: string[] } {
  const set = new Set(ids);
  return pasteClip(anns, anns.filter((a) => set.has(a.id)), offset, newId);
}

/** Remove `ids`; remaining step badges renumber. */
export function removeIds(anns: ReadonlyArray<Annotation>, ids: ReadonlyArray<string>): Annotation[] {
  const set = new Set(ids);
  return reflowSteps(anns.filter((a) => !set.has(a.id)));
}

/** Shift-drag: keep only the dominant axis. */
export function axisLock(dx: number, dy: number, on: boolean): { dx: number; dy: number } {
  if (!on) return { dx, dy };
  return Math.abs(dx) >= Math.abs(dy) ? { dx, dy: 0 } : { dx: 0, dy };
}

/** `+`/`-` on an element: stroke width ±1 (2–24) or text/step/emoji size ±2
 *  (8–200). Elements without either are returned unchanged. */
export function resizeBy(a: Annotation, dir: 1 | -1): Annotation {
  if ("width" in a && typeof a.width === "number") {
    return { ...a, width: Math.min(24, Math.max(2, a.width + dir)) } as Annotation;
  }
  if (a.type === "text" || a.type === "step" || a.type === "emoji") {
    return { ...a, size: Math.min(200, Math.max(8, a.size + 2 * dir)) };
  }
  return a;
}
