import { describe, it, expect } from "vitest";
import {
  CITY_ZONES,
  DEFAULT_ZONES,
  matchCities,
  normalizeZones,
  zoneByTz,
  tzFallbackCity,
  zoneTime,
  dayDelta,
} from "./clock";

describe("catalogue", () => {
  it("every entry has a plausible IANA tz and unique id", () => {
    const ids = new Set<string>();
    for (const z of CITY_ZONES) {
      expect(z.tz).toMatch(/^[A-Za-z]+(?:\/[A-Za-z0-9_+-]+){1,2}$/);
      expect(z.city.length).toBeGreaterThan(0);
      expect(ids.has(z.tz)).toBe(false);
      ids.add(z.tz);
    }
  });

  it("defaults are all real catalogue zones", () => {
    for (const tz of DEFAULT_ZONES) {
      expect(zoneByTz(tz)).toBeTruthy();
    }
  });

  it("tzFallbackCity humanises an unlisted zone id", () => {
    expect(tzFallbackCity("Pacific/Chatham")).toBe("Chatham");
    expect(tzFallbackCity("America/Argentina/Buenos_Aires")).toBe("Buenos Aires");
  });
});

describe("matchCities (autocomplete)", () => {
  it("prefix beats substring beats region/tz, excludes added, empty→[]", () => {
    expect(matchCities("", [])).toEqual([]);
    const berlin = matchCities("berl", []);
    expect(berlin[0].city).toBe("Berlin");
    // Region match: "japan" → Tokio (region "Japan").
    expect(matchCities("japan", []).some((z) => z.city === "Tokio")).toBe(true);
    // tz-id match: any IANA zone reachable by typing the path.
    expect(matchCities("auckland", []).some((z) => z.tz === "Pacific/Auckland")).toBe(true);
    // Already-added zones are filtered out.
    expect(matchCities("berlin", ["Europe/Berlin"]).some((z) => z.tz === "Europe/Berlin")).toBe(false);
  });

  it("honours the limit", () => {
    expect(matchCities("a", [], 3).length).toBeLessThanOrEqual(3);
  });
});

describe("normalizeZones", () => {
  it("dedupes, drops junk, keeps valid unlisted IANA ids, defaults on garbage", () => {
    expect(normalizeZones("nope")).toEqual([...DEFAULT_ZONES]);
    expect(normalizeZones([1, 2, 3])).toEqual([]);
    expect(normalizeZones(["Europe/Berlin", "Europe/Berlin", "Asia/Tokyo"])).toEqual([
      "Europe/Berlin",
      "Asia/Tokyo",
    ]);
    // A valid IANA id not in the catalogue survives (user added by tz).
    expect(normalizeZones(["Pacific/Chatham"])).toEqual(["Pacific/Chatham"]);
    // Shell-junk / spaces are rejected.
    expect(normalizeZones(["Europe/Berlin; rm", "x y"])).toEqual([]);
  });
});

describe("zoneTime (Intl-backed)", () => {
  // A fixed instant: 2026-08-25 12:00:00 UTC.
  const instant = new Date("2026-08-25T12:00:00Z");

  it("formats HH:MM in the target zone", () => {
    // Tokyo is UTC+9 → 21:00; New York UTC-4 (DST) → 08:00.
    expect(zoneTime(instant, "Asia/Tokyo").time).toBe("21:00");
    expect(zoneTime(instant, "America/New_York").time).toBe("08:00");
    expect(zoneTime(instant, "UTC").time).toBe("12:00");
  });

  it("computes the day-delta chip across the date line (ref pinned to UTC)", () => {
    // dayDelta is relative to a reference zone; the machine zone is unknowable
    // in CI, so tests pin refTz = "UTC" for determinism.
    // At 12:00 UTC everyone shares the 25th → delta 0.
    expect(dayDelta(instant, "Asia/Tokyo", "UTC")).toBe(0);
    // At 23:00 UTC, Tokyo (08:00 next day) is +1 vs UTC's 25th.
    const late = new Date("2026-08-25T23:00:00Z");
    expect(dayDelta(late, "Asia/Tokyo", "UTC")).toBe(1);
    // At 02:00 UTC, Los Angeles (19:00 on the 24th) is -1 vs UTC's 25th.
    const early = new Date("2026-08-25T02:00:00Z");
    expect(dayDelta(early, "America/Los_Angeles", "UTC")).toBe(-1);
  });

  it("marks night correctly (before 6:00 / from 20:00)", () => {
    // 12:00 UTC → Tokyo 21:00 = night; London 13:00 = day.
    expect(zoneTime(instant, "Asia/Tokyo").night).toBe(true);
    expect(zoneTime(instant, "Europe/London").night).toBe(false);
  });

  it("reports a UTC offset label", () => {
    expect(zoneTime(instant, "UTC").offset).toMatch(/^UTC/);
    expect(zoneTime(instant, "Asia/Kolkata").offset).toContain("UTC");
  });

  it("bad tz ids fall back instead of throwing", () => {
    const t = zoneTime(instant, "Not/AZone");
    expect(t.time).toBe("--:--");
  });
});
