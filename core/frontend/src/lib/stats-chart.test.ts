import { describe, expect, it } from "vitest";
import { areaPath, linePath, seriesExtent, type SeriesPoint } from "./stats-chart";

describe("seriesExtent", () => {
  it("returns [0,1] for an empty series", () => {
    expect(seriesExtent([])).toEqual([0, 1]);
  });

  it("pads a normal range symmetrically", () => {
    const [lo, hi] = seriesExtent([10, 20], 0.1);
    expect(lo).toBeCloseTo(9, 6); // 10 - (10*0.1)
    expect(hi).toBeCloseTo(21, 6); // 20 + (10*0.1)
  });

  it("pads around a flat series so it renders mid-height", () => {
    const [lo, hi] = seriesExtent([50, 50, 50]);
    expect(lo).toBeLessThan(50);
    expect(hi).toBeGreaterThan(50);
    expect(50 - lo).toBeCloseTo(hi - 50, 6); // symmetric
  });

  it("handles an all-zero flat series without collapsing", () => {
    const [lo, hi] = seriesExtent([0, 0]);
    expect(hi).toBeGreaterThan(lo);
  });

  it("ignores order", () => {
    expect(seriesExtent([20, 10, 15, 5], 0)).toEqual([5, 20]);
  });
});

describe("linePath", () => {
  const box = { w: 100, h: 50 };

  it("returns empty string for no points", () => {
    expect(linePath([], 0, 10, box.w, box.h, 0, 100)).toBe("");
  });

  it("starts with M then L commands", () => {
    const pts: SeriesPoint[] = [
      { t: 0, v: 0 },
      { t: 5, v: 50 },
      { t: 10, v: 100 },
    ];
    const d = linePath(pts, 0, 10, box.w, box.h, 0, 100);
    expect(d.startsWith("M")).toBe(true);
    expect(d.split("L").length).toBe(3); // M + 2 L segments
  });

  it("maps time to x across the full width", () => {
    const pts: SeriesPoint[] = [
      { t: 0, v: 50 },
      { t: 10, v: 50 },
    ];
    const d = linePath(pts, 0, 10, 100, 50, 0, 100);
    // First x = 0, last x = 100.
    expect(d).toContain("M0.00");
    expect(d).toContain("L100.00");
  });

  it("inverts y (larger value → smaller y)", () => {
    // Single point at the max value sits at the top (y≈0); at the min, bottom (y≈h).
    const top = linePath([{ t: 0, v: 100 }], 0, 10, 100, 50, 0, 100);
    const bottom = linePath([{ t: 0, v: 0 }], 0, 10, 100, 50, 0, 100);
    expect(top).toBe("M0.00 0.00");
    expect(bottom).toBe("M0.00 50.00");
  });

  it("clamps values outside the extent into the box", () => {
    const over = linePath([{ t: 0, v: 200 }], 0, 10, 100, 50, 0, 100);
    const under = linePath([{ t: 0, v: -50 }], 0, 10, 100, 50, 0, 100);
    expect(over).toBe("M0.00 0.00"); // clamped to top
    expect(under).toBe("M0.00 50.00"); // clamped to bottom
  });

  it("does not divide by zero on degenerate extents", () => {
    const d = linePath([{ t: 5, v: 5 }], 5, 5, 100, 50, 5, 5);
    expect(d).not.toContain("NaN");
    expect(d).not.toContain("Infinity");
  });
});

describe("areaPath", () => {
  it("returns empty string for no points", () => {
    expect(areaPath([], 0, 10, 100, 50, 0, 100)).toBe("");
  });

  it("closes the path down to the baseline", () => {
    const pts: SeriesPoint[] = [
      { t: 0, v: 50 },
      { t: 10, v: 80 },
    ];
    const d = areaPath(pts, 0, 10, 100, 50, 0, 100);
    expect(d.endsWith("Z")).toBe(true);
    expect(d).toContain("L100.00 50.00"); // down to baseline at the right
    expect(d).toContain("L0.00 50.00"); // back to baseline at the left
  });
});

describe("areaPath — degenerate time extent", () => {
  it("does not produce NaN when tMin === tMax", () => {
    const d = areaPath([{ t: 5, v: 1 }], 5, 5, 100, 40, 0, 2);
    expect(d).not.toContain("NaN");
    expect(d.endsWith("Z")).toBe(true);
  });
});
