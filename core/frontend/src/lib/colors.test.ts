import { describe, it, expect } from "vitest";
import { hsvToRgb, readableForeground, rgbToHex, rgbToHsl, rgbToHsv, tryParseColor } from "./colors";

describe("tryParseColor — happy paths", () => {
  it("parses 6-digit hex with hash", () => {
    const c = tryParseColor("#3366FF")!;
    expect(c.hex).toBe("#3366FF");
    expect(c.r).toBe(0x33);
    expect(c.g).toBe(0x66);
    expect(c.b).toBe(0xff);
    expect(c.a).toBe(1);
  });

  it("parses 6-digit hex without hash", () => {
    const c = tryParseColor("3366FF")!;
    expect(c.hex).toBe("#3366FF");
  });

  it("parses 3-digit hex (with hash, expanded)", () => {
    const c = tryParseColor("#abc")!;
    expect(c.hex).toBe("#AABBCC");
    expect(c.r).toBe(0xaa);
    expect(c.g).toBe(0xbb);
    expect(c.b).toBe(0xcc);
  });

  it("parses 4-digit hex (RGBA short form)", () => {
    const c = tryParseColor("#abcf")!;
    expect(c.r).toBe(0xaa);
    expect(c.a).toBe(1); // 0xff / 255 = 1.0
    // Fully opaque → no alpha in canonical
    expect(c.hex).toBe("#AABBCC");
  });

  it("parses 8-digit hex with alpha", () => {
    const c = tryParseColor("#3366FF80")!;
    expect(c.r).toBe(0x33);
    expect(c.b).toBe(0xff);
    expect(c.a).toBeCloseTo(0x80 / 255, 3);
    // Canonical includes alpha when < 1
    expect(c.hex).toBe("#3366FF80");
  });

  it("parses 8-digit hex without hash", () => {
    const c = tryParseColor("3366FF80")!;
    expect(c.hex).toBe("#3366FF80");
  });

  it("normalises lowercase to uppercase", () => {
    expect(tryParseColor("#abcdef")!.hex).toBe("#ABCDEF");
    expect(tryParseColor("abcdef")!.hex).toBe("#ABCDEF");
  });

  it("handles surrounding whitespace", () => {
    expect(tryParseColor("  #ff0000  ")!.hex).toBe("#FF0000");
  });
});

describe("tryParseColor — strict rejection", () => {
  it("rejects 3-digit hex without hash (too ambiguous)", () => {
    expect(tryParseColor("abc")).toBeNull();
    expect(tryParseColor("fed")).toBeNull();
    expect(tryParseColor("f00")).toBeNull();
  });

  it("rejects 4-digit hex without hash (too ambiguous)", () => {
    expect(tryParseColor("abcf")).toBeNull();
    expect(tryParseColor("f00d")).toBeNull();
  });

  it("rejects non-hex chars", () => {
    expect(tryParseColor("#xyzabc")).toBeNull();
    expect(tryParseColor("#3366GG")).toBeNull();
  });

  it("rejects invalid lengths", () => {
    expect(tryParseColor("#12")).toBeNull();
    expect(tryParseColor("#12345")).toBeNull();
    expect(tryParseColor("#1234567")).toBeNull();
    expect(tryParseColor("#123456789")).toBeNull();
  });

  it("rejects empty input", () => {
    expect(tryParseColor("")).toBeNull();
    expect(tryParseColor("   ")).toBeNull();
    expect(tryParseColor("#")).toBeNull();
  });

  it("rejects mixed alphanumeric search-like queries", () => {
    expect(tryParseColor("hello world")).toBeNull();
    expect(tryParseColor("note-12")).toBeNull();
    expect(tryParseColor("#hello")).toBeNull();
  });
});

describe("rgb / hsl conversions", () => {
  it("converts pure red", () => {
    const hsl = rgbToHsl(255, 0, 0);
    expect(hsl).toEqual({ h: 0, s: 100, l: 50 });
  });

  it("converts pure green", () => {
    const hsl = rgbToHsl(0, 255, 0);
    expect(hsl).toEqual({ h: 120, s: 100, l: 50 });
  });

  it("converts pure blue", () => {
    const hsl = rgbToHsl(0, 0, 255);
    expect(hsl).toEqual({ h: 240, s: 100, l: 50 });
  });

  it("converts white (no saturation)", () => {
    expect(rgbToHsl(255, 255, 255)).toEqual({ h: 0, s: 0, l: 100 });
  });

  it("converts black", () => {
    expect(rgbToHsl(0, 0, 0)).toEqual({ h: 0, s: 0, l: 0 });
  });

  it("converts mid-gray", () => {
    expect(rgbToHsl(128, 128, 128)).toEqual({ h: 0, s: 0, l: 50 });
  });

  it("formats RGB string", () => {
    expect(tryParseColor("#3366FF")!.rgbString).toBe("rgb(51, 102, 255)");
  });

  it("formats RGB string with alpha", () => {
    const c = tryParseColor("#3366FF80")!;
    expect(c.rgbString).toMatch(/^rgba\(51, 102, 255, 0\.50?\)$/);
  });
});

describe("hsv ⇄ rgb roundtrip", () => {
  it("converts pure red HSV(0, 100, 100) → RGB(255,0,0)", () => {
    expect(hsvToRgb(0, 100, 100)).toEqual([255, 0, 0]);
  });

  it("converts pure green HSV(120, 100, 100) → RGB(0,255,0)", () => {
    expect(hsvToRgb(120, 100, 100)).toEqual([0, 255, 0]);
  });

  it("converts pure blue HSV(240, 100, 100) → RGB(0,0,255)", () => {
    expect(hsvToRgb(240, 100, 100)).toEqual([0, 0, 255]);
  });

  it("rgbToHsv on pure red", () => {
    expect(rgbToHsv(255, 0, 0)).toEqual([0, 100, 100]);
  });

  it("rgbToHsv on white (no saturation)", () => {
    const [, s] = rgbToHsv(255, 255, 255);
    expect(s).toBe(0);
  });

  it("hsv → rgb → hsv round-trips for representative samples", () => {
    const samples: Array<[number, number, number]> = [
      [0, 100, 100],
      [60, 50, 50],
      [120, 100, 100],
      [180, 80, 60],
      [240, 100, 100],
      [300, 70, 90],
    ];
    for (const [h, s, v] of samples) {
      const [r, g, b] = hsvToRgb(h, s, v);
      const [h2, s2, v2] = rgbToHsv(r, g, b);
      // Allow ±2 deg / ±2 % rounding error from 8-bit channels.
      expect(Math.abs(h2 - h)).toBeLessThanOrEqual(2);
      expect(Math.abs(s2 - s)).toBeLessThanOrEqual(2);
      expect(Math.abs(v2 - v)).toBeLessThanOrEqual(2);
    }
  });
});

describe("rgbToHex", () => {
  it("formats with leading hash, uppercase", () => {
    expect(rgbToHex(51, 102, 255)).toBe("#3366FF");
  });

  it("clamps out-of-range values", () => {
    expect(rgbToHex(-5, 999, 128)).toBe("#00FF80");
  });
});

describe("readableForeground", () => {
  it("returns black on light backgrounds", () => {
    expect(readableForeground(255, 255, 255)).toBe("#000000");
    expect(readableForeground(255, 255, 0)).toBe("#000000");
  });

  it("returns white on dark backgrounds", () => {
    expect(readableForeground(0, 0, 0)).toBe("#FFFFFF");
    expect(readableForeground(33, 33, 33)).toBe("#FFFFFF");
    expect(readableForeground(0, 0, 255)).toBe("#FFFFFF");
  });
});
