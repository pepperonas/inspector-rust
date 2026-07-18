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
