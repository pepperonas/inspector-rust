/**
 * Tab completion for the ARGUMENTS of search-bar commands (v0.207.0).
 *
 * The keyword itself completes through the `command-suggestion` rows; this
 * module covers what comes after it: `spotify hi` → `spotify history`,
 * `wakelock o` → `on` / `off`, `settings spo` → `settings spotify`.
 *
 * One registry (`ARG_TREES`, keyed by command kind) describes each command's
 * fixed words as a tree: an option may lead to further options (`spotify
 * history` → `heute` / `woche` / `export` …). Free text (a search term, an
 * artist name, a path) ends the tree — there is nothing to complete there.
 * Dynamic lists come from the registries that own them (retro palettes,
 * settings sections), never from a copy here.
 *
 * Pure and UI-free: App shows the first candidate as ghost text, Tab takes
 * it, further Tabs cycle (zsh-style), Shift+Tab goes back. Commands with
 * their own suggestion rows (weather cities, `cv` units, figlet fonts, otp
 * issuers) are deliberately absent so the two never compete.
 */
import type { CommandKind } from "./commands";
import { palettesFor } from "./retro";
import { SETTINGS_SECTIONS } from "./settings-sections";
import { t, type MessageKey } from "./i18n";
import { parseSpotifyArg, SPOTIFY_RANGES } from "./spotify";

export interface ArgOption {
  value: string;
  aliases?: readonly string[];
  /** Short explanation shown next to the ghost text. */
  hint: string;
  /** What may follow this word. Absent = nothing (or free text). */
  next?: ArgNode;
}

/** A free-text slot whose values come from live data (App loads them by
 *  `source`, e.g. the artists of the Spotify history). The rest of the line
 *  is ONE value there — names have spaces. */
export interface ArgSource {
  source: string;
  hint: string;
}

/** The options at one position — static, computed, or live data. */
export type ArgNode = readonly ArgOption[] | (() => readonly ArgOption[]) | ArgSource;

/** Source key: most played artists of the Spotify history. */
export const SOURCE_SPOTIFY_ARTISTS = "spotify-artists";
/** Source key: the user's GitHub repositories (local cache), most stars first. */
export const SOURCE_GITHUB_REPOS = "github-repos";

const ON_OFF: readonly ArgOption[] = [
  { value: "on", hint: "arg.on" },
  { value: "off", hint: "arg.off" },
];

const SPOTIFY_EXPORT: readonly ArgOption[] = [
  { value: "json", hint: "arg.json" },
  { value: "csv", hint: "arg.csv" },
  { value: "md", hint: "arg.md" },
];

/** After `spotify history` the filters combine in any order, so the node
 * refers to itself. */
const SPOTIFY_HISTORY: ArgNode = () => [
  { value: "heute", hint: "arg.today", next: SPOTIFY_HISTORY },
  { value: "gestern", hint: "arg.yesterday", next: SPOTIFY_HISTORY },
  { value: "woche", hint: "arg.week", next: SPOTIFY_HISTORY },
  { value: "monat", hint: "arg.month", next: SPOTIFY_HISTORY },
  { value: "today", hint: "arg.today", next: SPOTIFY_HISTORY },
  { value: "yesterday", hint: "arg.yesterday", next: SPOTIFY_HISTORY },
  { value: "week", hint: "arg.week", next: SPOTIFY_HISTORY },
  { value: "month", hint: "arg.month", next: SPOTIFY_HISTORY },
  {
    value: "artist",
    hint: "arg.artist",
    next: { source: SOURCE_SPOTIFY_ARTISTS, hint: "arg.artist_source" },
  },
  { value: "export", hint: "arg.export_file", next: SPOTIFY_EXPORT },
];

const SPOTIFY: readonly ArgOption[] = [
  { value: "now", aliases: ["jetzt"], hint: "arg.now" },
  { value: "history", aliases: ["verlauf", "h"], hint: "arg.history_days", next: SPOTIFY_HISTORY },
  { value: "play", hint: "arg.play" },
  { value: "pause", hint: "arg.pause" },
  { value: "stop", hint: "arg.pause" },
  { value: "toggle", hint: "arg.toggle" },
  { value: "next", aliases: ["weiter", "skip"], hint: "arg.next" },
  { value: "previous", aliases: ["prev", "back", "zurück"], hint: "arg.previous" },
  {
    value: "volume",
    aliases: ["vol", "lautstärke"],
    hint: "arg.volume",
    next: [
      { value: "25", hint: "arg.quiet" },
      { value: "50", hint: "arg.medium" },
      { value: "75", hint: "arg.loud" },
      { value: "100", hint: "arg.full" },
    ],
  },
  { value: "seek", aliases: ["spulen"], hint: "arg.seek" },
];

const GITHUB_EXPORT: readonly ArgOption[] = [
  { value: "json", hint: "arg.gh.json", next: [{ value: "private", hint: "arg.gh.private" }] },
  { value: "csv", hint: "arg.gh.csv", next: [{ value: "private", hint: "arg.gh.private" }] },
];

const GITHUB: readonly ArgOption[] = [
  { value: "stats", hint: "arg.gh.stats" },
  {
    value: "repos",
    hint: "arg.gh.top",
    next: [
      { value: "stars", hint: "arg.gh.most_stars" },
      { value: "forks", hint: "arg.gh.most_forks" },
      { value: "updated", hint: "arg.gh.updated" },
      { value: "name", hint: "arg.gh.name" },
      { value: "growth7d", hint: "arg.gh.growth7" },
      { value: "growth30d", hint: "arg.gh.growth30" },
    ],
  },
  { value: "repo", hint: "arg.gh.repo", next: { source: SOURCE_GITHUB_REPOS, hint: "arg.gh.repo_source" } },
  { value: "followers", hint: "arg.gh.followers" },
  { value: "stars", hint: "arg.gh.stars" },
  { value: "trending", hint: "arg.gh.trending" },
  { value: "refresh", hint: "arg.gh.refresh" },
  { value: "export", hint: "arg.gh.export", next: GITHUB_EXPORT },
  { value: "connect", hint: "arg.gh.connect" },
];

const WAKELOCK: readonly ArgOption[] = [
  ...ON_OFF,
  { value: "dark", hint: "arg.wake_dark" },
];

const GESTURES: readonly ArgOption[] = [
  ...ON_OFF,
  { value: "calibrate", hint: "arg.calibrate" },
  { value: "record", hint: "arg.record" },
];

const retro = (mode: "8bit" | "16bit"): ArgNode => () => [
  ...ON_OFF,
  { value: "focus", hint: "arg.focus" },
  { value: "lens", hint: "arg.lens" },
  ...palettesFor(mode).map((p) => ({ value: p.id, hint: p.name })),
];

const LANGUAGES: readonly ArgOption[] = [
  { value: "de", hint: "arg.lang.de" },
  { value: "en", hint: "arg.lang.en" },
  { value: "fr", hint: "arg.lang.fr" },
  { value: "es", hint: "arg.lang.es" },
  { value: "it", hint: "arg.lang.it" },
  { value: "auto", hint: "arg.lang.auto" },
];

const DELAY: readonly ArgOption[] = [
  { value: "3", hint: "arg.in3s" },
  { value: "5", hint: "arg.in5s" },
  { value: "10", hint: "arg.in10s" },
];

const SETTINGS: ArgNode = () =>
  SETTINGS_SECTIONS.map((s) => ({ value: s.id, hint: s.label }));

/** Fixed argument words per command kind. */
export const ARG_TREES: Partial<Record<CommandKind, ArgNode>> = {
  spotify: SPOTIFY,
  github: GITHUB,
  wakelock: WAKELOCK,
  nosleep: ON_OFF,
  track: [
    { value: "on", hint: "arg.track_on" },
    { value: "off", hint: "arg.track_off" },
  ],
  disco: [
    { value: "1", hint: "arg.disco_on" },
    { value: "0", hint: "arg.disco_off" },
  ],
  gestures: GESTURES,
  "8bit": retro("8bit"),
  "16bit": retro("16bit"),
  adb: [
    { value: "remote", hint: "arg.adb_remote" },
    { value: "apps", hint: "arg.adb_apps" },
    { value: "wifi", hint: "arg.adb_wifi" },
  ],
  snitch: [{ value: "map", hint: "arg.snitch_map" }],
  shazam: [{ value: "history", hint: "arg.shazam_history" }],
  resize: [
    { value: "px", hint: "arg.rz_px" },
    { value: "%", hint: "arg.rz_pct" },
  ],
  kill: [{ value: "-9", hint: "arg.kill9" }],
  transcribe: [
    { value: "--language", hint: "arg.language", next: LANGUAGES },
  ],
  settings: SETTINGS,
  // Typical values — numbers are free, these are the common ones.
  timer: [
    { value: "5min", hint: "arg.t5" },
    { value: "10min", hint: "arg.t10" },
    { value: "25min", hint: "arg.t25" },
    { value: "1h", hint: "arg.t60" },
  ],
  "shot-region": DELAY,
  "shot-full": DELAY,
  "shot-window": DELAY,
  pwgen: [
    { value: "16", hint: "arg.chars" },
    { value: "24", hint: "arg.chars" },
    { value: "32", hint: "arg.chars" },
  ],
  uuid: [
    { value: "5", hint: "arg.uuid5" },
    { value: "10", hint: "arg.uuid10" },
  ],
  random: [
    { value: "6", hint: "arg.dice" },
    { value: "100", hint: "arg.1to100" },
  ],
  iris: [
    { value: "55", hint: "arg.db55" },
    { value: "65", hint: "arg.db65" },
    { value: "75", hint: "arg.db75" },
  ],
};

function isSource(node: ArgNode | undefined): node is ArgSource {
  return !!node && typeof node === "object" && !Array.isArray(node) && "source" in node;
}

function options(node: ArgNode | undefined): readonly ArgOption[] {
  if (!node || isSource(node)) return [];
  return typeof node === "function" ? node() : node;
}

/** Case- and accent-insensitive key. */
function fold(s: string): string {
  return s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

// ── Typo tolerance ──────────────────────────────────────────────────────────

/** Optimal string alignment distance: insert, delete, replace and swapping
 *  two neighbours each cost 1 (`histroy` → `history` is ONE mistake). */
export function editDistance(a: string, b: string): number {
  const d: number[][] = Array.from({ length: a.length + 1 }, (_, i) => [i, ...Array(b.length).fill(0)]);
  for (let j = 1; j <= b.length; j++) d[0][j] = j;
  for (let i = 1; i <= a.length; i++) {
    for (let j = 1; j <= b.length; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      d[i][j] = Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + cost);
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) {
        d[i][j] = Math.min(d[i][j], d[i - 2][j - 2] + 1);
      }
    }
  }
  return d[a.length][b.length];
}

/** Below this many letters a "typo" is a guess — never corrected. */
export const TYPO_MIN_LEN = 3;

/** How far `typed` is from `word` as a typo — against the whole word
 *  (`histroy`) and against its start while still typing (`histr`). Null =
 *  too far to be a slip. */
export function typoDistance(typed: string, word: string): number | null {
  // Numbers are free values, not misspellings (`timer 7min` ≠ `5min`).
  if (typed.length < TYPO_MIN_LEN || /\d/.test(typed)) return null;
  const allowed = typed.length >= 7 ? 2 : 1;
  const d = Math.min(editDistance(typed, word), editDistance(typed, word.slice(0, typed.length)));
  return d <= allowed ? d : null;
}

// ── Recently used first ─────────────────────────────────────────────────────

/** How often each value was used, per command + position. Plain data, so
 *  App can keep it in localStorage. */
export type ArgUsage = Record<string, Record<string, { n: number; last: number }>>;

/** A use counts half after this long — recent habits beat old ones. */
export const USAGE_HALF_LIFE_MS = 14 * 86_400_000;
/** Values remembered per position (the weakest go first). */
export const USAGE_KEEP = 30;

export function usageKey(kind: CommandKind, path: readonly string[]): string {
  return `${kind}:${path.join(" ")}`;
}

export function usageScore(u: { n: number; last: number } | undefined, now: number): number {
  if (!u) return 0;
  return u.n * Math.pow(0.5, Math.max(0, now - u.last) / USAGE_HALF_LIFE_MS);
}

/** Used values first (by score), then the registry's order — a stable
 *  re-sort: nothing is ever dropped. */
function rank<T extends { value: string }>(items: T[], usage: ArgUsage | undefined, key: string, now: number): T[] {
  const table = usage?.[key];
  if (!table) return items;
  return items
    .map((it, i) => ({ it, i, s: usageScore(table[it.value], now) }))
    .sort((a, b) => b.s - a.s || a.i - b.i)
    .map((x) => x.it);
}

// ── Completion ──────────────────────────────────────────────────────────────

export interface ArgCandidate {
  /** The word that goes in (`history`). */
  value: string;
  hint: string;
  /** The whole query after taking it (`spotify history`). */
  completion: string;
  /** A typo correction: the typed word is replaced, not continued. */
  fix?: boolean;
  /** This word leads to further options — a fill appends a trailing space
   *  so the next level appears (`spotify history` → `heute`/`woche`/…). */
  hasNext?: boolean;
}

/**
 * Commands whose argument is really a FREE value (a number) — the registry
 * entries are only convenience presets, not an exhaustive choice. They keep
 * the inline ghost + Tab hint but must NOT take over the list as selectable
 * rows: Enter there would hijack a custom value (`iris 5` → `iris 55`).
 */
export const FREE_VALUE_ARG_KINDS: ReadonlySet<CommandKind> = new Set<CommandKind>([
  "timer",
  "shot-region",
  "shot-full",
  "shot-window",
  "pwgen",
  "uuid",
  "random",
  "iris",
]);

export interface ArgCompletion {
  /** What the word being typed would become, in order. */
  candidates: ArgCandidate[];
  /** The typed part of the current word (`hi`). */
  partial: string;
}

export interface CompleteContext {
  /** Live lists for `ArgSource` slots, by source key (App loads them). */
  sources?: Partial<Record<string, readonly ArgOption[]>>;
  usage?: ArgUsage;
  now?: number;
}

interface Walk {
  /** The node the word being typed belongs to. */
  node: ArgNode;
  /** Options taken so far (for usage keys). */
  path: string[];
  /** Raw line up to the word being typed (ends with a space). */
  head: string;
  /** The word being typed — for a source slot, the whole rest of the line. */
  partial: string;
}

function tokens(arg: string): { text: string; at: number }[] {
  return [...arg.matchAll(/\S+/g)].map((m) => ({ text: m[0], at: m.index ?? 0 }));
}

function splitLine(line: string): { keyword: string; arg: string } | null {
  const m = /^\s*(\S+)(?:\s+([\s\S]*))?$/.exec(line);
  return m ? { keyword: m[1], arg: m[2] ?? "" } : null;
}

/** A complete Spotify control runs on Enter even when its numeric value
 * also prefixes a preset. Presets remain available through Tab. */
export function showArgSuggestions(kind: CommandKind, line: string): boolean {
  return !FREE_VALUE_ARG_KINDS.has(kind) &&
    !(kind === "spotify" && parseSpotifyArg(splitLine(line)?.arg ?? "").kind === "control");
}

function spellings(option: ArgOption): readonly string[] {
  return [option.value, ...(option.aliases ?? [])];
}

/** Follow the typed words through the tree to the position being typed. */
function walk(kind: CommandKind, line: string): Walk | null {
  let node: ArgNode | undefined = ARG_TREES[kind];
  const parts = splitLine(line);
  if (!node || !parts) return null;
  const { keyword, arg } = parts;
  const toks = tokens(arg);
  const typingNew = arg === "" || /\s$/.test(arg);
  const path: string[] = [];
  for (let i = 0; ; i++) {
    if (isSource(node)) {
      // The rest of the line is one value (artist names have spaces).
      const at = i < toks.length ? toks[i].at : arg.length;
      return { node, path, head: `${keyword} ${arg.slice(0, at)}`, partial: arg.slice(at) };
    }
    const left = toks.length - i;
    if (left === 0 || (left === 1 && !typingNew)) {
      const at = left === 0 ? arg.length : toks[i].at;
      return { node, path, head: `${keyword} ${arg.slice(0, at)}`, partial: left === 0 ? "" : toks[i].text };
    }
    const w = fold(toks[i].text);
    const hit: ArgOption | undefined = options(node).find((o) => spellings(o).some((s) => fold(s) === w));
    if (!hit?.next) return null; // free text or the end of the tree
    path.push(hit.value);
    node = hit.next;
  }
}

/** The source a line's current position needs but `ctx` doesn't have yet
 *  (App loads it, then completes again). */
export function missingSource(kind: CommandKind, line: string, ctx: CompleteContext = {}): string | null {
  const w = walk(kind, line);
  if (!w || !isSource(w.node)) return null;
  return ctx.sources?.[w.node.source] ? null : w.node.source;
}

/**
 * Candidates for `line`, the raw search text of a recognised command
 * (`spotify hi`). Works on the RAW text: the trailing space is what says
 * "next word" (the command parser trims it away). The keyword keeps the
 * spelling the user typed (aliases stay aliases). Order: what you used most
 * recently first, then the registry's order. No prefix match → typo
 * corrections (`histroy` → `history`). Null when nothing can be completed.
 */
export function completeArg(kind: CommandKind, line: string, ctx: CompleteContext = {}): ArgCompletion | null {
  const w = walk(kind, line);
  if (!w) return null;
  const opts = isSource(w.node) ? (ctx.sources?.[w.node.source] ?? []) : options(w.node);
  const prefix = fold(w.partial);
  let matches = opts.filter((o) => spellings(o).some((s) => fold(s).startsWith(prefix)));
  let fix = false;
  if (matches.length === 0) {
    const typed = prefix.trim();
    matches = opts
      .map((o) => ({ o, d: typoDistance(typed, fold(o.value)) }))
      .filter((x): x is { o: ArgOption; d: number } => x.d !== null)
      .sort((a, b) => a.d - b.d)
      .map((x) => x.o);
    fix = true;
  }
  if (matches.length === 0) return null;
  // A word typed out in full with nothing longer to offer: done.
  if (!fix && matches.length === 1 && spellings(matches[0]).some((s) => fold(s) === prefix)) return null;
  const ranked = fix ? matches : rank(matches, ctx.usage, usageKey(kind, w.path), ctx.now ?? 0);
  return {
    partial: w.partial,
    candidates: ranked.map((o) => {
      const range = kind === "spotify" && !isSource(w.node) && !!SPOTIFY_RANGES[o.value];
      // Time intervals are alternatives. Keep other filters, replace any
      // earlier interval, and leave a completed range ready for Enter.
      const head = range
        ? w.head.split(/\s+/).filter((word) => word && !SPOTIFY_RANGES[word.toLowerCase()]).join(" ") + " "
        : w.head;
      return {
        value: o.value,
        hint: t(o.hint as MessageKey),
        completion: `${head}${o.value}`,
        hasNext: !range && !!o.next,
        ...(fix ? { fix: true } : {}),
      };
    }),
  };
}

/**
 * Remember which options a run command used (`spotify history woche` →
 * `history` at the top level, `woche` after `history`). Only real options
 * count — free text is not a habit. Returns a new object.
 */
export function recordUse(usage: ArgUsage, kind: CommandKind, line: string, ctx: CompleteContext, now: number): ArgUsage {
  let node: ArgNode | undefined = ARG_TREES[kind];
  const parts = splitLine(line);
  if (!node || !parts) return usage;
  const next: ArgUsage = { ...usage };
  const bump = (path: string[], value: string) => {
    const key = usageKey(kind, path);
    const table = { ...(next[key] ?? {}) };
    const cur = table[value];
    table[value] = { n: (cur?.n ?? 0) + 1, last: now };
    const keep = Object.entries(table)
      .sort((a, b) => usageScore(b[1], now) - usageScore(a[1], now))
      .slice(0, USAGE_KEEP);
    next[key] = Object.fromEntries(keep);
  };
  const toks = tokens(parts.arg);
  const path: string[] = [];
  for (let i = 0; i < toks.length && node; i++) {
    if (isSource(node)) {
      const rest = fold(parts.arg.slice(toks[i].at).trim());
      const hit = ctx.sources?.[node.source]?.find((o) => fold(o.value) === rest);
      if (hit) bump(path, hit.value);
      break;
    }
    const w = fold(toks[i].text);
    const hit: ArgOption | undefined = options(node).find((o) => spellings(o).some((s) => fold(s) === w));
    if (!hit) break;
    bump(path, hit.value);
    path.push(hit.value);
    node = hit.next;
  }
  return next;
}

/**
 * The grey text to show after the caret for `query`: the rest of the
 * candidate's word. Case doesn't matter (`artist kr` + `aftwerk` — Tab then
 * writes `Kraftwerk`). A typo correction never gets one by construction: it
 * only exists when no candidate starts with what was typed.
 */
export function ghostSuffix(query: string, candidate: ArgCandidate): string {
  const c = candidate.completion;
  return c.length > query.length && c.toLowerCase().startsWith(query.toLowerCase())
    ? c.slice(query.length)
    : "";
}

export interface ArgCycle {
  cands: ArgCandidate[];
  /** The candidate currently on screen. */
  index: number;
}

/** A completed interval remains switchable with Tab, whether it was typed,
 * clicked, or accepted with Enter. Rebuild all alternatives, not just the
 * prefix matches that originally completed this word. */
export function spotifyRangeCycle(line: string): ArgCycle | null {
  const value = line.trimEnd().split(/\s+/).pop()?.toLowerCase() ?? "";
  if (!SPOTIFY_RANGES[value] || /\s$/.test(line)) return null;
  const cands = completeArg("spotify", line + " ")?.candidates.filter((c) => !!SPOTIFY_RANGES[c.value]) ?? [];
  const index = cands.findIndex((c) => c.value === value);
  return index < 0 ? null : { cands, index };
}

/** True once the line has a space after the keyword — argument territory. */
export function inArgTerritory(line: string): boolean {
  return /^\s*\S+\s/.test(line);
}

/**
 * The candidates shown as selectable rows in the list (v0.218.0 — the list
 * is the only suggestion surface). Empty = no rows, the normal list stays.
 * - Only in argument territory: a bare keyword keeps its own row.
 * - A completed Spotify interval lists EVERY interval, the current one first,
 *   so switching it is ↓ + Enter.
 * - Free-value commands and a ready Spotify control hide their rows (Enter
 *   must not swap a typed number for a preset) unless Tab opened the menu.
 */
export function suggestionRows(
  kind: CommandKind,
  line: string,
  completion: ArgCompletion | null,
  menuOpen: boolean,
): { cands: ArgCandidate[]; range: boolean } {
  if (!inArgTerritory(line)) return { cands: [], range: false };
  const range = kind === "spotify" ? spotifyRangeCycle(line) : null;
  if (range) {
    return { cands: [range.cands[range.index], ...range.cands.filter((_, i) => i !== range.index)], range: true };
  }
  if (!completion || (!menuOpen && !showArgSuggestions(kind, line))) return { cands: [], range: false };
  return { cands: completion.candidates, range: false };
}

/**
 * What Tab does while no rows are visible: put the line into argument
 * territory and say whether the rows need the explicit menu (free values /
 * a ready control — they would stay hidden otherwise). Null = no candidates.
 */
export function tabOpens(
  kind: CommandKind,
  line: string,
  completion: ArgCompletion | null,
): { line: string; menu: boolean } | null {
  if (!completion || completion.candidates.length === 0) return null;
  const next = inArgTerritory(line) ? line : line.trimEnd() + " ";
  return { line: next, menu: !showArgSuggestions(kind, next) };
}
