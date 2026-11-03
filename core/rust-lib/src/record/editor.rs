//! The editor document (`project.json → editor`): cuts, speed regions, crop per clip,
//! output shape and mic gain — plus its normalisation and the mapping between
//! programme time (what the export plays) and source time (the recording).
//!
//! Pure: no compositor dependency. The mapping ports the math of the compositor's
//! `regions::speed_segments_for_window` (regions sorted by start, gaps play at 1×), so the
//! editor timeline and the export agree on where every frame lands. `view()` is the ONE
//! place the UI gets its timeline geometry from — there must be no second implementation.

use super::project::{Project, RecordingMeta};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

pub const EDITOR_VERSION: u32 = 4;
/// Clips shorter than this are dropped by `normalize` (a sliver nobody can see or grab).
pub const MIN_CLIP_MS: u64 = 100;
pub const MIN_SPEED: f64 = 0.25;
pub const MAX_SPEED: f64 = 16.0;
/// Smallest crop edge, as a fraction of the source.
pub const MIN_CROP_EDGE: f64 = 0.05;
pub const MIN_MIC_GAIN_DB: f64 = -60.0;
pub const MAX_MIC_GAIN_DB: f64 = 12.0;
/// Zoom depth 1..=6 → scale factor (OpenScreen v2.1.2 `zoom-scale.ts`).
pub const ZOOM_DEPTHS: [f64; 6] = [1.25, 1.5, 1.8, 2.2, 3.5, 5.0];
pub const DEFAULT_ZOOM_DEPTH: u8 = 3;
/// Zoom regions shorter than this are dropped by `normalize`.
pub const MIN_ZOOM_MS: u64 = 250;
/// Length of a freshly added zoom region.
pub const NEW_ZOOM_MS: u64 = 2000;
pub const MIN_ZOOM_SCALE: f64 = 1.0;
pub const MAX_ZOOM_SCALE: f64 = 5.0;
/// Our first own wallpaper — the default background of a new document.
pub const DEFAULT_WALLPAPER: &str = "01";
pub const DEFAULT_CURSOR_THEME: &str = "default";

// ── annotations + audio tracks (stage 4, OpenScreen v2.1.2 defaults) ──────────
/// The text fonts on offer (`textFonts.ts:13`); anything else becomes the first.
pub const TEXT_FONTS: [&str; 5] = ["Inter", "Lora", "Oswald", "Caveat", "IBM Plex Mono"];
/// A new annotation's box `(x, y, w, h)` in percent of the frame (`types.ts:425-433`).
pub const ANNOTATION_DEFAULT_RECT: (f64, f64, f64, f64) = (50.0, 50.0, 30.0, 20.0);
pub const FONT_SIZE_RANGE: (f64, f64) = (8.0, 200.0);
pub const DEFAULT_FONT_SIZE: f64 = 32.0;
pub const BLUR_INTENSITY_RANGE: (f64, f64) = (2.0, 40.0);
pub const BLUR_BLOCK_RANGE: (f64, f64) = (4.0, 48.0);
pub const DEFAULT_BLUR_INTENSITY: f64 = 12.0;
pub const DEFAULT_BLUR_BLOCK: f64 = 12.0;
pub const DEFAULT_TEXT_COLOR: &str = "#ffffff";
/// The plate a NEW text gets (`background.ts:20`); a stored one without a valid colour falls
/// back to `transparent` (the schema default).
pub const DEFAULT_TEXT_PLATE: &str = "rgba(0, 0, 0, 0.7)";
pub const DEFAULT_ARROW_COLOR: &str = "#34B27B";
pub const DEFAULT_ARROW_STROKE: f64 = 4.0;
/// The free-form blur shape a new (or broken) free form gets, percent of the box (`types.ts:453-463`).
pub const DEFAULT_FREEHAND_POINTS: [(f64, f64); 9] = [
    (10.0, 30.0),
    (25.0, 10.0),
    (55.0, 8.0),
    (82.0, 20.0),
    (90.0, 45.0),
    (78.0, 72.0),
    (52.0, 90.0),
    (22.0, 84.0),
    (8.0, 58.0),
];
pub const TRACK_GAIN_RANGE: (f64, f64) = (-60.0, 12.0);
pub const FADE_MAX_MS: u64 = 5000;
pub const SYSTEM_GAIN_RANGE: (f64, f64) = (-12.0, 12.0);
pub const MUSIC_DEFAULT_GAIN_DB: f64 = -18.0;
pub const MUSIC_DEFAULT_FADE_MS: u64 = 1000;
/// Highest `z_index` an annotation keeps (`max + 1` must never overflow).
pub const MAX_Z_INDEX: i32 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditClip {
    pub start_ms: u64,
    pub end_ms: u64,
    /// Which recording the clip plays: `None` = the project's own take, `Some(id)` = an
    /// inserted take under `<project>/takes/<id>/`.
    #[serde(default)]
    pub source: Option<String>,
}

impl EditClip {
    /// A clip of the project's own take.
    pub fn main(start_ms: u64, end_ms: u64) -> Self {
        EditClip { start_ms, end_ms, source: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedRegion {
    /// Index into `EditDoc::clips`. Times are SOURCE ms of that clip's recording.
    pub clip: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub speed: f64,
}

/// A crop rectangle in fractions (0..1) of the source frame, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CropRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Aspect {
    /// Missing `output` in a stored document (v1) means "as the source".
    #[default]
    #[serde(rename = "source")]
    Source,
    /// Standard of a NEW document; the real size is computed from the layout (Task 4).
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "16:9")]
    W16H9,
    #[serde(rename = "9:16")]
    W9H16,
    #[serde(rename = "1:1")]
    W1H1,
    #[serde(rename = "4:3")]
    W4H3,
    #[serde(rename = "3:4")]
    W3H4,
    #[serde(rename = "21:9")]
    W21H9,
}

impl Aspect {
    /// Width / height, or `None` for "as the source".
    pub fn ratio(self) -> Option<f64> {
        let (w, h) = match self {
            Aspect::Source | Aspect::Auto => return None,
            Aspect::W16H9 => (16.0, 9.0),
            Aspect::W9H16 => (9.0, 16.0),
            Aspect::W1H1 => (1.0, 1.0),
            Aspect::W4H3 => (4.0, 3.0),
            Aspect::W3H4 => (3.0, 4.0),
            Aspect::W21H9 => (21.0, 9.0),
        };
        Some(w / h)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputTarget {
    P720,
    P1080,
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OutputShape {
    pub aspect: Aspect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Background {
    Wallpaper { id: String },
    Gradient {
        #[serde(rename = "angleDeg")]
        angle_deg: f64,
        stops: Vec<String>,
    },
    Color { color: String },
    /// File relative to `<project>/assets`.
    Image { file: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WallpaperMotion {
    #[default]
    None,
    Drift,
    Aurora,
    Waves,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Frame {
    #[default]
    None,
    Window,
    Laptop,
    Phone,
    Monitor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FrameTheme {
    #[default]
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Appearance {
    pub background: Background,
    pub background_motion: WallpaperMotion,
    /// Percent of the frame, 0..=100.
    pub padding: f64,
    /// Corner radius in px, 0..=64.
    pub radius_px: f64,
    pub shadow: f64,
    pub background_blur: f64,
    pub motion_blur: f64,
    pub frame: Frame,
    pub frame_theme: FrameTheme,
}

impl Default for Appearance {
    /// OpenScreen's defaults with our own first wallpaper.
    fn default() -> Self {
        Appearance {
            background: Background::Wallpaper { id: DEFAULT_WALLPAPER.into() },
            background_motion: WallpaperMotion::None,
            padding: 50.0,
            radius_px: 40.0,
            shadow: 0.6,
            background_blur: 0.0,
            motion_blur: 0.2,
            frame: Frame::None,
            frame_theme: FrameTheme::Light,
        }
    }
}

impl Appearance {
    /// The look of a v1 document: black, edge to edge, no effects.
    pub fn plain() -> Self {
        Appearance {
            background: Background::Color { color: "#000000".into() },
            background_motion: WallpaperMotion::None,
            padding: 0.0,
            radius_px: 0.0,
            shadow: 0.0,
            background_blur: 0.0,
            motion_blur: 0.0,
            frame: Frame::None,
            frame_theme: FrameTheme::Light,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomCursor {
    /// File relative to `<project>/assets`.
    pub file: String,
    pub hotspot_x: f64,
    pub hotspot_y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CursorStyle {
    pub show: bool,
    pub auto_hide: bool,
    pub theme: String,
    pub custom: Option<CustomCursor>,
    pub size: f64,
    pub smoothing: f64,
    pub motion_blur: f64,
    pub click_bounce: f64,
    pub click_impact: bool,
    pub click_sound: bool,
    pub click_sound_gain_db: f64,
}

impl Default for CursorStyle {
    fn default() -> Self {
        CursorStyle {
            show: true,
            auto_hide: false,
            theme: DEFAULT_CURSOR_THEME.into(),
            custom: None,
            size: 3.6,
            smoothing: 0.3,
            motion_blur: 0.35,
            click_bounce: 1.0,
            click_impact: false,
            click_sound: false,
            click_sound_gain_db: 0.0,
        }
    }
}

impl CursorStyle {
    /// A v1 document showed no pointer.
    pub fn plain() -> Self {
        CursorStyle { show: false, ..CursorStyle::default() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FocusMode {
    Manual,
    #[default]
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ZoomOrigin {
    #[default]
    Manual,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoomRegion {
    pub id: String,
    /// Index into `EditDoc::clips`. Times are SOURCE ms of that clip's recording.
    pub clip: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    /// 1..=6, see `ZOOM_DEPTHS`.
    pub depth: u8,
    #[serde(default)]
    pub custom_scale: Option<f64>,
    /// Focus point in fractions (0..1) of the source frame.
    pub focus_x: f64,
    pub focus_y: f64,
    #[serde(default)]
    pub focus_mode: FocusMode,
    #[serde(default)]
    pub origin: ZoomOrigin,
    #[serde(default)]
    pub hide_cursor: bool,
}

/// The scale a region zooms to: its custom scale (clamped to 1..5) or its depth's table value.
pub fn zoom_scale(z: &ZoomRegion) -> f64 {
    match z.custom_scale {
        Some(s) if s.is_finite() => s.clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE),
        _ => ZOOM_DEPTHS[(z.depth.clamp(1, 6) - 1) as usize],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TextAnimation {
    #[default]
    None,
    Fade,
    Rise,
    Pop,
    SlideLeft,
    Typewriter,
    Pulse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ArrowDirection {
    Up,
    Down,
    Left,
    #[default]
    Right,
    UpRight,
    UpLeft,
    DownRight,
    DownLeft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BlurStyle {
    Blur,
    #[default]
    Mosaic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BlurShape {
    #[default]
    Rectangle,
    Oval,
    Freehand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BlurTint {
    #[default]
    White,
    Black,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TextBody {
    pub content: String,
    pub color: String,
    pub background: String,
    pub font_size: f64,
    pub font_family: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub align: TextAlign,
    pub animation: TextAnimation,
}

impl Default for TextBody {
    /// A NEW text: white, bold, centred, on the dark plate.
    fn default() -> Self {
        TextBody {
            content: String::new(),
            color: DEFAULT_TEXT_COLOR.into(),
            background: DEFAULT_TEXT_PLATE.into(),
            font_size: DEFAULT_FONT_SIZE,
            font_family: TEXT_FONTS[0].into(),
            bold: true,
            italic: false,
            underline: false,
            align: TextAlign::Center,
            animation: TextAnimation::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BlurBody {
    pub style: BlurStyle,
    pub shape: BlurShape,
    pub color: BlurTint,
    pub intensity: f64,
    pub block_size: f64,
    /// Free-form outline in percent (0..100) of the annotation box.
    pub points: Vec<(f64, f64)>,
}

impl Default for BlurBody {
    fn default() -> Self {
        BlurBody {
            style: BlurStyle::Mosaic,
            shape: BlurShape::Rectangle,
            color: BlurTint::White,
            intensity: DEFAULT_BLUR_INTENSITY,
            block_size: DEFAULT_BLUR_BLOCK,
            points: DEFAULT_FREEHAND_POINTS.to_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AnnotationBody {
    Text(TextBody),
    Image {
        #[serde(default)]
        file: String,
    },
    Arrow {
        #[serde(default)]
        direction: ArrowDirection,
        #[serde(default = "default_arrow_color")]
        color: String,
        #[serde(rename = "strokeWidth", default = "default_arrow_stroke")]
        stroke_width: f64,
    },
    Blur(BlurBody),
}

fn default_arrow_color() -> String {
    DEFAULT_ARROW_COLOR.into()
}
fn default_arrow_stroke() -> f64 {
    DEFAULT_ARROW_STROKE
}
fn default_ann_x() -> f64 {
    ANNOTATION_DEFAULT_RECT.0
}
fn default_ann_y() -> f64 {
    ANNOTATION_DEFAULT_RECT.1
}
fn default_ann_w() -> f64 {
    ANNOTATION_DEFAULT_RECT.2
}
fn default_ann_h() -> f64 {
    ANNOTATION_DEFAULT_RECT.3
}

/// A list where an entry that does not parse is dropped instead of failing the whole document
/// (one broken annotation must not cost every clip and zoom edit).
fn lenient_vec<'de, D, T>(de: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let raw = Option::<Vec<serde_json::Value>>::deserialize(de)?.unwrap_or_default();
    Ok(raw.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect())
}

/// A text, image, arrow or blur box over the video. Times are SOURCE ms of its clip (like
/// [`ZoomRegion`]); the box is in percent (0..100) of the frame (blur: of the screen).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    #[serde(default)]
    pub id: String,
    pub clip: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    #[serde(default = "default_ann_x")]
    pub x: f64,
    #[serde(default = "default_ann_y")]
    pub y: f64,
    #[serde(default = "default_ann_w")]
    pub w: f64,
    #[serde(default = "default_ann_h")]
    pub h: f64,
    #[serde(default)]
    pub z_index: i32,
    pub body: AnnotationBody,
}

impl Annotation {
    /// A new annotation of `body` with the upstream box and `z_index` above every existing one.
    pub fn new(id: String, clip: usize, start_ms: u64, end_ms: u64, body: AnnotationBody, existing: &[Annotation]) -> Self {
        let (x, y, w, h) = ANNOTATION_DEFAULT_RECT;
        let top = existing.iter().map(|a| a.z_index).max().unwrap_or(0).clamp(0, MAX_Z_INDEX);
        let z_index = top.saturating_add(1).min(MAX_Z_INDEX);
        Annotation { id, clip, start_ms, end_ms, x, y, w, h, z_index, body }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TrackKind {
    #[default]
    Music,
    Voiceover,
}

/// A music or voiceover file laid under the programme. It starts at `start_ms` (SOURCE ms of
/// its anchor clip) and plays `offset_ms..` of the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioTrack {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub kind: TrackKind,
    /// File name under `<project>/assets/`.
    pub file: String,
    pub clip: usize,
    #[serde(default)]
    pub start_ms: u64,
    /// Length of the file.
    pub duration_ms: u64,
    #[serde(default)]
    pub offset_ms: u64,
    /// Voiceover: the recorded length.
    #[serde(default)]
    pub span_ms: Option<u64>,
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub looped: bool,
    #[serde(default)]
    pub fade_in_ms: u64,
    #[serde(default)]
    pub fade_out_ms: u64,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub label: String,
}

/// The length a track plays: a voiceover's recorded span, else the file from its offset. A
/// looped music track has no fixed length (`None`).
pub fn track_span_ms(t: &AudioTrack) -> Option<u64> {
    match t.span_ms {
        Some(s) => Some(s),
        None if t.looped => None,
        None => Some(t.duration_ms.saturating_sub(t.offset_ms)),
    }
}

/// Fades that do not fit the span share it in proportion (`resolveFadeSecs`): an asymmetric
/// pair keeps its shape.
pub fn fit_fades(fade_in_ms: u64, fade_out_ms: u64, span_ms: u64) -> (u64, u64) {
    let total = fade_in_ms + fade_out_ms;
    if total <= span_ms {
        return (fade_in_ms, fade_out_ms);
    }
    let scale = span_ms as f64 / total as f64;
    let fade_in = (fade_in_ms as f64 * scale).floor() as u64;
    (fade_in, (fade_out_ms as f64 * scale).floor() as u64)
}

// ── camera (stage 5) ─────────────────────────────────────────────────────────

/// Camera bubble size, percent of the frame's short side (`WEBCAM_SIZE_MIN/MAX`, default 40).
pub const CAMERA_SIZE_RANGE: (f64, f64) = (super::layout::CAMERA_SIZE_MIN, super::layout::CAMERA_SIZE_MAX);
pub const DEFAULT_CAMERA_SIZE: f64 = 40.0;
/// `DEFAULT_WEBCAM_ROUNDNESS` per mask (`projectDefaults.ts:144`).
pub const CAMERA_ROUNDNESS_SQUARE: f64 = 0.7;
pub const CAMERA_ROUNDNESS_RECTANGLE: f64 = 0.4;
/// Background blur of a cut-out camera, 0..1 (`webcamBlurIntensity`, default 0.5).
pub const DEFAULT_CAMERA_BLUR: f64 = 0.5;
/// Morph between two layouts (`LAYOUT_TRANSITION_SECONDS`, `layoutTypes.ts:2`).
pub const LAYOUT_TRANSITION_SEC: f64 = 0.2;

pub use super::layout::{CameraAnchor, CameraMask, LayoutPreset};

/// What is drawn behind a cut-out camera (`webcamBackgroundMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CameraBgMode {
    #[default]
    None,
    Transparent,
    Blur,
    Custom,
}

/// The camera's background removal: the mode, its blur and (custom) one of our backgrounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CameraBackground {
    pub mode: CameraBgMode,
    /// 0..1, used by `blur`.
    pub blur: f64,
    /// Used by `custom` (our own backgrounds, never upstream's wallpaper).
    pub custom: Background,
}

impl Default for CameraBackground {
    fn default() -> Self {
        CameraBackground { mode: CameraBgMode::None, blur: DEFAULT_CAMERA_BLUR, custom: Background::Wallpaper { id: DEFAULT_WALLPAPER.into() } }
    }
}

/// The camera's look for the whole video (upstream's webcam settings, `projectDefaults.ts:253-262`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CameraStyle {
    pub preset: LayoutPreset,
    /// Percent of the frame's short side, 15..=60.
    pub size_pct: f64,
    pub mask: CameraMask,
    /// 0..1; `None` = the mask's standard (`CAMERA_ROUNDNESS_SQUARE/RECTANGLE`).
    pub roundness: Option<f64>,
    pub mirror: bool,
    /// Shrink while a zoom is on (picture-in-picture only).
    pub reactive_zoom: bool,
    pub anchor: CameraAnchor,
    /// User framing in fractions (0..1) of the camera frame; `None` = the whole frame.
    pub crop: Option<CropRect>,
    pub background: CameraBackground,
}

impl Default for CameraStyle {
    fn default() -> Self {
        CameraStyle {
            preset: LayoutPreset::PictureInPicture,
            size_pct: DEFAULT_CAMERA_SIZE,
            mask: CameraMask::Square,
            roundness: None,
            mirror: false,
            reactive_zoom: true,
            anchor: CameraAnchor::BottomRight,
            crop: None,
            background: CameraBackground::default(),
        }
    }
}

/// `Default` is THE camera default (migration, new documents, serde): upstream's look with
/// picture-in-picture for every project, camera or not. A clip without a camera resolves to
/// `no-webcam` only where it is laid out (`LayoutPreset::resolve`); `no-webcam` in a document
/// is the user's explicit "without camera" — a default stored there would hide the camera of a
/// take inserted later and of every project a preset saved from it is applied to.
impl CameraStyle {
    /// The rounding in use: the stored one or the mask's standard.
    pub fn resolved_roundness(&self) -> f64 {
        self.roundness.unwrap_or(match self.mask {
            CameraMask::Square => CAMERA_ROUNDNESS_SQUARE,
            CameraMask::Rectangle => CAMERA_ROUNDNESS_RECTANGLE,
        })
    }

    /// What the layout reads, with `preset` instead of the global one (a region / a clip
    /// resolves it first).
    pub fn look(&self, preset: LayoutPreset) -> super::layout::CameraLook {
        super::layout::CameraLook { preset, size_pct: self.size_pct, mask: self.mask, roundness: self.resolved_roundness(), anchor: self.anchor }
    }
}

/// From `start_ms` (SOURCE ms of `clip`) until the clip's next region, the camera is laid out
/// as `preset` instead of the global one (upstream `layoutRegions`). Follows its clip like a zoom.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutRegion {
    #[serde(default)]
    pub id: String,
    pub clip: usize,
    pub start_ms: u64,
    pub preset: LayoutPreset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditDoc {
    pub version: u32,
    pub clips: Vec<EditClip>,
    #[serde(default)]
    pub speed: Vec<SpeedRegion>,
    /// One entry per clip (`None` = no crop).
    #[serde(default)]
    pub crop: Vec<Option<CropRect>>,
    #[serde(default)]
    pub output: OutputShape,
    #[serde(default)]
    pub mic_gain_db: f64,
    #[serde(default)]
    pub appearance: Appearance,
    #[serde(default)]
    pub cursor: CursorStyle,
    #[serde(default)]
    pub zooms: Vec<ZoomRegion>,
    /// The automatic first planning of zooms already ran (a v1 document counts as done,
    /// so an old project never gets zooms added behind the user's back).
    #[serde(default)]
    pub auto_zoom_done: bool,
    #[serde(default, deserialize_with = "lenient_vec")]
    pub annotations: Vec<Annotation>,
    #[serde(default, deserialize_with = "lenient_vec")]
    pub audio_tracks: Vec<AudioTrack>,
    /// Level of the recorded system sound, dB.
    #[serde(default)]
    pub system_gain_db: f64,
    /// The camera's look (v4).
    #[serde(default)]
    pub camera: CameraStyle,
    /// Layout changes within the video (v4), see [`LayoutRegion`].
    #[serde(default, deserialize_with = "lenient_vec")]
    pub layout_regions: Vec<LayoutRegion>,
}

/// One clip on the programme timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipSpan {
    pub prog_start_ms: u64,
    pub prog_end_ms: u64,
}

/// One speed region on the programme timeline. `index` points into `EditDoc::speed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedSpan {
    pub index: usize,
    pub prog_start_ms: u64,
    pub prog_end_ms: u64,
}

/// One zoom region on the programme timeline. `index` points into `EditDoc::zooms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoomSpan {
    pub index: usize,
    pub prog_start_ms: u64,
    pub prog_end_ms: u64,
}

/// One annotation on the programme timeline. `index` points into `EditDoc::annotations`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationSpan {
    pub index: usize,
    pub prog_start_ms: u64,
    pub prog_end_ms: u64,
}

/// What the editor UI draws its timeline from.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorView {
    pub doc: EditDoc,
    pub programme_ms: u64,
    pub clips: Vec<ClipSpan>,
    pub speed: Vec<SpeedSpan>,
    /// Zoom regions in programme ms (Rust's mapping; the UI never converts source ms itself).
    pub zooms: Vec<ZoomSpan>,
    /// Annotations in programme ms (Rust's mapping, like zooms): the annotation lane and the
    /// Tab order (by programme start) read these, never source ms.
    pub annotation_spans: Vec<AnnotationSpan>,
    /// The recorded clicks on the programme clock (`click_cues::click_hits`), for the preview's
    /// click sound. Filled by the editor session (empty from [`view`], which has no cursor data).
    pub click_hits: Vec<super::click_cues::ClickHit>,
    /// Per clip: the screen's box in the output `[x, y, w, h]` in fractions (the focus
    /// crosshair is drawn over it). Filled by the editor session (empty from [`view`]).
    pub screen_rects: Vec<[f64; 4]>,
    /// The preview scene's output size `[w, h]` (1080p target). Under `Aspect::Auto` its shape
    /// follows padding, frame and crop, so the UI sizes its preview box from this, never from
    /// the aspect alone. Filled by the editor session (`[0, 0]` from [`view`], which has no
    /// recording metadata).
    pub output_size: [u32; 2],
    /// The inserted takes the clips may name (`source`), for the UI's audio preview. Filled by
    /// the editor session (empty from [`view`]).
    pub takes: TakeMetas,
    /// Every music/voiceover entry on the programme clock (`audio_tracks::track_entries`): what
    /// the export mixes and what the timeline and the preview play, muted ones included.
    pub tracks: Vec<super::audio_tracks::TrackSpan>,
    /// The waveforms on the programme clock (mic, system sound, tracks). Filled by the editor
    /// session as peaks arrive (empty from [`view`]).
    pub waveforms: Vec<super::peaks::WaveSpan>,
}

/// One clip spanning the whole recording, speed 1, no crop. With a `preset` (the default
/// style preset, its files already in the project — `presets::resolve_for_project`) the look
/// is the preset's; its aspect never carries over to a new project.
pub fn default_doc(meta: &RecordingMeta, preset: Option<&super::presets::Preset>) -> EditDoc {
    EditDoc {
        version: EDITOR_VERSION,
        clips: vec![EditClip::main(0, meta.duration_ms)],
        speed: Vec::new(),
        crop: vec![None],
        output: OutputShape { aspect: Aspect::Auto },
        mic_gain_db: 0.0,
        appearance: preset.map(|p| p.appearance.clone()).unwrap_or_default(),
        cursor: preset.map(|p| p.cursor.clone()).unwrap_or_default(),
        zooms: Vec::new(),
        auto_zoom_done: false,
        annotations: Vec::new(),
        audio_tracks: Vec::new(),
        system_gain_db: 0.0,
        camera: {
            let base = CameraStyle::default();
            preset.and_then(|p| p.camera.as_ref()).map_or_else(|| base.clone(), |c| c.apply_to(&base))
        },
        layout_regions: Vec::new(),
    }
}

/// Reads a stored editor block: v4 as is, v3 upgraded with the standard camera look
/// (`CameraStyle::default()`, picture-in-picture whether or not there is a camera) and no layout
/// regions, v2 upgraded with no annotations, no tracks and the
/// system sound at 0 dB, v1 first upgraded to v2 with the look it always had
/// (`Appearance::plain`, no pointer, no zooms, aspect as saved). Anything else is `None`.
pub fn migrate(value: serde_json::Value) -> Option<EditDoc> {
    let mut doc = serde_json::from_value::<EditDoc>(value).ok()?;
    if doc.clips.is_empty() {
        return None;
    }
    if doc.version == 1 {
        doc.version = 2;
        doc.appearance = Appearance::plain();
        doc.cursor = CursorStyle::plain();
        doc.zooms = Vec::new();
        doc.auto_zoom_done = true;
    }
    if doc.version == 2 {
        doc.version = 3;
        doc.annotations = Vec::new();
        doc.audio_tracks = Vec::new();
        doc.system_gain_db = 0.0;
        // A v2 clip always played the project's own take.
        for c in &mut doc.clips {
            c.source = None;
        }
    }
    if doc.version == 3 {
        doc.version = 4;
        // The standard look for every project; clips without a camera resolve per clip.
        doc.camera = CameraStyle::default();
        doc.layout_regions = Vec::new();
    }
    (doc.version == EDITOR_VERSION).then_some(doc)
}

/// The stored document, normalised. Empty, unparseable or unknown-version blocks fall back
/// to `default_doc` — an editor that cannot read its block must still open the recording.
pub fn load(project: &Project, takes: &TakeMetas) -> EditDoc {
    load_with(project, takes, None)
}

/// [`load`]; a project without a readable editor block (a new one) starts with `preset`'s look.
pub fn load_with(project: &Project, takes: &TakeMetas, preset: Option<&super::presets::Preset>) -> EditDoc {
    let meta = &project.recording;
    match migrate(project.editor.clone()) {
        Some(doc) => normalize(doc, meta.duration_ms, takes),
        // A hand-edited preset file is clamped like any stored look.
        None if preset.is_some() => normalize(default_doc(meta, preset), meta.duration_ms, takes),
        None => default_doc(meta, None),
    }
}

/// No stored editor block can be read: the project opens as new.
pub fn is_new(project: &Project) -> bool {
    migrate(project.editor.clone()).is_none()
}

pub fn store(project: &mut Project, doc: &EditDoc) {
    // EditDoc is plain data (no maps with non-string keys); serialisation cannot fail.
    project.editor = serde_json::to_value(doc).unwrap_or_else(|_| serde_json::json!({}));
}

// ── sources: the project's own take and inserted ones ───────────────────────

/// Folder (inside the project) inserted takes are copied into, one sub-folder per take id.
pub const TAKES_DIR: &str = "takes";

/// What the editor needs to know about an inserted take (from its `project.json` + files).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TakeMeta {
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub system_audio: bool,
    pub has_mic: bool,
    /// The take's mic position against its video (`MicMeta::offset_ms`; 0 without a mic).
    pub mic_offset_ms: i64,
    pub has_cursor: bool,
    /// The take's camera file size (`camera.mp4`), `None` without a camera.
    #[serde(default)]
    pub camera: Option<(u32, u32)>,
    /// Camera time = screen time − this (`CameraMeta::offset_ms`; 0 without a camera).
    #[serde(default)]
    pub camera_offset_ms: i64,
}

impl TakeMeta {
    pub fn from_recording(meta: &RecordingMeta, has_cursor: bool) -> Self {
        TakeMeta {
            duration_ms: meta.duration_ms,
            width: meta.width,
            height: meta.height,
            system_audio: meta.system_audio,
            has_mic: meta.mic.is_some(),
            mic_offset_ms: meta.mic.as_ref().map_or(0, |m| m.offset_ms),
            has_cursor,
            camera: meta.camera.as_ref().map(|c| (c.width, c.height)),
            camera_offset_ms: meta.camera.as_ref().map_or(0, |c| c.offset_ms),
        }
    }
}

/// The inserted takes of a project by id. A clip whose `source` is not in here plays nothing.
pub type TakeMetas = BTreeMap<String, TakeMeta>;

/// A take id is a plain folder name: 1..=64 of `[A-Za-z0-9_-]` — never a path.
pub fn valid_take_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The folder a clip's recording lives in: the project itself, or `<project>/takes/<id>/`.
/// An id that is not a plain name (see [`valid_take_id`]) never escapes the takes folder: it
/// maps to a name no take can have, so nothing there exists.
pub fn take_dir(project: &Path, source: Option<&str>) -> PathBuf {
    match source {
        None => project.to_path_buf(),
        Some(id) if valid_take_id(id) => project.join(TAKES_DIR).join(id),
        Some(_) => project.join(TAKES_DIR).join(".invalid"),
    }
}

/// The clip plays an inserted take that is not available (not in `takes`, or not a plain id).
pub fn take_missing(clip: &EditClip, takes: &TakeMetas) -> bool {
    clip.source.as_deref().is_some_and(|id| !valid_take_id(id) || !takes.contains_key(id))
}

/// The document as it plays: a clip whose take is missing lasts no time (its window collapses
/// to its start), so the programme, the transport and the scene skip it while the stored
/// document keeps it. Every index stays the same.
pub fn playable(doc: &EditDoc, takes: &TakeMetas) -> EditDoc {
    let mut out = doc.clone();
    for c in &mut out.clips {
        if take_missing(c, takes) {
            c.end_ms = c.start_ms;
        }
    }
    out
}

/// Per clip: its index among the clips the scene holds (`None` = a missing take, not in it).
pub fn scene_clip_indices(doc: &EditDoc, takes: &TakeMetas) -> Vec<Option<usize>> {
    let mut next = 0;
    doc.clips
        .iter()
        .map(|c| {
            if take_missing(c, takes) {
                None
            } else {
                next += 1;
                Some(next - 1)
            }
        })
        .collect()
}

/// Maps every clip index through `map` (old → new); entries whose clip has no new index go.
fn remap_clips(doc: &mut EditDoc, map: impl Fn(usize) -> Option<usize>) {
    doc.speed.retain_mut(|r| map(r.clip).map(|c| r.clip = c).is_some());
    doc.zooms.retain_mut(|z| map(z.clip).map(|c| z.clip = c).is_some());
    doc.annotations.retain_mut(|a| map(a.clip).map(|c| a.clip = c).is_some());
    doc.audio_tracks.retain_mut(|t| map(t.clip).map(|c| t.clip = c).is_some());
    doc.layout_regions.retain_mut(|r| map(r.clip).map(|c| r.clip = c).is_some());
    doc.speed.sort_by_key(|r| (r.clip, r.start_ms));
    doc.zooms.sort_by_key(|z| (z.clip, z.start_ms));
    doc.layout_regions.sort_by_key(|r| (r.clip, r.start_ms));
}

/// Clip `from` moved to position `to` (the index it has afterwards, `Vec::remove` + `insert`).
/// Its speed regions, crop, zooms, annotations and audio tracks go along; every other clip's
/// things follow their clip's new index. Out of range or `from == to`: unchanged.
pub fn move_clip(doc: &EditDoc, from: usize, to: usize) -> EditDoc {
    let n = doc.clips.len();
    if from >= n || to >= n || from == to {
        return doc.clone();
    }
    // order[new] = old
    let mut order: Vec<usize> = (0..n).collect();
    let moved = order.remove(from);
    order.insert(to, moved);
    let mut new_of = vec![0; n];
    for (new, &old) in order.iter().enumerate() {
        new_of[old] = new;
    }
    let mut next = doc.clone();
    let mut crop = doc.crop.clone();
    crop.resize(n, None);
    next.clips = order.iter().map(|&o| doc.clips[o].clone()).collect();
    next.crop = order.iter().map(|&o| crop[o]).collect();
    remap_clips(&mut next, |c| new_of.get(c).copied());
    next
}

/// `{id}-copy`, `{id}-copy2`, … — the first that is not in `taken` (which it joins).
fn copy_id(id: &str, taken: &mut HashSet<String>) -> String {
    let base = if id.trim().is_empty() { "copy".to_string() } else { format!("{id}-copy") };
    let mut candidate = base.clone();
    let mut n = 2;
    while !taken.insert(candidate.clone()) {
        candidate = format!("{base}{n}");
        n += 1;
    }
    candidate
}

/// A copy of clip `index` right behind it, with copies of its speed regions, crop, zooms,
/// annotations, audio tracks and layout regions (those under new ids). Out of range: unchanged.
pub fn duplicate_clip(doc: &EditDoc, index: usize) -> EditDoc {
    let n = doc.clips.len();
    if index >= n {
        return doc.clone();
    }
    let mut next = doc.clone();
    next.crop.resize(n, None);
    next.clips.insert(index + 1, doc.clips[index].clone());
    let crop = next.crop[index];
    next.crop.insert(index + 1, crop);
    let copy = index + 1;
    remap_clips(&mut next, |c| Some(if c > index { c + 1 } else { c }));

    let speed: Vec<SpeedRegion> =
        doc.speed.iter().filter(|r| r.clip == index).map(|r| SpeedRegion { clip: copy, ..*r }).collect();
    next.speed.extend(speed);
    let mut zoom_ids: HashSet<String> = next.zooms.iter().map(|z| z.id.clone()).collect();
    let zooms: Vec<ZoomRegion> = doc
        .zooms
        .iter()
        .filter(|z| z.clip == index)
        .map(|z| ZoomRegion { id: copy_id(&z.id, &mut zoom_ids), clip: copy, ..z.clone() })
        .collect();
    next.zooms.extend(zooms);
    let mut ann_ids: HashSet<String> = next.annotations.iter().map(|a| a.id.clone()).collect();
    let anns: Vec<Annotation> = doc
        .annotations
        .iter()
        .filter(|a| a.clip == index)
        .map(|a| Annotation { id: copy_id(&a.id, &mut ann_ids), clip: copy, ..a.clone() })
        .collect();
    next.annotations.extend(anns);
    let mut track_ids: HashSet<String> = next.audio_tracks.iter().map(|t| t.id.clone()).collect();
    let tracks: Vec<AudioTrack> = doc
        .audio_tracks
        .iter()
        .filter(|t| t.clip == index)
        .map(|t| AudioTrack { id: copy_id(&t.id, &mut track_ids), clip: copy, ..t.clone() })
        .collect();
    next.audio_tracks.extend(tracks);
    let mut layout_ids: HashSet<String> = next.layout_regions.iter().map(|r| r.id.clone()).collect();
    let layouts: Vec<LayoutRegion> = doc
        .layout_regions
        .iter()
        .filter(|r| r.clip == index)
        .map(|r| LayoutRegion { id: copy_id(&r.id, &mut layout_ids), clip: copy, ..r.clone() })
        .collect();
    next.layout_regions.extend(layouts);
    next.speed.sort_by_key(|r| (r.clip, r.start_ms));
    next.zooms.sort_by_key(|z| (z.clip, z.start_ms));
    next.layout_regions.sort_by_key(|r| (r.clip, r.start_ms));
    next
}

/// One clip over the whole of take `take_id`, appended at the end (no crop, speed 1).
pub fn append_take(doc: &EditDoc, take_id: &str, meta: &TakeMeta) -> EditDoc {
    let mut next = doc.clone();
    next.crop.resize(doc.clips.len(), None);
    next.clips.push(EditClip { start_ms: 0, end_ms: meta.duration_ms, source: Some(take_id.to_string()) });
    next.crop.push(None);
    next
}

fn finite_or(v: f64, fallback: f64) -> f64 {
    if v.is_finite() { v } else { fallback }
}

fn normalize_crop(c: CropRect) -> Option<CropRect> {
    if ![c.x, c.y, c.w, c.h].iter().all(|v| v.is_finite()) {
        return None;
    }
    let w = c.w.clamp(MIN_CROP_EDGE, 1.0);
    let h = c.h.clamp(MIN_CROP_EDGE, 1.0);
    Some(CropRect { x: c.x.clamp(0.0, 1.0 - w), y: c.y.clamp(0.0, 1.0 - h), w, h })
}

/// Clamp everything into range, drop clips under `MIN_CLIP_MS` (one clip always stays),
/// clamp speed regions into their clip and resolve overlaps (a later region in the list
/// cuts its range out of earlier ones). Regions come out sorted by (clip, start).
/// Annotations and audio tracks follow their clip like zooms (see [`normalize_annotations`],
/// [`normalize_tracks`]). `duration_ms` is the project's own take; a clip of an inserted
/// take (`source: Some(id)`) is clamped to that take's length from `takes`; a clip whose take
/// is not there (folder gone, not a plain id) keeps its bounds and everything on it (see
/// [`take_missing`], [`playable`]).
pub fn normalize(doc: EditDoc, duration_ms: u64, takes: &TakeMetas) -> EditDoc {
    let EditDoc {
        clips,
        speed,
        mut crop,
        output,
        mic_gain_db,
        appearance,
        cursor,
        zooms,
        auto_zoom_done,
        annotations,
        audio_tracks,
        system_gain_db,
        camera,
        layout_regions,
        ..
    } = doc;
    crop.resize(clips.len(), None);

    // Clamp clips into their source; remember each survivor's old index.
    let clamped: Vec<(usize, EditClip)> = clips
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let source = c.source.filter(|s| !s.trim().is_empty());
            let limit = match &source {
                None => duration_ms,
                // A take that is not readable right now (moved, half-synced, deleted) keeps
                // the clip as stored: it is skipped when playing, never lost on the next save.
                Some(id) => takes.get(id).filter(|_| valid_take_id(id)).map_or(u64::MAX, |t| t.duration_ms),
            };
            let start = c.start_ms.min(limit);
            let end = c.end_ms.min(limit).max(start);
            (i, EditClip { start_ms: start, end_ms: end, source })
        })
        .collect();
    let mut kept: Vec<(usize, EditClip)> =
        clamped.iter().filter(|(_, c)| c.end_ms - c.start_ms >= MIN_CLIP_MS).cloned().collect();
    if kept.is_empty() {
        // Keep the longest clip that has any length (first wins on a tie); else the whole source.
        let longest = clamped
            .iter()
            .filter(|(_, c)| c.end_ms > c.start_ms)
            .fold(None::<&(usize, EditClip)>, |best, cur| match best {
                Some(b) if b.1.end_ms - b.1.start_ms >= cur.1.end_ms - cur.1.start_ms => Some(b),
                _ => Some(cur),
            });
        kept = match longest {
            Some(c) => vec![c.clone()],
            None => vec![(usize::MAX, EditClip::main(0, duration_ms))],
        };
    }
    let new_index = |old: usize| kept.iter().position(|(o, _)| *o == old);
    let new_clips: Vec<EditClip> = kept.iter().map(|(_, c)| c.clone()).collect();
    let new_crop: Vec<Option<CropRect>> = kept
        .iter()
        .map(|(old, _)| crop.get(*old).copied().flatten().and_then(normalize_crop))
        .collect();

    // Speed regions: re-index, clamp, then later-wins overlap resolution per clip.
    let mut resolved: Vec<SpeedRegion> = Vec::new();
    for r in speed {
        let Some(clip) = new_index(r.clip) else { continue };
        let c = &new_clips[clip];
        let start = r.start_ms.clamp(c.start_ms, c.end_ms);
        let end = r.end_ms.clamp(c.start_ms, c.end_ms);
        if end <= start {
            continue;
        }
        let speed = finite_or(r.speed, 1.0).clamp(MIN_SPEED, MAX_SPEED);
        let mut next = Vec::with_capacity(resolved.len() + 1);
        for e in resolved {
            if e.clip != clip || e.end_ms <= start || e.start_ms >= end {
                next.push(e);
                continue;
            }
            if e.start_ms < start {
                next.push(SpeedRegion { end_ms: start, ..e });
            }
            if e.end_ms > end {
                next.push(SpeedRegion { start_ms: end, ..e });
            }
        }
        next.push(SpeedRegion { clip, start_ms: start, end_ms: end, speed });
        resolved = next;
    }
    resolved.sort_by_key(|r| (r.clip, r.start_ms));

    let zooms = normalize_zooms(zooms, &new_clips, new_index);
    let annotations = normalize_annotations(annotations, &new_clips, new_index);
    let audio_tracks = normalize_tracks(audio_tracks, &new_clips, new_index);
    let layout_regions = normalize_layout_regions(layout_regions, &new_clips, new_index);

    EditDoc {
        version: EDITOR_VERSION,
        clips: new_clips,
        speed: resolved,
        crop: new_crop,
        output,
        mic_gain_db: finite_or(mic_gain_db, 0.0).clamp(MIN_MIC_GAIN_DB, MAX_MIC_GAIN_DB),
        appearance: normalize_appearance(appearance),
        cursor: normalize_cursor(cursor),
        zooms,
        auto_zoom_done,
        annotations,
        audio_tracks,
        system_gain_db: clamp_or(system_gain_db, SYSTEM_GAIN_RANGE.0, SYSTEM_GAIN_RANGE.1, 0.0),
        camera: normalize_camera(camera),
        layout_regions,
    }
}

/// `taken` ∪ the result never holds `id` twice: an empty or repeated id gets `{prefix}-{n}`.
fn unique_id(id: String, prefix: &str, taken: &mut std::collections::HashSet<String>) -> String {
    if !id.trim().is_empty() && taken.insert(id.clone()) {
        return id;
    }
    let mut n = taken.len() + 1;
    loop {
        let candidate = format!("{prefix}-{n}");
        if taken.insert(candidate.clone()) {
            return candidate;
        }
        n += 1;
    }
}

/// `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb(…)`, `rgba(…)` or `transparent` — what the
/// compositor's colour parser reads (`frame_geometry.rs:120-136`).
fn valid_color(c: &str) -> bool {
    let c = c.trim();
    if c.eq_ignore_ascii_case("transparent") {
        return true;
    }
    if let Some(hex) = c.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|ch| ch.is_ascii_hexdigit());
    }
    let lower = c.to_ascii_lowercase();
    (lower.starts_with("rgb(") || lower.starts_with("rgba(")) && lower.ends_with(')')
}

fn color_or(c: String, fallback: &str) -> String {
    if valid_color(&c) { c } else { fallback.into() }
}

fn normalize_points(points: Vec<(f64, f64)>, shape: BlurShape) -> Vec<(f64, f64)> {
    let pts: Vec<(f64, f64)> = points
        .into_iter()
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .map(|(x, y)| (x.clamp(0.0, 100.0), y.clamp(0.0, 100.0)))
        .collect();
    if shape == BlurShape::Freehand && pts.len() < 3 {
        return DEFAULT_FREEHAND_POINTS.to_vec();
    }
    pts
}

fn normalize_body(body: AnnotationBody) -> AnnotationBody {
    match body {
        AnnotationBody::Text(t) => {
            let font_family = if TEXT_FONTS.contains(&t.font_family.as_str()) {
                t.font_family
            } else {
                TEXT_FONTS[0].into()
            };
            AnnotationBody::Text(TextBody {
                color: color_or(t.color, DEFAULT_TEXT_COLOR),
                background: color_or(t.background, "transparent"),
                font_size: clamp_or(t.font_size, FONT_SIZE_RANGE.0, FONT_SIZE_RANGE.1, DEFAULT_FONT_SIZE),
                font_family,
                ..t
            })
        }
        AnnotationBody::Image { file } => AnnotationBody::Image { file },
        AnnotationBody::Arrow { direction, color, stroke_width } => AnnotationBody::Arrow {
            direction,
            color: color_or(color, DEFAULT_ARROW_COLOR),
            stroke_width: if stroke_width.is_finite() && stroke_width >= 0.0 {
                stroke_width
            } else {
                DEFAULT_ARROW_STROKE
            },
        },
        AnnotationBody::Blur(b) => AnnotationBody::Blur(BlurBody {
            intensity: clamp_or(b.intensity, BLUR_INTENSITY_RANGE.0, BLUR_INTENSITY_RANGE.1, DEFAULT_BLUR_INTENSITY),
            block_size: clamp_or(b.block_size, BLUR_BLOCK_RANGE.0, BLUR_BLOCK_RANGE.1, DEFAULT_BLUR_BLOCK),
            points: normalize_points(b.points, b.shape),
            ..b
        }),
    }
}

/// Like zooms: re-index onto the surviving clips (a deleted clip takes its annotations along),
/// clamp into the clip (an annotation belongs to the part holding its start; a cut shortens it
/// at the edge), drop empty spans. Annotations may overlap — the list order stays. The box,
/// colours, font and blur numbers are clamped; ids are made unique.
fn normalize_annotations(
    annotations: Vec<Annotation>,
    clips: &[EditClip],
    new_index: impl Fn(usize) -> Option<usize>,
) -> Vec<Annotation> {
    let mut taken = std::collections::HashSet::new();
    let (dx, dy, dw, dh) = ANNOTATION_DEFAULT_RECT;
    let mut out = Vec::with_capacity(annotations.len());
    for a in annotations {
        let Some(clip) = new_index(a.clip) else { continue };
        let c = &clips[clip];
        let start = a.start_ms.clamp(c.start_ms, c.end_ms);
        let end = a.end_ms.clamp(c.start_ms, c.end_ms);
        if end <= start {
            continue;
        }
        out.push(Annotation {
            id: unique_id(a.id, "ann", &mut taken),
            clip,
            start_ms: start,
            end_ms: end,
            x: clamp_or(a.x, 0.0, 100.0, dx),
            y: clamp_or(a.y, 0.0, 100.0, dy),
            w: clamp_or(a.w, 1.0, 100.0, dw),
            h: clamp_or(a.h, 1.0, 100.0, dh),
            z_index: a.z_index.clamp(0, MAX_Z_INDEX),
            body: normalize_body(a.body),
        });
    }
    out
}

/// Re-index the anchor clip (a deleted clip takes its tracks along; after a split the track
/// belongs to the part holding its start), clamp the start into that clip, the numbers into
/// range, and fades that do not fit the track's span share it in proportion.
fn normalize_tracks(
    tracks: Vec<AudioTrack>,
    clips: &[EditClip],
    new_index: impl Fn(usize) -> Option<usize>,
) -> Vec<AudioTrack> {
    let mut taken = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(tracks.len());
    for t in tracks {
        let Some(clip) = new_index(t.clip) else { continue };
        let c = &clips[clip];
        let mut t = AudioTrack {
            id: unique_id(t.id, "track", &mut taken),
            clip,
            start_ms: t.start_ms.clamp(c.start_ms, c.end_ms),
            offset_ms: t.offset_ms.min(t.duration_ms),
            gain_db: clamp_or(t.gain_db, TRACK_GAIN_RANGE.0, TRACK_GAIN_RANGE.1, 0.0),
            fade_in_ms: t.fade_in_ms.min(FADE_MAX_MS),
            fade_out_ms: t.fade_out_ms.min(FADE_MAX_MS),
            ..t
        };
        if let Some(span) = track_span_ms(&t) {
            (t.fade_in_ms, t.fade_out_ms) = fit_fades(t.fade_in_ms, t.fade_out_ms, span);
        }
        out.push(t);
    }
    out
}

fn clamp_or(v: f64, lo: f64, hi: f64, fallback: f64) -> f64 {
    finite_or(v, fallback).clamp(lo, hi)
}

fn normalize_appearance(a: Appearance) -> Appearance {
    let d = Appearance::default();
    Appearance {
        padding: clamp_or(a.padding, 0.0, 100.0, d.padding),
        radius_px: clamp_or(a.radius_px, 0.0, 64.0, d.radius_px),
        shadow: clamp_or(a.shadow, 0.0, 1.0, d.shadow),
        background_blur: clamp_or(a.background_blur, 0.0, 1.0, d.background_blur),
        motion_blur: clamp_or(a.motion_blur, 0.0, 1.0, d.motion_blur),
        ..a
    }
}

fn normalize_camera(c: CameraStyle) -> CameraStyle {
    let d = CameraStyle::default();
    CameraStyle {
        size_pct: clamp_or(c.size_pct, CAMERA_SIZE_RANGE.0, CAMERA_SIZE_RANGE.1, d.size_pct),
        roundness: c.roundness.filter(|r| r.is_finite()).map(|r| r.clamp(0.0, 1.0)),
        // Same edge minimum as the screen crop.
        crop: c.crop.and_then(normalize_crop),
        background: CameraBackground { blur: clamp_or(c.background.blur, 0.0, 1.0, DEFAULT_CAMERA_BLUR), ..c.background },
        ..c
    }
}

/// Re-index onto the surviving clips like zooms; a region before its clip's start applies from
/// the start, one at or after its end has nothing to change and goes. Of two regions on the same
/// start the later in the list wins (a region holds from its start until the next one); ids made
/// unique. Sorted by (clip, start).
fn normalize_layout_regions(
    regions: Vec<LayoutRegion>,
    clips: &[EditClip],
    new_index: impl Fn(usize) -> Option<usize>,
) -> Vec<LayoutRegion> {
    // In order of the original start: of several regions clamped onto the clip's start, the one
    // that began last is the one in force there.
    let mut placed: Vec<(usize, u64, LayoutRegion)> = Vec::new();
    for (order, r) in regions.into_iter().enumerate() {
        let Some(clip) = new_index(r.clip) else { continue };
        let c = &clips[clip];
        if r.start_ms >= c.end_ms {
            continue;
        }
        let original = r.start_ms;
        placed.push((order, original, LayoutRegion { clip, start_ms: r.start_ms.max(c.start_ms), ..r }));
    }
    placed.sort_by_key(|(order, original, r)| (r.clip, *original, *order));
    let mut out: Vec<LayoutRegion> = Vec::new();
    for (_, _, r) in placed {
        out.retain(|e| !(e.clip == r.clip && e.start_ms == r.start_ms));
        out.push(r);
    }
    out.sort_by_key(|r| (r.clip, r.start_ms));
    let mut taken = HashSet::new();
    for r in &mut out {
        r.id = unique_id(std::mem::take(&mut r.id), "layout", &mut taken);
    }
    out
}

fn normalize_cursor(c: CursorStyle) -> CursorStyle {
    let d = CursorStyle::default();
    CursorStyle {
        size: clamp_or(c.size, 1.5, 6.0, d.size),
        smoothing: clamp_or(c.smoothing, 0.0, 1.0, d.smoothing),
        motion_blur: clamp_or(c.motion_blur, 0.0, 1.0, d.motion_blur),
        click_bounce: clamp_or(c.click_bounce, 0.0, 2.0, d.click_bounce),
        click_sound_gain_db: clamp_or(c.click_sound_gain_db, -24.0, 0.0, d.click_sound_gain_db),
        custom: c.custom.map(|k| CustomCursor {
            hotspot_x: clamp_or(k.hotspot_x, 0.0, 1.0, 0.0),
            hotspot_y: clamp_or(k.hotspot_y, 0.0, 1.0, 0.0),
            ..k
        }),
        ..c
    }
}

/// Re-index onto the surviving clips, clamp into the clip (a region belongs to the clip
/// part holding its start; a cut shortens it at the edge), later-wins overlap resolution,
/// then drop slivers under `MIN_ZOOM_MS`.
fn normalize_zooms(
    zooms: Vec<ZoomRegion>,
    clips: &[EditClip],
    new_index: impl Fn(usize) -> Option<usize>,
) -> Vec<ZoomRegion> {
    let mut resolved: Vec<ZoomRegion> = Vec::new();
    for z in zooms {
        let Some(clip) = new_index(z.clip) else { continue };
        let c = &clips[clip];
        let start = z.start_ms.clamp(c.start_ms, c.end_ms);
        let end = z.end_ms.clamp(c.start_ms, c.end_ms);
        if end <= start {
            continue;
        }
        let custom_scale = z
            .custom_scale
            .filter(|s| s.is_finite())
            .map(|s| s.clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE));
        let new = ZoomRegion {
            clip,
            start_ms: start,
            end_ms: end,
            depth: z.depth.clamp(1, 6),
            custom_scale,
            focus_x: clamp_or(z.focus_x, 0.0, 1.0, 0.5),
            focus_y: clamp_or(z.focus_y, 0.0, 1.0, 0.5),
            ..z
        };
        let mut next = Vec::with_capacity(resolved.len() + 1);
        for e in resolved {
            if e.clip != clip || e.end_ms <= start || e.start_ms >= end {
                next.push(e);
            } else if e.start_ms < start {
                // The later region cuts the earlier one's tail (or its middle: the head stays).
                next.push(ZoomRegion { end_ms: start, ..e });
            } else if e.end_ms > end {
                next.push(ZoomRegion { start_ms: end, ..e });
            }
        }
        next.push(new);
        resolved = next;
    }
    resolved.retain(|z| z.end_ms - z.start_ms >= MIN_ZOOM_MS);
    resolved.sort_by_key(|z| (z.clip, z.start_ms));
    resolved
}

/// One clip's source window cut into speed segments `(start_ms, end_ms, speed)` — the port
/// of `regions::speed_segments_for_window`: regions of this clip sorted by start, clipped to
/// the window and to what earlier regions already covered; gaps play at 1×.
fn segments(doc: &EditDoc, clip: usize) -> Vec<(f64, f64, f64)> {
    let Some(c) = doc.clips.get(clip) else { return Vec::new() };
    let (ws, we) = (c.start_ms as f64, c.end_ms as f64);
    if we <= ws {
        return Vec::new();
    }
    let mut regions: Vec<&SpeedRegion> = doc
        .speed
        .iter()
        .filter(|r| r.clip == clip && (r.start_ms as f64) < we && (r.end_ms as f64) > ws)
        .collect();
    regions.sort_by_key(|r| r.start_ms);
    let mut out = Vec::new();
    let mut cursor = ws;
    for r in regions {
        let start = (r.start_ms as f64).max(ws).max(cursor);
        let end = (r.end_ms as f64).min(we);
        if end <= start {
            continue;
        }
        if start > cursor {
            out.push((cursor, start, 1.0));
        }
        let speed = if r.speed.is_finite() && r.speed > 0.0 { r.speed } else { 1.0 };
        out.push((start, end, speed));
        cursor = end;
    }
    if cursor < we {
        out.push((cursor, we, 1.0));
    }
    out
}

fn clip_programme_ms(doc: &EditDoc, clip: usize) -> f64 {
    segments(doc, clip).iter().map(|(s, e, v)| (e - s) / v).sum()
}

fn round_ms(v: f64) -> u64 {
    if v <= 0.0 { 0 } else { v.round() as u64 }
}

/// The programme length — by definition the programme time of the last clip's end, so the
/// timeline (`view`) and this number can never disagree by a rounding step.
pub fn programme_duration_ms(doc: &EditDoc) -> u64 {
    match doc.clips.last() {
        Some(c) => source_to_programme(doc, doc.clips.len() - 1, c.end_ms),
        None => 0,
    }
}

/// Where programme time `t_ms` lands in the source: `(clip, source ms)`. The very end of
/// the programme maps to the last clip's end; anything past it is `None`.
pub fn programme_to_source(doc: &EditDoc, t_ms: u64) -> Option<(usize, u64)> {
    let t = t_ms as f64;
    let mut acc = 0.0;
    let mut last = None;
    for clip in 0..doc.clips.len() {
        for (s, e, v) in segments(doc, clip) {
            let len = (e - s) / v;
            if t < acc + len {
                return Some((clip, round_ms(s + (t - acc) * v)));
            }
            acc += len;
            last = Some((clip, round_ms(e)));
        }
    }
    if t <= acc + 0.5 { last } else { None }
}

/// Programme time of `source_ms` within `clip` (clamped into the clip's window).
pub fn source_to_programme(doc: &EditDoc, clip: usize, source_ms: u64) -> u64 {
    let Some(c) = doc.clips.get(clip) else {
        return round_ms((0..doc.clips.len()).map(|i| clip_programme_ms(doc, i)).sum());
    };
    let src = (source_ms.clamp(c.start_ms, c.end_ms.max(c.start_ms))) as f64;
    let mut acc: f64 = (0..clip).map(|i| clip_programme_ms(doc, i)).sum();
    for (s, e, v) in segments(doc, clip) {
        if src <= s {
            break;
        }
        acc += (src.min(e) - s) / v;
    }
    round_ms(acc)
}

/// Source ms of programme time `prog_ms` within `clip` (clamped to the clip's window) — the
/// inverse of [`source_to_programme`] for one clip, unrounded (waveforms sample it per block).
pub fn clip_source_at(doc: &EditDoc, clip: usize, prog_ms: f64) -> f64 {
    let Some(c) = doc.clips.get(clip) else { return 0.0 };
    let mut acc: f64 = (0..clip).map(|i| clip_programme_ms(doc, i)).sum();
    for (s, e, v) in segments(doc, clip) {
        let len = (e - s) / v;
        if prog_ms < acc + len {
            return s + (prog_ms - acc).max(0.0) * v;
        }
        acc += len;
    }
    c.end_ms.max(c.start_ms) as f64
}

/// Playback speed at `source_ms` in `clip` (the segment holding it; its end belongs to the
/// segment before). Outside the clip: 1.
pub fn speed_at(doc: &EditDoc, clip: usize, source_ms: u64) -> f64 {
    let t = source_ms as f64;
    let segs = segments(doc, clip);
    segs.iter()
        .find(|(s, e, _)| *s <= t && t < *e)
        .or_else(|| segs.last().filter(|(_, e, _)| t == *e))
        .map_or(1.0, |(_, _, v)| *v)
}

/// The timeline geometry the editor UI draws: clip and speed spans in programme ms.
pub fn view(doc: &EditDoc) -> EditorView {
    // Each clip starts exactly where the previous one ended (no second float sum per edge).
    let mut prev_end = 0;
    let clips = doc
        .clips
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let span = ClipSpan { prog_start_ms: prev_end, prog_end_ms: source_to_programme(doc, i, c.end_ms) };
            prev_end = span.prog_end_ms;
            span
        })
        .collect();
    let speed = doc
        .speed
        .iter()
        .enumerate()
        .filter(|(_, r)| r.clip < doc.clips.len())
        .map(|(index, r)| SpeedSpan {
            index,
            prog_start_ms: source_to_programme(doc, r.clip, r.start_ms),
            prog_end_ms: source_to_programme(doc, r.clip, r.end_ms),
        })
        .collect();
    let zooms = doc
        .zooms
        .iter()
        .enumerate()
        .filter(|(_, z)| z.clip < doc.clips.len())
        .map(|(index, z)| ZoomSpan {
            index,
            prog_start_ms: source_to_programme(doc, z.clip, z.start_ms),
            prog_end_ms: source_to_programme(doc, z.clip, z.end_ms),
        })
        .collect();
    let annotation_spans = doc
        .annotations
        .iter()
        .enumerate()
        .filter(|(_, a)| a.clip < doc.clips.len())
        .map(|(index, a)| AnnotationSpan {
            index,
            prog_start_ms: source_to_programme(doc, a.clip, a.start_ms),
            prog_end_ms: source_to_programme(doc, a.clip, a.end_ms),
        })
        .collect();
    EditorView {
        doc: doc.clone(),
        programme_ms: programme_duration_ms(doc),
        clips,
        speed,
        zooms,
        annotation_spans,
        click_hits: Vec::new(),
        screen_rects: Vec::new(),
        output_size: [0, 0],
        takes: TakeMetas::new(),
        tracks: super::audio_tracks::track_entries(doc),
        waveforms: Vec::new(),
    }
}

fn even_round(v: f64) -> u32 {
    (((v / 2.0).round() as u32) * 2).max(2)
}

fn even_floor(v: f64) -> u32 {
    (((v / 2.0).floor() as u32) * 2).max(2)
}

/// The source frame's size (1920×1080 when unknown).
fn source_dims(meta: &RecordingMeta) -> (f64, f64) {
    if meta.width > 0 && meta.height > 0 {
        (meta.width as f64, meta.height as f64)
    } else {
        (1920.0, 1080.0)
    }
}

/// Clip `clip`'s screen source in px: its recording (the project's own or an inserted take)
/// × its crop (`screenSourceSize`).
pub fn clip_source_size(meta: &RecordingMeta, takes: &TakeMetas, doc: &EditDoc, clip: usize) -> (u32, u32) {
    let take = doc.clips.get(clip).and_then(|c| c.source.as_ref()).and_then(|id| takes.get(id));
    let (sw, sh) = match take {
        Some(t) if t.width > 0 && t.height > 0 => (t.width as f64, t.height as f64),
        _ => source_dims(meta),
    };
    let c = doc.crop.get(clip).copied().flatten();
    let (cw, ch) = c.map_or((1.0, 1.0), |c| (c.w, c.h));
    (((sw * cw).round() as u32).max(1), ((sh * ch).round() as u32).max(1))
}

/// Clip `clip`'s camera: its file's size and offset (camera time = screen time − offset), from
/// its recording (`RecordingMeta::camera` / `TakeMeta::camera`). `None` = no camera on that clip
/// (or its take is missing). The scene's `webcamPath` and the layout both ask this.
pub fn clip_camera(meta: &RecordingMeta, takes: &TakeMetas, doc: &EditDoc, clip: usize) -> Option<((u32, u32), i64)> {
    let c = doc.clips.get(clip)?;
    if take_missing(c, takes) {
        return None;
    }
    match &c.source {
        None => meta.camera.as_ref().map(|m| ((m.width, m.height), m.offset_ms)),
        Some(id) => takes.get(id).and_then(|t| t.camera.map(|size| (size, t.camera_offset_ms))),
    }
}

/// Clip `clip`'s camera box source: its camera × the framing crop (`webcamBoxSourceSize`).
pub fn clip_camera_box(meta: &RecordingMeta, takes: &TakeMetas, doc: &EditDoc, clip: usize) -> Option<(u32, u32)> {
    let ((w, h), _) = clip_camera(meta, takes, doc, clip)?;
    // Unknown size (0): upstream's stand-in camera, 960×720.
    let (w, h) = if w > 0 && h > 0 { (w as f64, h as f64) } else { (960.0, 720.0) };
    let (cw, ch) = doc.camera.crop.map_or((1.0, 1.0), |c| (c.w, c.h));
    Some((((w * cw).round() as u32).max(1), ((h * ch).round() as u32).max(1)))
}

/// The Auto shape (width / height): `layout::auto_aspect` over the first clip's screen
/// (crop included), the padding, the frame and the camera layout resolved for that clip (a
/// block layout adds its camera; OpenScreen `autoAspectRatioValue`).
pub fn auto_ratio(meta: &RecordingMeta, takes: &TakeMetas, doc: &EditDoc) -> f64 {
    let (w, h) = clip_source_size(meta, takes, doc, 0);
    let preset = doc.camera.preset.resolve(clip_camera(meta, takes, doc, 0).is_some());
    super::layout::auto_aspect(w as f64 / h as f64, doc.appearance.padding, doc.appearance.frame, preset)
}

/// Export frame size of `doc` for `target`. `Aspect::Auto` takes `auto_ratio`; its LONG side
/// is the target's (1280 / 1920 / the source's long side), the other derived and even
/// (`toEvenPx = max(2, round(v/2)·2)`, OpenScreen `pickOutputDims`). Every other aspect:
/// [`aspect_output_size`].
pub fn output_size(meta: &RecordingMeta, takes: &TakeMetas, doc: &EditDoc, target: OutputTarget) -> (u32, u32) {
    if doc.output.aspect != Aspect::Auto {
        return aspect_output_size(meta, doc.output.aspect, target);
    }
    let ratio = auto_ratio(meta, takes, doc);
    let (sw, sh) = source_dims(meta);
    let long = match target {
        OutputTarget::P720 => 1280.0,
        OutputTarget::P1080 => 1920.0,
        OutputTarget::Source => sw.max(sh),
    };
    let long = even_round(long) as f64;
    if ratio >= 1.0 {
        (long as u32, even_round(long / ratio))
    } else {
        (even_round(long * ratio), long as u32)
    }
}

/// Export frame size for a fixed aspect. Even on both axes (H.264/HEVC need it). For 720p/1080p
/// the SHORT side is 720/1080; at source resolution it is the largest rect of the aspect inside
/// the source. `Auto` has no fixed ratio — it needs the document, see [`output_size`]; here it
/// falls back to the source's shape.
pub fn aspect_output_size(meta: &RecordingMeta, aspect: Aspect, target: OutputTarget) -> (u32, u32) {
    let (sw, sh) = source_dims(meta);
    let ratio = aspect.ratio().unwrap_or(sw / sh);
    let short = match target {
        OutputTarget::P720 => 720.0,
        OutputTarget::P1080 => 1080.0,
        OutputTarget::Source => {
            let (w, h) = if sw / sh > ratio { (sh * ratio, sh) } else { (sw, sw / ratio) };
            return (even_floor(w + 1e-6), even_floor(h + 1e-6));
        }
    };
    if ratio >= 1.0 {
        (even_round(short * ratio), even_round(short))
    } else {
        (even_round(short), even_round(short / ratio))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    static NO_TAKES: TakeMetas = TakeMetas::new();

    use super::*;
    use crate::record::project::{MicMeta, ProjectStatus, PROJECT_SCHEMA};

    pub(crate) fn meta(duration_ms: u64) -> RecordingMeta {
        RecordingMeta {
            width: 3024,
            height: 1964,
            fps: 60,
            duration_ms,
            system_audio: true,
            mic: Some(MicMeta { offset_ms: 120, sample_rate: 48_000 }),
            ..RecordingMeta::default()
        }
    }

    fn project_with(editor: serde_json::Value) -> Project {
        Project {
            schema: PROJECT_SCHEMA,
            id: "take".into(),
            title: String::new(),
            created_at: 0,
            updated_at: 0,
            status: ProjectStatus::Ready,
            failure: None,
            recording: meta(10_000),
            editor,
        }
    }

    fn clip(start_ms: u64, end_ms: u64) -> EditClip {
        EditClip::main(start_ms, end_ms)
    }

    fn sr(clip: usize, start_ms: u64, end_ms: u64, speed: f64) -> SpeedRegion {
        SpeedRegion { clip, start_ms, end_ms, speed }
    }

    fn doc(clips: Vec<EditClip>, speed: Vec<SpeedRegion>) -> EditDoc {
        let n = clips.len();
        EditDoc {
            version: EDITOR_VERSION,
            clips,
            speed,
            crop: vec![None; n],
            output: OutputShape::default(),
            mic_gain_db: 0.0,
            appearance: Appearance::plain(),
            cursor: CursorStyle::plain(),
            zooms: Vec::new(),
            auto_zoom_done: true,
            annotations: Vec::new(),
            audio_tracks: Vec::new(),
            system_gain_db: 0.0,
            camera: CameraStyle::default(),
            layout_regions: Vec::new(),
        }
    }

    fn zoom(clip: usize, start_ms: u64, end_ms: u64) -> ZoomRegion {
        ZoomRegion {
            id: format!("z{clip}-{start_ms}"),
            clip,
            start_ms,
            end_ms,
            depth: DEFAULT_ZOOM_DEPTH,
            custom_scale: None,
            focus_x: 0.5,
            focus_y: 0.5,
            focus_mode: FocusMode::Auto,
            origin: ZoomOrigin::Manual,
            hide_cursor: false,
        }
    }

    /// The brief's example: one clip 0–10 s, ×2 on 2–6 s.
    fn example() -> EditDoc {
        doc(vec![clip(0, 10_000)], vec![sr(0, 2_000, 6_000, 2.0)])
    }

    #[test]
    fn default_doc_from_meta() {
        let d = default_doc(&meta(7_500), None);
        assert_eq!(d.version, EDITOR_VERSION);
        assert_eq!(d.clips, vec![clip(0, 7_500)]);
        assert!(d.speed.is_empty());
        assert_eq!(d.crop, vec![None]);
        assert_eq!(d.output.aspect, Aspect::Auto);
        assert_eq!(d.mic_gain_db, 0.0);
    }

    #[test]
    fn load_falls_back_on_garbage() {
        let def = default_doc(&meta(10_000), None);
        for editor in [
            serde_json::json!({}),
            serde_json::json!(null),
            serde_json::json!("garbage"),
            serde_json::json!({"version": 1, "clips": "nope"}),
            serde_json::json!({"version": 5, "clips": [{"startMs": 0, "endMs": 5000}]}),
            serde_json::json!({"version": 0, "clips": [{"startMs": 0, "endMs": 5000}]}),
            serde_json::json!({"version": 1, "clips": []}),
        ] {
            assert_eq!(load(&project_with(editor.clone()), &NO_TAKES), def, "{editor}");
        }
    }

    #[test]
    fn store_then_load_roundtrips() {
        let mut p = project_with(serde_json::json!({}));
        let mut d = example();
        d.crop = vec![Some(CropRect { x: 0.1, y: 0.2, w: 0.5, h: 0.5 })];
        d.output.aspect = Aspect::W9H16;
        d.mic_gain_db = -6.0;
        store(&mut p, &d);
        assert_eq!(p.editor["version"], EDITOR_VERSION);
        assert_eq!(p.editor["output"]["aspect"], "9:16");
        assert_eq!(p.editor["micGainDb"], -6.0);
        assert_eq!(load(&p, &NO_TAKES), d);
    }

    #[test]
    fn normalize_clamps_and_drops_tiny_clips() {
        let mut d = doc(
            vec![clip(0, 50), clip(2_000, 99_000), clip(4_000, 4_000)],
            vec![sr(1, 1_000, 50_000, 40.0), sr(0, 0, 50, 2.0), sr(1, 3_000, 4_000, 0.1)],
        );
        d.crop = vec![
            None,
            Some(CropRect { x: -0.5, y: 0.98, w: 0.01, h: 2.0 }),
            Some(CropRect { x: 0.0, y: 0.0, w: 0.5, h: 0.5 }),
        ];
        d.mic_gain_db = 40.0;
        let n = normalize(d, 10_000, &NO_TAKES);
        // Clip 0 (50 ms) and clip 2 (empty) go; clip 1 is clamped into the source.
        assert_eq!(n.clips, vec![clip(2_000, 10_000)]);
        // Regions follow their clip's new index, are clamped into it, speed into 0.25..16;
        // the region on the dropped clip goes with it.
        assert_eq!(n.speed, vec![sr(0, 2_000, 3_000, 16.0), sr(0, 3_000, 4_000, 0.25), sr(0, 4_000, 10_000, 16.0)]);
        // Crop of the surviving clip: min edge 0.05, inside 0..1.
        assert_eq!(n.crop, vec![Some(CropRect { x: 0.0, y: 0.0, w: MIN_CROP_EDGE, h: 1.0 })]);
        assert_eq!(n.mic_gain_db, MAX_MIC_GAIN_DB);
        let quiet = normalize(EditDoc { mic_gain_db: -200.0, ..example() }, 10_000, &NO_TAKES);
        assert_eq!(quiet.mic_gain_db, MIN_MIC_GAIN_DB);
    }

    #[test]
    fn normalize_rejects_non_finite_numbers() {
        let mut d = doc(vec![clip(0, 10_000)], vec![sr(0, 0, 1_000, f64::NAN)]);
        d.crop = vec![Some(CropRect { x: f64::NAN, y: 0.0, w: 0.5, h: 0.5 })];
        d.mic_gain_db = f64::NAN;
        let n = normalize(d, 10_000, &NO_TAKES);
        assert_eq!(n.speed[0].speed, 1.0);
        assert_eq!(n.crop, vec![None]);
        assert_eq!(n.mic_gain_db, 0.0);
    }

    #[test]
    fn normalize_overlap_later_wins() {
        let d = doc(
            vec![clip(0, 10_000), clip(0, 10_000)],
            vec![
                sr(0, 1_000, 5_000, 2.0),
                sr(0, 3_000, 4_000, 4.0),  // cuts a hole into the first
                sr(1, 1_000, 5_000, 2.0),  // other clip: untouched by clip 0's regions
                sr(0, 0, 2_000, 0.5),      // eats the head of the first
            ],
        );
        let n = normalize(d, 10_000, &NO_TAKES);
        assert_eq!(
            n.speed,
            vec![
                sr(0, 0, 2_000, 0.5),
                sr(0, 2_000, 3_000, 2.0),
                sr(0, 3_000, 4_000, 4.0),
                sr(0, 4_000, 5_000, 2.0),
                sr(1, 1_000, 5_000, 2.0),
            ]
        );
    }

    #[test]
    fn normalize_keeps_one_clip() {
        // Every clip too short: the longest survives rather than leaving nothing to play.
        let n = normalize(doc(vec![clip(0, 30), clip(500, 580), clip(900, 950)], vec![]), 10_000, &NO_TAKES);
        assert_eq!(n.clips, vec![clip(500, 580)]);
        assert_eq!(n.crop, vec![None]);
        // Nothing usable at all: the whole source.
        let n = normalize(doc(vec![clip(20_000, 30_000)], vec![]), 10_000, &NO_TAKES);
        assert_eq!(n.clips, vec![clip(0, 10_000)]);
        let n = normalize(doc(vec![], vec![]), 10_000, &NO_TAKES);
        assert_eq!(n.clips, vec![clip(0, 10_000)]);
        assert_eq!(n.version, EDITOR_VERSION);
    }

    #[test]
    fn programme_mapping_roundtrip() {
        let d = example();
        assert_eq!(programme_duration_ms(&d), 8_000);
        assert_eq!(programme_to_source(&d, 0), Some((0, 0)));
        assert_eq!(programme_to_source(&d, 1_000), Some((0, 1_000)));
        assert_eq!(programme_to_source(&d, 3_000), Some((0, 4_000)));
        assert_eq!(programme_to_source(&d, 5_000), Some((0, 7_000)));
        assert_eq!(programme_to_source(&d, 8_000), Some((0, 10_000)));
        assert_eq!(programme_to_source(&d, 8_001), None);
        assert_eq!(source_to_programme(&d, 0, 7_000), 5_000);
        assert_eq!(source_to_programme(&d, 0, 4_000), 3_000);
        // Out-of-clip source times clamp to the clip's edges.
        assert_eq!(source_to_programme(&d, 0, 99_000), 8_000);
        for t in (0..=8_000).step_by(37) {
            let (c, s) = programme_to_source(&d, t).unwrap();
            let back = source_to_programme(&d, c, s);
            assert!(back.abs_diff(t) <= 1, "t={t} s={s} back={back}");
        }
    }

    #[test]
    fn programme_mapping_across_clips() {
        // Clip A = source 1–3 s at 1×, clip B = source 6–10 s with ×4 on 6–8 s and ×0.5 on 9–10 s.
        let d = doc(
            vec![clip(1_000, 3_000), clip(6_000, 10_000)],
            vec![sr(1, 6_000, 8_000, 4.0), sr(1, 9_000, 10_000, 0.5)],
        );
        // 2000 + (500 + 1000 + 2000) = 5500
        assert_eq!(programme_duration_ms(&d), 5_500);
        assert_eq!(programme_to_source(&d, 0), Some((0, 1_000)));
        assert_eq!(programme_to_source(&d, 1_999), Some((0, 2_999)));
        assert_eq!(programme_to_source(&d, 2_000), Some((1, 6_000)));
        assert_eq!(programme_to_source(&d, 2_250), Some((1, 7_000)));
        assert_eq!(programme_to_source(&d, 3_000), Some((1, 8_500)));
        assert_eq!(programme_to_source(&d, 4_500), Some((1, 9_500)));
        assert_eq!(source_to_programme(&d, 1, 8_000), 2_500);
        assert_eq!(source_to_programme(&d, 1, 10_000), 5_500);
        assert_eq!(source_to_programme(&d, 0, 3_000), 2_000);
        // The same source time in two clips maps to two programme times.
        let twice = doc(vec![clip(0, 2_000), clip(0, 2_000)], vec![]);
        assert_eq!(source_to_programme(&twice, 0, 1_000), 1_000);
        assert_eq!(source_to_programme(&twice, 1, 1_000), 3_000);
    }

    #[test]
    fn view_spans_match_mapping() {
        let v = view(&example());
        assert_eq!(v.programme_ms, 8_000);
        assert_eq!(v.clips, vec![ClipSpan { prog_start_ms: 0, prog_end_ms: 8_000 }]);
        assert_eq!(v.speed, vec![SpeedSpan { index: 0, prog_start_ms: 2_000, prog_end_ms: 4_000 }]);
        assert_eq!(v.doc, example());

        let d = doc(
            vec![clip(1_000, 3_000), clip(6_000, 10_000)],
            vec![sr(1, 9_000, 10_000, 0.5), sr(1, 6_000, 8_000, 4.0)],
        );
        let v = view(&d);
        assert_eq!(v.programme_ms, programme_duration_ms(&d));
        for (i, c) in v.clips.iter().enumerate() {
            assert_eq!(c.prog_start_ms, source_to_programme(&d, i, d.clips[i].start_ms));
            assert_eq!(c.prog_end_ms, source_to_programme(&d, i, d.clips[i].end_ms));
        }
        for s in &v.speed {
            let r = d.speed[s.index];
            assert_eq!(s.prog_start_ms, source_to_programme(&d, r.clip, r.start_ms));
            assert_eq!(s.prog_end_ms, source_to_programme(&d, r.clip, r.end_ms));
            assert_eq!(programme_to_source(&d, s.prog_start_ms), Some((r.clip, r.start_ms)));
        }
        // Indices point back into the document as given (not re-sorted).
        assert_eq!(v.speed.iter().map(|s| s.index).collect::<Vec<_>>(), vec![0, 1]);
        // camelCase on the wire.
        let j = serde_json::to_value(&v).unwrap();
        assert!(j.get("programmeMs").is_some() && j["clips"][0].get("progStartMs").is_some());
    }

    #[test]
    fn view_clips_are_contiguous() {
        // Clip i ends exactly where clip i+1 starts, and the last end is the programme length —
        // for awkward speeds whose float sums round differently per path.
        for k in 1..60u64 {
            let d = doc(
                vec![clip(0, 333 + k), clip(1_000, 1_777 + 3 * k), clip(4_000, 4_101 + 7 * k)],
                vec![sr(0, 10, 300, 3.0), sr(1, 1_001, 1_500 + k, 0.3 + k as f64 * 0.11), sr(2, 4_001, 4_100, 7.0)],
            );
            let v = view(&d);
            assert_eq!(v.clips[0].prog_start_ms, 0);
            for w in v.clips.windows(2) {
                assert_eq!(w[0].prog_end_ms, w[1].prog_start_ms, "gap or overlap at k={k}");
            }
            assert_eq!(v.clips.last().unwrap().prog_end_ms, v.programme_ms, "k={k}");
        }
    }

    #[test]
    fn output_size_presets() {
        let m = meta(1_000);
        assert_eq!(aspect_output_size(&m, Aspect::W16H9, OutputTarget::P1080), (1920, 1080));
        assert_eq!(aspect_output_size(&m, Aspect::W9H16, OutputTarget::P1080), (1080, 1920));
        assert_eq!(aspect_output_size(&m, Aspect::Source, OutputTarget::Source), (3024, 1964));
        assert_eq!(aspect_output_size(&m, Aspect::W16H9, OutputTarget::P720), (1280, 720));
        assert_eq!(aspect_output_size(&m, Aspect::W1H1, OutputTarget::P720), (720, 720));
        assert_eq!(aspect_output_size(&m, Aspect::W21H9, OutputTarget::P1080), (2520, 1080));
        // Source aspect at 1080p: short side 1080, long side scaled and even.
        assert_eq!(aspect_output_size(&m, Aspect::Source, OutputTarget::P1080), (1662, 1080));
        // A fixed aspect at source resolution: the largest such rect inside the source.
        assert_eq!(aspect_output_size(&m, Aspect::W9H16, OutputTarget::Source), (1104, 1964));
        assert_eq!(aspect_output_size(&m, Aspect::W16H9, OutputTarget::Source), (3024, 1700));
        // Every size is even (H.264/HEVC need it).
        let odd = RecordingMeta { width: 1001, height: 777, ..m.clone() };
        for a in [Aspect::Source, Aspect::W16H9, Aspect::W9H16, Aspect::W4H3, Aspect::W3H4, Aspect::W21H9] {
            for t in [OutputTarget::P720, OutputTarget::P1080, OutputTarget::Source] {
                let (w, h) = aspect_output_size(&odd, a, t);
                assert!(w % 2 == 0 && h % 2 == 0 && w >= 2 && h >= 2, "{a:?} {t:?} → {w}×{h}");
            }
        }
        // An unknown source size falls back to 16:9 rather than dividing by zero.
        let unknown = RecordingMeta { width: 0, height: 0, ..m };
        assert_eq!(aspect_output_size(&unknown, Aspect::Source, OutputTarget::P1080), (1920, 1080));
    }

    #[test]
    fn output_size_auto_follows_layout() {
        let m = meta(1_000);
        let mut d = default_doc(&m, None); // Auto, 50 % padding, no frame
        // e' = 0.8 · 3024/1964 + 0.2 = 1.4318; the long side is the target's.
        assert_eq!(output_size(&m, &NO_TAKES, &d, OutputTarget::P1080), (1920, 1340));
        assert_eq!(output_size(&m, &NO_TAKES, &d, OutputTarget::P720), (1280, 894));
        assert_eq!(output_size(&m, &NO_TAKES, &d, OutputTarget::Source), (3024, 2112));
        // No padding: the screen's own shape.
        d.appearance.padding = 0.0;
        assert_eq!(output_size(&m, &NO_TAKES, &d, OutputTarget::P1080), (1920, 1246));
        // The first clip's crop decides the shape (a vertical strip → portrait, long side tall).
        d.crop[0] = Some(CropRect { x: 0.25, y: 0.0, w: 0.25, h: 1.0 });
        let (w, h) = output_size(&m, &NO_TAKES, &d, OutputTarget::P1080);
        assert_eq!(h, 1920);
        assert!(w < h && w % 2 == 0, "{w}x{h}");
        // A fixed aspect ignores the layout.
        d.output.aspect = Aspect::W16H9;
        assert_eq!(output_size(&m, &NO_TAKES, &d, OutputTarget::P1080), (1920, 1080));
    }

    // ── v2: appearance, cursor, zooms ───────────────────────────────────────────

    /// A stage-2 document as it sits in a project file.
    fn v1_json() -> serde_json::Value {
        serde_json::json!({
            "version": 1,
            "clips": [{"startMs": 0, "endMs": 8000}],
            "speed": [{"clip": 0, "startMs": 1000, "endMs": 2000, "speed": 2.0}],
            "crop": [{"x": 0.1, "y": 0.2, "w": 0.5, "h": 0.5}],
            "output": {"aspect": "9:16"},
            "micGainDb": -3.0
        })
    }

    #[test]
    fn v1_doc_migrates_without_visual_change() {
        let d = load(&project_with(v1_json()), &NO_TAKES);
        assert_eq!(d.version, EDITOR_VERSION);
        assert_eq!(d.appearance, Appearance::plain());
        assert!(!d.cursor.show);
        assert!(d.zooms.is_empty());
        assert!(d.auto_zoom_done, "an old project must not get zooms planned afterwards");
        assert_eq!(d.output.aspect, Aspect::W9H16);
        assert_eq!(d.clips, vec![clip(0, 8_000)]);
        assert_eq!(d.speed, vec![sr(0, 1_000, 2_000, 2.0)]);
        assert_eq!(d.mic_gain_db, -3.0);
        // A v1 block without any `output` keeps meaning "as the source".
        let bare = serde_json::json!({"version": 1, "clips": [{"startMs": 0, "endMs": 5000}]});
        assert_eq!(load(&project_with(bare), &NO_TAKES).output.aspect, Aspect::Source);
    }

    #[test]
    fn new_doc_gets_upstream_defaults() {
        let d = default_doc(&meta(5_000), None);
        assert_eq!(d.output.aspect, Aspect::Auto);
        let a = &d.appearance;
        assert_eq!((a.padding, a.radius_px, a.shadow, a.background_blur, a.motion_blur), (50.0, 40.0, 0.6, 0.0, 0.2));
        assert_eq!(a.background, Background::Wallpaper { id: "01".into() });
        assert_eq!((a.frame, a.frame_theme, a.background_motion), (Frame::None, FrameTheme::Light, WallpaperMotion::None));
        let c = &d.cursor;
        assert!(c.show && !c.auto_hide && !c.click_impact && !c.click_sound);
        assert_eq!((c.size, c.smoothing, c.motion_blur, c.click_bounce, c.click_sound_gain_db), (3.6, 0.3, 0.35, 1.0, 0.0));
        assert_eq!(c.theme, "default");
        assert!(d.zooms.is_empty() && !d.auto_zoom_done);
    }

    #[test]
    fn unknown_version_falls_back_to_default() {
        let json = serde_json::json!({"version": 99, "clips": [{"startMs": 0, "endMs": 5000}]});
        assert!(migrate(json.clone()).is_none());
        assert_eq!(load(&project_with(json), &NO_TAKES), default_doc(&meta(10_000), None));
    }

    #[test]
    fn zoom_scale_table() {
        for (i, want) in ZOOM_DEPTHS.iter().enumerate() {
            let z = ZoomRegion { depth: i as u8 + 1, ..zoom(0, 0, 1_000) };
            assert_eq!(zoom_scale(&z), *want);
        }
        assert_eq!(ZOOM_DEPTHS, [1.25, 1.5, 1.8, 2.2, 3.5, 5.0]);
        assert_eq!(zoom_scale(&ZoomRegion { custom_scale: Some(7.0), ..zoom(0, 0, 1_000) }), 5.0);
        assert_eq!(zoom_scale(&ZoomRegion { custom_scale: Some(0.5), ..zoom(0, 0, 1_000) }), 1.0);
        assert_eq!(zoom_scale(&ZoomRegion { custom_scale: Some(2.5), ..zoom(0, 0, 1_000) }), 2.5);
    }

    fn with_zooms(clips: Vec<EditClip>, zooms: Vec<ZoomRegion>) -> EditDoc {
        EditDoc { zooms, ..doc(clips, vec![]) }
    }

    #[test]
    fn normalize_zoom_follows_split() {
        // What the TS split leaves behind: two clips, the zoom still reaching across the cut.
        let d = with_zooms(vec![clip(0, 3_000), clip(3_000, 10_000)], vec![zoom(0, 1_000, 5_000)]);
        let n = normalize(d, 10_000, &NO_TAKES);
        assert_eq!(n.zooms.len(), 1);
        assert_eq!((n.zooms[0].clip, n.zooms[0].start_ms, n.zooms[0].end_ms), (0, 1_000, 3_000));
    }

    #[test]
    fn normalize_drops_zoom_of_deleted_clip() {
        // Clip 0 is a sliver and goes; its zoom goes with it, clip 1's zoom moves to index 0.
        let d = with_zooms(vec![clip(0, 50), clip(2_000, 9_000)], vec![zoom(0, 0, 50), zoom(1, 3_000, 5_000)]);
        let n = normalize(d, 10_000, &NO_TAKES);
        assert_eq!(n.clips, vec![clip(2_000, 9_000)]);
        assert_eq!(n.zooms.len(), 1);
        assert_eq!((n.zooms[0].clip, n.zooms[0].start_ms), (0, 3_000));
    }

    #[test]
    fn normalize_drops_short_zoom() {
        let d = with_zooms(vec![clip(0, 10_000)], vec![zoom(0, 1_000, 1_200), zoom(0, 4_000, 4_250)]);
        let n = normalize(d, 10_000, &NO_TAKES);
        assert_eq!(n.zooms.iter().map(|z| z.start_ms).collect::<Vec<_>>(), vec![4_000]);
        // A trim that leaves under 250 ms of a zoom removes it too.
        let d = with_zooms(vec![clip(0, 1_100)], vec![zoom(0, 1_000, 5_000)]);
        assert!(normalize(d, 10_000, &NO_TAKES).zooms.is_empty());
    }

    #[test]
    fn normalize_zoom_overlap_later_wins() {
        let d = with_zooms(vec![clip(0, 10_000)], vec![zoom(0, 1_000, 5_000), zoom(0, 3_000, 7_000)]);
        let n = normalize(d, 10_000, &NO_TAKES);
        let spans: Vec<_> = n.zooms.iter().map(|z| (z.start_ms, z.end_ms)).collect();
        assert_eq!(spans, vec![(1_000, 3_000), (3_000, 7_000)]);
        // The later one eating the earlier one's head.
        let d = with_zooms(vec![clip(0, 10_000)], vec![zoom(0, 3_000, 7_000), zoom(0, 1_000, 5_000)]);
        let spans: Vec<_> = normalize(d, 10_000, &NO_TAKES).zooms.iter().map(|z| (z.start_ms, z.end_ms)).collect();
        assert_eq!(spans, vec![(1_000, 5_000), (5_000, 7_000)]);
    }

    #[test]
    fn normalize_clamps_zoom_numbers() {
        let z = ZoomRegion { depth: 9, focus_x: 1.7, focus_y: f64::NAN, custom_scale: Some(40.0), ..zoom(0, 0, 2_000) };
        let n = normalize(with_zooms(vec![clip(0, 10_000)], vec![z]), 10_000, &NO_TAKES);
        let z = &n.zooms[0];
        assert_eq!((z.depth, z.focus_x, z.focus_y, z.custom_scale), (6, 1.0, 0.5, Some(5.0)));
        let z0 = ZoomRegion { depth: 0, ..zoom(0, 0, 2_000) };
        assert_eq!(normalize(with_zooms(vec![clip(0, 10_000)], vec![z0]), 10_000, &NO_TAKES).zooms[0].depth, 1);
    }

    #[test]
    fn normalize_clamps_appearance() {
        let mut d = example();
        d.appearance = Appearance {
            padding: 150.0,
            radius_px: -3.0,
            shadow: 2.0,
            background_blur: f64::NAN,
            motion_blur: -1.0,
            ..Appearance::default()
        };
        d.cursor = CursorStyle {
            size: 99.0,
            smoothing: -1.0,
            motion_blur: 5.0,
            click_bounce: 9.0,
            click_sound_gain_db: 6.0,
            custom: Some(CustomCursor { file: "c.png".into(), hotspot_x: 3.0, hotspot_y: -1.0 }),
            ..CursorStyle::default()
        };
        let n = normalize(d, 10_000, &NO_TAKES);
        let a = &n.appearance;
        assert_eq!((a.padding, a.radius_px, a.shadow, a.background_blur, a.motion_blur), (100.0, 0.0, 1.0, 0.0, 0.0));
        let c = &n.cursor;
        assert_eq!((c.size, c.smoothing, c.motion_blur, c.click_bounce, c.click_sound_gain_db), (6.0, 0.0, 1.0, 2.0, 0.0));
        let k = c.custom.as_ref().unwrap();
        assert_eq!((k.hotspot_x, k.hotspot_y), (1.0, 0.0));
    }

    #[test]
    fn serde_roundtrip_v2() {
        let mut d = default_doc(&meta(10_000), None);
        d.appearance.background = Background::Gradient { angle_deg: 135.0, stops: vec!["#111111".into(), "#eeeeee".into()] };
        d.appearance.frame = Frame::Laptop;
        d.appearance.frame_theme = FrameTheme::Dark;
        d.appearance.background_motion = WallpaperMotion::Aurora;
        d.cursor.custom = Some(CustomCursor { file: "cursor.png".into(), hotspot_x: 0.25, hotspot_y: 0.5 });
        d.zooms = vec![ZoomRegion { origin: ZoomOrigin::Auto, focus_mode: FocusMode::Manual, hide_cursor: true, custom_scale: Some(2.0), ..zoom(0, 1_000, 3_000) }];
        d.auto_zoom_done = true;
        let j = serde_json::to_value(&d).unwrap();
        assert_eq!(j["version"], EDITOR_VERSION);
        assert_eq!(j["appearance"]["background"]["kind"], "gradient");
        assert_eq!(j["appearance"]["background"]["angleDeg"], 135.0);
        assert_eq!(j["appearance"]["frame"], "laptop");
        assert_eq!(j["appearance"]["frameTheme"], "dark");
        assert_eq!(j["appearance"]["backgroundMotion"], "aurora");
        assert_eq!(j["output"]["aspect"], "auto");
        assert_eq!(j["cursor"]["clickSoundGainDb"], 0.0);
        assert_eq!(j["zooms"][0]["focusMode"], "manual");
        assert_eq!(j["autoZoomDone"], true);
        let back: EditDoc = serde_json::from_value(j).unwrap();
        assert_eq!(back, d);
        let mut p = project_with(serde_json::json!({}));
        store(&mut p, &d);
        assert_eq!(load(&p, &NO_TAKES), d);
        for b in [
            Background::Wallpaper { id: "03".into() },
            Background::Color { color: "#fff".into() },
            Background::Image { file: "bg.png".into() },
        ] {
            let v = serde_json::to_value(&b).unwrap();
            assert_eq!(serde_json::from_value::<Background>(v).unwrap(), b);
        }
    }

    // ── v3: annotations + audio tracks ──────────────────────────────────────

    fn v2_json() -> serde_json::Value {
        serde_json::json!({
            "version": 2,
            "clips": [{"startMs": 0, "endMs": 4000}, {"startMs": 5000, "endMs": 9000}],
            "speed": [{"clip": 1, "startMs": 6000, "endMs": 7000, "speed": 2.0}],
            "crop": [null, {"x": 0.1, "y": 0.1, "w": 0.5, "h": 0.5}],
            "output": {"aspect": "16:9"},
            "micGainDb": -4.0,
            "appearance": {"padding": 20.0, "background": {"kind": "color", "color": "#123456"}},
            "cursor": {"size": 2.0},
            "zooms": [{"id": "z1", "clip": 0, "startMs": 1000, "endMs": 3000, "depth": 4, "focusX": 0.2, "focusY": 0.3}],
            "autoZoomDone": true
        })
    }

    #[test]
    fn v2_doc_migrates_without_change() {
        let v2: EditDoc = serde_json::from_value(v2_json()).unwrap();
        let d = migrate(v2_json()).unwrap();
        assert_eq!(d.version, EDITOR_VERSION);
        assert!(d.annotations.is_empty() && d.audio_tracks.is_empty());
        assert_eq!(d.system_gain_db, 0.0);
        assert!(d.clips.iter().all(|c| c.source.is_none()));
        // The standard camera look (clips without a camera resolve per clip); no layout regions.
        assert_eq!(d.camera, CameraStyle::default());
        assert!(d.layout_regions.is_empty());
        // Everything a v2 document carried comes through untouched.
        assert_eq!(EditDoc { version: 2, camera: v2.camera.clone(), ..d.clone() }, v2);
        assert_eq!(load(&project_with(v2_json()), &NO_TAKES), normalize(d, 10_000, &NO_TAKES));
    }

    fn text(content: &str) -> AnnotationBody {
        AnnotationBody::Text(TextBody { content: content.into(), ..TextBody::default() })
    }

    fn ann(clip: usize, start_ms: u64, end_ms: u64, body: AnnotationBody) -> Annotation {
        Annotation::new(format!("a{clip}-{start_ms}"), clip, start_ms, end_ms, body, &[])
    }

    #[test]
    fn new_text_annotation_defaults() {
        let existing = vec![
            Annotation { z_index: 4, ..ann(0, 0, 1_000, text("a")) },
            Annotation { z_index: 2, ..ann(0, 0, 1_000, text("b")) },
        ];
        let a = Annotation::new("n".into(), 0, 0, 2_000, AnnotationBody::Text(TextBody::default()), &existing);
        assert_eq!((a.x, a.y, a.w, a.h), (50.0, 50.0, 30.0, 20.0));
        assert_eq!(a.z_index, 5);
        assert_eq!(Annotation::new("m".into(), 0, 0, 1, text(""), &[]).z_index, 1);
        let AnnotationBody::Text(t) = &a.body else { panic!("text") };
        assert_eq!((t.color.as_str(), t.background.as_str()), ("#ffffff", "rgba(0, 0, 0, 0.7)"));
        assert_eq!((t.font_size, t.font_family.as_str()), (32.0, "Inter"));
        assert!(t.bold && !t.italic && !t.underline);
        assert_eq!((t.align, t.animation), (TextAlign::Center, TextAnimation::None));
        let b = BlurBody::default();
        assert_eq!((b.style, b.shape, b.color), (BlurStyle::Mosaic, BlurShape::Rectangle, BlurTint::White));
        assert_eq!((b.intensity, b.block_size, b.points.len()), (12.0, 12.0, 9));
        assert_eq!(TEXT_FONTS, ["Inter", "Lora", "Oswald", "Caveat", "IBM Plex Mono"]);
    }

    fn with_annotations(clips: Vec<EditClip>, annotations: Vec<Annotation>) -> EditDoc {
        EditDoc { annotations, ..doc(clips, vec![]) }
    }

    #[test]
    fn annotation_spans_in_programme_time() {
        // Clip 0 = source 1–3 s (programme 0–2 s), clip 1 = source 6–10 s with a 2× region on
        // 6–8 s (programme 2–3 s at 2×, then 3–5 s).
        let mut d = with_annotations(
            vec![clip(1_000, 3_000), clip(6_000, 10_000)],
            vec![ann(1, 7_000, 9_000, text("late")), ann(0, 1_500, 2_500, text("early"))],
        );
        d.speed = vec![sr(1, 6_000, 8_000, 2.0)];
        let v = view(&d);
        assert_eq!(
            v.annotation_spans,
            vec![
                AnnotationSpan { index: 0, prog_start_ms: 2_500, prog_end_ms: 4_000 },
                AnnotationSpan { index: 1, prog_start_ms: 500, prog_end_ms: 1_500 },
            ]
        );
        for s in &v.annotation_spans {
            let a = &d.annotations[s.index];
            assert_eq!(s.prog_start_ms, source_to_programme(&d, a.clip, a.start_ms));
            assert_eq!(s.prog_end_ms, source_to_programme(&d, a.clip, a.end_ms));
        }
        // An annotation naming a clip that does not exist gets no span.
        d.annotations.push(ann(5, 0, 1_000, text("orphan")));
        assert_eq!(view(&d).annotation_spans.len(), 2);
        let j = serde_json::to_value(view(&d)).unwrap();
        assert_eq!(j["annotationSpans"][0]["progStartMs"], 2_500);
    }

    #[test]
    fn normalize_drops_annotation_of_deleted_clip() {
        let d = with_annotations(
            vec![clip(0, 50), clip(2_000, 9_000)],
            vec![ann(0, 0, 50, text("gone")), ann(1, 3_000, 5_000, text("kept"))],
        );
        let n = normalize(d, 10_000, &NO_TAKES);
        assert_eq!(n.annotations.len(), 1);
        assert_eq!((n.annotations[0].clip, n.annotations[0].start_ms), (0, 3_000));
        assert_eq!(n.annotations[0].body, text("kept"));
    }

    #[test]
    fn normalize_annotation_follows_split() {
        // What the TS split leaves behind: two clips, the annotation still reaching across the cut.
        let d = with_annotations(vec![clip(0, 3_000), clip(3_000, 10_000)], vec![ann(0, 1_000, 5_000, text("x"))]);
        let n = normalize(d, 10_000, &NO_TAKES);
        assert_eq!(n.annotations.len(), 1);
        assert_eq!((n.annotations[0].clip, n.annotations[0].start_ms, n.annotations[0].end_ms), (0, 1_000, 3_000));
        // Overlapping annotations both stay (unlike zooms).
        let d = with_annotations(vec![clip(0, 10_000)], vec![ann(0, 0, 4_000, text("a")), ann(0, 1_000, 3_000, text("b"))]);
        assert_eq!(normalize(d, 10_000, &NO_TAKES).annotations.len(), 2);
    }

    #[test]
    fn normalize_clamps_blur_and_font() {
        let blur = AnnotationBody::Blur(BlurBody { intensity: 99.0, block_size: 1.0, ..BlurBody::default() });
        let t = AnnotationBody::Text(TextBody {
            font_size: 300.0,
            font_family: "Comic".into(),
            color: "red; drop".into(),
            ..TextBody::default()
        });
        let arrow = AnnotationBody::Arrow { direction: ArrowDirection::UpLeft, color: "nope".into(), stroke_width: f64::NAN };
        let boxed = Annotation { x: -5.0, y: 140.0, w: 0.0, h: f64::NAN, z_index: -3, ..ann(0, 0, 1_000, text("b")) };
        let d = with_annotations(
            vec![clip(0, 10_000)],
            vec![ann(0, 0, 1_000, blur), ann(0, 0, 1_000, t), ann(0, 0, 1_000, arrow), boxed],
        );
        let n = normalize(d, 10_000, &NO_TAKES);
        let AnnotationBody::Blur(b) = &n.annotations[0].body else { panic!("blur") };
        assert_eq!((b.intensity, b.block_size), (40.0, 4.0));
        let AnnotationBody::Text(t) = &n.annotations[1].body else { panic!("text") };
        assert_eq!((t.font_size, t.font_family.as_str(), t.color.as_str()), (200.0, "Inter", "#ffffff"));
        let AnnotationBody::Arrow { color, stroke_width, direction } = &n.annotations[2].body else { panic!("arrow") };
        assert_eq!((color.as_str(), *stroke_width, *direction), ("#34B27B", 4.0, ArrowDirection::UpLeft));
        let a = &n.annotations[3];
        assert_eq!((a.x, a.y, a.w, a.h, a.z_index), (0.0, 100.0, 1.0, 20.0, 0));
        // Valid colours pass as written.
        for c in ["#fff", "#11223344", "rgba(0, 0, 0, 0.7)", "transparent"] {
            assert!(valid_color(c), "{c}");
        }
    }

    #[test]
    fn normalize_makes_annotation_ids_unique() {
        let d = with_annotations(
            vec![clip(0, 10_000)],
            vec![
                Annotation { id: "same".into(), ..ann(0, 0, 1_000, text("1")) },
                Annotation { id: "same".into(), ..ann(0, 0, 1_000, text("2")) },
                Annotation { id: "".into(), ..ann(0, 0, 1_000, text("3")) },
            ],
        );
        let ids: Vec<String> = normalize(d, 10_000, &NO_TAKES).annotations.into_iter().map(|a| a.id).collect();
        assert_eq!(ids[0], "same");
        assert_eq!(ids.iter().collect::<std::collections::HashSet<_>>().len(), 3);
        assert!(ids.iter().all(|i| !i.is_empty()));
    }

    #[test]
    fn freehand_with_two_points_gets_default() {
        let free = |points: Vec<(f64, f64)>| {
            AnnotationBody::Blur(BlurBody { shape: BlurShape::Freehand, points, ..BlurBody::default() })
        };
        let d = with_annotations(
            vec![clip(0, 10_000)],
            vec![
                ann(0, 0, 1_000, free(vec![(1.0, 1.0), (2.0, 2.0)])),
                ann(0, 0, 1_000, free(vec![(0.0, 0.0), (150.0, 50.0), (50.0, -4.0)])),
            ],
        );
        let n = normalize(d, 10_000, &NO_TAKES);
        let AnnotationBody::Blur(b) = &n.annotations[0].body else { panic!("blur") };
        assert_eq!(b.points, DEFAULT_FREEHAND_POINTS.to_vec());
        let AnnotationBody::Blur(b) = &n.annotations[1].body else { panic!("blur") };
        assert_eq!(b.points, vec![(0.0, 0.0), (100.0, 50.0), (50.0, 0.0)]);
    }

    fn track(clip: usize, start_ms: u64, duration_ms: u64) -> AudioTrack {
        AudioTrack {
            id: format!("t{clip}-{start_ms}"),
            kind: TrackKind::Music,
            file: "song.mp3".into(),
            clip,
            start_ms,
            duration_ms,
            offset_ms: 0,
            span_ms: None,
            gain_db: MUSIC_DEFAULT_GAIN_DB,
            looped: false,
            fade_in_ms: MUSIC_DEFAULT_FADE_MS,
            fade_out_ms: MUSIC_DEFAULT_FADE_MS,
            muted: false,
            label: String::new(),
        }
    }

    #[test]
    fn track_fades_scaled_to_span() {
        // A 1.5 s file with 1 s + 2 s fades (3 s total): shared in proportion, shape kept.
        let t = AudioTrack { fade_in_ms: 1_000, fade_out_ms: 2_000, ..track(0, 0, 1_500) };
        let v = AudioTrack {
            kind: TrackKind::Voiceover,
            span_ms: Some(800),
            fade_in_ms: 9_999,
            fade_out_ms: 400,
            gain_db: 40.0,
            ..track(1, 2_000, 60_000)
        };
        let looped = AudioTrack { looped: true, fade_in_ms: 4_000, fade_out_ms: 4_000, ..track(0, 0, 1_000) };
        let doc = EditDoc { audio_tracks: vec![t, v, looped], ..doc(vec![clip(0, 5_000), clip(5_000, 10_000)], vec![]) };
        let n = normalize(doc, 10_000, &NO_TAKES);
        assert_eq!((n.audio_tracks[0].fade_in_ms, n.audio_tracks[0].fade_out_ms), (500, 1_000));
        // Fade capped at 5 s first, then 5000 + 400 share 800 ms.
        assert_eq!((n.audio_tracks[1].fade_in_ms, n.audio_tracks[1].fade_out_ms), (740, 59));
        assert_eq!(n.audio_tracks[1].gain_db, 12.0);
        // Anchor clamped into its clip (start 2 s → clip 1 begins at 5 s).
        assert_eq!((n.audio_tracks[1].clip, n.audio_tracks[1].start_ms), (1, 5_000));
        // A looped music track has no fixed length: fades are only capped.
        assert_eq!((n.audio_tracks[2].fade_in_ms, n.audio_tracks[2].fade_out_ms), (4_000, 4_000));
        assert_eq!(fit_fades(300, 300, 1_000), (300, 300));
    }

    #[test]
    fn normalize_drops_track_of_deleted_clip_and_reindexes() {
        let doc = EditDoc {
            audio_tracks: vec![track(0, 0, 1_000), track(1, 3_000, 1_000)],
            system_gain_db: 30.0,
            ..doc(vec![clip(0, 50), clip(2_000, 9_000)], vec![])
        };
        let n = normalize(doc, 10_000, &NO_TAKES);
        assert_eq!(n.audio_tracks.len(), 1);
        assert_eq!((n.audio_tracks[0].clip, n.audio_tracks[0].start_ms), (0, 3_000));
        assert_eq!(n.system_gain_db, 12.0);
    }

    #[test]
    fn inserted_take_clip_is_clamped_to_its_take() {
        let mut other = clip(0, 30_000);
        other.source = Some("t2".into());
        let mut blank = clip(0, 4_000);
        blank.source = Some("  ".into());
        let takes = TakeMetas::from([("t2".to_string(), TakeMeta { duration_ms: 20_000, ..TakeMeta::default() })]);
        let n = normalize(doc(vec![clip(0, 30_000), other, blank], vec![]), 10_000, &takes);
        assert_eq!(n.clips[0], clip(0, 10_000));
        // Clamped to the take's own 20 s, not the project's 10 s and not left at 30 s.
        assert_eq!((n.clips[1].end_ms, n.clips[1].source.as_deref()), (20_000, Some("t2")));
        // A blank source is the project's own take.
        assert_eq!(n.clips[2].source, None);
    }

    #[test]
    fn missing_take_clip_survives_normalize_unchanged() {
        let mut gone = clip(2_000, 40_000);
        gone.source = Some("missing".into());
        let mut evil = clip(0, 4_000);
        evil.source = Some("../t2".into());
        let mut d = doc(vec![clip(0, 5_000), gone.clone(), evil.clone()], vec![sr(1, 3_000, 4_000, 2.0)]);
        d.zooms = vec![zoom(1, 2_500, 3_500)];
        d.annotations = vec![ann(1, 2_000, 3_000, text("kept"))];
        d.audio_tracks = vec![track(1, 2_000, 9_000)];
        let takes = TakeMetas::from([("../t2".to_string(), TakeMeta { duration_ms: 1_000, ..TakeMeta::default() })]);
        let n = normalize(d.clone(), 10_000, &takes);
        assert_eq!(n.clips, vec![clip(0, 5_000), gone, evil]);
        assert_eq!((n.speed.len(), n.zooms.len(), n.annotations.len(), n.audio_tracks.len()), (1, 1, 1, 1));
        assert_eq!(n.zooms[0].clip, 1);
        // Stable: normalising again changes nothing (no loss on the next save).
        assert_eq!(normalize(n.clone(), 10_000, &takes), n);
        // As it plays, the missing clips last no time; indices stay.
        assert!(take_missing(&n.clips[1], &takes) && take_missing(&n.clips[2], &takes));
        let play = playable(&n, &takes);
        assert_eq!(play.clips.len(), 3);
        assert_eq!(programme_duration_ms(&play), 5_000);
        assert_eq!(programme_to_source(&play, 4_999), Some((0, 4_999)));
        assert_eq!(scene_clip_indices(&n, &takes), vec![Some(0), None, None]);
    }

    #[test]
    fn serde_roundtrip_v3() {
        let mut d = default_doc(&meta(10_000), None);
        d.clips[0].source = Some("take-2".into());
        d.annotations = vec![
            ann(0, 0, 2_000, text("Hallo")),
            ann(0, 0, 2_000, AnnotationBody::Image { file: "logo.png".into() }),
            ann(0, 0, 2_000, AnnotationBody::Arrow { direction: ArrowDirection::DownRight, color: "#34B27B".into(), stroke_width: 4.0 }),
            ann(0, 0, 2_000, AnnotationBody::Blur(BlurBody { shape: BlurShape::Freehand, ..BlurBody::default() })),
        ];
        d.audio_tracks = vec![AudioTrack { kind: TrackKind::Voiceover, span_ms: Some(900), ..track(0, 100, 1_000) }];
        d.system_gain_db = -3.0;
        d.camera = CameraStyle { anchor: CameraAnchor::TopRight, roundness: Some(0.2), ..CameraStyle::default() };
        d.camera.background.mode = CameraBgMode::Transparent;
        d.layout_regions = vec![LayoutRegion { id: "l".into(), clip: 0, start_ms: 500, preset: LayoutPreset::VerticalStack }];
        let j = serde_json::to_value(&d).unwrap();
        assert_eq!(j["version"], EDITOR_VERSION);
        assert_eq!(j["camera"]["preset"], "picture-in-picture");
        assert_eq!(j["camera"]["sizePct"], 40.0);
        assert_eq!(j["camera"]["anchor"], "top-right");
        assert_eq!(j["camera"]["reactiveZoom"], true);
        assert_eq!(j["camera"]["background"]["mode"], "transparent");
        assert_eq!(j["layoutRegions"][0], serde_json::json!({"id": "l", "clip": 0, "startMs": 500, "preset": "vertical-stack"}));
        assert_eq!(j["clips"][0]["source"], "take-2");
        assert_eq!(j["annotations"][0]["body"]["kind"], "text");
        assert_eq!(j["annotations"][0]["body"]["fontFamily"], "Inter");
        assert_eq!(j["annotations"][0]["zIndex"], 1);
        assert_eq!(j["annotations"][2]["body"]["direction"], "down-right");
        assert_eq!(j["annotations"][2]["body"]["strokeWidth"], 4.0);
        assert_eq!(j["annotations"][3]["body"]["blockSize"], 12.0);
        assert_eq!(j["annotations"][3]["body"]["points"][0], serde_json::json!([10.0, 30.0]));
        assert_eq!(j["audioTracks"][0]["kind"], "voiceover");
        assert_eq!(j["audioTracks"][0]["spanMs"], 900);
        assert_eq!(j["systemGainDb"], -3.0);
        let anim = serde_json::to_value(TextAnimation::SlideLeft).unwrap();
        assert_eq!(anim, "slide-left");
        let back: EditDoc = serde_json::from_value(j).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn broken_annotation_only_drops_itself() {
        let json = serde_json::json!({
            "version": 3,
            "clips": [{"startMs": 0, "endMs": 4000}, {"startMs": 5000, "endMs": 9000}],
            "zooms": [{"id": "z1", "clip": 0, "startMs": 1000, "endMs": 3000, "depth": 4, "focusX": 0.2, "focusY": 0.3}],
            "annotations": [
                {"id": "good", "clip": 0, "startMs": 0, "endMs": 1000, "body": {"kind": "text", "content": "Hi"}},
                {"id": "bad-kind", "clip": 0, "startMs": 0, "endMs": 1000, "body": {"kind": "sticker"}},
                {"id": "bad-shape", "clip": "zero", "startMs": 0, "endMs": 1000, "body": {"kind": "arrow"}},
                {"id": "arrow", "clip": 1, "startMs": 6000, "endMs": 7000, "body": {"kind": "arrow"}}
            ],
            "audioTracks": [
                {"id": "t", "file": "a.mp3", "clip": 0, "durationMs": 1000},
                {"id": "broken", "kind": "podcast", "file": "b.mp3", "clip": 0, "durationMs": 1000}
            ]
        });
        let d = load(&project_with(json), &NO_TAKES);
        assert_eq!(d.clips, vec![clip(0, 4_000), clip(5_000, 9_000)]);
        assert_eq!(d.zooms.len(), 1);
        assert_eq!(d.annotations.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["good", "arrow"]);
        let AnnotationBody::Text(t) = &d.annotations[0].body else { panic!("text") };
        assert_eq!((t.content.as_str(), t.font_family.as_str(), t.font_size), ("Hi", "Inter", 32.0));
        assert_eq!((d.annotations[0].x, d.annotations[0].w), (50.0, 30.0));
        let AnnotationBody::Arrow { color, stroke_width, direction } = &d.annotations[1].body else { panic!("arrow") };
        assert_eq!((color.as_str(), *stroke_width, *direction), ("#34B27B", 4.0, ArrowDirection::Right));
        assert_eq!(d.audio_tracks.len(), 1);
        assert_eq!((d.audio_tracks[0].kind, d.audio_tracks[0].gain_db), (TrackKind::Music, 0.0));
    }

    #[test]
    fn z_index_is_capped() {
        let top = Annotation { z_index: i32::MAX, ..ann(0, 0, 1_000, text("t")) };
        assert_eq!(Annotation::new("n".into(), 0, 0, 1_000, text("n"), std::slice::from_ref(&top)).z_index, MAX_Z_INDEX);
        let n = normalize(with_annotations(vec![clip(0, 10_000)], vec![top]), 10_000, &NO_TAKES);
        assert_eq!(n.annotations[0].z_index, MAX_Z_INDEX);
    }

    // ── clip operations + sources ────────────────────────────────────────────

    /// Three clips, each carrying one of every per-clip thing, so a re-index shows up in all.
    fn three_clip_doc() -> EditDoc {
        let mut d = doc(
            vec![clip(0, 3_000), clip(3_000, 6_000), clip(6_000, 9_000)],
            vec![sr(0, 500, 1_000, 2.0), sr(2, 7_000, 8_000, 0.5)],
        );
        d.crop = vec![Some(CropRect { x: 0.1, y: 0.1, w: 0.5, h: 0.5 }), None, None];
        d.zooms = vec![zoom(0, 0, 1_000), zoom(1, 3_500, 4_500), zoom(2, 6_500, 7_500)];
        d.annotations = vec![ann(0, 100, 900, text("first")), ann(2, 6_100, 6_900, text("third"))];
        d.audio_tracks = vec![track(0, 0, 60_000), track(2, 6_000, 60_000)];
        d
    }

    #[test]
    fn reorder_moves_annotations_and_tracks() {
        let d = three_clip_doc();
        // Clip 0 to the end: old 1 → 0, old 2 → 1, old 0 → 2.
        let m = move_clip(&d, 0, 2);
        assert_eq!(m.clips, vec![clip(3_000, 6_000), clip(6_000, 9_000), clip(0, 3_000)]);
        assert_eq!(m.crop, vec![None, None, d.crop[0]]);
        let ann_clip = |m: &EditDoc, label: &str| {
            m.annotations
                .iter()
                .find(|a| matches!(&a.body, AnnotationBody::Text(t) if t.content == label))
                .map(|a| a.clip)
        };
        assert_eq!(ann_clip(&m, "first"), Some(2));
        assert_eq!(ann_clip(&m, "third"), Some(1));
        let track_clip = |id: &str| m.audio_tracks.iter().find(|t| t.id == id).map(|t| t.clip);
        assert_eq!(track_clip("t0-0"), Some(2));
        assert_eq!(track_clip("t2-6000"), Some(1));
        let zoom_clip = |id: &str| m.zooms.iter().find(|z| z.id == id).map(|z| z.clip);
        assert_eq!((zoom_clip("z0-0"), zoom_clip("z1-3500"), zoom_clip("z2-6500")), (Some(2), Some(0), Some(1)));
        let speeds: Vec<_> = m.speed.iter().map(|r| (r.clip, r.start_ms)).collect();
        assert_eq!(speeds, vec![(1, 7_000), (2, 500)]);
        // Source times never change, only which clip they belong to.
        assert_eq!(m.annotations.iter().map(|a| a.start_ms).collect::<HashSet<_>>(), HashSet::from([100, 6_100]));
        // Moving back restores the document (same things, same order of clips).
        let back = move_clip(&m, 2, 0);
        assert_eq!(back.clips, d.clips);
        assert_eq!(back.crop, d.crop);
        // Out of range / no move: unchanged.
        assert_eq!(move_clip(&d, 0, 3), d);
        assert_eq!(move_clip(&d, 1, 1), d);
    }

    #[test]
    fn reorder_survives_normalize_and_changes_the_programme_order() {
        let d = three_clip_doc();
        let m = normalize(move_clip(&d, 2, 0), 10_000, &NO_TAKES);
        assert_eq!(programme_to_source(&m, 0), Some((0, 6_000)));
        assert_eq!(m.annotations.len(), 2);
        assert_eq!(m.audio_tracks.len(), 2);
    }

    #[test]
    fn duplicate_copies_regions_with_new_ids() {
        let d = three_clip_doc();
        let c = duplicate_clip(&d, 0);
        assert_eq!(c.clips, vec![clip(0, 3_000), clip(0, 3_000), clip(3_000, 6_000), clip(6_000, 9_000)]);
        assert_eq!(c.crop, vec![d.crop[0], d.crop[0], None, None]);
        // The copy (clip 1) has its own zoom/annotation/track under a new id; later clips shifted.
        let zooms: Vec<_> = c.zooms.iter().map(|z| (z.clip, z.id.as_str())).collect();
        assert_eq!(zooms, vec![(0, "z0-0"), (1, "z0-0-copy"), (2, "z1-3500"), (3, "z2-6500")]);
        let anns: Vec<_> = c.annotations.iter().map(|a| (a.clip, a.id.as_str())).collect();
        assert!(anns.contains(&(0, "a0-100")) && anns.contains(&(1, "a0-100-copy")) && anns.contains(&(3, "a2-6100")));
        let tracks: Vec<_> = c.audio_tracks.iter().map(|t| (t.clip, t.id.as_str())).collect();
        assert!(tracks.contains(&(0, "t0-0")) && tracks.contains(&(1, "t0-0-copy")) && tracks.contains(&(3, "t2-6000")));
        let speeds: Vec<_> = c.speed.iter().map(|r| (r.clip, r.start_ms)).collect();
        assert_eq!(speeds, vec![(0, 500), (1, 500), (3, 7_000)]);
        // A second copy of the same clip gets yet another id; all ids stay unique.
        let c2 = duplicate_clip(&c, 0);
        let ids: Vec<_> = c2.zooms.iter().map(|z| z.id.clone()).collect();
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), ids.len());
        assert!(ids.contains(&"z0-0-copy2".to_string()));
        // Normalize keeps every copy (copies may overlap in source time: different clips).
        let n = normalize(c, 10_000, &NO_TAKES);
        assert_eq!((n.clips.len(), n.zooms.len(), n.annotations.len(), n.audio_tracks.len()), (4, 4, 3, 3));
        assert_eq!(duplicate_clip(&d, 3), d);
    }

    #[test]
    fn append_take_adds_clip_at_end() {
        let d = three_clip_doc();
        let meta = TakeMeta { duration_ms: 12_345, width: 1280, height: 720, ..TakeMeta::default() };
        let a = append_take(&d, "t9", &meta);
        assert_eq!(a.clips.len(), 4);
        assert_eq!(a.clips[3], EditClip { start_ms: 0, end_ms: 12_345, source: Some("t9".into()) });
        assert_eq!(a.crop.len(), 4);
        assert_eq!(a.crop[3], None);
        // Nothing else moves.
        assert_eq!((a.zooms.clone(), a.annotations.clone(), a.speed.clone()), (d.zooms.clone(), d.annotations.clone(), d.speed.clone()));
        let takes = TakeMetas::from([("t9".to_string(), meta)]);
        let n = normalize(a, 10_000, &takes);
        assert_eq!(n.clips[3].end_ms, 12_345);
        assert_eq!(programme_duration_ms(&n), programme_duration_ms(&d) + 12_345);
        // The take's own size is that clip's source size.
        assert_eq!(clip_source_size(&meta_fixture(), &takes, &n, 3), (1280, 720));
        assert_eq!(clip_source_size(&meta_fixture(), &takes, &n, 1), (3024, 1964));
    }

    fn meta_fixture() -> RecordingMeta {
        meta(10_000)
    }

    #[test]
    fn take_dir_never_leaves_the_takes_folder() {
        let p = Path::new("/p");
        assert_eq!(take_dir(p, None), PathBuf::from("/p"));
        assert_eq!(take_dir(p, Some("ab-12_C")), PathBuf::from("/p/takes/ab-12_C"));
        for bad in ["", "..", "../x", "a/b", "a b", "/abs", "ä"] {
            assert_eq!(take_dir(p, Some(bad)), PathBuf::from("/p/takes/.invalid"), "{bad:?}");
        }
        assert!(!valid_take_id(&"x".repeat(65)));
    }

    // ── camera (v4) ──────────────────────────────────────────────────────────

    fn v3_json() -> serde_json::Value {
        serde_json::json!({
            "version": 3,
            "clips": [{"startMs": 0, "endMs": 4000}],
            "appearance": {"padding": 20.0},
            "autoZoomDone": true
        })
    }

    fn camera_meta() -> crate::record::project::CameraMeta {
        crate::record::project::CameraMeta { width: 1920, height: 1080, fps: 30, offset_ms: 0, device_name: "Cam".into(), ended_early_ms: None }
    }

    fn lr(id: &str, clip: usize, start_ms: u64, preset: LayoutPreset) -> LayoutRegion {
        LayoutRegion { id: id.into(), clip, start_ms, preset }
    }

    #[test]
    fn v3_doc_migrates_with_camera_default() {
        // Every project gets the standard look — with or without a camera (ruling R3).
        let d = migrate(v3_json()).expect("v3 migrates");
        assert_eq!(d.version, EDITOR_VERSION);
        assert_eq!(d.camera, CameraStyle::default());
        assert_eq!(d.camera.preset, LayoutPreset::PictureInPicture);
        assert_eq!(d.camera.size_pct, 40.0);
        assert_eq!(d.camera.anchor, CameraAnchor::BottomRight);
        assert_eq!(d.camera.mask, CameraMask::Square);
        assert_eq!(d.camera.resolved_roundness(), 0.7);
        assert!(d.camera.reactive_zoom && !d.camera.mirror);
        assert_eq!(d.camera.background, CameraBackground { mode: CameraBgMode::None, blur: 0.5, custom: Background::Wallpaper { id: DEFAULT_WALLPAPER.into() } });
        assert!(d.layout_regions.is_empty());
        // Through load: a camera-less project stores PiP too (no `no-webcam` default).
        let mut p = project_with(v3_json());
        assert_eq!(load(&p, &NO_TAKES).camera.preset, LayoutPreset::PictureInPicture);
        p.recording.camera = Some(camera_meta());
        assert_eq!(load(&p, &NO_TAKES).camera.preset, LayoutPreset::PictureInPicture);
        // A stored v4 document keeps its own camera look, including an explicit "without camera".
        for preset in [LayoutPreset::DualFrame, LayoutPreset::NoWebcam] {
            let mut v4 = d.clone();
            v4.camera.preset = preset;
            store(&mut p, &v4);
            assert_eq!(load(&p, &NO_TAKES).camera.preset, preset);
        }
        // A new project, camera or not: the same standard.
        assert_eq!(default_doc(&meta(1_000), None).camera, CameraStyle::default());
        let m = RecordingMeta { camera: Some(camera_meta()), ..meta(1_000) };
        assert_eq!(default_doc(&m, None).camera, CameraStyle::default());
        // A v4 block without a `camera` key reads the same default (serde).
        let bare: EditDoc = serde_json::from_value(serde_json::json!({"version": 4, "clips": [{"startMs": 0, "endMs": 1000}]})).unwrap();
        assert_eq!(bare.camera, CameraStyle::default());
    }

    #[test]
    fn camera_style_is_clamped() {
        let mut d = doc(vec![clip(0, 10_000)], vec![]);
        d.camera = CameraStyle {
            size_pct: 90.0,
            roundness: Some(2.0),
            crop: Some(CropRect { x: 0.98, y: -1.0, w: 0.0, h: 3.0 }),
            background: CameraBackground { blur: f64::NAN, ..CameraBackground::default() },
            ..CameraStyle::default()
        };
        let n = normalize(d.clone(), 10_000, &NO_TAKES);
        assert_eq!(n.camera.size_pct, 60.0);
        assert_eq!(n.camera.roundness, Some(1.0));
        assert_eq!(n.camera.background.blur, DEFAULT_CAMERA_BLUR);
        assert_eq!(n.camera.crop, Some(CropRect { x: 0.95, y: 0.0, w: 0.05, h: 1.0 }));
        d.camera.size_pct = 3.0;
        d.camera.roundness = Some(f64::NAN);
        d.camera.background.blur = -2.0;
        let n = normalize(d, 10_000, &NO_TAKES);
        assert_eq!((n.camera.size_pct, n.camera.roundness, n.camera.background.blur), (15.0, None, 0.0));
        // A NaN size goes back to the standard; the standard roundness follows the mask.
        let mut d = doc(vec![clip(0, 10_000)], vec![]);
        d.camera.size_pct = f64::NAN;
        d.camera.mask = CameraMask::Rectangle;
        let n = normalize(d, 10_000, &NO_TAKES);
        assert_eq!((n.camera.size_pct, n.camera.resolved_roundness()), (40.0, 0.4));
    }

    #[test]
    fn layout_region_follows_split_and_reorder() {
        // What the TS split leaves behind: clip 0 cut at 3 s, the 5 s region moved to clip 1,
        // a region at the cut carries the preset that was active there.
        let mut d = doc(vec![clip(0, 3_000), clip(3_000, 10_000), clip(10_000, 12_000)], vec![]);
        d.crop = vec![None; 3];
        d.layout_regions = vec![
            lr("a", 0, 1_000, LayoutPreset::DualFrame),
            lr("cut", 1, 3_000, LayoutPreset::DualFrame),
            lr("b", 1, 5_000, LayoutPreset::VerticalStack),
            // Before its clip's start: clamped to the start; beyond its end: dropped.
            lr("early", 2, 9_000, LayoutPreset::CameraOnly),
            lr("late", 2, 12_000, LayoutPreset::NoWebcam),
        ];
        let n = normalize(d, 20_000, &NO_TAKES);
        let regs = |n: &EditDoc| n.layout_regions.iter().map(|r| (r.id.clone(), r.clip, r.start_ms, r.preset)).collect::<Vec<_>>();
        assert_eq!(
            regs(&n),
            vec![
                ("a".into(), 0, 1_000, LayoutPreset::DualFrame),
                ("cut".into(), 1, 3_000, LayoutPreset::DualFrame),
                ("b".into(), 1, 5_000, LayoutPreset::VerticalStack),
                ("early".into(), 2, 10_000, LayoutPreset::CameraOnly),
            ]
        );
        // Reorder: clip 0 to the end; the regions follow their clip.
        let m = move_clip(&n, 0, 2);
        assert_eq!(m.layout_regions.iter().map(|r| (r.id.as_str(), r.clip)).collect::<Vec<_>>(), vec![("cut", 0), ("b", 0), ("early", 1), ("a", 2)]);
        // Duplicate: the copy gets copies of its clip's regions under new ids.
        let dup = duplicate_clip(&n, 1);
        let ids: Vec<_> = dup.layout_regions.iter().filter(|r| r.clip == 2).map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["cut-copy", "b-copy"]);
        assert_eq!(dup.layout_regions.iter().find(|r| r.id == "early").unwrap().clip, 3);
        // A deleted clip (a sliver) takes its regions along; a repeated start keeps the later one.
        let mut d = doc(vec![clip(0, 50), clip(2_000, 9_000)], vec![]);
        d.layout_regions = vec![lr("x", 0, 0, LayoutPreset::DualFrame), lr("y", 1, 4_000, LayoutPreset::DualFrame), lr("y", 1, 4_000, LayoutPreset::CameraOnly)];
        let n = normalize(d, 10_000, &NO_TAKES);
        assert_eq!(n.layout_regions.len(), 1);
        assert_eq!((n.layout_regions[0].clip, n.layout_regions[0].start_ms, n.layout_regions[0].preset), (0, 4_000, LayoutPreset::CameraOnly));
        assert!(!n.layout_regions[0].id.is_empty());
    }

    #[test]
    fn layout_regions_get_unique_ids() {
        let mut d = doc(vec![clip(0, 10_000)], vec![]);
        d.layout_regions = vec![lr("", 0, 1_000, LayoutPreset::DualFrame), lr("same", 0, 2_000, LayoutPreset::DualFrame), lr("same", 0, 3_000, LayoutPreset::CameraOnly)];
        let n = normalize(d, 10_000, &NO_TAKES);
        let ids: HashSet<_> = n.layout_regions.iter().map(|r| r.id.clone()).collect();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains("same"));
    }

    #[test]
    fn auto_ratio_follows_the_camera_layout() {
        let m = RecordingMeta { camera: Some(camera_meta()), ..meta(10_000) };
        let mut d = doc(vec![clip(0, 10_000)], vec![]);
        d.appearance.padding = 20.0;
        let ratio = |d: &EditDoc, m: &RecordingMeta| auto_ratio(m, &NO_TAKES, d);
        d.camera.preset = LayoutPreset::DualFrame;
        let screen_only = super::super::layout::auto_aspect(3024.0 / 1964.0, 20.0, Frame::None, LayoutPreset::NoWebcam);
        let dual = super::super::layout::auto_aspect(3024.0 / 1964.0, 20.0, Frame::None, LayoutPreset::DualFrame);
        assert!((dual - screen_only).abs() > 0.1, "{dual} vs {screen_only}");
        assert_eq!(ratio(&d, &m), dual);
        // Without a camera on the first clip the preset resolves to no-webcam.
        assert_eq!(ratio(&d, &meta(10_000)), screen_only);
        // PiP floats over the screen and does not change the shape.
        d.camera.preset = LayoutPreset::PictureInPicture;
        assert_eq!(ratio(&d, &m), screen_only);
    }

    #[test]
    fn clip_camera_follows_source() {
        let m = RecordingMeta { camera: Some(camera_meta()), ..meta(10_000) };
        let mut d = doc(vec![clip(0, 4_000), EditClip { start_ms: 0, end_ms: 2_000, source: Some("t".into()) }], vec![]);
        let takes = TakeMetas::from([("t".to_string(), TakeMeta { duration_ms: 5_000, camera: Some((1280, 720)), camera_offset_ms: 40, ..TakeMeta::default() })]);
        assert_eq!(clip_camera(&m, &takes, &d, 0), Some(((1920, 1080), 0)));
        assert_eq!(clip_camera(&m, &takes, &d, 1), Some(((1280, 720), 40)));
        assert_eq!(clip_camera(&meta(10_000), &takes, &d, 0), None);
        // The framing crop shapes the camera's box (upstream `webcamBoxSourceSize`).
        d.camera.crop = Some(CropRect { x: 0.0, y: 0.0, w: 0.5, h: 1.0 });
        assert_eq!(clip_camera_box(&m, &takes, &d, 0), Some((960, 1080)));
        assert_eq!(clip_camera_box(&m, &NO_TAKES, &d, 1), None, "a missing take has no camera");
    }
}
