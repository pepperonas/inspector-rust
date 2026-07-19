/**
 * Shazam-history search — the pure filter behind the History view's search
 * field. Case-insensitive substring match over title · artist · album ·
 * genre; multiple whitespace-separated terms must ALL match (in any field —
 * `dua lipa levit` finds "Levitating — Dua Lipa"). Pure + unit-tested.
 */
import type { ShazamHistoryEntry } from "./ipc";

export function filterShazamHistory(
  entries: readonly ShazamHistoryEntry[],
  query: string,
): ShazamHistoryEntry[] {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return [...entries];
  return entries.filter((e) => {
    const hay = `${e.title} ${e.artist} ${e.album} ${e.genre}`.toLowerCase();
    return terms.every((t) => hay.includes(t));
  });
}
