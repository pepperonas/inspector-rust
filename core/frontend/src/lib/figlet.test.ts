import { describe, it, expect } from "vitest";
import { parseCommand } from "./commands";
import {
  parseFigletCommand,
  fuzzyFont,
  galleryFonts,
  optsFromDefaults,
  type FigletFontMeta,
  type FigletDefaults,
} from "./figlet";

const FONTS: FigletFontMeta[] = [
  { name: "standard", category: "standard", popular: true, pinned: false },
  { name: "slant", category: "slanted", popular: true, pinned: false },
  { name: "small", category: "small", popular: true, pinned: false },
  { name: "banner", category: "banner", popular: true, pinned: false },
  { name: "3d-ascii", category: "slanted", popular: true, pinned: false },
  { name: "1943____", category: "other", popular: false, pinned: false },
  { name: "graffiti", category: "decorative", popular: true, pinned: false },
];

describe("parseFigletCommand — token classification (order-agnostic)", () => {
  it("treats plain tokens as the banner text", () => {
    expect(parseFigletCommand("Hello World").text).toBe("Hello World");
  });
  it("extracts @font anywhere in the line", () => {
    expect(parseFigletCommand("Hi @slant").fontQuery).toBe("slant");
    expect(parseFigletCommand("@slant Hi there").fontQuery).toBe("slant");
    expect(parseFigletCommand("one @big two").text).toBe("one two");
  });
  it("parses --flags regardless of position", () => {
    const p = parseFigletCommand("--center Hello --width=40 @slant");
    expect(p.text).toBe("Hello");
    expect(p.fontQuery).toBe("slant");
    expect(p.opts.align).toBe("center");
    expect(p.opts.width).toBe(40);
  });
  it("supports right/left/box/comment/trim flags", () => {
    expect(parseFigletCommand("x --right").opts.align).toBe("right");
    expect(parseFigletCommand("x --box").opts.boxed).toBe(true);
    expect(parseFigletCommand("x --no-trim").opts.trim).toBe(false);
    expect(parseFigletCommand("x --comment=slashes").opts.comment).toBe("slashes");
    expect(parseFigletCommand("x --comment=bogus").opts.comment).toBeUndefined();
  });
  it("--left selects left alignment, and the LAST align flag wins", () => {
    expect(parseFigletCommand("x --left").opts.align).toBe("left");
    // Contradicting flags: the user's most recent word is their intent.
    expect(parseFigletCommand("x --center --left").opts.align).toBe("left");
    expect(parseFigletCommand("x --left --right").opts.align).toBe("right");
  });
  it("keeps an UNKNOWN --token as literal text", () => {
    expect(parseFigletCommand("--> go").text).toBe("--> go");
    expect(parseFigletCommand("--widht=5 hi").text).toBe("--widht=5 hi");
  });
  it("caps width and rejects negatives", () => {
    expect(parseFigletCommand("x --width=99999").opts.width).toBe(400);
    expect(parseFigletCommand("x --width=-5").opts.width).toBeUndefined();
  });
  it("free text can contain spaces; @font/flags removed", () => {
    const p = parseFigletCommand("Guten  Morgen @doh --width=60");
    expect(p.text).toBe("Guten Morgen");
    expect(p.fontQuery).toBe("doh");
    expect(p.opts.width).toBe(60);
  });
  it("empty arg → empty text, no font", () => {
    const p = parseFigletCommand("");
    expect(p.text).toBe("");
    expect(p.fontQuery).toBeNull();
  });
});

describe("the command gate does not fire mid-history-search", () => {
  it("only `figlet`/`banner`/`ascii` as the first token trigger the command", () => {
    expect(parseCommand("figlet Hi")?.spec.kind).toBe("figlet");
    expect(parseCommand("banner Hi")?.spec.kind).toBe("figlet");
    expect(parseCommand("ascii Hi")?.spec.kind).toBe("figlet");
    // A history search that merely contains the word must NOT parse as figlet.
    expect(parseCommand("my figlet notes")).toBeNull();
    expect(parseCommand("figletx")).toBeNull();
    expect(parseCommand("asciiart")).toBeNull();
  });
});

describe("fuzzyFont", () => {
  it("resolves exact > prefix > subsequence", () => {
    expect(fuzzyFont("slant", FONTS)).toBe("slant");
    expect(fuzzyFont("sla", FONTS)).toBe("slant");
    expect(fuzzyFont("graf", FONTS)).toBe("graffiti");
  });
  it("returns null for no match", () => {
    expect(fuzzyFont("zzzzz", FONTS)).toBeNull();
    expect(fuzzyFont("", FONTS)).toBeNull();
  });
});

describe("galleryFonts", () => {
  it("with no query: pinned → popular → rest, alphabetical", () => {
    const pinned = FONTS.map((f) => (f.name === "banner" ? { ...f, pinned: true } : f));
    const ordered = galleryFonts(null, pinned).map((f) => f.name);
    expect(ordered[0]).toBe("banner"); // pinned first
    // the non-popular "1943____" comes last
    expect(ordered[ordered.length - 1]).toBe("1943____");
  });
  it("with a query: fuzzy matches only, best first", () => {
    const names = galleryFonts("s", FONTS).map((f) => f.name);
    expect(names).toContain("slant");
    expect(names).toContain("small");
    expect(names).not.toContain("banner"); // no 's' prefix/subseq anchor
  });
});

describe("optsFromDefaults", () => {
  it("projects the render options", () => {
    const d: FigletDefaults = {
      font: "slant", width: 100, align: "center", trim: false,
      comment: "hash", boxed: true, pinned: [], save_history: true,
    };
    expect(optsFromDefaults(d)).toEqual({
      width: 100, align: "center", trim: false, comment: "hash", boxed: true,
    });
  });
});

describe("parseFigletCommand — flag/font edge cases", () => {
  it("last @font wins when several are given", () => {
    const p = parseFigletCommand("@slant hi @doh");
    expect(p.fontQuery).toBe("doh");
    expect(p.text).toBe("hi");
  });
  it("a lone @ is literal text, not an empty font query", () => {
    const p = parseFigletCommand("a @ b");
    expect(p.fontQuery).toBeNull();
    expect(p.text).toBe("a @ b");
  });
  it("later conflicting flags override earlier ones", () => {
    expect(parseFigletCommand("x --center --right").opts.align).toBe("right");
    expect(parseFigletCommand("x --box --no-box").opts.boxed).toBe(false);
    expect(parseFigletCommand("x --no-trim --trim").opts.trim).toBe(true);
  });
  it("--width without a value or non-numeric is ignored", () => {
    expect(parseFigletCommand("x --width").opts.width).toBeUndefined();
    expect(parseFigletCommand("x --width=abc").opts.width).toBeUndefined();
  });
  it("--width=0 disables wrapping (0 is a valid value)", () => {
    expect(parseFigletCommand("x --width=0").opts.width).toBe(0);
  });
  it("--comment without =value is ignored (empty is not a style)", () => {
    expect(parseFigletCommand("x --comment").opts.comment).toBeUndefined();
  });
  it("every valid comment style parses", () => {
    for (const c of ["none", "slashes", "hash", "block", "html"] as const) {
      expect(parseFigletCommand(`x --comment=${c}`).opts.comment).toBe(c);
    }
  });
  it("umlauts and emoji stay verbatim in the banner text", () => {
    expect(parseFigletCommand("Grüße 🎉 @slant").text).toBe("Grüße 🎉");
  });
  it("whitespace-only arg behaves like empty", () => {
    const p = parseFigletCommand("   ");
    expect(p.text).toBe("");
    expect(p.fontQuery).toBeNull();
    expect(p.opts).toEqual({});
  });
  it("a single-dash token is literal text (flags need --)", () => {
    expect(parseFigletCommand("-center hi").text).toBe("-center hi");
  });
});

describe("fuzzyFont — ranking details", () => {
  it("prefers the shorter name on equal-quality prefix matches", () => {
    const fonts: FigletFontMeta[] = [
      { name: "small", category: "x", popular: false, pinned: false },
      { name: "smallcaps", category: "x", popular: false, pinned: false },
    ];
    expect(fuzzyFont("sma", fonts)).toBe("small");
  });
  it("is case-insensitive on the query", () => {
    expect(fuzzyFont("SLANT", FONTS)).toBe("slant");
  });
  it("whitespace-padded query still resolves", () => {
    expect(fuzzyFont("  slant  ", FONTS)).toBe("slant");
  });
  it("empty catalogue → null", () => {
    expect(fuzzyFont("slant", [])).toBeNull();
  });
});

describe("galleryFonts — ordering details", () => {
  it("empty-string query behaves like no query (default ranking)", () => {
    expect(galleryFonts("", FONTS).map((f) => f.name)).toEqual(
      galleryFonts(null, FONTS).map((f) => f.name),
    );
  });
  it("does not mutate the input array", () => {
    const copy = [...FONTS];
    galleryFonts(null, FONTS);
    expect(FONTS).toEqual(copy);
  });
  it("query matching nothing → empty list, not the full catalogue", () => {
    expect(galleryFonts("zzzz", FONTS)).toEqual([]);
  });
  it("ties within a rank break alphabetically", () => {
    const names = galleryFonts(null, FONTS).map((f) => f.name);
    const popular = names.slice(0, 6); // all popular fonts, none pinned
    expect(popular).toEqual([...popular].sort((a, b) => a.localeCompare(b)));
  });
});
