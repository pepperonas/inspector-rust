//! Pure bridge between the settings and a renderer: per-monitor geometry,
//! global-point → local-pixel conversion, capture size, and the uniform block
//! the GPU shader reads (`shader.metal` `Uniforms`, byte-for-byte). Shared by
//! the macOS and Windows overlays and by the CPU preview.

use super::config::{LensView, ModeSettings};
use super::font::FONT_5X7;
use super::{bayer_matrix, cell_px, dither_spread, extremes, snap_rect_out, Lens, Rect, Reduce, Regions, RenderParams};

/// One monitor: its origin and size in GLOBAL top-left points, and its
/// backing scale (Retina 2×). Mixed setups get one of these per monitor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Display {
    pub id: u32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub scale: f64,
}

impl Display {
    pub fn px_size(&self) -> (u32, u32) {
        ((self.w * self.scale).round() as u32, (self.h * self.scale).round() as u32)
    }

    /// Global point → this monitor's local physical pixel.
    pub fn to_local(self, gx: f64, gy: f64) -> (f32, f32) {
        (((gx - self.x) * self.scale) as f32, ((gy - self.y) * self.scale) as f32)
    }

    pub fn contains(&self, gx: f64, gy: f64) -> bool {
        gx >= self.x && gy >= self.y && gx < self.x + self.w && gy < self.y + self.h
    }

    /// Global rect (points) → local px rect, `None` when it misses this monitor.
    pub fn rect_to_local(&self, r: Rect) -> Option<Rect> {
        let (x0, y0) = self.to_local(r.x as f64, r.y as f64);
        let (x1, y1) = self.to_local((r.x + r.w) as f64, (r.y + r.h) as f64);
        let (w, h) = self.px_size();
        if x1 <= 0.0 || y1 <= 0.0 || x0 >= w as f32 || y0 >= h as f32 {
            return None;
        }
        Some(Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
    }
}

/// A window for the stage-2 frames (global points), front-to-back order.
#[derive(Debug, Clone, PartialEq)]
pub struct WinInfo {
    pub rect: Rect,
    pub title: String,
    pub focused: bool,
}

/// Live inputs that change between frames.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Live {
    /// Focused window in global points (`None` = unknown / no permission).
    pub focus: Option<Rect>,
    /// Focused app is full-screen on this monitor → whole monitor is focus.
    pub focus_fullscreen: bool,
    pub mouse: Option<(f64, f64)>,
    pub windows: Vec<WinInfo>,
}

/// Pure: the capture size for a monitor. Without focus and lens nothing
/// needs more than one texel per background cell, so we capture at that
/// size directly (minimal load); otherwise at full pixel resolution.
pub fn capture_size(d: &Display, s: &ModeSettings) -> (u32, u32) {
    let (w, h) = d.px_size();
    if s.focus || s.lens {
        return (w.max(1), h.max(1));
    }
    let cell = cell_px(s.pixel_pt, d.scale as f32).max(1.0);
    ((w as f32 / cell).ceil().max(1.0) as u32, (h as f32 / cell).ceil().max(1.0) as u32)
}

/// The focus rect in local px, already snapped outward to the background grid.
pub fn local_focus(d: &Display, s: &ModeSettings, live: &Live) -> Option<Rect> {
    if !s.focus {
        return None;
    }
    let bg = cell_px(s.pixel_pt, d.scale as f32);
    let (w, h) = d.px_size();
    if live.focus_fullscreen {
        return Some(Rect { x: 0.0, y: 0.0, w: w as f32, h: h as f32 });
    }
    live.focus.and_then(|r| d.rect_to_local(r)).map(|r| snap_rect_out(r, bg))
}

/// Everything a frame needs, in local px.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameParams {
    pub size: (u32, u32),
    pub bg_cell: f32,
    pub focus_cell: f32,
    pub focus: Option<Rect>,
    pub lens: Option<Lens>,
    pub reduce: Reduce,
    pub dither: Option<usize>,
    pub dither_strength: f32,
    pub focus_border: bool,
    pub scanlines: Option<f32>,
    pub scanline_px: f32,
    pub crt: Option<f32>,
    pub opacity: f32,
    pub sprite: Option<(f32, f32)>,
    pub windows: Vec<(Rect, String, bool)>,
    pub title_px: f32,
}

pub fn frame_params(d: &Display, s: &ModeSettings, live: &Live) -> FrameParams {
    let scale = d.scale as f32;
    let bg = cell_px(s.pixel_pt, scale);
    let focus_cell = cell_px(s.focus_pixel_pt, scale);
    let mouse = live.mouse.filter(|(x, y)| d.contains(*x, *y)).map(|(x, y)| d.to_local(x, y));
    let lens = if s.lens {
        mouse.map(|(cx, cy)| Lens {
            cx,
            cy,
            radius: s.lens_radius_pt as f32 * scale,
            cell: match s.lens_view {
                LensView::Original => None,
                LensView::Focus => Some(focus_cell),
            },
        })
    } else {
        None
    };
    let windows = if s.retro_frames {
        live.windows
            .iter()
            .filter_map(|w| d.rect_to_local(w.rect).map(|r| (snap_rect_out(r, bg), w.title.clone(), w.focused)))
            .take(MAX_WINDOWS)
            .collect()
    } else {
        Vec::new()
    };
    FrameParams {
        size: d.px_size(),
        bg_cell: bg,
        focus_cell,
        focus: local_focus(d, s, live),
        lens,
        reduce: s.reduce(),
        dither: s.dither.size(),
        dither_strength: s.dither_strength as f32 / 100.0,
        focus_border: s.focus_border,
        scanlines: s.scanlines.then_some(s.scanline_intensity as f32 / 100.0),
        scanline_px: (bg / 2.0).max(scale),
        crt: s.crt.then_some(s.crt_strength as f32 / 100.0),
        opacity: s.opacity as f32 / 100.0,
        sprite: if s.sprite_cursor { mouse } else { None },
        windows,
        title_px: (bg * 3.0).max(12.0 * scale),
    }
}

/// The CPU-reference version of the same frame (preview).
pub fn render_params(p: &FrameParams) -> RenderParams {
    RenderParams {
        bg_cell: p.bg_cell,
        reduce: p.reduce.clone(),
        dither: p.dither,
        dither_strength: p.dither_strength,
        regions: Regions { focus: p.focus.map(|r| (r, p.focus_cell)), lens: p.lens },
        focus_border: p.focus_border,
        scanlines: p.scanlines,
        scanline_px: p.scanline_px,
        vignette: p.crt,
    }
}

/// Scale a frame to a smaller preview: every px quantity shrinks with it,
/// cells never below one pixel.
pub fn scaled(p: &FrameParams, f: f32) -> FrameParams {
    let r = |r: Rect| Rect { x: r.x * f, y: r.y * f, w: r.w * f, h: r.h * f };
    FrameParams {
        size: (((p.size.0 as f32) * f).round().max(1.0) as u32, ((p.size.1 as f32) * f).round().max(1.0) as u32),
        bg_cell: (p.bg_cell * f).max(1.0),
        focus_cell: (p.focus_cell * f).max(1.0),
        focus: p.focus.map(r),
        lens: p.lens.map(|l| Lens { cx: l.cx * f, cy: l.cy * f, radius: l.radius * f, cell: l.cell.map(|c| (c * f).max(1.0)) }),
        scanline_px: (p.scanline_px * f).max(1.0),
        windows: p.windows.iter().map(|(w, t, fo)| (r(*w), t.clone(), *fo)).collect(),
        title_px: (p.title_px * f).max(1.0),
        sprite: p.sprite.map(|(x, y)| (x * f, y * f)),
        ..p.clone()
    }
}

// ── GPU packing (must match `shader.metal`) ─────────────────────────────────

pub const MAX_WINDOWS: usize = 32;
pub const MAX_TITLE: usize = 48;
pub const MAX_PALETTE: usize = 64;

// align(16): MSL rounds a struct holding a float4 up to 16 bytes.
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GpuUniforms {
    pub size: [f32; 2],
    pub bg_cell: f32,
    pub focus_cell: f32,
    pub focus: [f32; 4],
    pub lens_c: [f32; 2],
    pub lens_r: f32,
    pub lens_cell: f32,
    pub focus_on: i32,
    pub lens_mode: i32,
    pub reduce_fixed: i32,
    pub bits: i32,
    pub pal_n: i32,
    pub bayer_n: i32,
    pub spread: f32,
    pub dstr: f32,
    pub scan: i32,
    pub scan_i: f32,
    pub scan_px: f32,
    pub crt: i32,
    pub crt_s: f32,
    pub opacity: f32,
    pub border: i32,
    pub sprite: i32,
    pub light: [f32; 4],
    pub dark: [f32; 4],
    pub mouse: [f32; 2],
    pub win_n: i32,
    pub title_px: f32,
}

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GpuWin {
    pub r: [f32; 4],
    pub title_off: i32,
    pub title_len: i32,
    pub focused: i32,
    pub _pad: i32,
}

/// All GPU-side data of one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuFrame {
    pub uniforms: GpuUniforms,
    pub palette: Vec<[f32; 4]>,
    pub bayer: Vec<f32>,
    pub windows: Vec<GpuWin>,
    pub titles: Vec<u8>,
}

fn rgba(c: [u8; 3]) -> [f32; 4] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0]
}

pub fn pack(p: &FrameParams) -> GpuFrame {
    let (dark, light) = extremes(&p.reduce);
    let (reduce_fixed, bits, palette) = match &p.reduce {
        Reduce::Fixed { colors } => (1, 8, colors.iter().take(MAX_PALETTE).map(|c| rgba(*c)).collect()),
        Reduce::Depth { bits } => (0, *bits as i32, vec![[0.0; 4]]),
    };
    let (lens_mode, lens_c, lens_r, lens_cell) = match p.lens {
        None => (0, [0.0; 2], 0.0, 1.0),
        Some(l) => (if l.cell.is_some() { 2 } else { 1 }, [l.cx, l.cy], l.radius, l.cell.unwrap_or(1.0)),
    };
    let mut titles = Vec::new();
    let windows = p
        .windows
        .iter()
        .map(|(r, t, f)| {
            let off = titles.len() as i32;
            let bytes: Vec<u8> = t.chars().map(|c| if c.is_ascii() && (' '..='~').contains(&c) { c as u8 } else { b'?' }).take(MAX_TITLE).collect();
            titles.extend_from_slice(&bytes);
            GpuWin { r: [r.x, r.y, r.w, r.h], title_off: off, title_len: bytes.len() as i32, focused: *f as i32, _pad: 0 }
        })
        .collect::<Vec<_>>();
    if titles.is_empty() {
        titles.push(0);
    }
    let uniforms = GpuUniforms {
        size: [p.size.0 as f32, p.size.1 as f32],
        bg_cell: p.bg_cell,
        focus_cell: p.focus_cell,
        focus: p.focus.map(|r| [r.x, r.y, r.w, r.h]).unwrap_or([0.0; 4]),
        lens_c,
        lens_r,
        lens_cell,
        focus_on: p.focus.is_some() as i32,
        lens_mode,
        reduce_fixed,
        bits,
        pal_n: if reduce_fixed == 1 { palette.len() as i32 } else { 0 },
        bayer_n: p.dither.unwrap_or(0) as i32,
        spread: dither_spread(&p.reduce),
        dstr: if p.dither.is_some() { p.dither_strength } else { 0.0 },
        scan: p.scanlines.is_some() as i32,
        scan_i: p.scanlines.unwrap_or(0.0),
        scan_px: p.scanline_px,
        crt: p.crt.is_some() as i32,
        crt_s: p.crt.unwrap_or(0.0),
        opacity: p.opacity,
        border: p.focus_border as i32,
        sprite: p.sprite.is_some() as i32,
        light: rgba(light),
        dark: rgba(dark),
        mouse: p.sprite.map(|(x, y)| [x, y]).unwrap_or([0.0; 2]),
        win_n: windows.len() as i32,
        title_px: p.title_px,
    };
    let bayer = match p.dither {
        Some(n) => bayer_matrix(n).into_iter().map(|v| v as f32).collect(),
        None => vec![0.0],
    };
    GpuFrame { uniforms, palette, bayer, windows: if windows.is_empty() { vec![GpuWin::default()] } else { windows }, titles }
}

/// Font buffer for the shader.
pub fn font() -> &'static [u8] {
    &FONT_5X7
}

#[cfg(test)]
mod tests {
    use super::super::config::{Mode, ModeSettings};
    use super::*;

    fn retina() -> Display {
        Display { id: 1, x: 0.0, y: 0.0, w: 1512.0, h: 982.0, scale: 2.0 }
    }
    fn external() -> Display {
        Display { id: 2, x: 1512.0, y: -200.0, w: 2560.0, h: 1440.0, scale: 1.0 }
    }

    #[test]
    fn gpu_layout_matches_the_shader() {
        // offsets from MSL alignment rules (float4 = 16, float2 = 8)
        assert_eq!(std::mem::size_of::<GpuUniforms>(), 160);
        assert_eq!(std::mem::offset_of!(GpuUniforms, focus), 16);
        assert_eq!(std::mem::offset_of!(GpuUniforms, lens_c), 32);
        assert_eq!(std::mem::offset_of!(GpuUniforms, focus_on), 48);
        assert_eq!(std::mem::offset_of!(GpuUniforms, sprite), 108);
        assert_eq!(std::mem::offset_of!(GpuUniforms, light), 112);
        assert_eq!(std::mem::offset_of!(GpuUniforms, dark), 128);
        assert_eq!(std::mem::offset_of!(GpuUniforms, mouse), 144);
        assert_eq!(std::mem::offset_of!(GpuUniforms, title_px), 156);
        assert_eq!(std::mem::size_of::<GpuWin>(), 32);
    }

    #[test]
    fn points_map_to_local_pixels_per_monitor() {
        assert_eq!(retina().px_size(), (3024, 1964));
        assert_eq!(retina().to_local(100.0, 50.0), (200.0, 100.0));
        assert_eq!(external().to_local(1512.0 + 100.0, -200.0 + 50.0), (100.0, 50.0));
        // a window on the external screen misses the Retina one
        let w = Rect { x: 1600.0, y: 0.0, w: 400.0, h: 300.0 };
        assert!(retina().rect_to_local(w).is_none());
        assert_eq!(external().rect_to_local(w), Some(Rect { x: 88.0, y: 200.0, w: 400.0, h: 300.0 }));
    }

    #[test]
    fn same_setting_looks_the_same_at_1x_and_2x() {
        let s = ModeSettings::defaults(Mode::Eight); // 4 pt
        let a = frame_params(&retina(), &s, &Live::default());
        let b = frame_params(&external(), &s, &Live::default());
        assert_eq!((a.bg_cell, b.bg_cell), (8.0, 4.0)); // both 4 pt
    }

    #[test]
    fn capture_is_low_res_without_focus_and_lens() {
        let s = ModeSettings::defaults(Mode::Eight); // 4 pt → 8 px cells on Retina
        assert_eq!(capture_size(&retina(), &s), (378, 246));
        let f = ModeSettings { focus: true, ..s.clone() };
        assert_eq!(capture_size(&retina(), &f), (3024, 1964));
        let l = ModeSettings { lens: true, ..s };
        assert_eq!(capture_size(&external(), &l), (2560, 1440));
    }

    #[test]
    fn focus_rect_is_local_and_snapped_and_fullscreen_covers_all() {
        let s = ModeSettings { focus: true, ..ModeSettings::defaults(Mode::Eight) };
        let live = Live { focus: Some(Rect { x: 10.0, y: 3.0, w: 100.0, h: 50.0 }), ..Live::default() };
        let r = local_focus(&retina(), &s, &live).unwrap();
        assert_eq!(r, Rect { x: 16.0, y: 0.0, w: 208.0, h: 112.0 }); // 20,6 → 16,0 ; 220,106 → 224,112
        let full = Live { focus_fullscreen: true, ..live.clone() };
        assert_eq!(local_focus(&retina(), &s, &full).unwrap(), Rect { x: 0.0, y: 0.0, w: 3024.0, h: 1964.0 });
        let off = ModeSettings { focus: false, ..s };
        assert!(local_focus(&retina(), &off, &live).is_none());
    }

    #[test]
    fn lens_follows_the_mouse_only_on_its_monitor() {
        let s = ModeSettings { lens: true, lens_radius_pt: 80, ..ModeSettings::defaults(Mode::Eight) };
        let on = Live { mouse: Some((100.0, 100.0)), ..Live::default() };
        let l = frame_params(&retina(), &s, &on).lens.unwrap();
        assert_eq!((l.cx, l.cy, l.radius, l.cell), (200.0, 200.0, 160.0, None));
        assert!(frame_params(&external(), &s, &on).lens.is_none());
        let f = ModeSettings { lens_view: LensView::Focus, focus_pixel_pt: 1.5, ..s };
        assert_eq!(frame_params(&retina(), &f, &on).lens.unwrap().cell, Some(3.0));
    }

    #[test]
    fn pack_carries_palette_dither_and_titles() {
        let s = ModeSettings { retro_frames: true, ..ModeSettings::defaults(Mode::Eight) };
        let live = Live {
            windows: vec![WinInfo { rect: Rect { x: 0.0, y: 0.0, w: 200.0, h: 100.0 }, title: "Notes — ä".into(), focused: true }],
            ..Live::default()
        };
        let g = pack(&frame_params(&retina(), &s, &live));
        assert_eq!(g.uniforms.reduce_fixed, 1);
        assert_eq!(g.uniforms.pal_n as usize, g.palette.len());
        assert_eq!(g.uniforms.bayer_n, 4);
        assert_eq!(g.bayer.len(), 16);
        assert_eq!(g.uniforms.win_n, 1);
        assert_eq!(&g.titles, b"Notes ? ?"); // non-ASCII → '?'
        let d = pack(&frame_params(&retina(), &ModeSettings::defaults(Mode::Sixteen), &Live::default()));
        assert_eq!((d.uniforms.reduce_fixed, d.uniforms.bits, d.uniforms.pal_n), (0, 5, 0));
        assert_eq!(d.uniforms.focus_on, 0); // focus on, but no focused window known
    }

    #[test]
    fn preview_scaling_shrinks_every_px_quantity() {
        let s = ModeSettings { focus: true, ..ModeSettings::defaults(Mode::Eight) };
        let live = Live { focus: Some(Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 }), ..Live::default() };
        let p = frame_params(&retina(), &s, &live);
        let q = scaled(&p, 0.125);
        assert_eq!(q.size, (378, 246));
        assert_eq!(q.bg_cell, 1.0);
        assert_eq!(q.focus.unwrap().w, 200.0 * 0.125);
    }
}
