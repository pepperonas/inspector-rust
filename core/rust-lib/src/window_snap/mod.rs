//! Window snapping — drag a window to a screen edge to snap it into a zone
//! (left/right half, or maximize), with a live preview overlay. macOS only.
//!
//! This module is the **platform-independent, unit-tested core**: the snap-zone
//! geometry (which edge the cursor is near, with hysteresis so a fast drag
//! doesn't make a zone "stick"), the target-rect math, and the AX↔Cocoa
//! coordinate conversion. The impure parts — the global mouse-drag monitor
//! (CGEventTap), reading/moving the dragged window via the Accessibility API
//! (AXUIElement), and the preview overlay window — live in `macos.rs`.
//!
//! **Coordinate convention:** the public core works in **top-left-origin global
//! coordinates** (the space of `CGEventGetLocation` and AX `kAXPosition`), so
//! the cursor + the snap target need no per-axis flipping. `NSScreen.visible-
//! Frame` (Cocoa, bottom-left) is converted once at the FFI boundary via
//! [`cocoa_rect_to_topleft`]. Snapping is **opt-in** (settings
//! `windowsnap.enabled`, off by default); `apply` starts/stops the macOS
//! monitor to match — mirrors `gestures`/`auto_expand`.

#![allow(dead_code)] // platform monitor lands in macos.rs (phase 2)

use crate::db::DbHandle;

// ── Geometry ─────────────────────────────────────────────────────────────────

/// A rectangle in top-left-origin coordinates (x right, y down).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Rect { x, y, w, h }
    }
}

/// A screen-edge snap target. (Corners → quarter tiles + bottom half are easy
/// follow-ups; the spec's v1 set is the three below.)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapZone {
    /// Top edge → fill the whole visible frame.
    Maximize,
    /// Left edge → left half.
    Left,
    /// Right edge → right half.
    Right,
}

/// Distance (px) from a **side** edge at which a zone **activates**.
pub const ENTER_PX: f64 = 8.0;
/// Larger distance at which an *already-active* side zone **deactivates** —
/// hysteresis so jitter / a fast pass near the edge doesn't make a zone flicker
/// or "stick".
pub const EXIT_PX: f64 = 28.0;
/// **Maximize** uses a *deeper* top band than the sides so you can release it
/// **below the menu bar** — dragging a window all the way to the menu bar
/// triggers macOS's own Mission Control (a system gesture a listen-only tap
/// can't suppress). The deep band means a normal drag registers maximize well
/// before reaching the menu bar.
pub const MAXIMIZE_ENTER_PX: f64 = 38.0;
/// Once maximize is active it **latches** — even if the cursor overshoots up
/// into the menu bar (`top_d` < 0) — so a fast overshoot still maximizes on
/// release instead of dropping the zone.
pub const MAXIMIZE_EXIT_PX: f64 = 70.0;

/// Which snap zone (if any) the `cursor` is engaging, given the `screen`'s
/// visible frame (top-left coords) and the `current` zone for hysteresis.
///
/// Priority at a corner: **top wins** (maximize), then left, then right. The
/// cursor must be inside the screen's vertical/horizontal span for an edge to
/// count, so dragging along the very top of a side edge still reads as the side.
pub fn classify_zone(cursor: (f64, f64), screen: Rect, current: Option<SnapZone>) -> Option<SnapZone> {
    let (cx, cy) = cursor;
    // Ignore a cursor that isn't actually over this screen (with a small slop).
    if cx < screen.x - EXIT_PX
        || cx > screen.x + screen.w + EXIT_PX
        || cy < screen.y - EXIT_PX
        || cy > screen.y + screen.h + EXIT_PX
    {
        return None;
    }
    let left_d = cx - screen.x;
    let right_d = (screen.x + screen.w) - cx;
    let top_d = cy - screen.y;
    // Hysteresis for the side edges: keep the current zone until the cursor
    // moves past EXIT_PX.
    let th = |z: SnapZone| if current == Some(z) { EXIT_PX } else { ENTER_PX };

    // Maximize: a deep top band that activates **below** the menu bar
    // (`top_d >= 0`), and once active **latches** even into the menu bar
    // (`top_d` may go negative) so an overshoot still maximizes.
    let max_active = if current == Some(SnapZone::Maximize) {
        top_d <= MAXIMIZE_EXIT_PX
    } else {
        (0.0..=MAXIMIZE_ENTER_PX).contains(&top_d)
    };
    if max_active {
        return Some(SnapZone::Maximize);
    }
    if left_d <= th(SnapZone::Left) {
        return Some(SnapZone::Left);
    }
    if right_d <= th(SnapZone::Right) {
        return Some(SnapZone::Right);
    }
    None
}

/// The target rect for `zone` within `screen` (top-left coords).
pub fn zone_rect(zone: SnapZone, screen: Rect) -> Rect {
    let half = (screen.w / 2.0).floor();
    match zone {
        SnapZone::Maximize => screen,
        SnapZone::Left => Rect::new(screen.x, screen.y, half, screen.h),
        SnapZone::Right => Rect::new(screen.x + half, screen.y, screen.w - half, screen.h),
    }
}

/// Convert a Cocoa rect (bottom-left origin, y up, in the global desktop space)
/// to top-left-origin coords (y down) — the space AX `kAXPosition` and
/// `CGEventGetLocation` use. The flip pivots on the **primary** display's full
/// height (`primary_height`), the way the window server defines global y.
pub fn cocoa_rect_to_topleft(cocoa: Rect, primary_height: f64) -> Rect {
    Rect::new(cocoa.x, primary_height - (cocoa.y + cocoa.h), cocoa.w, cocoa.h)
}

// ── Config + lifecycle ───────────────────────────────────────────────────────

const KEY_ENABLED: &str = "windowsnap.enabled";

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct WindowSnapConfig {
    /// Off by default (opt-in) — the `bool` default `false` is exactly that.
    pub enabled: bool,
}

impl WindowSnapConfig {
    pub fn load(db: &DbHandle) -> WindowSnapConfig {
        WindowSnapConfig {
            enabled: crate::settings::get_bool(db, KEY_ENABLED, false).unwrap_or(false),
        }
    }
    pub fn save(&self, db: &DbHandle) -> anyhow::Result<()> {
        crate::settings::set(db, KEY_ENABLED, if self.enabled { "true" } else { "false" })?;
        Ok(())
    }
}

/// Managed Tauri state (the running monitor lives in `macos.rs`; nothing to hold
/// here yet — kept for symmetry + future handles).
#[derive(Default)]
pub struct WindowSnapState;

/// Start/stop the drag-to-snap monitor to match the saved config. Called at
/// startup and after a settings change (idempotent). On non-macOS it's a no-op.
pub fn apply(app: &tauri::AppHandle, db: &DbHandle, _state: &WindowSnapState) {
    let cfg = WindowSnapConfig::load(db);
    #[cfg(target_os = "macos")]
    {
        macos::set_active(app, cfg.enabled);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, cfg);
    }
}

#[cfg(target_os = "macos")]
mod macos;

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 };

    #[test]
    fn classify_picks_the_nearest_edge() {
        // Near the top → maximize.
        assert_eq!(classify_zone((700.0, 3.0), SCREEN, None), Some(SnapZone::Maximize));
        // Near the left, not the top.
        assert_eq!(classify_zone((4.0, 400.0), SCREEN, None), Some(SnapZone::Left));
        // Near the right.
        assert_eq!(classify_zone((1437.0, 400.0), SCREEN, None), Some(SnapZone::Right));
        // Middle → nothing.
        assert_eq!(classify_zone((700.0, 400.0), SCREEN, None), None);
    }

    #[test]
    fn classify_applies_hysteresis() {
        // Enter the left zone at 4 px.
        let z = classify_zone((4.0, 400.0), SCREEN, None);
        assert_eq!(z, Some(SnapZone::Left));
        // At 15 px the zone is past ENTER (8) but within EXIT (28) → *stays* Left
        // because it's already active; would be None from a cold start.
        assert_eq!(classify_zone((15.0, 400.0), SCREEN, Some(SnapZone::Left)), Some(SnapZone::Left));
        assert_eq!(classify_zone((15.0, 400.0), SCREEN, None), None);
        // Past EXIT → releases.
        assert_eq!(classify_zone((40.0, 400.0), SCREEN, Some(SnapZone::Left)), None);
    }

    #[test]
    fn maximize_band_is_below_the_menu_bar_and_latches_through_it() {
        // Cold: cursor up in the menu bar (top_d < 0) → NOT maximize, so the
        // user can reach the menu bar without us fighting macOS Mission Control.
        assert_eq!(classify_zone((700.0, -10.0), SCREEN, None), None);
        // Cold: in the deep band below the visible-frame top → maximize.
        assert_eq!(classify_zone((700.0, 30.0), SCREEN, None), Some(SnapZone::Maximize));
        // Once active, overshooting up into the menu bar keeps it latched.
        assert_eq!(
            classify_zone((700.0, -10.0), SCREEN, Some(SnapZone::Maximize)),
            Some(SnapZone::Maximize)
        );
        // Dragging well back down releases it.
        assert_eq!(classify_zone((700.0, 200.0), SCREEN, Some(SnapZone::Maximize)), None);
    }

    #[test]
    fn classify_top_wins_at_a_corner() {
        // Top-left corner: top edge has priority → maximize, not left.
        assert_eq!(classify_zone((2.0, 2.0), SCREEN, None), Some(SnapZone::Maximize));
    }

    #[test]
    fn classify_offscreen_cursor_is_none() {
        assert_eq!(classify_zone((-100.0, 400.0), SCREEN, None), None);
        assert_eq!(classify_zone((700.0, 2000.0), SCREEN, None), None);
    }

    #[test]
    fn zone_rects_tile_the_screen() {
        assert_eq!(zone_rect(SnapZone::Maximize, SCREEN), SCREEN);
        let l = zone_rect(SnapZone::Left, SCREEN);
        let r = zone_rect(SnapZone::Right, SCREEN);
        assert_eq!(l, Rect::new(0.0, 0.0, 720.0, 900.0));
        assert_eq!(r, Rect::new(720.0, 0.0, 720.0, 900.0));
        // The two halves exactly cover the width, no gap/overlap.
        assert_eq!(l.w + r.w, SCREEN.w);
        assert_eq!(l.x + l.w, r.x);
    }

    #[test]
    fn cocoa_to_topleft_flips_y_about_primary_height() {
        // A 1440x900 primary: a Cocoa rect at the top of the screen
        // (y = 900 - 900 = 0 from bottom is the bottom; top is y=825 for a 75px-tall
        // bar) converts so its top-left y is small.
        let primary_h = 900.0;
        // Cocoa window: origin (100, 800) size 200x50 → its top is at cocoaY+h=850
        // from the bottom → 50 px from the top → top-left y = 900-850 = 50.
        let tl = cocoa_rect_to_topleft(Rect::new(100.0, 800.0, 200.0, 50.0), primary_h);
        assert_eq!(tl, Rect::new(100.0, 50.0, 200.0, 50.0));
        // A full-height rect stays full-height at y=0.
        let full = cocoa_rect_to_topleft(Rect::new(0.0, 0.0, 1440.0, 900.0), primary_h);
        assert_eq!(full, Rect::new(0.0, 0.0, 1440.0, 900.0));
    }
}
