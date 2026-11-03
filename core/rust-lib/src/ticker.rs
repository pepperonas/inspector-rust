//! The transcription ticker: a thin, transparent, click-through bar at the
//! top or bottom of a screen, above full-screen apps and on every Space.
//! Built on the main thread (AppKit), shown with `orderFrontRegardless` so it
//! never takes focus, and exempt from `app.hide()` (`setCanHide:NO`) — the
//! iris-overlay recipe. Hidden means destroyed; every show builds a window
//! under a fresh label (a re-shown hidden webview can stay blank — the
//! status-toast lesson).

use crate::meeting::{TickerConfig, EVENT_TICKER_CONFIG};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL_PREFIX: &str = "transcribe-ticker-";
/// Gap between the bar and the screen edge (logical px).
pub const MARGIN: f64 = 8.0;
/// Width of the minimised pill (logical px).
pub const MINI_WIDTH: f64 = 360.0;
/// Lines shown when maximised.
pub const MAX_LINES: f64 = 5.0;
/// Hover gate tick: the bar is click-through except under the pointer.
const GATE_MS: u64 = 50;
/// Emitted to the ticker window when the pointer enters or leaves it.
pub const EVENT_HOVER: &str = "meeting-ticker-hover";

/// Physical `(x, y, width, height)`.
type Rect = (i32, i32, u32, u32);

static CURRENT: Mutex<Option<(String, Rect)>> = Mutex::new(None);
static SEQ: AtomicU64 = AtomicU64::new(1);

/// Bar height for a font size (logical px).
pub fn bar_height(font_px: u8) -> f64 {
    (f64::from(font_px) * 2.2).round() + 8.0
}

/// Height of the maximised view (logical px).
pub fn max_height(font_px: u8) -> f64 {
    (f64::from(font_px) * 1.35 * MAX_LINES).round() + 20.0
}

/// The bar inside a work area, all in physical px. Pure.
pub fn ticker_rect(work: Rect, scale: f64, cfg: &TickerConfig) -> Rect {
    let (wx, wy, ww, wh) = work;
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let bar_w = (f64::from(ww) * f64::from(cfg.width_pct.clamp(30, 100)) / 100.0).round() as u32;
    let (w, h) = match cfg.mode.as_str() {
        "mini" => (bar_w.min((MINI_WIDTH * scale).round() as u32), bar_height(cfg.font_px)),
        "max" => (bar_w, max_height(cfg.font_px)),
        _ => (bar_w, bar_height(cfg.font_px)),
    };
    let w = w.min(ww);
    let h = ((h * scale).round() as u32).min(wh);
    if let Some((fx, fy)) = cfg.place {
        let x = wx + (f64::from(ww - w) * f64::from(fx.clamp(0.0, 1.0))).round() as i32;
        let y = wy + (f64::from(wh - h) * f64::from(fy.clamp(0.0, 1.0))).round() as i32;
        return (x, y, w, h);
    }
    let m = (MARGIN * scale).round() as i32;
    let x = wx + ((ww - w) / 2) as i32;
    let y = if cfg.position == "top" { wy + m } else { wy + wh as i32 - h as i32 - m };
    (x, y, w, h)
}

/// Where a window at `win` sits in `work`, as the fractions `place` stores.
/// Pure.
pub fn place_of(work: Rect, win: Rect) -> (f32, f32) {
    let (wx, wy, ww, wh) = work;
    let (x, y, w, h) = win;
    let free = |total: u32, size: u32| f64::from(total.saturating_sub(size)).max(1.0);
    let fx = f64::from(x - wx) / free(ww, w);
    let fy = f64::from(y - wy) / free(wh, h);
    (fx.clamp(0.0, 1.0) as f32, fy.clamp(0.0, 1.0) as f32)
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
            start_gate(app);
        }
        Err(e) => tracing::warn!("ticker: window build failed: {e:#}"),
    }
}

static GATE_RUNNING: AtomicBool = AtomicBool::new(false);

/// Is a pointer at window-local `(x, y)` inside a window of `size`? Pure.
pub fn inside(p: (f64, f64), size: (f64, f64)) -> bool {
    p.0 >= 0.0 && p.1 >= 0.0 && p.0 < size.0 && p.1 < size.1
}

/// The bar is click-through so it never blocks the app under it; while the
/// pointer is over it, it takes clicks (controls, dragging). Polls at 20 Hz
/// while a ticker exists.
fn start_gate(app: &AppHandle) {
    if GATE_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("ir-ticker-gate".into()).spawn(move || {
        let mut last: Option<(String, bool)> = None;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(GATE_MS));
            let Some((label, _)) = CURRENT.lock().clone() else { break };
            let (tx, rx) = std::sync::mpsc::channel();
            let (app2, l2) = (app.clone(), label.clone());
            let _ = app.run_on_main_thread(move || {
                let r = app2.get_webview_window(&l2).and_then(|w| {
                    let p = crate::status_toast::pointer_local(&w)?;
                    let sf = w.scale_factor().ok()?;
                    let s = w.inner_size().ok()?;
                    Some(inside(p, (f64::from(s.width) / sf, f64::from(s.height) / sf)))
                });
                let _ = tx.send(r);
            });
            let Ok(Some(now)) = rx.recv_timeout(std::time::Duration::from_millis(500)) else { continue };
            if last.as_ref() != Some(&(label.clone(), now)) {
                if let Some(w) = app.get_webview_window(&label) {
                    let _ = w.set_ignore_cursor_events(!now);
                }
                let _ = app.emit_to(label.as_str(), EVENT_HOVER, now);
                last = Some((label, now));
            }
        }
        GATE_RUNNING.store(false, Ordering::SeqCst);
        if CURRENT.lock().is_some() {
            start_gate(&app);
        }
    });
    if spawned.is_err() {
        GATE_RUNNING.store(false, Ordering::SeqCst);
    }
}

/// The window's current place as `place` fractions of its monitor's work
/// area (after the user dragged it). Main thread.
pub fn current_place(app: &AppHandle) -> Option<(f32, f32)> {
    let (label, _) = CURRENT.lock().clone()?;
    let w = app.get_webview_window(&label)?;
    let pos = w.outer_position().ok()?;
    let size = w.outer_size().ok()?;
    let monitors = w.available_monitors().ok()?;
    let centre = (pos.x + size.width as i32 / 2, pos.y + size.height as i32 / 2);
    let m = monitors.iter().find(|m| {
        let (p, s) = (m.position(), m.size());
        centre.0 >= p.x && centre.0 < p.x + s.width as i32 && centre.1 >= p.y && centre.1 < p.y + s.height as i32
    })?;
    let wa = m.work_area();
    let work = (wa.position.x, wa.position.y, wa.size.width, wa.size.height);
    let win = (pos.x, pos.y, size.width, size.height);
    if let Some(cur) = CURRENT.lock().as_mut() {
        cur.1 = win; // the window is already there: no snap back
    }
    Some(place_of(work, win))
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
    fn mini_is_a_short_pill_and_max_is_several_lines_tall() {
        let mut c = cfg("bottom", 70);
        c.mode = "mini".into();
        let r = ticker_rect((0, 0, 2000, 1000), 2.0, &c);
        assert_eq!(r.2, (MINI_WIDTH * 2.0) as u32);
        assert_eq!(r.3, (bar_height(15) * 2.0) as u32);
        c.mode = "max".into();
        let r = ticker_rect((0, 0, 2000, 1000), 2.0, &c);
        assert_eq!(r.2, 1400);
        assert_eq!(r.3, (max_height(15) * 2.0) as u32);
    }

    #[test]
    fn a_moved_ticker_keeps_its_place_and_stays_on_screen() {
        let mut c = cfg("bottom", 50);
        c.place = Some((0.0, 0.0));
        assert_eq!(ticker_rect((100, 50, 1000, 800), 1.0, &c).0, 100);
        assert_eq!(ticker_rect((100, 50, 1000, 800), 1.0, &c).1, 50);
        c.place = Some((1.0, 1.0));
        let r = ticker_rect((100, 50, 1000, 800), 1.0, &c);
        assert_eq!((r.0 + r.2 as i32, r.1 + r.3 as i32), (1100, 850), "flush with the far corner, never past it");
        // A place stored as fractions survives a smaller screen.
        let r = ticker_rect((0, 0, 600, 400), 1.0, &c);
        assert!(r.0 + r.2 as i32 <= 600 && r.1 + r.3 as i32 <= 400);
    }

    #[test]
    fn the_pointer_counts_as_inside_only_within_the_window() {
        assert!(inside((0.0, 0.0), (300.0, 40.0)));
        assert!(inside((299.9, 39.9), (300.0, 40.0)));
        assert!(!inside((300.0, 10.0), (300.0, 40.0)));
        assert!(!inside((-1.0, 10.0), (300.0, 40.0)));
        assert!(!inside((10.0, 40.0), (300.0, 40.0)));
    }

    #[test]
    fn place_from_a_window_position_round_trips() {
        let mut c = cfg("bottom", 50);
        let work = (0, 25, 1000, 800);
        let (x, y) = (300, 400);
        c.place = Some(place_of(work, (x, y, 500, 41)));
        let r = ticker_rect(work, 1.0, &c);
        assert!((r.0 - x).abs() <= 1 && (r.1 - y).abs() <= 1, "{r:?}");
    }

    #[test]
    fn second_screen_offsets_are_kept() {
        let r = ticker_rect((-1920, 0, 1920, 1080), 1.0, &cfg("bottom", 50));
        assert_eq!(r.0, -1920 + 480);
    }
}
