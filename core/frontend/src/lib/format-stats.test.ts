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
});
