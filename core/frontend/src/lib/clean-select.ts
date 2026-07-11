/**
 * Pure selection logic for the `clean` panel (v0.84.242): the scan plan comes
 * back as a flat item list + per-category totals; the panel lets the user pick
 * categories, so these helpers summarise, preview and filter the plan. The
 * backend re-validates every path at execute time — filtering here is purely
 * about which files the user consented to delete.
 */
import type { CleanItem, CleanPlan } from "./ipc";

export interface CleanCategorySummary {
  key: string;
  label: string;
  bytes: number;
  count: number;
}

/** Per-category summary rows, largest first; categories with no items are
 * dropped (nothing to choose there). */
export function categorySummaries(plan: CleanPlan): CleanCategorySummary[] {
  const counts = new Map<string, number>();
  for (const it of plan.items) {
    counts.set(it.category, (counts.get(it.category) ?? 0) + 1);
  }
  return plan.categories
    .map(([key, label, bytes]) => ({ key, label, bytes, count: counts.get(key) ?? 0 }))
    .filter((c) => c.count > 0)
    .sort((a, b) => b.bytes - a.bytes);
}

/** The `n` largest items of one category (for the "what is this actually?"
 * preview under the selected row). */
export function topItems(plan: CleanPlan, key: string, n: number): CleanItem[] {
  return plan.items
    .filter((i) => i.category === key)
    .sort((a, b) => b.size - a.size)
    .slice(0, n);
}

/** Reduce the plan to the selected categories, with recomputed totals — this
 * is exactly what gets handed to `cleaner_execute`. */
export function filterPlan(plan: CleanPlan, selected: ReadonlySet<string>): CleanPlan {
  const items = plan.items.filter((i) => selected.has(i.category));
  return {
    items,
    total_bytes: items.reduce((s, i) => s + i.size, 0),
    categories: plan.categories.filter(([key]) => selected.has(key)),
  };
}

/** Live "Selected: N files · X" footer numbers. */
export function selectionTotals(
  summaries: readonly CleanCategorySummary[],
  selected: ReadonlySet<string>,
): { files: number; bytes: number } {
  let files = 0;
  let bytes = 0;
  for (const s of summaries) {
    if (selected.has(s.key)) {
      files += s.count;
      bytes += s.bytes;
    }
  }
  return { files, bytes };
}

/** The final path segment, for compact top-item rows. */
export function basename(path: string): string {
  const norm = path.replace(/\\/g, "/");
  const idx = norm.lastIndexOf("/");
  return idx >= 0 ? norm.slice(idx + 1) : norm;
}
