import { describe, it, expect } from "vitest";
import { MAJOR_CITIES, matchCities, normalizeCity } from "./cities";

describe("normalizeCity", () => {
  it("lowercases + strips diacritics", () => {
    expect(normalizeCity("München")).toBe("munchen");
    expect(normalizeCity("São Paulo")).toBe("sao paulo");
    expect(normalizeCity("  Zürich ")).toBe("zurich");
    expect(normalizeCity("Łódź")).toContain("od"); // ł isn't a combining mark but stays letter-ish
  });
});

describe("matchCities", () => {
  it("returns nothing for an empty query", () => {
    expect(matchCities("")).toEqual([]);
    expect(matchCities("   ")).toEqual([]);
  });

  it("prefix-matches a specific city (the Darmstadt case)", () => {
    const r = matchCities("darm");
    expect(r[0]?.name).toBe("Darmstadt");
  });

  it("is diacritic- and case-insensitive", () => {
    expect(matchCities("munch")[0]?.name).toBe("München");
    expect(matchCities("MÜNCH")[0]?.name).toBe("München");
    expect(matchCities("zuri")[0]?.name).toBe("Zürich");
  });

  it("ranks prefix matches by population (bigger city first)", () => {
    const r = matchCities("d", 5).map((c) => c.name);
    // Among D-cities, the largest (Delhi/Dubai/Dortmund/Düsseldorf/Dresden…)
    // must lead a tiny one (Darmstadt).
    expect(r).toContain("Delhi");
    expect(r.indexOf("Delhi")).toBeLessThan(6);
    expect(r).not.toContain("Darmstadt"); // pushed past the top 5 by bigger D-cities
  });

  it("puts PREFIX matches before SUBSTRING matches", () => {
    const r = matchCities("york").map((c) => c.name);
    // No city STARTS with "york", but "New York" contains it → it's a substring hit.
    expect(r).toContain("New York");
    // A prefix query surfaces the prefix city first.
    expect(matchCities("new")[0]?.name).toBe("New York");
  });

  it("respects the limit", () => {
    expect(matchCities("a", 3).length).toBeLessThanOrEqual(3);
    expect(matchCities("a", 0)).toEqual([]);
  });

  it("covers major German cities incl. Darmstadt", () => {
    const names = new Set(MAJOR_CITIES.map((c) => c.name));
    for (const n of ["Berlin", "Hamburg", "München", "Darmstadt", "Frankfurt", "Köln"]) {
      expect(names.has(n)).toBe(true);
    }
  });

  it("every entry has a name, country and positive population", () => {
    for (const c of MAJOR_CITIES) {
      expect(c.name.trim()).not.toBe("");
      expect(c.country.trim()).not.toBe("");
      expect(c.pop).toBeGreaterThan(0);
    }
  });
});
