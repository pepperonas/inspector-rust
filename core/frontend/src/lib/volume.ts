/** Pure volume-step math shared by the SoundPanel slider arrows.
 *
 * Mirrors the Rust `system_commands::snap_volume_step`: from `current`, move
 * to the **next multiple of |delta|** in the direction of `delta`, clamped to
 * 0–100. So ±5 from 81 gives 85 / 80, then 90 / 75, … — every step lands on
 * the 5-grid instead of the old plain `current + delta` (81 → 86 → 91 …),
 * which never aligned and read as inconsistent stepping.
 */
export function snapVolumeStep(current: number, delta: number): number {
  const step = Math.max(1, Math.abs(Math.round(delta)));
  const cur = Math.round(current);
  const target =
    delta >= 0
      ? (Math.floor(cur / step) + 1) * step
      : (Math.ceil(cur / step) - 1) * step;
  return Math.max(0, Math.min(100, target));
}
