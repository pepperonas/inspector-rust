/**
 * Pure selection logic for the `clean` panel.
 *
 * A scan returns *directory rows* (`CleanDir`), not the raw file list — the
 * file-granular plan stays in the backend (v0.84.264), which is what keeps a
 * 100k-file cache scan cheap and the preview readable. The panel therefore
 * lets the user tick individual directories, grouped under their category, and
 * hands the ticked `CleanDir.path` values to `cleaner_execute`. The backend
 * re-validates every path at delete time — the selection is only about consent.
 */
import type { CleanDir, CleanPlanView } from "./ipc";

export interface CleanCategorySummary {
  key: string;
  label: string;
  bytes: number;
  count: number;
}

/** One rendered line: either a category header or a selectable directory. */
export type CleanRow =
  | { kind: "header"; key: string; label: string; bytes: number; dirs: number }
  | { kind: "dir"; dir: CleanDir };

/** Per-category summaries, largest first; categories with no rows are dropped
 * (nothing to choose there). */
export function categorySummaries(view: CleanPlanView): CleanCategorySummary[] {
  const counts = new Map<string, number>();
  for (const d of view.dirs) {
    counts.set(d.category, (counts.get(d.category) ?? 0) + d.count);
  }
  return view.categories
    .map(([key, label, bytes]) => ({ key, label, bytes, count: counts.get(key) ?? 0 }))
    .filter((c) => c.count > 0)
    .sort((a, b) => b.bytes - a.bytes);
}

/** The rendered list: categories largest-first, each followed by its directory
 * rows largest-first. Headers are not selectable — only the `dir` rows are. */
export function buildRows(view: CleanPlanView): CleanRow[] {
  const rows: CleanRow[] = [];
  for (const cat of categorySummaries(view)) {
    const dirs = view.dirs
      .filter((d) => d.category === cat.key)
      .sort((a, b) => b.size - a.size || a.path.localeCompare(b.path));
    if (dirs.length === 0) continue;
    rows.push({
      kind: "header",
      key: cat.key,
      label: cat.label,
      bytes: cat.bytes,
      dirs: dirs.length,
    });
    for (const dir of dirs) rows.push({ kind: "dir", dir });
  }
  return rows;
}

/** Every directory path of a category — for the header's toggle-the-whole-group
 * click. */
export function dirsOfCategory(view: CleanPlanView, key: string): string[] {
  return view.dirs.filter((d) => d.category === key).map((d) => d.path);
}

/** Live "Selected: N files · X" footer numbers, over the ticked directories. */
export function selectionTotals(
  dirs: readonly CleanDir[],
  selected: ReadonlySet<string>,
): { files: number; bytes: number } {
  let files = 0;
  let bytes = 0;
  for (const d of dirs) {
    if (selected.has(d.path)) {
      files += d.count;
      bytes += d.size;
    }
  }
  return { files, bytes };
}

/** The final path segment, for compact rows. Command pseudo-items carry a human
 * sentence instead of a path (`"Docker build cache — freed via …"`) — those are
 * returned unchanged rather than chopped at their last slash. */
export function basename(path: string): string {
  const norm = path.replace(/\\/g, "/");
  if (!norm.startsWith("/") && !/^[A-Za-z]:/.test(norm)) return path;
  const idx = norm.lastIndexOf("/");
  return idx >= 0 ? norm.slice(idx + 1) || norm : norm;
}

/** Shorten a long absolute path for display: `~/Library/Caches/foo`. */
export function prettyPath(path: string, home?: string): string {
  // The prefix must end at a path-segment boundary — `/Users/xy/foo` must NOT
  // collapse under home `/Users/x` (it used to render as `~y/foo`).
  if (
    home &&
    home.length > 1 &&
    path.startsWith(home) &&
    (path.length === home.length || path[home.length] === "/" || path[home.length] === "\\")
  ) {
    return `~${path.slice(home.length)}`;
  }
  return path;
}
