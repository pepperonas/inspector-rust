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
