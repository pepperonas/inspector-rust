import { describe, it, expect } from "vitest";
import { pinnedClips } from "./history-filter";
import type { ClipEntry } from "./types";

function clip(id: number, pinned: boolean): ClipEntry {
  return {
    id,
    content_type: "text",
    content_text: `clip ${id}`,
    content_data: `clip ${id}`,
    hash: `h${id}`,
    byte_size: 6,
    created_at: 0,
    last_used_at: 0,
    pinned,
    note: null,
    derived_from: null,
    derived_kind: null,
  };
}

describe("pinnedClips", () => {
  it("keeps only the pinned clips", () => {
    const out = pinnedClips([clip(1, false), clip(2, true), clip(3, false), clip(4, true)]);
    expect(out.map((c) => c.id)).toEqual([2, 4]);
  });

  it("preserves the incoming order (backend pinned/recency order)", () => {
    // Already ordered pinned-first by the backend; the filter must not reorder.
    const out = pinnedClips([clip(5, true), clip(6, true), clip(1, false)]);
    expect(out.map((c) => c.id)).toEqual([5, 6]);
  });

  it("returns an empty list when nothing is pinned", () => {
    expect(pinnedClips([clip(1, false), clip(2, false)])).toEqual([]);
    expect(pinnedClips([])).toEqual([]);
  });

  it("does not mutate the input", () => {
    const input = [clip(1, true), clip(2, false)];
    const snapshot = input.map((c) => c.id);
    pinnedClips(input);
    expect(input.map((c) => c.id)).toEqual(snapshot);
  });
});
