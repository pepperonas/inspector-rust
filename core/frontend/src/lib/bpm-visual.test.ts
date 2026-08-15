import { describe, it, expect } from "vitest";
import {
  clamp01,
  lerp,
  easeOutCubic,
  parseColor,
  mixRgb,
  rgba,
  spectrumBars,
  smoothBars,
  confidenceColor,
} from "./bpm-visual";

describe("clamp01 / lerp / easeOutCubic", () => {
  it("clamps to [0,1]", () => {
    expect(clamp01(-1)).toBe(0);
    expect(clamp01(0.5)).toBe(0.5);
    expect(clamp01(2)).toBe(1);
  });
  it("lerps linearly", () => {
    expect(lerp(0, 10, 0)).toBe(0);
    expect(lerp(0, 10, 0.5)).toBe(5);
    expect(lerp(0, 10, 1)).toBe(10);
  });
  it("ease-out is 0→1 monotonic and front-loaded", () => {
    expect(easeOutCubic(0)).toBe(0);
    expect(easeOutCubic(1)).toBe(1);
    expect(easeOutCubic(0.5)).toBeGreaterThan(0.5); // fast start
    expect(easeOutCubic(2)).toBe(1); // clamped
  });
});

describe("parseColor", () => {
  it("parses #rrggbb", () => {
    expect(parseColor("#ff8800")).toEqual({ r: 255, g: 136, b: 0 });
  });
  it("parses #rgb shorthand", () => {
    expect(parseColor("#f80")).toEqual({ r: 255, g: 136, b: 0 });
  });
  it("parses rgb()/rgba()", () => {
    expect(parseColor("rgb(10, 20, 30)")).toEqual({ r: 10, g: 20, b: 30 });
    expect(parseColor("rgba(10, 20, 30, 0.5)")).toEqual({ r: 10, g: 20, b: 30 });
  });
  it("returns null on garbage", () => {
    expect(parseColor("teal")).toBeNull();
    expect(parseColor("")).toBeNull();
  });
});

describe("mixRgb / rgba", () => {
  it("mixes endpoints", () => {
    const a = { r: 0, g: 0, b: 0 };
    const b = { r: 100, g: 200, b: 50 };
    expect(mixRgb(a, b, 0)).toEqual(a);
    expect(mixRgb(a, b, 1)).toEqual(b);
    expect(mixRgb(a, b, 0.5)).toEqual({ r: 50, g: 100, b: 25 });
  });
  it("formats rgba() with clamped alpha", () => {
    expect(rgba({ r: 1, g: 2, b: 3 }, 0.5)).toBe("rgba(1, 2, 3, 0.5)");
    expect(rgba({ r: 1, g: 2, b: 3 }, 5)).toBe("rgba(1, 2, 3, 1)");
  });
});

describe("spectrumBars", () => {
  it("produces the requested count, normalized to [0,1]", () => {
    const freq = new Uint8Array(512).fill(255);
    const bars = spectrumBars(freq, 64);
    expect(bars).toHaveLength(64);
    for (const v of bars) {
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThanOrEqual(1);
    }
    // an all-max spectrum normalizes to ~1 everywhere
    expect(bars[0]).toBeCloseTo(1, 5);
    expect(bars[63]).toBeCloseTo(1, 5);
  });
  it("is silent for a zero spectrum", () => {
    const bars = spectrumBars(new Uint8Array(512), 32);
    expect([...bars].every((v) => v === 0)).toBe(true);
  });
  it("handles degenerate inputs", () => {
    expect(spectrumBars(new Uint8Array(0), 16)).toHaveLength(16);
    expect(spectrumBars(new Uint8Array(64).fill(255), 0)).toHaveLength(0);
  });
  it("log spacing keeps low bars at fine (single-bin) resolution", () => {
    // A spike in only bin 1 should light the first bar but not the last.
    const freq = new Uint8Array(512);
    freq[1] = 255;
    const bars = spectrumBars(freq, 48);
    expect(bars[0]).toBeGreaterThan(0);
    expect(bars[47]).toBe(0);
  });
});

describe("smoothBars", () => {
  it("snaps up fast and falls slowly", () => {
    const prev = new Float32Array([0, 1]);
    const next = new Float32Array([1, 0]);
    smoothBars(prev, next, 1 / 60);
    // bar 0 rising: moved a large fraction toward 1
    expect(prev[0]).toBeGreaterThan(0.4);
    // bar 1 falling: still well above 0 after one short frame
    expect(prev[1]).toBeGreaterThan(0.5);
    expect(prev[1]).toBeLessThan(1);
  });
});

describe("confidenceColor", () => {
  it("ramps rose → amber → emerald", () => {
    expect(confidenceColor(0)).toEqual({ r: 244, g: 63, b: 94 });
    expect(confidenceColor(0.5)).toEqual({ r: 245, g: 158, b: 11 });
    expect(confidenceColor(1)).toEqual({ r: 16, g: 185, b: 129 });
  });
});

describe("spectrumBars — degenerate bin counts", () => {
  it("a 1-bin spectrum yields zeros, never NaN bar heights", () => {
    // The window walker can end up with an empty [lo,hi) slice when there are
    // fewer bins than bars; `sum / 0` would be NaN and NaN bar heights poison
    // every canvas path drawn from them.
    const bars = spectrumBars(new Uint8Array(1).fill(255), 4);
    expect(bars).toHaveLength(4);
    expect([...bars].every(Number.isFinite)).toBe(true);
    expect([...bars].every((v) => v >= 0 && v <= 1)).toBe(true);
  });

  it("more bars than bins still produces finite, in-range values", () => {
    const freq = new Uint8Array(8).fill(128);
    const bars = spectrumBars(freq, 64);
    expect([...bars].every(Number.isFinite)).toBe(true);
    expect([...bars].every((v) => v >= 0 && v <= 1)).toBe(true);
  });
});
