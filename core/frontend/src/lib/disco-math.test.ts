import { describe, it, expect } from "vitest";
import { nextIndex, beatColor, floorBrightness } from "./disco-math";

const PAL = ["#a", "#b", "#c"] as const;

describe("nextIndex", () => {
  it("advances and wraps", () => {
    expect(nextIndex(0, 3)).toBe(1);
    expect(nextIndex(2, 3)).toBe(0);
    expect(nextIndex(1, 1)).toBe(0);
  });
  it("is safe for n ≤ 0", () => {
    expect(nextIndex(5, 0)).toBe(0);
    expect(nextIndex(0, -1)).toBe(0);
  });
});

describe("beatColor", () => {
  it("rainbow steps through the palette, wrapping the index", () => {
    expect(beatColor("rainbow", "#fff", 0, PAL)).toEqual({ hex: "#a", nextPal: 1 });
    expect(beatColor("rainbow", "#fff", 1, PAL)).toEqual({ hex: "#b", nextPal: 2 });
    expect(beatColor("rainbow", "#fff", 2, PAL)).toEqual({ hex: "#c", nextPal: 0 });
  });
  it("cycles a full lap back to the start", () => {
    let pal = 0;
    const seen: string[] = [];
    for (let i = 0; i < PAL.length + 1; i++) {
      const r = beatColor("rainbow", "#fff", pal, PAL);
      seen.push(r.hex);
      pal = r.nextPal;
    }
    expect(seen).toEqual(["#a", "#b", "#c", "#a"]);
  });
  it("pulse / strobe hold the fixed colour and never advance the palette", () => {
    expect(beatColor("pulse", "#ff2d95", 1, PAL)).toEqual({ hex: "#ff2d95", nextPal: 1 });
    expect(beatColor("strobe", "#00ff00", 2, PAL)).toEqual({ hex: "#00ff00", nextPal: 2 });
  });
  it("falls back to the fixed colour with an empty palette", () => {
    expect(beatColor("rainbow", "#abc", 0, [])).toEqual({ hex: "#abc", nextPal: 0 });
  });
});

describe("floorBrightness", () => {
  it("strobe goes to the near-dark floor; others keep the dim floor", () => {
    expect(floorBrightness("strobe", 22, 1)).toBe(1);
    expect(floorBrightness("rainbow", 22, 1)).toBe(22);
    expect(floorBrightness("pulse", 22, 1)).toBe(22);
  });
});
