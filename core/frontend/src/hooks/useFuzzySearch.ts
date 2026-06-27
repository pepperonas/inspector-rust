import { useMemo } from "react";
import type { ClipEntry } from "../lib/types";

/**
 * Filter clipboard history by a **substring** match (case-insensitive) — NOT
 * fuzzy. Prefix matches rank first; within a tier the backend's pinned/recency
 * order is preserved (entries are pushed in iteration order). Fuzzy /
 * non-contiguous matching is deliberately reserved for the power commands + the
 * app launcher, so a clip only surfaces when it literally contains the query.
 *
 * (The name is kept for import stability; the behaviour is plain substring.)
 */
export function useFuzzySearch(entries: ClipEntry[], query: string): ClipEntry[] {
  return useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return entries;
    const prefix: ClipEntry[] = [];
    const infix: ClipEntry[] = [];
    for (const e of entries) {
      const text = (e.content_text ?? "").toLowerCase();
      if (!text) continue;
      const idx = text.indexOf(q);
      if (idx === 0) prefix.push(e);
      else if (idx > 0) infix.push(e);
    }
    return [...prefix, ...infix];
  }, [entries, query]);
}
