//! `record` stage 5 — the webcam: quality, devices, permission and its capture into
//! `camera.mp4` on the shared pause clock.
//!
//! The pure part (quality ladder, device choice, timestamps, format choice, loss rule, the
//! types the settings carry) compiles on every target, because `RecordConfig`/`StartOptions`
//! hold it. The AVFoundation edge (permission, devices, [`CameraCapture`]) lives in the
//! macOS-only `mod mac` below.
//!
//! Timing: a camera frame's capture time goes through the same rule as a screen frame —
//! [`PauseShared::offset_at`] drops it inside a pause and otherwise shifts it back by the
//! pauses that ended before it — and is then expressed relative to the screen's first frame
//! (`origin_ns`). So the camera file starts where the screen file starts (offset 0) and the
//! pauses are cut out of both identically. Frames before the origin are dropped; the first
//! admitted one is written at 0 (held), so the file never starts later than the screen.
//! The writer session ends at the screen's end, so the file covers the screen's length even
//! when the camera dropped out (the export truncates a clip to its camera's length).

use super::core::PauseShared;
use super::project::CameraMeta;
use serde::{Deserialize, Serialize};

/// Frame rate the camera is asked for (OpenScreen: `WEBCAM_TARGET_FRAME_RATE`).
pub const CAMERA_FPS: u32 = 30;
/// After the first frame: no frame for this long = the camera is gone.
pub const CAMERA_STALL_NS: i64 = 2_000_000_000;
/// Before the first frame: a camera may take this long after the screen's first frame to
/// deliver (Continuity Camera and some UVC cameras need several seconds to wake). Past it
/// without a single frame the camera counts as never started.
pub const CAMERA_FIRST_FRAME_GRACE_NS: i64 = 8_000_000_000;
/// A sample time further than this before its arrival is not on the host clock.
const HOST_CLOCK_SLACK_BEHIND_NS: i64 = 1_000_000_000;
/// …or more than this after its arrival.
const HOST_CLOCK_SLACK_AHEAD_NS: i64 = 50_000_000;

/// Programme time (ns from the screen's first frame) of a camera frame captured at host
/// time `sample_ns`, or `None` = drop it: captured inside a pause, or before the origin.
/// The same rule as the screen frames ([`PauseShared::offset_at`]).
pub fn camera_pts(sample_ns: u64, origin_ns: u64, clock: &PauseShared) -> Option<u64> {
    let off = clock.offset_at(sample_ns as i64)?;
    let t = sample_ns as i64 - off;
    let origin = origin_ns as i64;
    (t >= origin).then(|| (t - origin) as u64)
}

/// The host time of a frame. AVCapture stamps frames with the session's clock, which on
/// macOS is the host clock; a stamp that does not fit the arrival time (another clock) is
/// replaced by the arrival time, so a foreign clock can never misplace the camera.
pub fn sample_host_ns(pts_ns: Option<i64>, arrival_ns: i64) -> i64 {
    match pts_ns {
        Some(p) if p >= arrival_ns - HOST_CLOCK_SLACK_BEHIND_NS && p <= arrival_ns + HOST_CLOCK_SLACK_AHEAD_NS => p,
        _ => arrival_ns,
    }
}

/// One capture format of a device: its size and the highest frame rate it offers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormatInfo {
    pub width: u32,
    pub height: u32,
    pub max_fps: f64,
}

/// The format nearest to `ideal` ("ideal, never exact"): formats that reach [`CAMERA_FPS`]
/// first, then ones with the ideal's aspect (a 16:9 wish must not get the FaceTime camera's
/// 1552×1552 just because its pixel count is closer), then the smallest difference in pixel
/// count; a tie goes to the larger format. Portrait formats are skipped when a landscape one
/// exists.
pub fn pick_format(formats: &[FormatInfo], ideal: (u32, u32)) -> Option<usize> {
    let landscape = formats.iter().any(|f| f.width >= f.height);
    let area = |w: u32, h: u32| w as i64 * h as i64;
    let want = area(ideal.0, ideal.1);
    let ratio = |w: u32, h: u32| (w as f64 / h.max(1) as f64).ln();
    let want_ratio = ratio(ideal.0, ideal.1);
    formats
        .iter()
        .enumerate()
        .filter(|(_, f)| f.width > 0 && f.height > 0 && (!landscape || f.width >= f.height))
        .min_by_key(|(_, f)| {
            let slow = f.max_fps + 0.5 < CAMERA_FPS as f64;
            let other_aspect = (ratio(f.width, f.height) - want_ratio).abs() > 0.05;
            let a = area(f.width, f.height);
            (slow, other_aspect, (a - want).abs(), -a)
        })
        .map(|(i, _)| i)
}

/// The frame rate asked of a format that offers at most `max_fps`.
pub fn target_fps(max_fps: f64) -> u32 {
    if !max_fps.is_finite() || max_fps < 1.0 {
        return CAMERA_FPS;
    }
    (max_fps.round() as u32).clamp(1, CAMERA_FPS)
}

/// The file's frame size: the delivered size, scaled down (aspect kept) to fit the quality's
/// ideal size, even, at least 2.
pub fn camera_output_size(delivered: (u32, u32), ideal: (u32, u32)) -> (u32, u32) {
    super::core::capture_output_size(delivered.0 as f64, delivered.1 as f64, 1.0, ideal.0, ideal.1)
}

/// How a running camera is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraHealth {
    Ok,
    /// It delivered and then dropped out (unplugged, stopped, frames stopped).
    Lost,
    /// It never delivered a single frame (in the first-frame grace, or gone before).
    NeverStarted,
}

/// The health rule. `first_arrival_ns`/`last_arrival_ns` = host ns of the first/last
/// delivered frame (frames keep arriving during a pause — they are only dropped — so a pause
/// never counts); `origin_at_ns` = host ns when the screen's first frame was known. The
/// 2-s stall rule only applies once a frame came; before that the camera gets
/// [`CAMERA_FIRST_FRAME_GRACE_NS`] from the origin.
pub fn camera_health(
    connected: bool,
    running: bool,
    first_arrival_ns: Option<i64>,
    last_arrival_ns: i64,
    origin_at_ns: i64,
    now_ns: i64,
) -> CameraHealth {
    match first_arrival_ns {
        None if !connected || !running || now_ns - origin_at_ns > CAMERA_FIRST_FRAME_GRACE_NS => {
            CameraHealth::NeverStarted
        }
        None => CameraHealth::Ok,
        Some(_) if !connected || !running || now_ns - last_arrival_ns > CAMERA_STALL_NS => CameraHealth::Lost,
        Some(_) => CameraHealth::Ok,
    }
}

/// After a failed finish (writer error, finishWriting timeout): keep the file only when it
/// can be read back with a length (`readable_secs` = AVURLAsset duration of the file —
/// fragmented MP4s are readable up to the last fragment).
pub fn keep_after_failed_finish(readable_secs: Option<f64>) -> bool {
    readable_secs.is_some_and(|d| d.is_finite() && d > 0.0)
}

/// What the camera wrote, for `project.json`. No frame written = no camera at all.
pub fn camera_meta(
    written: Option<(u32, u32)>,
    fps: u32,
    device_name: &str,
    lost_at_ms: Option<u64>,
) -> Option<CameraMeta> {
    let (width, height) = written?;
    Some(CameraMeta { width, height, fps, offset_ms: 0, device_name: device_name.to_string(), ended_early_ms: lost_at_ms })
}

/// The end of a camera recording.
#[derive(Debug, Clone, PartialEq)]
pub struct CameraOutcome {
    /// `None` = no usable camera file (nothing written; the file was removed).
    pub meta: Option<CameraMeta>,
    /// The camera dropped out mid-take.
    pub lost: bool,
}

/// The camera quality the user asks for. "Ideal", never forced: the device picks the
/// nearest format it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CameraQuality {
    #[default]
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "1440p")]
    P1440,
    #[serde(rename = "2160p")]
    P2160,
}

impl CameraQuality {
    /// The frame size this quality asks the device for.
    pub fn ideal_size(self) -> (u32, u32) {
        match self {
            Self::P1080 => (1920, 1080),
            Self::P1440 => (2560, 1440),
            Self::P2160 => (3840, 2160),
        }
    }

    /// Stored settings value → quality; anything unknown is the default.
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "1440p" => Self::P1440,
            "2160p" => Self::P2160,
            _ => Self::P1080,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::P1080 => "1080p",
            Self::P1440 => "1440p",
            Self::P2160 => "2160p",
        }
    }
}

/// H.264 bitrate for a camera frame of `w`×`h`, measured on the shorter side (OpenScreen's
/// `webcamBitrateFor`): ≥ 2160 → 40 Mb/s, ≥ 1440 → 24, ≥ 1080 → 16, ≥ 720 → 8, else 4.
pub fn camera_bitrate(w: u32, h: u32) -> u32 {
    match w.min(h) {
        s if s >= 2160 => 40_000_000,
        s if s >= 1440 => 24_000_000,
        s if s >= 1080 => 16_000_000,
        s if s >= 720 => 8_000_000,
        _ => 4_000_000,
    }
}

/// A video capture device as the HUD and settings show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraDevice {
    /// `AVCaptureDevice.uniqueID` — stable across launches, stored in the settings.
    pub id: String,
    pub name: String,
    /// The Mac's own camera (`AVCaptureDeviceTypeBuiltInWideAngleCamera`).
    pub builtin: bool,
}

/// Camera permission (TCC) as AVFoundation reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraAuth {
    Authorized,
    Denied,
    Restricted,
    NotDetermined,
}

/// The device a recording uses: the wanted id, else the built-in camera, else the first.
pub fn pick_device<'a>(devices: &'a [CameraDevice], wanted: &str) -> Option<&'a CameraDevice> {
    let wanted = wanted.trim();
    (!wanted.is_empty())
        .then(|| devices.iter().find(|d| d.id == wanted))
        .flatten()
        .or_else(|| devices.iter().find(|d| d.builtin))
        .or_else(|| devices.first())
}

#[cfg(target_os = "macos")]
pub use mac::{authorization, devices, request_access};

#[cfg(target_os = "macos")]
mod mac {
    use super::{CameraAuth, CameraDevice};
    use block2::RcBlock;
    use objc2::rc::autoreleasepool;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{
        AVAuthorizationStatus, AVCaptureDevice, AVCaptureDeviceTypeBuiltInWideAngleCamera, AVMediaTypeVideo,
    };
    use std::sync::mpsc;
    use std::time::Duration;

    /// How long `request_access` waits for the user to answer the system dialog.
    const REQUEST_WAIT: Duration = Duration::from_secs(300);

    fn map(status: AVAuthorizationStatus) -> CameraAuth {
        match status {
            AVAuthorizationStatus::Authorized => CameraAuth::Authorized,
            AVAuthorizationStatus::Denied => CameraAuth::Denied,
            AVAuthorizationStatus::Restricted => CameraAuth::Restricted,
            _ => CameraAuth::NotDetermined,
        }
    }

    /// The camera permission. Reading it never shows a dialog.
    pub fn authorization() -> CameraAuth {
        // SAFETY: `AVMediaTypeVideo` is a framework-provided static NSString, valid for the
        // process lifetime; with a video/audio media type the class method does not throw.
        let media = unsafe { AVMediaTypeVideo }.expect("AVMediaTypeVideo is always exported");
        // SAFETY: plain class-method read with a valid media type (see above); the pool
        // drains anything AVFoundation autoreleases on this non-main thread.
        autoreleasepool(|_| map(unsafe { AVCaptureDevice::authorizationStatusForMediaType(media) }))
    }

    /// Asks for camera access when it is not decided yet and waits for the answer; a decided
    /// permission is returned as it is (macOS only asks once — afterwards only Settings help).
    /// Needs `NSCameraUsageDescription` in the bundle, else TCC ends the process.
    pub fn request_access() -> CameraAuth {
        let now = authorization();
        if now != CameraAuth::NotDetermined {
            return now;
        }
        let (tx, rx) = mpsc::channel::<bool>();
        let block = RcBlock::new(move |granted: Bool| {
            let _ = tx.send(granted.as_bool());
        });
        // SAFETY: see `authorization` for the media type.
        let media = unsafe { AVMediaTypeVideo }.expect("AVMediaTypeVideo is always exported");
        // SAFETY: valid media type; AVFoundation copies the block and calls it once on an
        // arbitrary queue — the closure only sends on a channel (Send + Sync), no UI work.
        autoreleasepool(|_| unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(media, &block) });
        match rx.recv_timeout(REQUEST_WAIT) {
            Ok(true) => CameraAuth::Authorized,
            // A "no" or a dialog left open: report what TCC says now.
            _ => authorization(),
        }
    }

    /// Every video capture device (built-in and external). Listing needs no permission.
    pub fn devices() -> Vec<CameraDevice> {
        // SAFETY: framework statics, valid for the process lifetime (see `authorization`).
        let media = unsafe { AVMediaTypeVideo }.expect("AVMediaTypeVideo is always exported");
        // SAFETY: as above; the type constant exists since macOS 10.15 (our minimum).
        let builtin_type = unsafe { AVCaptureDeviceTypeBuiltInWideAngleCamera };
        // `devicesWithMediaType:` instead of a discovery session: the external-camera type
        // constant is macOS 14+ and would be a hard link against older systems.
        autoreleasepool(|_| {
            #[allow(deprecated)]
            // SAFETY: class method with a valid media type; returns an NSArray we retain. The
            // pool drains the autoreleased objects of this (non-main) thread.
            let list = unsafe { AVCaptureDevice::devicesWithMediaType(media) };
            list.iter()
                .map(|d| {
                    // SAFETY: `d` is a live AVCaptureDevice from the array; these getters have
                    // no preconditions and return retained NSStrings.
                    let (id, name, kind) = unsafe { (d.uniqueID(), d.localizedName(), d.deviceType()) };
                    CameraDevice { id: id.to_string(), name: name.to_string(), builtin: *kind == *builtin_type }
                })
                .collect()
        })
    }
}

/// The macOS camera recorder: `AVCaptureSession` → `AVCaptureVideoDataOutput` (420v, scaled
/// to the file size, not mirrored, late frames discarded) → own `AVAssetWriter` (H.264,
/// [`camera_bitrate`]). Timestamps per [`camera_pts`] + `VideoTimestampGate`.
#[cfg(target_os = "macos")]
mod capture {
    use super::{
        camera_bitrate, camera_health, camera_meta, camera_output_size, camera_pts, keep_after_failed_finish,
        pick_format, sample_host_ns, target_fps, CameraDevice, CameraHealth, CameraOutcome, CameraQuality, FormatInfo,
    };
    use crate::record::capture_macos::{cmtime_ns, key, num, video_settings, writer_error, Sendable};
    use crate::record::core::{GateVerdict, VideoTimestampGate};
    use crate::record::{host_now_ns, PauseShared};
    use block2::RcBlock;
    use dispatch2::{DispatchQueue, DispatchRetained};
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass};
    use objc2_av_foundation::{
        AVAssetWriter, AVAssetWriterInput, AVAssetWriterStatus, AVCaptureConnection, AVCaptureDevice,
        AVCaptureDeviceInput, AVCaptureOutput, AVCaptureSession, AVCaptureVideoDataOutput,
        AVCaptureVideoDataOutputSampleBufferDelegate, AVFileTypeMPEG4, AVMediaTypeVideo,
    };
    use objc2_core_foundation::{CFRetained, CFString};
    use objc2_core_media::{
        CMFormatDescription, CMSampleBuffer, CMSampleTimingInfo, CMTime, CMTimeFlags, CMVideoFormatDescriptionGetDimensions,
    };
    use objc2_core_video::{kCVPixelBufferHeightKey, kCVPixelBufferPixelFormatTypeKey, kCVPixelBufferWidthKey};
    use objc2_foundation::{NSDictionary, NSString, NSURL};
    use parking_lot::Mutex;
    use std::path::{Path, PathBuf};
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    const NS_PER_S: i64 = 1_000_000_000;
    /// `kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange` ('420v'), what the H.264 encoder takes.
    const PIXEL_420V: u32 = u32::from_be_bytes(*b"420v");
    const NO_ORIGIN: i64 = i64::MIN;

    struct CamState {
        writer: Retained<AVAssetWriter>,
        input: Retained<AVAssetWriterInput>,
        started: bool,
        stopping: bool,
        gate: VideoTimestampGate,
        written: u64,
        last_pts: Option<u64>,
        not_ready: u32,
        error: Option<String>,
    }

    struct CamInner {
        clock: Arc<PauseShared>,
        /// Timeline ns of the screen's first frame; [`NO_ORIGIN`] until the session knows it.
        origin: AtomicI64,
        /// Host ns of the last delivered frame (also during pauses).
        last_arrival: AtomicI64,
        /// Host ns of the first delivered frame; [`NO_ORIGIN`] until one came.
        first_arrival: AtomicI64,
        /// Host ns when `set_origin` was called; [`NO_ORIGIN`] before.
        origin_at: AtomicI64,
        /// The health verdict once it stopped being Ok (0 = Ok, 1 = lost, 2 = never started).
        verdict: std::sync::atomic::AtomicU8,
        /// Once lost, always lost: no frame is written after the camera dropped out.
        lost: AtomicBool,
        st: Mutex<Sendable<CamState>>,
    }

    impl CamInner {
        fn on_sample(&self, sample: &CMSampleBuffer) {
            let arrival = host_now_ns();
            self.last_arrival.store(arrival, Ordering::Relaxed);
            let _ = self.first_arrival.compare_exchange(NO_ORIGIN, arrival, Ordering::Relaxed, Ordering::Relaxed);
            let origin = self.origin.load(Ordering::Acquire);
            if origin == NO_ORIGIN || self.lost.load(Ordering::Relaxed) {
                return;
            }
            // SAFETY: `sample` is a live CMSampleBuffer handed to the delegate for this call.
            if !unsafe { sample.data_is_ready() } {
                return;
            }
            // SAFETY: as above; plain timing read.
            let raw = unsafe { sample.presentation_time_stamp() };
            let host = sample_host_ns(cmtime_ns(raw.value, raw.timescale, raw.flags.0), arrival);
            if host < 0 {
                return;
            }
            let Some(mut pts) = camera_pts(host as u64, origin as u64, &self.clock) else { return };
            let mut guard = self.st.lock();
            let st = &mut guard.0;
            if st.stopping || st.error.is_some() {
                return;
            }
            if !st.started {
                // SAFETY: the writer was set up with one video input and not started yet.
                if !unsafe { st.writer.startWriting() } {
                    st.error = Some(format!("start: {}", writer_error(&st.writer)));
                    return;
                }
                // SAFETY: called once, right after startWriting, before any append.
                unsafe { st.writer.startSessionAtSourceTime(CMTime::new(0, NS_PER_S as i32)) };
                st.started = true;
                // The first frame stands at 0 (held) so the camera never starts later than
                // the screen; the next ones keep their own time.
                pts = 0;
            }
            if st.gate.check(Some(pts as i64)) != GateVerdict::Admit {
                return;
            }
            let Some(buf) = with_pts(sample, pts as i64) else { return };
            // SAFETY: status read on our own writer under the state lock.
            if unsafe { st.writer.status() } == AVAssetWriterStatus::Failed {
                st.error = Some(format!("writer status: {}", writer_error(&st.writer)));
                return;
            }
            // SAFETY: the input belongs to the started writer; `buf` is a valid, retimed copy.
            if unsafe { st.input.isReadyForMoreMediaData() } {
                if unsafe { st.input.appendSampleBuffer(&buf) } {
                    st.gate.record(pts as i64);
                    st.last_pts = Some(pts);
                    st.written += 1;
                } else {
                    st.error = Some(format!("append: {}", writer_error(&st.writer)));
                    tracing::warn!("record camera: {}", st.error.as_deref().unwrap_or_default());
                }
            } else {
                st.not_ready += 1;
            }
        }
    }

    /// A copy of `sample` presented at `pts_ns` (programme time). `None` = CoreMedia refused.
    fn with_pts(sample: &CMSampleBuffer, pts_ns: i64) -> Option<CFRetained<CMSampleBuffer>> {
        let invalid = CMTime { value: 0, timescale: 0, flags: CMTimeFlags(0), epoch: 0 };
        // SAFETY: CMTimeMake has no preconditions (a positive timescale).
        let pts = unsafe { CMTime::new(pts_ns, NS_PER_S as i32) };
        let timing = [CMSampleTimingInfo {
            duration: invalid,
            presentationTimeStamp: pts,
            decodeTimeStamp: invalid,
        }];
        let mut out: *mut CMSampleBuffer = std::ptr::null_mut();
        // SAFETY: `sample` is valid for the call; one timing entry for the one video frame;
        // `out` is a valid out-pointer that receives a +1 buffer on success.
        let st = unsafe {
            CMSampleBuffer::create_copy_with_new_timing(None, sample, 1, timing.as_ptr(), NonNull::from(&mut out))
        };
        if st != 0 {
            return None;
        }
        // SAFETY: on success `out` is a retained CMSampleBuffer we now own.
        NonNull::new(out).map(|p| unsafe { CFRetained::from_raw(p) })
    }

    pub struct CamIvars {
        inner: Arc<CamInner>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "IRRecordCameraOutput"]
        #[ivars = CamIvars]
        struct CameraOutput;

        unsafe impl NSObjectProtocol for CameraOutput {}

        unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CameraOutput {
            #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
            fn did_output(&self, _output: &AVCaptureOutput, sample: &CMSampleBuffer, _conn: &AVCaptureConnection) {
                let inner = &self.ivars().inner;
                // An Objective-C exception from AVAssetWriter must not unwind through the
                // dispatch queue (that aborts the app).
                let res = objc2::exception::catch(std::panic::AssertUnwindSafe(|| inner.on_sample(sample)));
                if let Err(e) = res {
                    let msg = e.map(|e| format!("{e:?}")).unwrap_or_else(|| "exception".into());
                    tracing::warn!("record camera: exception while writing a frame: {msg}");
                    let mut g = inner.st.lock();
                    if g.0.error.is_none() {
                        g.0.error = Some(msg);
                    }
                }
            }
        }
    );

    impl CameraOutput {
        fn create(inner: Arc<CamInner>) -> Retained<Self> {
            let this = Self::alloc().set_ivars(CamIvars { inner });
            // SAFETY: NSObject's designated initialiser on a freshly allocated instance.
            unsafe { msg_send![super(this), init] }
        }
    }

    fn queue() -> &'static DispatchQueue {
        static Q: std::sync::OnceLock<Sendable<DispatchRetained<DispatchQueue>>> = std::sync::OnceLock::new();
        &Q.get_or_init(|| Sendable(DispatchQueue::new("io.celox.inspector-rust.record-camera", None))).0
    }

    /// A CoreVideo key constant as an NSString (CFString and NSString are toll-free bridged).
    fn cv_key(k: &'static CFString) -> &'static NSString {
        // SAFETY: CFStringRef and NSString* are toll-free bridged; the constant lives for the
        // process lifetime.
        unsafe { &*(k as *const CFString as *const NSString) }
    }

    /// The device's formats as [`FormatInfo`], in `device.formats()` order.
    fn format_infos(device: &AVCaptureDevice) -> Vec<FormatInfo> {
        // SAFETY: plain property reads on a live device.
        let formats = unsafe { device.formats() };
        formats
            .iter()
            .map(|f| {
                // SAFETY: a live AVCaptureDeviceFormat; its description is a video format
                // description for a video device.
                let desc: Retained<CMFormatDescription> = unsafe { f.formatDescription() };
                let dims = unsafe { CMVideoFormatDescriptionGetDimensions(&desc) };
                // SAFETY: as above.
                let max_fps = unsafe { f.videoSupportedFrameRateRanges() }
                    .iter()
                    .map(|r| unsafe { r.maxFrameRate() })
                    .fold(0.0_f64, f64::max);
                FormatInfo { width: dims.width.max(0) as u32, height: dims.height.max(0) as u32, max_fps }
            })
            .collect()
    }

    /// Records a camera into an MP4 on the shared pause clock. See the module docs.
    pub struct CameraCapture {
        inner: Arc<CamInner>,
        session: Retained<AVCaptureSession>,
        device: Retained<AVCaptureDevice>,
        output: Retained<AVCaptureVideoDataOutput>,
        _delegate: Retained<CameraOutput>,
        path: PathBuf,
        name: String,
        fps: u32,
        size: (u32, u32),
        mirrored: bool,
    }

    // SAFETY: the AVFoundation objects are only touched from `is_lost`/`finish` with methods
    // AVFoundation allows from any thread (isRunning, isConnected, stopRunning); the writer
    // state is behind a mutex.
    unsafe impl Send for CameraCapture {}

    impl CameraCapture {
        /// Opens `device`, asks it for `quality` at 30 fps and starts it. Frames are dropped
        /// until [`Self::set_origin`]. Needs camera permission (the caller checks it).
        pub fn start(device: &CameraDevice, quality: CameraQuality, path: &Path, clock: Arc<PauseShared>) -> Result<CameraCapture, String> {
            let fail = |r: String| crate::i18n::tf("rec5.camera_start_failed", &[("reason", &r)]);
            let res = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
                autoreleasepool(|_| Self::start_inner(device, quality, path, clock))
            }));
            match res {
                Ok(r) => r.map_err(fail),
                Err(e) => Err(fail(e.map(|e| format!("{e:?}")).unwrap_or_else(|| "exception".into()))),
            }
        }

        fn start_inner(device: &CameraDevice, quality: CameraQuality, path: &Path, clock: Arc<PauseShared>) -> Result<CameraCapture, String> {
            // SAFETY: class method with a valid NSString; returns a retained device or None.
            let dev = unsafe { AVCaptureDevice::deviceWithUniqueID(&NSString::from_str(&device.id)) }
                .ok_or_else(|| crate::i18n::t("rec5.camera_no_device"))?;
            let infos = format_infos(&dev);
            let pick = pick_format(&infos, quality.ideal_size());
            // SAFETY: live device; plain read.
            let formats = unsafe { dev.formats() };
            let (delivered, fps) = match pick {
                Some(i) => ((infos[i].width, infos[i].height), target_fps(infos[i].max_fps)),
                None => (quality.ideal_size(), super::CAMERA_FPS),
            };
            let size = camera_output_size(delivered, quality.ideal_size());

            // SAFETY: AVFoundation objects created and configured on this thread before the
            // session runs; every call has its documented preconditions met (input/output
            // checked with canAdd*, configuration bracketed, device locked for format changes).
            unsafe {
                let session = AVCaptureSession::new();
                let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&dev)
                    .map_err(|e| e.localizedDescription().to_string())?;
                let output = AVCaptureVideoDataOutput::new();
                output.setAlwaysDiscardsLateVideoFrames(true);
                let vs = NSDictionary::<NSString, AnyObject>::from_retained_objects(
                    &[cv_key(kCVPixelBufferPixelFormatTypeKey), cv_key(kCVPixelBufferWidthKey), cv_key(kCVPixelBufferHeightKey)],
                    &[num(PIXEL_420V as i64), num(size.0 as i64), num(size.1 as i64)],
                );
                output.setVideoSettings(Some(&vs));
                session.beginConfiguration();
                if !session.canAddInput(&input) || !session.canAddOutput(&output) {
                    session.commitConfiguration();
                    return Err("session refused the camera".into());
                }
                session.addInput(&input);
                session.addOutput(&output);
                session.commitConfiguration();

                if let Some(i) = pick {
                    if dev.lockForConfiguration().is_ok() {
                        let fmt = formats.objectAtIndex(i);
                        dev.setActiveFormat(&fmt);
                        if infos[i].max_fps + 0.5 >= super::CAMERA_FPS as f64 {
                            // Take what comes when the format refuses 30 fps; the gate keeps
                            // the timestamps in order either way.
                            let d = CMTime::new(1, fps as i32);
                            let _ = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
                                dev.setActiveVideoMinFrameDuration(d);
                                dev.setActiveVideoMaxFrameDuration(d);
                            }));
                        }
                        dev.unlockForConfiguration();
                    }
                }

                let media = AVMediaTypeVideo.expect("AVMediaTypeVideo is always exported");
                let mut mirrored = false;
                if let Some(conn) = output.connectionWithMediaType(media) {
                    if conn.isVideoMirroringSupported() {
                        conn.setAutomaticallyAdjustsVideoMirroring(false);
                        conn.setVideoMirrored(false);
                    }
                    mirrored = conn.isVideoMirrored();
                }

                let _ = std::fs::remove_file(path);
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                let url = NSURL::from_file_path(path).ok_or_else(|| "bad path".to_string())?;
                let writer = AVAssetWriter::initWithURL_fileType_error(AVAssetWriter::alloc(), &url, key(AVFileTypeMPEG4)?)
                    .map_err(|e| e.localizedDescription().to_string())?;
                writer.setMovieFragmentInterval(CMTime::new(1, 1));
                let settings = video_settings(size.0, size.1, fps, camera_bitrate(size.0, size.1))?;
                let winput =
                    AVAssetWriterInput::initWithMediaType_outputSettings(AVAssetWriterInput::alloc(), media, Some(&settings));
                winput.setExpectsMediaDataInRealTime(true);
                if !writer.canAddInput(&winput) {
                    return Err("H.264 input".into());
                }
                writer.addInput(&winput);

                let inner = Arc::new(CamInner {
                    clock,
                    origin: AtomicI64::new(NO_ORIGIN),
                    last_arrival: AtomicI64::new(host_now_ns()),
                    first_arrival: AtomicI64::new(NO_ORIGIN),
                    origin_at: AtomicI64::new(NO_ORIGIN),
                    verdict: std::sync::atomic::AtomicU8::new(0),
                    lost: AtomicBool::new(false),
                    st: Mutex::new(Sendable(CamState {
                        writer,
                        input: winput,
                        started: false,
                        stopping: false,
                        gate: VideoTimestampGate::new(),
                        written: 0,
                        last_pts: None,
                        not_ready: 0,
                        error: None,
                    })),
                });
                let delegate = CameraOutput::create(inner.clone());
                let proto: &ProtocolObject<dyn AVCaptureVideoDataOutputSampleBufferDelegate> = ProtocolObject::from_ref(&*delegate);
                output.setSampleBufferDelegate_queue(Some(proto), Some(queue()));
                session.startRunning();
                if !session.isRunning() {
                    let _ = std::fs::remove_file(path);
                    return Err("the camera did not start (in use by another app?)".into());
                }
                // SAFETY: plain read on a live device.
                let name = dev.localizedName().to_string();
                tracing::info!(
                    "record camera: {name} started, format {delivered:?} → {size:?} @ {fps} fps, mirrored {mirrored}"
                );
                Ok(CameraCapture {
                    inner,
                    session,
                    device: dev,
                    output,
                    _delegate: delegate,
                    path: path.to_path_buf(),
                    name,
                    fps,
                    size,
                    mirrored,
                })
            }
        }

        /// Timeline ns of the screen's first frame: from here on frames are written.
        pub fn set_origin(&self, origin_ns: u64) {
            self.inner.origin_at.store(host_now_ns(), Ordering::Relaxed);
            self.inner.origin.store(origin_ns as i64, Ordering::Release);
        }

        /// The camera has delivered a frame (written or not).
        pub fn has_frame(&self) -> bool {
            self.inner.first_arrival.load(Ordering::Relaxed) != NO_ORIGIN
        }

        /// The device's name.
        pub fn device_name(&self) -> &str {
            &self.name
        }

        /// The file's frame size.
        pub fn size(&self) -> (u32, u32) {
            self.size
        }

        /// The connection delivers unmirrored frames (read back after configuring).
        pub fn mirrored(&self) -> bool {
            self.mirrored
        }

        /// How the camera is doing ([`camera_health`]; a writer error counts as lost). Sticky:
        /// once not Ok it stays so, and no frame is written after it.
        pub fn health(&self) -> CameraHealth {
            match self.inner.verdict.load(Ordering::Relaxed) {
                1 => return CameraHealth::Lost,
                2 => return CameraHealth::NeverStarted,
                _ => {}
            }
            let origin_at = self.inner.origin_at.load(Ordering::Relaxed);
            if origin_at == NO_ORIGIN {
                return CameraHealth::Ok;
            }
            // SAFETY: thread-safe property reads on live objects.
            let (connected, running) = unsafe { (self.device.isConnected(), self.session.isRunning()) };
            let writer_failed = self.inner.st.lock().0.error.is_some();
            let first = self.inner.first_arrival.load(Ordering::Relaxed);
            let first = (first != NO_ORIGIN).then_some(first);
            let h = if writer_failed {
                CameraHealth::Lost
            } else {
                camera_health(connected, running, first, self.inner.last_arrival.load(Ordering::Relaxed), origin_at, host_now_ns())
            };
            if h != CameraHealth::Ok {
                tracing::warn!(
                    "record camera: {h:?} (connected {connected}, running {running}, writer failed {writer_failed})"
                );
                self.inner.verdict.store(if h == CameraHealth::Lost { 1 } else { 2 }, Ordering::Relaxed);
                self.inner.lost.store(true, Ordering::Relaxed);
            }
            h
        }

        /// The camera dropped out or never started (see [`Self::health`]).
        pub fn is_lost(&self) -> bool {
            self.health() != CameraHealth::Ok
        }

        /// Stops the camera and closes the file at the screen's end `end_ns` (timeline ns),
        /// so it covers the screen's whole length. Nothing written = the file is removed.
        pub fn finish(self, end_ns: u64) -> CameraOutcome {
            let lost = self.is_lost();
            // SAFETY: stopRunning is allowed from any thread; it blocks until stopped, after
            // which no more delegate calls arrive.
            unsafe { self.session.stopRunning() };
            let origin = self.inner.origin.load(Ordering::Acquire);
            let mut guard = self.inner.st.lock();
            let st = &mut guard.0;
            st.stopping = true;
            if !st.started || st.written == 0 {
                // SAFETY: cancel on our own writer (started or not).
                unsafe { st.writer.cancelWriting() };
                let _ = std::fs::remove_file(&self.path);
                tracing::info!("record camera: no frame written — no camera file");
                return CameraOutcome { meta: None, lost };
            }
            let last = st.last_pts.unwrap_or(0);
            let end_prog = if origin == NO_ORIGIN { last } else { (end_ns as i64 - origin).max(last as i64) as u64 };
            let ok = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
                // SAFETY: the writer is writing (started, no failure checked below); the
                // session end lies at or after every appended frame.
                unsafe {
                    if st.writer.status() != AVAssetWriterStatus::Writing {
                        return false;
                    }
                    st.writer.endSessionAtSourceTime(CMTime::new(end_prog as i64, NS_PER_S as i32));
                    st.input.markAsFinished();
                    let (tx, rx) = mpsc::channel::<()>();
                    let block = RcBlock::new(move || {
                        let _ = tx.send(());
                    });
                    st.writer.finishWritingWithCompletionHandler(&block);
                    rx.recv_timeout(Duration::from_secs(20)).is_ok()
                        && st.writer.status() == AVAssetWriterStatus::Completed
                }
            }));
            if !matches!(ok, Ok(true)) {
                tracing::warn!("record camera: finishing camera.mp4 failed: {}", writer_error(&st.writer));
                // The fragments written so far may still be a readable file: keep it, marked as
                // ending at the last written frame; otherwise nothing references it — remove it.
                let readable = readable_secs(&self.path);
                if keep_after_failed_finish(readable) {
                    tracing::info!("record camera: keeping the readable part ({readable:?} s)");
                    let meta = camera_meta(Some(self.size), self.fps, &self.name, Some(last / 1_000_000));
                    return CameraOutcome { meta, lost: true };
                }
                let _ = std::fs::remove_file(&self.path);
                return CameraOutcome { meta: None, lost };
            }
            tracing::info!(
                "record camera: {} frames, {} refused, {} encoder busy, end {} ms",
                st.written,
                st.gate.rejected,
                st.not_ready,
                end_prog / 1_000_000
            );
            let lost_at = lost.then_some(last / 1_000_000);
            CameraOutcome { meta: camera_meta(Some(self.size), self.fps, &self.name, lost_at), lost }
        }
    }

    /// Live, no permission needed (formats are readable without opening the device): the
    /// built-in camera's formats and which one each quality picks.
    #[cfg(test)]
    #[test]
    #[ignore]
    fn camera_formats_live() {
        let list = super::devices();
        let Some(d) = super::pick_device(&list, "") else {
            println!("no camera");
            return;
        };
        // SAFETY: class lookup with a valid NSString; formats are plain reads.
        let dev = unsafe { AVCaptureDevice::deviceWithUniqueID(&NSString::from_str(&d.id)) }.expect("device");
        let infos = format_infos(&dev);
        println!("{}: {} formats", d.name, infos.len());
        for f in &infos {
            println!("  {}x{} up to {:.0} fps", f.width, f.height, f.max_fps);
        }
        for q in [CameraQuality::P1080, CameraQuality::P1440, CameraQuality::P2160] {
            let i = pick_format(&infos, q.ideal_size()).expect("a format");
            let f = infos[i];
            println!(
                "{} → {}x{} @ {} fps, file {:?}",
                q.as_str(),
                f.width,
                f.height,
                target_fps(f.max_fps),
                camera_output_size((f.width, f.height), q.ideal_size())
            );
        }
    }

    impl Drop for CameraCapture {
        fn drop(&mut self) {
            // SAFETY: as in `finish`; idempotent on a stopped session. Clearing the delegate
            // (nil delegate with a nil queue is the documented way) before the objects are
            // released means no late callback can reach a freed delegate.
            unsafe {
                self.session.stopRunning();
                self.output.setSampleBufferDelegate_queue(None, None);
            }
        }
    }

    /// The AVURLAsset duration of a file in seconds, `None` when unreadable.
    #[allow(deprecated)] // the synchronous `duration`; this runs once, off the main thread
    fn readable_secs(path: &Path) -> Option<f64> {
        use objc2_av_foundation::AVURLAsset;
        let url = NSURL::from_file_path(path)?;
        let r = objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
            // SAFETY: a local file URL; `duration` loads synchronously and has no other
            // preconditions. Exceptions are caught.
            autoreleasepool(|_| unsafe {
                let d = AVURLAsset::URLAssetWithURL_options(&url, None).duration();
                (d.timescale > 0 && d.flags.0 & 1 != 0).then(|| d.value as f64 / d.timescale as f64)
            })
        }));
        r.ok().flatten()
    }

    /// The file size the camera will record at, without opening it (formats are readable
    /// without permission). `None` = the device is not there.
    pub fn planned_size(device_id: &str, quality: CameraQuality) -> Option<(u32, u32)> {
        let list = super::devices();
        let dev = super::pick_device(&list, device_id)?;
        autoreleasepool(|_| {
            // SAFETY: class lookup with a valid NSString.
            let d = unsafe { AVCaptureDevice::deviceWithUniqueID(&NSString::from_str(&dev.id)) }?;
            let infos = format_infos(&d);
            let i = pick_format(&infos, quality.ideal_size())?;
            Some(camera_output_size((infos[i].width, infos[i].height), quality.ideal_size()))
        })
    }
}

#[cfg(target_os = "macos")]
pub use capture::{planned_size, CameraCapture};

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, builtin: bool) -> CameraDevice {
        CameraDevice { id: id.into(), name: format!("cam {id}"), builtin }
    }

    fn ns(s: f64) -> u64 {
        (s * 1e9) as u64
    }

    #[test]
    fn camera_pts_drops_before_origin() {
        let clock = PauseShared::new();
        let origin = ns(10.0);
        assert_eq!(camera_pts(ns(9.99), origin, &clock), None, "before the first screen frame");
        assert_eq!(camera_pts(origin, origin, &clock), Some(0), "at the origin = 0");
        assert_eq!(camera_pts(ns(10.5), origin, &clock), Some(ns(0.5)));
    }

    #[test]
    fn camera_follows_pause_clock() {
        let clock = PauseShared::new();
        let origin = ns(10.0);
        clock.pause(ns(12.0) as i64);
        // Captured during the (still open) pause → dropped.
        assert_eq!(camera_pts(ns(12.5), origin, &clock), None);
        clock.resume(ns(14.0) as i64);
        assert_eq!(camera_pts(ns(13.9), origin, &clock), None, "inside the closed pause");
        // Before the pause: unshifted, even when delivered late.
        assert_eq!(camera_pts(ns(11.9), origin, &clock), Some(ns(1.9)));
        // After a 2-s pause: 2 s earlier on the programme clock.
        assert_eq!(camera_pts(ns(15.0), origin, &clock), Some(ns(3.0)));
        // The same rule the screen uses (timeline_of).
        assert_eq!(clock.timeline_of(ns(15.0) as i64).map(|t| (t - origin as i64) as u64), Some(ns(3.0)));
    }

    #[test]
    fn foreign_clock_stamps_fall_back_to_arrival() {
        let now = 100_000_000_000;
        assert_eq!(sample_host_ns(Some(now - 30_000_000), now), now - 30_000_000, "a host stamp is kept");
        assert_eq!(sample_host_ns(Some(5), now), now, "another clock → arrival");
        assert_eq!(sample_host_ns(Some(now + 1_000_000_000), now), now, "from the future → arrival");
        assert_eq!(sample_host_ns(None, now), now);
    }

    #[test]
    fn format_choice_prefers_30fps_then_nearest_size() {
        let f = |w, h, fps| FormatInfo { width: w, height: h, max_fps: fps };
        let list = [f(640, 480, 30.0), f(1280, 720, 30.0), f(1920, 1080, 30.0), f(3840, 2160, 15.0)];
        assert_eq!(pick_format(&list, (1920, 1080)), Some(2));
        assert_eq!(pick_format(&list, (3840, 2160)), Some(2), "4K only at 15 fps loses to 1080p30");
        assert_eq!(pick_format(&list, (2560, 1440)), Some(2));
        let only_slow = [f(1920, 1080, 15.0), f(1280, 720, 15.0)];
        assert_eq!(pick_format(&only_slow, (1920, 1080)), Some(0), "else the nearest size");
        // The FaceTime HD camera's real list: 4K/1440p wishes get 1080p, not the square.
        let facetime = [
            f(1920, 1080, 30.0), f(1280, 720, 30.0), f(1080, 1920, 30.0), f(1760, 1328, 30.0),
            f(640, 480, 30.0), f(1328, 1760, 30.0), f(1552, 1552, 30.0),
        ];
        for q in [(1920, 1080), (2560, 1440), (3840, 2160)] {
            assert_eq!(pick_format(&facetime, q), Some(0), "{q:?}");
        }
        let only_4_3 = [f(640, 480, 30.0), f(1760, 1328, 30.0)];
        assert_eq!(pick_format(&only_4_3, (1920, 1080)), Some(1), "no 16:9 → the nearest size");
        let with_portrait = [f(1080, 1920, 30.0), f(1280, 720, 30.0)];
        assert_eq!(pick_format(&with_portrait, (1920, 1080)), Some(1), "portrait skipped");
        assert_eq!(pick_format(&[], (1920, 1080)), None);
        assert_eq!(target_fps(60.0), 30);
        assert_eq!(target_fps(15.0), 15);
        assert_eq!(target_fps(f64::NAN), 30);
    }

    #[test]
    fn output_size_caps_at_the_quality() {
        assert_eq!(camera_output_size((1920, 1080), (1920, 1080)), (1920, 1080));
        assert_eq!(camera_output_size((3840, 2160), (1920, 1080)), (1920, 1080));
        assert_eq!(camera_output_size((1280, 720), (1920, 1080)), (1280, 720), "never scaled up");
        assert_eq!(camera_output_size((1552, 1552), (1920, 1080)), (1080, 1080), "aspect kept");
    }

    #[test]
    fn lost_on_unplug_stop_or_stall() {
        use CameraHealth::*;
        let now = 50_000_000_000;
        let origin = now - 20_000_000_000;
        let first = Some(origin);
        assert_eq!(camera_health(true, true, first, now - 100_000_000, origin, now), Ok);
        assert_eq!(camera_health(false, true, first, now, origin, now), Lost, "unplugged");
        assert_eq!(camera_health(true, false, first, now, origin, now), Lost, "session stopped");
        assert_eq!(camera_health(true, true, first, now - CAMERA_STALL_NS - 1, origin, now), Lost, "frames stopped");
        assert_eq!(camera_health(true, true, first, now - CAMERA_STALL_NS, origin, now), Ok, "exactly the limit");
    }

    #[test]
    fn slow_starting_camera_gets_a_grace_before_its_first_frame() {
        use CameraHealth::*;
        let origin = 10_000_000_000;
        // No frame yet, 3 s after the origin: not lost (the stall rule doesn't apply yet,
        // even though `last_arrival` still holds the start time far back).
        let stale_start = origin - 1_000_000_000;
        assert_eq!(camera_health(true, true, None, stale_start, origin, origin + 3_000_000_000), Ok);
        assert_eq!(camera_health(true, true, None, stale_start, origin, origin + CAMERA_FIRST_FRAME_GRACE_NS), Ok);
        // No frame by the end of the grace: never started.
        assert_eq!(
            camera_health(true, true, None, stale_start, origin, origin + CAMERA_FIRST_FRAME_GRACE_NS + 1),
            NeverStarted
        );
        assert_eq!(camera_health(false, true, None, stale_start, origin, origin + 1), NeverStarted, "gone before a frame");
        // A first frame at 6 s, then 2 s silence: lost.
        let first = origin + 6_000_000_000;
        assert_eq!(camera_health(true, true, Some(first), first, origin, first + 1_000_000_000), Ok);
        assert_eq!(camera_health(true, true, Some(first), first, origin, first + CAMERA_STALL_NS + 1), Lost);
    }

    #[test]
    fn failed_finish_keeps_only_a_readable_file() {
        assert!(keep_after_failed_finish(Some(3.2)));
        assert!(!keep_after_failed_finish(Some(0.0)), "an empty file says nothing");
        assert!(!keep_after_failed_finish(None), "unreadable");
        assert!(!keep_after_failed_finish(Some(f64::NAN)));
    }

    #[test]
    fn meta_only_when_something_was_written() {
        assert_eq!(camera_meta(None, 30, "FaceTime", None), None);
        let m = camera_meta(Some((1920, 1080)), 30, "FaceTime", Some(4200)).unwrap();
        assert_eq!((m.width, m.height, m.fps, m.offset_ms), (1920, 1080, 30, 0));
        assert_eq!(m.device_name, "FaceTime");
        assert_eq!(m.ended_early_ms, Some(4200));
    }

    #[test]
    fn camera_bitrate_ladder() {
        assert_eq!(camera_bitrate(3840, 2160), 40_000_000);
        assert_eq!(camera_bitrate(2560, 1440), 24_000_000);
        assert_eq!(camera_bitrate(1920, 1080), 16_000_000);
        assert_eq!(camera_bitrate(1280, 720), 8_000_000);
        assert_eq!(camera_bitrate(640, 480), 4_000_000);
        // Measured on the shorter side, so a portrait frame lands on the same rung.
        assert_eq!(camera_bitrate(1080, 1920), 16_000_000);
        assert_eq!(camera_bitrate(1920, 1079), 8_000_000, "just under 1080 drops a rung");
    }

    #[test]
    fn quality_sizes_and_names() {
        assert_eq!(CameraQuality::P1080.ideal_size(), (1920, 1080));
        assert_eq!(CameraQuality::P1440.ideal_size(), (2560, 1440));
        assert_eq!(CameraQuality::P2160.ideal_size(), (3840, 2160));
        assert_eq!(CameraQuality::default(), CameraQuality::P1080);
        for q in [CameraQuality::P1080, CameraQuality::P1440, CameraQuality::P2160] {
            assert_eq!(CameraQuality::parse(q.as_str()), q);
            assert_eq!(serde_json::to_string(&q).unwrap(), format!("\"{}\"", q.as_str()));
        }
        assert_eq!(CameraQuality::parse("8k"), CameraQuality::P1080, "unknown → default");
        assert_eq!(serde_json::to_string(&CameraAuth::NotDetermined).unwrap(), "\"not-determined\"");
    }

    #[test]
    fn pick_device_prefers_wanted_then_builtin() {
        let list = [dev("usb", false), dev("mac", true), dev("desk", false)];
        assert_eq!(pick_device(&list, "desk").unwrap().id, "desk", "the wanted id wins");
        assert_eq!(pick_device(&list, "").unwrap().id, "mac", "no wish → built-in");
        assert_eq!(pick_device(&list, "gone").unwrap().id, "mac", "unplugged wish → built-in");
        let no_builtin = [dev("usb", false), dev("desk", false)];
        assert_eq!(pick_device(&no_builtin, "gone").unwrap().id, "usb", "else the first");
    }

    #[test]
    fn pick_device_empty_is_none() {
        assert!(pick_device(&[], "").is_none());
        assert!(pick_device(&[], "mac").is_none());
    }

    /// Live: lists the cameras and reads the permission (never asks — the test binary has no
    /// usage string, a request would make TCC end the process).
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn camera_devices_live() {
        let auth = authorization();
        let list = devices();
        println!("auth {auth:?}, {} device(s): {list:?}", list.len());
        assert!(list.iter().any(|d| d.builtin), "expected the built-in camera");
    }
}
