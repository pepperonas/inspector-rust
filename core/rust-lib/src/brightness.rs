//! Monitor brightness control — Lunar / TwinkleTray style.
//!
//! Two mechanisms, chosen per platform:
//!
//! - **macOS → software (gamma-table) dimming (v0.72.0).** `CGSetDisplayTransferByFormula`
//!   scales each display's output transfer function, dimming it in software.
//!   This is the same "Software Dimming" Lunar / MonitorControl fall back to,
//!   and crucially it works on **every** display — the built-in Liquid Retina
//!   panel *and* external monitors connected through adapters/docks — with no
//!   DDC, no extra TCC permission, and no Apple-Silicon I²C quirks. We moved
//!   to this after confirming on real hardware that the DDC/CI path (`ddc-hi`
//!   / IOAVService) returns `invalid DDC/CI length` for an external monitor on
//!   an M-series Mac: neither reads nor (silent-failing) writes reach the
//!   panel through a DP/HDMI adapter. Software dimming reduces emitted light
//!   rather than backlight, so it can only go *darker* than native (capped at
//!   100%); a safety floor (`MIN_PERCENT`) keeps the screen from going fully
//!   black and unrecoverable. The applied level is tracked in-process (gamma
//!   has no "read current backlight"); it resets to 100% at logout.
//!
//! - **Windows → software (gamma-ramp) dimming (v0.72.0).** `SetDeviceGammaRamp`
//!   scales each monitor's GDI gamma ramp — the same universal software-dimming
//!   mechanism as the macOS path, mirrored for Windows 11. Covers the built-in
//!   laptop panel *and* external/adapter-connected monitors uniformly, no DDC
//!   round-trip needed. (**Windows runtime-unverified** — written compile-clean
//!   against the `windows` 0.61 GDI bindings, mirroring `screen_picker.rs`'s
//!   proven Win32 patterns; not yet exercised on a real Windows box.)
//!
//! - **Linux → DDC/CI VCP feature `0x10`** over the `ddc-hi` crate (wraps
//!   `ddc-i2c`), the real hardware-backlight path for external monitors.
//!
//! The percent↔value mappings are pure, unit-tested helpers.

use serde::Serialize;

/// VCP feature code for luminance / brightness (MCCS standard; DDC path).
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub const VCP_BRIGHTNESS: u8 = 0x10;

// ── Pure mappings (unit-tested) ─────────────────────────────────────────────

/// Map a 0–100 percentage to a raw VCP value in `0..=max_raw` (rounded).
/// Used by the DDC path (Linux); also exercised by tests on every platform.
#[cfg(any(not(any(target_os = "macos", target_os = "windows")), test))]
pub fn percent_to_raw(percent: u8, max_raw: u16) -> u16 {
    let p = u32::from(percent.min(100));
    ((p * u32::from(max_raw) + 50) / 100) as u16
}

/// Map a raw VCP value to a 0–100 percentage (rounded, clamped).
#[cfg(any(not(any(target_os = "macos", target_os = "windows")), test))]
pub fn raw_to_percent(raw: u16, max_raw: u16) -> u8 {
    if max_raw == 0 {
        return 0;
    }
    let pct = (u32::from(raw) * 100 + u32::from(max_raw) / 2) / u32::from(max_raw);
    pct.min(100) as u8
}

/// Map a 0–100 percentage to a gamma-table output scale in `0.0..=1.0`,
/// clamped to `[min_percent, 100]` so the screen never dims to fully black
/// (an unrecoverable state). Used by the macOS software-dimming path.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub fn percent_to_gamma_fraction(percent: u8, min_percent: u8) -> f32 {
    let floor = min_percent.min(100);
    let p = percent.clamp(floor, 100);
    f32::from(p) / 100.0
}

/// One entry of a Windows GDI gamma ramp (`SetDeviceGammaRamp`): maps an
/// 8-bit input `index` (0..=255) to a 16-bit output, scaled by `fraction`
/// (`0.0..=1.0`). `fraction = 1.0` is the identity ramp (`index * 257`, so
/// 255 → 65535); a smaller fraction dims linearly. Pure + unit-tested.
#[cfg(any(target_os = "windows", test))]
pub fn gamma_ramp_entry(index: u16, fraction: f32) -> u16 {
    let f = fraction.clamp(0.0, 1.0);
    let v = (f32::from(index.min(255)) * 257.0 * f).round();
    v.clamp(0.0, 65535.0) as u16
}

// ── Monitor info ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct MonitorInfo {
    /// Index into the cached enumeration (stable within a session).
    pub id: u32,
    /// Human-readable name (e.g. `Built-in Display`, `External Display 1`).
    pub name: String,
    /// Current brightness 0–100.
    pub brightness: u8,
    /// Whether the monitor's brightness is controllable (slider enabled).
    pub supports_ddc: bool,
    /// The slider's upper bound in percent. **100** on Windows/Linux + non-EDR
    /// macOS displays (SDR only); **> 100** on macOS EDR-capable displays (the
    /// extra range drives the EDR overlay — see `edr.rs`).
    pub edr_max: u16,
}

// ── Platform dispatch ────────────────────────────────────────────────────────

/// Enumerate controllable monitors + their current brightness. Slow on the
/// DDC path (probes each display); cheap on the macOS gamma path. Call when
/// the overlay opens.
pub fn enumerate() -> Vec<MonitorInfo> {
    #[cfg(target_os = "macos")]
    {
        macos_gamma::enumerate()
    }
    #[cfg(target_os = "windows")]
    {
        win_gamma::enumerate()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        ddc::enumerate()
    }
}

/// Set monitor `id`'s brightness to `percent` (0–100).
pub fn set(id: u32, percent: u8) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos_gamma::set(id, percent)
    }
    #[cfg(target_os = "windows")]
    {
        win_gamma::set(id, percent)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        ddc::set(id, percent)
    }
}

/// Read monitor `id`'s current brightness (0–100).
pub fn get(id: u32) -> Result<u8, String> {
    #[cfg(target_os = "macos")]
    {
        macos_gamma::get(id)
    }
    #[cfg(target_os = "windows")]
    {
        win_gamma::get(id)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        ddc::get(id)
    }
}

/// Drive the EDR boost for monitor `id` to `percent` (the full slider value;
/// ≤ 100 / 0 = off). macOS-only; a no-op everywhere else.
pub fn set_edr_level(app: &tauri::AppHandle, id: u32, percent: u16) {
    #[cfg(target_os = "macos")]
    {
        macos_gamma::set_edr_level(app, id, percent);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, id, percent);
    }
}

// ── macOS: software (gamma) dimming ──────────────────────────────────────────

#[cfg(target_os = "macos")]
mod macos_gamma {
    //! Raw FFI to CoreGraphics gamma-table APIs. Same `#[link(... kind =
    //! "framework")]` + `*const c_void` raw-FFI pattern as `input_lock.rs`.

    use super::{percent_to_gamma_fraction, MonitorInfo};
    use parking_lot::Mutex;
    use std::os::raw::c_int;
    use std::sync::OnceLock;

    type CGDirectDisplayID = u32;
    type CGError = c_int;
    type BooleanT = c_int;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGGetActiveDisplayList(
            max_displays: u32,
            active: *mut CGDirectDisplayID,
            display_count: *mut u32,
        ) -> CGError;
        fn CGDisplayIsBuiltin(display: CGDirectDisplayID) -> BooleanT;
        #[allow(clippy::too_many_arguments)]
        fn CGSetDisplayTransferByFormula(
            display: CGDirectDisplayID,
            red_min: f32,
            red_max: f32,
            red_gamma: f32,
            green_min: f32,
            green_max: f32,
            green_gamma: f32,
            blue_min: f32,
            blue_max: f32,
            blue_gamma: f32,
        ) -> CGError;
        fn CGDisplayRestoreColorSyncSettings();
    }

    /// Floor so the screen never dims to fully black (unrecoverable — the
    /// user couldn't see the slider to bring it back up).
    const MIN_PERCENT: u8 = 5;

    struct Entry {
        cg_id: CGDirectDisplayID,
        /// Requested percent (what the slider shows); applied value is floored.
        percent: u8,
    }

    fn cache() -> &'static Mutex<Vec<Entry>> {
        static C: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
        C.get_or_init(|| Mutex::new(Vec::new()))
    }

    fn apply(cg_id: CGDirectDisplayID, percent: u8) -> Result<(), String> {
        let f = percent_to_gamma_fraction(percent, MIN_PERCENT);
        // min=0, max=f, gamma=1 → linear output scaled to `f` (a dim).
        let err = unsafe {
            CGSetDisplayTransferByFormula(cg_id, 0.0, f, 1.0, 0.0, f, 1.0, 0.0, f, 1.0)
        };
        if err != 0 {
            return Err(format!("CGSetDisplayTransferByFormula failed: {err}"));
        }
        Ok(())
    }

    pub fn enumerate() -> Vec<MonitorInfo> {
        let mut ids = [0u32; 16];
        let mut count = 0u32;
        let e = unsafe { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &mut count) };
        if e != 0 {
            return Vec::new();
        }
        let ids = &ids[..count as usize];

        let mut guard = cache().lock();
        // Preserve previously-applied levels across re-enumeration so re-opening
        // the overlay shows the current dim level, not a reset 100%.
        let prev: Vec<(CGDirectDisplayID, u8)> =
            guard.iter().map(|en| (en.cg_id, en.percent)).collect();

        let external_total = ids
            .iter()
            .filter(|&&id| unsafe { CGDisplayIsBuiltin(id) } == 0)
            .count();

        let mut entries = Vec::with_capacity(ids.len());
        let mut infos = Vec::with_capacity(ids.len());
        let mut ext_n = 0;
        for (i, &id) in ids.iter().enumerate() {
            let builtin = unsafe { CGDisplayIsBuiltin(id) } != 0;
            let name = if builtin {
                "Built-in Display".to_string()
            } else {
                ext_n += 1;
                if external_total > 1 {
                    format!("External Display {ext_n}")
                } else {
                    "External Display".to_string()
                }
            };
            let percent = prev
                .iter()
                .find(|(pid, _)| *pid == id)
                .map(|(_, p)| *p)
                .unwrap_or(100);
            let headroom = crate::edr::display_headroom(id);
            let edr_max = crate::edr::edr_max_percent(headroom);
            if edr_max > 100 {
                tracing::info!("edr: display {id} EDR-capable (headroom {headroom:.2} → slider max {edr_max}%)");
            }
            infos.push(MonitorInfo {
                id: i as u32,
                name,
                brightness: percent,
                supports_ddc: true,
                edr_max,
            });
            entries.push(Entry { cg_id: id, percent });
        }
        *guard = entries;
        infos
    }

    pub fn set(idx: u32, percent: u8) -> Result<(), String> {
        let mut guard = cache().lock();
        let Some(en) = guard.get_mut(idx as usize) else {
            return Err(format!("monitor {idx} out of range"));
        };
        let p = percent.clamp(MIN_PERCENT, 100);
        apply(en.cg_id, p)?;
        en.percent = p;
        Ok(())
    }

    pub fn get(idx: u32) -> Result<u8, String> {
        let guard = cache().lock();
        guard
            .get(idx as usize)
            .map(|en| en.percent)
            .ok_or_else(|| format!("monitor {idx} out of range"))
    }

    /// Resolve the monitor index to its CoreGraphics id + hand the EDR boost to
    /// the EDR module (the overlay).
    pub fn set_edr_level(app: &tauri::AppHandle, idx: u32, percent: u16) {
        let cg = cache().lock().get(idx as usize).map(|en| en.cg_id);
        if let Some(cg_id) = cg {
            crate::edr::set_level(app, cg_id, percent);
        }
    }

    /// Restore every display's gamma to the system default. Wired to app
    /// teardown so dimming doesn't linger after quit; harmless to call
    /// when nothing was dimmed.
    #[allow(dead_code)]
    pub fn restore_all() {
        unsafe { CGDisplayRestoreColorSyncSettings() };
    }
}

// ── Windows: software (gamma-ramp) dimming ───────────────────────────────────

#[cfg(target_os = "windows")]
mod win_gamma {
    //! Per-monitor software dimming via the GDI `SetDeviceGammaRamp` API,
    //! mirroring the macOS gamma path. Uses the `windows` 0.61 bindings, the
    //! same crate + GDI module `screen_picker.rs` already relies on.
    //!
    //! **Runtime-unverified on real Windows hardware.**

    use super::{gamma_ramp_entry, percent_to_gamma_fraction, MonitorInfo};
    use parking_lot::Mutex;
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::sync::OnceLock;
    use windows::core::{BOOL, PCWSTR};
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        CreateDCW, DeleteDC, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
        MONITORINFOEXW,
    };
    use windows::Win32::UI::ColorSystem::SetDeviceGammaRamp;
    use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

    /// Safety floor so the screen never dims to fully black (unrecoverable).
    const MIN_PERCENT: u8 = 5;

    struct Entry {
        /// `\\.\DISPLAY1`-style adapter name, null-terminated wide string.
        device: Vec<u16>,
        percent: u8,
    }

    fn cache() -> &'static Mutex<Vec<Entry>> {
        static C: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();
        C.get_or_init(|| Mutex::new(Vec::new()))
    }

    /// Build a full 256×3 (R,G,B) gamma ramp scaled to `fraction`.
    fn build_ramp(fraction: f32) -> [u16; 256 * 3] {
        let mut ramp = [0u16; 256 * 3];
        for i in 0..256u16 {
            let v = gamma_ramp_entry(i, fraction);
            let idx = i as usize;
            ramp[idx] = v; // red
            ramp[256 + idx] = v; // green
            ramp[512 + idx] = v; // blue
        }
        ramp
    }

    fn apply(device: &[u16], percent: u8) -> Result<(), String> {
        let f = percent_to_gamma_fraction(percent, MIN_PERCENT);
        let ramp = build_ramp(f);
        unsafe {
            // Create a device context for this specific display adapter.
            let hdc = CreateDCW(
                PCWSTR(device.as_ptr()),
                PCWSTR(device.as_ptr()),
                PCWSTR::null(),
                None,
            );
            if hdc.0.is_null() {
                return Err("CreateDCW failed".into());
            }
            let ok = SetDeviceGammaRamp(hdc, ramp.as_ptr() as *const c_void).as_bool();
            let _ = DeleteDC(hdc);
            if !ok {
                return Err("SetDeviceGammaRamp failed (driver may reject the ramp)".into());
            }
        }
        Ok(())
    }

    unsafe extern "system" fn enum_proc(
        hmon: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let out = &mut *(lparam.0 as *mut Vec<(Vec<u16>, bool)>);
        let mut mi = MONITORINFOEXW::default();
        mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(hmon, &mut mi.monitorInfo as *mut MONITORINFO).as_bool() {
            let device: Vec<u16> = mi
                .szDevice
                .iter()
                .copied()
                .take_while(|&c| c != 0)
                .chain(std::iter::once(0))
                .collect();
            let is_primary = (mi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0;
            out.push((device, is_primary));
        }
        BOOL(1)
    }

    fn enumerate_monitors() -> Vec<(Vec<u16>, bool)> {
        let mut out: Vec<(Vec<u16>, bool)> = Vec::new();
        unsafe {
            let _ = EnumDisplayMonitors(
                None,
                None,
                Some(enum_proc),
                LPARAM(&mut out as *mut _ as isize),
            );
        }
        out
    }

    pub fn enumerate() -> Vec<MonitorInfo> {
        let monitors = enumerate_monitors();

        let mut guard = cache().lock();
        let prev: Vec<(Vec<u16>, u8)> =
            guard.iter().map(|en| (en.device.clone(), en.percent)).collect();

        let mut entries = Vec::with_capacity(monitors.len());
        let mut infos = Vec::with_capacity(monitors.len());
        let mut n = 0;
        for (device, is_primary) in monitors {
            n += 1;
            let name = if is_primary {
                format!("Display {n} (Primary)")
            } else {
                format!("Display {n}")
            };
            let percent = prev
                .iter()
                .find(|(d, _)| *d == device)
                .map(|(_, p)| *p)
                .unwrap_or(100);
            infos.push(MonitorInfo {
                id: (n - 1) as u32,
                name,
                brightness: percent,
                supports_ddc: true,
                edr_max: 100, // EDR is macOS-only
            });
            entries.push(Entry { device, percent });
        }
        *guard = entries;
        infos
    }

    pub fn set(idx: u32, percent: u8) -> Result<(), String> {
        let mut guard = cache().lock();
        let Some(en) = guard.get_mut(idx as usize) else {
            return Err(format!("monitor {idx} out of range"));
        };
        let p = percent.clamp(MIN_PERCENT, 100);
        let device = en.device.clone();
        apply(&device, p)?;
        en.percent = p;
        Ok(())
    }

    pub fn get(idx: u32) -> Result<u8, String> {
        let guard = cache().lock();
        guard
            .get(idx as usize)
            .map(|en| en.percent)
            .ok_or_else(|| format!("monitor {idx} out of range"))
    }
}

// ── Linux: DDC/CI hardware brightness ────────────────────────────────────────

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod ddc {
    use super::{percent_to_raw, raw_to_percent, MonitorInfo, VCP_BRIGHTNESS};
    use ddc_hi::{Ddc, Display};
    use parking_lot::Mutex;
    use std::sync::OnceLock;

    /// `ddc_hi::Display` holds a platform handle that isn't `Send` in the
    /// general case. We only ever touch it under the mutex below, so this is
    /// sound for our access pattern — same justification as the cached `Enigo`.
    struct DisplayCache(Vec<Display>);
    unsafe impl Send for DisplayCache {}

    fn cache() -> &'static Mutex<Option<DisplayCache>> {
        static C: OnceLock<Mutex<Option<DisplayCache>>> = OnceLock::new();
        C.get_or_init(|| Mutex::new(None))
    }

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
                edr_max: 100, // EDR is macOS-only
            });
            keep.push(d);
        }

        *cache().lock() = Some(DisplayCache(keep));
        infos
    }

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
        assert_eq!(raw_to_percent(300, 255), 100);
    }

    #[test]
    fn round_trip_is_stable_for_max_100() {
        for p in [0u8, 1, 25, 50, 75, 99, 100] {
            let raw = percent_to_raw(p, 100);
            assert_eq!(raw_to_percent(raw, 100), p);
        }
    }

    #[test]
    fn percent_to_raw_is_monotonic_non_decreasing() {
        for max in [100u16, 255, 1000] {
            let mut prev = 0u16;
            for p in 0..=100u8 {
                let raw = percent_to_raw(p, max);
                assert!(raw >= prev, "p={p} max={max}: {raw} < {prev}");
                assert!(raw <= max, "p={p} raw {raw} exceeds max {max}");
                prev = raw;
            }
        }
    }

    #[test]
    fn raw_to_percent_is_monotonic_non_decreasing() {
        let max = 255u16;
        let mut prev = 0u8;
        for raw in 0..=max {
            let pct = raw_to_percent(raw, max);
            assert!(pct >= prev, "raw={raw}: {pct} < {prev}");
            assert!(pct <= 100);
            prev = pct;
        }
    }

    #[test]
    fn round_trip_is_within_one_percent_for_max_255() {
        for p in 0..=100u8 {
            let back = raw_to_percent(percent_to_raw(p, 255), 255);
            assert!(
                (i16::from(back) - i16::from(p)).abs() <= 1,
                "p={p} round-tripped to {back}"
            );
        }
    }

    #[test]
    fn handles_max_one_endpoints() {
        assert_eq!(percent_to_raw(0, 1), 0);
        assert_eq!(percent_to_raw(100, 1), 1);
        assert_eq!(percent_to_raw(49, 1), 0);
        assert_eq!(percent_to_raw(50, 1), 1);
    }

    // ── gamma fraction (macOS software dimming) ──────────────────────────────

    #[test]
    fn gamma_fraction_maps_full_and_mid() {
        assert_eq!(percent_to_gamma_fraction(100, 10), 1.0);
        assert_eq!(percent_to_gamma_fraction(50, 10), 0.5);
        assert_eq!(percent_to_gamma_fraction(10, 10), 0.10);
    }

    #[test]
    fn gamma_fraction_floors_at_min_percent() {
        // Below the floor clamps up — never fully black.
        assert_eq!(percent_to_gamma_fraction(0, 10), 0.10);
        assert_eq!(percent_to_gamma_fraction(5, 10), 0.10);
        assert!(percent_to_gamma_fraction(0, 10) > 0.0);
    }

    #[test]
    fn gamma_fraction_allows_five_percent_floor() {
        // The live floor (MIN_PERCENT = 5) lets the user dim to 5%.
        assert_eq!(percent_to_gamma_fraction(5, 5), 0.05);
        assert_eq!(percent_to_gamma_fraction(0, 5), 0.05); // clamps up to 5%
        assert_eq!(percent_to_gamma_fraction(50, 5), 0.50);
    }

    #[test]
    fn gamma_fraction_clamps_over_100() {
        assert_eq!(percent_to_gamma_fraction(200, 10), 1.0);
    }

    #[test]
    fn gamma_fraction_is_monotonic_non_decreasing() {
        let mut prev = 0.0f32;
        for p in 0..=100u8 {
            let f = percent_to_gamma_fraction(p, 10);
            assert!(f >= prev, "p={p}: {f} < {prev}");
            assert!((0.0..=1.0).contains(&f));
            prev = f;
        }
    }

    #[test]
    fn gamma_fraction_handles_zero_floor() {
        // A zero floor is allowed (caller's choice) — 0% maps to fully black.
        assert_eq!(percent_to_gamma_fraction(0, 0), 0.0);
        assert_eq!(percent_to_gamma_fraction(100, 0), 1.0);
    }

    // ── Windows gamma ramp entries ───────────────────────────────────────────

    #[test]
    fn gamma_ramp_identity_endpoints() {
        // fraction 1.0 → identity ramp: index*257, so 0→0 and 255→65535.
        assert_eq!(gamma_ramp_entry(0, 1.0), 0);
        assert_eq!(gamma_ramp_entry(255, 1.0), 65535);
        assert_eq!(gamma_ramp_entry(128, 1.0), 128 * 257);
    }

    #[test]
    fn gamma_ramp_dims_linearly() {
        // Half brightness halves every output.
        assert_eq!(gamma_ramp_entry(255, 0.5), (65535.0f32 * 0.5).round() as u16);
        assert_eq!(gamma_ramp_entry(0, 0.5), 0);
    }

    #[test]
    fn gamma_ramp_clamps_fraction() {
        // Out-of-range fractions clamp into [0,1]; never overflows u16.
        assert_eq!(gamma_ramp_entry(255, 2.0), 65535);
        assert_eq!(gamma_ramp_entry(255, -1.0), 0);
        assert_eq!(gamma_ramp_entry(300, 1.0), 65535); // index clamps to 255
    }

    #[test]
    fn gamma_ramp_is_monotonic_for_fixed_fraction() {
        let mut prev = 0u16;
        for i in 0..=255u16 {
            let v = gamma_ramp_entry(i, 0.7);
            assert!(v >= prev, "index {i}: {v} < {prev}");
            prev = v;
        }
    }
}
