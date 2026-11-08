import { describe, it, expect } from "vitest";
import {
  DEFAULT_PREFS,
  addFavorite,
  parsePrefs,
  rememberStyle,
  removeFavorite,
  serializePrefs,
  styleFor,
  withLastTool,
} from "./editor-prefs";
import { DEFAULT_STYLE } from "./editor-geometry";

describe("parsePrefs", () => {
  it("garbage and nothing fall back to the defaults", () => {
    expect(parsePrefs(null)).toEqual(DEFAULT_PREFS);
    expect(parsePrefs("not json")).toEqual(DEFAULT_PREFS);
    expect(parsePrefs("[1,2]")).toEqual(DEFAULT_PREFS);
    expect(parsePrefs('"x"')).toEqual(DEFAULT_PREFS);
  });

  it("keeps valid fields and repairs the rest field by field", () => {
    const p = parsePrefs(JSON.stringify({
      version: 1,
      lastTool: "rect",
      styles: {
        arrow: { color: "#00ff00", width: 99, arrowStyle: "curved", opacity: "x" },
        rect: { color: "red", fill: "filled" },
        bogus: { color: "#000000" },
      },
      favorites: ["#123456", "nope", "#ABCDEF"],
    }));
    expect(p.lastTool).toBe("rect");
    expect(p.styles.arrow).toMatchObject({ color: "#00ff00", width: 24, arrowStyle: "curved", opacity: DEFAULT_STYLE.opacity });
    expect(p.styles.rect).toMatchObject({ color: DEFAULT_STYLE.color, fill: "filled" });
    expect(p.styles).not.toHaveProperty("bogus");
    expect(p.favorites).toEqual(["#123456", "#abcdef"]);
  });

  it("an unknown last tool falls back to the arrow", () => {
    expect(parsePrefs(JSON.stringify({ lastTool: "laser" })).lastTool).toBe("arrow");
  });

  it("caps favourites at 8", () => {
    const many = Array.from({ length: 20 }, (_, i) => `#0000${String(i).padStart(2, "0")}`);
    expect(parsePrefs(JSON.stringify({ favorites: many })).favorites).toHaveLength(8);
  });
});

describe("per-tool memory", () => {
  it("each tool keeps its own style", () => {
    let p = rememberStyle(DEFAULT_PREFS, "arrow", { color: "#ff0000", width: 8 });
    p = rememberStyle(p, "rect", { color: "#0000ff", width: 3 });
    expect(styleFor(p, "arrow")).toMatchObject({ color: "#ff0000", width: 8 });
    expect(styleFor(p, "rect")).toMatchObject({ color: "#0000ff", width: 3 });
    expect(styleFor(p, "line")).toEqual(DEFAULT_STYLE);
  });

  it("tools without a style remember nothing", () => {
    expect(rememberStyle(DEFAULT_PREFS, "crop", { color: "#ff0000" })).toBe(DEFAULT_PREFS);
    expect(styleFor(DEFAULT_PREFS, "select")).toEqual(DEFAULT_STYLE);
  });

  it("remembers the last tool", () => {
    expect(withLastTool(DEFAULT_PREFS, "text").lastTool).toBe("text");
  });

  it("survives a round trip", () => {
    let p = rememberStyle(DEFAULT_PREFS, "arrow", { arrowStyle: "marker", width: 9 });
    p = withLastTool(addFavorite(p, "#112233"), "ellipse");
    expect(parsePrefs(serializePrefs(p))).toEqual(p);
  });
});

describe("favourites", () => {
  it("newest first, no duplicates (case-insensitive), max 8", () => {
    let p = DEFAULT_PREFS;
    for (let i = 0; i < 10; i++) p = addFavorite(p, `#00000${i}`);
    expect(p.favorites).toHaveLength(8);
    expect(p.favorites[0]).toBe("#000009");
    p = addFavorite(p, "#000005".toUpperCase());
    expect(p.favorites[0]).toBe("#000005");
    expect(p.favorites.filter((c) => c === "#000005")).toHaveLength(1);
  });

  it("ignores invalid colours and removes on request", () => {
    expect(addFavorite(DEFAULT_PREFS, "red")).toBe(DEFAULT_PREFS);
    const p = addFavorite(DEFAULT_PREFS, "#abcdef");
    expect(removeFavorite(p, "#ABCDEF").favorites).toEqual([]);
  });
});
