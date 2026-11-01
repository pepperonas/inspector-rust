//! ffmpeg / ffprobe discovery, shared by the recorder, trim, audio swap, transcribe and downloads.

use std::path::{Path, PathBuf};

/// Locate the `ffmpeg` binary on PATH or in common install locations.
pub fn ffmpeg_path() -> Option<PathBuf> {
    // Honour an explicit PATH lookup first.
    if let Ok(path) = std::env::var("PATH") {
        let exe = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
        for dir in std::env::split_paths(&path) {
            let cand = dir.join(exe);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    // Common spots GUI apps (with a stripped PATH) miss.
    let extra: &[&str] = if cfg!(target_os = "macos") {
        &["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"]
    } else if cfg!(windows) {
        &[r"C:\ffmpeg\bin\ffmpeg.exe", r"C:\Program Files\ffmpeg\bin\ffmpeg.exe"]
    } else {
        &["/usr/bin/ffmpeg", "/usr/local/bin/ffmpeg"]
    };
    extra.iter().map(PathBuf::from).find(|p| p.is_file())
}

/// Locate `ffprobe` — it ships alongside `ffmpeg`, so try the resolved ffmpeg's
/// own directory first, then PATH.
pub fn ffprobe_beside(ffmpeg: &Path) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" };
    if let Some(dir) = ffmpeg.parent() {
        let cand = dir.join(exe);
        if cand.is_file() {
            return Some(cand);
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let cand = dir.join(exe);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// Locate `ffprobe` by first resolving `ffmpeg`.
pub fn ffprobe_path() -> Option<PathBuf> {
    ffprobe_beside(&ffmpeg_path()?)
}
