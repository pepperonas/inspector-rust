import { describe, expect, it } from "vitest";
import {
  basename,
  categorySummaries,
  filterPlan,
  selectionTotals,
  topItems,
} from "./clean-select";
import type { CleanPlan } from "./ipc";

const plan: CleanPlan = {
  items: [
    { path: "/tmp/a.log", size: 5, category: "logs" },
    { path: "/tmp/b.log", size: 3, category: "logs" },
    { path: "/caches/big.bin", size: 100, category: "caches" },
    { path: "/caches/small.bin", size: 1, category: "caches" },
    { path: "/caches/mid.bin", size: 50, category: "caches" },
  ],
  total_bytes: 159,
  categories: [
    ["logs", "Logs", 8],
    ["caches", "Caches", 151],
    ["empty", "Empty", 0],
  ],
};

describe("categorySummaries", () => {
  it("summarises per category, largest first, dropping empty ones", () => {
    const s = categorySummaries(plan);
    expect(s.map((c) => c.key)).toEqual(["caches", "logs"]);
    expect(s[0]).toEqual({ key: "caches", label: "Caches", bytes: 151, count: 3 });
    expect(s[1].count).toBe(2);
  });
});

describe("topItems", () => {
  it("returns the n largest items of one category", () => {
    const top = topItems(plan, "caches", 2);
    expect(top.map((i) => i.size)).toEqual([100, 50]);
  });
  it("is empty for an unknown category", () => {
    expect(topItems(plan, "nope", 3)).toEqual([]);
  });
});

describe("filterPlan", () => {
  it("keeps only selected categories and recomputes totals", () => {
    const f = filterPlan(plan, new Set(["logs"]));
    expect(f.items).toHaveLength(2);
    expect(f.total_bytes).toBe(8);
    expect(f.categories).toEqual([["logs", "Logs", 8]]);
  });
  it("empty selection yields an empty plan", () => {
    const f = filterPlan(plan, new Set());
    expect(f.items).toHaveLength(0);
    expect(f.total_bytes).toBe(0);
  });
});

describe("selectionTotals", () => {
  it("sums files + bytes across the selected summaries", () => {
    const s = categorySummaries(plan);
    expect(selectionTotals(s, new Set(["caches", "logs"]))).toEqual({ files: 5, bytes: 159 });
    expect(selectionTotals(s, new Set(["logs"]))).toEqual({ files: 2, bytes: 8 });
    expect(selectionTotals(s, new Set())).toEqual({ files: 0, bytes: 0 });
  });
});

describe("basename", () => {
  it("takes the last segment of unix and windows paths", () => {
    expect(basename("/a/b/c.txt")).toBe("c.txt");
    expect(basename("C:\\Users\\x\\cache.bin")).toBe("cache.bin");
    expect(basename("plain.txt")).toBe("plain.txt");
  });
});
