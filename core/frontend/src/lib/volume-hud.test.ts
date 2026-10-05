import { describe, it, expect } from "vitest";
import { digitColumns, rollDirection, waveIntensity } from "./volume-hud";

describe("digitColumns", () => {
  it("decomposes levels into digit strings", () => {
    expect(digitColumns(85)).toEqual(["8", "5"]);
    expect(digitColumns(100)).toEqual(["1", "0", "0"]);
    expect(digitColumns(5)).toEqual(["5"]);
    expect(digitColumns(0)).toEqual(["0"]);
  });

  it("clamps garbage instead of rendering it", () => {
    expect(digitColumns(150)).toEqual(["1", "0", "0"]);
    expect(digitColumns(-3)).toEqual(["0"]);
    expect(digitColumns(NaN)).toEqual(["0"]);
    expect(digitColumns(42.9)).toEqual(["4", "2"]); // floors, never "42.9"
  });
});

describe("rollDirection", () => {
  it("reports the movement of this trigger", () => {
    expect(rollDirection(40, 45)).toBe("up");
    expect(rollDirection(45, 40)).toBe("down");
  });

  it("is none for the first reading and for boundary repeats", () => {
    // No previous value → no direction to animate; a wave firing without an
    // actual change would lie (holding ⇧↓ at 0 must not keep collapsing).
    expect(rollDirection(null, 50)).toBe("none");
    expect(rollDirection(0, 0)).toBe("none");
    expect(rollDirection(100, 100)).toBe("none");
  });
});

describe("waveIntensity", () => {
  it("is monotonic from whisper to radiant", () => {
    expect(waveIntensity(0)).toBeCloseTo(0.35);
    expect(waveIntensity(100)).toBeCloseTo(1.0);
    expect(waveIntensity(50)).toBeGreaterThan(waveIntensity(10));
    expect(waveIntensity(NaN)).toBeCloseTo(0.35); // garbage whispers, never blinds
  });
});

describe("levelFromPointer / holdMs (v0.197.0)", () => {
  it("maps the pointer across the track to 0..100, clamped outside", async () => {
    const { levelFromPointer } = await import("./volume-hud");
    expect(levelFromPointer(100, 100, 200)).toBe(0);
    expect(levelFromPointer(200, 100, 200)).toBe(50);
    expect(levelFromPointer(300, 100, 200)).toBe(100);
    expect(levelFromPointer(151, 100, 200)).toBe(26); // rounds
    expect(levelFromPointer(-50, 100, 200)).toBe(0);
    expect(levelFromPointer(900, 100, 200)).toBe(100);
    expect(levelFromPointer(150, 100, 0)).toBe(0);
    expect(levelFromPointer(Number.NaN, 100, 200)).toBe(0);
  });
  it("a mouse-touched HUD lingers 3 s, otherwise the base hold", async () => {
    const { holdMs } = await import("./volume-hud");
    expect(holdMs(true, 1100)).toBe(3000);
    expect(holdMs(false, 1100)).toBe(1100);
  });
});
