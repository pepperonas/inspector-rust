//! The live camera picture in the recording HUD: a small, separate capture session whose
//! frames the HUD polls as JPEG. It never runs during a take — `session::start` stops it
//! before the recording opens the camera — and it stops by itself when nobody asks for
//! frames any more, so the camera light can never stay on behind a closed HUD.

use parking_lot::Mutex;

/// Frames are scaled to this box (the camera's own shape is kept by the HUD's `object-fit`).
pub const PREVIEW_SIZE: (u32, u32) = (640, 360);
/// At most one encoded frame per this many ms (~12 fps).
pub const MIN_GAP_MS: i64 = 80;
/// Without a poll for this long the preview stops.
pub const IDLE_STOP_MS: i64 = 3_000;
pub const JPEG_QUALITY: u8 = 70;

/// Whether a frame arriving at `now_ms` is encoded (`last_ms` = the previous encoded one).
pub fn frame_due(last_ms: i64, now_ms: i64) -> bool {
    now_ms - last_ms >= MIN_GAP_MS || now_ms < last_ms
}

/// Whether a preview nobody polled since `last_poll_ms` is stopped.
pub fn idle(last_poll_ms: i64, now_ms: i64) -> bool {
    now_ms - last_poll_ms > IDLE_STOP_MS
}

/// BGRA rows (`row` bytes each, possibly padded) → tight RGB. `None` when the buffer is too
/// small for the stated size.
pub fn bgra_to_rgb(data: &[u8], row: usize, w: usize, h: usize) -> Option<Vec<u8>> {
    if w == 0 || h == 0 || row < w * 4 || data.len() < row * (h - 1) + w * 4 {
        return None;
    }
    let mut out = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        let line = &data[y * row..y * row + w * 4];
        for px in line.as_chunks::<4>().0 {
            out.extend_from_slice(&[px[2], px[1], px[0]]);
        }
    }
    Some(out)
}

pub fn encode_jpeg(rgb: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    // The encoder asserts on a buffer that does not match the size.
    if w == 0 || h == 0 || rgb.len() != w as usize * h as usize * 3 {
        return None;
    }
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY);
    enc.encode(rgb, w, h, image::ExtendedColorType::Rgb8).ok()?;
    Some(out)
}

/// The newest encoded frame and its running number.
#[derive(Default)]
struct Latest {
    gen: u64,
    jpeg: Vec<u8>,
}

static LATEST: Mutex<Latest> = Mutex::new(Latest { gen: 0, jpeg: Vec::new() });

/// The poll answer: 8 bytes generation (LE) + the JPEG, or empty when nothing newer than
/// `since` exists.
pub fn frame_bytes(since: u64) -> Vec<u8> {
    let g = LATEST.lock();
    if g.gen <= since || g.jpeg.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(8 + g.jpeg.len());
    out.extend_from_slice(&g.gen.to_le_bytes());
    out.extend_from_slice(&g.jpeg);
    out
}

fn publish(jpeg: Vec<u8>) {
    let mut g = LATEST.lock();
    g.gen += 1;
    g.jpeg = jpeg;
}

fn clear() {
    LATEST.lock().jpeg = Vec::new();
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{bgra_to_rgb, clear, encode_jpeg, frame_due, idle, publish, PREVIEW_SIZE};
    use crate::record::camera::{authorization, CameraAuth};
    use crate::record::capture_macos::{num, Sendable};
    use dispatch2::{DispatchQueue, DispatchRetained};
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{define_class, msg_send, AllocAnyThread};
    use objc2_av_foundation::{
        AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceInput, AVCaptureOutput, AVCaptureSession,
        AVCaptureSessionPreset1280x720, AVCaptureVideoDataOutput, AVCaptureVideoDataOutputSampleBufferDelegate,
    };
    use objc2_core_foundation::CFString;
    use objc2_core_media::CMSampleBuffer;
    use objc2_core_video::{
        kCVPixelBufferHeightKey, kCVPixelBufferPixelFormatTypeKey, kCVPixelBufferWidthKey, CVPixelBufferGetBaseAddress,
        CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
        CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    };
    use objc2_foundation::{NSDictionary, NSString};
    use parking_lot::Mutex;
    use std::sync::atomic::{AtomicI64, Ordering};

    /// `kCVPixelFormatType_32BGRA`.
    const PIXEL_BGRA: u32 = u32::from_be_bytes(*b"BGRA");

    static LAST_ENCODED_MS: AtomicI64 = AtomicI64::new(0);
    static LAST_POLL_MS: AtomicI64 = AtomicI64::new(0);

    fn now_ms() -> i64 {
        crate::record::host_now_ns() / 1_000_000
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "IRRecordCameraPreview"]
        struct PreviewOutput;

        unsafe impl NSObjectProtocol for PreviewOutput {}

        unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for PreviewOutput {
            #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
            fn did_output(&self, _output: &AVCaptureOutput, sample: &CMSampleBuffer, _conn: &AVCaptureConnection) {
                let now = now_ms();
                if !frame_due(LAST_ENCODED_MS.load(Ordering::Relaxed), now) {
                    return;
                }
                LAST_ENCODED_MS.store(now, Ordering::Relaxed);
                // Nothing here may unwind through the dispatch queue.
                let _ = objc2::exception::catch(std::panic::AssertUnwindSafe(|| on_sample(sample)));
            }
        }
    );

    fn on_sample(sample: &CMSampleBuffer) {
        // SAFETY: a live sample buffer from the capture output; its image buffer is locked
        // read-only for the copy and unlocked before returning.
        let rgb = unsafe {
            let Some(buf) = sample.image_buffer() else { return };
            if CVPixelBufferLockBaseAddress(&buf, CVPixelBufferLockFlags::ReadOnly) != 0 {
                return;
            }
            let (w, h, row) = (CVPixelBufferGetWidth(&buf), CVPixelBufferGetHeight(&buf), CVPixelBufferGetBytesPerRow(&buf));
            let base = CVPixelBufferGetBaseAddress(&buf) as *const u8;
            let out = if base.is_null() || w == 0 || h == 0 || row < w * 4 {
                None
            } else {
                bgra_to_rgb(std::slice::from_raw_parts(base, row * (h - 1) + w * 4), row, w, h).map(|rgb| (rgb, w, h))
            };
            CVPixelBufferUnlockBaseAddress(&buf, CVPixelBufferLockFlags::ReadOnly);
            out
        };
        if let Some((rgb, w, h)) = rgb {
            if let Some(jpeg) = encode_jpeg(&rgb, w as u32, h as u32) {
                publish(jpeg);
            }
        }
    }

    fn queue() -> &'static DispatchQueue {
        static Q: std::sync::OnceLock<Sendable<DispatchRetained<DispatchQueue>>> = std::sync::OnceLock::new();
        &Q.get_or_init(|| Sendable(DispatchQueue::new("io.celox.inspector-rust.record-camera-preview", None))).0
    }

    fn cv_key(k: &'static CFString) -> &'static NSString {
        // SAFETY: CFStringRef and NSString* are toll-free bridged; the constant lives for the
        // process lifetime.
        unsafe { &*(k as *const CFString as *const NSString) }
    }

    struct Running {
        device: String,
        session: Retained<AVCaptureSession>,
        output: Retained<AVCaptureVideoDataOutput>,
        _delegate: Retained<PreviewOutput>,
    }

    impl Drop for Running {
        fn drop(&mut self) {
            // SAFETY: stopping is idempotent; clearing the delegate before release means no
            // late callback reaches a freed object.
            unsafe {
                self.session.stopRunning();
                self.output.setSampleBufferDelegate_queue(None, None);
            }
        }
    }

    static CURRENT: Mutex<Option<Sendable<Running>>> = Mutex::new(None);

    /// Starts (or keeps) the preview of camera `device_id` (an `AVCaptureDevice.uniqueID`).
    /// Needs granted camera permission — it never asks.
    pub fn start(device_id: &str) -> Result<(), String> {
        if authorization() != CameraAuth::Authorized {
            return Err("record.camera.not_authorized".into());
        }
        let mut cur = CURRENT.lock();
        LAST_POLL_MS.store(now_ms(), Ordering::Relaxed);
        if cur.as_ref().is_some_and(|r| r.0.device == device_id) {
            return Ok(());
        }
        cur.take();
        clear();
        let res = objc2::exception::catch(std::panic::AssertUnwindSafe(|| autoreleasepool(|_| open(device_id))));
        let running = match res {
            Ok(r) => r?,
            Err(e) => return Err(e.map(|e| format!("{e:?}")).unwrap_or_else(|| "exception".into())),
        };
        *cur = Some(Sendable(running));
        drop(cur);
        spawn_idle_watch();
        Ok(())
    }

    fn open(device_id: &str) -> Result<Running, String> {
        // SAFETY: AVFoundation objects created and configured on this thread before the
        // session runs; input/output are checked with canAdd*, configuration is bracketed.
        unsafe {
            let dev = AVCaptureDevice::deviceWithUniqueID(&NSString::from_str(device_id))
                .ok_or_else(|| crate::i18n::t("rec5.camera_no_device"))?;
            let session = AVCaptureSession::new();
            let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&dev).map_err(|e| e.localizedDescription().to_string())?;
            let output = AVCaptureVideoDataOutput::new();
            output.setAlwaysDiscardsLateVideoFrames(true);
            let vs = NSDictionary::<NSString, AnyObject>::from_retained_objects(
                &[cv_key(kCVPixelBufferPixelFormatTypeKey), cv_key(kCVPixelBufferWidthKey), cv_key(kCVPixelBufferHeightKey)],
                &[num(PIXEL_BGRA as i64), num(PREVIEW_SIZE.0 as i64), num(PREVIEW_SIZE.1 as i64)],
            );
            output.setVideoSettings(Some(&vs));
            session.beginConfiguration();
            if session.canSetSessionPreset(AVCaptureSessionPreset1280x720) {
                session.setSessionPreset(AVCaptureSessionPreset1280x720);
            }
            if !session.canAddInput(&input) || !session.canAddOutput(&output) {
                session.commitConfiguration();
                return Err("session refused the camera".into());
            }
            session.addInput(&input);
            session.addOutput(&output);
            session.commitConfiguration();
            let delegate: Retained<PreviewOutput> = msg_send![PreviewOutput::alloc(), init];
            let proto: &ProtocolObject<dyn AVCaptureVideoDataOutputSampleBufferDelegate> = ProtocolObject::from_ref(&*delegate);
            output.setSampleBufferDelegate_queue(Some(proto), Some(queue()));
            session.startRunning();
            if !session.isRunning() {
                return Err("the camera did not start (in use by another app?)".into());
            }
            tracing::info!("record camera preview: {} started", dev.localizedName());
            Ok(Running { device: device_id.to_string(), session, output, _delegate: delegate })
        }
    }

    /// Stops the preview and frees the camera. Safe to call when none runs.
    pub fn stop() {
        let taken = CURRENT.lock().take();
        if taken.is_some() {
            drop(taken);
            clear();
            tracing::info!("record camera preview: stopped");
        }
    }

    fn running() -> bool {
        CURRENT.lock().is_some()
    }

    /// The HUD's poll: marks the preview as wanted and returns the newest frame.
    pub fn frame(since: u64) -> Vec<u8> {
        LAST_POLL_MS.store(now_ms(), Ordering::Relaxed);
        super::frame_bytes(since)
    }

    fn spawn_idle_watch() {
        static WATCHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if WATCHING.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = std::thread::Builder::new().name("ir-camera-preview-watch".into()).spawn(|| {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                if !running() {
                    break;
                }
                if idle(LAST_POLL_MS.load(Ordering::Relaxed), now_ms()) {
                    stop();
                    break;
                }
            }
            WATCHING.store(false, Ordering::SeqCst);
        });
    }
}

#[cfg(target_os = "macos")]
pub use mac::{frame, start, stop};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_thinned_to_about_twelve_a_second() {
        assert!(frame_due(0, 80));
        assert!(!frame_due(1000, 1079));
        assert!(frame_due(1000, 1080));
        // A clock that jumped back must not block frames forever.
        assert!(frame_due(5000, 10));
        let mut last = 0;
        let mut n = 0;
        for t in (0..1000).step_by(33) {
            if frame_due(last, t) {
                last = t;
                n += 1;
            }
        }
        assert!((9..=13).contains(&n), "{n} frames in a second");
    }

    #[test]
    fn an_unpolled_preview_stops() {
        assert!(!idle(1000, 1000 + IDLE_STOP_MS));
        assert!(idle(1000, 1001 + IDLE_STOP_MS));
        assert!(!idle(1000, 900));
    }

    #[test]
    fn bgra_rows_become_tight_rgb() {
        // 2×2 with 4 bytes of row padding.
        let row = 12;
        let data = [
            1, 2, 3, 255, 4, 5, 6, 255, 9, 9, 9, 9, //
            7, 8, 9, 255, 10, 11, 12, 255,
        ];
        assert_eq!(bgra_to_rgb(&data, row, 2, 2).unwrap(), vec![3, 2, 1, 6, 5, 4, 9, 8, 7, 12, 11, 10]);
        // Too short, a row narrower than the picture, or an empty picture: nothing, never a panic.
        assert_eq!(bgra_to_rgb(&data[..10], row, 2, 2), None);
        assert_eq!(bgra_to_rgb(&data, 4, 2, 2), None);
        assert_eq!(bgra_to_rgb(&data, row, 0, 2), None);
    }

    #[test]
    fn a_frame_encodes_to_a_jpeg() {
        let rgb = vec![128u8; 16 * 8 * 3];
        let jpeg = encode_jpeg(&rgb, 16, 8).unwrap();
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        assert_eq!(encode_jpeg(&rgb, 16, 9), None, "a size that does not match the data");
    }

    #[test]
    fn the_poll_answers_only_with_something_newer() {
        clear();
        let before = LATEST.lock().gen;
        assert!(frame_bytes(before).is_empty());
        publish(vec![0xFF, 0xD8, 1]);
        let got = frame_bytes(before);
        assert_eq!(u64::from_le_bytes(got[..8].try_into().unwrap()), before + 1);
        assert_eq!(&got[8..], &[0xFF, 0xD8, 1]);
        assert!(frame_bytes(before + 1).is_empty(), "the same frame is not sent twice");
        clear();
        assert!(frame_bytes(0).is_empty(), "a stopped preview has no picture");
    }
}
