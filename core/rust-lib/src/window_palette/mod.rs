//! Moom-style **window palette** (macOS, MVP): hover the green zoom button of a
//! window → a small overlay appears anchored under it with preset layouts
//! (maximize / halves / quarters) and a **hex grid** you drag a rectangle over;
//! release → the window snaps into that region of its screen.
//!
//! This module is the platform-independent core: config (opt-in + grid density),
//! the [`WindowPaletteState`] lifecycle, and the pure, unit-tested
//! [`fraction_to_rect`] (0..1 grid fraction → absolute top-left rect on the
//! target screen). The impure pieces — the global mouse-move hover monitor + AX
//! zoom-button hit-testing + the palette overlay window + the AX window
//! move/resize — live in `macos.rs` (which reuses `window_snap`'s screen
//! helpers + AX↔Cocoa conversion). The palette UI itself is a Tauri webview
//! (`WindowPalette.tsx`); the hex-grid geometry is the pure `lib/hexgrid.ts`.

#![allow(dead_code)] // platform monitor + AX live in macos.rs

use crate::db::DbHandle;
use crate::window_snap::Rect;

const KEY_ENABLED: &str = "windowpalette.enabled";
const KEY_TRIGGER: &str = "windowpalette.trigger";

/// First macOS release whose system tiling owns plain hover over the green
/// zoom button (macOS 15 Sequoia).
pub const FIRST_TILING_MACOS: u32 = 15;
const KEY_COLS: &str = "windowpalette.cols";
const KEY_ROWS: &str = "windowpalette.rows";

pub const DEFAULT_COLS: u32 = 16;
pub const DEFAULT_ROWS: u32 = 10;
const MIN_CELLS: u32 = 2;
const MAX_CELLS: u32 = 24;

/// How the palette is summoned.
///
/// ⚠️ `ZoomHover` was the original (Moom-style) trigger and is no longer the
/// default, because **since macOS 15 the system itself owns plain hover over
/// the green zoom button**: it opens Apple's own tiling menu ("Move & Resize" /
/// "Fill & Arrange"), and there is NO way to suppress it. Verified empirically
/// on macOS 26.6.2 — the WindowManager binary carries exactly three tiling
/// switches (`enableTilingByEdgeDrag`, `enableTopTilingByEdgeDrag`,
/// `enableTilingOptionAccelerator`), none of which touches the hover menu, and
/// System Settings offers no toggle for it either. Two popovers then fight over
/// the same few pixels, so the default moved to a trigger macOS does not claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaletteTrigger {
    /// Hover the green zoom button. Pre-macOS-15 behaviour; conflicts on 15+.
    ZoomHover,
    /// Hover a window's title bar while holding Control+Option. macOS claims
    /// neither that band nor that chord, so nothing else pops up.
    TitlebarModifier,
    /// A rebindable global shortcut, acting on the focused window. Conflict-free
    /// by construction — no pointer monitoring at all.
    Hotkey,
}

impl PaletteTrigger {
    fn as_key(self) -> &'static str {
        match self {
            PaletteTrigger::ZoomHover => "zoom_hover",
            PaletteTrigger::TitlebarModifier => "titlebar_modifier",
            PaletteTrigger::Hotkey => "hotkey",
        }
    }
    /// Parse a stored value. Unknown/garbage yields `None` so the caller falls
    /// back to the OS-aware default rather than silently arming a mode the user
    /// never chose.
    fn parse(s: &str) -> Option<PaletteTrigger> {
        match s.trim() {
            "zoom_hover" => Some(PaletteTrigger::ZoomHover),
            "titlebar_modifier" => Some(PaletteTrigger::TitlebarModifier),
            "hotkey" => Some(PaletteTrigger::Hotkey),
            _ => None,
        }
    }
    /// Does moving the pointer summon the palette?
    ///
    /// ⚠️ This is NOT "does the mouse tap run". The tap runs in every mode,
    /// because the palette is a non-activating floating panel whose WKWebView
    /// hover tracking is unreliable — the tap is what forwards the pointer to
    /// it. Only the hit-test is gated on this.
    pub fn summoned_by_pointer(self) -> bool {
        !matches!(self, PaletteTrigger::Hotkey)
    }
}

/// Which trigger a fresh install (or one that never chose explicitly) gets.
///
/// ⚠️ `None` — an unreadable OS version — deliberately resolves to the
/// conflict-free trigger, not to the legacy one: the cost of guessing "old
/// macOS" wrongly is two popovers fighting on every window, while guessing
/// "new macOS" wrongly only costs a slightly less convenient trigger.
pub fn default_trigger(os_major: Option<u32>) -> PaletteTrigger {
    match os_major {
        Some(m) if m < FIRST_TILING_MACOS => PaletteTrigger::ZoomHover,
        _ => PaletteTrigger::TitlebarModifier,
    }
}

/// Major version out of a `sysinfo` OS version string ("26.6.2" -> 26).
pub fn parse_os_major(s: &str) -> Option<u32> {
    s.trim().split('.').next()?.parse().ok()
}

/// Host macOS major version, read once. `None` when unreadable (or not macOS).
fn host_os_major() -> Option<u32> {
    #[cfg(target_os = "macos")]
    {
        use std::sync::OnceLock;
        static CACHE: OnceLock<Option<u32>> = OnceLock::new();
        *CACHE.get_or_init(|| sysinfo::System::os_version().as_deref().and_then(parse_os_major))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct WindowPaletteConfig {
    /// Off by default (opt-in).
    pub enabled: bool,
    /// Hex-grid horizontal cell count.
    pub cols: u32,
    /// Hex-grid vertical cell count.
    pub rows: u32,
    /// How the palette is summoned.
    pub trigger: PaletteTrigger,
}

impl Default for WindowPaletteConfig {
    fn default() -> Self {
        WindowPaletteConfig {
            enabled: false,
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
            trigger: default_trigger(host_os_major()),
        }
    }
}

impl WindowPaletteConfig {
    pub fn load(db: &DbHandle) -> WindowPaletteConfig {
        let d = WindowPaletteConfig::default();
        WindowPaletteConfig {
            enabled: crate::settings::get_bool(db, KEY_ENABLED, false).unwrap_or(false),
            cols: clamp_cells(
                crate::settings::get(db, KEY_COLS)
                    .ok()
                    .flatten()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(d.cols),
            ),
            trigger: crate::settings::get(db, KEY_TRIGGER)
                .ok()
                .flatten()
                .as_deref()
                .and_then(PaletteTrigger::parse)
                .unwrap_or_else(|| default_trigger(host_os_major())),
            rows: clamp_cells(
                crate::settings::get(db, KEY_ROWS)
                    .ok()
                    .flatten()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(d.rows),
            ),
        }
    }
    pub fn save(&self, db: &DbHandle) -> anyhow::Result<()> {
        crate::settings::set(db, KEY_ENABLED, if self.enabled { "true" } else { "false" })?;
        crate::settings::set(db, KEY_COLS, &clamp_cells(self.cols).to_string())?;
        crate::settings::set(db, KEY_ROWS, &clamp_cells(self.rows).to_string())?;
        crate::settings::set(db, KEY_TRIGGER, self.trigger.as_key())?;
        Ok(())
    }
}

fn clamp_cells(n: u32) -> u32 {
    n.clamp(MIN_CELLS, MAX_CELLS)
}

/// CGEvent modifier masks for the title-bar trigger (Quartz `CGEventFlags`).
const FLAG_CONTROL: u64 = 0x0004_0000;
const FLAG_ALTERNATE: u64 = 0x0008_0000;

/// Height of the band at a window's top edge that counts as its title bar.
pub const TITLEBAR_BAND: f64 = 28.0;

/// Is the title-bar trigger's chord (Control+Option) held?
///
/// ⚠️ Deliberately TWO modifiers. One alone is not safe here: Control-click is
/// the right-click substitute on macOS, and Option-click on a title bar is
/// meaningful in several apps — either would fire the palette during ordinary
/// use. Holding both while merely moving over a title bar essentially never
/// happens by accident.
pub fn titlebar_chord_held(flags: u64) -> bool {
    flags & FLAG_CONTROL != 0 && flags & FLAG_ALTERNATE != 0
}

/// Is `cursor` inside `win`'s title-bar band (top-left coords)?
pub fn in_titlebar_band(win: Rect, cursor: (f64, f64), band: f64) -> bool {
    cursor.0 >= win.x
        && cursor.0 <= win.x + win.w
        && cursor.1 >= win.y
        && cursor.1 <= win.y + band.min(win.h)
}

/// Map a 0..1 grid fraction to an absolute top-left rect within `screen` (the
/// target screen's visible frame, top-left coords). Pure + unit-tested.
pub fn fraction_to_rect(fx: f64, fy: f64, fw: f64, fh: f64, screen: Rect) -> Rect {
    Rect::new(
        screen.x + fx * screen.w,
        screen.y + fy * screen.h,
        fw * screen.w,
        fh * screen.h,
    )
}

/// Context handed to the palette webview on show: grid density + the target
/// screen's visible-frame dimensions (so the hex grid matches the screen aspect).
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct PaletteContext {
    pub cols: u32,
    pub rows: u32,
    pub screen_w: f64,
    pub screen_h: f64,
}

/// Managed Tauri state (handles live in `macos.rs` statics; unit for symmetry).
#[derive(Default)]
pub struct WindowPaletteState;

/// Start/stop the hover monitor to match the saved config. Called at startup and
/// after a settings change (idempotent). No-op on non-macOS.
pub fn apply(app: &tauri::AppHandle, db: &DbHandle, _state: &WindowPaletteState) {
    let cfg = WindowPaletteConfig::load(db);
    #[cfg(target_os = "macos")]
    {
        macos::set_active(app, db, cfg);
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

    const VF: Rect = Rect { x: 0.0, y: 25.0, w: 1440.0, h: 875.0 };

    fn test_db() -> crate::db::DbHandle {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let db = std::sync::Arc::new(parking_lot::Mutex::new(conn));
        crate::settings::init_table(&db).unwrap();
        db
    }

    #[test]
    fn the_default_trigger_avoids_the_macos_tiling_conflict() {
        // macOS 15+ owns plain hover over the green button -> never default there.
        for m in [15u32, 16, 26, 99] {
            assert_eq!(default_trigger(Some(m)), PaletteTrigger::TitlebarModifier, "macOS {m}");
        }
        // Older releases keep the nicer Moom-style hover.
        for m in [10u32, 13, 14] {
            assert_eq!(default_trigger(Some(m)), PaletteTrigger::ZoomHover, "macOS {m}");
        }
        // ⚠️ Unreadable version resolves to the CONFLICT-FREE trigger, not the
        // legacy one: guessing "old" wrongly costs two fighting popovers on
        // every window; guessing "new" wrongly costs only convenience.
        assert_eq!(default_trigger(None), PaletteTrigger::TitlebarModifier);
    }

    #[test]
    fn os_major_is_parsed_from_the_version_string() {
        assert_eq!(parse_os_major("26.6.2"), Some(26));
        assert_eq!(parse_os_major("15"), Some(15));
        assert_eq!(parse_os_major(" 14.7.1 "), Some(14));
        assert_eq!(parse_os_major(""), None);
        assert_eq!(parse_os_major("Sonoma"), None);
    }

    #[test]
    fn trigger_round_trips_and_garbage_falls_back() {
        for tr in [
            PaletteTrigger::ZoomHover,
            PaletteTrigger::TitlebarModifier,
            PaletteTrigger::Hotkey,
        ] {
            assert_eq!(PaletteTrigger::parse(tr.as_key()), Some(tr));
        }
        // A hand-edited value must not silently arm a mode nobody picked.
        assert_eq!(PaletteTrigger::parse("hover"), None);
        assert_eq!(PaletteTrigger::parse(""), None);
    }

    #[test]
    fn only_the_hotkey_trigger_is_not_summoned_by_the_pointer() {
        assert!(PaletteTrigger::ZoomHover.summoned_by_pointer());
        assert!(PaletteTrigger::TitlebarModifier.summoned_by_pointer());
        assert!(!PaletteTrigger::Hotkey.summoned_by_pointer());
    }

    #[test]
    fn the_titlebar_chord_needs_both_modifiers() {
        assert!(titlebar_chord_held(FLAG_CONTROL | FLAG_ALTERNATE));
        assert!(titlebar_chord_held(FLAG_CONTROL | FLAG_ALTERNATE | 0x0010_0000)); // + Cmd is fine
        // ⚠️ One alone must NOT fire: Control-click is right-click, and
        // Option-click on a title bar is meaningful in several apps.
        assert!(!titlebar_chord_held(FLAG_CONTROL));
        assert!(!titlebar_chord_held(FLAG_ALTERNATE));
        assert!(!titlebar_chord_held(0));
    }

    #[test]
    fn the_titlebar_band_is_the_top_edge_only() {
        let w = Rect::new(100.0, 200.0, 800.0, 600.0);
        assert!(in_titlebar_band(w, (500.0, 205.0), TITLEBAR_BAND));
        assert!(in_titlebar_band(w, (100.0, 200.0), TITLEBAR_BAND)); // top-left corner
        // Below the band -> the window body, not the title bar.
        assert!(!in_titlebar_band(w, (500.0, 240.0), TITLEBAR_BAND));
        // Outside horizontally.
        assert!(!in_titlebar_band(w, (99.0, 205.0), TITLEBAR_BAND));
        assert!(!in_titlebar_band(w, (901.0, 205.0), TITLEBAR_BAND));
        // ⚠️ A window shorter than the band never yields a band taller than it.
        let tiny = Rect::new(0.0, 0.0, 200.0, 10.0);
        assert!(!in_titlebar_band(tiny, (100.0, 15.0), TITLEBAR_BAND));
    }

    #[test]
    fn an_existing_install_without_a_stored_trigger_gets_the_default() {
        let db = test_db();
        // Nothing stored -> the OS-aware default. ⚠️ This IS the migration:
        // an install from before the setting existed leaves the conflict
        // without the user having to change anything.
        let loaded = WindowPaletteConfig::load(&db);
        assert_eq!(loaded.trigger, default_trigger(host_os_major()));
        // An explicit choice survives, including the legacy one.
        let mut cfg = loaded;
        cfg.trigger = PaletteTrigger::ZoomHover;
        cfg.save(&db).unwrap();
        assert_eq!(WindowPaletteConfig::load(&db).trigger, PaletteTrigger::ZoomHover);
    }

    #[test]
    fn fraction_maps_full_screen() {
        assert_eq!(fraction_to_rect(0.0, 0.0, 1.0, 1.0, VF), VF);
    }

    #[test]
    fn fraction_maps_left_half() {
        let r = fraction_to_rect(0.0, 0.0, 0.5, 1.0, VF);
        assert_eq!(r, Rect::new(0.0, 25.0, 720.0, 875.0));
    }

    #[test]
    fn fraction_maps_right_half_with_screen_offset() {
        let r = fraction_to_rect(0.5, 0.0, 0.5, 1.0, VF);
        assert_eq!(r, Rect::new(720.0, 25.0, 720.0, 875.0));
    }

    #[test]
    fn fraction_maps_bottom_right_quarter() {
        let r = fraction_to_rect(0.5, 0.5, 0.5, 0.5, VF);
        assert_eq!(r.x, 720.0);
        assert!((r.y - (25.0 + 437.5)).abs() < 1e-9);
        assert_eq!(r.w, 720.0);
        assert!((r.h - 437.5).abs() < 1e-9);
    }

    #[test]
    fn fraction_honours_the_screen_origin() {
        // A secondary screen offset to the right: fractions land on *that* screen.
        let sec = Rect::new(1440.0, 0.0, 1920.0, 1080.0);
        let r = fraction_to_rect(0.0, 0.0, 0.5, 1.0, sec);
        assert_eq!(r, Rect::new(1440.0, 0.0, 960.0, 1080.0));
    }

    #[test]
    fn config_clamps_cell_counts() {
        assert_eq!(clamp_cells(0), MIN_CELLS);
        assert_eq!(clamp_cells(99), MAX_CELLS);
        assert_eq!(clamp_cells(8), 8);
    }

    fn mem_db() -> DbHandle {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let db = std::sync::Arc::new(parking_lot::Mutex::new(conn));
        crate::settings::init_table(&db).unwrap();
        db
    }

    #[test]
    fn config_load_defaults_when_unset() {
        let db = mem_db();
        let cfg = WindowPaletteConfig::load(&db);
        assert!(!cfg.enabled);
        assert_eq!(cfg.cols, DEFAULT_COLS);
        assert_eq!(cfg.rows, DEFAULT_ROWS);
    }

    #[test]
    fn config_save_load_roundtrip_and_clamps_on_load() {
        let db = mem_db();
        // Save an out-of-range density; load clamps it back into [MIN,MAX].
        WindowPaletteConfig { enabled: true, cols: 99, rows: 1, ..Default::default() }.save(&db).unwrap();
        let cfg = WindowPaletteConfig::load(&db);
        assert!(cfg.enabled);
        assert_eq!(cfg.cols, MAX_CELLS); // 99 → 24
        assert_eq!(cfg.rows, MIN_CELLS); // 1 → 2
    }

    #[test]
    fn config_load_falls_back_on_unparseable_stored_value() {
        let db = mem_db();
        crate::settings::set(&db, KEY_COLS, "not-a-number").unwrap();
        let cfg = WindowPaletteConfig::load(&db);
        assert_eq!(cfg.cols, DEFAULT_COLS);
    }

    #[test]
    fn fraction_maps_an_interior_cell() {
        // A middle column band on the offset visible frame.
        let r = fraction_to_rect(0.25, 0.0, 0.25, 0.5, VF);
        assert!((r.x - 360.0).abs() < 1e-9); // 0 + 0.25*1440
        assert_eq!(r.y, 25.0);
        assert!((r.w - 360.0).abs() < 1e-9);
        assert!((r.h - 437.5).abs() < 1e-9);
    }
}
