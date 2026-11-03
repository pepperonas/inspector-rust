//! Where the screen sits in the output frame: padding, device-frame room, the Auto aspect and the
//! deepest zoom a clip may take.
//!
//! A port of the **no-webcam** branch of OpenScreen v2.1.2's `src/lib/compositeLayout.ts`
//! (`paddingFit`, `paddedContentSize`, `autoFrameAspect`, `autoFormatAspect`, `centerScreen`,
//! `centerRect`, `snapRect`) and of `src/lib/frameFootprint.ts`, whose constants mirror the
//! compositor's `frame_geometry.rs` (`WINDOW_FRAME_BAR_FRAC` … `DEV_FOOT_Z`). The compositor
//! draws the frames; this only measures them so the padding is counted from what the frame
//! draws (`layout.screenRect` already includes that room). Pure.
//!
//! Since stage 5 also the **camera** branches of `computeCompositeLayout`
//! (`compositeLayout.ts:447-646`): picture-in-picture, the two block layouts (dual-frame,
//! vertical-stack), camera-only, plus `restingCompositionAspect` for the Auto aspect. What the
//! compositor recomputes per frame (reactive shrink, layout morphs, the camera's shadow) is
//! not here. The no-webcam case is [`screen_rect`], unchanged.

use super::editor::Frame;
use serde::{Deserialize, Serialize};

/// A rect in fractions (0..1) of the output frame, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Upstream `LayoutPreset` (`layoutTypes.ts`), serialised under the same names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LayoutPreset {
    #[default]
    PictureInPicture,
    DualFrame,
    VerticalStack,
    CameraOnly,
    NoWebcam,
}

impl LayoutPreset {
    /// `resolveWebcamLayoutPreset`: without a camera to lay out the answer is `NoWebcam`,
    /// whatever was picked (a block preset would squeeze the screen into half an empty frame).
    pub fn resolve(self, has_camera: bool) -> Self {
        if has_camera { self } else { LayoutPreset::NoWebcam }
    }

    /// `isWebcamBlockLayout`: screen and camera welded into one block.
    pub fn is_block(self) -> bool {
        matches!(self, LayoutPreset::DualFrame | LayoutPreset::VerticalStack)
    }

    /// `supportsWebcamReactiveZoom`: only the free-floating bubble may shrink during a zoom.
    pub fn supports_reactive_zoom(self) -> bool {
        self == LayoutPreset::PictureInPicture
    }
}

/// Where the picture-in-picture camera sits (`WebcamAnchor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraAnchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    #[default]
    BottomRight,
}

impl CameraAnchor {
    pub const ALL: [CameraAnchor; 8] = [
        CameraAnchor::TopLeft,
        CameraAnchor::Top,
        CameraAnchor::TopRight,
        CameraAnchor::Left,
        CameraAnchor::Right,
        CameraAnchor::BottomLeft,
        CameraAnchor::Bottom,
        CameraAnchor::BottomRight,
    ];

    /// `webcamAnchorFractions`: share of the free room left of / above the camera (0, ½, 1).
    pub fn fractions(self) -> (f64, f64) {
        use CameraAnchor::*;
        let fx = match self {
            TopLeft | Left | BottomLeft => 0.0,
            TopRight | Right | BottomRight => 1.0,
            Top | Bottom => 0.5,
        };
        let fy = match self {
            TopLeft | Top | TopRight => 0.0,
            BottomLeft | Bottom | BottomRight => 1.0,
            Left | Right => 0.5,
        };
        (fx, fy)
    }
}

/// The camera mask's proportion: square = the short side both ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CameraMask {
    #[default]
    Square,
    Rectangle,
}

/// The look settings the layout reads. Defaults = OpenScreen `projectDefaults.ts:253-262`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraLook {
    pub preset: LayoutPreset,
    /// Picture-in-picture size, percent of the frame's short side (clamped 15..=60).
    pub size_pct: f64,
    pub mask: CameraMask,
    /// 0 square corners … 1 fully round (picture-in-picture only).
    pub roundness: f64,
    pub anchor: CameraAnchor,
}

impl Default for CameraLook {
    fn default() -> Self {
        CameraLook { preset: LayoutPreset::PictureInPicture, size_pct: 40.0, mask: CameraMask::Square, roundness: 0.7, anchor: CameraAnchor::BottomRight }
    }
}

impl CameraLook {
    /// The look with no camera at all (`no-webcam`).
    pub fn none() -> Self {
        CameraLook { preset: LayoutPreset::NoWebcam, ..CameraLook::default() }
    }
}

/// `WebcamCompositeLayout`, rects in fractions (0..1) of the output, radii as a fraction of
/// their own box's short side (upstream `radiusFractionOf`), `None` = not rounded by the layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompositeLayout {
    pub screen: Rect,
    pub camera: Option<Rect>,
    pub screen_radius_frac: Option<f64>,
    pub camera_radius_frac: Option<f64>,
    /// Block layouts: the screen video covers its box and the box masks zooms/tilts.
    pub screen_cover: bool,
    /// The RESOLVED mask (block layouts and camera-only always cut a rectangle).
    pub mask: CameraMask,
}

/// Picture-in-picture: margin from the border, fraction of the short side (`MARGIN_FRACTION`).
pub const PIP_MARGIN_FRACTION: f64 = 0.02;
/// Block layouts: gap between screen and camera, fraction of the screen's width.
pub const BLOCK_GAP_FRACTION: f64 = 0.02;
/// Block layouts: the camera stays within `[1/T, T]` of square.
pub const BLOCK_CAMERA_ASPECT_TOLERANCE: f64 = 1.25;
pub const CAMERA_SIZE_MIN: f64 = 15.0;
pub const CAMERA_SIZE_MAX: f64 = 60.0;

/// `[left, top, right, bottom]` around the screen box, in the unit of the screen size given.
type Insets = [f64; 4];

// Mirror of `frame_geometry.rs`, every length in frame units (`frame_unit`).
pub const WINDOW_FRAME_BAR_FRAC: f64 = 0.04;
pub const WINDOW_FRAME_LINE_FRAC: f64 = 0.0012;
const DEV_RIM: f64 = 0.0053;
const DEV_EYE_MIN: f64 = 9.6;
const PERSPECTIVE_FACTOR: f64 = 1.6;
const DEV_DECK_LEN: f64 = 1.3;
const DEV_DECK_GAP: f64 = 0.0093;
const DEV_DECK_THICK: f64 = 0.0446;
const DEV_DECK_EYE_CLEARANCE: f64 = 0.0178;
// Upstream's literal (≈ π/3), kept as written so the footprint matches the compositor's.
#[allow(clippy::approx_constant)]
const DEV_HINGE_MIN_RAD: f64 = 1.047198;
const DEV_HINGE_MAX_RAD: f64 = 2.356194;
const DEV_NECK_W: f64 = 0.255;
const DEV_NECK_LEN: f64 = 0.177;
const DEV_FOOT_W: f64 = 0.456;
const DEV_FOOT_H: f64 = 0.0456;
const DEV_STAND_Z: f64 = 0.0219;
const DEV_FOOT_Z: f64 = 0.195;

/// How far a recording may be blown up on the finished frame: output px per source px.
pub const MAX_SOURCE_MAGNIFICATION: f64 = 2.0;
/// The frame magnification is measured on: the short side of the default (1080p) export.
pub const MAGNIFICATION_REFERENCE_PX: f64 = 1080.0;
pub const MAX_ZOOM_SCALE: f64 = super::editor::MAX_ZOOM_SCALE;
pub const MIN_ZOOM_SCALE: f64 = super::editor::MIN_ZOOM_SCALE;

/// Black glass of each device's bezel: (sides and top, bottom), and its thickness.
fn device(frame: Frame) -> ((f64, f64), f64) {
    match frame {
        Frame::Laptop => ((0.0356, 0.0409), 0.0284),
        Frame::Phone => ((0.014, 0.014), 0.05),
        _ => ((0.0178, 0.0178), 0.0284), // monitor
    }
}

/// The share of the frame the composition keeps at `padding_pct` (0..100): 1 at 0 %, 0.6 at 100 %.
pub fn padding_fit(padding_pct: f64) -> f64 {
    let p = if padding_pct.is_finite() { padding_pct.clamp(0.0, 100.0) } else { 0.0 };
    1.0 - p / 100.0 * 0.4
}

/// `frameUnit`: the canvas' short side scaled by the share of the canvas the screen fills in its
/// fuller dimension — in the unit of `screen`.
fn frame_unit(screen: (f64, f64), canvas: (f64, f64)) -> f64 {
    (screen.0 / canvas.0).max(screen.1 / canvas.1) * canvas.0.min(canvas.1)
}

/// `deviceDeckAngle`: the laptop's deck, seen edge-on from the flat camera.
fn deck_angle(half: (f64, f64), chin: f64, thick: f64) -> f64 {
    let eye = (2.0 * half.0.min(half.1) * PERSPECTIVE_FACTOR).max(DEV_EYE_MIN);
    let h = half.1 + chin;
    let opening = ((eye + thick * 0.5) / (h - DEV_DECK_EYE_CLEARANCE).max(1e-3)).atan();
    std::f64::consts::PI - opening.clamp(DEV_HINGE_MIN_RAD, DEV_HINGE_MAX_RAD)
}

fn box_corners(lo: [f64; 3], hi: [f64; 3]) -> [[f64; 3]; 8] {
    std::array::from_fn(|i| {
        [
            if i & 1 != 0 { hi[0] } else { lo[0] },
            if i & 2 != 0 { hi[1] } else { lo[1] },
            if i & 4 != 0 { hi[2] } else { lo[2] },
        ]
    })
}

/// `frameFootprint`: `(body, outer)` insets of `frame` around a screen box of size `screen`, for a
/// frame unit `unit` in the same unit; the insets come back in it too.
fn frame_footprint(frame: Frame, screen: (f64, f64), unit: f64) -> (Insets, Insets) {
    let none = [0.0; 4];
    if frame == Frame::None || unit.is_nan() || unit <= 0.0 {
        return (none, none);
    }
    if frame == Frame::Window {
        let line = WINDOW_FRAME_LINE_FRAC * unit;
        let body = [line, WINDOW_FRAME_BAR_FRAC * unit, line, line];
        return (body, body);
    }
    let ((side, bottom), thick) = device(frame);
    let margins = [side + DEV_RIM, side + DEV_RIM, side + DEV_RIM, bottom + DEV_RIM];
    let half = (screen.0 / (2.0 * unit), screen.1 / (2.0 * unit));
    let lo = [-half.0 - margins[0], -half.1 - margins[1]];
    let hi = [half.0 + margins[2], half.1 + margins[3]];
    let mut extra: Vec<[f64; 3]> = Vec::new();
    if frame == Frame::Laptop {
        let angle = deck_angle(half, margins[3], thick);
        let (ca, sa) = (angle.cos(), angle.sin());
        let hinge = (hi[1], -thick * 0.5);
        for [x, y, z] in box_corners([lo[0], DEV_DECK_GAP, -DEV_DECK_THICK], [hi[0], DEV_DECK_GAP + DEV_DECK_LEN, 0.0]) {
            extra.push([x, hinge.0 + y * ca - z * sa, hinge.1 + y * sa + z * ca]);
        }
    } else if frame == Frame::Monitor {
        let (y0, z0) = (hi[1], -thick * 0.5);
        extra.extend(box_corners([-DEV_NECK_W, y0 - 0.01, z0 - DEV_STAND_Z], [DEV_NECK_W, y0 + DEV_NECK_LEN, z0]));
        extra.extend(box_corners(
            [-DEV_FOOT_W, y0 + DEV_NECK_LEN - 0.01, z0 - DEV_FOOT_Z],
            [DEV_FOOT_W, y0 + DEV_NECK_LEN + DEV_FOOT_H, z0],
        ));
    }
    let mut outer = [lo[0], lo[1], hi[0], hi[1]];
    for [x, y, z] in extra {
        let k = DEV_EYE_MIN / (DEV_EYE_MIN - z);
        outer[0] = outer[0].min(x * k);
        outer[1] = outer[1].min(y * k);
        outer[2] = outer[2].max(x * k);
        outer[3] = outer[3].max(y * k);
    }
    (
        margins.map(|m| m * unit),
        [
            (-outer[0] - half.0) * unit,
            (-outer[1] - half.1) * unit,
            (outer[2] - half.0) * unit,
            (outer[3] - half.1) * unit,
        ],
    )
}

/// `framedScreen`: the screen and its frame in unit space (screen exactly 1 wide):
/// `(screen size, body insets, outer insets, element size)`.
fn framed_screen(screen_aspect: f64, canvas: (f64, f64), frame: Frame) -> ((f64, f64), Insets, Insets, (f64, f64)) {
    let screen = (1.0, 1.0 / screen_aspect);
    let (body, outer) = frame_footprint(frame, screen, frame_unit(screen, canvas));
    let element = (outer[0] + screen.0 + outer[2], outer[1] + screen.1 + outer[3]);
    (screen, body, outer, element)
}

/// `blockUnitSize`: element + gap + a camera `along` long on the split axis, in unit space.
fn block_unit_size(element: (f64, f64), row: bool, along: f64) -> (f64, f64) {
    if row {
        (element.0 + BLOCK_GAP_FRACTION + along, element.1)
    } else {
        (element.0, element.1 + BLOCK_GAP_FRACTION + along)
    }
}

/// `restingCompositionAspect`: the composition's own shape. A block layout adds its camera at
/// rest — square, lined up with the frame's body; picture-in-picture floats over the screen.
fn resting_aspect(screen_aspect: f64, canvas: (f64, f64), frame: Frame, preset: LayoutPreset) -> f64 {
    let (screen, body, _, element) = framed_screen(screen_aspect, canvas, frame);
    if !preset.is_block() {
        return element.0 / element.1;
    }
    let row = preset == LayoutPreset::DualFrame;
    let cross = if row { body[1] + screen.1 + body[3] } else { body[0] + screen.0 + body[2] };
    let block = block_unit_size(element, row, cross);
    block.0 / block.1
}

/// `autoFrameAspect`: elongation `e' = fit · e + (1 − fit)`, orientation kept.
fn auto_frame_aspect(composition_aspect: f64, padding_pct: f64) -> f64 {
    let fit = padding_fit(padding_pct);
    let elongation = composition_aspect.max(1.0 / composition_aspect);
    let framed = fit * elongation + (1.0 - fit);
    if composition_aspect >= 1.0 { framed } else { 1.0 / framed }
}

fn sane_aspect(a: f64) -> f64 {
    if a.is_finite() && a > 0.0 { a } else { 16.0 / 9.0 }
}

/// `autoFormatAspect`: the composition's resting shape (the block camera included) plus an even
/// padding border; under a device frame 8 fixed-point passes (the frame's thickness follows the
/// scene's shape). `preset` is the RESOLVED layout (`LayoutPreset::resolve`).
pub fn auto_aspect(screen_aspect: f64, padding_pct: f64, frame: Frame, preset: LayoutPreset) -> f64 {
    let screen_aspect = sane_aspect(screen_aspect);
    let unframed = (screen_aspect, 1.0);
    let mut aspect = auto_frame_aspect(resting_aspect(screen_aspect, unframed, Frame::None, preset), padding_pct);
    if frame == Frame::None {
        return aspect;
    }
    for _ in 0..8 {
        aspect = auto_frame_aspect(resting_aspect(screen_aspect, (aspect, 1.0), frame, preset), padding_pct);
    }
    aspect
}

/// `paddedContentSize` in px: a fixed format keeps `fit` of each axis; an Auto frame
/// (`even_border`) a border of one thickness on all four sides.
fn padded_content_size(canvas: (f64, f64), padding_pct: f64, even_border: bool) -> (f64, f64) {
    let fit = padding_fit(padding_pct);
    if !even_border {
        return ((canvas.0 * fit).round(), (canvas.1 * fit).round());
    }
    let border = (1.0 - fit) / 2.0 * canvas.0.min(canvas.1);
    ((canvas.0 - 2.0 * border).round(), (canvas.1 - 2.0 * border).round())
}

/// `snapRect`: whole pixels, never under 1.
fn snap_rect(x: f64, y: f64, w: f64, h: f64) -> (f64, f64, f64, f64) {
    let (left, top) = (x.round(), y.round());
    (left, top, ((x + w).round() - left).max(1.0), ((y + h).round() - top).max(1.0))
}

/// `centerRect`: contain-fit `size` into `max`, centred on the canvas; within 4 px of the whole
/// canvas it snaps to it.
fn center_rect(canvas: (f64, f64), size: (f64, f64), max: (f64, f64)) -> (f64, f64, f64, f64) {
    let scale = (max.0 / size.0).min(max.1 / size.1);
    let (w, h) = ((size.0 * scale).round(), (size.1 * scale).round());
    if max.0 >= canvas.0 && max.1 >= canvas.1 && (canvas.0 - w).abs() <= 4.0 && (canvas.1 - h).abs() <= 4.0 {
        return (0.0, 0.0, canvas.0, canvas.1);
    }
    (((canvas.0 - w) / 2.0).floor().max(0.0), ((canvas.1 - h) / 2.0).floor().max(0.0), w, h)
}

/// The screen's box in the output, in fractions of `out`: contain-fitted into the padded area and
/// centred; under a frame everything the frame draws is fitted and centred, so the screen is
/// smaller. `even_border` = the output's shape is Auto (`paddedContentSize`'s even border).
pub fn screen_rect(out: (u32, u32), screen_aspect: f64, padding_pct: f64, frame: Frame, even_border: bool) -> Rect {
    if out.0 == 0 || out.1 == 0 {
        return Rect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 };
    }
    let canvas = (out.0 as f64, out.1 as f64);
    let screen_aspect = sane_aspect(screen_aspect);
    let max = padded_content_size(canvas, padding_pct, even_border);
    let (x, y, w, h) = if frame == Frame::None {
        center_rect(canvas, (screen_aspect, 1.0), max)
    } else {
        let (screen, _, outer, element) = framed_screen(screen_aspect, canvas, frame);
        let scale = (max.0 / element.0).min(max.1 / element.1);
        let ox = (canvas.0 - element.0 * scale) / 2.0;
        let oy = (canvas.1 - element.1 * scale) / 2.0;
        snap_rect(ox + outer[0] * scale, oy + outer[1] * scale, scale, screen.1 * scale)
    };
    Rect { x: x / canvas.0, y: y / canvas.1, width: w / canvas.0, height: h / canvas.1 }
}

/// `computeCompositeLayout` (`compositeLayout.ts:447-646`) for one clip: where the screen and
/// the camera sit in an `out`-sized frame. `camera` = the camera's source size, `None` = no
/// camera on this clip; `look.preset` is used as given (resolve it per clip first).
/// `even_border` = the output's shape is Auto. Degenerate inputs diverge from upstream on
/// purpose: an empty `out` (0 px) returns the whole frame, an invalid screen aspect is read as
/// 16:9 (`sane_aspect`) — upstream has no guard there and yields empty or NaN rects, which the
/// scene cannot carry.
pub fn composite_layout(out: (u32, u32), screen_aspect: f64, camera: Option<(u32, u32)>, look: &CameraLook, padding_pct: f64, frame: Frame, even_border: bool) -> CompositeLayout {
    let screen_only = |screen: Rect| CompositeLayout { screen, camera: None, screen_radius_frac: None, camera_radius_frac: None, screen_cover: false, mask: look.mask };
    let alone = screen_rect(out, screen_aspect, padding_pct, frame, even_border);
    if out.0 == 0 || out.1 == 0 {
        return screen_only(alone);
    }
    let canvas = (out.0 as f64, out.1 as f64);
    let screen_aspect = sane_aspect(screen_aspect);
    let max = padded_content_size(canvas, padding_pct, even_border);
    // Px rects → output fractions, radii → share of their own box's short side.
    let frac = |(x, y, w, h): (f64, f64, f64, f64)| Rect { x: x / canvas.0, y: y / canvas.1, width: w / canvas.0, height: h / canvas.1 };
    let radius_frac = |(_, _, w, h): (f64, f64, f64, f64), r: f64| {
        let short = w.min(h);
        (short > 0.0).then(|| r / short)
    };
    let cam_size = camera.filter(|c| c.0 > 0 && c.1 > 0);

    match look.preset {
        LayoutPreset::NoWebcam => screen_only(alone),
        // `camera-only`: the camera takes the whole frame; the screen is centred under it
        // without its frame (upstream `centerRect`), drawn and then covered.
        LayoutPreset::CameraOnly if camera.is_some() => {
            let full = (0.0, 0.0, canvas.0, canvas.1);
            CompositeLayout {
                screen: frac(center_rect(canvas, (screen_aspect, 1.0), max)),
                camera: Some(frac(full)),
                screen_radius_frac: None,
                camera_radius_frac: Some(0.0),
                screen_cover: false,
                mask: CameraMask::Rectangle,
            }
        }
        LayoutPreset::DualFrame | LayoutPreset::VerticalStack => {
            // No camera on this clip: the block degenerates to the screen alone.
            if cam_size.is_none() {
                return screen_only(alone);
            }
            let row = look.preset == LayoutPreset::DualFrame;
            let (min_r, max_r, frac_r) = if row { (12.0, 24.0, 0.06) } else { (8.0, 24.0, 0.06) };
            // Unit space: the screen is exactly 1 wide; the camera's source aspect plays no part.
            let unit_h = 1.0 / screen_aspect;
            let gap = BLOCK_GAP_FRACTION;
            let (_, body, outer, element) = framed_screen(screen_aspect, canvas, frame);
            let content = (canvas.0.min(max.0.max(1.0)), canvas.1.min(max.1.max(1.0)));
            let cross = if row { body[1] + unit_h + body[3] } else { body[0] + 1.0 + body[2] };
            let content_aspect = content.0 / content.1;
            let along_for_fill = if row {
                content_aspect * element.1 - element.0 - gap
            } else {
                element.0 / content_aspect - element.1 - gap
            };
            let along = (cross * BLOCK_CAMERA_ASPECT_TOLERANCE).min((cross / BLOCK_CAMERA_ASPECT_TOLERANCE).max(along_for_fill));
            let (cam_w, cam_h) = if row { (along, cross) } else { (cross, along) };
            let (block_w, block_h) = block_unit_size(element, row, along);
            let scale = (content.0 / block_w).min(content.1 / block_h);
            let ox = (canvas.0 - block_w * scale) / 2.0;
            let oy = (canvas.1 - block_h * scale) / 2.0;
            let screen = snap_rect(ox + outer[0] * scale, oy + outer[1] * scale, scale, unit_h * scale);
            let cam = snap_rect(
                if row { ox + (element.0 + gap) * scale } else { ox + (outer[0] - body[0]) * scale },
                if row { oy + (outer[1] - body[1]) * scale } else { oy + (element.1 + gap) * scale },
                cam_w * scale,
                cam_h * scale,
            );
            let radius = ((cam.2.min(cam.3) * frac_r).round()).max(min_r).min(max_r);
            CompositeLayout {
                screen: frac(screen),
                camera: Some(frac(cam)),
                screen_radius_frac: radius_frac(screen, radius),
                camera_radius_frac: radius_frac(cam, radius),
                screen_cover: true,
                mask: CameraMask::Rectangle,
            }
        }
        // Picture-in-picture (and camera-only without a camera, which upstream lays out as an
        // overlay with nothing to overlay): the screen as it is alone, the camera floating over it.
        LayoutPreset::PictureInPicture | LayoutPreset::CameraOnly => {
            let Some((cw, ch)) = cam_size.filter(|_| look.preset == LayoutPreset::PictureInPicture) else {
                return screen_only(alone);
            };
            let (cw, ch) = (cw as f64, ch as f64);
            let margin = (canvas.0.min(canvas.1) * PIP_MARGIN_FRACTION).round().max(0.0);
            let size = if look.size_pct.is_finite() { look.size_pct } else { 25.0 };
            let max_side = canvas.0.min(canvas.1) * size.clamp(CAMERA_SIZE_MIN, CAMERA_SIZE_MAX) / 100.0;
            let scale = (max_side / cw).min(max_side / ch);
            let (mut w, mut h) = ((cw * scale).round(), (ch * scale).round());
            if look.mask == CameraMask::Square {
                let side = w.min(h);
                (w, h) = (side, side);
            }
            let (fx, fy) = look.anchor.fractions();
            let x = (margin + fx * (canvas.0 - 2.0 * margin - w)).round().max(0.0);
            let y = (margin + fy * (canvas.1 - 2.0 * margin - h)).round().max(0.0);
            let roundness = if look.roundness.is_finite() { look.roundness.clamp(0.0, 1.0) } else { 0.0 };
            let cam = (x, y, w, h);
            CompositeLayout {
                screen: alone,
                camera: Some(frac(cam)),
                screen_radius_frac: None,
                camera_radius_frac: radius_frac(cam, (roundness * w.min(h) / 2.0).round()),
                screen_cover: false,
                mask: look.mask,
            }
        }
    }
}

/// `maxZoomScaleFor` over `zoomScaleLimitOf`: the deepest zoom that keeps the clip at or under
/// `MAX_SOURCE_MAGNIFICATION` on a 1080-px reference frame, given its box and source (crop
/// included) size. Never below 1, never above 5.
pub fn max_zoom_scale(screen_rect: Rect, out: (u32, u32), src: (u32, u32)) -> f64 {
    let (ow, oh) = (out.0 as f64, out.1 as f64);
    let (sw, sh) = (src.0.max(1) as f64, src.1.max(1) as f64);
    let to_reference = MAGNIFICATION_REFERENCE_PX / ow.min(oh).max(1.0);
    let rest = to_reference * (screen_rect.width * ow / sw).max(screen_rect.height * oh / sh);
    if rest.is_nan() || rest <= 0.0 {
        return MAX_ZOOM_SCALE;
    }
    let scale = (MAX_SOURCE_MAGNIFICATION / rest * 100.0).floor() / 100.0;
    scale.clamp(MIN_ZOOM_SCALE, MAX_ZOOM_SCALE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn padding_fit_values() {
        assert!(close(padding_fit(0.0), 1.0));
        assert!(close(padding_fit(50.0), 0.8));
        assert!(close(padding_fit(100.0), 0.6));
        assert!(close(padding_fit(250.0), 0.6), "clamped");
        assert!(close(padding_fit(f64::NAN), 1.0));
    }

    #[test]
    fn screen_rect_centred_without_frame() {
        // 16:9 screen in a 16:9 frame, no padding: the whole frame.
        assert_eq!(screen_rect((1920, 1080), 16.0 / 9.0, 0.0, Frame::None, false), Rect { x: 0.0, y: 0.0, width: 1.0, height: 1.0 });
        // 50 % padding on a fixed format: 0.8 of each axis, centred.
        let r = screen_rect((1920, 1080), 16.0 / 9.0, 50.0, Frame::None, false);
        assert_eq!((r.width * 1920.0, r.height * 1080.0), (1536.0, 864.0));
        assert_eq!((r.x * 1920.0, r.y * 1080.0), (192.0, 108.0));
        // A narrower screen is letterboxed horizontally and centred.
        let r = screen_rect((1920, 1080), 4.0 / 3.0, 0.0, Frame::None, false);
        assert_eq!((r.x * 1920.0, r.width * 1920.0, r.height), (240.0, 1440.0, 1.0));
        // Auto's even border: the same border on all four sides.
        let a = auto_aspect(16.0 / 9.0, 50.0, Frame::None, LayoutPreset::NoWebcam);
        let out = ((1080.0 * a / 2.0).round() as u32 * 2, 1080);
        let r = screen_rect(out, 16.0 / 9.0, 50.0, Frame::None, true);
        let (left, top) = (r.x * out.0 as f64, r.y * out.1 as f64);
        assert!((left - top).abs() <= 1.0, "{left} vs {top}");
        assert!((top - 108.0).abs() <= 1.0, "{top}");
    }

    #[test]
    fn frame_reserves_room() {
        let plain = screen_rect((1920, 1080), 16.0 / 9.0, 50.0, Frame::None, false);
        let framed = screen_rect((1920, 1080), 16.0 / 9.0, 50.0, Frame::Window, false);
        assert!(framed.width < plain.width && framed.height < plain.height, "{framed:?} vs {plain:?}");
        // The title bar sits on top: the screen starts lower than the centred box would.
        let max_h = 864.0;
        let bar = framed.y * 1080.0 - (1080.0 - max_h) / 2.0;
        let line = (framed.y + framed.height) * 1080.0;
        let bottom = (1080.0 + max_h) / 2.0 - line;
        assert!(bar > bottom * 10.0, "bar {bar} bottom {bottom}");
        // The bar is WINDOW_FRAME_BAR_FRAC frame units: unit = screen width share × short side.
        let unit = framed.width.max(framed.height) * 1080.0;
        assert!((bar - WINDOW_FRAME_BAR_FRAC * unit).abs() <= 1.5, "bar {bar} expected {}", WINDOW_FRAME_BAR_FRAC * unit);
        // Devices reserve room too, a laptop (deck) most of all below the screen.
        for f in [Frame::Laptop, Frame::Phone, Frame::Monitor] {
            let r = screen_rect((1920, 1080), 16.0 / 9.0, 50.0, f, false);
            assert!(r.width < plain.width, "{f:?} {r:?}");
        }
        let laptop = screen_rect((1920, 1080), 16.0 / 9.0, 0.0, Frame::Laptop, false);
        assert!(laptop.y + laptop.height < 0.9, "{laptop:?}");
    }

    #[test]
    fn auto_aspect_without_padding_is_screen_aspect() {
        for a in [16.0 / 9.0, 1.54, 9.0 / 16.0, 1.0] {
            assert!(close(auto_aspect(a, 0.0, Frame::None, LayoutPreset::NoWebcam), a), "{a}");
        }
    }

    #[test]
    fn auto_aspect_with_padding_flattens_elongation() {
        let e = 16.0 / 9.0;
        let fit = padding_fit(50.0);
        assert!(close(auto_aspect(e, 50.0, Frame::None, LayoutPreset::NoWebcam), fit * e + (1.0 - fit)));
        // Portrait keeps its orientation.
        assert!(close(auto_aspect(1.0 / e, 50.0, Frame::None, LayoutPreset::NoWebcam), 1.0 / (fit * e + (1.0 - fit))));
        // A window frame's bar makes the composition taller → a little less wide.
        let w = auto_aspect(e, 50.0, Frame::Window, LayoutPreset::NoWebcam);
        assert!(w < auto_aspect(e, 50.0, Frame::None, LayoutPreset::NoWebcam) && w > 1.0, "{w}");
        // Converged: one more pass changes nothing visible.
        let (_, _, _, el) = framed_screen(e, (w, 1.0), Frame::Window);
        assert!((auto_frame_aspect(el.0 / el.1, 50.0) - w).abs() < 1e-6);
    }

    #[test]
    fn max_zoom_scale_limits_small_sources() {
        // A 1080p take filling a 1080p frame at 50 % padding: rest 0.8 → 2.5×.
        let r = Rect { x: 0.1, y: 0.1, width: 0.8, height: 0.8 };
        assert!(close(max_zoom_scale(r, (1920, 1080), (1920, 1080)), 2.5));
        // A 2160p take: rest 0.4 → 5×.
        assert!(close(max_zoom_scale(r, (1920, 1080), (3840, 2160)), 5.0));
        // A tiny source blown up past 2× at rest cannot zoom at all.
        assert!(close(max_zoom_scale(r, (1920, 1080), (640, 360)), 1.0));
        // Measured on the 1080 reference, whatever the output size.
        assert!(close(max_zoom_scale(r, (1280, 720), (1920, 1080)), 2.5));
    }

    // ─── stage 5: composite camera layouts ────────────────────────────────────────

    const FRAMES: [Frame; 4] = [Frame::Window, Frame::Laptop, Frame::Phone, Frame::Monitor];
    const CAPTURES: [(u32, u32); 6] = [(2560, 1080), (1920, 1080), (1440, 1080), (1080, 1080), (1080, 1350), (1080, 1920)];
    const SCENES: [(u32, u32); 5] = [(1920, 1080), (1080, 1920), (1080, 1080), (1080, 1350), (2560, 1080)];
    const CAM: (u32, u32) = (960, 720);

    /// A rect in output px: `[x, y, w, h]`.
    fn px(r: Rect, out: (u32, u32)) -> [f64; 4] {
        let (w, h) = (out.0 as f64, out.1 as f64);
        [r.x * w, r.y * h, r.width * w, r.height * h]
    }

    fn look(preset: LayoutPreset) -> CameraLook {
        CameraLook { preset, size_pct: 40.0, mask: CameraMask::Rectangle, roundness: 0.4, anchor: CameraAnchor::BottomRight }
    }

    fn lay(out: (u32, u32), screen: (u32, u32), camera: Option<(u32, u32)>, l: &CameraLook, padding: f64, frame: Frame, even: bool) -> (CompositeLayout, [f64; 4], Option<[f64; 4]>) {
        let c = composite_layout(out, screen.0 as f64 / screen.1 as f64, camera, l, padding, frame, even);
        (c, px(c.screen, out), c.camera.map(|r| px(r, out)))
    }

    /// The screen grown by insets (upstream test helper `expand`).
    fn expand(r: [f64; 4], i: Insets) -> [f64; 4] {
        [r[0] - i[0], r[1] - i[1], r[2] + i[0] + i[2], r[3] + i[1] + i[3]]
    }

    /// `frameFootprint(frame, screen, frameUnit(screen, scene))` on a px screen rect.
    fn footprint(frame: Frame, screen: [f64; 4], scene: (u32, u32)) -> (Insets, Insets) {
        let size = (screen[2], screen[3]);
        frame_footprint(frame, size, frame_unit(size, (scene.0 as f64, scene.1 as f64)))
    }

    fn parse_preset(v: &serde_json::Value) -> LayoutPreset {
        serde_json::from_value(v.clone()).expect("preset")
    }

    fn parse_frame(s: &str) -> Frame {
        match s {
            "none" => Frame::None,
            "window" => Frame::Window,
            "laptop" => Frame::Laptop,
            "phone" => Frame::Phone,
            "monitor" => Frame::Monitor,
            other => panic!("frame {other}"),
        }
    }

    fn pair(v: &serde_json::Value) -> (u32, u32) {
        (v[0].as_u64().unwrap() as u32, v[1].as_u64().unwrap() as u32)
    }

    fn rect4(v: &serde_json::Value) -> [f64; 4] {
        std::array::from_fn(|i| v[i].as_f64().unwrap())
    }

    fn same_px(a: [f64; 4], b: [f64; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    #[test]
    fn golden_cases_match_upstream() {
        let doc: serde_json::Value = serde_json::from_str(include_str!("fixtures/composite-layout-golden.json")).unwrap();
        let cases = doc["cases"].as_array().unwrap();
        assert!(cases.len() >= 15, "fixture lost cases");
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let l = &case["look"];
            let look = CameraLook {
                preset: parse_preset(&l["preset"]),
                size_pct: l["size_pct"].as_f64().unwrap(),
                mask: serde_json::from_value(l["mask"].clone()).unwrap(),
                roundness: l["roundness"].as_f64().unwrap(),
                anchor: serde_json::from_value(l["anchor"].clone()).unwrap(),
            };
            let out = pair(&case["out"]);
            let camera = if case["camera"].is_null() { None } else { Some(pair(&case["camera"])) };
            let (c, screen, cam) = lay(
                out,
                pair(&case["screen"]),
                camera,
                &look,
                case["padding_pct"].as_f64().unwrap(),
                parse_frame(case["frame"].as_str().unwrap()),
                case["even_border"].as_bool().unwrap(),
            );
            let e = case["expect"].as_object().unwrap();
            for (key, want) in e {
                match key.as_str() {
                    "screen" => assert!(same_px(screen, rect4(want)), "{name}: screen {screen:?} want {want}"),
                    "camera" if want.is_null() => assert!(cam.is_none(), "{name}: camera {cam:?} want none"),
                    "camera" => assert!(same_px(cam.expect(name), rect4(want)), "{name}: camera {cam:?} want {want}"),
                    "camera_height" => assert!((cam.expect(name)[3] - want.as_f64().unwrap()).abs() < 1e-6, "{name}: {cam:?}"),
                    "camera_x" => assert!((cam.expect(name)[0] - want.as_f64().unwrap()).abs() < 1e-6, "{name}: {cam:?}"),
                    "camera_y" => assert!((cam.expect(name)[1] - want.as_f64().unwrap()).abs() < 1e-6, "{name}: {cam:?}"),
                    "camera_square" => {
                        let r = cam.expect(name);
                        assert_eq!(r[2] == r[3], want.as_bool().unwrap(), "{name}: {r:?}");
                    }
                    "camera_radius_frac" => assert!((c.camera_radius_frac.expect(name) - want.as_f64().unwrap()).abs() < 1e-6, "{name}: {:?}", c.camera_radius_frac),
                    "screen_radius_frac" if want.is_null() => assert_eq!(c.screen_radius_frac, None, "{name}"),
                    "screen_radius_frac" => assert!((c.screen_radius_frac.expect(name) - want.as_f64().unwrap()).abs() < 1e-6, "{name}"),
                    "camera_radius_px" => {
                        let r = cam.expect(name);
                        let got = c.camera_radius_frac.expect(name) * r[2].min(r[3]);
                        assert!((got - want.as_f64().unwrap()).abs() < 1e-6, "{name}: camera radius {got}");
                    }
                    "screen_radius_px" => {
                        let got = c.screen_radius_frac.expect(name) * screen[2].min(screen[3]);
                        assert!((got - want.as_f64().unwrap()).abs() < 1e-6, "{name}: screen radius {got}");
                    }
                    "screen_cover" => assert_eq!(c.screen_cover, want.as_bool().unwrap(), "{name}"),
                    "mask" => assert_eq!(c.mask, serde_json::from_value::<CameraMask>(want.clone()).unwrap(), "{name}"),
                    other => panic!("{name}: unknown expectation {other}"),
                }
            }
        }
    }

    #[test]
    fn pip_anchor_corners() {
        // Upstream "keeps the same margin from the border at every anchor", all eight anchors.
        let out = (1920, 1080);
        let margin = (1080.0_f64 * PIP_MARGIN_FRACTION).round();
        for size in [CAMERA_SIZE_MIN, 40.0, CAMERA_SIZE_MAX] {
            for anchor in CameraAnchor::ALL {
                let l = CameraLook { size_pct: size, anchor, ..look(LayoutPreset::PictureInPicture) };
                let (_, _, cam) = lay(out, (1920, 1080), Some((1280, 720)), &l, 0.0, Frame::None, false);
                let [x, y, w, h] = cam.unwrap();
                let (fx, fy) = anchor.fractions();
                let want_x = if fx == 0.0 { margin } else if fx == 1.0 { 1920.0 - margin - w } else { x };
                let want_y = if fy == 0.0 { margin } else if fy == 1.0 { 1080.0 - margin - h } else { y };
                assert_eq!((x, y), (want_x, want_y), "{anchor:?} at {size}");
                if fx == 0.5 {
                    assert!((x + w / 2.0 - 960.0).abs() <= 1.0, "{anchor:?} centred x");
                }
                if fy == 0.5 {
                    assert!((y + h / 2.0 - 540.0).abs() <= 1.0, "{anchor:?} centred y");
                }
            }
        }
    }

    #[test]
    fn square_mask_uses_short_side() {
        let out = (1920, 1080);
        for cam in [(1280, 720), (720, 1280), (960, 720)] {
            let l = CameraLook { mask: CameraMask::Square, ..look(LayoutPreset::PictureInPicture) };
            let (c, _, r) = lay(out, (1920, 1080), Some(cam), &l, 0.0, Frame::None, false);
            let [_, _, w, h] = r.unwrap();
            assert_eq!(w, h, "{cam:?}");
            assert_eq!(c.mask, CameraMask::Square);
            let rect = lay(out, (1920, 1080), Some(cam), &look(LayoutPreset::PictureInPicture), 0.0, Frame::None, false).2.unwrap();
            assert_eq!(w, rect[2].min(rect[3]), "the square is the rectangle's short side");
        }
    }

    #[test]
    fn per_clip_layout_without_camera() {
        for p in [LayoutPreset::PictureInPicture, LayoutPreset::DualFrame, LayoutPreset::VerticalStack, LayoutPreset::CameraOnly, LayoutPreset::NoWebcam] {
            assert_eq!(p.resolve(false), LayoutPreset::NoWebcam);
            assert_eq!(p.resolve(true), p);
            let resolved = CameraLook { preset: p.resolve(false), ..CameraLook::default() };
            for frame in [Frame::None, Frame::Window, Frame::Laptop] {
                let c = composite_layout((1920, 1080), 1.6, None, &resolved, 30.0, frame, false);
                assert_eq!(c.screen, screen_rect((1920, 1080), 1.6, 30.0, frame, false), "{p:?} {frame:?}");
                assert_eq!(c.camera, None);
                assert!(!c.screen_cover);
            }
        }
        assert!(LayoutPreset::DualFrame.is_block() && LayoutPreset::VerticalStack.is_block());
        assert!(!LayoutPreset::PictureInPicture.is_block() && !LayoutPreset::CameraOnly.is_block());
        assert!(LayoutPreset::PictureInPicture.supports_reactive_zoom());
        for p in [LayoutPreset::DualFrame, LayoutPreset::VerticalStack, LayoutPreset::CameraOnly, LayoutPreset::NoWebcam] {
            assert!(!p.supports_reactive_zoom(), "{p:?}");
        }
        // Serialised under upstream's names.
        assert_eq!(serde_json::to_string(&LayoutPreset::PictureInPicture).unwrap(), "\"picture-in-picture\"");
        assert_eq!(serde_json::to_string(&LayoutPreset::NoWebcam).unwrap(), "\"no-webcam\"");
        assert_eq!(serde_json::to_string(&CameraAnchor::BottomRight).unwrap(), "\"bottom-right\"");
        assert_eq!(CameraAnchor::default(), CameraAnchor::BottomRight);
    }

    #[test]
    fn auto_aspect_includes_block_camera() {
        // Upstream "frames the block layouts with their camera at rest, square" (0 % padding =
        // the resting shape itself).
        let h = 1080.0 / 1920.0;
        let e = 16.0 / 9.0;
        assert!(close(auto_aspect(e, 0.0, Frame::None, LayoutPreset::PictureInPicture), e));
        assert!(close(auto_aspect(e, 0.0, Frame::None, LayoutPreset::NoWebcam), e));
        assert!(close(auto_aspect(e, 0.0, Frame::None, LayoutPreset::DualFrame), (1.0 + 0.02 + h) / h));
        assert!(close(auto_aspect(e, 0.0, Frame::None, LayoutPreset::VerticalStack), 1.0 / (h + 0.02 + 1.0)));
        assert!(close(auto_aspect(e, 0.0, Frame::None, LayoutPreset::CameraOnly), e));
        // Padding flattens the block's shape like any other.
        let fit = padding_fit(50.0);
        let dual = (1.0 + 0.02 + h) / h;
        assert!(close(auto_aspect(e, 50.0, Frame::None, LayoutPreset::DualFrame), fit * dual + 1.0 - fit));
        // A frame changes a block's shape too, and converges.
        let framed = auto_aspect(e, 50.0, Frame::Laptop, LayoutPreset::DualFrame);
        assert!(framed.is_finite() && (framed - auto_aspect(e, 50.0, Frame::None, LayoutPreset::DualFrame)).abs() > 1e-3, "{framed}");
    }

    #[test]
    fn zoom_limit_uses_block_screen_rect() {
        // The block's screen is smaller than the screen alone, so it may zoom deeper before the
        // recording is blown up past 2× (sceneDescription.ts:1443-1449 reads the clip's own box).
        let out = (1920, 1080);
        let src = (1920, 1080);
        let alone = lay(out, src, Some(CAM), &look(LayoutPreset::NoWebcam), 0.0, Frame::None, false).0.screen;
        let block = lay(out, src, Some(CAM), &look(LayoutPreset::VerticalStack), 0.0, Frame::None, false).0.screen;
        assert!(block.width < alone.width);
        let (za, zb) = (max_zoom_scale(alone, out, src), max_zoom_scale(block, out, src));
        assert!(close(za, 2.0), "{za}");
        // vertical-stack screen is 782 px wide (golden case): rest = 782/1920 → floor(2/rest·100)/100.
        assert!(close(zb, (2.0 / (782.0 / 1920.0) * 100.0_f64).floor() / 100.0), "{zb}");
        assert!(zb > za);
    }

    #[test]
    fn no_webcam_unchanged() {
        for &out in &SCENES {
            for &cap in &CAPTURES {
                for frame in [Frame::None, Frame::Window, Frame::Laptop, Frame::Phone, Frame::Monitor] {
                    for (pad, even) in [(0.0, false), (50.0, false), (30.0, true)] {
                        let a = cap.0 as f64 / cap.1 as f64;
                        let c = composite_layout(out, a, Some(CAM), &CameraLook::none(), pad, frame, even);
                        assert_eq!(c.screen, screen_rect(out, a, pad, frame, even));
                        assert_eq!((c.camera, c.screen_radius_frac, c.camera_radius_frac, c.screen_cover), (None, None, None, false));
                    }
                }
            }
        }
    }

    #[test]
    fn block_matrix_screen_ratio_kept_block_contained_camera_near_square() {
        // Upstream: `${preset}: capture … in scene … — screen ratio kept, block contained, camera near square`.
        for preset in [LayoutPreset::DualFrame, LayoutPreset::VerticalStack] {
            for &cap in &CAPTURES {
                for &scene in &SCENES {
                    let (c, s, cam) = lay(scene, cap, Some(CAM), &look(preset), 0.0, Frame::None, false);
                    let cam = cam.unwrap();
                    let tag = format!("{preset:?} {cap:?} in {scene:?}");
                    assert!((s[2] / s[3] - cap.0 as f64 / cap.1 as f64).abs() < 0.05, "{tag} screen ratio");
                    for r in [s, cam] {
                        assert!(r[0] >= -1.0 && r[1] >= -1.0, "{tag} {r:?}");
                        assert!(r[0] + r[2] <= scene.0 as f64 + 1.0 && r[1] + r[3] <= scene.1 as f64 + 1.0, "{tag} {r:?}");
                    }
                    let ca = cam[2] / cam[3];
                    assert!((1.0 / 1.25 - 0.02..=1.25 + 0.02).contains(&ca), "{tag} camera aspect {ca}");
                    if preset == LayoutPreset::DualFrame {
                        assert!((cam[3] - s[3]).abs() < 0.5, "{tag}");
                        assert!(cam[0] >= s[0] + s[2], "{tag}");
                    } else {
                        assert!((cam[2] - s[2]).abs() < 0.5, "{tag}");
                        assert!(cam[1] >= s[1] + s[3], "{tag}");
                    }
                    assert!(c.screen_cover && c.mask == CameraMask::Rectangle);
                    let (sr, cr) = (c.screen_radius_frac.unwrap() * s[2].min(s[3]), c.camera_radius_frac.unwrap() * cam[2].min(cam[3]));
                    assert!((sr - cr).abs() < 1e-6 && sr > 0.0, "{tag}: one corner radius for both halves");
                }
            }
        }
    }

    #[test]
    fn block_fills_a_reachable_scene_exactly() {
        // Upstream "fills the scene exactly (block aspect == scene aspect) when a near-square camera allows it".
        let scene = (1000, 1587);
        let (_, s, cam) = lay(scene, (1920, 1080), Some(CAM), &look(LayoutPreset::VerticalStack), 0.0, Frame::None, false);
        let cam = cam.unwrap();
        let bw = (s[0] + s[2]).max(cam[0] + cam[2]) - s[0].min(cam[0]);
        let bh = (s[1] + s[3]).max(cam[1] + cam[3]) - s[1].min(cam[1]);
        assert!((bw / bh - 1000.0 / 1587.0).abs() < 0.05);
        assert!((cam[2] / cam[3] - 1.0).abs() < 0.05);
        // "makes the camera squarer for a landscape capture top/bottom than the old fixed 50/50 did"
        let (_, s, cam) = lay((1920, 1080), (1920, 1080), Some(CAM), &look(LayoutPreset::VerticalStack), 0.0, Frame::None, false);
        let cam = cam.unwrap();
        assert!((cam[2] - s[2]).abs() < 0.5 && cam[2] / cam[3] < 1.3);
    }

    #[test]
    fn block_padding_shrinks_the_whole_block_centred() {
        // Upstream "insets the whole %s block by the padding, keeping it centered" and
        // "contain-fits the %s block into a portrait scene without overflowing".
        for preset in [LayoutPreset::DualFrame, LayoutPreset::VerticalStack] {
            let (_, fs, fc) = lay((1920, 1080), (1920, 1080), Some(CAM), &look(preset), 0.0, Frame::None, false);
            let (_, ps, pc) = lay((1920, 1080), (1920, 1080), Some(CAM), &look(preset), 50.0, Frame::None, false);
            let (fc, pc) = (fc.unwrap(), pc.unwrap());
            assert!((ps[2] / fs[2] - 0.8).abs() < 0.005, "{preset:?}");
            assert!((pc[2] / fc[2] - 0.8).abs() < 0.005 && (pc[3] / fc[3] - 0.8).abs() < 0.005, "{preset:?}");
            let (l, r) = (ps[0].min(pc[0]), (ps[0] + ps[2]).max(pc[0] + pc[2]));
            let (t, b) = (ps[1].min(pc[1]), (ps[1] + ps[3]).max(pc[1] + pc[3]));
            assert!((l - (1920.0 - r)).abs() < 0.5 && (t - (1080.0 - b)).abs() < 0.5, "{preset:?} centred");
            let (_, s, cam) = lay((1080, 1920), (1920, 1080), Some(CAM), &look(preset), 0.0, Frame::None, false);
            for rr in [s, cam.unwrap()] {
                assert!(rr[0] >= 0.0 && rr[1] >= 0.0 && rr[0] + rr[2] <= 1080.0 && rr[1] + rr[3] <= 1920.0, "{preset:?} {rr:?}");
            }
            assert!((s[2] / s[3] - 16.0 / 9.0).abs() < 0.005);
        }
    }

    #[test]
    fn block_under_a_frame_lines_up_with_the_body() {
        // Upstream "${layoutPreset} under a ${frame} in scene …: the camera lines up with the frame and clears it".
        let near = |a: f64, b: f64, tag: &str| assert!((a - b).abs() <= 2.0, "{tag}: {a} vs {b}");
        for preset in [LayoutPreset::DualFrame, LayoutPreset::VerticalStack] {
            for frame in FRAMES {
                for &scene in &SCENES {
                    let tag = format!("{preset:?} {frame:?} {scene:?}");
                    let padded = padded_content_size((scene.0 as f64, scene.1 as f64), 50.0, false);
                    let (c, s, cam) = lay(scene, (1920, 1080), Some(CAM), &look(preset), 50.0, frame, false);
                    let cam = cam.unwrap();
                    let (body, outer) = footprint(frame, s, scene);
                    let (shell, drawn) = (expand(s, body), expand(s, outer));
                    assert!((s[2] / s[3] - 16.0 / 9.0).abs() < 0.05, "{tag}");
                    assert!(c.screen_cover);
                    let gap = 0.02 * s[2];
                    if preset == LayoutPreset::DualFrame {
                        near(cam[1], shell[1], &tag);
                        near(cam[3], shell[3], &tag);
                        near(cam[0] - (drawn[0] + drawn[2]), gap, &tag);
                    } else {
                        near(cam[0], shell[0], &tag);
                        near(cam[2], shell[2], &tag);
                        near(cam[1] - (drawn[1] + drawn[3]), gap, &tag);
                    }
                    let (l, t) = (drawn[0].min(cam[0]), drawn[1].min(cam[1]));
                    let (r, b) = ((drawn[0] + drawn[2]).max(cam[0] + cam[2]), (drawn[1] + drawn[3]).max(cam[1] + cam[3]));
                    assert!(r - l <= padded.0 + 2.0 && b - t <= padded.1 + 2.0, "{tag} contained");
                    assert!((padded.0 - (r - l)).min(padded.1 - (b - t)) <= 2.0, "{tag} touches one axis");
                    near(l, scene.0 as f64 - r, &tag);
                    near(t, scene.1 as f64 - b, &tag);
                }
            }
        }
        // Upstream "lays a block out exactly as before without a frame" compares a layout with
        // `frame` absent to one with `frame: "none"`. Our API has no "absent" — `Frame::None` IS
        // the frameless path — so the case does not apply here (comparing a call with itself
        // would prove nothing).
    }

    #[test]
    fn pip_and_no_webcam_frame_fits_and_centres() {
        // Upstream "${layoutPreset} under a ${frame} in scene …: the whole frame fits and centres".
        for preset in [LayoutPreset::PictureInPicture, LayoutPreset::NoWebcam] {
            for frame in FRAMES {
                for &scene in &SCENES {
                    let tag = format!("{preset:?} {frame:?} {scene:?}");
                    let (sw, sh) = (scene.0 as f64, scene.1 as f64);
                    let (_, s, _) = lay(scene, (1920, 1080), Some(CAM), &look(preset), 0.0, frame, false);
                    let (_, outer) = footprint(frame, s, scene);
                    let d = expand(s, outer);
                    assert!((s[2] / s[3] - 16.0 / 9.0).abs() < 0.05, "{tag}");
                    assert!(d[2] <= sw + 2.0 && d[3] <= sh + 2.0, "{tag}");
                    assert!((sw - d[2]).min(sh - d[3]) <= 2.0, "{tag}");
                    assert!((d[0] - (sw - d[0] - d[2])).abs() <= 2.0 && (d[1] - (sh - d[1] - d[3])).abs() <= 2.0, "{tag}");
                }
            }
        }
    }

    #[test]
    fn auto_frame_gives_an_even_border_around_every_layout() {
        // Upstream "Auto frame > ${label}, padding ${padding}%: even border around every frame, nothing bent".
        let cases: [((u32, u32), LayoutPreset); 7] = [
            ((1920, 1080), LayoutPreset::NoWebcam),
            ((1920, 1080), LayoutPreset::PictureInPicture),
            ((1080, 1920), LayoutPreset::NoWebcam),
            ((1366, 911), LayoutPreset::NoWebcam),
            ((1920, 1080), LayoutPreset::DualFrame),
            ((1920, 1080), LayoutPreset::VerticalStack),
            ((1080, 1920), LayoutPreset::DualFrame),
        ];
        for (screen, preset) in cases {
            for padding in [0.0, 30.0, 50.0, 100.0] {
                for frame in [Frame::None, Frame::Window, Frame::Laptop, Frame::Phone, Frame::Monitor] {
                    let tag = format!("{screen:?} {preset:?} {padding} {frame:?}");
                    let a = screen.0 as f64 / screen.1 as f64;
                    let aspect = auto_aspect(a, padding, frame, preset);
                    let canvas = if aspect >= 1.0 { (1920, (1920.0 / aspect).round() as u32) } else { ((1920.0 * aspect).round() as u32, 1920) };
                    let camera = (preset != LayoutPreset::NoWebcam).then_some((1280, 720));
                    let (_, s, cam) = lay(canvas, screen, camera, &look(preset), padding, frame, true);
                    let drawn = expand(s, footprint(frame, s, canvas).1);
                    let mut rects = vec![drawn];
                    if preset.is_block() {
                        rects.push(cam.unwrap());
                    }
                    let (cw, ch) = (canvas.0 as f64, canvas.1 as f64);
                    let border = (1.0 - padding_fit(padding)) / 2.0 * cw.min(ch);
                    let sides = [
                        rects.iter().map(|r| r[0]).fold(f64::INFINITY, f64::min),
                        rects.iter().map(|r| r[1]).fold(f64::INFINITY, f64::min),
                        cw - rects.iter().map(|r| r[0] + r[2]).fold(f64::NEG_INFINITY, f64::max),
                        ch - rects.iter().map(|r| r[1] + r[3]).fold(f64::NEG_INFINITY, f64::max),
                    ];
                    for side in sides {
                        assert!((side - border).abs() <= 2.0, "{tag}: {sides:?} border {border}");
                    }
                    assert!((s[2] / s[3] - a).abs() < 0.05, "{tag}");
                    if preset.is_block() {
                        let cam = cam.unwrap();
                        assert!((cam[2] / cam[3] - 1.0).abs() < 0.05, "{tag}: camera {cam:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn pip_size_follows_the_short_side() {
        let pip = |out: (u32, u32), cam: (u32, u32), size: f64| {
            let l = CameraLook { size_pct: size, ..look(LayoutPreset::PictureInPicture) };
            lay(out, out, Some(cam), &l, 0.0, Frame::None, false).2.unwrap()
        };
        // Upstream "produces consistent webcam size across landscape and portrait aspect ratios".
        let (a, b) = (pip((1920, 1080), (1280, 720), 50.0), pip((1080, 1920), (1280, 720), 50.0));
        assert_eq!(a[2] * a[3], b[2] * b[3]);
        // "keeps the webcam a modest fraction of a narrow (9:16) frame's own width".
        assert!(pip((230, 408), (960, 720), 34.0)[2] / 230.0 < 0.4);
        // "scales the webcam proportionally as webcamSizePreset increases" (10 clamps to 15).
        let (s, m, l) = (pip((1920, 1080), (1280, 720), 10.0), pip((1920, 1080), (1280, 720), 25.0), pip((1920, 1080), (1280, 720), 50.0));
        assert!(s[2] < m[2] && m[2] < l[2] && s[3] < m[3] && m[3] < l[3]);
        // "clamps webcamSizePreset to the slider's range".
        assert_eq!(pip((1920, 1080), (1280, 720), 1.0), pip((1920, 1080), (1280, 720), CAMERA_SIZE_MIN));
        assert_eq!(pip((1920, 1080), (1280, 720), 100.0), pip((1920, 1080), (1280, 720), CAMERA_SIZE_MAX));
        // "anchors the overlay in the lower-right corner" (default look).
        let r = pip((1920, 1080), (1280, 720), 40.0);
        assert!(r[0] + r[2] <= 1920.0 && r[1] + r[3] <= 1080.0 && r[0] > 960.0 && r[1] > 540.0);
        // A NaN size reads as upstream's 25.
        assert_eq!(pip((1920, 1080), (1280, 720), f64::NAN), pip((1920, 1080), (1280, 720), 25.0));
    }

    #[test]
    fn pip_radius_is_a_share_of_half_the_short_side() {
        // Upstream "rounds by the roundness …": the same roundness is the same shape at any size.
        let frac = |size: f64, roundness: f64| {
            let l = CameraLook { size_pct: size, roundness, ..look(LayoutPreset::PictureInPicture) };
            composite_layout((1920, 1080), 16.0 / 9.0, Some((1280, 720)), &l, 0.0, Frame::None, false).camera_radius_frac.unwrap()
        };
        assert!((frac(15.0, 0.5) - frac(35.0, 0.5)).abs() < 0.01);
        assert!((frac(40.0, 1.0) - 0.5).abs() < 0.01);
        assert_eq!(frac(40.0, -3.0), 0.0, "clamped");
        assert_eq!(frac(40.0, f64::NAN), 0.0, "NaN reads as 0 (upstream clamp01)");
        assert!((frac(40.0, 7.0) - frac(40.0, 1.0)).abs() < 1e-12, "clamped at 1");
    }
}
