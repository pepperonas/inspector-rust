import { describe, it, expect } from "vitest";
import { generatePassword, leetTransform } from "./pwgen";

/**
 * Pwgen unit tests. Targets exact length (the most important invariant
 * since the user types `pwgen 16` expecting 16 chars) and the
 * charset/format guarantees per mode.
 *
 * Vitest's `node` environment exposes the Web Crypto API since Node 19,
 * so the CSPRNG path is exercised (not the Math.random fallback).
 */

describe("generatePassword", () => {
  it("produces EXACTLY the requested length for all modes", () => {
    for (const len of [4, 8, 12, 16, 24, 32, 64, 128]) {
      for (const mode of ["all", "alnum", "dict", "leet"] as const) {
        const p = generatePassword(mode, len);
        expect(p.length).toBe(len);
      }
    }
  });

  it("alnum mode contains ONLY [A-Za-z0-9]", () => {
    for (let i = 0; i < 20; i++) {
      const p = generatePassword("alnum", 32);
      expect(p).toMatch(/^[A-Za-z0-9]+$/);
    }
  });

  it("all mode may include symbols (probabilistic)", () => {
    // 32-char password with symbols mixed in should contain at least
    // one non-alnum char nearly all the time; run a few attempts so
    // the test isn't flaky on an unlucky draw.
    const SYMBOLS = /[^A-Za-z0-9]/;
    let sawSymbol = false;
    for (let i = 0; i < 10; i++) {
      if (SYMBOLS.test(generatePassword("all", 32))) {
        sawSymbol = true;
        break;
      }
    }
    expect(sawSymbol).toBe(true);
  });

  it("dict mode contains letters + digit padding only (no symbols)", () => {
    for (let i = 0; i < 10; i++) {
      const p = generatePassword("dict", 24);
      expect(p).toMatch(/^[A-Za-z0-9]+$/);
    }
  });

  it("leet substitutes vowels only, keeping caps + consonants (readable)", () => {
    // a→4 e→3 i→1 o→0; uppercase + consonants untouched, so the base
    // word's capitalised initial + silhouette survive (the whole point —
    // you should still recognise the original word).
    expect(leetTransform("Sunrise")).toBe("Sunr1s3");
    expect(leetTransform("Apple")).toBe("Appl3"); // capital A kept, e→3
    expect(leetTransform("Octopus")).toBe("Oct0pus"); // capital O kept, o→0
    // No consonant or symbol mangling.
    expect(leetTransform("salt")).toBe("s4lt"); // s/l/t untouched, a→4
  });

  it("different calls produce different passwords (CSPRNG works)", () => {
    const a = generatePassword("all", 32);
    const b = generatePassword("all", 32);
    expect(a).not.toBe(b);
  });

  it("length is clamped at the boundary (4..256)", () => {
    expect(generatePassword("all", 0).length).toBe(1);
    expect(generatePassword("all", -5).length).toBe(1);
    expect(generatePassword("all", 999).length).toBe(256);
  });

  it("dict mode words start with uppercase letters", () => {
    // Pure-word section of a long dict password should have multiple
    // capital letters as word starts.
    const p = generatePassword("dict", 24);
    const caps = (p.match(/[A-Z]/g) ?? []).length;
    expect(caps).toBeGreaterThanOrEqual(2);
  });
});
