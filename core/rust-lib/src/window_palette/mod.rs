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
const KEY_COLS: &str = "windowpalette.cols";
const KEY_ROWS: &str = "windowpalette.rows";

pub const DEFAULT_COLS: u32 = 16;
pub const DEFAULT_ROWS: u32 = 10;
const MIN_CELLS: u32 = 2;
const MAX_CELLS: u32 = 24;

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct WindowPaletteConfig {
    /// Off by default (opt-in).
    pub enabled: bool,
    /// Hex-grid horizontal cell count.
    pub cols: u32,
    /// Hex-grid vertical cell count.
    pub rows: u32,
}

impl Default for WindowPaletteConfig {
    fn default() -> Self {
        WindowPaletteConfig { enabled: false, cols: DEFAULT_COLS, rows: DEFAULT_ROWS }
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
        Ok(())
    }
}

fn clamp_cells(n: u32) -> u32 {
    n.clamp(MIN_CELLS, MAX_CELLS)
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
        WindowPaletteConfig { enabled: true, cols: 99, rows: 1 }.save(&db).unwrap();
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
