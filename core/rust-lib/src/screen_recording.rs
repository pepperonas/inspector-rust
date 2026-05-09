//! macOS Screen Recording (TCC `kTCCServiceScreenCapture`) permission
//! checks.
//!
//! Why this exists separately from the Accessibility check in
//! `expander.rs`: macOS treats the two TCC policies as independent
//! grants. A user can grant Accessibility (so paste works) but never
//! Screen Recording — and the OCR feature (which spawns
//! `screencapture -i`) then fails silently because the OS attributes
//! the capture to the spawning app and denies it.
//!
//! Without surfacing this, the user sees:
//!   - press `⌘⇧O`
//!   - marquee never appears
//!   - nothing happens
//!   - no error message anywhere
//!
//! With this module the IPC layer can pre-check before invoking
//! `screencapture`, return a structured error, and the UI can offer
//! a one-click path to the right Privacy pane.

use anyhow::Result;

/// Whether **Screen Recording** is currently granted. macOS-only;
/// other platforms always return `true` because there's no equivalent
/// gate (Windows / X11 don't restrict screen capture by app).
#[cfg(target_os = "macos")]
pub fn screen_recording_granted() -> bool {
    unsafe { macos_cg::CGPreflightScreenCaptureAccess() }
}

#[cfg(not(target_os = "macos"))]
pub fn screen_recording_granted() -> bool {
    true
}

/// Trigger the macOS Screen Recording prompt. Returns the *current*
/// status (almost always `false` immediately — the user still has to
/// flip the toggle in System Settings, then ClipSnap usually has to
/// be re-launched for the cached TCC verdict to refresh).
#[cfg(target_os = "macos")]
pub fn request_screen_recording_grant() -> bool {
    unsafe { macos_cg::CGRequestScreenCaptureAccess() }
}

#[cfg(not(target_os = "macos"))]
pub fn request_screen_recording_grant() -> bool {
    true
}

/// Open **System Settings → Privacy & Security → Screen Recording**.
/// No-op on other OSes.
#[cfg(target_os = "macos")]
pub fn open_screen_recording_settings() -> Result<()> {
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        .spawn()
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("open System Settings: {e}"))
}

#[cfg(not(target_os = "macos"))]
pub fn open_screen_recording_settings() -> Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
mod macos_cg {
    // Both CGPreflight* and CGRequest* live in the CoreGraphics
    // framework, which is part of the ApplicationServices umbrella
    // we already link via `expander::macos_ax`. Linking it again here
    // is a no-op for the linker.
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        pub fn CGPreflightScreenCaptureAccess() -> bool;
        pub fn CGRequestScreenCaptureAccess() -> bool;
    }
}
