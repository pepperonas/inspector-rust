/**
 * Power-command palette for the popup search bar.
 *
 * Five typed-in shell-style commands the user can run from the
 * search bar. They surface as a special `ListEntry` kind, distinct
 * from clip / snippet / calc / color entries; pressing Enter
 * dispatches them to either an external URL (translation) or a
 * backend IPC (image / text ops).
 *
 *   tren <text>    → translate <text> EN → DE  (open Google Translate)
 *   trde <text>    → translate <text> DE → EN  (open Google Translate)
 *   tr <text>      → translate <text> auto → DE
 *   rz <W>x<H>     → resize clipboard image to W × H (Lanczos3)
 *   optim          → optimise clipboard PNG → save to ~/Downloads
 *   rmvvls <text>  → strip vowels from <text> → clipboard
 *
 * The parser is intentionally strict on the *command keyword* (must
 * be the first token, exact match) and forgiving on whitespace
 * around the argument. Partial-prefix typing surfaces an autocomplete
 * suggestion rather than a runnable command, so accidental typos
 * don't run anything destructive.
 */

export type CommandKind =
  | "translate-en"
  | "translate-de"
  | "translate-auto"
  | "resize"
  | "optim"
  | "rmvvls"
  | "kill"
  | "reboot"
  | "shutdown"
  | "lock";

/** Static metadata for one power command. */
export interface CommandSpec {
  /** Stable id for switch/dispatch. */
  kind: CommandKind;
  /** The keyword the user types as the first token. */
  keyword: string;
  /** Human-readable syntax hint, e.g. `tren <text>`. */
  syntax: string;
  /** One-line description shown in the suggestion list. */
  description: string;
  /** True if the command needs an argument after the keyword. */
  requiresArg: boolean;
}

/** Catalogue of all supported commands. Order = suggestion-list order. */
export const COMMANDS: ReadonlyArray<CommandSpec> = [
  {
    kind: "translate-en",
    keyword: "tren",
    syntax: "tren <text>",
    description: "Translate text English → German (opens Google Translate)",
    requiresArg: true,
  },
  {
    kind: "translate-de",
    keyword: "trde",
    syntax: "trde <text>",
    description: "Translate text German → English (opens Google Translate)",
    requiresArg: true,
  },
  {
    kind: "translate-auto",
    keyword: "tr",
    syntax: "tr <text>",
    description: "Translate text → German (auto-detect source language)",
    requiresArg: true,
  },
  {
    kind: "resize",
    keyword: "rz",
    syntax: "rz <W>x<H>",
    description: "Resize clipboard image (Lanczos3) — e.g. rz 1200x800",
    requiresArg: true,
  },
  {
    kind: "optim",
    keyword: "optim",
    syntax: "optim",
    description: "Optimise clipboard PNG → Downloads (lossless oxipng)",
    requiresArg: false,
  },
  {
    kind: "rmvvls",
    keyword: "rmvvls",
    syntax: "rmvvls <text>",
    description: "Remove vowels from text → clipboard (e.g. rmvvls hello → hll)",
    requiresArg: true,
  },
  // ── System commands ────────────────────────────────────────────────
  {
    kind: "kill",
    keyword: "kill",
    syntax: "kill [-9] [pattern]",
    description: "Kill a process — live picker (e.g. kill slack, kill -9 …)",
    // requiresArg = false → empty `kill` is valid; the UI opens the
    // process picker showing all processes for selection.
    requiresArg: false,
  },
  {
    kind: "reboot",
    keyword: "reboot",
    syntax: "reboot",
    description: "Restart the system (macOS — confirms before executing)",
    requiresArg: false,
  },
  {
    kind: "shutdown",
    keyword: "shutdown",
    syntax: "shutdown",
    description: "Power off the system (macOS — confirms before executing)",
    requiresArg: false,
  },
  {
    kind: "lock",
    keyword: "lock",
    syntax: "lock",
    description: "Lock the screen (macOS — no confirmation, instant)",
    requiresArg: false,
  },
];

/** Lookup by exact keyword. O(n=6); a HashMap would be premature. */
function lookupKeyword(keyword: string): CommandSpec | undefined {
  return COMMANDS.find((c) => c.keyword === keyword);
}

/** Parsed command + its raw arg string ("" if none). */
export interface ParsedCommand {
  spec: CommandSpec;
  arg: string;
}

/**
 * Parse `query` for a *complete* command invocation. Returns the
 * parsed shape only if (a) the first token matches a known keyword
 * AND (b) the argument requirement is satisfied.
 *
 * Returns `null` for partial input ("tre"), unknown keywords, and
 * commands missing their required argument ("tren " with empty arg).
 */
export function parseCommand(query: string): ParsedCommand | null {
  const trimmed = query.trimStart();
  if (trimmed.length === 0) return null;

  // Find the first whitespace to split keyword from args.
  const space = trimmed.search(/\s/);
  const keyword = space === -1 ? trimmed : trimmed.slice(0, space);
  const arg = space === -1 ? "" : trimmed.slice(space + 1).trim();

  const spec = lookupKeyword(keyword);
  if (!spec) return null;

  if (spec.requiresArg && arg.length === 0) {
    // Keyword recognised but argument missing — caller can surface
    // an autocomplete suggestion instead of running the command.
    return null;
  }

  return { spec, arg };
}

/**
 * Return commands whose keyword starts with the *first token* of
 * `query` (case-insensitive). Used to render autocomplete suggestions
 * under the search bar.
 *
 * - Empty input → no suggestions (would clutter the History list).
 * - Exact keyword match → no suggestions (the command itself surfaces
 *   as a ParsedCommand instead).
 * - Partial match → all commands whose keyword starts with the prefix.
 */
export function commandSuggestions(query: string): CommandSpec[] {
  const trimmed = query.trimStart();
  if (trimmed.length === 0) return [];

  const space = trimmed.search(/\s/);
  // Once the user has typed a space (= moved past the keyword), they're
  // either filling in an arg or composing a non-command query — no
  // command suggestions either way.
  if (space !== -1) return [];

  const firstToken = trimmed.toLowerCase();
  if (firstToken.length === 0) return [];

  // Prefix match across ALL keywords. Critical for `tr` — `tr` is both
  // an exact command AND a prefix for `tren`/`trde`. The user typing
  // "tr" might mean any of the three; surface them all and let them
  // pick.
  const matches = COMMANDS.filter((c) => c.keyword.startsWith(firstToken));

  // Suppress no-arg exact matches — the user can run them with Enter
  // directly via the parseCommand path, and an autocomplete row showing
  // the same keyword they just typed is pure clutter.
  return matches.filter((c) => !(c.keyword === firstToken && !c.requiresArg));
}

/** Build the Google Translate URL for a translate command. */
export function translateUrl(kind: CommandKind, text: string): string {
  const encoded = encodeURIComponent(text);
  switch (kind) {
    case "translate-en":
      return `https://translate.google.com/?sl=en&tl=de&text=${encoded}&op=translate`;
    case "translate-de":
      return `https://translate.google.com/?sl=de&tl=en&text=${encoded}&op=translate`;
    case "translate-auto":
      return `https://translate.google.com/?sl=auto&tl=de&text=${encoded}&op=translate`;
    default:
      throw new Error(`translateUrl called with non-translation kind: ${kind}`);
  }
}

/**
 * Parse `rz <W>x<H>` arg into integers. Accepts "1200x800", "1200X800",
 * "1200 x 800", " 1200x800 ". Returns null on malformed input so the
 * caller can show a syntax-error suggestion instead of crashing.
 */
export function parseResizeArg(arg: string): { width: number; height: number } | null {
  const match = arg.trim().match(/^(\d+)\s*[xX]\s*(\d+)$/);
  if (!match) return null;
  const width = parseInt(match[1], 10);
  const height = parseInt(match[2], 10);
  if (width <= 0 || height <= 0) return null;
  return { width, height };
}

/**
 * Parse the kill command's argument into `{ force, pattern }`.
 * - `kill <pattern>`     → force=false, pattern=<pattern>
 * - `kill -9 <pattern>`  → force=true,  pattern=<pattern>
 * - `kill -9`            → force=true,  pattern=""   (show all, picker)
 * - `kill`               → force=false, pattern=""   (show all, picker)
 *
 * Pattern matching is case-insensitive substring on the process name.
 */
export function parseKillArg(arg: string): { force: boolean; pattern: string } {
  const trimmed = arg.trim();
  if (trimmed === "-9" || trimmed.startsWith("-9 ")) {
    return { force: true, pattern: trimmed.slice(2).trim() };
  }
  return { force: false, pattern: trimmed };
}
