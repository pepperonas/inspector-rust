import { describe, expect, it } from "vitest";
import { barSegments, chartModel, forecastText, forecastTone, paceText } from "./claude-limits";
import type { LimitForecast } from "./ipc";

// Week Sat 2026-10-03 23:00Z → Sat 2026-10-10 23:00Z (= Sun 01:00 Berlin, CEST).
const WEEK = { start: "2026-10-03T23:00:00Z", end: "2026-10-10T23:00:00Z" };

function fc(over: Partial<LimitForecast>): LimitForecast {
  return {
    version: 1,
    basis: "linear",
    confidence: "rough",
    status: "reserve",
    window: WEEK,
    now: "2026-10-07T11:00:00Z",
    pace: { planPercent: 50, deltaPoints: -7.2 },
    atReset: { median: 78.4, low: 61, high: 96 },
    exhaustsAt: null,
    k: null,
    notes: [],
    series: null,
    source: "local",
    ...over,
  };
}

const NOW = Date.parse("2026-10-07T11:00:00Z");

describe("forecastTone", () => {
  it("maps status to colour, nothing for idle/unknown", () => {
    expect(forecastTone(fc({ status: "reserve" }))).toBe("ok");
    expect(forecastTone(fc({ status: "ahead" }))).toBe("warn");
    expect(forecastTone(fc({ status: "exhausts" }))).toBe("crit");
    expect(forecastTone(fc({ status: "idle" }))).toBeNull();
    expect(forecastTone(null)).toBeNull();
  });
});

describe("paceText", () => {
  it("says reserve, ahead or on plan", () => {
    expect(paceText(-7.2)).toBe("7 Punkte Reserve");
    expect(paceText(8.4)).toBe("8 Punkte voraus");
    expect(paceText(1)).toBe("1 Punkt voraus");
    expect(paceText(0.4)).toBe("genau im Plan");
  });
});

describe("forecastText", () => {
  it("reserve: pace · expected at reset · reset time · rough", () => {
    expect(forecastText(fc({}), NOW)).toBe(
      "7 Punkte Reserve · voraussichtlich 78 % · Reset So 01:00 · grobe Schätzung",
    );
  });
  it("good confidence drops the hedge", () => {
    expect(forecastText(fc({ confidence: "good" }), NOW)).not.toContain("grobe Schätzung");
  });
  it("week exhausting: day and time with the spread", () => {
    const f = fc({
      status: "exhausts",
      confidence: "good",
      exhaustsAt: {
        median: "2026-10-08T12:20:00Z", // Do 14:20 Berlin
        early: "2026-10-07T20:00:00Z", // Mi 22 Uhr
        late: "2026-10-09T07:00:00Z", // Fr 9 Uhr
      },
    });
    expect(forecastText(f, NOW)).toBe("leer Do ~14:20 (Mi 22 Uhr – Fr 9 Uhr)");
  });
  it("only the fast end known: 'frühestens'", () => {
    const f = fc({
      status: "exhausts",
      confidence: "good",
      exhaustsAt: { median: "2026-10-08T12:20:00Z", early: "2026-10-07T20:00:00Z", late: null },
    });
    expect(forecastText(f, NOW)).toBe("leer Do ~14:20 (frühestens Mi 22 Uhr)");
  });
  it("session exhausting: relative hours", () => {
    const f = fc({
      status: "exhausts",
      window: { start: "2026-10-07T09:00:00Z", end: "2026-10-07T14:00:00Z" },
      exhaustsAt: { median: "2026-10-07T12:40:00Z", early: null, late: null },
    });
    expect(forecastText(f, NOW)).toBe("leer in ~1:40 h · grobe Schätzung");
  });
  it("too early, idle and unknown", () => {
    expect(forecastText(fc({ notes: ["too_early"], atReset: null, confidence: "none" }), NOW)).toBe(
      "7 Punkte Reserve · zu früh für eine Hochrechnung · Reset So 01:00",
    );
    expect(forecastText(fc({ status: "idle" }), NOW)).toBe("noch nichts verbraucht");
    expect(forecastText(fc({ status: "unknown" }), NOW)).toBeNull();
    expect(forecastText(null, NOW)).toBeNull();
  });
});

describe("barSegments", () => {
  it("fill, plan line and projection to the reset", () => {
    expect(barSegments(49, fc({ pace: { planPercent: 50, deltaPoints: -1 }, atReset: { median: 78, low: 0, high: 0 } }))).toEqual({
      fill: 49,
      plan: 50,
      projection: 78,
      over: false,
    });
  });
  it("a projection past 100 is clamped and flagged for the red cap", () => {
    const s = barSegments(60, fc({ atReset: { median: 120, low: 0, high: 0 } }));
    expect(s.projection).toBe(100);
    expect(s.over).toBe(true);
  });
  it("no forecast → just the fill", () => {
    expect(barSegments(30, null)).toEqual({ fill: 30, plan: null, projection: null, over: false });
  });
  it("a projection not above the fill draws nothing extra", () => {
    expect(barSegments(40, fc({ atReset: { median: 40, low: 0, high: 0 } })).projection).toBeNull();
  });
});

describe("chartModel", () => {
  const series = {
    actual: [
      ["2026-10-03T23:00:00Z", 0],
      ["2026-10-07T11:00:00Z", 50],
    ] as [string, number][],
    measured: [["2026-10-05T23:00:00Z", 20]] as [string, number][],
    forecast: [
      ["2026-10-07T11:00:00Z", 50, 50, 50],
      ["2026-10-09T11:00:00Z", 90, 80, 100],
      ["2026-10-10T11:00:00Z", 110, 90, 130],
    ] as [string, number, number, number][],
    ghosts: [{ start: "2026-09-26T23:00:00Z", points: [[0, 0], [10080, 70]] as [number, number][] }],
  };

  it("needs a window and a series", () => {
    expect(chartModel(fc({}), 300, 100)).toBeNull();
  });

  it("scales time to x and percent to y, with 120 % room on an overrun", () => {
    const m = chartModel(fc({ series }), 280, 120)!;
    expect(m.yMax).toBe(120);
    expect(m.y100).toBeCloseTo(20);
    // now = 3.5 of 7 days → half the width
    expect(m.nowX).toBeCloseTo(140);
    expect(m.plan).toBe("M0.0 120.0 L280.0 20.0");
    expect(m.measured[0].x).toBeCloseTo(80);
    expect(m.measured[0].y).toBeCloseTo(100);
    expect(m.ghosts).toHaveLength(1);
    expect(m.band.endsWith("Z")).toBe(true);
  });

  it("marks where the median crosses 100 (interpolated)", () => {
    const m = chartModel(fc({ series }), 280, 120)!;
    // 90 → 110 over 24 h: 100 is reached 12 h after Fri 11:00 → Fri 23:00Z.
    expect(m.hit!.ms).toBe(Date.parse("2026-10-09T23:00:00Z"));
    expect(m.hit!.y).toBeCloseTo(20);
  });

  it("no crossing, no marker, and 100 % is the top", () => {
    const calm = { ...series, forecast: [["2026-10-09T11:00:00Z", 60, 50, 70]] as [string, number, number, number][], ghosts: [] };
    const m = chartModel(fc({ series: calm }), 280, 100)!;
    expect(m.hit).toBeNull();
    expect(m.yMax).toBe(100);
  });

  it("shades nights 22–7 Berlin: seven of them in a week starting 01:00", () => {
    const m = chartModel(fc({ series }), 700, 100)!;
    // Sun 01:00 start sits inside a night → first range starts at x 0.
    expect(m.nights[0].x).toBe(0);
    expect(m.nights.length).toBeGreaterThanOrEqual(7);
    // A full night is 9 h of 168 → 37.5 px of 700.
    expect(m.nights[1].w).toBeCloseTo(37.5);
  });
});
