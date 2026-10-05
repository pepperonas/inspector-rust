import { describe, expect, it } from "vitest";
import { formatBytesDecimal, freeSpaceLevel, freeSpaceTitle } from "./free-space";

describe("formatBytesDecimal", () => {
  it("uses decimal units like System Settings", () => {
    expect(formatBytesDecimal(494_384_795_648, 2)).toBe("494,38 GB");
    expect(formatBytesDecimal(8_300_000_000)).toBe("8,3 GB");
  });
  it("is compact: one decimal under 10, none above", () => {
    expect(formatBytesDecimal(41_200_000_000)).toBe("41 GB");
    expect(formatBytesDecimal(1_500_000_000_000)).toBe("1,5 TB");
  });
  it("drops to MB and never shows negatives", () => {
    expect(formatBytesDecimal(850_000_000)).toBe("850 MB");
    expect(formatBytesDecimal(-5)).toBe("0 B");
  });
});

describe("freeSpaceLevel", () => {
  const T = 500e9;
  it("is ok with plenty of room", () => expect(freeSpaceLevel(200e9, T)).toBe("ok"));
  it("warns under 20 GB", () => expect(freeSpaceLevel(19e9, T)).toBe("warn"));
  it("warns under 10 % even when the absolute number is large", () =>
    expect(freeSpaceLevel(90e9, 1000e9)).toBe("warn"));
  it("is critical under 5 GB", () => expect(freeSpaceLevel(4.9e9, T)).toBe("crit"));
  it("boundary: exactly 5 GB is only a warning", () => expect(freeSpaceLevel(5e9, 100e9)).toBe("warn"));
  it("under 2 % of a big disk is critical even above 5 GB", () => expect(freeSpaceLevel(9e9, 1000e9)).toBe("crit"));
  it("a zero total never divides by zero", () => expect(freeSpaceLevel(30e9, 0)).toBe("ok"));
});

describe("freeSpaceTitle", () => {
  it("mirrors the System Settings sentence", () => {
    expect(freeSpaceTitle("Macintosh HD", 8_300_000_000, 494_384_795_648)).toBe(
      "Macintosh HD — 8,3 GB verfügbar von 494,38 GB",
    );
  });
});
