/**
 * Pure helpers for the volume/mute HUD's "Klangaura" animation (v0.118.0):
 * digit decomposition for the rolling readout, trigger direction, and the
 * wave intensity curve. The component owns only DOM/SVG rendering.
 */

/** 85 → ["8","5"], 100 → ["1","0","0"], 0 → ["0"]. Clamps + floors. */
export function digitColumns(level: number): string[] {
  const v = Math.max(0, Math.min(100, Math.floor(Number.isFinite(level) ? level : 0)));
  return String(v).split("");
}

/**
 * Which way did this trigger move the level? Drives the wave direction
 * (louder → waves emanate outward, quieter → they collapse inward) and the
 * digit roll direction. `none` for the first reading and for repeats at a
 * boundary (0/100 held) — a wave that fires without a change would lie.
 */
export type RollDirection = "up" | "down" | "none";
export function rollDirection(prev: number | null, next: number): RollDirection {
  if (prev == null || !Number.isFinite(next) || prev === next) return "none";
  return next > prev ? "up" : "down";
}

/**
 * Wave opacity for a level: quiet volumes whisper (0.35), loud ones radiate
 * (1.0). Linear over the audible range — the wave is a level METER, not a
 * flourish, so the mapping must be monotonic and boring.
 */
export function waveIntensity(level: number): number {
  const v = Math.max(0, Math.min(100, Number.isFinite(level) ? level : 0));
  return 0.35 + (v / 100) * 0.65;
}
