import { describe, expect, it } from "vitest";
import {
  CATEGORIES,
  UNITS,
  convertAll,
  convertBetween,
  formatValue,
  lookupUnit,
  suggestUnits,
  unitById,
  unitKeyCollisions,
  unitsOf,
  type Rates,
} from "./units";

const U = (id: string) => {
  const x = unitById(id);
  if (!x) throw new Error(`no unit ${id}`);
  return x;
};
const conv = (v: number, a: string, b: string, rates?: Rates) =>
  convertBetween(v, U(a), U(b), rates) as number;

const RATES: Rates = { EUR: 1, USD: 1 / 1.08, GBP: 1 / 0.85, BTC: 58000, JPY: 1 / 160 };

describe("registry structure", () => {
  it("no spelling maps to two units (an ambiguous alias would silently pick one)", () => {
    expect(unitKeyCollisions()).toEqual([]);
  });

  it("every category has units and its default unit exists in it", () => {
    for (const c of CATEGORIES) {
      expect(UNITS.some((x) => x.cat === c.id)).toBe(true);
      expect(unitById(c.defaultUnit)?.cat).toBe(c.id);
    }
  });

  it("every non-currency unit is convertible (factor or both functions)", () => {
    for (const x of UNITS.filter((x) => x.cat !== "currency")) {
      const linear = typeof x.factor === "number" && x.factor > 0;
      expect(linear || (!!x.toBase && !!x.fromBase), x.id).toBe(true);
    }
  });

  it("the full category set is present (user decision: all fourteen)", () => {
    expect(CATEGORIES.map((c) => c.id)).toEqual([
      "length", "area", "volume", "mass", "temperature", "speed", "time",
      "data", "energy", "power", "pressure", "fuel", "angle", "currency",
    ]);
  });
});

describe("lookup", () => {
  it("resolves ids, symbols and aliases case-insensitively", () => {
    expect(lookupUnit("MPH")?.id).toBe("mph");
    expect(lookupUnit("km/h")?.id).toBe("kmh");
    expect(lookupUnit("kph")?.id).toBe("kmh");
    expect(lookupUnit("Knoten")?.id).toBe("kn");
    expect(lookupUnit("°F")?.id).toBe("f");
    expect(lookupUnit("m²")?.id).toBe("m2");
    expect(lookupUnit("fl oz")?.id).toBe("floz");
    expect(lookupUnit("€")?.id).toBe("eur");
    expect(lookupUnit("$")?.id).toBe("usd");
    expect(lookupUnit("bitcoin")?.id).toBe("btc");
  });

  it("a lone ° is an angle, not a temperature", () => {
    expect(lookupUnit("°")?.id).toBe("deg");
  });

  it("unknown tokens resolve to nothing", () => {
    expect(lookupUnit("furlong")).toBeNull();
    expect(lookupUnit("")).toBeNull();
  });
});

describe("reference values (independently computed)", () => {
  it("165 mph in every speed unit", () => {
    expect(conv(165, "mph", "kmh")).toBeCloseTo(265.54176, 9);
    expect(conv(165, "mph", "mps")).toBeCloseTo(73.7616, 9);
    expect(conv(165, "mph", "kn")).toBeCloseTo(143.3810799, 6);
  });

  it("temperature is affine, not proportional", () => {
    expect(conv(72, "f", "c")).toBeCloseTo(22.2222222, 6);
    expect(conv(-40, "c", "f")).toBeCloseTo(-40, 9);
    expect(conv(0, "k", "c")).toBeCloseTo(-273.15, 9);
    expect(conv(373.15, "k", "f")).toBeCloseTo(212, 9);
  });

  it("fuel consumption is reciprocal", () => {
    expect(conv(5, "l100km", "mpg")).toBeCloseTo(47.0429167, 6);
    expect(conv(20, "kml", "l100km")).toBeCloseTo(5, 9);
  });

  it("data keeps SI and IEC apart", () => {
    expect(conv(2, "gib", "gb")).toBeCloseTo(2.147483648, 9);
    expect(conv(1, "gb", "mb")).toBe(1000);
    expect(conv(1, "b", "bit")).toBe(8);
  });

  it("exact definitions stay exact", () => {
    expect(conv(1, "mi", "m")).toBe(1609.344);
    expect(conv(1, "st", "lb")).toBeCloseTo(14, 12);
    expect(conv(1, "kn", "kmh")).toBeCloseTo(1.852, 12);
    expect(conv(1, "kwh", "j")).toBe(3.6e6);
    expect(conv(1, "atm", "hpa")).toBe(1013.25);
    expect(conv(1, "turn", "rad")).toBeCloseTo(2 * Math.PI, 12);
  });

  it("round-trips every unit through its category's default", () => {
    for (const x of UNITS.filter((x) => x.cat !== "currency")) {
      const def = unitById(CATEGORIES.find((c) => c.id === x.cat)!.defaultUnit)!;
      const there = convertBetween(7.5, x, def) as number;
      const back = convertBetween(there, def, x) as number;
      expect(back, x.id).toBeCloseTo(7.5, 9);
    }
  });

  it("refuses to convert across categories", () => {
    expect(convertBetween(1, U("km"), U("kg"))).toBeNull();
    expect(convertBetween(1, U("mph"), U("h"))).toBeNull();
  });
});

describe("currency", () => {
  it("converts through EUR with the supplied rates", () => {
    expect(conv(100, "usd", "eur", RATES)).toBeCloseTo(92.5925926, 6);
    expect(conv(1, "eur", "usd", RATES)).toBeCloseTo(1.08, 9);
    expect(conv(0.01, "btc", "eur", RATES)).toBeCloseTo(580, 9);
  });

  it("a currency without a rate is not converted and not listed — never 0", () => {
    expect(convertBetween(1, U("chf"), U("eur"), RATES)).toBeNull();
    const ids = convertAll(1, U("eur"), RATES).map((r) => r.unit.id);
    expect(ids).toContain("usd");
    expect(ids).not.toContain("chf");
    expect(unitsOf("currency", {}).length).toBe(0);
  });

  it("without any rates currencies convert to nothing", () => {
    expect(convertBetween(1, U("usd"), U("eur"))).toBeNull();
  });
});

describe("convertAll", () => {
  it("lists the whole category, source included, in registry order", () => {
    const rows = convertAll(165, U("mph"));
    expect(rows.map((r) => r.unit.id)).toEqual(["kmh", "mph", "mps", "kn", "fps"]);
    expect(rows.find((r) => r.unit.id === "mph")!.value).toBe(165);
  });
});

describe("suggestUnits", () => {
  it("prefix matches come first", () => {
    const ids = suggestUnits("mp").map((x) => x.id);
    expect(ids[0]).toBe("mph");
    expect(ids).toContain("mpg");
  });

  it("can be restricted to one category and exclude the source", () => {
    const ids = suggestUnits("k", { cat: "speed", exclude: "kmh" }).map((x) => x.id);
    expect(ids).toEqual(["kn"]);
  });

  it("drops currencies without a rate when rates are given", () => {
    const ids = suggestUnits("ch", { rates: RATES }).map((x) => x.id);
    expect(ids).not.toContain("chf");
  });
});

describe("formatValue", () => {
  it("German separators and seven significant digits", () => {
    expect(formatValue(265.54176)).toBe("265,5418");
    expect(formatValue(1609344)).toBe("1.609.344");
  });

  it("money gets two decimals, crypto up to eight significant digits", () => {
    expect(formatValue(1.5, U("eur"))).toBe("1,50");
    expect(formatValue(0.000123456789, U("btc"))).toBe("0,00012345679");
  });

  it("never shows -0, NaN or Infinity", () => {
    expect(formatValue(-0)).toBe("0");
    expect(formatValue(NaN)).toBe("—");
    expect(formatValue(Infinity)).toBe("—");
  });

  it("switches to scientific notation only at the extremes", () => {
    expect(formatValue(1e-9)).toBe("1·10⁻⁹");
    expect(formatValue(2.5e18)).toBe("2,5·10¹⁸");
    expect(formatValue(0.5)).toBe("0,5");
  });
});
