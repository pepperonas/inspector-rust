//! Standalone image operations triggered by the "power command" line
//! in the popup search bar (`rz <W>x<H>`, `optim`). Distinct from
//! `recolor.rs` (logo tinting) and `cutout_ml.rs` (ML background
//! removal): these are general-purpose, format-agnostic helpers.
//!
//! Both functions operate on whatever bitmap is currently on the
//! system clipboard:
//!
//! - [`resize_clipboard_image_lanczos`] reads → resizes via Lanczos3
//!   → writes back to clipboard + pushes a new history entry.
//! - [`optimize_clipboard_png`] reads → runs the embedded [`oxipng`]
//!   optimiser → writes the optimised PNG to `~/Downloads/<…>-optim-<ts>.png`
//!   (does *not* touch the clipboard).

use anyhow::{anyhow, Context, Result};
use clipboard_rs::{common::RustImage, Clipboard, ClipboardContext, RustImageData};
use image::{ImageFormat, ImageReader};
use std::io::Cursor;
use std::path::PathBuf;

/// Per-operation absolute size cap (in pixels). Same 16 MP ceiling as the
/// recolor / cutout pipelines — keeps the resize / optimise paths from
/// chewing through gigabytes of RAM on a misclick.
const MAX_PIXELS: u64 = 16 * 1024 * 1024;

/// Result of [`resize_clipboard_image_lanczos`] — the new dimensions
/// + PNG byte size, returned to the frontend so a toast can confirm
/// what landed on the clipboard.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResizeResult {
    pub width: u32,
    pub height: u32,
    pub bytes: usize,
}

/// Read the clipboard's bitmap, resize it to `(width, height)` using
/// Lanczos3 sampling (best quality for downscaling), re-encode as PNG,
/// and write the result back to the clipboard. Returns the new
/// dimensions + size in bytes.
///
/// Errors:
/// - clipboard has no image format set
/// - target dimensions are 0 or > MAX_PIXELS
/// - the bitmap fails to decode (shouldn't happen if the clipboard says it's an image)
pub fn resize_clipboard_image_lanczos(width: u32, height: u32) -> Result<ResizeResult> {
    if width == 0 || height == 0 {
        return Err(anyhow!("width and height must be > 0 (got {width}x{height})"));
    }
    let target_pixels = u64::from(width) * u64::from(height);
    if target_pixels > MAX_PIXELS {
        return Err(anyhow!(
            "target {width}x{height} = {target_pixels} px exceeds {MAX_PIXELS} px cap",
        ));
    }

    let bytes = read_clipboard_png()?;
    let img = ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .context("guess image format")?
        .decode()
        .context("decode clipboard image")?;

    let resized = img.resize_exact(width, height, image::imageops::FilterType::Lanczos3);

    let mut out = Vec::with_capacity(bytes.len());
    resized
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .context("encode resized PNG")?;

    write_clipboard_png(&out)?;

    Ok(ResizeResult {
        width,
        height,
        bytes: out.len(),
    })
}

/// Result of [`optimize_clipboard_png`] — the saved file path + before /
/// after byte counts, so the frontend can show "Saved 12.3 KB → 8.1 KB
/// (-34 %) to Downloads".
#[derive(Debug, Clone, serde::Serialize)]
pub struct OptimResult {
    pub path: PathBuf,
    pub before_bytes: usize,
    pub after_bytes: usize,
}

/// Read the clipboard's PNG, run it through oxipng (lossless), and
/// write the result to `~/Downloads/inspector-rust-optim-<ts>.png`.
/// Does NOT modify the clipboard. Returns the saved path + before/after
/// sizes.
pub fn optimize_clipboard_png() -> Result<OptimResult> {
    let bytes = read_clipboard_png()?;
    let before_bytes = bytes.len();

    // oxipng's in-memory API takes a Vec<u8> input + returns Vec<u8>.
    // Use Options::max_compression() — slowest but smallest output.
    // Acceptable for a user-triggered command (not a hot loop).
    let opts = oxipng::Options::max_compression();
    let optimised = oxipng::optimize_from_memory(&bytes, &opts)
        .context("oxipng optimise_from_memory failed")?;

    let after_bytes = optimised.len();
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let filename = format!("inspector-rust-optim-{stamp}.png");
    let mut path = dirs::download_dir().context("no Downloads dir on this platform")?;
    path.push(&filename);

    std::fs::write(&path, &optimised)
        .with_context(|| format!("write optimised PNG to {}", path.display()))?;

    Ok(OptimResult {
        path,
        before_bytes,
        after_bytes,
    })
}

// ── helpers ────────────────────────────────────────────────────────────

fn read_clipboard_png() -> Result<Vec<u8>> {
    let ctx = ClipboardContext::new()
        .map_err(|e| anyhow!("clipboard ctx init failed: {e:?}"))?;
    let img = ctx
        .get_image()
        .map_err(|e| anyhow!("no image on clipboard: {e:?}"))?;
    let png = img
        .to_png()
        .map_err(|e| anyhow!("clipboard image → PNG failed: {e:?}"))?;
    Ok(png.get_bytes().to_vec())
}

fn write_clipboard_png(bytes: &[u8]) -> Result<()> {
    let ctx = ClipboardContext::new()
        .map_err(|e| anyhow!("clipboard ctx init failed: {e:?}"))?;
    let img = RustImageData::from_bytes(bytes)
        .map_err(|e| anyhow!("decode PNG for clipboard write: {e:?}"))?;
    ctx.set_image(img)
        .map_err(|e| anyhow!("clipboard set_image failed: {e:?}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    #[allow(dead_code)]
    fn make_png(w: u32, h: u32) -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_fn(w, h, |x, y| Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png).unwrap();
        buf
    }

    #[test]
    fn resize_validates_dimensions_are_positive() {
        // We can't easily put an image on the clipboard from a unit test —
        // but we *can* assert the pre-check fires before we even try.
        let r = resize_clipboard_image_lanczos(0, 100);
        assert!(r.is_err(), "width=0 must be rejected");
        let r = resize_clipboard_image_lanczos(100, 0);
        assert!(r.is_err(), "height=0 must be rejected");
    }

    #[test]
    fn resize_rejects_oversized_targets() {
        // 5000 × 5000 = 25 MP > 16 MP cap.
        let r = resize_clipboard_image_lanczos(5000, 5000);
        assert!(r.is_err(), "target above MAX_PIXELS must be rejected");
    }

    #[test]
    fn max_pixels_is_16_megapixels() {
        // Locks the constant; a regression that lowers it without good
        // reason would silently start rejecting reasonable user requests.
        assert_eq!(MAX_PIXELS, 16 * 1024 * 1024);
    }

    #[test]
    fn resize_result_serialises_to_expected_shape() {
        let r = ResizeResult {
            width: 100,
            height: 200,
            bytes: 1234,
        };
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["width"], 100);
        assert_eq!(j["height"], 200);
        assert_eq!(j["bytes"], 1234);
    }

    #[test]
    fn optim_result_serialises_to_expected_shape() {
        let r = OptimResult {
            path: PathBuf::from("/tmp/foo.png"),
            before_bytes: 1000,
            after_bytes: 500,
        };
        let j = serde_json::to_value(&r).unwrap();
        assert_eq!(j["before_bytes"], 1000);
        assert_eq!(j["after_bytes"], 500);
        assert!(j["path"].as_str().unwrap().ends_with("foo.png"));
    }
}
