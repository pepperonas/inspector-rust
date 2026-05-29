import { invoke } from "@tauri-apps/api/core";
import type { BackupImportResult, ClipEntry, Note, Snippet } from "./types";

// ── Clipboard history ────────────────────────────────────────────────────────

export function getHistory(limit = 500, offset = 0): Promise<ClipEntry[]> {
  return invoke("get_history", { limit, offset });
}

export function searchHistory(query: string, limit = 500): Promise<ClipEntry[]> {
  return invoke("search_history", { query, limit });
}

/** Paste a clipboard entry. Honours the `paste.plain_text_only` setting:
 *  HTML / RTF entries are downgraded to their plain-text preview when
 *  the toggle is on. Image / Files entries paste as-is. */
export function pasteEntry(id: number): Promise<void> {
  return invoke("paste_entry", { id });
}

/** Paste a clipboard entry preserving its original content type. Bypasses
 *  the plain-text setting — used by Shift+Enter as a one-shot override. */
export function pasteEntryFormatted(id: number): Promise<void> {
  return invoke("paste_entry_formatted", { id });
}

export function getPastePlainTextOnly(): Promise<boolean> {
  return invoke("get_paste_plain_text_only");
}

export function setPastePlainTextOnly(value: boolean): Promise<void> {
  return invoke("set_paste_plain_text_only", { value });
}

/** Read the persisted `ocr.save_source_image` flag. When `false`
 *  (the default since v0.26.3), the OCR pipeline persists only the
 *  recognised text to history; when `true`, the source PNG is also
 *  upserted so the user can re-OCR it later. */
export function getOcrSaveSourceImage(): Promise<boolean> {
  return invoke("get_ocr_save_source_image");
}

export function setOcrSaveSourceImage(value: boolean): Promise<void> {
  return invoke("set_ocr_save_source_image", { value });
}

// ── Screenshot preview window (CleanShot-X-style) ──────────────────────────

/** Path of the currently-pending captured PNG, or null if none. The
 *  preview React component calls this on mount to know which file to
 *  display in its thumbnail. */
export function getPendingScreenshotPath(): Promise<string | null> {
  return invoke("get_pending_screenshot_path");
}

/** Richer variant — includes the frontmost-app name captured at
 *  shot time + the current pin state. Used by the preview HUD to
 *  show the source-app chip and reflect the pinned visual state. */
export interface PendingScreenshotInfo {
  path: string;
  app_name: string | null;
  pinned: boolean;
}
export function getPendingScreenshotInfo(): Promise<PendingScreenshotInfo | null> {
  return invoke("get_pending_screenshot_info");
}

/** Set the pin state. While pinned, a subsequent screenshot does NOT
 *  replace the on-screen preview (the new PNG still goes to clipboard
 *  + history). Returns the resulting state. */
export function setScreenshotPinned(pinned: boolean): Promise<boolean> {
  return invoke("set_screenshot_pinned", { pinned });
}

/** Save: promote the temp PNG to ~/Downloads (with the captured app
 *  name baked into the filename), push to clipboard, push to history,
 *  close the preview window. */
export function screenshotPreviewSave(): Promise<void> {
  return invoke("screenshot_preview_save");
}

/** Copy: re-write the PNG to the clipboard. Preview stays open
 *  (unlike Save). Useful when the user has copied something else in
 *  the meantime and wants the screenshot back on the clipboard. */
export function screenshotPreviewCopy(): Promise<void> {
  return invoke("screenshot_preview_copy");
}

/** Discard: delete the temp PNG, close the preview window. No
 *  side effects on clipboard / Downloads / history. */
export function screenshotPreviewDiscard(): Promise<void> {
  return invoke("screenshot_preview_discard");
}

/** Edit: open the annotation editor window (arrows / text / rect /
 *  highlight / blur). The preview hides itself; the editor's Save
 *  bakes the annotated PNG to ~/Downloads + clipboard + history and
 *  re-shows the preview with the edited image. */
export function screenshotPreviewEdit(): Promise<void> {
  return invoke("screenshot_preview_edit");
}

// ── Screenshot editor ──────────────────────────────────────────────────────

/** Save the annotated PNG (base64 from canvas.toDataURL). Backend
 *  writes to ~/Downloads with `<App>-<ts>-edited.png`, pushes to
 *  clipboard + history, closes the editor, re-shows the preview. */
export function editorSave(pngB64: string): Promise<string> {
  return invoke("editor_save", { pngB64 });
}

/** Cancel: close the editor, re-show the preview with the original
 *  (unedited) capture. */
export function editorCancel(): Promise<void> {
  return invoke("editor_cancel");
}

/** Cursor-follow: if the cursor has crossed to a different monitor,
 *  re-position the preview window to the new monitor's bottom-left.
 *  Called from the preview React component every 200 ms while the
 *  window is open. */
export function repositionPreviewToCursor(): Promise<void> {
  return invoke("reposition_preview_to_cursor");
}

// ── Input lock (macOS-lock-style chord-to-unlock) ──────────────────────────

/** Read the persisted unlock chord. Defaults to `["i", "r"]` on a
 *  fresh install or a malformed stored value. */
export function getInputLockChord(): Promise<string[]> {
  return invoke("get_input_lock_chord");
}

/** Persist a new unlock chord. Backend rejects empty / all-unparseable
 *  chords so the user can't lock themselves out via Settings. */
export function setInputLockChord(keys: string[]): Promise<void> {
  return invoke("set_input_lock_chord", { keys });
}

/** Activate the input lock — block all keyboard / mouse input until
 *  the configured chord is pressed. On macOS needs Accessibility (same
 *  grant the text-expander already uses). On Linux Wayland this
 *  returns an error (rdev's grab is X11-only). */
export function startInputLock(): Promise<void> {
  return invoke("start_input_lock");
}

// ── Wakelock (mouse-jiggle keep-awake) ─────────────────────────────────────

/** Toggle the wakelock. While active, the cursor jumps 1 px right
 *  and immediately back every 60 s — defeats idle-sleep timers and
 *  "away" detection (Teams, Slack, screen savers). Resolves with the
 *  resulting state. */
export function wakelockSet(enable: boolean): Promise<boolean> {
  return invoke("wakelock_set", { enable });
}

export function wakelockGet(): Promise<boolean> {
  return invoke("wakelock_get");
}

// ── Bruno (Brutto-Netto-Rechner — German income-tax + SV) ─────────────

/** Per-user defaults applied to a bare `bruno <€>` invocation.
 *  Persistent via the SQLite settings table. Settings panel has a
 *  collapsible Bruno section that edits these. */
export interface BrunoDefaults {
  tax_class: number;          // 1..6
  state: string;              // German state ISO short
  children: number;
  is_church_member: boolean;
  /** Krankenkasse-Zusatzbeitrag in **percent** (e.g. 2.45 for TK 2025). */
  health_add: number;
}

export function brunoGetDefaults(): Promise<BrunoDefaults> {
  return invoke("bruno_get_defaults");
}

export function brunoSetDefaults(defaults: BrunoDefaults): Promise<void> {
  return invoke("bruno_set_defaults", { defaults });
}

// ── App launcher (Spotlight-like, macOS only in v0.37) ────────────────

export interface AppEntry {
  name: string;
  path: string;
  name_lower: string;
}

/** Return the cached app index (scanned once at startup). One-shot per
 *  popup mount; no polling. Empty on non-macOS. */
export function listApps(): Promise<AppEntry[]> {
  return invoke("list_apps");
}

/** Re-scan installed apps. Used by Settings → Apps → Refresh. Returns
 *  the new count. Also clears the icon cache. */
export function refreshApps(): Promise<number> {
  return invoke("refresh_apps");
}

/** Launch the app at `path` via macOS Launch Services. Activates the
 *  existing instance if the app is already running. */
export function launchApp(path: string): Promise<void> {
  return invoke("launch_app", { path });
}

/** Lazy icon fetch. Returns base64 PNG (128×128). First call per app
 *  shells out to `sips` (~50 ms); subsequent calls hit the in-memory
 *  cache (instant). */
export function getAppIcon(path: string): Promise<string> {
  return invoke("get_app_icon", { path });
}

// ── Timer (search-bar `timer N s|min|h`) ─────────────────────────────

export interface TimerView {
  id: number;
  label: string;
  remaining_secs: number;
}

/** Start a new timer; backend spawns a worker thread that sleeps for
 *  `seconds` then fires macOS native notification + sound + emits a
 *  `timer-fired` event. Returns the new timer's id. */
export function startTimer(seconds: number, label: string): Promise<number> {
  return invoke("start_timer", { seconds, label });
}

/** Cancel an in-flight timer by id. Returns `true` if the id was
 *  active (was cancelled), `false` if it was unknown (already fired). */
export function cancelTimer(id: number): Promise<boolean> {
  return invoke("cancel_timer", { id });
}

/** Snapshot of currently-active timers. Used by the footer indicator
 *  to show count + (future) inline cancel buttons. */
export function listTimers(): Promise<TimerView[]> {
  return invoke("list_timers");
}

// ── Finder selection (macOS) ──────────────────────────────────────────

/** One item in the current Finder selection. `is_image` is a cheap
 *  extension test — good enough to decide whether to surface the
 *  Resize action. `size_bytes` is `null` when stat fails. */
export interface FinderItem {
  path: string;
  name: string;
  size_bytes: number | null;
  is_image: boolean;
}

/** Read the current Finder selection. Returns an empty list if
 *  nothing is selected. On macOS without Automation→Finder TCC
 *  permission this rejects with `"finder.automation_denied"`, which
 *  the frontend surfaces as a tailored "open System Settings" banner. */
export function getFinderSelection(): Promise<FinderItem[]> {
  return invoke("get_finder_selection");
}

/** Resize an image file with Lanczos3, writing the output next to
 *  the source as `<stem>-<W>x<H>.<ext>`. Returns the absolute path
 *  of the written file. */
export function resizeFile(path: string, width: number, height: number): Promise<string> {
  return invoke("resize_file", { path, width, height });
}

/** Optimise a single PNG file losslessly with oxipng. Writes the
 *  result next to the source as `<stem>-optim.png`. Returns the output
 *  path + before/after byte counts. Non-PNG sources reject with a
 *  clear error (oxipng is PNG-only). */
export function optimizeFile(
  path: string,
): Promise<{ path: string; before_bytes: number; after_bytes: number }> {
  return invoke("optimize_file", { path });
}

/** Read the persisted theme preference — `"light"`, `"dark"`, or
 *  `"system"`. Defaults to `"system"` on a fresh install. Backend:
 *  `commands::get_theme_preference`. */
export function getThemePreference(): Promise<string> {
  return invoke("get_theme_preference");
}

/** Persist the theme preference. The backend rejects anything that
 *  isn't one of the three valid values. Backend:
 *  `commands::set_theme_preference`. */
export function setThemePreference(theme: string): Promise<void> {
  return invoke("set_theme_preference", { theme });
}

export function deleteEntry(id: number): Promise<void> {
  return invoke("delete_entry", { id });
}

export function clearHistory(): Promise<void> {
  return invoke("clear_history");
}

export function toggleCapture(paused: boolean): Promise<void> {
  return invoke("toggle_capture", { paused });
}

export function getCaptureState(): Promise<boolean> {
  return invoke("get_capture_state");
}

export function hidePopup(): Promise<void> {
  return invoke("hide_popup");
}

/** Write `text` to the OS clipboard and paste it into the previously
 *  active app. Used by the inline calculator. */
export function pasteText(text: string): Promise<void> {
  return invoke("paste_text", { text });
}

/** Tell the backend to (not) auto-hide the popup on blur. Use while a
 *  native modal (file dialog) is open, then reset to `false`. */
export function setSuppressHide(suppress: boolean): Promise<void> {
  return invoke("set_suppress_hide", { suppress });
}

// ── Snippets ─────────────────────────────────────────────────────────────────

export function listSnippets(): Promise<Snippet[]> {
  return invoke("list_snippets");
}

export function findSnippets(query: string): Promise<Snippet[]> {
  return invoke("find_snippets", { query });
}

/** Pass id = null to create, id = number to update. Returns the snippet id. */
export function upsertSnippet(
  id: number | null,
  abbreviation: string,
  title: string,
  body: string,
): Promise<number> {
  return invoke("upsert_snippet", { id, abbreviation, title, body });
}

export function deleteSnippet(id: number): Promise<void> {
  return invoke("delete_snippet", { id });
}

export function pasteSnippet(id: number): Promise<void> {
  return invoke("paste_snippet", { id });
}

export interface ImportResult {
  imported: number;
  skipped: number;
  errors: string[];
}

/** Import snippets from a JSON string. Existing abbreviations get overwritten. */
export function importSnippets(json: string): Promise<ImportResult> {
  return invoke("import_snippets", { json });
}

/** Read a JSON file from the given path and import its snippets. */
export function importSnippetsFromFile(path: string): Promise<ImportResult> {
  return invoke("import_snippets_from_file", { path });
}

/** Re-import the bundled default AI-prompt snippets. Existing snippets
 *  sharing an `abbreviation` get overwritten; user-added snippets with
 *  distinct abbreviations are untouched. Surfaced via the Snippets-tab
 *  "Restore defaults" button. */
export function restoreDefaultPrompts(): Promise<ImportResult> {
  return invoke("restore_default_prompts");
}

// ── Notes ────────────────────────────────────────────────────────────────────

export function listNotes(): Promise<Note[]> {
  return invoke("list_notes");
}

export function listNoteCategories(): Promise<string[]> {
  return invoke("list_note_categories");
}

/** Promote a clipboard entry to a persistent note. Returns the new note id. */
export function saveClipAsNote(
  clipId: number,
  title: string,
  category: string,
): Promise<number> {
  return invoke("save_clip_as_note", { clipId, title, category });
}

/** Create a from-scratch text note. Returns the new note id. */
export function createNote(
  title: string,
  body: string,
  category: string,
): Promise<number> {
  return invoke("create_note", { title, body, category });
}

/** Update a note's title / body / category. Body edits are ignored for
 *  image and files notes (the backend short-circuits). */
export function updateNote(
  id: number,
  title: string,
  body: string,
  category: string,
): Promise<void> {
  return invoke("update_note", { id, title, body, category });
}

export function deleteNote(id: number): Promise<void> {
  return invoke("delete_note", { id });
}

export function clearNotes(): Promise<void> {
  return invoke("clear_notes");
}

export function pasteNote(id: number): Promise<void> {
  return invoke("paste_note", { id });
}

// ── Backup (full app export / import) ────────────────────────────────────────

export interface BackupExportOptions {
  includeHistory?: boolean;
  includeSnippets?: boolean;
  includeNotes?: boolean;
}

/** Returns a pretty-printed JSON string. Each section is included only
 *  when the corresponding flag is true (or undefined — defaults to true
 *  for backwards compatibility). */
export function exportBackup(opts: BackupExportOptions = {}): Promise<string> {
  return invoke("export_backup", {
    includeHistory: opts.includeHistory ?? true,
    includeSnippets: opts.includeSnippets ?? true,
    includeNotes: opts.includeNotes ?? true,
  });
}

/** Build the backup JSON (with the same selective semantics as
 *  `exportBackup`) and write it directly to `path`. Returns the number
 *  of bytes written. */
export function saveBackupToFile(
  path: string,
  opts: BackupExportOptions = {},
): Promise<number> {
  return invoke("save_backup_to_file", {
    path,
    includeHistory: opts.includeHistory ?? true,
    includeSnippets: opts.includeSnippets ?? true,
    includeNotes: opts.includeNotes ?? true,
  });
}

// ── Text expander ────────────────────────────────────────────────────────────

export interface ExpanderConfig {
  enabled: boolean;
  /** Tauri shortcut string, e.g. "Alt+Backquote", "Ctrl+Shift+E". */
  hotkey: string;
  /** True if the OS has granted Inspector Rust permission to synthesize keyboard
   *  events. macOS: Accessibility. Other OSes: always true. */
  accessibility_granted: boolean;
}

export function getExpanderConfig(): Promise<ExpanderConfig> {
  return invoke("get_expander_config");
}

/** Persist a new expander config and re-register the hotkey. The backend
 *  validates the hotkey string and errors out *before* writing settings if
 *  it's malformed, so the previous registration stays intact on failure. */
export function setExpanderConfig(
  enabled: boolean,
  hotkey: string,
): Promise<ExpanderConfig> {
  return invoke("set_expander_config", { enabled, hotkey });
}

/** Programmatically trigger an expand-at-cursor cycle. Used by the
 *  "Test now" button in settings. */
export function triggerExpandAtCursor(): Promise<void> {
  return invoke("trigger_expand_at_cursor");
}

// ── Popup hotkey (v0.43.0+) ────────────────────────────────────────────

/** Read the user-configured popup hotkey (or default if never customised). */
export function getPopupHotkey(): Promise<string> {
  return invoke("get_popup_hotkey");
}

/** The hard-coded default hotkey string, useful for "Reset to default" buttons. */
export function getPopupHotkeyDefault(): Promise<string> {
  return invoke("get_popup_hotkey_default");
}

/** Set the popup hotkey. Backend validates against the reserved global
 *  shortcuts (OCR / Screenshot / Eyedropper / Finder / expander / direct
 *  slots) and re-registers; nothing is persisted if the new hotkey is
 *  rejected, so the previous hotkey stays armed. Returns the applied
 *  hotkey on success; rejects with a descriptive error on collision. */
export function setPopupHotkey(hotkey: string): Promise<string> {
  return invoke("set_popup_hotkey", { hotkey });
}

export interface DiagnoseResult {
  captured: string;
  matched_abbreviation: string | null;
  paste_preview: string | null;
  /** Which capture mechanism was actually used. */
  path: "ax" | "uia" | "clipboard";
}

/** Capture the word before the cursor (select prev word + copy) and run
 *  the snippet lookup, but *don't* paste. Hides the popup first so the
 *  synthetic keystrokes target the prior frontmost app. Returns the raw
 *  captured text and the matched snippet abbreviation, if any. */
export function diagnoseExpandAtCursor(): Promise<DiagnoseResult> {
  return invoke("diagnose_expand_at_cursor");
}

// ── Direct hotkey → snippet slots ────────────────────────────────────────────

/** A "press this hotkey → paste this snippet's body" binding. Unlike the
 *  abbreviation expander it reads nothing — it just pastes — so it works in
 *  any app, including terminals. `abbreviation`/`title` are `null` if the
 *  bound snippet was deleted. */
export interface DirectSlot {
  /** Tauri shortcut string, e.g. "Alt+Digit2". */
  hotkey: string;
  snippet_id: number;
  abbreviation: string | null;
  title: string | null;
}

export function getDirectSlots(): Promise<DirectSlot[]> {
  return invoke("get_direct_slots");
}

/** Replace the whole direct-slot list. The backend validates snippet ids,
 *  re-registers the global shortcuts (rejecting collisions with the popup /
 *  OCR / abbreviation hotkeys and duplicates), then persists — nothing is
 *  written if registration fails, so the previous slots stay live on error.
 *  Returns the re-resolved list. */
export function setDirectSlots(
  slots: { hotkey: string; snippet_id: number }[],
): Promise<DirectSlot[]> {
  return invoke("set_direct_slots", { slots });
}

/** Cheap probe — returns true if synthetic-input permission is granted
 *  (macOS Accessibility / other OSes always true). Used for polling
 *  while the user is in System Settings granting access. */
export function getAccessibilityStatus(): Promise<boolean> {
  return invoke("get_accessibility_status");
}

/** Triggers the macOS "would like to control this computer" dialog and
 *  adds Inspector Rust to the Accessibility list. Returns the still-likely-
 *  false trusted state immediately after the prompt fires. No-op on
 *  Windows / Linux. */
export function requestAccessibilityGrant(): Promise<boolean> {
  return invoke("request_accessibility_grant");
}

/** Opens System Settings → Privacy & Security → Accessibility on macOS
 *  via `open x-apple.systempreferences:…`. No-op on other OSes. */
export function openAccessibilitySettings(): Promise<void> {
  return invoke("open_accessibility_settings");
}

/** Wipe stale TCC Accessibility/PostEvent entries for Inspector Rust (via
 *  `tccutil reset`) then fire the system "would like to control" prompt
 *  with the *current* cdhash. Use this when the System Settings toggle
 *  says "on" but Inspector Rust still asks for permission on every action —
 *  that means the toggle is for an older binary's cdhash. */
export function forceResetAndRequestGrant(): Promise<boolean> {
  return invoke("force_reset_and_request_grant");
}

/** Quit the app process. Used after granting Accessibility on macOS so
 *  the next launch picks up the fresh AXIsProcessTrusted state. */
export function quitApp(): Promise<void> {
  return invoke("quit_app");
}

/** Spawn a fresh Inspector Rust process and exit the current one. Used by the
 *  Settings panel's auto-restart prompt: the new process picks up the
 *  freshly granted Accessibility state which the running process can't
 *  see (macOS caches the trust check per-process). */
export function relaunchApp(): Promise<void> {
  return invoke("relaunch_app");
}

// ── Autostart (login item / LaunchAgent) ─────────────────────────────────────

/** Whether Inspector Rust is set to launch automatically on login.
 *  macOS: checks `~/Library/LaunchAgents/InspectorRust.plist`.
 *  Windows: checks the run-key registry entry. */
export function getAutostartEnabled(): Promise<boolean> {
  return invoke("get_autostart_enabled");
}

/** Toggle autostart. Returns the *now-effective* state read back from the
 *  OS (so the UI can reconcile against actual filesystem / registry state
 *  if the underlying call partially failed). The backend also emits the
 *  `autostart-changed` event with the same boolean — listen for it to
 *  keep tray + Settings in sync when one toggles the other. */
export function setAutostartEnabled(enabled: boolean): Promise<boolean> {
  return invoke("set_autostart_enabled", { enabled });
}

/** Read a backup JSON file from `path` and merge it into the live database. */
export function importBackup(path: string): Promise<BackupImportResult> {
  return invoke("import_backup", { path });
}

/** Show the system-wide screen color picker (eyedropper). Returns
 *  immediately. The result arrives later via the Tauri event
 *  `"color-picked"` with payload `string | null` (`null` = cancelled).
 *
 *  - macOS: invokes Apple's NSColorSampler (the same magnifier-loupe
 *    used by Pages / Keynote / Sketch). 10.15+.
 *  - Windows: pops up a fullscreen overlay; click anywhere to sample. */
export function pickScreenColor(): Promise<void> {
  return invoke("pick_screen_color");
}

/** Tint an image clipboard entry to `hex` (with or without leading `#`).
 *  Creates a NEW history entry containing the recolored PNG; the
 *  original stays put. Returns the new entry id. The popup list
 *  auto-refreshes via the `clipboard-changed` event. */
export function recolorImageEntry(id: number, hex: string): Promise<number> {
  return invoke("recolor_image_entry", { id, hex });
}

/** Returns max chromaticity (0..1) from a sample of opaque pixels in an
 *  image entry. ~0 means grayscale silhouette → tint will look clean.
 *  ~0.5+ means a saturated photo → tint will look weird. */
export function imageChromaticity(id: number): Promise<number> {
  return invoke("image_chromaticity", { id });
}

/** Background-remove an image entry via corner-sampled chroma-key.
 *  Saves the transparent PNG to `~/Downloads/inspector-rust-cutout-<ts>.png`
 *  and returns the absolute path. Leaves the history entry untouched. */
export function cutOutImageEntry(id: number): Promise<string> {
  return invoke("cut_out_image_entry", { id });
}

/** Same as `cutOutImageEntry` but reads the image from a file path on
 *  disk (any supported format: PNG, JPEG, WebP, GIF, BMP). Output is
 *  always PNG with alpha. Used when the selected entry is a single-file
 *  Files-typed clipboard entry pointing at an image. */
export function cutOutImageFile(path: string): Promise<string> {
  return invoke("cut_out_image_file", { path });
}

/** Save a clipboard image entry to `~/Downloads/inspector-rust-image-<ts>.png`
 *  unchanged. Companion to recolor — recolor produces a new history
 *  entry with the tinted image; this lets the user grab that entry as
 *  a real file on disk. Returns the saved absolute path. */
export function saveImageEntryToDownloads(id: number): Promise<string> {
  return invoke("save_image_entry_to_downloads", { id });
}

/** Result of an OCR run. `cancelled` distinguishes user-pressed-Esc
 *  from "ran but no text detected". `chars` is the unicode character
 *  count of the recognized text, included so toasts don't have to
 *  recalculate. */
export interface OcrResult {
  text: string;
  cancelled: boolean;
  chars: number;
}

/** Trigger the OCR pipeline: hide popup → interactive region pick
 *  (macOS `screencapture -i`) → OCR via Vision → write text to system
 *  clipboard → also push as a History entry. macOS only for now;
 *  Windows returns an error string. Blocks while the user is dragging
 *  the marquee.
 *
 *  Possible error sentinels (raw strings, switch on these):
 *    - "screen.permission_denied" — Screen Recording not granted
 *    - other — wrapped error message from the backend  */
export function ocrRegion(): Promise<OcrResult> {
  return invoke("ocr_region");
}

/** Result of a screenshot region capture. `cancelled` separates user
 *  pressed-Esc from "captured N bytes". `bytes` is the PNG payload size
 *  so a "saved 12.3 KB" toast can be rendered without re-measuring. */
export interface ScreenshotResult {
  cancelled: boolean;
  bytes: number;
}

/** Trigger the screenshot pipeline: hide popup → interactive region
 *  pick (macOS `screencapture -i`) → write PNG to system clipboard →
 *  also push as a History entry. No OCR step, so regions with no text
 *  (a button, a chart, a photo) still produce a usable payload.
 *  macOS only for now; Windows returns an error string. Blocks while
 *  the user is dragging the marquee.
 *
 *  Possible error sentinels (raw strings, switch on these):
 *    - "screen.permission_denied" — Screen Recording not granted
 *    - other — wrapped error message from the backend  */
export function screenshotRegion(): Promise<ScreenshotResult> {
  return invoke("screenshot_region");
}

/** Fire the eyedropper (macOS NSColorSampler loupe / Windows GDI overlay)
 *  *without* opening the popup or modal. The picked hex (`#RRGGBB`) lands
 *  on the system clipboard and as a Text History entry. Backend dispatches
 *  asynchronously — this promise resolves immediately once the picker
 *  is queued. Parallel to `ocrRegion` / `screenshotRegion` — the
 *  global-shortcut UX, not the modal UX. */
export function eyedropperToClipboard(): Promise<void> {
  return invoke("eyedropper_to_clipboard");
}

// ── Power commands (rz / optim / rmvvls) ──────────────────────────────

/** Result of `rz <W>x<H>`. */
export interface ResizeResult {
  width: number;
  height: number;
  bytes: number;
}

/** Resize the clipboard image to `width × height` using Lanczos3
 *  sampling. The resized PNG replaces the clipboard contents and is
 *  also pushed to History. Backend: `commands::resize_clipboard_image`. */
export function resizeClipboardImage(width: number, height: number): Promise<ResizeResult> {
  return invoke("resize_clipboard_image", { width, height });
}

/** Result of `optim`. `path` is the saved file, `before_bytes` /
 *  `after_bytes` let the UI show a "saved 12.3 KB → 8.1 KB" toast. */
export interface OptimResult {
  path: string;
  before_bytes: number;
  after_bytes: number;
}

/** Read the clipboard PNG, run through oxipng (lossless), save to
 *  `~/Downloads/inspector-rust-optim-<ts>.png`. Backend:
 *  `commands::optimize_clipboard_image`. */
export function optimizeClipboardImage(): Promise<OptimResult> {
  return invoke("optimize_clipboard_image");
}

/** Strip vowels (aeiou + AEIOU + ä/ö/ü/Ä/Ö/Ü) from `text` and write
 *  the result to the clipboard + History. Returns the stripped string
 *  for the UI to display. Backend: `commands::remove_vowels_to_clipboard`. */
export function removeVowelsToClipboard(text: string): Promise<string> {
  return invoke("remove_vowels_to_clipboard", { text });
}

// ── System commands (kill / reboot / shutdown / lock) ────────────────

/** One row from the kill-picker process list. */
export interface ProcessInfo {
  pid: number;
  name: string;
  memory_mb: number;
  exe: string;
}

/** Snapshot of currently-running processes, sorted by memory desc.
 *  Excludes the Inspector Rust process itself. Backend:
 *  `commands::list_processes`. */
export function listProcesses(): Promise<ProcessInfo[]> {
  return invoke("list_processes");
}

/** Send SIGTERM (graceful) or SIGKILL (force) to a process. Errors
 *  if the PID is unknown or we don't have permission. Backend:
 *  `commands::kill_process`. */
export function killProcess(pid: number, force: boolean): Promise<void> {
  return invoke("kill_process", { pid, force });
}

/** Restart the system gracefully (osascript → loginwindow). macOS-only;
 *  Windows returns "not implemented". Backend: `commands::system_reboot`. */
export function systemReboot(): Promise<void> {
  return invoke("system_reboot");
}

/** Power down the system gracefully. macOS-only; same semantics as
 *  reboot but a different Apple Event. Backend: `commands::system_shutdown`. */
export function systemShutdown(): Promise<void> {
  return invoke("system_shutdown");
}

/** Lock the screen (`pmset displaysleepnow`). macOS-only; no privilege
 *  required. Backend: `commands::system_lock`. */
export function systemLock(): Promise<void> {
  return invoke("system_lock");
}

/** Adjust system output volume by `delta` percentage points (+ louder,
 *  − quieter). Returns the new level (0–100). Bound to Shift+↑ / Shift+↓
 *  in the popup. macOS-only; Windows errors. Backend:
 *  `commands::adjust_volume`. */
export function adjustVolume(delta: number): Promise<number> {
  return invoke("adjust_volume", { delta });
}

/** Toggle system output mute. Returns the new state (`true` = now
 *  muted). The `mute` search-bar command. macOS-only. Backend:
 *  `commands::toggle_mute`. */
export function toggleMute(): Promise<boolean> {
  return invoke("toggle_mute");
}

/** Commit an already-transformed string to the clipboard + a new Text
 *  history entry. Used by the string-manipulation transforms
 *  (`Cmd/Ctrl+1…9` on a selected text entry — see `lib/text-transform.ts`).
 *  Backend: `commands::commit_transformed_text`. */
export function commitTransformedText(text: string): Promise<void> {
  return invoke("commit_transformed_text", { text });
}

// ── macOS Screen Recording permission ──────────────────────────────────────

/** Whether Inspector Rust currently has Screen Recording (TCC ScreenCapture)
 *  granted. Required for OCR to work — `screencapture -i` is attributed
 *  to Inspector Rust, so without this the marquee never appears. Always
 *  `true` on non-macOS. */
export function getScreenRecordingStatus(): Promise<boolean> {
  return invoke("get_screen_recording_status");
}

/** Trigger the macOS Screen Recording prompt. Returns the (almost
 *  always false) status immediately after firing. */
export function requestScreenRecordingGrant(): Promise<boolean> {
  return invoke("request_screen_recording_grant");
}

/** Open System Settings → Privacy & Security → Screen Recording. */
export function openScreenRecordingSettings(): Promise<void> {
  return invoke("open_screen_recording_settings");
}

/** Reset the Screen Recording TCC entry for Inspector Rust (no sudo) and
 *  re-fire the prompt. Use when System Settings shows Inspector Rust as
 *  enabled but the running process still sees the policy as denied. */
export function forceResetScreenRecordingGrant(): Promise<boolean> {
  return invoke("force_reset_screen_recording_grant");
}

// ── Automation → Finder (macOS) ────────────────────────────────────────

/** Whether Inspector Rust can read the Finder selection (Automation →
 *  Finder TCC grant). Probes by running a no-op `tell application "Finder"`
 *  script; macOS fires the Automation prompt on the first uninitialised
 *  call ever, then this is silent on every subsequent check. Always
 *  `true` on non-macOS. */
export function getFinderAutomationStatus(): Promise<boolean> {
  return invoke("get_finder_automation_status");
}

/** Open System Settings → Privacy & Security → Automation, the pane
 *  with the per-app sub-toggles. */
export function openFinderAutomationSettings(): Promise<void> {
  return invoke("open_finder_automation_settings");
}

/** `tccutil reset AppleEvents` for Inspector Rust + re-probe (which
 *  re-fires the Automation prompt). Use when System Settings shows the
 *  toggle on but Inspector Rust still can't see the Finder selection. */
export function forceResetFinderAutomationGrant(): Promise<boolean> {
  return invoke("force_reset_finder_automation_grant");
}

// ── Linux desktop shortcuts (GNOME/Cinnamon) ───────────────────────────────

export interface LinuxShortcutCandidate {
  binding: string;
  display: string;
  free: boolean;
}

export interface LinuxShortcutRow {
  id: string;
  name: string;
  arg: string;
  candidates: LinuxShortcutCandidate[];
  chosen: string;
  chosen_display: string;
}

export interface LinuxShortcutSetupScan {
  desktop: string;
  profile: string;
  can_configure: boolean;
  message: string | null;
  terminal_profiles_to_fix: number;
  rows: LinuxShortcutRow[];
  saved_summary: string | null;
}

export function linuxScanDesktopShortcuts(): Promise<LinuxShortcutSetupScan> {
  return invoke("linux_scan_desktop_shortcuts");
}

export function linuxApplyDesktopShortcuts(
  bindings: Array<{ id: string; binding: string }>,
): Promise<void> {
  return invoke("linux_apply_desktop_shortcuts", { bindings });
}

export function linuxWebHotkeyToGsettings(shortcut: string): Promise<string> {
  return invoke("linux_web_hotkey_to_gsettings", { shortcut });
}
