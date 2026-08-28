import { invoke } from "@tauri-apps/api/core";
import type { BackupImportResult, ClipEntry, Note, Snippet } from "./types";

// ── Clipboard history ────────────────────────────────────────────────────────

export function getHistory(limit = 500, offset = 0): Promise<ClipEntry[]> {
  return invoke("get_history", { limit, offset });
}

/** Fetch one entry with its **full** payload — including the image blob the
 *  slim history list omits. Used by the preview when an image clip is
 *  selected. Returns null if the row no longer exists. */
export function getClip(id: number): Promise<ClipEntry | null> {
  return invoke("get_clip", { id });
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

/** Read the pending screenshot's PNG bytes as a `data:image/png;base64,…`
 *  URL. The annotation editor loads this instead of `convertFileSrc` so
 *  the image is same-origin: on Windows the asset protocol both fails to
 *  render inside the editor webview and taints the canvas (breaking the
 *  Save path's `toDataURL()`). A data URL renders everywhere and never
 *  taints. Returns null when nothing is pending or the file is unreadable. */
export function getPendingScreenshotDataUrl(): Promise<string | null> {
  return invoke("get_pending_screenshot_data_url");
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

/** Copy the *edited* canvas (base64 PNG) straight to the clipboard —
 *  no file, no window close. Bound to Cmd/Ctrl+C in the editor.
 *  Returns the PNG byte size. */
export function editorCopy(pngB64: string): Promise<number> {
  return invoke("editor_copy", { pngB64 });
}

/** Persist the editor window size (logical px) so the next open restores
 *  it. Called from the editor's debounced resize listener. (v0.66.0) */
export function setEditorSize(width: number, height: number): Promise<void> {
  return invoke("set_editor_size", { width, height });
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
/** Toggle keep-awake. `source` ("wakelock" | "caffeine") only brands the
 *  on-screen status toast; both behave identically. `mode` (v0.116.0):
 *  "full" (default — screen forced on, the historical behaviour) or
 *  "dark" (screen may sleep, system stays awake — SSH/remote reachable). */
export function wakelockSet(
  enable: boolean,
  source?: string,
  mode?: "full" | "dark",
  keepPopup?: boolean,
): Promise<boolean> {
  return invoke("wakelock_set", {
    enable,
    source: source ?? "wakelock",
    mode: mode ?? "full",
    keepPopup: keepPopup ?? false,
  });
}

/** Wakelock status: on + which mode ("full" | "dark"). The
 *  `wakelock-changed` event carries the same shape. */
export interface WakelockStatus {
  on: boolean;
  mode: string;
}

/** Open the user's terminal (iTerm2 if installed, else Terminal.app) at the
 *  frontmost Finder window's folder. Returns the directory. macOS-only.
 *  Backend: `commands::finder_open_terminal`. */
export function finderOpenTerminal(): Promise<string> {
  return invoke("finder_open_terminal");
}

/** Markdown → PDF (same action as Ctrl+Shift+M). With `path`, converts that
 *  file; bare, converts the file-manager selection (macOS). Fire-and-forget:
 *  resolves once the conversion has been kicked off; the result surfaces via
 *  a system notification. Backend: `commands::md_to_pdf_run`. */
export function mdToPdfRun(path?: string): Promise<void> {
  return invoke("md_to_pdf_run", { path: path ?? null });
}

/** Show an on-screen status toast (hide popup + animated flourish). Used for
 *  timer / alarm confirmations. Backend: `commands::show_status_toast`. */
export function showStatusToast(
  kind: string,
  on: boolean,
  title: string,
  subtitle: string,
): Promise<void> {
  return invoke("show_status_toast", { kind, on, title, subtitle });
}

export function wakelockGet(): Promise<WakelockStatus> {
  return invoke("wakelock_get");
}

// ── Bruno (Brutto-Netto-Rechner — German income-tax + SV) ─────────────

/** Per-user defaults applied to a bare `bruno <€>` invocation.
 *  Persistent via the SQLite settings table. Settings panel has a
 *  collapsible Bruno section that edits these. */
export interface BrunoDefaults {
  tax_class: number; // 1..6
  state: string; // German state ISO short
  children: number;
  is_church_member: boolean;
  /** Krankenkasse-Zusatzbeitrag in **percent** (e.g. 2.45 for TK 2025). */
  health_add: number;
  // ── Selbständigen-Defaults (nur fürs `f`-Suffix, v0.86.0) ──
  /** "gkv" (freiwillig, berechnet) | "pkv" (fester Beitrag). */
  kv_type: string;
  /** PKV-Monatsbeitrag in Euro (inkl. privater Pflegepflicht). */
  pkv_monthly: number;
  /** GKV mit Krankengeldanspruch (14,6 %) statt ermäßigt (14,0 %). */
  kv_sick_pay: boolean;
  /** "freiberufler" (keine GewSt) | "gewerbe". */
  business_type: string;
  /** Gewerbesteuer-Hebesatz in Prozent (z. B. 400). */
  hebesatz: number;
  /** Zusammenveranlagung → Splittingtarif. */
  self_married: boolean;
}

export function brunoGetDefaults(): Promise<BrunoDefaults> {
  return invoke("bruno_get_defaults");
}

export function brunoSetDefaults(defaults: BrunoDefaults): Promise<void> {
  return invoke("bruno_set_defaults", { defaults });
}

// ── Faker (fake-data generator, v0.84.270) ────────────────────────────
import type {
  CatalogEntry,
  FakerDefaults,
  FakerGenResult,
  FakerSpec,
} from "./faker";

export interface FakerLocaleOption {
  code: string;
  label: string;
}

/** The generator catalogue with a live sample per row. Backend: faker_catalog. */
export function fakerCatalog(): Promise<CatalogEntry[]> {
  return invoke("faker_catalog");
}

// ── Figlet (ASCII-art banners, v0.85.0) ───────────────────────────────
import type {
  FigletBanner,
  FigletDefaults,
  FigletFontMeta,
  FigletOpts,
  FigletSample,
} from "./figlet";

/** All font metadata (name/category/popular/pinned). Backend: figlet_fonts. */
export function figletFonts(): Promise<FigletFontMeta[]> {
  return invoke("figlet_fonts");
}

/** Render the big preview / copy payload for one font. Backend: figlet_render. */
export function figletRender(text: string, font: string, opts: FigletOpts): Promise<FigletBanner> {
  return invoke("figlet_render", { text, font, opts });
}

/** Compact samples of `text` for a window of fonts. Backend: figlet_gallery. */
export function figletGallery(
  text: string,
  fonts: string[],
  maxLines: number,
  maxCols: number,
): Promise<FigletSample[]> {
  return invoke("figlet_gallery", { text, fonts, maxLines, maxCols });
}

export function figletGetDefaults(): Promise<FigletDefaults> {
  return invoke("figlet_get_defaults");
}

export function figletSetDefaults(defaults: FigletDefaults): Promise<void> {
  return invoke("figlet_set_defaults", { defaults });
}

/** Generate all requested values in one call. Backend: faker_generate. */
export function fakerGenerate(spec: FakerSpec): Promise<FakerGenResult> {
  return invoke("faker_generate", {
    req: {
      generator: spec.generator,
      n: spec.n,
      locale: spec.locale ?? null,
      seed: spec.seed ?? null,
      args: spec.args ?? null,
      template: spec.template ?? null,
    },
  });
}

/** The selectable locales (code + label). Backend: faker_locales. */
export function fakerLocales(): Promise<FakerLocaleOption[]> {
  return invoke("faker_locales");
}

export function fakerGetDefaults(): Promise<FakerDefaults> {
  return invoke("faker_get_defaults");
}

export function fakerSetDefaults(defaults: FakerDefaults): Promise<void> {
  return invoke("faker_set_defaults", { defaults });
}

/** Paste generated text into the focused app; optionally store in history.
 *  Backend: paste_generated. */
export function pasteGenerated(text: string, saveHistory: boolean): Promise<void> {
  return invoke("paste_generated", { text, saveHistory });
}

// ── Security command builders (sec, v0.84.271) ────────────────────────
import type { SecCatalog, SecDefaults } from "./sec";

/** The pentest-tool catalogue (presets + flag help). Backend: sec_catalog. */
export function secCatalog(): Promise<SecCatalog> {
  return invoke("sec_catalog");
}

export function secGetDefaults(): Promise<SecDefaults> {
  return invoke("sec_get_defaults");
}

export function secSetDefaults(defaults: SecDefaults): Promise<void> {
  return invoke("sec_set_defaults", { defaults });
}

/** Open the user's terminal with `command` inserted (macOS; no tool subprocess).
 *  Backend: sec_open_in_terminal. `autoEnter=false` leaves it un-submitted. */
export function secOpenInTerminal(command: string, autoEnter: boolean): Promise<void> {
  return invoke("sec_open_in_terminal", { command, autoEnter });
}

/** Whether a path exists (wordlist existence check). Backend: sec_path_exists. */
export function secPathExists(path: string): Promise<boolean> {
  return invoke("sec_path_exists", { path });
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

/** Create a file named `name` in the frontmost Finder/Explorer window's folder
 *  (or the Desktop if no window is open), optionally with `content` written into
 *  it (`touch <name> > <text>`). Returns the absolute path created. Needs the
 *  Automation→Finder TCC grant on macOS. Backend: `commands::finder_touch`. */
export function finderTouch(name: string, content = ""): Promise<string> {
  return invoke("finder_touch", { name, content });
}

/** Create a folder named `name` in the frontmost Finder window's folder.
 *  Returns the absolute path created. Backend: `commands::finder_mkdir`. */
export function finderMkdir(name: string): Promise<string> {
  return invoke("finder_mkdir", { name });
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

/** Master toggle for UI feedback sounds (expand click, OCR, screenshot,
 *  record start/stop, copy). Defaults to `true`. Backend:
 *  `commands::get_sound_enabled`. */
export function getSoundEnabled(): Promise<boolean> {
  return invoke("get_sound_enabled");
}

/** Persist + apply the feedback-sound toggle (takes effect immediately,
 *  no relaunch). Backend: `commands::set_sound_enabled`. */
/** Screenshot shutter style: "snap" (default) | "dslr" | "switch" | "off". */
export type ScreenshotSoundStyle = "snap" | "dslr" | "switch" | "off";

export function getScreenshotSound(): Promise<ScreenshotSoundStyle> {
  return invoke("get_screenshot_sound");
}

/** Persist + apply the style; the backend plays the new sound once as preview. */
export function setScreenshotSound(style: ScreenshotSoundStyle): Promise<ScreenshotSoundStyle> {
  return invoke("set_screenshot_sound", { style });
}

export function setSoundEnabled(enabled: boolean): Promise<void> {
  return invoke("set_sound_enabled", { enabled });
}

/** Popup overlay size — one of `"small"`, `"medium"`, `"large"`. Defaults
 *  to `"medium"` (the 700×500 the window ships with). Backend:
 *  `commands::get_window_size_preference`. */
export function getWindowSizePreference(): Promise<string> {
  return invoke("get_window_size_preference");
}

/** Persist the popup size and resize the live window. The backend rejects
 *  anything that isn't one of the three presets. Backend:
 *  `commands::set_window_size_preference`. */
export function setWindowSizePreference(size: string): Promise<void> {
  return invoke("set_window_size_preference", { size });
}

// ── Status toast (v0.51.0+) ────────────────────────────────────────────

/** Payload rendered by the transient on-screen status-toast window. */
export interface StatusToast {
  kind: string;
  on: boolean;
  title: string;
  subtitle: string;
}

/** Pull the latest status-toast payload (read by the toast window on
 *  mount + on each `status-toast-changed` event). */
export function getStatusToast(): Promise<StatusToast | null> {
  return invoke("get_status_toast");
}

/** Hide the toast window — called by its own auto-dismiss timer. */
export function hideStatusToast(): Promise<void> {
  return invoke("hide_status_toast");
}

export function deleteEntry(id: number): Promise<void> {
  return invoke("delete_entry", { id });
}

/** Pin / unpin a clipboard entry (floats to top, exempt from pruning). */
export function setClipPinned(id: number, pinned: boolean): Promise<void> {
  return invoke("set_clip_pinned", { id, pinned });
}

/** Attach / update / clear a note on a clipboard entry ("" clears it). Noted
 *  entries are highlighted in the list and exempt from pruning. */
export function setClipNote(id: number, note: string): Promise<void> {
  return invoke("set_clip_note", { id, note });
}

export interface ClipboardPrivacy {
  /** Comma/newline-separated app-name substrings never captured from. */
  exclude_apps: string;
  /** Seconds after a copy to auto-wipe the clipboard (0 = off). */
  auto_clear_seconds: number;
}
export function getClipboardPrivacy(): Promise<ClipboardPrivacy> {
  return invoke("get_clipboard_privacy");
}
export function setClipboardPrivacy(p: ClipboardPrivacy): Promise<void> {
  return invoke("set_clipboard_privacy", {
    excludeApps: p.exclude_apps,
    autoClearSeconds: p.auto_clear_seconds,
  });
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

/** Persisted: does clicking outside the popup close it? Default true. */
export function getPopupCloseOnBlur(): Promise<boolean> {
  return invoke("get_popup_close_on_blur");
}

/** Set + persist the click-outside behaviour (live effect, no restart). */
export function setPopupCloseOnBlur(close: boolean): Promise<void> {
  return invoke("set_popup_close_on_blur", { close });
}

// ── Snippets ─────────────────────────────────────────────────────────────────

export function listSnippets(): Promise<Snippet[]> {
  return invoke("list_snippets");
}

/** Snippet count + on-disk footprint (bytes of the stored, encrypted columns). */
export interface SnippetStorage {
  count: number;
  bytes: number;
}

export function getSnippetStorage(): Promise<SnippetStorage> {
  return invoke("get_snippet_storage");
}

export function findSnippets(query: string): Promise<Snippet[]> {
  return invoke("find_snippets", { query });
}

/** Pass id = null to create, id = number to update. Returns the snippet id.
 *  categoryId = null puts the snippet in no group ("Ungrouped"). */
export function upsertSnippet(
  id: number | null,
  abbreviation: string,
  title: string,
  body: string,
  categoryId: number | null = null,
): Promise<number> {
  return invoke("upsert_snippet", { id, abbreviation, title, body, categoryId });
}

export function deleteSnippet(id: number): Promise<void> {
  return invoke("delete_snippet", { id });
}

// ── Snippet categories (groups) ───────────────────────────────────────────────

export interface SnippetCategory {
  id: number;
  name: string;
  sort_order: number;
  /** Number of snippets currently in this group. */
  count: number;
}

export function listSnippetCategories(): Promise<SnippetCategory[]> {
  return invoke("list_snippet_categories");
}

/** Create a group (or return the existing one if the name is taken). Returns its id. */
export function createSnippetCategory(name: string): Promise<number> {
  return invoke("create_snippet_category", { name });
}

export function renameSnippetCategory(id: number, name: string): Promise<void> {
  return invoke("rename_snippet_category", { id, name });
}

/** Delete a group. Its snippets are *ungrouped*, never deleted. */
export function deleteSnippetCategory(id: number): Promise<void> {
  return invoke("delete_snippet_category", { id });
}

/** Persist a new group order (array of category ids, top to bottom). */
export function reorderSnippetCategories(ids: number[]): Promise<void> {
  return invoke("reorder_snippet_categories", { ids });
}

/** Assign a snippet to a group (categoryId = null → ungroup). */
export function setSnippetCategory(id: number, categoryId: number | null): Promise<void> {
  return invoke("set_snippet_category", { id, categoryId });
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

/** Read a JSON file from the given path and import its snippets. Accepts a full
 *  IR backup (only its snippets + groups are applied), a snippets-only backup,
 *  or the lean `[{abbreviation,title,body}]` shape. Used by the file picker AND
 *  the Snippets-tab drag-and-drop. */
export function importSnippetsFromFile(path: string): Promise<ImportResult> {
  return invoke("import_snippets_from_file", { path });
}

/** Write all snippets + their groups to `path` as a snippets-only backup
 *  document — the exchange format for editing them in another app. Returns the
 *  number of snippets written. */
export function exportSnippetsToFile(path: string): Promise<number> {
  return invoke("export_snippets_to_file", { path });
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
  includeTotp?: boolean;
  includeSettings?: boolean;
  /** Timesheet tracking data — opt-in (bumps the file format to v3). */
  includeTimesheet?: boolean;
  /** If set, encrypt the backup with this password (AES-256-GCM + Argon2id). */
  password?: string;
}

/** Returns a pretty-printed JSON string (or encrypted envelope if password
 *  is provided). Each section is included only when the corresponding flag
 *  is true (or undefined — defaults to true for backwards compatibility). */
export function exportBackup(opts: BackupExportOptions = {}): Promise<string> {
  return invoke("export_backup", {
    includeHistory: opts.includeHistory ?? true,
    includeSnippets: opts.includeSnippets ?? true,
    includeNotes: opts.includeNotes ?? true,
    includeTotp: opts.includeTotp ?? true,
    includeSettings: opts.includeSettings ?? true,
    includeTimesheet: opts.includeTimesheet ?? false,
    password: opts.password ?? null,
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
    includeTotp: opts.includeTotp ?? true,
    includeSettings: opts.includeSettings ?? true,
    includeTimesheet: opts.includeTimesheet ?? false,
    password: opts.password ?? null,
  });
}

/** Check if a backup file is encrypted (requires password to import). */
export function isBackupEncrypted(path: string): Promise<boolean> {
  return invoke("is_backup_encrypted", { path });
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

// ── Passive auto-expansion (aText-style, v0.56.0) ──────────────────────────────

export type AutoExpandTrigger = "delimiter" | "immediate";

export interface AutoExpandConfig {
  /** Master on/off for the passive keystroke monitor. */
  enabled: boolean;
  /** When a complete abbreviation expands: after a delimiter (default) or
   *  the instant it's typed. */
  trigger: AutoExpandTrigger;
  /** Match abbreviations case-sensitively (default false). */
  match_case: boolean;
  /** Let an abbreviation fire even when glued to a longer word (default false). */
  expand_inside_words: boolean;
  /** A single Backspace right after an expansion restores the abbreviation. */
  undo_enabled: boolean;
}

export function getAutoExpandConfig(): Promise<AutoExpandConfig> {
  return invoke("get_auto_expand_config");
}

// ── Cloud sync with cue (cue.celox.io) ───────────────────────────────────────

export interface SyncConfig {
  /** Master on/off for the background sync worker. */
  enabled: boolean;
  /** cue base URL (default https://cue.celox.io). */
  url: string;
  /** Per-user sync token, generated in cue → Settings → Snippet-Sync. */
  token: string;
}

export interface SyncStatus {
  /** Last successful cycle (ms epoch), 0 = never. */
  last_ms: number;
  /** Empty when the last cycle succeeded. */
  last_error: string;
}

export function getSyncConfig(): Promise<SyncConfig> {
  return invoke("get_sync_config");
}

export function setSyncConfig(config: SyncConfig): Promise<SyncConfig> {
  return invoke("set_sync_config", { config });
}

export function getSyncStatus(): Promise<SyncStatus> {
  return invoke("get_sync_status");
}

export function syncNow(): Promise<void> {
  return invoke("sync_now");
}

// ── Device sync (shared folder, v0.139.0) ───────────────────────────────────

export interface DeviceSyncConfig {
  /** Master on/off. Default OFF — disabled means no thread work at all. */
  enabled: boolean;
  /** Absolute path of the shared folder (default: iCloud Drive). */
  folder: string;
  /** 2FA secrets travel only when this is ticked. Default off. */
  include_totp: boolean;
}

export interface DeviceSyncStatus {
  last_ms: number;
  last_error: string;
  device_id: string;
  /** Other devices' files currently in the folder. */
  peers: number;
  has_passphrase: boolean;
  folder_ok: boolean;
}

export interface DeviceSyncStats {
  peers_read: number;
  clips: number;
  snippets: number;
  notes: number;
  totp: number;
  published: boolean;
  skipped: string[];
}

export function getDeviceSyncConfig(): Promise<DeviceSyncConfig> {
  return invoke("get_device_sync_config");
}
export function setDeviceSyncConfig(config: DeviceSyncConfig): Promise<void> {
  return invoke("set_device_sync_config", { config });
}
export function getDeviceSyncStatus(): Promise<DeviceSyncStatus> {
  return invoke("get_device_sync_status");
}
/** Stored in THIS Mac's keychain; the same phrase must be entered on each. */
export function setDeviceSyncPassphrase(passphrase: string): Promise<void> {
  return invoke("set_device_sync_passphrase", { passphrase });
}
export function deviceSyncNow(): Promise<DeviceSyncStats> {
  return invoke("device_sync_now");
}

/** Result of the live connection probe (Settings → Cloud-Sync checkmarks). */
export interface SyncProbe {
  /** The server answered HTTP at all. */
  reachable: boolean;
  /** The token was accepted (only meaningful when `reachable`). */
  authorized: boolean;
  /** Failure detail; "" when everything is fine. */
  message: string;
}

/** Probe the configured cue server + token — async, runs off the main thread. */
export function syncTestConnection(): Promise<SyncProbe> {
  return invoke("sync_test_connection");
}

/** Touchpad-gesture config (BetterTouchTool-style; opt-in, off by default).
 *  3-finger swipe up/down → volume, 3-finger tap → mute. */
export interface GestureConfig {
  enabled: boolean;
  fingers: number;
  volume_step: number;
  /** Tip-tap tab switching (rest one finger, tap another left/right). */
  tiptap: boolean;
  /** Suppress volume/mute for ~0.5 s after a keystroke (default on). */
  typing_guard: boolean;
  /** Per-gesture switches (default on). */
  volume: boolean;
  mute: boolean;
}

export function getGestureConfig(): Promise<GestureConfig> {
  return invoke("get_gesture_config");
}

/** Persist a new gesture config and (re)start/stop the OS capture source. */
export function setGestureConfig(config: GestureConfig): Promise<GestureConfig> {
  return invoke("set_gesture_config", { config });
}

/** A configurable global action hotkey (OCR, screenshot, timesheet, …). */
export interface ActionHotkey {
  id: string;
  label: string;
  /** Effective binding spec ("" = disabled). */
  shortcut: string;
  default: string;
  is_default: boolean;
}

export function listActionHotkeys(): Promise<ActionHotkey[]> {
  return invoke("list_action_hotkeys");
}

/** Bind (or clear, with empty `shortcut`) an action hotkey. Rejects with a
 *  human message on conflict / parse failure. */
export function setActionHotkey(id: string, shortcut: string): Promise<void> {
  return invoke("set_action_hotkey", { id, shortcut });
}

export function resetActionHotkey(id: string): Promise<void> {
  return invoke("reset_action_hotkey", { id });
}

/** Window snapping (drag a window to a screen edge to snap it; macOS, opt-in). */
export interface WindowSnapConfig {
  enabled: boolean;
}

export function getWindowSnapConfig(): Promise<WindowSnapConfig> {
  return invoke("get_window_snap_config");
}

export function setWindowSnapConfig(config: WindowSnapConfig): Promise<WindowSnapConfig> {
  return invoke("set_window_snap_config", { config });
}

/** boom — system-wide audio enhancement (macOS, driverless). */
export interface BoomEffects {
  bass: number;
  fidelity: number;
  ambience: number;
  clarity: number;
  night: number;
}

export interface BoomConfig {
  enabled: boolean;
  preset: string;
  preamp_db: number;
  band_gains_db: number[];
  boost_pct: number;
  controlled_boost: boolean;
  effects: BoomEffects;
}

export interface BoomPreset {
  name: string;
  group: string;
  gains: number[];
}

export function boomAvailable(): Promise<boolean> {
  return invoke("boom_available");
}

/** Whether the "boom Audio" virtual driver is installed + loaded. */
export function boomDriverInstalled(): Promise<boolean> {
  return invoke("boom_driver_installed");
}

/** Install the bundled driver (admin prompt + coreaudiod restart). */
export function boomInstallDriver(): Promise<void> {
  return invoke("boom_install_driver");
}

/** Uninstall the driver (admin prompt + coreaudiod restart). */
export function boomUninstallDriver(): Promise<void> {
  return invoke("boom_uninstall_driver");
}

export function boomPresets(): Promise<BoomPreset[]> {
  return invoke("boom_presets");
}

/** Live level-meter readout (input/output RMS 0..1 + clip + output-mute). */
export interface BoomLevels {
  input: number;
  output: number;
  clip: boolean;
  /** boom Audio's output is muted — the "why is it silent" flag. */
  muted: boolean;
}

export function boomLevels(): Promise<BoomLevels> {
  return invoke("boom_levels");
}

/** Clear the mute on both bridge devices (boom panel's Unmute button). */
export function boomUnmute(): Promise<boolean> {
  return invoke("boom_unmute");
}

export function getBoomConfig(): Promise<BoomConfig> {
  return invoke("get_boom_config");
}

export function setBoomConfig(config: BoomConfig): Promise<BoomConfig> {
  return invoke("set_boom_config", { config });
}

// ── snitch — network monitor + best-effort per-app blocker (macOS) ──────────
export interface SnitchApp {
  key: string;
  command: string;
  pids: number[];
  connection_count: number;
  remotes: string[];
  blocked: boolean;
}
export interface SnitchConnection {
  pid: number;
  command: string;
  proto: string;
  remote_ip: string;
  remote_port: number;
  v6: boolean;
}
export interface GeoLocation {
  ip: string;
  lat: number;
  lon: number;
  country: string;
  city: string;
  isp: string;
}
export function snitchListApps(): Promise<SnitchApp[]> {
  return invoke("snitch_list_apps");
}
export function snitchConnections(): Promise<SnitchConnection[]> {
  return invoke("snitch_connections");
}
export function snitchGeolocate(ips: string[]): Promise<GeoLocation[]> {
  return invoke("snitch_geolocate", { ips });
}
export interface NetActivity {
  pid: number;
  bytes_per_sec: number;
}
export function snitchActivity(): Promise<NetActivity[]> {
  return invoke("snitch_activity");
}
export function snitchHome(): Promise<GeoLocation | null> {
  return invoke("snitch_home");
}

// ── Shazam song recognition ─────────────────────────────────────────────────
export interface ShazamMatch {
  title: string;
  artist: string;
  cover_url: string;
  shazam_url: string;
  genre: string;
  album: string;
  released: string;
  apple_music_url: string;
  spotify_url: string;
  youtube_url: string;
}
export interface ShazamHistoryEntry {
  id: number;
  recognized_at: number;
  title: string;
  artist: string;
  cover_url: string;
  shazam_url: string;
  spotify_url: string;
  youtube_url: string;
  genre: string;
  album: string;
  released: string;
}
/** Recognize a song from 16 kHz mono PCM (i16). null = no match. Persists to history. */
export function shazamRecognize(samples: Int16Array): Promise<ShazamMatch | null> {
  return invoke("shazam_recognize", { samples: Array.from(samples) });
}
/**
 * Record `seconds` NATIVELY (cpal, in Rust — bypasses the webview so playback
 * doesn't stutter), then recognize + persist. Emits `shazam-progress` (0..1)
 * events while recording. null = no match.
 */
export function shazamListen(seconds?: number): Promise<ShazamMatch | null> {
  return invoke("shazam_listen", { seconds });
}
/** Is a recognition currently running in the backend (survives panel close)? */
export function shazamIsListening(): Promise<boolean> {
  return invoke("shazam_is_listening");
}
/** Payload of the `shazam-done` event (emitted when a run finishes). */
export interface ShazamDone {
  matched: ShazamMatch | null;
  error: string | null;
}
/** Open the matched song in Spotify — desktop app if installed, else web player. */
export function openSpotify(url: string, title: string, artist: string): Promise<void> {
  return invoke("open_spotify", { url, title, artist });
}
/** Start/stop native mic streaming (event `mic-audio`; shared by BPM + disco). */
export function micCaptureStart(): Promise<void> {
  return invoke("mic_capture_start");
}
export function micCaptureStop(): Promise<void> {
  return invoke("mic_capture_stop");
}
export function shazamHistoryList(limit?: number): Promise<ShazamHistoryEntry[]> {
  return invoke("shazam_history_list", { limit });
}

/** Lyrics for a recognized track (lrclib.net, anonymous — only artist + title
 *  are sent). `null` = the catalogue has no lyrics for this track. */
export function shazamLyrics(artist: string, title: string): Promise<string | null> {
  return invoke("shazam_lyrics", { artist, title });
}

/** One lyrics line/segment + its German translation (aligned by the translate API). */
export interface LyricSegment {
  orig: string;
  trans: string;
}
/** Bilingual lyrics: the detected source language (e.g. "en", "de") + segments. */
export interface TranslatedLyrics {
  src_lang: string;
  segments: LyricSegment[];
}
/** Lyrics translated to German (lrclib → keyless translate). `null` = no lyrics
 *  in the catalogue. When `src_lang === "de"` the track is already German
 *  (`trans` equals `orig`) and the UI shows the plain lyrics. */
export function shazamLyricsTranslated(
  artist: string,
  title: string,
): Promise<TranslatedLyrics | null> {
  return invoke("shazam_lyrics_translated", { artist, title });
}
/** A live translation result (Google gtx → MyMemory fallback, both keyless). */
export interface Translation {
  text: string;
  /** Detected source language, or "" when the provider doesn't report one. */
  detected_source: string;
  /** Which provider produced it: "google" | "mymemory". */
  provider: string;
}

/** Translate `text` from `source` → `target` for the live `tr*` preview. Rejects
 *  on timeout / rate-limit / network error — the caller then keeps the
 *  browser-open Google-Translate fallback. */
export function translateText(
  text: string,
  source: string,
  target: string,
): Promise<Translation> {
  return invoke("translate_text", { text, source, target });
}

export function shazamHistoryDelete(id: number): Promise<void> {
  return invoke("shazam_history_delete", { id });
}
export function shazamHistoryClear(): Promise<void> {
  return invoke("shazam_history_clear");
}
export function snitchSetBlocked(blocked: string[]): Promise<void> {
  return invoke("snitch_set_blocked", { blocked });
}
export function snitchIsArmed(): Promise<boolean> {
  return invoke("snitch_is_armed");
}
export function snitchArm(): Promise<void> {
  return invoke("snitch_arm");
}
export function snitchDisarm(): Promise<void> {
  return invoke("snitch_disarm");
}

/** Standard 10-band EQ centre-frequency labels (aligned to BANDS_10 in Rust). */
export const BOOM_BANDS = [
  "31",
  "62",
  "125",
  "250",
  "500",
  "1k",
  "2k",
  "4k",
  "8k",
  "16k",
];

/** Window palette (Moom-style hover palette over the green zoom button; macOS, opt-in). */
export interface WindowPaletteConfig {
  enabled: boolean;
  cols: number;
  rows: number;
}

export interface PaletteContext {
  cols: number;
  rows: number;
  screen_w: number;
  screen_h: number;
}

export function getWindowPaletteConfig(): Promise<WindowPaletteConfig> {
  return invoke("get_window_palette_config");
}

export function setWindowPaletteConfig(
  config: WindowPaletteConfig,
): Promise<WindowPaletteConfig> {
  return invoke("set_window_palette_config", { config });
}

export function windowPaletteContext(): Promise<PaletteContext> {
  return invoke("window_palette_context");
}

export function windowPaletteApply(
  fx: number,
  fy: number,
  fw: number,
  fh: number,
): Promise<void> {
  return invoke("window_palette_apply", { fx, fy, fw, fh });
}

export function windowPaletteCancel(): Promise<void> {
  return invoke("window_palette_cancel");
}

/** Show the live screen-outline preview for a 0..1 fraction (frame on-screen). */
export function windowPalettePreview(
  fx: number,
  fy: number,
  fw: number,
  fh: number,
): Promise<void> {
  return invoke("window_palette_preview", { fx, fy, fw, fh });
}

export function windowPalettePreviewHide(): Promise<void> {
  return invoke("window_palette_preview_hide");
}

/** Keep-alive: whether the OS supervisor that auto-relaunches the app is installed. */
export function getKeepaliveEnabled(): Promise<boolean> {
  return invoke("get_keepalive_enabled");
}

/** Install / remove the keep-alive supervisor; returns the now-effective state. */
export function setKeepaliveEnabled(enabled: boolean): Promise<boolean> {
  return invoke("set_keepalive_enabled", { enabled });
}

/** Persist a new auto-expansion config and (re)arm/disarm the passive
 *  monitor. Returns the now-effective config. */
export function setAutoExpandConfig(config: AutoExpandConfig): Promise<AutoExpandConfig> {
  return invoke("set_auto_expand_config", { config });
}

// ── TOTP / 2FA (v0.47.0+) ──────────────────────────────────────────────

import type { TotpCode, TotpEntry } from "./totp";

export function totpList(): Promise<TotpEntry[]> {
  return invoke("totp_list");
}

export function totpAdd(args: {
  issuer: string;
  account: string;
  secret: string;
  digits?: number;
  period?: number;
  algorithm?: string;
}): Promise<TotpEntry> {
  return invoke("totp_add", args);
}

export function totpDelete(id: number): Promise<void> {
  return invoke("totp_delete", { id });
}

/** Persist a manual drag-reorder (ids top-to-bottom). */
export function totpSetOrder(ids: number[]): Promise<void> {
  return invoke("totp_set_order", { ids });
}

/** Update every field of an entry (incl. the secret). */
export function totpUpdate(args: {
  id: number;
  issuer: string;
  account: string;
  secret: string;
  digits?: number;
  period?: number;
  algorithm?: string;
}): Promise<void> {
  return invoke("totp_update", args);
}

/** Remove duplicate entries (same issuer+account+secret). Returns the count. */
export function totpRemoveDuplicates(): Promise<number> {
  return invoke("totp_remove_duplicates");
}

/** Delete every entry. Returns the count removed. */
export function totpDeleteAll(): Promise<number> {
  return invoke("totp_delete_all");
}

export function totpCurrentCode(
  id: number,
): Promise<{ code: string; seconds_remaining: number }> {
  return invoke("totp_current_code", { id });
}

/** Polling helper for the management overlay — one IPC fetches every
 *  entry's code in one round-trip. */
export function totpCurrentCodesAll(): Promise<TotpCode[]> {
  return invoke("totp_current_codes_all");
}

export interface TotpImportResult {
  added: number;
  skipped?: number;
  /** Parsed but failed validation at insert (empty issuer, bad secret, …). */
  failed?: number;
  error: string | null;
}

export function totpImport(input: string): Promise<TotpImportResult> {
  return invoke("totp_import", { input });
}

/** Import TOTP entries from a dropped file path (read + parsed in Rust). */
export function totpImportFile(path: string): Promise<TotpImportResult> {
  return invoke("totp_import_file", { path });
}

/** Returns a newline-separated list of `otpauth://` URIs. Plaintext —
 *  the user is responsible for storing safely. */
export function totpExport(): Promise<string> {
  return invoke("totp_export");
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

/** Read the second (clipboard-history) popup hotkey, default `Ctrl+Shift+V`.
 *  Empty string = disabled. */
export function getHistoryHotkey(): Promise<string> {
  return invoke("get_history_hotkey");
}
/** The hard-coded default for the clipboard-history hotkey. */
export function getHistoryHotkeyDefault(): Promise<string> {
  return invoke("get_history_hotkey_default");
}
/** Set the clipboard-history hotkey (empty string disables it). Validated +
 *  re-registered like the main popup hotkey. */
export function setHistoryHotkey(hotkey: string): Promise<string> {
  return invoke("set_history_hotkey", { hotkey });
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

/** Read a backup JSON file from `path` and merge it into the live database.
 *  If the backup is encrypted, `password` must be provided. */
export function importBackup(
  path: string,
  password?: string,
): Promise<BackupImportResult> {
  return invoke("import_backup", { path, password: password ?? null });
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

/** Copy a frontend-rendered PNG (base64, no data: prefix) to the clipboard +
 *  history. Used by the `qr` command. Returns the new history row id. */
export function qrCopyPng(pngB64: string, label: string): Promise<number> {
  return invoke("qr_copy_png", { pngB64, label });
}

/** Copy a frontend-rendered figlet-banner PNG (base64, no data: prefix) to
 *  the clipboard + history — the figlet Shift+Enter path. Returns the new
 *  history row id. */
export function figletCopyPng(pngB64: string, label: string): Promise<number> {
  return invoke("figlet_copy_png", { pngB64, label });
}

/** Save a frontend-rendered figlet-banner PNG straight to
 *  `~/Downloads/figlet[-<slug>]-<ts>.png` (revealed in the file manager) —
 *  the figlet Cmd/Ctrl+Shift+Enter path. Returns the written path. */
export function figletSavePng(pngB64: string, label: string): Promise<string> {
  return invoke("figlet_save_png", { pngB64, label });
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

/** Capture in a specific mode with an optional self-timer (v0.57.0).
 *  mode = "region" | "fullscreen" | "window". Same staging/preview flow
 *  as `screenshotRegion`. Remembers the mode for `screenshotRepeatLast`. */
export function screenshotCapture(
  mode: "region" | "fullscreen" | "window",
  delaySeconds = 0,
): Promise<ScreenshotResult> {
  return invoke("screenshot_capture", { mode, delaySeconds });
}

/** Repeat the last capture mode (defaults to region). (v0.57.0) */
export function screenshotRepeatLast(): Promise<ScreenshotResult> {
  return invoke("screenshot_repeat_last");
}

// ── Pin to screen (v0.59.0) ────────────────────────────────────────────────

/** Pin the current pending screenshot as a floating always-on-top window.
 *  Returns the new pin window's label. Multiple pins may coexist. */
export function pinCurrentScreenshot(): Promise<string> {
  return invoke("pin_current_screenshot");
}

/** Resolve the PNG path for a pin label (the pin window calls this with its
 *  own window.label). */
export function getPinImage(label: string): Promise<string | null> {
  return invoke("get_pin_image", { label });
}

/** Close a pin window + delete its cached PNG. */
export function closePin(label: string): Promise<void> {
  return invoke("close_pin", { label });
}

// ── Cleaning workflow (v0.60.0) ────────────────────────────────────────────

export type CleanerLevel = "safe" | "standard" | "aggressive";

export interface CleanItem {
  path: string;
  size: number;
  category: string;
}
export interface CleanPlan {
  items: CleanItem[];
  total_bytes: number;
  /** [key, label, bytes] per scanned category. */
  categories: [string, string, number][];
}
/** One selectable row: a directory the scan's files were rolled up into (or a
 *  command pseudo-item like the Docker build cache). `path` is the selection
 *  key handed back to `cleaner_execute`. */
export interface CleanDir {
  path: string;
  size: number;
  count: number;
  category: string;
}
/** What a scan returns: aggregated rows only. The file-granular plan stays in
 *  the backend (a cache scan is 100k+ files — see `PlanStore`, v0.84.264). */
export interface CleanPlanView {
  dirs: CleanDir[];
  total_bytes: number;
  categories: [string, string, number][];
}
export interface CleanResult {
  deleted: number;
  freed_bytes: number;
  errors: string[];
}
/** Snapshot of the in-flight / pending clean job (survives overlay close). */
export interface CleanStatus {
  phase: "idle" | "scanning" | "executing" | string;
  view: CleanPlanView | null;
}
/** Payload of the `clean-done` event (scan or execute finished). */
export interface CleanDone {
  kind: "scan" | "execute" | string;
  view: CleanPlanView | null;
  result: CleanResult | null;
  error: string | null;
}
export interface CleanerConfig {
  level: CleanerLevel;
  min_age_days: number;
  /** category key → enabled override. */
  categories: Record<string, boolean>;
  /** Project folders searched for stale build artifacts (absolute paths). */
  dev_roots: string[];
  /** A project counts as stale after this many days untouched. */
  stale_days: number;
}
export interface CleanerCategory {
  key: string;
  label: string;
  level: CleanerLevel;
  roots: string[];
  default_enabled: boolean;
}

/** Reconnect helper — phase + pending scan view after Esc mid-job. */
export function cleanerStatus(): Promise<CleanStatus> {
  return invoke("cleaner_status");
}
/** Read-only dry-run: what would be deleted + how much. Deletes nothing.
 *  Survives overlay close; emits `clean-done` when finished. */
export function cleanerScan(): Promise<CleanPlanView> {
  return invoke("cleaner_scan");
}
/** Delete the files under the ticked directory rows (`CleanDir.path` values).
 *  Every path is re-validated against the allowlist in the backend.
 *  Survives overlay close; emits `clean-done` when finished. */
export function cleanerExecute(selected: string[]): Promise<CleanResult> {
  return invoke("cleaner_execute", { selected });
}
export function getCleanerConfig(): Promise<CleanerConfig> {
  return invoke("get_cleaner_config");
}
export function setCleanerConfig(config: CleanerConfig): Promise<CleanerConfig> {
  return invoke("set_cleaner_config", { config });
}
export function cleanerCategories(): Promise<CleanerCategory[]> {
  return invoke("cleaner_categories");
}

// ── Meme picker (v0.70.0) ──────────────────────────────────────────────────

import type { MemeEntry } from "./meme";

/** Scan the configured meme library (recursive). */
export function listMemes(): Promise<MemeEntry[]> {
  return invoke("list_memes");
}

/** Copy a meme file to the clipboard (animation preserved on macOS). */
export function copyMeme(path: string): Promise<void> {
  return invoke("copy_meme", { path });
}

/** The configured meme library directory (resolved default if unset). */
export function getMemeDir(): Promise<string> {
  return invoke("get_meme_dir");
}

/** Persist the meme library directory (blank resets to the default). */
export function setMemeDir(dir: string): Promise<void> {
  return invoke("set_meme_dir", { dir });
}

// ── Monitor brightness (v0.62.0) ───────────────────────────────────────────

export interface MonitorInfo {
  id: number;
  name: string;
  /** 0–100, current. */
  brightness: number;
  /** Whether the monitor answered DDC (slider usable). */
  supports_ddc: boolean;
  /** Slider upper bound (%). 100 = SDR only; > 100 on macOS EDR-capable displays. */
  edr_max: number;
}

/** Enumerate DDC monitors + current brightness (slow; call once on open). */
export function listBrightnessMonitors(): Promise<MonitorInfo[]> {
  return invoke("list_brightness_monitors");
}
export function getMonitorBrightness(id: number): Promise<number> {
  return invoke("get_monitor_brightness", { id });
}
export function setMonitorBrightness(id: number, percent: number): Promise<void> {
  return invoke("set_monitor_brightness", { id, percent });
}
/** Drive the EDR boost for monitor `id` to the full slider value (≤ 100 / 0 = off).
 *  macOS EDR-capable displays only; a no-op elsewhere. */
export function setEdrLevel(id: number, percent: number): Promise<void> {
  return invoke("set_edr_level", { id, percent });
}
/** Hide the popup + open the brightness slider overlay window. */
export function brightnessOpen(): Promise<void> {
  return invoke("brightness_open");
}
export function brightnessClose(): Promise<void> {
  return invoke("brightness_close");
}

// ── Audio output device (v0.80.0) ───────────────────────────────────────────

export interface AudioDevice {
  /** Opaque per-platform id (CoreAudio AudioDeviceID on macOS, MMDevice id on Windows). */
  id: string;
  name: string;
  is_default: boolean;
}
/** List the system audio output devices, marking the current default. */
export function listAudioOutputs(): Promise<AudioDevice[]> {
  return invoke("list_audio_outputs");
}
/** Set the default audio output device by its id. */
export function setAudioOutput(id: string): Promise<void> {
  return invoke("set_audio_output", { id });
}

// ── System stats (`stats` command) ──────────────────────────────────────────

export interface DiskStat {
  name: string;
  mount: string;
  fs: string;
  total: number;
  available: number;
  removable: boolean;
  kind: string;
}
export interface TempStat {
  label: string;
  celsius: number;
}
export interface FanStat {
  label: string;
  rpm: number;
}
export interface BatteryStat {
  percent: number;
  state: string;
  /** Instantaneous power draw in watts (discharge while on battery), if reported. */
  power_watts: number | null;
  time_to_empty_secs: number | null;
  time_to_full_secs: number | null;
  health_percent: number | null;
  cycle_count: number | null;
  temperature_c: number | null;
  vendor: string | null;
  model: string | null;
}
export interface SystemStats {
  host_name: string | null;
  os_name: string | null;
  kernel: string | null;
  cpu_arch: string | null;
  uptime_secs: number;
  cpu_brand: string;
  cpu_usage: number;
  cpu_freq_mhz: number;
  physical_cores: number | null;
  logical_cores: number;
  per_core: number[];
  /** `[1m, 5m, 15m]` — Unix only (null on Windows). */
  load_avg: [number, number, number] | null;
  mem_total: number;
  mem_used: number;
  mem_available: number;
  swap_total: number;
  swap_used: number;
  disks: DiskStat[];
  net_rx_per_sec: number;
  net_tx_per_sec: number;
  temps: TempStat[];
  fans: FanStat[];
  battery: BatteryStat | null;
}
/** One live snapshot of system stats (CPU/mem/disks/net/temps/fans/battery). */
export function getSystemStats(): Promise<SystemStats> {
  return invoke("get_system_stats");
}

/** One downsampled point of system-stats history (core time-series). */
export interface StatsHistoryPoint {
  ts: number; // unix seconds
  cpu: number; // %
  mem: number; // %
  net_rx: number; // bytes/s
  net_tx: number; // bytes/s
  power: number | null; // watts
  cpu_temp: number | null; // °C
  battery: number | null; // %
}

export interface StatsHistory {
  points: StatsHistoryPoint[];
  range_secs: number;
  sample_count: number;
  interval_secs: number;
}

/** Downsampled system-stats history over the last `rangeSecs` seconds. */
export function getStatsHistory(rangeSecs: number): Promise<StatsHistory> {
  return invoke("get_stats_history", { rangeSecs });
}

/** System uptime in whole seconds (the live `uptime` command anchors this to a
 * high-resolution timer and animates the sub-second digits). */
export function getUptimeSecs(): Promise<number> {
  return invoke("get_uptime_secs");
}

// ── Timer / alarm ────────────────────────────────────────────────────────────

export type AlarmStyle = "overlay" | "notification";
/** How a fired timer alerts: the loud dismiss-to-stop overlay (default) or the
 *  legacy OS notification. */
export function getAlarmStyle(): Promise<AlarmStyle> {
  return invoke("get_alarm_style");
}
export function setAlarmStyle(style: AlarmStyle): Promise<void> {
  return invoke("set_alarm_style", { style });
}
/** The fired-timer label for the alarm overlay (null if no alarm is active). */
export function alarmOverlayLabel(): Promise<string | null> {
  return invoke("alarm_overlay_label");
}
/** Silence + dismiss the active alarm (the overlay's Stop button). */
export function stopAlarm(): Promise<void> {
  return invoke("stop_alarm");
}

// ── Timesheet / time tracking ────────────────────────────────────────────────

export interface TrackStatus {
  active: boolean;
  session_id: number | null;
  paused: boolean;
  /** Manually paused (session active, nothing recorded until resume). */
  manual_paused: boolean;
  since: number | null;
  active_app: string | null;
}
export interface TrackEvent {
  id: number;
  session_id: number;
  app_name: string;
  app_id: string | null;
  window_title: string | null;
  url: string | null;
  host: string | null;
  category: string | null;
  project: string | null;
  source: string;
  is_idle: boolean;
  started_at: number;
  ended_at: number | null;
  duration_s: number | null;
}
export interface TrackBucket {
  key: string;
  seconds: number;
}
export interface ClaudeAgg {
  project: string;
  seconds: number;
  tokens_in: number;
  tokens_out: number;
}
export interface AppDetail {
  label: string;
  seconds: number;
  count: number;
}
export interface AppBreakdown {
  app: string;
  seconds: number;
  /** "browser" → details are hosts; otherwise window titles. */
  source: string;
  /** The app's current category (from its events), if any. */
  category: string | null;
  details: AppDetail[];
}
export interface DayReport {
  date: string;
  events: TrackEvent[];
  total_active_s: number;
  total_idle_s: number;
  session_count: number;
  longest_focus_s: number;
  focus_segments: number;
  by_app: TrackBucket[];
  by_category: TrackBucket[];
  by_host: TrackBucket[];
  /** Time per project tag (manual + Claude); a cross-cut, may overlap active. */
  by_project: TrackBucket[];
  /** Claude-Code usage per project (separate from the focus/browser totals). */
  claude: ClaudeAgg[];
  /** Per-app usage with an expandable detail list (browser → hosts; else titles). */
  app_breakdown: AppBreakdown[];
}
export interface DaySummary {
  date: string;
  active_s: number;
  idle_s: number;
  /** Active seconds per local hour 0..23 — for the heatmap. */
  hours: number[];
  /** This day's category breakdown — for stacked daily bars. */
  by_category: TrackBucket[];
}
export interface RangeReport {
  from: string;
  to: string;
  days: DaySummary[];
  total_active_s: number;
  total_idle_s: number;
  by_category: TrackBucket[];
  by_app: TrackBucket[];
  by_project: TrackBucket[];
}
export interface TrackEventPatch {
  app_name?: string;
  /** "" clears the column; non-empty sets it. */
  category?: string;
  project?: string;
  window_title?: string;
  is_idle?: boolean;
  started_at?: number;
  ended_at?: number;
}

export function trackStart(label?: string): Promise<number> {
  return invoke("track_start", { label: label ?? null });
}
/** Manually pause/resume recording without ending the session. */
export function trackSetPaused(paused: boolean): Promise<void> {
  return invoke("track_set_paused", { paused });
}

export function trackStop(): Promise<void> {
  return invoke("track_stop");
}
export function trackStatus(): Promise<TrackStatus> {
  return invoke("track_status");
}
export function trackGetDay(date: string): Promise<DayReport> {
  return invoke("track_get_day", { date });
}
/** Range/week report over the inclusive local-day range [from, to]. */
export function trackGetRange(from: string, to: string): Promise<RangeReport> {
  return invoke("track_get_range", { from, to });
}

// ── Consolidated slots ───────────────────────────────────────────────────────

/** How a slot's project was determined (see tracking/slots.rs). */
export type SlotOrigin = "tagged" | "claude" | "title" | "neighbour" | "unassigned";

export interface SlotApp {
  app: string;
  seconds: number;
}

/** One consolidated, bookable block derived from the raw focus events. */
export interface Slot {
  start_ms: number;
  end_ms: number;
  project: string | null;
  label: string;
  description: string;
  origin: SlotOrigin;
  apps: SlotApp[];
  event_ids: number[];
  active_s: number;
  span_s: number;
  confidence: number;
}

export interface SlotConfig {
  bridge_gap_s: number;
  noise_s: number;
  min_break_s: number;
  min_slot_s: number;
  grid_min: number;
  neighbour_gap_s: number;
  /** `project name = bcsbook shortcut`, one per line. */
  project_map: string;
  /** Master toggle for the private-app filter (default on). */
  private_filter: boolean;
  /** Apps/hosts hidden from slots + exports, one pattern per line. */
  private_apps: string;
}

/** Consolidated slots for a local day — recomputed, never stored. */
export function trackSlots(date: string): Promise<Slot[]> {
  return invoke("track_slots", { date });
}
/** Slots per day over the inclusive range [from, to]. */
export function trackSlotsRange(from: string, to: string): Promise<[string, Slot[]][]> {
  return invoke("track_slots_range", { from, to });
}
export function getSlotConfig(): Promise<SlotConfig> {
  return invoke("get_slot_config", {});
}
export function setSlotConfig(config: SlotConfig): Promise<void> {
  return invoke("set_slot_config", { config });
}

export interface BcsRow {
  shortcut: string;
  from: string;
  to: string;
  description: string;
  label?: string;
  hours?: number;
}

export interface BcsPushResult {
  written: number;
  added: number;
  skipped: number;
  unmapped: string[];
  base_url: string;
}

/** What a push would send, without sending it. */
export function trackBcsbookPreview(date: string): Promise<[BcsRow[], string[]]> {
  return invoke("track_bcsbook_preview", { date });
}
/** Hand the day's slots to bcsbook. `replace` overwrites the whole day there. */
export function trackPushBcsbook(
  date: string,
  baseUrl: string | null,
  replace: boolean,
): Promise<BcsPushResult> {
  return invoke("track_push_bcsbook", { date, baseUrl, replace });
}
/** Project-grouped customer export over [from, to] (dates) as csv|html.
 *  `project` "" = all projects; `detail` = summary | daily | full. */
export function trackExportProjects(
  format: "csv" | "html",
  from: string,
  to: string,
  project: string,
  detail: "summary" | "daily" | "full",
): Promise<string> {
  return invoke("track_export_projects", { format, from, to, project, detail });
}
export function trackUpdateEvent(id: number, patch: TrackEventPatch): Promise<void> {
  return invoke("track_update_event", { id, patch });
}
export function trackDeleteEvent(id: number): Promise<void> {
  return invoke("track_delete_event", { id });
}
export function trackMergeEvents(ids: number[]): Promise<number | null> {
  return invoke("track_merge_events", { ids });
}
export function trackSetCategory(appName: string, category: string): Promise<void> {
  return invoke("track_set_category", { appName, category });
}
/** All app→category rules as [appName, category] pairs. */
export function trackCategoryRules(): Promise<[string, string][]> {
  return invoke("track_category_rules");
}
export function trackDeleteCategoryRule(appName: string): Promise<void> {
  return invoke("track_delete_category_rule", { appName });
}
/** Distinct category names ever used — for assign autocomplete. */
export function trackDistinctCategories(): Promise<string[]> {
  return invoke("track_distinct_categories");
}
/** Assign a project to the given events (empty string clears). Returns count. */
export function trackSetProject(ids: number[], project: string): Promise<number> {
  return invoke("track_set_project", { ids, project });
}
/** Distinct project names ever used — for assign autocomplete. */
export function trackDistinctProjects(): Promise<string[]> {
  return invoke("track_distinct_projects");
}
export function trackClearAll(): Promise<void> {
  return invoke("track_clear_all");
}
/** Manually add a completed time entry (start/end in unix ms). */
export function trackAddEvent(args: {
  appName: string;
  category?: string | null;
  project?: string | null;
  windowTitle?: string | null;
  startedAt: number;
  endedAt: number;
}): Promise<number> {
  return invoke("track_add_event", {
    appName: args.appName,
    category: args.category ?? null,
    project: args.project ?? null,
    windowTitle: args.windowTitle ?? null,
    startedAt: args.startedAt,
    endedAt: args.endedAt,
  });
}
/** Tidy a day: delete idle spans + sub-`minSeconds` fragments. Returns count. */
/** Tidy a whole local-date range (inclusive) in one call — see trackCleanupDay. */
export function trackCleanupRange(
  from: string,
  to: string,
  minSeconds: number,
): Promise<number> {
  return invoke("track_cleanup_range", { from, to, minSeconds });
}

export function trackCleanupDay(date: string, minSeconds: number): Promise<number> {
  return invoke("track_cleanup_day", { date, minSeconds });
}
/** Export `[from, to)` (unix ms) to ~/Downloads as `csv` or self-contained
 *  `html`; reveals the file. Returns the written path. */
export function trackExport(
  format: "csv" | "html" | "pdf",
  from: number,
  to: number,
): Promise<string> {
  return invoke("track_export", { format, from, to });
}

export interface BridgeInfo {
  port: number;
  token: string;
}
/** Loopback-bridge connection info for the browser extension. */
export function trackBridgeInfo(): Promise<BridgeInfo> {
  return invoke("track_bridge_info");
}
/** Rotate the bridge token; returns the new token. */
export function trackBridgeRegenerate(): Promise<string> {
  return invoke("track_bridge_regenerate");
}
/** Write the browser extension to ~/Downloads and reveal it (load unpacked in
 *  Chrome). Returns the folder path. */
export function trackExportExtension(): Promise<string> {
  return invoke("track_export_extension");
}

export interface TimesheetConfig {
  idle_seconds: number;
  retention_days: number;
  claude_watcher: boolean;
  denylist: string;
  daily_goal_minutes: number;
}
export function getTimesheetConfig(): Promise<TimesheetConfig> {
  return invoke("get_timesheet_config");
}
export function setTimesheetConfig(config: TimesheetConfig): Promise<void> {
  return invoke("set_timesheet_config", { config });
}

// ── Color loupe (custom eyedropper with live hex under the loupe) ────────────

export interface LoupeData {
  /** Base64 PNG snapshot of the cursor's display, magnified by the overlay. */
  b64: string;
  /** true = modal "pick from screen" flow; false = clipboard/history eyedropper. */
  event_mode: boolean;
}
/** The loupe overlay fetches its snapshot + mode on mount. */
export function colorLoupeData(): Promise<LoupeData | null> {
  return invoke("color_loupe_data");
}
/** Commit the picked hex (clipboard+history or `color-picked` event per mode). */
export function colorLoupePick(hex: string): Promise<void> {
  return invoke("color_loupe_pick", { hex });
}
/** Dismiss the loupe without picking. */
export function colorLoupeCancel(): Promise<void> {
  return invoke("color_loupe_cancel");
}

// ── Philips Hue (`hue` command, v0.84.40) ───────────────────────────────────

export interface HueStatus {
  /** Bridge IP + username stored AND the bridge answered a light list. */
  connected: boolean;
  /** Stored bridge IP, if any. */
  bridge_ip: string | null;
  /** A username exists (paired) — may still be unreachable. */
  paired: boolean;
}

export interface HueLight {
  id: string;
  name: string;
  on: boolean;
  /** 0–100. */
  brightness: number;
  reachable: boolean;
  /** Render the colour swatches for this lamp. */
  supports_color: boolean;
  /** Render the brightness slider for this lamp. */
  dimmable: boolean;
}

/** Sentinel from `huePair` when the bridge link button wasn't pressed. */
export const HUE_LINK_BUTTON = "hue.link_button";

export function hueStatus(): Promise<HueStatus> {
  return invoke("hue_status");
}
/** Best-effort local SSDP bridge discovery (~3 s); returns an IP or null. */
export function hueDiscover(): Promise<string | null> {
  return invoke("hue_discover");
}
export function hueSetBridgeIp(ip: string): Promise<void> {
  return invoke("hue_set_bridge_ip", { ip });
}
/** Pair with the bridge at `ip` — the link button must be pressed first. */
export function huePair(ip: string): Promise<void> {
  return invoke("hue_pair", { ip });
}
export function hueForget(): Promise<void> {
  return invoke("hue_forget");
}
export function hueListLights(): Promise<HueLight[]> {
  return invoke("hue_list_lights");
}
export function hueSetLight(
  id: string,
  on: boolean,
  brightness: number | null,
  hex: string | null,
): Promise<void> {
  return invoke("hue_set_light", { id, on, brightness, hex });
}
export function hueSetAll(
  on: boolean,
  brightness: number | null,
  hex: string | null,
): Promise<void> {
  return invoke("hue_set_all", { on, brightness, hex });
}

// ── Weather (`weather` command) ─────────────────────────────────────────────

/** Animation-friendly sky classification (mirrors Rust `WeatherKind`). */
export type WeatherKind =
  | "clear-day"
  | "clear-night"
  | "clouds"
  | "drizzle"
  | "rain"
  | "thunderstorm"
  | "snow"
  | "mist";

export interface WeatherCurrent {
  temp: number;
  feels_like: number;
  temp_min: number;
  temp_max: number;
  humidity: number;
  pressure: number;
  /** m/s for metric. */
  wind_speed: number;
  wind_deg: number | null;
  description: string;
  icon: string;
  kind: WeatherKind;
  sunrise: number;
  sunset: number;
}

export interface WeatherDaily {
  dt: number;
  /** `YYYY-MM-DD`. */
  date: string;
  temp_min: number;
  temp_max: number;
  icon: string;
  kind: WeatherKind;
  description: string;
}

/** One 3-hour slot of the "next 12 h" strip (v0.106.0). */
export interface WeatherHourly {
  /** Unix seconds (UTC) of the slot start. */
  dt: number;
  temp: number;
  icon: string;
  kind: WeatherKind;
  description: string;
  /** Probability of precipitation, 0–1. */
  pop: number;
}

export interface WeatherReport {
  location: string;
  lat: number;
  lon: number;
  current: WeatherCurrent;
  /** The next ~12 hours as 3-hour slots (up to 5). */
  hourly: WeatherHourly[];
  daily: WeatherDaily[];
  /** The LOCATION's UTC offset in seconds (for labelling the hourly slots). */
  tz_offset: number;
  units: string;
}

export interface WeatherConfig {
  /** An API key is stored (the key itself never crosses IPC). */
  has_key: boolean;
  units: string;
}

/** Sentinel from `weatherFetch` when no API key is configured. */
export const WEATHER_NO_KEY = "weather.no_key";

/** Fetch current + 5-day forecast. Empty/undefined `city` → IP-located. */
export function weatherFetch(city?: string): Promise<WeatherReport> {
  return invoke("weather_fetch", { city: city && city.trim() ? city.trim() : null });
}
export function getWeatherConfig(): Promise<WeatherConfig> {
  return invoke("get_weather_config");
}

// ── Token usage (`tokens` command) ──────────────────────────────────────────

export interface TokenUsageOverview {
  total_tokens: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_create_tokens: number;
  estimated_cost: number;
  input_cost: number;
  output_cost: number;
  cache_read_cost: number;
  cache_create_cost: number;
  sessions: number;
  total_active_min: number;
  avg_active_min_per_day: number;
  active_days: number;
  messages: number;
  rate_limit_hits: number;
  lines_added: number;
  lines_removed: number;
  lines_written: number;
  period_from: string | null;
  period_to: string | null;
}

export interface TokenUsageModel {
  model: string;
  label: string;
  total_tokens: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_create_tokens: number;
  cost: number;
  messages: number;
}

export interface TokenUsageProject {
  name: string;
  total_tokens: number;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_create_tokens: number;
  cost: number;
  messages: number;
  sessions: number;
  lines_added: number;
  lines_removed: number;
  lines_written: number;
  first_ts: string | null;
  last_ts: string | null;
}

export interface TokenUsageSession {
  id: string;
  project: string;
  models: string[];
  first_ts: string;
  last_ts: string;
  duration_min: number;
  active_min: number;
  messages: number;
  total_tokens: number;
  cost: number;
}

export interface TokenUsageDayPeek {
  date: string;
  total_tokens: number;
  estimated_cost: number;
  messages: number;
  sessions: number;
}

export interface TokenUsageSnapshot {
  overview: TokenUsageOverview;
  models: TokenUsageModel[];
  projects: TokenUsageProject[];
  sessions: TokenUsageSession[];
  from: string | null;
  to: string;
  /** Yesterday's totals when period is `today` (empty-state hint). */
  prior_day?: TokenUsageDayPeek | null;
  /** Whether `sessions` was populated (lazy). */
  sessions_loaded: boolean;
}

/** Sentinel when Token Tracker isn't listening on :5010. */
export const TOKENS_UNREACHABLE = "tokens.unreachable";

/** Fetch Claude Code usage from the local Token Tracker. */
export function tokenUsageFetch(
  period: "today" | "7d" | "30d" | "all" = "today",
  includeSessions = false,
): Promise<TokenUsageSnapshot> {
  return invoke("token_usage_fetch", {
    period,
    includeSessions,
  });
}

// ── Clipboard-history cap ───────────────────────────────────────────────────

export interface HistoryLimit {
  /** Current cap. */
  max: number;
  /** Smallest allowed cap. */
  min: number;
  /** Largest allowed cap. */
  ceiling: number;
}

export function getHistoryMax(): Promise<HistoryLimit> {
  return invoke("get_history_max");
}

// ── nosleep — persistent AC sleep profile (v0.124.0, macOS) ─────────────────

export interface NoSleepStatus {
  supported: boolean;
  ac_sleep: number | null;
  battery_sleep: number | null;
  ac_disabled: boolean;
}

export function nosleepStatus(): Promise<NoSleepStatus> {
  return invoke("nosleep_status");
}
/** Toggle the persistent AC sleep lock (admin prompt). Returns the new state. */
export function nosleepSet(disable: boolean): Promise<NoSleepStatus> {
  return invoke("nosleep_set", { disable });
}

/** `alias` — create the shell alias on THIS machine (append to rc / $PROFILE).
 *  Resolves to a human success message; rejects with a human error (duplicate,
 *  fish shell, invalid name). */
export function aliasCreate(name: string, command: string, overwrite: boolean): Promise<string> {
  return invoke("alias_create", { name, command, overwrite });
}

/** One alias definition from the shell-startup chain (rc + sourced files). */
export interface AliasEntry {
  name: string;
  command: string;
  /** Display label of the defining file, e.g. `~/.claude/aliases/aliases.zsh`. */
  file: string;
  /** Defined in the primary rc file (where new aliases are appended). */
  primary: boolean;
}

/** The aliases defined in the current rc file (empty on Windows). */
export function aliasList(): Promise<AliasEntry[]> {
  return invoke("alias_list");
}

/** Remove an alias from the rc file; a miss rejects with a clear message. */
export function aliasDelete(name: string): Promise<string> {
  return invoke("alias_delete", { name });
}

// ── repo — git activity stats (v0.123.0) ────────────────────────────────────

export interface RepoMonth { month: string; commits: number }
export interface RepoFile { path: string; changes: number; churn: number }
export interface RepoExt { ext: string; commits: number; churn: number }
export interface RepoAuthor { name: string; commits: number; churn: number }
export interface RepoCat { cat: string; commits: number }
export interface RepoStats {
  name: string;
  source: string;
  commits: number;
  contributors: number;
  first_commit: string;
  last_commit: string;
  active_days: number;
  insertions: number;
  deletions: number;
  by_weekday: number[];
  by_hour: number[];
  by_month: RepoMonth[];
  top_files: RepoFile[];
  top_exts: RepoExt[];
  top_authors: RepoAuthor[];
  categories: RepoCat[];
  longest_streak: number;
  avg_msg_len: number;
}

/** Analyse a repo. `target`: a git URL, a local path, or null for the
 *  Finder-selected .git folder. Sentinel "repo.no_target" = nothing to scan. */
export function repoAnalyze(target: string | null): Promise<RepoStats> {
  return invoke("repo_analyze", { target });
}
/** Analyse + write the report to ~/Downloads; returns the path. */
export function repoExport(
  target: string | null,
  format: "html" | "pdf" = "html",
): Promise<string> {
  return invoke("repo_export", { target, format });
}

// ── clock — world clock zone persistence (v0.121.0) ─────────────────────────

/** Raw persisted zones JSON ("" when never set — normalise on the frontend). */
export function getClockZones(): Promise<string> {
  return invoke("get_clock_zones");
}
export function setClockZones(zonesJson: string): Promise<void> {
  return invoke("set_clock_zones", { zonesJson });
}

// ── disk / daisy — DaisyDisk-style usage (v0.120.0) ─────────────────────────

export interface DiskNode {
  name: string;
  size: number;
  is_dir: boolean;
  other?: boolean;
  child_count: number;
  children?: DiskNode[];
}
export interface DiskTopFile {
  path: string;
  size: number;
}
export interface DiskScan {
  root_path: string;
  root_name: string;
  total: number;
  volume_mount: string;
  volume_total: number;
  volume_free: number;
  is_volume_root: boolean;
  tree: DiskNode;
  top_files: DiskTopFile[];
  items: number;
}
export interface DiskScanProgress {
  items: number;
  bytes: number;
}

/** Scan a folder (blank → home dir). Emits `disk-scan-progress` while walking. */
export function diskScan(path: string | null): Promise<DiskScan> {
  return invoke("disk_scan", { path });
}
/** Move a scanned path to the Trash (the DaisyDisk collector). */
export function diskTrash(path: string): Promise<void> {
  return invoke("disk_trash", { path });
}

// ── adb — Android device control (v0.119.0) ─────────────────────────────────

export interface AdbDevice {
  serial: string;
  /** "device" | "unauthorized" | "offline" (verbatim from adb). */
  state: string;
  model: string;
  wifi: boolean;
}

export interface AdbStatus {
  found: boolean;
  devices: AdbDevice[];
  recording: boolean;
}

export interface AdbDashboard {
  model: string;
  manufacturer: string;
  android_version: string;
  sdk: string;
  build_id: string;
  uptime_secs: number;
  battery_level: number | null;
  battery_status: string;
  battery_health: string;
  battery_temp_c: number | null;
  battery_voltage_mv: number | null;
  mem_total_kb: number;
  mem_used_kb: number;
  storage_total_kb: number;
  storage_used_kb: number;
  wifi_ssid: string;
  ip: string;
  rssi_dbm: number | null;
  resolution: string;
  dpi: string;
  brightness: number | null;
  volume_media: number | null;
}

export function adbStatus(): Promise<AdbStatus> {
  return invoke("adb_status");
}
export function adbDashboard(serial: string): Promise<AdbDashboard> {
  return invoke("adb_dashboard", { serial });
}
/** what: brightness | volume_media/ring/alarm | wifi | bluetooth | airplane |
 *  dnd | screen_wake/sleep/lock (the audited ADBOSS command forms). */
export function adbSet(serial: string, what: string, value: number): Promise<void> {
  return invoke("adb_set", { serial, what, value });
}
export function adbKey(serial: string, keycode: string): Promise<void> {
  return invoke("adb_key", { serial, keycode });
}
export function adbText(serial: string, text: string): Promise<void> {
  return invoke("adb_text", { serial, text });
}
export function adbTap(serial: string, x: number, y: number): Promise<void> {
  return invoke("adb_tap", { serial, x, y });
}
export function adbSwipe(
  serial: string,
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  durMs: number,
): Promise<void> {
  return invoke("adb_swipe", { serial, x1, y1, x2, y2, durMs });
}
/** Device screenshot → Mac clipboard + history (canonical PNG path). */
export function adbScreenshot(serial: string): Promise<void> {
  return invoke("adb_screenshot", { serial });
}
export function adbRecordStart(serial: string): Promise<void> {
  return invoke("adb_record_start", { serial });
}
/** Stops, pulls to ~/Downloads and reveals; returns the local path. */
export function adbRecordStop(): Promise<string> {
  return invoke("adb_record_stop");
}
export function adbPackages(serial: string, includeSystem: boolean): Promise<string[]> {
  return invoke("adb_packages", { serial, includeSystem });
}
/** action: launch | stop | clear | disable | enable | uninstall. */
export function adbAppAction(serial: string, action: string, pkg: string): Promise<string> {
  return invoke("adb_app_action", { serial, action, package: pkg });
}
/** Returns the device's WLAN IP (best-effort) for the connect step. */
export function adbWifiTcpip(serial: string, port: number): Promise<string> {
  return invoke("adb_wifi_tcpip", { serial, port });
}
export function adbWifiConnect(ipPort: string): Promise<string> {
  return invoke("adb_wifi_connect", { ipPort });
}
export function adbWifiDisconnect(serial: string): Promise<void> {
  return invoke("adb_wifi_disconnect", { serial });
}

// ── loc — lines-of-code statistics (v0.117.0) ───────────────────────────────

export interface LocLanguage {
  name: string;
  files: number;
  code: number;
  comments: number;
  blanks: number;
  /** Share of all code lines, 0..100. */
  code_pct: number;
}

export interface LocReport {
  root_label: string;
  paths: string[];
  respected_ignores: boolean;
  languages: LocLanguage[];
  total_files: number;
  total_code: number;
  total_comments: number;
  total_blanks: number;
  total_lines: number;
  inaccurate: boolean;
  /** Immediate subdirectory names — what the panel offers to descend into. */
  subdirs: string[];
}

/** Count lines of code. Omitted `paths` → the live Finder selection (macOS).
 *  Sentinels: "loc.no_selection" (nothing selected), the Finder Automation
 *  sentinel, or a plain error string. */
export function locCount(paths: string[] | null, respectIgnores: boolean): Promise<LocReport> {
  return invoke("loc_count", { paths, respectIgnores });
}

/** Export the loc report to ~/Downloads and reveal it. Returns the path.
 *  One HTML renderer in Rust feeds all three formats, so they cannot drift. */
export function locExport(report: LocReport, format: "html" | "pdf" | "png"): Promise<string> {
  return invoke("loc_export", { report, format });
}

// ── System sleep status (footer indicator, v0.114.0, macOS) ─────────────────

export interface SleepStatus {
  /** false on non-macOS or when `pmset` failed — hide the indicator. */
  supported: boolean;
  /** The ACTIVE pmset profile has `sleep 0` — idle sleep never happens. */
  sleep_disabled: boolean;
  /** ≥1 counted PreventUserIdleSystemSleep assertion is active. */
  prevented: boolean;
  /** ≥1 counted assertion has no timeout (held until its process lets go). */
  indefinite: boolean;
  /** Largest assertion timeout in seconds = time until sleep is possible. */
  max_timeout_secs: number | null;
  /** Deduplicated holder names with counts, e.g. ["caffeinate ×4", "sharingd"]. */
  holders: string[];
}

export function getSleepStatus(): Promise<SleepStatus> {
  return invoke("get_sleep_status");
}

// ── CRT popup animation duration (v0.113.0) ─────────────────────────────────

export interface CrtAnimation {
  /** Power-on duration in ms; `0` = animation off. The power-off is derived. */
  ms: number;
  min: number;
  max: number;
  default_ms: number;
}

export function getCrtAnimation(): Promise<CrtAnimation> {
  return invoke("get_crt_animation");
}
/** Persist the duration (clamped backend-side, `0` = off); returns the stored value. */
export function setCrtAnimation(ms: number): Promise<number> {
  return invoke("set_crt_animation", { ms });
}

/** Set the cap (clamped backend-side) and prune now; returns the stored value. */
export function setHistoryMax(max: number): Promise<number> {
  return invoke("set_history_max", { max });
}
export function setWeatherConfig(
  apiKey: string | null,
  units: string | null,
): Promise<void> {
  return invoke("set_weather_config", { apiKey, units });
}

/** Screen-recording region in **physical** pixels (CSS-rect × devicePixelRatio). */
export interface RecordRegion {
  x: number;
  y: number;
  w: number;
  h: number;
}
/** Which audio tracks to capture. Both false → silent video. */
export interface AudioChoice {
  system: boolean;
  mic: boolean;
}
/** Sentinel returned by `startScreenRecord` when ffmpeg isn't installed. */
export const ERR_NO_FFMPEG = "record.no_ffmpeg";
/** Open the fullscreen region-select overlay (start of the Ctrl+Shift+R flow). */
export function screenRecordOpenOverlay(): Promise<void> {
  return invoke("screen_record_open_overlay");
}
/** Esc / cancel from the overlay — closes it without recording. */
export function cancelRecordOverlay(): Promise<void> {
  return invoke("cancel_record_overlay");
}
/** Start recording the chosen region with the chosen audio tracks. Closes the
 *  overlay, shows the floating stop bar. Rejects with `record.no_ffmpeg` if
 *  ffmpeg is missing. */
export function startScreenRecord(
  region: RecordRegion,
  audio: AudioChoice,
): Promise<void> {
  return invoke("start_screen_record", { region, audio });
}
/** Pause the active recording (finalises the current segment). */
export function pauseScreenRecord(): Promise<void> {
  return invoke("pause_screen_record");
}
/** Resume a paused recording (starts a fresh segment). */
export function resumeScreenRecord(): Promise<void> {
  return invoke("resume_screen_record");
}
/** Stop the active recording, finalise + concat the MP4, reveal it. Returns the path. */
export function stopScreenRecord(): Promise<string> {
  return invoke("stop_screen_record");
}
/** Whether a recording is currently active. */
export function isRecording(): Promise<boolean> {
  return invoke("is_recording");
}

// ── Audio swap (replace / overlay a video's audio) ───────────────────────────

export type SwapMode = "replace" | "mix";

/** ffmpeg mux spec (camelCase → Rust `SwapSpec`). Times in seconds. */
export interface SwapSpec {
  mode: SwapMode;
  startSeconds: number;
  audioIn: number;
  audioOut: number | null;
  overlayVolume: number;
  originalVolume: number;
  videoSeconds: number;
}

/** Sentinels surfaced by the audio-swap commands. */
export const ERR_NO_YTDLP = "audioswap.no_ytdlp";
export const ERR_NO_FFMPEG_SWAP = "audioswap.no_ffmpeg";

/** The Finder-selected video the overlay should preload (or null). */
export function audioSwapGetSelectedVideo(): Promise<string | null> {
  return invoke("audio_swap_get_selected_video");
}
/** Media duration in seconds (video or audio), or null if unreadable. */
export function audioSwapProbe(path: string): Promise<number | null> {
  return invoke("audio_swap_probe", { path });
}
/** Whether yt-dlp is installed (gates the YouTube field). */
export function audioSwapYtdlpAvailable(): Promise<boolean> {
  return invoke("audio_swap_ytdlp_available");
}
/** Download a URL's audio via yt-dlp; returns the produced file path. */
export function audioSwapDownloadYoutube(url: string): Promise<string> {
  return invoke("audio_swap_download_youtube", { url });
}
/** Mux the audio into the video; returns the output path (revealed in Finder). */
export function audioSwapApply(
  video: string,
  audio: string,
  spec: SwapSpec,
): Promise<string> {
  return invoke("audio_swap_apply", { video, audio, spec });
}
/** Close the audio-swap overlay window. */
export function audioSwapCancelOverlay(): Promise<void> {
  return invoke("audio_swap_cancel_overlay");
}

// ── Social download + trim ───────────────────────────────────────────────────

export type DlMode = "video" | "audio";

/** Whether yt-dlp is installed (gates the download buttons). */
export function socialYtdlpAvailable(): Promise<boolean> {
  return invoke("social_ytdlp_available");
}
/** Download a social-media URL (video/audio) → Downloads; returns the path. */
/** Show one file in Finder/Explorer. */
export function revealPath(path: string): Promise<void> {
  return invoke("reveal_path", { path });
}

export function socialDownload(
  url: string,
  mode: DlMode,
  /** Batch downloads pass false — see the Rust doc for why revealing each
   *  file would kill the queue that started it. */
  reveal = true,
): Promise<string> {
  return invoke("social_download", { url, mode, reveal });
}

export interface TrimFileInfo {
  duration: number;
  is_video: boolean;
}
/** Open the trim overlay window. */
export function trimOpenOverlay(): Promise<void> {
  return invoke("trim_open_overlay");
}
/** Close the trim overlay window. */
export function trimCancelOverlay(): Promise<void> {
  return invoke("trim_cancel_overlay");
}
/** Duration + whether the file has video, for the trim timeline. */
export function trimFileInfo(path: string): Promise<TrimFileInfo | null> {
  return invoke("trim_file_info", { path });
}
/** Trim a file to [start, end] (seconds); returns the output path (revealed). */
export function trimApply(
  input: string,
  start: number,
  end: number,
  lossless: boolean,
): Promise<string> {
  return invoke("trim_apply", { input, start, end, lossless });
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
export function resizeClipboardImage(
  width: number,
  height: number,
): Promise<ResizeResult> {
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

/** Read the current system output volume (0–100), or null if no read-back
 *  (e.g. Windows). Backend: `commands::get_system_volume`. */
export function getSystemVolume(): Promise<number | null> {
  return invoke("get_system_volume");
}

/** Set the system output volume to an absolute level (0–100); returns the
 *  applied level, or null if unsupported. Backend: `commands::set_system_volume`. */
export function setSystemVolume(level: number): Promise<number | null> {
  return invoke("set_system_volume", { level });
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
export function commitTransformedText(
  text: string,
  sourceId?: number | null,
  kind?: string | null,
): Promise<void> {
  return invoke("commit_transformed_text", {
    text,
    sourceId: sourceId ?? null,
    kind: kind ?? null,
  });
}

/** Whether the history list draws the lineage rails that connect a derived
 *  copy to the clip it came from (v0.93.1, default on). */
export function getLineageHighlight(): Promise<boolean> {
  return invoke("get_lineage_highlight");
}

/** Persist the lineage-rail preference. */
export function setLineageHighlight(value: boolean): Promise<void> {
  return invoke("set_lineage_highlight", { value });
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

// ------------------------------------------------------------------- iris

/** Live state of the `iris` mic-triggered screen vignette. */
export type IrisStatus = {
  active: boolean;
  /** SPL on the disco-controller convention (`dBFS + 90`). */
  threshold_spl: number;
  over: boolean;
  min_spl: number;
  max_spl: number;
};

/** Arm iris. Omit the threshold to reuse the last saved one. Returns the
 *  clamped threshold actually applied. */
export function irisStart(thresholdSpl?: number): Promise<number> {
  return invoke("iris_start", { thresholdSpl });
}

export function irisStop(): Promise<void> {
  return invoke("iris_stop");
}

export function irisStatus(): Promise<IrisStatus> {
  return invoke("iris_status");
}

/** Retune a running session (or persist for the next arm). */
export function irisSetThreshold(thresholdSpl: number): Promise<number> {
  return invoke("iris_set_threshold", { thresholdSpl });
}

// ── pagespeed — Google PageSpeed Insights (v0.142.0) ────────────────────────

export interface PsCategory { id: string; label: string; score: number | null }
export interface PsMetric { id: string; label: string; display: string; score: number | null }
export interface PsRun {
  strategy: string;
  categories: PsCategory[];
  metrics: PsMetric[];
  final_url: string;
  fetch_time: string;
  lighthouse_version: string;
}
export interface PageSpeedReport {
  url: string;
  desktop: PsRun | null;
  mobile: PsRun | null;
  /** Why a strategy is missing — never silently empty. */
  errors: string[];
}

/** Analyse a URL. Both strategies run in parallel in the backend. */
export function pagespeedAnalyze(url: string): Promise<PageSpeedReport> {
  return invoke("pagespeed_analyze", { url });
}
/** Export desktop AND mobile as one document. Returns the written path. */
export function pagespeedExport(report: PageSpeedReport, format: "html" | "pdf"): Promise<string> {
  return invoke("pagespeed_export", { report, format });
}
/** Only WHETHER a key is stored — the key itself never crosses IPC. */
export function getPagespeedKey(): Promise<boolean> {
  return invoke("get_pagespeed_key");
}
export function setPagespeedKey(key: string): Promise<void> {
  return invoke("set_pagespeed_key", { key });
}
