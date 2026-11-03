/**
 * Geometry of the free-area picker (window `record-region`). Everything is in window-local
 * CSS pixels, which on macOS are points — the same space the backend clamps and records.
 * Pure: the component only feeds pointer positions in and draws what comes out.
 */

export interface RegionRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Bounds {
  w: number;
  h: number;
}

/** Smallest accepted area per side (points) — mirrors `MIN_REGION_PT` in record/core.rs. */
export const MIN_REGION = 64;

export type Handle = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";
export const HANDLES: readonly Handle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** The rectangle spanned by a drag from `a` to `b`, whichever way it went, kept on screen. */
export function fromDrag(a: { x: number; y: number }, b: { x: number; y: number }, bounds: Bounds): RegionRect {
  const x0 = clamp(Math.min(a.x, b.x), 0, bounds.w);
  const y0 = clamp(Math.min(a.y, b.y), 0, bounds.h);
  const x1 = clamp(Math.max(a.x, b.x), 0, bounds.w);
  const y1 = clamp(Math.max(a.y, b.y), 0, bounds.h);
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/** Big enough to be a recording, not a stray click. */
export function isBigEnough(r: RegionRect | null, min = MIN_REGION): r is RegionRect {
  return !!r && r.w >= min && r.h >= min;
}

/** `r` moved by (dx, dy), its size kept, stopped at the screen edges. */
export function moveRect(r: RegionRect, dx: number, dy: number, bounds: Bounds): RegionRect {
  const w = Math.min(r.w, bounds.w);
  const h = Math.min(r.h, bounds.h);
  return { x: clamp(r.x + dx, 0, bounds.w - w), y: clamp(r.y + dy, 0, bounds.h - h), w, h };
}

/**
 * `r` with the edges named by `handle` moved by (dx, dy). An edge stops at the screen and
 * never comes closer than `min` to the opposite one, so the rectangle can't flip or vanish.
 */
export function resizeRect(
  r: RegionRect,
  handle: Handle,
  dx: number,
  dy: number,
  bounds: Bounds,
  min = MIN_REGION,
): RegionRect {
  let x0 = r.x;
  let y0 = r.y;
  let x1 = r.x + r.w;
  let y1 = r.y + r.h;
  if (handle.includes("w")) x0 = clamp(x0 + dx, 0, x1 - min);
  if (handle.includes("e")) x1 = clamp(x1 + dx, x0 + min, bounds.w);
  if (handle.includes("n")) y0 = clamp(y0 + dy, 0, y1 - min);
  if (handle.includes("s")) y1 = clamp(y1 + dy, y0 + min, bounds.h);
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/**
 * Edges on whole points (the backend rounds the same way) and, for a `scale`× display, a
 * size that is a whole even number of pixels — the encoder needs even dimensions, and an
 * area that already has them is recorded exactly as shown.
 */
export function roundRect(r: RegionRect, scale = 1, bounds?: Bounds): RegionRect {
  const s = Number.isFinite(scale) && scale > 0 ? scale : 1;
  const x = Math.round(r.x);
  const y = Math.round(r.y);
  // Even pixels: pixels = w × s; step points down until w × s is an even integer (≤ 2 steps
  // for s ∈ {1, 2}; other scales round to the nearest even pixel count).
  const even = (pt: number, max: number) => {
    let px = Math.floor((Math.min(Math.round(pt), max) * s) / 2) * 2;
    if (px < 2) px = 2;
    return Math.round(px / s);
  };
  const maxW = bounds ? bounds.w - x : Infinity;
  const maxH = bounds ? bounds.h - y : Infinity;
  return { x, y, w: even(r.w, maxW), h: even(r.h, maxH) };
}

/** "1280 × 720". */
export function sizeLabel(r: RegionRect): string {
  return `${Math.round(r.w)} × ${Math.round(r.h)}`;
}

/** CSS cursor for a handle. */
export function handleCursor(h: Handle): string {
  switch (h) {
    case "n":
    case "s":
      return "ns-resize";
    case "e":
    case "w":
      return "ew-resize";
    case "ne":
    case "sw":
      return "nesw-resize";
    default:
      return "nwse-resize";
  }
}

/** Centre of a handle on the rectangle's outline. */
export function handlePoint(r: RegionRect, h: Handle): { x: number; y: number } {
  const x = h.includes("w") ? r.x : h.includes("e") ? r.x + r.w : r.x + r.w / 2;
  const y = h.includes("n") ? r.y : h.includes("s") ? r.y + r.h : r.y + r.h / 2;
  return { x, y };
}

/**
 * Where the button row goes: under the area if there is room, else above it, else inside
 * its bottom edge. `rowH` = the row's height, `gap` = distance from the area.
 */
export function toolbarTop(r: RegionRect, bounds: Bounds, rowH: number, gap = 10): number {
  if (r.y + r.h + gap + rowH <= bounds.h) return r.y + r.h + gap;
  if (r.y - gap - rowH >= 0) return r.y - gap - rowH;
  return r.y + r.h - gap - rowH;
}
