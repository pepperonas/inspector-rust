import type { MemeEntry } from "./meme";

/** One `clown` style row: the style plus this text rendered in it. */
export interface ClownEntryView {
  key: string;
  name: string;
  hint: string;
  output: string;
}
import type { CommandKind } from "./commands";
import type { SocialTarget } from "./social";

export type ContentType = "text" | "rtf" | "html" | "image" | "files";

export interface ClipEntry {
  id: number;
  content_type: ContentType;
  content_text: string;
  /** For text/rtf/html: raw payload. image: base64 PNG. files: JSON array. */
  content_data: string;
  hash: string;
  byte_size: number;
  created_at: number;
  last_used_at: number;
  /** Pinned entries float to the top and are never pruned (v0.76.0). */
  pinned: boolean;
  /** Optional user note; noted entries are highlighted yellow + kept (v0.84.106). */
  note: string | null;
  /** Lineage (v0.93.1): the entry this clip was derived from — set when the
   *  user copied an existing clip in another shape (plain text, upper-case,
   *  base64, …). `null` for organically captured clips. */
  derived_from: number | null;
  /** Which manipulation produced this clip (`plain`, `upper`, `base64`, …). */
  derived_kind: string | null;
}

export interface Snippet {
  id: number;
  abbreviation: string;
  title: string;
  body: string;
  created_at: number;
  updated_at: number;
  /** Group name this snippet belongs to, or null when ungrouped. */
  category?: string | null;
  /** Content-revision counter (cue-compatible). Bumps only on a content change
   *  (abbreviation/title/body), never on group edits. Missing → 1. */
  version?: number;
}

export interface CalcEntry {
  /** The raw expression typed by the user (trimmed, may include `=` prefix). */
  expression: string;
  /** Numeric result. */
  value: number;
  /** Display-formatted result that gets pasted on activation. */
  display: string;
}

/** A hex-color preview row, surfaced when the search query parses as a
 *  hex color. Activating pastes `pasteValue` (canonical `#RRGGBB`). */
export interface ColorEntryView {
  hex: string;
  pasteValue: string;
  r: number;
  g: number;
  b: number;
  a: number;
  hsl: { h: number; s: number; l: number };
  rgbString: string;
  hslString: string;
}

export interface Note {
  id: number;
  content_type: ContentType;
  /** Plain-text preview (always populated for search). */
  content_text: string;
  /** Raw payload — same convention as ClipEntry.content_data. */
  content_data: string;
  title: string;
  category: string;
  byte_size: number;
  created_at: number;
  updated_at: number;
}

export interface BackupImportResult {
  history_imported: number;
  snippets_imported: number;
  notes_imported: number;
  totp_imported: number;
  settings_imported: number;
  /** Timesheet events imported (v3 backups only; 0 otherwise). */
  timesheet_imported: number;
  errors: string[];
}

/** A runnable power-command parsed out of the search bar. */
export interface CommandEntryView {
  /** Stable kind for activate-dispatch (sourced from `commands.ts` so new
   *  command kinds never need a parallel edit here). */
  commandKind: CommandKind;
  /** What the user typed (e.g. "tren hello world"). */
  rawInput: string;
  /** The argument portion ("" for `optim`). */
  arg: string;
  /** Label shown in the list (e.g. "Translate 'hello' → DE"). */
  label: string;
  /** Sub-label, e.g. "Google Translate · opens in browser". */
  hint: string;
}

/** A *partial* command match — surfaces as autocomplete in the list. */
export interface CommandSuggestionView {
  /** Same shape as CommandSpec.keyword + syntax for the row label. */
  keyword: string;
  syntax: string;
  description: string;
  /** Hint string the user should type to complete the command (without
   *  the leading argument). Activating the suggestion populates the
   *  search bar with this prefix + a trailing space. */
  completion: string;
}

/** One process in the `kill` live picker — surfaces as its own
 *  ListEntry kind so the row gets its own icon + activation. */
export interface KillTargetView {
  pid: number;
  name: string;
  memory_mb: number;
  exe: string;
  /** Carry the `-9` flag through to activate so the user doesn't have
   *  to re-type it after selecting a process. */
  force: boolean;
}

/** Random German pickup-line surfaced by the hidden `opener` trigger. */
export interface OpenerEntryView {
  /** The German opener text — pre-rolled by `pickOpener(query)`. */
  text: string;
  /** Last ←/→ switch direction (`1` = next/right, `-1` = prev/left), used to
   *  pick the directional slide-in animation in the preview. Absent on the
   *  initial seeded pick (renders as a neutral slide-from-right). */
  dir?: 1 | -1;
}

/** Installed-app match surfaced as a top entry when the user's query
 *  fuzzy-matches an app's name. v0.37.0+. Icon is fetched lazily by
 *  the row (only the selected row triggers the IPC). */
export interface AppEntryView {
  name: string;
  path: string;
  /** Fuse.js match score (0 = perfect, 1 = worst). Used for ranking
   *  the top-1 app against other heuristics — not currently shown. */
  score?: number;
}

/** Bruno (Brutto→Netto) result surfaced as a top entry once the
 *  user has typed `bruno <€>`. The numbers come from the pure-TS
 *  computation in `lib/bruno.ts`. */
/** One `?`-index row: a documented command, navigable with ↑/↓ — the right
 *  preview renders the full CommandDoc of the selection live. */
export interface HelpEntryView {
  command: string;
  tagline: string;
  category: string;
}

export interface BrunoEntryView {
  yearlyGross: number;
  /** What the user typed: monthly vs yearly. Drives the row's
   *  primary label (so `bruno 5000m` shows `5.000 €/Monat`). */
  period: "monthly" | "yearly";
  netYear: number;
  netMonth: number;
  totalDeductions: number;
  deductionRate: number;
  marginalRate: number;
  // Components for the preview-pane split.
  social: { health: number; care: number; pension: number; unemployment: number };
  incomeTax: number;
  soli: number;
  churchTax: number;
  // The defaults used (so the preview can show "Klasse I · NRW · …").
  taxClass: number;
  state: string;
  children: number;
  isChurchMember: boolean;
  /** Set when the `f` suffix selected the SELF-EMPLOYED calculation —
   *  carries the full freelancer breakdown; the employee-shaped fields
   *  above then mirror its headline numbers (net, rates) so row
   *  rendering + Enter-paste work unchanged. */
  self?: import("./bruno").BrunoSelfBreakdownView;
}

/** One file from the current Finder selection (Ctrl+Shift+F path). */
export interface FinderFileView {
  path: string;
  name: string;
  size_bytes: number | null;
  /** Cheap extension test — drives whether the Resize action shows. */
  is_image: boolean;
}

export type ListEntry =
  | { kind: "clip"; data: ClipEntry }
  | { kind: "snippet"; data: Snippet }
  | { kind: "calc"; data: CalcEntry }
  | { kind: "color"; data: ColorEntryView }
  | { kind: "command"; data: CommandEntryView }
  | { kind: "command-suggestion"; data: CommandSuggestionView }
  | { kind: "kill-target"; data: KillTargetView }
  | { kind: "opener"; data: OpenerEntryView }
  | { kind: "finder-file"; data: FinderFileView }
  | { kind: "bruno"; data: BrunoEntryView }
  | { kind: "app"; data: AppEntryView }
  | { kind: "pwgen"; data: PwgenEntryView }
  | { kind: "bpm"; data: BpmTriggerView }
  | { kind: "xhype"; data: { label: string; mode: "features" | "news" } }
  | { kind: "equalizer"; data: EqualizerTriggerView }
  | { kind: "help"; data: HelpEntryView } // `?` index row (command + tagline)
  | { kind: "settings-section"; data: SettingsSectionView } // settings-search jump row
  // mode "add" (v0.104.0, `2fa add [issuer]`) opens the overlay straight on
  // the Add form, `issuer` pre-filled; absent/"list" is the classic manager.
  | { kind: "totp-manage"; data: { label: string; mode?: "list" | "add"; issuer?: string } }
  | { kind: "totp"; data: TotpListView }
  | { kind: "meme"; data: MemeEntry }
  | { kind: "clown"; data: ClownEntryView }
  | { kind: "figlet-font"; data: FigletFontView }
  | { kind: "social"; data: SocialTarget };

/** One font row in the `figlet` gallery: the font's metadata + a compact
 *  live sample of the user's text in that font (empty until fetched). The big
 *  monospace banner renders in the preview when this row is selected; Enter
 *  copies that full banner. */
export interface FigletFontView {
  name: string;
  category: string;
  popular: boolean;
  pinned: boolean;
  /** Compact multi-line sample of the current text (may be "" until loaded). */
  sample: string;
}

/** Single TOTP autocomplete row — shows issuer + account + live
 *  6-digit code with countdown. Activate (Enter) → code is copied to
 *  clipboard + popup hides. */
export interface TotpListView {
  id: number;
  issuer: string;
  account: string;
  digits: number;
  period: number;
  /** Currently-displayed code. Refreshed by the App.tsx polling tick
   *  while a `totp` row is in `combined`. */
  code: string;
  /** Seconds until the code rolls over (0..period). Drives the
   *  countdown ring in the row. */
  seconds_remaining: number;
}

/** Row surfaced when the user types `bpm` exactly. Enter activates
 *  the live BPM detector overlay (`<BpmDetector />`). The view itself
 *  carries no data — it's just a marker for the activate handler. */
export interface BpmTriggerView {
  /** Static label rendered in the row, kept here for the renderer
   *  to read so it doesn't have to special-case the kind. */
  label: string;
}

/** Row surfaced when the user types `equalizer` (or `eq`) exactly. Enter
 *  activates the live spectrum equalizer overlay (`<EqualizerVisualizer />`).
 *  A marker for the activate handler, like `BpmTriggerView`. */
export interface EqualizerTriggerView {
  label: string;
}

/** Generated-password row surfaced when the user types `pwgen N`.
 *  v0.40.0+. `password` is regenerated each render based on the
 *  active `mode` (which is component state in App.tsx) so the user
 *  can mash Enter for a fresh random or click mode buttons in the
 *  preview pane to switch modes. */
export interface PwgenEntryView {
  length: number;
  mode: "all" | "alnum" | "dict" | "leet";
  password: string;
}

/**
 * The kinds rendered with the reddish "custom command" treatment. Covers
 * EVERY row reached by typing a command keyword — uniformly, including with
 * a parameter (`kill slack`, `meme cat`, …): the generic `command` + its
 * suggestions, the dedicated keyword-command rows (2fa, otp, pwgen, bruno,
 * bpm, …), the whole-list command pickers (kill-target, meme, figlet,
 * clown) and calculator results (typing an expression should feel as
 * "active" as a keyword, v0.84.27). Expression `color` rows and
 * non-command rows (app, finder, clip, snippet, opener, help-index rows'
 * neutral cousins) keep the neutral accent.
 *
 * ONE registry (the never-hard-code-a-list invariant): `HistoryItem`
 * derives its accent from it AND `HistoryList` colours the sliding
 * selection indicator from it — restating the set in either place would
 * let the row accent and the indicator disagree on what a command is.
 */
export const CUSTOM_COMMAND_KINDS: ReadonlySet<ListEntry["kind"]> = new Set([
  "command",
  "command-suggestion",
  "help",
  "totp-manage",
  "bruno",
  "pwgen",
  "bpm",
  "equalizer",
  "totp",
  "kill-target",
  "meme",
  "clown",
  "xhype",
  "figlet-font",
  "social",
  "calc",
] satisfies ListEntry["kind"][]);

export function isCustomCommandEntry(entry: ListEntry): boolean {
  return CUSTOM_COMMAND_KINDS.has(entry.kind);
}

/** Settings-search suggestion row (v0.164.0) — Enter jumps to the Settings
 *  section (`settingsJump` + highlight), like the macOS/Android settings
 *  search. Navigation, not a keyword command: deliberately NOT in
 *  `CUSTOM_COMMAND_KINDS` (neutral accent, like an app-launcher hit — the
 *  sliding selection indicator stays accent-coloured over it). */
export interface SettingsSectionView {
  id: string;
  label: string;
}
