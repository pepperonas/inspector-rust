import { describe, expect, it } from "vitest";
import { barGeometry, peakDecay } from "./equalizer-visual";

describe("peakDecay", () => {
  it("snaps instantly up to a bar that rose above the peak", () => {
    const peaks = new Float32Array([0.2, 0.5]);
    const bars = new Float32Array([0.8, 0.9]);
    peakDecay(peaks, bars, 0.016);
    expect(peaks[0]).toBeCloseTo(0.8, 6);
    expect(peaks[1]).toBeCloseTo(0.9, 6);
  });

  it("falls slowly when the bar drops below the peak", () => {
    const peaks = new Float32Array([0.9]);
    const bars = new Float32Array([0.1]);
    peakDecay(peaks, bars, 0.1, 0.55); // fall = 0.055
    expect(peaks[0]).toBeCloseTo(0.845, 6);
  });

  it("never falls below the bar's current value", () => {
    const peaks = new Float32Array([0.5]);
    const bars = new Float32Array([0.4]);
    // A huge dt would over-fall; the bar floor must win.
    peakDecay(peaks, bars, 100, 0.55);
    expect(peaks[0]).toBeCloseTo(0.4, 6);
  });

  it("clamps bar values into [0,1] before comparing", () => {
    const peaks = new Float32Array([0.3]);
    const bars = new Float32Array([1.5]); // out of range
    peakDecay(peaks, bars, 0.016);
    expect(peaks[0]).toBe(1);
  });

  it("is frame-rate corrected: larger dt falls further", () => {
    const slow = new Float32Array([0.9]);
    const fast = new Float32Array([0.9]);
    const bars = new Float32Array([0]);
    peakDecay(slow, bars, 0.016, 1);
    peakDecay(fast, bars, 0.05, 1);
    expect(fast[0]).toBeLessThan(slow[0]);
  });

  it("only touches the overlapping prefix when lengths differ", () => {
    const peaks = new Float32Array([0.1, 0.1, 0.1]);
    const bars = new Float32Array([0.9]); // shorter
    peakDecay(peaks, bars, 0.016);
    expect(peaks[0]).toBeCloseTo(0.9, 6);
    expect(peaks[1]).toBeCloseTo(0.1, 6); // untouched (float32 stores 0.1 imprecisely)
    expect(peaks[2]).toBeCloseTo(0.1, 6);
  });
});

describe("barGeometry", () => {
  it("spaces bars evenly and centres them in their slot", () => {
    const g = barGeometry(100, 4, 0.34);
    expect(g.step).toBeCloseTo(25, 6);
    expect(g.barW).toBeCloseTo(25 * 0.66, 6);
    // first bar padded by half the gap, then each is one step apart
    const pad = (25 - g.barW) / 2;
    expect(g.x(0)).toBeCloseTo(pad, 6);
    expect(g.x(1)).toBeCloseTo(25 + pad, 6);
    expect(g.x(3)).toBeCloseTo(75 + pad, 6);
  });

  it("bars + gaps fill exactly the width (last bar's right edge = width)", () => {
    const g = barGeometry(200, 8, 0.4);
    const lastRight = g.x(7) + g.barW;
    expect(lastRight).toBeCloseTo(200 - (g.step - g.barW) / 2, 6);
  });

  it("gapRatio 0 → touching bars fill the width", () => {
    const g = barGeometry(100, 5, 0);
    expect(g.barW).toBeCloseTo(20, 6);
    expect(g.x(0)).toBeCloseTo(0, 6);
    expect(g.x(4) + g.barW).toBeCloseTo(100, 6);
  });

  it("returns safe zeros for non-positive count or width", () => {
    expect(barGeometry(0, 8).barW).toBe(0);
    expect(barGeometry(100, 0).step).toBe(0);
    expect(barGeometry(-10, 8).x(3)).toBe(0);
  });

  it("clamps an out-of-range gapRatio", () => {
    const g = barGeometry(100, 4, 5); // clamped to 1 → zero-width bars
    expect(g.barW).toBe(0);
  });
});
