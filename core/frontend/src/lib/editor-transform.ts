//! Rotate/flip and crop-aspect geometry for the screenshot editor. Pure.
//!
//! The document stores annotations and crop in the coordinates of the
//! ORIENTED image (what you see). Turning or flipping maps every point
//! once (`transformDoc`) and updates `orient`; drawing renders the captured
//! image through `orient`. Text, step badges and emoji stay upright — only
//! their position follows the turn.

import type { Annotation, EditorDoc, Orient, Point, Rect } from "./editor-geometry";
import { boundsOf, normRect } from "./editor-geometry";
import type { Box4, OcrLine } from "./editor-ocr";

export type OrientOp = "cw" | "ccw" | "flipH" | "flipV";

export const IDENTITY: Orient = { rot: 0, flip: false };

export function isIdentity(o: Orient | undefined): boolean {
  return !o || (o.rot === 0 && !o.flip);
}

const norm = (r: number): Orient["rot"] => (((r % 360) + 360) % 360) as Orient["rot"];

/** Orientation after applying `op` to an image already shown as `o`.
 *  Forward map is R(rot)∘F^flip; flipH∘R(θ) = R(−θ)∘flipH and flipV = R(180)∘flipH. */
export function orientOf(o: Orient, op: OrientOp): Orient {
  switch (op) {
    case "cw":
      return { rot: norm(o.rot + 90), flip: o.flip };
    case "ccw":
      return { rot: norm(o.rot - 90), flip: o.flip };
    case "flipH":
      return { rot: norm(-o.rot), flip: !o.flip };
    case "flipV":
      return { rot: norm(180 - o.rot), flip: !o.flip };
  }
}

/** Size of the captured W×H image after `o`. */
export function orientedSize(W: number, H: number, o: Orient | undefined): { w: number; h: number } {
  return o && (o.rot === 90 || o.rot === 270) ? { w: H, h: W } : { w: W, h: H };
}

/** Map a point of a W×H (oriented) image through one operation. */
export function mapPoint(p: Point, W: number, H: number, op: OrientOp): Point {
  switch (op) {
    case "cw":
      return { x: H - p.y, y: p.x };
    case "ccw":
      return { x: p.y, y: W - p.x };
    case "flipH":
      return { x: W - p.x, y: p.y };
    case "flipV":
      return { x: p.x, y: H - p.y };
  }
}

/** Where captured-image point `p` (W×H = captured size) is shown under `o`. */
export function forwardPoint(p: Point, W: number, H: number, o: Orient): Point {
  let q = o.flip ? mapPoint(p, W, H, "flipH") : p;
  let w = W;
  let h = H;
  for (let i = 0; i < o.rot / 90; i++) {
    q = mapPoint(q, w, h, "cw");
    [w, h] = [h, w];
  }
  return q;
}

/** The captured-image point (W×H = captured size) shown at `q` under `o`. */
export function sourcePoint(q: Point, W: number, H: number, o: Orient): Point {
  let p = q;
  let { w, h } = orientedSize(W, H, o);
  for (let i = 0; i < o.rot / 90; i++) {
    p = mapPoint(p, w, h, "ccw");
    [w, h] = [h, w];
  }
  return o.flip ? mapPoint(p, w, h, "flipH") : p;
}

function mapRect(r: Rect, W: number, H: number, op: OrientOp): Rect {
  const a = mapPoint({ x: r.x, y: r.y }, W, H, op);
  const b = mapPoint({ x: r.x + r.w, y: r.y + r.h }, W, H, op);
  return normRect({ x: a.x, y: a.y, w: b.x - a.x, h: b.y - a.y });
}

function mapAnnotation(a: Annotation, W: number, H: number, op: OrientOp): Annotation {
  const m = (x: number, y: number) => mapPoint({ x, y }, W, H, op);
  switch (a.type) {
    case "arrow": {
      const p1 = m(a.x1, a.y1);
      const p2 = m(a.x2, a.y2);
      const out = { ...a, x1: p1.x, y1: p1.y, x2: p2.x, y2: p2.y };
      if (a.cx !== undefined && a.cy !== undefined) {
        const c = m(a.cx, a.cy);
        out.cx = c.x;
        out.cy = c.y;
      }
      return out;
    }
    case "line": {
      const p1 = m(a.x1, a.y1);
      const p2 = m(a.x2, a.y2);
      return { ...a, x1: p1.x, y1: p1.y, x2: p2.x, y2: p2.y };
    }
    case "rect":
    case "ellipse":
    case "highlight":
    case "blur":
    case "redact":
    case "spotlight":
      return { ...a, ...mapRect(a, W, H, op) };
    case "step":
    case "emoji": {
      const c = m(a.x, a.y);
      return { ...a, x: c.x, y: c.y };
    }
    case "text": {
      const b = boundsOf(a);
      const c = m(b.x + b.w / 2, b.y + b.h / 2);
      return { ...a, x: a.x + c.x - (b.x + b.w / 2), y: a.y + c.y - (b.y + b.h / 2) };
    }
    case "pen":
      return { ...a, points: a.points.map((p) => m(p.x, p.y)) };
  }
}

/** Apply one rotate/flip to the whole document. `W`×`H` = the CURRENT
 *  (oriented) image size. */
export function transformDoc(doc: EditorDoc, W: number, H: number, op: OrientOp): EditorDoc {
  return {
    ...doc,
    annotations: doc.annotations.map((a) => mapAnnotation(a, W, H, op)),
    crop: doc.crop ? mapRect(doc.crop, W, H, op) : null,
    orient: orientOf(doc.orient ?? IDENTITY, op),
  };
}

function orientBox(b: Box4, o: Orient): Box4 {
  let r: Rect = { x: b[0], y: b[1], w: b[2], h: b[3] };
  if (o.flip) r = mapRect(r, 1, 1, "flipH");
  for (let i = 0; i < o.rot / 90; i++) r = mapRect(r, 1, 1, "cw");
  return [r.x, r.y, r.w, r.h];
}

/** Text-recognition boxes (normalised, captured image) shown under `o`. */
export function orientLines(lines: OcrLine[], o: Orient | undefined): OcrLine[] {
  if (!o || isIdentity(o)) return lines;
  return lines.map((l) => ({ ...l, b: orientBox(l.b, o), w: l.w.map((w) => ({ ...w, b: orientBox(w.b, o) })) }));
}

// ---------------------------------------------------------------- crop aspect

export type AspectId = "free" | "1:1" | "4:3" | "3:2" | "16:9" | "9:16";
export const ASPECTS: ReadonlyArray<{ id: AspectId; ratio: number | null }> = [
  { id: "free", ratio: null },
  { id: "1:1", ratio: 1 },
  { id: "4:3", ratio: 4 / 3 },
  { id: "3:2", ratio: 3 / 2 },
  { id: "16:9", ratio: 16 / 9 },
  { id: "9:16", ratio: 9 / 16 },
];

export function aspectRatio(id: AspectId): number | null {
  return ASPECTS.find((a) => a.id === id)?.ratio ?? null;
}

/** A crop drag from `start` to `cur` held to `ratio` (w/h), growing toward
 *  the drag and shrunk (ratio kept) so it never leaves the image. */
export function constrainAspect(start: Point, cur: Point, ratio: number, W: number, H: number): Rect {
  const dx = cur.x - start.x;
  const dy = cur.y - start.y;
  const sx = dx < 0 ? -1 : 1;
  const sy = dy < 0 ? -1 : 1;
  let w = Math.max(Math.abs(dx), Math.abs(dy) * ratio);
  const maxW = sx > 0 ? W - start.x : start.x;
  const maxH = sy > 0 ? H - start.y : start.y;
  w = Math.max(0, Math.min(w, maxW, maxH * ratio));
  const h = w / ratio;
  return { x: sx > 0 ? start.x : start.x - w, y: sy > 0 ? start.y : start.y - h, w, h };
}

/** A crop at `ratio`: no crop → the largest centred one; an existing crop →
 *  same centre and area, shrunk and slid to stay inside the image. */
export function aspectCrop(crop: Rect | null, ratio: number, W: number, H: number): Rect {
  let w: number;
  let cx: number;
  let cy: number;
  if (crop && crop.w > 0 && crop.h > 0) {
    w = Math.sqrt(crop.w * crop.h * ratio);
    cx = crop.x + crop.w / 2;
    cy = crop.y + crop.h / 2;
  } else {
    w = Infinity;
    cx = W / 2;
    cy = H / 2;
  }
  w = Math.min(w, W, H * ratio);
  const h = w / ratio;
  const x = Math.min(Math.max(0, cx - w / 2), W - w);
  const y = Math.min(Math.max(0, cy - h / 2), H - h);
  return { x, y, w, h };
}
