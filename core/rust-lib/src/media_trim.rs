//! Trim a local audio/video file — the `trim` search-bar command. Pick a file,
//! set start/end, and either cut **lossless & fast** (`-c copy`, snaps to the
//! nearest keyframe) or **frame-accurate** (re-encode). Output is a sibling
//! `<stem>-trim.<ext>` (non-destructive), revealed in Finder.
//!
//! Engine: ffmpeg. The pure argv builder + output-path logic are unit-tested.

use std::path::{Path, PathBuf};
use std::process::Command;

pub use crate::audio_swap::ERR_NO_FFMPEG;

fn fmt(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".into() } else { s.to_string() }
}

/// The trimmed-output path: `<stem>-trim.<ext>`. Lossless keeps the source
/// container; a re-encode normalises to mp4 (video) / m4a (audio).
pub fn output_path_for(input: &Path, lossless: bool, is_video: bool) -> PathBuf {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("clip");
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let ext = if lossless {
        input.extension().and_then(|e| e.to_str()).unwrap_or("mp4").to_string()
    } else if is_video {
        "mp4".into()
    } else {
        "m4a".into()
    };
    parent.join(format!("{stem}-trim.{ext}"))
}

/// Build the ffmpeg argv to cut `input` to `[start, end]` (seconds).
///
/// * `lossless` → input-side seek + stream copy (fast, no quality loss, but the
///   cut snaps to the nearest keyframe so the start can be up to ~1 s early).
/// * otherwise → output-side seek + re-encode (frame-accurate, slower). Video is
///   `libx264`; audio is AAC 256 k. Pure.
pub fn build_trim_args(
    input: &str,
    start_s: f64,
    end_s: f64,
    lossless: bool,
    is_video: bool,
    out: &str,
) -> Vec<String> {
    let mut a: Vec<String> = vec!["-y".into(), "-hide_banner".into()];
    let start = fmt(start_s.max(0.0));
    let end = fmt(end_s.max(0.0));

    if lossless {
        // Seek before -i (fast); -c copy keeps both streams as-is.
        a.extend(["-ss".into(), start, "-to".into(), end, "-i".into(), input.into()]);
        a.extend(["-c".into(), "copy".into()]);
    } else {
        // Seek after -i for frame-accuracy; re-encode.
        a.extend(["-i".into(), input.into(), "-ss".into(), start, "-to".into(), end]);
        if is_video {
            a.extend([
                "-c:v".into(), "libx264".into(),
                "-preset".into(), "veryfast".into(),
                "-pix_fmt".into(), "yuv420p".into(),
            ]);
        }
        a.extend(["-c:a".into(), "aac".into(), "-b:a".into(), "256k".into()]);
    }
    a.extend(["-movflags".into(), "+faststart".into(), out.into()]);
    a
}

/// Whether `file` has a video stream (drives codec choice + the preview).
pub fn has_video_stream(file: &Path) -> bool {
    let Some(ffmpeg) = crate::screen_record::ffmpeg_path() else { return false };
    let Some(ffprobe) = ffprobe_path(&ffmpeg) else { return false };
    Command::new(ffprobe)
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=codec_type", "-of", "csv=p=0"])
        .arg(file)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("video"))
        .unwrap_or(false)
}

fn ffprobe_path(ffmpeg: &Path) -> Option<PathBuf> {
    let exe = if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" };
    ffmpeg.parent().map(|d| d.join(exe)).filter(|p| p.is_file())
}

/// Trim `input` to `[start, end]`, writing the sibling output. Returns its path.
pub fn apply_trim(input: &Path, start_s: f64, end_s: f64, lossless: bool) -> Result<PathBuf, String> {
    let ffmpeg = crate::screen_record::ffmpeg_path().ok_or_else(|| ERR_NO_FFMPEG.to_string())?;
    if end_s <= start_s {
        return Err("end must be after start".into());
    }
    let is_video = has_video_stream(input);
    let out = output_path_for(input, lossless, is_video);
    let args = build_trim_args(
        &input.to_string_lossy(),
        start_s,
        end_s,
        lossless,
        is_video,
        &out.to_string_lossy(),
    );
    let result = Command::new(&ffmpeg).args(&args).output().map_err(|e| format!("ffmpeg: {e}"))?;
    if !result.status.success() {
        let err = String::from_utf8_lossy(&result.stderr);
        return Err(format!("trim failed: {}", err.lines().last().unwrap_or("unknown error")));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossless_keeps_container_and_copies() {
        let args = build_trim_args("/v/clip.mkv", 5.0, 12.0, true, true, "/v/clip-trim.mkv");
        let j = args.join(" ");
        // input-side seek (before -i) + stream copy
        assert!(j.contains("-ss 5 -to 12 -i /v/clip.mkv"));
        assert!(j.contains("-c copy"));
        assert!(!j.contains("libx264"));
    }

    #[test]
    fn accurate_video_reencodes_after_input_seek() {
        let args = build_trim_args("/v/clip.mov", 1.5, 4.0, false, true, "/v/clip-trim.mp4");
        let j = args.join(" ");
        // output-side seek (after -i) for frame accuracy
        assert!(j.contains("-i /v/clip.mov -ss 1.5 -to 4"));
        assert!(j.contains("-c:v libx264"));
        assert!(j.contains("-c:a aac"));
    }

    #[test]
    fn accurate_audio_only_has_no_video_codec() {
        let args = build_trim_args("/a/song.mp3", 0.0, 30.0, false, false, "/a/song-trim.m4a");
        let j = args.join(" ");
        assert!(!j.contains("-c:v"));
        assert!(j.contains("-c:a aac"));
    }

    #[test]
    fn both_modes_set_faststart_and_target_out() {
        for lossless in [true, false] {
            let a = build_trim_args("/v/clip.mp4", 0.0, 5.0, lossless, true, "/v/out.mp4");
            let j = a.join(" ");
            assert!(j.contains("-movflags +faststart"), "faststart missing (lossless={lossless})");
            assert_eq!(a.last().unwrap(), "/v/out.mp4");
            assert_eq!(a[0], "-y");
        }
    }

    #[test]
    fn lossless_audio_keeps_container_and_copies() {
        let a = build_trim_args("/a/song.mp3", 10.0, 40.0, true, false, "/a/song-trim.mp3");
        let j = a.join(" ");
        assert!(j.contains("-ss 10 -to 40 -i /a/song.mp3"));
        assert!(j.contains("-c copy"));
        assert!(!j.contains("-c:a aac")); // no re-encode in lossless mode
    }

    #[test]
    fn trim_times_format_without_trailing_zeros() {
        let a = build_trim_args("/v/c.mp4", 1.5, 12.0, true, true, "/v/o.mp4").join(" ");
        assert!(a.contains("-ss 1.5 -to 12 ")); // 12.0 → "12", 1.5 stays
    }

    #[test]
    fn output_paths() {
        assert_eq!(output_path_for(Path::new("/v/My Clip.mkv"), true, true), PathBuf::from("/v/My Clip-trim.mkv"));
        assert_eq!(output_path_for(Path::new("/v/My Clip.mkv"), false, true), PathBuf::from("/v/My Clip-trim.mp4"));
        assert_eq!(output_path_for(Path::new("/a/song.wav"), false, false), PathBuf::from("/a/song-trim.m4a"));
    }

    #[test]
    fn output_path_without_extension_defaults_to_mp4() {
        // Lossless "keep the container" on an extension-less input still needs
        // a valid ext for ffmpeg's muxer choice.
        assert_eq!(
            output_path_for(Path::new("/v/rawclip"), true, true),
            PathBuf::from("/v/rawclip-trim.mp4")
        );
    }

    #[test]
    fn output_path_preserves_unicode_stem() {
        assert_eq!(
            output_path_for(Path::new("/v/Grüße Video.mov"), true, true),
            PathBuf::from("/v/Grüße Video-trim.mov")
        );
    }

    #[test]
    fn negative_times_clamp_to_zero() {
        // A slider glitch / bad IPC arg must never produce a negative seek —
        // ffmpeg would interpret "-3" as an option.
        let j = build_trim_args("/v/c.mp4", -3.0, -1.0, true, true, "/v/o.mp4").join(" ");
        assert!(j.contains("-ss 0 -to 0"), "got: {j}");
    }

    #[test]
    fn fmt_strips_trailing_zeros_and_keeps_millis() {
        assert_eq!(fmt(0.0), "0");
        assert_eq!(fmt(2.0), "2");
        assert_eq!(fmt(1.5), "1.5");
        assert_eq!(fmt(2.125), "2.125");
        assert_eq!(fmt(0.001), "0.001");
        // Sub-millisecond input rounds to millisecond precision.
        assert_eq!(fmt(1.23456), "1.235");
        // Rounds down to a clean integer when the fraction vanishes.
        assert_eq!(fmt(9.9999), "10");
    }

    #[test]
    fn paths_with_spaces_stay_single_argv_elements() {
        // The args are handed to Command::args element-wise — a path with
        // spaces must be ONE element, never pre-split (join(" ") in the other
        // tests is only a readability aid).
        let args = build_trim_args(
            "/v/My Holiday Clip.mp4", 1.0, 2.0, true, true, "/v/My Holiday Clip-trim.mp4",
        );
        assert!(args.contains(&"/v/My Holiday Clip.mp4".to_string()));
        assert_eq!(args.last().unwrap(), "/v/My Holiday Clip-trim.mp4");
    }

    #[test]
    fn lossless_output_keeps_extension_case_verbatim() {
        // "Keep the container" means keeping the ext exactly as the user has it
        // (macOS filesystems are case-preserving) — no silent lowercase.
        assert_eq!(
            output_path_for(Path::new("/v/clip.MOV"), true, true),
            PathBuf::from("/v/clip-trim.MOV")
        );
    }

    #[test]
    fn dotfiles_and_stemless_paths_still_yield_a_sane_output_name() {
        // A dotfile keeps its full (dot-)name as the stem…
        assert_eq!(
            output_path_for(Path::new("/v/.hidden"), false, true),
            PathBuf::from("/v/.hidden-trim.mp4")
        );
        // …and a path with no file name at all falls back to the "clip" stem
        // instead of producing an empty "-trim.mp4".
        let out = output_path_for(Path::new("/"), false, true);
        assert_eq!(out.file_name().unwrap().to_string_lossy(), "clip-trim.mp4");
    }

    #[test]
    fn lossless_seek_precedes_the_input_flag_in_argv_order() {
        // The fast-seek contract is positional: -ss/-to BEFORE -i. Assert on
        // actual argv indices, not substring order in a joined string.
        let a = build_trim_args("/v/c.mp4", 3.0, 9.0, true, true, "/v/o.mp4");
        let idx = |s: &str| a.iter().position(|x| x == s).unwrap_or_else(|| panic!("{s} missing"));
        assert!(idx("-ss") < idx("-i"), "lossless: input-side seek");
        let b = build_trim_args("/v/c.mp4", 3.0, 9.0, false, true, "/v/o.mp4");
        let idxb = |s: &str| b.iter().position(|x| x == s).unwrap_or_else(|| panic!("{s} missing"));
        assert!(idxb("-i") < idxb("-ss"), "accurate: output-side seek");
    }

    #[test]
    fn accurate_mode_ends_with_output_and_no_stream_copy() {
        let a = build_trim_args("/v/clip.mp4", 2.0, 8.0, false, true, "/v/clip-trim.mp4");
        assert_eq!(a.last().unwrap(), "/v/clip-trim.mp4");
        let j = a.join(" ");
        assert!(!j.contains("-c copy"), "re-encode mode must not stream-copy");
        assert!(j.contains("-preset veryfast"));
        assert!(j.contains("-pix_fmt yuv420p"));
    }
}
