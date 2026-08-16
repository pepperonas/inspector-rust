//! Custom screen **loupe** for the eyedropper. Apple's `NSColorSampler` shows a
//! magnified loupe but won't let us draw the hex under it, so this provides a
//! one-shot **snapshot of the cursor's display** that an overlay window
//! magnifies in the webview — smooth (no per-frame screen capture) and able to
//! render the live hex beneath the loupe. Reuses the Screen-Recording grant the
//! app already needs for OCR / screenshots.

use anyhow::{Context, Result};
use base64::Engine;
use parking_lot::Mutex;

/// The active loupe snapshot + which caller opened it. Managed Tauri state.
#[derive(Default)]
pub struct LoupeState(pub Mutex<Option<Session>>);

pub struct Session {
    /// Base64 PNG of the cursor's display, magnified by the overlay webview.
    pub b64: String,
    /// `true` = modal "pick from screen" (emits `color-picked`); `false` =
    /// hotkey/tray eyedropper (writes hex to clipboard + history).
    pub event_mode: bool,
}

/// Capture display `display_index` (1-based, `screencapture -D`) as base64 PNG.
/// Non-macOS falls back to a fullscreen capture (the index is ignored there).
pub fn capture_display_b64(display_index: usize) -> Result<String> {
    let png = capture_display(display_index)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(png))
}

#[cfg(target_os = "macos")]
fn capture_display(display_index: usize) -> Result<Vec<u8>> {
    use chrono::Utc;
    use std::process::Command;
    let idx = display_index.max(1);
    let tmp = std::env::temp_dir().join(format!(
        "inspector-rust-loupe-{}.png",
        Utc::now().timestamp_millis()
    ));
    // -x silent, -D <n> 1-based display index (the cursor's), -t png.
    let status = Command::new("/usr/sbin/screencapture")
        .args(["-x", "-D", &idx.to_string(), "-t", "png"])
        .arg(&tmp)
        .status()
        .context("spawn /usr/sbin/screencapture")?;
    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("screencapture exited with status {:?}", status.code());
    }
    // Remove the temp UNCONDITIONALLY — `?`-ing the read first left a
    // full-display PNG (multi-MB) in the temp dir on every failed read.
    let bytes = std::fs::read(&tmp).context("read loupe png");
    let _ = std::fs::remove_file(&tmp);
    bytes
}

#[cfg(not(target_os = "macos"))]
fn capture_display(_display_index: usize) -> Result<Vec<u8>> {
    crate::region_picker::capture_fullscreen()
}
