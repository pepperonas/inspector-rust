import { describe, expect, it } from "vitest";
import {
  barShare,
  deltaPercent,
  downloadMeaning,
  formatMbps,
  formatMs,
  overallProgress,
  PHASE_LABELS,
  rateDownload,
} from "./speedtest";

describe("speedtest helpers", () => {
  it("formats Mbit/s with three significant digits and a decimal comma", () => {
    expect(formatMbps(452_304_161)).toBe("452");
    expect(formatMbps(44_196_779)).toBe("44,2");
    expect(formatMbps(8_370_000)).toBe("8,37");
    expect(formatMbps(null)).toBe("—");
    expect(formatMbps(Number.NaN)).toBe("—");
  });

  it("formats milliseconds", () => {
    expect(formatMs(38.87)).toBe("39 ms");
    expect(formatMs(6.48)).toBe("6,5 ms");
    expect(formatMs(undefined)).toBe("—");
  });

  it("maps phase progress onto one overall bar, monotonically", () => {
    expect(overallProgress("meta", 0)).toBe(0);
    expect(overallProgress("upload", 1)).toBe(1);
    const order = ["meta", "latency", "download", "upload"] as const;
    let prev = -1;
    for (const p of order) {
      for (const f of [0, 0.5, 1]) {
        const v = overallProgress(p, f);
        expect(v).toBeGreaterThanOrEqual(prev);
        prev = v;
      }
    }
    expect(overallProgress("download", 7)).toBe(overallProgress("download", 1));
  });

  it("rates download speeds in plain language", () => {
    expect(rateDownload(452e6)).toBe("sehr gut");
    expect(rateDownload(100e6)).toBe("gut");
    expect(rateDownload(20e6)).toBe("mittel");
    expect(rateDownload(5e6)).toBe("langsam");
    expect(rateDownload(null)).toBeNull();
    expect(downloadMeaning(5e6)).toMatch(/ruckeln/);
  });

  it("compares against the median of earlier runs", () => {
    expect(deltaPercent(110, [100, 90, 200])).toBe(10); // median 100
    expect(deltaPercent(50, [100, 100])).toBe(-50);
    expect(deltaPercent(100, [])).toBeNull();
    expect(deltaPercent(100, [null, 0])).toBeNull(); // missing / zero don't count
    expect(deltaPercent(null, [100])).toBeNull();
  });

  it("scales bars against the largest value and clamps", () => {
    expect(barShare(50, 100)).toBe(0.5);
    expect(barShare(200, 100)).toBe(1);
    expect(barShare(null, 100)).toBe(0);
    expect(barShare(10, 0)).toBe(0);
  });

  it("has a label for every phase", () => {
    expect(Object.keys(PHASE_LABELS).sort()).toEqual(["download", "latency", "meta", "upload"]);
  });

  it("phase names match the Rust progress events", async () => {
    const { readFileSync } = (await import("node:" + "fs")) as unknown as {
      readFileSync(path: string, encoding: "utf8"): string;
    };
    const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
    const rs = readFileSync(cwd + "/../rust-lib/src/speedtest.rs", "utf8");
    for (const p of Object.keys(PHASE_LABELS)) expect(rs).toContain(`"${p}"`);
  });
});
