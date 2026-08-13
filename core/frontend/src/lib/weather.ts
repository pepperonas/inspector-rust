/**
 * Pure, unit-tested helpers for the `weather` command's inline panel. The
 * component (`WeatherPanel.tsx`) owns only the animated rendering; everything
 * deterministic (labels, unit conversion, wind compass, date formatting) lives
 * here so it can be tested without a DOM or a live API.
 */
import type { WeatherKind } from "./ipc";

/** Short human label per sky kind (English UI). */
export const WEATHER_LABELS: Record<WeatherKind, string> = {
  "clear-day": "Clear",
  "clear-night": "Clear night",
  clouds: "Cloudy",
  drizzle: "Drizzle",
  rain: "Rain",
  thunderstorm: "Thunderstorm",
  snow: "Snow",
  mist: "Mist",
};

/** A two-stop background gradient (top → bottom) evoking each sky kind. Drives
 *  the scene backdrop; values are CSS colours. */
export const WEATHER_GRADIENT: Record<WeatherKind, [string, string]> = {
  "clear-day": ["#4aa3e8", "#8fd0ff"],
  "clear-night": ["#141a35", "#2b3566"],
  clouds: ["#5b6b7d", "#93a3b3"],
  drizzle: ["#4a5b6d", "#71838f"],
  rain: ["#3c4a5a", "#5a6b7b"],
  thunderstorm: ["#2a2f45", "#4a4363"],
  snow: ["#6b7c8f", "#c3d2e0"],
  mist: ["#6a7480", "#9aa4ac"],
};

/** m/s → km/h, rounded to a whole number. */
export function mpsToKmh(mps: number): number {
  return Math.round(mps * 3.6);
}

/** Meteorological degrees (0 = N) → an 8-point compass label. `null` → "". */
export function windCompass(deg: number | null): string {
  if (deg == null || !Number.isFinite(deg)) return "";
  const points = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
  const idx = Math.round((((deg % 360) + 360) % 360) / 45) % 8;
  return points[idx];
}

/** Round a temperature to a whole number for display, avoiding "-0". */
export function roundTemp(t: number): number {
  const r = Math.round(t);
  return r === 0 ? 0 : r;
}

/** Parse a `YYYY-MM-DD` string into a **local** Date (no timezone shift), or
 *  `null` if malformed. */
export function parseYmd(date: string): Date | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  if (!m) return null;
  const y = Number(m[1]);
  const mo = Number(m[2]);
  const d = Number(m[3]);
  if (mo < 1 || mo > 12 || d < 1 || d > 31) return null;
  return new Date(y, mo - 1, d);
}

/** Localized short weekday for a `YYYY-MM-DD` (e.g. "Wed"); "" if malformed. */
export function dayName(date: string): string {
  const d = parseYmd(date);
  if (!d) return "";
  return new Intl.DateTimeFormat(undefined, { weekday: "short" }).format(d);
}

/** Whether a `YYYY-MM-DD` is today's local date. `now` injectable for tests. */
export function isToday(date: string, now: Date = new Date()): boolean {
  const d = parseYmd(date);
  if (!d) return false;
  return (
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate()
  );
}

/** Whether the kind is a night scene (drives the star field / moon). */
export function isNight(kind: WeatherKind): boolean {
  return kind === "clear-night";
}

/** Whether the scene should render falling precipitation (rain/drizzle/storm). */
export function hasPrecip(kind: WeatherKind): boolean {
  return kind === "rain" || kind === "drizzle" || kind === "thunderstorm";
}

/** Hour label for an hourly slot, in the LOCATION's local time: the slot's
 *  UTC seconds shifted by the report's `tz_offset`, rendered as "HH:00".
 *  Pure — `weather tokyo` labels the strip in Tokyo hours, not the
 *  machine's. */
export function hourLabel(dt: number, tzOffset: number): string {
  const h = new Date((dt + tzOffset) * 1000).getUTCHours();
  return `${String(h).padStart(2, "0")}:00`;
}
