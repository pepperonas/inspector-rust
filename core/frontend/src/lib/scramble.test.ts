import { describe, it, expect } from "vitest";
import { scrambleFrame, digitCount, LOCK_START } from "./scramble";

// Deterministic rng → every scrambled digit becomes the same known value.
const fixed = (v: number) => () => v;

describe("digitCount", () => {
  it("counts only 0-9 characters", () => {
    expect(digitCount("1234.5")).toBe(5);
    expect(digitCount("0xff")).toBe(1); // only the leading 0
    expect(digitCount("5 km")).toBe(1);
    expect(digitCount("hello")).toBe(0);
  });
});

describe("scrambleFrame", () => {
  it("at progress 1 returns the exact target", () => {
    expect(scrambleFrame("1234.5", 1)).toBe("1234.5");
    expect(scrambleFrame("0xff", 1)).toBe("0xff");
    expect(scrambleFrame("-42", 1)).toBe("-42");
  });

  it("clamps out-of-range progress", () => {
    expect(scrambleFrame("123", 5)).toBe("123");
    expect(scrambleFrame("123", -1, fixed(0))).toBe("000");
  });

  it("at progress 0 scrambles every digit, preserving non-digits", () => {
    // fixed rng → 0.55*10 = 5.5 → floor → 5
    expect(scrambleFrame("12.3 km", 0, fixed(0.55))).toBe("55.5 km");
    // separators / signs untouched
    expect(scrambleFrame("-1,200", 0, fixed(0.0))).toBe("-0,000");
  });

  it("locks left→right: leftmost digit settles before the rightmost", () => {
    // 4 digits → lock thresholds 0.35, 0.57, 0.78, 1.0
    const target = "1234";
    const r = fixed(0.9); // any unlocked digit becomes 9
    // progress 0.4: only the leftmost (0.35) is locked
    expect(scrambleFrame(target, 0.4, () => 0.9)).toBe("1999");
    // progress 0.8: first three locked, last still spinning
    expect(scrambleFrame(target, 0.8, () => 0.9)).toBe("1239");
    void r;
  });

  it("a single digit only locks at the very end", () => {
    expect(scrambleFrame("7", 0.99, fixed(0.2))).toBe("2");
    expect(scrambleFrame("7", 1)).toBe("7");
  });

  it("never emits a non-digit for a digit position (rng edge = 1)", () => {
    // rng returning 1 must still clamp to 9, not produce '10'
    expect(scrambleFrame("5", 0, fixed(1))).toBe("9");
  });

  it("strings with no digits are returned unchanged at any progress", () => {
    expect(scrambleFrame("NaN", 0)).toBe("NaN");
    expect(scrambleFrame("Infinity", 0.5)).toBe("Infinity");
  });

  it("LOCK_START is the leftmost threshold", () => {
    // with 2 digits, the left locks at LOCK_START, the right at 1
    expect(scrambleFrame("12", LOCK_START - 0.01, () => 0.3)).toBe("33");
    expect(scrambleFrame("12", LOCK_START, () => 0.3)).toBe("13");
  });
});
