import { describe, it, expect } from "vitest";
import { computeLineage, laneCount, derivedKindLabel, LINEAGE_COLORS } from "./lineage";
import type { ClipEntry } from "./types";

/** Minimal clip stub — only the fields `computeLineage` reads matter. */
function clip(id: number, derivedFrom: number | null = null): ClipEntry {
  return {
    id,
    content_type: "text",
    content_text: `clip ${id}`,
    content_data: `clip ${id}`,
    hash: `h${id}`,
    byte_size: 6,
    created_at: 0,
    last_used_at: 0,
    pinned: false,
    note: null,
    derived_from: derivedFrom,
    derived_kind: derivedFrom == null ? null : "upper",
  };
}

describe("computeLineage", () => {
  it("returns nothing when no clip was derived", () => {
    const rails = computeLineage([clip(1), clip(2), clip(3)]);
    expect(rails.size).toBe(0);
    expect(laneCount(rails)).toBe(0);
  });

  it("connects a derived copy to its source through the rows in between", () => {
    // 0: the copy (newest, top of the list) — 2: the original it came from.
    const rails = computeLineage([clip(10, 30), clip(20), clip(30)]);

    expect(rails.get(10)).toEqual([{ lane: 0, color: LINEAGE_COLORS[0], node: true }]);
    // The row in between is only crossed by the lane, it is not a member.
    expect(rails.get(20)).toEqual([{ lane: 0, color: LINEAGE_COLORS[0], node: false }]);
    expect(rails.get(30)).toEqual([{ lane: 0, color: LINEAGE_COLORS[0], node: true }]);
  });

  it("draws a lone node when the source is no longer in the list", () => {
    // The original was pruned/deleted — the copy still shows it is a copy.
    const rails = computeLineage([clip(10, 999), clip(20)]);
    expect(rails.get(10)).toEqual([{ lane: 0, color: LINEAGE_COLORS[0], node: true }]);
    expect(rails.has(20)).toBe(false);
  });

  it("shows nothing on a clip whose only descendants are off-list", () => {
    // Having produced a copy somewhere is not itself a lineage to draw.
    const rails = computeLineage([clip(1), clip(2), clip(3)]);
    expect(rails.has(2)).toBe(false);
  });

  it("gives overlapping lineages distinct lanes and colours", () => {
    //  0: copy A ─┐
    //  1: copy B ─┼┐
    //  2: src  A ─┘│
    //  3: src  B ──┘
    const rails = computeLineage([clip(1, 3), clip(2, 4), clip(3), clip(4)]);

    // A row can carry several rails (its own + lanes merely passing through);
    // the lane that *belongs* to the clip is the one it is a node on.
    const laneOf = (id: number) => rails.get(id)!.find((r) => r.node)!.lane;
    expect(laneOf(1)).toBe(0);
    expect(laneOf(2)).toBe(1);
    expect(laneOf(3)).toBe(0);
    expect(laneOf(4)).toBe(1);
    const colorOf = (id: number) => rails.get(id)!.find((r) => r.node)!.color;
    expect(colorOf(1)).not.toBe(colorOf(2));
    expect(laneCount(rails)).toBe(2);

    // Row 1 is crossed by lane 0 (A's rail) *and* is a member of lane 1 (B).
    expect(rails.get(2)).toEqual([
      { lane: 0, color: LINEAGE_COLORS[0], node: false },
      { lane: 1, color: LINEAGE_COLORS[1], node: true },
    ]);
  });

  it("reuses the innermost lane for lineages that do not overlap", () => {
    // Two separate families, one entirely above the other.
    const rails = computeLineage([clip(1, 2), clip(2), clip(3, 4), clip(4)]);
    expect(rails.get(1)![0].lane).toBe(0);
    expect(rails.get(3)![0].lane).toBe(0);
    expect(laneCount(rails)).toBe(1);
  });

  it("keeps a chain of copies in a single lane", () => {
    // C derived from B derived from A — one family, one colour.
    const rails = computeLineage([clip(3, 2), clip(2, 1), clip(1)]);
    for (const id of [3, 2, 1]) {
      expect(rails.get(id)).toEqual([{ lane: 0, color: LINEAGE_COLORS[0], node: true }]);
    }
    expect(laneCount(rails)).toBe(1);
  });

  it("spans correctly when the source sits ABOVE its copy", () => {
    // Pasting the original bumps it back to the top, so the rail runs the
    // other way — the span must not assume the source is the lower row.
    const rails = computeLineage([clip(1), clip(2), clip(3, 1)]);
    expect(rails.get(1)).toEqual([{ lane: 0, color: LINEAGE_COLORS[0], node: true }]);
    expect(rails.get(2)).toEqual([{ lane: 0, color: LINEAGE_COLORS[0], node: false }]);
    expect(rails.get(3)).toEqual([{ lane: 0, color: LINEAGE_COLORS[0], node: true }]);
  });

  it("gathers one source and its several copies into one family", () => {
    // Same clip copied as upper-case and as base64 → both hang off one rail.
    const rails = computeLineage([clip(1, 3), clip(2, 3), clip(3)]);
    expect(laneCount(rails)).toBe(1);
    for (const id of [1, 2, 3]) expect(rails.get(id)![0].node).toBe(true);
  });

  it("wraps around the palette when more lanes than colours are needed", () => {
    // Seven mutually overlapping families (6 colours available).
    const entries: ClipEntry[] = [];
    for (let i = 0; i < 7; i++) entries.push(clip(100 + i, 200 + i));
    for (let i = 0; i < 7; i++) entries.push(clip(200 + i));
    const rails = computeLineage(entries);
    expect(laneCount(rails)).toBe(7);
    expect(rails.get(106)![0].color).toBe(LINEAGE_COLORS[6 % LINEAGE_COLORS.length]);
  });

  it("keeps positions aligned when non-clip rows are interleaved", () => {
    // Command / snippet rows are passed as null so a lane's span still matches
    // the rendered row positions; they can't carry a rail themselves.
    const rails = computeLineage([null, clip(10, 30), null, clip(30)]);
    expect(rails.get(10)![0].node).toBe(true);
    expect(rails.get(30)![0].node).toBe(true);
    expect(rails.size).toBe(2);
  });

  it("handles an empty list", () => {
    expect(computeLineage([]).size).toBe(0);
  });

  it("survives a self-referential derived_from without hanging", () => {
    // Defensive: a corrupt row pointing at itself must not spin the union-find.
    const rails = computeLineage([clip(1, 1), clip(2)]);
    expect(rails.get(1)![0].node).toBe(true);
  });
});

describe("derivedKindLabel", () => {
  it("uses the transform catalogue's own label", () => {
    expect(derivedKindLabel("upper")).toBe("UPPERCASE");
    expect(derivedKindLabel("base64-encode")).toBe("Base64 encode");
    expect(derivedKindLabel("plain-text")).toBe("Plain text");
  });

  it("falls back to a generic label / the raw kind", () => {
    expect(derivedKindLabel(null)).toBe("Copied from another entry");
    expect(derivedKindLabel(undefined)).toBe("Copied from another entry");
    expect(derivedKindLabel("something-new")).toBe("something-new");
  });
});
