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
}

export interface Snippet {
  id: number;
  abbreviation: string;
  title: string;
  body: string;
  created_at: number;
  updated_at: number;
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
  errors: string[];
}

/** A runnable power-command parsed out of the search bar. */
export interface CommandEntryView {
  /** Stable kind for activate-dispatch. */
  commandKind:
    | "translate-en"
    | "translate-de"
    | "translate-auto"
    | "resize"
    | "optim"
    | "rmvvls"
    | "reboot"
    | "shutdown"
    | "lock";
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

export type ListEntry =
  | { kind: "clip"; data: ClipEntry }
  | { kind: "snippet"; data: Snippet }
  | { kind: "calc"; data: CalcEntry }
  | { kind: "color"; data: ColorEntryView }
  | { kind: "command"; data: CommandEntryView }
  | { kind: "command-suggestion"; data: CommandSuggestionView }
  | { kind: "kill-target"; data: KillTargetView };
