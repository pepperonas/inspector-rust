//! Arrow geometry for the screenshot editor's four arrow styles (pure, no
//! canvas). The component draws what these functions return.

import type { ArrowAnnotation, Point } from "./editor-geometry";

/** Bend a fresh curved/marker arrow starts with: the midpoint pushed
 *  perpendicular (to the left of the direction of travel) by 18 % of the
 *  arrow's length, so the curve is visible right away. */
export function defaultBend(x1: number, y1: number, x2: number, y2: number): Point {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const len = Math.hypot(dx, dy);
  const mx = (x1 + x2) / 2;
  const my = (y1 + y2) / 2;
  if (len < 1) return { x: mx, y: my };
  const k = 0.18;
  return { x: mx + dy * k, y: my - dx * k };
}


/** Effective style: documents from before stage 1 carry none. */
function styleOf(a: ArrowAnnotation) {
  return a.style ?? "classic";
}

function hasBend(a: ArrowAnnotation): boolean {
  const s = styleOf(a);
  return (s === "curved" || s === "marker") && a.cx !== undefined && a.cy !== undefined;
}

/** Filled polygon for the straight styles (`classic`, `bold`). `classic` is
 *  the exact pre-stage-1 geometry. Bent styles fall back to their chord. */
export function arrowOutline(a: ArrowAnnotation): Point[] {
  const dx = a.x2 - a.x1;
  const dy = a.y2 - a.y1;
  const len = Math.hypot(dx, dy);
  if (len < 1) return [];
  const ux = dx / len;
  const uy = dy / len;
  const px = -uy;
  const py = ux;
  const w = a.width;
  let headLen: number;
  let headHalf: number;
  let shaftHalf: number;
  let tailHalf: number;
  if (styleOf(a) === "bold") {
    headLen = Math.min(len * 0.6, Math.max(26, w * 6));
    headHalf = headLen * 0.55;
    shaftHalf = Math.min(Math.max(w * 1.4, 3), headHalf * 0.6);
    tailHalf = Math.max(w * 0.35, 1);
  } else {
    headLen = Math.min(len * 0.6, Math.max(18, w * 5.5));
    headHalf = headLen * 0.55;
    shaftHalf = Math.min(Math.max(w * 1.0, 2), headHalf * 0.5);
    tailHalf = shaftHalf * 0.5;
  }
  const bx = a.x2 - ux * headLen;
  const by = a.y2 - uy * headLen;
  const at = (x: number, y: number, s: number): Point => ({ x: x + px * s, y: y + py * s });
  return [
    at(a.x1, a.y1, tailHalf),
    at(bx, by, shaftHalf),
    at(bx, by, headHalf),
    { x: a.x2, y: a.y2 },
    at(bx, by, -headHalf),
    at(bx, by, -shaftHalf),
    at(a.x1, a.y1, -tailHalf),
  ];
}

/** Sample the whole shaft from tail to tip (quadratic through the control
 *  point, or the straight chord). */
function samples(a: ArrowAnnotation, segments: number): Point[] {
  const out: Point[] = [];
  const bent = hasBend(a);
  for (let i = 0; i <= segments; i++) {
    const t = i / segments;
    if (bent) {
      const u = 1 - t;
      out.push({
        x: u * u * a.x1 + 2 * u * t * (a.cx as number) + t * t * a.x2,
        y: u * u * a.y1 + 2 * u * t * (a.cy as number) + t * t * a.y2,
      });
    } else {
      out.push({ x: a.x1 + (a.x2 - a.x1) * t, y: a.y1 + (a.y2 - a.y1) * t });
    }
  }
  return out;
}

function headLength(a: ArrowAnnotation): number {
  const chord = Math.hypot(a.x2 - a.x1, a.y2 - a.y1);
  return Math.min(chord * 0.4, Math.max(16, a.width * 4.5));
}

/** Index of the first sample that lies within `dist` (along the curve) of
 *  the tip, walking back from the tip. */
function headStartIndex(pts: Point[], dist: number): number {
  let acc = 0;
  for (let i = pts.length - 1; i > 0; i--) {
    acc += Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y);
    if (acc >= dist) return i - 1;
  }
  return 0;
}

/** Shaft polyline for the stroked styles. `curved` stops where its filled
 *  head starts; `marker` (open head) runs to the tip. */
export function arrowCurve(a: ArrowAnnotation, segments = 32): Point[] {
  const pts = samples(a, segments);
  if (styleOf(a) === "marker") return pts;
  return pts.slice(0, headStartIndex(pts, headLength(a) * 0.8) + 1);
}

/** Head triangle (or open chevron for `marker`), oriented on the curve's end
 *  tangent. */
export function arrowHead(a: ArrowAnnotation): { tip: Point; left: Point; right: Point; open: boolean } {
  const pts = samples(a, 32);
  const tip = { x: a.x2, y: a.y2 };
  const len = headLength(a);
  const from = pts[headStartIndex(pts, len)];
  let dx = tip.x - from.x;
  let dy = tip.y - from.y;
  const d = Math.hypot(dx, dy) || 1;
  dx /= d;
  dy /= d;
  const open = styleOf(a) === "marker";
  const half = len * (open ? 0.6 : 0.55);
  const bx = tip.x - dx * len;
  const by = tip.y - dy * len;
  return {
    tip,
    left: { x: bx - dy * half, y: by + dx * half },
    right: { x: bx + dy * half, y: by - dx * half },
    open,
  };
}

function distToSeg(p: Point, v: Point, w: Point): number {
  const l2 = (w.x - v.x) ** 2 + (w.y - v.y) ** 2;
  if (l2 === 0) return Math.hypot(p.x - v.x, p.y - v.y);
  const t = Math.max(0, Math.min(1, ((p.x - v.x) * (w.x - v.x) + (p.y - v.y) * (w.y - v.y)) / l2));
  return Math.hypot(p.x - (v.x + t * (w.x - v.x)), p.y - (v.y + t * (w.y - v.y)));
}

/** Distance from `p` to the arrow's shaft (curve or chord). */
export function distToArrow(a: ArrowAnnotation, p: Point): number {
  const pts = samples(a, hasBend(a) ? 32 : 1);
  let best = Infinity;
  for (let i = 1; i < pts.length; i++) best = Math.min(best, distToSeg(p, pts[i - 1], pts[i]));
  return best;
}

/** Bounding points of the drawn shaft (for bounds/marquee). */
export function arrowExtent(a: ArrowAnnotation): Point[] {
  return samples(a, hasBend(a) ? 32 : 1);
}

/** Apex (t = 0.5) of a bent arrow — where its mid handle sits. */
export function arrowApex(a: ArrowAnnotation): Point {
  return {
    x: 0.25 * a.x1 + 0.5 * (a.cx ?? (a.x1 + a.x2) / 2) + 0.25 * a.x2,
    y: 0.25 * a.y1 + 0.5 * (a.cy ?? (a.y1 + a.y2) / 2) + 0.25 * a.y2,
  };
}

export { hasBend as arrowHasBend };
