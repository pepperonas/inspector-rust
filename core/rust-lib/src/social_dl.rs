//! Download social-media content (YouTube / Instagram / TikTok / Facebook) via
//! `yt-dlp`. IR auto-detects a social URL (in a clip or the search bar) and the
//! preview offers a download — **video or audio** for YouTube, **video** for the
//! rest. Output lands in `~/Downloads` and is revealed in Finder.
//!
//! Engine: `yt-dlp` (+ our ffmpeg for muxing/extraction), the same tool the
//! audio-swap feature already uses. The platform detector + the argv builders
//! are pure and unit-tested.

use std::path::{Path, PathBuf};
use std::process::Command;

pub use crate::audio_swap::{yt_dlp_path, ERR_NO_FFMPEG, ERR_NO_YTDLP};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    YouTube,
    Instagram,
    TikTok,
    Facebook,
}

/// Recognise a supported social-media URL by host. Returns `None` for anything
/// else (so the preview only offers a download when it makes sense).
pub fn detect_platform(url: &str) -> Option<Platform> {
    let u = url.trim().to_lowercase();
    if !(u.starts_with("http://") || u.starts_with("https://")) {
        return None;
    }
    if u.contains("youtube.com") || u.contains("youtu.be") {
        Some(Platform::YouTube)
    } else if u.contains("instagram.com") {
        Some(Platform::Instagram)
    } else if u.contains("tiktok.com") {
        Some(Platform::TikTok)
    } else if u.contains("facebook.com") || u.contains("fb.watch") || u.contains("fb.com") {
        Some(Platform::Facebook)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DlMode {
    Video,
    Audio,
}

/// Build the yt-dlp argv. `--print after_move:filepath` prints the final path on
/// stdout; `--` ends option parsing so a `-…` URL can't smuggle a flag. Pure.
pub fn build_dl_args(url: &str, mode: DlMode, ffmpeg_dir: &str, out_template: &str) -> Vec<String> {
    let mut a: Vec<String> = vec![];
    match mode {
        DlMode::Video => {
            // Prefer **H.264** video + AAC/m4a audio so the result plays in macOS
            // QuickTime. Instagram/TikTok/etc. also serve VP9 (higher res) — but
            // VP9 can't be muxed into a playable mp4 and QuickTime can't decode it,
            // so the download looked "audio-only / broken". Sorting H.264 to the
            // top makes yt-dlp pick the H.264 rendition when one exists (no
            // re-encode); `--merge-output-format mp4` sets the container.
            a.extend(["-S".into(), "vcodec:h264,res,acodec:m4a".into()]);
            a.extend(["--merge-output-format".into(), "mp4".into()]);
        }
        DlMode::Audio => {
            a.extend(["-x".into(), "--audio-format".into(), "m4a".into()]);
            a.extend(["--audio-quality".into(), "0".into()]);
        }
    }
    a.extend([
        "--no-playlist".into(),
        "--no-progress".into(),
        // Stamp the file with the *download* time, not the video's upload date —
        // otherwise it sorts to the wrong place when the user sorts Downloads by
        // date (yt-dlp's default `--mtime` uses the metadata timestamp).
        "--no-mtime".into(),
        // Work around YouTube's "SABR streaming" format restriction on the
        // default web client (formats come back without URLs → "requested format
        // not available"). The ios / web_safari clients still expose real URLs.
        // YouTube-scoped, so it's a harmless no-op for the other platforms.
        "--extractor-args".into(),
        "youtube:player_client=default,ios,web_safari".into(),
        "--ffmpeg-location".into(),
        ffmpeg_dir.into(),
        "--print".into(),
        "after_move:filepath".into(),
        "-o".into(),
        out_template.into(),
        "--".into(),
        url.into(),
    ]);
    a
}

/// Browsers tried (in order) for `--cookies-from-browser` when YouTube's
/// anti-bot check blocks an anonymous download. Safari is omitted — macOS
/// sandboxing blocks reading its `Cookies.binarycookies` without Full Disk
/// Access. yt-dlp fast-fails on a browser that isn't installed, so the loop
/// just moves on.
const COOKIE_BROWSERS: &[&str] = &["chrome", "firefox", "brave", "edge"];

/// True if the yt-dlp error is YouTube's "confirm you're not a bot" gate, which
/// is bypassed by passing the user's logged-in browser cookies.
fn is_bot_block(stderr: &str) -> bool {
    let l = stderr.to_lowercase();
    l.contains("not a bot") || l.contains("sign in to confirm") || l.contains("--cookies-from-browser")
}

/// Run yt-dlp once. On success returns the produced file path (from
/// `--print after_move:filepath`); on failure returns the full stderr.
fn run_ytdlp(yt: &Path, args: &[String]) -> Result<PathBuf, String> {
    let out = Command::new(yt).args(args).output().map_err(|e| format!("yt-dlp: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| PathBuf::from(l.trim()))
        .filter(|p| p.is_file())
        .ok_or_else(|| "download produced no file".into())
}

/// Download `url` (video or audio) into `dir`. Returns the produced file path.
/// Falls back to browser cookies if YouTube's anti-bot check blocks the
/// anonymous request.
pub fn download(url: &str, mode: DlMode, dir: &Path) -> Result<PathBuf, String> {
    let u = url.trim();
    if detect_platform(u).is_none() {
        return Err("not a supported social-media URL".into());
    }
    let yt = yt_dlp_path().ok_or_else(|| ERR_NO_YTDLP.to_string())?;
    let ffmpeg = crate::screen_record::ffmpeg_path().ok_or_else(|| ERR_NO_FFMPEG.to_string())?;
    let ffmpeg_dir = ffmpeg.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    std::fs::create_dir_all(dir).map_err(|e| format!("create dir: {e}"))?;
    let template = dir.join("%(title).100B [%(id)s].%(ext)s");
    let base = build_dl_args(u, mode, &ffmpeg_dir, &template.to_string_lossy());

    match run_ytdlp(&yt, &base) {
        Ok(p) => Ok(p),
        Err(stderr) if is_bot_block(&stderr) => {
            // Retry with each browser's cookies; first success wins.
            for br in COOKIE_BROWSERS {
                let mut args = vec!["--cookies-from-browser".to_string(), (*br).to_string()];
                args.extend(base.iter().cloned());
                if let Ok(p) = run_ytdlp(&yt, &args) {
                    return Ok(p);
                }
            }
            Err("YouTube blocked the download with its anti-bot check, and no usable \
                 browser cookies were found. Log into YouTube in Chrome or Firefox and retry."
                .into())
        }
        Err(stderr) => Err(format!("download failed: {}", stderr.lines().last().unwrap_or("unknown error"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_each_platform() {
        assert_eq!(detect_platform("https://www.youtube.com/watch?v=x"), Some(Platform::YouTube));
        assert_eq!(detect_platform("https://youtu.be/x"), Some(Platform::YouTube));
        assert_eq!(detect_platform("https://www.instagram.com/reel/x/"), Some(Platform::Instagram));
        assert_eq!(detect_platform("https://www.tiktok.com/@u/video/123"), Some(Platform::TikTok));
        assert_eq!(detect_platform("https://www.facebook.com/watch/?v=1"), Some(Platform::Facebook));
        assert_eq!(detect_platform("https://fb.watch/abc/"), Some(Platform::Facebook));
    }

    #[test]
    fn rejects_non_social_and_non_http() {
        assert_eq!(detect_platform("https://example.com/x"), None);
        assert_eq!(detect_platform("youtube.com/x"), None); // no scheme
        assert_eq!(detect_platform("-x"), None);
        assert_eq!(detect_platform("file:///etc/passwd"), None);
    }

    #[test]
    fn video_args_merge_to_mp4_with_guard() {
        let a = build_dl_args("https://youtu.be/x", DlMode::Video, "/opt/homebrew/bin", "/d/%(id)s.%(ext)s");
        // prefer H.264 (Mac-playable) via sort, not a raw best-video filter
        assert!(a.windows(2).any(|w| w[0] == "-S" && w[1] == "vcodec:h264,res,acodec:m4a"));
        assert!(a.windows(2).any(|w| w[0] == "--merge-output-format" && w[1] == "mp4"));
        assert!(a.windows(2).any(|w| w[0] == "--ffmpeg-location" && w[1] == "/opt/homebrew/bin"));
        // `--` immediately precedes the URL (argv flag-smuggling guard).
        assert_eq!(a[a.len() - 2], "--");
        assert_eq!(a.last().unwrap(), "https://youtu.be/x");
        // download time, not the video's upload date
        assert!(a.contains(&"--no-mtime".to_string()));
        // SABR workaround
        assert!(a.windows(2).any(|w| w[0] == "--extractor-args"
            && w[1] == "youtube:player_client=default,ios,web_safari"));
    }

    #[test]
    fn audio_args_extract_m4a() {
        let a = build_dl_args("https://youtu.be/x", DlMode::Audio, "/d", "/d/%(id)s.%(ext)s");
        assert!(a.contains(&"-x".to_string()));
        assert!(a.windows(2).any(|w| w[0] == "--audio-format" && w[1] == "m4a"));
        assert!(!a.windows(2).any(|w| w[0] == "--merge-output-format"));
    }

    #[test]
    fn detects_youtube_bot_block() {
        let real = "ERROR: [youtube] -fWw7FE9tTo: Sign in to confirm you're not a bot. \
                    Use --cookies-from-browser or --cookies for the authentication.";
        assert!(is_bot_block(real));
        assert!(is_bot_block("Please use --cookies-from-browser"));
        assert!(!is_bot_block("ERROR: Requested format is not available"));
        assert!(!is_bot_block("Video unavailable"));
    }

    #[test]
    fn download_rejects_non_social_before_spawn() {
        assert!(download("https://example.com/x", DlMode::Video, Path::new("/tmp")).is_err());
        assert!(download("-x", DlMode::Audio, Path::new("/tmp")).is_err());
    }
}
