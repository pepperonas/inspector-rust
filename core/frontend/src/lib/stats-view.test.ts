import { describe, expect, it } from "vitest";
import {
  cardForKey,
  groupTemps,
  loadLevel,
  memSegments,
  parseCollapsed,
  pushSample,
  shareOfTop,
  sparkPaths,
  splitCores,
  stackUpTo,
  tempGroupOf,
  toggleCollapsed,
  visibleDisks,
} from "./stats-view";

describe("loadLevel", () => {
  it("reads the load against the core count", () => {
    expect(loadLevel(1, 10)).toBe("idle");
    expect(loadLevel(3, 10)).toBe("normal");
    expect(loadLevel(7, 10)).toBe("busy");
    expect(loadLevel(10, 10)).toBe("busy");
    expect(loadLevel(10.1, 10)).toBe("over");
  });
  it("the same load is harmless on more cores", () => {
    expect(loadLevel(8, 8)).toBe("busy");
    expect(loadLevel(8, 32)).toBe("idle");
  });
  it("never divides by nothing", () => {
    expect(loadLevel(5, 0)).toBe("idle");
    expect(loadLevel(Number.NaN, 8)).toBe("idle");
  });
});

describe("splitCores", () => {
  it("puts the efficiency cores first, as Apple Silicon numbers them", () => {
    const g = splitCores([1, 2, 3, 4, 5, 6], 4, 2);
    expect(g?.efficiency).toEqual([{ index: 0, pct: 1 }, { index: 1, pct: 2 }]);
    expect(g?.performance.map((c) => c.index)).toEqual([2, 3, 4, 5]);
  });
  it("gives up rather than mislabel when the counts do not add up", () => {
    expect(splitCores([1, 2, 3], 4, 2)).toBeNull();
    expect(splitCores([1, 2, 3], null, null)).toBeNull();
    expect(splitCores([1, 2], 2, 0)).toBeNull();
  });
});

describe("memSegments", () => {
  it("shares add up to one, in drawing order", () => {
    const s = memSegments({ wired: 2, active: 4, compressed: 1, inactive: 2, free: 1 });
    expect(s.map((x) => x.kind)).toEqual(["wired", "active", "compressed", "inactive", "free"]);
    expect(s.map((x) => x.share)).toEqual([0.2, 0.4, 0.1, 0.2, 0.1]);
    expect(s.reduce((a, x) => a + x.share, 0)).toBeCloseTo(1);
  });
  it("an empty or broken reading is all zeros, never NaN", () => {
    const s = memSegments({ wired: 0, active: Number.NaN, compressed: -5, inactive: 0, free: 0 });
    expect(s.every((x) => x.share === 0 && x.bytes === 0)).toBe(true);
  });
});

describe("temperatures", () => {
  it("sorts real sensor labels into parts of the machine", () => {
    expect(tempGroupOf("PMU tdie4")).toBe("cpu");
    expect(tempGroupOf("PMU TP1s")).toBe("cpu");
    expect(tempGroupOf("CPU performance core 3")).toBe("cpu");
    expect(tempGroupOf("PMU TP2g #2")).toBe("gpu");
    expect(tempGroupOf("GPU")).toBe("gpu");
    expect(tempGroupOf("gas gauge battery #3")).toBe("battery");
    expect(tempGroupOf("NAND CH0 temp")).toBe("storage");
    expect(tempGroupOf("PMU tdev3")).toBe("other");
    expect(tempGroupOf("PMU tcal")).toBe("other");
  });
  it("groups in a fixed order with the hottest value and the mean", () => {
    const g = groupTemps([
      { label: "NAND CH0 temp", celsius: 36 },
      { label: "PMU tdie1", celsius: 50 },
      { label: "PMU tdie2", celsius: 60 },
      { label: "gas gauge battery", celsius: 30 },
    ]);
    expect(g.map((x) => x.key)).toEqual(["cpu", "battery", "storage"]);
    expect(g[0]).toMatchObject({ max: 60, avg: 55 });
    expect(g[0].sensors).toHaveLength(2);
  });
  it("no sensors, no groups", () => {
    expect(groupTemps([])).toEqual([]);
  });
});

describe("visibleDisks", () => {
  const apfs = { total: 494, available: 5 };
  it("shows one row per volume, the shortest mount point", () => {
    const d = visibleDisks([
      { name: "Macintosh HD", mount: "/System/Volumes/Data", ...apfs },
      { name: "Macintosh HD", mount: "/", ...apfs },
      { name: "Untitled", mount: "/Volumes/Untitled", ...apfs },
      { name: "USB", mount: "/Volumes/USB", total: 123, available: 33 },
    ]);
    expect(d.map((x) => x.mount)).toEqual(["/", "/Volumes/USB"]);
  });
  it("drops pseudo volumes without a size", () => {
    expect(visibleDisks([{ name: "x", mount: "/dev", total: 0, available: 0 }])).toEqual([]);
  });
});

describe("rolling window", () => {
  it("keeps the newest values", () => {
    expect(pushSample([1, 2, 3], 4, 3)).toEqual([2, 3, 4]);
    expect(pushSample([], 1, 3)).toEqual([1]);
    expect(pushSample([1], Number.NaN, 3)).toEqual([1, 0]);
  });
  it("does not change the window it was given", () => {
    const buf = [1, 2];
    pushSample(buf, 3, 2);
    expect(buf).toEqual([1, 2]);
  });
});

describe("sparkPaths", () => {
  it("needs two readings", () => {
    expect(sparkPaths([5], 10, 100, 20)).toEqual({ line: "", area: "" });
  });
  it("puts the newest value on the right edge and never stretches a short window", () => {
    // 3 of 11 slots at 10 px each: the curve starts at x = 80.
    const { line, area } = sparkPaths([0, 50, 100], 11, 100, 20, 100);
    expect(line).toBe("M80.0,20.0 L90.0,10.0 L100.0,0.0");
    expect(area).toBe("M80.0,20.0 L90.0,10.0 L100.0,0.0 L100.0,20 L80.0,20 Z");
  });
  it("scales to the largest value without a fixed top, and survives all zeros", () => {
    expect(sparkPaths([0, 4, 2], 3, 100, 20).line).toBe("M0.0,20.0 L50.0,0.0 L100.0,10.0");
    expect(sparkPaths([0, 0], 2, 100, 20).line).toBe("M0.0,20.0 L100.0,20.0");
  });
  it("clamps values above the fixed top", () => {
    expect(sparkPaths([0, 250], 2, 100, 20, 100).line).toBe("M0.0,20.0 L100.0,0.0");
  });
});

describe("keys, bars, folding", () => {
  it("a digit jumps to that card, anything else to none", () => {
    const cards = ["cpu", "memory", "network"];
    expect(cardForKey("1", cards)).toBe("cpu");
    expect(cardForKey("3", cards)).toBe("network");
    expect(cardForKey("4", cards)).toBeNull();
    expect(cardForKey("0", cards)).toBeNull();
    expect(cardForKey("a", cards)).toBeNull();
    expect(cardForKey("12", cards)).toBeNull();
  });
  it("list bars are relative to the top row", () => {
    expect(shareOfTop(50, 200)).toBe(25);
    expect(shareOfTop(300, 200)).toBe(100);
    expect(shareOfTop(5, 0)).toBe(0);
    expect(shareOfTop(-1, 10)).toBe(0);
  });
  it("stacked parts become running totals", () => {
    expect(stackUpTo([10, 20, 5])).toEqual([10, 30, 35]);
    expect(stackUpTo([10, -4, Number.NaN, 5])).toEqual([10, 10, 10, 15]);
    expect(stackUpTo([])).toEqual([]);
  });
  it("folding toggles and the stored list survives junk", () => {
    expect(toggleCollapsed([], "cpu")).toEqual(["cpu"]);
    expect(toggleCollapsed(["cpu", "host"], "cpu")).toEqual(["host"]);
    expect(parseCollapsed('["cpu","host"]')).toEqual(["cpu", "host"]);
    expect(parseCollapsed('["cpu",3,null]')).toEqual(["cpu"]);
    expect(parseCollapsed("{}")).toEqual([]);
    expect(parseCollapsed("not json")).toEqual([]);
    expect(parseCollapsed(null)).toEqual([]);
  });
});
