//! `record` — screen/window recording into editable projects (stage 1).
#![allow(dead_code)]

pub mod core;
#[cfg(target_os = "macos")]
pub mod capture_macos;
#[cfg(target_os = "macos")]
pub mod cursor_macos;
pub mod mic;
pub mod project;
pub mod wav;

// Used by the macOS cursor sampler; unused on other targets.
#[allow(unused_imports)]
pub use core::PauseShared;

/// Monotonic host time in nanoseconds. On macOS this is the same clock as the
/// CMSampleBuffer timestamps of the host clock (mach_absolute_time x timebase).
#[cfg(target_os = "macos")]
// libc marks the mach time API deprecated in favour of `mach2`; the calls are
// still the right (and only) ones here.
#[allow(deprecated)]
pub fn host_now_ns() -> i64 {
    use std::sync::OnceLock;
    static TB: OnceLock<(u32, u32)> = OnceLock::new();
    let (numer, denom) = *TB.get_or_init(|| {
        let mut info = libc::mach_timebase_info { numer: 0, denom: 0 };
        // SAFETY: `info` is a valid out-pointer for the duration of the call.
        unsafe { libc::mach_timebase_info(&mut info) };
        if info.denom == 0 {
            (1, 1)
        } else {
            (info.numer, info.denom)
        }
    });
    // SAFETY: mach_absolute_time has no preconditions.
    let ticks = unsafe { libc::mach_absolute_time() } as u128;
    (ticks * numer as u128 / denom as u128) as i64
}

/// Monotonic nanoseconds since first use (non-macOS fallback).
#[cfg(not(target_os = "macos"))]
pub fn host_now_ns() -> i64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_nanos() as i64
}
