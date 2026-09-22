//! Cursor wrap-around — "Infinity Monitor" (SopoNext) reimplemented inline.
//!
//! When the pointer is pushed past a **true outer edge** of the whole display
//! arrangement it reappears on the opposite outer edge (Pac-Man wrap), so a
//! trackpad user never has to swipe all the way back across a wide or
//! multi-monitor desktop. Configurable per edge, with a corner dead-zone so
//! macOS Hot Corners keep working.
//!
//! This file is the **platform-independent, unit-tested core**: the wrap
//! geometry ([`wrap_target`]) and the config. The impure part — a listen-only
//! `CGEventTap` on mouse moves that reads the cursor + raw delta and calls
//! `CGWarpMouseCursorPosition` — lives in `macos.rs`.
//!
//! **Coordinate convention:** global top-left-origin **points** — exactly the
//! space of `CGEventGetLocation`, `CGWarpMouseCursorPosition` and
//! `CGDisplayBounds`, so nothing is flipped. Multi-monitor correctness rests on
//! one rule: **wrap only where one pixel past the edge lands on NO display.**
//! An internal seam between adjacent monitors is never wrapped — macOS already
//! flows the cursor across it, and wrapping there would trap the pointer.
//!
//! Wrapping is **on by default** (the user's request); `apply` starts/stops the
//! macOS monitor to match — mirrors `window_snap`/`gestures`. macOS only.

#![allow(dead_code)] // platform monitor lands in macos.rs

use crate::db::DbHandle;

// ── Geometry ─────────────────────────────────────────────────────────────────

/// A display rectangle in top-left-origin points (x right, y down).
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
    fn max_x(&self) -> f64 {
        self.x + self.w
    }
    fn max_y(&self) -> f64 {
        self.y + self.h
    }
    /// Half-open on the far side so a shared seam (e.g. x == 1440 between two
    /// abutting monitors) belongs to exactly ONE display — the right/lower one.
    fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.max_x() && py >= self.y && py < self.max_y()
    }
}

/// The four edges the pointer can be pushed against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// A point (`x`,`y`) is considered "at" an edge when it is within this many
/// points of the boundary — macOS clamps the cursor to ~1 px inside the outer
/// bound, so a small tolerance is needed.
pub const EDGE_EPS: f64 = 2.0;

/// The warp target lands this far inside the destination display, so it never
/// sits exactly on the boundary and immediately re-triggers the opposite edge.
pub const TARGET_INSET: f64 = 2.0;

/// Default corner dead-zone (points): near a corner along the pressed edge, do
/// not wrap, so macOS Hot Corners stay reachable.
pub const DEFAULT_CORNER_DEADZONE_PX: f64 = 8.0;

/// Upper clamp on the configurable dead-zone.
pub const CORNER_DEADZONE_MAX: f64 = 400.0;

/// Find the display containing the cursor (half-open, so a seam is unambiguous).
fn display_at(cursor: (f64, f64), displays: &[Rect]) -> Option<Rect> {
    displays.iter().copied().find(|d| d.contains(cursor.0, cursor.1))
}

/// Vertical distance from `y` to a display's `[min_y, max_y]` span (0 if inside).
fn vgap(y: f64, d: &Rect) -> f64 {
    if y < d.y {
        d.y - y
    } else if y > d.max_y() {
        y - d.max_y()
    } else {
        0.0
    }
}

/// Horizontal distance from `x` to a display's `[min_x, max_x]` span (0 if inside).
fn hgap(x: f64, d: &Rect) -> f64 {
    if x < d.x {
        d.x - x
    } else if x > d.max_x() {
        x - d.max_x()
    } else {
        0.0
    }
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if lo > hi {
        // Degenerate display smaller than 2*inset — just centre.
        (lo + hi) / 2.0
    } else {
        v.max(lo).min(hi)
    }
}

/// The opposite-edge warp target for a confirmed outer edge, clamped into a
/// real display so an offset / L-shaped arrangement never lands the cursor in a
/// gap. Returns `None` only if `displays` is empty.
fn opposite_target(edge: Edge, cursor: (f64, f64), displays: &[Rect]) -> Option<(f64, f64)> {
    if displays.is_empty() {
        return None;
    }
    match edge {
        // Pushed off the RIGHT → reappear on the far LEFT.
        Edge::Right => {
            let gx = displays.iter().map(|d| d.x).fold(f64::INFINITY, f64::min);
            let d = displays
                .iter()
                .filter(|d| (d.x - gx).abs() < 1.0)
                .min_by(|a, b| vgap(cursor.1, a).total_cmp(&vgap(cursor.1, b)))?;
            Some((d.x + TARGET_INSET, clamp(cursor.1, d.y + TARGET_INSET, d.max_y() - TARGET_INSET)))
        }
        // Pushed off the LEFT → reappear on the far RIGHT.
        Edge::Left => {
            let gx = displays.iter().map(|d| d.max_x()).fold(f64::NEG_INFINITY, f64::max);
            let d = displays
                .iter()
                .filter(|d| (d.max_x() - gx).abs() < 1.0)
                .min_by(|a, b| vgap(cursor.1, a).total_cmp(&vgap(cursor.1, b)))?;
            Some((
                d.max_x() - TARGET_INSET,
                clamp(cursor.1, d.y + TARGET_INSET, d.max_y() - TARGET_INSET),
            ))
        }
        // Pushed off the BOTTOM → reappear on the far TOP.
        Edge::Bottom => {
            let gy = displays.iter().map(|d| d.y).fold(f64::INFINITY, f64::min);
            let d = displays
                .iter()
                .filter(|d| (d.y - gy).abs() < 1.0)
                .min_by(|a, b| hgap(cursor.0, a).total_cmp(&hgap(cursor.0, b)))?;
            Some((clamp(cursor.0, d.x + TARGET_INSET, d.max_x() - TARGET_INSET), d.y + TARGET_INSET))
        }
        // Pushed off the TOP → reappear on the far BOTTOM.
        Edge::Top => {
            let gy = displays.iter().map(|d| d.max_y()).fold(f64::NEG_INFINITY, f64::max);
            let d = displays
                .iter()
                .filter(|d| (d.max_y() - gy).abs() < 1.0)
                .min_by(|a, b| hgap(cursor.0, a).total_cmp(&hgap(cursor.0, b)))?;
            Some((
                clamp(cursor.0, d.x + TARGET_INSET, d.max_x() - TARGET_INSET),
                d.max_y() - TARGET_INSET,
            ))
        }
    }
}

/// Is `edge` enabled in the config?
fn edge_enabled(cfg: &CursorWrapConfig, edge: Edge) -> bool {
    match edge {
        Edge::Left => cfg.left,
        Edge::Right => cfg.right,
        Edge::Top => cfg.top,
        Edge::Bottom => cfg.bottom,
    }
}

/// Would wrapping across `edge` land the pointer in a Hot-Corner dead-zone?
/// Near a corner ALONG the pressed edge (the perpendicular coordinate close to
/// the containing display's corner) we suppress the wrap so the corner is free.
fn in_corner_deadzone(edge: Edge, cursor: (f64, f64), d: &Rect, dz: f64) -> bool {
    if dz <= 0.0 {
        return false;
    }
    match edge {
        Edge::Left | Edge::Right => cursor.1 < d.y + dz || cursor.1 > d.max_y() - dz,
        Edge::Top | Edge::Bottom => cursor.0 < d.x + dz || cursor.0 > d.max_x() - dz,
    }
}

/// Is the pointer pressed against `edge` of display `d` (within [`EDGE_EPS`])
/// AND moving toward it (per the raw movement delta `dir`)?
fn pressed_against(edge: Edge, cursor: (f64, f64), d: &Rect, dir: (f64, f64)) -> bool {
    match edge {
        Edge::Right => dir.0 > 0.0 && cursor.0 >= d.max_x() - EDGE_EPS,
        Edge::Left => dir.0 < 0.0 && cursor.0 <= d.x + EDGE_EPS,
        Edge::Bottom => dir.1 > 0.0 && cursor.1 >= d.max_y() - EDGE_EPS,
        Edge::Top => dir.1 < 0.0 && cursor.1 <= d.y + EDGE_EPS,
    }
}

/// Is the edge just past `edge` of `d` a TRUE OUTER edge — i.e. the point one
/// point beyond lands on NO display? (An internal seam returns `false`.)
fn is_outer_edge(edge: Edge, cursor: (f64, f64), d: &Rect, displays: &[Rect]) -> bool {
    let probe = match edge {
        Edge::Right => (d.max_x() + 1.0, cursor.1),
        Edge::Left => (d.x - 1.0, cursor.1),
        Edge::Bottom => (cursor.0, d.max_y() + 1.0),
        Edge::Top => (cursor.0, d.y - 1.0),
    };
    !displays.iter().any(|o| o.contains(probe.0, probe.1))
}

/// **Pure core.** Given the cursor position (global top-left points), every
/// display rect, the raw movement delta `dir` (sign = direction, magnitude only
/// picks the dominant axis at a corner) and the config, return the point to
/// warp the cursor to, or `None`.
pub fn wrap_target(
    cursor: (f64, f64),
    displays: &[Rect],
    dir: (f64, f64),
    cfg: &CursorWrapConfig,
) -> Option<(f64, f64)> {
    if !cfg.enabled {
        return None;
    }
    let d = display_at(cursor, displays)?;

    // Candidate edges, dominant-axis first (so a near-diagonal push at a corner
    // resolves to the axis it's mostly moving along).
    let horizontal = if dir.0 > 0.0 {
        Some(Edge::Right)
    } else if dir.0 < 0.0 {
        Some(Edge::Left)
    } else {
        None
    };
    let vertical = if dir.1 > 0.0 {
        Some(Edge::Bottom)
    } else if dir.1 < 0.0 {
        Some(Edge::Top)
    } else {
        None
    };
    let ordered = if dir.0.abs() >= dir.1.abs() {
        [horizontal, vertical]
    } else {
        [vertical, horizontal]
    };

    for edge in ordered.into_iter().flatten() {
        if !edge_enabled(cfg, edge) {
            continue;
        }
        if !pressed_against(edge, cursor, &d, dir) {
            continue;
        }
        if in_corner_deadzone(edge, cursor, &d, cfg.corner_deadzone_px) {
            continue;
        }
        if !is_outer_edge(edge, cursor, &d, displays) {
            continue; // internal seam — macOS flows the cursor across it
        }
        if let Some(t) = opposite_target(edge, cursor, displays) {
            return Some(t);
        }
    }
    None
}

// ── Config + lifecycle ───────────────────────────────────────────────────────

const KEY_ENABLED: &str = "cursorwrap.enabled";
const KEY_LEFT: &str = "cursorwrap.edge_left";
const KEY_RIGHT: &str = "cursorwrap.edge_right";
const KEY_TOP: &str = "cursorwrap.edge_top";
const KEY_BOTTOM: &str = "cursorwrap.edge_bottom";
const KEY_DEADZONE: &str = "cursorwrap.corner_deadzone_px";
const KEY_DRAG: &str = "cursorwrap.wrap_during_drag";

fn t() -> bool {
    true
}
fn f() -> bool {
    false
}
fn default_deadzone() -> f64 {
    DEFAULT_CORNER_DEADZONE_PX
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct CursorWrapConfig {
    /// ⚠️ Every field is serde-defaulted (the gestures.mute lesson): a payload
    /// from an older frontend missing a field must not silently flip it off.
    #[serde(default = "t")]
    pub enabled: bool,
    #[serde(default = "t")]
    pub left: bool,
    #[serde(default = "t")]
    pub right: bool,
    #[serde(default = "t")]
    pub top: bool,
    #[serde(default = "t")]
    pub bottom: bool,
    #[serde(default = "default_deadzone")]
    pub corner_deadzone_px: f64,
    /// Wrap even while a mouse button is held (a drag). Default off — wrapping
    /// mid-drag can drop a window/selection drag.
    #[serde(default = "f")]
    pub wrap_during_drag: bool,
}

impl Default for CursorWrapConfig {
    fn default() -> Self {
        CursorWrapConfig {
            enabled: true,
            left: true,
            right: true,
            top: true,
            bottom: true,
            corner_deadzone_px: DEFAULT_CORNER_DEADZONE_PX,
            wrap_during_drag: false,
        }
    }
}

/// Pure: stored string → effective dead-zone. Unset/garbage/negative → default;
/// otherwise clamped to `[0, CORNER_DEADZONE_MAX]`.
pub fn normalise_corner_deadzone(raw: Option<&str>) -> f64 {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => DEFAULT_CORNER_DEADZONE_PX,
        Some(s) => match s.parse::<f64>() {
            Ok(n) if n.is_finite() && n >= 0.0 => n.min(CORNER_DEADZONE_MAX),
            _ => DEFAULT_CORNER_DEADZONE_PX,
        },
    }
}

impl CursorWrapConfig {
    pub fn load(db: &DbHandle) -> CursorWrapConfig {
        let dz = crate::settings::get_or(db, KEY_DEADZONE, "").ok();
        CursorWrapConfig {
            enabled: crate::settings::get_bool(db, KEY_ENABLED, true).unwrap_or(true),
            left: crate::settings::get_bool(db, KEY_LEFT, true).unwrap_or(true),
            right: crate::settings::get_bool(db, KEY_RIGHT, true).unwrap_or(true),
            top: crate::settings::get_bool(db, KEY_TOP, true).unwrap_or(true),
            bottom: crate::settings::get_bool(db, KEY_BOTTOM, true).unwrap_or(true),
            corner_deadzone_px: normalise_corner_deadzone(dz.as_deref()),
            wrap_during_drag: crate::settings::get_bool(db, KEY_DRAG, false).unwrap_or(false),
        }
    }
    pub fn save(&self, db: &DbHandle) -> anyhow::Result<()> {
        let b = |v: bool| if v { "true" } else { "false" };
        crate::settings::set(db, KEY_ENABLED, b(self.enabled))?;
        crate::settings::set(db, KEY_LEFT, b(self.left))?;
        crate::settings::set(db, KEY_RIGHT, b(self.right))?;
        crate::settings::set(db, KEY_TOP, b(self.top))?;
        crate::settings::set(db, KEY_BOTTOM, b(self.bottom))?;
        crate::settings::set(
            db,
            KEY_DEADZONE,
            &self.corner_deadzone_px.clamp(0.0, CORNER_DEADZONE_MAX).to_string(),
        )?;
        crate::settings::set(db, KEY_DRAG, b(self.wrap_during_drag))?;
        Ok(())
    }
}

/// Managed Tauri state (the running monitor lives in `macos.rs`).
#[derive(Default)]
pub struct CursorWrapState;

/// Start/stop the wrap monitor to match the saved config. Called at startup and
/// after a settings change (idempotent). On non-macOS it's a no-op.
pub fn apply(app: &tauri::AppHandle, db: &DbHandle, _state: &CursorWrapState) {
    let cfg = CursorWrapConfig::load(db);
    #[cfg(target_os = "macos")]
    {
        // Push the live-tunable params first (they apply to a running tap
        // without a restart), then start/stop.
        macos::set_config(&cfg);
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

    const CFG: CursorWrapConfig = CursorWrapConfig {
        enabled: true,
        left: true,
        right: true,
        top: true,
        bottom: true,
        corner_deadzone_px: 8.0,
        wrap_during_drag: false,
    };
    const S: Rect = Rect { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 };

    fn approx(a: Option<(f64, f64)>, b: (f64, f64)) {
        let p = a.expect("expected a wrap target");
        assert!((p.0 - b.0).abs() < 0.01 && (p.1 - b.1).abs() < 0.01, "got {p:?}, want {b:?}");
    }

    // ── single display: each edge wraps to the opposite ──
    #[test]
    fn right_edge_wraps_to_left() {
        approx(wrap_target((1439.0, 450.0), &[S], (1.0, 0.0), &CFG), (2.0, 450.0));
    }
    #[test]
    fn left_edge_wraps_to_right() {
        approx(wrap_target((0.0, 450.0), &[S], (-1.0, 0.0), &CFG), (1438.0, 450.0));
    }
    #[test]
    fn bottom_edge_wraps_to_top() {
        approx(wrap_target((720.0, 899.0), &[S], (0.0, 1.0), &CFG), (720.0, 2.0));
    }
    #[test]
    fn top_edge_wraps_to_bottom() {
        approx(wrap_target((720.0, 0.0), &[S], (0.0, -1.0), &CFG), (720.0, 898.0));
    }

    // ── negatives ──
    #[test]
    fn not_moving_toward_the_edge_does_not_wrap() {
        assert_eq!(wrap_target((1439.0, 450.0), &[S], (-1.0, 0.0), &CFG), None);
    }
    #[test]
    fn not_at_an_edge_does_not_wrap() {
        assert_eq!(wrap_target((700.0, 450.0), &[S], (1.0, 0.0), &CFG), None);
    }
    #[test]
    fn a_disabled_edge_does_not_wrap() {
        let cfg = CursorWrapConfig { right: false, ..CFG };
        assert_eq!(wrap_target((1439.0, 450.0), &[S], (1.0, 0.0), &cfg), None);
    }
    #[test]
    fn globally_disabled_never_wraps() {
        let cfg = CursorWrapConfig { enabled: false, ..CFG };
        assert_eq!(wrap_target((1439.0, 450.0), &[S], (1.0, 0.0), &cfg), None);
    }

    // ── corner dead-zone keeps Hot Corners reachable ──
    #[test]
    fn top_right_corner_is_in_the_deadzone() {
        assert_eq!(wrap_target((1439.0, 3.0), &[S], (1.0, 0.0), &CFG), None);
    }
    #[test]
    fn bottom_right_corner_is_in_the_deadzone() {
        assert_eq!(wrap_target((1439.0, 897.0), &[S], (1.0, 0.0), &CFG), None);
    }
    #[test]
    fn a_zero_deadzone_wraps_right_up_to_the_corner() {
        let cfg = CursorWrapConfig { corner_deadzone_px: 0.0, ..CFG };
        approx(wrap_target((1439.0, 3.0), &[S], (1.0, 0.0), &cfg), (2.0, 3.0));
    }

    // ── two displays side by side: NEVER wrap at the internal seam ──
    const A: Rect = Rect { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 };
    const B: Rect = Rect { x: 1440.0, y: 0.0, w: 1440.0, h: 900.0 };

    #[test]
    fn internal_seam_from_the_left_screen_does_not_wrap() {
        assert_eq!(wrap_target((1439.0, 450.0), &[A, B], (1.0, 0.0), &CFG), None);
    }
    #[test]
    fn internal_seam_from_the_right_screen_does_not_wrap() {
        // x == 1440 belongs to B (half-open); pushing left probes into A.
        assert_eq!(wrap_target((1440.0, 450.0), &[A, B], (-1.0, 0.0), &CFG), None);
    }
    #[test]
    fn outer_right_of_the_arrangement_wraps_to_the_far_left() {
        approx(wrap_target((2879.0, 450.0), &[A, B], (1.0, 0.0), &CFG), (2.0, 450.0));
    }
    #[test]
    fn outer_left_of_the_arrangement_wraps_to_the_far_right() {
        approx(wrap_target((0.0, 450.0), &[A, B], (-1.0, 0.0), &CFG), (2878.0, 450.0));
    }

    // ── vertically OFFSET side-by-side: target clamps into a real display ──
    const AL: Rect = Rect { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 };
    const BL: Rect = Rect { x: 1440.0, y: 100.0, w: 1920.0, h: 1080.0 };

    #[test]
    fn offset_wrap_clamps_the_target_into_a_real_display_not_a_gap() {
        // Cursor at B's right edge, at a y ABOVE where A exists (y=1000, A ends
        // at 900). Wrapping left must clamp y into A, not land in empty space.
        approx(wrap_target((3359.0, 1000.0), &[AL, BL], (1.0, 0.0), &CFG), (2.0, 898.0));
    }

    // ── stacked vertically: internal seam vs. outer top/bottom ──
    const TOP: Rect = Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 };
    const BOT: Rect = Rect { x: 0.0, y: 1080.0, w: 1920.0, h: 1080.0 };

    #[test]
    fn stacked_internal_seam_does_not_wrap() {
        assert_eq!(wrap_target((960.0, 1079.0), &[TOP, BOT], (0.0, 1.0), &CFG), None);
    }
    #[test]
    fn stacked_outer_bottom_wraps_to_the_very_top() {
        approx(wrap_target((960.0, 2159.0), &[TOP, BOT], (0.0, 1.0), &CFG), (960.0, 2.0));
    }
    #[test]
    fn stacked_outer_top_wraps_to_the_very_bottom() {
        approx(wrap_target((960.0, 0.0), &[TOP, BOT], (0.0, -1.0), &CFG), (960.0, 2158.0));
    }

    // ── degenerate ──
    #[test]
    fn no_displays_never_wraps() {
        assert_eq!(wrap_target((0.0, 0.0), &[], (1.0, 0.0), &CFG), None);
    }
    #[test]
    fn cursor_outside_every_display_does_not_wrap() {
        assert_eq!(wrap_target((5000.0, 5000.0), &[S], (1.0, 0.0), &CFG), None);
    }

    // ── config normalisation ──
    fn feq(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "got {a}, want {b}");
    }
    #[test]
    fn deadzone_defaults_and_clamps() {
        feq(normalise_corner_deadzone(None), DEFAULT_CORNER_DEADZONE_PX);
        feq(normalise_corner_deadzone(Some("  ")), DEFAULT_CORNER_DEADZONE_PX);
        feq(normalise_corner_deadzone(Some("junk")), DEFAULT_CORNER_DEADZONE_PX);
        feq(normalise_corner_deadzone(Some("-5")), DEFAULT_CORNER_DEADZONE_PX);
        feq(normalise_corner_deadzone(Some("20")), 20.0);
        feq(normalise_corner_deadzone(Some("99999")), CORNER_DEADZONE_MAX);
        feq(normalise_corner_deadzone(Some("0")), 0.0);
    }
    #[test]
    fn defaults_are_on_all_edges() {
        let d = CursorWrapConfig::default();
        assert!(d.enabled && d.left && d.right && d.top && d.bottom);
        assert!(!d.wrap_during_drag);
        feq(d.corner_deadzone_px, DEFAULT_CORNER_DEADZONE_PX);
    }
}
