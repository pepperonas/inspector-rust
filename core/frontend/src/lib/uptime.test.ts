import { describe, it, expect } from "vitest";
import {
  uptimeBreakdown,
  odometerValue,
  integerDigitCount,
  odometerPowers,
} from "./uptime";

describe("uptime helpers", () => {
  it("uptimeBreakdown splits seconds into d/h/m/s", () => {
    expect(uptimeBreakdown(0)).toEqual({ days: 0, hours: 0, minutes: 0, seconds: 0 });
    // 1d 1h 1m 1s = 86400+3600+60+1
    expect(uptimeBreakdown(90061)).toEqual({ days: 1, hours: 1, minutes: 1, seconds: 1 });
    expect(uptimeBreakdown(59)).toEqual({ days: 0, hours: 0, minutes: 0, seconds: 59 });
    expect(uptimeBreakdown(86399)).toEqual({ days: 0, hours: 23, minutes: 59, seconds: 59 });
    // Negative / fractional clamps + floors.
    expect(uptimeBreakdown(-5)).toEqual({ days: 0, hours: 0, minutes: 0, seconds: 0 });
    expect(uptimeBreakdown(61.9)).toEqual({ days: 0, hours: 0, minutes: 1, seconds: 1 });
  });

  it("odometerValue gives the continuous digit at a place", () => {
    // 123.456 → units digit 3 (+ fraction)
    expect(odometerValue(123.456, 0)).toBeCloseTo(3.456, 5);
    // tens place: 12.3456 → 2.3456
    expect(odometerValue(123.456, 1)).toBeCloseTo(2.3456, 5);
    // first decimal place: 1234.56 → 4.56
    expect(odometerValue(123.456, -1)).toBeCloseTo(4.56, 4);
    // always within [0, 10)
    for (const p of [0, 1, 2, -1, -3, -6]) {
      const v = odometerValue(987654.321, p);
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThan(10);
    }
  });

  it("integerDigitCount counts base-10 digits", () => {
    expect(integerDigitCount(0)).toBe(1);
    expect(integerDigitCount(5)).toBe(1);
    expect(integerDigitCount(999)).toBe(3);
    expect(integerDigitCount(1000)).toBe(4);
    expect(integerDigitCount(86400)).toBe(5);
  });

  it("odometerPowers lists high→low places", () => {
    expect(odometerPowers(2, 3)).toEqual([1, 0, -1, -2, -3]);
    expect(odometerPowers(1, 6)).toEqual([0, -1, -2, -3, -4, -5, -6]);
  });
});

describe("odometerValue — negative input", () => {
  it("wraps a negative modulo into the positive digit range", () => {
    expect(odometerValue(-1, 0)).toBe(9);
    expect(odometerValue(-12, 0)).toBe(8); // -12 % 10 = -2 → 8
  });
});
