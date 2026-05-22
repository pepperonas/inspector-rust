/**
 * `opener` easter egg — pick a random German opener from the embedded
 * top-100 list (curated from the VPS `nicetobenice_db` by the maintainer's
 * ratings + favourites).
 *
 * Pure, deterministic helpers so the React layer can show a stable
 * suggestion while the trigger query is unchanged, and re-roll on every
 * keystroke without any global mutable state.
 */
import { TOP_OPENERS } from "./openers-data";

/** Re-export so the trigger handler doesn't have to know about the
 *  data module directly. */
export { TOP_OPENERS };

/**
 * Hash a string to a non-negative 32-bit integer.
 *
 * Cheap FNV-1a variant — collision-prone but plenty good for picking a
 * deterministic array index. Same input → same hash → same picked index,
 * so the React render loop doesn't show a flicker of different openers
 * while the user holds the query steady.
 */
export function hashString(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0; // unsigned 32-bit
}

/**
 * Pick a deterministic opener for the given `seed` string.
 *
 * Callers pass the user's full query as the seed — so the suggestion is
 * stable as long as the query is, and re-rolls on each keystroke (every
 * keystroke changes the seed). Returns `null` only when the embedded
 * list is empty (defensive — shouldn't happen in production).
 */
export function pickOpener(seed: string): string | null {
  if (TOP_OPENERS.length === 0) return null;
  const idx = hashString(seed) % TOP_OPENERS.length;
  return TOP_OPENERS[idx];
}
