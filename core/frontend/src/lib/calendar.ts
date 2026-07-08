/**
 * Pure date math for the `calendar` command's month-view panel.
 *
 * Everything here is deterministic and unit-tested; the component
 * (`CalendarPanel.tsx`) owns only rendering + keyboard handling. Weeks start
 * on Monday (ISO 8601), week numbers are ISO week numbers.
 */

/** One cell of the month grid. */
export interface DayCell {
  /** Day of month (1-based) — of `year`/`month` below, not necessarily the viewed month. */
  day: number;
  /** Full year + 0-based month this cell belongs to (leading/trailing neighbours differ). */
  year: number;
  month: number;
  /** Whether the cell belongs to the viewed month (neighbours render dimmed). */
  inMonth: boolean;
}

export interface WeekRow {
  /** ISO 8601 week number of this row. */
  isoWeek: number;
  days: DayCell[]; // always 7, Monday first
}

/** Days in `month` (0-based) of `year`. */
export function daysInMonth(year: number, month: number): number {
  return new Date(year, month + 1, 0).getDate();
}

/** Monday-based weekday index (Mon = 0 … Sun = 6) of a date. */
export function mondayIndex(year: number, month: number, day: number): number {
  return (new Date(year, month, day).getDay() + 6) % 7;
}

/** ISO 8601 week number (1..53) of a date. */
export function isoWeek(year: number, month: number, day: number): number {
  // Thursday of the same ISO week decides the week-year; count weeks from Jan 1.
  const date = new Date(Date.UTC(year, month, day));
  const dow = (date.getUTCDay() + 6) % 7; // Mon = 0
  date.setUTCDate(date.getUTCDate() - dow + 3); // → that week's Thursday
  const jan1 = new Date(Date.UTC(date.getUTCFullYear(), 0, 1));
  return Math.floor(1 + (date.getTime() - jan1.getTime()) / (7 * 86_400_000));
}

/** Month navigation: `(year, month0)` shifted by `delta` months. */
export function addMonths(
  year: number,
  month: number,
  delta: number,
): { year: number; month: number } {
  const t = year * 12 + month + delta;
  return { year: Math.floor(t / 12), month: ((t % 12) + 12) % 12 };
}

/**
 * The full month grid: Monday-started weeks covering the month, each row with
 * its ISO week number. Leading/trailing cells come from the neighbour months
 * (marked `inMonth: false`). 4–6 rows depending on the month.
 */
export function monthMatrix(year: number, month: number): WeekRow[] {
  const lead = mondayIndex(year, month, 1);
  const total = daysInMonth(year, month);
  const prev = addMonths(year, month, -1);
  const next = addMonths(year, month, 1);
  const prevTotal = daysInMonth(prev.year, prev.month);

  const cells: DayCell[] = [];
  for (let i = lead - 1; i >= 0; i--) {
    cells.push({
      day: prevTotal - i,
      year: prev.year,
      month: prev.month,
      inMonth: false,
    });
  }
  for (let d = 1; d <= total; d++) {
    cells.push({ day: d, year, month, inMonth: true });
  }
  let d = 1;
  while (cells.length % 7 !== 0) {
    cells.push({ day: d++, year: next.year, month: next.month, inMonth: false });
  }

  const rows: WeekRow[] = [];
  for (let i = 0; i < cells.length; i += 7) {
    const days = cells.slice(i, i + 7);
    // The row's ISO week is decided by its Thursday (index 3).
    const th = days[3];
    rows.push({ isoWeek: isoWeek(th.year, th.month, th.day), days });
  }
  return rows;
}

/** Whole-day difference `target - reference` (both at local midnight). */
export function dayDelta(
  ref: { year: number; month: number; day: number },
  target: { year: number; month: number; day: number },
): number {
  const a = Date.UTC(ref.year, ref.month, ref.day);
  const b = Date.UTC(target.year, target.month, target.day);
  return Math.round((b - a) / 86_400_000);
}

// ── Argument parsing (`calendar märz 1990`, `cal 3.2024`, …) ────────────────

/** German + English month names / abbreviations → 0-based month. */
const MONTH_NAMES: Record<string, number> = {};
const MONTH_TABLE: [string[], number][] = [
  [["januar", "january", "jan"], 0],
  [["februar", "february", "feb"], 1],
  [["märz", "maerz", "march", "mar", "mrz"], 2],
  [["april", "apr"], 3],
  [["mai", "may"], 4],
  [["juni", "june", "jun"], 5],
  [["juli", "july", "jul"], 6],
  [["august", "aug"], 7],
  [["september", "sep", "sept"], 8],
  [["oktober", "october", "oct", "okt"], 9],
  [["november", "nov"], 10],
  [["dezember", "december", "dec", "dez"], 11],
];
for (const [names, m] of MONTH_TABLE) {
  for (const n of names) MONTH_NAMES[n] = m;
}

/**
 * Parse the optional command argument into a target month. Accepted forms
 * (case-insensitive, whitespace-tolerant):
 *
 *   `2024`            → January 2024
 *   `3.2024` `3/2024` → March 2024 (also `03.2024`)
 *   `2024-03`         → March 2024 (ISO)
 *   `märz` / `may`    → that month of the current year
 *   `märz 1990` / `may 2025` / `1990 märz` → that month + year
 *
 * Returns null for an empty / unrecognised argument (panel stays on the
 * current view).
 */
export function parseCalendarArg(
  arg: string,
  nowYear: number,
): { year: number; month: number } | null {
  const s = arg.trim().toLowerCase();
  if (!s) return null;

  // ISO `YYYY-MM` / `YYYY/MM` / `YYYY.MM`
  let m = s.match(/^(\d{4})[-/.](\d{1,2})$/);
  if (m) {
    const month = Number(m[2]) - 1;
    return month >= 0 && month <= 11 ? { year: Number(m[1]), month } : null;
  }
  // `M.YYYY` / `M/YYYY` / `M-YYYY`
  m = s.match(/^(\d{1,2})[-/.](\d{4})$/);
  if (m) {
    const month = Number(m[1]) - 1;
    return month >= 0 && month <= 11 ? { year: Number(m[2]), month } : null;
  }
  // Bare year
  m = s.match(/^(\d{4})$/);
  if (m) return { year: Number(m[1]), month: 0 };

  // Month name [year] — in either order.
  const parts = s.split(/\s+/);
  if (parts.length <= 2) {
    let month: number | undefined;
    let year: number | undefined;
    for (const p of parts) {
      if (/^\d{4}$/.test(p)) year = Number(p);
      else if (MONTH_NAMES[p] !== undefined) month = MONTH_NAMES[p];
      else return null;
    }
    if (month !== undefined) return { year: year ?? nowYear, month };
    if (year !== undefined) return { year, month: 0 };
  }
  return null;
}
