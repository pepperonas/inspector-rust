import { describe, it, expect } from "vitest";
// Vite's `?raw` import — typed by `vite/client`, so this works in tsc AND in
// vitest. `node:fs` would not: the frontend tsconfig has no Node types.
import xoverlaySource from "../components/XOverlay.tsx?raw";
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
  captionFor,
  featureWords,
  newsWords,
  shorten,
  shuffle,
  FEATURES,
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

  it("runs for 30 s", () => {
    expect(X_DURATION).toBe(30000);
  });

  it("both modes fill every act with words", () => {
    for (const w of [featureWords(58, 1), newsWords(["Eins", "Zwei", "Drei"], 1)]) {
      for (const a of ACTS) {
        expect(w[a.key].length, a.key).toBeGreaterThan(0);
        for (const line of w[a.key]) expect(line.trim().length, a.key).toBeGreaterThan(0);
      }
    }
  });

  it("each mode has its own HUD captions", () => {
    for (let i = 0; i < ACTS.length; i++) {
      expect(captionFor("features", i).length).toBeGreaterThan(0);
      expect(captionFor("news", i).length).toBeGreaterThan(0);
    }
    expect(captionFor("features", 0)).not.toBe(captionFor("news", 0));
  });

  it("the renderer draws NO literal word — every act reads from WORDS", () => {
    // Regression: two acts had their text hard-coded, so editing WORDS
    // silently left them showing the old line. Comment-free source, per the
    // house rule (the prose above legitimately quotes such calls).
    const src = xoverlaySource
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/^\s*\/\/.*$/gm, "");
    expect(src).not.toMatch(/stab\(\s*["'`]/);
    expect(src).toMatch(/words\./);
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

describe("word builders", () => {

  it("names CAPABILITIES, not command keywords", () => {
    const w = featureWords(58, 3);
    for (const line of [...w.grid, ...w.slop, ...w.burn]) {
      expect(FEATURES, line).toContain(line);
    }
    // The nova still states the real command count.
    expect(w.nova[0]).toContain("58");
  });

  it("the capability list is usable copy: unique, short, non-empty", () => {
    expect(FEATURES.length).toBeGreaterThanOrEqual(40); // enough to vary
    expect(new Set(FEATURES).size).toBe(FEATURES.length);
    for (const f of FEATURES) {
      expect(f.trim(), f).toBe(f);
      expect(f.length, f).toBeGreaterThan(1);
      expect(f.length, f).toBeLessThanOrEqual(16); // fits 200px display type
    }
  });

  it("draws a DIFFERENT set on different runs", () => {
    const a = featureWords(58, 1);
    const b = featureWords(58, 2);
    expect([...a.grid, ...a.slop]).not.toEqual([...b.grid, ...b.slop]);
  });

  it("the draw varies by seed but is deterministic for one", () => {
    expect(featureWords(58, 1)).toEqual(featureWords(58, 1));
    expect(featureWords(58, 1).grid).not.toEqual(featureWords(58, 99).grid);
  });

  it("a zero command count still yields a playable piece", () => {
    const w = featureWords(0, 1);
    for (const a of ACTS) expect(w[a.key].length, a.key).toBeGreaterThan(0);
  });

  it("news uses the headlines and credits the source in the epilogue", () => {
    const heads = ["Erste Meldung", "Zweite Meldung", "Dritte Meldung"];
    const w = newsWords(heads, 5);
    expect(w.void[0]).toContain("tagesschau");
    const shown = [...w.grid, ...w.slop, ...w.burn, ...w.nova];
    for (const line of shown) expect(heads).toContain(line);
  });

  it("does not pass the array index as a length (the .map(shorten) trap)", () => {
    // `.map(shorten)` hands the INDEX to `shorten`'s optional `max`, which cut
    // headline #2 down to two characters. Short headlines must survive intact
    // no matter where they sit in the list.
    const heads = ["Alpha Meldung", "Beta Meldung", "Gamma Meldung", "Delta Meldung"];
    const w = newsWords(heads, 2);
    for (const line of [...w.grid, ...w.slop, ...w.burn]) {
      expect(line.endsWith("…"), line).toBe(false);
      expect(heads).toContain(line);
    }
  });

  it("no headlines → the feature showcase, never an empty stage", () => {
    const w = newsWords([], 1);
    for (const a of ACTS) expect(w[a.key].length, a.key).toBeGreaterThan(0);
    expect(w.ignition[0]).toBe("INSPECTOR");
    // Blank-only input counts as none.
    expect(newsWords(["   ", ""], 1).ignition[0]).toBe("INSPECTOR");
  });

  it("shuffle keeps every element exactly once and is seed-stable", () => {
    const src = [1, 2, 3, 4, 5, 6, 7, 8];
    const a = shuffle(src, 4);
    expect([...a].sort()).toEqual(src);
    expect(a).toEqual(shuffle(src, 4));
    expect(src).toEqual([1, 2, 3, 4, 5, 6, 7, 8]); // input untouched
  });
});

describe("shorten", () => {
  it("leaves short headlines alone", () => {
    expect(shorten("Kurze Meldung")).toBe("Kurze Meldung");
  });

  it("cuts long ones on a word boundary, never mid-word", () => {
    const long = "Eine ausgesprochen lange Schlagzeile ueber ein wichtiges Ereignis heute";
    const out = shorten(long, 42);
    expect(out.length).toBeLessThanOrEqual(43); // + the ellipsis
    expect(out.endsWith("…")).toBe(true);
    // The last kept word is whole — the source contains it verbatim.
    const lastWord = out.slice(0, -1).trim().split(" ").pop()!;
    expect(long.split(" ")).toContain(lastWord);
  });

  it("normalises whitespace", () => {
    expect(shorten("Zwei   Woerter\n hier")).toBe("Zwei Woerter hier");
  });
});
