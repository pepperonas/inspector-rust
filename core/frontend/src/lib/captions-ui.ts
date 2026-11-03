/**
 * The captions pane's rules (stage 6, pure): named looks, plates, clamped settings.
 *
 * Ported from OpenScreen's `captions/settings.ts` + `textContrast.ts` (RECON-stage6 §6). The
 * stored values are normalised again in Rust (`captions::normalize_settings`); these clamps are
 * what the pane offers, so the controls never show a value the scene would not draw.
 */
import {
  CAPTION_FONT_SIZE_UI_RANGE,
  CAPTION_INSET_STEP,
  CAPTION_INSET_X_MAX,
  CAPTION_INSET_Y_MAX,
  CAPTION_LOOKS,
  CAPTION_PLATE_DARK,
  CAPTION_PLATE_LIGHT,
  CAPTION_PLATE_OPACITY_RANGE,
  CAPTION_WORDS_RANGE,
  type CaptionLook,
} from "./editor";
import type { RecordCaptionSettings, RecordEditDoc } from "./ipc";

export type CaptionPlate = "none" | "dark" | "light";
export type CaptionLookId = "classic" | "bold" | "minimal" | "light";

/** Minimum WCAG contrast a text colour must keep on its plate (upstream). */
export const MIN_TEXT_CONTRAST = 4.5;
/** The text colours offered: four light ones for a dark plate, four dark ones for a light one. */
export const TEXT_PALETTE: readonly string[] = ["#ffffff", "#fde047", "#86efac", "#7dd3fc", "#111111", "#1d4ed8", "#b91c1c", "#166534"];

const PLATES: Record<Exclude<CaptionPlate, "none">, string> = { dark: CAPTION_PLATE_DARK, light: CAPTION_PLATE_LIGHT };

type Rgb = [number, number, number];
function parseHex(v: string): Rgb | null {
  const m = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(v.trim())?.[1];
  if (!m) return null;
  const full = m.length === 3 ? [...m].map((c) => c + c).join("") : m;
  return [0, 2, 4].map((i) => Number.parseInt(full.slice(i, i + 2), 16)) as Rgb;
}
const channel = (v: number) => {
  const c = v / 255;
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
};
const luminance = ([r, g, b]: Rgb) => 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);

/** WCAG contrast of two hex colours; `null` when one is not a hex colour. */
export function contrastRatio(a: string, b: string): number | null {
  const x = parseHex(a);
  const y = parseHex(b);
  if (!x || !y) return null;
  const [hi, lo] = [luminance(x), luminance(y)].sort((p, q) => q - p);
  return (hi + 0.05) / (lo + 0.05);
}

/** `text` if it reads on `plate`, else the palette colour with the most contrast there. */
export function textForPlate(text: string, plate: string): string {
  const r = contrastRatio(text, plate);
  if (r === null || r >= MIN_TEXT_CONTRAST) return text;
  return [...TEXT_PALETTE].sort((a, b) => (contrastRatio(b, plate) ?? 0) - (contrastRatio(a, plate) ?? 0))[0];
}

/** The named plate the settings carry, or `"custom"` for another colour. */
export function captionPlateOf(s: RecordCaptionSettings): CaptionPlate | "custom" {
  if (!s.plate) return "none";
  const c = s.plateColor.toLowerCase();
  if (c === CAPTION_PLATE_DARK) return "dark";
  if (c === CAPTION_PLATE_LIGHT) return "light";
  return "custom";
}

/** Choosing a plate turns it on (or off) and moves text that would vanish on it. */
export function captionPlatePatch(s: RecordCaptionSettings, plate: CaptionPlate): Partial<RecordCaptionSettings> {
  if (plate === "none") return { plate: false };
  const plateColor = PLATES[plate];
  return { plate: true, plateColor, color: textForPlate(s.color, plateColor) };
}

const LOOK_KEYS: (keyof CaptionLook)[] = ["fontFamily", "bold", "fontSize", "color", "plate", "plateColor", "plateOpacity"];

/** The named look the settings match exactly, or `null` once anything was tuned. */
export function captionLookOf(s: RecordCaptionSettings): CaptionLookId | null {
  for (const [id, look] of CAPTION_LOOKS) {
    if (LOOK_KEYS.every((k) => (typeof look[k] === "string" ? String(s[k]).toLowerCase() === look[k] : s[k] === look[k]))) {
      return id as CaptionLookId;
    }
  }
  return null;
}

export function captionLook(id: CaptionLookId): CaptionLook {
  return (CAPTION_LOOKS.find(([k]) => k === id) ?? CAPTION_LOOKS[0])[1];
}

const clamp = (v: number, lo: number, hi: number) => (Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : lo);
const onStep = (v: number) => Math.round(v / CAPTION_INSET_STEP) * CAPTION_INSET_STEP;

/** `s` with `patch` applied and every number clamped to what the pane offers (min ≤ max words). */
export function patchCaptions(s: RecordCaptionSettings, patch: Partial<RecordCaptionSettings>): RecordCaptionSettings {
  const n: RecordCaptionSettings = { ...s, ...patch };
  n.fontSize = Math.round(clamp(n.fontSize, CAPTION_FONT_SIZE_UI_RANGE[0], CAPTION_FONT_SIZE_UI_RANGE[1]));
  n.plateOpacity = clamp(n.plateOpacity, CAPTION_PLATE_OPACITY_RANGE[0], CAPTION_PLATE_OPACITY_RANGE[1]);
  if (n.insetY != null) n.insetY = onStep(clamp(n.insetY, 0, CAPTION_INSET_Y_MAX));
  if (n.insetX != null) n.insetX = onStep(clamp(n.insetX, 0, CAPTION_INSET_X_MAX));
  n.minWords = Math.round(clamp(n.minWords, CAPTION_WORDS_RANGE[0], CAPTION_WORDS_RANGE[1]));
  n.maxWords = Math.round(clamp(n.maxWords, CAPTION_WORDS_RANGE[0], CAPTION_WORDS_RANGE[1]));
  if (n.minWords > n.maxWords) {
    // The field being moved wins; the other follows it.
    if (patch.maxWords != null && patch.minWords == null) n.minWords = n.maxWords;
    else n.maxWords = n.minWords;
  }
  return n;
}

/** The document with its captions patched (`patchCaptions`). */
export function setCaptions(doc: RecordEditDoc, patch: Partial<RecordCaptionSettings>): RecordEditDoc {
  return { ...doc, captions: patchCaptions(doc.captions, patch) };
}
