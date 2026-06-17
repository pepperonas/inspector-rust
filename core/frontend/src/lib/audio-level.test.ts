import { describe, it, expect } from "vitest";
import { rms, rmsToDbfs, dbfsToLevel, smoothStep } from "./audio-level";

describe("rms", () => {
  it("is 0 for an empty or silent buffer", () => {
    expect(rms(new Float32Array(0))).toBe(0);
    expect(rms(new Float32Array([0, 0, 0, 0]))).toBe(0);
  });
  it("equals the amplitude of a constant signal", () => {
    expect(rms([0.5, 0.5, 0.5, 0.5])).toBeCloseTo(0.5, 6);
    expect(rms([-1, -1, -1])).toBeCloseTo(1, 6);
  });
  it("is the RMS of a ±1 square wave (=1)", () => {
    expect(rms([1, -1, 1, -1])).toBeCloseTo(1, 6);
  });
  it("computes a known mixed case", () => {
    // sqrt((3^2+4^2)/2) = sqrt(12.5)
    expect(rms([3, 4])).toBeCloseTo(Math.sqrt(12.5), 6);
  });
});

describe("rmsToDbfs", () => {
  it("maps 1.0 → 0 dBFS and 0.5 → ≈ -6 dBFS", () => {
    expect(rmsToDbfs(1)).toBeCloseTo(0, 4);
    expect(rmsToDbfs(0.5)).toBeCloseTo(-6.0206, 2);
    expect(rmsToDbfs(0.1)).toBeCloseTo(-20, 2);
  });
  it("returns the finite silent floor (not -Infinity) near zero", () => {
    expect(rmsToDbfs(0)).toBe(-120);
    expect(rmsToDbfs(1e-9)).toBe(-120);
    expect(rmsToDbfs(0, -90)).toBe(-90);
    expect(Number.isFinite(rmsToDbfs(0))).toBe(true);
  });
});

describe("dbfsToLevel", () => {
  it("clamps below the floor to 0 and above the ceiling to 1", () => {
    expect(dbfsToLevel(-80, -50, -10)).toBe(0);
    expect(dbfsToLevel(-50, -50, -10)).toBe(0);
    expect(dbfsToLevel(-10, -50, -10)).toBe(1);
    expect(dbfsToLevel(0, -50, -10)).toBe(1);
  });
  it("maps the midpoint to 0.5", () => {
    expect(dbfsToLevel(-30, -50, -10)).toBeCloseTo(0.5, 6);
  });
  it("guards a degenerate window", () => {
    expect(dbfsToLevel(-20, -10, -10)).toBe(0);
    expect(dbfsToLevel(-20, -10, -50)).toBe(0);
  });
});

describe("smoothStep", () => {
  it("rises toward a higher target with the attack factor", () => {
    expect(smoothStep(0, 1, 0.5, 0.1)).toBeCloseTo(0.5, 6);
  });
  it("falls toward a lower target with the release factor", () => {
    expect(smoothStep(1, 0, 0.5, 0.1)).toBeCloseTo(0.9, 6);
  });
  it("converges to the target after repeated steps", () => {
    let v = 0;
    for (let i = 0; i < 200; i++) v = smoothStep(v, 1, 0.5, 0.1);
    expect(v).toBeCloseTo(1, 4);
  });
  it("is a no-op when already at the target", () => {
    expect(smoothStep(0.42, 0.42, 0.5, 0.1)).toBeCloseTo(0.42, 6);
  });
});
