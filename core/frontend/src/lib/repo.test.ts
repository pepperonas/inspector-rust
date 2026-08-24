import { describe, it, expect } from "vitest";
import {
  WEEKDAY_LABELS,
  categoryColor,
  formatNum,
  shortDate,
  barPct,
  peakLabel,
  sparkPoints,
  totalChurn,
} from "./repo";

describe("repo display helpers", () => {
  it("category colours cover the known set and fall back", () => {
    expect(categoryColor("feat")).toBe("#81c995");
    expect(categoryColor("fix")).toBe("#f28b82");
    expect(categoryColor("nonsense")).toBe("#5f6368");
  });

  it("formatNum / shortDate", () => {
    expect(formatNum(1234567)).toBe("1.234.567");
    expect(shortDate("2026-08-24T09:15:00+02:00")).toBe("24.08.2026");
    expect(shortDate("")).toBe("—");
  });

  it("barPct clamps to 0..100", () => {
    expect(barPct(5, 10)).toBe(50);
    expect(barPct(0, 0)).toBe(0);
    expect(barPct(20, 10)).toBe(100);
    expect(barPct(-3, 10)).toBe(0);
  });

  it("peakLabel finds the busiest bucket", () => {
    expect(peakLabel([1, 5, 2], WEEKDAY_LABELS)).toBe("Di");
    expect(peakLabel([], WEEKDAY_LABELS)).toBe("—");
    // First max wins on ties.
    expect(peakLabel([3, 3, 1], ["a", "b", "c"])).toBe("a");
  });

  it("sparkPoints maps a series into the box, handles 0/1 points", () => {
    expect(sparkPoints([], 100, 20)).toBe("");
    // Single point → flat mid-line spanning the width.
    expect(sparkPoints([5], 100, 20, 2)).toBe("2,10.0 98,10.0");
    const pts = sparkPoints([0, 10], 100, 20, 2).split(" ");
    expect(pts).toHaveLength(2);
    // First point (value 0) sits at the bottom, last (max) at the top.
    expect(Number(pts[0].split(",")[1])).toBeGreaterThan(Number(pts[1].split(",")[1]));
  });

  it("totalChurn sums", () => {
    expect(totalChurn(100, 40)).toBe(140);
  });
});
