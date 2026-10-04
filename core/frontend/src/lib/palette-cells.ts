// Hex-grid density fields (Settings → Window palette). Pure, so the
// keystroke-by-keystroke behaviour is testable without a DOM.
//
// The field keeps its own DRAFT text while you type and only saves a value
// once it's complete: every keystroke used to be saved and clamped by the
// backend, so typing "12" saved "1", came back as "2", and the next digit
// landed behind it ("22").

/** Mirrors `MIN_CELLS` / `MAX_CELLS` in `window_palette/mod.rs` (pinned by a test). */
export const PALETTE_MIN_CELLS = 2;
export const PALETTE_MAX_CELLS = 24;

/** Pause after the last keystroke before a valid draft is saved on its own. */
export const CELL_DRAFT_DEBOUNCE_MS = 700;

/**
 * A draft that can be saved WITHOUT being corrected: digits only, in range.
 * `null` while the text is incomplete ("", "1" on the way to "12") or out of
 * range — the field keeps waiting instead of saving a value nobody typed.
 */
export function validCellCount(draft: string): number | null {
  const t = draft.trim();
  if (!/^\d+$/.test(t)) return null;
  const n = Number(t);
  return n >= PALETTE_MIN_CELLS && n <= PALETTE_MAX_CELLS ? n : null;
}

/**
 * What leaving the field (blur / Enter) saves: an out-of-range number is
 * clamped (the user asked for "a lot" or "few"), anything that isn't a number
 * keeps the current value rather than inventing one.
 */
export function commitCellCount(draft: string, current: number): number {
  const t = draft.trim();
  if (!/^\d+$/.test(t)) return current;
  return Math.min(PALETTE_MAX_CELLS, Math.max(PALETTE_MIN_CELLS, Number(t)));
}
