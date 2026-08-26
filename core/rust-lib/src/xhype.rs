//! `x!` — the full-screen spectacle (v0.133.0).
//!
//! A hidden trigger that takes over the WHOLE screen for ~15 s with a
//! six-act canvas piece, then closes itself. This module owns only the window:
//! the art lives in `XOverlay.tsx`.
//!
//! Three things are load-bearing here:
//!
//! * **Built from a worker thread, never inline in a sync command.** A sync
//!   `#[tauri::command]` runs ON the main thread, and `WebviewWindowBuilder::
//!   build()` needs that thread's event loop to pump — building there
//!   deadlocks the app (the recorder's stop-bar lesson). We hop to a worker,
//!   which then marshals onto the main thread.
//! * **Focused, unlike the iris/snap overlays.** Those are click-through
//!   background monitors; this one is a show the user watches and dismisses
//!   with Esc, so it takes key focus on purpose.
//! * **macOS window level above the menu bar.** "Full screen" should mean the
//!   whole panel — a plain borderless window still leaves the menu bar and
//!   Dock on top. Raising `NSWindow.level` covers them without the slow
//!   native-fullscreen Space transition.

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const X_LABEL: &str = "x-overlay";

/// tagesschau's official, key-less news feed. Only HEADLINES are read from it
/// — short factual statements, shown with the source named in the overlay's
/// HUD. Article text is never fetched or displayed.
const TAGESSCHAU_URL: &str = "https://www.tagesschau.de/api2u/homepage";

/// What the overlay should say. Filled before the window is built (so a slow
/// network can't leave a black screen up) and read once by the webview.
#[derive(Default, serde::Serialize, Clone)]
pub struct XPayload {
    /// "features" | "news" — the frontend picks its word set from this.
    pub mode: String,
    /// Headlines for the news mode; empty means "fall back to features".
    pub headlines: Vec<String>,
}

static PAYLOAD: Mutex<Option<XPayload>> = Mutex::new(None);

/// The payload the overlay reads on mount.
pub fn payload() -> XPayload {
    PAYLOAD.lock().ok().and_then(|p| p.clone()).unwrap_or_default()
}

/// Pull the headline strings out of the feed. Pure + tested: the shape is
/// `{"news":[{"title":"…"}, …]}`, and anything without a usable title is
/// skipped rather than surfacing as an empty line.
pub fn parse_headlines(body: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    v.get("news")
        .and_then(|n| n.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|n| {
                    // `title` is the headline; some entries only carry
                    // `topline`, which is the shorter kicker — either works.
                    n.get("title")
                        .or_else(|| n.get("topline"))
                        .and_then(|t| t.as_str())
                        .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
                        .filter(|t| !t.is_empty())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Fetch today's headlines. Any failure yields an empty list — the caller
/// falls back to the feature showcase, so the piece always plays.
fn fetch_headlines() -> Vec<String> {
    match ureq::get(TAGESSCHAU_URL)
        .timeout(std::time::Duration::from_secs(6))
        .call()
    {
        Ok(resp) => resp
            .into_string()
            .map(|b| parse_headlines(&b))
            .unwrap_or_default(),
        Err(e) => {
            tracing::warn!("x!!: headline fetch failed: {e}");
            Vec::new()
        }
    }
}

/// Open the spectacle on the monitor under the cursor. Returns immediately;
/// the window builds on the main thread via a worker hop.
pub fn open(app: &AppHandle, mode: &str) {
    let app = app.clone();
    let mode = mode.to_string();
    std::thread::spawn(move || {
        // Fetch BEFORE the window exists: a slow network would otherwise
        // leave a black full-screen rectangle sitting there.
        let headlines = if mode == "news" { fetch_headlines() } else { Vec::new() };
        if let Ok(mut p) = PAYLOAD.lock() {
            *p = Some(XPayload { mode: mode.clone(), headlines });
        }
        let a = app.clone();
        let _ = app.run_on_main_thread(move || build_on_main(&a));
    });
}

fn build_on_main(app: &AppHandle) {
    // A stale window from an earlier run would sit at the old geometry.
    if let Some(old) = app.get_webview_window(X_LABEL) {
        let _ = old.close();
    }
    let Some(anchor) = app.get_webview_window(crate::hotkey::POPUP_LABEL) else {
        return;
    };
    let monitors = anchor.available_monitors().unwrap_or_default();
    if monitors.is_empty() {
        return;
    }
    // Same cursor-monitor resolution as the recorder/toast: the GLOBAL cursor
    // position, not `cursor_position()` on a fresh window (which is stale and
    // always resolves to the primary display). Falls back to the first monitor.
    let m = crate::screenshot_preview::pick_cursor_monitor_globally(&monitors)
        .unwrap_or_else(|| monitors[0].clone());
    let (pos, size) = (*m.position(), *m.size());

    // ⚠️ Hide the popup WINDOW ourselves — never `hotkey::hide_popup`, which
    // ends in `app.hide()` (`NSApp.hide(nil)`). That hides EVERY window this
    // process owns, so the overlay we're about to build vanished with it and
    // the user had to summon it again (field report). It also deactivates the
    // app, which would stop the overlay taking key focus — and this piece
    // needs focus, since any key aborts it. Same lesson as the iris overlays,
    // approached from the other side: they set `setCanHide:NO` to survive the
    // app-hide; here we simply don't fire one.
    #[cfg(target_os = "macos")]
    crate::esc_watch::disarm();
    if let Some(popup) = app.get_webview_window(crate::hotkey::POPUP_LABEL) {
        let _ = popup.hide();
    }
    // Let the frontend drop its transient state, exactly as a normal hide would.
    let _ = app.emit("popup-hidden", ());

    let built = WebviewWindowBuilder::new(app, X_LABEL, WebviewUrl::App("index.html".into()))
        .title("X")
        .inner_size(size.width as f64, size.height as f64)
        .position(pos.x as f64, pos.y as f64)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .focused(true)
        .visible(false)
        .build();
    match built {
        Ok(w) => {
            let _ = w.set_visible_on_all_workspaces(true);
            // Physical px — `inner_size` is logical, so a HiDPI monitor would
            // otherwise only be half covered.
            let _ = w.set_position(tauri::PhysicalPosition::new(pos.x, pos.y));
            let _ = w.set_size(tauri::PhysicalSize::new(size.width, size.height));
            raise_above_menu_bar(&w);
            exempt_from_app_hide(&w);
            let _ = w.show();
            let _ = w.set_focus();
        }
        Err(e) => tracing::warn!("x!: overlay build failed: {e:#}"),
    }
}

/// Close the spectacle (Esc, or when it finishes on its own).
pub fn close(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(X_LABEL) {
        let _ = w.close();
    }
}

/// macOS: lift the window above the menu bar + Dock so "full screen" is
/// literal. 1000 is the same level the snap overlay uses (above
/// `NSStatusWindowLevel` 25, below the screen saver).
#[cfg(target_os = "macos")]
fn raise_above_menu_bar(win: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    if let Ok(ns) = win.ns_window() {
        let w = ns as *mut AnyObject;
        if !w.is_null() {
            unsafe {
                let _: () = msg_send![w, setLevel: 1000_isize];
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn raise_above_menu_bar(_win: &tauri::WebviewWindow) {}

/// macOS: exempt the overlay from `NSApp.hide(nil)`. We avoid firing one
/// ourselves (see `build_on_main`), but any other path that hides the app —
/// a status toast finishing, a stray `hide_popup` — must not take the piece
/// down mid-play. Same flag the iris overlays use.
#[cfg(target_os = "macos")]
fn exempt_from_app_hide(win: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    if let Ok(ns) = win.ns_window() {
        let w = ns as *mut AnyObject;
        if !w.is_null() {
            unsafe {
                let _: () = msg_send![w, setCanHide: false];
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn exempt_from_app_hide(_win: &tauri::WebviewWindow) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headlines_and_normalises_whitespace() {
        let body = r#"{"news":[
            {"title":"Erste  Meldung\n mit Umbruch"},
            {"title":"Zweite Meldung"},
            {"topline":"Nur Kicker"},
            {"title":""},
            {"other":"ignored"}
        ]}"#;
        assert_eq!(
            parse_headlines(body),
            vec![
                "Erste Meldung mit Umbruch".to_string(),
                "Zweite Meldung".to_string(),
                "Nur Kicker".to_string(),
            ]
        );
    }

    #[test]
    fn malformed_or_empty_feeds_yield_nothing_rather_than_panicking() {
        // Every one of these must degrade to "no headlines" → the caller
        // falls back to the feature showcase, so the piece still plays.
        for body in ["", "not json", "{}", r#"{"news":null}"#, r#"{"news":[]}"#, "[]"] {
            assert!(parse_headlines(body).is_empty(), "{body:?}");
        }
    }
}
