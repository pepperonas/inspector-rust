/**
 * Pure geometry + colour + formatting for the `disk`/`daisy` sunburst
 * (v0.120.0). The component owns only SVG rendering + interaction; every
 * number here is testable.
 */

export interface DiskNode {
  name: string;
  size: number;
  is_dir: boolean;
  other?: boolean;
  child_count: number;
  children?: DiskNode[];
}

/** One rendered ring segment. `path` is the index chain from the focus root
 *  (for drill-down + hover identity); depth 0 = innermost ring. */
export interface Arc {
  path: number[];
  node: DiskNode;
  depth: number;
  a0: number; // start angle, radians, 0 = 12 o'clock, clockwise
  a1: number;
  r0: number; // inner radius (px)
  r1: number; // outer radius (px)
  color: string;
}

export interface SunburstOpts {
  /** Radius where the innermost ring STARTS (the centre hub radius). */
  hubR: number;
  /** Thickness of each ring. */
  ring: number;
  /** How many rings to draw. */
  rings: number;
  /** Segments narrower than this (radians) are dropped — invisible slivers
   *  cost paint and can't be hovered meaningfully. */
  minAngle?: number;
}

/**
 * Build the ring segments for `root`'s subtree. The root itself is the centre
 * hub (not an arc); its children fill ring 0 across the full circle, their
 * children nest within each parent's angular span on ring 1, and so on. A
 * `startAngle` lets a caller rotate the whole chart.
 */
export function sunburstArcs(root: DiskNode, opts: SunburstOpts, startAngle = 0): Arc[] {
  const { hubR, ring, rings } = opts;
  const minAngle = opts.minAngle ?? 0.012;
  const arcs: Arc[] = [];

  const recur = (node: DiskNode, depth: number, a0: number, a1: number, path: number[], hue: number) => {
    if (depth >= rings || !node.children || node.children.length === 0) return;
    const span = a1 - a0;
    const childTotal = node.children.reduce((s, c) => s + c.size, 0) || 1;
    let cursor = a0;
    node.children.forEach((child, i) => {
      const cSpan = (child.size / childTotal) * span;
      const c0 = cursor;
      const c1 = cursor + cSpan;
      cursor = c1;
      if (cSpan < minAngle) return;
      // Top ring: each segment owns a hue; deeper rings inherit the parent hue
      // with a lightness step by index (DaisyDisk's "same family, lighter").
      const childHue = depth === 0 ? topHue(i, node.children!.length) : hue;
      arcs.push({
        path: [...path, i],
        node: child,
        depth,
        a0: c0,
        a1: c1,
        r0: hubR + depth * ring,
        r1: hubR + (depth + 1) * ring,
        color: segmentColor(childHue, depth, i),
      });
      recur(child, depth + 1, c0, c1, [...path, i], childHue);
    });
  };
  recur(root, 0, startAngle, startAngle + Math.PI * 2, [], 0);
  // Larger arcs paint first so a hovered thin arc's outline sits on top.
  return arcs.sort((a, b) => b.depth - a.depth || a.a0 - b.a0);
}

/** Evenly spaced, pleasant hues around the wheel for the top ring. Skews away
 *  from muddy yellow-greens by sampling a curated band. */
export function topHue(i: number, n: number): number {
  // Golden-angle-ish spacing gives good separation for any n without a table.
  return (i * (360 / Math.max(1, n)) + i * 12) % 360;
}

/** HSL colour for a segment: parent hue, saturation/lightness stepped by ring
 *  depth (outer rings lighter) and nudged by index so siblings differ. */
export function segmentColor(hue: number, depth: number, index: number): string {
  const sat = 62 - depth * 5;
  const light = 52 + depth * 7 + (index % 2 === 0 ? 0 : 4);
  return `hsl(${Math.round(hue)}, ${Math.max(30, sat)}%, ${Math.min(80, light)}%)`;
}

/** SVG path `d` for a ring segment (annular sector). A near-full-circle
 *  segment is split into two arcs — an SVG arc with identical endpoints draws
 *  nothing (the 100 % donut bug, learned in loc's donut). */
export function arcPath(a: Arc, cx: number, cy: number, gap = 0.004): string {
  // Detect a full ring on the RAW span (a single 100 % child) — then draw a
  // complete ring as two half arcs with NO gap: an SVG arc with identical
  // endpoints draws nothing (the 100 % donut bug), and a lone full segment
  // needs no separating gap anyway.
  if (a.a1 - a.a0 >= Math.PI * 2 - 0.001) {
    const mid = a.a0 + Math.PI;
    return ring(a, cx, cy, a.a0, mid) + " " + ring(a, cx, cy, mid, a.a1);
  }
  const a0 = a.a0 + gap;
  const a1 = a.a1 - gap;
  if (a1 <= a0) return "";
  return ring(a, cx, cy, a0, a1);
}

function pt(cx: number, cy: number, r: number, ang: number): [number, number] {
  // 0 rad = 12 o'clock, clockwise.
  return [cx + r * Math.sin(ang), cy - r * Math.cos(ang)];
}

function ring(a: Arc, cx: number, cy: number, a0: number, a1: number): string {
  const large = a1 - a0 > Math.PI ? 1 : 0;
  const [ox0, oy0] = pt(cx, cy, a.r1, a0);
  const [ox1, oy1] = pt(cx, cy, a.r1, a1);
  const [ix1, iy1] = pt(cx, cy, a.r0, a1);
  const [ix0, iy0] = pt(cx, cy, a.r0, a0);
  return (
    `M ${ox0.toFixed(2)} ${oy0.toFixed(2)} ` +
    `A ${a.r1} ${a.r1} 0 ${large} 1 ${ox1.toFixed(2)} ${oy1.toFixed(2)} ` +
    `L ${ix1.toFixed(2)} ${iy1.toFixed(2)} ` +
    `A ${a.r0} ${a.r0} 0 ${large} 0 ${ix0.toFixed(2)} ${iy0.toFixed(2)} Z`
  );
}

/** Navigate a node by an index path (drill-down). Returns null if any step is
 *  out of range or lands on a non-drillable node. */
export function nodeAt(root: DiskNode, path: number[]): DiskNode | null {
  let cur: DiskNode = root;
  for (const i of path) {
    const next = cur.children?.[i];
    if (!next) return null;
    cur = next;
  }
  return cur;
}

/** Binary size (binary units, DaisyDisk style — one decimal from MB up). */
export function formatBytes(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return i <= 1 ? `${Math.round(v)} ${units[i]}` : `${v.toFixed(1)} ${units[i]}`;
}

/** Percentage string, no "-0", one decimal under 10 % else integer. */
export function formatPct(part: number, whole: number): string {
  if (whole <= 0 || part <= 0) return "0 %";
  const p = (part / whole) * 100;
  const s = p < 10 ? p.toFixed(1) : Math.round(p).toString();
  return `${s} %`;
}

/** The basename for the top-files list (paths are absolute). */
export function baseName(path: string): string {
  const parts = path.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** The containing directory of an absolute path, or `null` at the filesystem
 *  root — that `null` is what stops "go up" from walking off the top. */
export function parentPath(path: string): string | null {
  const segs = path.split("/").filter(Boolean);
  if (segs.length === 0) return null; // already "/"
  segs.pop();
  return "/" + segs.join("/");
}

/** Append name segments to an absolute root. Kept pure so the panel's
 *  path arithmetic is tested rather than eyeballed. */
export function joinPath(root: string, parts: readonly string[]): string {
  const base = root === "/" ? "" : root.replace(/\/+$/, "");
  return parts.length ? `${base}/${parts.join("/")}` : base || "/";
}

/** One segment of the panel's path bar. */
export interface PathCrumb {
  /** Display label ("/" for the filesystem root). */
  name: string;
  /** Absolute path this segment stands for. */
  path: string;
  /**
   * How many in-tree drill steps reach this crumb, or `null` when it lies
   * ABOVE the current scan root. That distinction is the whole point: a crumb
   * inside the scanned tree is reachable instantly (the sizes are already
   * computed), one above it needs a fresh scan.
   */
  steps: number | null;
}

/**
 * The full absolute path of the current view, segment by segment — the scan
 * root's own path plus one crumb per drill step.
 */
export function pathCrumbs(rootPath: string, drillNames: readonly string[]): PathCrumb[] {
  const rootSegs = rootPath.split("/").filter(Boolean);
  const out: PathCrumb[] = [
    { name: "/", path: "/", steps: rootSegs.length === 0 ? 0 : null },
  ];
  let acc = "";
  rootSegs.forEach((seg, i) => {
    acc += "/" + seg;
    out.push({ name: seg, path: acc, steps: i === rootSegs.length - 1 ? 0 : null });
  });
  drillNames.forEach((name, i) => {
    acc += "/" + name;
    out.push({ name, path: acc, steps: i + 1 });
  });
  return out;
}

/** One row of the child list under the chart. */
export interface ChildRow {
  /** Index in the parent's `children` — the drill path is built from it, so
   *  the sort must NOT lose it. */
  index: number;
  node: DiskNode;
  /** Share of the parent, 0..1. */
  share: number;
}

/**
 * Every child of the focused node, largest first.
 *
 * ⚠️ This is the answer to the sunburst's built-in blind spot: the chart is
 * area-proportional *by design* (that honesty is the whole point of a
 * DaisyDisk-style view), which means a 2 MB `src` next to a 20 GB `target`
 * renders as a sub-pixel hairline you cannot hit — exactly the folders you
 * want to open in a software project. A list has no minimum angle, so every
 * child stays reachable no matter how small. Nothing is dropped here, not even
 * the synthetic "Sonstiges" bucket (it is shown, just not drillable).
 */
export function childRows(focus: DiskNode): ChildRow[] {
  const kids = focus.children ?? [];
  const total = kids.reduce((s, c) => s + c.size, 0) || focus.size || 1;
  return kids
    .map((node, index) => ({ index, node, share: node.size / total }))
    .sort((a, b) => b.node.size - a.node.size || a.node.name.localeCompare(b.node.name));
}

// ── Deleting from the view (v0.169.0) ────────────────────────────────────────
//
// Trashing an item does NOT re-scan. A full `~` walk takes seconds and the old
// flow also reset the drill to the root, so deleting three things deep in a
// tree meant three round trips back to the top. Instead the tree is pruned
// locally and exactly: the item's on-disk bytes leave the scanned folder (they
// now live in the Trash), so subtracting them up the ancestor chain is the
// honest number — while the VOLUME's free space stays as it was, because the
// Trash still holds the bytes until it is emptied. Everything here is pure so
// that arithmetic is tested rather than eyeballed.

/** One entry the user has collected for deletion — an absolute-path snapshot,
 *  so it survives drilling elsewhere and even a re-scan of another folder. */
export interface CollectorItem {
  path: string;
  name: string;
  size: number;
  is_dir: boolean;
}

/** Shape of the batch-trash IPC result (mirrors Rust `TrashReport`). */
export interface TrashReport {
  trashed: string[];
  failed: { path: string; error: string }[];
}

export function collectorTotals(items: readonly CollectorItem[]): { count: number; bytes: number } {
  return { count: items.length, bytes: items.reduce((s, i) => s + i.size, 0) };
}

/** Absolute filesystem path of a node addressed by an index chain from the
 *  scan root. `null` when any step is the synthetic "Other" bucket (it has no
 *  path) or out of range. Inverse of `indexPathFor`. */
export function absPathOf(
  scan: { root_path: string; tree: DiskNode },
  idxPath: readonly number[],
): string | null {
  let cur: DiskNode = scan.tree;
  const parts: string[] = [];
  for (const i of idxPath) {
    const next = cur.children?.[i];
    if (!next || next.other) return null;
    parts.push(next.name);
    cur = next;
  }
  return joinPath(scan.root_path, parts);
}

/** Resolve an absolute path back to an index chain inside the scanned tree —
 *  `[]` for the root itself, `null` when the path lies outside the scan, runs
 *  through the "Other" bucket, or names a child the pruned tree does not
 *  carry. Inverse of `absPathOf`. */
export function indexPathFor(
  scan: { root_path: string; tree: DiskNode },
  absPath: string,
): number[] | null {
  const root = scan.root_path === "/" ? "" : scan.root_path.replace(/\/+$/, "");
  const target = absPath.replace(/\/+$/, "");
  if (target === (root || "/")) return [];
  const prefix = root + "/";
  if (!target.startsWith(prefix)) return null;
  const segs = target.slice(prefix.length).split("/").filter(Boolean);
  const out: number[] = [];
  let cur: DiskNode = scan.tree;
  for (const seg of segs) {
    const i = (cur.children ?? []).findIndex((c) => !c.other && c.name === seg);
    if (i < 0) return null;
    out.push(i);
    cur = cur.children![i];
  }
  return out;
}

/** Is `path` equal to or inside `dir`? Path-boundary aware, so `/a/bc` is
 *  never "under" `/a/b`. */
export function isUnder(path: string, dir: string): boolean {
  const d = dir === "/" ? "/" : dir.replace(/\/+$/, "");
  if (path === d) return true;
  return d === "/" ? path.startsWith("/") : path.startsWith(d + "/");
}

/**
 * A NEW tree with the node at `idxPath` removed and its size subtracted from
 * every ancestor (`child_count` of the parent decremented). Never mutates the
 * input — the tree is React state. An empty path (the root) or an invalid one
 * returns the tree unchanged: there is nothing sensible to remove.
 */
export function removeAt(root: DiskNode, idxPath: readonly number[]): DiskNode {
  if (idxPath.length === 0) return root;
  const walk = (node: DiskNode, depth: number): DiskNode | null => {
    const i = idxPath[depth];
    const kids = node.children ?? [];
    const victim = kids[i];
    if (!victim) return null; // invalid path → signal "unchanged"
    if (depth === idxPath.length - 1) {
      const children = kids.filter((_, k) => k !== i);
      return {
        ...node,
        size: Math.max(0, node.size - victim.size),
        child_count: Math.max(0, node.child_count - 1),
        ...(children.length ? { children } : { children: [] }),
      };
    }
    const replaced = walk(victim, depth + 1);
    if (!replaced) return null;
    const children = kids.map((k, idx) => (idx === i ? replaced : k));
    return { ...node, size: Math.max(0, node.size - (victim.size - replaced.size)), children };
  };
  return walk(root, 0) ?? root;
}

/**
 * Re-resolve a drill by folder NAMES after the tree changed — the longest
 * prefix that still exists, as fresh indices.
 *
 * ⚠️ Positions are not identity. Pruning a sibling shifts every index after
 * it, so a stale index chain keeps "working" while silently addressing a
 * DIFFERENT folder (trash `target` at index 0 and the old `[0, 0]` now lands
 * on `src/main.rs`). Only the names say where the user was standing; if an
 * ancestor itself was trashed, the walk stops there.
 */
export function drillByNames(tree: DiskNode, names: readonly string[]): number[] {
  const out: number[] = [];
  let cur: DiskNode = tree;
  for (const name of names) {
    const i = (cur.children ?? []).findIndex((c) => !c.other && c.name === name);
    if (i < 0) break;
    out.push(i);
    cur = cur.children![i];
  }
  return out;
}

/**
 * Apply a set of trashed absolute paths to a scan: each path that resolves
 * inside the tree is pruned (size subtracted up the chain), the largest-files
 * list drops every file at or under a trashed path, and `total` follows the
 * tree. Paths outside the scan (collected under another root) are ignored —
 * there is nothing of them in this picture. Volume free space is deliberately
 * NOT touched: the bytes sit in the Trash until it is emptied.
 */
export function pruneScan<S extends { root_path: string; tree: DiskNode; total: number; top_files: { path: string; size: number }[] }>(
  scan: S,
  trashed: readonly string[],
): S {
  let tree = scan.tree;
  // Prune deepest paths first so an ancestor's removal can't invalidate a
  // descendant's index chain resolved against the original tree.
  const sorted = [...trashed].sort((a, b) => b.split("/").length - a.split("/").length);
  for (const p of sorted) {
    const idx = indexPathFor({ root_path: scan.root_path, tree }, p);
    if (idx && idx.length > 0) tree = removeAt(tree, idx);
  }
  const top_files = scan.top_files.filter((f) => !trashed.some((t) => isUnder(f.path, t)));
  return { ...scan, tree, total: tree.size, top_files };
}
