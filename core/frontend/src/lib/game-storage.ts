/**
 * Tiny localStorage-backed persistence for the hidden easter-egg games.
 *
 * Two independent things are stored per game, under a namespaced key:
 *   - **high score** (`.best`) — survives forever, only ever grows.
 *   - **suspended run** (`.state`) — the full in-progress game state,
 *     written when the player presses Esc mid-game and consumed on the
 *     next launch so the run resumes exactly where it left off.
 *
 * Each game (and each Snake variant) passes its own `key`, so the two
 * Snake modes keep separate high scores and separate suspended runs:
 *   `pong` · `snake-classic` · `snake-wrap` · `space`
 *
 * Everything is best-effort: a disabled/full/throwing localStorage (or a
 * corrupt JSON blob) degrades to "no saved data" rather than crashing the
 * game. Pure-ish module → unit-tested in `game-storage.test.ts`.
 */

const PREFIX = "inspector-rust.game.";

const bestKey = (key: string) => `${PREFIX}${key}.best`;
const stateKey = (key: string) => `${PREFIX}${key}.state`;

/** Read the persisted high score for `key` (0 if none / unreadable). */
export function loadHighScore(key: string): number {
  try {
    const raw = localStorage.getItem(bestKey(key));
    if (raw == null) return 0;
    const n = Number.parseInt(raw, 10);
    return Number.isFinite(n) && n > 0 ? n : 0;
  } catch {
    return 0;
  }
}

/** Persist `value` as the high score for `key` (no-op on failure). */
export function saveHighScore(key: string, value: number): void {
  try {
    localStorage.setItem(bestKey(key), String(Math.max(0, Math.floor(value))));
  } catch {
    /* localStorage unavailable — high score just won't persist. */
  }
}

/**
 * Convenience: persist `value` only if it beats the stored best, and
 * return the resulting best. Lets a game finalise its high score in one
 * call without re-reading first.
 */
export function commitHighScore(key: string, value: number): number {
  const best = Math.max(loadHighScore(key), value);
  saveHighScore(key, best);
  return best;
}

/** Read a suspended run for `key` (`null` if none / corrupt). */
export function loadSavedGame<T>(key: string): T | null {
  try {
    const raw = localStorage.getItem(stateKey(key));
    return raw == null ? null : (JSON.parse(raw) as T);
  } catch {
    return null;
  }
}

/** Persist a suspended run for `key` (no-op on failure). */
export function saveGame<T>(key: string, state: T): void {
  try {
    localStorage.setItem(stateKey(key), JSON.stringify(state));
  } catch {
    /* localStorage unavailable — the run just won't resume. */
  }
}

/** Drop any suspended run for `key` (e.g. after a game ends). */
export function clearSavedGame(key: string): void {
  try {
    localStorage.removeItem(stateKey(key));
  } catch {
    /* nothing to clean up if storage is unavailable. */
  }
}
