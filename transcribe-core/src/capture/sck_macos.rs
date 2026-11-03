//! System audio through ScreenCaptureKit — the fallback when no process tap
//! can be created (macOS < 14.2). Needs the screen recording permission.
//! Ported from the app's `system_audio.rs` (same delegate, own class name).

use super::{AudioInput, Chunk, OnError};
use crate::resample::Resampler;
use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_foundation::{NSArray, NSError};
use objc2_screen_capture_kit::{
    SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate, SCStreamOutput,
    SCStreamOutputType, SCWindow,
};
use std::os::raw::c_void;
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[repr(C)]
struct Asbd {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}
const FLAG_FLOAT: u32 = 1;
const FLAG_NON_INTERLEAVED: u32 = 1 << 5;

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    fn CMSampleBufferGetDataBuffer(sbuf: *const c_void) -> *const c_void;
    fn CMSampleBufferGetFormatDescription(sbuf: *const c_void) -> *const c_void;
    fn CMSampleBufferGetNumSamples(sbuf: *const c_void) -> isize;
    fn CMAudioFormatDescriptionGetStreamBasicDescription(desc: *const c_void) -> *const Asbd;
    fn CMBlockBufferGetDataLength(bb: *const c_void) -> usize;
    fn CMBlockBufferCopyDataBytes(bb: *const c_void, offset: usize, len: usize, dest: *mut c_void) -> i32;
}

struct Sendable<T>(T);
// SAFETY: the wrapped Objective-C objects are only used through thread-safe APIs.
unsafe impl<T> Send for Sendable<T> {}
// SAFETY: as above.
unsafe impl<T> Sync for Sendable<T> {}

/// Downmix interleaved or planar frames to mono.
pub fn to_mono(data: &[f32], channels: usize, frames: usize, planar: bool) -> Vec<f32> {
    if channels == 0 {
        return Vec::new();
    }
    let frames = frames.min(data.len() / channels);
    (0..frames)
        .map(|i| {
            (0..channels)
                .map(|c| if planar { data[c * frames + i] } else { data[i * channels + c] })
                .sum::<f32>()
                / channels as f32
        })
        .collect()
}

struct State {
    chunk: Chunk,
    error: OnError,
    rs: Option<Resampler>,
}

pub struct Ivars {
    state: Mutex<State>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "IRTranscribeSysOutput"]
    #[ivars = Ivars]
    struct Output;

    unsafe impl NSObjectProtocol for Output {}

    unsafe impl SCStreamOutput for Output {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn did_output(&self, _stream: &SCStream, sample: &CMSampleBuffer, ty: SCStreamOutputType) {
            if ty != SCStreamOutputType::Audio {
                return;
            }
            // SAFETY: a valid sample buffer for the duration of the call.
            if let Some((mono, rate)) = unsafe { decode(sample as *const CMSampleBuffer as *const c_void) } {
                let mut st = self.ivars().state.lock().unwrap_or_else(|e| e.into_inner());
                let rs = st.rs.get_or_insert_with(|| Resampler::new(rate, 1));
                let out = rs.push(&mono);
                if !out.is_empty() {
                    (st.chunk)(&out);
                }
            }
        }
    }

    unsafe impl SCStreamDelegate for Output {
        #[unsafe(method(stream:didStopWithError:))]
        fn did_stop(&self, _stream: &SCStream, error: &NSError) {
            let msg = error.localizedDescription().to_string();
            let mut st = self.ivars().state.lock().unwrap_or_else(|e| e.into_inner());
            (st.error)(format!("sck: {msg}"));
        }
    }
);

impl Output {
    fn create(chunk: Chunk, error: OnError) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars { state: Mutex::new(State { chunk, error, rs: None }) });
        // SAFETY: NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

/// SAFETY: `sbuf` is a valid CMSampleBuffer.
unsafe fn decode(sbuf: *const c_void) -> Option<(Vec<f32>, u32)> {
    let desc = CMSampleBufferGetFormatDescription(sbuf);
    if desc.is_null() {
        return None;
    }
    let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(desc).as_ref()?;
    if asbd.format_flags & FLAG_FLOAT == 0 || asbd.bits_per_channel != 32 {
        return None;
    }
    let bb = CMSampleBufferGetDataBuffer(sbuf);
    if bb.is_null() {
        return None;
    }
    let len = CMBlockBufferGetDataLength(bb);
    let mut data = vec![0f32; len / 4];
    if CMBlockBufferCopyDataBytes(bb, 0, data.len() * 4, data.as_mut_ptr().cast()) != 0 {
        return None;
    }
    let frames = CMSampleBufferGetNumSamples(sbuf).max(0) as usize;
    let planar = asbd.format_flags & FLAG_NON_INTERLEAVED != 0;
    Some((to_mono(&data, asbd.channels_per_frame as usize, frames, planar), asbd.sample_rate as u32))
}

fn queue() -> &'static DispatchQueue {
    static Q: OnceLock<Sendable<DispatchRetained<DispatchQueue>>> = OnceLock::new();
    &Q.get_or_init(|| Sendable(DispatchQueue::new("io.celox.inspector-rust.transcribe-sck", None))).0
}

pub struct SckInput {
    stream: Sendable<Retained<SCStream>>,
    _output: Sendable<Retained<Output>>,
    warning: Option<String>,
}

impl SckInput {
    pub fn set_warning(&mut self, w: &str) {
        self.warning = Some(w.to_string());
    }
}

impl AudioInput for SckInput {
    fn stop(self: Box<Self>) {
        let (tx, rx) = mpsc::channel::<()>();
        let done = RcBlock::new(move |_err: *mut NSError| {
            let _ = tx.send(());
        });
        // SAFETY: a started stream; the completion block outlives the call via the channel wait.
        unsafe { self.stream.0.stopCaptureWithCompletionHandler(Some(&done)) };
        let _ = rx.recv_timeout(Duration::from_secs(3));
    }
    fn warning(&self) -> Option<String> {
        self.warning.clone()
    }
}

fn shareable_content() -> Result<Sendable<Retained<SCShareableContent>>, String> {
    let (tx, rx) = mpsc::channel::<Result<Sendable<Retained<SCShareableContent>>, String>>();
    let block = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        // SAFETY: ScreenCaptureKit passes either a content object or an error.
        let r = if let Some(c) = unsafe { Retained::retain(content) } {
            Ok(Sendable(c))
        } else if let Some(e) = unsafe { err.as_ref() } {
            Err(e.localizedDescription().to_string())
        } else {
            Err("sys.no_permission".into())
        };
        let _ = tx.send(r);
    });
    // SAFETY: plain asynchronous ScreenCaptureKit call.
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(false, true, &block)
    };
    rx.recv_timeout(Duration::from_secs(5)).map_err(|_| "sys.start_hangs".to_string())?
}

pub fn start(chunk: Chunk, error: OnError) -> Result<Box<SckInput>, String> {
    let content = shareable_content()?;
    // SAFETY: standard ScreenCaptureKit setup, as in the app's `system_audio.rs`.
    unsafe {
        let display = content.0.displays().firstObject().ok_or("sys.no_display")?;
        let none: Retained<NSArray<SCWindow>> = NSArray::new();
        let filter = SCContentFilter::initWithDisplay_excludingWindows(SCContentFilter::alloc(), &display, &none);
        let cfg = SCStreamConfiguration::new();
        cfg.setCapturesAudio(true);
        cfg.setExcludesCurrentProcessAudio(true);
        cfg.setSampleRate(48_000);
        cfg.setChannelCount(2);
        cfg.setWidth(2);
        cfg.setHeight(2);
        cfg.setMinimumFrameInterval(CMTime::new(1, 1));
        cfg.setShowsCursor(false);
        let out = Output::create(chunk, error);
        let delegate: &ProtocolObject<dyn SCStreamDelegate> = ProtocolObject::from_ref(&*out);
        let stream = SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, &cfg, Some(delegate));
        let output: &ProtocolObject<dyn SCStreamOutput> = ProtocolObject::from_ref(&*out);
        stream
            .addStreamOutput_type_sampleHandlerQueue_error(output, SCStreamOutputType::Audio, Some(queue()))
            .map_err(|e| e.localizedDescription().to_string())?;
        let (tx, rx) = mpsc::channel::<Option<String>>();
        let done = RcBlock::new(move |err: *mut NSError| {
            let _ = tx.send(err.as_ref().map(|e| e.localizedDescription().to_string()));
        });
        stream.startCaptureWithCompletionHandler(Some(&done));
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(None) => Ok(Box::new(SckInput { stream: Sendable(stream), _output: Sendable(out), warning: None })),
            Ok(Some(e)) => Err(e),
            Err(_) => Err("sys.start_hangs".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_mono_handles_both_layouts() {
        assert_eq!(to_mono(&[1.0, 0.0, 1.0, 0.0], 2, 2, false), vec![0.5, 0.5]);
        assert_eq!(to_mono(&[1.0, 1.0, 0.0, 0.0], 2, 2, true), vec![0.5, 0.5]);
        assert!(to_mono(&[1.0], 0, 1, false).is_empty());
    }
}
