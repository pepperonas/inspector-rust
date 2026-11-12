/**
 * Pure formatting helpers for the `stats` panel — byte sizes, transfer rates,
 * durations. Kept out of the component so they're unit-testable.
 */

import { locale } from "./i18n";

const NUMBER_FORMATS = new Map<string, Intl.NumberFormat>();

/** A number with exactly `digits` decimals in the app language ("1,4" in
 *  German, "1.4" in English); thousands grouped unless `group` is false. */
export function fmtNum(v: number, digits = 0, group = true): string {
  const key = `${locale()}|${digits}|${group}`;
  let f = NUMBER_FORMATS.get(key);
  if (!f) {
    f = new Intl.NumberFormat(locale(), { minimumFractionDigits: digits, maximumFractionDigits: digits, useGrouping: group });
    NUMBER_FORMATS.set(key, f);
  }
  const n = Number.isFinite(v) ? v : 0;
  // Never "-0": a value that rounds to zero is zero.
  const scale = Math.pow(10, digits);
  return f.format(Math.round(n * scale) === 0 ? 0 : n);
}

const NBSP = "\u00a0";

/** Percent: "42 %" in German (with a non-breaking space), "42%" in English. */
export function fmtPct(v: number, digits = 0): string {
  return `${fmtNum(v, digits)}${locale().startsWith("de") ? NBSP : ""}%`;
}

/** A value with its unit, one non-breaking space apart ("56,4 °C", "12,3 W"). */
export function fmtUnit(v: number, unit: string, digits = 1): string {
  return `${fmtNum(v, digits)}${NBSP}${unit}`;
}

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
  // Sizes stay ungrouped ("1023 B"): they never get long enough to need it.
  return `${fmtNum(v, digits, false)} ${units[i]}`;
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
