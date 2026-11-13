//! `record` — screen/window recording into editable projects (stage 1).
#![allow(dead_code)]

pub mod assets;
#[cfg_attr(not(all(target_os = "macos", target_arch = "aarch64")), allow(dead_code))]
pub mod audio_tracks;
pub mod autocut;
// Webcam: cross-platform types (the settings carry them); its AVFoundation edge is macOS-only.
pub mod camera;
pub mod camera_preview;
#[cfg_attr(not(all(target_os = "macos", target_arch = "aarch64")), allow(dead_code))]
pub mod captions;
pub mod click_cues;
pub mod core;
pub mod denoise;
#[cfg(target_os = "macos")]
pub mod capture_macos;
#[cfg(target_os = "macos")]
pub mod cursor_macos;
// Only the Apple-Silicon editor IPC uses these three; elsewhere they are compiled but unused.
#[cfg_attr(not(all(target_os = "macos", target_arch = "aarch64")), allow(dead_code))]
pub mod editor;
#[cfg_attr(not(all(target_os = "macos", target_arch = "aarch64")), allow(dead_code))]
pub mod effects;
pub mod layout;
pub mod mic;
pub mod mic_meter;
#[cfg_attr(not(all(target_os = "macos", target_arch = "aarch64")), allow(dead_code))]
pub mod peaks;
#[cfg(target_os = "macos")]
pub mod picker_macos;
#[cfg_attr(not(all(target_os = "macos", target_arch = "aarch64")), allow(dead_code))]
pub mod platform;
pub mod presets;
pub mod session;
pub mod project;
#[cfg_attr(not(all(target_os = "macos", target_arch = "aarch64")), allow(dead_code))]
pub mod render_client;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod editor_ipc;

/// The record editor is transcribing a take right now (it shares the helper
/// with `transcribe`, the meeting and dictation). Always false where the
/// editor does not exist — callers on every platform ask through this.
pub fn editor_is_transcribing() -> bool {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        editor_ipc::is_transcribing()
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        false
    }
}
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod control_ops;
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub mod control_rec;
#[cfg_attr(not(all(target_os = "macos", target_arch = "aarch64")), allow(dead_code))]
pub mod scene_json;
#[cfg_attr(not(all(target_os = "macos", target_arch = "aarch64")), allow(dead_code))]
pub mod voiceover;
pub mod wav;
pub mod zoom_plan;

/// The record methods of the control socket (stage 8). Off Apple Silicon (no editor there)
/// they answer `record.unsupported`.
pub fn register_control() {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    control_ops::register_all();
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    for m in CONTROL_METHODS {
        crate::control::register(m, |_, _| {
            Err(crate::control::ControlError::new(session::ERR_UNSUPPORTED, "record needs a Mac with Apple Silicon"))
        });
    }
}

/// Every record control method (the list `register_control` serves).
pub const CONTROL_METHODS: [&str; 16] = [
    "record.sources",
    "record.start",
    "record.stop",
    "record.pause",
    "record.resume",
    "record.status",
    "record.projects",
    "record.project",
    "record.doc",
    "record.transcript",
    "record.edit",
    "record.export",
    "record.transcribe",
    "record.frames",
    "record.delete",
    "record.rename",
];

/// Window label of the record editor (the status toast must not `app.hide()` it away).
pub const EDITOR_WINDOW: &str = "record-editor";

// Used by the macOS cursor sampler; unused on other targets.
#[allow(unused_imports)]
pub use core::PauseShared;
#[allow(unused_imports)]
pub use session::{record_supported, RecordConfig, RecordStatus, StartOptions};

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
