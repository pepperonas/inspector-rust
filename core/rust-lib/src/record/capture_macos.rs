//! ScreenCaptureKit capture of a display or window into `screen.mp4`:
//! H.264 video + (optionally) AAC system audio, written by AVAssetWriter.
//!
//! Port of OpenScreen's `ScreenCaptureRecorder.swift` (`setupWriter`,
//! `makeStreamConfiguration`, `retimedSampleBuffer`, `isCompleteFrame`):
//!
//! * Buffer size from `filter.contentRect × pointPixelScale`
//!   ([`capture_output_size`], capped at 3840×2160), 420v, BT.709, `queueDepth 6`.
//! * H.264 with `AllowFrameReordering = false` (a reordered stream needs
//!   negative composition offsets that a fragmented MP4 can't carry) and a
//!   keyframe every second (by count AND by duration).
//! * Pauses are cut out of the timeline by each sample's CAPTURE time (its
//!   host-clock PTS), never its delivery time: [`PauseShared::offset_at`]
//!   drops a sample captured inside a pause and otherwise shifts it back by
//!   the pauses that ended before it (`CMSampleBufferCreateCopyWithNewTiming`).
//!   So pre-pause frames delivered late keep their place, frames captured in
//!   a pause never appear, and the file has no gap.
//! * Video goes through [`VideoTimestampGate`] (a non-advancing PTS kills the
//!   writer one append later); only `SCFrameStatus::Complete` frames are used.
//! * The session starts at the first admitted video frame; audio before it
//!   is dropped. Its timeline time is [`Capture::origin_ns`] — microphone and
//!   cursor align to it.
//! * A writer failure is recorded with `writer.error` and returned by
//!   [`Capture::stop`]; nothing fails silently.

use std::path::Path;
use std::ptr::NonNull;
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, sel, AllocAnyThread, DefinedClass, Message};
use objc2_av_foundation::{
    AVAssetWriter, AVAssetWriterInput, AVAssetWriterStatus, AVFileTypeMPEG4, AVMediaTypeAudio, AVMediaTypeVideo,
    AVVideoAllowFrameReorderingKey, AVVideoAverageBitRateKey, AVVideoCodecKey, AVVideoCodecTypeH264,
    AVVideoColorPrimariesKey, AVVideoColorPrimaries_ITU_R_709_2, AVVideoColorPropertiesKey,
    AVVideoCompressionPropertiesKey, AVVideoExpectedSourceFrameRateKey, AVVideoHeightKey,
    AVVideoMaxKeyFrameIntervalDurationKey, AVVideoMaxKeyFrameIntervalKey, AVVideoTransferFunctionKey,
    AVVideoTransferFunction_ITU_R_709_2, AVVideoWidthKey, AVVideoYCbCrMatrixKey, AVVideoYCbCrMatrix_ITU_R_709_2,
};
use objc2_core_foundation::{CFRetained, CFString, CGRect};
use objc2_core_media::{CMSampleBuffer, CMSampleTimingInfo, CMTime};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSString, NSURL};
use objc2_screen_capture_kit::{
    SCContentFilter, SCFrameStatus, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate,
    SCRunningApplication, SCStreamFrameInfoStatus, SCStreamOutput, SCStreamOutputType, SCWindow,
};
use parking_lot::{Condvar, Mutex};

use super::core::{capture_output_size, default_video_bitrate, key_frame_interval, GateVerdict, VideoTimestampGate};
use super::{host_now_ns, PauseShared};

/// Output ceiling (Global Constraints): 4K UHD.
/// Frame-rate ceiling for the stream and the encoder hint.
pub const MAX_FPS: u32 = 60;
pub const MAX_W: u32 = 3840;
pub const MAX_H: u32 = 2160;
/// System audio: 48 kHz stereo, AAC 192 kb/s.
pub const AUDIO_RATE: i64 = 48_000;
pub const AUDIO_CHANNELS: i64 = 2;
pub const AUDIO_BITRATE: i64 = 192_000;
/// `kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange` ('420v').
const PIXEL_420V: u32 = u32::from_be_bytes(*b"420v");
const NS_PER_S: i64 = 1_000_000_000;
/// `kCMTimeFlags_Valid`.
const CMTIME_VALID: u32 = 1;

// ---------- pure timing helpers ----------

/// Nanoseconds of a CMTime (value / timescale), `None` if invalid or the
/// timescale is not positive.
pub fn cmtime_ns(value: i64, timescale: i32, flags: u32) -> Option<i64> {
    if flags & CMTIME_VALID == 0 || timescale <= 0 {
        return None;
    }
    Some((value as i128 * NS_PER_S as i128 / timescale as i128) as i64)
}

/// A nanosecond span expressed in `timescale` ticks, rounded to nearest.
pub fn ns_to_ticks(ns: i64, timescale: i32) -> i64 {
    if timescale <= 0 {
        return 0;
    }
    let num = ns as i128 * timescale as i128;
    let den = NS_PER_S as i128;
    let q = num / den;
    let r = num % den;
    let adj = if 2 * r.abs() >= den { num.signum() } else { 0 };
    (q + adj) as i64
}

/// The `value` of a timestamp shifted back by a pause offset (ns), in the
/// timestamp's own timescale. An invalid time stays untouched.
pub fn retime_value(value: i64, timescale: i32, flags: u32, offset_ns: i64) -> i64 {
    if flags & CMTIME_VALID == 0 || offset_ns == 0 {
        return value;
    }
    value - ns_to_ticks(offset_ns, timescale)
}

/// Audio admission once the writer session runs: only samples at or after
/// the session start, and strictly after the last appended audio sample.
#[derive(Debug, Default)]
pub struct AudioGate {
    last: Option<i64>,
    pub dropped: u32,
}

impl AudioGate {
    pub fn admit(&mut self, pts_ns: i64, session_start_ns: Option<i64>) -> bool {
        let Some(start) = session_start_ns else {
            self.dropped += 1;
            return false;
        };
        if pts_ns < start || self.last.is_some_and(|l| pts_ns <= l) {
            self.dropped += 1;
            return false;
        }
        true
    }
    pub fn record(&mut self, pts_ns: i64) {
        self.last = Some(pts_ns);
    }
}

/// Recording length from the first admitted frame to the session end.
pub fn duration_ms(first_ns: Option<i64>, end_ns: i64) -> u64 {
    first_ns.map(|f| ((end_ns - f).max(0) / 1_000_000) as u64).unwrap_or(0)
}

// ---------- public types ----------

#[derive(Debug, Clone, Copy)]
pub struct CaptureOptions {
    pub system_audio: bool,
    pub show_system_cursor: bool,
    pub fps: u32,
}

#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub width: u32,
    pub height: u32,
    /// First admitted video frame → session end (= the file's length).
    pub duration_ms: u64,
    /// Video frames refused by the timestamp gate.
    pub rejected_frames: u32,
    /// Timeline ns of video time 0 (`None` only when no frame arrived — then
    /// `stop` returns an error instead).
    pub origin_ns: Option<i64>,
    /// Timeline ns the writer session ended at (pad microphone audio to it).
    pub end_ns: i64,
    /// Complete frames the encoder was not ready for.
    pub dropped_frames: u32,
    /// ScreenCaptureKit stopped on its own mid-take (display gone, permission
    /// revoked …). The file is still finished up to that point.
    pub stream_error: Option<String>,
}

struct Sendable<T>(T);
unsafe impl<T> Send for Sendable<T> {}
unsafe impl<T> Sync for Sendable<T> {}

struct WState {
    writer: Retained<AVAssetWriter>,
    video: Retained<AVAssetWriterInput>,
    audio: Option<Retained<AVAssetWriterInput>>,
    started: bool,
    stopping: bool,
    gate: VideoTimestampGate,
    audio_gate: AudioGate,
    first_ns: Option<i64>,
    last_video_ns: Option<i64>,
    not_ready: u32,
    error: Option<String>,
}

struct Inner {
    clock: Arc<PauseShared>,
    st: Mutex<Sendable<WState>>,
    first: (Mutex<Option<i64>>, Condvar),
    stream_error: Mutex<Option<String>>,
}

fn writer_error(w: &AVAssetWriter) -> String {
    unsafe {
        w.error()
            .map(|e| e.localizedDescription().to_string())
            .unwrap_or_else(|| format!("AVAssetWriter status {}", w.status().0))
    }
}

impl Inner {
    fn fail(st: &mut WState, stage: &str) {
        if st.error.is_none() {
            let msg = format!("{stage}: {}", writer_error(&st.writer));
            tracing::warn!("record: writer failed — {msg}");
            st.error = Some(msg);
        }
    }

    fn on_sample(&self, sample: &CMSampleBuffer, ty: SCStreamOutputType) {
        if !unsafe { sample.data_is_ready() } {
            return;
        }
        // Pause decisions use the sample's own capture time (host clock),
        // never the delivery time: pre-pause content arriving during a pause
        // is kept with its own offset, content captured inside a pause is
        // dropped even when it arrives after the resume.
        let raw = unsafe { sample.presentation_time_stamp() };
        let Some(raw_ns) = cmtime_ns(raw.value, raw.timescale, raw.flags.0) else { return };
        let Some(offset) = self.clock.offset_at(raw_ns) else { return };
        let mut guard = self.st.lock();
        let st = &mut guard.0;
        if st.stopping || st.error.is_some() {
            return;
        }
        if ty == SCStreamOutputType::Screen {
            if !is_complete_frame(sample) {
                return;
            }
            let Some(buf) = retimed(sample, offset) else { return };
            let pts = unsafe { buf.presentation_time_stamp() };
            let ns = cmtime_ns(pts.value, pts.timescale, pts.flags.0);
            if self_check(&mut st.gate, ns) != GateVerdict::Admit {
                return;
            }
            let ns = ns.unwrap_or_default();
            if !st.started {
                if !unsafe { st.writer.startWriting() } {
                    Self::fail(st, "start");
                    return;
                }
                unsafe { st.writer.startSessionAtSourceTime(pts) };
                st.started = true;
                st.first_ns = Some(ns);
                let (lock, cv) = &self.first;
                *lock.lock() = Some(ns);
                cv.notify_all();
            }
            if unsafe { st.writer.status() } == AVAssetWriterStatus::Failed {
                Self::fail(st, "writer status");
                return;
            }
            if unsafe { st.video.isReadyForMoreMediaData() } {
                if unsafe { st.video.appendSampleBuffer(&buf) } {
                    st.gate.record(ns);
                    st.last_video_ns = Some(ns);
                } else {
                    Self::fail(st, "video append");
                }
            } else {
                st.not_ready += 1;
            }
        } else if ty == SCStreamOutputType::Audio {
            if !st.started || st.audio.is_none() {
                return;
            }
            let Some(buf) = retimed(sample, offset) else { return };
            let pts = unsafe { buf.presentation_time_stamp() };
            let Some(ns) = cmtime_ns(pts.value, pts.timescale, pts.flags.0) else { return };
            if !st.audio_gate.admit(ns, st.first_ns) {
                return;
            }
            let Some(audio) = st.audio.clone() else { return };
            if unsafe { audio.isReadyForMoreMediaData() } {
                if unsafe { audio.appendSampleBuffer(&buf) } {
                    st.audio_gate.record(ns);
                } else {
                    Self::fail(st, "audio append");
                }
            }
        }
    }
}

fn clamp_fps(fps: u32) -> u32 {
    fps.clamp(1, MAX_FPS)
}

fn self_check(gate: &mut VideoTimestampGate, ns: Option<i64>) -> GateVerdict {
    let v = gate.check(ns);
    if v != GateVerdict::Admit && gate.rejected == 1 {
        tracing::info!("record: first refused video frame ({v:?})");
    }
    v
}

/// `SCStreamFrameInfo.status == complete`. A buffer without the attachment
/// counts as complete (Swift does the same).
fn is_complete_frame(sample: &CMSampleBuffer) -> bool {
    unsafe {
        let Some(arr) = sample.sample_attachments_array(false) else { return true };
        let arr = CFRetained::as_ptr(&arr).as_ptr() as *mut AnyObject;
        let n: usize = msg_send![arr, count];
        if n == 0 {
            return true;
        }
        let dict: *mut AnyObject = msg_send![arr, objectAtIndex: 0usize];
        if dict.is_null() {
            return true;
        }
        let key: &NSString = SCStreamFrameInfoStatus;
        let v: *mut AnyObject = msg_send![dict, objectForKey: key];
        if v.is_null() {
            return true;
        }
        let raw: isize = msg_send![v, integerValue];
        SCFrameStatus(raw) == SCFrameStatus::Complete
    }
}

/// A copy of `sample` with every PTS/DTS moved back by `offset_ns`; the
/// sample itself when there is nothing to shift. `None` (sample dropped, never
/// passed on unshifted) if CoreMedia refuses.
fn retimed(sample: &CMSampleBuffer, offset_ns: i64) -> Option<CFRetained<CMSampleBuffer>> {
    let same = || unsafe { CFRetained::retain(NonNull::from(sample)) };
    if offset_ns == 0 {
        return Some(same());
    }
    let count = unsafe { sample.num_samples() };
    if count <= 0 {
        return Some(same());
    }
    let mut needed: isize = 0;
    let zero = CMTime { value: 0, timescale: 0, flags: objc2_core_media::CMTimeFlags(0), epoch: 0 };
    let blank = CMSampleTimingInfo { duration: zero, presentationTimeStamp: zero, decodeTimeStamp: zero };
    // Timing entries can be fewer than samples (one entry for all).
    let st = unsafe { sample.sample_timing_info_array(0, std::ptr::null_mut(), &mut needed) };
    if st != 0 || needed <= 0 {
        return None;
    }
    let mut timing = vec![blank; needed as usize];
    let st = unsafe { sample.sample_timing_info_array(needed, timing.as_mut_ptr(), &mut needed) };
    if st != 0 {
        return None;
    }
    for t in timing.iter_mut() {
        let p = &mut t.presentationTimeStamp;
        p.value = retime_value(p.value, p.timescale, p.flags.0, offset_ns);
        let d = &mut t.decodeTimeStamp;
        d.value = retime_value(d.value, d.timescale, d.flags.0, offset_ns);
    }
    let mut out: *mut CMSampleBuffer = std::ptr::null_mut();
    let st = unsafe {
        CMSampleBuffer::create_copy_with_new_timing(
            None,
            sample,
            timing.len() as isize,
            timing.as_ptr(),
            NonNull::from(&mut out),
        )
    };
    if st != 0 {
        return None;
    }
    NonNull::new(out).map(|p| unsafe { CFRetained::from_raw(p) })
}

// ---------- Objective-C glue ----------

pub struct Ivars {
    inner: Arc<Inner>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "IRRecordCaptureOutput"]
    #[ivars = Ivars]
    struct CaptureOutput;

    unsafe impl NSObjectProtocol for CaptureOutput {}

    unsafe impl SCStreamOutput for CaptureOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn did_output(&self, _stream: &SCStream, sample: &CMSampleBuffer, ty: SCStreamOutputType) {
            let inner = &self.ivars().inner;
            // An Objective-C exception from AVAssetWriter must not unwind
            // through the dispatch queue (that aborts the app).
            let res = objc2::exception::catch(std::panic::AssertUnwindSafe(|| inner.on_sample(sample, ty)));
            if let Err(e) = res {
                let msg = e.map(|e| format!("{e:?}")).unwrap_or_else(|| "exception".into());
                tracing::warn!("record: exception while writing a sample: {msg}");
                let mut g = inner.st.lock();
                if g.0.error.is_none() {
                    g.0.error = Some(msg);
                }
            }
        }
    }

    unsafe impl SCStreamDelegate for CaptureOutput {
        #[unsafe(method(stream:didStopWithError:))]
        fn did_stop(&self, _stream: &SCStream, error: &NSError) {
            let msg = error.localizedDescription().to_string();
            tracing::warn!("record: capture stopped: {msg}");
            *self.ivars().inner.stream_error.lock() = Some(msg);
        }
    }
);

impl CaptureOutput {
    fn create(inner: Arc<Inner>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars { inner });
        unsafe { msg_send![super(this), init] }
    }
}

fn queue() -> &'static DispatchQueue {
    static Q: OnceLock<Sendable<DispatchRetained<DispatchQueue>>> = OnceLock::new();
    &Q.get_or_init(|| Sendable(DispatchQueue::new("io.celox.inspector-rust.record-capture", None))).0
}

fn any<T: Message>(r: Retained<T>) -> Retained<AnyObject> {
    // SAFETY: every Objective-C object is an AnyObject.
    unsafe { Retained::cast_unchecked(r) }
}

fn num(v: i64) -> Retained<AnyObject> {
    any(NSNumber::new_i64(v))
}

fn key(k: Option<&'static NSString>) -> Result<&'static NSString, String> {
    k.ok_or_else(|| crate::i18n::tf("rec2.capture_writer_failed", &[("reason", "AVFoundation constant missing")]))
}

fn video_settings(w: u32, h: u32, fps: u32) -> Result<Retained<NSDictionary<NSString, AnyObject>>, String> {
    unsafe {
        let color = NSDictionary::<NSString, AnyObject>::from_retained_objects(
            &[key(AVVideoColorPrimariesKey)?, key(AVVideoTransferFunctionKey)?, key(AVVideoYCbCrMatrixKey)?],
            &[
                any(key(AVVideoColorPrimaries_ITU_R_709_2)?.retain()),
                any(key(AVVideoTransferFunction_ITU_R_709_2)?.retain()),
                any(key(AVVideoYCbCrMatrix_ITU_R_709_2)?.retain()),
            ],
        );
        let compression = NSDictionary::<NSString, AnyObject>::from_retained_objects(
            &[
                key(AVVideoAverageBitRateKey)?,
                key(AVVideoExpectedSourceFrameRateKey)?,
                key(AVVideoAllowFrameReorderingKey)?,
                key(AVVideoMaxKeyFrameIntervalKey)?,
                key(AVVideoMaxKeyFrameIntervalDurationKey)?,
            ],
            &[
                num(default_video_bitrate(w, h, fps) as i64),
                num(fps as i64),
                any(NSNumber::new_bool(false)),
                num(key_frame_interval(fps) as i64),
                num(1),
            ],
        );
        let codec: &NSString = key(AVVideoCodecTypeH264)?;
        Ok(NSDictionary::<NSString, AnyObject>::from_retained_objects(
            &[
                key(AVVideoCodecKey)?,
                key(AVVideoWidthKey)?,
                key(AVVideoHeightKey)?,
                key(AVVideoColorPropertiesKey)?,
                key(AVVideoCompressionPropertiesKey)?,
            ],
            &[any(codec.retain()), num(w as i64), num(h as i64), any(color), any(compression)],
        ))
    }
}

/// AAC settings. The AVFAudio key constants' values are their own names
/// (`AVFormatIDKey` = @"AVFormatIDKey" …); spelled out to avoid pulling in
/// the whole AVFAudio binding for four strings.
fn audio_settings() -> Retained<NSDictionary<NSString, AnyObject>> {
    let k = |s: &str| NSString::from_str(s);
    let keys = [k("AVFormatIDKey"), k("AVSampleRateKey"), k("AVNumberOfChannelsKey"), k("AVEncoderBitRateKey")];
    let refs: Vec<&NSString> = keys.iter().map(|k| &**k).collect();
    // kAudioFormatMPEG4AAC = 'aac '
    let aac = u32::from_be_bytes(*b"aac ") as i64;
    NSDictionary::from_retained_objects(&refs, &[num(aac), num(AUDIO_RATE), num(AUDIO_CHANNELS), num(AUDIO_BITRATE)])
}

// ---------- window bounds / filters ----------

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CgRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *const std::ffi::c_void;
    fn CGRectMakeWithDictionaryRepresentation(dict: *const std::ffi::c_void, rect: *mut CgRect) -> bool;
    // System constants — static for the process lifetime. SCStreamConfiguration's
    // `colorMatrix` / `colorSpaceName` are `assign` (non-retaining) CFStringRef
    // properties: a temporary CFString there dangles and crashes the config copy
    // inside `-[SCStream initWithFilter:…]` (CFStringCreateCopy, SIGTRAP).
    static kCGDisplayStreamYCbCrMatrix_ITU_R_709_2: &'static CFString;
    static kCGColorSpaceSRGB: &'static CFString;
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const std::ffi::c_void);
}

/// The main display's CoreGraphics id.
pub fn main_display_id() -> u32 {
    unsafe { CGMainDisplayID() }
}

/// A window's frame in global top-left points (`kCGWindowBounds`).
pub(crate) fn cg_window_bounds(window_id: u32) -> Option<(f64, f64, f64, f64)> {
    unsafe {
        // kCGWindowListOptionIncludingWindow
        let arr = CGWindowListCopyWindowInfo(1 << 3, window_id) as *mut AnyObject;
        if arr.is_null() {
            return None;
        }
        let k_id = NSString::from_str("kCGWindowNumber");
        let k_bounds = NSString::from_str("kCGWindowBounds");
        let n: usize = msg_send![arr, count];
        let mut out = None;
        for i in 0..n {
            let d: *mut AnyObject = msg_send![arr, objectAtIndex: i];
            let id: *mut AnyObject = msg_send![d, objectForKey: &*k_id];
            if id.is_null() {
                continue;
            }
            let v: u32 = msg_send![id, unsignedIntValue];
            if v != window_id {
                continue;
            }
            let b: *mut AnyObject = msg_send![d, objectForKey: &*k_bounds];
            let mut r = CgRect::default();
            if !b.is_null() && CGRectMakeWithDictionaryRepresentation(b as *const _, &mut r) && r.w > 0.0 && r.h > 0.0
            {
                out = Some((r.x, r.y, r.w, r.h));
            }
            break;
        }
        CFRelease(arr as *const _);
        out
    }
}

/// Captured content in global top-left points: a one-window filter → the
/// window's `kCGWindowBounds` (its `contentRect` sits below the pixels, see
/// Swift `captureContentFrame`), a display filter → `CGDisplayBounds` of its
/// display, else `contentRect`.
fn content_bounds(filter: &SCContentFilter) -> (f64, f64, f64, f64) {
    unsafe {
        let r = filter.contentRect();
        let fallback = (r.origin.x, r.origin.y, r.size.width, r.size.height);
        let fobj: &AnyObject = filter.as_ref();
        if fobj.class().responds_to(sel!(includedWindows)) {
            let wins = filter.includedWindows();
            if wins.count() == 1 {
                if let Some(b) = wins.firstObject().and_then(|w| cg_window_bounds(w.windowID())) {
                    return b;
                }
            }
        }
        if fobj.class().responds_to(sel!(includedDisplays)) {
            let displays: Retained<NSArray<AnyObject>> = msg_send![filter, includedDisplays];
            if displays.count() == 1 {
                if let Some(d) = displays.firstObject() {
                    // SCDisplay.frame: global top-left points.
                    let b: CGRect = msg_send![&*d, frame];
                    if b.size.width > 0.0 && b.size.height > 0.0 {
                        return (b.origin.x, b.origin.y, b.size.width, b.size.height);
                    }
                }
            }
        }
        fallback
    }
}

fn shareable_content(on_screen_only: bool) -> Result<Sendable<Retained<SCShareableContent>>, String> {
    let (tx, rx) = mpsc::channel::<Result<Sendable<Retained<SCShareableContent>>, String>>();
    let block = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        let r = if let Some(c) = unsafe { Retained::retain(content) } {
            Ok(Sendable(c))
        } else if let Some(e) = unsafe { err.as_ref() } {
            Err(e.localizedDescription().to_string())
        } else {
            Err(crate::i18n::t("rec2.capture_no_content"))
        };
        let _ = tx.send(r);
    });
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            false, on_screen_only, &block,
        )
    };
    rx.recv_timeout(Duration::from_secs(5)).map_err(|_| crate::i18n::t("rec2.capture_start_hangs"))?
}

/// Filter for a whole display, minus EVERY window of this process (HUD, bar,
/// countdown, popup) — current and future ones, because the exclusion is by
/// application, not by window id. Listed with off-screen windows too, so a
/// hidden popup still makes the process appear in `applications`.
pub fn display_filter(display_id: u32) -> Result<Retained<SCContentFilter>, String> {
    let content = shareable_content(false)?;
    unsafe {
        let displays = content.0.displays();
        let d = displays
            .iter()
            .find(|d| d.displayID() == display_id)
            .ok_or_else(|| crate::i18n::t("rec2.capture_no_display"))?;
        let me = std::process::id() as i32;
        let own: Vec<Retained<SCRunningApplication>> = content
            .0
            .applications()
            .iter()
            .filter(|a| {
                let pid: i32 = msg_send![&**a, processID];
                pid == me
            })
            .collect();
        if own.is_empty() {
            // A process without any window is not listed — then it has
            // nothing that could appear in the recording either.
            tracing::info!("record: own process not in shareable applications (no windows yet)");
        }
        let apps = NSArray::from_retained_slice(&own);
        let none: Retained<NSArray<SCWindow>> = NSArray::new();
        Ok(SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
            SCContentFilter::alloc(),
            &d,
            &apps,
            &none,
        ))
    }
}

/// Filter for one window (follows it across Spaces, nothing else in frame).
pub fn window_filter(window_id: u32) -> Result<Retained<SCContentFilter>, String> {
    let content = shareable_content(true)?;
    unsafe {
        let w = content
            .0
            .windows()
            .iter()
            .find(|w| w.windowID() == window_id)
            .ok_or_else(|| crate::i18n::t("rec2.capture_no_window"))?;
        Ok(SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &w))
    }
}

// ---------- the capture ----------

pub struct Capture {
    inner: Arc<Inner>,
    stream: Retained<SCStream>,
    _output: Retained<CaptureOutput>,
    width: u32,
    height: u32,
    bounds: (f64, f64, f64, f64),
    out: std::path::PathBuf,
}

// SAFETY: SCStream / the output object are only touched again in `stop`,
// which ScreenCaptureKit allows from any thread; the writer state is behind
// a mutex.
unsafe impl Send for Capture {}

impl Capture {
    pub fn start(
        filter: Retained<SCContentFilter>,
        opts: CaptureOptions,
        out: &Path,
        clock: Arc<PauseShared>,
    ) -> Result<Capture, String> {
        let fps = clamp_fps(opts.fps);
        let bounds = content_bounds(&filter);
        let (width, height) = unsafe {
            let r = filter.contentRect();
            capture_output_size(r.size.width, r.size.height, filter.pointPixelScale() as f64, MAX_W, MAX_H)
        };
        let _ = std::fs::remove_file(out);
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let writer_fail = |reason: String| crate::i18n::tf("rec2.capture_writer_failed", &[("reason", &reason)]);
        unsafe {
            let url = NSURL::from_file_path(out).ok_or_else(|| writer_fail("bad path".into()))?;
            let mp4 = key(AVFileTypeMPEG4)?;
            let writer = AVAssetWriter::initWithURL_fileType_error(AVAssetWriter::alloc(), &url, mp4)
                .map_err(|e| writer_fail(e.localizedDescription().to_string()))?;
            // A readable file even if we die before finishWriting.
            writer.setMovieFragmentInterval(CMTime::new(1, 1));
            let settings = video_settings(width, height, fps)?;
            let video = AVAssetWriterInput::initWithMediaType_outputSettings(
                AVAssetWriterInput::alloc(),
                key(AVMediaTypeVideo)?,
                Some(&settings),
            );
            video.setExpectsMediaDataInRealTime(true);
            if !writer.canAddInput(&video) {
                return Err(writer_fail("H.264 input".into()));
            }
            writer.addInput(&video);
            let audio = if opts.system_audio {
                let a = AVAssetWriterInput::initWithMediaType_outputSettings(
                    AVAssetWriterInput::alloc(),
                    key(AVMediaTypeAudio)?,
                    Some(&audio_settings()),
                );
                a.setExpectsMediaDataInRealTime(true);
                if !writer.canAddInput(&a) {
                    return Err(writer_fail("AAC input".into()));
                }
                writer.addInput(&a);
                Some(a)
            } else {
                None
            };

            let inner = Arc::new(Inner {
                clock,
                st: Mutex::new(Sendable(WState {
                    writer,
                    video,
                    audio,
                    started: false,
                    stopping: false,
                    gate: VideoTimestampGate::new(),
                    audio_gate: AudioGate::default(),
                    first_ns: None,
                    last_video_ns: None,
                    not_ready: 0,
                    error: None,
                })),
                first: (Mutex::new(None), Condvar::new()),
                stream_error: Mutex::new(None),
            });

            let cfg = SCStreamConfiguration::new();
            cfg.setWidth(width as usize);
            cfg.setHeight(height as usize);
            cfg.setScalesToFit(true);
            cfg.setMinimumFrameInterval(CMTime::new(1, fps as i32));
            cfg.setQueueDepth(6);
            cfg.setShowsCursor(opts.show_system_cursor);
            cfg.setPixelFormat(PIXEL_420V);
            cfg.setColorMatrix(kCGDisplayStreamYCbCrMatrix_ITU_R_709_2);
            cfg.setColorSpaceName(kCGColorSpaceSRGB);
            cfg.setCapturesAudio(opts.system_audio);
            cfg.setSampleRate(AUDIO_RATE as isize);
            cfg.setChannelCount(AUDIO_CHANNELS as isize);
            cfg.setExcludesCurrentProcessAudio(true);

            let output = CaptureOutput::create(inner.clone());
            let delegate: &ProtocolObject<dyn SCStreamDelegate> = ProtocolObject::from_ref(&*output);
            let stream = SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, &cfg, Some(delegate));
            let o: &ProtocolObject<dyn SCStreamOutput> = ProtocolObject::from_ref(&*output);
            stream
                .addStreamOutput_type_sampleHandlerQueue_error(o, SCStreamOutputType::Screen, Some(queue()))
                .map_err(|e| e.localizedDescription().to_string())?;
            if opts.system_audio {
                stream
                    .addStreamOutput_type_sampleHandlerQueue_error(o, SCStreamOutputType::Audio, Some(queue()))
                    .map_err(|e| e.localizedDescription().to_string())?;
            }
            let (tx, rx) = mpsc::channel::<Option<String>>();
            let done = RcBlock::new(move |err: *mut NSError| {
                let _ = tx.send(err.as_ref().map(|e| e.localizedDescription().to_string()));
            });
            stream.startCaptureWithCompletionHandler(Some(&done));
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(None) => {}
                Ok(Some(e)) => {
                    let _ = std::fs::remove_file(out);
                    return Err(e);
                }
                Err(_) => {
                    // Best effort: a start that answers late must not leave a
                    // running stream behind.
                    let (stx, srx) = mpsc::channel::<()>();
                    let stopped = RcBlock::new(move |_err: *mut NSError| {
                        let _ = stx.send(());
                    });
                    stream.stopCaptureWithCompletionHandler(Some(&stopped));
                    let _ = srx.recv_timeout(Duration::from_secs(1));
                    let _ = std::fs::remove_file(out);
                    return Err(crate::i18n::t("rec2.capture_start_hangs"));
                }
            }
            Ok(Capture { inner, stream, _output: output, width, height, bounds, out: out.to_path_buf() })
        }
    }

    /// Captured content in global top-left points (for the cursor sampler).
    pub fn content_bounds_pt(&self) -> (f64, f64, f64, f64) {
        self.bounds
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Timeline ns of the first written video frame (= video time 0), once
    /// one has arrived.
    pub fn origin_ns(&self) -> Option<i64> {
        *self.inner.first.0.lock()
    }

    /// A writer error or a stream that stopped on its own, mid-take.
    pub fn failure(&self) -> Option<String> {
        if let Some(e) = self.inner.stream_error.lock().clone() {
            return Some(e);
        }
        self.inner.st.lock().0.error.clone().map(|e| crate::i18n::tf("rec2.capture_writer_failed", &[("reason", &e)]))
    }

    /// Block until the first video frame is written, at most `timeout`.
    pub fn wait_for_first_frame(&self, timeout: Duration) -> Option<i64> {
        let (lock, cv) = &self.inner.first;
        let mut g = lock.lock();
        if g.is_none() {
            let _ = cv.wait_while_for(&mut g, |v| v.is_none(), timeout);
        }
        *g
    }

    /// Stop the stream, close the session at the current timeline time and
    /// finish the file. Errors carry the writer's own message.
    pub fn stop(self) -> Result<CaptureResult, String> {
        let (tx, rx) = mpsc::channel::<()>();
        let done = RcBlock::new(move |_err: *mut NSError| {
            let _ = tx.send(());
        });
        unsafe { self.stream.stopCaptureWithCompletionHandler(Some(&done)) };
        if rx.recv_timeout(Duration::from_secs(5)).is_err() {
            tracing::warn!("record: stopCapture did not answer within 5 s");
        }

        let mut guard = self.inner.st.lock();
        let st = &mut guard.0;
        st.stopping = true;
        let writer_fail = |reason: &str| crate::i18n::tf("rec2.capture_writer_failed", &[("reason", reason)]);
        if !st.started {
            unsafe { st.writer.cancelWriting() };
            let _ = std::fs::remove_file(&self.out);
            return Err(st.error.clone().map(|e| writer_fail(&e)).unwrap_or_else(|| crate::i18n::t("rec2.capture_no_frame")));
        }
        // The session end comes from the same clock the samples were shifted
        // with, read after the stream stopped: at or past every appended sample.
        let now = self.inner.clock.timeline_ns(host_now_ns());
        let end_ns = now.max(st.last_video_ns.unwrap_or(now));
        if let Some(e) = st.error.clone() {
            return Err(writer_fail(&e));
        }
        let finished = unsafe {
            objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
                if st.writer.status() != AVAssetWriterStatus::Writing {
                    return false;
                }
                st.writer.endSessionAtSourceTime(CMTime::new(end_ns, NS_PER_S as i32));
                st.video.markAsFinished();
                if let Some(a) = &st.audio {
                    a.markAsFinished();
                }
                let (ftx, frx) = mpsc::channel::<()>();
                let block = RcBlock::new(move || {
                    let _ = ftx.send(());
                });
                st.writer.finishWritingWithCompletionHandler(&block);
                frx.recv_timeout(Duration::from_secs(20)).is_ok()
            }))
        };
        match finished {
            Err(e) => {
                let msg = e.map(|e| format!("{e:?}")).unwrap_or_else(|| "exception".into());
                return Err(writer_fail(&msg));
            }
            Ok(false) if unsafe { st.writer.status() } != AVAssetWriterStatus::Completed => {
                let status = unsafe { st.writer.status() };
                if status == AVAssetWriterStatus::Writing {
                    return Err(crate::i18n::t("rec2.capture_stop_hangs"));
                }
                return Err(writer_fail(&writer_error(&st.writer)));
            }
            _ => {}
        }
        if unsafe { st.writer.status() } != AVAssetWriterStatus::Completed {
            return Err(writer_fail(&writer_error(&st.writer)));
        }
        Ok(CaptureResult {
            width: self.width,
            height: self.height,
            duration_ms: duration_ms(st.first_ns, end_ns),
            rejected_frames: st.gate.rejected,
            origin_ns: st.first_ns,
            end_ns,
            dropped_frames: st.not_ready,
            stream_error: self.inner.stream_error.lock().clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmtime_converts_to_nanoseconds_and_rejects_invalid() {
        assert_eq!(cmtime_ns(3, 2, 1), Some(1_500_000_000));
        assert_eq!(cmtime_ns(24_000_000, 24_000_000, 1), Some(NS_PER_S));
        assert_eq!(cmtime_ns(5, 1, 0), None, "invalid flag");
        assert_eq!(cmtime_ns(5, 0, 1), None, "zero timescale");
        // Host-clock sized values (days of uptime at 1e9) must not overflow.
        assert_eq!(cmtime_ns(400_000 * NS_PER_S, 1_000_000_000, 1), Some(400_000 * NS_PER_S));
    }

    #[test]
    fn offset_converts_to_the_samples_own_timescale_with_rounding() {
        assert_eq!(ns_to_ticks(1_000_000_000, 48_000), 48_000);
        assert_eq!(ns_to_ticks(100_000_000, 600), 60);
        // 1 ns at 48 kHz rounds to 0, 15 µs (0.72 tick) rounds to 1.
        assert_eq!(ns_to_ticks(1, 48_000), 0);
        assert_eq!(ns_to_ticks(15_000, 48_000), 1);
        assert_eq!(ns_to_ticks(-15_000, 48_000), -1);
        assert_eq!(ns_to_ticks(5, 0), 0);
    }

    #[test]
    fn retime_moves_valid_times_back_and_leaves_invalid_ones() {
        // 10 s at 1e9 minus a 250 ms pause.
        assert_eq!(retime_value(10 * NS_PER_S, 1_000_000_000, 1, 250_000_000), 9_750_000_000);
        // Audio at 48 kHz: 100 ms = 4800 ticks.
        assert_eq!(retime_value(480_000, 48_000, 1, 100_000_000), 475_200);
        // Invalid DTS (flags 0) is not touched.
        assert_eq!(retime_value(7, 1_000_000_000, 0, 250_000_000), 7);
        assert_eq!(retime_value(7, 1_000_000_000, 1, 0), 7);
    }

    #[test]
    fn audio_waits_for_the_session_and_never_goes_backwards() {
        let mut g = AudioGate::default();
        assert!(!g.admit(100, None), "no session yet");
        assert!(!g.admit(99, Some(100)), "before the first video frame");
        assert!(g.admit(100, Some(100)));
        g.record(100);
        assert!(!g.admit(100, Some(100)), "same PTS again");
        assert!(!g.admit(50, Some(100)), "late buffer from before a resume");
        assert!(g.admit(101, Some(100)));
        assert_eq!(g.dropped, 4);
    }

    #[test]
    fn audio_gate_counts_only_after_record() {
        let mut g = AudioGate::default();
        assert!(g.admit(200, Some(100)));
        // Not recorded (append failed / not ready): an earlier one may follow.
        assert!(g.admit(150, Some(100)));
    }

    #[test]
    fn duration_runs_from_first_frame_to_session_end() {
        assert_eq!(duration_ms(Some(1_000_000_000), 4_500_000_000), 3_500);
        assert_eq!(duration_ms(None, 4_500_000_000), 0);
        assert_eq!(duration_ms(Some(5), 1), 0, "never negative");
    }

    #[test]
    fn fps_is_clamped_to_1_through_60() {
        assert_eq!(clamp_fps(0), 1);
        assert_eq!(clamp_fps(30), 30);
        assert_eq!(clamp_fps(60), 60);
        assert_eq!(clamp_fps(120), 60);
    }

    #[test]
    fn pixel_format_is_420v() {
        assert_eq!(PIXEL_420V, 0x3432_3076);
    }
}

/// `cargo test -p inspector-rust-core --lib capture_live_with_pauses -- --ignored --nocapture`
/// Needs Screen Recording for the test binary's responsible process.
#[cfg(test)]
mod live {
    use super::*;
    use std::process::Command;

    #[test]
    #[ignore]
    fn capture_live_with_pauses() {
        let dir = std::env::temp_dir().join(format!("ir-capture-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("screen.mp4");
        let clock = Arc::new(PauseShared::new());
        let filter = display_filter(main_display_id()).expect("filter");
        let cap = Capture::start(
            filter,
            CaptureOptions { system_audio: true, show_system_cursor: true, fps: 60 },
            &out,
            clock.clone(),
        )
        .expect("start");
        println!("size {:?}, bounds {:?}", cap.size(), cap.content_bounds_pt());
        let origin = cap.wait_for_first_frame(Duration::from_secs(5)).expect("first frame");
        println!("origin_ns {origin}");
        // 4 s wall clock containing 5 pauses of 100 ms each.
        let t0 = std::time::Instant::now();
        for _ in 0..5 {
            std::thread::sleep(Duration::from_millis(600));
            clock.pause(host_now_ns());
            std::thread::sleep(Duration::from_millis(100));
            clock.resume(host_now_ns());
        }
        let rest = Duration::from_secs(4).saturating_sub(t0.elapsed());
        std::thread::sleep(rest);
        let res = cap.stop().expect("stop");
        println!("result {res:?}");
        println!("rejected_frames {}", res.rejected_frames);

        let probe = crate::ffmpeg_util::ffprobe_path().expect("ffprobe");
        let o = Command::new(probe)
            .args(["-v", "error", "-show_entries", "stream=codec_type,codec_name,sample_rate,channels,width,height,duration", "-show_entries", "format=duration", "-of", "compact"])
            .arg(&out)
            .output()
            .expect("ffprobe run");
        let text = String::from_utf8_lossy(&o.stdout).to_string();
        println!("{text}");
        assert!(text.contains("codec_name=h264"), "H.264 video");
        assert!(text.contains("codec_name=aac") && text.contains("sample_rate=48000"), "AAC 48 kHz");
        let fmt_dur: f64 = text
            .lines()
            .find(|l| l.starts_with("format"))
            .and_then(|l| l.split("duration=").nth(1))
            .and_then(|d| d.trim().parse().ok())
            .expect("format duration");
        assert!((fmt_dur - 3.5).abs() <= 0.3, "duration {fmt_dur} not ≈ 3.5 s");
        assert!((res.duration_ms as f64 / 1000.0 - 3.5).abs() <= 0.3);

        // A/V alignment from the packets themselves: if pause holes collapsed
        // (audio stored back to back), the audio would end early.
        let last_packet = |sel: &str| -> (f64, f64) {
            let o = Command::new(crate::ffmpeg_util::ffprobe_path().expect("ffprobe"))
                .args(["-v", "error", "-select_streams", sel, "-show_entries", "packet=pts_time,duration_time", "-of", "csv=p=0"])
                .arg(&out)
                .output()
                .expect("ffprobe packets");
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            let mut best = (f64::MIN, 0.0);
            for l in text.lines() {
                let mut it = l.split(',');
                let pts: f64 = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(f64::MIN);
                let dur: f64 = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(0.0);
                if pts > best.0 {
                    best = (pts, dur);
                }
            }
            best
        };
        let (v_pts, _) = last_packet("v:0");
        let (a_pts, a_dur) = last_packet("a:0");
        let a_end = a_pts + a_dur;
        println!("last video packet pts {v_pts:.3} s, last audio packet end {a_end:.3} s, diff {:.3} s", a_end - v_pts);
        assert!((a_end - v_pts).abs() < 0.15, "audio end {a_end} vs video {v_pts}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
