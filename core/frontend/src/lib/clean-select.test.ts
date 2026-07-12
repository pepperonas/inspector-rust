import { describe, expect, it } from "vitest";
import {
  basename,
  buildRows,
  categorySummaries,
  dirsOfCategory,
  prettyPath,
  selectionTotals,
} from "./clean-select";
import type { CleanPlanView } from "./ipc";

/** What a scan hands the panel: directory rows + per-category totals. */
const view: CleanPlanView = {
  dirs: [
    { path: "/caches/big", size: 100, count: 3, category: "caches" },
    { path: "/caches/mid", size: 50, count: 2, category: "caches" },
    { path: "/caches/small", size: 1, count: 1, category: "caches" },
    { path: "/tmp/logs", size: 8, count: 2, category: "logs" },
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
    const s = categorySummaries(view);
    expect(s.map((c) => c.key)).toEqual(["caches", "logs"]);
    expect(s[0]).toEqual({ key: "caches", label: "Caches", bytes: 151, count: 6 });
    expect(s[1].count).toBe(2);
  });
});

describe("buildRows", () => {
  it("groups directories under their category, both largest-first", () => {
    const rows = buildRows(view);
    expect(rows.map((r) => (r.kind === "header" ? `# ${r.key}` : r.dir.path))).toEqual([
      "# caches", // biggest category first
      "/caches/big",
      "/caches/mid",
      "/caches/small",
      "# logs",
      "/tmp/logs",
    ]);
  });

  it("reports how many folders sit under each header", () => {
    const header = buildRows(view).find((r) => r.kind === "header" && r.key === "caches");
    expect(header).toMatchObject({ kind: "header", dirs: 3, bytes: 151 });
  });

  it("is empty for an empty scan", () => {
    expect(buildRows({ dirs: [], total_bytes: 0, categories: [] })).toEqual([]);
  });
});

describe("dirsOfCategory", () => {
  it("lists every directory path of one category (the header's group toggle)", () => {
    expect(dirsOfCategory(view, "caches")).toEqual([
      "/caches/big",
      "/caches/mid",
      "/caches/small",
    ]);
    expect(dirsOfCategory(view, "nope")).toEqual([]);
  });
});

describe("selectionTotals", () => {
  it("sums files + bytes across the ticked directories", () => {
    expect(selectionTotals(view.dirs, new Set(["/caches/big", "/tmp/logs"]))).toEqual({
      files: 5,
      bytes: 108,
    });
    expect(selectionTotals(view.dirs, new Set())).toEqual({ files: 0, bytes: 0 });
  });

  it("ignores selections that aren't in the scan", () => {
    expect(selectionTotals(view.dirs, new Set(["/gone"]))).toEqual({ files: 0, bytes: 0 });
  });
});

describe("basename", () => {
  it("takes the last segment of unix and windows paths", () => {
    expect(basename("/a/b/c.txt")).toBe("c.txt");
    expect(basename("C:\\Users\\x\\cache.bin")).toBe("cache.bin");
  });

  it("leaves a command pseudo-item's sentence intact", () => {
    // These rows carry a human label, not a path — chopping at the last slash
    // would mangle them.
    const label = "Docker build cache — freed via `docker builder prune`";
    expect(basename(label)).toBe(label);
  });
});

describe("prettyPath", () => {
  it("shortens the home prefix", () => {
    expect(prettyPath("/Users/x/Library/Caches/foo", "/Users/x")).toBe("~/Library/Caches/foo");
    expect(prettyPath("/opt/other", "/Users/x")).toBe("/opt/other");
    expect(prettyPath("/Users/x/a")).toBe("/Users/x/a");
  });
});
