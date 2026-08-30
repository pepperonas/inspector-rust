import { describe, it, expect } from "vitest";
import {
  isResizeQuery,
  resizeQueryArg,
  parseResizeCommand,
  targetSize,
  exceedsCap,
  describeSpec,
  MAX_PIXELS,
  PCT_MAX,
  PX_MAX,
} from "./resize";

describe("parseResizeCommand — the two-case rule", () => {
  it("reads a single number as percent", () => {
    expect(parseResizeCommand("50")).toEqual({ mode: "pct", x: 50, y: 50, explicit: false });
    expect(parseResizeCommand("200")).toEqual({ mode: "pct", x: 200, y: 200, explicit: false });
  });

  it("keeps two numbers as PIXELS — every documented example still means px", () => {
    // ⚠️ Backwards compatibility is the whole reason the rule has two cases:
    // these three inputs are the CommandDoc's own examples and have meant
    // pixels since v0.84.72. Reading them as percent would blow the 16 MP cap
    // instead of resizing.
    for (const input of ["1200x800", "512 512", "64x64", "200 x 200", "200X200"]) {
      expect(parseResizeCommand(input)?.mode, input).toBe("px");
    }
    expect(parseResizeCommand("1200x800")).toEqual({ mode: "px", x: 1200, y: 800, explicit: false });
  });
});

describe("parseResizeCommand — named modes win", () => {
  it("accepts every pixel spelling", () => {
    for (const w of ["px", "pixel", "pixels"]) {
      expect(parseResizeCommand(`${w} 800x600`), w).toEqual({
        mode: "px", x: 800, y: 600, explicit: true,
      });
      // ⚠️ A single number under an explicit px mode is a square in PIXELS,
      // not a percentage — the named mode overrides the shape rule.
      expect(parseResizeCommand(`${w} 50`)?.mode, w).toBe("px");
    }
  });

  it("accepts every percent spelling", () => {
    for (const w of ["%", "pc", "pct", "percent", "prozent"]) {
      expect(parseResizeCommand(`${w} 50`), w).toEqual({
        mode: "pct", x: 50, y: 50, explicit: true,
      });
      // Two numbers under an explicit percent mode scale the axes separately.
      expect(parseResizeCommand(`${w} 50x25`), w).toEqual({
        mode: "pct", x: 50, y: 25, explicit: true,
      });
    }
  });

  it("accepts a glued or trailing percent sign", () => {
    expect(parseResizeCommand("%50")).toEqual({ mode: "pct", x: 50, y: 50, explicit: true });
    expect(parseResizeCommand("50%")).toEqual({ mode: "pct", x: 50, y: 50, explicit: true });
    // ⚠️ A trailing % must flip TWO numbers to percent too, or `50x25%` would
    // silently resize to 50×25 pixels — the opposite of what was typed.
    expect(parseResizeCommand("50x25%")).toEqual({ mode: "pct", x: 50, y: 25, explicit: true });
  });

  it("rejects an unknown word instead of guessing", () => {
    // A typo must not fall through to a mode the user did not ask for.
    expect(parseResizeCommand("pixl 800x600")).toBeNull();
    expect(parseResizeCommand("prozent")).toBeNull(); // mode without a spec
    expect(parseResizeCommand("px")).toBeNull();
  });
});

describe("parseResizeCommand — ranges", () => {
  it("refuses what cannot work", () => {
    expect(parseResizeCommand("")).toBeNull();
    expect(parseResizeCommand("0")).toBeNull();
    expect(parseResizeCommand("0x100")).toBeNull();
    expect(parseResizeCommand(`${PCT_MAX + 1}%`)).toBeNull();
    expect(parseResizeCommand(`px ${PX_MAX + 1}x10`)).toBeNull();
    expect(parseResizeCommand("abc")).toBeNull();
    expect(parseResizeCommand("50x")).toBeNull();
  });

  it("refuses a pixel target beyond the backend's area cap", () => {
    // 16 MP. 5000×5000 = 25 MP -> the backend would reject it; do not offer it.
    expect(parseResizeCommand("5000x5000")).toBeNull();
    expect(parseResizeCommand("4096x4096")).not.toBeNull(); // exactly 16 MP
  });
});

describe("targetSize", () => {
  const src = { w: 3024, h: 4032 };

  it("passes pixels through unchanged", () => {
    expect(targetSize(src, { mode: "px", x: 1200, y: 800, explicit: true })).toEqual({
      w: 1200, h: 800,
    });
  });

  it("scales each axis and rounds", () => {
    expect(targetSize(src, { mode: "pct", x: 50, y: 50, explicit: false })).toEqual({
      w: 1512, h: 2016,
    });
    expect(targetSize({ w: 101, h: 101 }, { mode: "pct", x: 50, y: 50, explicit: false })).toEqual({
      w: 51, h: 51, // 50.5 rounds up
    });
    expect(targetSize(src, { mode: "pct", x: 50, y: 25, explicit: true })).toEqual({
      w: 1512, h: 1008,
    });
  });

  it("never rounds down to zero", () => {
    // ⚠️ 2 % of 30 px is 0.6. A zero-sized target is rejected by the backend
    // and would fail the whole batch over one tiny image.
    expect(targetSize({ w: 30, h: 30 }, { mode: "pct", x: 2, y: 2, explicit: true })).toEqual({
      w: 1, h: 1,
    });
  });
});

describe("exceedsCap", () => {
  it("is per image, because percent depends on the source", () => {
    expect(exceedsCap({ w: 4096, h: 4096 })).toBe(false);
    expect(exceedsCap({ w: 4097, h: 4096 })).toBe(true);
    // A 500 % scale of an 8000×8000 source is far past the cap.
    const big = targetSize({ w: 8000, h: 8000 }, { mode: "pct", x: 500, y: 500, explicit: true });
    expect(big.w * big.h).toBeGreaterThan(MAX_PIXELS);
    expect(exceedsCap(big)).toBe(true);
  });
});

describe("describeSpec", () => {
  it("phrases both modes the same way everywhere", () => {
    expect(describeSpec({ mode: "pct", x: 50, y: 50, explicit: false })).toBe("50 % × 50 %");
    expect(describeSpec({ mode: "px", x: 1200, y: 800, explicit: false })).toBe("1200 × 800 px");
  });
});

describe("parseResizeCommand — inherited from parseResizeArg (removed in v0.153.0)", () => {
  it("keeps every separator the old parser accepted", () => {
    const px = { mode: "px", explicit: false } as const;
    expect(parseResizeCommand("1200X800")).toEqual({ ...px, x: 1200, y: 800 });
    expect(parseResizeCommand("1200 x 800")).toEqual({ ...px, x: 1200, y: 800 });
    expect(parseResizeCommand("  1200x800  ")).toEqual({ ...px, x: 1200, y: 800 });
    expect(parseResizeCommand("200 200")).toEqual({ ...px, x: 200, y: 200 });
    expect(parseResizeCommand("1200   800")).toEqual({ ...px, x: 1200, y: 800 });
  });

  it("keeps rejecting the same malformed input", () => {
    expect(parseResizeCommand("1200x")).toBeNull();
    expect(parseResizeCommand("x800")).toBeNull();
    expect(parseResizeCommand("12ab")).toBeNull();
    // ⚠️ `200200` used to be rejected as "no separator". It is now read as a
    // single number = 200200 %, which is out of range -> still null, and
    // still for a defensible reason.
    expect(parseResizeCommand("200200")).toBeNull();
  });

  it("DELIBERATELY changes one case: a lone number is now a percentage", () => {
    // The old parser returned null for `rz 200`; that is the whole point of
    // the new mode. 200 % is in range, so it now parses.
    expect(parseResizeCommand("200")).toEqual({ mode: "pct", x: 200, y: 200, explicit: false });
  });
});

describe("isResizeQuery / resizeQueryArg", () => {
  it("fires for the BARE keyword, which is not a complete command", () => {
    // ⚠️ This is why the panel is keyed off the query: `rz` alone has no
    // argument, so `parsedCommand` is null and the preview fell back to the
    // generic suggestion card — exactly what the modes list replaces.
    expect(isResizeQuery("rz")).toBe(true);
    expect(isResizeQuery("rz ")).toBe(true);
    expect(isResizeQuery("resize 50")).toBe(true);
    expect(isResizeQuery("RZ px 800x600")).toBe(true);
  });

  it("does not fire for a different word that merely starts with rz", () => {
    expect(isResizeQuery("rzz")).toBe(false);
    expect(isResizeQuery("rza 50")).toBe(false);
    expect(isResizeQuery("brz")).toBe(false);
    expect(isResizeQuery("")).toBe(false);
  });

  it("strips the keyword and leaves the argument intact", () => {
    expect(resizeQueryArg("rz")).toBe("");
    expect(resizeQueryArg("rz 50")).toBe("50");
    expect(resizeQueryArg("  rz   px 800x600 ")).toBe("px 800x600");
    expect(resizeQueryArg("resize % 150")).toBe("% 150");
  });
});
