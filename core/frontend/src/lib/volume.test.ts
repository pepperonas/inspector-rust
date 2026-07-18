import { describe, it, expect } from "vitest";
import { snapVolumeStep } from "./volume";

describe("snapVolumeStep", () => {
  it("snaps off-grid starts onto the grid, then steps consistently", () => {
    expect(snapVolumeStep(81, 5)).toBe(85);
    expect(snapVolumeStep(85, 5)).toBe(90);
    expect(snapVolumeStep(81, -5)).toBe(80);
    expect(snapVolumeStep(80, -5)).toBe(75);
  });

  it("takes a full step from an on-grid value", () => {
    expect(snapVolumeStep(50, 5)).toBe(55);
    expect(snapVolumeStep(50, -5)).toBe(45);
    expect(snapVolumeStep(0, 5)).toBe(5);
    expect(snapVolumeStep(100, -5)).toBe(95);
  });

  it("clamps at the edges", () => {
    expect(snapVolumeStep(100, 5)).toBe(100);
    expect(snapVolumeStep(98, 5)).toBe(100);
    expect(snapVolumeStep(0, -5)).toBe(0);
    expect(snapVolumeStep(3, -5)).toBe(0);
  });

  it("handles non-integer input and degenerate deltas", () => {
    // A pointer-derived fractional current still snaps sanely.
    expect(snapVolumeStep(81.4, 5)).toBe(85);
    expect(snapVolumeStep(0.4, -5)).toBe(0);
    // |delta| < 1 never divides by zero.
    expect(snapVolumeStep(50, 0)).toBe(51);
  });

  it("always lands on the grid or a clamped edge (full 0–100 sweep)", () => {
    for (let cv = 0; cv <= 100; cv++) {
      for (const delta of [5, -5]) {
        const r = snapVolumeStep(cv, delta);
        const onGrid = r % Math.abs(delta) === 0;
        expect(onGrid || r === 0 || r === 100).toBe(true);
        // Direction is respected (unless already pinned at the edge).
        if (delta > 0 && cv < 100) expect(r).toBeGreaterThan(cv);
        if (delta < 0 && cv > 0) expect(r).toBeLessThan(cv);
        // A single step never moves more than a full step width.
        expect(Math.abs(r - cv)).toBeLessThanOrEqual(Math.abs(delta));
      }
    }
  });

  it("repeated stepping terminates at the edges (no oscillation)", () => {
    // From any start, at most 21 up-steps reach 100 and stay there.
    for (const start of [0, 1, 42, 81, 99]) {
      let v = start;
      for (let i = 0; i < 21; i++) v = snapVolumeStep(v, 5);
      expect(v).toBe(100);
      expect(snapVolumeStep(v, 5)).toBe(100);
      for (let i = 0; i < 21; i++) v = snapVolumeStep(v, -5);
      expect(v).toBe(0);
      expect(snapVolumeStep(v, -5)).toBe(0);
    }
  });

  it("clamps out-of-range current values back into 0–100", () => {
    // A drifted/garbage device read-back must still produce a sane level.
    expect(snapVolumeStep(-10, 5)).toBe(0); // (-2+1)·5 = -5 → clamp 0
    expect(snapVolumeStep(-3, -5)).toBe(0);
    expect(snapVolumeStep(150, -5)).toBe(100);
    expect(snapVolumeStep(101, 5)).toBe(100);
  });

  it("supports other grid widths (1 and 10)", () => {
    expect(snapVolumeStep(50, 1)).toBe(51);
    expect(snapVolumeStep(50, -1)).toBe(49);
    expect(snapVolumeStep(95, 10)).toBe(100); // 100 clamps onto the edge
    expect(snapVolumeStep(95, -10)).toBe(90);
    expect(snapVolumeStep(4, -10)).toBe(0);
  });

  it("matches the Rust snap_volume_step formula across a sweep", () => {
    // Same reference table as the Rust unit test — the two implementations
    // must agree so panel arrows and gesture swipes land on the same grid.
    for (const delta of [5, -5, 6, -6, 10, -10]) {
      for (const cv of [0, 1, 3, 49, 50, 81, 98, 100]) {
        const step = Math.abs(delta);
        const raw =
          delta >= 0
            ? (Math.floor(cv / step) + 1) * step
            : (Math.ceil(cv / step) - 1) * step;
        const expected = Math.max(0, Math.min(100, raw));
        expect(snapVolumeStep(cv, delta)).toBe(expected);
      }
    }
  });
});
