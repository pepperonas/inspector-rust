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

  it("only collapses at a path-segment boundary (sibling users stay intact)", () => {
    // `/Users/martina` must NOT render as `~a` under home `/Users/martin`.
    expect(prettyPath("/Users/martina/foo", "/Users/martin")).toBe("/Users/martina/foo");
    expect(prettyPath("/Users/martin/foo", "/Users/martin")).toBe("~/foo");
  });

  it("home itself renders as ~", () => {
    expect(prettyPath("/Users/x", "/Users/x")).toBe("~");
  });

  it("a root home never collapses everything", () => {
    expect(prettyPath("/anything/here", "/")).toBe("/anything/here");
  });

  it("handles Windows-style separators at the boundary", () => {
    expect(prettyPath("C:\\Users\\x\\cache", "C:\\Users\\x")).toBe("~\\cache");
    expect(prettyPath("C:\\Users\\xy\\cache", "C:\\Users\\x")).toBe("C:\\Users\\xy\\cache");
  });
});

describe("basename — more shapes", () => {
  it("relative-looking labels are returned unchanged (pseudo-item guard)", () => {
    expect(basename("foo/bar.txt")).toBe("foo/bar.txt");
    expect(basename("just a sentence")).toBe("just a sentence");
  });

  it("handles drive-letter forward-slash and single-segment paths", () => {
    expect(basename("C:/tmp/x.log")).toBe("x.log");
    expect(basename("/single")).toBe("single");
  });

  it("never returns an empty string", () => {
    for (const p of ["/", "/a/b/", "C:\\", "/a/b/c.txt", "label only"]) {
      expect(basename(p).length).toBeGreaterThan(0);
    }
  });
});

describe("buildRows / summaries — orphan-free invariants", () => {
  it("breaks size ties by path (deterministic ordering)", () => {
    const tied: CleanPlanView = {
      dirs: [
        { path: "/c/bbb", size: 10, count: 1, category: "c" },
        { path: "/c/aaa", size: 10, count: 1, category: "c" },
      ],
      total_bytes: 20,
      categories: [["c", "C", 20]],
    };
    const rows = buildRows(tied);
    expect(rows.map((r) => (r.kind === "dir" ? r.dir.path : "#"))).toEqual([
      "#",
      "/c/aaa",
      "/c/bbb",
    ]);
  });

  it("selecting every directory reproduces the scan totals", () => {
    const all = new Set(view.dirs.map((d) => d.path));
    const t = selectionTotals(view.dirs, all);
    expect(t.bytes).toBe(view.dirs.reduce((a, d) => a + d.size, 0));
    expect(t.files).toBe(view.dirs.reduce((a, d) => a + d.count, 0));
  });

  it("every dir row in buildRows sits under its own category header", () => {
    let current = "";
    for (const r of buildRows(view)) {
      if (r.kind === "header") current = r.key;
      else expect(r.dir.category).toBe(current);
    }
  });

  it("a category with bytes but no dirs never renders a header", () => {
    const v: CleanPlanView = {
      dirs: [{ path: "/x", size: 5, count: 1, category: "real" }],
      total_bytes: 5,
      categories: [
        ["real", "Real", 5],
        ["ghost", "Ghost", 999],
      ],
    };
    expect(buildRows(v).flatMap((r) => (r.kind === "header" ? [r.key] : []))).toEqual(["real"]);
  });
});
