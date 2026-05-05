// Hex-color parser — Alfred-style inline color preview for the search input.
//
// Accepted shapes:
//   #RGB            (3-digit)
//   #RGBA           (4-digit)
//   #RRGGBB         (6-digit)
//   #RRGGBBAA       (8-digit)
//   RRGGBB          (6-digit, no #)
//   RRGGBBAA        (8-digit, no #)
//
// The 3-digit / 4-digit forms REQUIRE a leading `#` — `abc` alone is too
// likely to be a search query for a snippet abbreviation. The 6/8-digit
// forms are unambiguous enough to accept either way.
//
// `tryParseColor` returns null for anything that isn't strictly a hex
// color string (no leading/trailing junk, no `0x` prefix, etc.) so the
// search list stays clean.

export interface ColorEntry {
  /** Canonical `#RRGGBB` (or `#RRGGBBAA` if alpha < 1), uppercase. */
  hex: string;
  /** What gets pasted on activation. Same as `hex` in v1. */
  pasteValue: string;
  r: number; // 0-255
  g: number;
  b: number;
  /** 0.0 to 1.0. */
  a: number;
  /** Hue 0-360, saturation 0-100, lightness 0-100. */
  hsl: { h: number; s: number; l: number };
  /** Display-friendly RGB string, e.g. "rgb(51, 102, 255)" or "rgba(51, 102, 255, 0.5)". */
  rgbString: string;
  /** Display-friendly HSL string. */
  hslString: string;
}

/** Parse a hex color or return null. */
export function tryParseColor(input: string): ColorEntry | null {
  const trimmed = input.trim();
  if (!trimmed) return null;

  const hasHash = trimmed.startsWith("#");
  const hex = hasHash ? trimmed.slice(1) : trimmed;

  // Strict: only hex chars, only the supported lengths.
  if (!/^[0-9a-fA-F]+$/.test(hex)) return null;
  if (hex.length !== 3 && hex.length !== 4 && hex.length !== 6 && hex.length !== 8) {
    return null;
  }

  // 3/4-digit forms only valid with the `#` prefix — too ambiguous with
  // search otherwise (think "fed" / "abc" / "f00d" — all valid hex *and*
  // plausible search input).
  if (!hasHash && (hex.length === 3 || hex.length === 4)) return null;

  let r: number;
  let g: number;
  let b: number;
  let a = 1;
  switch (hex.length) {
    case 3:
      r = parseInt(hex[0] + hex[0], 16);
      g = parseInt(hex[1] + hex[1], 16);
      b = parseInt(hex[2] + hex[2], 16);
      break;
    case 4:
      r = parseInt(hex[0] + hex[0], 16);
      g = parseInt(hex[1] + hex[1], 16);
      b = parseInt(hex[2] + hex[2], 16);
      a = parseInt(hex[3] + hex[3], 16) / 255;
      break;
    case 6:
      r = parseInt(hex.slice(0, 2), 16);
      g = parseInt(hex.slice(2, 4), 16);
      b = parseInt(hex.slice(4, 6), 16);
      break;
    case 8:
      r = parseInt(hex.slice(0, 2), 16);
      g = parseInt(hex.slice(2, 4), 16);
      b = parseInt(hex.slice(4, 6), 16);
      a = parseInt(hex.slice(6, 8), 16) / 255;
      break;
    default:
      return null;
  }

  // Canonical hex output. Always with `#`. `#RRGGBB` when alpha is fully
  // opaque; `#RRGGBBAA` when alpha < 1. Uppercase.
  const pad = (n: number) => n.toString(16).padStart(2, "0").toUpperCase();
  let canonical = `#${pad(r)}${pad(g)}${pad(b)}`;
  if (a < 1) {
    canonical += pad(Math.round(a * 255));
  }

  const hsl = rgbToHsl(r, g, b);
  const rgbString =
    a < 1
      ? `rgba(${r}, ${g}, ${b}, ${a.toFixed(2).replace(/\.?0+$/, "")})`
      : `rgb(${r}, ${g}, ${b})`;
  const hslString =
    a < 1
      ? `hsla(${hsl.h}, ${hsl.s}%, ${hsl.l}%, ${a.toFixed(2).replace(/\.?0+$/, "")})`
      : `hsl(${hsl.h}, ${hsl.s}%, ${hsl.l}%)`;

  return {
    hex: canonical,
    pasteValue: canonical,
    r,
    g,
    b,
    a,
    hsl,
    rgbString,
    hslString,
  };
}

/** Convert sRGB integer triplet (0-255 each) to HSL with H in [0,360],
 *  S/L in [0,100]. Uses the standard conversion. */
export function rgbToHsl(r: number, g: number, b: number): { h: number; s: number; l: number } {
  const rf = r / 255;
  const gf = g / 255;
  const bf = b / 255;
  const max = Math.max(rf, gf, bf);
  const min = Math.min(rf, gf, bf);
  const l = (max + min) / 2;
  let h = 0;
  let s = 0;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    switch (max) {
      case rf:
        h = (gf - bf) / d + (gf < bf ? 6 : 0);
        break;
      case gf:
        h = (bf - rf) / d + 2;
        break;
      case bf:
        h = (rf - gf) / d + 4;
        break;
    }
    h *= 60;
  }
  return {
    h: Math.round(h),
    s: Math.round(s * 100),
    l: Math.round(l * 100),
  };
}

/** Decide whether white or black contrasts better with a given color
 *  (relative luminance per WCAG 2). Used to pick a readable text color
 *  for the swatch label. */
export function readableForeground(r: number, g: number, b: number): "#000000" | "#FFFFFF" {
  const lum = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
  return lum > 0.55 ? "#000000" : "#FFFFFF";
}

/** Convert HSV (hue 0-360, sat 0-100, val 0-100) to sRGB triplet (0-255). */
export function hsvToRgb(h: number, s: number, v: number): [number, number, number] {
  const sf = s / 100;
  const vf = v / 100;
  const c = vf * sf;
  const hh = (((h % 360) + 360) % 360) / 60;
  const x = c * (1 - Math.abs((hh % 2) - 1));
  let rf: number;
  let gf: number;
  let bf: number;
  if (hh < 1) [rf, gf, bf] = [c, x, 0];
  else if (hh < 2) [rf, gf, bf] = [x, c, 0];
  else if (hh < 3) [rf, gf, bf] = [0, c, x];
  else if (hh < 4) [rf, gf, bf] = [0, x, c];
  else if (hh < 5) [rf, gf, bf] = [x, 0, c];
  else [rf, gf, bf] = [c, 0, x];
  const m = vf - c;
  return [
    Math.round((rf + m) * 255),
    Math.round((gf + m) * 255),
    Math.round((bf + m) * 255),
  ];
}

/** Convert sRGB triplet (0-255) to HSV (hue 0-360, sat 0-100, val 0-100). */
export function rgbToHsv(r: number, g: number, b: number): [number, number, number] {
  const rf = r / 255;
  const gf = g / 255;
  const bf = b / 255;
  const max = Math.max(rf, gf, bf);
  const min = Math.min(rf, gf, bf);
  const d = max - min;
  let h = 0;
  if (d !== 0) {
    if (max === rf) h = ((gf - bf) / d) % 6;
    else if (max === gf) h = (bf - rf) / d + 2;
    else h = (rf - gf) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  const s = max === 0 ? 0 : (d / max) * 100;
  const v = max * 100;
  return [Math.round(h), Math.round(s), Math.round(v)];
}

/** Build a `#RRGGBB` (uppercase, no alpha) from an sRGB triplet. */
export function rgbToHex(r: number, g: number, b: number): string {
  const pad = (n: number) => Math.max(0, Math.min(255, Math.round(n))).toString(16).padStart(2, "0").toUpperCase();
  return `#${pad(r)}${pad(g)}${pad(b)}`;
}
