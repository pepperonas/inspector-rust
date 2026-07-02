use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::auto_expand;
use crate::gestures;
use crate::keepalive;
use crate::backup::{self, BackupImportResult};
use crate::cleaner;
use crate::meme;
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
#[cfg(target_os = "linux")]
use crate::desktop_shortcuts;
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
    // Slim list: omits the (multi-MB) image blobs the list view never renders.
    // The PreviewPanel fetches a selected image's data on demand via `get_clip`.
    db::list_slim(&db, limit, offset).map_err(map_err)
}

/// Fetch a single history entry **with its full payload** (including the image
/// blob the slim list omits). Used by the preview when an image clip is
/// selected.
#[tauri::command]
pub fn get_clip(db: State<'_, DbHandle>, id: i64) -> Result<Option<ClipEntry>, String> {
    db::get(&db, id).map_err(map_err)
}

#[tauri::command]
pub fn search_history(
    db: State<'_, DbHandle>,
    query: String,
    limit: usize,
) -> Result<Vec<ClipEntry>, String> {
    let all = db::list_slim(&db, 1000, 0).map_err(map_err)?;
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

// ── Timer (search-bar `timer N s|min|h`) ──────────────────────────────

#[tauri::command]
pub fn start_timer(
    app: AppHandle,
    state: State<'_, crate::timer::TimerRegistry>,
    seconds: u64,
    label: String,
) -> u64 {
    let id = crate::timer::start(state.inner(), app.clone(), seconds, label);
    let _ = app.emit("timers-changed", ());
    id
}

#[tauri::command]
pub fn cancel_timer(
    app: AppHandle,
    state: State<'_, crate::timer::TimerRegistry>,
    id: u64,
) -> bool {
    let ok = crate::timer::cancel(state.inner(), id);
    if ok {
        let _ = app.emit("timers-changed", ());
    }
    ok
}

#[tauri::command]
pub fn list_timers(
    state: State<'_, crate::timer::TimerRegistry>,
) -> Vec<crate::timer::TimerView> {
    crate::timer::list(state.inner())
}

// ── Timer/alarm style + the overlay alarm ─────────────────────────────────

const KEY_ALARM_STYLE: &str = "timer.alarm_style";

/// Which alarm a fired timer shows: `"overlay"` (default — the loud
/// dismiss-to-stop overlay) or `"notification"` (the legacy OS notification).
#[tauri::command]
pub fn get_alarm_style(db: State<'_, DbHandle>) -> Result<String, String> {
    crate::settings::get_or(&db, KEY_ALARM_STYLE, "overlay").map_err(map_err)
}

#[tauri::command]
pub fn set_alarm_style(db: State<'_, DbHandle>, style: String) -> Result<(), String> {
    let v = if style == "notification" {
        "notification"
    } else {
        "overlay"
    };
    crate::settings::set(&db, KEY_ALARM_STYLE, v).map_err(map_err)
}

/// The fired-timer label for the alarm overlay UI (`None` if no alarm active).
#[tauri::command]
pub fn alarm_overlay_label(app: AppHandle) -> Option<String> {
    crate::alarm::current_label(&app)
}

/// Silence + dismiss the active alarm (the overlay's Stop button).
#[tauri::command]
pub fn stop_alarm(app: AppHandle) {
    crate::alarm::stop(&app);
}

// ── Timesheet / time tracking ─────────────────────────────────────────────

/// `track on` — start a tracking session + the focus/idle loop.
#[tauri::command]
pub fn track_start(
    app: AppHandle,
    db: State<'_, DbHandle>,
    state: State<'_, crate::tracking::TrackerState>,
    label: Option<String>,
) -> Result<i64, String> {
    crate::tracking::start(&app, &db, &state, label)
}

/// `track off` — end the active session (closes the open interval).
#[tauri::command]
pub fn track_stop(
    app: AppHandle,
    db: State<'_, DbHandle>,
    state: State<'_, crate::tracking::TrackerState>,
) -> Result<(), String> {
    crate::tracking::stop(&app, &db, &state)
}

#[tauri::command]
pub fn track_status(
    state: State<'_, crate::tracking::TrackerState>,
) -> crate::tracking::TrackStatus {
    crate::tracking::status(&state)
}

/// Manually pause / resume recording without ending the session (distinct from
/// the automatic idle pause). Pausing closes the open interval immediately.
#[tauri::command]
pub fn track_set_paused(
    app: AppHandle,
    db: State<'_, DbHandle>,
    state: State<'_, crate::tracking::TrackerState>,
    paused: bool,
) -> Result<(), String> {
    crate::tracking::set_manual_paused(&app, &db, &state, paused)
}

/// Day report (`"YYYY-MM-DD"`, local): events + totals + breakdowns.
#[tauri::command]
pub fn track_get_day(
    db: State<'_, DbHandle>,
    date: String,
) -> Result<crate::tracking::DayReport, String> {
    crate::tracking::day_report(&db, &date)
}

/// Range/week report over the inclusive local-day range `[from, to]`.
#[tauri::command]
pub fn track_get_range(
    db: State<'_, DbHandle>,
    from: String,
    to: String,
) -> Result<crate::tracking::RangeReport, String> {
    crate::tracking::range_report(&db, &from, &to)
}

#[tauri::command]
pub fn track_update_event(
    db: State<'_, DbHandle>,
    id: i64,
    patch: crate::tracking::db::EventPatch,
) -> Result<(), String> {
    crate::tracking::db::update_event(&db, id, &patch).map_err(map_err)
}

/// The still-growing (live) event must never be deleted/merged: the run loop's
/// heartbeat keeps writing to that id, so removing the row silently stops
/// persisting all further time in the current focus span (unbounded data loss).
const ERR_LIVE_EVENT: &str =
    "This entry is still being recorded — switch apps or stop tracking first.";

#[tauri::command]
pub fn track_delete_event(
    db: State<'_, DbHandle>,
    tracker: State<'_, crate::tracking::TrackerState>,
    id: i64,
) -> Result<(), String> {
    if crate::tracking::live_event_id(&tracker) == Some(id) {
        return Err(ERR_LIVE_EVENT.into());
    }
    crate::tracking::db::delete_event(&db, id).map_err(map_err)
}

#[tauri::command]
pub fn track_merge_events(
    db: State<'_, DbHandle>,
    tracker: State<'_, crate::tracking::TrackerState>,
    ids: Vec<i64>,
) -> Result<Option<i64>, String> {
    if let Some(live) = crate::tracking::live_event_id(&tracker) {
        if ids.contains(&live) {
            return Err(ERR_LIVE_EVENT.into());
        }
    }
    crate::tracking::db::merge_events(&db, &ids).map_err(map_err)
}

#[tauri::command]
pub fn track_set_category(
    db: State<'_, DbHandle>,
    app_name: String,
    category: String,
) -> Result<(), String> {
    crate::tracking::db::set_category(&db, &app_name, &category).map_err(map_err)
}

/// All app→category rules, as `[appName, category]` pairs.
#[tauri::command]
pub fn track_category_rules(db: State<'_, DbHandle>) -> Result<Vec<(String, String)>, String> {
    crate::tracking::db::list_category_rules(&db).map_err(map_err)
}

#[tauri::command]
pub fn track_delete_category_rule(db: State<'_, DbHandle>, app_name: String) -> Result<(), String> {
    crate::tracking::db::delete_category_rule(&db, &app_name).map_err(map_err)
}

/// Distinct category names ever used — for assign autocomplete.
#[tauri::command]
pub fn track_distinct_categories(db: State<'_, DbHandle>) -> Result<Vec<String>, String> {
    crate::tracking::db::distinct_categories(&db).map_err(map_err)
}

/// Assign a project to the given events (empty string clears). Returns rows.
#[tauri::command]
pub fn track_set_project(
    db: State<'_, DbHandle>,
    ids: Vec<i64>,
    project: String,
) -> Result<usize, String> {
    crate::tracking::db::set_project_for_events(&db, &ids, Some(project.as_str())).map_err(map_err)
}

/// Distinct project names ever used — for assign autocomplete.
#[tauri::command]
pub fn track_distinct_projects(db: State<'_, DbHandle>) -> Result<Vec<String>, String> {
    crate::tracking::db::distinct_projects(&db).map_err(map_err)
}

#[tauri::command]
pub fn track_clear_all(db: State<'_, DbHandle>) -> Result<(), String> {
    crate::tracking::db::clear_all(&db).map_err(map_err)
}

/// Manually add a completed time entry (start/end in unix ms). Attaches to the
/// active session if tracking, else a "Manual entries" container.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn track_add_event(
    db: State<'_, DbHandle>,
    app_name: String,
    category: Option<String>,
    project: Option<String>,
    window_title: Option<String>,
    started_at: i64,
    ended_at: i64,
) -> Result<i64, String> {
    if ended_at <= started_at {
        return Err("end must be after start".into());
    }
    let now = chrono::Utc::now().timestamp_millis();
    let sid = crate::tracking::db::manual_session_id(&db, now).map_err(map_err)?;
    let ev = crate::tracking::db::NewEvent {
        session_id: sid,
        app_name,
        app_id: None,
        window_title: window_title.filter(|s| !s.is_empty()),
        url: None,
        host: None,
        category: category.filter(|s| !s.is_empty()),
        project: project.filter(|s| !s.is_empty()),
        source: "manual".to_string(),
        is_idle: false,
        started_at,
    };
    crate::tracking::db::insert_event(&db, &ev, ended_at).map_err(map_err)
}

/// Tidy a day: delete idle spans + sub-`min_seconds` fragments. Returns count.
#[tauri::command]
pub fn track_cleanup_day(
    db: State<'_, DbHandle>,
    tracker: State<'_, crate::tracking::TrackerState>,
    date: String,
    min_seconds: i64,
) -> Result<usize, String> {
    let (from, to) = crate::tracking::day_bounds(&date)?;
    // The live event is excluded — a cleanup while idle-paused (or right after
    // a focus switch) would otherwise delete the row the heartbeat writes to.
    let live = crate::tracking::live_event_id(&tracker);
    crate::tracking::db::cleanup_day(&db, from, to, min_seconds.max(0), live).map_err(map_err)
}

/// Tidy a whole date range (`from`..=`to`, local "YYYY-MM-DD" days): the same
/// idle + sub-`min_seconds` sweep as `track_cleanup_day`, in one call — so a
/// week (or month) of noise doesn't need per-day clicking.
#[tauri::command]
pub fn track_cleanup_range(
    db: State<'_, DbHandle>,
    tracker: State<'_, crate::tracking::TrackerState>,
    from: String,
    to: String,
    min_seconds: i64,
) -> Result<usize, String> {
    let (start, _) = crate::tracking::day_bounds(&from)?;
    let (_, end) = crate::tracking::day_bounds(&to)?;
    if end <= start {
        return Err("range end before start".into());
    }
    let live = crate::tracking::live_event_id(&tracker);
    crate::tracking::db::cleanup_day(&db, start, end, min_seconds.max(0), live).map_err(map_err)
}

/// Timesheet settings (Settings → Timesheet).
#[derive(serde::Serialize, serde::Deserialize)]
pub struct TimesheetConfig {
    idle_seconds: i64,
    retention_days: i64,
    claude_watcher: bool,
    denylist: String,
    daily_goal_minutes: i64,
}

#[tauri::command]
pub fn get_timesheet_config(db: State<'_, DbHandle>) -> TimesheetConfig {
    let g = |k: &str, d: &str| crate::settings::get_or(&db, k, d).unwrap_or_else(|_| d.to_string());
    TimesheetConfig {
        idle_seconds: g("track.idle_seconds", "300").parse().unwrap_or(300),
        retention_days: g("track.retention_days", "0").parse().unwrap_or(0),
        claude_watcher: g("track.claude_watcher", "1") != "0",
        denylist: g("track.denylist", ""),
        daily_goal_minutes: g("track.daily_goal_minutes", "0").parse().unwrap_or(0),
    }
}

#[tauri::command]
pub fn set_timesheet_config(db: State<'_, DbHandle>, config: TimesheetConfig) -> Result<(), String> {
    let s = |k: &str, v: &str| crate::settings::set(&db, k, v).map_err(map_err);
    s("track.idle_seconds", &config.idle_seconds.max(10).to_string())?;
    s("track.retention_days", &config.retention_days.max(0).to_string())?;
    s("track.claude_watcher", if config.claude_watcher { "1" } else { "0" })?;
    s("track.denylist", &config.denylist)?;
    s("track.daily_goal_minutes", &config.daily_goal_minutes.max(0).to_string())?;
    Ok(())
}

/// Project-grouped export over the inclusive local-day range `[from, to]`
/// (`"YYYY-MM-DD"`) as `csv` or printable `html` → `~/Downloads`, revealed.
/// Lists, per project, when + how long + on what (billable: active, non-Claude,
/// project-tagged). Returns the written path.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn track_export_projects(
    db: State<'_, DbHandle>,
    format: String,
    from: String,
    to: String,
    project: Option<String>,
    detail: Option<String>,
) -> Result<String, String> {
    let (from_ms, _) = crate::tracking::day_bounds(&from)?;
    let (_, to_ms) = crate::tracking::day_bounds(&to)?;
    if to_ms <= from_ms {
        return Err("range end before start".into());
    }
    let events = crate::tracking::db::events_in_range(&db, from_ms, to_ms).map_err(map_err)?;
    let now = chrono::Utc::now().timestamp_millis();
    let detail = crate::tracking::export::Detail::parse(detail.as_deref().unwrap_or("full"));
    let proj = project.as_deref().filter(|p| !p.is_empty());
    let (content, ext) = if format == "html" {
        (crate::tracking::export::project_html(&events, from_ms, to_ms, now, detail, proj), "html")
    } else {
        (crate::tracking::export::project_csv(&events, now, detail, proj), "csv")
    };
    // Filename: include a sanitized project slug when scoped to one client.
    let slug = proj
        .map(|p| {
            let s: String = p
                .chars()
                .map(|c| if c.is_alphanumeric() { c } else { '-' })
                .collect();
            format!("-{}", s.trim_matches('-'))
        })
        .unwrap_or_default();
    let dir = dirs::download_dir().ok_or_else(|| "no Downloads folder".to_string())?;
    let path = dir.join(format!("timesheet-projects{slug}-{from}_{to}.{ext}"));
    std::fs::write(&path, content).map_err(|e| format!("write export: {e}"))?;
    reveal_in_file_manager(&path);
    Ok(path.display().to_string())
}

/// Loopback-bridge connection info for the browser extension's options page.
#[derive(serde::Serialize)]
pub struct BridgeInfo {
    port: u16,
    token: String,
}

#[tauri::command]
pub fn track_bridge_info(db: State<'_, DbHandle>) -> BridgeInfo {
    BridgeInfo {
        port: crate::tracking::bridge::bridge_port(&db),
        token: crate::tracking::bridge::bridge_token(&db),
    }
}

/// Generate a fresh bridge token (invalidates the old one — re-enter it in the
/// extension). Returns the new token.
#[tauri::command]
pub fn track_bridge_regenerate(db: State<'_, DbHandle>) -> String {
    crate::tracking::bridge::regenerate_token(&db)
}

/// Write the browser extension to `~/Downloads/inspector-rust-timesheet-extension/`
/// and reveal it (Chrome can't be auto-installed; the user loads it unpacked).
/// Returns the folder path.
#[tauri::command]
pub fn track_export_extension() -> Result<String, String> {
    let dir = crate::tracking::extension::write_to_downloads()?;
    reveal_in_file_manager(&dir);
    Ok(dir.display().to_string())
}

/// Export the events in `[from, to)` (unix ms) to `~/Downloads` as `csv` or a
/// self-contained `html` report; reveals the file. Returns the written path.
#[tauri::command]
pub fn track_export(
    db: State<'_, DbHandle>,
    format: String,
    from: i64,
    to: i64,
) -> Result<String, String> {
    use chrono::TimeZone;
    let events = crate::tracking::db::events_in_range(&db, from, to).map_err(map_err)?;
    let now = chrono::Utc::now().timestamp_millis();
    let (content, ext) = if format == "html" {
        let tokens = crate::tracking::db::claude_tokens_by_project(&db, from, to).unwrap_or_default();
        (crate::tracking::export::html(&events, &tokens, from, to, now), "html")
    } else {
        (crate::tracking::export::csv(&events, now), "csv")
    };
    let dir = dirs::download_dir().ok_or_else(|| "no Downloads folder".to_string())?;
    let stamp = chrono::Local
        .timestamp_millis_opt(from)
        .single()
        .map(|d| d.format("%Y%m%d").to_string())
        .unwrap_or_else(|| from.to_string());
    let path = dir.join(format!("timesheet-{stamp}.{ext}"));
    std::fs::write(&path, content).map_err(|e| format!("write export: {e}"))?;
    reveal_in_file_manager(&path);
    Ok(path.display().to_string())
}

// ── App launcher (Spotlight-like) ─────────────────────────────────────

/// Return the cached app index. Frontend fuzzy-matches against this
/// list in the popup search bar; one shot at popup-mount, no polling.
#[tauri::command]
pub fn list_apps(state: State<'_, crate::app_launcher::AppIndex>) -> Vec<crate::app_launcher::AppEntry> {
    state.inner().apps.lock().clone()
}

/// Re-scan installed apps. Called from Settings → Apps → Refresh.
/// Returns the new count so the UI can confirm the rescan ran.
#[tauri::command]
pub fn refresh_apps(state: State<'_, crate::app_launcher::AppIndex>) -> usize {
    let fresh = crate::app_launcher::scan();
    let n = fresh.len();
    *state.inner().apps.lock() = fresh;
    // Drop cached icons too — a fresh scan may have replaced apps at
    // the same path with a new version (Sparkle/MAS update).
    state.inner().icons.lock().clear();
    n
}

/// Launch the app at `path` via `/usr/bin/open` (macOS Launch Services).
/// If the app is already running, this activates the existing instance
/// instead of spawning a duplicate.
#[tauri::command]
pub fn launch_app(path: String) -> Result<(), String> {
    crate::app_launcher::launch(std::path::Path::new(&path)).map_err(map_err)
}

/// Lazy icon fetch. First call per `path` shells out to sips
/// (~50 ms); subsequent calls hit the in-memory LRU cache.
#[tauri::command]
pub fn get_app_icon(
    state: State<'_, crate::app_launcher::AppIndex>,
    path: String,
) -> Result<String, String> {
    {
        let cache = state.inner().icons.lock();
        if let Some(cached) = cache.get(&path) {
            return Ok(cached.clone());
        }
    }
    let b64 = crate::app_launcher::icon_png_base64(std::path::Path::new(&path)).map_err(map_err)?;
    state.inner().icons.lock().insert(path, b64.clone());
    Ok(b64)
}

// ── Bruno (Brutto-Netto-Rechner) ──────────────────────────────────────

#[tauri::command]
pub fn bruno_get_defaults(
    db: State<'_, DbHandle>,
) -> Result<crate::bruno::BrunoDefaults, String> {
    crate::bruno::get_defaults(&db).map_err(map_err)
}

#[tauri::command]
pub fn bruno_set_defaults(
    app: AppHandle,
    db: State<'_, DbHandle>,
    defaults: crate::bruno::BrunoDefaults,
) -> Result<(), String> {
    crate::bruno::set_defaults(&db, &defaults).map_err(map_err)?;
    // Let the popup re-fetch — otherwise the running App.tsx keeps
    // using stale defaults until next app launch.
    let _ = app.emit("bruno-defaults-changed", ());
    Ok(())
}

// ── Wakelock ──────────────────────────────────────────────────────────

/// Toggle the wakelock. Returns the resulting state (`true` = active,
/// `false` = off). On macOS this spawns `caffeinate -disu`; on
/// Windows / Linux it spawns the cursor-jiggle worker. Also emits
/// `wakelock-changed` with the resulting state so the popup's footer
/// LED can update without polling.
#[tauri::command]
pub fn wakelock_set(
    app: AppHandle,
    state: State<'_, crate::wakelock::WakelockState>,
    enable: bool,
    source: Option<String>,
) -> bool {
    let new_state = crate::wakelock::set_enabled(state.inner(), enable);
    let _ = app.emit("wakelock-changed", new_state);
    // Close the popup the normal way (on macOS this also `app.hide()`s, so
    // focus returns to the prior app), then — a beat LATER — pop the
    // status flourish. Showing the overlay shortly after `app.hide()` has
    // settled mirrors the screenshot-preview flow, which is the one path
    // that reliably orders a fresh Accessory-app window on-screen; showing
    // it synchronously (or after a window-only hide) left it off-screen.
    // Brand the toast by which keyword was used (`caffeine` vs `wakelock`);
    // both drive the identical animation/behaviour.
    let label = if source.as_deref() == Some("caffeine") { "Caffeine" } else { "Wakelock" };
    let (title, subtitle) = if new_state {
        (format!("{label} On"), "Sleep & screen lock are paused")
    } else {
        (format!("{label} Off"), "Normal sleep behaviour resumed")
    };
    crate::status_toast::announce(
        &app,
        crate::status_toast::StatusToast {
            kind: "wakelock".into(),
            on: new_state,
            title,
            subtitle: subtitle.into(),
        },
    );
    new_state
}

#[tauri::command]
pub fn wakelock_get(state: State<'_, crate::wakelock::WakelockState>) -> bool {
    crate::wakelock::is_enabled(state.inner())
}

/// Show an on-screen status toast (hide popup + animated flourish). Used
/// by the frontend for timer / alarm confirmations (wakelock fires its
/// own via `wakelock_set`).
#[tauri::command]
pub fn show_status_toast(app: AppHandle, kind: String, on: bool, title: String, subtitle: String) {
    crate::status_toast::announce(
        &app,
        crate::status_toast::StatusToast { kind, on, title, subtitle },
    );
}

/// Pull the most recent status-toast payload (the toast window reads this
/// on mount + on each `status-toast-changed` event).
#[tauri::command]
pub fn get_status_toast(
    state: State<'_, crate::status_toast::LatestToast>,
) -> Option<crate::status_toast::StatusToast> {
    state.0.lock().clone()
}

/// Hide the toast window — called by the toast's own auto-dismiss timer.
#[tauri::command]
pub fn hide_status_toast(app: AppHandle) {
    crate::status_toast::hide(&app);
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

// ── Feedback sounds (v0.84.57) ─────────────────────────────────────────

const KEY_SOUND_ENABLED: &str = "sound.enabled";

/// Read the persisted master sound toggle. Defaults to `true` (sounds on) on
/// a fresh install — the long-standing behaviour where the expand click
/// always played.
#[tauri::command]
pub fn get_sound_enabled(db: State<'_, DbHandle>) -> Result<bool, String> {
    settings::get_bool(&db, KEY_SOUND_ENABLED, true).map_err(map_err)
}

/// Persist the master sound toggle and apply it in-process immediately (so
/// the change takes effect without a relaunch).
#[tauri::command]
pub fn set_sound_enabled(db: State<'_, DbHandle>, enabled: bool) -> Result<(), String> {
    settings::set(&db, KEY_SOUND_ENABLED, if enabled { "true" } else { "false" })
        .map_err(map_err)?;
    crate::sound::set_enabled(enabled);
    Ok(())
}

// ── Clipboard privacy (v0.76.0) ────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ClipboardPrivacy {
    /// Comma/newline-separated app-name substrings never captured from.
    pub exclude_apps: String,
    /// Seconds after a copy to auto-wipe the clipboard (0 = off).
    pub auto_clear_seconds: u32,
}

#[tauri::command]
pub fn get_clipboard_privacy(db: State<'_, DbHandle>) -> Result<ClipboardPrivacy, String> {
    use crate::clipboard_watcher::{KEY_AUTO_CLEAR_SECS, KEY_EXCLUDE_APPS};
    let exclude_apps = settings::get_or(&db, KEY_EXCLUDE_APPS, "").map_err(map_err)?;
    let auto_clear_seconds = settings::get_or(&db, KEY_AUTO_CLEAR_SECS, "0")
        .map_err(map_err)?
        .trim()
        .parse::<u32>()
        .unwrap_or(0);
    Ok(ClipboardPrivacy {
        exclude_apps,
        auto_clear_seconds,
    })
}

#[tauri::command]
pub fn set_clipboard_privacy(
    db: State<'_, DbHandle>,
    exclude_apps: String,
    auto_clear_seconds: u32,
) -> Result<(), String> {
    use crate::clipboard_watcher::{KEY_AUTO_CLEAR_SECS, KEY_EXCLUDE_APPS};
    settings::set(&db, KEY_EXCLUDE_APPS, exclude_apps.trim()).map_err(map_err)?;
    // Clamp to a sane ceiling (1 hour) so a typo can't park a wipe forever.
    let secs = auto_clear_seconds.min(3600);
    settings::set(&db, KEY_AUTO_CLEAR_SECS, &secs.to_string()).map_err(map_err)?;
    Ok(())
}

// ── Popup overlay size (v0.49.0+) ──────────────────────────────────────

const KEY_WINDOW_SIZE: &str = "appearance.window_size";

/// Normalise a stored / incoming popup-size string to one of the three
/// valid presets. Anything unrecognised collapses to `"medium"` so a
/// hand-edited settings DB can never wedge the window.
fn normalise_window_size(s: &str) -> &'static str {
    match s {
        "small" => "small",
        "large" => "large",
        _ => "medium",
    }
}

/// Logical (point) dimensions for each popup-size preset. `medium` is the
/// historical 700×500 default the window ships with in `tauri.conf.json`.
fn window_size_dimensions(size: &str) -> (f64, f64) {
    match size {
        "small" => (600.0, 430.0),
        "large" => (840.0, 600.0),
        _ => (700.0, 500.0),
    }
}

/// Resize the popup window to a preset. The actual mutation is dispatched
/// to the main thread (macOS requires window changes there). Best-effort —
/// a missing window is a silent no-op. The next `show_and_position` recentres
/// the window using the new size, so no explicit re-centre is needed here.
fn resize_popup(app: &AppHandle, size: &str) {
    let (w, h) = window_size_dimensions(size);
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(win) = app2.get_webview_window(crate::hotkey::POPUP_LABEL) {
            let _ = win.set_size(tauri::LogicalSize::new(w, h));
        }
    });
}

/// Apply the persisted popup size at startup. Called from `lib.rs` setup so
/// the window opens at the user's chosen size from the very first show.
pub fn apply_window_size(app: &AppHandle, db: &DbHandle) {
    let size = settings::get_or(db, KEY_WINDOW_SIZE, "medium")
        .map(|s| normalise_window_size(&s).to_string())
        .unwrap_or_else(|_| "medium".to_string());
    resize_popup(app, &size);
}

/// Read the persisted popup-size preference. One of `"small"`, `"medium"`,
/// `"large"`. Defaults to `"medium"` on a fresh install.
#[tauri::command]
pub fn get_window_size_preference(db: State<'_, DbHandle>) -> Result<String, String> {
    let raw = settings::get_or(&db, KEY_WINDOW_SIZE, "medium").map_err(map_err)?;
    Ok(normalise_window_size(&raw).to_string())
}

/// Persist the popup-size preference and resize the live window. Rejects
/// anything that isn't one of the three valid presets.
#[tauri::command]
pub fn set_window_size_preference(
    app: AppHandle,
    db: State<'_, DbHandle>,
    size: String,
) -> Result<(), String> {
    let normalised = normalise_window_size(&size);
    if normalised != size {
        return Err(format!(
            "invalid window size {size:?} — expected one of small / medium / large",
        ));
    }
    settings::set(&db, KEY_WINDOW_SIZE, normalised).map_err(map_err)?;
    resize_popup(&app, normalised);
    Ok(())
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

/// Pin / unpin a clipboard entry. Pinned entries float to the top of the
/// history and are exempt from the 1 000-row prune.
#[tauri::command]
pub fn set_clip_pinned(
    app: AppHandle,
    db: State<'_, DbHandle>,
    id: i64,
    pinned: bool,
) -> Result<(), String> {
    db::set_pinned(&db, id, pinned).map_err(map_err)?;
    let _ = app.emit("clipboard-changed", ());
    Ok(())
}

/// Attach / update / clear a note on a clipboard entry. An empty string clears
/// the note (and re-exposes the entry to pruning). Noted entries are highlighted
/// in the list and exempt from the prune.
#[tauri::command]
pub fn set_clip_note(
    app: AppHandle,
    db: State<'_, DbHandle>,
    id: i64,
    note: String,
) -> Result<(), String> {
    db::set_note(&db, id, Some(note.as_str())).map_err(map_err)?;
    let _ = app.emit("clipboard-changed", ());
    Ok(())
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
    ae: State<'_, auto_expand::AutoExpandState>,
    id: Option<i64>,
    abbreviation: String,
    title: String,
    body: String,
) -> Result<i64, String> {
    let result = match id {
        None => snippets::create(&db, &abbreviation, &title, &body).map_err(map_err),
        Some(existing_id) => {
            snippets::update(&db, existing_id, &abbreviation, &title, &body)
                .map_err(map_err)?;
            Ok(existing_id)
        }
    };
    if result.is_ok() {
        auto_expand::rebuild_table(&db, &ae);
    }
    result
}

#[tauri::command]
pub fn delete_snippet(
    db: State<'_, DbHandle>,
    ae: State<'_, auto_expand::AutoExpandState>,
    id: i64,
) -> Result<(), String> {
    snippets::delete(&db, id).map_err(map_err)?;
    auto_expand::rebuild_table(&db, &ae);
    Ok(())
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
    // Expand dynamic placeholders ({date}, {clipboard}, {cursor}, …) against
    // the current clipboard, then paste and honour any {cursor} marker.
    let rendered = crate::expander::render_snippet_body(&snippet.body);
    watcher.mark_self_write(crate::models::ContentType::Text, &rendered.text);
    paste::paste_text(&rendered.text).map_err(map_err)?;
    let _ = paste::move_cursor_left(rendered.cursor_back);
    Ok(())
}

/// Import snippets from a JSON document. Existing rows with the same
/// abbreviation are overwritten. Per-row errors are returned in the result
/// instead of aborting the whole import.
#[tauri::command]
pub fn import_snippets(
    db: State<'_, DbHandle>,
    ae: State<'_, auto_expand::AutoExpandState>,
    json: String,
) -> Result<ImportResult, String> {
    let r = snippets::import_from_json(&db, &json).map_err(map_err)?;
    auto_expand::rebuild_table(&db, &ae);
    Ok(r)
}

/// Read a JSON file from disk and import its snippets. Path is supplied by
/// the frontend after the user picked a file via the native dialog plugin.
#[tauri::command]
pub fn import_snippets_from_file(
    db: State<'_, DbHandle>,
    ae: State<'_, auto_expand::AutoExpandState>,
    path: String,
) -> Result<ImportResult, String> {
    let json = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {path}: {e}"))?;
    let r = snippets::import_from_json(&db, &json).map_err(map_err)?;
    auto_expand::rebuild_table(&db, &ae);
    Ok(r)
}

/// Re-import the bundled default AI-prompt snippets. Existing rows
/// sharing an `abbreviation` get overwritten; user snippets with
/// distinct abbreviations are untouched. Surfaced via the Snippets-tab
/// "Restore defaults" button.
#[tauri::command]
pub fn restore_default_prompts(
    db: State<'_, DbHandle>,
    ae: State<'_, auto_expand::AutoExpandState>,
) -> Result<ImportResult, String> {
    let r = seed::restore_defaults(&db).map_err(map_err)?;
    auto_expand::rebuild_table(&db, &ae);
    Ok(r)
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
/// notes / totp / settings) is included only if the corresponding flag
/// is `true` — lets the user opt out of, say, exporting their clipboard
/// history when sharing snippets with a colleague. Defaults to *all true*
/// if invoked without the flags (legacy callers). If `password` is
/// provided, the backup is encrypted with AES-256-GCM + Argon2id.
#[tauri::command]
pub fn export_backup(
    db: State<'_, DbHandle>,
    include_history: Option<bool>,
    include_snippets: Option<bool>,
    include_notes: Option<bool>,
    include_totp: Option<bool>,
    include_settings: Option<bool>,
    password: Option<String>,
) -> Result<String, String> {
    let opts = backup::ExportOptions {
        include_history: include_history.unwrap_or(true),
        include_snippets: include_snippets.unwrap_or(true),
        include_notes: include_notes.unwrap_or(true),
        include_totp: include_totp.unwrap_or(true),
        include_settings: include_settings.unwrap_or(true),
    };
    backup::export_json_maybe_encrypted(&db, opts, password.as_deref()).map_err(map_err)
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
    include_totp: Option<bool>,
    include_settings: Option<bool>,
    password: Option<String>,
) -> Result<usize, String> {
    let opts = backup::ExportOptions {
        include_history: include_history.unwrap_or(true),
        include_snippets: include_snippets.unwrap_or(true),
        include_notes: include_notes.unwrap_or(true),
        include_totp: include_totp.unwrap_or(true),
        include_settings: include_settings.unwrap_or(true),
    };
    let json = backup::export_json_maybe_encrypted(&db, opts, password.as_deref())
        .map_err(map_err)?;
    std::fs::write(&path, &json).map_err(|e| format!("write {path}: {e}"))?;
    Ok(json.len())
}

/// Read a backup JSON file from `path` and merge it into the live database.
/// If the file is encrypted (detected automatically), the `password` parameter
/// is required. Returns import counts.
#[tauri::command]
pub fn import_backup(
    db: State<'_, DbHandle>,
    path: String,
    password: Option<String>,
) -> Result<BackupImportResult, String> {
    let json = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {path}: {e}"))?;
    backup::import_json_maybe_encrypted(&db, &json, password.as_deref()).map_err(map_err)
}

/// Check if a backup file is encrypted. The frontend uses this to decide
/// whether to prompt for a password before importing.
#[tauri::command]
pub fn is_backup_encrypted(path: String) -> Result<bool, String> {
    let json = std::fs::read_to_string(&path)
        .map_err(|e| format!("read {path}: {e}"))?;
    Ok(backup::is_encrypted(&json))
}

// ── Passive auto-expansion (aText-style, v0.56.0) ──────────────────────────────

/// Read the passive auto-expansion config from settings (defaults applied).
#[tauri::command]
pub fn get_auto_expand_config(
    db: State<'_, DbHandle>,
) -> Result<auto_expand::AutoExpandConfig, String> {
    Ok(auto_expand::load_config(&db))
}

/// Persist a new auto-expansion config and (re)arm or disarm the passive
/// key monitor to match. Returns the now-effective config.
#[tauri::command]
pub fn set_auto_expand_config(
    app: AppHandle,
    db: State<'_, DbHandle>,
    ae: State<'_, auto_expand::AutoExpandState>,
    config: auto_expand::AutoExpandConfig,
) -> Result<auto_expand::AutoExpandConfig, String> {
    auto_expand::save_config(&db, &config).map_err(map_err)?;
    auto_expand::apply(&app, &db, &ae);
    Ok(auto_expand::load_config(&db))
}

// ── Configurable global action hotkeys ───────────────────────────────────────

/// List every configurable action hotkey with its effective + default binding.
#[tauri::command]
pub fn list_action_hotkeys(app: AppHandle) -> Vec<crate::hotkey::ActionHotkeyView> {
    crate::hotkey::action_views(&app)
}

/// Bind (or clear, with empty `shortcut`) an action hotkey. Validates against
/// every other binding; returns a human error on conflict / parse failure.
#[tauri::command]
pub fn set_action_hotkey(app: AppHandle, id: String, shortcut: String) -> Result<(), String> {
    crate::hotkey::set_action_hotkey(&app, &id, &shortcut)
}

/// Reset an action hotkey to its built-in default.
#[tauri::command]
pub fn reset_action_hotkey(app: AppHandle, id: String) -> Result<(), String> {
    crate::hotkey::reset_action_hotkey(&app, &id)
}

// ── boom — audio enhancement (DSP engine; phase 1a) ───────────────────────────

/// Whether the host supports the boom engine (macOS 14.2+ process-tap API).
#[tauri::command]
pub fn boom_available() -> bool {
    crate::boom::is_supported()
}

/// All built-in EQ presets (genre + device-correction).
#[tauri::command]
pub fn boom_presets() -> Vec<crate::boom::Preset> {
    crate::boom::presets()
}

/// Whether the audio backend is installed — macOS: the "boom Audio" virtual
/// driver; Windows: Equalizer APO (boom writes its config; no own driver).
#[tauri::command]
pub fn boom_driver_installed() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::boom::macos::driver_present()
    }
    #[cfg(target_os = "windows")]
    {
        crate::boom::windows::available()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

/// Install the bundled driver (admin prompt + coreaudiod restart).
#[tauri::command]
pub fn boom_install_driver(app: AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::boom::macos::install_driver(&app)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Ok(())
    }
}

/// Uninstall the driver (admin prompt + coreaudiod restart).
#[tauri::command]
pub fn boom_uninstall_driver() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::boom::macos::uninstall_driver()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

/// Live level-meter readout (input/output RMS + clip).
#[tauri::command]
pub fn boom_levels() -> crate::boom::BoomLevels {
    crate::boom::levels()
}

#[tauri::command]
pub fn get_boom_config(db: State<'_, DbHandle>) -> crate::boom::BoomConfig {
    crate::boom::BoomConfig::load(&db)
}

/// Persist the boom config (and, in phase 1b, push it to the live DSP engine).
#[tauri::command]
pub fn set_boom_config(
    db: State<'_, DbHandle>,
    config: crate::boom::BoomConfig,
) -> Result<crate::boom::BoomConfig, String> {
    config.save(&db).map_err(map_err)?;
    crate::boom::apply(&db); // start/stop the engine + push DSP params
    Ok(crate::boom::BoomConfig::load(&db))
}

// ── Window palette (Moom-style hover palette) ─────────────────────────────────

#[tauri::command]
pub fn get_window_palette_config(db: State<'_, DbHandle>) -> crate::window_palette::WindowPaletteConfig {
    crate::window_palette::WindowPaletteConfig::load(&db)
}

#[tauri::command]
pub fn set_window_palette_config(
    app: AppHandle,
    db: State<'_, DbHandle>,
    wp: State<'_, crate::window_palette::WindowPaletteState>,
    config: crate::window_palette::WindowPaletteConfig,
) -> Result<crate::window_palette::WindowPaletteConfig, String> {
    config.save(&db).map_err(map_err)?;
    crate::window_palette::apply(&app, &db, &wp);
    Ok(crate::window_palette::WindowPaletteConfig::load(&db))
}

/// Context for the palette webview (grid density + target-screen dimensions).
#[tauri::command]
pub fn window_palette_context() -> crate::window_palette::PaletteContext {
    #[cfg(target_os = "macos")]
    {
        crate::window_palette::macos::context()
    }
    #[cfg(not(target_os = "macos"))]
    {
        crate::window_palette::PaletteContext::default()
    }
}

/// Apply a chosen 0..1 fraction (preset or hex-grid selection) to the hovered
/// window, then close the palette.
#[tauri::command]
pub fn window_palette_apply(fx: f64, fy: f64, fw: f64, fh: f64) {
    #[cfg(target_os = "macos")]
    crate::window_palette::macos::apply_fraction(fx, fy, fw, fh);
    #[cfg(not(target_os = "macos"))]
    let _ = (fx, fy, fw, fh);
}

/// Dismiss the palette without changing the window (Esc / click-away).
#[tauri::command]
pub fn window_palette_cancel() {
    #[cfg(target_os = "macos")]
    {
        crate::window_palette::macos::cancel();
    }
}

/// Show the live screen-outline preview for a 0..1 fraction (hex-grid drag /
/// preset hover) — a frame on the actual screen where the window will land.
#[tauri::command]
pub fn window_palette_preview(fx: f64, fy: f64, fw: f64, fh: f64) {
    #[cfg(target_os = "macos")]
    crate::window_palette::macos::preview(fx, fy, fw, fh);
    #[cfg(not(target_os = "macos"))]
    let _ = (fx, fy, fw, fh);
}

/// Hide the live screen-outline preview.
#[tauri::command]
pub fn window_palette_preview_hide() {
    #[cfg(target_os = "macos")]
    crate::window_palette::macos::preview_hide();
}

// ── Window snapping (drag-to-snap) ────────────────────────────────────────────

/// Current window-snap config (opt-in; off by default).
#[tauri::command]
pub fn get_window_snap_config(db: State<'_, DbHandle>) -> crate::window_snap::WindowSnapConfig {
    crate::window_snap::WindowSnapConfig::load(&db)
}

/// Persist a new window-snap config and (re)start/stop the drag monitor.
#[tauri::command]
pub fn set_window_snap_config(
    app: AppHandle,
    db: State<'_, DbHandle>,
    ws: State<'_, crate::window_snap::WindowSnapState>,
    config: crate::window_snap::WindowSnapConfig,
) -> Result<crate::window_snap::WindowSnapConfig, String> {
    config.save(&db).map_err(map_err)?;
    crate::window_snap::apply(&app, &db, &ws);
    Ok(crate::window_snap::WindowSnapConfig::load(&db))
}

// ── Keep-alive (always running) ──────────────────────────────────────────────

/// Is the keep-alive supervisor installed (app auto-relaunches when not running)?
#[tauri::command]
pub fn get_keepalive_enabled() -> bool {
    keepalive::is_enabled()
}

/// Install / remove the keep-alive supervisor; returns the now-effective state.
#[tauri::command]
pub fn set_keepalive_enabled(enabled: bool) -> Result<bool, String> {
    keepalive::set_enabled(enabled)?;
    Ok(keepalive::is_enabled())
}

// ── Touchpad gestures ────────────────────────────────────────────────────────

/// Current touchpad-gesture config (opt-in; off by default).
#[tauri::command]
pub fn get_gesture_config(db: State<'_, DbHandle>) -> gestures::GestureConfig {
    gestures::GestureConfig::load(&db)
}

/// Persist a new gesture config and (re)start or stop the OS capture source to
/// match. Returns the now-effective config.
#[tauri::command]
pub fn set_gesture_config(
    app: AppHandle,
    db: State<'_, DbHandle>,
    g: State<'_, gestures::GestureState>,
    config: gestures::GestureConfig,
) -> Result<gestures::GestureConfig, String> {
    config.save(&db).map_err(map_err)?;
    gestures::apply(&app, &db, &g);
    Ok(gestures::GestureConfig::load(&db))
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
        // Custom loupe (snapshot magnified in an overlay) so the live hex is
        // shown under the loupe; emits `color-picked` on pick/cancel, exactly
        // like the old NSColorSampler path the modal listens for.
        open_color_loupe(&app, true);
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
    ae: State<'_, auto_expand::AutoExpandState>,
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

    // Arm/disarm the passive keystroke monitor: enabling the abbreviation
    // hotkey makes it track keystrokes so `Alt+1` can expand the typed word
    // from the buffer (reliable everywhere, incl. terminals).
    auto_expand::apply(&app, &db, &ae);

    Ok(ExpanderConfig {
        enabled,
        hotkey,
        accessibility_granted: expander::accessibility_granted(),
    })
}

// ── Popup hotkey ─────────────────────────────────────────────────────────

/// Read the user-configured popup hotkey (or the default if never set).
#[tauri::command]
pub fn get_popup_hotkey(db: State<'_, DbHandle>) -> Result<String, String> {
    settings::get_or(&db, hotkey::KEY_POPUP_HOTKEY, hotkey::DEFAULT_POPUP_HOTKEY)
        .map_err(map_err)
}

/// The hard-coded default. Used by the frontend to display "reset" / "default" hints.
#[tauri::command]
pub fn get_popup_hotkey_default() -> String {
    hotkey::DEFAULT_POPUP_HOTKEY.to_string()
}

/// Set the popup hotkey: validate not colliding with the other globals
/// (OCR / Screenshot / Eyedropper / Finder / expander / direct slots),
/// re-register, persist. On any failure (invalid string, collision)
/// the **old** popup hotkey stays armed — nothing is persisted, so a
/// user can keep clicking around safely.
#[tauri::command]
pub fn set_popup_hotkey(
    app: AppHandle,
    db: State<'_, DbHandle>,
    state: State<'_, hotkey::PopupShortcutState>,
    hotkey: String,
) -> Result<String, String> {
    hotkey::register_popup(&app, &state, &hotkey).map_err(map_err)?;
    settings::set(&db, hotkey::KEY_POPUP_HOTKEY, &hotkey).map_err(map_err)?;
    Ok(hotkey)
}

// ── Clipboard-history hotkey (second popup hotkey) ────────────────────────

/// Read the user-configured clipboard-history hotkey (or the default).
#[tauri::command]
pub fn get_history_hotkey(db: State<'_, DbHandle>) -> Result<String, String> {
    settings::get_or(&db, hotkey::KEY_HISTORY_HOTKEY, hotkey::DEFAULT_HISTORY_HOTKEY)
        .map_err(map_err)
}

/// The hard-coded default for the clipboard-history hotkey.
#[tauri::command]
pub fn get_history_hotkey_default() -> String {
    hotkey::DEFAULT_HISTORY_HOTKEY.to_string()
}

/// Set the second (clipboard-history) popup hotkey. An empty string disables
/// it. Validates against the other globals + the main popup hotkey; on failure
/// the old binding stays and nothing is persisted.
#[tauri::command]
pub fn set_history_hotkey(
    app: AppHandle,
    db: State<'_, DbHandle>,
    state: State<'_, hotkey::PopupShortcutState>,
    hotkey: String,
) -> Result<String, String> {
    hotkey::register_history_hotkey(&app, &state, &hotkey).map_err(map_err)?;
    settings::set(&db, hotkey::KEY_HISTORY_HOTKEY, &hotkey).map_err(map_err)?;
    Ok(hotkey)
}

// ── TOTP (2FA) ──────────────────────────────────────────────────────────

/// List all TOTP entries (without the secret). Sorted by issuer.
#[tauri::command]
pub fn totp_list(db: State<'_, DbHandle>) -> Result<Vec<crate::totp_store::TotpEntry>, String> {
    crate::totp_store::list(&db).map_err(map_err)
}

/// Add a new TOTP entry. Secret is base32-encoded per RFC 4648 (the
/// format every authenticator QR code uses). Defaults: 6 digits, 30s, SHA1.
#[tauri::command]
pub fn totp_add(
    db: State<'_, DbHandle>,
    issuer: String,
    account: String,
    secret: String,
    digits: Option<u32>,
    period: Option<u32>,
    algorithm: Option<String>,
) -> Result<crate::totp_store::TotpEntry, String> {
    crate::totp_store::add(
        &db,
        &issuer,
        &account,
        &secret,
        digits.unwrap_or(6),
        period.unwrap_or(30),
        &algorithm.unwrap_or_else(|| "SHA1".into()),
    )
    .map_err(map_err)
}

#[tauri::command]
pub fn totp_delete(db: State<'_, DbHandle>, id: i64) -> Result<(), String> {
    crate::totp_store::delete(&db, id).map_err(map_err)
}

/// Persist a manual drag-reorder: `ids` in the desired top-to-bottom order.
#[tauri::command]
pub fn totp_set_order(db: State<'_, DbHandle>, ids: Vec<i64>) -> Result<(), String> {
    crate::totp_store::set_order(&db, &ids).map_err(map_err)
}

/// Update every field of an entry (incl. the secret, re-encrypted).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn totp_update(
    db: State<'_, DbHandle>,
    id: i64,
    issuer: String,
    account: String,
    secret: String,
    digits: Option<u32>,
    period: Option<u32>,
    algorithm: Option<String>,
) -> Result<(), String> {
    crate::totp_store::update(
        &db,
        id,
        &issuer,
        &account,
        &secret,
        digits.unwrap_or(6),
        period.unwrap_or(30),
        &algorithm.unwrap_or_else(|| "SHA1".into()),
    )
    .map_err(map_err)
}

/// Remove duplicate entries (same issuer+account+secret). Returns the count.
#[tauri::command]
pub fn totp_remove_duplicates(db: State<'_, DbHandle>) -> Result<usize, String> {
    crate::totp_store::remove_duplicates(&db).map_err(map_err)
}

/// Delete every entry. Returns the count removed.
#[tauri::command]
pub fn totp_delete_all(db: State<'_, DbHandle>) -> Result<usize, String> {
    crate::totp_store::delete_all(&db).map_err(map_err)
}

/// Current code + seconds-until-next-roll for a single entry.
#[tauri::command]
pub fn totp_current_code(
    db: State<'_, DbHandle>,
    id: i64,
) -> Result<crate::totp_store::TotpCode, String> {
    crate::totp_store::current_code(&db, id).map_err(map_err)
}

/// Current codes for every entry in one shot — the management
/// overlay polls this once a second instead of N IPCs.
#[tauri::command]
pub fn totp_current_codes_all(
    db: State<'_, DbHandle>,
) -> Result<Vec<TotpCodeEntry>, String> {
    let codes = crate::totp_store::current_codes_all(&db).map_err(map_err)?;
    Ok(codes
        .into_iter()
        .map(|(id, c)| TotpCodeEntry {
            id,
            code: c.code,
            seconds_remaining: c.seconds_remaining,
        })
        .collect())
}

#[derive(serde::Serialize)]
pub struct TotpCodeEntry {
    pub id: i64,
    pub code: String,
    pub seconds_remaining: u32,
}

/// Import TOTP entries from any of the supported formats (otpauth://,
/// otpauth-migration://, Aegis JSON, 2FAS JSON, plaintext URI list).
/// Per-entry failures are silently dropped — the count of successes
/// is what the UI reports.
#[derive(serde::Serialize)]
pub struct TotpImportResult {
    pub added: usize,
    #[serde(default)]
    pub skipped: usize,
    /// Entries that parsed but failed validation at insert (e.g. empty issuer,
    /// undecodable secret) — surfaced so the UI never under-reports drops.
    #[serde(default)]
    pub failed: usize,
    pub error: Option<String>,
}

#[tauri::command]
pub fn totp_import(
    db: State<'_, DbHandle>,
    input: String,
) -> Result<TotpImportResult, String> {
    let parsed = match crate::totp_import::import_auto(&input) {
        Ok(p) => p,
        Err(e) => {
            return Ok(TotpImportResult {
                added: 0,
                skipped: 0,
                failed: 0,
                error: Some(format!("{e:#}")),
            });
        }
    };
    // Skip entries already present (same issuer+account+secret) so re-importing
    // the same export doesn't create duplicates. The set also dedups within this
    // one import (an export containing the same entry twice).
    let mut seen = crate::totp_store::existing_keys(&db).unwrap_or_default();
    let mut added = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    for entry in parsed {
        let key = crate::totp_store::dedup_key(&entry.issuer, &entry.account, &entry.secret_base32);
        if !seen.insert(key) {
            skipped += 1;
            continue;
        }
        match crate::totp_store::add(
            &db,
            &entry.issuer,
            &entry.account,
            &entry.secret_base32,
            entry.digits,
            entry.period,
            &entry.algorithm,
        ) {
            Ok(_) => added += 1,
            Err(e) => {
                failed += 1;
                tracing::warn!("totp_import: skipping entry {entry:?}: {e:#}");
            }
        }
    }
    Ok(TotpImportResult { added, skipped, failed, error: None })
}

/// Import TOTP entries from a **file path** (drag-and-drop). Reads the file as
/// UTF-8 text and runs the same autodetecting importer as `totp_import`.
#[tauri::command]
pub fn totp_import_file(db: State<'_, DbHandle>, path: String) -> Result<TotpImportResult, String> {
    let input = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            return Ok(TotpImportResult {
                added: 0,
                skipped: 0,
                failed: 0,
                error: Some(format!("Couldn't read {path}: {e}")),
            });
        }
    };
    totp_import(db, input)
}

/// Export all entries as a newline-separated list of `otpauth://`
/// URIs. **Plaintext** — the user must understand they're holding the
/// crown jewels of their 2FA. UI hint says so.
#[tauri::command]
pub fn totp_export(db: State<'_, DbHandle>) -> Result<String, String> {
    let uris = crate::totp_import::export_otpauth_uris(&db).map_err(map_err)?;
    Ok(uris.join("\n"))
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
    // The focus-settle delay must NOT run on the main thread — sleeping the
    // AppKit run loop for 250 ms freezes the whole UI. Wait on a worker, then
    // dispatch only the enigo synthesis to the main thread (enigo's macOS TSM
    // mapping asserts main-thread).
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let _ = app2.clone().run_on_main_thread(move || {
            if let Some(db) = app2.try_state::<DbHandle>() {
                let watcher = app2.try_state::<WatcherState>();
                if let Err(e) = expander::expand_at_cursor(&db, watcher.as_deref()) {
                    tracing::warn!("expand_at_cursor failed: {e:#}");
                }
            }
        });
    });
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
    // Focus-settle delay off the main thread (see `trigger_expand_at_cursor`);
    // only the capture itself is dispatched onto the main thread.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(250));
        let tx2 = tx.clone();
        let dispatched = app2.clone().run_on_main_thread(move || {
            let result = match app2.try_state::<DbHandle>() {
                Some(db) => expander::diagnose_at_cursor(&db).map_err(|e| e.to_string()),
                None => Err("db state not initialized".to_string()),
            };
            let _ = tx2.send(result);
        });
        if let Err(e) = dispatched {
            let _ = tx.send(Err(format!("dispatch to main thread: {e}")));
        }
    });
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

/// Copy a frontend-rendered PNG (base64) to the clipboard + history. Used by
/// the `qr` command, which renders the QR to a canvas and hands the PNG here.
#[tauri::command]
pub fn qr_copy_png(
    app: AppHandle,
    db: State<'_, DbHandle>,
    watcher: State<'_, WatcherState>,
    png_b64: String,
    label: String,
) -> Result<i64, String> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};

    let bytes = B64
        .decode(png_b64.as_bytes())
        .map_err(|e| format!("base64 decode: {e}"))?;
    // Put the QR image on the clipboard. The canonical b64 it returns is the PNG
    // re-encoded through clipboard-rs's encoder — exactly what the watcher reads
    // back — so we arm the fuse + store *that* payload. Otherwise the watcher's
    // read-back b64 wouldn't match the frontend-canvas b64 and a duplicate
    // `[image W×H]` entry would land next to the intended `[qr · …]` one.
    let canon_b64 = crate::image_ops::write_clipboard_png_canonical(&bytes).map_err(map_err)?;
    watcher.mark_self_write(crate::models::ContentType::Image, &canon_b64);

    let byte_size = (canon_b64.len() * 3 / 4) as i64; // decoded bytes ≈ b64 len × 3/4
    let summary = if label.trim().is_empty() {
        format!("[qr · {byte_size} B]")
    } else {
        format!("[qr · {}]", label.trim())
    };
    let new_id = db::upsert_clip(
        &db,
        &crate::models::NewClip {
            content_type: crate::models::ContentType::Image,
            content_text: summary,
            content_data: canon_b64,
            byte_size,
        },
    )
    .map_err(map_err)?;
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
            if let Err(e) = db::upsert_clip(
                &db,
                &crate::models::NewClip {
                    content_type: crate::models::ContentType::Image,
                    content_text: summary,
                    content_data: b64,
                    byte_size,
                },
            ) {
                tracing::warn!("OCR: failed to save source image to history: {e:#}");
            }
        }
        // Then the recognised text — this becomes the most-recent
        // entry, matching what's on the clipboard and what Enter will
        // paste.
        if let Err(e) = db::upsert_clip(
            &db,
            &crate::models::NewClip {
                content_type: crate::models::ContentType::Text,
                content_text: trimmed.to_string(),
                content_data: trimmed.to_string(),
                byte_size: trimmed.len() as i64,
            },
        ) {
            tracing::warn!("OCR: failed to save recognised text to history: {e:#}");
        }
    }
    let _ = app.emit("clipboard-changed", ());
    crate::sound::play(crate::sound::Sound::Ocr);

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
    run_capture_pipeline(app, region_picker::CaptureMode::Region, 0)
}

/// Generalised screenshot pipeline (v0.57.0): capture via `mode` after an
/// optional `delay_seconds` self-timer, then the same staging → clipboard →
/// floating-preview flow as the region path. `run_screenshot_pipeline` is the
/// region/no-delay shorthand used by the tray + `Ctrl+Shift+S`.
pub fn run_capture_pipeline(
    app: &AppHandle,
    mode: region_picker::CaptureMode,
    delay_seconds: u32,
) -> Result<ScreenshotResult, String> {
    if !screen_recording::screen_recording_granted() {
        return Err(ERR_NO_SCREEN_RECORDING.to_string());
    }

    // Capture the frontmost app name BEFORE hiding the popup or
    // starting screencapture — once those run, focus may already
    // have shifted to System Events / our own process. macOS-only;
    // best-effort, never fails the pipeline.
    let captured_app_name = crate::frontmost_app::name();

    hotkey::hide_popup(app);

    // Self-timer: wait before capturing so the user can set up the shot.
    // Capped at 60 s defensively. The popup is already hidden.
    if delay_seconds > 0 {
        std::thread::sleep(std::time::Duration::from_secs(delay_seconds.min(60) as u64));
    }

    let png_bytes = match mode.capture() {
        Ok(b) => b,
        Err(e) => {
            if e.downcast_ref::<region_picker::Cancelled>().is_some() {
                return Ok(ScreenshotResult { cancelled: true, bytes: 0 });
            }
            return Err(format!("{} capture failed: {e:#}", mode.as_str()));
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

    // Stash the path + app name in shared state so the preview-window
    // IPCs can pick them up without the frontend round-tripping. When
    // the previous preview is pinned, we still write the new PNG to
    // clipboard + history (already done above) but DON'T replace the
    // pinned preview's pending entry or re-show the window. The new
    // shot still ends up everywhere it normally would; just the
    // floating preview stays put.
    let pinned = app
        .try_state::<crate::screenshot_preview::PendingScreenshot>()
        .map(|p| {
            p.inner()
                .pinned
                .load(std::sync::atomic::Ordering::SeqCst)
        })
        .unwrap_or(false);

    if !pinned {
        if let Some(pending) =
            app.try_state::<crate::screenshot_preview::PendingScreenshot>()
        {
            *pending.inner().current.lock() =
                Some(crate::screenshot_preview::Pending {
                    path: temp_path.clone(),
                    app_name: captured_app_name.clone(),
                    // A fresh capture lives in the cache dir — discardable.
                    saved: false,
                });
        } else {
            tracing::warn!("PendingScreenshot state missing — preview won't work");
        }

        // Build (or reuse) and position the preview window. Failure isn't
        // fatal — the temp PNG is still on disk and the user can rerun.
        if let Err(e) = crate::screenshot_preview::show_preview(app) {
            tracing::warn!("screenshot preview window: {e:#}");
        }
    } else {
        tracing::info!("screenshot preview pinned — keeping existing preview, new PNG only goes to clipboard");
    }

    let _ = app.emit("clipboard-changed", ());
    crate::sound::play(crate::sound::Sound::Screenshot);

    Ok(ScreenshotResult { cancelled: false, bytes: png_bytes.len() })
}

/// IPC entry point. Same threading note as `ocr_region` — the Tauri
/// IPC layer already provides a worker thread.
#[tauri::command]
pub fn screenshot_region(app: AppHandle) -> Result<ScreenshotResult, String> {
    run_screenshot_pipeline(&app)
}

/// Settings key: the last capture mode used (for `screenshot_repeat_last`).
const KEY_SHOT_LAST_MODE: &str = "screenshot.last_mode";

/// Capture in a specific `mode` ("region" | "fullscreen" | "window") with an
/// optional self-timer `delay_seconds`. Remembers the mode so
/// `screenshot_repeat_last` can replay it. (v0.57.0)
#[tauri::command]
pub fn screenshot_capture(
    app: AppHandle,
    db: State<'_, DbHandle>,
    mode: String,
    delay_seconds: Option<u32>,
) -> Result<ScreenshotResult, String> {
    let m = region_picker::CaptureMode::from_str_loose(&mode);
    let _ = settings::set(&db, KEY_SHOT_LAST_MODE, m.as_str());
    run_capture_pipeline(&app, m, delay_seconds.unwrap_or(0))
}

/// Repeat the last capture mode (defaults to region if none stored). (v0.57.0)
#[tauri::command]
pub fn screenshot_repeat_last(
    app: AppHandle,
    db: State<'_, DbHandle>,
) -> Result<ScreenshotResult, String> {
    let stored = settings::get_or(&db, KEY_SHOT_LAST_MODE, "region")
        .unwrap_or_else(|_| "region".to_string());
    let m = region_picker::CaptureMode::from_str_loose(&stored);
    run_capture_pipeline(&app, m, 0)
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
        // Custom loupe with the live hex under it (snapshot magnified in an
        // overlay). On pick it writes the hex to clipboard + history; on cancel
        // it just restores focus — same outcomes as the old NSColorSampler path.
        open_color_loupe(app, false);
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

// ── Custom screen loupe (eyedropper with live hex under the loupe) ─────────
// macOS: a one-shot snapshot of the cursor's display, magnified in a fullscreen
// overlay webview where the live hex is rendered under the loupe (Apple's
// NSColorSampler can't show that). `event_mode` distinguishes the two callers:
// the modal "pick from screen" (emits `color-picked`) vs. the hotkey/tray
// eyedropper (writes the hex to clipboard + history).

pub const LOUPE_LABEL: &str = "color-loupe";

#[derive(serde::Serialize)]
pub struct LoupeData {
    b64: String,
    event_mode: bool,
}

/// Capture the cursor's display + open the loupe overlay. The capture +
/// window build run on a worker thread (screencapture blocks ~100 ms, and
/// building a window from a worker lets Tauri marshal it onto the event loop —
/// the proven pattern from the record stop bar).
pub fn open_color_loupe(app: &AppHandle, event_mode: bool) {
    let app2 = app.clone();
    std::thread::spawn(move || {
        let idx = cursor_display_index(&app2);
        let b64 = match crate::color_loupe::capture_display_b64(idx) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("color loupe capture failed: {e:#}");
                if event_mode {
                    clear_pick_suppress_hide(&app2);
                } else {
                    clear_eyedropper_no_popup(&app2);
                }
                return;
            }
        };
        if let Some(st) = app2.try_state::<crate::color_loupe::LoupeState>() {
            *st.0.lock() = Some(crate::color_loupe::Session { b64, event_mode });
        }
        build_loupe_overlay(&app2);
        let app_esc = app2.clone();
        std::thread::spawn(move || arm_loupe_escape(&app_esc));
    });
}

/// 1-based index (for `screencapture -D`) of the display under the cursor.
/// Matches the cursor's monitor (via the proven global cursor query) against
/// the active-display list order. macOS only; elsewhere returns 1 (the
/// fullscreen fallback ignores it).
fn cursor_display_index(app: &AppHandle) -> usize {
    #[cfg(target_os = "macos")]
    {
        if let Some(w) = app.get_webview_window(crate::hotkey::POPUP_LABEL) {
            let monitors = w.available_monitors().unwrap_or_default();
            if let Some(m) = crate::screenshot_preview::pick_cursor_monitor_globally(&monitors) {
                let p = m.position();
                let rects = crate::screen_record::cg_displays::physical_rects();
                if !rects.is_empty() {
                    let mut best = 0usize;
                    let mut best_d = i64::MAX;
                    for (i, (rx, ry, _, _)) in rects.iter().enumerate() {
                        let dx = (*rx - p.x) as i64;
                        let dy = (*ry - p.y) as i64;
                        let d = dx * dx + dy * dy;
                        if d < best_d {
                            best_d = d;
                            best = i;
                        }
                    }
                    return best + 1;
                }
            }
        }
    }
    let _ = app;
    1
}

fn build_loupe_overlay(app: &AppHandle) {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    if let Some(existing) = app.get_webview_window(LOUPE_LABEL) {
        let _ = existing.close();
    }
    let win = match WebviewWindowBuilder::new(
        app,
        LOUPE_LABEL,
        WebviewUrl::App("index.html".into()),
    )
    .title("Color loupe")
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .build()
    {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!("build color loupe overlay: {e}");
            return;
        }
    };
    // Cover the cursor's monitor (same approach + caveats as the record overlay).
    let monitors = win.available_monitors().unwrap_or_default();
    let geom = crate::screenshot_preview::pick_cursor_monitor_globally(&monitors)
        .or_else(|| win.primary_monitor().ok().flatten())
        .map(|m| {
            let p = m.position();
            let s = m.size();
            (p.x, p.y, s.width, s.height)
        });
    let apply = |w: &tauri::WebviewWindow| {
        if let Some((x, y, ww, hh)) = geom {
            let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
            let _ = w.set_size(tauri::PhysicalSize::new(ww, hh));
        }
    };
    apply(&win);
    let _ = win.show();
    apply(&win);
    let _ = win.set_focus();
    if let Some((x, y, ww, hh)) = geom {
        let app_d = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(90));
            let app_m = app_d.clone();
            let _ = app_d.run_on_main_thread(move || {
                if let Some(w) = app_m.get_webview_window(LOUPE_LABEL) {
                    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
                    let _ = w.set_size(tauri::PhysicalSize::new(ww, hh));
                }
            });
        });
    }
}

fn arm_loupe_escape(app: &AppHandle) {
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutState};
    let esc = Shortcut::new(None, Code::Escape);
    let _ = app.global_shortcut().unregister(esc);
    let app2 = app.clone();
    if let Err(e) = app.global_shortcut().on_shortcut(esc, move |_a, _sc, event| {
        if event.state == ShortcutState::Pressed {
            let app3 = app2.clone();
            std::thread::spawn(move || do_cancel_loupe(&app3));
        }
    }) {
        tracing::debug!("arm_loupe_escape: couldn't register global Esc: {e:#}");
    }
}

fn disarm_loupe_escape(app: &AppHandle) {
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut};
    let _ = app
        .global_shortcut()
        .unregister(Shortcut::new(None, Code::Escape));
}

fn finish_loupe(app: &AppHandle) {
    disarm_loupe_escape(app);
    if let Some(w) = app.get_webview_window(LOUPE_LABEL) {
        let _ = w.close();
    }
}

fn take_loupe_mode(app: &AppHandle) -> bool {
    app.try_state::<crate::color_loupe::LoupeState>()
        .and_then(|st| st.0.lock().take())
        .map(|s| s.event_mode)
        .unwrap_or(false)
}

fn do_pick_loupe(app: &AppHandle, hex: String) {
    let event_mode = take_loupe_mode(app);
    finish_loupe(app);
    if event_mode {
        let _ = app.emit("color-picked", hex);
        clear_pick_suppress_hide(app);
    } else {
        write_eyedropper_result(app, &hex);
        clear_eyedropper_no_popup(app);
    }
}

fn do_cancel_loupe(app: &AppHandle) {
    let event_mode = take_loupe_mode(app);
    finish_loupe(app);
    if event_mode {
        let _ = app.emit("color-picked", Option::<String>::None);
        clear_pick_suppress_hide(app);
    } else {
        clear_eyedropper_no_popup(app);
    }
}

/// The loupe overlay fetches its snapshot + mode.
#[tauri::command]
pub fn color_loupe_data(
    state: State<'_, crate::color_loupe::LoupeState>,
) -> Option<LoupeData> {
    state.0.lock().as_ref().map(|s| LoupeData {
        b64: s.b64.clone(),
        event_mode: s.event_mode,
    })
}

/// The user clicked a pixel — commit the picked hex.
#[tauri::command]
pub fn color_loupe_pick(app: AppHandle, hex: String) {
    do_pick_loupe(&app, hex);
}

/// The user dismissed the loupe (Esc / click-away).
#[tauri::command]
pub fn color_loupe_cancel(app: AppHandle) {
    do_cancel_loupe(&app);
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

/// `touch <name>` — create an empty file in the frontmost file-manager
/// window's folder (Finder on macOS, Explorer on Windows), or the Desktop
/// when none is open. Returns the absolute path created. On macOS this needs
/// the Automation→Finder TCC grant (returns the `finder.automation_denied`
/// sentinel on a miss).
#[tauri::command]
pub fn finder_touch(name: String, content: Option<String>) -> Result<String, String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        crate::finder_selection::create_file(&name, content.as_deref().unwrap_or(""))
            .map(|p| p.display().to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (name, content);
        Err("touch needs Finder (macOS) or Explorer (Windows)".into())
    }
}

/// `mkdir <name>` — create a folder in the frontmost file-manager window's
/// folder (Finder on macOS, Explorer on Windows), or the Desktop when none
/// is open. Returns the absolute path created.
#[tauri::command]
pub fn finder_mkdir(name: String) -> Result<String, String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        crate::finder_selection::create_dir(&name).map(|p| p.display().to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = name;
        Err("mkdir needs Finder (macOS) or Explorer (Windows)".into())
    }
}

/// `terminal` — open the user's terminal at the frontmost file-manager
/// window's folder: iTerm2/Terminal.app on macOS, Windows Terminal / PowerShell
/// / cmd on Windows (Explorer folder, or Desktop when none is open). Returns
/// the directory opened.
#[tauri::command]
pub fn finder_open_terminal() -> Result<String, String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        crate::finder_selection::open_terminal().map(|p| p.display().to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("terminal needs Finder (macOS) or Explorer (Windows)".into())
    }
}

/// `md2pdf [path]` — Markdown → PDF, the same action as the Ctrl+Shift+M
/// hotkey but invokable from the search bar. With `path` it converts that
/// file; bare, it converts the file-manager selection. Spawns a worker so
/// the command returns immediately (macOS WKWebView rendering needs the
/// main thread, dispatched from the worker; Windows uses Edge headless on
/// the worker). Result is surfaced via the same notification as the hotkey.
#[tauri::command]
pub fn md_to_pdf_run(app: AppHandle, path: Option<String>) -> Result<(), String> {
    // Resolve the target paths up front so a bad/empty selection errors
    // synchronously (the frontend can show it) before we spawn.
    let arg_path = path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
    let paths: Vec<std::path::PathBuf> = if let Some(p) = arg_path {
        vec![std::path::PathBuf::from(p)]
    } else {
        #[cfg(target_os = "macos")]
        {
            crate::finder_selection::read()?
        }
        #[cfg(not(target_os = "macos"))]
        {
            return Err("md2pdf: pass a file path (selection reading is macOS-only for now)".into());
        }
    };
    if paths.is_empty() {
        return Err("md2pdf: nothing selected (and no path given)".into());
    }

    let app2 = app.clone();
    std::thread::spawn(move || {
        // macOS: WKWebView render must run on the main thread; bounce
        // through a oneshot channel. Other platforms convert in-thread.
        #[cfg(target_os = "macos")]
        let summary = {
            let (tx, rx) = std::sync::mpsc::channel::<crate::md_to_pdf::ConvertSummary>();
            let _ = app2.run_on_main_thread(move || {
                let _ = tx.send(crate::md_to_pdf::convert_files(&paths));
            });
            rx.recv().unwrap_or_default()
        };
        #[cfg(not(target_os = "macos"))]
        let summary = crate::md_to_pdf::convert_files(&paths);

        tracing::info!(
            "md2pdf: {} converted, {} skipped, {} failed",
            summary.converted.len(),
            summary.skipped.len(),
            summary.failed.len()
        );
        crate::md_to_pdf::notify(&summary);
    });
    Ok(())
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

/// Read the current system output volume (0–100), or null if no read-back.
#[tauri::command]
pub fn get_system_volume() -> Option<u8> {
    crate::system_commands::get_system_volume()
}

/// Set the system output volume to an absolute level; returns the applied level.
#[tauri::command]
pub fn set_system_volume(level: i32) -> Option<u8> {
    crate::system_commands::set_system_volume(level)
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
        if let Err(e) = db::upsert_clip(
            &db,
            &crate::models::NewClip {
                content_type: crate::models::ContentType::Text,
                content_text: text.clone(),
                content_data: text.clone(),
                byte_size: text.len() as i64,
            },
        ) {
            tracing::warn!("transform: failed to save transformed text to history: {e:#}");
        }
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
mod window_size_tests {
    use super::{normalise_window_size, window_size_dimensions};

    #[test]
    fn passes_through_the_three_valid_presets() {
        assert_eq!(normalise_window_size("small"), "small");
        assert_eq!(normalise_window_size("medium"), "medium");
        assert_eq!(normalise_window_size("large"), "large");
    }

    #[test]
    fn collapses_unknown_to_medium() {
        assert_eq!(normalise_window_size("huge"), "medium");
        assert_eq!(normalise_window_size(""), "medium");
        assert_eq!(normalise_window_size("SMALL"), "medium"); // case-sensitive
        assert_eq!(normalise_window_size(" small "), "medium"); // no trimming
    }

    #[test]
    fn dimensions_grow_monotonically_with_preset() {
        let (sw, sh) = window_size_dimensions("small");
        let (mw, mh) = window_size_dimensions("medium");
        let (lw, lh) = window_size_dimensions("large");
        assert!(sw < mw && mw < lw, "widths must increase small < medium < large");
        assert!(sh < mh && mh < lh, "heights must increase small < medium < large");
        // Medium stays the historical default the window ships with.
        assert_eq!((mw, mh), (700.0, 500.0));
    }

    #[test]
    fn unknown_dimensions_fall_back_to_medium() {
        assert_eq!(window_size_dimensions("garbage"), (700.0, 500.0));
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
    crate::sound::play(crate::sound::Sound::Copy);
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

// ── Linux desktop shortcuts (GNOME/Cinnamon gsettings) ───────────────────────

#[derive(Debug, Deserialize)]
pub struct LinuxShortcutBindingInput {
    pub id: String,
    pub binding: String,
}

/// Scan occupied keys, terminal conflicts, and recommended bindings.
#[cfg(target_os = "linux")]
#[tauri::command]
pub fn linux_scan_desktop_shortcuts(
    db: State<'_, DbHandle>,
) -> Result<desktop_shortcuts::ShortcutSetupScan, String> {
    desktop_shortcuts::scan_shortcut_setup(&db).map_err(map_err)
}

/// Apply user-chosen bindings (or auto-pick when `bindings` is empty).
#[cfg(target_os = "linux")]
#[tauri::command]
pub fn linux_apply_desktop_shortcuts(
    db: State<'_, DbHandle>,
    bindings: Vec<LinuxShortcutBindingInput>,
) -> Result<(), String> {
    let pairs: Vec<(String, String)> = bindings
        .into_iter()
        .map(|b| (b.id, b.binding))
        .collect();
    desktop_shortcuts::apply_shortcut_setup(&db, pairs).map_err(map_err)
}

/// Convert a recorded W3C hotkey to GNOME gsettings accel format.
#[cfg(target_os = "linux")]
#[tauri::command]
pub fn linux_web_hotkey_to_gsettings(shortcut: String) -> Result<String, String> {
    desktop_shortcuts::web_hotkey_to_gsettings(&shortcut)
}

// ── Cleaning workflow (v0.60.0) ────────────────────────────────────────────────

/// Read-only dry-run scan for the current cleaner config. Returns the plan
/// (paths + sizes + per-category totals) so the frontend can show a preview.
/// Deletes nothing.
#[tauri::command]
pub fn cleaner_scan(db: State<'_, DbHandle>) -> Result<cleaner::CleanPlan, String> {
    let cfg = cleaner::load_config(&db);
    Ok(cleaner::scan(&cfg))
}

/// Execute a previously-scanned plan. Re-validates every path against the
/// current config's allowlist (containment + non-symlink) before deleting, so
/// a stale or tampered plan can't escape the cache roots. Returns counts +
/// freed bytes + per-path errors (never aborts the batch).
#[tauri::command]
pub fn cleaner_execute(
    db: State<'_, DbHandle>,
    plan: cleaner::CleanPlan,
) -> Result<cleaner::CleanResult, String> {
    let cfg = cleaner::load_config(&db);
    Ok(cleaner::execute(&cfg, &plan))
}

#[tauri::command]
pub fn get_cleaner_config(db: State<'_, DbHandle>) -> Result<cleaner::CleanerConfig, String> {
    Ok(cleaner::load_config(&db))
}

/// The full category catalogue for this OS (key + label + level +
/// default_enabled) so Settings can render per-category checkboxes.
#[tauri::command]
pub fn cleaner_categories() -> Vec<cleaner::Category> {
    cleaner::categories()
}

#[tauri::command]
pub fn set_cleaner_config(
    db: State<'_, DbHandle>,
    config: cleaner::CleanerConfig,
) -> Result<cleaner::CleanerConfig, String> {
    cleaner::save_config(&db, &config).map_err(map_err)?;
    Ok(cleaner::load_config(&db))
}

// ── Monitor brightness (v0.62.0) ───────────────────────────────────────────────

/// Enumerate DDC-capable monitors with their current brightness. Slow (probes
/// each display) — the overlay calls it once on open.
#[tauri::command]
pub fn list_brightness_monitors() -> Vec<crate::brightness::MonitorInfo> {
    crate::brightness::enumerate()
}

#[tauri::command]
pub fn get_monitor_brightness(id: u32) -> Result<u8, String> {
    crate::brightness::get(id)
}

/// Set monitor `id` to `percent` (0–100). The frontend debounces during a
/// slider drag so we don't flood the (sometimes slow) DDC bus.
#[tauri::command]
pub fn set_monitor_brightness(id: u32, percent: u8) -> Result<(), String> {
    crate::brightness::set(id, percent)
}

/// Drive the EDR boost for monitor `id` to `percent` (the full slider value;
/// ≤ 100 / 0 = off). macOS EDR-capable displays only; a no-op elsewhere.
#[tauri::command]
pub fn set_edr_level(app: AppHandle, id: u32, percent: u16) {
    crate::brightness::set_edr_level(&app, id, percent);
}

/// List the system audio output devices (`sound` command). Marks the default.
#[tauri::command]
pub fn list_audio_outputs() -> Result<Vec<crate::audio::AudioDevice>, String> {
    crate::audio::list_outputs()
}

/// Set the default audio output device by its opaque per-platform id.
#[tauri::command]
pub fn set_audio_output(id: String) -> Result<(), String> {
    crate::audio::set_output(&id)
}

/// System uptime in whole seconds for the live `uptime` command. Cheap (a
/// single sysctl/proc read, no `System` instance). The frontend anchors this to
/// a high-resolution timer and animates the sub-second digits down to µs.
#[tauri::command]
pub fn get_uptime_secs() -> u64 {
    sysinfo::System::uptime()
}

/// One live snapshot of system stats for the `stats` command (CPU / memory /
/// disks / network / temps / fans / battery). Blocks ~200 ms (CPU sample
/// window) — Tauri runs sync commands off the main thread, so the popup UI
/// stays responsive; the frontend polls this on an interval.
#[tauri::command]
pub fn get_system_stats() -> crate::system_stats::SystemStats {
    crate::system_stats::gather()
}

/// Downsampled system-stats history over the last `range_secs` seconds (for the
/// Stats panel's "history" view). Backed by the always-on background sampler.
#[tauri::command]
pub fn get_stats_history(
    db: State<'_, DbHandle>,
    range_secs: i64,
) -> crate::stats_history::StatsHistory {
    crate::stats_history::history(&db, range_secs)
}

// ── Philips Hue (`hue` command, v0.84.40) ───────────────────────────────────

/// Connection status: do we have a bridge IP + paired username, and does the
/// bridge answer? Drives the connect-vs-control branch in the Hue panel.
#[tauri::command]
pub fn hue_status(db: State<'_, DbHandle>) -> crate::hue::HueStatus {
    let bridge_ip = crate::hue::bridge_ip(&db);
    let user = crate::hue::username(&db);
    let paired = bridge_ip.is_some() && user.is_some();
    // "connected" = we can actually list lights right now.
    let connected = match (&bridge_ip, &user) {
        (Some(ip), Some(u)) => crate::hue::list_lights(ip, u).is_ok(),
        _ => false,
    };
    crate::hue::HueStatus { connected, bridge_ip, paired }
}

/// Best-effort local SSDP discovery of a bridge IP (no cloud). May take ~3 s;
/// the frontend calls it on a button press, not on mount.
#[tauri::command]
pub fn hue_discover() -> Option<String> {
    crate::hue::discover_bridge()
}

/// Persist a manually-entered bridge IP (discovery fallback).
#[tauri::command]
pub fn hue_set_bridge_ip(db: State<'_, DbHandle>, ip: String) -> Result<(), String> {
    settings::set(&db, crate::hue::KEY_BRIDGE_IP, ip.trim()).map_err(map_err)
}

/// Pair with the bridge at `ip` (the user must have pressed the link button).
/// On success the created username is stored; the IP is stored too. Returns the
/// `hue.link_button` sentinel if the button wasn't pressed.
#[tauri::command]
pub fn hue_pair(db: State<'_, DbHandle>, ip: String) -> Result<(), String> {
    let ip = ip.trim().to_string();
    let user = crate::hue::pair(&ip)?;
    settings::set(&db, crate::hue::KEY_BRIDGE_IP, &ip).map_err(map_err)?;
    settings::set(&db, crate::hue::KEY_USERNAME, &user).map_err(map_err)?;
    Ok(())
}

/// Forget the stored bridge + username (re-pair from scratch).
#[tauri::command]
pub fn hue_forget(db: State<'_, DbHandle>) -> Result<(), String> {
    settings::set(&db, crate::hue::KEY_BRIDGE_IP, "").map_err(map_err)?;
    settings::set(&db, crate::hue::KEY_USERNAME, "").map_err(map_err)
}

fn hue_creds(db: &DbHandle) -> Result<(String, String), String> {
    let ip = crate::hue::bridge_ip(db).ok_or("hue.not_connected")?;
    let user = crate::hue::username(db).ok_or("hue.not_connected")?;
    Ok((ip, user))
}

/// List all lamps with their current state.
#[tauri::command]
pub fn hue_list_lights(db: State<'_, DbHandle>) -> Result<Vec<crate::hue::HueLight>, String> {
    let (ip, user) = hue_creds(&db)?;
    crate::hue::list_lights(&ip, &user)
}

/// Set a single lamp: on/off, optional brightness %, optional hex colour.
#[tauri::command]
pub fn hue_set_light(
    db: State<'_, DbHandle>,
    id: String,
    on: bool,
    brightness: Option<u8>,
    hex: Option<String>,
) -> Result<(), String> {
    let (ip, user) = hue_creds(&db)?;
    crate::hue::set_light(&ip, &user, &id, on, brightness, hex.as_deref())
}

/// Set **all** lamps at once (group 0): on/off, optional brightness %, colour.
#[tauri::command]
pub fn hue_set_all(
    db: State<'_, DbHandle>,
    on: bool,
    brightness: Option<u8>,
    hex: Option<String>,
) -> Result<(), String> {
    let (ip, user) = hue_creds(&db)?;
    crate::hue::set_all(&ip, &user, on, brightness, hex.as_deref())
}

// ── Screen recording (Ctrl+Shift+R, v0.81.0) ────────────────────────────────

pub const RECORD_OVERLAY_LABEL: &str = "record-overlay";
pub const RECORD_STOP_LABEL: &str = "record-stop";

/// Open the fullscreen region-select overlay (the start of the recording flow).
#[tauri::command]
pub fn screen_record_open_overlay(app: AppHandle) -> Result<(), String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    if let Some(state) = app.try_state::<crate::screen_record::RecordState>() {
        if state.is_recording() {
            return Err("already recording".into());
        }
    }
    if let Some(popup) = app.get_webview_window(hotkey::POPUP_LABEL) {
        let _ = popup.hide();
    }
    let _ = app.emit("popup-hidden", ());
    if let Some(existing) = app.get_webview_window(RECORD_OVERLAY_LABEL) {
        let _ = existing.close();
    }
    let win = WebviewWindowBuilder::new(
        &app,
        RECORD_OVERLAY_LABEL,
        WebviewUrl::App("index.html".into()),
    )
    .title("Select recording region")
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .build()
    .map_err(|e| format!("build record overlay: {e}"))?;
    // Cover the monitor under the cursor (fully). A single window can't reliably
    // span mixed-DPI monitors — the virtual-desktop bounding box in physical
    // pixels isn't coherent when a Retina (scale 2) and a non-Retina (scale 1)
    // display are combined, so the overlay only partially covered secondary
    // screens. One monitor's physical position+size ARE self-consistent, so
    // covering just the cursor's monitor is exact. To record a different screen,
    // move the cursor there before triggering.
    //
    // CRITICAL: pick the cursor's monitor via the GLOBAL cursor query
    // (`screenshot_preview::pick_cursor_monitor_globally` → `CGEventGetLocation`),
    // NOT `win.cursor_position()`. A freshly-built overlay window has never
    // received a mouse event, so its `cursor_position()` is stale and always
    // resolved to the primary monitor — which is why a selection on the
    // secondary screen never worked. This reuses the proven detection the
    // screenshot preview uses.
    let monitors = win.available_monitors().unwrap_or_default();
    let geom = crate::screenshot_preview::pick_cursor_monitor_globally(&monitors)
        .or_else(|| win.primary_monitor().ok().flatten())
        .map(|m| {
            let p = m.position();
            let s = m.size();
            (p.x, p.y, s.width, s.height)
        });
    let apply = |w: &tauri::WebviewWindow| {
        if let Some((x, y, ww, hh)) = geom {
            let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
            let _ = w.set_size(tauri::PhysicalSize::new(ww, hh));
        }
    };
    // Apply before show; some macOS configurations ignore sizing on a window
    // that hasn't been realised yet, so apply again right after show.
    apply(&win);
    let _ = win.show();
    apply(&win);
    let _ = win.set_focus();
    // Esc must abort from anywhere — the transparent overlay doesn't reliably
    // hold keyboard focus (the in-webview keydown listener needs a click first),
    // so register a GLOBAL Esc that cancels. Disarmed on cancel / record-start.
    //
    // CRITICAL: arm it on a WORKER thread, never inline. When this function runs
    // from the record hotkey, we are *inside* the global-shortcut event handler,
    // which holds the plugin's manager mutex; calling `global_shortcut()
    // .unregister`/`.on_shortcut` here re-enters that mutex → deadlock (the main
    // thread hangs forever and NO hotkey fires again — the v0.84.7 regression).
    // The worker blocks on the mutex only until the handler returns, then arms.
    let app_esc = app.clone();
    std::thread::spawn(move || arm_overlay_escape(&app_esc));
    // Deferred re-apply: `set_size(PhysicalSize)` converts physical→logical via
    // the window's CURRENT scale factor, which can still be the OLD monitor's
    // right after a move to a different-scale display (Retina ↔ non-Retina) —
    // leaving the overlay half/double sized. Re-assert the geometry once the
    // move + scale change have settled (off-main sleep → main-thread set).
    if let Some((x, y, ww, hh)) = geom {
        let app_defer = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(90));
            let app_main = app_defer.clone();
            let _ = app_defer.run_on_main_thread(move || {
                if let Some(w) = app_main.get_webview_window(RECORD_OVERLAY_LABEL) {
                    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
                    let _ = w.set_size(tauri::PhysicalSize::new(ww, hh));
                }
            });
        });
    }
    Ok(())
}

/// Register a temporary global Esc shortcut that cancels the record overlay, so
/// the user can abort the region selection without first clicking into the
/// (focus-less) overlay. Idempotent; the in-webview Esc listener stays as a
/// fallback if a bare-Escape global shortcut can't be registered on this OS.
fn arm_overlay_escape(app: &AppHandle) {
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutState};
    let esc = Shortcut::new(None, Code::Escape);
    let _ = app.global_shortcut().unregister(esc); // clear any stale registration
    let app2 = app.clone();
    if let Err(e) = app.global_shortcut().on_shortcut(esc, move |_a, _sc, event| {
        if event.state == ShortcutState::Pressed {
            // Defer off the shortcut callback (closing a window + unregistering
            // from inside the dispatch is best avoided).
            let app3 = app2.clone();
            std::thread::spawn(move || {
                let _ = cancel_record_overlay(app3);
            });
        }
    }) {
        tracing::debug!("arm_overlay_escape: couldn't register global Esc: {e:#}");
    }
}

/// Drop the temporary global Esc shortcut (overlay closed / recording started).
fn disarm_overlay_escape(app: &AppHandle) {
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut};
    let _ = app
        .global_shortcut()
        .unregister(Shortcut::new(None, Code::Escape));
}

/// Esc / cancel from the overlay — close it and drop the global Esc shortcut.
#[tauri::command]
pub fn cancel_record_overlay(app: AppHandle) -> Result<(), String> {
    disarm_overlay_escape(&app);
    if let Some(w) = app.get_webview_window(RECORD_OVERLAY_LABEL) {
        let _ = w.close();
    }
    Ok(())
}

/// Start recording the chosen region with the chosen audio tracks. Closes the
/// overlay and shows the floating stop bar. Returns the sentinel
/// `record.no_ffmpeg` if ffmpeg isn't installed (frontend shows an install hint).
#[tauri::command]
pub async fn start_screen_record(
    app: AppHandle,
    state: State<'_, crate::screen_record::RecordState>,
    region: crate::screen_record::RecordRegion,
    audio: crate::screen_record::AudioChoice,
) -> Result<(), String> {
    // The overlay sends the marquee rect relative to its own (multi-monitor-
    // spanning) window. Add the overlay window's screen position to get an
    // ABSOLUTE virtual-desktop region, so a selection on any monitor is
    // addressable. The overlay still exists here (closed just below).
    let region = match app
        .get_webview_window(RECORD_OVERLAY_LABEL)
        .and_then(|w| w.outer_position().ok())
    {
        Some(pos) => crate::screen_record::RecordRegion {
            x: region.x + pos.x,
            y: region.y + pos.y,
            w: region.w,
            h: region.h,
        },
        None => region,
    };
    // `async fn` so Tauri runs this OFF the main thread — `screen_record::start`
    // blocks ~0.5 s listing ffmpeg devices, which would otherwise freeze the UI.
    crate::screen_record::start(&state, region, audio)?;
    crate::sound::play(crate::sound::Sound::RecordStart);
    // Recording started → drop the global Esc cancel + close the overlay
    // (closing isn't a window build, so it's safe off-main).
    disarm_overlay_escape(&app);
    if let Some(w) = app.get_webview_window(RECORD_OVERLAY_LABEL) {
        let _ = w.close();
    }
    // Build the stop bar from a dedicated thread so Tauri marshals the window
    // creation onto the event loop cleanly (same proven pattern as
    // `screenshot_editor::open_editor`).
    let app2 = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = open_record_stop_bar(&app2) {
            tracing::warn!("open record stop bar: {e}");
        }
    });
    Ok(())
}

/// Pause the active recording (finalises the current segment). Synchronous
/// so the finalize-wait (up to 5 s) runs on Tauri's blocking thread pool.
#[tauri::command]
pub fn pause_screen_record(
    state: State<'_, crate::screen_record::RecordState>,
) -> Result<(), String> {
    crate::screen_record::pause(&state)
}

/// Resume a paused recording (starts a fresh segment). Synchronous so device
/// re-listing + spawn runs on a blocking thread.
#[tauri::command]
pub fn resume_screen_record(
    state: State<'_, crate::screen_record::RecordState>,
) -> Result<(), String> {
    crate::screen_record::resume(&state)
}

fn open_record_stop_bar(app: &AppHandle) -> Result<(), String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    if let Some(existing) = app.get_webview_window(RECORD_STOP_LABEL) {
        let _ = existing.close();
    }
    let win = WebviewWindowBuilder::new(app, RECORD_STOP_LABEL, WebviewUrl::App("index.html".into()))
        .title("Recording")
        .inner_size(312.0, 54.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(true)
        .visible(false)
        .build()
        .map_err(|e| format!("build stop bar: {e}"))?;
    // Position at the top-centre of the primary monitor's work area so it
    // never clips below the screen (the previous bottom-centre placement
    // could land behind the Windows taskbar or outside the visible bounds).
    if let Ok(Some(mon)) = win.primary_monitor() {
        let area = mon.work_area();
        let scale = mon.scale_factor();
        let bar_w = (312.0 * scale) as i32;
        let margin = (12.0 * scale) as i32;
        let x = area.position.x + (area.size.width as i32 - bar_w) / 2;
        let y = area.position.y + margin;
        let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
    }
    let _ = win.show();
    let _ = win.set_focus();
    Ok(())
}

/// Stop the active recording, finalise the MP4, reveal it, and toast the path.
/// Synchronous (not `async`) so Tauri runs it on a blocking thread automatically
/// — `finalize_child` can take up to 5 s, and a non-async command won't starve
/// the async runtime's worker pool.
#[tauri::command]
pub fn stop_screen_record(
    app: AppHandle,
    state: State<'_, crate::screen_record::RecordState>,
) -> Result<String, String> {
    let path = crate::screen_record::stop(&state)?;
    if let Some(w) = app.get_webview_window(RECORD_STOP_LABEL) {
        let _ = w.close();
    }
    reveal_in_file_manager(&path);
    crate::sound::play(crate::sound::Sound::RecordStop);
    let s = path.to_string_lossy().to_string();
    let _ = app.emit("recording-saved", s.clone());
    Ok(s)
}

#[tauri::command]
pub fn is_recording(state: State<'_, crate::screen_record::RecordState>) -> bool {
    state.is_recording()
}

/// Reveal a file in Finder (macOS) / Explorer (Windows) with it selected.
fn reveal_in_file_manager(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let arg = format!("/select,\"{}\"", path.display());
        let _ = std::process::Command::new("explorer.exe").raw_arg(arg).spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = path;
    }
}

// ── Audio swap (replace / overlay a video's audio) ───────────────────────────

/// Window label for the audio-swap overlay.
pub const AUDIO_SWAP_LABEL: &str = "audio-swap";

/// Holds the Finder-selected video for the audio-swap overlay to pick up on
/// open (the hotkey reads the selection on a worker thread, then opens the UI).
#[derive(Default)]
pub struct AudioSwapState {
    pub video: parking_lot::Mutex<Option<std::path::PathBuf>>,
}

/// Build the audio-swap overlay window (centered, decorated). Must run on the
/// main thread (dispatched via `run_on_main_thread` from the hotkey worker).
pub fn build_audio_swap_overlay(app: &AppHandle) -> Result<(), String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    if let Some(existing) = app.get_webview_window(AUDIO_SWAP_LABEL) {
        let _ = existing.set_focus();
        return Ok(());
    }
    let win = WebviewWindowBuilder::new(app, AUDIO_SWAP_LABEL, WebviewUrl::App("index.html".into()))
        .title("Replace / overlay audio")
        .inner_size(560.0, 660.0)
        .min_inner_size(460.0, 540.0)
        .resizable(true)
        .always_on_top(true)
        .center()
        .visible(true)
        .build()
        .map_err(|e| format!("build audio-swap overlay: {e}"))?;
    let _ = win.set_focus();
    Ok(())
}

#[tauri::command]
pub fn open_audio_swap_overlay(app: AppHandle) -> Result<(), String> {
    build_audio_swap_overlay(&app)
}

/// The Finder-selected video path (if any) the overlay should preload.
#[tauri::command]
pub fn audio_swap_get_selected_video(state: State<'_, AudioSwapState>) -> Option<String> {
    state.video.lock().clone().map(|p| p.to_string_lossy().into_owned())
}

/// Media duration in seconds (video or audio), for the overlay's timeline.
#[tauri::command]
pub fn audio_swap_probe(path: String) -> Option<f64> {
    crate::audio_swap::probe_duration(std::path::Path::new(&path))
}

/// Whether `yt-dlp` is installed (gates the YouTube field in the overlay).
#[tauri::command]
pub fn audio_swap_ytdlp_available() -> bool {
    crate::audio_swap::yt_dlp_path().is_some()
}

/// Download a URL's audio (m4a) via yt-dlp; returns the produced file path.
/// `async` → Tauri runs it off the main thread (yt-dlp can take a while).
#[tauri::command]
pub async fn audio_swap_download_youtube(url: String) -> Result<String, String> {
    let dir = dirs::cache_dir()
        .map(|d| d.join("InspectorRust").join("audioswap"))
        .ok_or("no cache dir")?;
    let p = crate::audio_swap::download_youtube_audio(&url, &dir)?;
    Ok(p.to_string_lossy().into_owned())
}

/// Mux the chosen audio into the video per `spec`; returns the output path and
/// reveals it in Finder/Explorer. `async` → runs off the main thread (ffmpeg).
#[tauri::command]
pub async fn audio_swap_apply(
    app: AppHandle,
    video: String,
    audio: String,
    spec: crate::audio_swap::SwapSpec,
) -> Result<String, String> {
    let out = crate::audio_swap::apply_swap(
        std::path::Path::new(&video),
        std::path::Path::new(&audio),
        spec,
    )?;
    reveal_in_file_manager(&out);
    let s = out.to_string_lossy().into_owned();
    let _ = app.emit("audio-swap-done", s.clone());
    Ok(s)
}

#[tauri::command]
pub fn audio_swap_cancel_overlay(app: AppHandle) {
    if let Some(w) = app.get_webview_window(AUDIO_SWAP_LABEL) {
        let _ = w.close();
    }
}

// ── Social-media download (YouTube / Instagram / TikTok / Facebook) ───────────

/// Whether yt-dlp is available (shared by the social downloader + audio-swap).
#[tauri::command]
pub fn social_ytdlp_available() -> bool {
    crate::social_dl::yt_dlp_path().is_some()
}

/// Download a social-media URL's video/audio into ~/Downloads; reveals it.
/// `async` → runs off the main thread (yt-dlp can take a while).
#[tauri::command]
pub async fn social_download(
    app: AppHandle,
    url: String,
    mode: crate::social_dl::DlMode,
) -> Result<String, String> {
    let dir = dirs::download_dir().ok_or("no Downloads folder")?;
    let out = crate::social_dl::download(&url, mode, &dir)?;
    reveal_in_file_manager(&out);
    let s = out.to_string_lossy().into_owned();
    let _ = app.emit("social-download-done", s.clone());
    Ok(s)
}

// ── Trim (local audio/video) ─────────────────────────────────────────────────

/// Window label for the trim overlay.
pub const TRIM_LABEL: &str = "trim-overlay";

/// Build the trim overlay window (centered). Opened by the `trim` command.
#[tauri::command]
pub fn trim_open_overlay(app: AppHandle) -> Result<(), String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    if let Some(existing) = app.get_webview_window(TRIM_LABEL) {
        let _ = existing.set_focus();
        return Ok(());
    }
    let win = WebviewWindowBuilder::new(&app, TRIM_LABEL, WebviewUrl::App("index.html".into()))
        .title("Trim audio / video")
        .inner_size(520.0, 480.0)
        .min_inner_size(440.0, 420.0)
        .resizable(true)
        .always_on_top(true)
        .center()
        .visible(true)
        .build()
        .map_err(|e| format!("build trim overlay: {e}"))?;
    let _ = win.set_focus();
    Ok(())
}

#[tauri::command]
pub fn trim_cancel_overlay(app: AppHandle) {
    if let Some(w) = app.get_webview_window(TRIM_LABEL) {
        let _ = w.close();
    }
}

/// `{ duration, isVideo }` for the picked file (drives the overlay's timeline).
#[derive(serde::Serialize)]
pub struct TrimFileInfo {
    pub duration: f64,
    pub is_video: bool,
}

#[tauri::command]
pub fn trim_file_info(path: String) -> Option<TrimFileInfo> {
    let p = std::path::Path::new(&path);
    let duration = crate::audio_swap::probe_duration(p)?;
    Some(TrimFileInfo {
        duration,
        is_video: crate::media_trim::has_video_stream(p),
    })
}

/// Trim a file; returns the output path (revealed). `async` → off main thread.
#[tauri::command]
pub async fn trim_apply(
    app: AppHandle,
    input: String,
    start: f64,
    end: f64,
    lossless: bool,
) -> Result<String, String> {
    let out = crate::media_trim::apply_trim(std::path::Path::new(&input), start, end, lossless)?;
    reveal_in_file_manager(&out);
    let s = out.to_string_lossy().into_owned();
    let _ = app.emit("trim-done", s.clone());
    Ok(s)
}

/// Window label for the brightness slider overlay.
pub const BRIGHTNESS_OVERLAY_LABEL: &str = "brightness-overlay";

/// Open the brightness overlay: hide the popup *window*, then build/show a
/// small interactive (focusable, NOT click-through) always-on-top window
/// centred on screen with one slider per monitor.
#[tauri::command]
pub fn brightness_open(app: AppHandle) -> Result<(), String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    // Hide only the popup *window* — deliberately NOT `hotkey::hide_popup`,
    // which on macOS calls `app.hide()`. Hiding the whole app deactivates it,
    // and the freshly-shown overlay then never comes to the front / can't take
    // key focus (the bug: triggering `brightness` did "nothing"). The overlay
    // is interactive and needs focus, so keep the app active and just hide the
    // popup window. We still emit `popup-hidden` so the popup resets its state.
    if let Some(popup) = app.get_webview_window(hotkey::POPUP_LABEL) {
        let _ = popup.hide();
    }
    let _ = app.emit("popup-hidden", ());
    // Always build a FRESH overlay (close any leftover one first) so the
    // monitor list is re-enumerated on every open — a reused window keeps its
    // stale React state and wouldn't re-probe DDC.
    if let Some(existing) = app.get_webview_window(BRIGHTNESS_OVERLAY_LABEL) {
        let _ = existing.close();
    }
    let win = WebviewWindowBuilder::new(
        &app,
        BRIGHTNESS_OVERLAY_LABEL,
        WebviewUrl::App("index.html".into()),
    )
    .title("Brightness")
    .inner_size(380.0, 360.0)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(true)
    .visible(false)
    .center()
    .build()
    .map_err(|e| format!("build brightness overlay: {e}"))?;
    let _ = win.show();
    let _ = win.set_focus();
    Ok(())
}

/// Close the brightness overlay (called by its own close button / Esc).
#[tauri::command]
pub fn brightness_close(app: AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(BRIGHTNESS_OVERLAY_LABEL) {
        let _ = win.close();
    }
    Ok(())
}

// ── Meme picker (v0.70.0) ──────────────────────────────────────────────────────

/// List all memes in the configured library (recursive scan). Cheap enough to
/// call when the picker opens; the frontend fuzzy-filters the result.
#[tauri::command]
pub fn list_memes(db: State<'_, DbHandle>) -> Vec<meme::MemeEntry> {
    meme::list(&db)
}

/// Copy a meme file to the clipboard (animation preserved on macOS via a
/// file-URL on the pasteboard).
#[tauri::command]
pub fn copy_meme(path: String) -> Result<(), String> {
    meme::copy_to_clipboard(&path)
}

/// The currently configured meme library directory (the `meme.dir` setting,
/// or the home-relative default). Used by Settings → Meme library.
#[tauri::command]
pub fn get_meme_dir(db: State<'_, DbHandle>) -> String {
    meme::meme_dir(&db).to_string_lossy().to_string()
}

/// Persist the meme library directory. A blank value resets to the default.
#[tauri::command]
pub fn set_meme_dir(db: State<'_, DbHandle>, dir: String) -> Result<(), String> {
    settings::set(&db, meme::KEY_MEME_DIR, dir.trim()).map_err(map_err)
}
