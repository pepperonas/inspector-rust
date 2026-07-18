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

describe("parseCalendarArg — word-form edge cases", () => {
  const Y = 2026;
  it("month name + junk word → null (each part must be a year or month)", () => {
    expect(parseCalendarArg("märz kartoffel", Y)).toBeNull();
  });
  it("more than two words → null", () => {
    expect(parseCalendarArg("märz 1990 extra", Y)).toBeNull();
  });
  it("two bare years → the last one wins, month defaults to January", () => {
    expect(parseCalendarArg("1990 2000", Y)).toEqual({ year: 2000, month: 0 });
  });
  it("is case-insensitive and whitespace-tolerant", () => {
    expect(parseCalendarArg("  MÄRZ   1990  ", Y)).toEqual({ year: 1990, month: 2 });
    expect(parseCalendarArg("SEPT", Y)).toEqual({ year: Y, month: 8 });
  });
  it("month range boundaries of the numeric forms", () => {
    expect(parseCalendarArg("1.2024", Y)).toEqual({ year: 2024, month: 0 });
    expect(parseCalendarArg("12.2024", Y)).toEqual({ year: 2024, month: 11 });
    expect(parseCalendarArg("0.2024", Y)).toBeNull();
    expect(parseCalendarArg("2024-13", Y)).toBeNull();
    expect(parseCalendarArg("2024-12", Y)).toEqual({ year: 2024, month: 11 });
  });
  it("2-digit years are not accepted (avoids ambiguous '24')", () => {
    expect(parseCalendarArg("24", Y)).toBeNull();
    expect(parseCalendarArg("3.24", Y)).toBeNull();
  });
  it("every German + English month alias resolves", () => {
    const cases: [string, number][] = [
      ["januar", 0], ["january", 0], ["jan", 0],
      ["februar", 1], ["february", 1], ["feb", 1],
      ["märz", 2], ["maerz", 2], ["march", 2], ["mar", 2], ["mrz", 2],
      ["april", 3], ["apr", 3],
      ["mai", 4], ["may", 4],
      ["juni", 5], ["june", 5], ["jun", 5],
      ["juli", 6], ["july", 6], ["jul", 6],
      ["august", 7], ["aug", 7],
      ["september", 8], ["sep", 8], ["sept", 8],
      ["oktober", 9], ["october", 9], ["oct", 9], ["okt", 9],
      ["november", 10], ["nov", 10],
      ["dezember", 11], ["december", 11], ["dec", 11], ["dez", 11],
    ];
    for (const [name, month] of cases) {
      expect(parseCalendarArg(name, Y)).toEqual({ year: Y, month });
    }
  });
});

describe("monthMatrix — structural invariants across many months", () => {
  it("every month 2020–2030 yields 4–6 rows of 7 contiguous days", () => {
    for (let year = 2020; year <= 2030; year++) {
      for (let month = 0; month < 12; month++) {
        const rows = monthMatrix(year, month);
        expect(rows.length).toBeGreaterThanOrEqual(4);
        expect(rows.length).toBeLessThanOrEqual(6);
        const flat = rows.flatMap((r) => r.days);
        expect(flat.length % 7).toBe(0);
        // Exactly daysInMonth in-month cells, numbered 1..N in order.
        const inMonth = flat.filter((d) => d.inMonth);
        expect(inMonth.length).toBe(daysInMonth(year, month));
        expect(inMonth[0].day).toBe(1);
        expect(inMonth[inMonth.length - 1].day).toBe(daysInMonth(year, month));
        // The first cell is always a Monday.
        expect(mondayIndex(flat[0].year, flat[0].month, flat[0].day)).toBe(0);
      }
    }
  });

  it("a 31-day month starting on Sunday needs 6 rows (max lead)", () => {
    // March 2026 starts on a Sunday → 6 leading neighbour cells.
    const rows = monthMatrix(2026, 2);
    expect(rows).toHaveLength(6);
    expect(rows[0].days.filter((d) => !d.inMonth)).toHaveLength(6);
  });
});

describe("isoWeek — year-boundary fixtures", () => {
  it("week 53 years and week-1 spillover", () => {
    expect(isoWeek(2020, 11, 31)).toBe(53); // 2020 has 53 ISO weeks
    expect(isoWeek(2021, 0, 3)).toBe(53); // Sunday of that same week
    expect(isoWeek(2021, 0, 4)).toBe(1); // Monday starts week 1
    expect(isoWeek(2025, 11, 29)).toBe(1); // 29 Dec 2025 already belongs to 2026-W01
  });
});

describe("dayDelta — leap-day and DST-immune arithmetic", () => {
  it("counts across a leap day", () => {
    expect(
      dayDelta({ year: 2024, month: 1, day: 28 }, { year: 2024, month: 2, day: 1 }),
    ).toBe(2); // 29 Feb exists
    expect(
      dayDelta({ year: 2025, month: 1, day: 28 }, { year: 2025, month: 2, day: 1 }),
    ).toBe(1);
  });
  it("is symmetric", () => {
    const a = { year: 2026, month: 0, day: 1 };
    const b = { year: 2026, month: 11, day: 31 };
    expect(dayDelta(a, b)).toBe(-dayDelta(b, a));
    expect(dayDelta(a, b)).toBe(364);
  });
  it("crosses the DST transitions as whole days (UTC-based math)", () => {
    // Europe/Berlin DST starts 2026-03-29 — a naive local-ms diff would be off.
    expect(
      dayDelta({ year: 2026, month: 2, day: 28 }, { year: 2026, month: 2, day: 30 }),
    ).toBe(2);
  });
});
