import { describe, it, expect } from "vitest";
import {
  fmtClock,
  parseClock,
  clampTime,
  moveHandle,
  moveRange,
  fullRange,
  isFullRange,
  sectionFor,
  timeAtX,
  pctAt,
  nudgeStep,
  MIN_LEN,
} from "./trim-range";

// The reference video from the brief: a DJ set of 4956 s.
const DUR = 4956;

describe("sectionFor — the additive contract", () => {
  it("returns undefined for an untouched range, so the download is unchanged", () => {
    // ⚠️ This is the load-bearing rule: without a narrowed range the download
    // must be byte-for-byte what it was before the trim feature existed.
    expect(sectionFor(fullRange(DUR), DUR, true)).toBeUndefined();
    expect(sectionFor({ start: 0, end: DUR }, DUR, true)).toBeUndefined();
  });

  it("returns undefined while the trim panel is closed, whatever the range says", () => {
    expect(sectionFor({ start: 600, end: 620 }, DUR, false)).toBeUndefined();
  });

  it("hands over a narrowed range, rounded to milliseconds", () => {
    expect(sectionFor({ start: 600.0004, end: 620.5006 }, DUR, true)).toEqual([600, 620.501]);
  });

  it("refuses a range shorter than the minimum instead of cutting nothing", () => {
    expect(sectionFor({ start: 10, end: 10.4 }, DUR, true)).toBeUndefined();
    expect(sectionFor({ start: 10, end: 10 + MIN_LEN }, DUR, true)).toEqual([10, 10 + MIN_LEN]);
  });

  it("returns undefined when the duration is unknown", () => {
    expect(sectionFor({ start: 5, end: 20 }, 0, true)).toBeUndefined();
  });
});

describe("moveHandle", () => {
  it("keeps start before end by PUSHING, never by swapping", () => {
    // ⚠️ A swap mid-drag makes the bar feel like it fights the pointer.
    const r = moveHandle({ start: 100, end: 200 }, "start", 500, DUR);
    expect(r.start).toBeLessThan(r.end);
    expect(r.end - r.start).toBeGreaterThanOrEqual(MIN_LEN);
    const q = moveHandle({ start: 100, end: 200 }, "end", 20, DUR);
    expect(q.start).toBeLessThan(q.end);
    expect(q.end - q.start).toBeGreaterThanOrEqual(MIN_LEN);
  });

  it("clamps to the media bounds", () => {
    expect(moveHandle({ start: 100, end: 200 }, "start", -50, DUR).start).toBe(0);
    expect(moveHandle({ start: 100, end: 200 }, "end", DUR + 999, DUR).end).toBe(DUR);
  });

  it("moves the handle it was asked to move", () => {
    expect(moveHandle({ start: 100, end: 200 }, "start", 150, DUR)).toEqual({ start: 150, end: 200 });
    expect(moveHandle({ start: 100, end: 200 }, "end", 180, DUR)).toEqual({ start: 100, end: 180 });
  });
});

describe("moveRange", () => {
  it("slides without changing the length", () => {
    expect(moveRange({ start: 100, end: 200 }, 50, DUR)).toEqual({ start: 150, end: 250 });
  });

  it("stops at both ends instead of shrinking", () => {
    const atStart = moveRange({ start: 10, end: 110 }, -500, DUR);
    expect(atStart).toEqual({ start: 0, end: 100 });
    const atEnd = moveRange({ start: DUR - 110, end: DUR - 10 }, 500, DUR);
    expect(atEnd.end).toBe(DUR);
    expect(atEnd.end - atEnd.start).toBe(100);
  });
});

describe("fmtClock / parseClock", () => {
  it("formats to m:ss and grows to h:mm:ss", () => {
    expect(fmtClock(0)).toBe("0:00");
    expect(fmtClock(83)).toBe("1:23");
    expect(fmtClock(DUR)).toBe("1:22:36");
    expect(fmtClock(83, true)).toBe("0:01:23");
  });

  it("reads the shapes a person types", () => {
    expect(parseClock("1:23")).toBe(83);
    expect(parseClock("1:02:03")).toBe(3723);
    expect(parseClock("90")).toBe(90);
    expect(parseClock("1:23.5")).toBe(83.5);
  });

  it("returns null for anything it cannot read, rather than 0", () => {
    // ⚠️ A half-typed field must not jump the handle to the start.
    expect(parseClock("")).toBeNull();
    expect(parseClock(":")).toBeNull();
    expect(parseClock("abc")).toBeNull();
    expect(parseClock("1:2:3:4")).toBeNull();
    expect(parseClock("-5")).toBeNull();
  });

  it("round-trips through the formatter", () => {
    for (const s of [0, 59, 60, 83, 3599, 3600, DUR]) {
      expect(parseClock(fmtClock(s))).toBe(s);
    }
  });
});

describe("track geometry", () => {
  it("maps pointer x to time and back", () => {
    expect(timeAtX(100, 100, 400, DUR)).toBe(0);
    expect(timeAtX(500, 100, 400, DUR)).toBe(DUR);
    expect(timeAtX(300, 100, 400, DUR)).toBeCloseTo(DUR / 2, 5);
    // Outside the track clamps rather than running past the media.
    expect(timeAtX(0, 100, 400, DUR)).toBe(0);
    expect(timeAtX(9999, 100, 400, DUR)).toBe(DUR);
  });

  it("survives a zero-width track and an unknown duration", () => {
    expect(timeAtX(100, 0, 0, DUR)).toBe(0);
    expect(timeAtX(100, 0, 400, 0)).toBe(0);
    expect(pctAt(50, 0)).toBe(0);
  });

  it("gives percentages for CSS", () => {
    expect(pctAt(0, DUR)).toBe(0);
    expect(pctAt(DUR, DUR)).toBe(100);
    expect(pctAt(DUR / 4, DUR)).toBe(25);
  });
});

describe("misc", () => {
  it("clamps time into the media", () => {
    expect(clampTime(-1, DUR)).toBe(0);
    expect(clampTime(DUR + 1, DUR)).toBe(DUR);
    expect(clampTime(5, 0)).toBe(0);
  });

  it("recognises the untouched range", () => {
    expect(isFullRange(fullRange(DUR), DUR)).toBe(true);
    expect(isFullRange({ start: 1, end: DUR }, DUR)).toBe(false);
    expect(isFullRange({ start: 0, end: DUR - 1 }, DUR)).toBe(false);
  });

  it("scales the keyboard step to the material, finer with Shift", () => {
    // A 5 s step on an 83-minute set; 1 s on a short clip; 0.1 s with Shift.
    expect(nudgeStep(DUR, false)).toBe(5);
    expect(nudgeStep(45, false)).toBe(1);
    expect(nudgeStep(DUR, true)).toBe(0.1);
  });
});
