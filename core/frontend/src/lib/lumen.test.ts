import { describe, expect, it } from "vitest";
import { formatLux, lightBand, luxLevel } from "./lumen";

describe("lumen visual scale", () => {
  it("maps lux logarithmically and clamps invalid values", () => {
    expect(luxLevel(-1)).toBe(0);
    expect(luxLevel(Number.NaN)).toBe(0);
    expect(luxLevel(100)).toBeGreaterThan(35);
    expect(luxLevel(100_000)).toBe(100);
    expect(luxLevel(1_000_000)).toBe(100);
  });

  it("classifies common lighting conditions", () => {
    expect(lightBand(5).label).toBe("Dark");
    expect(lightBand(50).label).toBe("Dim");
    expect(lightBand(300).label).toBe("Indoor");
    expect(lightBand(1_000).label).toBe("Bright");
    expect(lightBand(20_000).label).toBe("Daylight");
  });

  it("keeps low readings precise and large readings compact", () => {
    expect(formatLux(6.25)).toBe("6.3");
    expect(formatLux(412.8)).toBe("413");
  });
});
