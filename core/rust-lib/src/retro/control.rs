//! Platform-neutral control of the retro overlay: start / stop / toggle,
//! live settings, focus and lens toggles. Every entry point used by IPC,
//! hotkeys, the tray and the CLI goes through here. Blocking platform work
//! runs on a worker thread so no caller (often the main thread or a hotkey
//! handler) ever waits on window building or ScreenCaptureKit.

use tauri::{AppHandle, Emitter, Manager};

use super::config::{self as rc, RetroConfig};
use crate::db::DbHandle;

#[cfg(target_os = "macos")]
use super::macos as platform;
#[cfg(target_os = "windows")]
use super::windows as platform;

pub const ERR_UNSUPPORTED: &str = "retro.unsupported";

#[derive(Debug, Clone, serde::Serialize)]
pub struct RetroStatus {
    pub running: bool,
    pub supported: bool,
    pub screen_permission: bool,
    pub accessibility: bool,
    /// Short platform note for the panel (e.g. "Linux wird nicht unterstützt").
    pub note: Option<String>,
}

fn db(app: &AppHandle) -> Option<DbHandle> {
    app.try_state::<DbHandle>().map(|s| s.inner().clone())
}

pub fn status() -> RetroStatus {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        RetroStatus {
            running: platform::is_running(),
            supported: platform::supported(),
            screen_permission: platform::screen_permission(),
            accessibility: platform::accessibility(),
            note: platform::note(),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        RetroStatus {
            running: false,
            supported: false,
            screen_permission: false,
            accessibility: false,
            note: Some("Das Retro-Overlay gibt es unter Linux nicht.".into()),
        }
    }
}

pub fn is_running() -> bool {
    status().running
}

fn report_error(app: &AppHandle, e: &str) {
    tracing::warn!("retro: {e}");
    let _ = app.emit("retro-error", e.to_string());
    let msg = match e {
        "retro.no_permission" => "Bildschirmaufnahme nicht erlaubt — siehe Systemeinstellungen",
        "retro.unsupported" => "Auf diesem System nicht verfügbar",
        other => other,
    };
    toast(app, "Retro-Overlay", msg);
}

pub fn toast(app: &AppHandle, title: &str, subtitle: &str) {
    let app2 = app.clone();
    let t = crate::status_toast::StatusToast {
        kind: "retro".into(),
        on: false,
        title: title.into(),
        subtitle: subtitle.into(),
    };
    let _ = app.run_on_main_thread(move || crate::status_toast::show_passive(&app2, t));
}

/// Start with the saved config (worker thread).
pub fn start_async(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(db) = db(&app) else { return };
        let cfg = rc::load(&db);
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Err(e) = platform::start(&app, &cfg) {
            report_error(&app, &e);
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = cfg;
            report_error(&app, ERR_UNSUPPORTED);
        }
    });
}

/// Load a preset (or, failing that, a palette) and start / restart the
/// overlay with it (worker thread). Used by `--retro-preset`.
pub fn start_with_async(app: &AppHandle, name: &str) {
    let app = app.clone();
    let name = name.to_string();
    std::thread::spawn(move || {
        let Some(db) = db(&app) else { return };
        let mut cfg = rc::load(&db);
        let all = rc::all_presets(&db);
        if let Some(p) = rc::find_preset(&all, &name) {
            cfg.apply_preset(p);
        } else if let Err(e) = cfg.select_palette(&name) {
            report_error(&app, &e);
            return;
        }
        let Ok(saved) = rc::save(&db, &cfg) else { return };
        let _ = app.emit("retro-config-changed", &saved);
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            if platform::is_running() {
                platform::stop(&app);
            }
            if let Err(e) = platform::start(&app, &saved) {
                report_error(&app, &e);
            }
        }
    });
}

pub fn stop_async(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        platform::stop(&app);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = app;
    });
}

pub fn toggle_async(app: &AppHandle) {
    if is_running() {
        stop_async(app)
    } else {
        start_async(app)
    }
}

/// A saved config changed: push it to a running overlay and the preview.
pub fn apply(cfg: &RetroConfig) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        platform::apply(cfg);
        platform::set_preview_settings(cfg);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = cfg;
}

/// Flip one boolean of the active mode, save, apply, emit.
fn flip(app: &AppHandle, f: impl FnOnce(&mut rc::ModeSettings) -> bool) -> Option<bool> {
    let db = db(app)?;
    let mut cfg = rc::load(&db);
    let v = f(cfg.active_mut());
    let saved = rc::save(&db, &cfg).ok()?;
    apply(&saved);
    let _ = app.emit("retro-config-changed", &saved);
    Some(v)
}

pub fn toggle_focus(app: &AppHandle) -> Option<bool> {
    let v = flip(app, |s| {
        s.focus = !s.focus;
        s.focus
    })?;
    if is_running() {
        toast(app, "Retro-Overlay", if v { "Fokus-Modus an" } else { "Fokus-Modus aus" });
    }
    Some(v)
}

pub fn toggle_lens(app: &AppHandle) -> Option<bool> {
    let v = flip(app, |s| {
        s.lens = !s.lens;
        s.lens
    })?;
    if is_running() {
        toast(app, "Retro-Overlay", if v { "Lupe an" } else { "Lupe aus" });
    }
    Some(v)
}

pub fn preview_start(app: &AppHandle) -> Result<(), String> {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let db = db(app).ok_or("keine Datenbank")?;
        platform::preview_start(app, &rc::load(&db))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = app;
        Err(ERR_UNSUPPORTED.into())
    }
}

pub fn preview_stop() {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    platform::preview_stop();
}

/// Stop the overlay on sleep / screen lock / session switch.
pub fn on_system_pause(app: &AppHandle, why: &str) {
    if is_running() {
        let app2 = app.clone();
        let why = why.to_string();
        std::thread::spawn(move || {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            platform::stop(&app2);
            toast(&app2, "Retro-Overlay gestoppt", &why);
        });
    }
}

/// Displays changed (hot-plug, resolution, scale): rebuild a running overlay.
pub fn on_displays_changed(app: &AppHandle) {
    if !is_running() {
        return;
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            platform::stop(&app2);
            std::thread::sleep(std::time::Duration::from_millis(400));
            if let Some(db) = db(&app2) {
                if let Err(e) = platform::start(&app2, &rc::load(&db)) {
                    report_error(&app2, &e);
                }
            }
        }
    });
}
