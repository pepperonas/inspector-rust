//! PNG recoloring for the "tint a clipboard image" feature.
//!
//! The intent is the same as ImageMagick's `+level-colors target,white`:
//! the darkest pixels in the source map to the target colour, the
//! brightest pixels stay white, and intermediate brightness is a linear
//! blend. Alpha is preserved untouched, so a logo on a transparent
//! background stays cleanly cut out.
//!
//! This is intentionally crude — it's not a full hue-shift, it just
//! re-tints monochrome silhouettes. Photos look weird; the UI gates the
//! feature behind a "looks mostly grayscale" sample check.

use anyhow::{Context, Result};
use image::{ImageBuffer, ImageFormat, Rgba};
use std::io::Cursor;

/// Pixel-count cap. A 4K-square PNG is 16M pixels and ~50 MB decoded as
/// RGBA8 — already the upper limit before recoloring becomes noticeable
/// on the UI thread. Larger inputs are rejected to keep latency bounded.
const MAX_PIXELS: u32 = 16_000_000;

/// Decode `png_bytes`, replace every pixel's RGB with a tint anchored at
/// `(r, g, b)` (darks → target, lights → white), and re-encode as PNG.
///
/// `alpha` of each pixel is preserved verbatim so transparent areas
/// remain transparent. The output is always 8-bit RGBA regardless of the
/// input's pixel type (paletted, grayscale, 16-bit, etc.) — `image`
/// handles the conversion in `to_rgba8`.
pub fn recolor_png(png_bytes: &[u8], r: u8, g: u8, b: u8) -> Result<Vec<u8>> {
    let img = image::load_from_memory_with_format(png_bytes, ImageFormat::Png)
        .context("decode PNG")?;
    let (w, h) = (img.width(), img.height());
    if w.saturating_mul(h) > MAX_PIXELS {
        anyhow::bail!("image too large to recolor ({}×{}), max 16 MP", w, h);
    }

    let rgba = img.to_rgba8();
    let mut out: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(w, h);

    let (tr, tg, tb) = (r as f32, g as f32, b as f32);
    for (px_in, px_out) in rgba.pixels().zip(out.pixels_mut()) {
        let [pr, pg, pb, pa] = px_in.0;
        // Perceptual luminance — the standard ITU-R BT.601 weights. For
        // already-grayscale PNGs (R=G=B), this collapses to L = R as
        // expected.
        let l = (0.299 * pr as f32 + 0.587 * pg as f32 + 0.114 * pb as f32) / 255.0;
        let mix = |target: f32| (target + (255.0 - target) * l).clamp(0.0, 255.0) as u8;
        *px_out = Rgba([mix(tr), mix(tg), mix(tb), pa]);
    }

    let mut buf: Vec<u8> = Vec::with_capacity(png_bytes.len());
    out.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .context("encode PNG")?;
    Ok(buf)
}

/// Sample up to `samples` opaque pixels and return the maximum
/// chromaticity (max-min channel divided by max). Result is in [0, 1] —
/// 0 means fully grayscale, 1 means at least one strongly saturated
/// pixel exists.
///
/// Used by the frontend to decide whether the recolor button is worth
/// surfacing on this entry. Photos will trip over this; logos and
/// monochrome silhouettes won't.
pub fn max_chromaticity_sample(png_bytes: &[u8], samples: u32) -> Result<f32> {
    let img = image::load_from_memory_with_format(png_bytes, ImageFormat::Png)
        .context("decode PNG")?;
    let rgba = img.to_rgba8();
    let total = rgba.width().saturating_mul(rgba.height());
    if total == 0 {
        return Ok(0.0);
    }
    let stride = (total / samples.max(1)).max(1);
    let mut max_chroma: f32 = 0.0;
    for (i, px) in rgba.pixels().enumerate() {
        if (i as u32) % stride != 0 {
            continue;
        }
        let [r, g, b, a] = px.0;
        if a < 16 {
            continue; // ignore (near-)transparent pixels
        }
        let mx = r.max(g).max(b) as f32;
        let mn = r.min(g).min(b) as f32;
        if mx <= 0.0 {
            continue;
        }
        let chroma = (mx - mn) / mx;
        if chroma > max_chroma {
            max_chroma = chroma;
        }
    }
    Ok(max_chroma)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_gray_png(w: u32, h: u32, gray: u8, alpha: u8) -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_fn(w, h, |_, _| Rgba([gray, gray, gray, alpha]));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png).unwrap();
        buf
    }

    #[test]
    fn recolors_dark_pixels_to_target() {
        // 100% black silhouette → recolor to red → expect pure red, alpha kept.
        let png = make_gray_png(4, 4, 0, 255);
        let out = recolor_png(&png, 220, 30, 30).unwrap();
        let img = image::load_from_memory(&out).unwrap().to_rgba8();
        let px = img.get_pixel(2, 2);
        assert_eq!(px.0, [220, 30, 30, 255]);
    }

    #[test]
    fn recolors_white_pixels_to_white() {
        // Pure white stays white regardless of target colour — that's the
        // anchor at the bright end of the tint.
        let png = make_gray_png(4, 4, 255, 255);
        let out = recolor_png(&png, 220, 30, 30).unwrap();
        let img = image::load_from_memory(&out).unwrap().to_rgba8();
        assert_eq!(img.get_pixel(2, 2).0, [255, 255, 255, 255]);
    }

    #[test]
    fn preserves_alpha() {
        let png = make_gray_png(4, 4, 128, 64);
        let out = recolor_png(&png, 0, 0, 255).unwrap();
        let img = image::load_from_memory(&out).unwrap().to_rgba8();
        assert_eq!(img.get_pixel(2, 2).0[3], 64);
    }

    #[test]
    fn rejects_oversized_images() {
        // 5000×5000 = 25 MP — should bail out.
        let big = make_gray_png(5000, 5000, 0, 255);
        assert!(recolor_png(&big, 1, 2, 3).is_err());
    }

    #[test]
    fn chromaticity_zero_for_grayscale() {
        let png = make_gray_png(8, 8, 128, 255);
        let c = max_chromaticity_sample(&png, 16).unwrap();
        assert!(c < 0.01);
    }

    #[test]
    fn chromaticity_high_for_pure_red() {
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_fn(8, 8, |_, _| Rgba([255, 0, 0, 255]));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png).unwrap();
        let c = max_chromaticity_sample(&buf, 16).unwrap();
        assert!(c > 0.9);
    }
}
