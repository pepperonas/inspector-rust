/**
 * Lineage rails for the history list (v0.93.1) — the git-graph-style coloured
 * paths that connect a *derived* copy (plain text, upper-case, base64, …) to
 * the clip it was made from.
 *
 * Copying a clip in another shape never rewrites the original: the copy becomes
 * its own entry at the top of the list while the source keeps its content and
 * its position. The rail is what makes that relationship visible — exactly like
 * a commit graph, where a branch line ties commits that belong together.
 *
 * Everything here is **pure**: entries in (already in display order), one rail
 * list per entry id out. The rendering (`HistoryItem`) only draws what this
 * returns, so the layout logic is fully unit-testable.
 */
import { TRANSFORMS } from "./text-transform";

/**
 * The only thing a row needs to expose to be laid out — `ClipEntry` satisfies
 * it structurally. Rows that aren't clips (commands, snippets, …) are passed as
 * `null` so positions stay aligned with the rendered list.
 */
export interface LineageNode {
  id: number;
  derived_from: number | null;
}

/**
 * Lane colours, in allocation order. Deliberately assigned **by lane** rather
 * than by clip id: what matters visually is that two lineages drawn next to
 * each other can never share a colour. Mid-tone hues so they read on both the
 * dark and the light palette.
 */
export const LINEAGE_COLORS = [
  "#34d399", // emerald
  "#a78bfa", // violet
  "#fbbf24", // amber
  "#38bdf8", // sky
  "#f472b6", // pink
  "#a3e635", // lime
] as const;

/** One rail segment crossing a single row. */
export interface Rail {
  /** Column, 0 = closest to the list edge. */
  lane: number;
  /** Lane colour (from {@link LINEAGE_COLORS}). */
  color: string;
  /**
   * `true` when this row is itself part of the lineage (a derived copy or the
   * source it came from) — drawn with a node dot. `false` when the lane merely
   * passes through this row on its way to a member further down.
   */
  node: boolean;
}

/** A connected lineage family, reduced to the rows it touches. */
interface Family {
  /** Positions (list indices) of the members that are actually visible. */
  members: Set<number>;
  first: number;
  last: number;
}

/**
 * Group the visible entries into lineage families and lay them out in lanes.
 *
 * Returns a map keyed by **entry id**; ids absent from the map have no rail.
 * A family is only drawn when it contains at least one derived clip — a clip
 * that merely *has* descendants somewhere off-list shows nothing on its own.
 *
 * The source of a copy is not necessarily below it: pasting the original moves
 * it back to the top, so a rail may run in either direction. Spans are
 * therefore built from `min`/`max` of the member positions, never assuming an
 * order.
 */
export function computeLineage(
  entries: readonly (LineageNode | null)[],
): Map<number, Rail[]> {
  const out = new Map<number, Rail[]>();
  if (entries.length === 0) return out;

  const posById = new Map<number, number>();
  entries.forEach((e, i) => {
    if (e) posById.set(e.id, i);
  });

  // ── 1. Build families by walking the derived_from edges ───────────────────
  // Union-find keyed on list position; only edges whose *both* ends are on
  // screen can be drawn, but a derived clip whose source is gone (pruned,
  // deleted) still gets a lone node so the user can tell it is a copy.
  const parent = entries.map((_, i) => i);
  const find = (i: number): number => {
    let root = i;
    while (parent[root] !== root) root = parent[root];
    while (parent[i] !== root) {
      const next = parent[i];
      parent[i] = root;
      i = next;
    }
    return root;
  };
  const union = (a: number, b: number) => {
    const ra = find(a);
    const rb = find(b);
    if (ra !== rb) parent[Math.max(ra, rb)] = Math.min(ra, rb);
  };

  const inLineage = new Set<number>();
  entries.forEach((e, i) => {
    if (!e || e.derived_from == null) return;
    inLineage.add(i);
    const src = posById.get(e.derived_from);
    if (src !== undefined && src !== i) {
      inLineage.add(src);
      union(i, src);
    }
  });
  if (inLineage.size === 0) return out;

  const families = new Map<number, Family>();
  for (const i of [...inLineage].sort((a, b) => a - b)) {
    const root = find(i);
    const fam = families.get(root);
    if (fam) {
      fam.members.add(i);
      fam.first = Math.min(fam.first, i);
      fam.last = Math.max(fam.last, i);
    } else {
      families.set(root, { members: new Set([i]), first: i, last: i });
    }
  }

  // ── 2. Greedy lane allocation ────────────────────────────────────────────
  // Families are placed top-down; each takes the lowest lane not occupied by a
  // family it overlaps. Non-overlapping families reuse lanes (and colours),
  // exactly like a commit graph.
  const ordered = [...families.values()].sort((a, b) => a.first - b.first || a.last - b.last);
  const laneFreeFrom: number[] = []; // laneFreeFrom[lane] = first row the lane is free again
  for (const fam of ordered) {
    let lane = laneFreeFrom.findIndex((freeFrom) => freeFrom <= fam.first);
    if (lane === -1) lane = laneFreeFrom.length;
    laneFreeFrom[lane] = fam.last + 1;

    const color = LINEAGE_COLORS[lane % LINEAGE_COLORS.length];
    for (let pos = fam.first; pos <= fam.last; pos++) {
      const row = entries[pos];
      if (!row) continue; // a non-clip row can't carry a rail
      const rail: Rail = { lane, color, node: fam.members.has(pos) };
      const id = row.id;
      const existing = out.get(id);
      if (existing) existing.push(rail);
      else out.set(id, [rail]);
    }
  }
  return out;
}

/** How many lanes the computed rails occupy — drives the gutter width. */
export function laneCount(rails: Map<number, Rail[]>): number {
  let max = -1;
  for (const list of rails.values()) {
    for (const r of list) if (r.lane > max) max = r.lane;
  }
  return max + 1;
}

/** Horizontal pitch of one lane, in px. */
export const LANE_W = 5;

/**
 * How many lanes a row can show. Every lane widens the gutter, so an absurd
 * number of concurrent lineages would eat the row; past this the deeper lanes
 * are simply not drawn. Kept here — next to {@link railGutterPx} — because the
 * gutter and the renderer must agree, or rails would spill into the text.
 */
export const MAX_LANES = 4;

/** Gutter the list must reserve on the left for {@link visibleRails}. */
export function railGutterPx(rails: Map<number, Rail[]> | null): number {
  if (!rails) return 0;
  return Math.min(laneCount(rails), MAX_LANES) * LANE_W;
}

/** The rails a row may actually draw — those that fit the reserved gutter. */
export function visibleRails(rails: Rail[] | undefined): Rail[] {
  if (!rails) return [];
  return rails.filter((r) => r.lane < MAX_LANES);
}

/**
 * Human-readable label for a `derived_kind`, used in the rail tooltip.
 *
 * The kinds *are* the transform kinds, so the labels come straight from the
 * `TRANSFORMS` catalogue — a second copy here would drift the moment a
 * transform is renamed.
 */
export function derivedKindLabel(kind: string | null | undefined): string {
  if (!kind) return "Copied from another entry";
  const spec = TRANSFORMS.find((t) => t.kind === kind);
  return spec ? spec.label : kind;
}
