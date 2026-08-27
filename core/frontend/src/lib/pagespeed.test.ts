import { describe, it, expect } from "vitest";
import { scoreBand, bandColor } from "./pagespeed";

describe("score bands", () => {
  it("follows Lighthouse's own thresholds", () => {
    expect(scoreBand(100)).toBe("good");
    expect(scoreBand(90)).toBe("good");
    expect(scoreBand(89)).toBe("average");
    expect(scoreBand(50)).toBe("average");
    expect(scoreBand(49)).toBe("poor");
    expect(scoreBand(0)).toBe("poor");
  });

  it("an unscored category is unknown — never a zero", () => {
    // A 0 here would read as "catastrophically bad" for something Lighthouse
    // simply could not measure.
    expect(scoreBand(null)).toBe("unknown");
    expect(scoreBand(NaN)).toBe("unknown");
    expect(bandColor("unknown")).not.toBe(bandColor("poor"));
  });

  it("mirrors the Rust colours exactly — panel and export must agree", () => {
    // ⚠️ These four literals are duplicated in pagespeed_export::band_color.
    // If one side changes, the exported PDF stops matching what was on screen.
    expect(bandColor("good")).toBe("#0cce6b");
    expect(bandColor("average")).toBe("#ffa400");
    expect(bandColor("poor")).toBe("#ff4e42");
    expect(bandColor("unknown")).toBe("#9aa1ab");
  });
});
