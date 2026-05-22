import { describe, it, expect } from "vitest";
import { TOP_OPENERS, hashString, pickOpener } from "./openers";

describe("TOP_OPENERS data set", () => {
  it("contains exactly the curated top 100", () => {
    expect(TOP_OPENERS.length).toBe(100);
  });
  it("has no empty strings (data-integrity guard)", () => {
    for (const o of TOP_OPENERS) {
      expect(o.length).toBeGreaterThan(0);
    }
  });
  it("every entry is unique (no accidental dupes from the seed export)", () => {
    expect(new Set(TOP_OPENERS).size).toBe(TOP_OPENERS.length);
  });
});

describe("hashString", () => {
  it("returns a non-negative 32-bit integer", () => {
    const h = hashString("hello");
    expect(h).toBeGreaterThanOrEqual(0);
    expect(h).toBeLessThan(2 ** 32);
    expect(Number.isInteger(h)).toBe(true);
  });
  it("is deterministic — same input, same output", () => {
    expect(hashString("opener")).toBe(hashString("opener"));
    expect(hashString("opener xyz")).toBe(hashString("opener xyz"));
  });
  it("distinguishes distinct inputs", () => {
    expect(hashString("opener")).not.toBe(hashString("openers"));
    expect(hashString("a")).not.toBe(hashString("b"));
  });
  it("handles empty + Unicode inputs without throwing", () => {
    expect(typeof hashString("")).toBe("number");
    expect(typeof hashString("über 🦊")).toBe("number");
  });
});

describe("pickOpener", () => {
  it("always returns a string from the TOP_OPENERS set", () => {
    const set = new Set(TOP_OPENERS);
    for (const seed of ["opener", "opener ", "opener xyz", "Opener", "OPENER 123"]) {
      const picked = pickOpener(seed);
      expect(picked).not.toBeNull();
      expect(set.has(picked!)).toBe(true);
    }
  });
  it("is deterministic per seed — pinning the React render-loop", () => {
    // The same query rendered 60×/sec must show the same opener;
    // otherwise the user sees a flicker of different lines.
    expect(pickOpener("opener")).toBe(pickOpener("opener"));
    expect(pickOpener("opener xyz")).toBe(pickOpener("opener xyz"));
  });
  it("changes between distinct seeds — each keystroke re-rolls", () => {
    // Most pairs should differ. Test a handful — picks could collide,
    // but it'd be deeply surprising for the entire batch to share one
    // index. If this ever flakes, audit the hash distribution.
    const samples = ["opener", "opener ", "opener x", "opener xy", "opener xyz", "opener a"];
    const picks = samples.map((s) => pickOpener(s));
    expect(new Set(picks).size).toBeGreaterThan(1);
  });
});
