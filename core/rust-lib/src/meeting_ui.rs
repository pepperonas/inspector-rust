//! Tray, hotkeys and ticker for the live transcription. The tray icon turns
//! red while recording: a recording must always be visible.

use crate::meeting::{self, MeetingConfig, MeetingState};
use parking_lot::Mutex;
use tauri::menu::{CheckMenuItem, CheckMenuItemBuilder, MenuItem, MenuItemBuilder, PredefinedMenuItem};
use tauri::{AppHandle, Manager, Wry};
use transcribe_protocol::Source;

pub struct MeetingTrayItems {
    pub sep: PredefinedMenuItem<Wry>,
    pub toggle: MenuItem<Wry>,
    pub mic: CheckMenuItem<Wry>,
    pub sys: CheckMenuItem<Wry>,
    pub ticker: CheckMenuItem<Wry>,
    pub open: MenuItem<Wry>,
}

pub struct TrayHandle(pub Mutex<Option<MeetingTrayItems>>);

const ICON_IDLE: &[u8] = include_bytes!("../assets/tray-hat.png");
const ICON_REC: &[u8] = include_bytes!("../assets/tray-hat-rec.png");

fn t(key: &str) -> String {
    crate::i18n::t(key)
}

/// Menu items, or `None` where transcription does not exist (not on the tray).
pub fn build_tray_items(app: &AppHandle) -> tauri::Result<Option<MeetingTrayItems>> {
    if !crate::transcribe_client::supported() {
        return Ok(None);
    }
    let cfg = app.try_state::<crate::db::DbHandle>().map(|db| MeetingConfig::load(&db)).unwrap_or_default();
    let items = MeetingTrayItems {
        sep: PredefinedMenuItem::separator(app)?,
        toggle: MenuItemBuilder::with_id("meeting_toggle", t("tray.meeting_start")).build(app)?,
        mic: CheckMenuItemBuilder::with_id("meeting_mic", t("tray.meeting_mic")).checked(cfg.mic).build(app)?,
        sys: CheckMenuItemBuilder::with_id("meeting_sys", t("tray.meeting_sys")).checked(cfg.sys).build(app)?,
        ticker: CheckMenuItemBuilder::with_id("meeting_ticker", t("tray.meeting_ticker")).checked(cfg.ticker.visible).build(app)?,
        open: MenuItemBuilder::with_id("meeting_open", t("tray.meeting_open")).build(app)?,
    };
    let keep = MeetingTrayItems {
        sep: items.sep.clone(),
        toggle: items.toggle.clone(),
        mic: items.mic.clone(),
        sys: items.sys.clone(),
        ticker: items.ticker.clone(),
        open: items.open.clone(),
    };
    app.manage(TrayHandle(Mutex::new(Some(keep))));
    Ok(Some(items))
}

fn with_items(app: &AppHandle, f: impl FnOnce(&MeetingTrayItems)) {
    if let Some(h) = app.try_state::<TrayHandle>() {
        if let Some(items) = h.0.lock().as_ref() {
            f(items);
        }
    }
}

fn set_tray_recording(app: &AppHandle, recording: bool, warning: Option<&str>) {
    let Some(tray) = app.tray_by_id("main") else { return };
    let bytes = if recording { ICON_REC } else { ICON_IDLE };
    if let Ok(img) = tauri::image::Image::from_bytes(bytes) {
        let _ = tray.set_icon(Some(img));
        #[cfg(target_os = "macos")]
        let _ = tray.set_icon_as_template(!recording);
    }
    let tip = match (recording, warning) {
        (true, Some(w)) => format!("Inspector Rust — {} ⚠ {w}", t("tray.meeting_running")),
        (true, None) => format!("Inspector Rust — {}", t("tray.meeting_running")),
        _ => "Inspector Rust".to_string(),
    };
    let _ = tray.set_tooltip(Some(tip));
    with_items(app, |i| {
        let _ = i.toggle.set_text(t(if recording { "tray.meeting_stop" } else { "tray.meeting_start" }));
    });
}

fn toast(app: &AppHandle, on: bool, title: String, subtitle: String) {
    crate::status_toast::show_passive(
        app,
        crate::status_toast::StatusToast { kind: "transcribe".into(), on, title, subtitle },
    );
}

pub fn on_session_start(app: &AppHandle, _state: &MeetingState) {
    set_tray_recording(app, true, None);
    ticker_refresh(app);
}

pub fn on_session_end(app: &AppHandle, state: &MeetingState) {
    set_tray_recording(app, false, None);
    ticker_refresh(app);
    let n = state.line_count.to_string();
    match &state.error {
        Some(e) => toast(app, false, t("meeting.stopped_error"), crate::i18n::tf("meeting.stopped_error_sub", &[("error", e)])),
        None => toast(app, false, t("meeting.stopped"), crate::i18n::tf("meeting.stopped_sub", &[("n", &n)])),
    }
}

pub fn on_warning(app: &AppHandle, state: &MeetingState) {
    let w = state.warnings.last().cloned();
    set_tray_recording(app, state.running, w.as_deref());
    if let Some(w) = w {
        toast(app, true, t("meeting.warning"), w);
    }
}

/// Settings changed: tray checks and the ticker follow.
pub fn apply_config(app: &AppHandle, cfg: &MeetingConfig) {
    with_items(app, |i| {
        let _ = i.mic.set_checked(cfg.mic);
        let _ = i.sys.set_checked(cfg.sys);
        let _ = i.ticker.set_checked(cfg.ticker.visible);
    });
    ticker_refresh(app);
}

fn db(app: &AppHandle) -> Option<crate::db::DbHandle> {
    app.try_state::<crate::db::DbHandle>().map(|d| d.inner().clone())
}

fn error_text(e: &str) -> String {
    let key = format!("meeting.err.{}", e.replace('.', "_"));
    let s = t(&key);
    if s == key {
        e.to_string()
    } else {
        s
    }
}

/// Hotkey / tray: start, or stop a running session.
pub fn toggle_recording(app: &AppHandle) {
    if meeting::is_running() {
        if let Err(e) = meeting::stop() {
            toast(app, false, t("meeting.stopped_error"), error_text(&e));
        }
        return;
    }
    let Some(db) = db(app) else { return };
    match meeting::start(&db, None) {
        Ok(_) => toast(app, true, t("meeting.starting"), t("meeting.starting_sub")),
        Err(e) => {
            toast(app, false, t("meeting.start_failed"), error_text(&e));
            // Needs a decision in the app (model download, permission): open it.
            if e == transcribe_protocol::models::ERR_MISSING || e == meeting::ERR_MIC_DENIED {
                let _ = crate::hotkey::show_popup(app);
                use tauri::Emitter;
                let _ = app.emit("open-meeting", ());
            }
        }
    }
}

pub fn toggle_ticker(app: &AppHandle) {
    let Some(db) = db(app) else { return };
    let mut cfg = MeetingConfig::load(&db);
    cfg.ticker.visible = !cfg.ticker.visible;
    if cfg.save(&db).is_ok() {
        apply_config(app, &cfg);
        toast(app, cfg.ticker.visible, t(if cfg.ticker.visible { "meeting.ticker_on" } else { "meeting.ticker_off" }), String::new());
    }
}

fn toggle_source(app: &AppHandle, source: Source) {
    let Some(db) = db(app) else { return };
    let mut cfg = MeetingConfig::load(&db);
    let on = match source {
        Source::Mic => {
            cfg.mic = !cfg.mic;
            cfg.mic
        }
        Source::Sys => {
            cfg.sys = !cfg.sys;
            cfg.sys
        }
    };
    let _ = cfg.save(&db);
    apply_config(app, &cfg);
    let _ = meeting::set_source(source, on);
}

pub fn on_tray(app: &AppHandle, id: &str) {
    let app = app.clone();
    let id = id.to_string();
    std::thread::spawn(move || match id.as_str() {
        "meeting_toggle" => toggle_recording(&app),
        "meeting_mic" => toggle_source(&app, Source::Mic),
        "meeting_sys" => toggle_source(&app, Source::Sys),
        "meeting_ticker" => toggle_ticker(&app),
        "meeting_open" => {
            let dir = meeting::status()
                .dir
                .or_else(|| db(&app).map(|d| MeetingConfig::load(&d).root()));
            if let Some(d) = dir {
                let _ = std::fs::create_dir_all(&d);
                let _ = std::process::Command::new("open").arg(&d).status();
            }
        }
        _ => {}
    });
}

/// Show or hide the ticker to match the config and the session.
fn ticker_refresh(app: &AppHandle) {
    let Some(db) = db(app) else { return };
    let cfg = MeetingConfig::load(&db);
    crate::ticker::apply(app, &cfg.ticker, meeting::is_running());
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_recording_icon_is_a_menu_bar_square_with_red() {
        let img = image::load_from_memory(super::ICON_REC).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (44, 44));
        assert!(img.pixels().any(|p| p.0[0] > 200 && p.0[1] < 90 && p.0[2] < 90 && p.0[3] == 255), "no red dot");
    }
}
