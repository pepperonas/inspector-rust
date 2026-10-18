import { describe, it, expect } from "vitest";
import { sceneLayout, toImagePoint, toScenePoint } from "./editor-background";
import type { BackgroundSpec } from "./editor-geometry";

const BG: BackgroundSpec = { kind: "solid", color: "#111827", padding: 24, radius: 16, shadow: true };

describe("sceneLayout", () => {
  it("no crop, no background → identity layout matching the image", () => {
    expect(sceneLayout(800, 600, null, null)).toEqual({
      width: 800,
      height: 600,
      offsetX: 0,
      offsetY: 0,
      content: { x: 0, y: 0, w: 800, h: 600 },
      pad: 0,
      radius: 0,
    });
  });

  it("crop only shifts the origin and shrinks the canvas to the crop", () => {
    const l = sceneLayout(800, 600, { x: 100, y: 50, w: 300, h: 200 }, null);
    expect(l).toMatchObject({ width: 300, height: 200, offsetX: -100, offsetY: -50, pad: 0 });
  });

  it("background pads the canvas and offsets the content by the padding", () => {
    const l = sceneLayout(800, 600, null, BG);
    expect(l).toMatchObject({ width: 848, height: 648, offsetX: 24, offsetY: 24, pad: 24, radius: 16 });
  });

  it("crop + background compose: padding wraps the cropped region", () => {
    const l = sceneLayout(800, 600, { x: 100, y: 50, w: 300, h: 200 }, BG);
    // scene = crop + 2·pad; content maps to (pad,pad): offset = pad − crop.origin
    expect(l).toMatchObject({ width: 348, height: 248, offsetX: -76, offsetY: -26 });
  });

  it("clamps negative padding/radius to zero", () => {
    const l = sceneLayout(100, 100, null, { kind: "solid", color: "#000", padding: -5, radius: -3, shadow: false });
    expect(l).toMatchObject({ pad: 0, radius: 0, width: 100 });
  });
});

describe("point mapping", () => {
  it("image↔scene round-trips through the offset", () => {
    const l = sceneLayout(800, 600, { x: 100, y: 50, w: 300, h: 200 }, BG);
    const img = { x: 150, y: 90 };
    const scene = toScenePoint(l, img);
    expect(toImagePoint(l, scene)).toEqual(img);
    // a scene click at the content's top-left maps to the crop origin
    expect(toImagePoint(l, { x: l.pad, y: l.pad })).toEqual({ x: 100, y: 50 });
  });
});
