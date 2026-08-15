import { describe, it, expect } from "vitest";
import { tryConvert } from "./convert";

describe("tryConvert — unit conversions", () => {
  it("length km → mi", () => {
    const r = tryConvert("5 km in mi");
    expect(r?.value).toBeCloseTo(3.106856, 5);
    expect(r?.display).toBe("3.106856 mi");
  });
  it("length mi → km (round trip-ish)", () => {
    expect(tryConvert("1 mi in km")?.display).toBe("1.609344 km");
  });
  it("mass kg → lb", () => {
    expect(tryConvert("10 kg to lb")?.value).toBeCloseTo(22.046226, 4);
  });
  it("data gb → mb (binary 1024)", () => {
    expect(tryConvert("2 gb in mb")?.display).toBe("2048 mb");
  });
  it("time h → min", () => {
    expect(tryConvert("1.5 h in min")?.display).toBe("90 min");
  });
  it("speed kmh → mph", () => {
    expect(tryConvert("100 kmh in mph")?.value).toBeCloseTo(62.137119, 4);
  });
  it("tolerates no space between number and unit", () => {
    expect(tryConvert("5km in mi")?.value).toBeCloseTo(3.106856, 5);
  });
  it("rejects cross-category conversions", () => {
    expect(tryConvert("5 km in kg")).toBeNull();
    expect(tryConvert("5 kg in mi")).toBeNull();
  });
  it("rejects unknown units", () => {
    expect(tryConvert("5 furlong in mi")).toBeNull();
  });
});

describe("tryConvert — temperature", () => {
  it("F → C", () => {
    expect(tryConvert("212 f in c")?.value).toBeCloseTo(100, 6);
    expect(tryConvert("32 f to c")?.value).toBeCloseTo(0, 6);
  });
  it("C → F", () => {
    expect(tryConvert("100 c in f")?.value).toBeCloseTo(212, 6);
  });
  it("C → K", () => {
    expect(tryConvert("0 c in k")?.value).toBeCloseTo(273.15, 6);
  });
  it("K → C", () => {
    expect(tryConvert("273.15 k in c")?.value).toBeCloseTo(0, 6);
    expect(tryConvert("373.15 k to c")?.value).toBeCloseTo(100, 6);
  });
  it("F → K (cross-unit, both conversion arms)", () => {
    // 32 °F = 0 °C = 273.15 K
    expect(tryConvert("32 f in k")?.value).toBeCloseTo(273.15, 6);
  });
  it("accepts a degree sign", () => {
    expect(tryConvert("100 °c in °f")?.value).toBeCloseTo(212, 6);
  });
});

describe("tryConvert — number base", () => {
  it("hex → dec", () => {
    expect(tryConvert("0xff in dec")?.display).toBe("255");
    expect(tryConvert("0xFF to decimal")?.value).toBe(255);
  });
  it("dec → hex", () => {
    expect(tryConvert("255 in hex")?.display).toBe("0xff");
  });
  it("bin → dec and dec → bin", () => {
    expect(tryConvert("0b1010 in dec")?.value).toBe(10);
    expect(tryConvert("10 in bin")?.display).toBe("0b1010");
  });
  it("oct → dec", () => {
    expect(tryConvert("0o17 in dec")?.value).toBe(15);
  });
  it("rejects invalid digits for the source base", () => {
    expect(tryConvert("0xZZ in dec")).toBeNull();
  });
});

describe("tryConvert — epoch", () => {
  it("unix seconds → ISO date", () => {
    const r = tryConvert("1717000000 as date");
    expect(r?.display).toBe("2024-05-29T16:26:40.000Z");
    expect(r?.value).toBe(1717000000);
  });
  it("unix millis → ISO date", () => {
    expect(tryConvert("1717000000000 as date")?.display).toBe(
      "2024-05-29T16:26:40.000Z",
    );
  });
});

describe("tryConvert — non-matches", () => {
  it("returns null for math, plain text, and empty", () => {
    expect(tryConvert("2 + 2")).toBeNull();
    expect(tryConvert("hello world")).toBeNull();
    expect(tryConvert("")).toBeNull();
    expect(tryConvert("   ")).toBeNull();
  });
  it("the `expression` echoes the trimmed input for pasteable provenance", () => {
    expect(tryConvert("  5 km in mi ")?.expression).toBe("5 km in mi");
  });
});

describe("tryConvert — the value slot must be an integer literal", () => {
  // `parseIntLike` returns null for these; the row must then fall through the
  // remaining grammars and end up as "not a conversion" rather than showing a
  // NaN result.
  it("a bare radix prefix with no digits is not a number", () => {
    expect(tryConvert("0x in dec")).toBeNull();
    expect(tryConvert("0b in hex")).toBeNull();
    expect(tryConvert("0o to bin")).toBeNull();
  });

  it("a word in the value slot converts nothing", () => {
    expect(tryConvert("hello in hex")).toBeNull();
    expect(tryConvert("- in dec")).toBeNull();
  });

  it("a fractional value has no base representation", () => {
    expect(tryConvert("2.5 in hex")).toBeNull();
  });

  it("a lone minus sign is not a negative number", () => {
    expect(tryConvert("-  in hex")).toBeNull();
  });

  it("negative decimal input is accepted (the sign is part of the literal)", () => {
    expect(tryConvert("-10 in dec")?.value).toBe(-10);
  });
});

describe("tryConvert — the `in` unit vs the `in` keyword", () => {
  // `in` is both the inch unit and the conversion keyword — the single
  // riskiest ambiguity in this grammar.
  it("converts inches when `in` sits in the unit slot", () => {
    expect(tryConvert("5 in in cm")?.display).toBe("12.7 cm");
  });

  it("the spelled-out alias works the same", () => {
    expect(tryConvert("1 inch to cm")?.display).toBe("2.54 cm");
  });

  it("converting TO inches keeps the keyword unambiguous", () => {
    expect(tryConvert("2.54 cm to in")?.display).toBe("1 in");
  });
});

describe("tryConvert — grammar boundaries", () => {
  it("a missing source unit is not a conversion (both units are required)", () => {
    expect(tryConvert("1000 in m")).toBeNull();
    expect(tryConvert("5 in kg")).toBeNull();
  });

  it("a unit-shaped target that isn't a unit yields nothing", () => {
    expect(tryConvert("5 m in dec")).toBeNull();
  });

  it("uppercase input is normalised, and the display echoes the lowercase unit", () => {
    const r = tryConvert("5 KM IN MI");
    expect(r?.value).toBeCloseTo(3.106856, 5);
    expect(r?.display).toBe("3.106856 mi");
  });

  it("`as` is only a keyword for the base + epoch grammars, not for units", () => {
    // The unit grammar deliberately accepts only in/to.
    expect(tryConvert("5 km as mi")).toBeNull();
  });
});

describe("tryConvert — temperature sign handling", () => {
  it("the −40 crossing point maps to itself", () => {
    expect(tryConvert("-40 c in f")?.value).toBeCloseTo(-40, 6);
    expect(tryConvert("-40 f in c")?.value).toBeCloseTo(-40, 6);
  });

  it("absolute zero round-trips through Kelvin", () => {
    expect(tryConvert("-273.15 c in k")?.value).toBeCloseTo(0, 6);
    expect(tryConvert("0 k in c")?.value).toBeCloseTo(-273.15, 6);
  });

  it("K → F goes through Celsius (both conversion arms in one call)", () => {
    // 373.15 K = 100 °C = 212 °F
    expect(tryConvert("373.15 k in f")?.value).toBeCloseTo(212, 6);
  });

  it("same-unit temperature is the identity", () => {
    expect(tryConvert("21 c in c")?.value).toBe(21);
  });
});

describe("tryConvert — unit aliases + exact factors", () => {
  it("one stone is exactly fourteen pounds", () => {
    expect(tryConvert("1 st in lb")?.display).toBe("14 lb");
  });

  it("one knot is exactly 1.852 km/h", () => {
    expect(tryConvert("1 kn in kmh")?.display).toBe("1.852 kmh");
  });

  it("the time aliases (sec / hr / day / wk) all resolve", () => {
    expect(tryConvert("120 sec in min")?.display).toBe("2 min");
    expect(tryConvert("1 hr in min")?.display).toBe("60 min");
    expect(tryConvert("1 day in h")?.display).toBe("24 h");
    expect(tryConvert("1 wk in d")?.display).toBe("7 d");
  });

  it("one mile is exactly 1609344 mm (no float dust in the display)", () => {
    expect(tryConvert("1 mi in mm")?.display).toBe("1609344 mm");
  });

  it("rounds to six decimals and drops the trailing zeros", () => {
    expect(tryConvert("0.5 km in m")?.display).toBe("500 m");
    // 1 / 1609.344 = 0.000621371… → six decimals.
    expect(tryConvert("1 m in mi")?.display).toBe("0.000621 mi");
  });
});

describe("tryConvert — epoch grammar", () => {
  it("accepts every keyword/target spelling", () => {
    for (const q of [
      "1717000000 as date",
      "1717000000 to date",
      "1717000000 in date",
      "1717000000 as iso",
      "1717000000 as utc",
    ]) {
      expect(tryConvert(q)?.display).toBe("2024-05-29T16:26:40.000Z");
    }
  });

  it("13 digits is the milliseconds boundary; 12 is still seconds", () => {
    // Exactly 13 digits → milliseconds.
    expect(tryConvert("1000000000000 as date")?.value).toBe(1000000000);
    // 12 digits → seconds (×1000), i.e. a date far in the future.
    expect(tryConvert("100000000000 as date")?.value).toBe(100000000000);
  });

  it("9 digits is the shortest accepted epoch, 8 is not an epoch at all", () => {
    expect(tryConvert("100000000 as date")?.display).toBe("1973-03-03T09:46:40.000Z");
    expect(tryConvert("10000000 as date")).toBeNull();
  });

  it("an epoch asked for a BASE is a base conversion, not a date", () => {
    // `<digits> in hex` must keep working — the base grammar is checked first.
    // 1717000000 === 0x66575740 (independently verified, not copied from the impl).
    expect(tryConvert("1717000000 in hex")?.display).toBe("0x66575740");
    expect(parseInt("66575740", 16)).toBe(1717000000);
  });
});

describe("tryConvert — magnitude robustness", () => {
  it("an absurd magnitude never renders NaN and never throws", () => {
    // The unit grammar has no exponent form, so this is what an overflow
    // actually looks like when a user pastes a huge digit run.
    const huge = "1" + "0".repeat(320);
    let out: ReturnType<typeof tryConvert>;
    expect(() => {
      out = tryConvert(`${huge} t in mg`);
    }).not.toThrow();
    expect(out!.display).not.toContain("NaN");
  });

  it("zero converts cleanly in every category", () => {
    expect(tryConvert("0 km in mi")?.display).toBe("0 mi");
    expect(tryConvert("0 gb in mb")?.display).toBe("0 mb");
  });
});

describe("tryConvert — octal + negative bases", () => {
  it("oct → dec accepts the 0o prefix", () => {
    expect(tryConvert("0o17 in dec")?.display).toBe("15");
  });

  it("dec → oct formats with the 0o prefix", () => {
    expect(tryConvert("15 in oct")?.display).toBe("0o17");
  });

  it("negative dec → hex keeps the sign in front of the prefix", () => {
    expect(tryConvert("-255 in hex")?.display).toBe("-0xff");
  });

  it("negative dec → bin", () => {
    expect(tryConvert("-5 in bin")?.display).toBe("-0b101");
  });

  it("long-form base aliases work (hexadecimal/binary/octal/decimal)", () => {
    expect(tryConvert("255 in hexadecimal")?.display).toBe("0xff");
    expect(tryConvert("0xff in decimal")?.display).toBe("255");
    expect(tryConvert("5 in binary")?.display).toBe("0b101");
    expect(tryConvert("9 in octal")?.display).toBe("0o11");
  });
});
