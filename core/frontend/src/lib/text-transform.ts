/**
 * String-manipulation transforms for selected text history entries.
 *
 * Pure, side-effect-free functions — the transform runs in the
 * frontend; the *result* is committed to the clipboard + History via
 * the `commit_transformed_text` IPC. Triggered from `PreviewPanel`
 * by `Cmd/Ctrl+1…9` or the numbered toolbar chips.
 */

export type TransformKind =
  | "remove-vowels"
  | "upper"
  | "lower"
  | "title"
  | "camel"
  | "snake"
  | "kebab"
  | "base64-encode"
  | "url-encode"
  | "base64-decode"
  | "url-decode";

export interface TransformSpec {
  kind: TransformKind;
  /** Toolbar / cheat-sheet label. */
  label: string;
  /** 1–9 → bound to `Cmd/Ctrl+<digit>`. Undefined → click-only. */
  digit?: number;
}

/** The transform catalogue. Array order = toolbar order. The first
 *  nine carry a `digit` and get a `Cmd/Ctrl+<n>` keyboard shortcut;
 *  the decode pair is click-only. */
export const TRANSFORMS: ReadonlyArray<TransformSpec> = [
  { kind: "remove-vowels", label: "Remove vowels", digit: 1 },
  { kind: "upper", label: "UPPERCASE", digit: 2 },
  { kind: "lower", label: "lowercase", digit: 3 },
  { kind: "title", label: "Title Case", digit: 4 },
  { kind: "camel", label: "camelCase", digit: 5 },
  { kind: "snake", label: "snake_case", digit: 6 },
  { kind: "kebab", label: "kebab-case", digit: 7 },
  { kind: "base64-encode", label: "Base64 encode", digit: 8 },
  { kind: "url-encode", label: "URL encode", digit: 9 },
  { kind: "base64-decode", label: "Base64 decode" },
  { kind: "url-decode", label: "URL decode" },
];

// ── word tokeniser (shared by camel / snake / kebab) ──────────────────

/** Split a string into words. Breaks on whitespace, `_`, `-` *and*
 *  camelCase boundaries, so "helloWorld", "hello_world" and
 *  "hello world" all tokenise to ["hello", "world"]. */
function words(s: string): string[] {
  return s
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2") // break camelCase boundaries
    .split(/[\s_-]+/)
    .filter((w) => w.length > 0);
}

/** Capitalise: first letter upper, rest lower. */
function cap(w: string): string {
  return w.charAt(0).toUpperCase() + w.slice(1).toLowerCase();
}

// ── base64 (Unicode-safe — btoa/atob are byte-oriented) ───────────────

function base64Encode(s: string): string {
  const bytes = new TextEncoder().encode(s);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin);
}

function base64Decode(s: string): string {
  // Invalid base64 → return the input unchanged (a harmless no-op:
  // the result equals the source, dedup just bumps last_used_at).
  try {
    const bin = atob(s.trim());
    const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
    return new TextDecoder().decode(bytes);
  } catch {
    return s;
  }
}

// ── the transforms ────────────────────────────────────────────────────

/** Apply a transform. Total — never throws; malformed decode input
 *  falls back to returning the source string unchanged. */
export function applyTransform(kind: TransformKind, input: string): string {
  switch (kind) {
    case "remove-vowels":
      return input.replace(/[aeiouAEIOUäöüÄÖÜ]/g, "");
    case "upper":
      return input.toUpperCase();
    case "lower":
      return input.toLowerCase();
    case "title":
      return input
        .toLowerCase()
        .replace(/(^|\s)(\S)/g, (_m, sp: string, ch: string) => sp + ch.toUpperCase());
    case "camel":
      return words(input)
        .map((w, i) => (i === 0 ? w.toLowerCase() : cap(w)))
        .join("");
    case "snake":
      return words(input)
        .map((w) => w.toLowerCase())
        .join("_");
    case "kebab":
      return words(input)
        .map((w) => w.toLowerCase())
        .join("-");
    case "base64-encode":
      return base64Encode(input);
    case "url-encode":
      return encodeURIComponent(input);
    case "base64-decode":
      return base64Decode(input);
    case "url-decode":
      try {
        return decodeURIComponent(input);
      } catch {
        return input; // malformed % sequence → no-op
      }
  }
}
