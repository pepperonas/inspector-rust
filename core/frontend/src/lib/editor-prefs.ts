//! Remembered screenshot-editor settings (pure): one style per tool, the
//! last tool and favourite colours. Stored as JSON in the settings table
//! (`screenshot.editor_styles`, synced between Macs); parsing is tolerant
//! field by field so a damaged value never resets everything.

import {
  DEFAULT_STYLE,
  type ArrowStyle,
  type FillMode,
  type Style,
  type TextStyle,
  type Tool,
} from "./editor-geometry";

export type StyledTool = "arrow" | "line" | "rect" | "ellipse" | "text" | "highlight" | "pen" | "step" | "emoji";

export type EditorPrefs = {
  version: 1;
  lastTool: Tool;
  styles: Partial<Record<StyledTool, Style>>;
  favorites: string[];
};

export const MAX_FAVORITES = 8;

const STYLED: ReadonlyArray<StyledTool> = ["arrow", "line", "rect", "ellipse", "text", "highlight", "pen", "step", "emoji"];
const TOOLS: ReadonlyArray<Tool> = [
  "select", "arrow", "line", "text", "rect", "ellipse", "highlight", "blur", "redact", "step", "pen", "crop", "spotlight", "emoji",
];
const ARROW_STYLES: ReadonlyArray<ArrowStyle> = ["classic", "bold", "curved", "marker"];
const FILLS: ReadonlyArray<FillMode> = ["outline", "filled", "tint"];
const TEXT_STYLES: ReadonlyArray<TextStyle> = ["plain", "outline", "pill", "box"];

export const DEFAULT_PREFS: EditorPrefs = { version: 1, lastTool: "arrow", styles: {}, favorites: [] };

const HEX = /^#[0-9a-f]{6}$/i;

function isStyled(t: Tool): t is StyledTool {
  return (STYLED as ReadonlyArray<string>).includes(t);
}

function num(v: unknown, lo: number, hi: number, dflt: number): number {
  return typeof v === "number" && Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : dflt;
}

function oneOf<T extends string>(v: unknown, all: ReadonlyArray<T>, dflt: T): T {
  return typeof v === "string" && (all as ReadonlyArray<string>).includes(v) ? (v as T) : dflt;
}

function parseStyle(v: unknown): Style {
  const o = (v && typeof v === "object" ? v : {}) as Record<string, unknown>;
  const d = DEFAULT_STYLE;
  return {
    color: typeof o.color === "string" && HEX.test(o.color) ? o.color.toLowerCase() : d.color,
    width: num(o.width, 2, 24, d.width),
    opacity: num(o.opacity, 0.1, 1, d.opacity),
    shadow: typeof o.shadow === "boolean" ? o.shadow : d.shadow,
    fontSize: num(o.fontSize, 8, 200, d.fontSize),
    arrowStyle: oneOf(o.arrowStyle, ARROW_STYLES, d.arrowStyle),
    fill: oneOf(o.fill, FILLS, d.fill),
    textStyle: oneOf(o.textStyle, TEXT_STYLES, d.textStyle),
  };
}

function cleanFavorites(v: unknown): string[] {
  if (!Array.isArray(v)) return [];
  const out: string[] = [];
  for (const c of v) {
    if (typeof c !== "string" || !HEX.test(c)) continue;
    const lc = c.toLowerCase();
    if (!out.includes(lc)) out.push(lc);
    if (out.length === MAX_FAVORITES) break;
  }
  return out;
}

export function parsePrefs(json: string | null): EditorPrefs {
  if (!json) return DEFAULT_PREFS;
  let raw: unknown;
  try {
    raw = JSON.parse(json);
  } catch {
    return DEFAULT_PREFS;
  }
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return DEFAULT_PREFS;
  const o = raw as Record<string, unknown>;
  const styles: Partial<Record<StyledTool, Style>> = {};
  if (o.styles && typeof o.styles === "object" && !Array.isArray(o.styles)) {
    for (const [k, v] of Object.entries(o.styles as Record<string, unknown>)) {
      if ((STYLED as ReadonlyArray<string>).includes(k)) styles[k as StyledTool] = parseStyle(v);
    }
  }
  return {
    version: 1,
    lastTool: oneOf(o.lastTool, TOOLS, "arrow"),
    styles,
    favorites: cleanFavorites(o.favorites),
  };
}

export function serializePrefs(p: EditorPrefs): string {
  return JSON.stringify(p);
}

export function styleFor(p: EditorPrefs, tool: Tool): Style {
  return (isStyled(tool) && p.styles[tool]) || DEFAULT_STYLE;
}

export function rememberStyle(p: EditorPrefs, tool: Tool, patch: Partial<Style>): EditorPrefs {
  if (!isStyled(tool)) return p;
  return { ...p, styles: { ...p.styles, [tool]: { ...styleFor(p, tool), ...patch } } };
}

export function withLastTool(p: EditorPrefs, tool: Tool): EditorPrefs {
  return p.lastTool === tool ? p : { ...p, lastTool: tool };
}

export function addFavorite(p: EditorPrefs, color: string): EditorPrefs {
  if (!HEX.test(color)) return p;
  const lc = color.toLowerCase();
  return { ...p, favorites: [lc, ...p.favorites.filter((c) => c !== lc)].slice(0, MAX_FAVORITES) };
}

export function removeFavorite(p: EditorPrefs, color: string): EditorPrefs {
  const lc = color.toLowerCase();
  return { ...p, favorites: p.favorites.filter((c) => c !== lc) };
}
