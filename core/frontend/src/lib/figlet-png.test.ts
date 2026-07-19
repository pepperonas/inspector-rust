import { afterEach, describe, expect, it } from "vitest";
import { bannerGrid, bannerPngBase64, cropBanner, themePngColors } from "./figlet-png";

describe("cropBanner", () => {
  it("empty / whitespace-only banners crop to nothing", () => {
    expect(cropBanner("")).toEqual([]);
    expect(cropBanner("   \n\t\n  \n")).toEqual([]);
  });

  it("drops leading and trailing blank lines", () => {
    expect(cropBanner("\n\n  ##\n\n\n")).toEqual(["##"]);
  });

  it("strips the COMMON leading indent, preserving relative art offsets", () => {
    //   "  _|_"    → common indent 2 → "_|_" / " | " keeps its extra space
    expect(cropBanner("  _|_\n   | ")).toEqual(["_|_", " |"]);
  });

  it("keeps interior blank lines without letting them zero the indent", () => {
    expect(cropBanner("  ##\n\n  ##")).toEqual(["##", "", "##"]);
  });

  it("strips trailing whitespace per line (figlet pads to full width)", () => {
    expect(cropBanner("##   \n#    ")).toEqual(["##", "#"]);
  });

  it("no indent → lines unchanged (fast path)", () => {
    expect(cropBanner("ab\ncd")).toEqual(["ab", "cd"]);
  });

  it("normalises CRLF and lone CR line endings", () => {
    expect(cropBanner("##\r\n##\r##")).toEqual(["##", "##", "##"]);
  });

  it("preserves Unicode art glyphs verbatim", () => {
    expect(cropBanner("  ██▓\n  ░▒█")).toEqual(["██▓", "░▒█"]);
  });

  it("a single fully-indented line collapses to its glyphs", () => {
    expect(cropBanner("        #")).toEqual(["#"]);
  });
});

describe("themePngColors", () => {
  afterEach(() => {
    document.documentElement.style.removeProperty("--color-fg");
    document.documentElement.style.removeProperty("--color-bg");
  });

  it("reads the app's CSS custom properties", () => {
    document.documentElement.style.setProperty("--color-fg", "#123456");
    document.documentElement.style.setProperty("--color-bg", "#abcdef");
    expect(themePngColors()).toEqual({ fg: "#123456", bg: "#abcdef" });
  });

  it("falls back to the dark palette when the vars are unset (e.g. tests)", () => {
    const c = themePngColors();
    expect(c.fg).toBe("#e5e7eb");
    expect(c.bg).toBe("#111318");
  });

  it("transparent flag nulls the background but keeps the text colour", () => {
    document.documentElement.style.setProperty("--color-fg", "#ffffff");
    document.documentElement.style.setProperty("--color-bg", "#000000");
    expect(themePngColors(true)).toEqual({ fg: "#ffffff", bg: null });
  });
});

describe("bannerPngBase64 — guard paths (canvas-free)", () => {
  it("an empty/whitespace-only banner yields null before any canvas work", () => {
    expect(bannerPngBase64("", { fg: "#fff", bg: null })).toBeNull();
    expect(bannerPngBase64("   \n  \n", { fg: "#fff", bg: "#000" })).toBeNull();
  });

  it("degrades to null when the environment offers no 2D canvas context", () => {
    // happy-dom's canvas may or may not implement getContext("2d"); either
    // way the function must return null-or-string, never throw.
    const out = bannerPngBase64("##\n##", { fg: "#fff", bg: "#000" });
    expect(out === null || typeof out === "string").toBe(true);
  });
});

describe("bannerGrid", () => {
  it("cols = longest line, rows = line count", () => {
    expect(bannerGrid(["####", "#", "##"])).toEqual({ cols: 4, rows: 3 });
  });
  it("empty input → zero grid", () => {
    expect(bannerGrid([])).toEqual({ cols: 0, rows: 0 });
  });
  it("interior empty lines count as rows but not cols", () => {
    expect(bannerGrid(["##", "", "###"])).toEqual({ cols: 3, rows: 3 });
  });
});
