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

// ── Dwell arming (v0.165.0) ──────────────────────────────────────────────────
//
// Field report 2026-09-04: "der Vorschlag kommt zu früh — oft will ich das
// Windowmanagement gar nicht starten". Dragging a window NEAR an edge to
// position it there put the cursor inside ENTER_PX and instantly showed the
// preview — and releasing snapped. The fix is INTENT detection, the
// Magnet/macOS-tiling model: a zone only ARMS (preview shown + snap on
// release) after the cursor has DWELLED in it for `dwell_ms`. A fast
// pass-through or a brisk place-near-the-edge never arms; pausing at the
// edge does. `dwell_ms = 0` restores the old instant behaviour.

/// Ships-with dwell before a zone arms. Long enough that "just placing the
/// window near the edge" doesn't trigger, short enough that a deliberate
/// hold doesn't feel like waiting.
pub const DEFAULT_DWELL_MS: u64 = 350;
/// Ceiling for the setting — past this the feature feels broken, not calm.
pub const DWELL_MS_MAX: u64 = 2000;

/// Dwell-arming state, advanced once per observation (drag event or the
/// between-slice tick for a perfectly still cursor). Pure and clock-agnostic:
/// `now_ms` is any monotonic millisecond counter.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct DwellState {
    /// The zone the cursor is currently in (classified, armed or not).
    pub candidate: Option<SnapZone>,
    /// When `candidate` was entered.
    pub since_ms: u64,
    /// Whether the candidate has passed the dwell and is LIVE — only an armed
    /// zone shows the preview, and only an armed zone snaps on release.
    pub armed: bool,
}

/// Advance the dwell state. Leaving all zones resets everything; switching
/// zones restarts the clock (an armed Left does NOT carry its arming over to
/// Maximize at the corner); staying armed stays armed for as long as the
/// zone is held.
pub fn dwell_step(prev: DwellState, zone: Option<SnapZone>, now_ms: u64, dwell_ms: u64) -> DwellState {
    match zone {
        None => DwellState::default(),
        Some(z) if prev.candidate == Some(z) => DwellState {
            candidate: Some(z),
            since_ms: prev.since_ms,
            armed: prev.armed || now_ms.saturating_sub(prev.since_ms) >= dwell_ms,
        },
        Some(z) => DwellState {
            candidate: Some(z),
            since_ms: now_ms,
            // dwell 0 = the pre-v0.165 instant behaviour.
            armed: dwell_ms == 0,
        },
    }
}

/// The zone that is allowed to act (preview + snap): armed candidate or none.
pub fn armed_zone(state: DwellState) -> Option<SnapZone> {
    if state.armed { state.candidate } else { None }
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
const KEY_DWELL_MS: &str = "windowsnap.dwell_ms";

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct WindowSnapConfig {
    /// Off by default (opt-in) — the `bool` default `false` is exactly that.
    pub enabled: bool,
    /// Dwell before a zone arms (v0.165.0); `0` = instant (old behaviour).
    /// ⚠️ serde-defaulted — the gestures.mute lesson: a payload from an older
    /// frontend without this field must not silently zero the delay.
    #[serde(default = "default_dwell_ms")]
    pub dwell_ms: u64,
}

fn default_dwell_ms() -> u64 {
    DEFAULT_DWELL_MS
}

impl Default for WindowSnapConfig {
    fn default() -> Self {
        WindowSnapConfig { enabled: false, dwell_ms: DEFAULT_DWELL_MS }
    }
}

/// Pure: stored string → effective dwell. Unset/garbage falls back to the
/// default; `0` is honoured as "instant"; everything else clamps to the max.
pub fn normalise_dwell_ms(raw: Option<&str>) -> u64 {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => DEFAULT_DWELL_MS,
        Some(s) => match s.parse::<u64>() {
            Ok(n) => n.min(DWELL_MS_MAX),
            Err(_) => DEFAULT_DWELL_MS,
        },
    }
}

impl WindowSnapConfig {
    pub fn load(db: &DbHandle) -> WindowSnapConfig {
        let raw = crate::settings::get_or(db, KEY_DWELL_MS, "").ok();
        WindowSnapConfig {
            enabled: crate::settings::get_bool(db, KEY_ENABLED, false).unwrap_or(false),
            dwell_ms: normalise_dwell_ms(raw.as_deref()),
        }
    }
    pub fn save(&self, db: &DbHandle) -> anyhow::Result<()> {
        crate::settings::set(db, KEY_ENABLED, if self.enabled { "true" } else { "false" })?;
        crate::settings::set(db, KEY_DWELL_MS, &self.dwell_ms.min(DWELL_MS_MAX).to_string())?;
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
        // Dwell first, and unconditionally: `set_active` early-returns when
        // the monitor is already running, but the atomic makes a settings
        // change take effect live without a restart.
        macos::set_dwell_ms(cfg.dwell_ms);
        macos::set_active(app, cfg.enabled);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, cfg);
    }
}

#[cfg(target_os = "macos")]
pub(crate) mod macos;

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 };

    #[test]
    fn dwell_arms_only_after_the_configured_hold() {
        // The v0.165.0 contract: dragging INTO a zone shows nothing at first
        // ("der Vorschlag kommt zu früh" — placing a window near the edge
        // must not trigger); only holding it there arms preview + snap.
        let d = 350;
        let s0 = dwell_step(DwellState::default(), Some(SnapZone::Left), 1_000, d);
        assert_eq!(s0.candidate, Some(SnapZone::Left));
        assert!(!s0.armed, "entering a zone must NOT arm immediately");
        assert_eq!(armed_zone(s0), None);
        // Still short of the dwell → still calm.
        let s1 = dwell_step(s0, Some(SnapZone::Left), 1_000 + d - 1, d);
        assert!(!s1.armed);
        // Boundary inclusive: exactly dwell later → armed.
        let s2 = dwell_step(s1, Some(SnapZone::Left), 1_000 + d, d);
        assert!(s2.armed);
        assert_eq!(armed_zone(s2), Some(SnapZone::Left));
        // Armed stays armed for as long as the zone is held.
        let s3 = dwell_step(s2, Some(SnapZone::Left), 999_999, d);
        assert!(s3.armed);
    }

    #[test]
    fn dwell_zone_change_restarts_the_clock_and_drops_the_arming() {
        // An armed Left must not carry its arming to Maximize at the corner —
        // the user gets the same grace period before the NEW zone acts.
        let d = 350;
        let armed = DwellState { candidate: Some(SnapZone::Left), since_ms: 0, armed: true };
        let switched = dwell_step(armed, Some(SnapZone::Maximize), 5_000, d);
        assert_eq!(switched.candidate, Some(SnapZone::Maximize));
        assert!(!switched.armed, "arming must not survive a zone switch");
        assert_eq!(switched.since_ms, 5_000, "the clock must restart on a zone switch");
        // Leaving all zones resets everything.
        assert_eq!(dwell_step(switched, None, 5_100, d), DwellState::default());
    }

    #[test]
    fn dwell_zero_is_the_old_instant_behaviour() {
        let s = dwell_step(DwellState::default(), Some(SnapZone::Right), 42, 0);
        assert!(s.armed, "dwell 0 must arm on entry — the pre-v0.165 feel");
        assert_eq!(armed_zone(s), Some(SnapZone::Right));
    }

    #[test]
    fn normalise_dwell_defaults_and_clamps() {
        // Unset / blank / garbage → default (a hand-edited DB must not make
        // snapping instant OR unreachable by accident); big values clamp.
        for raw in [None, Some(""), Some("  "), Some("abc"), Some("-5")] {
            assert_eq!(normalise_dwell_ms(raw), DEFAULT_DWELL_MS, "raw={raw:?}");
        }
        assert_eq!(normalise_dwell_ms(Some("0")), 0);
        assert_eq!(normalise_dwell_ms(Some("500")), 500);
        assert_eq!(normalise_dwell_ms(Some("999999")), DWELL_MS_MAX);
        assert_eq!(normalise_dwell_ms(Some(" 350 ")), 350);
    }

    #[test]
    fn config_payload_without_dwell_keeps_the_default() {
        // ⚠️ The gestures.mute lesson (2026-09-04): an older frontend sending
        // `{"enabled":true}` must not zero the delay through deserialisation.
        let cfg: WindowSnapConfig = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert_eq!(cfg.dwell_ms, DEFAULT_DWELL_MS);
        assert!(cfg.enabled);
    }

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

    // ── Offset / multi-monitor screens ──────────────────────────────────────

    /// A secondary display to the right of the primary (origin not at 0,0),
    /// with a top inset (menu bar) so the visible frame starts below it.
    const SECONDARY: Rect = Rect { x: 1440.0, y: 25.0, w: 1920.0, h: 1055.0 };

    #[test]
    fn classify_is_relative_to_an_offset_screen() {
        // Left edge of the secondary is at x=1440, not 0.
        assert_eq!(classify_zone((1443.0, 500.0), SECONDARY, None), Some(SnapZone::Left));
        // Right edge is at x = 1440 + 1920 = 3360.
        assert_eq!(classify_zone((3357.0, 500.0), SECONDARY, None), Some(SnapZone::Right));
        // Top band is below the screen's y origin (25), not below 0.
        assert_eq!(classify_zone((2400.0, 40.0), SECONDARY, None), Some(SnapZone::Maximize));
        // The primary's left edge (x≈0) is NOT on the secondary.
        assert_eq!(classify_zone((3.0, 500.0), SECONDARY, None), None);
    }

    #[test]
    fn zone_rects_offset_screen_tile_exactly() {
        let l = zone_rect(SnapZone::Left, SECONDARY);
        let r = zone_rect(SnapZone::Right, SECONDARY);
        assert_eq!(l, Rect::new(1440.0, 25.0, 960.0, 1055.0));
        assert_eq!(r, Rect::new(2400.0, 25.0, 960.0, 1055.0));
        assert_eq!(l.x + l.w, r.x); // no gap
        assert_eq!(l.w + r.w, SECONDARY.w); // no overlap
        assert_eq!(zone_rect(SnapZone::Maximize, SECONDARY), SECONDARY);
    }

    #[test]
    fn odd_width_halves_floor_and_still_cover_the_full_width() {
        // Odd width: the left half floors, the right half takes the remainder
        // (one extra px) so the two together cover the width with no gap.
        let screen = Rect::new(0.0, 0.0, 1001.0, 600.0);
        let l = zone_rect(SnapZone::Left, screen);
        let r = zone_rect(SnapZone::Right, screen);
        assert_eq!(l.w, 500.0);
        assert_eq!(r.w, 501.0);
        assert_eq!(l.x + l.w, r.x);
        assert_eq!(l.w + r.w, screen.w);
        assert_eq!(r.x + r.w, screen.x + screen.w); // right edge flush
    }

    #[test]
    fn right_edge_applies_hysteresis_like_the_left() {
        let right_x = SCREEN.x + SCREEN.w; // 1440
        // Cold: within ENTER (8) of the right edge → Right.
        assert_eq!(classify_zone((right_x - 4.0, 400.0), SCREEN, None), Some(SnapZone::Right));
        // Already Right: stays until past EXIT (28).
        assert_eq!(
            classify_zone((right_x - 20.0, 400.0), SCREEN, Some(SnapZone::Right)),
            Some(SnapZone::Right)
        );
        // Cold at 20 px in → not yet (past ENTER).
        assert_eq!(classify_zone((right_x - 20.0, 400.0), SCREEN, None), None);
        // Past EXIT → releases.
        assert_eq!(
            classify_zone((right_x - 40.0, 400.0), SCREEN, Some(SnapZone::Right)),
            None
        );
    }

    #[test]
    fn maximize_enter_boundary_is_inclusive() {
        // Exactly at MAXIMIZE_ENTER_PX → maximize (inclusive range).
        assert_eq!(
            classify_zone((700.0, MAXIMIZE_ENTER_PX), SCREEN, None),
            Some(SnapZone::Maximize)
        );
        // Just past it → not (from a cold start).
        assert_eq!(classify_zone((700.0, MAXIMIZE_ENTER_PX + 1.0), SCREEN, None), None);
    }

    #[test]
    fn maximize_latch_releases_past_the_exit_threshold() {
        // Latched and just within EXIT → stays maximize.
        assert_eq!(
            classify_zone((700.0, MAXIMIZE_EXIT_PX), SCREEN, Some(SnapZone::Maximize)),
            Some(SnapZone::Maximize)
        );
        // Latched but past EXIT → releases.
        assert_eq!(
            classify_zone((700.0, MAXIMIZE_EXIT_PX + 1.0), SCREEN, Some(SnapZone::Maximize)),
            None
        );
    }

    #[test]
    fn top_band_beats_a_side_edge_in_the_corner_overlap() {
        // Cursor near the LEFT edge but also inside the maximize band → top
        // (maximize) wins, since it's checked first.
        assert_eq!(classify_zone((3.0, 20.0), SCREEN, None), Some(SnapZone::Maximize));
    }

    #[test]
    fn side_edge_wins_below_the_maximize_band() {
        // Near the left edge but BELOW the maximize band (y > ENTER) → Left,
        // not maximize (so you can left-snap along the top of the side).
        assert_eq!(
            classify_zone((3.0, MAXIMIZE_ENTER_PX + 10.0), SCREEN, None),
            Some(SnapZone::Left)
        );
    }

    #[test]
    fn cold_cursor_in_the_menu_bar_is_not_maximize() {
        // top_d < 0 (above the visible-frame top, i.e. in the menu bar) → no
        // maximize from a cold start, so we don't fight macOS Mission Control.
        assert_eq!(classify_zone((700.0, -5.0), SCREEN, None), None);
    }

    #[test]
    fn slop_boundary_just_outside_the_screen() {
        // Within EXIT_PX of the left edge (outside) is still considered (and is
        // a Left candidate); beyond EXIT_PX → fully ignored.
        assert_eq!(classify_zone((-5.0, 400.0), SCREEN, None), Some(SnapZone::Left));
        assert_eq!(classify_zone((-(EXIT_PX + 1.0), 400.0), SCREEN, None), None);
        assert_eq!(classify_zone((700.0, SCREEN.h + EXIT_PX + 1.0), SCREEN, None), None);
    }

    #[test]
    fn cocoa_to_topleft_handles_a_screen_above_the_primary() {
        // A second display stacked ABOVE the primary has positive Cocoa y
        // beyond the primary height; the flip yields a negative top-left y
        // (above the primary's top), which is the correct global coordinate.
        let primary_h = 900.0;
        let tl = cocoa_rect_to_topleft(Rect::new(0.0, 900.0, 1440.0, 900.0), primary_h);
        assert_eq!(tl, Rect::new(0.0, -900.0, 1440.0, 900.0));
    }
}
