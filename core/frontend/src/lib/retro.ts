// Pure helpers for the `8bit` / `16bit` retro overlay: command-argument
// parser, palette lookup, per-mode value ranges. The palette file is shared
// with Rust (`retro/mod.rs` include_str!s it), so both sides read the same
// ids and aliases.
import paletteFile from "./retro-palettes.json";

export type RetroMode = "8bit" | "16bit";

export interface RetroPaletteInfo {
  id: string;
  name: string;
  aliases: string[];
  mode: RetroMode;
}

export const RETRO_PALETTES: RetroPaletteInfo[] = [
  ...paletteFile.fixed.map((p) => ({ id: p.id, name: p.name, aliases: p.aliases, mode: "8bit" as const })),
  ...paletteFile.depth.map((p) => ({ id: p.id, name: p.name, aliases: p.aliases, mode: "16bit" as const })),
];

/** Palette by id or alias, case-insensitive. */
export function findRetroPalette(token: string): RetroPaletteInfo | null {
  const t = token.trim().toLowerCase();
  if (!t) return null;
  return (
    RETRO_PALETTES.find((p) => p.id === t || p.aliases.some((a) => a.toLowerCase() === t)) ?? null
  );
}

export type RetroAction =
  /** Open the panel; `mode` set when the keyword asks for one (`16bit`). */
  | { kind: "panel"; mode: RetroMode | null }
  | { kind: "on" }
  | { kind: "off" }
  | { kind: "focus" }
  | { kind: "lens" }
  | { kind: "preset"; name: string }
  | { kind: "palette"; id: string; mode: RetroMode }
  | { kind: "unknown"; text: string };

const ON = new Set(["on", "an", "start", "1"]);
const OFF = new Set(["off", "aus", "stop", "0"]);
const FOCUS = new Set(["focus", "fokus"]);
const LENS = new Set(["lens", "lupe"]);

/**
 * Parse the argument of `8bit` / `16bit`. `presetNames` are the current
 * preset names (built-in + user). A preset wins over a palette of the same
 * name — the backend already refuses such names, this just fixes the order.
 */
export function parseRetroArg(
  keyword: string,
  arg: string,
  presetNames: readonly string[] = [],
): RetroAction {
  const kw = keyword.trim().toLowerCase();
  const a = arg.trim();
  const low = a.toLowerCase();
  if (!a) return { kind: "panel", mode: kw === "16bit" ? "16bit" : null };
  if (ON.has(low)) return { kind: "on" };
  if (OFF.has(low)) return { kind: "off" };
  if (FOCUS.has(low)) return { kind: "focus" };
  if (LENS.has(low)) return { kind: "lens" };
  const preset = presetNames.find((n) => n.toLowerCase() === low);
  if (preset) return { kind: "preset", name: preset };
  const pal = findRetroPalette(low);
  if (pal) return { kind: "palette", id: pal.id, mode: pal.mode };
  return { kind: "unknown", text: a };
}

// ── Ranges (mirror retro/config.rs; pinned by a test on both sides) ─────────

export const PIXEL_RANGE: Record<RetroMode, [number, number]> = {
  "8bit": [3, 12],
  "16bit": [1, 4],
};
export const FOCUS_PX_RANGE: [number, number] = [1, 4];
export const LENS_RANGE: [number, number] = [40, 200];
export const OPACITY_RANGE: [number, number] = [50, 100];
export const PIXEL_STEP = 0.5;

/** Clamp to a range in 0.5 steps (NaN → lower bound). */
export function clampHalf(v: number, [lo, hi]: [number, number]): number {
  if (!Number.isFinite(v)) return lo;
  return Math.min(hi, Math.max(lo, Math.round(v * 2) / 2));
}

/** Palettes that belong to a mode. */
export function palettesFor(mode: RetroMode): RetroPaletteInfo[] {
  return RETRO_PALETTES.filter((p) => p.mode === mode);
}
