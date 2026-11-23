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
//!
//! **Screenshot sound is selectable (v0.111.0; `camera` default since
//! v0.217.0):** the `Screenshot` cue plays one of four shutter styles from the
//! `sound-effects` archive — `camera` (classic camera shutter, crisp/light,
//! `screenshot` in the archive; the default), `snap` (sharp finger snap),
//! `dslr` (rich mirror-slap + double curtain, `screenshot_dslr` in the
//! archive) or `switch` (mechanical switch click) — or nothing at all
//! (`off`). Same atomic-seeded pattern as the master toggle (settings key
//! `sound.screenshot_style`, `AtomicU8`); the per-style cache filenames are
//! distinct so switching styles re-materialises the right WAV.

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

/// Which shutter the `Screenshot` cue plays (settings key
/// `sound.screenshot_style`). `Off` = the screenshot stays silent while every
/// other cue keeps following the master toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenshotStyle {
    /// Classic camera shutter (the shipped default since v0.217.0) — a crisp,
    /// light "a photo was taken" cue, vendored from the `sound-effects` archive.
    Camera,
    Snap,
    Dslr,
    Switch,
    Off,
}

/// Current screenshot style as an in-process atomic (0..=4 in enum order) —
/// same "hot path never reads the DB" pattern as the master toggle. The
/// numbers are internal only; the SETTING persists the string, so this
/// ordering can be renumbered freely.
static SCREENSHOT_STYLE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Whitelist an arbitrary stored/IPC string to a valid style, defaulting to
/// `"camera"` (the shipped default) on anything unknown — a hand-edited or
/// legacy value must never disable the cue by accident.
pub fn normalise_screenshot_style(s: &str) -> &'static str {
    match s.trim().to_ascii_lowercase().as_str() {
        "snap" => "snap",
        "dslr" => "dslr",
        "switch" => "switch",
        "off" => "off",
        _ => "camera",
    }
}

/// Set the screenshot style. Called at startup from the persisted setting and
/// by the `set_screenshot_sound` IPC.
pub fn set_screenshot_style(style: &str) {
    let v = match normalise_screenshot_style(style) {
        "snap" => 1,
        "dslr" => 2,
        "switch" => 3,
        "off" => 4,
        _ => 0,
    };
    SCREENSHOT_STYLE.store(v, Ordering::Relaxed);
}

/// Pure u8 → style mapping (the atomic's encoding). `0` — including a never-set
/// atomic — is the shipped default, so an install without a stored setting
/// hears it. Kept separate from the atomic read so it is testable without
/// touching the process-global (parallel tests mutate it).
fn screenshot_style_from_u8(v: u8) -> ScreenshotStyle {
    match v {
        1 => ScreenshotStyle::Snap,
        2 => ScreenshotStyle::Dslr,
        3 => ScreenshotStyle::Switch,
        4 => ScreenshotStyle::Off,
        _ => ScreenshotStyle::Camera,
    }
}

/// The current screenshot style.
pub fn screenshot_style() -> ScreenshotStyle {
    screenshot_style_from_u8(SCREENSHOT_STYLE.load(Ordering::Relaxed))
}

/// The current screenshot style as its settings/IPC string.
pub fn screenshot_style_str() -> &'static str {
    match screenshot_style() {
        ScreenshotStyle::Camera => "camera",
        ScreenshotStyle::Snap => "snap",
        ScreenshotStyle::Dslr => "dslr",
        ScreenshotStyle::Switch => "switch",
        ScreenshotStyle::Off => "off",
    }
}

/// The embedded asset for a given (audible) style — `None` for `Off`. Pure
/// per-style accessor so tests never depend on the process-global atomic.
fn screenshot_asset(style: ScreenshotStyle) -> Option<(&'static str, &'static [u8])> {
    match style {
        ScreenshotStyle::Camera => {
            Some(("shutter_camera.wav", include_bytes!("../assets/shutter_camera.wav")))
        }
        ScreenshotStyle::Snap => Some(("shutter_snap.wav", include_bytes!("../assets/shutter_snap.wav"))),
        ScreenshotStyle::Dslr => Some(("shutter_dslr.wav", include_bytes!("../assets/shutter_dslr.wav"))),
        ScreenshotStyle::Switch => {
            Some(("shutter_switch.wav", include_bytes!("../assets/shutter_switch.wav")))
        }
        ScreenshotStyle::Off => None,
    }
}

/// An embedded sound: cache file name + WAV bytes.
type Asset = (&'static str, &'static [u8]);
/// One selectable style: its settings name and its sound (`None` = off).
type Style = (&'static str, Option<Asset>);

/// A cue whose sound the user picks (settings `sound.ocr_style`,
/// `sound.expand_style`). Entry 0 is the default; `None` = the cue stays
/// silent while every other cue keeps following the master toggle. Same
/// "hot path never reads the DB" pattern as the screenshot style.
pub struct CueStyles {
    /// Settings key the choice persists under.
    pub key: &'static str,
    current: std::sync::atomic::AtomicU8,
    styles: &'static [Style],
}

impl CueStyles {
    fn index_of(&self, s: &str) -> usize {
        let s = s.trim().to_ascii_lowercase();
        self.styles.iter().position(|(n, _)| *n == s).unwrap_or(0)
    }

    /// Whitelist a stored/IPC string; anything unknown is the default — a
    /// hand-edited value must never silence the cue by accident.
    pub fn normalise(&self, s: &str) -> &'static str {
        self.styles[self.index_of(s)].0
    }

    /// Apply a style (startup seed, settings sync, IPC).
    pub fn set(&self, s: &str) {
        self.current.store(self.index_of(s) as u8, Ordering::Relaxed);
    }

    fn entry(&self) -> &'static Style {
        let i = self.current.load(Ordering::Relaxed) as usize;
        self.styles.get(i).unwrap_or(&self.styles[0])
    }

    /// The current style as its settings/IPC string.
    pub fn current_str(&self) -> &'static str {
        self.entry().0
    }

    /// The current asset — `None` when the style is `off`.
    fn asset(&self) -> Option<Asset> {
        self.entry().1
    }

    /// Every style name, default first.
    pub fn names(&self) -> Vec<&'static str> {
        self.styles.iter().map(|(n, _)| *n).collect()
    }
}

/// Sound of the "text recognised" cue.
pub static OCR_STYLES: CueStyles = CueStyles {
    key: "sound.ocr_style",
    current: std::sync::atomic::AtomicU8::new(0),
    styles: &[
        ("success", Some(("ocr.wav", include_bytes!("../assets/ocr.wav")))),
        ("chime", Some(("ocr_chime.wav", include_bytes!("../assets/ocr_chime.wav")))),
        ("ping", Some(("ocr_ping.wav", include_bytes!("../assets/ocr_ping.wav")))),
        ("pop", Some(("ocr_pop.wav", include_bytes!("../assets/ocr_pop.wav")))),
        ("off", None),
    ],
};

/// Sound of the snippet-expansion cue.
pub static EXPAND_STYLES: CueStyles = CueStyles {
    key: "sound.expand_style",
    current: std::sync::atomic::AtomicU8::new(0),
    styles: &[
        ("mechkey", Some(("paste_mechkey.wav", include_bytes!("../assets/paste_mechkey.wav")))),
        ("key", Some(("expand_key.wav", include_bytes!("../assets/expand_key.wav")))),
        ("bubble", Some(("expand_bubble.wav", include_bytes!("../assets/expand_bubble.wav")))),
        ("scifi", Some(("expand_scifi.wav", include_bytes!("../assets/expand_scifi.wav")))),
        ("off", None),
    ],
};

/// The selectable cue behind an IPC name (`"ocr"` / `"expand"`).
pub fn cue_styles(cue: &str) -> Option<(&'static CueStyles, Sound)> {
    match cue {
        "ocr" => Some((&OCR_STYLES, Sound::Ocr)),
        "expand" => Some((&EXPAND_STYLES, Sound::Expand)),
        _ => None,
    }
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
    /// The style table of a cue the user can pick a sound for.
    fn styles(self) -> Option<&'static CueStyles> {
        match self {
            Sound::Expand => Some(&EXPAND_STYLES),
            Sound::Ocr => Some(&OCR_STYLES),
            _ => None,
        }
    }

    /// Whether the user switched this one cue off.
    fn silenced(self) -> bool {
        match self {
            Sound::Screenshot => screenshot_style() == ScreenshotStyle::Off,
            _ => self.styles().is_some_and(|s| s.asset().is_none()),
        }
    }

    /// The embedded WAV bytes for this cue.
    fn bytes(self) -> &'static [u8] {
        match self {
            // `play` returns early on off; the default is the fallback.
            Sound::Expand | Sound::Ocr => {
                let st = self.styles().unwrap();
                st.asset().or(st.styles[0].1).unwrap().1
            }
            // Style-selected; `play` returns early on Off, and the Camera asset
            // (the default) is the defensive fallback if this is ever reached
            // with Off.
            Sound::Screenshot => {
                screenshot_asset(screenshot_style())
                    .unwrap_or_else(|| screenshot_asset(ScreenshotStyle::Camera).unwrap())
                    .1
            }
            Sound::RecordStart => include_bytes!("../assets/record_start.wav"),
            Sound::RecordStop => include_bytes!("../assets/record_stop.wav"),
            Sound::Copy => include_bytes!("../assets/copy.wav"),
        }
    }

    /// Stable cache filename (one per cue).
    fn file_name(self) -> &'static str {
        match self {
            Sound::Expand | Sound::Ocr => {
                let st = self.styles().unwrap();
                st.asset().or(st.styles[0].1).unwrap().0
            }
            Sound::Screenshot => {
                screenshot_asset(screenshot_style())
                    .unwrap_or_else(|| screenshot_asset(ScreenshotStyle::Camera).unwrap())
                    .0
            }
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
    if sound.silenced() {
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

    /// Every audible screenshot style, resolved through the PURE per-style
    /// accessor — the single source of truth for the structural WAV tests + the
    /// `off`-has-no-asset test, so neither depends on the process-global atomic
    /// (parallel tests mutate it).
    const AUDIBLE_STYLES: [ScreenshotStyle; 4] = [
        ScreenshotStyle::Camera,
        ScreenshotStyle::Snap,
        ScreenshotStyle::Dslr,
        ScreenshotStyle::Switch,
    ];

    /// Every embedded asset as (name, bytes) — the fixed cues, every style of the
    /// selectable cues + the audible
    /// screenshot styles resolved through the PURE per-style accessor, so no
    /// structural test depends on the process-global style atomic.
    fn all_assets() -> Vec<(&'static str, &'static [u8])> {
        let mut v: Vec<(&'static str, &'static [u8])> = vec![
            (Sound::RecordStart.file_name(), Sound::RecordStart.bytes()),
            (Sound::RecordStop.file_name(), Sound::RecordStop.bytes()),
            (Sound::Copy.file_name(), Sound::Copy.bytes()),
        ];
        for style in AUDIBLE_STYLES {
            v.push(screenshot_asset(style).expect("audible style has an asset"));
        }
        for set in [&OCR_STYLES, &EXPAND_STYLES] {
            v.extend(set.styles.iter().filter_map(|(_, a)| *a));
        }
        v
    }

    /// A private table, so these tests never touch the process-wide choice.
    fn sample() -> CueStyles {
        CueStyles {
            key: "sound.test_style",
            current: std::sync::atomic::AtomicU8::new(0),
            styles: OCR_STYLES.styles,
        }
    }

    #[test]
    fn a_cue_style_falls_back_to_the_default_never_to_silence() {
        let s = sample();
        assert_eq!(s.normalise(" PING "), "ping");
        assert_eq!(s.normalise("off"), "off");
        for junk in ["", "none", "silent", "mechkey"] {
            assert_eq!(s.normalise(junk), "success", "{junk:?}");
        }
        s.set("garbage");
        assert_eq!(s.current_str(), "success");
        assert!(s.asset().is_some());
    }

    #[test]
    fn a_cue_style_round_trips_and_off_has_no_asset() {
        let s = sample();
        for name in s.names() {
            s.set(name);
            assert_eq!(s.current_str(), name);
            assert_eq!(s.asset().is_none(), name == "off", "{name}");
        }
    }

    #[test]
    fn every_selectable_cue_starts_audible_and_ends_with_off() {
        for (cue, default) in [("ocr", "success"), ("expand", "mechkey")] {
            let (set, _) = cue_styles(cue).unwrap();
            let names = set.names();
            assert_eq!(names[0], default);
            assert!(set.styles[0].1.is_some(), "{cue}: the default must sound");
            assert_eq!(names.last(), Some(&"off"));
            assert_eq!(names.iter().filter(|n| **n == "off").count(), 1);
            let unique: std::collections::HashSet<_> = names.iter().collect();
            assert_eq!(unique.len(), names.len(), "{cue}: duplicate style name");
            assert!(set.key.starts_with("sound."), "settings sync keys on the prefix");
        }
        assert!(cue_styles("screenshot").is_none());
    }

    #[test]
    fn all_embedded_wavs_are_riff_wave() {
        // Sanity: every cue's asset is a real WAV (RIFF…WAVE) and non-trivial.
        for (name, bytes) in all_assets() {
            assert!(bytes.len() > 1000, "{name} WAV too small");
            assert_eq!(&bytes[0..4], b"RIFF", "{name} not RIFF");
            assert_eq!(&bytes[8..12], b"WAVE", "{name} not WAVE");
        }
    }

    #[test]
    fn cue_filenames_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for (name, _) in all_assets() {
            assert!(seen.insert(name), "duplicate cue filename: {name}");
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

    #[test]
    fn riff_size_field_matches_payload() {
        // A truncated / corrupted asset would make the CLI players fail
        // silently at runtime — pin the RIFF chunk-size invariant here.
        for (name, bytes) in all_assets() {
            let declared =
                u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
            assert_eq!(
                declared,
                bytes.len() - 8,
                "{name}: RIFF size field disagrees with file length"
            );
        }
    }

    #[test]
    fn wavs_carry_fmt_and_data_chunks() {
        for (name, bytes) in all_assets() {
            let has = |tag: &[u8]| bytes.windows(4).any(|w| w == tag);
            assert!(has(b"fmt "), "{name}: missing fmt chunk");
            assert!(has(b"data"), "{name}: missing data chunk");
        }
    }

    #[test]
    fn every_cue_has_distinct_audio_content() {
        // A copy-paste slip mapping two cues onto the same asset would make
        // e.g. record-start and record-stop — or two shutter styles —
        // indistinguishable by ear.
        let assets = all_assets();
        for (i, (an, ab)) in assets.iter().enumerate() {
            for (bn, bb) in &assets[i + 1..] {
                assert_ne!(ab, bb, "{an} and {bn} share the same WAV");
            }
        }
    }

    #[test]
    fn wav_data_chunks_are_nonempty_pcm_payloads() {
        // A zero-length data chunk would "play" as silence — the cue silently
        // stops giving feedback. Parse the declared data-chunk size directly.
        for (name, bytes) in all_assets() {
            let pos = bytes
                .windows(4)
                .position(|w| w == b"data")
                .unwrap_or_else(|| panic!("{name}: no data chunk"));
            let size = u32::from_le_bytes([
                bytes[pos + 4],
                bytes[pos + 5],
                bytes[pos + 6],
                bytes[pos + 7],
            ]) as usize;
            assert!(size > 0, "{name}: empty data chunk");
            assert!(
                pos + 8 + size <= bytes.len(),
                "{name}: data chunk claims {size} B past the end of the file"
            );
        }
    }

    #[test]
    fn cue_filenames_are_wav_and_path_safe() {
        for (name, _) in all_assets() {
            assert!(name.ends_with(".wav"), "{name}: player is picked for WAV");
            // Materialised straight into the cache dir — no separators allowed.
            assert!(
                !name.contains('/') && !name.contains('\\') && !name.contains(".."),
                "{name}: filename must be a plain leaf name"
            );
        }
    }

    #[test]
    fn screenshot_style_normalisation_defaults_to_camera() {
        // Unknown / legacy / hand-edited values must fall back to the shipped
        // default (camera since v0.217.0), never to silence.
        assert_eq!(normalise_screenshot_style("camera"), "camera");
        assert_eq!(normalise_screenshot_style("snap"), "snap");
        assert_eq!(normalise_screenshot_style("DSLR"), "dslr");
        assert_eq!(normalise_screenshot_style(" switch "), "switch");
        assert_eq!(normalise_screenshot_style("off"), "off");
        assert_eq!(normalise_screenshot_style(""), "camera");
        assert_eq!(normalise_screenshot_style("shutter"), "camera");
        assert_eq!(normalise_screenshot_style("none"), "camera");
    }

    #[test]
    fn screenshot_style_round_trips_through_the_atomic() {
        let prev = screenshot_style_str().to_string();
        for style in ["camera", "dslr", "switch", "off", "snap"] {
            set_screenshot_style(style);
            assert_eq!(screenshot_style_str(), style);
        }
        set_screenshot_style("definitely-invalid");
        assert_eq!(screenshot_style_str(), "camera", "invalid input must land on the default");
        set_screenshot_style(&prev);
    }

    #[test]
    fn the_fresh_atomic_default_is_camera() {
        // A never-set atomic (value 0) MUST resolve to the shipped default, so
        // an install without a stored setting hears the camera shutter.
        assert_eq!(screenshot_style_from_u8(0), ScreenshotStyle::Camera);
    }

    #[test]
    fn off_style_has_no_asset_and_audible_styles_do() {
        assert!(screenshot_asset(ScreenshotStyle::Off).is_none());
        for style in AUDIBLE_STYLES {
            let (name, bytes) = screenshot_asset(style).unwrap();
            assert!(name.starts_with("shutter_"), "style filename convention: {name}");
            assert!(!bytes.is_empty());
        }
    }
}
