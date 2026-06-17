/**
 * Pure disco-engine helpers (colour selection, round-robin, floor brightness).
 * Extracted from `disco-engine.ts` so the beat→light mapping is unit-tested
 * without needing Web Audio or a Hue bridge.
 */

export type DiscoMode = "rainbow" | "pulse" | "strobe";

/** Round-robin: the next index after `i` across `n` items (wraps; safe for n≤0). */
export function nextIndex(i: number, n: number): number {
  return n <= 0 ? 0 : (i + 1) % n;
}

/**
 * The colour for this beat + the advanced palette index.
 * - `rainbow`: step through `palette` (one entry per beat, wrapping).
 * - `pulse` / `strobe`: hold the user's `fixedHex` (palette index unchanged).
 */
export function beatColor(
  mode: DiscoMode,
  fixedHex: string,
  palIndex: number,
  palette: readonly string[],
): { hex: string; nextPal: number } {
  if (mode === "rainbow" && palette.length > 0) {
    return { hex: palette[palIndex % palette.length], nextPal: (palIndex + 1) % palette.length };
  }
  return { hex: fixedHex, nextPal: palIndex };
}

/** The brightness % the *previous* lamp settles to between beats. Strobe goes
 *  near-dark (hard blink); the cyclic/pulse modes keep a dim floor. */
export function floorBrightness(mode: DiscoMode, rainbowFloor: number, strobeFloor: number): number {
  return mode === "strobe" ? strobeFloor : rainbowFloor;
}
