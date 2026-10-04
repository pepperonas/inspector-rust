//! `8bit` / `16bit` — live retro overlay. This module is the **reference
//! implementation** of the image math: palettes, per-channel quantisation,
//! Bayer dithering, cell formation with focus / lens regions. It is pure and
//! unit-tested, renders the panel preview, and is the specification the
//! Metal shader (phase 3) must match.
//!
//! Units: everything here works in **physical pixels**. Settings are stored in
//! logical points and converted per monitor with [`pt_to_px`].
//!
//! Design rule from the brief: readability depends on the pixel size, not on
//! the colour depth — hence the separate focus and lens regions with their own
//! cell sizes.

// The renderer is called by the panel preview (phase 2) and is the spec the
// Metal shader follows (phase 3); until those land only tests use it.
#![allow(dead_code)]

pub mod config;

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

// ── Palettes ────────────────────────────────────────────────────────────────

/// The palette file is shared with the frontend (single source).
const PALETTES_JSON: &str = include_str!("../../../frontend/src/lib/retro-palettes.json");

#[derive(Debug, Clone, Deserialize)]
struct RawFixed {
    id: String,
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    colors: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawDepth {
    id: String,
    name: String,
    #[serde(default)]
    aliases: Vec<String>,
    bits: u8,
}

#[derive(Debug, Clone, Deserialize)]
struct RawFile {
    fixed: Vec<RawFixed>,
    depth: Vec<RawDepth>,
}

/// How a palette reduces colour.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reduce {
    /// 8-bit: nearest colour of a fixed list.
    Fixed { colors: Vec<[u8; 3]> },
    /// 16-bit: quantise each channel to `bits`.
    Depth { bits: u8 },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Palette {
    pub id: String,
    pub name: String,
    pub aliases: Vec<String>,
    /// `true` = belongs to the 8-bit mode (fixed list).
    pub eight_bit: bool,
    pub reduce: Reduce,
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// Pure: parse + validate the palette file. Rejects bad hex, empty lists,
/// bit depths outside 1..=8, and any id/alias used twice.
pub fn parse_palettes(json: &str) -> Result<Vec<Palette>, String> {
    let raw: RawFile = serde_json::from_str(json).map_err(|e| format!("palettes: {e}"))?;
    let mut out = Vec::new();
    for f in raw.fixed {
        if f.colors.is_empty() {
            return Err(format!("palette {} has no colours", f.id));
        }
        let colors = f
            .colors
            .iter()
            .map(|c| parse_hex(c).ok_or_else(|| format!("palette {}: bad colour {c}", f.id)))
            .collect::<Result<Vec<_>, _>>()?;
        out.push(Palette { id: f.id, name: f.name, aliases: f.aliases, eight_bit: true, reduce: Reduce::Fixed { colors } });
    }
    for d in raw.depth {
        if !(1..=8).contains(&d.bits) {
            return Err(format!("palette {}: bits {} out of 1..=8", d.id, d.bits));
        }
        out.push(Palette { id: d.id, name: d.name, aliases: d.aliases, eight_bit: false, reduce: Reduce::Depth { bits: d.bits } });
    }
    let mut seen = std::collections::HashSet::new();
    for p in &out {
        for k in std::iter::once(&p.id).chain(p.aliases.iter()) {
            let k = k.to_lowercase();
            if !seen.insert(k.clone()) {
                return Err(format!("palette name {k} used twice"));
            }
        }
    }
    Ok(out)
}

/// The bundled palettes (parsed once; the file is validated by a test).
pub fn palettes() -> &'static [Palette] {
    static P: OnceLock<Vec<Palette>> = OnceLock::new();
    P.get_or_init(|| parse_palettes(PALETTES_JSON).unwrap_or_default())
}

/// Look up a palette by id or alias (case-insensitive).
pub fn find_palette(token: &str) -> Option<&'static Palette> {
    let t = token.trim().to_lowercase();
    palettes().iter().find(|p| p.id == t || p.aliases.iter().any(|a| a.to_lowercase() == t))
}

// ── Colour reduction ────────────────────────────────────────────────────────

/// Pure: quantise one 0..=255 channel to `bits` and expand back to 8 bit.
pub fn quantize_channel(v: u8, bits: u8) -> u8 {
    let bits = bits.clamp(1, 8);
    if bits == 8 {
        return v;
    }
    let max = ((1u32 << bits) - 1) as f32;
    let level = (v as f32 / 255.0 * max).round();
    (level / max * 255.0).round() as u8
}

/// Luminance weights (Rec. 601) used for the palette distance — the eye is
/// far more sensitive to green than to blue.
const LUMA_W: [f32; 3] = [0.299, 0.587, 0.114];

/// Pure: luminance-weighted squared distance.
pub fn luma_distance(a: [f32; 3], b: [u8; 3]) -> f32 {
    (0..3).map(|i| LUMA_W[i] * (a[i] - b[i] as f32).powi(2)).sum()
}

/// Pure: nearest colour of `palette`. An empty palette returns the input.
pub fn nearest_color(c: [f32; 3], palette: &[[u8; 3]]) -> [u8; 3] {
    palette
        .iter()
        .copied()
        .min_by(|a, b| luma_distance(c, *a).total_cmp(&luma_distance(c, *b)))
        .unwrap_or([c[0].clamp(0.0, 255.0) as u8, c[1].clamp(0.0, 255.0) as u8, c[2].clamp(0.0, 255.0) as u8])
}

/// Pure: the classic recursive Bayer index matrix (n = 2, 4 or 8; other values
/// fall back to 4). Values are 0..n²-1, row-major.
pub fn bayer_matrix(n: usize) -> Vec<u32> {
    let n = if matches!(n, 2 | 4 | 8) { n } else { 4 };
    let mut m = vec![0u32];
    let mut size = 1;
    while size < n {
        let mut next = vec![0u32; (size * 2) * (size * 2)];
        for y in 0..size {
            for x in 0..size {
                let v = 4 * m[y * size + x];
                let s2 = size * 2;
                next[y * s2 + x] = v;
                next[y * s2 + x + size] = v + 2;
                next[(y + size) * s2 + x] = v + 3;
                next[(y + size) * s2 + x + size] = v + 1;
            }
        }
        m = next;
        size *= 2;
    }
    m
}

/// Pure: the Bayer threshold for cell (x, y), centred to [-0.5, 0.5).
pub fn bayer_threshold(n: usize, x: i64, y: i64) -> f32 {
    let m = bayer_matrix(n);
    let n = (m.len() as f64).sqrt() as i64;
    let idx = (y.rem_euclid(n) * n + x.rem_euclid(n)) as usize;
    (m[idx] as f32 + 0.5) / (n * n) as f32 - 0.5
}

/// Pure: how far one dither step moves a channel. For a channel depth this is
/// one quantisation step; for a fixed palette it shrinks with the number of
/// colours (cube root ≈ levels per channel).
pub fn dither_spread(reduce: &Reduce) -> f32 {
    match reduce {
        Reduce::Depth { bits } => 255.0 / ((1u32 << (*bits).clamp(1, 8)) - 1) as f32,
        Reduce::Fixed { colors } => 255.0 / (colors.len().max(2) as f32).cbrt(),
    }
}

/// Pure: reduce one colour, after adding the dither offset.
pub fn reduce_color(c: [u8; 3], reduce: &Reduce, offset: f32) -> [u8; 3] {
    let shifted = [c[0] as f32 + offset, c[1] as f32 + offset, c[2] as f32 + offset];
    match reduce {
        Reduce::Depth { bits } => {
            let q = |v: f32| quantize_channel(v.round().clamp(0.0, 255.0) as u8, *bits);
            [q(shifted[0]), q(shifted[1]), q(shifted[2])]
        }
        Reduce::Fixed { colors } => nearest_color(shifted, colors),
    }
}

/// Darkest and lightest colour of a reduction — used for retro frames.
pub fn extremes(reduce: &Reduce) -> ([u8; 3], [u8; 3]) {
    match reduce {
        Reduce::Depth { .. } => ([0, 0, 0], [255, 255, 255]),
        Reduce::Fixed { colors } => {
            let luma = |c: &[u8; 3]| LUMA_W[0] * c[0] as f32 + LUMA_W[1] * c[1] as f32 + LUMA_W[2] * c[2] as f32;
            let dark = colors.iter().copied().min_by(|a, b| luma(a).total_cmp(&luma(b))).unwrap_or([0; 3]);
            let light = colors.iter().copied().max_by(|a, b| luma(a).total_cmp(&luma(b))).unwrap_or([255; 3]);
            (dark, light)
        }
    }
}

// ── Geometry ────────────────────────────────────────────────────────────────

/// Pure: logical points → physical pixels.
pub fn pt_to_px(pt: f32, scale: f32) -> f32 {
    pt * scale.max(0.1)
}

/// Pure: the cell size in px for a pixel size in pt. 1 pt (or less) means
/// "no pixelation, colour reduction only" — one physical pixel per cell.
pub fn cell_px(pt: f32, scale: f32) -> f32 {
    if pt <= 1.0 { 1.0 } else { pt_to_px(pt, scale).max(1.0) }
}

/// Pure: snap a coordinate down to the global grid of `cell`.
pub fn snap(p: f32, cell: f32) -> f32 {
    (p / cell).floor() * cell
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

/// Pure: grow a rect outward to the background grid, so a region edge always
/// lies on a background cell edge (no half cells, no flicker when a window
/// moves by a pixel).
pub fn snap_rect_out(r: Rect, cell: f32) -> Rect {
    let x0 = snap(r.x, cell);
    let y0 = snap(r.y, cell);
    let x1 = ((r.x + r.w) / cell).ceil() * cell;
    let y1 = ((r.y + r.h) / cell).ceil() * cell;
    Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lens {
    pub cx: f32,
    pub cy: f32,
    pub radius: f32,
    /// `None` = show the original pixels; `Some(cell)` = that cell size.
    pub cell: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Regions {
    /// Focus rect (already in this monitor's px) + its cell size.
    pub focus: Option<(Rect, f32)>,
    pub lens: Option<Lens>,
}

/// Which region a pixel belongs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Region {
    Background,
    Focus(f32),
    /// Inside the lens, untouched.
    LensOriginal,
    Lens(f32),
}

/// Pure: classify a pixel. The lens test is done on the CENTRE of the
/// background cell, so the circle's edge is stepped on the background grid
/// (pixel look). The focus rect is snapped outward to the same grid.
pub fn region_at(x: f32, y: f32, bg_cell: f32, regions: &Regions) -> Region {
    if let Some(l) = regions.lens {
        let cx = snap(x, bg_cell) + bg_cell / 2.0;
        let cy = snap(y, bg_cell) + bg_cell / 2.0;
        if (cx - l.cx).powi(2) + (cy - l.cy).powi(2) <= l.radius * l.radius {
            return match l.cell {
                None => Region::LensOriginal,
                Some(c) => Region::Lens(c),
            };
        }
    }
    if let Some((r, cell)) = regions.focus {
        if snap_rect_out(r, bg_cell).contains(x, y) {
            return Region::Focus(cell);
        }
    }
    Region::Background
}

// ── Reference renderer ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct RenderParams {
    pub bg_cell: f32,
    pub reduce: Reduce,
    /// Bayer size (2/4/8) or `None` for no dithering.
    pub dither: Option<usize>,
    /// 0..=1
    pub dither_strength: f32,
    pub regions: Regions,
    /// Draw a frame around the focus region in the palette's light colour.
    pub focus_border: bool,
    /// Scanline darkening 0..=1 (`None` = off) and the line height in px.
    pub scanlines: Option<f32>,
    pub scanline_px: f32,
    /// Vignette strength 0..=1 (`None` = off).
    pub vignette: Option<f32>,
}

/// Pure: render an RGBA frame (`w*h*4` bytes) with the retro effect. Each
/// cell samples the source at its centre (exactly what the shader does).
/// CRT curvature is shader-only and not part of the reference.
pub fn render(src: &[u8], w: usize, h: usize, p: &RenderParams) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    if src.len() < w * h * 4 || w == 0 || h == 0 {
        return out;
    }
    let spread = dither_spread(&p.reduce);
    let (_, light) = extremes(&p.reduce);
    let focus_frame = if p.focus_border { p.regions.focus.map(|(r, _)| snap_rect_out(r, p.bg_cell)) } else { None };
    let sample = |x: f32, y: f32| -> [u8; 3] {
        let xi = (x as usize).min(w - 1);
        let yi = (y as usize).min(h - 1);
        let i = (yi * w + xi) * 4;
        [src[i], src[i + 1], src[i + 2]]
    };
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut c = match region_at(fx, fy, p.bg_cell, &p.regions) {
                Region::LensOriginal => sample(fx, fy),
                Region::Background => cell_color(fx, fy, p.bg_cell, p, spread, &sample),
                Region::Focus(cell) | Region::Lens(cell) => cell_color(fx, fy, cell, p, spread, &sample),
            };
            if let Some(r) = focus_frame {
                let b = p.bg_cell.max(1.0);
                let inside = r.contains(fx, fy);
                let inner = Rect { x: r.x + b, y: r.y + b, w: r.w - 2.0 * b, h: r.h - 2.0 * b };
                if inside && !inner.contains(fx, fy) {
                    c = light;
                }
            }
            let mut f = 1.0f32;
            if let Some(s) = p.scanlines {
                if (fy / p.scanline_px.max(1.0)).floor() as i64 % 2 == 1 {
                    f *= 1.0 - 0.5 * s.clamp(0.0, 1.0);
                }
            }
            if let Some(v) = p.vignette {
                let dx = fx / w as f32 - 0.5;
                let dy = fy / h as f32 - 0.5;
                f *= 1.0 - v.clamp(0.0, 1.0) * 0.6 * (dx * dx + dy * dy) * 2.0;
            }
            let i = (y * w + x) * 4;
            for k in 0..3 {
                out[i + k] = (c[k] as f32 * f).round().clamp(0.0, 255.0) as u8;
            }
            out[i + 3] = 255;
        }
    }
    out
}

fn cell_color(
    x: f32,
    y: f32,
    cell: f32,
    p: &RenderParams,
    spread: f32,
    sample: &dyn Fn(f32, f32) -> [u8; 3],
) -> [u8; 3] {
    let cx = snap(x, cell);
    let cy = snap(y, cell);
    let c = sample(cx + cell / 2.0, cy + cell / 2.0);
    let offset = match p.dither {
        Some(n) if p.dither_strength > 0.0 => {
            bayer_threshold(n, (cx / cell) as i64, (cy / cell) as i64) * spread * p.dither_strength.clamp(0.0, 1.0)
        }
        _ => 0.0,
    };
    reduce_color(c, &p.reduce, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_palettes_parse_and_are_complete() {
        let p = parse_palettes(PALETTES_JSON).unwrap();
        let ids: Vec<&str> = p.iter().map(|p| p.id.as_str()).collect();
        for id in ["nes", "c64", "pico8", "gb", "cga", "gray", "snes", "megadrive", "amiga"] {
            assert!(ids.contains(&id), "missing {id}");
        }
        assert_eq!(find_palette("GameBoy").unwrap().id, "gb");
        assert_eq!(find_palette("genesis").unwrap().id, "megadrive");
        assert!(find_palette("nope").is_none());
        let bits = |id: &str| match &find_palette(id).unwrap().reduce {
            Reduce::Depth { bits } => *bits,
            _ => 0,
        };
        assert_eq!((bits("snes"), bits("amiga"), bits("megadrive")), (5, 4, 3));
    }

    #[test]
    fn palette_parser_rejects_bad_input() {
        let bad_hex = r#"{"fixed":[{"id":"a","name":"A","colors":["XYZ123"]}],"depth":[]}"#;
        assert!(parse_palettes(bad_hex).is_err());
        let empty = r#"{"fixed":[{"id":"a","name":"A","colors":[]}],"depth":[]}"#;
        assert!(parse_palettes(empty).is_err());
        let bits = r#"{"fixed":[],"depth":[{"id":"x","name":"X","bits":9}]}"#;
        assert!(parse_palettes(bits).is_err());
        let dup = r#"{"fixed":[{"id":"a","name":"A","colors":["000000"]}],"depth":[{"id":"b","name":"B","aliases":["A"],"bits":4}]}"#;
        assert!(parse_palettes(dup).unwrap_err().contains("twice"));
        assert!(parse_palettes("{").is_err());
    }

    #[test]
    fn channel_quantisation_5_4_3_bits() {
        // 5 bit: 32 levels, step 255/31
        assert_eq!(quantize_channel(0, 5), 0);
        assert_eq!(quantize_channel(255, 5), 255);
        assert_eq!(quantize_channel(128, 5), 132); // level 16 → 16*255/31 = 131.6
        // 4 bit: 16 levels, step 17
        assert_eq!(quantize_channel(128, 4), 136);
        assert_eq!(quantize_channel(8, 4), 0);
        assert_eq!(quantize_channel(9, 4), 17);
        // 3 bit: 8 levels
        assert_eq!(quantize_channel(128, 3), 146);
        // exactly 2^b distinct outputs
        for bits in [3u8, 4, 5] {
            let n = (0..=255u8).map(|v| quantize_channel(v, bits)).collect::<std::collections::HashSet<_>>().len();
            assert_eq!(n, 1 << bits);
        }
        assert_eq!(quantize_channel(77, 8), 77);
    }

    #[test]
    fn bayer_matrices_are_permutations_and_canonical() {
        assert_eq!(bayer_matrix(2), vec![0, 2, 3, 1]);
        assert_eq!(
            bayer_matrix(4),
            vec![0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5]
        );
        for n in [2usize, 4, 8] {
            let mut m = bayer_matrix(n);
            m.sort();
            assert_eq!(m, (0..(n * n) as u32).collect::<Vec<_>>());
        }
        // thresholds centred: mean ≈ 0, in [-0.5, 0.5)
        let ts: Vec<f32> = (0..4).flat_map(|y| (0..4).map(move |x| bayer_threshold(4, x, y))).collect();
        assert!(ts.iter().sum::<f32>().abs() < 1e-5);
        assert!(ts.iter().all(|t| (-0.5..0.5).contains(t)));
        // wraps on negative coordinates too
        assert_eq!(bayer_threshold(4, -1, 0), bayer_threshold(4, 3, 0));
    }

    #[test]
    fn nearest_color_uses_luminance_weights() {
        let pal = [[0, 0, 0], [255, 255, 255], [255, 0, 0]];
        assert_eq!(nearest_color([250.0, 10.0, 10.0], &pal), [255, 0, 0]);
        assert_eq!(nearest_color([20.0, 20.0, 20.0], &pal), [0, 0, 0]);
        // pure blue is perceptually dark: closer to black than to white
        assert_eq!(nearest_color([0.0, 0.0, 255.0], &[[0, 0, 0], [255, 255, 255]]), [0, 0, 0]);
        // pure green is perceptually bright
        assert_eq!(nearest_color([0.0, 255.0, 0.0], &[[0, 0, 0], [255, 255, 255]]), [255, 255, 255]);
    }

    #[test]
    fn points_to_pixels_at_1x_and_2x() {
        assert_eq!(pt_to_px(2.0, 1.0), 2.0);
        assert_eq!(pt_to_px(2.0, 2.0), 4.0);
        assert_eq!(cell_px(4.0, 2.0), 8.0);
        assert_eq!(cell_px(1.0, 2.0), 1.0); // 1 pt = colour reduction only
        assert_eq!(cell_px(1.5, 2.0), 3.0);
    }

    #[test]
    fn regions_snap_to_the_background_grid() {
        let bg = 8.0;
        let r = snap_rect_out(Rect { x: 10.0, y: 3.0, w: 20.0, h: 10.0 }, bg);
        assert_eq!(r, Rect { x: 8.0, y: 0.0, w: 24.0, h: 16.0 });
        let regions = Regions { focus: Some((Rect { x: 10.0, y: 3.0, w: 20.0, h: 10.0 }, 2.0)), lens: None };
        assert_eq!(region_at(8.5, 0.5, bg, &regions), Region::Focus(2.0)); // grown edge
        assert_eq!(region_at(7.5, 0.5, bg, &regions), Region::Background);
        assert_eq!(region_at(31.5, 15.5, bg, &regions), Region::Focus(2.0));
        assert_eq!(region_at(32.5, 15.5, bg, &regions), Region::Background);
    }

    #[test]
    fn lens_edge_is_stepped_on_the_grid_and_wins_over_focus() {
        let bg = 8.0;
        let lens = Lens { cx: 40.0, cy: 40.0, radius: 12.0, cell: None };
        let regions = Regions { focus: Some((Rect { x: 0.0, y: 0.0, w: 80.0, h: 80.0 }, 2.0)), lens: Some(lens) };
        // every pixel of one background cell gets the same answer
        for (x, y) in [(32.0, 32.0), (32.5, 39.5), (39.9, 33.0)] {
            assert_eq!(region_at(x, y, bg, &regions), Region::LensOriginal);
        }
        // a cell whose centre is outside the circle is entirely outside,
        // even if one of its corners is inside
        assert_eq!(region_at(52.1, 48.1, bg, &regions), Region::Focus(2.0));
        // Property: every pixel of every background cell lands in the same
        // region — the circle's edge is stepped, never cuts through a cell.
        for cy in 0..10 {
            for cx in 0..10 {
                let first = region_at(cx as f32 * bg + 0.5, cy as f32 * bg + 0.5, bg, &regions);
                for dy in 0..8 {
                    for dx in 0..8 {
                        let (x, y) = (cx as f32 * bg + dx as f32 + 0.5, cy as f32 * bg + dy as f32 + 0.5);
                        assert_eq!(region_at(x, y, bg, &regions), first, "cell ({cx},{cy}) split at ({x},{y})");
                    }
                }
            }
        }
        let with_cell = Regions { lens: Some(Lens { cell: Some(3.0), ..lens }), ..regions };
        assert_eq!(region_at(36.0, 36.0, bg, &with_cell), Region::Lens(3.0));
    }

    fn gradient(w: usize, h: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            for x in 0..w {
                v.extend_from_slice(&[(x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 128, 255]);
            }
        }
        v
    }

    fn params(reduce: Reduce) -> RenderParams {
        RenderParams {
            bg_cell: 4.0,
            reduce,
            dither: None,
            dither_strength: 0.0,
            regions: Regions::default(),
            focus_border: false,
            scanlines: None,
            scanline_px: 2.0,
            vignette: None,
        }
    }

    #[test]
    fn cells_are_uniform_and_colours_are_from_the_palette() {
        let (w, h) = (16, 16);
        let src = gradient(w, h);
        let pal = find_palette("pico8").unwrap().reduce.clone();
        let colors = match &pal { Reduce::Fixed { colors } => colors.clone(), _ => unreachable!() };
        let out = render(&src, w, h, &params(pal));
        let px = |x: usize, y: usize| [out[(y * w + x) * 4], out[(y * w + x) * 4 + 1], out[(y * w + x) * 4 + 2]];
        for cy in 0..4 {
            for cx in 0..4 {
                let first = px(cx * 4, cy * 4);
                assert!(colors.contains(&first));
                for dy in 0..4 {
                    for dx in 0..4 {
                        assert_eq!(px(cx * 4 + dx, cy * 4 + dy), first);
                    }
                }
            }
        }
    }

    #[test]
    fn depth_mode_output_stays_on_the_channel_grid_with_dither() {
        let (w, h) = (16, 16);
        let src = gradient(w, h);
        let mut p = params(Reduce::Depth { bits: 3 });
        p.dither = Some(4);
        p.dither_strength = 1.0;
        let out = render(&src, w, h, &p);
        let levels: std::collections::HashSet<u8> = (0..=255u8).map(|v| quantize_channel(v, 3)).collect();
        assert!(out.chunks(4).all(|c| levels.contains(&c[0]) && levels.contains(&c[1]) && c[3] == 255));
    }

    #[test]
    fn dithering_changes_a_flat_mid_tone_into_a_pattern() {
        // flat grey exactly between two 1-bit levels: without dither one
        // colour, with dither both appear
        let (w, h) = (8, 8);
        let src: Vec<u8> = std::iter::repeat_n([127u8, 127, 127, 255], w * h).flatten().collect();
        let mut p = params(Reduce::Depth { bits: 1 });
        p.bg_cell = 1.0;
        let plain = render(&src, w, h, &p);
        let set = |v: &Vec<u8>| v.chunks(4).map(|c| c[0]).collect::<std::collections::HashSet<_>>();
        assert_eq!(set(&plain).len(), 1);
        p.dither = Some(2);
        p.dither_strength = 1.0;
        let dith = render(&src, w, h, &p);
        assert_eq!(set(&dith), [0u8, 255].into_iter().collect());
        // fixed Bayer pattern on a fixed image → deterministic result
        assert_eq!(dith, render(&src, w, h, &p));
    }

    #[test]
    fn lens_original_keeps_source_pixels_and_focus_border_uses_light_colour() {
        let (w, h) = (32, 32);
        let src = gradient(w, h);
        let mut p = params(find_palette("gb").unwrap().reduce.clone());
        p.regions.lens = Some(Lens { cx: 16.0, cy: 16.0, radius: 5.0, cell: None });
        let out = render(&src, w, h, &p);
        let i = (16 * w + 16) * 4;
        assert_eq!(&out[i..i + 3], &src[i..i + 3]);

        let mut q = params(find_palette("gb").unwrap().reduce.clone());
        q.regions.focus = Some((Rect { x: 8.0, y: 8.0, w: 16.0, h: 16.0 }, 1.0));
        q.focus_border = true;
        let out = render(&src, w, h, &q);
        let (_, light) = extremes(&q.reduce);
        let j = (8 * w + 12) * 4; // top edge of the frame
        assert_eq!([out[j], out[j + 1], out[j + 2]], light);
    }

    #[test]
    fn scanlines_darken_every_other_line() {
        let (w, h) = (4, 4);
        let src: Vec<u8> = std::iter::repeat_n([200u8, 200, 200, 255], w * h).flatten().collect();
        let mut p = params(Reduce::Depth { bits: 8 });
        p.bg_cell = 1.0;
        p.scanlines = Some(1.0);
        p.scanline_px = 1.0;
        let out = render(&src, w, h, &p);
        assert_eq!(out[0], 200);
        assert_eq!(out[w * 4], 100);
    }

    #[test]
    fn short_or_empty_input_does_not_panic() {
        assert!(render(&[], 0, 0, &params(Reduce::Depth { bits: 5 })).is_empty());
        assert_eq!(render(&[1, 2, 3], 2, 2, &params(Reduce::Depth { bits: 5 })).len(), 16);
    }
}
