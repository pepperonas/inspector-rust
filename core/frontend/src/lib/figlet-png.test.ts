import { describe, expect, it } from "vitest";
import { bannerGrid, cropBanner } from "./figlet-png";

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
