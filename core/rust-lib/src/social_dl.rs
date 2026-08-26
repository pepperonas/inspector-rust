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
    Dailymotion,
}

impl Platform {
    pub fn display_name(self) -> &'static str {
        match self {
            Platform::YouTube => "YouTube",
            Platform::Instagram => "Instagram",
            Platform::TikTok => "TikTok",
            Platform::Facebook => "Facebook",
            Platform::Dailymotion => "Dailymotion",
        }
    }
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
    } else if u.contains("dailymotion.com") || u.contains("dai.ly") {
        // `dai.ly` is Dailymotion's own short form — recognising only the long
        // host would silently refuse half the links people actually share.
        Some(Platform::Dailymotion)
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

/// Build the yt-dlp argv. The `--print after_move:…` marker line carries the
/// final path + naming metadata on stdout; `--` ends option parsing so a `-…`
/// URL can't smuggle a flag. Pure.
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
        // One marker line, unit-separator-delimited: final path + the metadata
        // the smart renamer needs (`|` defaults keep absent fields EMPTY, not
        // "NA"). Parsed by `run_ytdlp`; `media_name::smart_stem` does the rest.
        "after_move:IRMETA\u{1f}%(filepath)s\u{1f}%(artist,creator|)s\u{1f}%(track|)s\u{1f}%(title|)s\u{1f}%(uploader|)s\u{1f}%(release_year|)s".into(),
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
/// Does the yt-dlp failure look like an auth / anti-bot wall that browser
/// cookies could get past? Covers YouTube's "not a bot" check **and** the
/// login/region walls common on TikTok / Instagram / Facebook (yt-dlp suggests
/// `--cookies(-from-browser)` for those too).
fn is_bot_block(stderr: &str) -> bool {
    let l = stderr.to_lowercase();
    l.contains("not a bot")
        || l.contains("sign in to confirm")
        || l.contains("--cookies-from-browser")
        || l.contains("--cookies")
        || l.contains("login required")
        || l.contains("requires authentication")
        || l.contains("you need to log in")
        || l.contains("log in to")
}

/// Signatures of YouTube breakage that a **yt-dlp update** usually fixes
/// (extraction/format failures) or that are transient rate-limiting — either
/// way the user's action is "update yt-dlp / wait", so we append a hint. Pure.
pub fn looks_stale_or_rate_limited(stderr: &str) -> bool {
    let l = stderr.to_lowercase();
    // "isn't"/"isn’t" (straight + curly apostrophe both appear in yt-dlp output)
    l.contains("content isn't available")
        || l.contains("content isn\u{2019}t available")
        || l.contains("requested format is not available")
        || l.contains("nsig extraction failed")
        || l.contains("signature extraction failed")
        || l.contains("rate-limited")
        || l.contains("please report this issue on https://github.com/yt-dlp")
}

/// Parse the `IRMETA`-marked print line into (path, naming metadata). Pure.
fn parse_meta_line(stdout: &str) -> Option<(PathBuf, crate::media_name::MediaMeta)> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with("IRMETA\u{1f}"))?;
    let parts: Vec<&str> = line.trim_start().split('\u{1f}').collect();
    if parts.len() < 7 {
        return None;
    }
    Some((
        PathBuf::from(parts[1].trim()),
        crate::media_name::MediaMeta {
            artist: parts[2].trim().to_string(),
            track: parts[3].trim().to_string(),
            title: parts[4].trim().to_string(),
            uploader: parts[5].trim().to_string(),
            release_year: parts[6].trim().to_string(),
        },
    ))
}

/// Run yt-dlp once. On success returns the produced file path + the naming
/// metadata from the marker line; on failure returns the full stderr.
fn run_ytdlp(
    yt: &Path,
    args: &[String],
) -> Result<(PathBuf, Option<crate::media_name::MediaMeta>), String> {
    let out = Command::new(yt).args(args).output().map_err(|e| format!("yt-dlp: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    if let Some((p, meta)) = parse_meta_line(&stdout) {
        if p.is_file() {
            return Ok((p, Some(meta)));
        }
    }
    // Fallback: an older yt-dlp / unexpected output — last non-empty line.
    stdout
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(|l| PathBuf::from(l.trim()))
        .filter(|p| p.is_file())
        .map(|p| (p, None))
        .ok_or_else(|| "download produced no file".into())
}

/// Rename the downloaded file to its smart library name (`Artist - Track
/// (Edition).ext`) — or at least strip the ` [videoid]` tail. Best-effort:
/// any failure keeps the original path (a finished download must never be
/// lost to a rename).
fn smart_rename(path: PathBuf, meta: Option<&crate::media_name::MediaMeta>) -> PathBuf {
    let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else {
        return path;
    };
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned());
    let target_stem = meta
        .and_then(crate::media_name::smart_stem)
        .unwrap_or_else(|| crate::media_name::strip_id_suffix(&stem));
    if target_stem.is_empty() || target_stem == stem {
        return path;
    }
    let Some(dir) = path.parent() else { return path };
    let with_ext = |s: &str| match &ext {
        Some(e) => dir.join(format!("{s}.{e}")),
        None => dir.join(s),
    };
    // Collision-safe: never overwrite an existing file.
    let mut target = with_ext(&target_stem);
    let mut n = 2;
    while target.exists() {
        target = with_ext(&format!("{target_stem} ({n})"));
        n += 1;
        if n > 50 {
            return path;
        }
    }
    match std::fs::rename(&path, &target) {
        Ok(()) => target,
        Err(_) => path,
    }
}

/// Download `url` (video or audio) into `dir`. Returns the produced file path.
/// Falls back to browser cookies if YouTube's anti-bot check blocks the
/// anonymous request.
pub fn download(url: &str, mode: DlMode, dir: &Path) -> Result<PathBuf, String> {
    let u = url.trim();
    let platform_name = match detect_platform(u) {
        Some(p) => p.display_name(),
        None => return Err("not a supported social-media URL".into()),
    };
    let yt = yt_dlp_path().ok_or_else(|| ERR_NO_YTDLP.to_string())?;
    let ffmpeg = crate::screen_record::ffmpeg_path().ok_or_else(|| ERR_NO_FFMPEG.to_string())?;
    let ffmpeg_dir = ffmpeg.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    std::fs::create_dir_all(dir).map_err(|e| format!("create dir: {e}"))?;
    let template = dir.join("%(title).100B [%(id)s].%(ext)s");
    let base = build_dl_args(u, mode, &ffmpeg_dir, &template.to_string_lossy());

    match run_ytdlp(&yt, &base) {
        Ok((p, meta)) => Ok(smart_rename(p, meta.as_ref())),
        Err(stderr) if is_bot_block(&stderr) => {
            // Retry with each browser's cookies; first success wins.
            for br in COOKIE_BROWSERS {
                let mut args = vec!["--cookies-from-browser".to_string(), (*br).to_string()];
                args.extend(base.iter().cloned());
                if let Ok((p, meta)) = run_ytdlp(&yt, &args) {
                    return Ok(smart_rename(p, meta.as_ref()));
                }
            }
            Err(format!(
                "{platform_name} requires you to be signed in (or blocked the download), and no \
                 usable browser cookies were found. Log into {platform_name} in Chrome or Firefox \
                 and retry."
            ))
        }
        Err(stderr) => {
            let last = stderr.lines().last().unwrap_or("unknown error");
            if looks_stale_or_rate_limited(&stderr) {
                // The #1 cause of YouTube extraction failures is an outdated
                // yt-dlp; the runner-up is a temporary rate-limit.
                Err(format!(
                    "download failed: {last}\n\nThis usually means yt-dlp is out of date \
                     (update it: `brew upgrade yt-dlp` or `yt-dlp -U`), or {platform_name} is \
                     temporarily rate-limiting you (wait a few minutes)."
                ))
            } else {
                Err(format!("download failed: {last}"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_dailymotion_including_its_short_host() {
        assert_eq!(
            detect_platform("https://www.dailymotion.com/video/x7xd3st"),
            Some(Platform::Dailymotion)
        );
        assert_eq!(detect_platform("https://dai.ly/x7xd3st"), Some(Platform::Dailymotion));
        assert_eq!(
            detect_platform("https://geo.dailymotion.com/player.html?video=x7xd3st"),
            Some(Platform::Dailymotion)
        );
        assert_eq!(Platform::Dailymotion.display_name(), "Dailymotion");
        // A lookalike host is not Dailymotion.
        assert_eq!(detect_platform("https://notdaily.example.com/x"), None);
    }

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
    fn detects_real_world_url_variants() {
        // shapes the apps actually produce
        assert_eq!(detect_platform("https://www.youtube.com/shorts/abc123"), Some(Platform::YouTube));
        assert_eq!(detect_platform("https://m.youtube.com/watch?v=x"), Some(Platform::YouTube));
        assert_eq!(detect_platform("https://youtu.be/x?t=42"), Some(Platform::YouTube));
        assert_eq!(detect_platform("https://www.instagram.com/p/DZYTeeHtPZL/"), Some(Platform::Instagram));
        assert_eq!(detect_platform("https://www.instagram.com/tv/x/"), Some(Platform::Instagram));
        assert_eq!(detect_platform("https://vm.tiktok.com/ZMabc/"), Some(Platform::TikTok));
        assert_eq!(detect_platform("https://www.facebook.com/reel/123"), Some(Platform::Facebook));
        assert_eq!(detect_platform("https://fb.com/watch/?v=1"), Some(Platform::Facebook));
    }

    #[test]
    fn audio_args_also_have_timestamp_and_sabr_flags() {
        let a = build_dl_args("https://youtu.be/x", DlMode::Audio, "/d", "/d/%(id)s.%(ext)s");
        assert!(a.contains(&"--no-mtime".to_string()), "audio must download with current time too");
        assert!(a.windows(2).any(|w| w[0] == "--extractor-args"
            && w[1] == "youtube:player_client=default,ios,web_safari"));
        assert_eq!(a[a.len() - 2], "--"); // flag-smuggling guard before the URL
    }

    #[test]
    fn cookie_browser_fallback_list_is_sane() {
        assert!(!COOKIE_BROWSERS.is_empty());
        assert_eq!(COOKIE_BROWSERS[0], "chrome"); // most common + most cookies on macOS
        assert!(!COOKIE_BROWSERS.contains(&"safari")); // sandbox-blocked, deliberately skipped
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
    fn stale_or_rate_limited_detection() {
        // The exact message from the field report (outdated yt-dlp → YouTube).
        let real = "ERROR: [youtube] O20XTW3X2-8: This content isn't available, try again \
                    later.. The current session has been rate-limited by YouTube for up to an hour.";
        assert!(looks_stale_or_rate_limited(real));
        assert!(looks_stale_or_rate_limited("ERROR: Requested format is not available"));
        assert!(looks_stale_or_rate_limited("nsig extraction failed: Some(...)"));
        // Curly apostrophe variant.
        assert!(looks_stale_or_rate_limited("This content isn\u{2019}t available, try again later"));
        // Not every failure — a genuine private/removed video isn't "update yt-dlp".
        assert!(!looks_stale_or_rate_limited("ERROR: Video unavailable. This video is private"));
        assert!(!looks_stale_or_rate_limited("ERROR: This video is not available in your country"));
    }

    #[test]
    fn auth_wall_detection_covers_tiktok_instagram_walls() {
        // Login/region walls on the non-YouTube platforms also trigger the
        // cookie retry now (yt-dlp suggests --cookies for those too).
        assert!(is_bot_block("ERROR: [tiktok] Login required to access this content"));
        assert!(is_bot_block("ERROR: [instagram] You need to log in to access this"));
        assert!(is_bot_block("This content requires authentication. Use --cookies"));
        // Still no false positive on a plain region/availability error.
        assert!(!is_bot_block("ERROR: This video is not available in your country"));
    }

    #[test]
    fn tiktok_downloads_as_h264_mp4_video() {
        // TikTok takes the same path as the others: H.264-preferred video → mp4.
        assert_eq!(
            detect_platform("https://www.tiktok.com/@u/video/123"),
            Some(Platform::TikTok)
        );
        let a = build_dl_args(
            "https://www.tiktok.com/@u/video/123",
            DlMode::Video,
            "/d",
            "/d/%(title)s.%(ext)s",
        );
        assert!(a.windows(2).any(|w| w[0] == "-S" && w[1].contains("h264")));
        assert!(a.windows(2).any(|w| w[0] == "--merge-output-format" && w[1] == "mp4"));
        assert_eq!(Platform::TikTok.display_name(), "TikTok");
    }

    #[test]
    fn download_rejects_non_social_before_spawn() {
        assert!(download("https://example.com/x", DlMode::Video, Path::new("/tmp")).is_err());
        assert!(download("-x", DlMode::Audio, Path::new("/tmp")).is_err());
    }
}
