//! Arrow geometry for the screenshot editor's four arrow styles (pure, no
//! canvas). The component draws what these functions return.

import type { Point } from "./editor-geometry";

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
