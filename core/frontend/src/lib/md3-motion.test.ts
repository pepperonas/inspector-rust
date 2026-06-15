import { describe, it, expect } from "vitest";
import {
  MD3_SPRING,
  MD3_EASING,
  MD3_DURATION,
  simulateSpring,
  popInKeyframes,
} from "./md3-motion";

describe("MD3 token tables", () => {
  it("carries the spec spring values", () => {
    expect(MD3_SPRING.spatial.expressive.fast).toEqual({ stiffness: 800, dampingRatio: 0.6 });
    expect(MD3_SPRING.spatial.standard.default).toEqual({ stiffness: 700, dampingRatio: 0.9 });
    // effects springs are critically damped in both schemes
    expect(MD3_SPRING.effects.expressive.default.dampingRatio).toBe(1);
    expect(MD3_SPRING.effects.standard.fast.dampingRatio).toBe(1);
  });
  it("has the canonical easing curves + duration scale", () => {
    expect(MD3_EASING.emphasized).toBe("cubic-bezier(0.2, 0, 0, 1)");
    expect(MD3_EASING.emphasizedDecelerate).toBe("cubic-bezier(0.05, 0.7, 0.1, 1)");
    expect(MD3_DURATION.medium2).toBe(300);
    expect(MD3_DURATION.short4).toBe(200);
  });
});

describe("simulateSpring", () => {
  it("starts at 0, ends pinned at 1, finite positive duration", () => {
    const { samples, durationMs } = simulateSpring(380, 0.8);
    expect(samples[0]).toBe(0);
    expect(samples[samples.length - 1]).toBe(1);
    expect(durationMs).toBeGreaterThan(0);
    expect(Number.isFinite(durationMs)).toBe(true);
    expect(samples.every((s) => Number.isFinite(s))).toBe(true);
  });

  it("underdamped (expressive spatial) overshoots past 1", () => {
    const { samples } = simulateSpring(800, 0.6); // ζ=0.6 → bouncy
    expect(Math.max(...samples)).toBeGreaterThan(1.02);
  });

  it("critically damped (effects) never overshoots", () => {
    const { samples } = simulateSpring(1600, 1);
    // allow a hair over 1 only at the pinned endpoint
    expect(Math.max(...samples.slice(0, -1))).toBeLessThanOrEqual(1.0001);
  });

  it("stiffer springs settle faster", () => {
    const fast = simulateSpring(1400, 0.9).durationMs;
    const slow = simulateSpring(300, 0.9).durationMs;
    expect(fast).toBeLessThan(slow);
  });

  it("honours the requested sample count", () => {
    expect(simulateSpring(700, 0.9, { sampleCount: 24 }).samples).toHaveLength(24);
  });

  it("is monotonic non-decreasing for a critically damped rise", () => {
    const { samples } = simulateSpring(800, 1);
    for (let i = 1; i < samples.length - 1; i++) {
      expect(samples[i]).toBeGreaterThanOrEqual(samples[i - 1] - 1e-9);
    }
  });
});

describe("popInKeyframes", () => {
  it("produces ordered offsets 0→1, ending at the identity transform", () => {
    const { keyframes, durationMs } = popInKeyframes(MD3_SPRING.spatial.expressive.fast);
    expect(keyframes[0].offset).toBe(0);
    expect(keyframes[keyframes.length - 1].offset).toBe(1);
    for (let i = 1; i < keyframes.length; i++) {
      expect(keyframes[i].offset as number).toBeGreaterThan(keyframes[i - 1].offset as number);
    }
    const last = keyframes[keyframes.length - 1];
    expect(last.opacity).toBe(1);
    expect(last.transform).toBe("translateY(0.000px) scale(1.0000)");
    expect(durationMs).toBeGreaterThan(0);
  });

  it("starts faded out and below the resting scale", () => {
    const { keyframes } = popInKeyframes(MD3_SPRING.spatial.expressive.fast, { fromScale: 0.9 });
    expect(keyframes[0].opacity).toBe(0);
    expect(keyframes[0].transform).toContain("scale(0.9000)");
  });
});
