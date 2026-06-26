//! macOS drag-to-snap monitor — **phase 2 stub.**
//!
//! Phase 1 ships the pure geometry core + the opt-in plumbing; this is where the
//! impure pieces land next: a global `CGEventTap` watching left-mouse drag/up +
//! Esc (on a `CFRunLoop` thread, same pattern as `input_lock`/`gestures`),
//! reading + moving the dragged window via the Accessibility API (AXUIElement
//! `kAXFocusedWindow` → set `kAXPosition`/`kAXSize`), `NSScreen.visibleFrame` →
//! [`super::cocoa_rect_to_topleft`], and a transparent click-through preview
//! overlay window. For now `set_active` only records intent.

use std::sync::atomic::{AtomicBool, Ordering};

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Enable/disable the drag-to-snap monitor. Phase 1: records the flag + logs
/// (the actual CGEventTap/AX/overlay arrive in phase 2). Requires Accessibility
/// when the real monitor lands.
pub fn set_active(_app: &tauri::AppHandle, enabled: bool) {
    let was = ACTIVE.swap(enabled, Ordering::SeqCst);
    if was != enabled {
        tracing::info!("window-snap: {} (monitor impl lands in phase 2)", if enabled { "enabled" } else { "disabled" });
    }
}
