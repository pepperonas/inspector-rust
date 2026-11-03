//! The transcription ticker: a thin, transparent, click-through bar at the
//! top or bottom of a screen, above full-screen apps and on every Space.
//! Built on the main thread (AppKit), shown with `orderFrontRegardless` so it
//! never takes focus, and exempt from `app.hide()` (`setCanHide:NO`) — the
//! iris-overlay recipe. Hidden means destroyed; every show builds a window
//! under a fresh label (a re-shown hidden webview can stay blank — the
//! status-toast lesson).

use crate::meeting::{TickerConfig, EVENT_TICKER_CONFIG};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL_PREFIX: &str = "transcribe-ticker-";
/// Gap between the bar and the screen edge (logical px).
pub const MARGIN: f64 = 8.0;

static CURRENT: Mutex<Option<(String, (i32, i32, u32, u32))>> = Mutex::new(None);
static SEQ: AtomicU64 = AtomicU64::new(1);

/// Bar height for a font size (logical px).
pub fn bar_height(font_px: u8) -> f64 {
    (f64::from(font_px) * 2.2).round() + 8.0
}

/// The bar inside a work area, all in physical px. Pure.
pub fn ticker_rect(work: (i32, i32, u32, u32), scale: f64, cfg: &TickerConfig) -> (i32, i32, u32, u32) {
    let (wx, wy, ww, wh) = work;
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let w = (f64::from(ww) * f64::from(cfg.width_pct.clamp(30, 100)) / 100.0).round() as u32;
    let h = (bar_height(cfg.font_px) * scale).round() as u32;
    let m = (MARGIN * scale).round() as i32;
    let x = wx + ((ww - w.min(ww)) / 2) as i32;
    let y = if cfg.position == "top" { wy + m } else { wy + wh as i32 - h as i32 - m };
    (x, y, w, h.min(wh))
}

fn pick_monitor(app: &AppHandle, cfg: &TickerConfig) -> Option<tauri::Monitor> {
    let anchor = app.get_webview_window(crate::hotkey::POPUP_LABEL)?;
    let monitors = anchor.available_monitors().ok()?;
    if let Some(name) = &cfg.display {
        if let Some(m) = monitors.iter().find(|m| m.name().is_some_and(|n| n == name)) {
            return Some(m.clone());
        }
    }
    crate::screenshot_preview::pick_cursor_monitor_globally(&monitors).or_else(|| monitors.into_iter().next())
}

/// Names of the connected displays (Settings).
pub fn display_names(app: &AppHandle) -> Vec<String> {
    app.get_webview_window(crate::hotkey::POPUP_LABEL)
        .and_then(|w| w.available_monitors().ok())
        .map(|ms| ms.iter().filter_map(|m| m.name().cloned()).collect())
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn float_above_everything(win: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    let Ok(ns) = win.ns_window() else {
        let _ = win.show();
        return;
    };
    let w = ns as *mut AnyObject;
    if w.is_null() {
        let _ = win.show();
        return;
    }
    // canJoinAllSpaces | stationary | ignoresCycle | fullScreenAuxiliary.
    const BEHAVIOR: u64 = 1 | 16 | 64 | 256;
    // SAFETY: a live NSWindow on the main thread.
    unsafe {
        let _: () = msg_send![w, setLevel: 1000_i64];
        let _: () = msg_send![w, setCollectionBehavior: BEHAVIOR];
        let _: () = msg_send![w, setCanHide: false];
        let _: () = msg_send![w, setHasShadow: false];
        let _: () = msg_send![w, orderFrontRegardless];
    }
}

#[cfg(not(target_os = "macos"))]
fn float_above_everything(win: &tauri::WebviewWindow) {
    let _ = win.show();
}

fn close_current(app: &AppHandle) {
    if let Some((label, _)) = CURRENT.lock().take() {
        if let Some(w) = app.get_webview_window(&label) {
            let _ = w.close();
        }
    }
}

fn show_on_main(app: &AppHandle, cfg: &TickerConfig) {
    let Some(m) = pick_monitor(app, cfg) else { return };
    let wa = m.work_area();
    let rect = ticker_rect((wa.position.x, wa.position.y, wa.size.width, wa.size.height), m.scale_factor(), cfg);
    let existing = CURRENT.lock().clone();
    if let Some((label, geom)) = existing {
        if let Some(w) = app.get_webview_window(&label) {
            if geom != rect {
                let _ = w.set_position(tauri::PhysicalPosition::new(rect.0, rect.1));
                let _ = w.set_size(tauri::PhysicalSize::new(rect.2, rect.3));
                *CURRENT.lock() = Some((label, rect));
            }
            return;
        }
    }
    let label = format!("{LABEL_PREFIX}{}", SEQ.fetch_add(1, Ordering::SeqCst));
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title("Transkription")
        .inner_size(f64::from(rect.2) / m.scale_factor(), f64::from(rect.3) / m.scale_factor())
        .position(f64::from(rect.0) / m.scale_factor(), f64::from(rect.1) / m.scale_factor())
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .focused(false)
        .visible(false)
        .build();
    match built {
        Ok(w) => {
            let _ = w.set_ignore_cursor_events(true);
            let _ = w.set_visible_on_all_workspaces(true);
            let _ = w.set_position(tauri::PhysicalPosition::new(rect.0, rect.1));
            let _ = w.set_size(tauri::PhysicalSize::new(rect.2, rect.3));
            float_above_everything(&w);
            *CURRENT.lock() = Some((label, rect));
        }
        Err(e) => tracing::warn!("ticker: window build failed: {e:#}"),
    }
}

/// Show the ticker when it is switched on and a session runs; otherwise
/// remove it. Safe from any thread.
pub fn apply(app: &AppHandle, cfg: &TickerConfig, running: bool) {
    let _ = app.emit(EVENT_TICKER_CONFIG, cfg);
    let show = cfg.visible && running;
    let app2 = app.clone();
    let cfg = cfg.clone();
    let _ = app.run_on_main_thread(move || {
        if show {
            show_on_main(&app2, &cfg);
        } else {
            close_current(&app2);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(position: &str, width_pct: u8) -> TickerConfig {
        TickerConfig { position: position.into(), width_pct, ..Default::default() }
    }

    #[test]
    fn bar_sits_centred_at_the_bottom_of_the_work_area() {
        let r = ticker_rect((0, 25, 1000, 800), 1.0, &cfg("bottom", 50));
        let h = bar_height(15) as u32;
        assert_eq!(r, (250, 25 + 800 - h as i32 - 8, 500, h));
    }

    #[test]
    fn top_position_is_below_the_menu_bar() {
        let r = ticker_rect((0, 25, 1000, 800), 1.0, &cfg("top", 100));
        assert_eq!((r.0, r.1, r.2), (0, 33, 1000));
    }

    #[test]
    fn scale_applies_to_height_and_margin_not_width() {
        let r1 = ticker_rect((0, 0, 2000, 1600), 2.0, &cfg("top", 50));
        assert_eq!(r1.2, 1000);
        assert_eq!(r1.3, (bar_height(15) * 2.0) as u32);
        assert_eq!(r1.1, 16);
    }

    #[test]
    fn second_screen_offsets_are_kept() {
        let r = ticker_rect((-1920, 0, 1920, 1080), 1.0, &cfg("bottom", 50));
        assert_eq!(r.0, -1920 + 480);
    }
}
