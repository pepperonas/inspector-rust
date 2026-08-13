import { describe, it, expect } from "vitest";
import {
  WEATHER_LABELS,
  WEATHER_GRADIENT,
  mpsToKmh,
  windCompass,
  roundTemp,
  parseYmd,
  dayName,
  isToday,
  isNight,
  hasPrecip,
  hourLabel,
} from "./weather";
import type { WeatherKind } from "./ipc";

const ALL_KINDS: WeatherKind[] = [
  "clear-day",
  "clear-night",
  "clouds",
  "drizzle",
  "rain",
  "thunderstorm",
  "snow",
  "mist",
];

describe("weather helpers", () => {
  it("has a label + gradient for every kind", () => {
    for (const k of ALL_KINDS) {
      expect(WEATHER_LABELS[k]).toBeTruthy();
      expect(WEATHER_GRADIENT[k]).toHaveLength(2);
    }
  });

  it("converts m/s to km/h", () => {
    expect(mpsToKmh(0)).toBe(0);
    expect(mpsToKmh(10)).toBe(36);
    expect(mpsToKmh(3.6)).toBe(13); // 12.96 → 13
  });

  it("maps wind degrees to an 8-point compass", () => {
    expect(windCompass(0)).toBe("N");
    expect(windCompass(45)).toBe("NE");
    expect(windCompass(90)).toBe("E");
    expect(windCompass(180)).toBe("S");
    expect(windCompass(270)).toBe("W");
    expect(windCompass(359)).toBe("N"); // wraps to N
    expect(windCompass(-45)).toBe("NW"); // negative normalises
    expect(windCompass(null)).toBe("");
  });

  it("rounds temps and never shows -0", () => {
    expect(roundTemp(18.4)).toBe(18);
    expect(roundTemp(18.6)).toBe(19);
    expect(roundTemp(-0.3)).toBe(0);
    expect(Object.is(roundTemp(-0.3), -0)).toBe(false);
  });

  it("parses YYYY-MM-DD to a local date, rejecting garbage", () => {
    const d = parseYmd("2026-07-29");
    expect(d?.getFullYear()).toBe(2026);
    expect(d?.getMonth()).toBe(6); // July = 6
    expect(d?.getDate()).toBe(29);
    expect(parseYmd("bad")).toBeNull();
    expect(parseYmd("2026-13-01")).toBeNull();
    expect(parseYmd("2026-07-32")).toBeNull();
  });

  it("returns a weekday name (or empty for garbage)", () => {
    // 2026-07-29 is a Wednesday.
    expect(dayName("2026-07-29")).toMatch(/\w+/);
    expect(dayName("nope")).toBe("");
  });

  it("detects today against an injected clock", () => {
    const now = new Date(2026, 6, 29, 15, 0, 0);
    expect(isToday("2026-07-29", now)).toBe(true);
    expect(isToday("2026-07-30", now)).toBe(false);
    expect(isToday("bad", now)).toBe(false);
  });

  it("classifies night + precipitation", () => {
    expect(isNight("clear-night")).toBe(true);
    expect(isNight("clear-day")).toBe(false);
    expect(hasPrecip("rain")).toBe(true);
    expect(hasPrecip("drizzle")).toBe(true);
    expect(hasPrecip("thunderstorm")).toBe(true);
    expect(hasPrecip("snow")).toBe(false);
    expect(hasPrecip("clear-day")).toBe(false);
  });
});

describe("hourLabel", () => {
  it("labels a slot in the LOCATION's local time via tz offset", () => {
    // 2026-08-13 12:00:00 UTC …
    const noonUtc = Date.UTC(2026, 7, 13, 12, 0, 0) / 1000;
    expect(hourLabel(noonUtc, 0)).toBe("12:00");
    expect(hourLabel(noonUtc, 7200)).toBe("14:00"); // Berlin summer
    expect(hourLabel(noonUtc, 9 * 3600)).toBe("21:00"); // Tokyo
    expect(hourLabel(noonUtc, -5 * 3600)).toBe("07:00"); // NYC (negative offset)
  });
  it("pads single-digit hours and wraps across midnight", () => {
    const late = Date.UTC(2026, 7, 13, 23, 0, 0) / 1000;
    expect(hourLabel(late, 7200)).toBe("01:00"); // wraps to the next day
    expect(hourLabel(Date.UTC(2026, 7, 13, 6, 0, 0) / 1000, 0)).toBe("06:00");
  });
});
