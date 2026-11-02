//! Where the screen sits in the output frame: padding, device-frame room, the Auto aspect and the
//! deepest zoom a clip may take.
//!
//! A port of the **no-webcam** branch of OpenScreen v2.1.2's `src/lib/compositeLayout.ts`
//! (`paddingFit`, `paddedContentSize`, `autoFrameAspect`, `autoFormatAspect`, `centerScreen`,
//! `centerRect`, `snapRect`) and of `src/lib/frameFootprint.ts`, whose constants mirror the
//! compositor's `frame_geometry.rs` (`WINDOW_FRAME_BAR_FRAC` … `DEV_FOOT_Z`). The compositor
//! draws the frames; this only measures them so the padding is counted from what the frame
//! draws (`layout.screenRect` already includes that room). Pure.

use super::editor::Frame;

/// A rect in fractions (0..1) of the output frame, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

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
/// `(screen size, outer insets, element size)`.
fn framed_screen(screen_aspect: f64, canvas: (f64, f64), frame: Frame) -> ((f64, f64), Insets, (f64, f64)) {
    let screen = (1.0, 1.0 / screen_aspect);
    let (_, outer) = frame_footprint(frame, screen, frame_unit(screen, canvas));
    let element = (outer[0] + screen.0 + outer[2], outer[1] + screen.1 + outer[3]);
    (screen, outer, element)
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

/// `autoFormatAspect` (no webcam): the composition's resting shape plus an even padding border;
/// under a device frame 8 fixed-point passes (the frame's thickness follows the scene's shape).
pub fn auto_aspect(screen_aspect: f64, padding_pct: f64, frame: Frame) -> f64 {
    let screen_aspect = sane_aspect(screen_aspect);
    let mut aspect = auto_frame_aspect(screen_aspect, padding_pct);
    if frame == Frame::None {
        return aspect;
    }
    for _ in 0..8 {
        let (_, _, element) = framed_screen(screen_aspect, (aspect, 1.0), frame);
        aspect = auto_frame_aspect(element.0 / element.1, padding_pct);
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
        let (screen, outer, element) = framed_screen(screen_aspect, canvas, frame);
        let scale = (max.0 / element.0).min(max.1 / element.1);
        let ox = (canvas.0 - element.0 * scale) / 2.0;
        let oy = (canvas.1 - element.1 * scale) / 2.0;
        snap_rect(ox + outer[0] * scale, oy + outer[1] * scale, scale, screen.1 * scale)
    };
    Rect { x: x / canvas.0, y: y / canvas.1, width: w / canvas.0, height: h / canvas.1 }
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
        let a = auto_aspect(16.0 / 9.0, 50.0, Frame::None);
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
            assert!(close(auto_aspect(a, 0.0, Frame::None), a), "{a}");
        }
    }

    #[test]
    fn auto_aspect_with_padding_flattens_elongation() {
        let e = 16.0 / 9.0;
        let fit = padding_fit(50.0);
        assert!(close(auto_aspect(e, 50.0, Frame::None), fit * e + (1.0 - fit)));
        // Portrait keeps its orientation.
        assert!(close(auto_aspect(1.0 / e, 50.0, Frame::None), 1.0 / (fit * e + (1.0 - fit))));
        // A window frame's bar makes the composition taller → a little less wide.
        let w = auto_aspect(e, 50.0, Frame::Window);
        assert!(w < auto_aspect(e, 50.0, Frame::None) && w > 1.0, "{w}");
        // Converged: one more pass changes nothing visible.
        let (_, _, el) = framed_screen(e, (w, 1.0), Frame::Window);
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
}
