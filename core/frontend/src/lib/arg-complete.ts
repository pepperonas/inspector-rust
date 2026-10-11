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

/** The options at one position — static or computed when needed. */
export type ArgNode = readonly ArgOption[] | (() => readonly ArgOption[]);

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
  { value: "artist", hint: "nur ein Künstler — Name folgt" },
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
  { value: "volume", hint: "Lautstärke 0–100" },
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
};

function options(node: ArgNode | undefined): readonly ArgOption[] {
  if (!node) return [];
  return typeof node === "function" ? node() : node;
}

/** Case- and accent-insensitive key. */
function fold(s: string): string {
  return s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

export interface ArgCandidate {
  /** The word that goes in (`history`). */
  value: string;
  hint: string;
  /** The whole query after taking it (`spotify history`). */
  completion: string;
}

export interface ArgCompletion {
  /** What the word being typed would become, in order. */
  candidates: ArgCandidate[];
  /** The typed part of the current word (`hi`). */
  partial: string;
}

/**
 * Candidates for `line`, the raw search text of a recognised command
 * (`spotify hi`). Works on the RAW text: the trailing space is what says
 * "next word" (the command parser trims it away). The keyword keeps the
 * spelling the user typed (aliases stay aliases). Null when nothing can be
 * completed here.
 */
export function completeArg(kind: CommandKind, line: string): ArgCompletion | null {
  let node: ArgNode | undefined = ARG_TREES[kind];
  if (!node) return null;
  const m = /^\s*(\S+)(?:\s+([\s\S]*))?$/.exec(line);
  if (!m) return null;
  const keyword = m[1];
  const arg = m[2] ?? (/\s$/.test(line) ? " " : "");
  // Words before the one being typed must each be a known option.
  const words = arg.split(/\s+/).filter(Boolean);
  const typingNew = arg === "" || /\s$/.test(arg);
  const done = typingNew ? words : words.slice(0, -1);
  const partial = typingNew ? "" : (words[words.length - 1] ?? "");
  for (const w of done) {
    const hit: ArgOption | undefined = options(node).find((o) => fold(o.value) === fold(w));
    if (!hit?.next) return null; // free text or the end of the tree
    node = hit.next;
  }
  const prefix = fold(partial);
  const matches = options(node).filter((o) => fold(o.value).startsWith(prefix));
  if (matches.length === 0) return null;
  // A word typed out in full with nothing longer to offer: done.
  if (matches.length === 1 && fold(matches[0].value) === prefix) return null;
  const head = [keyword, ...done].join(" ");
  return {
    partial,
    candidates: matches.map((o) => ({
      value: o.value,
      hint: o.hint,
      completion: `${head} ${o.value}`,
    })),
  };
}

/**
 * The grey text to show after the caret for `query`: the rest of the
 * candidate's word, but only while the typed part matches it letter for
 * letter including case — otherwise the ghost would contradict what is on
 * screen (`spotify HI` + `story`).
 */
export function ghostSuffix(query: string, candidate: ArgCandidate): string {
  return candidate.completion.startsWith(query) ? candidate.completion.slice(query.length) : "";
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
