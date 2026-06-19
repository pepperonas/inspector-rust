/**
 * Pure formatting helpers for the `stats` panel — byte sizes, transfer rates,
 * durations. Kept out of the component so they're unit-testable.
 */

/** Bytes → a human size like "512 MB", "1.4 GB" (binary units, 1024-based). */
export function humanBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  // <10 → one decimal (e.g. "1.4 GB"), otherwise whole number ("512 MB").
  const digits = i === 0 ? 0 : v < 10 ? 1 : 0;
  return `${v.toFixed(digits)} ${units[i]}`;
}

/** Bytes-per-second → "1.2 MB/s" (uses {@link humanBytes}). */
export function humanRate(bytesPerSec: number): string {
  return `${humanBytes(bytesPerSec)}/s`;
}

/** Seconds → a compact uptime like "3d 4h", "4h 12m", "12m", "45s". */
export function humanUptime(secs: number): string {
  if (!Number.isFinite(secs) || secs <= 0) return "0m";
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m`;
  return `${Math.floor(secs)}s`;
}

/** Seconds → "2h 15m" / "45m" / "30s" — for battery time-remaining. */
export function humanDuration(secs: number): string {
  if (!Number.isFinite(secs) || secs <= 0) return "—";
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m`;
  return `${Math.floor(secs)}s`;
}

/** Clamp a percentage to [0, 100] for bar widths. */
export function clampPct(p: number): number {
  if (!Number.isFinite(p)) return 0;
  return Math.max(0, Math.min(100, p));
}

/** Used/total → an integer percentage (0 when total is 0). */
export function usedPct(used: number, total: number): number {
  if (!Number.isFinite(used) || !Number.isFinite(total) || total <= 0) return 0;
  return clampPct((used / total) * 100);
}
