import { describe, it, expect } from "vitest";
import { sameSnippetList } from "./snippet-list";
import type { Snippet } from "./types";

const snip = (id: number, over: Partial<Snippet> = {}): Snippet => ({
  id,
  abbreviation: `a${id}`,
  title: `t${id}`,
  body: `b${id}`,
  created_at: 1,
  updated_at: 1,
  ...over,
});

describe("sameSnippetList", () => {
  it("treats two empty results as the same (no rebuild for a fresh [])", () => {
    expect(sameSnippetList([], [])).toBe(true);
  });
  it("equal content in a new array is the same", () => {
    expect(sameSnippetList([snip(1), snip(2)], [snip(1), snip(2)])).toBe(true);
  });
  it("an edited body is a change (else the preview shows a stale snippet)", () => {
    expect(sameSnippetList([snip(1)], [snip(1, { body: "new", updated_at: 2 })])).toBe(false);
    expect(sameSnippetList([snip(1)], [snip(1, { body: "new" })])).toBe(false);
  });
  it("a regroup is a change", () => {
    expect(sameSnippetList([snip(1)], [snip(1, { category: "AI" })])).toBe(false);
  });
  it("order and length matter", () => {
    expect(sameSnippetList([snip(1), snip(2)], [snip(2), snip(1)])).toBe(false);
    expect(sameSnippetList([snip(1)], [snip(1), snip(2)])).toBe(false);
  });
});
