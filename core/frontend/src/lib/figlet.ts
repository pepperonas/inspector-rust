/**
 * `figlet` — ASCII-art banner command: shared types (mirroring the Rust
 * `figlet` module's serde shapes) + the argument parser.
 *
 * Rendering lives in Rust (the font engine is there); this module owns the
 * command grammar and the option types the IPC layer + UI pass around. The
 * parser is added in the frontend commit; here are the types the IPC wrappers
 * need.
 */

export type FigletAlign = "left" | "center" | "right";
export type FigletComment = "none" | "slashes" | "hash" | "block" | "html";

/** Render options — mirrors Rust `RenderOpts`. `width: 0` disables wrapping. */
export interface FigletOpts {
  width: number;
  align: FigletAlign;
  trim: boolean;
  comment: FigletComment;
  boxed: boolean;
}

/** A rendered banner + diagnostics — mirrors Rust `Banner`. `unsupported` is
 *  the distinct characters the font couldn't render (each a 1-char string). */
export interface FigletBanner {
  text: string;
  unsupported: string[];
  wrapped: boolean;
}

/** Font metadata for the gallery — mirrors Rust `FontMeta`. */
export interface FigletFontMeta {
  name: string;
  category: string;
  popular: boolean;
  pinned: boolean;
}

/** A compact gallery sample — mirrors Rust `FigletSample`. */
export interface FigletSample {
  font: string;
  sample: string;
}

/** Persisted Settings → Figlet defaults — mirrors Rust `FigletDefaults`. */
export interface FigletDefaults {
  font: string;
  width: number;
  align: FigletAlign;
  trim: boolean;
  comment: FigletComment;
  boxed: boolean;
  pinned: string[];
  save_history: boolean;
}

/** The starting `FigletOpts` implied by the persisted defaults. */
export function optsFromDefaults(d: FigletDefaults): FigletOpts {
  return { width: d.width, align: d.align, trim: d.trim, comment: d.comment, boxed: d.boxed };
}

import { fuzzyScore } from "./commands";

/** Parsed `figlet` command: the free text, an optional `@font` query, and any
 *  `--flag` overrides. Token order is irrelevant — everything that isn't a
 *  `@font` or a known `--flag` is the banner text (joined by spaces). */
export interface ParsedFiglet {
  text: string;
  /** The raw `@font` query (resolve against the catalogue via `fuzzyFont`). */
  fontQuery: string | null;
  /** Flag overrides applied on top of the persisted defaults. */
  opts: Partial<FigletOpts>;
}

const KNOWN_FLAGS = new Set([
  "width", "center", "right", "left", "box", "no-box", "trim", "no-trim", "comment",
]);
const COMMENTS: readonly FigletComment[] = ["none", "slashes", "hash", "block", "html"];

/**
 * Parse the argument string (everything after `figlet `). `@x` selects a font,
 * `--flag[=val]` overrides an option, and everything else is the banner text.
 * An UNKNOWN `--token` is kept as literal text (so `figlet --> go` still
 * renders `--> go`), only known flags are consumed.
 */
export function parseFigletCommand(arg: string): ParsedFiglet {
  const tokens = arg.split(/\s+/).filter(Boolean);
  let fontQuery: string | null = null;
  const opts: Partial<FigletOpts> = {};
  const textParts: string[] = [];

  for (const tok of tokens) {
    if (tok.startsWith("@") && tok.length > 1) {
      fontQuery = tok.slice(1);
      continue;
    }
    if (tok.startsWith("--")) {
      const eq = tok.indexOf("=");
      const key = tok.slice(2, eq === -1 ? undefined : eq);
      const val = eq === -1 ? "" : tok.slice(eq + 1);
      if (KNOWN_FLAGS.has(key)) {
        applyFlag(opts, key, val);
        continue;
      }
    }
    textParts.push(tok);
  }
  return { text: textParts.join(" "), fontQuery, opts };
}

function applyFlag(opts: Partial<FigletOpts>, key: string, val: string) {
  switch (key) {
    case "width": {
      const n = parseInt(val, 10);
      if (Number.isFinite(n) && n >= 0) opts.width = Math.min(n, 400);
      break;
    }
    case "center":
      opts.align = "center";
      break;
    case "right":
      opts.align = "right";
      break;
    case "left":
      opts.align = "left";
      break;
    case "box":
      opts.boxed = true;
      break;
    case "no-box":
      opts.boxed = false;
      break;
    case "trim":
      opts.trim = true;
      break;
    case "no-trim":
      opts.trim = false;
      break;
    case "comment":
      if ((COMMENTS as string[]).includes(val)) opts.comment = val as FigletComment;
      break;
  }
}

/** Resolve a `@font` query to a real font name: exact > prefix > fuzzy
 *  subsequence (reusing the command fuzzy scorer). `null` if nothing matches. */
export function fuzzyFont(query: string, fonts: readonly FigletFontMeta[]): string | null {
  const q = query.trim().toLowerCase();
  if (!q) return null;
  let best: { name: string; score: number } | null = null;
  for (const f of fonts) {
    const s = fuzzyScore(f.name.toLowerCase(), q);
    if (s === null) continue;
    if (!best || s < best.score) best = { name: f.name, score: s };
  }
  return best?.name ?? null;
}

/** Rank fonts for the gallery against a `@font` filter query. Empty query →
 *  pinned first, then popular, then the rest, each alphabetical. With a query →
 *  fuzzy matches only, best first. */
export function galleryFonts(
  query: string | null,
  fonts: readonly FigletFontMeta[],
): FigletFontMeta[] {
  const q = (query ?? "").trim().toLowerCase();
  if (q) {
    return fonts
      .map((f) => ({ f, s: fuzzyScore(f.name.toLowerCase(), q) }))
      .filter((x): x is { f: FigletFontMeta; s: number } => x.s !== null)
      .sort((a, b) => a.s - b.s || a.f.name.localeCompare(b.f.name))
      .map((x) => x.f);
  }
  const rank = (f: FigletFontMeta) => (f.pinned ? 0 : f.popular ? 1 : 2);
  return [...fonts].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
}
