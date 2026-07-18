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

describe("uptimeBreakdown — invariants", () => {
  it("reassembles to the floored total for arbitrary inputs", () => {
    for (const t of [0, 1, 59, 60, 3599, 3600, 86399, 86400, 123456.78, 9999999]) {
      const b = uptimeBreakdown(t);
      expect(b.days * 86400 + b.hours * 3600 + b.minutes * 60 + b.seconds).toBe(
        Math.max(0, Math.floor(t)),
      );
    }
  });

  it("keeps every field inside its unit range", () => {
    for (const t of [86399, 86401, 123456789, 55.5]) {
      const b = uptimeBreakdown(t);
      expect(b.hours).toBeGreaterThanOrEqual(0);
      expect(b.hours).toBeLessThan(24);
      expect(b.minutes).toBeGreaterThanOrEqual(0);
      expect(b.minutes).toBeLessThan(60);
      expect(b.seconds).toBeGreaterThanOrEqual(0);
      expect(b.seconds).toBeLessThan(60);
    }
  });
});

describe("odometerValue — sub-second places", () => {
  it("scales fractional seconds into whole digit values", () => {
    // 0.5 s at the first decimal place (power -1) → digit value 5.
    expect(odometerValue(0.5, -1)).toBeCloseTo(5, 6);
    // Microsecond place of 1.000002 s → digit 2.
    expect(odometerValue(1.000002, -6)).toBeCloseTo(2, 3);
  });

  it("a place above all digits reads as (fractional) zero", () => {
    expect(odometerValue(5, 3)).toBeCloseTo(0.005, 9);
  });
});

describe("integerDigitCount — edge inputs", () => {
  it("clamps sub-1, negative and non-finite input to 1 digit", () => {
    expect(integerDigitCount(0.5)).toBe(1);
    expect(integerDigitCount(-42)).toBe(1);
    expect(integerDigitCount(NaN)).toBe(1);
    expect(integerDigitCount(Infinity)).toBe(1);
  });

  it("switches exactly at each power of ten", () => {
    expect(integerDigitCount(9)).toBe(1);
    expect(integerDigitCount(10)).toBe(2);
    expect(integerDigitCount(99)).toBe(2);
    expect(integerDigitCount(100)).toBe(3);
    expect(integerDigitCount(1e6)).toBe(7);
  });

  it("counts only the integer part of a fractional number", () => {
    expect(integerDigitCount(1.999)).toBe(1);
    expect(integerDigitCount(12.34)).toBe(2);
  });
});

describe("odometerPowers — degenerate shapes", () => {
  it("handles zero-length halves", () => {
    expect(odometerPowers(0, 0)).toEqual([]);
    expect(odometerPowers(3, 0)).toEqual([2, 1, 0]);
    expect(odometerPowers(0, 2)).toEqual([-1, -2]);
  });

  it("length is always intDigits + fracDigits, strictly descending", () => {
    const p = odometerPowers(5, 6);
    expect(p.length).toBe(11);
    for (let i = 1; i < p.length; i++) expect(p[i]).toBe(p[i - 1] - 1);
  });
});
