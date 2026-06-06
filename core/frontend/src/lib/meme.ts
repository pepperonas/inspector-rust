/**
 * Meme picker frontend helpers — pure, no DOM / no IPC (those live in
 * `ipc.ts`). `meme [query]` surfaces meme rows; `matchMemes` fuzzy-ranks the
 * library against the query (matching both the file name and its category
 * folder), the selected one previews (animated) and copies on Enter.
 *
 * **Build flag:** the whole meme command is gated by `MEME_ENABLED` so a
 * "without memes" build can be produced (`VITE_IR_MEME=0`) — the personal
 * library folder is then never surfaced. Default = enabled.
 */

export const MEME_ENABLED: boolean = import.meta.env.VITE_IR_MEME !== "0";

export interface MemeEntry {
  /** File stem — primary display + match key. */
  name: string;
  /** Parent folder ("category"), or "" for a top-level file. */
  category: string;
  /** Absolute path (used for preview via convertFileSrc + copy IPC). */
  path: string;
}

/** Lower-cased, ranked match of `query` against a meme's name + category.
 *  Higher score = better; `null` = no match. Mirrors the tiered approach of
 *  the command/TOTP matchers (exact > prefix > infix > subsequence), with a
 *  bonus when the *name* (not just the category) matches. */
export function memeScore(query: string, m: MemeEntry): number | null {
  const q = query.trim().toLowerCase();
  if (q === "") return 0;
  const name = m.name.toLowerCase();
  const cat = m.category.toLowerCase();

  const tier = (h: string): number | null => {
    if (h === q) return 100;
    if (h.startsWith(q)) return 80 - Math.min(h.length, 40) * 0.1;
    if (h.includes(q)) return 55 - h.indexOf(q) * 0.2;
    if (q.length >= 3 && subsequence(q, h)) return 25;
    return null;
  };

  const ns = tier(name);
  const cs = tier(cat);
  // Name matches outrank category matches; take the best available.
  if (ns !== null && cs !== null) return Math.max(ns + 6, cs);
  if (ns !== null) return ns + 6;
  if (cs !== null) return cs;
  return null;
}

/** Is `needle` an in-order subsequence of `hay`? */
function subsequence(needle: string, hay: string): boolean {
  let i = 0;
  for (let j = 0; j < hay.length && i < needle.length; j++) {
    if (hay[j] === needle[i]) i++;
  }
  return i === needle.length;
}

/** Fuzzy-filter + rank the library; empty query returns all (capped). */
export function matchMemes(query: string, memes: MemeEntry[], limit = 60): MemeEntry[] {
  const q = query.trim();
  if (q === "") return memes.slice(0, limit);
  const scored: Array<{ m: MemeEntry; s: number }> = [];
  for (const m of memes) {
    const s = memeScore(q, m);
    if (s !== null) scored.push({ m, s });
  }
  scored.sort((a, b) => b.s - a.s || a.m.name.localeCompare(b.m.name));
  return scored.slice(0, limit).map((x) => x.m);
}
