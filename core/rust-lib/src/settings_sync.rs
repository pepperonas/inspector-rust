//! Settings over device sync (v0.243.0).
//!
//! Device sync carries the app settings between Macs. Two rules decide what
//! happens to one key:
//!
//! * **Which keys travel** — [`syncable`]. Everything except what belongs to
//!   one machine: folder paths, audio and camera devices, display levels, the
//!   sync configuration itself, migration flags, caches and status keys.
//!   A deny list, not an allow list: a new setting syncs unless it is one of
//!   these kinds — the same choice the user made ("App-Einstellungen sync").
//! * **Which value wins** — [`adopt`]. Each key carries the time of its last
//!   value change (`settings.updated_at`); the newer change wins. A Mac that
//!   joins for the first time adopts the values already published, so a fresh
//!   install's defaults never overwrite a configured Mac.
//!
//! After a merge, [`plan`] says which running parts must pick the new values
//! up (hotkeys, monitors, sounds, the popup); [`reapply`] does it.

use std::collections::HashMap;

use tauri::{AppHandle, Emitter, Manager};

use crate::db::DbHandle;
use crate::settings;

/// Set once this Mac has finished a sync cycle; before that it adopts the
/// published settings instead of competing with them.
pub const KEY_JOINED: &str = "devicesync.settings_joined";

/// Exact keys that belong to one machine.
const LOCAL_KEYS: &[&str] = &[
    "autobackup.folder",
    "boom.config",
    "boom.last_output_uid",
    "brightness.levels",
    "nosleep.prev_ac_sleep",
    "windowpalette.sysmenu_owned",
    "screenshot.editor_size",
    "screenshot.last_mode",
    "record.camera_device",
    "record.project_dir",
    "repo.clone_dir",
    "meme.dir",
    "cleaner.dev_roots",
    "sec.output_dir",
    "sec.wordlist",
    "track.bridge_port",
    "track.bridge_token",
    "fx.cache",
    "github.user_id",
    "github.login",
    "github.rate",
    "github.scopes",
    "crypto.migrated_v1",
];

/// Prefixes that belong to one machine.
const LOCAL_PREFIXES: &[&str] = &["devicesync.", "seed.", "linux."];

/// Does `key` travel between Macs?
pub fn syncable(key: &str) -> bool {
    if crate::backup::machine_bound_setting(key) {
        return false;
    }
    if LOCAL_KEYS.contains(&key) || LOCAL_PREFIXES.iter().any(|p| key.starts_with(p)) {
        return false;
    }
    // One-time migrations and seeds ran (or must still run) on THIS database.
    if key.contains("migrated") || key.contains("seeded") {
        return false;
    }
    true
}

/// Should the incoming value replace the local one?
///
/// * a key this Mac doesn't have yet → take it;
/// * same value → nothing to do;
/// * not joined yet → take it (the published set is the reference);
/// * otherwise the newer change wins; on an exact tie the larger value wins,
///   so both Macs reach the same answer and nothing swings back and forth.
pub fn adopt(local: Option<(&str, i64)>, incoming: (&str, i64), joined: bool) -> bool {
    let Some((lv, lt)) = local else { return true };
    let (iv, it) = incoming;
    if lv == iv {
        return false;
    }
    if !joined {
        return true;
    }
    match it.cmp(&lt) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => iv > lv,
    }
}

/// The settings this Mac publishes: syncable keys with their change times.
pub fn export(db: &DbHandle) -> (HashMap<String, String>, HashMap<String, i64>) {
    let mut values = HashMap::new();
    let mut times = HashMap::new();
    for (k, v, t) in settings::all_with_times(db).unwrap_or_default() {
        if syncable(&k) {
            times.insert(k.clone(), t);
            values.insert(k, v);
        }
    }
    (values, times)
}

/// Merge a peer's settings. Returns the keys whose value changed here.
pub fn merge(
    db: &DbHandle,
    incoming: &HashMap<String, String>,
    times: &HashMap<String, i64>,
    joined: bool,
) -> Vec<String> {
    let local: HashMap<String, (String, i64)> = settings::all_with_times(db)
        .unwrap_or_default()
        .into_iter()
        .map(|(k, v, t)| (k, (v, t)))
        .collect();
    let mut changed = Vec::new();
    let mut keys: Vec<&String> = incoming.keys().collect();
    keys.sort();
    for key in keys {
        if !syncable(key) {
            continue;
        }
        let value = &incoming[key];
        // A peer from before v0.243.0 sends no times: treat them as 0, which
        // never beats a real local change.
        let at = times.get(key).copied().unwrap_or(0);
        let here = local.get(key).map(|(v, t)| (v.as_str(), *t));
        if adopt(here, (value, at), joined) && settings::set_at(db, key, value, at).is_ok() {
            changed.push(key.clone());
        }
    }
    changed
}

/// Which running parts must re-read their settings.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub hotkeys: bool,
    pub expander: bool,
    pub gestures: bool,
    pub window_snap: bool,
    pub window_palette: bool,
    pub cursor_wrap: bool,
    pub sound: bool,
    pub window_size: bool,
    pub close_on_blur: bool,
    pub language: bool,
    pub crt: bool,
    pub animations: bool,
    pub pulse: bool,
    pub faker: bool,
}

/// Pure: map changed keys to the parts that need a re-apply.
pub fn plan(changed: &[String]) -> Plan {
    let mut p = Plan::default();
    for k in changed {
        let k = k.as_str();
        if k.starts_with("hotkey.") || k == "popup.hotkey" || k == "popup.history_hotkey" {
            p.hotkeys = true;
        }
        if k.starts_with("expander.") {
            p.expander = true;
        }
        if k.starts_with("gestures.") {
            p.gestures = true;
        }
        if k.starts_with("windowsnap.") {
            p.window_snap = true;
        }
        if k.starts_with("windowpalette.") {
            p.window_palette = true;
        }
        if k.starts_with("cursorwrap.") {
            p.cursor_wrap = true;
        }
        if k.starts_with("sound.") {
            p.sound = true;
        }
        if k.starts_with("pulse.") {
            p.pulse = true;
        }
        if k.starts_with("faker.") {
            p.faker = true;
        }
        match k {
            "appearance.window_size" => p.window_size = true,
            "popup.close_on_blur" => p.close_on_blur = true,
            "appearance.language" => p.language = true,
            "appearance.crt_ms" => p.crt = true,
            "appearance.animations" => p.animations = true,
            _ => {}
        }
    }
    p
}

/// Bring the running app in line with synced values, then tell the windows.
pub fn reapply(app: &AppHandle, db: &DbHandle, changed: &[String]) {
    if changed.is_empty() {
        return;
    }
    let p = plan(changed);
    tracing::info!("settings sync: {} key(s) changed — {p:?}", changed.len());

    if p.sound {
        crate::sound::set_enabled(settings::get_bool(db, "sound.enabled", true).unwrap_or(true));
        crate::sound::set_screenshot_style(
            &settings::get_or(db, "sound.screenshot_style", "camera").unwrap_or_else(|_| "camera".into()),
        );
    }
    if p.window_size {
        crate::commands::apply_window_size(app, db);
    }
    if p.close_on_blur {
        if let Some(ui) = app.try_state::<crate::ui_state::UiState>() {
            let close = settings::get_bool(db, "popup.close_on_blur", true).unwrap_or(true);
            ui.close_on_blur.store(close, std::sync::atomic::Ordering::SeqCst);
        }
    }
    if p.pulse {
        crate::pulse::sampler::apply_config(crate::pulse::sampler::PulseConfig::load(db));
    }
    if p.faker {
        crate::faker::init_process_default(db);
    }
    // Gesture capture and the keyboard tap are restarted on this worker, like
    // `set_gesture_config` does — joining the capture thread on the main
    // thread could freeze the UI. Gestures also re-arm the tap (typing guard).
    if p.gestures {
        crate::gestures::bindings::set_live(crate::gestures::bindings::load(db));
        if let Some(s) = app.try_state::<crate::gestures::GestureState>() {
            crate::gestures::apply(app, db, s.inner());
        }
    }
    if p.gestures || p.expander {
        if let Some(s) = app.try_state::<crate::auto_expand::AutoExpandState>() {
            crate::auto_expand::apply(app, db, &s);
        }
    }
    // The pointer monitors are (re)started on the main thread, exactly like
    // the Settings commands that change them.
    if p.window_snap || p.window_palette || p.cursor_wrap {
        let app2 = app.clone();
        let db2 = db.clone();
        let (ws, wp, cw) = (p.window_snap, p.window_palette, p.cursor_wrap);
        let _ = app.run_on_main_thread(move || {
            let (app, db) = (&app2, &db2);
            if ws {
                if let Some(s) = app.try_state::<crate::window_snap::WindowSnapState>() {
                    crate::window_snap::apply(app, db, s.inner());
                }
            }
            if wp {
                if let Some(s) = app.try_state::<crate::window_palette::WindowPaletteState>() {
                    crate::window_palette::apply(app, db, s.inner());
                }
            }
            if cw {
                if let Some(s) = app.try_state::<crate::cursor_wrap::CursorWrapState>() {
                    crate::cursor_wrap::apply(app, db, s.inner());
                }
            }
        });
    }
    if p.language {
        let raw = settings::get_or(db, crate::i18n::KEY_LANGUAGE, "system").unwrap_or_else(|_| "system".into());
        let info = crate::commands::language_info(&raw);
        crate::i18n::set_current(info.effective);
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || crate::retranslate_tray(&app2));
        let _ = app.emit("language-changed", &info);
    }
    if p.crt {
        let _ = app.emit("crt-animation-changed", crate::commands::crt_animation(db).ms);
    }
    if p.animations {
        let raw = settings::get(db, "appearance.animations").ok().flatten();
        let _ = app.emit("animation-stage-changed", crate::commands::normalise_animation_stage(raw.as_deref()));
    }
    // Hotkeys are registered on the main thread, like the Settings commands do.
    if p.hotkeys || p.expander {
        let app2 = app.clone();
        let db2 = db.clone();
        let _ = app.run_on_main_thread(move || reregister_hotkeys(&app2, &db2));
    }
    let _ = app.emit("settings-synced", changed);
}

fn reregister_hotkeys(app: &AppHandle, db: &DbHandle) {
    use crate::hotkey;
    let popup_state = app.state::<hotkey::PopupShortcutState>();
    let popup = settings::get_or(db, "popup.hotkey", hotkey::DEFAULT_POPUP_HOTKEY)
        .unwrap_or_else(|_| hotkey::DEFAULT_POPUP_HOTKEY.into());
    if let Err(e) = hotkey::register_popup(app, &popup_state, &popup) {
        tracing::warn!("settings sync: popup hotkey {popup:?} not registered: {e:#}");
    }
    let hist = settings::get_or(db, hotkey::KEY_HISTORY_HOTKEY, hotkey::DEFAULT_HISTORY_HOTKEY)
        .unwrap_or_else(|_| hotkey::DEFAULT_HISTORY_HOTKEY.into());
    if let Err(e) = hotkey::register_history_hotkey(app, &popup_state, &hist) {
        tracing::warn!("settings sync: history hotkey {hist:?} not registered: {e:#}");
    }
    let state = app.state::<hotkey::ExpanderShortcutState>();
    let enabled = settings::get_bool(db, crate::expander::KEY_ENABLED, false).unwrap_or(false);
    let ex = settings::get_or(db, "expander.hotkey", "").unwrap_or_default();
    if !ex.is_empty() {
        if let Err(e) = hotkey::register_expander(app, &state, &ex, enabled) {
            tracing::warn!("settings sync: expander hotkey not registered: {e:#}");
        }
    }
    if let Ok(slots) = crate::expander::get_direct_slots(db) {
        if let Err(e) = hotkey::register_direct_slots(app, &state, &slots) {
            tracing::warn!("settings sync: direct slots not registered: {e:#}");
        }
    }
    hotkey::apply_action_hotkeys(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_keys_stay_home() {
        for k in [
            "devicesync.folder",
            "devicesync.enabled",
            "devicesync.device_id",
            KEY_JOINED,
            "autobackup.folder",
            "autobackup.last_ms",
            "brightness.levels",
            "boom.config",
            "boom.last_output_uid",
            "record.camera_device",
            "meme.dir",
            "repo.clone_dir",
            "track.bridge_token",
            "fx.cache",
            "github.login",
            "seed.default_snippets_v1",
            "crypto.migrated_v1",
            "popup.hotkey_migrated_v0_67",
            "track.private_apps_seeded_v1",
            "linux.desktop_shortcuts_profile",
            "sync.last_error",
        ] {
            assert!(!syncable(k), "{k} must stay on this Mac");
        }
    }

    #[test]
    fn app_settings_travel() {
        for k in [
            "appearance.theme",
            "appearance.language",
            "popup.hotkey",
            "hotkey.ocr",
            "expander.auto_expand_trigger",
            "gestures.bindings",
            "history.max_entries",
            "autobackup.interval_min",
            "weather.api_key",
            "sync.token",
            "clipboard.exclude_apps",
        ] {
            assert!(syncable(k), "{k} should sync");
        }
    }

    #[test]
    fn the_newer_change_wins() {
        assert!(adopt(Some(("a", 10)), ("b", 20), true));
        assert!(!adopt(Some(("a", 20)), ("b", 10), true));
    }

    #[test]
    fn a_tie_resolves_the_same_on_both_macs() {
        let one = adopt(Some(("a", 5)), ("b", 5), true);
        let two = adopt(Some(("b", 5)), ("a", 5), true);
        assert!(one && !two);
    }

    #[test]
    fn same_value_or_missing_key() {
        assert!(!adopt(Some(("a", 1)), ("a", 99), true));
        assert!(adopt(None, ("a", 0), true));
    }

    #[test]
    fn a_new_mac_adopts_the_published_set() {
        // Not joined: even an older peer value wins over a fresh local default.
        assert!(adopt(Some(("fresh", 999)), ("configured", 1), false));
    }

    #[test]
    fn plan_maps_keys_to_parts() {
        let p = plan(&["hotkey.ocr".into(), "appearance.language".into(), "gestures.tiptap".into()]);
        assert!(p.hotkeys && p.language && p.gestures);
        assert!(!p.sound && !p.window_snap);
        assert_eq!(plan(&["appearance.theme".into()]), Plan::default());
    }

    fn db() -> DbHandle {
        let db: DbHandle = std::sync::Arc::new(parking_lot::Mutex::new(rusqlite::Connection::open_in_memory().unwrap()));
        settings::init_table(&db).unwrap();
        db
    }

    #[test]
    fn writing_the_same_value_keeps_its_change_time() {
        let db = db();
        settings::set_at(&db, "appearance.theme", "dark", 100).unwrap();
        settings::set(&db, "appearance.theme", "dark").unwrap();
        let t = settings::all_with_times(&db).unwrap()[0].2;
        assert_eq!(t, 100, "an unchanged value must not look like a new decision");
        settings::set(&db, "appearance.theme", "light").unwrap();
        assert!(settings::all_with_times(&db).unwrap()[0].2 > 100);
    }

    #[test]
    fn two_macs_converge_and_local_keys_stay() {
        let a = db();
        let b = db();
        settings::set_at(&a, "appearance.theme", "dark", 200).unwrap();
        settings::set_at(&b, "appearance.theme", "light", 100).unwrap();
        settings::set_at(&a, "meme.dir", "/a", 200).unwrap();
        settings::set_at(&b, "meme.dir", "/b", 100).unwrap();

        let (va, ta) = export(&a);
        assert!(!va.contains_key("meme.dir"));
        let changed = merge(&b, &va, &ta, true);
        assert_eq!(changed, vec!["appearance.theme".to_string()]);
        assert_eq!(settings::get_or(&b, "appearance.theme", "").unwrap(), "dark");
        assert_eq!(settings::get_or(&b, "meme.dir", "").unwrap(), "/b");

        // Back the other way nothing moves: same value, peer's time kept.
        let (vb, tb) = export(&b);
        assert_eq!(tb["appearance.theme"], 200);
        assert!(merge(&a, &vb, &tb, true).is_empty());
    }

    #[test]
    fn a_peer_without_times_never_beats_a_real_change() {
        let a = db();
        settings::set(&a, "appearance.theme", "dark").unwrap();
        let mut v = HashMap::new();
        v.insert("appearance.theme".to_string(), "light".to_string());
        assert!(merge(&a, &v, &HashMap::new(), true).is_empty());
    }
}
