import { describe, it, expect } from "vitest";
import {
  sunburstArcs,
  arcPath,
  nodeAt,
  segmentColor,
  topHue,
  formatBytes,
  formatPct,
  baseName,
  parentPath,
  joinPath,
  pathCrumbs,
  childRows,
  type DiskNode,
} from "./disk";

const leaf = (name: string, size: number): DiskNode => ({
  name,
  size,
  is_dir: false,
  child_count: 0,
});
const dir = (name: string, children: DiskNode[]): DiskNode => ({
  name,
  size: children.reduce((s, c) => s + c.size, 0),
  is_dir: true,
  child_count: children.length,
  children,
});

const OPTS = { hubR: 40, ring: 22, rings: 4 };

describe("sunburstArcs", () => {
  it("children of the root fill the full circle proportional to size", () => {
    const root = dir("root", [leaf("a", 75), leaf("b", 25)]);
    const arcs = sunburstArcs(root, OPTS);
    expect(arcs).toHaveLength(2);
    const a = arcs.find((x) => x.node.name === "a")!;
    const b = arcs.find((x) => x.node.name === "b")!;
    // a is 75 % → three times b's span; together they span 2π.
    expect(a.a1 - a.a0).toBeCloseTo(Math.PI * 2 * 0.75, 5);
    expect(b.a1 - b.a0).toBeCloseTo(Math.PI * 2 * 0.25, 5);
    expect(a.a1 - a.a0 + (b.a1 - b.a0)).toBeCloseTo(Math.PI * 2, 5);
  });

  it("nests children within the parent's angular span on the next ring", () => {
    const root = dir("root", [dir("big", [leaf("x", 50), leaf("y", 50)]), leaf("small", 100)]);
    const arcs = sunburstArcs(root, OPTS);
    const big = arcs.find((a) => a.node.name === "big")!;
    const x = arcs.find((a) => a.node.name === "x")!;
    const y = arcs.find((a) => a.node.name === "y")!;
    // x and y live on ring 1, inside big's span, and together fill it.
    expect(x.depth).toBe(1);
    expect(x.r0).toBe(OPTS.hubR + OPTS.ring);
    expect(x.a0).toBeGreaterThanOrEqual(big.a0 - 1e-9);
    expect(y.a1).toBeCloseTo(big.a1, 5);
    expect(x.a1).toBeCloseTo(y.a0, 5);
  });

  it("respects the ring limit and drops sub-minAngle slivers", () => {
    const root = dir("root", [leaf("huge", 10000), leaf("dust", 1)]);
    const arcs = sunburstArcs(root, { ...OPTS, minAngle: 0.05 });
    // "dust" is far below minAngle → dropped.
    expect(arcs.map((a) => a.node.name)).toEqual(["huge"]);
  });

  it("carries the index path for drill-down identity", () => {
    const root = dir("root", [dir("d", [leaf("z", 10)])]);
    const arcs = sunburstArcs(root, OPTS);
    const z = arcs.find((a) => a.node.name === "z")!;
    expect(z.path).toEqual([0, 0]);
    expect(nodeAt(root, z.path)?.name).toBe("z");
    expect(nodeAt(root, [9])).toBeNull();
  });
});

describe("arcPath", () => {
  const base = { path: [0], node: leaf("x", 1), depth: 0, r0: 40, r1: 62, color: "#000" };

  it("emits a closed annular sector", () => {
    const d = arcPath({ ...base, a0: 0, a1: Math.PI / 2 }, 100, 100);
    expect(d.startsWith("M ")).toBe(true);
    expect(d).toContain("A ");
    expect(d.trimEnd().endsWith("Z")).toBe(true);
  });

  it("splits a full circle into two arcs (the 100 % donut bug)", () => {
    const d = arcPath({ ...base, a0: 0, a1: Math.PI * 2 }, 100, 100);
    expect((d.match(/A /g) ?? []).length).toBeGreaterThanOrEqual(4);
  });

  it("returns empty when the gap collapses the segment", () => {
    expect(arcPath({ ...base, a0: 0, a1: 0.005 }, 100, 100, 0.01)).toBe("");
  });
});

describe("colours", () => {
  it("top hues are distinct and in range", () => {
    const hues = [0, 1, 2, 3, 4].map((i) => topHue(i, 5));
    for (const h of hues) {
      expect(h).toBeGreaterThanOrEqual(0);
      expect(h).toBeLessThan(360);
    }
    expect(new Set(hues.map(Math.round)).size).toBe(5);
  });

  it("outer rings are lighter (same family)", () => {
    const light = (d: number) => Number(segmentColor(200, d, 0).match(/(\d+)%\)$/)![1]);
    expect(light(1)).toBeGreaterThan(light(0));
    expect(light(2)).toBeGreaterThan(light(1));
  });
});

describe("formatters", () => {
  it("formatBytes is DaisyDisk-style", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(2048)).toBe("2 KB");
    expect(formatBytes(5 * 1024 ** 3)).toBe("5.0 GB");
  });
  it("formatPct never shows -0 and is fine at the edges", () => {
    expect(formatPct(0, 100)).toBe("0 %");
    expect(formatPct(1, 0)).toBe("0 %");
    expect(formatPct(50, 100)).toBe("50 %");
    expect(formatPct(3, 100)).toBe("3.0 %");
  });
  it("baseName takes the last path segment", () => {
    expect(baseName("/Users/martin/big.bin")).toBe("big.bin");
    expect(baseName("/Users/martin/dir/")).toBe("dir");
  });
});

describe("navigation paths", () => {
  it("parentPath climbs one level and STOPS at the root", () => {
    expect(parentPath("/Users/martin/claude")).toBe("/Users/martin");
    expect(parentPath("/Users/martin")).toBe("/Users");
    expect(parentPath("/Users")).toBe("/");
    // The null is load-bearing: it's what keeps "go up" from walking off the
    // top of the filesystem into a nonsense path.
    expect(parentPath("/")).toBeNull();
    expect(parentPath("")).toBeNull();
  });

  it("parentPath tolerates a trailing slash", () => {
    expect(parentPath("/Users/martin/")).toBe("/Users");
  });

  it("joinPath never doubles or drops a separator", () => {
    expect(joinPath("/Users/martin", ["claude", "src"])).toBe("/Users/martin/claude/src");
    expect(joinPath("/Users/martin/", ["claude"])).toBe("/Users/martin/claude");
    expect(joinPath("/Users/martin", [])).toBe("/Users/martin");
    // From the filesystem root the base is empty — "//Users" would be wrong.
    expect(joinPath("/", ["Users"])).toBe("/Users");
    expect(joinPath("/", [])).toBe("/");
  });

  it("pathCrumbs spells out the whole absolute path", () => {
    const c = pathCrumbs("/Users/martin", ["claude", "inspector-rust"]);
    expect(c.map((x) => x.name)).toEqual(["/", "Users", "martin", "claude", "inspector-rust"]);
    expect(c.map((x) => x.path)).toEqual([
      "/",
      "/Users",
      "/Users/martin",
      "/Users/martin/claude",
      "/Users/martin/claude/inspector-rust",
    ]);
  });

  it("marks which crumbs are reachable in-tree and which need a scan", () => {
    const c = pathCrumbs("/Users/martin", ["claude"]);
    // Above the scan root → null → clicking re-scans there.
    expect(c[0].steps).toBeNull(); // /
    expect(c[1].steps).toBeNull(); // /Users
    // The scan root itself and every drill step are instant.
    expect(c[2].steps).toBe(0); // /Users/martin  = the root
    expect(c[3].steps).toBe(1); // one drill step in
  });

  it("scanning the filesystem root yields a single, in-tree crumb", () => {
    const c = pathCrumbs("/", []);
    expect(c).toHaveLength(1);
    expect(c[0]).toEqual({ name: "/", path: "/", steps: 0 });
  });

  it("a trailing slash on the root does not produce an empty crumb", () => {
    const c = pathCrumbs("/Users/martin/", []);
    expect(c.map((x) => x.name)).toEqual(["/", "Users", "martin"]);
    expect(c[c.length - 1].steps).toBe(0);
  });

  it("the last crumb always addresses the currently shown directory", () => {
    for (const drill of [[], ["a"], ["a", "b", "c"]]) {
      const c = pathCrumbs("/Users/martin", drill);
      const last = c[c.length - 1];
      expect(last.steps).toBe(drill.length);
      expect(last.path).toBe(joinPath("/Users/martin", drill));
    }
  });
});

describe("childRows — reaching the folders the chart cannot show", () => {
  const dir = (name: string, size: number): DiskNode => ({
    name,
    size,
    is_dir: true,
    child_count: 0,
  });

  it("keeps even a hairline child, and keeps its drill index", () => {
    // The real case: 2 MB of source next to 20 GB of build output. Its arc is
    // sub-pixel — the list is the only way in.
    const focus: DiskNode = {
      ...dir("projekt", 20_002_000_000),
      children: [dir("src", 2_000_000), dir("target", 20_000_000_000)],
    };
    const rows = childRows(focus);
    expect(rows).toHaveLength(2);
    expect(rows[0].node.name).toBe("target"); // largest first
    expect(rows[1].node.name).toBe("src");
    // …and `src` still addresses child 0, whatever the sort did.
    expect(rows[1].index).toBe(0);
    expect(rows[1].share).toBeLessThan(0.001);
  });

  it("shows the synthetic remainder too rather than hiding it", () => {
    const focus: DiskNode = {
      ...dir("x", 300),
      children: [dir("a", 200), { ...dir("rest", 100), other: true }],
    };
    expect(childRows(focus).map((r) => r.node.name)).toEqual(["a", "rest"]);
  });

  it("ties break by name, so the order never wobbles between renders", () => {
    const focus: DiskNode = {
      ...dir("x", 300),
      children: [dir("b", 100), dir("a", 100), dir("c", 100)],
    };
    expect(childRows(focus).map((r) => r.node.name)).toEqual(["a", "b", "c"]);
  });

  it("a leaf has no rows and never divides by zero", () => {
    expect(childRows(dir("leer", 0))).toEqual([]);
    const zero: DiskNode = { ...dir("x", 0), children: [dir("a", 0)] };
    expect(childRows(zero)[0].share).toBe(0);
  });
});
