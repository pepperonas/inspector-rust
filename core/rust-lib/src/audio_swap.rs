//! Replace / overlay a video's audio track with another audio file — the
//! `Ctrl+Shift+Alt+M` flow. Reads the Finder selection (a video), opens an
//! overlay to pick an audio source (a local file, or a YouTube track via
//! `yt-dlp`), choose a start position + trim, and either **replace** the
//! original track or **mix** the new audio over it. Output is written to a
//! sibling file (`<stem>-audioswap.mp4`) — non-destructive.
//!
//! Engine: **ffmpeg** (already a dependency; video is stream-copied so the mux
//! is fast and lossless, only the audio is re-encoded). The pure argv builders +
//! the yt-dlp arg builder are unit-tested; the spawn/probe is a thin wrapper.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Sentinel — yt-dlp not found (frontend shows an install hint).
pub const ERR_NO_YTDLP: &str = "audioswap.no_ytdlp";
/// Sentinel — ffmpeg not found.
pub const ERR_NO_FFMPEG: &str = "audioswap.no_ffmpeg";

/// Video extensions the Finder-selection step accepts.
pub const VIDEO_EXTS: &[&str] = &["mp4", "mov", "m4v", "mkv", "avi", "webm"];

/// Whether a path looks like a video we can process (by extension).
pub fn is_video_path(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SwapMode {
    /// Drop the original audio; the inserted audio becomes the track (silence
    /// before its start position and after it ends).
    Replace,
    /// Keep the original audio; mix the inserted audio over it at the start
    /// position.
    Mix,
}

/// Everything the ffmpeg mux needs. Times are in seconds.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwapSpec {
    pub mode: SwapMode,
    /// Where in the **video** timeline the inserted audio begins.
    pub start_seconds: f64,
    /// Trim within the **source audio**: take `[audio_in, audio_out]`
    /// (`audio_out = None` → to the end of the audio).
    pub audio_in: f64,
    pub audio_out: Option<f64>,
    /// Mix mode: gain on the inserted audio (1.0 = unchanged).
    pub overlay_volume: f64,
    /// Mix mode: gain on the original audio (1.0 = unchanged).
    pub original_volume: f64,
    /// Total video duration (seconds) — bounds the Replace track length.
    pub video_seconds: f64,
}

fn fmt(v: f64) -> String {
    // up to 3 decimals, no trailing-zero noise
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".into() } else { s.to_string() }
}

/// The inserted-audio filter chain (shared by both modes): trim → reset PTS →
/// volume → delay to the start position. `label` is the output pad name.
fn inserted_chain(spec: &SwapSpec, volume: f64, label: &str) -> String {
    let delay_ms = (spec.start_seconds.max(0.0) * 1000.0).round() as i64;
    let mut c = String::from("[1:a]atrim=start=");
    c.push_str(&fmt(spec.audio_in.max(0.0)));
    if let Some(o) = spec.audio_out {
        c.push_str(":end=");
        c.push_str(&fmt(o));
    }
    c.push_str(",asetpts=N/SR/TB");
    if (volume - 1.0).abs() > f64::EPSILON {
        c.push_str(&format!(",volume={}", fmt(volume)));
    }
    if delay_ms > 0 {
        c.push_str(&format!(",adelay={delay_ms}:all=1"));
    }
    c.push_str(&format!("[{label}]"));
    c
}

/// Build the ffmpeg argv. Input 0 = video, input 1 = audio. Video is copied;
/// audio is re-encoded to AAC. Pure — no I/O.
pub fn build_swap_args(video: &str, audio: &str, spec: &SwapSpec, out: &str) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-y".into(),
        "-hide_banner".into(),
        "-i".into(),
        video.into(),
        "-i".into(),
        audio.into(),
        "-filter_complex".into(),
    ];

    match spec.mode {
        SwapMode::Replace => {
            // inserted audio + pad to silence; bound to the video length.
            let mut g = inserted_chain(spec, spec.overlay_volume, "ins");
            g.push_str(";[ins]apad[a]");
            a.push(g);
            a.push("-map".into());
            a.push("0:v".into());
            a.push("-map".into());
            a.push("[a]".into());
            a.push("-t".into());
            a.push(fmt(spec.video_seconds));
        }
        SwapMode::Mix => {
            // original (optionally gained) + inserted, mixed; bounded to the
            // original audio length. normalize=0 keeps both at full level.
            let mut g = String::new();
            if (spec.original_volume - 1.0).abs() > f64::EPSILON {
                g.push_str(&format!("[0:a]volume={}[orig];", fmt(spec.original_volume)));
            }
            let orig_pad = if (spec.original_volume - 1.0).abs() > f64::EPSILON {
                "[orig]"
            } else {
                "[0:a]"
            };
            g.push_str(&inserted_chain(spec, spec.overlay_volume, "ins"));
            g.push_str(&format!(
                ";{orig_pad}[ins]amix=inputs=2:duration=first:normalize=0[a]"
            ));
            a.push(g);
            a.push("-map".into());
            a.push("0:v".into());
            a.push("-map".into());
            a.push("[a]".into());
        }
    }

    for s in [
        "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart",
    ] {
        a.push(s.into());
    }
    a.push(out.into());
    a
}

/// Build the yt-dlp argv to extract a track's audio as m4a into `out_template`
/// (a yt-dlp output template, e.g. `/dir/ytaudio.%(ext)s`). `--print
/// after_move:filepath` makes yt-dlp emit the final path on stdout. `ffmpeg_dir`
/// points yt-dlp at our ffmpeg. Pure.
pub fn build_ytdlp_args(url: &str, ffmpeg_dir: &str, out_template: &str) -> Vec<String> {
    vec![
        "-x".into(),
        "--audio-format".into(),
        "m4a".into(),
        "--audio-quality".into(),
        "0".into(),
        "--no-playlist".into(),
        "--no-progress".into(),
        "--ffmpeg-location".into(),
        ffmpeg_dir.into(),
        "--print".into(),
        "after_move:filepath".into(),
        "-o".into(),
        out_template.into(),
        url.into(),
    ]
}

/// The sibling output path for a processed video: `<stem>-audioswap.mp4`.
pub fn output_path_for(video: &Path) -> PathBuf {
    let stem = video.file_stem().and_then(|s| s.to_str()).unwrap_or("video");
    let parent = video.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{stem}-audioswap.mp4"))
}

// ── tool discovery ───────────────────────────────────────────────────────────

fn which(exe_unix: &str, exe_win: &str, extra: &[&str]) -> Option<PathBuf> {
    let exe = if cfg!(windows) { exe_win } else { exe_unix };
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            let cand = dir.join(exe);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    extra.iter().map(PathBuf::from).find(|p| p.is_file())
}

pub fn yt_dlp_path() -> Option<PathBuf> {
    which(
        "yt-dlp",
        "yt-dlp.exe",
        &["/opt/homebrew/bin/yt-dlp", "/usr/local/bin/yt-dlp", "/usr/bin/yt-dlp"],
    )
}

fn ffprobe_path(ffmpeg: &Path) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" };
    ffmpeg
        .parent()
        .map(|d| d.join(exe))
        .filter(|p| p.is_file())
        .or_else(|| which("ffprobe", "ffprobe.exe", &[]))
}

// ── probes ───────────────────────────────────────────────────────────────────

/// Media duration in seconds (via ffprobe `format=duration`).
pub fn probe_duration(file: &Path) -> Option<f64> {
    let ffmpeg = crate::screen_record::ffmpeg_path()?;
    let ffprobe = ffprobe_path(&ffmpeg)?;
    let out = Command::new(ffprobe)
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "default=noprint_wrappers=1:nokey=1"])
        .arg(file)
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().ok().filter(|v| *v > 0.0)
}

/// Whether the file has at least one audio stream.
pub fn has_audio_stream(file: &Path) -> bool {
    let Some(ffmpeg) = crate::screen_record::ffmpeg_path() else { return false };
    let Some(ffprobe) = ffprobe_path(&ffmpeg) else { return false };
    Command::new(ffprobe)
        .args(["-v", "error", "-select_streams", "a", "-show_entries", "stream=index", "-of", "csv=p=0"])
        .arg(file)
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

// ── pipelines ────────────────────────────────────────────────────────────────

/// Download a YouTube (or any yt-dlp-supported) URL's audio as m4a into `dir`.
/// Returns the produced file path.
pub fn download_youtube_audio(url: &str, dir: &Path) -> Result<PathBuf, String> {
    let yt = yt_dlp_path().ok_or_else(|| ERR_NO_YTDLP.to_string())?;
    let ffmpeg = crate::screen_record::ffmpeg_path().ok_or_else(|| ERR_NO_FFMPEG.to_string())?;
    let ffmpeg_dir = ffmpeg.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    std::fs::create_dir_all(dir).map_err(|e| format!("create dir: {e}"))?;
    let template = dir.join("ytaudio.%(ext)s");
    let args = build_ytdlp_args(url, &ffmpeg_dir, &template.to_string_lossy());
    let out = Command::new(yt).args(&args).output().map_err(|e| format!("yt-dlp: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("yt-dlp failed: {}", err.lines().last().unwrap_or("unknown error")));
    }
    // `--print after_move:filepath` → the final path is the last stdout line.
    let stdout = String::from_utf8_lossy(&out.stdout);
    if let Some(line) = stdout.lines().rev().find(|l| !l.trim().is_empty()) {
        let p = PathBuf::from(line.trim());
        if p.is_file() {
            return Ok(p);
        }
    }
    // Fallback: the templated m4a.
    let m4a = dir.join("ytaudio.m4a");
    if m4a.is_file() {
        Ok(m4a)
    } else {
        Err("yt-dlp produced no output file".into())
    }
}

/// Mux `audio` into `video` per `spec`, writing a sibling `<stem>-audioswap.mp4`.
/// Returns the output path. The mode is downgraded to Replace if Mix was asked
/// for but the video has no original audio stream.
pub fn apply_swap(video: &Path, audio: &Path, mut spec: SwapSpec) -> Result<PathBuf, String> {
    let ffmpeg = crate::screen_record::ffmpeg_path().ok_or_else(|| ERR_NO_FFMPEG.to_string())?;
    if spec.video_seconds <= 0.0 {
        spec.video_seconds = probe_duration(video).ok_or("could not read video duration")?;
    }
    if spec.mode == SwapMode::Mix && !has_audio_stream(video) {
        spec.mode = SwapMode::Replace; // nothing to mix with
    }
    let out = output_path_for(video);
    let args = build_swap_args(
        &video.to_string_lossy(),
        &audio.to_string_lossy(),
        &spec,
        &out.to_string_lossy(),
    );
    let result = Command::new(&ffmpeg).args(&args).output().map_err(|e| format!("ffmpeg: {e}"))?;
    if !result.status.success() {
        let err = String::from_utf8_lossy(&result.stderr);
        return Err(format!("ffmpeg failed: {}", err.lines().last().unwrap_or("unknown error")));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(mode: SwapMode) -> SwapSpec {
        SwapSpec {
            mode,
            start_seconds: 0.0,
            audio_in: 0.0,
            audio_out: None,
            overlay_volume: 1.0,
            original_volume: 1.0,
            video_seconds: 30.0,
        }
    }

    #[test]
    fn is_video_path_checks_extension() {
        assert!(is_video_path(Path::new("/a/clip.MP4")));
        assert!(is_video_path(Path::new("/a/clip.mov")));
        assert!(!is_video_path(Path::new("/a/song.mp3")));
        assert!(!is_video_path(Path::new("/a/noext")));
    }

    #[test]
    fn output_path_is_sibling_with_suffix() {
        assert_eq!(
            output_path_for(Path::new("/movies/My Clip.mov")),
            PathBuf::from("/movies/My Clip-audioswap.mp4")
        );
    }

    #[test]
    fn replace_drops_original_audio_and_bounds_to_video() {
        let args = build_swap_args("v.mp4", "a.m4a", &spec(SwapMode::Replace), "out.mp4");
        let fc = args.iter().position(|s| s == "-filter_complex").unwrap();
        let graph = &args[fc + 1];
        assert!(graph.contains("[1:a]atrim=start=0"));
        assert!(graph.contains("apad[a]"));
        // video mapped, [a] mapped, original audio NOT mapped
        assert!(args.windows(2).any(|w| w[0] == "-map" && w[1] == "0:v"));
        assert!(args.windows(2).any(|w| w[0] == "-map" && w[1] == "[a]"));
        assert!(!args.windows(2).any(|w| w[0] == "-map" && w[1] == "0:a"));
        // bounded to the video length
        assert!(args.windows(2).any(|w| w[0] == "-t" && w[1] == "30"));
        // video stream-copied
        assert!(args.windows(2).any(|w| w[0] == "-c:v" && w[1] == "copy"));
    }

    #[test]
    fn mix_keeps_original_and_uses_amix_no_normalize() {
        let args = build_swap_args("v.mp4", "a.m4a", &spec(SwapMode::Mix), "out.mp4");
        let graph = &args[args.iter().position(|s| s == "-filter_complex").unwrap() + 1];
        assert!(graph.contains("amix=inputs=2:duration=first:normalize=0"));
        assert!(graph.contains("[0:a][ins]")); // original at unity → used directly
        assert!(!args.windows(2).any(|w| w[0] == "-t")); // mix is bounded by amix, not -t
    }

    #[test]
    fn start_position_becomes_adelay_ms() {
        let mut s = spec(SwapMode::Replace);
        s.start_seconds = 2.5;
        let args = build_swap_args("v.mp4", "a.m4a", &s, "out.mp4");
        let graph = &args[args.iter().position(|x| x == "-filter_complex").unwrap() + 1];
        assert!(graph.contains("adelay=2500:all=1"), "graph={graph}");
    }

    #[test]
    fn audio_trim_in_out_and_volume_render() {
        let mut s = spec(SwapMode::Mix);
        s.audio_in = 5.0;
        s.audio_out = Some(12.5);
        s.overlay_volume = 0.5;
        s.original_volume = 0.8;
        let args = build_swap_args("v.mp4", "a.m4a", &s, "out.mp4");
        let graph = &args[args.iter().position(|x| x == "-filter_complex").unwrap() + 1];
        assert!(graph.contains("atrim=start=5:end=12.5"));
        assert!(graph.contains("volume=0.5"));
        assert!(graph.contains("[0:a]volume=0.8[orig]"));
        assert!(graph.contains("[orig][ins]amix"));
    }

    #[test]
    fn no_delay_filter_when_start_is_zero() {
        let args = build_swap_args("v.mp4", "a.m4a", &spec(SwapMode::Replace), "out.mp4");
        let graph = &args[args.iter().position(|x| x == "-filter_complex").unwrap() + 1];
        assert!(!graph.contains("adelay"));
    }

    #[test]
    fn ytdlp_args_extract_m4a_with_ffmpeg_location() {
        let a = build_ytdlp_args("https://youtu.be/x", "/opt/homebrew/bin", "/tmp/ytaudio.%(ext)s");
        assert!(a.windows(2).any(|w| w[0] == "--audio-format" && w[1] == "m4a"));
        assert!(a.contains(&"-x".to_string()));
        assert!(a.windows(2).any(|w| w[0] == "--ffmpeg-location" && w[1] == "/opt/homebrew/bin"));
        assert!(a.windows(2).any(|w| w[0] == "--print" && w[1] == "after_move:filepath"));
        assert_eq!(a.last().unwrap(), "https://youtu.be/x");
    }
}
