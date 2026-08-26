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

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const X_LABEL: &str = "x-overlay";

/// Open the spectacle on the monitor under the cursor. Returns immediately;
/// the window builds on the main thread via a worker hop.
pub fn open(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
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
