//! Export for video platforms: presets (YouTube, Shorts, Reels, TikTok), the bitrate a quality
//! level means for a frame size, duration limits and the voice level. Pure — the editor's
//! export dialog, `record.export` and the tests all read the same table.

use serde::{Deserialize, Serialize};

use super::editor::{Aspect, EditDoc, OutputTarget, TrackKind};
use record_render_protocol::Codec;

/// How much data the video gets. `Auto` leaves the choice to the compositor (the behaviour
/// before quality levels existed); `Custom` uses `bitrate_mbps` from the options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    #[default]
    Auto,
    Small,
    Standard,
    High,
    Max,
    Custom,
}

/// Target level of the voice. `L16` is what the compositor does on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Loudness {
    #[default]
    #[serde(rename = "-16")]
    L16,
    #[serde(rename = "-14")]
    L14,
}

impl Loudness {
    /// dB on top of the compositor's own −16 LUFS levelling.
    pub fn offset_db(self) -> f64 {
        match self {
            Loudness::L16 => 0.0,
            Loudness::L14 => 2.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Youtube,
    Shorts,
    Reels,
    Tiktok,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubFormat {
    Srt,
    Vtt,
}

impl SubFormat {
    pub fn ext(self) -> &'static str {
        match self {
            SubFormat::Srt => "srt",
            SubFormat::Vtt => "vtt",
        }
    }
}

/// What one platform wants. Sizes are fixed by the platforms' players; the limits and levels
/// are their published upload guidance (checked 2026-11) and may move — they only drive
/// defaults and warnings, never a refusal.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformSpec {
    pub id: Platform,
    pub aspect: Aspect,
    pub target: OutputTarget,
    pub fps: u32,
    pub quality: Quality,
    pub loudness: Loudness,
    /// Longer videos are refused or cut by the platform.
    pub max_duration_s: Option<u64>,
    /// Longer videos are accepted but leave the short-video feed.
    pub soft_duration_s: Option<u64>,
    /// Appended to the exported file's name.
    pub suffix: &'static str,
}

const PRESETS: [PlatformSpec; 4] = [
    PlatformSpec {
        id: Platform::Youtube,
        aspect: Aspect::W16H9,
        target: OutputTarget::P1080,
        fps: 30,
        quality: Quality::Standard,
        loudness: Loudness::L14,
        max_duration_s: None,
        soft_duration_s: None,
        suffix: "youtube",
    },
    PlatformSpec {
        id: Platform::Shorts,
        aspect: Aspect::W9H16,
        target: OutputTarget::P1080,
        fps: 30,
        quality: Quality::High,
        loudness: Loudness::L14,
        max_duration_s: Some(180),
        soft_duration_s: None,
        suffix: "shorts",
    },
    PlatformSpec {
        id: Platform::Reels,
        aspect: Aspect::W9H16,
        target: OutputTarget::P1080,
        fps: 30,
        quality: Quality::High,
        loudness: Loudness::L14,
        max_duration_s: Some(900),
        soft_duration_s: Some(180),
        suffix: "reels",
    },
    PlatformSpec {
        id: Platform::Tiktok,
        aspect: Aspect::W9H16,
        target: OutputTarget::P1080,
        fps: 30,
        quality: Quality::High,
        loudness: Loudness::L14,
        max_duration_s: Some(3600),
        soft_duration_s: Some(600),
        suffix: "tiktok",
    },
];

pub fn platform_presets() -> &'static [PlatformSpec] {
    &PRESETS
}

pub fn preset(id: Platform) -> &'static PlatformSpec {
    PRESETS.iter().find(|p| p.id == id).expect("every platform has a preset")
}

/// Frame rates an MP4 export offers.
pub const MP4_FPS: [u32; 3] = [24, 30, 60];

pub const BITRATE_MIN: u64 = 2_000_000;
pub const BITRATE_MAX: u64 = 80_000_000;
/// The range of a custom bitrate in Mbit/s.
pub const CUSTOM_MBPS: (f64, f64) = (2.0, 80.0);

/// Bits per pixel per frame at 30 fps for a quality level (H.264).
fn bits_per_pixel(q: Quality) -> Option<f64> {
    match q {
        Quality::Auto | Quality::Custom => None,
        Quality::Small => Some(0.07),
        Quality::Standard => Some(0.13),
        Quality::High => Some(0.20),
        Quality::Max => Some(0.30),
    }
}

/// Video bitrate in bits per second, or `None` when the compositor decides (`Auto`).
/// `q · w · h · 30 · (fps/30)^0.585` — 60 fps costs about 1.5× of 30, not 2× — and HEVC needs
/// about two thirds of H.264. Clamped to [`BITRATE_MIN`]..[`BITRATE_MAX`].
pub fn video_bitrate(w: u32, h: u32, fps: u32, codec: Codec, quality: Quality, custom_mbps: Option<f64>) -> Option<u64> {
    let raw = match quality {
        Quality::Auto => return None,
        Quality::Custom => {
            let m = custom_mbps.filter(|m| m.is_finite())?;
            m.clamp(CUSTOM_MBPS.0, CUSTOM_MBPS.1) * 1_000_000.0
        }
        q => {
            let bpp = bits_per_pixel(q)?;
            let rate = (fps.max(1) as f64 / 30.0).powf(0.585);
            let codec = if codec == Codec::Hevc { 0.65 } else { 1.0 };
            bpp * w as f64 * h as f64 * 30.0 * rate * codec
        }
    };
    Some((raw.round() as u64).clamp(BITRATE_MIN, BITRATE_MAX))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DurationWarning {
    /// Over the platform's hard limit.
    TooLong,
    /// Accepted, but no longer a short video there.
    Long,
}

pub fn duration_warning(spec: &PlatformSpec, programme_ms: u64) -> Option<(DurationWarning, u64)> {
    let over = |limit: Option<u64>| limit.filter(|l| programme_ms > l * 1000);
    if let Some(l) = over(spec.max_duration_s) {
        return Some((DurationWarning::TooLong, l));
    }
    over(spec.soft_duration_s).map(|l| (DurationWarning::Long, l))
}

/// H.264 hardware encoding ends at 4096 px on the long side and 2304 on the short one
/// (level 5.2); anything larger needs HEVC.
pub fn needs_hevc(w: u32, h: u32) -> bool {
    w.max(h) > 4096 || w.min(h) > 2304
}

/// How far `out` enlarges a `source` picture (`None` when it does not).
pub fn upscale(source: (u32, u32), out: (u32, u32)) -> Option<f64> {
    let f = (out.0 as f64 / source.0.max(1) as f64).max(out.1 as f64 / source.1.max(1) as f64);
    (f > 1.05).then_some((f * 10.0).round() / 10.0)
}

/// `doc` with the voice raised for `loudness`. The compositor levels every voice file to
/// −16 LUFS on its own; a higher target is that plus a fixed gain on the microphone and the
/// voiceovers — and on the system sound when the take has no microphone (then that is what
/// gets levelled). Music keeps its level.
pub fn with_loudness(doc: &EditDoc, loudness: Loudness, has_mic: bool) -> EditDoc {
    let db = loudness.offset_db();
    let mut d = doc.clone();
    if db == 0.0 {
        return d;
    }
    d.mic_gain_db += db;
    for t in d.audio_tracks.iter_mut().filter(|t| t.kind == TrackKind::Voiceover) {
        t.gain_db += db;
    }
    if !has_mic {
        d.system_gain_db += db;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::editor::aspect_output_size;
    use crate::record::editor::tests::meta;

    #[test]
    fn standard_matches_the_usual_upload_rates() {
        let b = |w, h, fps, c, q| video_bitrate(w, h, fps, c, q, None).unwrap() as f64 / 1e6;
        assert!((b(1920, 1080, 30, Codec::H264, Quality::Standard) - 8.1).abs() < 0.1);
        assert!((b(1920, 1080, 60, Codec::H264, Quality::Standard) - 12.1).abs() < 0.2);
        assert!((b(3840, 2160, 30, Codec::H264, Quality::Standard) - 32.3).abs() < 0.3);
        // Portrait costs the same as landscape of the same size.
        assert_eq!(
            video_bitrate(1080, 1920, 30, Codec::H264, Quality::High, None),
            video_bitrate(1920, 1080, 30, Codec::H264, Quality::High, None)
        );
    }

    #[test]
    fn quality_levels_rise_and_hevc_needs_less() {
        let at = |q| video_bitrate(1920, 1080, 30, Codec::H264, q, None).unwrap();
        assert!(at(Quality::Small) < at(Quality::Standard));
        assert!(at(Quality::Standard) < at(Quality::High));
        assert!(at(Quality::High) < at(Quality::Max));
        let hevc = video_bitrate(1920, 1080, 30, Codec::Hevc, Quality::High, None).unwrap();
        assert!(hevc < at(Quality::High) && hevc > at(Quality::High) / 2);
    }

    #[test]
    fn auto_leaves_the_bitrate_to_the_compositor() {
        assert_eq!(video_bitrate(1920, 1080, 30, Codec::H264, Quality::Auto, Some(20.0)), None);
    }

    #[test]
    fn custom_is_clamped_and_needs_a_number() {
        let c = |m| video_bitrate(1920, 1080, 30, Codec::H264, Quality::Custom, m);
        assert_eq!(c(Some(12.5)), Some(12_500_000));
        assert_eq!(c(Some(0.1)), Some(BITRATE_MIN));
        assert_eq!(c(Some(900.0)), Some(BITRATE_MAX));
        assert_eq!(c(None), None);
        assert_eq!(c(Some(f64::NAN)), None);
    }

    #[test]
    fn tiny_and_huge_frames_stay_in_range() {
        assert_eq!(video_bitrate(320, 180, 24, Codec::Hevc, Quality::Small, None), Some(BITRATE_MIN));
        assert_eq!(video_bitrate(7680, 4320, 60, Codec::H264, Quality::Max, None), Some(BITRATE_MAX));
    }

    #[test]
    fn every_preset_is_exportable() {
        let m = meta(10_000);
        let mut seen = std::collections::HashSet::new();
        for p in platform_presets() {
            assert!(seen.insert(p.id), "one preset per platform");
            assert!(MP4_FPS.contains(&p.fps));
            let (w, h) = aspect_output_size(&m, p.aspect, p.target);
            assert!(w % 2 == 0 && h % 2 == 0);
            assert!(!needs_hevc(w, h), "a preset must not force a codec");
            assert_ne!(p.quality, Quality::Auto);
            assert!(!p.suffix.is_empty() && p.suffix.chars().all(|c| c.is_ascii_lowercase()));
            assert_eq!(preset(p.id), p);
            if let (Some(soft), Some(hard)) = (p.soft_duration_s, p.max_duration_s) {
                assert!(soft < hard);
            }
        }
        assert_eq!(aspect_output_size(&m, preset(Platform::Shorts).aspect, OutputTarget::P1080), (1080, 1920));
        assert_eq!(aspect_output_size(&m, preset(Platform::Youtube).aspect, OutputTarget::P1080), (1920, 1080));
    }

    #[test]
    fn duration_limits() {
        let shorts = preset(Platform::Shorts);
        assert_eq!(duration_warning(shorts, 180_000), None);
        assert_eq!(duration_warning(shorts, 180_001), Some((DurationWarning::TooLong, 180)));
        let reels = preset(Platform::Reels);
        assert_eq!(duration_warning(reels, 200_000), Some((DurationWarning::Long, 180)));
        assert_eq!(duration_warning(reels, 901_000), Some((DurationWarning::TooLong, 900)));
        assert_eq!(duration_warning(preset(Platform::Youtube), u64::MAX / 2), None);
    }

    #[test]
    fn hevc_only_beyond_the_h264_limit() {
        assert!(!needs_hevc(3840, 2160));
        assert!(!needs_hevc(2160, 3840));
        assert!(!needs_hevc(4096, 2304));
        assert!(needs_hevc(4098, 2304));
        assert!(needs_hevc(2880, 5120));
    }

    #[test]
    fn upscale_is_named_only_when_it_happens() {
        assert_eq!(upscale((1920, 1080), (1920, 1080)), None);
        assert_eq!(upscale((1920, 1080), (1280, 720)), None);
        assert_eq!(upscale((1920, 1080), (3840, 2160)), Some(2.0));
        assert_eq!(upscale((1920, 1080), (1080, 1920)), Some(1.8));
    }

    #[test]
    fn louder_voice_is_a_gain_on_the_voice_only() {
        let mut d: EditDoc =
            serde_json::from_value(serde_json::json!({"version": 6, "clips": [{"startMs": 0, "endMs": 1000}]})).unwrap();
        d.mic_gain_db = -3.0;
        d.system_gain_db = 1.0;
        let same = with_loudness(&d, Loudness::L16, true);
        assert_eq!(same, d, "-16 is the compositor's own level: nothing changes");
        let up = with_loudness(&d, Loudness::L14, true);
        assert_eq!(up.mic_gain_db, -1.0);
        assert_eq!(up.system_gain_db, 1.0, "with a microphone the system sound keeps its level");
        let no_mic = with_loudness(&d, Loudness::L14, false);
        assert_eq!(no_mic.system_gain_db, 3.0);
    }

    #[test]
    fn option_names_on_the_wire() {
        assert_eq!(serde_json::to_string(&Loudness::L14).unwrap(), "\"-14\"");
        assert_eq!(serde_json::to_string(&Quality::Standard).unwrap(), "\"standard\"");
        assert_eq!(serde_json::to_string(&Platform::Tiktok).unwrap(), "\"tiktok\"");
        assert_eq!(serde_json::to_string(&SubFormat::Vtt).unwrap(), "\"vtt\"");
    }
}
