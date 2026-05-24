use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::backup::{self, BackupImportResult};
use crate::clipboard_watcher::WatcherState;
use crate::cutout_ml;
use crate::db::{self, DbHandle};
use crate::expander;
use crate::hotkey::{self, ExpanderShortcutState};
use crate::models::ClipEntry;
use crate::notes::{self, Note};
use crate::ocr;
use crate::paste;
use crate::recolor;
use crate::region_picker;
use crate::screen_recording;
use crate::seed;
use crate::settings;
use crate::snippets::{self, ImportResult, Snippet};
use crate::ui_state::UiState;

fn map_err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ── Clipboard history ────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_history(
    db: State<'_, DbHandle>,
    limit: usize,
    offset: usize,
) -> Result<Vec<ClipEntry>, String> {
    db::list(&db, limit, offset).map_err(map_err)
}

#[tauri::command]
pub fn search_history(
    db: State<'_, DbHandle>,
    query: String,
    limit: usize,
) -> Result<Vec<ClipEntry>, String> {
    let all = db::list(&db, 1000, 0).map_err(map_err)?;
    let q = query.to_lowercase();
    if q.is_empty() {
        return Ok(all.into_iter().take(limit).collect());
    }
    let filtered: Vec<_> = all
        .into_iter()
        .filter(|e| e.content_text.to_lowercase().contains(&q))
        .take(limit)
        .collect();
    Ok(filtered)
}

/// Settings key controlling whether HTML / RTF clipboard entries get
/// downgraded to plain text on paste. Defaults to `true` — most users
/// want to drop the source app's styling when pasting elsewhere.
const KEY_PLAIN_TEXT_ONLY: &str = "paste.plain_text_only";

/// When false (the default), the OCR pipeline persists only the
/// recognised text to history — the source PNG is captured for the
/// recognition step and then discarded. When true, the PNG is also
/// upserted as a history entry (the pre-v0.26.3 behaviour, opt-in via
/// Settings → Capture → "Keep OCR source image in history").
const KEY_OCR_SAVE_SOURCE: &str = "ocr.save_source_image";

/// Persisted list of key-name strings (e.g. `["i", "r"]`) that the
/// user must press simultaneously to release the input lock. Default
/// is `["i", "r"]` — hold `i`, press `r`. Stored as a JSON array.
const KEY_INPUT_LOCK_CHORD: &str = "input_lock.unlock_keys";

/// Sentinel error string the frontend recognises and presents as the
/// "Accessibility access required" toast. Kept stable so the JS side
/// can switch on it without parsing localized text.
const ERR_NO_ACCESSIBILITY: &str = "ax.permission_denied";

/// Same shape as `ERR_NO_ACCESSIBILITY` but for the **Screen Recording**
/// TCC policy — required by the OCR pipeline because `screencapture -i`
/// is attributed to Inspector Rust and macOS denies the capture without the
/// permission. Without this signal the OCR shortcut would silently
/// fail and the user would have no way to figure out why.
const ERR_NO_SCREEN_RECORDING: &str = "screen.permission_denied";

/// Bail-out helper: returns `Err(ERR_NO_ACCESSIBILITY)` when
/// `accessibility_granted()` is false, so paste IPCs short-circuit
/// before reaching enigo. Without this guard, paste actions on an
/// untrusted process would just silently no-op (because we now pass
/// `open_prompt_to_get_permissions = false` to enigo) — the user
/// wouldn't know why nothing happened. With this guard, the frontend
/// gets a structured error and can show a helpful toast.
fn require_accessibility() -> Result<(), String> {
    if expander::accessibility_granted() {
        Ok(())
    } else {
        Err(ERR_NO_ACCESSIBILITY.to_string())
    }
}

/// Default behaviour: respects the `paste.plain_text_only` setting. For
/// HTML / RTF entries with the setting on, pastes the plain-text
/// preview (`content_text`) instead of the formatted payload.
/// Image / Files entries are unaffected — they're always pasted as-is.
#[tauri::command]
pub fn paste_entry(
    app: AppHandle,
    db: State<'_, DbHandle>,
    watcher: State<'_, WatcherState>,
    id: i64,
) -> Result<(), String> {
    require_accessibility()?;
    let entry = db::get(&db, id)
        .map_err(map_err)?
        .ok_or_else(|| "entry not found".to_string())?;

    let plain_only = settings::get_bool(&db, KEY_PLAIN_TEXT_ONLY, true).unwrap_or(true);

    hotkey::hide_popup(&app);
    if plain_only
        && matches!(
            entry.content_type,
            crate::models::ContentType::Html | crate::models::ContentType::Rtf
        )
    {
        // Mark + write the plain-text downgrade so the watcher skips
        // capturing this back as a duplicate Text clip.
        watcher.mark_self_write(crate::models::ContentType::Text, &entry.content_text);
        paste::paste_text(&entry.content_text).map_err(map_err)?;
    } else {
        watcher.mark_self_write(entry.content_type, &entry.content_data);
        paste::paste_entry(&entry).map_err(map_err)?;
    }
    db::touch(&db, id).map_err(map_err)?;
    Ok(())
}

/// Read the current value of `paste.plain_text_only` (default `true`).
#[tauri::command]
pub fn get_paste_plain_text_only(db: State<'_, DbHandle>) -> Result<bool, String> {
    settings::get_bool(&db, KEY_PLAIN_TEXT_ONLY, true).map_err(map_err)
}

/// Persist a new value for `paste.plain_text_only`.
#[tauri::command]
pub fn set_paste_plain_text_only(
    db: State<'_, DbHandle>,
    value: bool,
) -> Result<(), String> {
    settings::set(
        &db,
        KEY_PLAIN_TEXT_ONLY,
        if value { "true" } else { "false" },
    )
    .map_err(map_err)
}

/// Read the current value of `ocr.save_source_image` (default `false` —
/// OCR persists only the recognised text to history). When `true`, the
/// source PNG is also upserted as a history entry.
#[tauri::command]
pub fn get_ocr_save_source_image(db: State<'_, DbHandle>) -> Result<bool, String> {
    settings::get_bool(&db, KEY_OCR_SAVE_SOURCE, false).map_err(map_err)
}

/// Persist a new value for `ocr.save_source_image`.
#[tauri::command]
pub fn set_ocr_save_source_image(
    db: State<'_, DbHandle>,
    value: bool,
) -> Result<(), String> {
    settings::set(
        &db,
        KEY_OCR_SAVE_SOURCE,
        if value { "true" } else { "false" },
    )
    .map_err(map_err)
}

/// Read the persisted unlock chord for the input lock. Returns the
/// default (`["i", "r"]`) if nothing is stored or the stored JSON is
/// malformed.
#[tauri::command]
pub fn get_input_lock_chord(db: State<'_, DbHandle>) -> Result<Vec<String>, String> {
    let default = vec!["i".to_string(), "r".to_string()];
    let raw = match settings::get(&db, KEY_INPUT_LOCK_CHORD) {
        Ok(Some(s)) => s,
        _ => return Ok(default),
    };
    match serde_json::from_str::<Vec<String>>(&raw) {
        Ok(v) if !v.is_empty() => Ok(v),
        _ => Ok(default),
    }
}

/// Persist a new unlock chord. Rejects empty / all-unparseable
/// chords so the user can never lock themselves out by saving an
/// unusable chord.
#[tauri::command]
pub fn set_input_lock_chord(
    db: State<'_, DbHandle>,
    keys: Vec<String>,
) -> Result<(), String> {
    if keys.is_empty() {
        return Err("chord cannot be empty".into());
    }
    let any_valid = keys
        .iter()
        .any(|k| crate::input_lock::key_from_str(k).is_some());
    if !any_valid {
        return Err("chord contains no recognised keys".into());
    }
    let json =
        serde_json::to_string(&keys).map_err(|e| format!("serialise chord: {e}"))?;
    settings::set(&db, KEY_INPUT_LOCK_CHORD, &json).map_err(map_err)
}

/// Activate the input lock. Reads the persisted unlock chord from
/// settings and hands it to `input_lock::start_input_lock`.
#[tauri::command]
pub fn start_input_lock(
    db: State<'_, DbHandle>,
    app: AppHandle,
) -> Result<(), String> {
    let chord = get_input_lock_chord(db)?;
    // Hide the popup so the user isn't visually staring at an open
    // window that can no longer accept clicks.
    hotkey::hide_popup(&app);
    crate::input_lock::start_input_lock(chord)
}

// ── Wakelock ──────────────────────────────────────────────────────────

/// Toggle the mouse-jiggle wakelock. Returns the resulting state
/// (`true` = active, `false` = off).
#[tauri::command]
pub fn wakelock_set(
    state: State<'_, crate::wakelock::WakelockState>,
    enable: bool,
) -> bool {
    crate::wakelock::set_enabled(state.inner(), enable)
}

#[tauri::command]
pub fn wakelock_get(state: State<'_, crate::wakelock::WakelockState>) -> bool {
    crate::wakelock::is_enabled(state.inner())
}

// ── Appearance / theme ────────────────────────────────────────────────

const KEY_THEME: &str = "appearance.theme";

/// Normalise an arbitrary stored / incoming theme string to one of the
/// three valid values. Anything unrecognised collapses to `"system"`
/// so a hand-edited settings DB can never wedge the UI.
fn normalise_theme(s: &str) -> &'static str {
    match s {
        "light" => "light",
        "dark" => "dark",
        _ => "system",
    }
}

/// Read the persisted theme preference. One of `"light"`, `"dark"`,
/// `"system"`. Defaults to `"system"` (follow the OS) on a fresh
/// install — the long-standing pre-v0.20.0 behaviour.
#[tauri::command]
pub fn get_theme_preference(db: State<'_, DbHandle>) -> Result<String, String> {
    let raw = settings::get_or(&db, KEY_THEME, "system").map_err(map_err)?;
    Ok(normalise_theme(&raw).to_string())
}

/// Persist the theme preference. Rejects anything that isn't one of
/// the three valid values rather than silently storing garbage.
#[tauri::command]
pub fn set_theme_preference(
    db: State<'_, DbHandle>,
    theme: String,
) -> Result<(), String> {
    let normalised = normalise_theme(&theme);
    if normalised != theme {
        return Err(format!(
            "invalid theme {theme:?} — expected one of light / dark / system",
        ));
    }
    settings::set(&db, KEY_THEME, normalised).map_err(map_err)
}

/// Force-format paste — bypasses the `paste.plain_text_only` setting and
/// always uses the entry's original content type. Wired to Shift+Enter
/// in the popup as a one-shot override for users who normally paste as
/// plain text but want to keep formatting *this* time.
#[tauri::command]
pub fn paste_entry_formatted(
    app: AppHandle,
    db: State<'_, DbHandle>,
    watcher: State<'_, WatcherState>,
    id: i64,
) -> Result<(), String> {
    require_accessibility()?;
    let entry = db::get(&db, id)
        .map_err(map_err)?
        .ok_or_else(|| "entry not found".to_string())?;

    hotkey::hide_popup(&app);
    watcher.mark_self_write(entry.content_type, &entry.content_data);
    paste::paste_entry(&entry).map_err(map_err)?;
    db::touch(&db, id).map_err(map_err)?;
    Ok(())
}

#[tauri::command]
pub fn delete_entry(db: State<'_, DbHandle>, id: i64) -> Result<(), String> {
    db::delete(&db, id).map_err(map_err)
}

#[tauri::command]
pub fn clear_history(db: State<'_, DbHandle>) -> Result<(), String> {
    db::clear(&db).map_err(map_err)
}

#[tauri::command]
pub fn toggle_capture(state: State<'_, WatcherState>, paused: bool) -> Result<(), String> {
    state.paused.store(paused, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn get_capture_state(state: State<'_, WatcherState>) -> bool {
    state.paused.load(Ordering::Relaxed)
}

#[tauri::command]
pub fn hide_popup(app: AppHandle) -> Result<(), String> {
    hotkey::hide_popup(&app);
    Ok(())
}

/// Hide the popup, write `text` to the clipboard, and synthesize the paste
/// shortcut. Used by the inline calculator (and any other "compute and
/// paste" flow). The freshly-written clipboard entry would normally be
/// picked up by the watcher and recorded in history; we mark the write
/// so the watcher skips that one event — calc/color results aren't worth
/// adding to history (they're cheap to recompute).
#[tauri::command]
pub fn paste_text(
    app: AppHandle,
    watcher: State<'_, WatcherState>,
    text: String,
) -> Result<(), String> {
    require_accessibility()?;
    hotkey::hide_popup(&app);
    watcher.mark_self_write(crate::models::ContentType::Text, &text);
    paste::paste_text(&text).map_err(map_err)
}

/// Toggle the popup's hide-on-blur behaviour. The frontend sets this to
/// `true` before opening a modal child window (file dialog) so the popup
/// stays visible while the modal owns focus, then resets to `false` once
/// the modal is dismissed.
#[tauri::command]
pub fn set_suppress_hide(state: State<'_, UiState>, suppress: bool) -> Result<(), String> {
    state.suppress_hide.store(suppress, Ordering::Relaxed);
    Ok(())
}

// ── Snippets ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_snippets(db: State<'_, DbHandle>) -> Result<Vec<Snippet>, String> {
    snippets::list_all(&db).map_err(map_err)
}

#[tauri::command]
pub fn find_snippets(
    db: State<'_, DbHandle>,
    query: String,
) -> Result<Vec<Snippet>, String> {
    snippets::find_by_query(&db, &query).map_err(map_err)
}

/// Create (id = null) or update (id = some) a snippet.
#[tauri::command]
pub fn upsert_snippet(
    db: State<'_, DbHandle>,
    id: Option<i64>,
    abbreviation: String,
    title: String,
    body: String,
) -> Result<i64, String> {
    match id {
        None => snippets::create(&db, &abbreviation, &title, &body).map_err(map_err),
        Some(existing_id) => {
            snippets::update(&db, existing_id, &abbreviation, &title, &body)
                .map_err(map_err)?;
            Ok(existing_id)
        }
    }
}

#[tauri::command]
pub fn delete_snippet(db: State<'_, DbHandle>, id: i64) -> Result<(), String> {
    snippets::delete(&db, id).map_err(map_err)
}

/// Paste a snippet: hide the popup, write body to clipboard, simulate Ctrl+V.
#[tauri::command]
pub fn paste_snippet(
    app: AppHandle,
    db: State<'_, DbHandle>,
    watcher: State<'_, WatcherState>,
    id: i64,
) -> Result<(), String> {
    require_accessibility()?;
    let snippet = snippets::list_all(&db)
        .map_err(map_err)?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| "snippet not found".to_string())?;

    hotkey::hide_popup(&app);
    watcher.mark_self_write(crate::models::ContentType::Text, &snippet.body);
    paste::paste_text(&snippet.body).map_err(map_err)?;
    Ok(())
}

/// Import snippets from a JSON document. Existing rows with the same
/// abbreviation are overwritten. Per-row errors are returned in the result
/// instead of aborting the whole import.
#[tauri::command]
pub fn import_snippets(
    db: State<'_, DbHandle>,
    json: String,
) -> Result<ImportResult, String> {
    snippets::import_from_json(&db, &json).map_err(map_err)
}

/// Read a JSON file from disk and import its snippets. Path is supplied by
/// the frontend after the user picked a file via the native dialog plugin.
#[tauri::command]
pub fn import_snippets_from_file(
    db: State<'_, DbHandle>,
    path: String,
) -> Result<ImportResult, String> {
    let json = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {path}: {e}"))?;
    snippets::import_from_json(&db, &json).map_err(map_err)
}

/// Re-import the bundled default AI-prompt snippets. Existing rows
/// sharing an `abbreviation` get overwritten; user snippets with
/// distinct abbreviations are untouched. Surfaced via the Snippets-tab
/// "Restore defaults" button.
#[tauri::command]
pub fn restore_default_prompts(db: State<'_, DbHandle>) -> Result<ImportResult, String> {
    seed::restore_defaults(&db).map_err(map_err)
}

// ── Notes ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_notes(db: State<'_, DbHandle>) -> Result<Vec<Note>, String> {
    notes::list_all(&db).map_err(map_err)
}

#[tauri::command]
pub fn list_note_categories(db: State<'_, DbHandle>) -> Result<Vec<String>, String> {
    notes::list_categories(&db).map_err(map_err)
}

/// Promote a clipboard entry to a persistent note. Returns the note's id.
/// Errors if the clip no longer exists (e.g. just got pruned).
#[tauri::command]
pub fn save_clip_as_note(
    db: State<'_, DbHandle>,
    clip_id: i64,
    title: String,
    category: String,
) -> Result<i64, String> {
    notes::save_from_clip(&db, clip_id, &title, &category)
        .map_err(map_err)?
        .ok_or_else(|| "clipboard entry not found".to_string())
}

#[tauri::command]
pub fn create_note(
    db: State<'_, DbHandle>,
    title: String,
    body: String,
    category: String,
) -> Result<i64, String> {
    notes::create_text(&db, &title, &body, &category).map_err(map_err)
}

#[tauri::command]
pub fn update_note(
    db: State<'_, DbHandle>,
    id: i64,
    title: String,
    body: String,
    category: String,
) -> Result<(), String> {
    notes::update(&db, id, &title, &body, &category).map_err(map_err)
}

#[tauri::command]
pub fn delete_note(db: State<'_, DbHandle>, id: i64) -> Result<(), String> {
    notes::delete(&db, id).map_err(map_err)
}

#[tauri::command]
pub fn clear_notes(db: State<'_, DbHandle>) -> Result<(), String> {
    notes::clear_all(&db).map_err(map_err)
}

/// Paste a note. Honours the `paste.plain_text_only` setting in the same
/// way `paste_entry` does: HTML / RTF notes get downgraded to their
/// plain-text preview when the toggle is on. Image / Files notes paste
/// as-is regardless.
#[tauri::command]
pub fn paste_note(
    app: AppHandle,
    db: State<'_, DbHandle>,
    watcher: State<'_, WatcherState>,
    id: i64,
) -> Result<(), String> {
    require_accessibility()?;
    let note = notes::get(&db, id)
        .map_err(map_err)?
        .ok_or_else(|| "note not found".to_string())?;

    let plain_only = settings::get_bool(&db, KEY_PLAIN_TEXT_ONLY, true).unwrap_or(true);

    hotkey::hide_popup(&app);
    if plain_only
        && matches!(
            note.content_type,
            crate::models::ContentType::Html | crate::models::ContentType::Rtf
        )
    {
        watcher.mark_self_write(crate::models::ContentType::Text, &note.content_text);
        paste::paste_text(&note.content_text).map_err(map_err)
    } else {
        watcher.mark_self_write(note.content_type, &note.content_data);
        paste::paste_payload(note.content_type, &note.content_data, &note.content_text)
            .map_err(map_err)
    }
}

/// Force-format paste for notes — bypasses the plain-text setting and
/// uses the note's original content type. Mirrors `paste_entry_formatted`
/// for symmetry; expose to the frontend if a Shift+click override on
/// the Notes-tab Paste button is wanted in a future iteration.
#[tauri::command]
pub fn paste_note_formatted(
    app: AppHandle,
    db: State<'_, DbHandle>,
    watcher: State<'_, WatcherState>,
    id: i64,
) -> Result<(), String> {
    require_accessibility()?;
    let note = notes::get(&db, id)
        .map_err(map_err)?
        .ok_or_else(|| "note not found".to_string())?;

    hotkey::hide_popup(&app);
    watcher.mark_self_write(note.content_type, &note.content_data);
    paste::paste_payload(note.content_type, &note.content_data, &note.content_text)
        .map_err(map_err)
}

// ── Backup (full app export / import) ────────────────────────────────────────

/// Build a backup JSON document. Each section (history / snippets /
/// notes) is included only if the corresponding flag is `true` — lets
/// the user opt out of, say, exporting their clipboard history when
/// sharing snippets with a colleague. Defaults to *all true* if invoked
/// without the flags (legacy callers).
#[tauri::command]
pub fn export_backup(
    db: State<'_, DbHandle>,
    include_history: Option<bool>,
    include_snippets: Option<bool>,
    include_notes: Option<bool>,
) -> Result<String, String> {
    let opts = backup::ExportOptions {
        include_history: include_history.unwrap_or(true),
        include_snippets: include_snippets.unwrap_or(true),
        include_notes: include_notes.unwrap_or(true),
    };
    backup::export_json(&db, opts).map_err(map_err)
}

/// Convenience: build the backup JSON and write it directly to `path`.
/// Returns the number of bytes written. Same selective semantics as
/// `export_backup`.
#[tauri::command]
pub fn save_backup_to_file(
    db: State<'_, DbHandle>,
    path: String,
    include_history: Option<bool>,
    include_snippets: Option<bool>,
    include_notes: Option<bool>,
) -> Result<usize, String> {
    let opts = backup::ExportOptions {
        include_history: include_history.unwrap_or(true),
        include_snippets: include_snippets.unwrap_or(true),
        include_notes: include_notes.unwrap_or(true),
    };
    let json = backup::export_json(&db, opts).map_err(map_err)?;
    std::fs::write(&path, &json).map_err(|e| format!("write {path}: {e}"))?;
    Ok(json.len())
}

/// Read a backup JSON file from `path` and merge it into the live database
/// (snippets upsert by abbreviation, history dedupes by hash, notes are
/// appended).
#[tauri::command]
pub fn import_backup(
    db: State<'_, DbHandle>,
    path: String,
) -> Result<BackupImportResult, String> {
    let json = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {path}: {e}"))?;
    backup::import_json(&db, &json).map_err(map_err)
}

// ── Text expander ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ExpanderConfig {
    pub enabled: bool,
    pub hotkey: String,
    /// Whether the OS-level synthetic-input permission is granted.
    /// macOS: Accessibility. Other OSes: always `true`.
    pub accessibility_granted: bool,
}

/// Read the expander config from the settings table, applying defaults
/// for any missing key. Used by the frontend on Settings panel mount.
#[tauri::command]
pub fn get_expander_config(db: State<'_, DbHandle>) -> Result<ExpanderConfig, String> {
    let enabled = settings::get_bool(&db, expander::KEY_ENABLED, false).map_err(map_err)?;
    let hotkey = settings::get_or(&db, expander::KEY_HOTKEY, expander::DEFAULT_HOTKEY)
        .map_err(map_err)?;
    Ok(ExpanderConfig {
        enabled,
        hotkey,
        accessibility_granted: expander::accessibility_granted(),
    })
}

/// Probe whether Inspector Rust currently has Accessibility access. Cheap; safe
/// to call repeatedly (e.g. polling from the Settings panel after the
/// user grants in System Settings).
#[tauri::command]
pub fn get_accessibility_status() -> bool {
    expander::accessibility_granted()
}

/// Trigger the macOS "would like to control this computer" dialog and
/// add Inspector Rust to the Accessibility list. Returns the (still-likely-false)
/// trusted status immediately after the prompt fires.
#[tauri::command]
pub fn request_accessibility_grant() -> bool {
    expander::request_accessibility_grant()
}

/// Open the System Settings → Privacy & Security → Accessibility pane
/// (macOS only). On other OSes this is a no-op.
#[tauri::command]
pub fn open_accessibility_settings() -> Result<(), String> {
    expander::open_accessibility_settings().map_err(map_err)
}

/// Wipe stale TCC entries for Inspector Rust and fire the system Accessibility
/// prompt with the current cdhash. Used when the user has the toggle
/// "on" in System Settings but the running process still sees itself as
/// untrusted (the typical "stale grant from a previous build" state).
#[tauri::command]
pub fn force_reset_and_request_grant() -> Result<bool, String> {
    expander::force_reset_and_request_grant().map_err(map_err)
}

// ── Screen Recording (macOS TCC ScreenCapture policy) ─────────────────────

/// Whether Inspector Rust currently has the Screen Recording grant. Cheap;
/// safe to poll from the Settings panel after the user grants it.
/// Always `true` on non-macOS (no equivalent permission gate).
#[tauri::command]
pub fn get_screen_recording_status() -> bool {
    screen_recording::screen_recording_granted()
}

/// Trigger the macOS Screen Recording prompt. Returns the (still-likely-
/// false) status immediately. The user usually has to relaunch Inspector Rust
/// after granting because macOS caches the TCC verdict per-process.
#[tauri::command]
pub fn request_screen_recording_grant() -> bool {
    screen_recording::request_screen_recording_grant()
}

/// Open System Settings → Privacy & Security → Screen Recording.
#[tauri::command]
pub fn open_screen_recording_settings() -> Result<(), String> {
    screen_recording::open_screen_recording_settings().map_err(map_err)
}

/// Reset the Screen Recording TCC entry for Inspector Rust (no sudo needed
/// for the user's own bundle id) and re-fire the prompt. Mirror of
/// `force_reset_and_request_grant` but for the screencapture policy.
#[tauri::command]
pub fn force_reset_screen_recording_grant() -> bool {
    let _ = std::process::Command::new("tccutil")
        .args(["reset", "ScreenCapture", "io.celox.inspector-rust"])
        .status();
    screen_recording::request_screen_recording_grant()
}

// ── Automation→Finder (macOS TCC AppleEvents policy) ──────────────────

/// Whether Inspector Rust can read the Finder selection (TCC Automation
/// → Finder grant). Probes by sending a trivial `tell application "Finder"
/// to return name` and checking for the errno -1743 "not permitted"
/// reply. *Important:* the first probe ever made after install triggers
/// the macOS Automation prompt — there's no separate "not determined"
/// state in the TCC AppleEvents policy. We accept that: the prompt copy
/// (NSAppleEventsUsageDescription in Info.plist) explains the request,
/// and once the user grants it the check is silent every time after.
///
/// Always `true` on non-macOS (no equivalent permission).
#[tauri::command]
pub fn get_finder_automation_status() -> bool {
    #[cfg(target_os = "macos")]
    {
        // Match `finder_selection::read` — re-use it so the probe goes
        // through the exact same code path the feature does. An empty
        // selection counts as success. The `finder.automation_denied`
        // sentinel is what we treat as "not granted".
        match crate::finder_selection::read() {
            Ok(_) => true,
            Err(e) => e != crate::finder_selection::ERR_AUTOMATION_DENIED,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Open System Settings → Privacy & Security → Automation, where the
/// user grants per-app Apple-Events automation. macOS deep-link URL
/// scheme has stayed compatible from Catalina through Sonoma.
#[tauri::command]
pub fn open_finder_automation_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Automation")
            .status()
            .map_err(map_err)?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("only macOS has the Automation permission".into())
    }
}

/// Reset the Automation→Finder TCC entry and re-fire the prompt. The
/// `AppleEvents` service in TCC keys both ends of the pair; a single
/// reset by bundle id wipes our entry on every target app (currently
/// only Finder).
#[tauri::command]
pub fn force_reset_finder_automation_grant() -> bool {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("tccutil")
            .args(["reset", "AppleEvents", "io.celox.inspector-rust"])
            .status();
        // Re-probe to fire the prompt; result is ignored — the caller
        // polls `get_finder_automation_status` on a 1 s tick anyway.
        get_finder_automation_status()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Quit the running app process. Intended for the Settings panel's
/// "Quit Inspector Rust" button after the user grants Accessibility — macOS
/// caches `AXIsProcessTrusted()` per-process, so a freshly granted app
/// stays "untrusted" until restarted.
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

/// Relaunch Inspector Rust by spawning a fresh instance of the installed `.app`
/// and exiting the current process. Used by the Settings panel's auto-
/// restart prompt after the user grants Accessibility — `open` returns
/// immediately, the new Inspector Rust process inherits the just-granted TCC
/// state, and the old process exits cleanly.
///
/// macOS-only meaningful behaviour. On other platforms it just exits.
#[tauri::command]
pub fn relaunch_app(app: AppHandle) {
    #[cfg(target_os = "macos")]
    {
        // Detach `open` so the spawned process is fully owned by launchd —
        // not by us — and survives the `app.exit(0)` that follows.
        let _ = std::process::Command::new("open")
            .arg("-n") // -n: open a new instance even if one is already running
            .arg("/Applications/InspectorRust.app")
            .spawn();
        // Tiny delay so `open` has a chance to actually fork before we exit.
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
    app.exit(0);
}

// ── Autostart (login item / LaunchAgent) ────────────────────────────────────

/// Whether Inspector Rust is set to launch automatically on login. On macOS this
/// checks for `~/Library/LaunchAgents/InspectorRust.plist`; on Windows it
/// checks the run-key registry entry. Both go through the
/// `tauri-plugin-autostart` `AutoLaunchManager`.
#[tauri::command]
pub fn get_autostart_enabled(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

/// Enable or disable autostart. Returns the *now-effective* state (read
/// back from the OS) so the caller can reconcile its UI with reality
/// without a separate round-trip. Emits the `autostart-changed` event so
/// the tray menu and any other listeners (the Settings panel itself
/// re-renders on the IPC result) stay in sync.
#[tauri::command]
pub fn set_autostart_enabled(app: AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    let am = app.autolaunch();
    let res = if enabled { am.enable() } else { am.disable() };
    res.map_err(|e| e.to_string())?;
    let now = am.is_enabled().map_err(|e| e.to_string())?;
    let _ = app.emit("autostart-changed", now);
    Ok(now)
}

/// Show the system-wide screen eyedropper. Returns immediately;
/// the picked hex (or `null` on cancel) is delivered later via the
/// Tauri event `"color-picked"`.
///
/// - macOS uses Apple's `NSColorSampler` (10.15+) — must run on the
///   main thread, dispatched via `app.run_on_main_thread`.
/// - Windows spawns a worker thread that puts up a fullscreen layered
///   overlay and reads the pixel under the cursor on click.
///
/// Hides the popup window before sampling and re-shows it on result —
/// the popup is `alwaysOnTop`, so without hiding it the user can't
/// sample any area covered by it (NSColorSampler reads live screen
/// pixels including the popup's).
#[tauri::command]
pub fn pick_screen_color(app: AppHandle) -> Result<(), String> {
    use tauri::Manager;

    // The popup is `alwaysOnTop`. NSColorSampler renders its loupe at
    // a window level just BELOW alwaysOnTop on macOS Tahoe — leaving
    // the popup visible obscures the loupe entirely, so the user can't
    // see what they're sampling. Hide the popup before showing the
    // sampler; it gets re-shown by `clear_pick_suppress_hide` once the
    // user clicks (or cancels).
    if let Some(ui) = app.try_state::<UiState>() {
        ui.suppress_hide.store(true, Ordering::Relaxed);
    }
    if let Some(w) = app.get_webview_window(crate::hotkey::POPUP_LABEL) {
        // Same multi-screen fix as run_eyedropper_pipeline — park the
        // popup on the cursor's monitor before hiding so the
        // NSColorSampler loupe appears on the right display in
        // multi-monitor setups.
        crate::hotkey::park_on_cursor_monitor(&w);
        let _ = w.hide();
    }

    #[cfg(target_os = "macos")]
    {
        let app_inner = app.clone();
        app.run_on_main_thread(move || {
            let app_for_event = app_inner.clone();
            let app_for_restore = app_inner.clone();
            if let Err(e) = crate::screen_picker::pick_color_async(move |hex| {
                let _ = app_for_event.emit("color-picked", hex);
                clear_pick_suppress_hide(&app_for_event);
            }) {
                tracing::warn!("pick_screen_color: pick_color_async err: {e}");
                let _ = app_inner.emit("color-picked", Option::<String>::None);
                clear_pick_suppress_hide(&app_for_restore);
            }
        })
        .map_err(map_err)?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let app_for_thread = app.clone();
        std::thread::spawn(move || {
            let result = crate::screen_picker::pick_color_blocking().ok();
            let _ = app_for_thread.emit("color-picked", result);
            clear_pick_suppress_hide(&app_for_thread);
        });
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        clear_pick_suppress_hide(&app);
        Err("screen color picker not implemented on this platform".to_string())
    }
}

/// Restores the popup-and-modal state after a screen-pick finishes.
///
/// Sequencing here is delicate. The naïve order — show window, demote
/// activation policy, clear suppress-hide — caused the popup to vanish
/// the instant the policy demote ran on macOS Tahoe (the demote
/// dispatched a focus-loss event, the focus handler ran with the
/// suppress-hide flag *just* cleared, and called `hide_popup` before
/// the user saw the result).
///
/// Fix: defer the suppress-hide clear *and* the policy demote to a
/// background thread that sleeps long enough for the focus events
/// from the show / set_focus calls to drain. The popup stays visible,
/// the user sees the picked color, and the Dock icon disappears half
/// a second later.
fn clear_pick_suppress_hide(app: &AppHandle) {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window(crate::hotkey::POPUP_LABEL) {
        let _ = w.show();
        let _ = w.set_focus();
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Some(ui) = app2.try_state::<UiState>() {
            ui.suppress_hide.store(false, Ordering::Relaxed);
        }
        #[cfg(target_os = "macos")]
        {
            // Demote on the main thread — AppKit policy changes are
            // expected from the main run loop.
            let _ = app2.run_on_main_thread(|| {
                crate::screen_picker::demote_to_accessory();
            });
        }
    });
}

/// Persist a new expander config and re-register the global hotkey.
/// Returns the (now-effective) config so the frontend can confirm what
/// actually got applied — if the hotkey string was malformed, the function
/// errors *before* writing settings, leaving the previous registration in
/// place.
#[tauri::command]
pub fn set_expander_config(
    app: AppHandle,
    db: State<'_, DbHandle>,
    state: State<'_, ExpanderShortcutState>,
    enabled: bool,
    hotkey: String,
) -> Result<ExpanderConfig, String> {
    // Re-register first — if the hotkey is invalid, this fails and we
    // don't touch the persisted settings.
    hotkey::register_expander(&app, &state, &hotkey, enabled).map_err(map_err)?;

    settings::set(&db, expander::KEY_HOTKEY, &hotkey).map_err(map_err)?;
    settings::set(
        &db,
        expander::KEY_ENABLED,
        if enabled { "true" } else { "false" },
    )
    .map_err(map_err)?;

    Ok(ExpanderConfig {
        enabled,
        hotkey,
        accessibility_granted: expander::accessibility_granted(),
    })
}

/// Trigger an expand-at-cursor cycle programmatically (no hotkey press).
/// Hides the popup first so the synthetic Cmd+Shift+← / Cmd+C / Cmd+V
/// land in the previously focused app instead of Inspector Rust itself.
///
/// Dispatches the enigo work to the **main thread** because enigo's macOS
/// `Key::Unicode(...)` mapping uses TSM (Text Services Manager) which
/// asserts main-thread, and dies with EXC_BREAKPOINT otherwise.
#[tauri::command]
pub fn trigger_expand_at_cursor(app: AppHandle) -> Result<(), String> {
    hotkey::hide_popup(&app);
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        // Give macOS a moment to hand key focus back to the prior app
        // before we start synthesizing keystrokes.
        std::thread::sleep(std::time::Duration::from_millis(250));
        if let Some(db) = app2.try_state::<DbHandle>() {
            if let Err(e) = expander::expand_at_cursor(&db) {
                tracing::warn!("expand_at_cursor failed: {e:#}");
            }
        }
    })
    .map_err(|e| format!("dispatch to main thread: {e}"))?;
    Ok(())
}

/// Diagnose the capture half of expansion: select previous word, copy,
/// look up — but **don't paste**. Returns what was captured and whether
/// any snippet matches. Used by the Settings panel's "Test now" button.
///
/// Same main-thread requirement as `trigger_expand_at_cursor`. Uses a
/// blocking `mpsc` to ferry the result back from the main-thread closure
/// to the IPC handler thread.
#[tauri::command]
pub fn diagnose_expand_at_cursor(
    app: AppHandle,
) -> Result<expander::DiagnoseResult, String> {
    hotkey::hide_popup(&app);
    let app2 = app.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let result = match app2.try_state::<DbHandle>() {
            Some(db) => expander::diagnose_at_cursor(&db).map_err(|e| e.to_string()),
            None => Err("db state not initialized".to_string()),
        };
        let _ = tx.send(result);
    })
    .map_err(|e| format!("dispatch to main thread: {e}"))?;
    rx.recv()
        .map_err(|e| format!("main thread didn't reply: {e}"))?
}

// ── Direct hotkey → snippet slots ───────────────────────────────────────────

/// A direct slot with the bound snippet's display info resolved. `None`
/// abbreviation/title means the snippet has since been deleted (the slot
/// is dangling — pressing the hotkey does nothing; the UI shows it so the
/// user can rebind or remove it).
#[derive(Debug, Serialize)]
pub struct DirectSlotView {
    pub hotkey: String,
    pub snippet_id: i64,
    pub abbreviation: Option<String>,
    pub title: Option<String>,
}

fn resolve_slots(db: &DbHandle, slots: &[expander::DirectSlot]) -> Vec<DirectSlotView> {
    slots
        .iter()
        .map(|s| {
            let snip = snippets::get_by_id(db, s.snippet_id).ok().flatten();
            DirectSlotView {
                hotkey: s.hotkey.clone(),
                snippet_id: s.snippet_id,
                abbreviation: snip.as_ref().map(|x| x.abbreviation.clone()),
                title: snip.as_ref().map(|x| x.title.clone()),
            }
        })
        .collect()
}

#[tauri::command]
pub fn get_direct_slots(db: State<'_, DbHandle>) -> Result<Vec<DirectSlotView>, String> {
    let slots = expander::get_direct_slots(&db).map_err(map_err)?;
    Ok(resolve_slots(&db, &slots))
}

/// Replace the direct-slot list: validate snippet ids, (re-)register the
/// global shortcuts (this rejects collisions with the popup / OCR /
/// abbreviation hotkeys and duplicates), then persist. Returns the
/// re-resolved list. Nothing is persisted if registration fails.
#[tauri::command]
pub fn set_direct_slots(
    app: AppHandle,
    db: State<'_, DbHandle>,
    state: State<'_, ExpanderShortcutState>,
    slots: Vec<expander::DirectSlot>,
) -> Result<Vec<DirectSlotView>, String> {
    let parsed: Vec<expander::DirectSlot> = slots
        .into_iter()
        .map(|s| expander::DirectSlot {
            hotkey: s.hotkey.trim().to_string(),
            snippet_id: s.snippet_id,
        })
        .collect();
    for s in &parsed {
        if snippets::get_by_id(&db, s.snippet_id).map_err(map_err)?.is_none() {
            return Err(format!("snippet id {} no longer exists", s.snippet_id));
        }
    }
    hotkey::register_direct_slots(&app, &state, &parsed).map_err(map_err)?;
    expander::set_direct_slots(&db, &parsed).map_err(map_err)?;
    Ok(resolve_slots(&db, &parsed))
}

// ── Image recolor ───────────────────────────────────────────────────────────

fn parse_hex_rgb(hex: &str) -> Result<(u8, u8, u8), String> {
    let s = hex.trim().trim_start_matches('#');
    if s.len() != 6 {
        return Err(format!("hex must be 6 chars, got {:?}", hex));
    }
    let r = u8::from_str_radix(&s[0..2], 16).map_err(|e| format!("invalid red: {e}"))?;
    let g = u8::from_str_radix(&s[2..4], 16).map_err(|e| format!("invalid green: {e}"))?;
    let b = u8::from_str_radix(&s[4..6], 16).map_err(|e| format!("invalid blue: {e}"))?;
    Ok((r, g, b))
}

/// Tint an image entry to `hex` and store the result as a new history
/// entry. The original is left untouched so the user can recover it.
/// Emits `clipboard-changed` to refresh the popup list.
#[tauri::command]
pub fn recolor_image_entry(
    app: AppHandle,
    db: State<'_, DbHandle>,
    id: i64,
    hex: String,
) -> Result<i64, String> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};

    let (r, g, b) = parse_hex_rgb(&hex)?;
    let entry = db::get(&db, id)
        .map_err(map_err)?
        .ok_or_else(|| "entry not found".to_string())?;
    if !matches!(entry.content_type, crate::models::ContentType::Image) {
        return Err("entry is not an image".to_string());
    }

    let png_bytes = B64
        .decode(entry.content_data.as_bytes())
        .map_err(|e| format!("base64 decode: {e}"))?;
    let recolored = recolor::recolor_png(&png_bytes, r, g, b).map_err(map_err)?;
    let b64 = B64.encode(&recolored);
    let byte_size = recolored.len() as i64;

    // Use the brightness/dimensions plus the chosen tint as the
    // human-readable preview line. Keeps it visually distinct from the
    // source entry in the history list.
    let summary = format!("[image · tinted #{}]", hex.trim_start_matches('#').to_uppercase());

    let new_id = db::upsert_clip(
        &db,
        &crate::models::NewClip {
            content_type: crate::models::ContentType::Image,
            content_text: summary,
            content_data: b64,
            byte_size,
        },
    )
    .map_err(map_err)?;

    // Refresh the list so the new entry surfaces at the top.
    let _ = app.emit("clipboard-changed", ());
    Ok(new_id)
}

/// Sample-based "is this image mostly grayscale?" probe. Returned value
/// is in [0, 1] — frontend treats anything below ~0.1 as "looks
/// monochrome, recolor button worth showing".
#[tauri::command]
pub fn image_chromaticity(
    db: State<'_, DbHandle>,
    id: i64,
) -> Result<f32, String> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};

    let entry = db::get(&db, id)
        .map_err(map_err)?
        .ok_or_else(|| "entry not found".to_string())?;
    if !matches!(entry.content_type, crate::models::ContentType::Image) {
        return Err("entry is not an image".to_string());
    }
    let png_bytes = B64
        .decode(entry.content_data.as_bytes())
        .map_err(|e| format!("base64 decode: {e}"))?;
    recolor::max_chromaticity_sample(&png_bytes, 4096).map_err(map_err)
}

/// Result returned to the frontend after an OCR run. `text` is empty
/// when the user cancelled (`cancelled = true`) or when Vision found no
/// text in the region — the UI uses the boolean to differentiate "user
/// pressed Esc" from "no text detected" so a toast can be skipped in
/// the cancel case.
#[derive(serde::Serialize)]
pub struct OcrResult {
    pub text: String,
    pub cancelled: bool,
    /// Length in characters — handy for a frontend "Recognized 142
    /// chars" toast without re-measuring on the JS side.
    pub chars: usize,
}

/// Run the OCR pipeline: hide popup → interactive region capture →
/// OCR → write to clipboard → add to history. Shared between the IPC
/// command (tray "OCR region…", future button) and the global
/// shortcut handler.
///
/// Blocks for the duration of the screencapture (user-driven) plus the
/// Vision call (~50–500 ms depending on region size). Always invoke
/// from a worker thread so the IPC handler thread / shortcut callback
/// thread doesn't stall.
pub fn run_ocr_pipeline(app: &AppHandle) -> Result<OcrResult, String> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use clipboard_rs::{Clipboard, ClipboardContext};

    // Pre-check Screen Recording. Without it, `screencapture -i`
    // returns 0 + an empty file on recent macOS versions — the user
    // sees the marquee never appear and has no error to act on.
    // Returning the sentinel here lets the JS side surface a clear
    // "grant Screen Recording" toast and a button into the right
    // System Settings pane.
    if !screen_recording::screen_recording_granted() {
        return Err(ERR_NO_SCREEN_RECORDING.to_string());
    }

    // Hide the popup so the screencapture overlay shows over the
    // *previously* focused window — same UX as Cmd+Shift+4.
    hotkey::hide_popup(app);

    let png_bytes = match region_picker::capture() {
        Ok(b) => b,
        Err(e) => {
            // Distinguish "user cancelled" from a real error.
            if e.downcast_ref::<region_picker::Cancelled>().is_some() {
                return Ok(OcrResult { text: String::new(), cancelled: true, chars: 0 });
            }
            return Err(format!("region capture failed: {e:#}"));
        }
    };

    let text = ocr::recognize(&png_bytes).map_err(|e| format!("ocr failed: {e:#}"))?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(OcrResult { text: String::new(), cancelled: false, chars: 0 });
    }

    // Write to system clipboard. Mark first so the watcher doesn't
    // double-capture this as a fresh user-initiated copy.
    if let Some(watcher) = app.try_state::<WatcherState>() {
        watcher.mark_self_write(crate::models::ContentType::Text, trimmed);
    }
    let ctx = ClipboardContext::new()
        .map_err(|e| format!("clipboard ctx init: {e:?}"))?;
    ctx.set_text(trimmed.to_string())
        .map_err(|e| format!("set_text: {e:?}"))?;

    // Persist the source PNG FIRST (when the setting is opted in) so
    // the recognised text gets the later `last_used_at` timestamp and
    // ends up at the top of the history list. By default the PNG is
    // skipped — keeps the history list focused on the *text* the user
    // actually wanted, instead of doubling up with a screenshot they
    // can't paste back into a text field. Toggle this back on via
    // Settings → Capture → "Keep OCR source image in history".
    if let Some(db) = app.try_state::<DbHandle>() {
        let save_source = settings::get_bool(&db, KEY_OCR_SAVE_SOURCE, false).unwrap_or(false);
        if save_source {
            let b64 = B64.encode(&png_bytes);
            let summary = format!("[ocr source · {} B]", png_bytes.len());
            let byte_size = png_bytes.len() as i64;
            let _ = db::upsert_clip(
                &db,
                &crate::models::NewClip {
                    content_type: crate::models::ContentType::Image,
                    content_text: summary,
                    content_data: b64,
                    byte_size,
                },
            );
        }
        // Then the recognised text — this becomes the most-recent
        // entry, matching what's on the clipboard and what Enter will
        // paste.
        let _ = db::upsert_clip(
            &db,
            &crate::models::NewClip {
                content_type: crate::models::ContentType::Text,
                content_text: trimmed.to_string(),
                content_data: trimmed.to_string(),
                byte_size: trimmed.len() as i64,
            },
        );
    }
    let _ = app.emit("clipboard-changed", ());

    let chars = trimmed.chars().count();
    Ok(OcrResult { text: trimmed.to_string(), cancelled: false, chars })
}

/// IPC entry point — the menu / button caller. Dispatched to a thread
/// so the screencapture wait doesn't block the IPC main thread.
#[tauri::command]
pub fn ocr_region(app: AppHandle) -> Result<OcrResult, String> {
    // Run synchronously here. The Tauri IPC layer already gives us a
    // worker thread, so wrapping in std::thread::spawn would just add
    // hand-off overhead. Worst case the JS promise sits open for 5–30 s
    // while the user drags the marquee.
    run_ocr_pipeline(&app)
}

/// Result of a screenshot region capture. `cancelled` distinguishes
/// "user pressed Esc" from "captured N bytes" — the UI skips the
/// "saved to clipboard" toast in the cancel case.
#[derive(serde::Serialize)]
pub struct ScreenshotResult {
    pub cancelled: bool,
    /// PNG payload size in bytes — for a frontend "Captured 12.3 KB"
    /// toast without re-measuring on the JS side.
    pub bytes: usize,
}

/// Run the screenshot pipeline: hide popup → interactive region
/// capture → write PNG to a temp file → spawn the floating preview
/// window on the cursor's monitor (bottom-left, CleanShot-X style).
/// The user chooses Save / Discard / Edit from the preview, which
/// runs the appropriate IPC (`screenshot_preview_*`). Until then NO
/// clipboard write, NO Downloads file, NO history entry — so
/// discarding is a true discard.
///
/// Shared between the IPC command (tray "Screenshot Region") and the
/// global shortcut handler. Blocks for the duration of the
/// screencapture (user-driven) — always invoke from a worker thread.
pub fn run_screenshot_pipeline(app: &AppHandle) -> Result<ScreenshotResult, String> {
    if !screen_recording::screen_recording_granted() {
        return Err(ERR_NO_SCREEN_RECORDING.to_string());
    }

    hotkey::hide_popup(app);

    let png_bytes = match region_picker::capture() {
        Ok(b) => b,
        Err(e) => {
            if e.downcast_ref::<region_picker::Cancelled>().is_some() {
                return Ok(ScreenshotResult { cancelled: true, bytes: 0 });
            }
            return Err(format!("region capture failed: {e:#}"));
        }
    };

    // Stage the PNG to the OS cache dir under a timestamped name. The
    // preview window reads it via a `convertFileSrc`-style URL; the
    // Save / Discard / Edit IPCs move or delete it.
    let cache = dirs::cache_dir()
        .map(|d| d.join("InspectorRust"))
        .ok_or_else(|| "no cache dir on this system".to_string())?;
    std::fs::create_dir_all(&cache)
        .map_err(|e| format!("create cache dir {}: {e}", cache.display()))?;
    let temp_path = cache.join(format!(
        "screenshot-pending-{}.png",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ));
    std::fs::write(&temp_path, &png_bytes)
        .map_err(|e| format!("write temp screenshot {}: {e}", temp_path.display()))?;

    // ── Auto-clipboard ────────────────────────────────────────────────
    // Write the captured PNG to the system clipboard IMMEDIATELY,
    // before showing the preview. The user wanted the screenshot
    // ready to paste right away — the preview's Save / Discard /
    // Edit just decides what *else* happens to it (on-disk file +
    // history). `mark_self_write` keeps the clipboard watcher from
    // capturing this as a separate clipboard event.
    {
        use base64::{engine::general_purpose::STANDARD as B64, Engine};
        use clipboard_rs::{
            common::RustImage, Clipboard, ClipboardContext, RustImageData,
        };
        let b64 = B64.encode(&png_bytes);
        if let Some(watcher) = app.try_state::<WatcherState>() {
            watcher.mark_self_write(crate::models::ContentType::Image, &b64);
        }
        match ClipboardContext::new() {
            Ok(ctx) => match RustImageData::from_bytes(&png_bytes) {
                Ok(img) => {
                    if let Err(e) = ctx.set_image(img) {
                        tracing::warn!("auto-clipboard set_image: {e:?}");
                    }
                }
                Err(e) => tracing::warn!("auto-clipboard decode png: {e:?}"),
            },
            Err(e) => tracing::warn!("auto-clipboard ctx init: {e:?}"),
        }
    }

    // Stash the path in shared state so the preview-window IPCs can
    // pick it up without the frontend round-tripping a filesystem path.
    if let Some(pending) = app.try_state::<crate::screenshot_preview::PendingScreenshot>() {
        *pending.inner().0.lock() = Some(temp_path.clone());
    } else {
        tracing::warn!("PendingScreenshot state missing — preview won't work");
    }

    // Build (or reuse) and position the preview window. Failure isn't
    // fatal — the temp PNG is still on disk and the user can rerun.
    if let Err(e) = crate::screenshot_preview::show_preview(app) {
        tracing::warn!("screenshot preview window: {e:#}");
    }

    let _ = app.emit("clipboard-changed", ());

    Ok(ScreenshotResult { cancelled: false, bytes: png_bytes.len() })
}

/// IPC entry point. Same threading note as `ocr_region` — the Tauri
/// IPC layer already provides a worker thread.
#[tauri::command]
pub fn screenshot_region(app: AppHandle) -> Result<ScreenshotResult, String> {
    run_screenshot_pipeline(&app)
}

/// Run the eyedropper pipeline: hide popup → fire screen color picker
/// (macOS NSColorSampler loupe / Windows GDI overlay) → write the
/// picked hex string (`#RRGGBB`) to the system clipboard and add it
/// as a Text history entry. Used by the `Ctrl+Shift+C` global shortcut
/// and the tray's *Pick Color* menu item.
///
/// Distinct from `pick_screen_color` (which is the popup-modal entry
/// point and re-shows the popup with the picked color in the modal).
/// This pipeline is fire-and-forget — no popup, no modal, just the
/// hex on your clipboard, parallel to the OCR + screenshot global
/// shortcut UX.
pub fn run_eyedropper_pipeline(app: &AppHandle) {
    use tauri::Manager;

    // The popup is `alwaysOnTop`; hide it before showing the loupe so
    // the loupe sits on top and the user can sample anywhere on screen.
    if let Some(ui) = app.try_state::<UiState>() {
        ui.suppress_hide.store(true, Ordering::Relaxed);
    }
    if let Some(w) = app.get_webview_window(crate::hotkey::POPUP_LABEL) {
        // Multi-screen fix: park the popup on the cursor's monitor BEFORE
        // hiding it. When NSColorSampler shows its loupe, macOS positions
        // it on the calling app's *primary* screen — and that primary
        // screen is decided by where the app's last-active window was.
        // Without this park step, the loupe always appears on the main
        // display, even if the user moved the cursor to a secondary one.
        crate::hotkey::park_on_cursor_monitor(&w);
        let _ = w.hide();
    }

    #[cfg(target_os = "macos")]
    {
        let app_inner = app.clone();
        let _ = app.run_on_main_thread(move || {
            let app_for_cb = app_inner.clone();
            let app_for_err = app_inner.clone();
            if let Err(e) = crate::screen_picker::pick_color_async(move |hex_opt| {
                if let Some(hex) = hex_opt {
                    write_eyedropper_result(&app_for_cb, &hex);
                }
                clear_eyedropper_no_popup(&app_for_cb);
            }) {
                tracing::warn!("eyedropper pipeline: pick_color_async err: {e}");
                clear_eyedropper_no_popup(&app_for_err);
            }
        });
    }
    #[cfg(target_os = "windows")]
    {
        let app_for_thread = app.clone();
        std::thread::spawn(move || {
            if let Ok(hex) = crate::screen_picker::pick_color_blocking() {
                write_eyedropper_result(&app_for_thread, &hex);
            }
            clear_eyedropper_no_popup(&app_for_thread);
        });
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        clear_eyedropper_no_popup(app);
    }
}

/// IPC entry point for the eyedropper. Tray + frontend button alternative
/// to the `Ctrl+Shift+C` global shortcut. Returns immediately; the actual
/// pick is async (macOS) or runs on a worker thread (Windows).
#[tauri::command]
pub fn eyedropper_to_clipboard(app: AppHandle) -> Result<(), String> {
    run_eyedropper_pipeline(&app);
    Ok(())
}

// ── Finder selection (macOS) ──────────────────────────────────────────

/// One Finder-selected item — path + display name + size + image-ness.
/// `is_image` is a cheap extension test (`png`/`jpg`/`jpeg`/`webp`/`gif`/`bmp`/`heic`/`tiff`);
/// good enough to decide whether to surface the resize action.
#[derive(serde::Serialize, Clone)]
pub struct FinderItem {
    pub path: String,
    pub name: String,
    pub size_bytes: Option<u64>,
    pub is_image: bool,
}

fn finder_item_from_path(p: &std::path::Path) -> FinderItem {
    let name = p
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    let size_bytes = std::fs::metadata(p).map(|m| m.len()).ok();
    let is_image = p
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| matches!(
            e.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "heic" | "heif" | "tiff" | "tif"
        ))
        .unwrap_or(false);
    FinderItem {
        path: p.display().to_string(),
        name,
        size_bytes,
        is_image,
    }
}

/// Read the current Finder selection. Returns an empty list when
/// nothing is selected. Errors with the `finder.automation_denied`
/// sentinel when the user hasn't granted Automation→Finder in System
/// Settings (frontend turns that into a tailored banner).
#[tauri::command]
pub fn get_finder_selection() -> Result<Vec<FinderItem>, String> {
    let paths = crate::finder_selection::read()?;
    Ok(paths.iter().map(|p| finder_item_from_path(p)).collect())
}

/// Resize a single image file with Lanczos3, writing the output next
/// to the source as `<stem>-<W>x<H>.<ext>`. Returns the output path
/// so the frontend can show "Saved foo-1200x800.png" toast.
#[tauri::command]
pub fn resize_file(path: String, width: u32, height: u32) -> Result<String, String> {
    let src = std::path::PathBuf::from(&path);
    let r = crate::image_ops::resize_file_to_neighbor(&src, width, height).map_err(map_err)?;
    Ok(r.path.display().to_string())
}

/// Optimise a single PNG file losslessly with oxipng, writing the
/// result next to the source as `<stem>-optim.png`. Returns the output
/// path + before/after byte counts.
#[tauri::command]
pub fn optimize_file(path: String) -> Result<crate::image_ops::OptimResult, String> {
    let src = std::path::PathBuf::from(&path);
    crate::image_ops::optimize_file_to_neighbor(&src).map_err(map_err)
}

/// Runs the hotkey-driven Finder-selection pipeline: read the
/// selection, open the popup, emit the `finder-selection-loaded`
/// event with the items. Mirrors the pattern of the OCR / eyedropper
/// pipelines so the hotkey handler stays tiny.
pub fn run_finder_selection_pipeline(app: &AppHandle) {
    let items_result = crate::finder_selection::read();
    // Show the popup regardless of result — even an Automation-denied
    // error needs a visible surface to display the permission banner.
    let _ = crate::hotkey::show_popup(app);
    match items_result {
        Ok(paths) => {
            let items: Vec<FinderItem> =
                paths.iter().map(|p| finder_item_from_path(p)).collect();
            let _ = app.emit("finder-selection-loaded", items);
        }
        Err(e) => {
            if e == crate::finder_selection::ERR_AUTOMATION_DENIED {
                let _ = app.emit("finder-automation-needed", ());
            } else {
                tracing::warn!("finder selection: {e}");
                let _ = app.emit("finder-selection-loaded", Vec::<FinderItem>::new());
            }
        }
    }
}

// ── Power commands (search-bar shell): rz / optim / rmvvls ────────────

/// `rz <W>x<H>` — resize the clipboard image to the given dimensions
/// (Lanczos3 sampling) and write it back. Also pushes the resized image
/// into history as a new entry so the user can recover it.
#[tauri::command]
pub fn resize_clipboard_image(
    app: AppHandle,
    width: u32,
    height: u32,
) -> Result<crate::image_ops::ResizeResult, String> {
    let res = crate::image_ops::resize_clipboard_image_lanczos(width, height).map_err(map_err)?;
    // Mark the watcher so the round-trip doesn't get double-captured,
    // then push the resized PNG into history as a fresh entry.
    if let Some(_watcher) = app.try_state::<WatcherState>() {
        // The watcher's self-write fuse keys on (content_type, b64).
        // We didn't keep the PNG bytes here; the watcher's own capture
        // would fire on the clipboard set anyway and store it once.
        // No-op on our side.
    }
    let _ = app.emit("clipboard-changed", ());
    Ok(res)
}

/// `optim` — read clipboard image, run through oxipng (lossless), save
/// to `~/Downloads/inspector-rust-optim-<ts>.png`. Does NOT touch the
/// clipboard.
#[tauri::command]
pub fn optimize_clipboard_image() -> Result<crate::image_ops::OptimResult, String> {
    crate::image_ops::optimize_clipboard_png().map_err(map_err)
}

/// `rmvvls <text>` — strip vowels from `text` and write the result to
/// the system clipboard (plus a history entry so the user can find it
/// again). Vowels = a/e/i/o/u + their uppercase + the German umlauts
/// ä/ö/ü/Ä/Ö/Ü. Returns the stripped string for the UI to display.
#[tauri::command]
pub fn remove_vowels_to_clipboard(app: AppHandle, text: String) -> Result<String, String> {
    use clipboard_rs::{Clipboard, ClipboardContext};

    let stripped = strip_vowels(&text);

    if let Some(watcher) = app.try_state::<WatcherState>() {
        watcher.mark_self_write(crate::models::ContentType::Text, &stripped);
    }
    let ctx = ClipboardContext::new().map_err(|e| format!("clipboard ctx: {e:?}"))?;
    ctx.set_text(stripped.clone())
        .map_err(|e| format!("set_text: {e:?}"))?;

    if let Some(db) = app.try_state::<DbHandle>() {
        let _ = db::upsert_clip(
            &db,
            &crate::models::NewClip {
                content_type: crate::models::ContentType::Text,
                content_text: stripped.clone(),
                content_data: stripped.clone(),
                byte_size: stripped.len() as i64,
            },
        );
    }
    let _ = app.emit("clipboard-changed", ());
    Ok(stripped)
}

// ── System commands (kill / reboot / shutdown / lock) ─────────────────

/// List running processes for the `kill` live picker. Sorted by memory
/// usage descending so the picker surfaces heavy apps first.
#[tauri::command]
pub fn list_processes() -> Result<Vec<crate::system_commands::ProcessInfo>, String> {
    crate::system_commands::list_running_processes().map_err(map_err)
}

/// `kill <pid>` — send SIGTERM (graceful) by default, or SIGKILL (force
/// quit) when `force = true`. Requires no special permission for
/// processes owned by the current user.
#[tauri::command]
pub fn kill_process(pid: u32, force: bool) -> Result<(), String> {
    crate::system_commands::kill_process_by_pid(pid, force).map_err(map_err)
}

/// `reboot` — restart the system gracefully via `osascript` → loginwindow.
/// macOS will show its usual "These apps have unsaved changes…" prompt;
/// no sudo required.
#[tauri::command]
pub fn system_reboot() -> Result<(), String> {
    crate::system_commands::system_reboot().map_err(map_err)
}

/// `shutdown` — power down the system gracefully (same graceful path as
/// reboot, just a different Apple Event).
#[tauri::command]
pub fn system_shutdown() -> Result<(), String> {
    crate::system_commands::system_shutdown().map_err(map_err)
}

/// `lock` — lock the screen via `pmset displaysleepnow`. Requires no
/// privilege.
#[tauri::command]
pub fn system_lock() -> Result<(), String> {
    crate::system_commands::system_lock().map_err(map_err)
}

/// Adjust the system output volume by `delta` percentage points
/// (positive = louder, negative = quieter). Returns the new level
/// (0–100). Bound to Shift+↑ / Shift+↓ while the popup is open.
#[tauri::command]
pub fn adjust_volume(delta: i32) -> Result<u8, String> {
    crate::system_commands::adjust_system_volume(delta).map_err(map_err)
}

/// `mute` — toggle the system output mute. Returns the new state
/// (`true` = now muted). macOS-only.
#[tauri::command]
pub fn toggle_mute() -> Result<bool, String> {
    crate::system_commands::toggle_system_mute().map_err(map_err)
}

/// Commit an already-transformed string to the clipboard + History.
/// The string-manipulation transforms (`Cmd/Ctrl+1…9` on a selected
/// text entry) are computed frontend-side in `lib/text-transform.ts`;
/// this is the shared write path — mark self-write so the watcher
/// skips it, set the clipboard, push a Text history entry.
#[tauri::command]
pub fn commit_transformed_text(app: AppHandle, text: String) -> Result<(), String> {
    use clipboard_rs::{Clipboard, ClipboardContext};

    if let Some(watcher) = app.try_state::<WatcherState>() {
        watcher.mark_self_write(crate::models::ContentType::Text, &text);
    }
    let ctx = ClipboardContext::new().map_err(|e| format!("clipboard ctx: {e:?}"))?;
    ctx.set_text(text.clone())
        .map_err(|e| format!("set_text: {e:?}"))?;

    if let Some(db) = app.try_state::<DbHandle>() {
        let _ = db::upsert_clip(
            &db,
            &crate::models::NewClip {
                content_type: crate::models::ContentType::Text,
                content_text: text.clone(),
                content_data: text.clone(),
                byte_size: text.len() as i64,
            },
        );
    }
    let _ = app.emit("clipboard-changed", ());
    Ok(())
}

/// Strip vowels (English aeiou + uppercase + German umlauts) from `s`.
/// Pure function — public so the unit tests can exercise it without
/// going through the IPC + clipboard plumbing.
pub fn strip_vowels(s: &str) -> String {
    s.chars()
        .filter(|c| {
            !matches!(
                c,
                'a' | 'e' | 'i' | 'o' | 'u'
                    | 'A' | 'E' | 'I' | 'O' | 'U'
                    | 'ä' | 'ö' | 'ü'
                    | 'Ä' | 'Ö' | 'Ü'
            )
        })
        .collect()
}

#[cfg(test)]
mod theme_tests {
    use super::normalise_theme;

    #[test]
    fn passes_through_the_three_valid_themes() {
        assert_eq!(normalise_theme("light"), "light");
        assert_eq!(normalise_theme("dark"), "dark");
        assert_eq!(normalise_theme("system"), "system");
    }

    #[test]
    fn collapses_unknown_to_system() {
        // A hand-edited settings DB or a value from a future build must
        // never wedge the UI — anything unrecognised becomes "system".
        assert_eq!(normalise_theme("midnight"), "system");
        assert_eq!(normalise_theme(""), "system");
        assert_eq!(normalise_theme("DARK"), "system"); // case-sensitive
        assert_eq!(normalise_theme("Light"), "system");
        assert_eq!(normalise_theme("  dark  "), "system"); // no trimming
    }

    #[test]
    fn return_value_is_a_static_str_safe_to_store() {
        // Guard: normalise_theme must always return one of the literal
        // whitelist values, never echo the input back.
        for input in ["light", "dark", "system", "garbage", ""] {
            let out = normalise_theme(input);
            assert!(
                matches!(out, "light" | "dark" | "system"),
                "normalise_theme({input:?}) returned {out:?} — not in whitelist",
            );
        }
    }
}

#[cfg(test)]
mod strip_vowels_tests {
    use super::strip_vowels;

    #[test]
    fn removes_english_vowels_lowercase() {
        assert_eq!(strip_vowels("hello world"), "hll wrld");
    }

    #[test]
    fn removes_uppercase_vowels() {
        assert_eq!(strip_vowels("HELLO World"), "HLL Wrld");
    }

    #[test]
    fn removes_german_umlauts() {
        assert_eq!(strip_vowels("hällo wörld"), "hll wrld");
        assert_eq!(strip_vowels("ÄÖÜ"), "");
    }

    #[test]
    fn keeps_y_and_consonants() {
        assert_eq!(strip_vowels("fly by myself"), "fly by myslf");
    }

    #[test]
    fn keeps_whitespace_punctuation_digits() {
        assert_eq!(strip_vowels("a, b! 123 c."), ", b! 123 c.");
    }

    #[test]
    fn handles_empty_string() {
        assert_eq!(strip_vowels(""), "");
    }

    #[test]
    fn handles_string_of_only_vowels() {
        assert_eq!(strip_vowels("aeiouäöüAEIOU"), "");
    }

    #[test]
    fn preserves_emoji_and_non_latin_letters() {
        assert_eq!(strip_vowels("hello 🦀 世界"), "hll 🦀 世界");
    }
}

fn write_eyedropper_result(app: &AppHandle, hex: &str) {
    use clipboard_rs::{Clipboard, ClipboardContext};
    if let Some(watcher) = app.try_state::<WatcherState>() {
        watcher.mark_self_write(crate::models::ContentType::Text, hex);
    }
    if let Ok(ctx) = ClipboardContext::new() {
        let _ = ctx.set_text(hex.to_string());
    }
    if let Some(db) = app.try_state::<DbHandle>() {
        let _ = db::upsert_clip(
            &db,
            &crate::models::NewClip {
                content_type: crate::models::ContentType::Text,
                content_text: hex.to_string(),
                content_data: hex.to_string(),
                byte_size: hex.len() as i64,
            },
        );
    }
    let _ = app.emit("clipboard-changed", ());
}

/// Cleanup variant for the global eyedropper flow — clears the
/// suppress-hide flag + demotes the macOS activation policy back to
/// Accessory, **without** re-showing the popup window. The user
/// invoked the picker from a global hotkey / tray menu; the popup
/// wasn't open before, and re-showing it would be a UX surprise.
/// Mirrors the deferred sequencing of `clear_pick_suppress_hide`.
fn clear_eyedropper_no_popup(app: &AppHandle) {
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Some(ui) = app2.try_state::<UiState>() {
            ui.suppress_hide.store(false, Ordering::Relaxed);
        }
        #[cfg(target_os = "macos")]
        {
            let _ = app2.run_on_main_thread(|| {
                crate::screen_picker::demote_to_accessory();
            });
        }
    });
}

/// Background-remove an image entry via corner-sampled chroma-key, save
/// the resulting transparent PNG to `~/Downloads/inspector-rust-cutout-<ts>.png`,
/// and return the path string. The history entry is left untouched —
/// this is a "save the cutout to a file" action, not a clipboard
/// modification.
#[tauri::command]
pub fn cut_out_image_entry(
    db: State<'_, DbHandle>,
    id: i64,
) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};

    let entry = db::get(&db, id)
        .map_err(map_err)?
        .ok_or_else(|| "entry not found".to_string())?;
    if !matches!(entry.content_type, crate::models::ContentType::Image) {
        return Err("entry is not an image".to_string());
    }
    let png_bytes = B64
        .decode(entry.content_data.as_bytes())
        .map_err(|e| format!("base64 decode: {e}"))?;

    write_cutout(&png_bytes, None)
}

/// Save an image clipboard entry (PNG bytes already in the row) to
/// `~/Downloads/inspector-rust-image-<ts>.png`. Doesn't transform the image
/// in any way — it's the "I want this on disk" companion to cutout /
/// recolor. Particularly useful after a recolor since the new tinted
/// entry only lives in the SQLite history otherwise.
#[tauri::command]
pub fn save_image_entry_to_downloads(
    db: State<'_, DbHandle>,
    id: i64,
) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    use chrono::Local;

    let entry = db::get(&db, id)
        .map_err(map_err)?
        .ok_or_else(|| "entry not found".to_string())?;
    if !matches!(entry.content_type, crate::models::ContentType::Image) {
        return Err("entry is not an image".to_string());
    }
    let png_bytes = B64
        .decode(entry.content_data.as_bytes())
        .map_err(|e| format!("base64 decode: {e}"))?;

    let dir = dirs::download_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| "no Downloads or home directory available".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create downloads dir: {e}"))?;

    let stamp = Local::now().format("%Y%m%d-%H%M%S");
    let filename = format!("inspector-rust-image-{stamp}.png");
    let out_path = dir.join(&filename);
    std::fs::write(&out_path, &png_bytes).map_err(|e| format!("write {filename}: {e}"))?;
    Ok(out_path.to_string_lossy().into_owned())
}

/// Same as `cut_out_image_entry` but for an arbitrary image file on
/// disk (any of the formats `image::load_from_memory` supports — PNG,
/// JPEG, WebP, GIF, BMP). Used when the selected history row is a
/// **Files** entry pointing at a single image — copying a JPG/HEIC out
/// of Finder is the typical path. Output is still PNG with alpha so
/// the cutout's transparency survives.
#[tauri::command]
pub fn cut_out_image_file(path: String) -> Result<String, String> {
    let bytes = std::fs::read(&path).map_err(|e| format!("read {path}: {e}"))?;
    // The output filename embeds the input's stem so the user can
    // tell two cutouts apart in Downloads (timestamp alone makes them
    // anonymous). Falls back to "image" if the path has no stem.
    let stem = std::path::Path::new(&path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    write_cutout(&bytes, Some(stem))
}

/// Internal helper: run cutout, write to ~/Downloads, return the saved
/// path. `name_hint` becomes the filename prefix when present, falling
/// back to the timestamp-only name when absent.
///
/// Uses the ML pipeline (`cutout_ml`) — real subject segmentation via
/// the embedded U2Netp ONNX model. The chroma-key implementation in
/// `cutout.rs` is kept around for future use (e.g. as a fast-path for
/// known-uniform-background entries) but no longer wired by default
/// because it failed too noisily on real photos.
fn write_cutout(image_bytes: &[u8], name_hint: Option<&str>) -> Result<String, String> {
    use chrono::Local;

    let png_bytes = cutout_ml::cut_out_subject(image_bytes).map_err(map_err)?;

    // ~/Downloads is the agreed output location. Falls back to the
    // home directory only if Downloads doesn't resolve (very unusual on
    // a desktop OS, but better than failing the whole action).
    let dir = dirs::download_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| "no Downloads or home directory available".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create downloads dir: {e}"))?;

    let stamp = Local::now().format("%Y%m%d-%H%M%S");
    let filename = match name_hint {
        Some(n) if !n.is_empty() => format!("{n}-cutout-{stamp}.png"),
        _ => format!("inspector-rust-cutout-{stamp}.png"),
    };
    let out_path = dir.join(&filename);
    std::fs::write(&out_path, &png_bytes).map_err(|e| format!("write {filename}: {e}"))?;

    Ok(out_path.to_string_lossy().into_owned())
}
