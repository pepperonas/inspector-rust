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

// ── Mouse control (v0.197.0) ────────────────────────────────────────────────

/**
 * How long the volume/mute HUD lingers after the last gesture (2026-10-06:
 * doubled from 1100 on request — the readout vanished before it was read).
 * Only touchpad gestures raise this HUD, so this IS the gesture hold.
 */
export const HOLD_MS_GESTURE = 2200;
/** The hover event the Rust mouse gate emits on every enter/leave of the card. */
export const HOVER_EVENT = "status-toast-hover";
/** After the mouse touched the HUD it stays this long before fading. */
export const HOLD_AFTER_MOUSE_MS = 3000;
/** While dragging, the system volume is set at most this often. */
export const DRAG_SET_EVERY_MS = 50;

/**
 * Pointer x on the track → volume 0..100 (integer). Positions left of the
 * track are 0, right of it 100; a zero-width track yields 0.
 */
export function levelFromPointer(clientX: number, trackLeft: number, trackWidth: number): number {
  if (!(trackWidth > 0) || !Number.isFinite(clientX)) return 0;
  const f = (clientX - trackLeft) / trackWidth;
  return Math.round(Math.max(0, Math.min(1, f)) * 100);
}

/** How long the HUD lingers after the last trigger. */
export function holdMs(touchedByMouse: boolean, base: number): number {
  return touchedByMouse ? HOLD_AFTER_MOUSE_MS : base;
}
