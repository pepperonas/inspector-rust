// Pure helpers for the `8bit` / `16bit` retro overlay: command-argument
// parser, palette lookup, per-mode value ranges. The palette file is shared
// with Rust (`retro/mod.rs` include_str!s it), so both sides read the same
// ids and aliases.
import paletteFile from "./retro-palettes.json";
import type { RetroConfig, RetroModeSettings, RetroDither } from "./ipc";
import { locale, t } from "./i18n";

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

// ── Panel rows (keyboard model: ↑↓ row, ←→ value) ───────────────────────────


export type RetroRowId =
  | "mode" | "palette" | "pixel" | "dither" | "ditherStrength"
  | "focus" | "focusNative" | "focusPixel" | "focusBorder" | "retreat"
  | "lens" | "lensRadius" | "lensView"
  | "scanlines" | "scanlineIntensity" | "crt" | "crtStrength"
  | "opacity" | "frames" | "sprite" | "target" | "fps";

export interface RetroRow {
  id: RetroRowId;
  readonly label: string;
  readonly group: string;
  /** Row only makes sense while its parent switch is on (shown dimmed). */
  dependsOn?: RetroRowId;
}

export const RETRO_ROWS: RetroRow[] = [
  { id: "mode", get label() { return t("retro.row.mode"); }, get group() { return t("retro.group.image"); } },
  { id: "palette", get label() { return t("retro.row.palette"); }, get group() { return t("retro.group.image"); } },
  { id: "pixel", get label() { return t("retro.row.pixel"); }, get group() { return t("retro.group.image"); } },
  { id: "dither", get label() { return t("retro.row.dither"); }, get group() { return t("retro.group.image"); } },
  { id: "ditherStrength", get label() { return t("retro.row.ditherStrength"); }, get group() { return t("retro.group.image"); }, dependsOn: "dither" },
  { id: "focus", get label() { return t("retro.row.focus"); }, get group() { return t("retro.group.readability"); } },
  { id: "focusNative", get label() { return t("retro.row.focusNative"); }, get group() { return t("retro.group.readability"); }, dependsOn: "focus" },
  { id: "focusPixel", get label() { return t("retro.row.focusPixel"); }, get group() { return t("retro.group.readability"); }, dependsOn: "focus" },
  { id: "focusBorder", get label() { return t("retro.row.focusBorder"); }, get group() { return t("retro.group.readability"); }, dependsOn: "focus" },
  { id: "lens", get label() { return t("retro.row.lens"); }, get group() { return t("retro.group.readability"); } },
  { id: "lensRadius", get label() { return t("retro.row.lensRadius"); }, get group() { return t("retro.group.readability"); }, dependsOn: "lens" },
  { id: "lensView", get label() { return t("retro.row.lensView"); }, get group() { return t("retro.group.readability"); }, dependsOn: "lens" },
  { id: "retreat", get label() { return t("retro.row.retreat"); }, get group() { return t("retro.group.readability"); } },
  { id: "scanlines", get label() { return t("retro.row.scanlines"); }, get group() { return t("retro.group.tube"); } },
  { id: "scanlineIntensity", get label() { return t("retro.row.scanlineIntensity"); }, get group() { return t("retro.group.tube"); }, dependsOn: "scanlines" },
  { id: "crt", get label() { return t("retro.row.crt"); }, get group() { return t("retro.group.tube"); } },
  { id: "crtStrength", get label() { return t("retro.row.crtStrength"); }, get group() { return t("retro.group.tube"); }, dependsOn: "crt" },
  { id: "opacity", get label() { return t("retro.row.opacity"); }, get group() { return t("retro.group.tube"); } },
  { id: "frames", get label() { return t("retro.row.frames"); }, get group() { return t("retro.group.stage2"); } },
  { id: "sprite", get label() { return t("retro.row.sprite"); }, get group() { return t("retro.group.stage2"); } },
  { id: "target", get label() { return t("retro.row.target"); }, get group() { return t("retro.group.output"); } },
  { id: "fps", get label() { return t("retro.row.fps"); }, get group() { return t("retro.group.output"); } },
];

export const DITHER_ORDER: RetroDither[] = ["off", "bayer2", "bayer4", "bayer8"];
const DITHER_LABEL: Record<RetroDither, string> = {
  get off() { return t("retro.off"); },
  bayer2: "Bayer 2×2",
  bayer4: "Bayer 4×4",
  bayer8: "Bayer 8×8",
};

/** The settings slot of the active mode. */
export function activeSlot(cfg: RetroConfig): RetroModeSettings {
  return cfg.mode === "8bit" ? cfg.eight : cfg.sixteen;
}

function withSlot(cfg: RetroConfig, s: RetroModeSettings): RetroConfig {
  return cfg.mode === "8bit" ? { ...cfg, eight: s } : { ...cfg, sixteen: s };
}

const fmtNum = (v: number) => v.toLocaleString(locale(), { maximumFractionDigits: 1 });
const onOff = (b: boolean) => (b ? t("retro.on") : t("retro.off"));

/** Is a row's parent switched off (row shown dimmed)? */
export function rowInactive(row: RetroRow, cfg: RetroConfig): boolean {
  if (!row.dependsOn) return false;
  const s = activeSlot(cfg);
  // Fine focus pixels only matter when the window is pixelated, not real.
  if (row.id === "focusPixel" && s.focus && s.focus_native) return true;
  switch (row.dependsOn) {
    case "dither": return s.dither === "off";
    case "focus": return !s.focus;
    case "lens": return !s.lens;
    case "scanlines": return !s.scanlines;
    case "crt": return !s.crt;
    default: return false;
  }
}

export function rowValue(id: RetroRowId, cfg: RetroConfig): string {
  const s = activeSlot(cfg);
  switch (id) {
    case "mode": return cfg.mode === "8bit" ? "8-Bit" : "16-Bit";
    case "palette": return findRetroPalette(s.palette)?.name ?? s.palette;
    case "pixel": return `${fmtNum(s.pixel_pt)} pt`;
    case "dither": return DITHER_LABEL[s.dither];
    case "ditherStrength": return `${s.dither_strength} %`;
    case "focus": return onOff(s.focus);
    case "focusNative": return s.focus_native ? t("retro.original") : t("retro.fine_pixels");
    case "retreat": return onOff(cfg.retreat);
    case "focusPixel": return s.focus_pixel_pt <= 1 ? t("retro.one_pt_colours") : `${fmtNum(s.focus_pixel_pt)} pt`;
    case "focusBorder": return onOff(s.focus_border);
    case "lens": return onOff(s.lens);
    case "lensRadius": return `${s.lens_radius_pt} pt`;
    case "lensView": return s.lens_view === "original" ? t("retro.original") : t("retro.focus_pixels");
    case "scanlines": return onOff(s.scanlines);
    case "scanlineIntensity": return `${s.scanline_intensity} %`;
    case "crt": return onOff(s.crt);
    case "crtStrength": return `${s.crt_strength} %`;
    case "opacity": return `${s.opacity} %`;
    case "frames": return onOff(s.retro_frames);
    case "sprite": return onOff(s.sprite_cursor);
    case "target": return cfg.target === "current" ? t("retro.current_mon") : t("retro.all_mon");
    case "fps": return `${cfg.fps}`;
  }
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
function cycle<T>(list: readonly T[], cur: T, dir: number): T {
  const i = list.indexOf(cur);
  return list[((i < 0 ? 0 : i) + dir + list.length) % list.length];
}

/**
 * Pure: change one row by `dir` (−1 / +1). Returns a new config; switching the
 * mode just selects the other slot, so it shows that mode's own values —
 * its defaults until the user changed them.
 */
export function adjustRow(id: RetroRowId, cfg: RetroConfig, dir: number): RetroConfig {
  const s = activeSlot(cfg);
  const set = (patch: Partial<RetroModeSettings>) => withSlot(cfg, { ...s, ...patch });
  switch (id) {
    case "mode": return { ...cfg, mode: cfg.mode === "8bit" ? "16bit" : "8bit" };
    case "palette": {
      const ids = palettesFor(cfg.mode).map((p) => p.id);
      return set({ palette: cycle(ids, s.palette, dir) });
    }
    case "pixel": return set({ pixel_pt: clampHalf(s.pixel_pt + dir * PIXEL_STEP, PIXEL_RANGE[cfg.mode]) });
    case "dither": return set({ dither: cycle(DITHER_ORDER, s.dither, dir) });
    case "ditherStrength": return set({ dither_strength: clamp(s.dither_strength + dir * 5, 0, 100) });
    case "focus": return set({ focus: !s.focus });
    case "focusNative": return set({ focus_native: !s.focus_native });
    case "retreat": return { ...cfg, retreat: !cfg.retreat };
    case "focusPixel": return set({ focus_pixel_pt: clampHalf(s.focus_pixel_pt + dir * PIXEL_STEP, FOCUS_PX_RANGE) });
    case "focusBorder": return set({ focus_border: !s.focus_border });
    case "lens": return set({ lens: !s.lens });
    case "lensRadius": return set({ lens_radius_pt: clamp(s.lens_radius_pt + dir * 10, LENS_RANGE[0], LENS_RANGE[1]) });
    case "lensView": return set({ lens_view: s.lens_view === "original" ? "focus" : "original" });
    case "scanlines": return set({ scanlines: !s.scanlines });
    case "scanlineIntensity": return set({ scanline_intensity: clamp(s.scanline_intensity + dir * 5, 0, 100) });
    case "crt": return set({ crt: !s.crt });
    case "crtStrength": return set({ crt_strength: clamp(s.crt_strength + dir * 5, 0, 100) });
    case "opacity": return set({ opacity: clamp(s.opacity + dir * 5, OPACITY_RANGE[0], OPACITY_RANGE[1]) });
    case "frames": return set({ retro_frames: !s.retro_frames });
    case "sprite": return set({ sprite_cursor: !s.sprite_cursor });
    case "target": return { ...cfg, target: cfg.target === "current" ? "all" : "current" };
    case "fps": return { ...cfg, fps: cfg.fps === 60 ? 30 : 60 };
  }
}

/** Pure: next row index for ↑/↓ (wraps). */
export function moveRow(index: number, dir: number, count = RETRO_ROWS.length): number {
  return (index + dir + count) % count;
}

/** Human text for a backend error code. */
export function retroErrorText(code: string): string {
  switch (code) {
    case "retro.no_permission": return t("retro.err.no_permission");
    case "retro.unsupported": return t("retro.err.unsupported");
    default: return code;
  }
}
