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

describe("leetTransform — mapping invariants", () => {
  it("maps exactly the four lowercase vowels a/e/i/o", () => {
    expect(leetTransform("aeio")).toBe("4310");
    expect(leetTransform("u")).toBe("u"); // u is deliberately NOT mapped
  });

  it("leaves uppercase vowels, consonants and non-letters untouched", () => {
    expect(leetTransform("AEIOU")).toBe("AEIOU");
    expect(leetTransform("xyz-123_!")).toBe("xyz-123_!");
    expect(leetTransform("")).toBe("");
  });

  it("passes Umlauts through unchanged", () => {
    expect(leetTransform("über")).toBe("üb3r"); // only the e maps
    expect(leetTransform("Größe")).toBe("Größ3");
  });

  it("is idempotent (output contains no mappable vowels)", () => {
    for (const s of ["Sunrise", "aeio", "über", "Apple Pie"]) {
      const once = leetTransform(s);
      expect(leetTransform(once)).toBe(once);
    }
  });

  it("preserves string length 1:1", () => {
    for (const s of ["", "a", "Sunrise", "ökonomie"]) {
      expect(leetTransform(s).length).toBe(s.length);
    }
  });
});

describe("generatePassword — mode charsets", () => {
  it("all mode only draws from the documented charset", () => {
    const allowed = /^[A-Za-z0-9!@#$%^&*()_+\-=[\]{};:,.?]+$/;
    for (let i = 0; i < 10; i++) {
      expect(generatePassword("all", 64)).toMatch(allowed);
    }
  });

  it("leet mode never contains a lowercase a/e/i/o (all substituted)", () => {
    for (let i = 0; i < 10; i++) {
      expect(generatePassword("leet", 32)).not.toMatch(/[aeio]/);
    }
  });

  it("leet mode stays alphanumeric (readable — no symbols introduced)", () => {
    for (let i = 0; i < 5; i++) {
      expect(generatePassword("leet", 24)).toMatch(/^[A-Za-z0-9]+$/);
    }
  });

  it("floors a fractional requested length", () => {
    expect(generatePassword("all", 12.9).length).toBe(12);
    expect(generatePassword("dict", 16.5).length).toBe(16);
  });

  it("length 1 works in every mode", () => {
    for (const mode of ["all", "alnum", "dict", "leet"] as const) {
      expect(generatePassword(mode, 1).length).toBe(1);
    }
  });
});

describe("generatePassword — without Web Crypto (fallback path)", () => {
  it("still honours length + charset when crypto is unavailable", () => {
    const original = globalThis.crypto;
    Object.defineProperty(globalThis, "crypto", { value: undefined, configurable: true });
    try {
      const pw = generatePassword("alnum", 16);
      expect(pw).toHaveLength(16);
      expect(pw).toMatch(/^[A-Za-z0-9]+$/);
      const dict = generatePassword("dict", 20);
      expect(dict).toHaveLength(20);
    } finally {
      Object.defineProperty(globalThis, "crypto", { value: original, configurable: true });
    }
  });
});
