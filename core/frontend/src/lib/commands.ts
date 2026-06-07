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
 *   trde2it/trit2de, trde2sp/trsp2de, trde2pl/trpl2de
 *                  → German ↔ Italian / Spanish / Polish (Google Translate)
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

import { MEME_ENABLED } from "./meme";

export type CommandKind =
  | "translate-en"
  | "translate-de"
  | "translate-auto"
  | "translate-de-it"
  | "translate-it-de"
  | "translate-de-es"
  | "translate-es-de"
  | "translate-de-pl"
  | "translate-pl-de"
  | "resize"
  | "optim"
  | "rmvvls"
  | "kill"
  | "reboot"
  | "shutdown"
  | "lock"
  | "mute"
  | "freeze"
  | "wakelock"
  | "bruno"
  | "timer"
  | "pwgen"
  | "touch"
  | "mkdir"
  | "terminal"
  | "alarm"
  | "md2pdf"
  | "shot-region"
  | "shot-full"
  | "shot-window"
  | "shot-last"
  | "clean"
  | "brightness"
  | "random"
  | "meme";

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
  /** If true, parses to a runnable command but never appears in the
   *  autocomplete suggestion list. Reserved for alias spellings that
   *  shouldn't clutter the list. */
  hidden?: boolean;
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
    kind: "translate-de-it",
    keyword: "trde2it",
    syntax: "trde2it <text>",
    description: "Translate text German → Italian (opens Google Translate)",
    requiresArg: true,
  },
  {
    kind: "translate-it-de",
    keyword: "trit2de",
    syntax: "trit2de <text>",
    description: "Translate text Italian → German (opens Google Translate)",
    requiresArg: true,
  },
  {
    kind: "translate-de-es",
    keyword: "trde2sp",
    syntax: "trde2sp <text>",
    description: "Translate text German → Spanish (opens Google Translate)",
    requiresArg: true,
  },
  {
    kind: "translate-es-de",
    keyword: "trsp2de",
    syntax: "trsp2de <text>",
    description: "Translate text Spanish → German (opens Google Translate)",
    requiresArg: true,
  },
  {
    kind: "translate-de-pl",
    keyword: "trde2pl",
    syntax: "trde2pl <text>",
    description: "Translate text German → Polish (opens Google Translate)",
    requiresArg: true,
  },
  {
    kind: "translate-pl-de",
    keyword: "trpl2de",
    syntax: "trpl2de <text>",
    description: "Translate text Polish → German (opens Google Translate)",
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
  {
    kind: "mute",
    keyword: "mute",
    syntax: "mute",
    description: "Toggle system mute / unmute (macOS)",
    requiresArg: false,
  },
  {
    kind: "freeze",
    keyword: "freeze",
    syntax: "freeze",
    description:
      "Block all keyboard / mouse input — unlock with the configured chord (default: i + r)",
    requiresArg: false,
  },
  // ── Wakelock / caffeine (keep-awake) ──────────────────────────────
  // `wakelock on|off` and the `caffeine on|off` alias. The on/off arg
  // (no more `=1`/`=0`) parses via `parseWakelockArg`.
  {
    kind: "wakelock",
    keyword: "wakelock",
    syntax: "wakelock on|off",
    description:
      "Keep awake (pause sleep + screen lock). e.g. `wakelock on` / `wakelock off`",
    requiresArg: true,
  },
  {
    kind: "wakelock",
    keyword: "caffeine",
    syntax: "caffeine on|off",
    description: "Keep awake — alias of wakelock. e.g. `caffeine on` / `caffeine off`",
    requiresArg: true,
  },
  // ── Bruno — German income-tax / net-pay calculator ────────────────
  {
    kind: "bruno",
    keyword: "bruno",
    syntax: "bruno <€>[m|j]",
    description:
      "Brutto → Netto (2025). e.g. `bruno 60000` (yearly) or `bruno 5000m` (monthly)",
    requiresArg: true,
  },
  // ── Timer ─────────────────────────────────────────────────────────
  {
    kind: "timer",
    keyword: "timer",
    syntax: "timer <N>[s|min|h]",
    description:
      "Timer + visual/audio notification. e.g. `timer 12` (12 min) · `timer 30s` · `timer 2h`",
    requiresArg: true,
  },
  // ── Alarm (absolute clock time) ───────────────────────────────────
  {
    kind: "alarm",
    keyword: "alarm",
    syntax: "alarm <HH:MM>",
    description:
      "Alarm at a clock time (next occurrence). e.g. `alarm 3:00` (3 AM) · `alarm 15:15`",
    requiresArg: true,
  },
  // ── Password generator ────────────────────────────────────────────
  {
    kind: "pwgen",
    keyword: "pwgen",
    syntax: "pwgen [N]",
    description:
      "Password generator. Bare `pwgen` uses a default length; `pwgen 16` sets it. Enter copies; Alt+Enter = alphanumeric only. Dict + leet modes via preview-pane buttons.",
    // No required arg — bare `pwgen` immediately surfaces the generator
    // action (default length) at the top instead of a faint suggestion
    // that lets matching snippets outrank it.
    requiresArg: false,
  },
  // ── Finder file/folder creation (in the front Finder window's dir) ──
  {
    kind: "touch",
    keyword: "touch",
    syntax: "touch <name>",
    description: "Create an empty file in the frontmost Finder window's folder",
    requiresArg: true,
  },
  {
    kind: "mkdir",
    keyword: "mkdir",
    syntax: "mkdir <name>",
    description: "Create a new folder in the frontmost Finder window's folder",
    requiresArg: true,
  },
  {
    kind: "terminal",
    keyword: "terminal",
    syntax: "terminal",
    description: "Open the terminal (iTerm2 / Terminal) at the frontmost Finder folder",
    requiresArg: false,
  },
  // ── Markdown → PDF ─────────────────────────────────────────────────
  {
    kind: "md2pdf",
    keyword: "md2pdf",
    syntax: "md2pdf [path]",
    description:
      "Convert Markdown → PDF (same as Ctrl+Shift+M). Bare = file-manager selection; or `md2pdf <path>`",
    requiresArg: false,
  },
  {
    kind: "shot-region",
    keyword: "shot",
    syntax: "shot [seconds]",
    description:
      "Screenshot a region (same as Ctrl+Shift+S). Optional self-timer, e.g. `shot 3`",
    requiresArg: false,
  },
  {
    kind: "shot-full",
    keyword: "shotfull",
    syntax: "shotfull [seconds]",
    description: "Screenshot the whole screen (optional self-timer)",
    requiresArg: false,
  },
  {
    kind: "shot-window",
    keyword: "shotwin",
    syntax: "shotwin [seconds]",
    description: "Screenshot the active window (optional self-timer)",
    requiresArg: false,
  },
  {
    kind: "shot-last",
    keyword: "shotlast",
    syntax: "shotlast",
    description: "Repeat the last screenshot mode",
    requiresArg: false,
  },
  {
    kind: "clean",
    keyword: "clean",
    syntax: "clean",
    description: "Clean caches/logs — shows a preview first, deletes only after you confirm",
    requiresArg: false,
  },
  {
    kind: "clean",
    keyword: "cleanup",
    syntax: "cleanup",
    description: "Alias for clean",
    requiresArg: false,
    hidden: true,
  },
  {
    kind: "brightness",
    keyword: "brightness",
    syntax: "brightness",
    description: "Adjust monitor brightness — opens a slider per (DDC) monitor",
    requiresArg: false,
  },
  {
    kind: "brightness",
    keyword: "bri",
    syntax: "bri",
    description: "Alias for brightness",
    requiresArg: false,
    hidden: true,
  },
  {
    kind: "random",
    keyword: "rnd",
    syntax: "rnd [max] | rnd [min] [max]",
    description: "Roll a random number (default 1–6); `rnd 100` = 1–100, `rnd 5 500` = 5–500",
    requiresArg: false,
  },
  {
    kind: "random",
    keyword: "random",
    syntax: "random [max] | random [min] [max]",
    description: "Alias for rnd",
    requiresArg: false,
    hidden: true,
  },
  // Meme picker — only present in builds with the feature enabled
  // (VITE_IR_MEME !== "0"). A "without memes" build omits it entirely.
  ...(MEME_ENABLED
    ? [
        {
          kind: "meme" as const,
          keyword: "meme",
          syntax: "meme [query]",
          description: "Browse the meme library — fuzzy-filter, preview, Enter copies",
          requiresArg: false,
        },
      ]
    : []),
];

/** Parse the `rnd` / `random` argument into an inclusive `[min, max]`.
 *  - empty            → 1–6 (a die)
 *  - one number `N`   → 1–N
 *  - two numbers `A B`→ A–B (swapped if A > B)
 *  Returns `null` for anything non-integer or more than two numbers. */
export function parseRandomArg(arg: string): { min: number; max: number } | null {
  const t = arg.trim();
  if (t === "") return { min: 1, max: 6 };
  const parts = t.split(/\s+/);
  if (parts.length > 2) return null;
  if (!parts.every((p) => /^-?\d+$/.test(p))) return null;
  const nums = parts.map((p) => parseInt(p, 10));
  let min: number;
  let max: number;
  if (nums.length === 1) {
    min = 1;
    max = nums[0];
  } else {
    min = nums[0];
    max = nums[1];
  }
  if (min > max) [min, max] = [max, min];
  return { min, max };
}

/** Uniform random integer in the inclusive range `[min, max]`, using the
 *  CSPRNG with rejection sampling to avoid modulo bias (same approach as
 *  the password generator). */
export function randomInt(min: number, max: number): number {
  const lo = Math.min(min, max);
  const hi = Math.max(min, max);
  const range = hi - lo + 1;
  if (range <= 1) return lo;
  const buf = new Uint32Array(1);
  // Largest multiple of `range` that fits in u32 — reject above it.
  const limit = Math.floor(0x1_0000_0000 / range) * range;
  let x: number;
  do {
    crypto.getRandomValues(buf);
    x = buf[0];
  } while (x >= limit);
  return lo + (x % range);
}

/** Format a byte count as a short human string (e.g. 1.2 MB). */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

/** Parse an optional self-timer seconds argument for the screenshot
 *  commands. Empty / non-numeric → 0 (no delay). Capped at 60. */
export function parseShotDelay(arg: string): number {
  const n = parseInt(arg.trim(), 10);
  return Number.isFinite(n) && n > 0 ? Math.min(n, 60) : 0;
}

/** Default password length when `pwgen` is typed without a number. */
export const DEFAULT_PWGEN_LENGTH = 12;

/** Parse a wakelock/caffeine on/off argument. Accepts on/off (the new
 *  canonical syntax) plus 1/0/true/false for leniency. Returns the
 *  desired enabled state, or `null` if unrecognised. */
export function parseWakelockArg(arg: string): boolean | null {
  switch (arg.trim().toLowerCase()) {
    case "on":
    case "1":
    case "true":
    case "an":
      return true;
    case "off":
    case "0":
    case "false":
    case "aus":
      return false;
    default:
      return null;
  }
}

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
 * Fuzzy-match score of `needle` against a command `keyword`. Returns
 * `null` when there's no match, else a score where **lower is better**.
 *
 * Tiers (so prefix matches always rank above loose fuzzy ones):
 *   - exact keyword            → -10000
 *   - prefix (`wak` → wakelock`) → -5000 + len  (shorter keyword wins)
 *   - subsequence (`wlk` → wakelock) → positive: fewer gaps + shorter wins
 *
 * The subsequence tier is **first-character-anchored** and only enabled
 * for needles of 3+ chars, so 1–2 char queries stay conservative
 * (prefix-only) and a stray short query doesn't flood the list.
 */
export function fuzzyScore(keyword: string, needle: string): number | null {
  if (needle.length === 0) return 0;
  if (keyword === needle) return -10000;
  if (keyword.startsWith(needle)) return -5000 + keyword.length;
  if (needle.length < 3 || keyword[0] !== needle[0]) return null;

  // First char anchored; walk the rest as an ordered subsequence.
  let k = 1;
  let prev = 0;
  let gaps = 0;
  for (let n = 1; n < needle.length; n++) {
    const ch = needle[n];
    let found = -1;
    while (k < keyword.length) {
      if (keyword[k] === ch) {
        found = k;
        k++;
        break;
      }
      k++;
    }
    if (found === -1) return null;
    gaps += found - prev - 1;
    prev = found;
  }
  return gaps * 3 + keyword.length;
}

/**
 * Return commands matching the *first token* of `query` (case-insensitive),
 * ranked best-first. Used to render autocomplete suggestions under the
 * search bar. Matching is **fuzzy** (see {@link fuzzyScore}): a prefix like
 * `wak`, OR a 3+ char first-char-anchored subsequence like `wlk` / `frz` /
 * `pwg`, surfaces the command so it can be invoked without typing it in full.
 *
 * - Empty input → no suggestions (would clutter the History list).
 * - Exact match of a no-arg command → no suggestion (it runs directly via
 *   the ParsedCommand path; an echo row would be clutter).
 */
export function commandSuggestions(query: string): CommandSpec[] {
  const trimmed = query.trimStart();
  if (trimmed.length === 0) return [];

  const space = trimmed.search(/\s/);
  // Once the user has typed a space (= moved past the keyword), they're
  // either filling in an arg or composing a non-command query — no
  // command suggestions either way.
  if (space !== -1) return [];

  const needle = trimmed.toLowerCase();
  if (needle.length === 0) return [];

  return COMMANDS.filter((c) => !c.hidden)
    .map((c) => ({ c, score: fuzzyScore(c.keyword.toLowerCase(), needle) }))
    .filter((s): s is { c: CommandSpec; score: number } => s.score !== null)
    .sort((a, b) => a.score - b.score || a.c.keyword.localeCompare(b.c.keyword))
    .map((s) => s.c)
    // Suppress a no-arg EXACT match — it runs with Enter via parseCommand,
    // so an autocomplete row echoing what was just typed is pure clutter.
    .filter((c) => !(c.keyword === needle && !c.requiresArg));
}

/**
 * Source/target Google-Translate language codes per translate command kind.
 * `sl`/`tl` are Google's `sl=`/`tl=` query params (Spanish is `es`, even
 * though the command keyword spells it `sp`). Also drives the row label /
 * hint, so a new language pair is a single entry here + a `COMMANDS` row.
 */
export const TRANSLATE_LANGS: Partial<
  Record<CommandKind, { sl: string; tl: string; target: string }>
> = {
  "translate-en": { sl: "en", tl: "de", target: "German" },
  "translate-de": { sl: "de", tl: "en", target: "English" },
  "translate-auto": { sl: "auto", tl: "de", target: "German" },
  "translate-de-it": { sl: "de", tl: "it", target: "Italian" },
  "translate-it-de": { sl: "it", tl: "de", target: "German" },
  "translate-de-es": { sl: "de", tl: "es", target: "Spanish" },
  "translate-es-de": { sl: "es", tl: "de", target: "German" },
  "translate-de-pl": { sl: "de", tl: "pl", target: "Polish" },
  "translate-pl-de": { sl: "pl", tl: "de", target: "German" },
};

/** Whether a command kind is one of the Google-Translate commands. */
export function isTranslateKind(kind: CommandKind): boolean {
  return kind in TRANSLATE_LANGS;
}

/** Build the Google Translate URL for a translate command. */
export function translateUrl(kind: CommandKind, text: string): string {
  const pair = TRANSLATE_LANGS[kind];
  if (!pair) {
    throw new Error(`translateUrl called with non-translation kind: ${kind}`);
  }
  const encoded = encodeURIComponent(text);
  return `https://translate.google.com/?sl=${pair.sl}&tl=${pair.tl}&text=${encoded}&op=translate`;
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

/** Canonical resize-dimension presets surfaced as autocomplete rows
 *  once the user has typed `rz` (or `rz <partial>`). Selecting one
 *  with **Enter** runs the resize immediately; **Tab** or **→**
 *  (with the cursor at the end of the input) fills it into the
 *  search bar so the user can tweak before running. */
export const RESIZE_PRESETS: ReadonlyArray<{ dims: string; label: string }> = [
  { dims: "1920x1080", label: "Full HD · 1920×1080" },
  { dims: "1280x720", label: "HD · 1280×720" },
  { dims: "1024x768", label: "XGA · 1024×768" },
  { dims: "800x600", label: "SVGA · 800×600" },
  { dims: "500x500", label: "Square · 500×500" },
  { dims: "200x200", label: "Thumb · 200×200" },
  { dims: "100x100", label: "Icon · 100×100" },
];

/** One context-aware suggestion (e.g. a resize preset). Same shape
 *  the regular `command-suggestion` rows use — `completion` is the
 *  full runnable command, `label` + `description` drive the row. */
export interface PresetSuggestion {
  completion: string;
  label: string;
  description: string;
}

/** When the user has typed `rz`, `rz `, or `rz <partial-dims>`,
 *  return the matching presets. Empty when the user has already
 *  typed a complete `<W>x<H>` (the runnable command row already
 *  shows what would happen, so presets would be noise). Filtering
 *  is by `dims.startsWith(partial)` — typing `rz 1` narrows to
 *  the four presets starting with `1`. */
export function resizePresetSuggestions(query: string): PresetSuggestion[] {
  const trimmed = query.trimStart();
  // `rz` alone, `rz `, or `rz <stuff>`. The `\b` rejects `rzz` etc.
  const m = trimmed.match(/^rz\b\s*(.*)$/i);
  if (!m) return [];
  const partial = m[1].trim().toLowerCase();
  // Complete WxH already — let the runnable command row carry it.
  if (/^\d+\s*[x×]\s*\d+$/i.test(partial)) return [];
  return RESIZE_PRESETS.filter(
    (p) => !partial || p.dims.toLowerCase().startsWith(partial),
  ).map((p) => ({
    completion: `rz ${p.dims}`,
    label: `rz ${p.dims}`,
    description: p.label,
  }));
}

/**
 * Hidden easter egg: `getshaky` turns the popup into a game of Pong.
 *
 * Deliberately NOT a member of `COMMANDS` — it must never surface in
 * the autocomplete suggestions. It only triggers on an exact, fully
 * typed match. Whitespace-tolerant + case-insensitive so "GetShaky "
 * still works, but you have to know the word.
 */
export function isGetShakyTrigger(query: string): boolean {
  return query.trim().toLowerCase() === "getshaky";
}

/**
 * Hidden easter egg: typing `opener` (optionally followed by anything,
 * separated by a word boundary) surfaces a random German pickup-line
 * "opener" from the embedded top-100 list.
 *
 * Word-boundary anchoring on purpose: matches `opener`, `Opener`,
 * `opener foo`, `opener xxxxxx` (so additional keystrokes re-roll via
 * the seed-hash picker), but NOT `openers` (plural) or `bopener`.
 *
 * Like `isGetShakyTrigger` / `rockTheBoxMode`, deliberately NOT a member
 * of `COMMANDS` — never surfaces in autocomplete.
 */
export function isOpenerTrigger(query: string): boolean {
  return /^opener\b/i.test(query.trim());
}

/** Which Snake variant a `rockthebox`-family trigger word selects. */
export type SnakeMode = "classic" | "wrap";

/**
 * Hidden easter egg: the `rockthebox` family turns the popup into a
 * game of Snake. The trigger word picks the variant:
 *   `rockthebox` → "classic" — hitting a wall ends the game.
 *   `rockthabox` → "wrap"    — the snake reappears on the opposite side.
 * Returns the mode, or `null` if `query` isn't a trigger word.
 *
 * Like `isGetShakyTrigger`, deliberately NOT a member of `COMMANDS` —
 * it must never surface in autocomplete. Exact, whitespace-tolerant,
 * case-insensitive match.
 */
export function rockTheBoxMode(query: string): SnakeMode | null {
  const q = query.trim().toLowerCase();
  if (q === "rockthebox") return "classic";
  if (q === "rockthabox") return "wrap";
  return null;
}

/**
 * `learningtofly` → the hidden Flappy-Bird game. Like the other game
 * triggers, NOT in `COMMANDS` / autocomplete — exact match only
 * (whitespace-tolerant, case-insensitive).
 */
export function isFlappyTrigger(query: string): boolean {
  return query.trim().toLowerCase() === "learningtofly";
}

/**
 * `spacer` → the hidden Space-Invaders game. Like the other game triggers,
 * NOT in `COMMANDS` / autocomplete — exact match only (whitespace-tolerant,
 * case-insensitive).
 */
export function isSpaceInvadersTrigger(query: string): boolean {
  return query.trim().toLowerCase() === "spacer";
}

/**
 * `bpm` → surface a "Detect BPM" row at the top of the list. Unlike
 * the game-mode triggers above, BPM mode is **Enter-activated**, not
 * instant-on-type: short word with too many possible false positives
 * (`bpms`, `bpmusic`, …) for an instant takeover. The trigger only
 * fires on the exact word, whitespace-tolerant + case-insensitive.
 *
 * Listed here next to its siblings; the App.tsx command-builder reads
 * this to decide whether to emit the bpm ListEntry row.
 */
export function isBpmTrigger(query: string): boolean {
  return query.trim().toLowerCase() === "bpm";
}

/**
 * `2fa` → surface a "2FA / TOTP management" row that on Enter opens
 * the full overlay (add / import / export / delete entries). Exact
 * match, whitespace + case tolerant.
 */
export function is2faTrigger(query: string): boolean {
  return query.trim().toLowerCase() === "2fa";
}

/**
 * `otp <query>` → autocomplete trigger for TOTP entries. Returns the
 * trimmed query portion (so `otp ama` → `"ama"`) when the input
 * starts with `otp` followed by a space; otherwise null.
 *
 * Special-case: bare `otp` (no space, no query) returns the empty
 * string so the autocomplete shows all entries unfiltered.
 */
export function parseOtpQuery(query: string): string | null {
  const m = query.match(/^otp(\s+(.*))?$/i);
  if (!m) return null;
  return (m[2] ?? "").trim();
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

/** Parsed `timer` invocation: how many seconds + a display label
 *  ("12 min", "30 sec", "2 h") for the notification. */
export interface TimerSpec {
  seconds: number;
  label: string;
}

/**
 * Parse a `timer` command. Accepts (case-insensitive, whitespace-
 * forgiving):
 *
 *   `timer 12`        → 12 minutes (default unit when none given)
 *   `timer 12 m`
 *   `timer 12min`     → 12 minutes (`min` / `mins` / `m`)
 *   `timer 12 mins`
 *   `timer 30s`       → 30 seconds (`s` / `sec` / `secs` / `sek`)
 *   `timer 30 sek`
 *   `timer 2h`        → 2 hours (`h` / `hr` / `hrs` / `hour` / `hours`)
 *
 * Returns `null` for any unparseable / empty / zero / negative input.
 * The "default unit = minutes" choice reflects the dominant use case
 * (pomodoro / cooking / break).
 */
export function parseTimerArg(arg: string): TimerSpec | null {
  const trimmed = arg.trim();
  if (trimmed.length === 0) return null;
  // Number, optional whitespace, optional unit token.
  const m = trimmed.match(/^(\d+(?:[.,]\d+)?)\s*([a-zäöüß]*)$/i);
  if (!m) return null;
  const n = parseFloat(m[1].replace(",", "."));
  if (!Number.isFinite(n) || n <= 0) return null;
  const unit = m[2].toLowerCase();

  // Sub-second precision wouldn't be useful (notification latency
  // alone is ~100 ms). Round to nearest whole second after applying
  // the unit multiplier.
  const SECONDS = new Set(["s", "sec", "secs", "sek", "second", "seconds", "sekunde", "sekunden"]);
  const MINUTES = new Set(["", "m", "min", "mins", "minute", "minutes", "minuten"]);
  const HOURS = new Set(["h", "hr", "hrs", "hour", "hours", "std", "stunde", "stunden"]);

  let seconds: number;
  let labelUnit: string;
  if (SECONDS.has(unit)) {
    seconds = Math.round(n);
    labelUnit = n === 1 ? "second" : "seconds";
  } else if (HOURS.has(unit)) {
    seconds = Math.round(n * 3600);
    labelUnit = n === 1 ? "hour" : "hours";
  } else if (MINUTES.has(unit)) {
    seconds = Math.round(n * 60);
    labelUnit = n === 1 ? "minute" : "minutes";
  } else {
    return null;
  }
  if (seconds < 1) return null;

  // Strip trailing `.0` so `12.0 min` displays as `12 minutes`.
  const numText = Number.isInteger(n) ? String(n) : String(n);
  const label = `${numText} ${labelUnit}`;
  return { seconds, label };
}

/**
 * Parse an `alarm <HH:MM>` argument into seconds-until-fire + a clock
 * label. 24-hour clock; fires at the NEXT occurrence (today if still in
 * the future, otherwise tomorrow). Accepts `H:MM` / `HH:MM`, and a bare
 * `H`/`HH` (→ `:00`). `now` is injectable for deterministic tests.
 */
export function parseAlarmArg(arg: string, now: Date = new Date()): TimerSpec | null {
  const m = arg.trim().match(/^(\d{1,2})(?::(\d{2}))?$/);
  if (!m) return null;
  const hh = parseInt(m[1], 10);
  const mm = m[2] != null ? parseInt(m[2], 10) : 0;
  if (hh > 23 || mm > 59) return null;

  const target = new Date(now);
  target.setHours(hh, mm, 0, 0);
  // Already passed today → the next occurrence is tomorrow.
  if (target.getTime() <= now.getTime()) {
    target.setDate(target.getDate() + 1);
  }
  const seconds = Math.round((target.getTime() - now.getTime()) / 1000);
  if (seconds < 1) return null;
  return { seconds, label: `${hh}:${String(mm).padStart(2, "0")}` };
}

/** Parse `pwgen <N>` length argument. Returns the integer length
 *  clamped to a sane range, or `null` for non-numeric / zero / too-big.
 *
 *  - Min: 4 chars (anything shorter is trivially brute-forceable).
 *  - Max: 128 chars (web password fields often cap there).
 */
export function parsePwgenArg(arg: string): number | null {
  const trimmed = arg.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const n = parseInt(trimmed, 10);
  if (!Number.isFinite(n) || n < 4 || n > 128) return null;
  return n;
}
