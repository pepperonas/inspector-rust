import { describe, it, expect, afterEach } from "vitest";
import { renderHook, cleanup } from "@testing-library/react";
import { useFuzzySearch } from "./useFuzzySearch";
import type { ClipEntry } from "../lib/types";

afterEach(cleanup);

/** Minimal ClipEntry factory — only fills the field the hook reads
 *  (`content_text`) plus an `id` to identify rows in assertions. */
function clip(id: number, text: string): ClipEntry {
  return {
    id,
    content_type: "text",
    content_text: text,
    content_data: text,
    hash: `h${id}`,
    byte_size: text.length,
    created_at: 0,
    last_used_at: 0,
    pinned: false,
    note: null,
  };
}

const entries: ClipEntry[] = [
  clip(1, "hello world"),
  clip(2, "rust testing rocks"),
  clip(3, "typescript code"),
  clip(4, "fuzzy matching"),
  clip(5, "rusty crusty"),
];

const idsOf = (rows: ClipEntry[]) => rows.map((r) => r.id);

describe("useFuzzySearch", () => {
  it("returns the entries unchanged for an empty query", () => {
    const { result } = renderHook(() => useFuzzySearch(entries, ""));
    expect(result.current).toBe(entries);
  });

  it("treats a whitespace-only query as empty", () => {
    const { result } = renderHook(() => useFuzzySearch(entries, "   "));
    expect(result.current).toBe(entries);
  });

  it("filters by content_text on an exact substring", () => {
    const { result } = renderHook(() => useFuzzySearch(entries, "rust"));
    const ids = idsOf(result.current);
    // Both "rust testing rocks" and "rusty crusty" should match.
    expect(ids).toContain(2);
    expect(ids).toContain(5);
    expect(ids).not.toContain(1);
    expect(ids).not.toContain(3);
  });

  it("does NOT fuzzy-match — substring only (fuzzy is reserved for commands + apps)", () => {
    // "tscrpt" is a non-contiguous subsequence of "typescript code" but not a
    // substring, so a clip must NOT surface for it.
    const { result } = renderHook(() => useFuzzySearch(entries, "tscrpt"));
    expect(idsOf(result.current)).not.toContain(3);
    expect(result.current).toEqual([]);
  });

  it("ranks prefix matches ahead of interior substring matches", () => {
    // "rust testing rocks" (prefix) should come before "crust"-style interior
    // hits; both surface, prefix first.
    const rows = [clip(10, "crusty bread"), clip(11, "rust first"), ...entries];
    const { result } = renderHook(() => useFuzzySearch(rows, "rust"));
    const ids = idsOf(result.current);
    // 11 ("rust first") + 2 ("rust testing rocks") + 5 ("rusty crusty") are
    // prefix matches; 10 ("crusty bread") is an interior match → comes last.
    expect(ids.indexOf(11)).toBeLessThan(ids.indexOf(10));
    expect(ids).toContain(10);
  });

  it("returns nothing for a query with no plausible match", () => {
    const { result } = renderHook(() => useFuzzySearch(entries, "zzzqxxq"));
    expect(result.current).toEqual([]);
  });

  it("returns the same array reference for identical inputs (memoisation)", () => {
    // The hook's useMemo cache should hold across re-renders that pass
    // the exact same entries + query — avoids re-running Fuse and
    // re-allocating result arrays on every parent re-render.
    const { result, rerender } = renderHook(
      ({ q }) => useFuzzySearch(entries, q),
      { initialProps: { q: "rust" } },
    );
    const first = result.current;
    rerender({ q: "rust" });
    expect(result.current).toBe(first);
  });

  it("recomputes when the query changes", () => {
    const { result, rerender } = renderHook(
      ({ q }) => useFuzzySearch(entries, q),
      { initialProps: { q: "rust" } },
    );
    const firstIds = idsOf(result.current);
    rerender({ q: "typescript" });
    const secondIds = idsOf(result.current);
    expect(firstIds).not.toEqual(secondIds);
    expect(secondIds).toContain(3);
  });

  it("handles an empty entry list without crashing", () => {
    const { result } = renderHook(() => useFuzzySearch([], "anything"));
    expect(result.current).toEqual([]);
  });
});
