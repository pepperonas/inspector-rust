import { describe, expect, it } from "vitest";
import params from "../../../rust-lib/assets/record/effects/params.json";
import { EFFECTS, coerceEffectValue, effectDefaults, effectSpec, effectValues } from "./effects";

describe("effects catalogue", () => {
  it("is the Rust declaration, not a copy", () => {
    expect(EFFECTS).toBe(params);
  });
  it("has the six known effects with unique ids and params", () => {
    expect(EFFECTS.map((e) => e.id)).toEqual(["vignette", "grain", "chromatic", "ripple", "spotlight", "scanlines"]);
    for (const e of EFFECTS) {
      expect(new Set(e.params.map((p) => p.name)).size).toBe(e.params.length);
      for (const p of e.params) {
        if (p.type === "number") {
          expect(p.min).toBeLessThanOrEqual(p.default);
          expect(p.default).toBeLessThanOrEqual(p.max);
        }
      }
    }
    expect(EFFECTS.filter((e) => e.needsPointer).map((e) => e.id)).toEqual(["ripple", "spotlight"]);
  });
  it("defaults, overrides and coercion", () => {
    expect(effectDefaults("vignette")).toMatchObject({ strength: 0.35, color: "#000000" });
    expect(effectValues("vignette", { strength: 0.9 }).strength).toBe(0.9);
    expect(effectValues("nope", {})).toEqual({});
    const strength = effectSpec("vignette")!.params[0];
    expect(coerceEffectValue(strength, 5)).toBe(1);
    expect(coerceEffectValue(strength, "x")).toBeUndefined();
    const color = effectSpec("vignette")!.params[3];
    expect(coerceEffectValue(color, "#FFAA00")).toBe("#ffaa00");
    expect(coerceEffectValue(color, "red")).toBeUndefined();
    const toggle = effectSpec("grain")!.params[2];
    expect(coerceEffectValue(toggle, false)).toBe(false);
    expect(coerceEffectValue(toggle, 1)).toBeUndefined();
  });
});
