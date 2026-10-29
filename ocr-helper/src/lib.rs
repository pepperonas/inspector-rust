//! Apple Vision text recognition for Inspector Rust.
//!
//! The library half is the Vision call itself. The binary half (`ir-ocr`)
//! runs it in a separate, long-lived process, and that is the point of this
//! crate: macOS compiles Vision's text model for the Neural Engine once per
//! **binary** and caches the result keyed by the binary's content (measured
//! 2026-10-29: an unchanged copy at another path stayed warm, a re-signed copy
//! went cold). Inspector Rust's main binary changes with every release, so
//! running Vision in-process cost 70–110 s on the first OCR after each update.
//! `ir-ocr` only changes when this crate does, so its cache survives updates.
//!
//! Wire format (both directions): one tag byte, a u32 little-endian length,
//! then that many bytes. Requests: `A` (accurate) or `F` (fast) + PNG bytes.
//! Replies: `T` + UTF-8 text, or `E` + UTF-8 error message.

use std::io::{Read, Write};

/// Vision's two recognition paths. `Fast` is much worse for German (no
/// umlauts, "€" read as "E") and waits behind a compiling `Accurate` model
/// anyway, so the app only uses `Accurate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Accurate,
    Fast,
}

pub const REQ_ACCURATE: u8 = b'A';
pub const REQ_FAST: u8 = b'F';
pub const REPLY_TEXT: u8 = b'T';
pub const REPLY_ERROR: u8 = b'E';

/// Largest frame accepted. A full 6K screen as PNG is well below this.
pub const MAX_FRAME: usize = 64 * 1024 * 1024;

/// Write one frame and flush.
pub fn write_frame(w: &mut impl Write, tag: u8, body: &[u8]) -> std::io::Result<()> {
    if body.len() > MAX_FRAME {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "frame too large"));
    }
    w.write_all(&[tag])?;
    w.write_all(&(body.len() as u32).to_le_bytes())?;
    w.write_all(body)?;
    w.flush()
}

/// Read one frame. `Ok(None)` = clean end of stream before a frame began.
pub fn read_frame(r: &mut impl Read) -> std::io::Result<Option<(u8, Vec<u8>)>> {
    let mut tag = [0u8; 1];
    if r.read(&mut tag)? == 0 {
        return Ok(None);
    }
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(Some((tag[0], body)))
}

/// Answer one request frame.
fn answer(tag: u8, png: &[u8]) -> (u8, Vec<u8>) {
    let level = match tag {
        REQ_ACCURATE => Level::Accurate,
        REQ_FAST => Level::Fast,
        other => return (REPLY_ERROR, format!("unknown request {other}").into_bytes()),
    };
    if png.is_empty() {
        return (REPLY_ERROR, b"empty image data".to_vec());
    }
    match recognize(png, level) {
        Ok(text) => (REPLY_TEXT, text.into_bytes()),
        Err(e) => (REPLY_ERROR, e.into_bytes()),
    }
}

/// The helper's main loop: answer frames until stdin ends or breaks.
pub fn serve(input: &mut impl Read, output: &mut impl Write) {
    while let Ok(Some((tag, body))) = read_frame(input) {
        let (reply, text) = answer(tag, &body);
        if write_frame(output, reply, &text).is_err() {
            return;
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn recognize(_png_bytes: &[u8], _level: Level) -> Result<String, String> {
    Err("Vision OCR is only available on macOS".to_string())
}

#[cfg(target_os = "macos")]
#[link(name = "Vision", kind = "framework")]
extern "C" {}

/// Run every recognition stage on the CPU instead of the Neural Engine.
///
/// On this M1 Pro the Neural Engine stopped loading freshly compiled models
/// (2026-10-28/29: `E5RT … create_precompiled_compute_operation … Code 13`,
/// surfacing as `CRImageReaderError 1`, every OCR failed after ~70 s, and a
/// restart of `aned` did not help). The CPU path read the same German text
/// identically (umlauts, €) in 0.34 s for a full page and never failed.
/// Best effort: if the API or the device is missing, Vision keeps its default.
#[cfg(target_os = "macos")]
unsafe fn use_cpu(request: *mut objc2::runtime::AnyObject) {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    let Some(cpu_cls) = AnyClass::get(c"MLCPUComputeDevice") else {
        return;
    };
    let responds: bool =
        msg_send![request, respondsToSelector: objc2::sel!(supportedComputeStageDevicesAndReturnError:)];
    if !responds {
        return;
    }
    let mut err: *mut AnyObject = std::ptr::null_mut();
    let stages: *mut AnyObject = msg_send![request, supportedComputeStageDevicesAndReturnError: &mut err];
    if stages.is_null() {
        return;
    }
    let keys: *mut AnyObject = msg_send![stages, allKeys];
    let n: usize = msg_send![keys, count];
    for i in 0..n {
        let stage: *mut AnyObject = msg_send![keys, objectAtIndex: i];
        let devices: *mut AnyObject = msg_send![stages, objectForKey: stage];
        let m: usize = msg_send![devices, count];
        for j in 0..m {
            let device: *mut AnyObject = msg_send![devices, objectAtIndex: j];
            let is_cpu: bool = msg_send![device, isKindOfClass: cpu_cls];
            if is_cpu {
                let _: () = msg_send![request, setComputeDevice: device, forComputeStage: stage];
                break;
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub fn recognize(png_bytes: &[u8], level: Level) -> Result<String, String> {
    use objc2::msg_send;
    use objc2::rc::autoreleasepool;
    use objc2::runtime::AnyObject;

    // Autorelease pool + explicit releases (fixed 2026-08-16). This runs on a
    // bare `std::thread::spawn` worker with NO pool, so every autoreleased
    // object — the NSData holding a full copy of the PNG, the results NSArray,
    // every recognised NSString — accumulated in a pool page that was never
    // drained; and the alloc/init'd handler + request (+1 owned, and the
    // handler retains the image data) were never released at all. Each OCR
    // therefore leaked the handler plus a copy of the source image: a day of
    // region-OCRs grew RSS by hundreds of MB with no way to reclaim it short
    // of quitting. The owned objects are tracked outside the fallible closure
    // so every early bail-out still releases them.
    autoreleasepool(|_| unsafe {
        let mut handler: *mut AnyObject = std::ptr::null_mut();
        let mut request: *mut AnyObject = std::ptr::null_mut();
        let result = recognize_in_pool(png_bytes, level, &mut handler, &mut request);
        if !request.is_null() {
            let _: () = msg_send![request, release];
        }
        if !handler.is_null() {
            let _: () = msg_send![handler, release];
        }
        result
    })
}

/// The fallible middle of [`recognize_impl`]. `handler`/`request` are handed
/// back to the caller AS SOON as they are created, so the caller can release
/// them no matter where this returns.
#[cfg(target_os = "macos")]
unsafe fn recognize_in_pool(
    png_bytes: &[u8],
    level: Level,
    handler_out: &mut *mut objc2::runtime::AnyObject,
    request_out: &mut *mut objc2::runtime::AnyObject,
) -> Result<String, String> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use std::ffi::c_void;

    {
        // ── 1. Wrap the PNG bytes in an NSData ───────────────────────
        // dataWithBytes:length: copies the buffer, so `png_bytes` can
        // safely go out of scope after this call. Autoreleased — the
        // pool in `recognize_impl` drains it.
        let nsdata_cls = AnyClass::get(c"NSData")
            .ok_or_else(|| "NSData class not available".to_string())?;
        let nsdata: *mut AnyObject = msg_send![
            nsdata_cls,
            dataWithBytes: png_bytes.as_ptr() as *const c_void,
            length: png_bytes.len()
        ];
        if nsdata.is_null() {
            return Err("NSData allocation failed".to_string());
        }

        // ── 2. Build the VNImageRequestHandler ───────────────────────
        let handler_cls = AnyClass::get(c"VNImageRequestHandler").ok_or_else(|| {
            "VNImageRequestHandler not available — Vision framework not linked?".to_string()
        })?;
        let handler: *mut AnyObject = msg_send![handler_cls, alloc];
        let handler: *mut AnyObject = msg_send![
            handler,
            initWithData: nsdata,
            options: std::ptr::null::<AnyObject>()
        ];
        if handler.is_null() {
            return Err("VNImageRequestHandler init failed".to_string());
        }
        *handler_out = handler;

        // ── 3. Build the VNRecognizeTextRequest ──────────────────────
        let request_cls = AnyClass::get(c"VNRecognizeTextRequest")
            .ok_or_else(|| "VNRecognizeTextRequest not available".to_string())?;
        let request: *mut AnyObject = msg_send![request_cls, alloc];
        // `init` (no completion handler) is the synchronous variant —
        // `perform` below will block until results are populated.
        let request: *mut AnyObject = msg_send![request, init];
        if request.is_null() {
            return Err("VNRecognizeTextRequest init failed".to_string());
        }
        *request_out = request;
        // Recognition level: 0 = Accurate (slower, much better), 1 = Fast.
        // For a one-shot user-triggered OCR the latency hit is fine.
        let level_raw: i64 = if level == Level::Fast { 1 } else { 0 };
        let _: () = msg_send![request, setRecognitionLevel: level_raw];
        let _: () = msg_send![request, setUsesLanguageCorrection: level == Level::Accurate];
        use_cpu(request);
        // Languages default to the user's preferred languages (all
        // installed Vision languages on macOS 13+). Don't override —
        // setting an empty array would make recognition fail entirely.

        // ── 4. Perform synchronously ─────────────────────────────────
        let array_cls = AnyClass::get(c"NSArray")
            .ok_or_else(|| "NSArray class not available".to_string())?;
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
            return Err(format!("OCR failed: {msg}"));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_survives_the_round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, REQ_ACCURATE, b"png bytes").unwrap();
        write_frame(&mut buf, REPLY_TEXT, "Größe €".as_bytes()).unwrap();
        let mut r = &buf[..];
        assert_eq!(read_frame(&mut r).unwrap(), Some((REQ_ACCURATE, b"png bytes".to_vec())));
        assert_eq!(read_frame(&mut r).unwrap(), Some((REPLY_TEXT, "Größe €".as_bytes().to_vec())));
        assert_eq!(read_frame(&mut r).unwrap(), None);
    }

    #[test]
    fn an_empty_body_is_a_valid_frame() {
        let mut buf = Vec::new();
        write_frame(&mut buf, REPLY_TEXT, b"").unwrap();
        assert_eq!(read_frame(&mut &buf[..]).unwrap(), Some((REPLY_TEXT, vec![])));
    }

    #[test]
    fn a_cut_off_frame_is_an_error_not_an_end() {
        let mut buf = Vec::new();
        write_frame(&mut buf, REQ_ACCURATE, b"abcdef").unwrap();
        buf.truncate(buf.len() - 2);
        assert!(read_frame(&mut &buf[..]).is_err());
    }

    #[test]
    fn an_oversized_length_is_refused_before_allocating() {
        let mut buf = vec![REQ_ACCURATE];
        buf.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_frame(&mut &buf[..]).is_err());
    }

    #[test]
    fn bad_requests_get_an_error_reply_and_the_loop_goes_on() {
        let mut input = Vec::new();
        write_frame(&mut input, b'X', b"x").unwrap();
        write_frame(&mut input, REQ_ACCURATE, b"").unwrap();
        let mut out = Vec::new();
        serve(&mut &input[..], &mut out);
        let mut r = &out[..];
        let (t1, b1) = read_frame(&mut r).unwrap().unwrap();
        let (t2, b2) = read_frame(&mut r).unwrap().unwrap();
        assert_eq!((t1, t2), (REPLY_ERROR, REPLY_ERROR));
        assert!(String::from_utf8(b1).unwrap().contains("unknown request"));
        assert_eq!(b2, b"empty image data");
        assert_eq!(read_frame(&mut r).unwrap(), None);
    }
}
