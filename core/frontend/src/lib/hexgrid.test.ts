import { describe, expect, it } from "vitest";
import {
  boundingFraction,
  cellInRange,
  hexCenters,
  hexPolygon,
  nearestCell,
} from "./hexgrid";

describe("hexCenters", () => {
  it("produces cols×rows cells", () => {
    expect(hexCenters(8, 6, 800, 600)).toHaveLength(48);
  });

  it("returns nothing for a degenerate grid", () => {
    expect(hexCenters(0, 6, 800, 600)).toHaveLength(0);
    expect(hexCenters(8, 0, 800, 600)).toHaveLength(0);
  });

  it("offsets odd rows to the right (pointy-top honeycomb)", () => {
    const cells = hexCenters(4, 2, 400, 200);
    const even = cells.find((c) => c.row === 0 && c.col === 0)!;
    const odd = cells.find((c) => c.row === 1 && c.col === 0)!;
    expect(odd.cx).toBeGreaterThan(even.cx);
  });

  it("rows interlock vertically (pitch < hex height)", () => {
    // Row pitch is 0.75·hexHeight, so row 1's center is less than a full hex
    // height below row 0 — that's what makes the cells interlock.
    const cells = hexCenters(3, 3, 300, 300);
    const r0 = cells.find((c) => c.row === 0 && c.col === 0)!;
    const r1 = cells.find((c) => c.row === 1 && c.col === 0)!;
    const hHex = 300 / (0.75 * 3 + 0.25);
    expect(r1.cy - r0.cy).toBeCloseTo(0.75 * hHex, 4);
  });

  it("keeps all centers inside the box", () => {
    for (const c of hexCenters(8, 6, 800, 600)) {
      expect(c.cx).toBeGreaterThanOrEqual(0);
      expect(c.cx).toBeLessThanOrEqual(800);
      expect(c.cy).toBeGreaterThanOrEqual(0);
      expect(c.cy).toBeLessThanOrEqual(600);
    }
  });
});

describe("nearestCell", () => {
  const cells = hexCenters(4, 4, 400, 400);

  it("returns null for an empty grid", () => {
    expect(nearestCell([], 10, 10)).toBeNull();
  });

  it("picks the top-left cell near the origin", () => {
    const c = nearestCell(cells, 5, 5)!;
    expect(c.col).toBe(0);
    expect(c.row).toBe(0);
  });

  it("picks the bottom-right cell near the far corner", () => {
    const c = nearestCell(cells, 399, 399)!;
    expect(c.row).toBe(3);
    expect(c.col).toBeGreaterThanOrEqual(2);
  });

  it("returns one of the grid's actual cells", () => {
    const c = nearestCell(cells, 137, 201)!;
    expect(cells).toContainEqual(c);
  });
});

describe("boundingFraction", () => {
  it("maps a single cell to a 1/cols × 1/rows tile", () => {
    const f = boundingFraction({ col: 0, row: 0 }, { col: 0, row: 0 }, 8, 6);
    expect(f).toEqual({ x: 0, y: 0, w: 1 / 8, h: 1 / 6 });
  });

  it("is order-independent (drag direction doesn't matter)", () => {
    const a = boundingFraction({ col: 1, row: 1 }, { col: 3, row: 4 }, 8, 6);
    const b = boundingFraction({ col: 3, row: 4 }, { col: 1, row: 1 }, 8, 6);
    expect(a).toEqual(b);
  });

  it("maps the whole grid to the full screen", () => {
    const f = boundingFraction({ col: 0, row: 0 }, { col: 7, row: 5 }, 8, 6);
    expect(f).toEqual({ x: 0, y: 0, w: 1, h: 1 });
  });

  it("maps the right half", () => {
    const f = boundingFraction({ col: 4, row: 0 }, { col: 7, row: 5 }, 8, 6);
    expect(f.x).toBeCloseTo(0.5, 6);
    expect(f.w).toBeCloseTo(0.5, 6);
    expect(f.y).toBe(0);
    expect(f.h).toBe(1);
  });
});

describe("cellInRange", () => {
  it("includes the endpoints and interior, excludes outside", () => {
    const a = { col: 1, row: 1 };
    const b = { col: 3, row: 3 };
    expect(cellInRange(1, 1, a, b)).toBe(true);
    expect(cellInRange(2, 2, a, b)).toBe(true);
    expect(cellInRange(3, 3, a, b)).toBe(true);
    expect(cellInRange(0, 2, a, b)).toBe(false);
    expect(cellInRange(2, 4, a, b)).toBe(false);
  });

  it("works regardless of corner order", () => {
    expect(cellInRange(2, 2, { col: 3, row: 3 }, { col: 1, row: 1 })).toBe(true);
  });
});

describe("hexPolygon", () => {
  it("returns six comma/space-separated vertices", () => {
    const p = hexPolygon(50, 50, 20, 20);
    const verts = p.split(" ");
    expect(verts).toHaveLength(6);
    for (const v of verts) expect(v).toMatch(/^-?\d+\.\d{2},-?\d+\.\d{2}$/);
  });

  it("is centered (mean of vertices ≈ center)", () => {
    const p = hexPolygon(100, 80, 30, 24);
    const xs = p.split(" ").map((v) => Number(v.split(",")[0]));
    const ys = p.split(" ").map((v) => Number(v.split(",")[1]));
    const mx = xs.reduce((s, x) => s + x, 0) / xs.length;
    const my = ys.reduce((s, y) => s + y, 0) / ys.length;
    expect(mx).toBeCloseTo(100, 1);
    expect(my).toBeCloseTo(80, 1);
  });
});
