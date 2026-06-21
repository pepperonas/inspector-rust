/**
 * Pure helpers for the Timesheet tab + (later) the HTML export — formatting,
 * SVG geometry for the inline charts, day math, a stable colour palette. Kept
 * dependency-free + unit-tested so the React tab and the self-contained HTML
 * export share the same building blocks.
 */

/** Seconds → "2h 14m" / "45m" / "12s" (compact, human). */
export function formatDuration(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (h > 0) return m > 0 ? `${h}h ${m}m` : `${h}h`;
  if (m > 0) return `${m}m`;
  return `${s}s`;
}

/** Unix ms → local "HH:MM". */
export function formatClock(ms: number): string {
  const d = new Date(ms);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

/** A local "YYYY-MM-DD" for a Date (default: now). */
export function localDateStr(d: Date = new Date()): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** Shift a "YYYY-MM-DD" by `delta` days (local), returning a new "YYYY-MM-DD". */
export function shiftDay(date: string, delta: number): string {
  const [y, m, d] = date.split("-").map((n) => parseInt(n, 10));
  const dt = new Date(y, (m || 1) - 1, d || 1);
  dt.setDate(dt.getDate() + delta);
  return localDateStr(dt);
}

/** Local midnight (unix ms) of a "YYYY-MM-DD". */
export function dayStartMs(date: string): number {
  const [y, m, d] = date.split("-").map((n) => parseInt(n, 10));
  return new Date(y, (m || 1) - 1, d || 1, 0, 0, 0, 0).getTime();
}

/** The ISO week (Mon–Sun) containing `date` as `{from, to}` ("YYYY-MM-DD"). */
export function weekBounds(date: string): { from: string; to: string } {
  const [y, m, d] = date.split("-").map((n) => parseInt(n, 10));
  const dt = new Date(y, (m || 1) - 1, d || 1);
  const dow = (dt.getDay() + 6) % 7; // Mon=0 … Sun=6
  const mon = new Date(dt);
  mon.setDate(dt.getDate() - dow);
  const sun = new Date(mon);
  sun.setDate(mon.getDate() + 6);
  return { from: localDateStr(mon), to: localDateStr(sun) };
}

/** Shift a date by `delta` weeks (returns a "YYYY-MM-DD"). */
export function shiftWeek(date: string, delta: number): string {
  return shiftDay(date, delta * 7);
}

/** "Mon, Jun 16" short label for a "YYYY-MM-DD". */
export function shortDayLabel(date: string): string {
  const [y, m, d] = date.split("-").map((n) => parseInt(n, 10));
  const dt = new Date(y, (m || 1) - 1, d || 1);
  return dt.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
}

/** Unix ms → "HH:MM" for a `<input type=time>` (local). */
export function msToTimeInput(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** "HH:MM" within the local day starting at `dayStart` → unix ms; preserves the
 *  event's seconds (so editing only the minute doesn't snap to :00). `null` for
 *  malformed input. */
export function timeInputToMs(dayStart: number, value: string, keepSecondsFrom?: number): number | null {
  const m = value.match(/^(\d{1,2}):(\d{2})$/);
  if (!m) return null;
  const h = parseInt(m[1], 10);
  const min = parseInt(m[2], 10);
  if (h > 23 || min > 59) return null;
  const secMs =
    keepSecondsFrom != null ? ((keepSecondsFrom % 60_000) + 60_000) % 60_000 : 0;
  return dayStart + h * 3_600_000 + min * 60_000 + secMs;
}

/** A fixed, dark-theme-friendly categorical palette (cycles for >N keys). */
const PALETTE = [
  "#b3c5ff", // accent-ish
  "#7dd3fc", // sky
  "#86efac", // green
  "#fcd34d", // amber
  "#f9a8d4", // pink
  "#c4b5fd", // violet
  "#fdba74", // orange
  "#5eead4", // teal
  "#a3e635", // lime
  "#f87171", // red
];
export function paletteColor(i: number): string {
  return PALETTE[((i % PALETTE.length) + PALETTE.length) % PALETTE.length];
}

/** FNV-1a hash → unsigned 32-bit, for deterministic colour assignment. */
export function hashString(s: string): number {
  let h = 2166136261 >>> 0;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619) >>> 0;
  }
  return h >>> 0;
}

/** A **stable** colour for a category name — the same category is always the
 *  same colour across day/week/all charts (hash → palette). "Uncategorized" is
 *  rendered muted/grey. */
export function categoryColor(name: string): string {
  if (name === "Uncategorized" || name === "") return "var(--color-muted)";
  return paletteColor(hashString(name));
}

/** A category→colour map for a set of category keys (stable per name). */
export function categoryColorMap(keys: string[]): Record<string, string> {
  const m: Record<string, string> = {};
  for (const k of keys) m[k] = categoryColor(k);
  return m;
}

/** Build a stable key→colour map from an ordered list of keys (top-first). */
export function colorMap(keys: string[]): Record<string, string> {
  const m: Record<string, string> = {};
  keys.forEach((k, i) => {
    m[k] = paletteColor(i);
  });
  return m;
}

/** A point on a circle. Angle in degrees, 0° at 12 o'clock, clockwise. */
function polar(cx: number, cy: number, r: number, angleDeg: number): [number, number] {
  const a = ((angleDeg - 90) * Math.PI) / 180;
  return [cx + r * Math.cos(a), cy + r * Math.sin(a)];
}

/** SVG path for a donut segment (ring sector) between two angles (degrees). */
export function donutSegmentPath(
  cx: number,
  cy: number,
  rOuter: number,
  rInner: number,
  startDeg: number,
  endDeg: number,
): string {
  // Avoid a zero/full-circle degenerate path.
  const sweep = endDeg - startDeg;
  const eps = sweep >= 360 ? -0.001 : 0;
  const [ox0, oy0] = polar(cx, cy, rOuter, startDeg);
  const [ox1, oy1] = polar(cx, cy, rOuter, endDeg + eps);
  const [ix1, iy1] = polar(cx, cy, rInner, endDeg + eps);
  const [ix0, iy0] = polar(cx, cy, rInner, startDeg);
  const largeArc = sweep % 360 > 180 ? 1 : 0;
  return [
    `M ${ox0.toFixed(2)} ${oy0.toFixed(2)}`,
    `A ${rOuter} ${rOuter} 0 ${largeArc} 1 ${ox1.toFixed(2)} ${oy1.toFixed(2)}`,
    `L ${ix1.toFixed(2)} ${iy1.toFixed(2)}`,
    `A ${rInner} ${rInner} 0 ${largeArc} 0 ${ix0.toFixed(2)} ${iy0.toFixed(2)}`,
    "Z",
  ].join(" ");
}

/** Cumulative donut segments (start/end degrees) for a list of values. */
export function donutSegments(values: number[]): { start: number; end: number }[] {
  const total = values.reduce((a, b) => a + b, 0);
  if (total <= 0) return [];
  let acc = 0;
  return values.map((v) => {
    const start = (acc / total) * 360;
    acc += v;
    const end = (acc / total) * 360;
    return { start, end };
  });
}

/** Map an event interval to a [left%, width%] band within a day window. Clips
 *  to the day; open events (ended null) extend to `now`. Returns null for
 *  zero-width. */
export function timelineBand(
  startedAt: number,
  endedAt: number | null,
  dayStart: number,
  now: number,
): { leftPct: number; widthPct: number } | null {
  const DAY = 86_400_000;
  const dayEnd = dayStart + DAY;
  const s = Math.max(startedAt, dayStart);
  const e = Math.min(endedAt ?? now, dayEnd);
  if (e <= s) return null;
  return {
    leftPct: ((s - dayStart) / DAY) * 100,
    widthPct: ((e - s) / DAY) * 100,
  };
}
