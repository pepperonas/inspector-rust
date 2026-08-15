import { describe, it, expect } from "vitest";
import { DICT_WORDS } from "./pwgen-dict";
import { leetTransform } from "./pwgen";

/**
 * Structural invariant tests for the `pwgen` word list — the house pattern for
 * a pure data catalogue (cf. Rust's `faker/registry.rs` / `sec/registry.rs`):
 * a hand-edit that breaks an assumption the *generator* silently relies on
 * should fail a named test instead of shipping.
 *
 * Every assertion below is derived from reading the consumer (`pwgen.ts`), not
 * from taste. The load-bearing facts are:
 *   - `randomWord()` indexes the array unguarded → it must never be empty.
 *   - `capitalise(w)` upper-cases `w[0]` and leaves the rest alone, explicitly
 *     "since the dict is already lowercase" → any non-lowercase word silently
 *     breaks that contract.
 *   - `generateDict` pads with DIGITS, so the words themselves must be pure
 *     letters for `dict`/`leet` output to stay `[A-Za-z0-9]`.
 *   - `leetTransform` only maps LOWERCASE a/e/i/o.
 */

describe("DICT_WORDS — the generator's hard requirements", () => {
  it("is never empty (randomWord indexes it without a guard)", () => {
    // `DICT_WORDS[randInt(0)]` would be `undefined` and `capitalise` would
    // throw on `.length` — an empty list crashes the whole pwgen row.
    expect(DICT_WORDS.length).toBeGreaterThan(0);
  });

  it("contains no empty string (an empty word cannot advance the builder)", () => {
    // generateDict loops "while s.length < length" adding words; a zero-length
    // word contributes nothing and only burns one of the 50 attempts.
    expect(DICT_WORDS.filter((w) => w.length === 0)).toEqual([]);
  });

  it("every word is lowercase ASCII letters only", () => {
    // No digits (they'd be indistinguishable from the digit padding), no
    // punctuation/spaces (they'd break the documented `[A-Za-z0-9]` charset of
    // the dict + leet modes), no Umlauts (they'd survive leet unchanged and
    // are awkward to type on a non-German keyboard).
    const offenders = DICT_WORDS.filter((w) => !/^[a-z]+$/.test(w));
    expect(offenders).toEqual([]);
  });

  it("has no duplicates (a dupe silently doubles that word's odds)", () => {
    const seen = new Map<string, number>();
    for (const w of DICT_WORDS) seen.set(w, (seen.get(w) ?? 0) + 1);
    const dupes = [...seen].filter(([, n]) => n > 1).map(([w]) => w);
    expect(dupes).toEqual([]);
  });
});

describe("DICT_WORDS — strength + packing properties", () => {
  it("carries at least 256 words, so a dict word is worth ≥ 8 bits", () => {
    // The dict/leet modes' whole security argument is words × entropy. A future
    // trim must be a conscious decision, not a silent weakening.
    expect(DICT_WORDS.length).toBeGreaterThanOrEqual(256);
    expect(Math.log2(DICT_WORDS.length)).toBeGreaterThanOrEqual(8);
  });

  it("word lengths stay in a range the packer can actually use", () => {
    const lens = DICT_WORDS.map((w) => w.length);
    const min = Math.min(...lens);
    const max = Math.max(...lens);
    // pwgen.test.ts asserts a 24-char dict password has ≥ 2 capitals, i.e. two
    // words. That holds for ANY draw only while two of the LONGEST words still
    // fit in 24 chars — otherwise the packer could break after one word and
    // pad the rest with digits.
    expect(max * 2).toBeLessThanOrEqual(24);
    // And two words must at least be *possible* at the 12-char default length.
    expect(min * 2).toBeLessThanOrEqual(12);
    // Long enough not to be noise; 1–2 letter "words" would read as padding.
    expect(min).toBeGreaterThanOrEqual(3);
  });

  it("offers plenty of short words so a tight target is still word-based", () => {
    // generateDict breaks out of the word loop as soon as the next word would
    // overflow, then pads with digits. With too few short words a 12-char
    // password would be mostly digits.
    const short = DICT_WORDS.filter((w) => w.length <= 5);
    expect(short.length / DICT_WORDS.length).toBeGreaterThan(0.5);
  });
});

describe("DICT_WORDS — contract with the transforms that consume it", () => {
  it("capitalising any word yields exactly one uppercase letter", () => {
    // `capitalise` only upper-cases index 0 and does NOT lower-case the rest,
    // so a word with an interior capital would produce a two-capital token and
    // blur the word boundaries the whole dict mode is readable by.
    for (const w of DICT_WORDS) {
      const capped = w[0].toUpperCase() + w.slice(1);
      expect(capped.match(/[A-Z]/g)).toHaveLength(1);
      expect(capped.length).toBe(w.length);
    }
  });

  it("every word survives leet as pure alphanumerics of the same length", () => {
    // Guarantees the `leet` mode's documented charset + that no word maps to a
    // symbol (the deliberate "keep it readable" decision).
    for (const w of DICT_WORDS) {
      const leet = leetTransform(w);
      expect(leet).toMatch(/^[a-z0-9]+$/);
      expect(leet).toHaveLength(w.length);
    }
  });

  it("leet leaves at least one letter in every word (never an all-digit token)", () => {
    // A word made only of a/e/i/o (e.g. a hypothetical "aioe") would leet into
    // pure digits and be indistinguishable from the digit padding.
    const allDigits = DICT_WORDS.filter((w) => /^[0-9]+$/.test(leetTransform(w)));
    expect(allDigits).toEqual([]);
  });
});
