import { describe, expect, it } from "vitest";
import { snapMove } from "./editor-guides";

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
