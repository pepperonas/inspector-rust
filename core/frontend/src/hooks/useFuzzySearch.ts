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
  // Lowercase once per clip-list change, not once per keystroke: clips are
  // often multi-KB text blobs and the history holds up to 1000 rows — the old
  // shape re-allocated the lowercased copy of EVERY clip on EVERY keystroke.
  const lowered = useMemo(
    () => entries.map((e) => (e.content_text ?? "").toLowerCase()),
    [entries],
  );
  return useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return entries;
    const prefix: ClipEntry[] = [];
    const infix: ClipEntry[] = [];
    for (let i = 0; i < entries.length; i++) {
      const text = lowered[i];
      if (!text) continue;
      const idx = text.indexOf(q);
      if (idx === 0) prefix.push(entries[i]);
      else if (idx > 0) infix.push(entries[i]);
    }
    return [...prefix, ...infix];
  }, [entries, lowered, query]);
}
