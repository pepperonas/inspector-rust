//! Tiny one-shot sound effects.
//!
//! Originally just the **paste/expand click** (v0.84.45); generalised to a
//! small palette of UI feedback sounds (v0.84.57): expand, OCR-recognised,
//! screenshot, record start/stop, copy-to-clipboard. Each is a short WAV
//! embedded with `include_bytes!`, materialised once to the cache dir, and
//! played via the per-OS CLI player (macOS `afplay`, Windows PowerShell
//! `SoundPlayer`, Linux `paplay`/`aplay`) on a **worker thread**,
//! fire-and-forget — playback must never block the calling path (expansion
//! runs on the main thread on macOS). WAV (not MP3) is used deliberately: no
//! decoder spin-up, so the cue is instant and low-latency.
//!
//! A single global toggle (`set_enabled`, settings key `sound.enabled`,
//! default on) gates every cue. It's an in-process `AtomicBool` seeded from
//! the settings DB at startup and updated by the `set_sound_enabled` IPC, so
//! the hot path never touches the DB.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Master on/off for all feedback sounds. Defaults to `true` (the pre-toggle
/// behaviour — the expand click always played). Seeded from settings at
/// startup via [`set_enabled`].
static SOUND_ENABLED: AtomicBool = AtomicBool::new(true);

/// Set the master sound toggle. Called at startup from the persisted setting
/// and by the `set_sound_enabled` IPC.
pub fn set_enabled(enabled: bool) {
    SOUND_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Whether feedback sounds are currently enabled.
pub fn is_enabled() -> bool {
    SOUND_ENABLED.load(Ordering::Relaxed)
}

/// A UI feedback sound. Each variant maps to an embedded WAV + a stable
/// cache filename (so the CLI players can read a real file).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// Snippet expansion / paste — a mechanical-keyboard clack.
    Expand,
    /// OCR finished and text landed on the clipboard.
    Ocr,
    /// A screen-region / window / fullscreen screenshot was captured.
    Screenshot,
    /// Screen recording started.
    RecordStart,
    /// Screen recording stopped (file saved).
    RecordStop,
    /// A value was copied to the clipboard (eyedropper hex, …).
    Copy,
}

impl Sound {
    /// The embedded WAV bytes for this cue.
    fn bytes(self) -> &'static [u8] {
        match self {
            Sound::Expand => include_bytes!("../assets/paste_mechkey.wav"),
            Sound::Ocr => include_bytes!("../assets/ocr.wav"),
            Sound::Screenshot => include_bytes!("../assets/screenshot.wav"),
            Sound::RecordStart => include_bytes!("../assets/record_start.wav"),
            Sound::RecordStop => include_bytes!("../assets/record_stop.wav"),
            Sound::Copy => include_bytes!("../assets/copy.wav"),
        }
    }

    /// Stable cache filename (one per cue).
    fn file_name(self) -> &'static str {
        match self {
            Sound::Expand => "paste_mechkey.wav",
            Sound::Ocr => "ocr.wav",
            Sound::Screenshot => "screenshot.wav",
            Sound::RecordStart => "record_start.wav",
            Sound::RecordStop => "record_stop.wav",
            Sound::Copy => "copy.wav",
        }
    }
}

/// Cache directory the WAVs are materialised into.
fn cache_dir() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("InspectorRust"))
}

/// Ensure the embedded WAV for `sound` exists on disk; return its path.
/// Each cue is written at most once (memoised in a process-wide map).
fn ensure_file(sound: Sound) -> Option<PathBuf> {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, Option<PathBuf>>>> = OnceLock::new();
    let map = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().ok()?;
    if let Some(entry) = guard.get(sound.file_name()) {
        return entry.clone();
    }
    let resolved = (|| {
        let path = cache_dir()?.join(sound.file_name());
        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&path, sound.bytes()).is_err() {
                return None;
            }
        }
        Some(path)
    })();
    guard.insert(sound.file_name(), resolved.clone());
    resolved
}

/// Play a feedback `sound`. No-op when sounds are disabled. Non-blocking:
/// spawns a worker that materialises the file (once) and shells out to the OS
/// player. Any failure is silently ignored — feedback sound is best-effort.
pub fn play(sound: Sound) {
    if !is_enabled() {
        return;
    }
    std::thread::spawn(move || {
        let Some(path) = ensure_file(sound) else { return };
        play_file(&path);
    });
}

/// Back-compat shorthand for the expand/paste click.
pub fn play_paste() {
    play(Sound::Expand);
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
    fn all_embedded_wavs_are_riff_wave() {
        // Sanity: every cue's asset is a real WAV (RIFF…WAVE) and non-trivial.
        for sound in [
            Sound::Expand,
            Sound::Ocr,
            Sound::Screenshot,
            Sound::RecordStart,
            Sound::RecordStop,
            Sound::Copy,
        ] {
            let bytes = sound.bytes();
            assert!(bytes.len() > 1000, "{sound:?} WAV too small");
            assert_eq!(&bytes[0..4], b"RIFF", "{sound:?} not RIFF");
            assert_eq!(&bytes[8..12], b"WAVE", "{sound:?} not WAVE");
        }
    }

    #[test]
    fn cue_filenames_are_unique() {
        let names = [
            Sound::Expand.file_name(),
            Sound::Ocr.file_name(),
            Sound::Screenshot.file_name(),
            Sound::RecordStart.file_name(),
            Sound::RecordStop.file_name(),
            Sound::Copy.file_name(),
        ];
        let mut seen = std::collections::HashSet::new();
        for n in names {
            assert!(seen.insert(n), "duplicate cue filename: {n}");
        }
    }

    #[test]
    fn toggle_round_trips() {
        let prev = is_enabled();
        set_enabled(false);
        assert!(!is_enabled());
        set_enabled(true);
        assert!(is_enabled());
        set_enabled(prev);
    }

    const ALL_SOUNDS: [Sound; 6] = [
        Sound::Expand,
        Sound::Ocr,
        Sound::Screenshot,
        Sound::RecordStart,
        Sound::RecordStop,
        Sound::Copy,
    ];

    #[test]
    fn riff_size_field_matches_payload() {
        // A truncated / corrupted asset would make the CLI players fail
        // silently at runtime — pin the RIFF chunk-size invariant here.
        for sound in ALL_SOUNDS {
            let bytes = sound.bytes();
            let declared =
                u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
            assert_eq!(
                declared,
                bytes.len() - 8,
                "{sound:?}: RIFF size field disagrees with file length"
            );
        }
    }

    #[test]
    fn wavs_carry_fmt_and_data_chunks() {
        for sound in ALL_SOUNDS {
            let bytes = sound.bytes();
            let has = |tag: &[u8]| bytes.windows(4).any(|w| w == tag);
            assert!(has(b"fmt "), "{sound:?}: missing fmt chunk");
            assert!(has(b"data"), "{sound:?}: missing data chunk");
        }
    }

    #[test]
    fn every_cue_has_distinct_audio_content() {
        // A copy-paste slip mapping two cues onto the same asset would make
        // e.g. record-start and record-stop indistinguishable by ear.
        for (i, a) in ALL_SOUNDS.iter().enumerate() {
            for b in &ALL_SOUNDS[i + 1..] {
                assert_ne!(a.bytes(), b.bytes(), "{a:?} and {b:?} share the same WAV");
            }
        }
    }

    #[test]
    fn wav_data_chunks_are_nonempty_pcm_payloads() {
        // A zero-length data chunk would "play" as silence — the cue silently
        // stops giving feedback. Parse the declared data-chunk size directly.
        for sound in ALL_SOUNDS {
            let bytes = sound.bytes();
            let pos = bytes
                .windows(4)
                .position(|w| w == b"data")
                .unwrap_or_else(|| panic!("{sound:?}: no data chunk"));
            let size = u32::from_le_bytes([
                bytes[pos + 4],
                bytes[pos + 5],
                bytes[pos + 6],
                bytes[pos + 7],
            ]) as usize;
            assert!(size > 0, "{sound:?}: empty data chunk");
            assert!(
                pos + 8 + size <= bytes.len(),
                "{sound:?}: data chunk claims {size} B past the end of the file"
            );
        }
    }

    #[test]
    fn cue_filenames_are_wav_and_path_safe() {
        for sound in ALL_SOUNDS {
            let name = sound.file_name();
            assert!(name.ends_with(".wav"), "{sound:?}: player is picked for WAV");
            // Materialised straight into the cache dir — no separators allowed.
            assert!(
                !name.contains('/') && !name.contains('\\') && !name.contains(".."),
                "{sound:?}: filename must be a plain leaf name"
            );
        }
    }
}
