/**
 * Pure geometry of dragging the picture-in-picture camera in the editor preview.
 *
 * The box itself comes from Rust (`view.cameraRects`, R12); these helpers only turn a drag on
 * that box into the document's `camera.position` (the box centre, fractions of the output) and
 * `camera.sizePct`, plus the box to show while the drag runs (Rust answers a moment later).
 */
import { CAMERA_SIZE_RANGE } from "./editor";

/** `[x, y, w, h]` in fractions of the output. */
export type CamRect = readonly [number, number, number, number];
export type CamCorner = "nw" | "ne" | "sw" | "se";
export const CAM_CORNERS: readonly CamCorner[] = ["nw", "ne", "sw", "se"];

/** Mirror of `layout::PIP_MARGIN_FRACTION`: the camera keeps this share of the short side from the border. */
export const PIP_MARGIN_FRACTION = 0.02;

export interface CameraPlacement {
  /** Box centre, fractions of the output. */
  x: number;
  y: number;
  /** The box as it will sit (for the drag preview). */
  rect: [number, number, number, number];
}

function marginPx(out: readonly [number, number]): number {
  return Math.max(0, Math.round(Math.min(out[0], out[1]) * PIP_MARGIN_FRACTION));
}

/** One axis: the box start for centre `c`, kept `margin` from both borders (Rust's `free_pos`). */
function axisStart(c: number, extent: number, size: number, margin: number): number {
  const hi = Math.max(margin, extent - margin - size);
  return Math.max(0, Math.min(hi, Math.max(margin, c * extent - size / 2)));
}

/** A box of `w × h` (fractions) around centre `(cx, cy)`, kept inside the margin. */
export function placeCamera(cx: number, cy: number, w: number, h: number, out: readonly [number, number]): CameraPlacement {
  const [W, H] = out;
  if (!(W > 0 && H > 0)) return { x: cx, y: cy, rect: [cx - w / 2, cy - h / 2, w, h] };
  const m = marginPx(out);
  const x0 = axisStart(cx, W, w * W, m);
  const y0 = axisStart(cy, H, h * H, m);
  const rect: [number, number, number, number] = [x0 / W, y0 / H, w, h];
  return { x: rect[0] + w / 2, y: rect[1] + h / 2, rect };
}

/** The box moved by `(dx, dy)` (fractions of the output). */
export function moveCamera(rect: CamRect, dx: number, dy: number, out: readonly [number, number]): CameraPlacement {
  const [x, y, w, h] = rect;
  return placeCamera(x + w / 2 + dx, y + h / 2 + dy, w, h, out);
}

/**
 * The box resized from `corner` towards the pointer (`pointer`, fractions of the output), the
 * opposite corner fixed, the box's aspect kept. The camera is laid out linearly in `sizePct`
 * (both masks), so the new size is the old one scaled by the width ratio — clamped to the size
 * range. The new centre is the resized box's midpoint (then kept inside the margin).
 */
export function resizeCamera(
  rect: CamRect,
  corner: CamCorner,
  pointer: readonly [number, number],
  sizePct: number,
  out: readonly [number, number],
): CameraPlacement & { sizePct: number } {
  const [W, H] = out;
  const [rx, ry, rw, rh] = rect;
  const w = rw * W;
  const h = rh * H;
  if (!(W > 0 && H > 0 && w > 0 && h > 0 && sizePct > 0)) {
    return { ...placeCamera(rx + rw / 2, ry + rh / 2, rw, rh, out), sizePct };
  }
  const aspect = w / h;
  const sx = corner.endsWith("e") ? 1 : -1;
  const sy = corner.startsWith("s") ? 1 : -1;
  const fixedX = sx > 0 ? rx * W : rx * W + w;
  const fixedY = sy > 0 ? ry * H : ry * H + h;
  const dx = (pointer[0] * W - fixedX) * sx;
  const dy = (pointer[1] * H - fixedY) * sy;
  const wanted = Math.max(1, dx, dy * aspect);
  const raw = (sizePct * wanted) / w;
  const size = Math.round(Math.min(CAMERA_SIZE_RANGE[1], Math.max(CAMERA_SIZE_RANGE[0], raw)) * 10) / 10;
  const nw = (w * size) / sizePct;
  const nh = nw / aspect;
  const x0 = sx > 0 ? fixedX : fixedX - nw;
  const y0 = sy > 0 ? fixedY : fixedY - nh;
  return { ...placeCamera((x0 + nw / 2) / W, (y0 + nh / 2) / H, nw / W, nh / H, out), sizePct: size };
}
