//! OCR via OS-native engines.
//!
//! macOS uses the Vision framework's `VNRecognizeTextRequest`, the
//! exact same engine that powers Apple's Live Text. No model bundles,
//! no network, no Python — same quality as Preview's "Copy Text from
//! Selection" but invoked from our region capture.
//!
//! Windows OCR (`Windows.Media.Ocr`) is stubbed for now — the macOS
//! path landed first because Vision is the path of least resistance
//! given the existing objc2 plumbing.

use anyhow::Result;

/// Run OCR on the supplied PNG bytes, returning the recognized text
/// joined with `\n` between observations (Vision returns one
/// `VNRecognizedTextObservation` per visual line). Empty string means
/// "the engine ran but found no text", which is a valid result — the
/// caller decides whether that's worth surfacing.
pub fn recognize(png_bytes: &[u8]) -> Result<String> {
    if png_bytes.is_empty() {
        anyhow::bail!("empty image data");
    }
    recognize_impl(png_bytes)
}

#[cfg(target_os = "macos")]
fn recognize_impl(png_bytes: &[u8]) -> Result<String> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use std::ffi::c_void;

    unsafe {
        // ── 1. Wrap the PNG bytes in an NSData ───────────────────────
        // dataWithBytes:length: copies the buffer, so `png_bytes` can
        // safely go out of scope after this call.
        let nsdata_cls = AnyClass::get(c"NSData")
            .ok_or_else(|| anyhow::anyhow!("NSData class not available"))?;
        let nsdata: *mut AnyObject = msg_send![
            nsdata_cls,
            dataWithBytes: png_bytes.as_ptr() as *const c_void,
            length: png_bytes.len()
        ];
        if nsdata.is_null() {
            anyhow::bail!("NSData allocation failed");
        }

        // ── 2. Build the VNImageRequestHandler ───────────────────────
        let handler_cls = AnyClass::get(c"VNImageRequestHandler").ok_or_else(|| {
            anyhow::anyhow!("VNImageRequestHandler not available — Vision framework not linked?")
        })?;
        let handler: *mut AnyObject = msg_send![handler_cls, alloc];
        let handler: *mut AnyObject = msg_send![
            handler,
            initWithData: nsdata,
            options: std::ptr::null::<AnyObject>()
        ];
        if handler.is_null() {
            anyhow::bail!("VNImageRequestHandler init failed");
        }

        // ── 3. Build the VNRecognizeTextRequest ──────────────────────
        let request_cls = AnyClass::get(c"VNRecognizeTextRequest")
            .ok_or_else(|| anyhow::anyhow!("VNRecognizeTextRequest not available"))?;
        let request: *mut AnyObject = msg_send![request_cls, alloc];
        // `init` (no completion handler) is the synchronous variant —
        // `perform` below will block until results are populated.
        let request: *mut AnyObject = msg_send![request, init];
        if request.is_null() {
            anyhow::bail!("VNRecognizeTextRequest init failed");
        }
        // Recognition level: 0 = Accurate (slower, much better), 1 = Fast.
        // For a one-shot user-triggered OCR the latency hit is fine.
        let _: () = msg_send![request, setRecognitionLevel: 0i64];
        let _: () = msg_send![request, setUsesLanguageCorrection: true];
        // Languages default to the user's preferred languages (all
        // installed Vision languages on macOS 13+). Don't override —
        // setting an empty array would make recognition fail entirely.

        // ── 4. Perform synchronously ─────────────────────────────────
        let array_cls = AnyClass::get(c"NSArray")
            .ok_or_else(|| anyhow::anyhow!("NSArray class not available"))?;
        let requests: *mut AnyObject = msg_send![array_cls, arrayWithObject: request];
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let ok: bool = msg_send![
            handler,
            performRequests: requests,
            error: &mut error
        ];
        if !ok {
            // Try to extract a descriptive error string.
            let msg = if !error.is_null() {
                let desc: *mut AnyObject = msg_send![error, localizedDescription];
                nsstring_to_rust(desc).unwrap_or_else(|| "unknown Vision error".to_string())
            } else {
                "Vision performRequests returned false without an error".to_string()
            };
            anyhow::bail!("OCR failed: {msg}");
        }

        // ── 5. Drain results ─────────────────────────────────────────
        let results: *mut AnyObject = msg_send![request, results];
        if results.is_null() {
            return Ok(String::new());
        }
        let count: usize = msg_send![results, count];
        let mut lines: Vec<String> = Vec::with_capacity(count);
        for i in 0..count {
            let observation: *mut AnyObject = msg_send![results, objectAtIndex: i];
            let candidates: *mut AnyObject = msg_send![observation, topCandidates: 1usize];
            if candidates.is_null() {
                continue;
            }
            let cand_count: usize = msg_send![candidates, count];
            if cand_count == 0 {
                continue;
            }
            let candidate: *mut AnyObject = msg_send![candidates, objectAtIndex: 0usize];
            let text_ns: *mut AnyObject = msg_send![candidate, string];
            if let Some(s) = nsstring_to_rust(text_ns) {
                lines.push(s);
            }
        }
        Ok(lines.join("\n"))
    }
}

/// Copy the UTF-8 contents of an `NSString *` into an owned Rust
/// `String`. Returns `None` if the pointer is null or the string isn't
/// representable as UTF-8 (Vision text is always valid UTF-8 in
/// practice, but we bail safely instead of unwrapping).
#[cfg(target_os = "macos")]
unsafe fn nsstring_to_rust(s: *mut objc2::runtime::AnyObject) -> Option<String> {
    use objc2::msg_send;
    use std::ffi::CStr;
    if s.is_null() {
        return None;
    }
    let utf8: *const i8 = msg_send![s, UTF8String];
    if utf8.is_null() {
        return None;
    }
    CStr::from_ptr(utf8).to_str().ok().map(str::to_owned)
}

#[cfg(target_os = "windows")]
fn recognize_impl(png_bytes: &[u8]) -> Result<String> {
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    // WinRT requires a COM apartment; worker threads aren't initialised by
    // default. S_FALSE = already done, RPC_E_CHANGED_MODE = STA — both fine.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };

    // ── 1. Write PNG bytes into an in-memory stream ──────────────────
    let stream = InMemoryRandomAccessStream::new()?;
    let output = stream.GetOutputStreamAt(0)?;
    let writer = DataWriter::CreateDataWriter(&output)?;
    writer.WriteBytes(png_bytes)?;
    writer.StoreAsync()?.get()?;
    let _ = writer.DetachStream(); // leave stream in a usable state
    stream.Seek(0)?;

    // ── 2. Decode PNG → SoftwareBitmap ───────────────────────────────
    let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
    let bitmap = decoder.GetSoftwareBitmapAsync()?.get()?;

    // ── 3. Create OCR engine from the user's installed language packs ─
    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| {
        anyhow::anyhow!(
            "Windows OCR: no language pack available — \
             install a language in Settings → Time & Language → Language"
        )
    })?;

    // ── 4. Recognise text (blocking) ─────────────────────────────────
    let ocr_result = engine.RecognizeAsync(&bitmap)?.get()?;

    // ── 5. Collect lines ─────────────────────────────────────────────
    let lines = ocr_result.Lines()?;
    let count = lines.Size()?;
    let mut parts: Vec<String> = Vec::with_capacity(count as usize);
    for i in 0..count {
        parts.push(lines.GetAt(i)?.Text()?.to_string());
    }
    Ok(parts.join("\n"))
}

/// Linux: Tesseract CLI (`apt install tesseract-ocr`). Offline, no extra Rust deps.
#[cfg(target_os = "linux")]
fn recognize_impl(png_bytes: &[u8]) -> Result<String> {
    use anyhow::Context;
    use chrono::Utc;
    use std::process::Command;

    let langs = tesseract_lang_arg()?;

    let tmp = std::env::temp_dir().join(format!(
        "inspector-rust-ocr-{}.png",
        Utc::now().timestamp_millis()
    ));
    std::fs::write(&tmp, png_bytes).context("write OCR temp png")?;

    let output = Command::new("tesseract")
        .arg(&tmp)
        .arg("stdout")
        .args(["-l", langs])
        .output()
        .with_context(|| format!("spawn tesseract -l {langs} ({TESSERACT_INSTALL_HINT})"))?;

    let _ = std::fs::remove_file(&tmp);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("tesseract failed: {stderr} ({TESSERACT_INSTALL_HINT})");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "linux")]
const TESSERACT_INSTALL_HINT: &str =
    "install: sudo apt install tesseract-ocr tesseract-ocr-eng; optional German: tesseract-ocr-deu";

/// Cached `-l` value: `eng`, `eng+deu`, or the best single pack found.
#[cfg(target_os = "linux")]
fn tesseract_lang_arg() -> Result<&'static str> {
    use std::sync::OnceLock;

    static LANGS: OnceLock<Result<String, String>> = OnceLock::new();
    LANGS
        .get_or_init(discover_tesseract_langs)
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{e}"))
        .map(|s| s.as_str())
}

#[cfg(target_os = "linux")]
fn discover_tesseract_langs() -> Result<String, String> {
    use std::process::Command;

    let output = Command::new("tesseract")
        .arg("--list-langs")
        .output()
        .map_err(|_| format!("tesseract not found ({TESSERACT_INSTALL_HINT})"))?;

    let langs: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let lang = line.trim();
            if lang.is_empty() || lang.starts_with("List of available") {
                None
            } else {
                Some(lang.to_string())
            }
        })
        .collect();

    pick_tesseract_langs(langs)
        .map_err(|_| format!("no OCR language packs found ({TESSERACT_INSTALL_HINT})"))
}

/// Prefer English; add German when installed; never require `deu` for English OCR.
#[cfg(target_os = "linux")]
fn pick_tesseract_langs(langs: Vec<String>) -> Result<String, &'static str> {
    if langs.is_empty() {
        return Err("no packs");
    }
    let has = |code: &str| langs.iter().any(|l| l == code);
    if has("eng") && has("deu") {
        return Ok("eng+deu".into());
    }
    if has("eng") {
        return Ok("eng".into());
    }
    if has("deu") {
        return Ok("deu".into());
    }
    langs.into_iter().find(|l| l != "osd").ok_or("no packs")
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tesseract_tests {
    use super::pick_tesseract_langs;

    #[test]
    fn prefers_eng_without_deu() {
        let langs = vec!["eng".to_string(), "osd".to_string()];
        assert_eq!(pick_tesseract_langs(langs).unwrap(), "eng");
    }

    #[test]
    fn uses_both_when_present() {
        let langs = vec!["deu".to_string(), "eng".to_string(), "osd".to_string()];
        assert_eq!(pick_tesseract_langs(langs).unwrap(), "eng+deu");
    }

    #[test]
    fn falls_back_to_deu_only() {
        let langs = vec!["deu".to_string(), "osd".to_string()];
        assert_eq!(pick_tesseract_langs(langs).unwrap(), "deu");
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn recognize_impl(_png_bytes: &[u8]) -> Result<String> {
    anyhow::bail!("OCR is not implemented on this platform")
}
