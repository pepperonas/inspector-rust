import { describe, expect, it } from "vitest";
import { resizeAnchors, snapMove, snapResize } from "./editor-guides";
import type { Annotation } from "./editor-geometry";

const box = { x: 100, y: 100, w: 50, h: 20 };
const other = { x: 300, y: 40, w: 80, h: 80 };

describe("snapMove", () => {
  it("pulls a near edge onto another element's edge and reports the line", () => {
    // moving left edge 100+197=297 → snaps to the other's left edge 300
    const r = snapMove(box, [other], 197, 0, 6);
    expect(r.dx).toBe(200);
    expect(r.guides).toContainEqual({ axis: "x", at: 300 });
  });
  it("snaps centres too", () => {
    // moving centre y = 110+dy, other centre y = 80 → dy -30 wanted, -28 given
    const r = snapMove(box, [other], 0, -28, 6);
    expect(r.dy).toBe(-30);
    expect(r.guides).toContainEqual({ axis: "y", at: 80 });
  });
  it("leaves the move alone when nothing is close", () => {
    expect(snapMove(box, [other], 20, 40, 6)).toEqual({ dx: 20, dy: 40, guides: [] });
  });
  it("takes the closest line on each axis independently", () => {
    // right edge 150+dx; other left 300 and right 380. dx=148 → right edge 298 (2 away from 300)
    const r = snapMove(box, [other, { x: 0, y: 0, w: 1000, h: 600 }], 148, 0, 6);
    expect(r.dx).toBe(150);
  });
  it("snaps to the frame's centre", () => {
    const frame = { x: 0, y: 0, w: 1000, h: 600 };
    // box centre x = 125+dx → 500 at dx=375
    const r = snapMove(box, [frame], 373, 0, 6);
    expect(r.dx).toBe(375);
    expect(r.guides).toContainEqual({ axis: "x", at: 500 });
  });
  it("a locked axis is never moved by snapping", () => {
    const r = snapMove(box, [other], 0, -28, 6, { x: true, y: false });
    expect(r.dy).toBe(-28);
    expect(r.guides).toEqual([]);
  });
  it("lists each guide line once", () => {
    const r = snapMove(box, [other, { ...other, y: 300 }], 197, 0, 6);
    expect(r.guides.filter((g) => g.axis === "x" && g.at === 300)).toHaveLength(1);
  });
});

describe("resizeAnchors", () => {
  const rect = { id: "r", type: "rect", x: 100, y: 100, w: 50, h: 20, color: "#f00", width: 2 } as Annotation;
  it("names the edges a box handle moves", () => {
    expect(resizeAnchors(rect, "e")).toEqual({ xs: [150], ys: [] });
    expect(resizeAnchors(rect, "nw")).toEqual({ xs: [100], ys: [100] });
    expect(resizeAnchors(rect, "s")).toEqual({ xs: [], ys: [120] });
  });
  it("names the dragged end of a line", () => {
    const line = { id: "l", type: "line", x1: 10, y1: 20, x2: 90, y2: 70, color: "#f00", width: 2 } as Annotation;
    expect(resizeAnchors(line, "end")).toEqual({ xs: [90], ys: [70] });
    expect(resizeAnchors(line, "start")).toEqual({ xs: [10], ys: [20] });
  });
  it("has nothing to snap for size handles, bends and aspect-locked picture corners", () => {
    const text = { id: "t", type: "text", x: 0, y: 0, text: "a", size: 20, color: "#000" } as Annotation;
    const pic = { id: "i", type: "image", x: 0, y: 0, w: 40, h: 20, src: "data:image/png;base64,AA" } as Annotation;
    const arrow = { id: "a", type: "arrow", x1: 0, y1: 0, x2: 50, y2: 0, color: "#000", width: 3 } as Annotation;
    expect(resizeAnchors(text, "se")).toBeNull();
    expect(resizeAnchors(pic, "se")).toBeNull();
    expect(resizeAnchors(arrow, "mid")).toBeNull();
    expect(resizeAnchors(pic, "e")).toEqual({ xs: [40], ys: [] });
  });
});

describe("snapResize", () => {
  it("pulls the dragged edge onto a line and reports it", () => {
    // right edge 150 + 147 = 297 → other's left edge 300
    const r = snapResize({ xs: [150], ys: [] }, [other], 147, 5, 6);
    expect(r.dx).toBe(150);
    expect(r.dy).toBe(5);
    expect(r.guides).toEqual([{ axis: "x", at: 300 }]);
  });
  it("leaves an axis the handle doesn't move alone", () => {
    const r = snapResize({ xs: [], ys: [120] }, [other], 0, -1, 6);
    expect(r.dy).toBe(0); // 119 snaps back to the other element's bottom edge 120
    expect(r.dx).toBe(0);
    expect(r.guides).toEqual([{ axis: "y", at: 120 }]);
  });
  it("does nothing when no line is close", () => {
    const r = snapResize({ xs: [150], ys: [120] }, [other], 20, 30, 6);
    expect(r).toEqual({ dx: 20, dy: 30, guides: [] });
  });
});
