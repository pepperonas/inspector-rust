import { describe, expect, it } from "vitest";
import LAYOUT_RS from "../../../rust-lib/src/record/layout.rs?raw";
import { CAMERA_SIZE_RANGE } from "./editor";
import { PIP_MARGIN_FRACTION, moveCamera, placeCamera, resizeCamera, type CamRect } from "./camera-drag";

const OUT: [number, number] = [1920, 1080];
const M = Math.round(1080 * PIP_MARGIN_FRACTION) / 1920; // margin as x fraction
const MY = Math.round(1080 * PIP_MARGIN_FRACTION) / 1080;
// A 432 × 243 box (16:9 camera, 40 % rectangle) in the middle.
const R: CamRect = [(960 - 216) / 1920, (540 - 121.5) / 1080, 432 / 1920, 243 / 1080];

describe("camera drag", () => {
  it("mirrors the Rust margin", () => {
    const m = /pub const PIP_MARGIN_FRACTION: f64 = ([\d.]+);/.exec(LAYOUT_RS);
    expect(m).not.toBeNull();
    expect(Number(m![1])).toBe(PIP_MARGIN_FRACTION);
  });

  it("moves the centre by the drag", () => {
    const p = moveCamera(R, 0.1, -0.05, OUT);
    expect(p.x).toBeCloseTo(0.6, 9);
    expect(p.y).toBeCloseTo(0.45, 9);
    expect(p.rect[2]).toBe(R[2]);
    expect(p.rect[3]).toBe(R[3]);
  });

  it("keeps the box inside the margin at every edge", () => {
    for (const [dx, dy] of [
      [-5, 0],
      [5, 0],
      [0, -5],
      [0, 5],
    ]) {
      const { rect } = moveCamera(R, dx, dy, OUT);
      expect(rect[0]).toBeGreaterThanOrEqual(M - 1e-9);
      expect(rect[0] + rect[2]).toBeLessThanOrEqual(1 - M + 1e-9);
      expect(rect[1]).toBeGreaterThanOrEqual(MY - 1e-9);
      expect(rect[1] + rect[3]).toBeLessThanOrEqual(1 - MY + 1e-9);
    }
    expect(moveCamera(R, -5, 0, OUT).rect[0]).toBeCloseTo(M, 9);
    expect(moveCamera(R, 5, 0, OUT).rect[0] + R[2]).toBeCloseTo(1 - M, 9);
  });

  it("a box larger than the room sits at the margin", () => {
    const p = placeCamera(0.5, 0.5, 1.2, 0.5, OUT);
    expect(p.rect[0]).toBeCloseTo(M, 9);
  });

  it("resizes from a corner with the opposite corner fixed and the aspect kept", () => {
    // Drag the bottom-right corner 216 px further right: width 432 → 648 → 60 % (from 40 %).
    const se = resizeCamera(R, "se", [(960 + 216 + 216) / 1920, 0.9], 40, OUT);
    expect(se.sizePct).toBe(60);
    expect(se.rect[0]).toBeCloseTo(R[0], 9); // top-left stays
    expect(se.rect[1]).toBeCloseTo(R[1], 9);
    expect(se.rect[2] * 1920).toBeCloseTo(648, 6);
    expect((se.rect[2] * 1920) / (se.rect[3] * 1080)).toBeCloseTo(432 / 243, 9);
    expect(se.x).toBeCloseTo(se.rect[0] + se.rect[2] / 2, 9);
    // The top-left corner pulled inwards: the bottom-right corner stays.
    const nw = resizeCamera(R, "nw", [(960 - 216 + 108) / 1920, (540 - 121.5 + 60.75) / 1080], 40, OUT);
    expect(nw.sizePct).toBe(30);
    expect(nw.rect[0] + nw.rect[2]).toBeCloseTo(R[0] + R[2], 9);
    expect(nw.rect[1] + nw.rect[3]).toBeCloseTo(R[1] + R[3], 9);
  });

  it("follows the axis that grew more", () => {
    // Pointer only lower (y): the width follows the height through the aspect.
    const p = resizeCamera(R, "se", [R[0] + R[2], R[1] + (243 * 1.25) / 1080], 40, OUT);
    expect(p.sizePct).toBe(50);
    const ne = resizeCamera(R, "ne", [R[0] + R[2], R[1] - (243 * 0.25) / 1080], 40, OUT);
    expect(ne.sizePct).toBe(50);
    expect(ne.rect[1] + ne.rect[3]).toBeCloseTo(R[1] + R[3], 9);
  });

  it("clamps the size to the range", () => {
    expect(resizeCamera(R, "se", [0.99, 0.99], 40, OUT).sizePct).toBe(CAMERA_SIZE_RANGE[1]);
    expect(resizeCamera(R, "se", [0, 0], 40, OUT).sizePct).toBe(CAMERA_SIZE_RANGE[0]);
    const sw = resizeCamera(R, "sw", [0.99, 0.01], 40, OUT);
    expect(sw.sizePct).toBe(CAMERA_SIZE_RANGE[0]);
    // Clamped: the box is the clamped size, the fixed corner (top-right) stays.
    expect(sw.rect[2] * 1920).toBeCloseTo((432 * 15) / 40, 6);
    expect(sw.rect[0] + sw.rect[2]).toBeCloseTo(R[0] + R[2], 9);
  });

  it("leaves a degenerate box alone", () => {
    const p = resizeCamera([0.1, 0.1, 0, 0], "se", [0.5, 0.5], 40, OUT);
    expect(p.sizePct).toBe(40);
    expect(resizeCamera(R, "se", [0.5, 0.5], 40, [0, 0]).sizePct).toBe(40);
  });
});
