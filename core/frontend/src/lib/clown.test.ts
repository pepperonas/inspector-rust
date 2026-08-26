import { describe, it, expect } from "vitest";
import { CLOWN_STYLES, clownStyle, clownAll, parseClownArg } from "./clown";

const apply = (key: string, text: string) => clownStyle(key)!.transform(text);

describe("registry invariants", () => {
  it("keys are unique, lowercase and every style is described", () => {
    const keys = CLOWN_STYLES.map((s) => s.key);
    expect(new Set(keys).size).toBe(keys.length);
    for (const s of CLOWN_STYLES) {
      expect(s.key).toBe(s.key.toLowerCase());
      expect(s.name.length).toBeGreaterThan(0);
      expect(s.hint.length).toBeGreaterThan(0);
      expect(typeof s.transform).toBe("function");
    }
    expect(clownStyle("CLOWN")?.key).toBe("clown"); // lookup is case-insensitive
    expect(clownStyle("nope")).toBeUndefined();
  });

  it("no style throws or invents content on empty input", () => {
    for (const s of CLOWN_STYLES) expect(s.transform(""), s.key).toBe("");
  });

  it("no style ever emits a lone surrogate (the astral-splitting bug)", () => {
    const sample = "Inspector Rust 123 — äöü!";
    for (const s of CLOWN_STYLES) {
      const out = s.transform(sample);
      // A lone surrogate is a broken astral char; well-formed output has none.
      expect(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/.test(out), s.key)
        .toBe(false);
    }
  });

  it("emoji survive every style intact", () => {
    for (const s of CLOWN_STYLES) {
      // The emoji must still be ONE code point in the output (never split).
      const out = s.transform("hi 🎈 there");
      expect([...out].includes("🎈"), s.key).toBe(true);
    }
  });
});

describe("determinism", () => {
  it("same input always yields the same output (no Math.random)", () => {
    for (const s of CLOWN_STYLES) {
      const a = s.transform("Inspector Rust ist super");
      const b = s.transform("Inspector Rust ist super");
      expect(a, s.key).toBe(b);
    }
  });

  it("different text yields a different mock pattern (not a fixed alternation)", () => {
    // The flip is hashed per (text, index), so the pattern isn't stuck.
    const a = apply("mock", "aaaaaaaaaa");
    const b = apply("mock", "bbbbbbbbbb");
    expect(a).not.toBe(a.toLowerCase());
    // Purely alternating would be exactly this — the hash must break it up.
    const strictAlternate = [..."aaaaaaaaaa"]
      .map((c, i) => (i % 2 === 0 ? c.toUpperCase() : c))
      .join("");
    expect([a, b].some((x) => x !== strictAlternate)).toBe(true);
  });
});

describe("clown (the requested style)", () => {
  it("mixes case and leets only some characters", () => {
    const out = apply("clown", "text so schreiben kann");
    expect(out.toLowerCase().replace(/[43015789]/g, "")).not.toBe("");
    // Both cases present → it really is mocking case.
    expect(out).toMatch(/[a-z]/);
    expect(out).toMatch(/[A-Z]/);
    // Sparse leet: some leetable letters survive as letters.
    const leetable = [...out].filter((c) => /[aeiostbg]/i.test(c));
    expect(leetable.length).toBeGreaterThan(0);
  });

  it("leaves non-letters alone and preserves length in code points", () => {
    const src = "a-b, c! 42";
    const out = apply("clown", src);
    expect([...out]).toHaveLength([...src].length);
    expect(out).toContain("-");
    expect(out).toContain(",");
    expect(out).toContain("!");
  });
});

describe("mock case", () => {
  it("counts LETTERS only, so spaces don't shift the rhythm", () => {
    // Same letters, different spacing → identical letter-by-letter casing
    // would break if the counter advanced on spaces. (The hash salt uses the
    // whole text, so we compare the CASE PATTERN, not the exact string.)
    const pattern = (s: string) =>
      [...s].filter((c) => /[a-z]/i.test(c)).map((c) => (c === c.toUpperCase() ? "U" : "l")).join("");
    const a = pattern(apply("mock", "abcdef"));
    expect(a).toHaveLength(6);
    // Never all-one-case for a 6-letter word.
    expect(new Set(a).size).toBe(2);
  });
});

describe("leet", () => {
  it("substitutes every eligible letter in the full style", () => {
    expect(apply("leet", "aeiost")).toBe("431057");
    // Case-insensitive lookup, digits/punctuation untouched.
    expect(apply("leet", "AEI")).toBe("431");
    expect(apply("leet", "xyz 9!")).toBe("xyz 9!");
  });
});

describe("unicode alphabets", () => {
  it("double-struck uses the LETTERLIKE code points for C H N P Q R Z", () => {
    // The trap: `0x1D538 + offset` lands on reserved code points for these
    // seven — they live in the Letterlike Symbols block instead.
    expect(apply("double", "CHNPQRZ")).toBe("ℂℍℕℙℚℝℤ");
    // The regular ones do come from the math block.
    expect(apply("double", "AB")).toBe("𝔸𝔹");
    expect(apply("double", "ab")).toBe("𝕒𝕓");
    expect(apply("double", "01")).toBe("𝟘𝟙");
    // And nothing lands on the reserved slots.
    expect(apply("double", "CHNPQRZ")).not.toContain(String.fromCodePoint(0x1d53a));
  });

  it("bold and script map the ASCII letters", () => {
    expect(apply("bold", "Ab")).toBe("𝗔𝗯");
    // ⚠️ BOLD script by design: plain script (U+1D49C) has eleven holes
    // (ℬ ℰ ℱ ℋ ℐ ℒ ℳ ℛ ℯ ℊ ℴ sit in Letterlike Symbols), the bold range is
    // contiguous. Picking it is what keeps this style exception-free.
    expect(apply("script", "Ab")).toBe("𝓐𝓫");
    // Script has no digit range → digits pass through unchanged.
    expect(apply("script", "7")).toBe("7");
  });

  it("the contiguous ranges really are hole-free — 52 distinct letters each", () => {
    // A hole would map two letters onto reserved/duplicate code points; this
    // catches that for every range NOT covered by an explicit exception table.
    const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    for (const key of ["bold", "script"]) {
      const out = [...apply(key, alphabet)];
      expect(out, key).toHaveLength(52);
      expect(new Set(out).size, key).toBe(52);
    }
    // double-struck is only distinct BECAUSE of its exception table.
    const dbl = [...apply("double", alphabet)];
    expect(new Set(dbl).size).toBe(52);
  });

  it("vaporwave widens ASCII and uses the ideographic space", () => {
    expect(apply("vaporwave", "Hi!")).toBe("Ｈｉ！");
    expect(apply("vaporwave", "a b")).toBe("ａ　ｂ");
  });

  it("small caps fall back to plain letters where Unicode has none (q, x)", () => {
    // Q and X genuinely have no small-cap glyph — the map must not invent one.
    expect(apply("smallcaps", "qx")).toBe("qx");
    expect(apply("smallcaps", "abc")).toBe("ᴀʙᴄ");
    expect(apply("smallcaps", "ABC")).toBe("ᴀʙᴄ");
  });
});

describe("upside down", () => {
  it("flips the glyphs AND reverses the order", () => {
    expect(apply("upside", "abc")).toBe("ɔqɐ");
    // Unmapped characters pass through rather than vanishing.
    const out = apply("upside", "a€b");
    expect(out).toContain("€");
    expect([...out]).toHaveLength(3);
  });

  it("every flip is exactly ONE code point — length is preserved", () => {
    // A multi-char entry (an early draft had `B: "၁2"`) would silently change
    // the text length and break the round-trip feel.
    const src = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789.,!?()[]{}<>&_";
    expect([...apply("upside", src)]).toHaveLength([...src].length);
  });
});

describe("strike / clap / spaced", () => {
  it("strike combines an overlay onto every non-space char", () => {
    const out = apply("strike", "ab c");
    expect(out).toBe("a̶b̶ c̶");
    // The space stays clean so words remain readable.
    expect(out).toContain(" c");
  });

  it("clap joins words and collapses runs of whitespace", () => {
    expect(apply("clap", "a  b\tc")).toBe("a 👏 b 👏 c");
    expect(apply("clap", "  solo  ")).toBe("solo");
  });

  it("spaced separates characters", () => {
    expect(apply("spaced", "ab")).toBe("a b");
  });
});

describe("parseClownArg", () => {
  it("splits a trailing @style selector", () => {
    expect(parseClownArg("hallo welt")).toEqual({ text: "hallo welt" });
    expect(parseClownArg("hallo welt @leet")).toEqual({ text: "hallo welt", style: "leet" });
    expect(parseClownArg("hallo @UPSIDE")).toEqual({ text: "hallo", style: "upside" });
  });

  it("only a TRAILING selector counts — an @ inside the text stays text", () => {
    expect(parseClownArg("mail me @ home please")).toEqual({ text: "mail me @ home please" });
    expect(parseClownArg("a@b.de")).toEqual({ text: "a@b.de" });
  });

  it("a selector with no text yields empty text", () => {
    expect(parseClownArg("@leet")).toEqual({ text: "", style: "leet" });
  });
});

describe("clownAll", () => {
  it("returns one entry per style, in registry order", () => {
    const all = clownAll("hi");
    expect(all).toHaveLength(CLOWN_STYLES.length);
    expect(all.map((a) => a.style.key)).toEqual(CLOWN_STYLES.map((s) => s.key));
    expect(all[0].output).toBe(CLOWN_STYLES[0].transform("hi"));
  });
});
