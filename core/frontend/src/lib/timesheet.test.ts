import { describe, it, expect } from "vitest";
import {
  formatDuration,
  shiftDay,
  donutSegments,
  donutSegmentPath,
  timelineBand,
  colorMap,
  paletteColor,
  msToTimeInput,
  timeInputToMs,
  weekBounds,
  shiftWeek,
} from "./timesheet";

describe("timesheet helpers", () => {
  it("formatDuration is compact and unit-stepped", () => {
    expect(formatDuration(0)).toBe("0s");
    expect(formatDuration(45)).toBe("45s");
    expect(formatDuration(60)).toBe("1m");
    expect(formatDuration(125)).toBe("2m");
    expect(formatDuration(3600)).toBe("1h");
    expect(formatDuration(3600 + 14 * 60)).toBe("1h 14m");
    expect(formatDuration(-5)).toBe("0s");
  });

  it("shiftDay moves by whole days across month boundaries", () => {
    expect(shiftDay("2026-06-20", -1)).toBe("2026-06-19");
    expect(shiftDay("2026-06-20", 1)).toBe("2026-06-21");
    expect(shiftDay("2026-06-30", 1)).toBe("2026-07-01");
    expect(shiftDay("2026-01-01", -1)).toBe("2025-12-31");
  });

  it("donutSegments are cumulative and span 360°", () => {
    const segs = donutSegments([1, 1, 2]);
    expect(segs.length).toBe(3);
    expect(segs[0].start).toBeCloseTo(0, 5);
    expect(segs[2].end).toBeCloseTo(360, 5);
    // middle segment is a quarter (90°..180°).
    expect(segs[1].start).toBeCloseTo(90, 5);
    expect(segs[1].end).toBeCloseTo(180, 5);
    expect(donutSegments([])).toEqual([]);
    expect(donutSegments([0, 0])).toEqual([]);
  });

  it("donutSegmentPath produces a closed ring-sector path", () => {
    const p = donutSegmentPath(50, 50, 40, 24, 0, 90);
    expect(p.startsWith("M ")).toBe(true);
    expect(p.endsWith("Z")).toBe(true);
    expect(p).toContain("A 40 40"); // outer arc
    expect(p).toContain("A 24 24"); // inner arc
  });

  it("timelineBand clips to the day and handles open events", () => {
    const day = new Date(2026, 5, 20, 0, 0, 0, 0).getTime();
    const hour = 3_600_000;
    // 06:00–08:00 → left 25%, width ~8.33%.
    const b = timelineBand(day + 6 * hour, day + 8 * hour, day, day + 12 * hour)!;
    expect(b.leftPct).toBeCloseTo(25, 3);
    expect(b.widthPct).toBeCloseTo((2 / 24) * 100, 3);
    // open event → extends to `now`.
    const o = timelineBand(day + 23 * hour, null, day, day + 23.5 * hour)!;
    expect(o.widthPct).toBeCloseTo((0.5 / 24) * 100, 3);
    // zero-width → null.
    expect(timelineBand(day + hour, day + hour, day, day + 2 * hour)).toBeNull();
  });

  it("msToTimeInput / timeInputToMs round-trip within a day", () => {
    const day = new Date(2026, 5, 20, 0, 0, 0, 0).getTime();
    const t = day + 9 * 3_600_000 + 35 * 60_000 + 12_000; // 09:35:12
    expect(msToTimeInput(t)).toBe("09:35");
    // Re-parse keeping the original seconds.
    expect(timeInputToMs(day, "09:35", t)).toBe(t);
    // Plain parse → :00.
    expect(timeInputToMs(day, "09:35")).toBe(day + 9 * 3_600_000 + 35 * 60_000);
    expect(timeInputToMs(day, "bad")).toBeNull();
    expect(timeInputToMs(day, "99:99")).toBeNull();
  });

  it("colorMap assigns stable palette colours by order", () => {
    const m = colorMap(["Code", "Safari", "Slack"]);
    expect(m.Code).toBe(paletteColor(0));
    expect(m.Safari).toBe(paletteColor(1));
    expect(m.Slack).toBe(paletteColor(2));
  });

  it("weekBounds returns Mon–Sun containing the date", () => {
    // 2026-06-17 is a Wednesday → week Mon 2026-06-15 .. Sun 2026-06-21.
    expect(weekBounds("2026-06-17")).toEqual({ from: "2026-06-15", to: "2026-06-21" });
    expect(weekBounds("2026-06-15").from).toBe("2026-06-15"); // Monday → itself
    expect(weekBounds("2026-06-21").from).toBe("2026-06-15"); // Sunday → preceding Monday
    expect(shiftWeek("2026-06-17", -1)).toBe("2026-06-10");
    expect(shiftWeek("2026-06-17", 1)).toBe("2026-06-24");
  });
});
