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
