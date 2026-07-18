import { describe, it, expect } from "vitest";
import {
  humanBytes,
  humanRate,
  humanUptime,
  humanDuration,
  clampPct,
  usedPct,
} from "./format-stats";

describe("format-stats", () => {
  it("humanBytes scales 1024-based with sensible precision", () => {
    expect(humanBytes(0)).toBe("0 B");
    expect(humanBytes(512)).toBe("512 B");
    expect(humanBytes(1024)).toBe("1.0 KB");
    expect(humanBytes(1536)).toBe("1.5 KB");
    expect(humanBytes(512 * 1024)).toBe("512 KB");
    expect(humanBytes(1.4 * 1024 * 1024 * 1024)).toBe("1.4 GB");
    expect(humanBytes(20 * 1024 * 1024 * 1024)).toBe("20 GB");
  });

  it("humanBytes guards against bad input", () => {
    expect(humanBytes(-5)).toBe("0 B");
    expect(humanBytes(NaN)).toBe("0 B");
  });

  it("humanRate appends /s", () => {
    expect(humanRate(0)).toBe("0 B/s");
    expect(humanRate(2 * 1024 * 1024)).toBe("2.0 MB/s");
  });

  it("humanUptime is compact and unit-stepped", () => {
    expect(humanUptime(0)).toBe("0m");
    expect(humanUptime(45)).toBe("45s");
    expect(humanUptime(12 * 60)).toBe("12m");
    expect(humanUptime(4 * 3600 + 12 * 60)).toBe("4h 12m");
    expect(humanUptime(3 * 86400 + 4 * 3600)).toBe("3d 4h");
  });

  it("humanDuration formats battery time-remaining, — when unknown", () => {
    expect(humanDuration(0)).toBe("—");
    expect(humanDuration(NaN)).toBe("—");
    expect(humanDuration(30)).toBe("30s");
    expect(humanDuration(45 * 60)).toBe("45m");
    expect(humanDuration(2 * 3600 + 15 * 60)).toBe("2h 15m");
  });

  it("clampPct and usedPct stay in [0,100]", () => {
    expect(clampPct(-10)).toBe(0);
    expect(clampPct(150)).toBe(100);
    expect(clampPct(NaN)).toBe(0);
    expect(usedPct(50, 100)).toBe(50);
    expect(usedPct(10, 0)).toBe(0);
    expect(usedPct(200, 100)).toBe(100);
  });

  it("humanBytes handles the 1024 boundary + the two-digit precision switch", () => {
    expect(humanBytes(1023)).toBe("1023 B"); // last whole-byte value
    expect(humanBytes(9.9 * 1024)).toBe("9.9 KB"); // < 10 → one decimal
    expect(humanBytes(10 * 1024)).toBe("10 KB"); // ≥ 10 → whole number
    expect(humanBytes(Math.pow(1024, 5))).toBe("1.0 PB");
  });

  it("humanBytes never runs past PB (unit cap, no crash)", () => {
    // 2^60 = 1024 PB — the loop stops at the last unit instead of inventing EB.
    expect(humanBytes(Math.pow(2, 60))).toBe("1024 PB");
  });

  it("humanBytes treats Infinity as unknown", () => {
    expect(humanBytes(Infinity)).toBe("0 B");
  });

  it("humanUptime shows a zero remainder at exact unit boundaries", () => {
    expect(humanUptime(3600)).toBe("1h 0m");
    expect(humanUptime(86400)).toBe("1d 0h");
    expect(humanUptime(59.9)).toBe("59s"); // floors, never rounds up to 1m
  });

  it("humanDuration at exact boundaries + Infinity", () => {
    expect(humanDuration(3600)).toBe("1h 0m");
    expect(humanDuration(59.9)).toBe("59s");
    expect(humanDuration(Infinity)).toBe("—");
    expect(humanDuration(-30)).toBe("—");
  });

  it("clampPct passes the exact bounds through", () => {
    expect(clampPct(0)).toBe(0);
    expect(clampPct(100)).toBe(100);
    expect(clampPct(42.5)).toBe(42.5);
  });

  it("usedPct is proportional and guards bad inputs", () => {
    // Note: the value is proportional, not rounded — callers round for display.
    expect(usedPct(1, 4)).toBeCloseTo(25, 10);
    expect(usedPct(1, 3)).toBeCloseTo(33.333, 2);
    expect(usedPct(0, 100)).toBe(0);
    expect(usedPct(-5, 100)).toBe(0); // negative used clamps to 0
    expect(usedPct(NaN, 100)).toBe(0);
    expect(usedPct(50, NaN)).toBe(0);
    expect(usedPct(50, -10)).toBe(0); // negative total is invalid
  });
});
