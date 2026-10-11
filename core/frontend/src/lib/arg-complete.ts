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

export interface ArgOption {
  value: string;
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

const ON_OFF: readonly ArgOption[] = [
  { value: "on", hint: "einschalten" },
  { value: "off", hint: "ausschalten" },
];

const SPOTIFY_EXPORT: readonly ArgOption[] = [
  { value: "json", hint: "strukturierte Daten" },
  { value: "csv", hint: "für Tabellen" },
  { value: "md", hint: "lesbares Protokoll" },
];

/** After `spotify history` the filters combine in any order, so the node
 * refers to itself. */
const SPOTIFY_HISTORY: ArgNode = () => [
  { value: "heute", hint: "nur heute", next: SPOTIFY_HISTORY },
  { value: "gestern", hint: "nur gestern", next: SPOTIFY_HISTORY },
  { value: "woche", hint: "letzte 7 Tage", next: SPOTIFY_HISTORY },
  { value: "monat", hint: "letzte 30 Tage", next: SPOTIFY_HISTORY },
  {
    value: "artist",
    hint: "nur ein Künstler",
    next: { source: SOURCE_SPOTIFY_ARTISTS, hint: "Künstler aus deinem Verlauf" },
  },
  { value: "export", hint: "als Datei nach ~/Downloads", next: SPOTIFY_EXPORT },
];

const SPOTIFY: readonly ArgOption[] = [
  { value: "now", hint: "läuft gerade" },
  { value: "history", hint: "Verlauf nach Tagen", next: SPOTIFY_HISTORY },
  { value: "play", hint: "abspielen" },
  { value: "pause", hint: "pausieren" },
  { value: "toggle", hint: "Wiedergabe umschalten" },
  { value: "next", hint: "nächster Titel" },
  { value: "previous", hint: "vorheriger Titel" },
  {
    value: "volume",
    hint: "Lautstärke 0–100",
    next: [
      { value: "25", hint: "leise" },
      { value: "50", hint: "mittel" },
      { value: "75", hint: "laut" },
      { value: "100", hint: "voll" },
    ],
  },
  { value: "seek", hint: "zu Sekunde springen" },
];

const WAKELOCK: readonly ArgOption[] = [
  ...ON_OFF,
  { value: "dark", hint: "wach, Bildschirm darf aus" },
];

const GESTURES: readonly ArgOption[] = [
  ...ON_OFF,
  { value: "calibrate", hint: "Erkennung kalibrieren" },
  { value: "record", hint: "Gesten aufzeichnen" },
];

const retro = (mode: "8bit" | "16bit"): ArgNode => () => [
  ...ON_OFF,
  { value: "focus", hint: "aktives Fenster aussparen" },
  { value: "lens", hint: "Lupe um den Zeiger" },
  ...palettesFor(mode).map((p) => ({ value: p.id, hint: `Palette ${p.name}` })),
];

const LANGUAGES: readonly ArgOption[] = [
  { value: "de", hint: "Deutsch" },
  { value: "en", hint: "Englisch" },
  { value: "fr", hint: "Französisch" },
  { value: "es", hint: "Spanisch" },
  { value: "it", hint: "Italienisch" },
  { value: "auto", hint: "automatisch erkennen" },
];

const DELAY: readonly ArgOption[] = [
  { value: "3", hint: "in 3 Sekunden" },
  { value: "5", hint: "in 5 Sekunden" },
  { value: "10", hint: "in 10 Sekunden" },
];

const SETTINGS: ArgNode = () =>
  SETTINGS_SECTIONS.map((s) => ({ value: s.id, hint: s.label }));

/** Fixed argument words per command kind. */
export const ARG_TREES: Partial<Record<CommandKind, ArgNode>> = {
  spotify: SPOTIFY,
  wakelock: WAKELOCK,
  nosleep: ON_OFF,
  track: [
    { value: "on", hint: "Zeiterfassung starten" },
    { value: "off", hint: "Zeiterfassung stoppen" },
  ],
  disco: [
    { value: "1", hint: "Beat-Sync an" },
    { value: "0", hint: "Beat-Sync aus" },
  ],
  gestures: GESTURES,
  "8bit": retro("8bit"),
  "16bit": retro("16bit"),
  adb: [
    { value: "remote", hint: "Bildschirm steuern" },
    { value: "apps", hint: "Apps verwalten" },
    { value: "wifi", hint: "per WLAN verbinden" },
  ],
  snitch: [{ value: "map", hint: "Verbindungen auf der Weltkarte" }],
  shazam: [{ value: "history", hint: "erkannte Songs" }],
  resize: [
    { value: "px", hint: "Größe in Pixeln" },
    { value: "%", hint: "Größe in Prozent" },
  ],
  kill: [{ value: "-9", hint: "sofort beenden (SIGKILL)" }],
  transcribe: [
    { value: "--language", hint: "Sprache vorgeben", next: LANGUAGES },
  ],
  settings: SETTINGS,
  // Typical values — numbers are free, these are the common ones.
  timer: [
    { value: "5min", hint: "kurze Pause" },
    { value: "10min", hint: "Tee ziehen" },
    { value: "25min", hint: "Pomodoro" },
    { value: "1h", hint: "eine Stunde" },
  ],
  "shot-region": DELAY,
  "shot-full": DELAY,
  "shot-window": DELAY,
  pwgen: [
    { value: "16", hint: "Zeichen" },
    { value: "24", hint: "Zeichen" },
    { value: "32", hint: "Zeichen" },
  ],
  uuid: [
    { value: "5", hint: "fünf UUIDs" },
    { value: "10", hint: "zehn UUIDs" },
  ],
  random: [
    { value: "6", hint: "Würfel" },
    { value: "100", hint: "1 bis 100" },
  ],
  iris: [
    { value: "55", hint: "dB-Schwelle (Standard)" },
    { value: "65", hint: "dB-Schwelle (laut)" },
    { value: "75", hint: "dB-Schwelle (sehr laut)" },
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
}

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
    const hit: ArgOption | undefined = options(node).find((o) => fold(o.value) === w);
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
  let matches = opts.filter((o) => fold(o.value).startsWith(prefix));
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
  if (!fix && matches.length === 1 && fold(matches[0].value) === prefix) return null;
  const ranked = fix ? matches : rank(matches, ctx.usage, usageKey(kind, w.path), ctx.now ?? 0);
  return {
    partial: w.partial,
    candidates: ranked.map((o) => ({
      value: o.value,
      hint: o.hint,
      completion: `${w.head}${o.value}`,
      ...(fix ? { fix: true } : {}),
    })),
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
    const hit: ArgOption | undefined = options(node).find((o) => fold(o.value) === w);
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

/**
 * What one Tab does. A live cycle (the line still shows the word the last
 * Tab put there) moves to the next / previous candidate; otherwise the first
 * candidate is taken (Shift+Tab: the last). Null = nothing to do.
 */
export function tabStep(
  cycle: ArgCycle | null,
  completion: ArgCompletion | null,
  back: boolean,
): ArgCycle | null {
  if (cycle && cycle.cands.length > 0) {
    return { cands: cycle.cands, index: cycleIndex(cycle.index, cycle.cands.length, back) };
  }
  if (completion && completion.candidates.length > 0) {
    return { cands: completion.candidates, index: back ? completion.candidates.length - 1 : 0 };
  }
  return null;
}

/** zsh-style cycling: the next index, wrapping, in either direction. */
export function cycleIndex(index: number, total: number, back: boolean): number {
  if (total <= 0) return 0;
  return (index + (back ? -1 : 1) + total) % total;
}
