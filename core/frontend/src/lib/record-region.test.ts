import { describe, expect, it } from "vitest";
import {
  fromDrag,
  handleCursor,
  handlePoint,
  HANDLES,
  isBigEnough,
  MIN_REGION,
  moveRect,
  resizeRect,
  roundRect,
  sizeLabel,
  toolbarTop,
} from "./record-region";

const B = { w: 1512, h: 982 };

describe("fromDrag", () => {
  it("normalises a drag in any direction", () => {
    const r = { x: 100, y: 50, w: 200, h: 150 };
    expect(fromDrag({ x: 100, y: 50 }, { x: 300, y: 200 }, B)).toEqual(r);
    expect(fromDrag({ x: 300, y: 200 }, { x: 100, y: 50 }, B)).toEqual(r);
    expect(fromDrag({ x: 300, y: 50 }, { x: 100, y: 200 }, B)).toEqual(r);
  });

  it("stays on the screen when the pointer leaves it", () => {
    expect(fromDrag({ x: 10, y: 10 }, { x: -40, y: 2000 }, B)).toEqual({ x: 0, y: 10, w: 10, h: 972 });
  });
});

describe("isBigEnough", () => {
  it("refuses a click and anything under the minimum on either side", () => {
    expect(isBigEnough(null)).toBe(false);
    expect(isBigEnough({ x: 0, y: 0, w: 0, h: 0 })).toBe(false);
    expect(isBigEnough({ x: 0, y: 0, w: MIN_REGION - 1, h: 500 })).toBe(false);
    expect(isBigEnough({ x: 0, y: 0, w: 500, h: MIN_REGION - 1 })).toBe(false);
    expect(isBigEnough({ x: 0, y: 0, w: MIN_REGION, h: MIN_REGION })).toBe(true);
  });

  it("matches the backend's minimum", () => {
    expect(MIN_REGION).toBe(64);
  });
});

describe("moveRect", () => {
  it("keeps the size and stops at every edge", () => {
    const r = { x: 100, y: 100, w: 300, h: 200 };
    expect(moveRect(r, 20, -30, B)).toEqual({ x: 120, y: 70, w: 300, h: 200 });
    expect(moveRect(r, -500, -500, B)).toEqual({ x: 0, y: 0, w: 300, h: 200 });
    expect(moveRect(r, 5000, 5000, B)).toEqual({ x: 1212, y: 782, w: 300, h: 200 });
  });
});

describe("resizeRect", () => {
  const r = { x: 100, y: 100, w: 300, h: 200 };

  it("moves only the named edges", () => {
    expect(resizeRect(r, "e", 50, 99, B)).toEqual({ x: 100, y: 100, w: 350, h: 200 });
    expect(resizeRect(r, "n", 99, -40, B)).toEqual({ x: 100, y: 60, w: 300, h: 240 });
    expect(resizeRect(r, "sw", -20, 30, B)).toEqual({ x: 80, y: 100, w: 320, h: 230 });
    expect(resizeRect(r, "ne", 10, 10, B)).toEqual({ x: 100, y: 110, w: 310, h: 190 });
  });

  it("never flips or shrinks under the minimum", () => {
    expect(resizeRect(r, "w", 1000, 0, B)).toEqual({ x: 400 - MIN_REGION, y: 100, w: MIN_REGION, h: 200 });
    expect(resizeRect(r, "s", 0, -1000, B)).toEqual({ x: 100, y: 100, w: 300, h: MIN_REGION });
  });

  it("stops at the screen", () => {
    expect(resizeRect(r, "nw", -1000, -1000, B)).toEqual({ x: 0, y: 0, w: 400, h: 300 });
    expect(resizeRect(r, "se", 5000, 5000, B)).toEqual({ x: 100, y: 100, w: 1412, h: 882 });
  });
});

describe("roundRect", () => {
  it("puts edges on whole points and keeps the pixel size even", () => {
    expect(roundRect({ x: 10.4, y: 20.6, w: 301.2, h: 199.6 }, 1)).toEqual({ x: 10, y: 21, w: 300, h: 200 });
    // On a 2× display every whole point is an even pixel count.
    expect(roundRect({ x: 0, y: 0, w: 301, h: 199 }, 2)).toEqual({ x: 0, y: 0, w: 301, h: 199 });
  });

  it("does not grow past the screen", () => {
    expect(roundRect({ x: 1400, y: 0, w: 112.4, h: 100 }, 1, B)).toEqual({ x: 1400, y: 0, w: 112, h: 100 });
  });
});

describe("labels and placement", () => {
  it("prints the size in points", () => {
    expect(sizeLabel({ x: 0, y: 0, w: 1280.4, h: 719.6 })).toBe("1280 × 720");
  });

  it("has eight handles on the outline", () => {
    const r = { x: 10, y: 20, w: 100, h: 50 };
    expect(HANDLES).toHaveLength(8);
    expect(handlePoint(r, "nw")).toEqual({ x: 10, y: 20 });
    expect(handlePoint(r, "s")).toEqual({ x: 60, y: 70 });
    expect(handlePoint(r, "e")).toEqual({ x: 110, y: 45 });
    expect(new Set(HANDLES.map(handleCursor))).toEqual(
      new Set(["ns-resize", "ew-resize", "nesw-resize", "nwse-resize"]),
    );
  });

  it("puts the buttons under, else above, else inside the area", () => {
    expect(toolbarTop({ x: 0, y: 100, w: 100, h: 100 }, B, 40)).toBe(210);
    expect(toolbarTop({ x: 0, y: 900, w: 100, h: 80 }, B, 40)).toBe(850);
    expect(toolbarTop({ x: 0, y: 0, w: 100, h: 982 }, B, 40)).toBe(932);
  });
});
