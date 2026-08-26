import { describe, it, expect } from "vitest";
import {
  ACTS,
  X_DURATION,
  FLASH_MIN_GAP_MS,
  actAt,
  flashAllowed,
  clamp01,
  easeOut,
  easeIn,
  arc,
  noise,
  horizonY,
  warpRadius,
  corrupt,
  WORDS,
} from "./xhype";

describe("timeline", () => {
  it("acts are contiguous, gapless and ordered", () => {
    let cursor = 0;
    for (const a of ACTS) {
      expect(a.at, a.key).toBe(cursor);
      expect(a.dur, a.key).toBeGreaterThan(0);
      cursor += a.dur;
    }
    expect(X_DURATION).toBe(cursor);
  });

  it("every act has words to show", () => {
    for (const a of ACTS) expect(WORDS[a.key].length, a.key).toBeGreaterThan(0);
  });

  it("actAt resolves each act at its boundaries", () => {
    expect(actAt(0)?.act.key).toBe("ignition");
    for (const a of ACTS) {
      expect(actAt(a.at)?.act.key, `start of ${a.key}`).toBe(a.key);
      expect(actAt(a.at + a.dur - 1)?.act.key, `end of ${a.key}`).toBe(a.key);
    }
  });

  it("local progress runs 0 → ~1 within an act", () => {
    const grid = ACTS.find((a) => a.key === "grid")!;
    expect(actAt(grid.at)!.local).toBe(0);
    expect(actAt(grid.at + grid.dur / 2)!.local).toBeCloseTo(0.5, 5);
    expect(actAt(grid.at + grid.dur - 1)!.local).toBeGreaterThan(0.99);
  });

  it("past the end there is no act — that's how the piece knows to close", () => {
    expect(actAt(X_DURATION)).toBeNull();
    expect(actAt(X_DURATION + 5000)).toBeNull();
    // Negative time clamps to the opening rather than throwing.
    expect(actAt(-100)?.act.key).toBe("ignition");
  });
});

describe("photosensitivity guard", () => {
  it("full-screen flashes stay under three per second (WCAG 2.3.1)", () => {
    // The threshold is 3/s; the gap must be strictly above 1000/3 ms… by
    // construction it is 340, so at most 2 flashes land in any 1000 ms.
    expect(FLASH_MIN_GAP_MS).toBeGreaterThanOrEqual(334);
    let last: number | null = null;
    let fired = 0;
    // Ask on every frame of a 1 s 120 Hz burst — a greedy renderer.
    for (let t = 0; t < 1000; t += 1000 / 120) {
      if (flashAllowed(last, t)) {
        fired += 1;
        last = t;
      }
    }
    expect(fired).toBeLessThanOrEqual(3);
  });

  it("the first flash is always allowed", () => {
    expect(flashAllowed(null, 0)).toBe(true);
    expect(flashAllowed(0, FLASH_MIN_GAP_MS - 1)).toBe(false);
    expect(flashAllowed(0, FLASH_MIN_GAP_MS)).toBe(true);
  });
});

describe("easing + noise", () => {
  it("clamp01 / easeIn / easeOut / arc stay in range and hit their anchors", () => {
    expect(clamp01(-1)).toBe(0);
    expect(clamp01(2)).toBe(1);
    for (const f of [easeIn, easeOut, arc]) {
      expect(f(0)).toBeCloseTo(f === arc ? 0 : f === easeIn ? 0 : 0, 6);
      for (let x = 0; x <= 1; x += 0.05) {
        const v = f(x);
        expect(v).toBeGreaterThanOrEqual(0);
        expect(v).toBeLessThanOrEqual(1.0001);
      }
    }
    expect(easeIn(1)).toBeCloseTo(1, 6);
    expect(easeOut(1)).toBeCloseTo(1, 6);
    expect(arc(0.5)).toBeCloseTo(1, 6); // peaks in the middle
    expect(arc(1)).toBeCloseTo(0, 6);
  });

  it("noise is deterministic, in [0,1), and varies with index + seed", () => {
    expect(noise(5)).toBe(noise(5));
    const vals = Array.from({ length: 200 }, (_, i) => noise(i));
    for (const v of vals) {
      expect(v).toBeGreaterThanOrEqual(0);
      expect(v).toBeLessThan(1);
    }
    // Not a constant, and the seed actually shifts the sequence.
    expect(new Set(vals).size).toBeGreaterThan(150);
    expect(noise(5, 2)).not.toBe(noise(5, 1));
  });
});

describe("geometry", () => {
  it("horizonY runs from the bottom edge to the horizon", () => {
    const h = 1000;
    const horizon = 400;
    expect(horizonY(0, h, horizon)).toBeCloseTo(h, 3); // at the viewer
    expect(horizonY(1, h, horizon)).toBeCloseTo(horizon, 3); // at the horizon
    // Monotonic: deeper is always higher on screen.
    let prev = Infinity;
    for (let z = 0; z <= 1; z += 0.05) {
      const y = horizonY(z, h, horizon);
      expect(y).toBeLessThanOrEqual(prev + 1e-9);
      prev = y;
    }
  });

  it("warpRadius accelerates outward from 0 to the full radius", () => {
    expect(warpRadius(0, 500)).toBe(0);
    expect(warpRadius(1, 500)).toBeCloseTo(500, 6);
    // Eased-in: the first half covers far less than half the distance.
    expect(warpRadius(0.5, 500)).toBeLessThan(250);
  });
});

describe("corrupt", () => {
  it("keeps the original characters and only adds marks", () => {
    const src = "SLOP";
    const out = corrupt(src, 1);
    // Every original letter survives, in order.
    expect([...out].filter((c) => /[A-Z]/.test(c)).join("")).toBe(src);
    expect(out.length).toBeGreaterThan(src.length);
  });

  it("amount 0 is a clean passthrough and spaces stay bare", () => {
    expect(corrupt("ALLES BRENNT", 0)).toBe("ALLES BRENNT");
    const out = corrupt("A B", 1);
    // The space itself never collects marks (words stay separable).
    expect(out).toContain(" ");
  });

  it("is deterministic for a given seed", () => {
    expect(corrupt("MEHR", 0.6, 3)).toBe(corrupt("MEHR", 0.6, 3));
    expect(corrupt("MEHR", 0.6, 3)).not.toBe(corrupt("MEHR", 0.6, 9));
  });
});
