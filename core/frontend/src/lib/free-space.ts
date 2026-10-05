/** Free disk space for the popup footer (v0.195.0). Pure helpers. */

export type FreeSpaceLevel = "ok" | "warn" | "crit";

const GB = 1e9;

/** Below this the footer turns red: builds, updates and swap start failing. */
export const CRIT_BYTES = 5 * GB;
/** Below this (or under 10 % of the disk) it turns amber. */
export const WARN_BYTES = 20 * GB;
const WARN_SHARE = 0.1;
const CRIT_SHARE = 0.02;

export function freeSpaceLevel(available: number, total: number): FreeSpaceLevel {
  const share = total > 0 ? available / total : 1;
  if (available < CRIT_BYTES || share < CRIT_SHARE) return "crit";
  if (available < WARN_BYTES || share < WARN_SHARE) return "warn";
  return "ok";
}

/**
 * Decimal units (1 GB = 10⁹ bytes) like macOS System Settings, German
 * number format. `digits` = fixed decimals; omitted = compact (1 decimal
 * below 10, none above), which is what the footer shows.
 */
export function formatBytesDecimal(bytes: number, digits?: number): string {
  const b = Math.max(0, bytes);
  const units: [number, string][] = [
    [1e12, "TB"],
    [1e9, "GB"],
    [1e6, "MB"],
    [1e3, "kB"],
  ];
  for (const [size, unit] of units) {
    if (b >= size) {
      const v = b / size;
      const d = digits ?? (v < 10 ? 1 : 0);
      return `${v.toLocaleString("de-DE", { minimumFractionDigits: d, maximumFractionDigits: d })} ${unit}`;
    }
  }
  return `${Math.round(b)} B`;
}

/** Tooltip in System Settings' wording: "8,3 GB verfügbar von 494,38 GB". */
export function freeSpaceTitle(name: string, available: number, total: number): string {
  const head = `${formatBytesDecimal(available, available < 10 * GB ? 1 : 2)} verfügbar von ${formatBytesDecimal(total, 2)}`;
  return name ? `${name} — ${head}` : head;
}
