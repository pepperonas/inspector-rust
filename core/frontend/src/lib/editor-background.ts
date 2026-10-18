//! Pure layout math for the editor's crop + "beautiful background" wrapper.
//!
//! Everything the editor edits lives in IMAGE coordinates. A crop and/or a
//! padded background change where the image sits in the exported "scene":
//! `sceneLayout` returns the scene size + the single (offsetX,offsetY) that
//! maps an image point into scene space. With no crop and no background the
//! offset is (0,0) and the scene is the image — the editor's historic case.
//!
//! This keeps the canvas drawing + mouse mapping funnelled through ONE offset
//! instead of special-casing crop and padding everywhere.

import type { BackgroundSpec, Point, Rect } from "./editor-geometry";

export type SceneLayout = {
  /** Scene (canvas) size. */
  width: number;
  height: number;
  /** scene = image + offset. */
  offsetX: number;
  offsetY: number;
  /** The region of the image that is shown (the crop, or the whole image). */
  content: Rect;
  /** Background padding (0 when no background). */
  pad: number;
  /** Corner radius of the content within the background (0 when none). */
  radius: number;
};

export function sceneLayout(
  imgW: number,
  imgH: number,
  crop: Rect | null,
  bg: BackgroundSpec | null,
): SceneLayout {
  const content: Rect = crop ?? { x: 0, y: 0, w: imgW, h: imgH };
  const pad = bg ? Math.max(0, bg.padding) : 0;
  const radius = bg ? Math.max(0, bg.radius) : 0;
  return {
    width: content.w + 2 * pad,
    height: content.h + 2 * pad,
    offsetX: pad - content.x,
    offsetY: pad - content.y,
    content,
    pad,
    radius,
  };
}

/** Map an image-space point into scene (canvas) space. */
export function toScenePoint(l: SceneLayout, p: Point): Point {
  return { x: p.x + l.offsetX, y: p.y + l.offsetY };
}

/** Map a scene (canvas) point back into image space. */
export function toImagePoint(l: SceneLayout, p: Point): Point {
  return { x: p.x - l.offsetX, y: p.y - l.offsetY };
}
