import { describe, expect, it } from "vitest";
import {
  addMonths,
  dayDelta,
  daysInMonth,
  isoWeek,
  mondayIndex,
  monthMatrix,
  parseCalendarArg,
} from "./calendar";

describe("month math", () => {
  it("knows month lengths incl. leap years", () => {
    expect(daysInMonth(2026, 6)).toBe(31); // July 2026
    expect(daysInMonth(2024, 1)).toBe(29); // Feb 2024 (leap)
    expect(daysInMonth(2025, 1)).toBe(28);
    expect(daysInMonth(2000, 1)).toBe(29); // 400-rule leap
    expect(daysInMonth(1900, 1)).toBe(28); // 100-rule non-leap
  });

  it("monday-based weekday index", () => {
    expect(mondayIndex(2026, 6, 8)).toBe(2); // 2026-07-08 is a Wednesday
    expect(mondayIndex(2026, 6, 6)).toBe(0); // Monday
    expect(mondayIndex(2026, 6, 12)).toBe(6); // Sunday
  });

  it("addMonths wraps across year boundaries both ways", () => {
    expect(addMonths(2026, 11, 1)).toEqual({ year: 2027, month: 0 });
    expect(addMonths(2026, 0, -1)).toEqual({ year: 2025, month: 11 });
    expect(addMonths(2026, 6, -18)).toEqual({ year: 2025, month: 0 });
    expect(addMonths(2026, 6, 12)).toEqual({ year: 2027, month: 6 });
  });
});

describe("monthMatrix", () => {
  it("builds July 2026 (starts Wed → 2 leading neighbours, 5 rows)", () => {
    const rows = monthMatrix(2026, 6);
    expect(rows).toHaveLength(5);
    // 1 July 2026 is a Wednesday → Mon/Tue cells are 29/30 June.
    expect(rows[0].days[0]).toEqual({ day: 29, year: 2026, month: 5, inMonth: false });
    expect(rows[0].days[2]).toEqual({ day: 1, year: 2026, month: 6, inMonth: true });
    // Last row ends with trailing August days.
    const last = rows[4].days[6];
    expect(last.inMonth).toBe(false);
    expect(last.month).toBe(7);
    // Every row has exactly 7 days.
    for (const r of rows) expect(r.days).toHaveLength(7);
  });

  it("a Monday-starting 28-day February needs exactly 4 rows, no neighbours", () => {
    // Feb 2027 starts on a Monday and has 28 days.
    const rows = monthMatrix(2027, 1);
    expect(rows).toHaveLength(4);
    expect(rows.every((r) => r.days.every((d) => d.inMonth))).toBe(true);
  });

  it("carries correct ISO week numbers across a year boundary", () => {
    // Jan 2026: the 1st is a Thursday → week 1; the row containing
    // 29 Dec 2025 (Mon) belongs to ISO week 1 of 2026 as well.
    const rows = monthMatrix(2026, 0);
    expect(rows[0].isoWeek).toBe(1);
    // 2021-01-01 was a Friday → its row's Thursday is 31 Dec 2020 → ISO week 53.
    expect(monthMatrix(2021, 0)[0].isoWeek).toBe(53);
  });

  it("isoWeek matches known fixtures", () => {
    expect(isoWeek(2026, 6, 8)).toBe(28); // 2026-07-08
    expect(isoWeek(2024, 0, 1)).toBe(1);
    expect(isoWeek(2023, 0, 1)).toBe(52); // Sunday → belongs to 2022's last week
  });
});

describe("dayDelta", () => {
  it("computes signed whole-day distances", () => {
    const ref = { year: 2026, month: 6, day: 8 };
    expect(dayDelta(ref, { year: 2026, month: 6, day: 8 })).toBe(0);
    expect(dayDelta(ref, { year: 2026, month: 6, day: 10 })).toBe(2);
    expect(dayDelta(ref, { year: 2026, month: 5, day: 8 })).toBe(-30);
    expect(dayDelta(ref, { year: 2027, month: 6, day: 8 })).toBe(365);
  });
});

describe("parseCalendarArg", () => {
  const Y = 2026;
  it("empty / junk → null", () => {
    expect(parseCalendarArg("", Y)).toBeNull();
    expect(parseCalendarArg("gibberish", Y)).toBeNull();
    expect(parseCalendarArg("13.2024", Y)).toBeNull(); // month out of range
    expect(parseCalendarArg("2024-00", Y)).toBeNull();
  });
  it("bare year → January of it", () => {
    expect(parseCalendarArg("1990", Y)).toEqual({ year: 1990, month: 0 });
  });
  it("numeric month/year forms", () => {
    expect(parseCalendarArg("3.2024", Y)).toEqual({ year: 2024, month: 2 });
    expect(parseCalendarArg("03/2024", Y)).toEqual({ year: 2024, month: 2 });
    expect(parseCalendarArg("2024-03", Y)).toEqual({ year: 2024, month: 2 });
    expect(parseCalendarArg("12/2003", Y)).toEqual({ year: 2003, month: 11 });
  });
  it("German + English month names, optional year, either order", () => {
    expect(parseCalendarArg("märz", Y)).toEqual({ year: Y, month: 2 });
    expect(parseCalendarArg("maerz 1990", Y)).toEqual({ year: 1990, month: 2 });
    expect(parseCalendarArg("May 2025", Y)).toEqual({ year: 2025, month: 4 });
    expect(parseCalendarArg("2003 dez", Y)).toEqual({ year: 2003, month: 11 });
    expect(parseCalendarArg("okt", Y)).toEqual({ year: Y, month: 9 });
  });
});
