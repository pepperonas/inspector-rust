import { describe, expect, it } from "vitest";
import {
  buildArg,
  convertSuggestions,
  counterpart,
  headline,
  parseConvertArg,
  parseValue,
  pasteNumber,
  type ConvertParse,
} from "./convert-cmd";
import { unitById, type Rates } from "./units";

const RATES: Rates = { EUR: 1, USD: 1 / 1.08, BTC: 58000 };
type Ready = Extract<ConvertParse, { kind: "ready" }>;
const ready = (arg: string): Ready => {
  const p = parseConvertArg(arg);
  if (p.kind !== "ready") throw new Error(`${arg} → ${p.kind}`);
  return p;
};

describe("parseConvertArg", () => {
  it("empty is the category browser", () => {
    expect(parseConvertArg("  ").kind).toBe("browse");
  });

  it("value + unit, with or without a space", () => {
    for (const arg of ["165 mph", "165mph", "165 MPH"]) {
      const p = ready(arg);
      expect(p.value).toBe(165);
      expect(p.unit.id).toBe("mph");
      expect(p.targetOpen).toBe(false);
    }
  });

  it("a target after in / to / nach / -> / → / =", () => {
    for (const arg of ["165 mph in kmh", "165 mph to km/h", "165 mph nach kph", "165mph->kmh", "165 mph → kmh", "165 mph = kmh"]) {
      expect(ready(arg).target?.id, arg).toBe("kmh");
    }
  });

  it("`in` is inches until a complete source precedes it", () => {
    expect(ready("5 in").unit.id).toBe("in");
    const p = ready("5 in in cm");
    expect(p.unit.id).toBe("in");
    expect(p.target?.id).toBe("cm");
  });

  it("an open separator without target is reported for autocomplete", () => {
    const p = ready("165 mph in ");
    expect(p.targetOpen).toBe(true);
    expect(p.target).toBeNull();
    expect(ready("165 mph in k").targetText).toBe("k");
  });

  it("a target from another category is not accepted as target", () => {
    const p = ready("165 mph in kg");
    expect(p.targetOpen).toBe(true);
    expect(p.target).toBeNull();
  });

  it("currency symbols before or after the number", () => {
    expect(ready("20€").unit.id).toBe("eur");
    expect(ready("$5").unit.id).toBe("usd");
    expect(ready("$5").value).toBe(5);
    expect(ready("€ 12,50").value).toBe(12.5);
  });

  it("a unit alone means one of it", () => {
    const p = ready("mph");
    expect(p.value).toBe(1);
    expect(p.valueText).toBe("");
  });

  it("an arithmetic value", () => {
    expect(ready("2*60 km").value).toBe(120);
    expect(ready("(3+4)/2 kg").value).toBe(3.5);
  });

  it("number without unit, unknown unit, nonsense", () => {
    expect(parseConvertArg("165").kind).toBe("value");
    const p = parseConvertArg("165 mp");
    expect(p.kind).toBe("partial");
    expect(p.kind === "partial" && p.unitText).toBe("mp");
    expect(parseConvertArg("1..2 km").kind).toBe("invalid");
  });

  it("negative values", () => {
    expect(ready("-40 c in f").value).toBe(-40);
  });
});

describe("parseValue", () => {
  it("both decimal conventions", () => {
    expect(parseValue("2,5")).toBe(2.5);
    expect(parseValue("2.5")).toBe(2.5);
    expect(parseValue("1.234,5")).toBe(1234.5);
    expect(parseValue("1,234.5")).toBe(1234.5);
    expect(parseValue("1.234.567")).toBe(1234567);
    expect(parseValue("1'000")).toBe(1000);
  });

  it("rejects garbage", () => {
    expect(parseValue("")).toBeNull();
    expect(parseValue("1..2")).toBeNull();
    expect(parseValue("1.23.4")).toBeNull();
  });
});

describe("headline", () => {
  it("compares with the natural counterpart when no target is typed", () => {
    const h = headline(ready("165 mph"))!;
    expect(h.toUnit.id).toBe("kmh");
    expect(h.to).toBe("265,5418 km/h");
    expect(h.from).toBe("165 mph");
  });

  it("uses the typed target", () => {
    expect(headline(ready("165 mph in kn"))!.toUnit.id).toBe("kn");
  });

  it("currency needs rates — none means no headline", () => {
    expect(headline(ready("100 usd"))).toBeNull();
    expect(headline(ready("100 usd"), RATES)!.to).toBe("92,59 EUR");
  });

  it("the counterpart is never the source itself", () => {
    expect(counterpart(unitById("kmh")!).id).toBe("mph");
    expect(counterpart(unitById("eur")!, RATES).id).toBe("usd");
  });
});

describe("convertSuggestions", () => {
  it("an unfinished unit offers completions with a live sample", () => {
    const s = convertSuggestions(parseConvertArg("165 mp"));
    expect(s[0].unit.id).toBe("mph");
    expect(s[0].arg).toBe("165 mph ");
    expect(s[0].sample).toBe("Geschwindigkeit · 265,5418 km/h");
  });

  it("a complete short unit still offers the LONGER units it starts", () => {
    const ids = convertSuggestions(parseConvertArg("165 m")).map((s) => s.unit.id);
    expect(ids).toContain("mi");
    expect(ids.every((id) => id.startsWith("m"))).toBe(true);
    expect(ids.length).toBeGreaterThan(3);
    expect(ids).not.toContain("m"); // the resolved unit is the command row itself
  });

  it("a settled unit with nothing longer offers nothing", () => {
    expect(convertSuggestions(parseConvertArg("165 mph"))).toEqual([]);
  });

  it("an open target offers only units of the same category", () => {
    const s = convertSuggestions(parseConvertArg("165 mph in k"));
    expect(s.map((x) => x.unit.id).sort()).toEqual(["kmh", "kn"]);
    const kmh = s.find((x) => x.unit.id === "kmh")!;
    expect(kmh.arg).toBe("165 mph in kmh");
    expect(kmh.sample).toBe("265,5418 km/h");
  });

  it("a resolved target needs no suggestions", () => {
    expect(convertSuggestions(parseConvertArg("165 mph in kmh"))).toEqual([]);
  });
});

describe("buildArg / pasteNumber", () => {
  it("round-trips through the parser", () => {
    const arg = buildArg("165", "mph", "kmh");
    expect(arg).toBe("165 mph in kmh");
    expect(ready(arg).target?.id).toBe("kmh");
    expect(buildArg("", "mph")).toBe("mph");
  });

  it("pastes plain machine-readable numbers", () => {
    expect(pasteNumber(265.54176, unitById("kmh")!)).toBe("265.54176");
    expect(pasteNumber(92.592592, unitById("eur")!)).toBe("92.59");
    expect(pasteNumber(-0, unitById("c")!)).toBe("0");
  });
});
