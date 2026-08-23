/**
 * Pure display helpers for the footer's system-sleep indicator (v0.114.0).
 * The data comes from `get_sleep_status` (pmset, macOS); this module only
 * formats — see `sleep_status.rs` for parsing and the powerd filter rule.
 */

/**
 * Format a countdown in seconds as `m:ss` (below one hour) or `h:mm:ss`.
 * Negative values clamp to `0:00` — the footer ticks locally between polls
 * and must stop at zero rather than count into nonsense (the next poll
 * corrects the value anyway).
 */
export function formatSleepCountdown(secs: number): string {
  const total = Math.max(0, Math.floor(secs));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

/** Tooltip body naming the holders, e.g. "caffeinate ×4, sharingd". */
export function formatHolders(holders: readonly string[]): string {
  return holders.join(", ");
}
