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

import { RETRO_ROWS, activeSlot, adjustRow, moveRow, rowInactive, rowValue } from "./retro";
import type { RetroConfig, RetroModeSettings } from "./ipc";

// Mirrors ModeSettings::defaults in retro/config.rs.
const EIGHT: RetroModeSettings = {
  palette: "nes", pixel_pt: 4, dither: "bayer4", dither_strength: 60,
  focus: true, focus_native: true, focus_pixel_pt: 1.5, focus_border: true,
  lens: false, lens_radius_pt: 80, lens_view: "original",
  scanlines: true, scanline_intensity: 40, crt: false, crt_strength: 40,
  opacity: 100, retro_frames: false, sprite_cursor: false,
};
const SIXTEEN: RetroModeSettings = {
  ...EIGHT, palette: "snes", pixel_pt: 2, dither_strength: 25, focus: true,
  focus_pixel_pt: 1, focus_border: false, scanlines: false, scanline_intensity: 30, crt_strength: 30,
};
const CFG: RetroConfig = { mode: "8bit", eight: EIGHT, sixteen: SIXTEEN, target: "current", fps: 60, retreat: true };

describe("panel rows", () => {
  it("↑/↓ wraps over every row", () => {
    expect(moveRow(0, -1)).toBe(RETRO_ROWS.length - 1);
    expect(moveRow(RETRO_ROWS.length - 1, 1)).toBe(0);
  });

  it("switching the mode shows that mode's own values (defaults until changed)", () => {
    let c = adjustRow("pixel", CFG, 1); // 8-bit 4 → 4.5
    expect(activeSlot(c).pixel_pt).toBe(4.5);
    c = adjustRow("mode", c, 1);
    expect(c.mode).toBe("16bit");
    expect(rowValue("palette", c)).toBe("SNES");
    expect(activeSlot(c).pixel_pt).toBe(2); // 16-bit default
    c = adjustRow("mode", c, -1);
    expect(activeSlot(c).pixel_pt).toBe(4.5); // the user's 8-bit change survived
  });

  it("value ranges follow the mode", () => {
    let c = CFG;
    for (let i = 0; i < 40; i++) c = adjustRow("pixel", c, 1);
    expect(activeSlot(c).pixel_pt).toBe(12);
    for (let i = 0; i < 40; i++) c = adjustRow("pixel", c, -1);
    expect(activeSlot(c).pixel_pt).toBe(3);
    let d: RetroConfig = { ...CFG, mode: "16bit" };
    for (let i = 0; i < 20; i++) d = adjustRow("pixel", d, 1);
    expect(activeSlot(d).pixel_pt).toBe(4);
    for (let i = 0; i < 20; i++) d = adjustRow("opacity", d, -1);
    expect(activeSlot(d).opacity).toBe(50);
    for (let i = 0; i < 30; i++) d = adjustRow("lensRadius", d, 1);
    expect(activeSlot(d).lens_radius_pt).toBe(200);
  });

  it("palettes cycle inside the active mode only", () => {
    let c = CFG;
    const seen = new Set<string>();
    for (let i = 0; i < 6; i++) { c = adjustRow("palette", c, 1); seen.add(activeSlot(c).palette); }
    expect([...seen].sort()).toEqual(["c64", "cga", "gb", "gray", "nes", "pico8"]);
    const d = adjustRow("palette", { ...CFG, mode: "16bit" }, -1);
    expect(activeSlot(d).palette).toBe("amiga");
  });

  it("dependent rows are inactive while their switch is off", () => {
    const row = RETRO_ROWS.find((r) => r.id === "focusPixel")!;
    const fine = adjustRow("focusNative", CFG, 1); // real window → fine pixels
    expect(activeSlot(fine).focus_native).toBe(false);
    expect(rowInactive(row, fine)).toBe(false);
    expect(rowInactive(row, adjustRow("focus", fine, 1))).toBe(true); // focus off
    // A real (unpixelated) window has no focus pixels to size.
    expect(rowInactive(row, CFG)).toBe(true);
    expect(rowValue("focusPixel", { ...CFG, mode: "16bit" })).toBe("1 pt (nur Farben)");
    expect(rowValue("pixel", adjustRow("pixel", CFG, 1))).toBe("4,5 pt");
  });

  it("the active window is real by default and retreat toggles", () => {
    expect(rowValue("focusNative", CFG)).toBe("Original");
    expect(rowValue("focusNative", adjustRow("focusNative", CFG, 1))).toBe("feine Pixel");
    expect(rowValue("retreat", CFG)).toBe("an");
    const off = adjustRow("retreat", CFG, 1);
    expect(off.retreat).toBe(false);
    expect(off.eight).toBe(CFG.eight); // config-level, the slot stays untouched
  });

  it("fps and target toggle", () => {
    expect(adjustRow("fps", CFG, 1).fps).toBe(30);
    expect(adjustRow("target", CFG, -1).target).toBe("all");
  });
});
