import { afterEach, describe, it, expect, vi } from "vitest";
import { TOP_OPENERS, hashString, pickOpener, pickOpenerIndex } from "./openers";

describe("TOP_OPENERS data set", () => {
  it("contains exactly the curated top 100", () => {
    expect(TOP_OPENERS.length).toBe(100);
  });
  it("has no empty strings (data-integrity guard)", () => {
    for (const o of TOP_OPENERS) {
      expect(o.length).toBeGreaterThan(0);
    }
  });
  it("every entry is unique (no accidental dupes from the seed export)", () => {
    expect(new Set(TOP_OPENERS).size).toBe(TOP_OPENERS.length);
  });
});

describe("hashString", () => {
  it("returns a non-negative 32-bit integer", () => {
    const h = hashString("hello");
    expect(h).toBeGreaterThanOrEqual(0);
    expect(h).toBeLessThan(2 ** 32);
    expect(Number.isInteger(h)).toBe(true);
  });
  it("is deterministic — same input, same output", () => {
    expect(hashString("opener")).toBe(hashString("opener"));
    expect(hashString("opener xyz")).toBe(hashString("opener xyz"));
  });
  it("distinguishes distinct inputs", () => {
    expect(hashString("opener")).not.toBe(hashString("openers"));
    expect(hashString("a")).not.toBe(hashString("b"));
  });
  it("handles empty + Unicode inputs without throwing", () => {
    expect(typeof hashString("")).toBe("number");
    expect(typeof hashString("über 🦊")).toBe("number");
  });
});

describe("pickOpenerIndex", () => {
  it("returns an in-bounds index for any non-empty seed", () => {
    for (const seed of ["opener", "Opener", "opener xyz", "", "über 🦊"]) {
      const i = pickOpenerIndex(seed);
      expect(i).toBeGreaterThanOrEqual(0);
      expect(i).toBeLessThan(TOP_OPENERS.length);
    }
  });
  it("is deterministic — same seed, same index (used to anchor App's cycle)", () => {
    expect(pickOpenerIndex("opener")).toBe(pickOpenerIndex("opener"));
  });
  it("agrees with pickOpener (same seed → same picked string)", () => {
    expect(TOP_OPENERS[pickOpenerIndex("opener")]).toBe(pickOpener("opener"));
  });
});

describe("pickOpener", () => {
  it("always returns a string from the TOP_OPENERS set", () => {
    const set = new Set(TOP_OPENERS);
    for (const seed of ["opener", "opener ", "opener xyz", "Opener", "OPENER 123"]) {
      const picked = pickOpener(seed);
      expect(picked).not.toBeNull();
      expect(set.has(picked!)).toBe(true);
    }
  });
  it("is deterministic per seed — pinning the React render-loop", () => {
    // The same query rendered 60×/sec must show the same opener;
    // otherwise the user sees a flicker of different lines.
    expect(pickOpener("opener")).toBe(pickOpener("opener"));
    expect(pickOpener("opener xyz")).toBe(pickOpener("opener xyz"));
  });
  it("changes between distinct seeds — each keystroke re-rolls", () => {
    // Most pairs should differ. Test a handful — picks could collide,
    // but it'd be deeply surprising for the entire batch to share one
    // index. If this ever flakes, audit the hash distribution.
    const samples = ["opener", "opener ", "opener x", "opener xy", "opener xyz", "opener a"];
    const picks = samples.map((s) => pickOpener(s));
    expect(new Set(picks).size).toBeGreaterThan(1);
  });
});

describe("TOP_OPENERS — pasteable-verbatim invariants", () => {
  // The picked line is pasted straight into whatever field had focus (chat,
  // DM box) via `pasteText`, and rendered as ONE list row.
  it("no entry carries leading or trailing whitespace", () => {
    expect(TOP_OPENERS.filter((o) => o !== o.trim())).toEqual([]);
  });

  it("no entry contains a newline (a stray \\n would send the message early)", () => {
    expect(TOP_OPENERS.filter((o) => /[\r\n]/.test(o))).toEqual([]);
  });

  it("no entry contains a tab or a double space (single-row rendering)", () => {
    expect(TOP_OPENERS.filter((o) => o.includes("\t") || o.includes("  "))).toEqual([]);
  });

  it("every entry is a plausible one-liner, not a truncated fragment", () => {
    for (const o of TOP_OPENERS) {
      expect(o.length).toBeGreaterThanOrEqual(5);
      expect(o.length).toBeLessThanOrEqual(400);
    }
  });
});

describe("hashString — distribution", () => {
  it("spreads similar seeds across many different openers", () => {
    // Each keystroke re-rolls with the whole query as the seed. A hash that
    // clustered (e.g. if the multiply were dropped) would make the "re-roll"
    // feel broken while still passing the determinism tests.
    const picks = new Set<number>();
    for (let i = 0; i < 500; i++) picks.add(pickOpenerIndex(`opener ${i}`));
    expect(picks.size).toBeGreaterThan(TOP_OPENERS.length / 2);
  });

  it("every seed maps to an integer index, however exotic the input", () => {
    for (const seed of ["", " ", "opener".repeat(500), "\u{1F98A}", "\uD800", "über 🦊 世界"]) {
      const i = pickOpenerIndex(seed);
      expect(Number.isInteger(i)).toBe(true);
      expect(i).toBeGreaterThanOrEqual(0);
      expect(i).toBeLessThan(TOP_OPENERS.length);
    }
  });
});

describe("empty catalogue — the defensive path", () => {
  // Both `-1` / `null` branches are unreachable with the bundled data, so the
  // only way to prove the guard works (rather than reading `TOP_OPENERS[-1]`
  // as undefined into the paste) is to re-import against an empty list.
  afterEach(() => {
    vi.doUnmock("./openers-data");
    vi.resetModules();
  });

  it("pickOpenerIndex reports -1 and pickOpener yields null", async () => {
    vi.doMock("./openers-data", () => ({ TOP_OPENERS: [] as string[] }));
    vi.resetModules();
    const mod = await import("./openers");
    expect(mod.TOP_OPENERS).toHaveLength(0);
    expect(mod.pickOpenerIndex("opener")).toBe(-1);
    expect(mod.pickOpener("opener")).toBeNull();
    // Never an `undefined` that would paste the string "undefined".
    expect(mod.pickOpener("")).not.toBe(undefined);
  });
});
