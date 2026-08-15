import { describe, it, expect } from "vitest";
import {
  computeLineage,
  laneCount,
  derivedKindLabel,
  railGutterPx,
  visibleRails,
  LANE_W,
  MAX_LANES,
  LINEAGE_COLORS,
} from "./lineage";
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

  it("a copy of a copy of a copy is ONE family on one lane", () => {
    // 4-deep chain, newest first: 10 ← 20 ← 30 ← 40. Deep parent chains are
    // exactly where union-find path compression runs — the family must not
    // split into separate lanes/colours halfway down the chain.
    const rails = computeLineage([clip(10, 20), clip(20, 30), clip(30, 40), clip(40)]);
    for (const id of [10, 20, 30, 40]) {
      expect(rails.get(id)).toEqual([{ lane: 0, color: LINEAGE_COLORS[0], node: true }]);
    }
    expect(laneCount(rails)).toBe(1);
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

describe("computeLineage — pruning gaps", () => {
  it("does not bridge a chain whose middle link was pruned", () => {
    // A ← B ← C, but B is gone from the list. C must stand alone rather than
    // being wired to A, which it was never directly copied from.
    const rails = computeLineage([clip(3, 2), clip(1)]); // C(from B), A
    expect(rails.get(3)).toEqual([{ lane: 0, color: LINEAGE_COLORS[0], node: true }]);
    expect(rails.has(1)).toBe(false);
  });

  it("leaves rows outside a lineage's span untouched", () => {
    // Rows above and below the family must carry no rail at all.
    const rails = computeLineage([clip(9), clip(10, 30), clip(30), clip(8)]);
    expect(rails.has(9)).toBe(false);
    expect(rails.has(8)).toBe(false);
    expect(rails.size).toBe(2);
  });
});

describe("railGutterPx / visibleRails", () => {
  it("reserves exactly the width the renderer draws", () => {
    // The gutter and the drawn rails share LANE_W/MAX_LANES — if they ever
    // disagree, rails spill into the row text.
    const rails = computeLineage([clip(1, 3), clip(2, 4), clip(3), clip(4)]);
    expect(laneCount(rails)).toBe(2);
    expect(railGutterPx(rails)).toBe(2 * LANE_W);
    for (const list of rails.values()) {
      for (const r of visibleRails(list)) {
        expect(r.lane * LANE_W).toBeLessThan(railGutterPx(rails));
      }
    }
  });

  it("caps the gutter and hides the lanes that no longer fit", () => {
    // Seven overlapping families → more lanes than a row can show.
    const entries: ClipEntry[] = [];
    for (let i = 0; i < 7; i++) entries.push(clip(100 + i, 200 + i));
    for (let i = 0; i < 7; i++) entries.push(clip(200 + i));
    const rails = computeLineage(entries);

    expect(laneCount(rails)).toBe(7);
    expect(railGutterPx(rails)).toBe(MAX_LANES * LANE_W);
    // Nothing beyond the reserved gutter is drawn.
    for (const list of rails.values()) {
      for (const r of visibleRails(list)) expect(r.lane).toBeLessThan(MAX_LANES);
    }
    // The deep lane exists in the data but is not rendered.
    expect(rails.get(106)!.some((r) => r.lane >= MAX_LANES)).toBe(true);
    expect(visibleRails(rails.get(106)!).some((r) => r.lane >= MAX_LANES)).toBe(false);
  });

  it("reserves nothing when the rails are switched off", () => {
    expect(railGutterPx(null)).toBe(0);
    expect(railGutterPx(computeLineage([clip(1), clip(2)]))).toBe(0);
    expect(visibleRails(undefined)).toEqual([]);
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

describe("computeLineage — links that point in mixed directions", () => {
  // Rows are ordered by recency, so a rail can run either way: pasting the
  // original bumps it back to the top, above its own copy. That means the
  // union-find can end up merging a family whose root is discovered LAST, and
  // must still collapse it into a single lineage (rather than two half-drawn
  // ones sharing rows).
  it("a chain discovered out of order is still ONE family on ONE lane", () => {
    // row 0 = the original (id 1), row 1 = a copy of row 2, row 2 = a copy of row 0.
    const rails = computeLineage([clip(1), clip(2, 3), clip(3, 1)]);
    expect(rails.size).toBe(3);
    const lanes = [1, 2, 3].map((id) => rails.get(id)!);
    for (const r of lanes) {
      expect(r).toHaveLength(1);
      expect(r[0].lane).toBe(0);
      expect(r[0].node).toBe(true); // every row is a member of the family
    }
    // One family → one colour for the whole run.
    expect(new Set(lanes.map((r) => r[0].color)).size).toBe(1);
  });

  it("a longer zig-zag chain still collapses to a single lane", () => {
    // 0←2, 2←3, 3←1, 1←(nothing): every row belongs to the same family no
    // matter which order the edges are visited in.
    const rails = computeLineage([clip(10), clip(11), clip(12, 10), clip(13, 12)]);
    const all = [10, 11, 12, 13].map((id) => rails.get(id));
    expect(all.every((r) => r && r.length === 1)).toBe(true);
    expect(new Set(all.map((r) => r![0].lane))).toEqual(new Set([0]));
    // Row 11 sits inside the family's span but is not itself derived → a
    // through-line, not a node.
    expect(rails.get(11)![0].node).toBe(false);
    expect(rails.get(13)![0].node).toBe(true);
  });

  it("two copies of the same source, one above and one below it", () => {
    // The span must cover both directions from the source in the middle.
    const rails = computeLineage([clip(2, 1), clip(1), clip(3, 1)]);
    expect(rails.size).toBe(3);
    expect(rails.get(1)![0].node).toBe(true);
    expect(rails.get(2)![0].node).toBe(true);
    expect(rails.get(3)![0].node).toBe(true);
    expect(new Set([1, 2, 3].map((id) => rails.get(id)![0].lane))).toEqual(new Set([0]));
  });
});
