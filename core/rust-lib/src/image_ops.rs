#![allow(clippy::doc_lazy_continuation)]
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
/// Dimensions of the image currently on the clipboard, for the percentage
/// path of `rz` when no Finder selection is usable.
///
/// ⚠️ Reads the header only, and the caller computes the target with the SAME
/// `targetSize` used for files — duplicating that maths in Rust would let the
/// two drift apart.
pub fn clipboard_image_dimensions() -> Result<(u32, u32)> {
    let bytes = read_clipboard_png()?;
    let dims = ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .context("guess clipboard image format")?
        .into_dimensions()
        .context("read clipboard image dimensions")?;
    Ok(dims)
}

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

/// Result of [`resize_file_to_neighbor`] — output path + dimensions.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ResizeFileResult {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub bytes: usize,
}

/// Read `src`, Lanczos3-resize to `(width, height)`, and write the
/// result next to the source as `<stem>-<W>x<H>.<ext>`. Preserves the
/// source format (PNG stays PNG, JPEG stays JPEG, …). Source is NOT
/// touched. Returns the output path + dimensions + size in bytes.
///
/// Errors:
/// - target dimensions are 0 or > MAX_PIXELS
/// - source can't be opened / decoded
/// - source has no `.<ext>` (we refuse to invent one)
/// What the `rz` preview needs to know about one selected file.
///
/// Every field is optional on purpose: an unreadable or non-image file must be
/// REPORTED, not silently dropped — a preview that quietly shows fewer images
/// than are selected is worse than one that says "not readable".
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImageInfo {
    pub path: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub format: Option<String>,
}

/// Human label for an image format. Pure + tested.
///
/// ⚠️ `image::ImageFormat` is `#[non_exhaustive]`, so the wildcard arm is
/// mandatory — and it falls back to the format's own extension rather than
/// inventing a name.
pub fn format_label(fmt: image::ImageFormat) -> String {
    match fmt {
        image::ImageFormat::Png => "PNG".into(),
        image::ImageFormat::Jpeg => "JPEG".into(),
        image::ImageFormat::Gif => "GIF".into(),
        image::ImageFormat::WebP => "WebP".into(),
        image::ImageFormat::Bmp => "BMP".into(),
        image::ImageFormat::Tiff => "TIFF".into(),
        image::ImageFormat::Ico => "ICO".into(),
        image::ImageFormat::Avif => "AVIF".into(),
        other => other
            .extensions_str()
            .first()
            .map(|e| e.to_uppercase())
            .unwrap_or_else(|| "?".into()),
    }
}

/// Probe one file for the `rz` preview.
///
/// ⚠️ Header-only: `image_dimensions` parses the header and does NOT decode.
/// The preview probes every selected file while the user types, and decoding a
/// handful of 40 MP JPEGs on each keystroke would freeze the UI.
pub fn probe_image(path: &std::path::Path) -> ImageInfo {
    let dims = image::image_dimensions(path).ok();
    let format = ImageReader::open(path)
        .ok()
        .and_then(|r| r.with_guessed_format().ok())
        .and_then(|r| r.format())
        .map(format_label);
    ImageInfo {
        path: path.to_string_lossy().into_owned(),
        width: dims.map(|d| d.0),
        height: dims.map(|d| d.1),
        format,
    }
}

pub fn resize_file_to_neighbor(
    src: &std::path::Path,
    width: u32,
    height: u32,
) -> Result<ResizeFileResult> {
    if width == 0 || height == 0 {
        return Err(anyhow!("width and height must be > 0 (got {width}x{height})"));
    }
    let target_pixels = u64::from(width) * u64::from(height);
    if target_pixels > MAX_PIXELS {
        return Err(anyhow!(
            "target {width}x{height} = {target_pixels} px exceeds {MAX_PIXELS} px cap",
        ));
    }

    let img = ImageReader::open(src)
        .with_context(|| format!("open source image {}", src.display()))?
        .with_guessed_format()
        .with_context(|| format!("guess image format for {}", src.display()))?
        .decode()
        .with_context(|| format!("decode source image {}", src.display()))?;

    let resized = img.resize_exact(width, height, image::imageops::FilterType::Lanczos3);

    // Output path: same dir, stem suffixed with `-WxH`, same extension.
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("source has no readable file stem: {}", src.display()))?;
    let ext = src
        .extension()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("source has no extension; refusing to invent one: {}", src.display()))?;
    let dir = src
        .parent()
        .ok_or_else(|| anyhow!("source has no parent dir: {}", src.display()))?;
    let out_path = dir.join(format!("{stem}-{width}x{height}.{ext}"));

    // Format from the extension. `image::ImageFormat::from_extension`
    // lowercases internally and recognises every format our cargo
    // features pull in (PNG/JPEG/WebP/GIF/BMP).
    let format = ImageFormat::from_extension(ext)
        .ok_or_else(|| anyhow!("unsupported image extension: {ext}"))?;

    let file = std::fs::File::create(&out_path)
        .with_context(|| format!("create output file {}", out_path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    resized
        .write_to(&mut writer, format)
        .with_context(|| format!("encode resized image to {}", out_path.display()))?;
    use std::io::Write;
    writer
        .flush()
        .with_context(|| format!("flush output file {}", out_path.display()))?;
    drop(writer);

    let bytes = std::fs::metadata(&out_path)
        .map(|m| m.len() as usize)
        .unwrap_or(0);

    Ok(ResizeFileResult {
        path: out_path,
        width,
        height,
        bytes,
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

/// JPEG re-encode quality for the optimiser (lossy). 85 is visually close to
/// the source while giving real savings; the result is only kept if it's
/// actually smaller than the original.
const OPTIM_JPEG_QUALITY: u8 = 85;

/// Optimise an image file from disk, writing the result next to the source as
/// `<stem>-optim.<ext>`. The source is NOT touched. Returns the output path +
/// before/after sizes.
///
/// - **PNG** → `oxipng` max-compression (lossless).
/// - **JPEG** (`jpg`/`jpeg`) → re-encode at quality 85 (lossy) — strips
///   metadata, recompresses; **kept only if smaller** than the source (else the
///   original bytes are written, so the sibling is never larger).
///
/// Other formats error (the caller filters to png/jpg/jpeg first).
pub fn optimize_file_to_neighbor(src: &std::path::Path) -> Result<OptimResult> {
    let ext_lower = src
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    let bytes = std::fs::read(src)
        .with_context(|| format!("read source {}", src.display()))?;
    let before_bytes = bytes.len();

    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| anyhow!("source has no readable file stem: {}", src.display()))?;
    let dir = src
        .parent()
        .ok_or_else(|| anyhow!("source has no parent dir: {}", src.display()))?;

    let (out_name, out_bytes) = match ext_lower.as_str() {
        "png" => {
            let opts = oxipng::Options::max_compression();
            let optimised = oxipng::optimize_from_memory(&bytes, &opts)
                .with_context(|| format!("oxipng optimise {}", src.display()))?;
            (format!("{stem}-optim.png"), optimised)
        }
        "jpg" | "jpeg" => {
            let img = image::load_from_memory(&bytes)
                .with_context(|| format!("decode jpeg {}", src.display()))?
                .to_rgb8();
            let (w, h) = (img.width(), img.height());
            let mut buf: Vec<u8> = Vec::new();
            {
                let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut buf,
                    OPTIM_JPEG_QUALITY,
                );
                enc.encode(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
                    .with_context(|| format!("re-encode jpeg {}", src.display()))?;
            }
            // Keep the re-encode only if it actually shrank the file.
            let kept = if buf.len() < before_bytes { buf } else { bytes.clone() };
            (format!("{stem}-optim.jpg"), kept)
        }
        _ => {
            return Err(anyhow!(
                "optim supports PNG + JPEG; got `.{ext_lower}` for {}",
                src.display()
            ));
        }
    };

    let out_path = dir.join(out_name);
    std::fs::write(&out_path, &out_bytes)
        .with_context(|| format!("write optimised image to {}", out_path.display()))?;

    Ok(OptimResult {
        path: out_path,
        after_bytes: out_bytes.len(),
        before_bytes,
    })
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

pub fn write_clipboard_png(bytes: &[u8]) -> Result<()> {
    let ctx = ClipboardContext::new()
        .map_err(|e| anyhow!("clipboard ctx init failed: {e:?}"))?;
    let img = RustImageData::from_bytes(bytes)
        .map_err(|e| anyhow!("decode PNG for clipboard write: {e:?}"))?;
    ctx.set_image(img)
        .map_err(|e| anyhow!("clipboard set_image failed: {e:?}"))?;
    Ok(())
}

/// Like [`write_clipboard_png`], but also returns the **canonical** PNG base64 —
/// the bytes re-encoded through clipboard-rs's own PNG encoder, which is exactly
/// what the watcher will read back off the clipboard. Callers (e.g. `qr_copy_png`)
/// use this for the `mark_self_write` fuse + the stored history payload so the
/// watcher recognises our own write and doesn't create a duplicate `[image …]`
/// entry alongside the intended one.
pub fn write_clipboard_png_canonical(bytes: &[u8]) -> Result<String> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    let ctx = ClipboardContext::new()
        .map_err(|e| anyhow!("clipboard ctx init failed: {e:?}"))?;
    let img = RustImageData::from_bytes(bytes)
        .map_err(|e| anyhow!("decode PNG for clipboard write: {e:?}"))?;
    // Canonicalise through the same encoder the watcher uses on read-back.
    let canon = img
        .to_png()
        .map_err(|e| anyhow!("re-encode PNG for clipboard write: {e:?}"))?;
    let canon_b64 = B64.encode(canon.get_bytes());
    ctx.set_image(img)
        .map_err(|e| anyhow!("clipboard set_image failed: {e:?}"))?;
    Ok(canon_b64)
}

#[cfg(test)]
mod tests {
    #[test]
    fn format_labels_are_human_and_never_invented() {
        use image::ImageFormat as F;
        assert_eq!(format_label(F::Png), "PNG");
        assert_eq!(format_label(F::Jpeg), "JPEG");
        assert_eq!(format_label(F::WebP), "WebP");
        // ⚠️ ImageFormat is #[non_exhaustive]; the wildcard must still produce
        // something truthful rather than a made-up name.
        let other = format_label(F::Qoi);
        assert!(!other.is_empty());
        assert_eq!(other, other.to_uppercase());
    }

    #[test]
    fn probing_a_missing_file_reports_instead_of_panicking() {
        // A preview that silently drops unreadable files would show fewer
        // images than are selected — worse than saying "not readable".
        let info = probe_image(std::path::Path::new("/definitely/not/here.png"));
        assert!(info.width.is_none() && info.height.is_none() && info.format.is_none());
        assert!(info.path.ends_with("here.png"));
    }

    use super::*;
    use image::{ImageBuffer, Rgb, Rgba};

    fn make_png(w: u32, h: u32) -> Vec<u8> {
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_fn(w, h, |x, y| Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255]));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png).unwrap();
        buf
    }

    fn make_jpeg(w: u32, h: u32, quality: u8) -> Vec<u8> {
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(w, h, |x, y| {
            Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
        });
        let mut buf = Vec::new();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
        enc.encode(img.as_raw(), w, h, image::ExtendedColorType::Rgb8)
            .unwrap();
        buf
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ir-optim-{}-{}", std::process::id(), tag));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn optimize_file_png_writes_lossless_sibling() {
        let dir = scratch_dir("png");
        let src = dir.join("pic.png");
        std::fs::write(&src, make_png(80, 80)).unwrap();
        let r = optimize_file_to_neighbor(&src).unwrap();
        assert!(r.path.ends_with("pic-optim.png"));
        assert!(r.path.exists());
        assert!(r.after_bytes <= r.before_bytes);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn optimize_file_jpeg_recompresses_smaller() {
        let dir = scratch_dir("jpg");
        let src = dir.join("photo.jpg");
        // A high-quality JPEG → re-encode at 85 should shrink it.
        std::fs::write(&src, make_jpeg(160, 160, 98)).unwrap();
        let r = optimize_file_to_neighbor(&src).unwrap();
        assert!(r.path.ends_with("photo-optim.jpg"));
        assert!(r.path.exists());
        assert!(r.after_bytes <= r.before_bytes);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn optimize_file_rejects_unsupported_format() {
        let dir = scratch_dir("gif");
        let src = dir.join("anim.gif");
        std::fs::write(&src, b"GIF89a").unwrap();
        assert!(optimize_file_to_neighbor(&src).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resize_file_writes_sibling_with_target_dimensions_png() {
        let dir = scratch_dir("rz-png");
        let src = dir.join("pic.png");
        std::fs::write(&src, make_png(120, 90)).unwrap();
        let r = resize_file_to_neighbor(&src, 40, 30).unwrap();
        assert!(r.path.ends_with("pic-40x30.png"));
        // The output really decodes at the requested size.
        let out = image::ImageReader::open(&r.path).unwrap().decode().unwrap();
        assert_eq!((out.width(), out.height()), (40, 30));
        assert_eq!((r.width, r.height), (40, 30));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resize_file_preserves_jpeg_format() {
        let dir = scratch_dir("rz-jpg");
        let src = dir.join("photo.jpg");
        std::fs::write(&src, make_jpeg(100, 100, 90)).unwrap();
        let r = resize_file_to_neighbor(&src, 50, 50).unwrap();
        assert!(r.path.ends_with("photo-50x50.jpg"));
        // Sibling is a valid JPEG (format preserved from the source extension).
        let fmt = image::ImageReader::open(&r.path)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .format();
        assert_eq!(fmt, Some(ImageFormat::Jpeg));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resize_file_rejects_missing_extension() {
        let dir = scratch_dir("rz-noext");
        let src = dir.join("noext");
        std::fs::write(&src, make_png(20, 20)).unwrap();
        assert!(resize_file_to_neighbor(&src, 10, 10).is_err(), "no extension → refuse");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn resize_file_validates_dimensions_before_touching_disk() {
        let src = std::path::Path::new("/no/such/file.png");
        // Zero + oversized are rejected before the file is ever opened.
        assert!(resize_file_to_neighbor(src, 0, 100).is_err());
        assert!(resize_file_to_neighbor(src, 5000, 5000).is_err());
    }

    #[test]
    fn optimize_file_jpeg_never_grows() {
        // A heavily-compressed source: re-encoding at q85 could be larger, but the
        // optimiser keeps the original bytes in that case — the sibling is never bigger.
        let dir = scratch_dir("jpg-small");
        let src = dir.join("tiny.jpg");
        std::fs::write(&src, make_jpeg(32, 32, 20)).unwrap();
        let r = optimize_file_to_neighbor(&src).unwrap();
        assert!(r.after_bytes <= r.before_bytes, "after {} > before {}", r.after_bytes, r.before_bytes);
        std::fs::remove_dir_all(&dir).ok();
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
