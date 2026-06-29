//! EDR (Extended Dynamic Range) brightness extension for the `brightness`
//! command — macOS-only, XDR/EDR-capable displays.
//!
//! macOS clamps SDR content to ~SDR-max. EDR-capable panels (14"/16" MBP XDR,
//! Pro Display XDR) can go to ~1000 nits sustained, but only while EDR content
//! (a `CAMetalLayer` with `wantsExtendedDynamicRangeContent` showing pixels above
//! 1.0) is on screen — then macOS releases the backlight headroom. This module
//! extends the existing brightness slider above its SDR max on such displays;
//! everywhere else it's inert (the slider behaves exactly as before).
//!
//! **Phase 1 (this): runtime EDR-capability detection** (per display, via
//! `NSScreen.maximumPotentialExtendedDynamicRangeColorComponentValue`) → the
//! slider's upper bound. **Phase 2 (next): the click-through CAMetalLayer
//! overlay + render loop** that actually lifts luminance into the EDR range.
//! All native code is `#[cfg(target_os = "macos")]`; other platforms get the
//! pure helpers (which always report "not capable") and nothing else.

#![allow(dead_code)] // the overlay (phase 2) consumes more of this

/// A display is treated as EDR-capable when its potential headroom exceeds this
/// (1.0 = no headroom). A tiny margin avoids float-noise false positives.
const EDR_MIN_HEADROOM: f32 = 1.05;
/// Cap the slider's EDR upper bound (%) regardless of the raw headroom — the
/// *usable* sustained boost is far below the raw color-component headroom (which
/// can read ~16 on XDR). Tunable once verified on hardware.
const EDR_MAX_CAP: u16 = 300;

/// Map a display's potential EDR headroom to the slider's upper bound in percent
/// (100 = SDR only). Pure + unit-tested. `> EDR_MIN_HEADROOM` ⇒ the slider runs
/// past 100 up to `min(headroom·100, EDR_MAX_CAP)`; otherwise it ends at 100.
pub fn edr_max_percent(headroom: f32) -> u16 {
    if headroom > EDR_MIN_HEADROOM {
        ((headroom * 100.0).round() as u16).clamp(110, EDR_MAX_CAP)
    } else {
        100
    }
}

/// The potential EDR headroom of the display with CoreGraphics id `cg_id`
/// (1.0 = none / not EDR-capable). Reads
/// `NSScreen.maximumPotentialExtendedDynamicRangeColorComponentValue` for the
/// `NSScreen` whose `NSScreenNumber` matches.
#[cfg(target_os = "macos")]
unsafe fn nsstring(s: &str) -> *mut objc2::runtime::AnyObject {
    use objc2::msg_send;
    use objc2::runtime::AnyClass;
    let Some(cls) = AnyClass::get(c"NSString") else {
        return std::ptr::null_mut();
    };
    let Ok(cstr) = std::ffi::CString::new(s) else {
        return std::ptr::null_mut();
    };
    msg_send![cls, stringWithUTF8String: cstr.as_ptr()]
}

#[cfg(target_os = "macos")]
pub fn display_headroom(cg_id: u32) -> f32 {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    unsafe {
        let Some(ns_screen) = AnyClass::get(c"NSScreen") else {
            return 1.0;
        };
        let screens: *mut AnyObject = msg_send![ns_screen, screens];
        if screens.is_null() {
            return 1.0;
        }
        let count: usize = msg_send![screens, count];
        let key = nsstring("NSScreenNumber");
        if key.is_null() {
            return 1.0;
        }
        for i in 0..count {
            let s: *mut AnyObject = msg_send![screens, objectAtIndex: i];
            if s.is_null() {
                continue;
            }
            let desc: *mut AnyObject = msg_send![s, deviceDescription];
            if desc.is_null() {
                continue;
            }
            let num: *mut AnyObject = msg_send![desc, objectForKey: key];
            if num.is_null() {
                continue;
            }
            let screen_no: u32 = msg_send![num, unsignedIntValue];
            if screen_no == cg_id {
                let v: f64 = msg_send![s, maximumPotentialExtendedDynamicRangeColorComponentValue];
                return v as f32;
            }
        }
        1.0
    }
}

#[cfg(not(target_os = "macos"))]
pub fn display_headroom(_cg_id: u32) -> f32 {
    1.0
}

/// Drive the EDR boost for `cg_id` to `percent` (the full slider value; ≤ 100 or
/// 0 ⇒ off). **Phase 1 stub** — records intent; the CAMetalLayer overlay lands
/// in phase 2. Logged so the wiring can be verified end-to-end.
#[cfg(target_os = "macos")]
pub fn set_level(cg_id: u32, percent: u16) {
    tracing::info!("edr: set_level display={cg_id} percent={percent} (overlay = phase 2, not yet active)");
}

#[cfg(not(target_os = "macos"))]
pub fn set_level(_cg_id: u32, _percent: u16) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edr_max_percent_gates_on_headroom() {
        assert_eq!(edr_max_percent(1.0), 100, "no headroom → SDR only");
        assert_eq!(edr_max_percent(1.04), 100, "below margin → SDR only");
        assert_eq!(edr_max_percent(1.6), 160, "1.6× → 160 %");
        assert_eq!(edr_max_percent(16.0), EDR_MAX_CAP, "huge headroom is capped");
        // Just-capable displays still get a usable range above 100.
        assert!(edr_max_percent(1.06) >= 110);
    }
}
