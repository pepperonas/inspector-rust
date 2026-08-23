import { describe, it, expect } from "vitest";
import {
  languageColor,
  LANGUAGE_COLORS,
  donutSegments,
  formatCount,
  formatPct,
} from "./loc";

describe("languageColor", () => {
  it("returns the Linguist colour for known languages", () => {
    expect(languageColor("Rust")).toBe("#dea584");
    expect(languageColor("TypeScript")).toBe("#3178c6");
  });

  it("is deterministic and readable for unknown languages", () => {
    const a = languageColor("SomeBrandNewLang");
    expect(languageColor("SomeBrandNewLang")).toBe(a); // stable across calls
    expect(a).toMatch(/^hsl\(\d+, 55%, 55%\)$/);
    // Different names → (almost always) different hues; pin one contrast.
    expect(languageColor("OtherLang")).not.toBe(a);
  });

  it("every mapped colour is a valid hex", () => {
    for (const c of Object.values(LANGUAGE_COLORS)) {
      expect(c).toMatch(/^#[0-9a-fA-F]{6}$/);
    }
  });
});

describe("donutSegments", () => {
  const OPTS = { cx: 50, cy: 50, rOuter: 45, rInner: 28 };

  it("splits shares into ring segments covering the full circle", () => {
    const segs = donutSegments(
      [
        { name: "Rust", pct: 60 },
        { name: "TypeScript", pct: 40 },
      ],
      OPTS,
    );
    expect(segs).toHaveLength(2);
    expect(segs.every((s) => s.d.startsWith("M ") && s.d.includes("A "))).toBe(true);
    expect(segs[0].color).toBe("#dea584");
  });

  it("folds slivers below minPct into an Other segment", () => {
    const segs = donutSegments(
      [
        { name: "Rust", pct: 97 },
        { name: "Lua", pct: 1.5 },
        { name: "Perl", pct: 1.5 },
      ],
      { ...OPTS, minPct: 2 },
    );
    expect(segs.map((s) => s.name)).toEqual(["Rust", "Other"]);
    expect(segs[1].pct).toBeCloseTo(3);
  });

  it("renders a single 100 % language as a visible full ring", () => {
    // An SVG arc whose start equals its end draws NOTHING — the classic
    // 100 % donut bug. The full circle must be composed of two half arcs.
    const segs = donutSegments([{ name: "Rust", pct: 100 }], OPTS);
    expect(segs).toHaveLength(1);
    const arcs = segs[0].d.match(/A /g) ?? [];
    expect(arcs.length).toBeGreaterThanOrEqual(4); // 2 half-rings × outer+inner
  });

  it("is empty for empty or zero shares (no NaN paths)", () => {
    expect(donutSegments([], OPTS)).toEqual([]);
    expect(donutSegments([{ name: "X", pct: 0 }], OPTS)).toEqual([]);
  });
});

describe("formatters", () => {
  it("groups thousands and formats percentages in de style", () => {
    expect(formatCount(1234567)).toBe("1.234.567");
    expect(formatCount(0)).toBe("0");
    expect(formatPct(42.34)).toBe("42,3 %");
    expect(formatPct(0.01)).toBe("0,0 %"); // never "-0,0"
    expect(formatPct(100)).toBe("100,0 %");
  });
});
