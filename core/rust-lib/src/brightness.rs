//! Monitor brightness control (v0.62.0) — Lunar / TwinkleTray style.
//!
//! Adjusts the brightness of every DDC/CI-capable monitor, **including
//! secondary/external ones**, via VCP feature `0x10` over the `ddc-hi`
//! crate (which wraps `ddc-macos` / `ddc-winapi` / `ddc-i2c`). This is the
//! same mechanism Lunar uses for external displays and TwinkleTray uses on
//! Windows.
//!
//! **Scope (v1): external / DDC-capable monitors on macOS + Windows + Linux.**
//! Internal laptop panels (MacBook built-in, which doesn't speak DDC) need the
//! private macOS `DisplayServices` framework / Windows WMI — tracked as a
//! follow-up (see `docs/reports/WINDOWS_PARITY.md`). A monitor that doesn't
//! answer DDC is reported with `supports_ddc = false` so the UI can disable
//! its slider instead of failing.
//!
//! The brightness↔VCP percentage mapping is a pure, unit-tested helper. The
//! DDC handles themselves are cached behind a mutex (they aren't `Send` in
//! the general case — same `unsafe impl Send` pattern as the cached `Enigo`)
//! so a slider drag doesn't pay the (slow) full enumeration on every step.

use parking_lot::Mutex;
use std::sync::OnceLock;

use ddc_hi::{Ddc, Display};
use serde::Serialize;

/// VCP feature code for luminance / brightness (MCCS standard).
pub const VCP_BRIGHTNESS: u8 = 0x10;

// ── Pure mapping (unit-tested) ─────────────────────────────────────────────

/// Map a 0–100 percentage to a raw VCP value in `0..=max_raw` (rounded).
pub fn percent_to_raw(percent: u8, max_raw: u16) -> u16 {
    let p = u32::from(percent.min(100));
    ((p * u32::from(max_raw) + 50) / 100) as u16
}

/// Map a raw VCP value to a 0–100 percentage (rounded, clamped).
pub fn raw_to_percent(raw: u16, max_raw: u16) -> u8 {
    if max_raw == 0 {
        return 0;
    }
    let pct = (u32::from(raw) * 100 + u32::from(max_raw) / 2) / u32::from(max_raw);
    pct.min(100) as u8
}

// ── Monitor info ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct MonitorInfo {
    /// Index into the cached enumeration (stable within a session).
    pub id: u32,
    /// Human-readable model name, or `Display N`.
    pub name: String,
    /// Current brightness 0–100 (0 if it couldn't be read).
    pub brightness: u8,
    /// Whether the monitor answered the brightness VCP read.
    pub supports_ddc: bool,
}

// ── Cached DDC handles ─────────────────────────────────────────────────────

/// `ddc_hi::Display` holds a platform handle that isn't `Send` in the general
/// case. We only ever touch it under the mutex below (and DDC I²C/IOKit calls
/// are fine off the main thread), so this is sound for our access pattern —
/// the same justification as the cached `Enigo` (`EnigoCell`).
struct DisplayCache(Vec<Display>);
unsafe impl Send for DisplayCache {}

fn cache() -> &'static Mutex<Option<DisplayCache>> {
    static C: OnceLock<Mutex<Option<DisplayCache>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

/// Enumerate all DDC-capable monitors, reading each one's current brightness,
/// and cache the handles for subsequent get/set. **Slow** (DDC probes each
/// display) — call when the overlay opens, not per slider step.
pub fn enumerate() -> Vec<MonitorInfo> {
    let displays = Display::enumerate();
    let mut infos = Vec::with_capacity(displays.len());
    let mut keep = Vec::with_capacity(displays.len());

    for (i, mut d) in displays.into_iter().enumerate() {
        let name = d
            .info
            .model_name
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("Display {}", i + 1));
        let (brightness, supports_ddc) = match d.handle.get_vcp_feature(VCP_BRIGHTNESS) {
            Ok(v) => (raw_to_percent(v.value(), v.maximum()), true),
            Err(e) => {
                tracing::debug!("monitor {i} ({name}) brightness read failed: {e}");
                (0, false)
            }
        };
        infos.push(MonitorInfo {
            id: i as u32,
            name,
            brightness,
            supports_ddc,
        });
        keep.push(d);
    }

    *cache().lock() = Some(DisplayCache(keep));
    infos
}

/// Set monitor `id`'s brightness to `percent` (0–100). Reads the monitor's
/// max VCP value first so the percentage maps to the device's actual range.
pub fn set(id: u32, percent: u8) -> Result<(), String> {
    let mut guard = cache().lock();
    let Some(DisplayCache(displays)) = guard.as_mut() else {
        return Err("no monitors enumerated yet".into());
    };
    let Some(d) = displays.get_mut(id as usize) else {
        return Err(format!("monitor {id} out of range"));
    };
    let cur = d
        .handle
        .get_vcp_feature(VCP_BRIGHTNESS)
        .map_err(|e| format!("read brightness VCP: {e}"))?;
    let raw = percent_to_raw(percent, cur.maximum());
    d.handle
        .set_vcp_feature(VCP_BRIGHTNESS, raw)
        .map_err(|e| format!("set brightness VCP: {e}"))?;
    Ok(())
}

/// Read monitor `id`'s current brightness (0–100).
pub fn get(id: u32) -> Result<u8, String> {
    let mut guard = cache().lock();
    let Some(DisplayCache(displays)) = guard.as_mut() else {
        return Err("no monitors enumerated yet".into());
    };
    let Some(d) = displays.get_mut(id as usize) else {
        return Err(format!("monitor {id} out of range"));
    };
    let v = d
        .handle
        .get_vcp_feature(VCP_BRIGHTNESS)
        .map_err(|e| format!("read brightness VCP: {e}"))?;
    Ok(raw_to_percent(v.value(), v.maximum()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_to_raw_maps_endpoints() {
        assert_eq!(percent_to_raw(0, 100), 0);
        assert_eq!(percent_to_raw(100, 100), 100);
        assert_eq!(percent_to_raw(50, 100), 50);
    }

    #[test]
    fn percent_to_raw_scales_to_device_max() {
        // Many monitors report a max of, say, 100; some use other ranges.
        assert_eq!(percent_to_raw(50, 200), 100);
        assert_eq!(percent_to_raw(100, 255), 255);
        assert_eq!(percent_to_raw(0, 255), 0);
        // Rounding: 33% of 255 = 84.15 → 84.
        assert_eq!(percent_to_raw(33, 255), 84);
    }

    #[test]
    fn percent_to_raw_clamps_over_100() {
        assert_eq!(percent_to_raw(200, 100), 100);
    }

    #[test]
    fn raw_to_percent_maps_endpoints() {
        assert_eq!(raw_to_percent(0, 100), 0);
        assert_eq!(raw_to_percent(100, 100), 100);
        assert_eq!(raw_to_percent(50, 100), 50);
    }

    #[test]
    fn raw_to_percent_handles_zero_max() {
        assert_eq!(raw_to_percent(50, 0), 0);
    }

    #[test]
    fn raw_to_percent_scales_and_clamps() {
        assert_eq!(raw_to_percent(255, 255), 100);
        assert_eq!(raw_to_percent(128, 255), 50);
        // A raw value above max clamps to 100.
        assert_eq!(raw_to_percent(300, 255), 100);
    }

    #[test]
    fn round_trip_is_stable_for_max_100() {
        for p in [0u8, 1, 25, 50, 75, 99, 100] {
            let raw = percent_to_raw(p, 100);
            assert_eq!(raw_to_percent(raw, 100), p);
        }
    }
}
