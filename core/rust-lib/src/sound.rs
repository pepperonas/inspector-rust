//! Tiny one-shot sound effects (v0.84.45).
//!
//! Currently just the **paste/expand click** — a mechanical-keyboard clack
//! played when a snippet is expanded via the abbreviation hotkey (`Alt+1` &
//! co.), so the expansion has tactile feedback even though it happens in
//! another app with no popup visible.
//!
//! The WAV is embedded (`include_bytes!`) and written once to the cache dir;
//! playback shells out to the per-OS CLI player (macOS `afplay`, Windows
//! PowerShell `SoundPlayer`, Linux `paplay`/`aplay`) on a **worker thread**,
//! fire-and-forget — it must never block the expansion path (which runs on the
//! main thread on macOS). WAV (not MP3) is used deliberately: no decoder
//! spin-up, so the click is instant and low-latency.

use std::path::PathBuf;
use std::sync::OnceLock;

/// The embedded expand/paste click (mono 16-bit PCM WAV, ~35 KB).
const PASTE_WAV: &[u8] = include_bytes!("../assets/paste_mechkey.wav");

/// Cache path the WAV is materialised to (so the CLI players can read a file).
fn cache_path() -> Option<PathBuf> {
    let dir = dirs::cache_dir()?.join("InspectorRust");
    Some(dir.join("paste_mechkey.wav"))
}

/// Ensure the embedded WAV exists on disk; return its path. Written once.
fn ensure_file() -> Option<PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| {
        let path = cache_path()?;
        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&path, PASTE_WAV).is_err() {
                return None;
            }
        }
        Some(path)
    })
    .clone()
}

/// Play the expand/paste click. Non-blocking: spawns a worker that
/// materialises the file (once) and shells out to the OS player. Any failure
/// is silently ignored — feedback sound is best-effort.
pub fn play_paste() {
    std::thread::spawn(|| {
        let Some(path) = ensure_file() else { return };
        play_file(&path);
    });
}

#[cfg(target_os = "macos")]
fn play_file(path: &std::path::Path) {
    let _ = std::process::Command::new("/usr/bin/afplay")
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(target_os = "windows")]
fn play_file(path: &std::path::Path) {
    use std::os::windows::process::CommandExt;
    // PowerShell's SoundPlayer plays a WAV synchronously; we run it in a
    // hidden, no-window child and don't wait on it. (Runtime-unverified.)
    let script = format!(
        "(New-Object System.Media.SoundPlayer '{}').PlaySync()",
        path.display()
    );
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(target_os = "linux")]
fn play_file(path: &std::path::Path) {
    // Try PulseAudio/PipeWire's paplay first, then ALSA's aplay. Both are
    // spawned non-blocking; either may be absent.
    if std::process::Command::new("paplay")
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_err()
    {
        let _ = std::process::Command::new("aplay")
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn play_file(_path: &std::path::Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_wav_is_a_riff_wave() {
        // Sanity: the asset is a real WAV (RIFF…WAVE) and non-trivial.
        assert!(PASTE_WAV.len() > 1000, "embedded WAV too small");
        assert_eq!(&PASTE_WAV[0..4], b"RIFF");
        assert_eq!(&PASTE_WAV[8..12], b"WAVE");
    }
}
