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
  | "lock"
  | "mute"
  | "freeze"
  | "wakelock-on"
  | "wakelock-off"
  | "bruno"
  | "timer"
  | "pwgen";

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
   *  autocomplete suggestion list. Used for alias spellings (e.g.
   *  `wakelock1` as a synonym for `wakelock=1`). */
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
  // ── Wakelock (mouse-jiggle keep-awake) ────────────────────────────
  // Two canonical forms visible in autocomplete; two un-equalsed
  // aliases (`wakelock1` / `wakelock0`) parse to the same kinds but
  // stay out of the suggestion list to keep it tidy.
  {
    kind: "wakelock-on",
    keyword: "wakelock=1",
    syntax: "wakelock=1",
    description:
      "Keep awake — nudge the cursor 1 px every 60 s until you turn it off (wakelock=0)",
    requiresArg: false,
  },
  {
    kind: "wakelock-off",
    keyword: "wakelock=0",
    syntax: "wakelock=0",
    description: "Disable the wakelock — stop the cursor jiggle",
    requiresArg: false,
  },
  {
    kind: "wakelock-on",
    keyword: "wakelock1",
    syntax: "wakelock1",
    description: "(alias of wakelock=1)",
    requiresArg: false,
    hidden: true,
  },
  {
    kind: "wakelock-off",
    keyword: "wakelock0",
    syntax: "wakelock0",
    description: "(alias of wakelock=0)",
    requiresArg: false,
    hidden: true,
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
  // ── Password generator ────────────────────────────────────────────
  {
    kind: "pwgen",
    keyword: "pwgen",
    syntax: "pwgen <N>",
    description:
      "Password generator. e.g. `pwgen 16`. Enter copies; Alt+Enter = alphanumeric only. Dict + leet modes via preview-pane buttons.",
    requiresArg: true,
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
  const matches = COMMANDS.filter(
    (c) => !c.hidden && c.keyword.startsWith(firstToken),
  );

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
 * Hidden easter egg: typing `space` turns the popup into Space Invaders.
 *
 * Like the other game triggers, NOT in `COMMANDS` / autocomplete — exact
 * match only (whitespace-tolerant, case-insensitive).
 */
export function isSpaceInvadersTrigger(query: string): boolean {
  return query.trim().toLowerCase() === "space";
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
