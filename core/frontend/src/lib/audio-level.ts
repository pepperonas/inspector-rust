/**
 * Pure audio-level helpers shared by the BPM detector and the disco engine.
 * Kept separate from the (impure) Web-Audio plumbing so the maths is unit-tested.
 */

/** Root-mean-square of a time-domain sample buffer (values in [-1, 1]). */
export function rms(buf: Float32Array | number[]): number {
  const n = buf.length;
  if (n === 0) return 0;
  let sum = 0;
  for (let i = 0; i < n; i++) sum += buf[i] * buf[i];
  return Math.sqrt(sum / n);
}

/** Convert a linear RMS amplitude to dBFS. Near-zero maps to `silentDb`
 *  (instead of `-Infinity`) so callers get a finite floor. */
export function rmsToDbfs(amplitude: number, silentDb = -120): number {
  return amplitude > 1e-5 ? 20 * Math.log10(amplitude) : silentDb;
}

/** Map a dBFS value onto a 0..1 gauge within `[floorDb, ceilDb]` (clamped).
 *  Below the floor → 0, above the ceiling → 1. */
export function dbfsToLevel(db: number, floorDb: number, ceilDb: number): number {
  if (ceilDb <= floorDb) return 0;
  const x = (db - floorDb) / (ceilDb - floorDb);
  return x < 0 ? 0 : x > 1 ? 1 : x;
}

/** One attack/release smoothing step toward `target`: rises fast (`attack`),
 *  falls slow (`release`). Both factors in [0, 1]. */
export function smoothStep(current: number, target: number, attack: number, release: number): number {
  const a = target > current ? attack : release;
  return current + (target - current) * a;
}
