import { describe, it, expect } from "vitest";
import {
  formatDuration,
  formatClock,
  localDateStr,
  shortDayLabel,
  shiftDay,
  dayStartMs,
  dayEndMs,
  donutSegments,
  donutSegmentPath,
  timelineBand,
  colorMap,
  paletteColor,
  hashString,
  categoryColorMap,
  msToTimeInput,
  timeInputToMs,
  weekBounds,
  shiftWeek,
  categoryColor,
  projectColor,
  NO_PROJECT,
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

  it("categoryColor is stable per name and mutes Uncategorized", () => {
    expect(categoryColor("Work")).toBe(categoryColor("Work")); // stable
    expect(categoryColor("Uncategorized")).toBe("var(--color-muted)");
    expect(categoryColor("")).toBe("var(--color-muted)");
    // A real category gets a concrete palette colour (hex).
    expect(categoryColor("Work")).toMatch(/^#/);
  });

  it("projectColor is stable per name and mutes the no-project sentinel", () => {
    expect(projectColor("Acme")).toBe(projectColor("Acme"));
    expect(projectColor(NO_PROJECT)).toBe("var(--color-muted)");
    expect(projectColor("")).toBe("var(--color-muted)");
    expect(projectColor("Acme")).toMatch(/^#/);
  });

  it("weekBounds returns Mon–Sun containing the date", () => {
    // 2026-06-17 is a Wednesday → week Mon 2026-06-15 .. Sun 2026-06-21.
    expect(weekBounds("2026-06-17")).toEqual({ from: "2026-06-15", to: "2026-06-21" });
    expect(weekBounds("2026-06-15").from).toBe("2026-06-15"); // Monday → itself
    expect(weekBounds("2026-06-21").from).toBe("2026-06-15"); // Sunday → preceding Monday
    expect(shiftWeek("2026-06-17", -1)).toBe("2026-06-10");
    expect(shiftWeek("2026-06-17", 1)).toBe("2026-06-24");
  });

  it("formatDuration handles boundaries + fractional seconds", () => {
    expect(formatDuration(3661)).toBe("1h 1m");
    expect(formatDuration(7200)).toBe("2h");
    expect(formatDuration(59)).toBe("59s");
    expect(formatDuration(61.9)).toBe("1m"); // floors, never rounds up
    expect(formatDuration(3600.9)).toBe("1h");
  });

  it("formatClock zero-pads local hours + minutes", () => {
    expect(formatClock(new Date(2026, 5, 20, 9, 5).getTime())).toBe("09:05");
    expect(formatClock(new Date(2026, 5, 20, 0, 0).getTime())).toBe("00:00");
    expect(formatClock(new Date(2026, 5, 20, 23, 59).getTime())).toBe("23:59");
  });

  it("localDateStr zero-pads month and day", () => {
    expect(localDateStr(new Date(2026, 0, 5))).toBe("2026-01-05");
    expect(localDateStr(new Date(2026, 11, 31))).toBe("2026-12-31");
  });

  it("shiftDay handles leap years and zero delta", () => {
    expect(shiftDay("2028-02-28", 1)).toBe("2028-02-29"); // 2028 is a leap year
    expect(shiftDay("2026-02-28", 1)).toBe("2026-03-01"); // 2026 is not
    expect(shiftDay("2026-06-20", 0)).toBe("2026-06-20");
    expect(shiftDay("2026-06-20", 365)).toBe("2027-06-20");
  });

  it("dayEndMs(d) === dayStartMs(next day) — the Rust day_bounds invariant", () => {
    for (const d of ["2026-06-20", "2026-12-31", "2028-02-28"]) {
      expect(dayEndMs(d)).toBe(dayStartMs(shiftDay(d, 1)));
    }
    // A DST-free day spans exactly 24 h.
    expect(dayEndMs("2026-06-20") - dayStartMs("2026-06-20")).toBe(86_400_000);
  });

  it("every day of a week maps to the same bounds; weeks can cross months", () => {
    const days = ["2026-06-15", "2026-06-16", "2026-06-17", "2026-06-18", "2026-06-19", "2026-06-20", "2026-06-21"];
    for (const d of days) {
      expect(weekBounds(d)).toEqual({ from: "2026-06-15", to: "2026-06-21" });
    }
    // 2026-07-01 is a Wednesday → its week starts back in June.
    expect(weekBounds("2026-07-01")).toEqual({ from: "2026-06-29", to: "2026-07-05" });
  });

  it("timeInputToMs accepts single-digit hours, rejects malformed shapes", () => {
    const day = new Date(2026, 5, 20).getTime();
    expect(timeInputToMs(day, "9:05")).toBe(day + 9 * 3_600_000 + 5 * 60_000);
    expect(timeInputToMs(day, "23:59")).toBe(day + 23 * 3_600_000 + 59 * 60_000);
    expect(timeInputToMs(day, "24:00")).toBeNull();
    expect(timeInputToMs(day, "12:60")).toBeNull();
    expect(timeInputToMs(day, "7:5")).toBeNull(); // minutes must be two digits
    expect(timeInputToMs(day, "")).toBeNull();
  });

  it("timeInputToMs preserves sub-minute milliseconds from the original event", () => {
    const day = new Date(2026, 5, 20).getTime();
    const orig = day + 10 * 3_600_000 + 20 * 60_000 + 12_345; // 10:20:12.345
    expect(timeInputToMs(day, "11:00", orig)).toBe(day + 11 * 3_600_000 + 12_345);
  });

  it("paletteColor wraps in both directions", () => {
    expect(paletteColor(10)).toBe(paletteColor(0));
    expect(paletteColor(-1)).toBe(paletteColor(9));
    expect(paletteColor(23)).toBe(paletteColor(3));
  });

  it("hashString is deterministic, spread, and Umlaut-sensitive", () => {
    expect(hashString("Work")).toBe(hashString("Work"));
    expect(hashString("")).toBe(2166136261 >>> 0); // FNV-1a offset basis
    expect(hashString("a")).not.toBe(hashString("b"));
    expect(hashString("ä")).not.toBe(hashString("a"));
    // Always an unsigned 32-bit int.
    for (const s of ["", "x", "Grüße", "a-long-category-name"]) {
      const h = hashString(s);
      expect(Number.isInteger(h)).toBe(true);
      expect(h).toBeGreaterThanOrEqual(0);
      expect(h).toBeLessThanOrEqual(0xffffffff);
    }
  });

  it("categoryColorMap mirrors categoryColor per key", () => {
    const m = categoryColorMap(["Work", "Uncategorized", "Fun"]);
    expect(m.Work).toBe(categoryColor("Work"));
    expect(m.Uncategorized).toBe("var(--color-muted)");
    expect(m.Fun).toBe(categoryColor("Fun"));
  });

  it("a same-named category and project get different colours (offset hash)", () => {
    // Not guaranteed distinct for every string, but the offset must apply.
    expect(projectColor("Work")).toBe(paletteColor(hashString("Work") + 5));
    expect(categoryColor("Work")).toBe(paletteColor(hashString("Work")));
  });

  it("donutSegmentPath sets the large-arc flag for sweeps > 180°", () => {
    // 270° sweep → both arcs must carry large-arc = 1.
    const p = donutSegmentPath(50, 50, 40, 24, 0, 270);
    expect(p).toContain("A 40 40 0 1 1");
    expect(p).toContain("A 24 24 0 1 0");
    // A quarter stays a small arc.
    const q = donutSegmentPath(50, 50, 40, 24, 0, 90);
    expect(q).toContain("A 40 40 0 0 1");
  });

  it("a full-circle segment (single category) renders as a large arc, not a sliver", () => {
    // Regression: `sweep % 360 > 180` cleared the large-arc flag at exactly
    // 360°, so a one-category donut drew a ~0° sliver instead of the ring.
    const p = donutSegmentPath(50, 50, 40, 24, 0, 360);
    expect(p).toContain("A 40 40 0 1 1");
    expect(p).toContain("A 24 24 0 1 0");
  });

  it("donutSegments preserve value proportions and ordering", () => {
    const segs = donutSegments([3, 1]);
    expect(segs[0]).toEqual({ start: 0, end: 270 });
    expect(segs[1]).toEqual({ start: 270, end: 360 });
    // Zero-value slice collapses to a zero-width segment in place.
    const withZero = donutSegments([1, 0, 1]);
    expect(withZero[1].start).toBe(withZero[1].end);
  });

  it("timelineBand clips events straddling the day edges", () => {
    const day = new Date(2026, 5, 20).getTime();
    const hour = 3_600_000;
    const end = day + 24 * hour;
    // Started yesterday → clipped to leftPct 0.
    const b = timelineBand(day - 2 * hour, day + 2 * hour, day, end)!;
    expect(b.leftPct).toBe(0);
    expect(b.widthPct).toBeCloseTo((2 / 24) * 100, 6);
    // Open event with `now` past the day end → clipped to the day.
    const o = timelineBand(day + 23 * hour, null, day, end + 5 * hour)!;
    expect(o.leftPct + o.widthPct).toBeCloseTo(100, 6);
    // Entirely outside the day → null.
    expect(timelineBand(day - 3 * hour, day - hour, day, end)).toBeNull();
  });

  it("timelineBand scales to an explicit DST day span and rejects empty spans", () => {
    const day = new Date(2026, 5, 20).getTime();
    const hour = 3_600_000;
    const dstEnd = day + 23 * hour; // spring-forward day
    const full = timelineBand(day, dstEnd, day, dstEnd + 99, dstEnd)!;
    expect(full.leftPct).toBe(0);
    expect(full.widthPct).toBeCloseTo(100, 6);
    // Half of a 23 h day is ~11.5 h.
    const half = timelineBand(day, day + 11.5 * hour, day, dstEnd, dstEnd)!;
    expect(half.widthPct).toBeCloseTo(50, 6);
    // Degenerate zero-length day window.
    expect(timelineBand(day, day + hour, day, day + hour, day)).toBeNull();
  });
});

describe("shortDayLabel", () => {
  it("parses YYYY-MM-DD and includes the day-of-month (locale-independent)", () => {
    // The exact weekday/month wording is locale-dependent, but the numeric day
    // must survive the split + local Date construction intact.
    expect(shortDayLabel("2026-07-27")).toContain("27");
    expect(shortDayLabel("2026-01-05")).toContain("5");
    expect(shortDayLabel("2026-12-31")).toContain("31");
  });

  it("distinguishes different dates", () => {
    expect(shortDayLabel("2026-01-01")).not.toBe(shortDayLabel("2026-12-01"));
  });

  it("does not off-by-one across the month boundary (local, not UTC)", () => {
    // Built as a LOCAL date, so the 1st never renders as the previous month's
    // last day regardless of the runner's timezone.
    expect(shortDayLabel("2026-03-01")).toContain("1");
    expect(shortDayLabel("2026-03-01")).not.toContain("28");
  });
});
