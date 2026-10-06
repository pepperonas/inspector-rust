import { describe, expect, it } from "vitest";
import {
  ageText,
  dominantLevel,
  forecastText,
  formatBytes,
  formatMinutes,
  formatRate,
  pressureLevel,
  pressureShares,
  series,
  swapShare,
  writeBars,
  PULSE_RANGES,
  RANGE_LABEL,
} from "./pulse";
import type { PulseSample } from "./ipc";

describe("pressureLevel", () => {
  it("maps the kernel levels", () => {
    expect(pressureLevel(1)).toBe("normal");
    expect(pressureLevel(2)).toBe("warn");
    expect(pressureLevel(4)).toBe("critical");
    expect(pressureLevel(0)).toBe("unknown");
    expect(pressureLevel(null)).toBe("unknown");
  });
});

describe("formatting", () => {
  it("uses decimal units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1_500_000)).toBe("1,50 MB");
    expect(formatBytes(20_000_000_000)).toBe("20,0 GB");
    expect(formatBytes(600e12)).toBe("600 TB");
    expect(formatBytes(-1)).toBe("–");
  });
  it("rates below 1 KB/s read as zero", () => {
    expect(formatRate(500)).toBe("0 MB/s");
    expect(formatRate(12_400_000)).toBe("12,4 MB/s");
  });
  it("minutes", () => {
    expect(formatMinutes(0)).toBe("0 min");
    expect(formatMinutes(30)).toBe("< 1 min");
    expect(formatMinutes(600)).toBe("10 min");
    expect(formatMinutes(3600 + 720)).toBe("1 h 12 min");
    expect(formatMinutes(7200)).toBe("2 h");
  });
});

describe("pressure aggregates", () => {
  it("shares add up and an empty bucket is all zero", () => {
    const s = pressureShares({ p_normal: 30, p_warn: 10, p_crit: 60 });
    expect(s.normal + s.warn + s.critical).toBeCloseTo(1);
    expect(s.critical).toBeCloseTo(0.6);
    expect(pressureShares({ p_normal: 0, p_warn: 0, p_crit: 0 })).toEqual({ normal: 0, warn: 0, critical: 0 });
  });
  it("a tie goes to the worse level", () => {
    expect(dominantLevel({ p_normal: 30, p_warn: 0, p_crit: 30, secs: 60 })).toBe("critical");
    expect(dominantLevel({ p_normal: 30, p_warn: 30, p_crit: 0, secs: 60 })).toBe("warn");
    expect(dominantLevel({ p_normal: 50, p_warn: 10, p_crit: 0, secs: 60 })).toBe("normal");
    expect(dominantLevel({ p_normal: 0, p_warn: 0, p_crit: 0, secs: 0 })).toBe("unknown");
  });
});

describe("writes", () => {
  it("swap share is capped and undefined without writes", () => {
    expect(swapShare(100, 25)).toBe(0.25);
    expect(swapShare(100, 300)).toBe(1);
    expect(swapShare(0, 5)).toBeNull();
  });
  it("bars scale to the largest bucket and swap never exceeds the total", () => {
    const b = writeBars(
      [
        { written: 50, swap_written: 10 },
        { written: 100, swap_written: 200 },
        { written: 0, swap_written: 0 },
      ],
      40,
    );
    expect(b[1].total).toBe(40);
    expect(b[0].total).toBe(20);
    expect(b[0].swap).toBe(4);
    expect(b[1].swap).toBe(40);
    expect(b[2]).toEqual({ total: 0, swap: 0 });
    expect(writeBars([{ written: 0, swap_written: 0 }], 40)).toEqual([{ total: 0, swap: 0 }]);
  });
});

describe("series", () => {
  it("keeps gaps as null", () => {
    const s = [{ cpu_pct: 10 }, { cpu_pct: null }, { cpu_pct: 30 }] as unknown as PulseSample[];
    expect(series(s, "cpu_pct")).toEqual([10, null, 30]);
  });
});

describe("forecastText", () => {
  it("never projects without a disk or without a day of data", () => {
    expect(forecastText(null)).toMatch(/keine Prognose/);
    expect(forecastText({ tbw_bytes: 3e14, per_day: 1e10, days: 0.3, lifetime_written: null, years: null })).toMatch(
      /zu wenig Daten/,
    );
  });
  it("always labels the figure as an estimate", () => {
    const t = forecastText({ tbw_bytes: 3e14, per_day: 50e9, days: 30, lifetime_written: 1e14, years: 10.96 });
    expect(t).toMatch(/^Schätzung:/);
    expect(t).toContain("ca. 11 Jahre");
    expect(t).toContain("50,0 GB pro Tag");
    const fromZero = forecastText({ tbw_bytes: 3e14, per_day: 50e9, days: 30, lifetime_written: null, years: 16.4 });
    expect(fromZero).toContain("ab null gerechnet");
    expect(forecastText({ tbw_bytes: 3e14, per_day: 1e6, days: 30, lifetime_written: null, years: 800 })).toContain(
      "über 100 Jahre",
    );
  });
});

describe("misc", () => {
  it("every range has a label and the list matches the backend plans", () => {
    expect(PULSE_RANGES).toEqual(["1h", "24h", "7d", "30d", "90d"]);
    for (const r of PULSE_RANGES) expect(RANGE_LABEL[r]).toBeTruthy();
  });
  it("the backend serves exactly these ranges", async () => {
    const { readFileSync } = (await import("node:" + "fs")) as unknown as {
      readFileSync(path: string, encoding: "utf8"): string;
    };
    const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
    const src = readFileSync(cwd + "/../rust-lib/src/pulse/sampler.rs", "utf8");
    const plan = src.slice(src.indexOf("pub fn history_plan"), src.indexOf("/// Fold one process scan"));
    for (const r of PULSE_RANGES) expect(plan).toContain(`"${r}" =>`);
    expect((plan.match(/"\w+" =>/g) ?? []).length).toBe(PULSE_RANGES.length);
  });
  it("ageText", () => {
    expect(ageText(100, 0)).toBe("noch nicht erfasst");
    expect(ageText(100, 88)).toBe("vor 12 s");
    expect(ageText(1000, 100)).toBe("vor 15 min");
  });
});
