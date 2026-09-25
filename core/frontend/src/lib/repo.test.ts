import { describe, it, expect } from "vitest";
import { RANGES, heatLevel, calendarCells, repoErrorHint, deltaLabel, githubErrorHint, windowsComplete, bucketLabel as bl, churnGeometry as cg, rangeTitle as rt } from "./repo";
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

describe("repo ranges + heat + calendar", () => {
  it("offers the five ranges in order", () => {
    expect(RANGES.map((r) => r.key)).toEqual(["d30", "d90", "d180", "y1", "all"]);
  });
  it("maps a value to 0..4 and never divides by zero", () => {
    expect(heatLevel(0, 10)).toBe(0);
    expect(heatLevel(10, 10)).toBe(4);
    expect(heatLevel(1, 10)).toBe(1);
    expect(heatLevel(5, 0)).toBe(0);
  });
  it("places days Monday-first in week columns (same as Rust calendar_svg)", () => {
    const cells = calendarCells([
      { date: "2026-08-24", commits: 2 }, // Monday
      { date: "2026-08-30", commits: 1 }, // Sunday
      { date: "2026-08-31", commits: 1 }, // next Monday
    ]);
    expect(cells.map((c) => [c.col, c.row])).toEqual([[0, 0], [0, 6], [1, 0]]);
    expect(calendarCells([])).toEqual([]);
  });
  it("turns sentinels into human hints", () => {
    expect(repoErrorHint("repo.auth: remote: Repository not found").title).toMatch(/Kein Zugriff/);
    expect(repoErrorHint("repo.network: Could not resolve host").title).toMatch(/Keine Verbindung/);
    expect(repoErrorHint("repo.no_target").title).toMatch(/Kein Repository/);
    expect(repoErrorHint("irgendwas").title).toMatch(/fehlgeschlagen/);
  });
});

import { rangeTitle, bucketLabel, churnGeometry, netTotal } from "./repo";

describe("timeline display (v0.183.x)", () => {
  it("titles the activity by the RANGE, never by a month count", () => {
    expect(rangeTitle("d30")).toBe("letzte 30 Tage");
    expect(rangeTitle("d90")).toBe("letzte 90 Tage");
    expect(rangeTitle("y1")).toBe("letztes Jahr");
    expect(rangeTitle("all")).toBe("gesamt");
  });
  it("labels buckets by granularity", () => {
    expect(bucketLabel("2026-09-25", "day")).toBe("25.09.");
    expect(bucketLabel("2026-09-21", "week")).toBe("ab 21.09.");
    expect(bucketLabel("2026-09-01", "month")).toBe("09/2026");
  });
  it("net total signs and formats", () => {
    expect(netTotal(16, 10)).toBe(6);
    expect(netTotal(0, 7)).toBe(-7);
  });
  const tl = [
    { start: "a", commits: 1, insertions: 10, deletions: 0 },
    { start: "b", commits: 1, insertions: 0, deletions: 5 },
    { start: "c", commits: 1, insertions: 5, deletions: 5 },
  ];
  it("uses ONE scale for added and removed lines", () => {
    const g = churnGeometry(tl, 300, 100);
    const a = g.bars[0];
    const d = g.bars[1];
    // 10 added vs 5 removed → the added bar is exactly twice as tall.
    expect(a.addH).toBeCloseTo(d.delH * 2, 5);
    expect(a.addY + a.addH).toBeCloseTo(g.zeroY, 5);
    expect(d.delY).toBeCloseTo(g.zeroY, 5);
  });
  it("puts the zero line where the data needs it (mostly-additive → low)", () => {
    const g = churnGeometry([{ insertions: 100, deletions: 1 }], 300, 100);
    expect(g.zeroY).toBeGreaterThan(70);
  });
  it("draws the cumulative net line ending at the final net", () => {
    const g = churnGeometry(tl, 300, 100);
    expect(g.netPoints.length).toBe(3);
    // net after a/b/c = 10, 5, 5 → last two points at the same height
    expect(g.netPoints[1][1]).toBeCloseTo(g.netPoints[2][1], 5);
    expect(g.netPoints[0][1]).toBeLessThan(g.netPoints[1][1]);
  });
  it("empty timeline → no geometry", () => {
    expect(churnGeometry([], 300, 100).bars).toEqual([]);
  });
});

describe("recent activity helpers", () => {
  it("labels the change vs the previous period neutrally", () => {
    expect(deltaLabel(5, 2)).toEqual({ text: "↑ 3", title: "Vorperiode: 2" });
    expect(deltaLabel(1, 4)).toEqual({ text: "↓ 3", title: "Vorperiode: 4" });
    expect(deltaLabel(3, 3)).toEqual({ text: "±0", title: "Vorperiode: 3" });
  });
  it("turns GitHub API errors into hints", () => {
    expect(githubErrorHint("github.rate_limit: HTTP 403")).toMatch(/Abfragelimit/);
    expect(githubErrorHint("github.not_found: HTTP 404")).toMatch(/nicht gefunden|kein Zugriff/i);
    expect(githubErrorHint("github.network: timeout")).toMatch(/nicht erreichbar/);
    expect(githubErrorHint("repo.auth: HTTP 401")).toMatch(/Token/);
  });
});

describe("GitHub coverage (review fix)", () => {
  const NOW = 1_790_000_000;
  const DAY = 86_400;
  it("a window is complete only if it starts at/after the coverage point", () => {
    expect(windowsComplete(null, NOW)).toEqual([true, true, true, true]);
    // data complete for the last 3 days: both 24-h windows yes, both weeks no
    expect(windowsComplete(NOW - 3 * DAY, NOW)).toEqual([true, true, false, false]);
    // 1.5 days: 24 h complete, the previous day not
    expect(windowsComplete(NOW - 1.5 * DAY, NOW)).toEqual([true, false, false, false]);
    expect(windowsComplete(NOW - DAY, NOW)[0]).toBe(true);
  });
});

describe("edge cases (release 0.185.0)", () => {
  it("a shrinking codebase draws the net line BELOW the zero line", () => {
    const g = cg([{ insertions: 1, deletions: 10 }, { insertions: 0, deletions: 5 }], 300, 100);
    expect(g.netPoints.every(([, y]) => y > g.zeroY)).toBe(true);
    // …and never outside the chart.
    expect(g.netPoints.every(([, y]) => y <= 100)).toBe(true);
    expect(g.bars.every((b) => b.addY >= 0 && b.delY + b.delH <= 100 + 1e-9)).toBe(true);
  });
  it("a single bucket still yields geometry (no division by zero)", () => {
    const g = cg([{ insertions: 3, deletions: 0 }], 300, 100);
    expect(g.bars).toHaveLength(1);
    expect(Number.isFinite(g.netPoints[0][1])).toBe(true);
  });
  it("junk dates never throw", () => {
    expect(bl("nope", "day")).toBe("nope");
    expect(calendarCells([{ date: "31.12.2026", commits: 1 }, { date: "2026-01-05", commits: 2 }])).toEqual([
      { col: 0, row: 0, date: "2026-01-05", commits: 2 },
    ]);
  });
  it("every range has a title", () => {
    for (const r of RANGES) expect(rt(r.key).length).toBeGreaterThan(0);
  });
  it("error hints strip the internal prefix and have a fallback", () => {
    expect(repoErrorHint("repo.git: fatal: kaputt").body).toBe("fatal: kaputt");
    expect(githubErrorHint("github.http: HTTP 500")).toMatch(/nicht verfügbar/);
  });
  it("a delta from zero is an increase, zero to zero is flat", () => {
    expect(deltaLabel(3, 0).text).toBe("↑ 3");
    expect(deltaLabel(0, 0).text).toBe("±0");
  });
  it("coverage exactly on a window start still counts as complete", () => {
    expect(windowsComplete(1000 - 86_400, 1000)).toEqual([true, false, false, false]);
  });
});

