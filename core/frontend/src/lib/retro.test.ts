import { describe, expect, it } from "vitest";
import {
  PIXEL_RANGE,
  RETRO_PALETTES,
  clampHalf,
  findRetroPalette,
  palettesFor,
  parseRetroArg,
} from "./retro";

const PRESETS = ["Show", "Alltag", "Retro-Arbeit", "Mein Look"];

describe("parseRetroArg", () => {
  it("bare keyword opens the panel; the 16bit alias asks for 16-bit", () => {
    expect(parseRetroArg("8bit", "")).toEqual({ kind: "panel", mode: null });
    expect(parseRetroArg("16bit", "  ")).toEqual({ kind: "panel", mode: "16bit" });
    expect(parseRetroArg("16BIT", "")).toEqual({ kind: "panel", mode: "16bit" });
  });

  it("on / off / focus / lens, German forms too", () => {
    expect(parseRetroArg("8bit", "on").kind).toBe("on");
    expect(parseRetroArg("8bit", "AN").kind).toBe("on");
    expect(parseRetroArg("8bit", "off").kind).toBe("off");
    expect(parseRetroArg("16bit", "aus").kind).toBe("off");
    expect(parseRetroArg("8bit", "focus").kind).toBe("focus");
    expect(parseRetroArg("8bit", "Fokus").kind).toBe("focus");
    expect(parseRetroArg("8bit", "lens").kind).toBe("lens");
    expect(parseRetroArg("8bit", "lupe").kind).toBe("lens");
  });

  it("palette names and aliases imply the mode", () => {
    expect(parseRetroArg("8bit", "gb")).toEqual({ kind: "palette", id: "gb", mode: "8bit" });
    expect(parseRetroArg("8bit", "GameBoy")).toEqual({ kind: "palette", id: "gb", mode: "8bit" });
    expect(parseRetroArg("8bit", "snes")).toEqual({ kind: "palette", id: "snes", mode: "16bit" });
    expect(parseRetroArg("16bit", "pico-8")).toEqual({ kind: "palette", id: "pico8", mode: "8bit" });
    expect(parseRetroArg("8bit", "genesis")).toEqual({ kind: "palette", id: "megadrive", mode: "16bit" });
  });

  it("presets match case-insensitively and keep their real name", () => {
    expect(parseRetroArg("8bit", "alltag", PRESETS)).toEqual({ kind: "preset", name: "Alltag" });
    expect(parseRetroArg("8bit", "mein look", PRESETS)).toEqual({ kind: "preset", name: "Mein Look" });
  });

  it("a preset wins over a palette of the same name", () => {
    expect(parseRetroArg("8bit", "nes", ["NES"])).toEqual({ kind: "preset", name: "NES" });
  });

  it("anything else is unknown, never a silent start", () => {
    expect(parseRetroArg("8bit", "fooo")).toEqual({ kind: "unknown", text: "fooo" });
    expect(parseRetroArg("8bit", "alltag")).toEqual({ kind: "unknown", text: "alltag" }); // no presets known
  });
});

describe("palettes + ranges", () => {
  it("the shared palette file has both families and unique names", () => {
    expect(palettesFor("8bit").map((p) => p.id)).toEqual(["nes", "c64", "pico8", "gb", "cga", "gray"]);
    expect(palettesFor("16bit").map((p) => p.id)).toEqual(["snes", "megadrive", "amiga"]);
    const names = RETRO_PALETTES.flatMap((p) => [p.id, ...p.aliases.map((a) => a.toLowerCase())]);
    expect(new Set(names).size).toBe(names.length);
    expect(findRetroPalette("")).toBeNull();
  });

  it("ranges mirror retro/config.rs and clamp in half steps", () => {
    expect(PIXEL_RANGE["8bit"]).toEqual([3, 12]);
    expect(PIXEL_RANGE["16bit"]).toEqual([1, 4]);
    expect(clampHalf(2.74, PIXEL_RANGE["16bit"])).toBe(2.5);
    expect(clampHalf(1, PIXEL_RANGE["8bit"])).toBe(3);
    expect(clampHalf(99, PIXEL_RANGE["8bit"])).toBe(12);
    expect(clampHalf(NaN, PIXEL_RANGE["8bit"])).toBe(3);
  });
});
