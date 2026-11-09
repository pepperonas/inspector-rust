//! A second picture placed on the screenshot (pasted, dropped or chosen from
//! a file). Pure: where it lands, how it scales, which sources are allowed.

import type { Annotation, ImageAnnotation, Rect } from "./editor-geometry";

/** Largest share of the screenshot a newly placed picture may cover, per side. */
export const PLACE_MAX_SHARE = 0.5;

/** Where a picture of `natW`×`natH` px lands on a `sceneW`×`sceneH`
 *  screenshot: centred, at most half its width and height, never upscaled. */
export function placeImage(sceneW: number, sceneH: number, natW: number, natH: number): Rect {
  const w0 = Math.max(1, natW);
  const h0 = Math.max(1, natH);
  const k = Math.min(1, (sceneW * PLACE_MAX_SHARE) / w0, (sceneH * PLACE_MAX_SHARE) / h0);
  const w = w0 * k;
  const h = h0 * k;
  return { x: (sceneW - w) / 2, y: (sceneH - h) / 2, w, h };
}

/** Only inline raster pictures are drawn: no network, no SVG (scripts). */
export function isSafeImageSrc(src: unknown): src is string {
  return typeof src === "string" && /^data:image\/(png|jpeg|gif|webp|bmp);base64,/i.test(src);
}

/** Scale around the centre. */
export function scaleImage(a: ImageAnnotation, k: number): ImageAnnotation {
  const w = Math.max(4, a.w * k);
  const h = Math.max(4, a.h * k);
  return { ...a, x: a.x + (a.w - w) / 2, y: a.y + (a.h - h) / 2, w, h };
}

/** Does some blur have an element drawn before it? Then the blur must read
 *  the composed picture (screenshot + those elements), not the bare
 *  screenshot — otherwise a blur over a placed picture or an arrow shows the
 *  screenshot underneath instead of hiding what is on top. Spotlights are
 *  drawn last, so they never count. */
export function blurNeedsLayer(list: readonly Annotation[]): boolean {
  let seen = false;
  for (const a of list) {
    if (a.type === "spotlight") continue;
    if (a.type === "blur" && seen) return true;
    seen = true;
  }
  return false;
}
