/**
 * `rz` — pixel or percentage resizing (v0.153.0).
 *
 * ⚠️ The mode rule has two cases on purpose, and the reason is backwards
 * compatibility: `rz 1200x800` has meant PIXELS since v0.84.72, and silently
 * turning it into "1200 % × 800 %" would change what an existing muscle-memory
 * input does — it would blow past the 16 MP cap instead of scaling. So:
 *
 *   one number      → percent   (`rz 50`      = 50 % × 50 %)
 *   two numbers     → pixels    (`rz 1200x800`= 1200 × 800 px, unchanged)
 *   named mode wins → always    (`rz % 1200x800`, `rz px 50`)
 *
 * Everything here is pure so the preview, the runnable row and the batch all
 * compute the same target from the same input.
 */
export type ResizeMode = "px" | "pct";

export interface ResizeSpec {
  mode: ResizeMode;
  /** Horizontal value — pixels, or percent of the source width. */
  x: number;
  /** Vertical value. A single number fills both axes. */
  y: number;
  /** Did the user name the mode, or was it inferred from the shape? */
  explicit: boolean;
}

export interface Size {
  w: number;
  h: number;
}

/** Accepted mode words. Both spellings the user asked for, plus the obvious
 *  siblings — a command that rejects `pixels` because it only knows `pixel`
 *  is annoying for no gain. */
export const PX_WORDS = ["px", "pixel", "pixels"] as const;
export const PCT_WORDS = ["%", "pc", "pct", "percent", "prozent"] as const;

export const PCT_MIN = 1;
export const PCT_MAX = 1000;
export const PX_MIN = 1;
export const PX_MAX = 20000;
/** Mirrors `image_ops::MAX_PIXELS` (16 MP) — the backend refuses more. */
export const MAX_PIXELS = 16 * 1024 * 1024;

/**
 * Parse the argument of `rz`. Returns null for anything incomplete or out of
 * range, so no runnable row is offered for input that cannot work.
 */
export function parseResizeCommand(arg: string): ResizeSpec | null {
  let s = arg.trim().toLowerCase();
  if (!s) return null;

  let mode: ResizeMode | null = null;
  // Leading mode word: `px 800x600`, `% 50`, and glued `%50`.
  const lead = s.match(/^(%|[a-z]+)\s*(.*)$/);
  if (lead) {
    const word = lead[1];
    if ((PX_WORDS as readonly string[]).includes(word)) {
      mode = "px";
      s = lead[2].trim();
    } else if ((PCT_WORDS as readonly string[]).includes(word)) {
      mode = "pct";
      s = lead[2].trim();
    } else {
      return null; // an unknown word is a typo, not a silent fallback
    }
  }
  // Trailing percent sign: `50%`, `50x25%`.
  if (mode === null && s.endsWith("%")) {
    mode = "pct";
    s = s.slice(0, -1).trim();
  }
  if (!s) return null;

  const one = s.match(/^(\d+)$/);
  const two = s.match(/^(\d+)(?:\s*[x×]\s*|\s+)(\d+)$/);
  let x: number;
  let y: number;
  if (one) {
    x = y = parseInt(one[1], 10);
    mode ??= "pct"; // ⚠️ a single number is a scale factor
  } else if (two) {
    x = parseInt(two[1], 10);
    y = parseInt(two[2], 10);
    mode ??= "px"; // ⚠️ two numbers stay pixels — unchanged since v0.84.72
  } else {
    return null;
  }

  const explicit = lead != null || arg.trim().endsWith("%");
  const lo = mode === "pct" ? PCT_MIN : PX_MIN;
  const hi = mode === "pct" ? PCT_MAX : PX_MAX;
  if (x < lo || x > hi || y < lo || y > hi) return null;
  if (mode === "px" && x * y > MAX_PIXELS) return null;
  return { mode, x, y, explicit };
}

/**
 * The pixel size `src` becomes under `spec`.
 *
 * ⚠️ Never rounds to zero: a 2 % scale of a 30 px image is 1 px, not 0 — a
 * zero-sized target is rejected by the backend and would fail the whole batch.
 */
export function targetSize(src: Size, spec: ResizeSpec): Size {
  if (spec.mode === "px") return { w: spec.x, h: spec.y };
  return {
    w: Math.max(1, Math.round((src.w * spec.x) / 100)),
    h: Math.max(1, Math.round((src.h * spec.y) / 100)),
  };
}

/** Would this target exceed the backend's area cap? Percent depends on the
 *  source, so this can only be answered per image. */
export function exceedsCap(target: Size): boolean {
  return target.w * target.h > MAX_PIXELS;
}

/** `50 % × 50 %` / `1200 × 800 px` — one phrasing everywhere. */
export function describeSpec(spec: ResizeSpec): string {
  return spec.mode === "pct"
    ? `${spec.x} % × ${spec.y} %`
    : `${spec.x} × ${spec.y} px`;
}

/**
 * Is the user typing `rz` at all? The preview must appear for the BARE keyword
 * too, and `rz` alone is not a complete command (`requiresArg: true`), so
 * keying the panel off the parsed command showed the generic suggestion card
 * instead of the modes — the one thing the preview exists to explain.
 */
export function isResizeQuery(query: string): boolean {
  return /^\s*(rz|resize)\b/i.test(query);
}

/** The part after the keyword: `rz px 50` -> `px 50`, `rz` -> ``. */
export function resizeQueryArg(query: string): string {
  const m = query.trim().match(/^(?:rz|resize)\b\s*(.*)$/i);
  return m ? m[1].trim() : "";
}
