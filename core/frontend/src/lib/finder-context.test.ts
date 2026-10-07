import { describe, it, expect } from "vitest";
import { contextTitle, selectionLabel, shortDir, tildePath } from "./finder-context";
import type { FinderContext } from "./ipc";

const ctx = (dir: string, selected: string[], selected_count = selected.length): FinderContext => ({
  dir,
  selected,
  selected_count,
  from_selection: selected.length > 0,
});

describe("paths", () => {
  it("replaces the home folder with ~", () => {
    expect(tildePath("/Users/martin/claude/docs/")).toBe("~/claude/docs");
    expect(tildePath("/Users/martin")).toBe("~");
    expect(tildePath("/Volumes/Samsung SSD/")).toBe("/Volumes/Samsung SSD");
    expect(tildePath("C:\\Users\\anna\\Desktop")).toBe("~\\Desktop");
    expect(tildePath("/")).toBe("/");
  });
  it("keeps the last two folders of a deep path", () => {
    expect(shortDir("/Users/martin/claude/inspector-rust/docs/")).toBe("…/inspector-rust/docs");
    expect(shortDir("/Users/martin/Desktop/")).toBe("~/Desktop");
    expect(shortDir("/Volumes/Samsung SSD/")).toBe("/Volumes/Samsung SSD");
    expect(shortDir("/")).toBe("/");
  });
});

describe("selectionLabel", () => {
  it("names a single file, counts several, stays quiet for none", () => {
    expect(selectionLabel(ctx("/Users/m/docs", ["/Users/m/docs/notiz.md"]))).toBe("notiz.md");
    expect(selectionLabel(ctx("/Users/m/docs", ["/a", "/b"], 37))).toBe("37 ausgewählt");
    expect(selectionLabel(ctx("/Users/m/docs", []))).toBe("");
  });
  it("a lone selected folder IS the working folder — not repeated", () => {
    expect(selectionLabel(ctx("/Users/m/docs/sub", ["/Users/m/docs/sub/"]))).toBe("");
  });
});

describe("contextTitle", () => {
  it("shows the full folder and up to five selected names", () => {
    const t = contextTitle(ctx("/Users/m/docs", ["/Users/m/docs/a.md", "/Users/m/docs/b.md"], 7));
    expect(t).toContain("~/docs");
    expect(t).toContain("• a.md");
    expect(t).toContain("Ausgewählt (7)");
    expect(contextTitle(ctx("/Users/m/Desktop", []))).toContain("Nichts ausgewählt");
  });
});
