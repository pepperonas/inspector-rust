//! What this Mac is playing, captured digitally (ScreenCaptureKit audio).
//!
//! Used by `transcribe --start --song`: a song played on this Mac reaches
//! Whisper clean instead of through speakers, room and microphone — and the
//! music never has to be turned down. Needs the Screen Recording grant (the
//! same one OCR and screenshots use). Our own process is excluded.
//!
//! Pure core: [`to_mono`] (interleaved or planar float frames → mono).

/// Mix `channels` channels of `frames` float samples down to mono.
/// `planar`: all of channel 0, then all of channel 1 … (ScreenCaptureKit's
/// non-interleaved layout); otherwise L R L R … A short buffer yields only
/// the frames it fully contains. Pure.
pub fn to_mono(data: &[f32], channels: usize, frames: usize, planar: bool) -> Vec<f32> {
    if channels == 0 {
        return Vec::new();
    }
    let frames = frames.min(data.len() / channels);
    let inv = 1.0 / channels as f32;
    (0..frames)
        .map(|i| {
            let sum: f32 = (0..channels)
                .map(|c| if planar { data[c * frames + i] } else { data[i * channels + c] })
                .sum();
            sum * inv
        })
        .collect()
}

#[cfg(target_os = "macos")]
pub use macos::{start, SystemAudio};

#[cfg(target_os = "macos")]
mod macos {
    use std::os::raw::c_void;
    use std::sync::mpsc;
    use std::sync::OnceLock;
    use std::time::Duration;

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
    unsafe impl<T> Send for Sendable<T> {}
    unsafe impl<T> Sync for Sendable<T> {}

    type Sink = Box<dyn Fn(&[f32], u32) + Send + Sync>;

    pub struct Ivars {
        sink: Sink,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "IRSystemAudioOutput"]
        #[ivars = Ivars]
        struct AudioOutput;

        unsafe impl NSObjectProtocol for AudioOutput {}

        unsafe impl SCStreamOutput for AudioOutput {
            #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
            fn did_output(&self, _stream: &SCStream, sample: &CMSampleBuffer, ty: SCStreamOutputType) {
                if ty != SCStreamOutputType::Audio {
                    return;
                }
                if let Some((mono, rate)) = unsafe { decode(sample as *const CMSampleBuffer as *const c_void) } {
                    (self.ivars().sink)(&mono, rate);
                }
            }
        }

        unsafe impl SCStreamDelegate for AudioOutput {
            #[unsafe(method(stream:didStopWithError:))]
            fn did_stop(&self, _stream: &SCStream, error: &NSError) {
                tracing::warn!("system_audio: capture stopped: {}", error.localizedDescription());
            }
        }
    );

    impl AudioOutput {
        fn create(sink: Sink) -> Retained<Self> {
            let this = Self::alloc().set_ivars(Ivars { sink });
            unsafe { msg_send![super(this), init] }
        }
    }

    /// Copy the float samples out of an audio sample buffer and mix to mono.
    ///
    /// SAFETY: `sbuf` is a live CMSampleBuffer for the duration of the call
    /// (ScreenCaptureKit holds it while the delegate runs).
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
        let mono = super::to_mono(&data, asbd.channels_per_frame as usize, frames, planar);
        Some((mono, asbd.sample_rate as u32))
    }

    fn queue() -> &'static DispatchQueue {
        static Q: OnceLock<Sendable<DispatchRetained<DispatchQueue>>> = OnceLock::new();
        &Q.get_or_init(|| Sendable(DispatchQueue::new("io.celox.inspector-rust.system-audio", None))).0
    }

    /// A running capture; [`SystemAudio::stop`] (or drop) ends it.
    pub struct SystemAudio {
        stream: Sendable<Retained<SCStream>>,
        _output: Sendable<Retained<AudioOutput>>,
        stopped: bool,
    }

    impl SystemAudio {
        pub fn stop(&mut self) {
            if self.stopped {
                return;
            }
            self.stopped = true;
            let (tx, rx) = mpsc::channel::<()>();
            let done = RcBlock::new(move |_err: *mut NSError| {
                let _ = tx.send(());
            });
            unsafe { self.stream.0.stopCaptureWithCompletionHandler(Some(&done)) };
            let _ = rx.recv_timeout(Duration::from_secs(3));
        }
    }

    impl Drop for SystemAudio {
        fn drop(&mut self) {
            self.stop();
        }
    }

    /// Start capturing everything this Mac plays (all apps except our own),
    /// delivered as mono float chunks with their sample rate.
    pub fn start(on_chunk: impl Fn(&[f32], u32) + Send + Sync + 'static) -> Result<SystemAudio, String> {
        let content = shareable_content()?;
        unsafe {
            let displays = content.0.displays();
            let display = displays
                .firstObject()
                .ok_or_else(|| crate::i18n::t("trx.song_no_display"))?;
            let none: Retained<NSArray<SCWindow>> = NSArray::new();
            let filter = SCContentFilter::initWithDisplay_excludingWindows(SCContentFilter::alloc(), &display, &none);
            let cfg = SCStreamConfiguration::new();
            cfg.setCapturesAudio(true);
            cfg.setExcludesCurrentProcessAudio(true);
            cfg.setSampleRate(48_000);
            cfg.setChannelCount(2);
            // The video part can't be switched off; keep it tiny and rare.
            cfg.setWidth(2);
            cfg.setHeight(2);
            cfg.setMinimumFrameInterval(CMTime::new(1, 1));
            cfg.setShowsCursor(false);

            let out = AudioOutput::create(Box::new(on_chunk));
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
                Ok(None) => Ok(SystemAudio { stream: Sendable(stream), _output: Sendable(out), stopped: false }),
                Ok(Some(e)) => Err(e),
                Err(_) => Err(crate::i18n::t("trx.song_start_hangs")),
            }
        }
    }

    fn shareable_content() -> Result<Sendable<Retained<SCShareableContent>>, String> {
        let (tx, rx) = mpsc::channel::<Result<Sendable<Retained<SCShareableContent>>, String>>();
        let block = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
            let r = if let Some(c) = unsafe { Retained::retain(content) } {
                Ok(Sendable(c))
            } else if let Some(e) = unsafe { err.as_ref() } {
                Err(e.localizedDescription().to_string())
            } else {
                Err(crate::i18n::t("trx.song_no_permission"))
            };
            let _ = tx.send(r);
        });
        unsafe {
            SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
                false, true, &block,
            )
        };
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|_| crate::i18n::t("trx.song_start_hangs"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaved_stereo_averages_each_frame() {
        assert_eq!(to_mono(&[1.0, 0.0, 0.5, 0.5, -1.0, 1.0], 2, 3, false), vec![0.5, 0.5, 0.0]);
    }

    #[test]
    fn planar_stereo_reads_one_plane_after_the_other() {
        // L = [1, 0.5, -1], R = [0, 0.5, 1]
        assert_eq!(to_mono(&[1.0, 0.5, -1.0, 0.0, 0.5, 1.0], 2, 3, true), vec![0.5, 0.5, 0.0]);
    }

    #[test]
    fn mono_passes_through_and_short_buffers_never_overrun() {
        assert_eq!(to_mono(&[0.1, 0.2], 1, 2, true), vec![0.1, 0.2]);
        let short = to_mono(&[0.1, 0.2, 0.3], 2, 9, false);
        assert_eq!(short.len(), 1);
        assert!((short[0] - 0.15).abs() < 1e-6);
        assert!(to_mono(&[0.1], 0, 1, false).is_empty());
    }
}

/// `IR_SONG_DIR=<dir> cargo test -p inspector-rust-core --lib system_audio_live -- --ignored --nocapture`
/// records 20 s (`IR_SONG_SECS`) of what this Mac plays as raw 16 kHz i16 (the format the
/// dictation diagnostics read). Nothing is printed but the level.
#[cfg(all(test, target_os = "macos"))]
mod live {
    #[test]
    #[ignore]
    fn system_audio_live() {
        use std::sync::{Arc, Mutex};
        let pcm: Arc<Mutex<Vec<i16>>> = Arc::default();
        let peak = Arc::new(Mutex::new(0f32));
        let (p2, k2) = (pcm.clone(), peak.clone());
        let mut cap = super::start(move |chunk, rate| {
            let mut k = k2.lock().unwrap();
            *k = k.max(crate::mic_capture::chunk_rms(chunk));
            p2.lock().unwrap().extend(crate::shazam::resample_to_16k_i16(chunk, rate));
        })
        .expect("start");
        let secs = std::env::var("IR_SONG_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(20);
        std::thread::sleep(std::time::Duration::from_secs(secs));
        cap.stop();
        let pcm = pcm.lock().unwrap();
        println!("samples {} (≈{:.1} s), peak rms {:.4}", pcm.len(), pcm.len() as f32 / 16_000.0, peak.lock().unwrap());
        if let Ok(dir) = std::env::var("IR_SONG_DIR") {
            let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
            std::fs::write(std::path::Path::new(&dir).join("song.pcm"), bytes).unwrap();
        }
        assert!(pcm.len() > 16_000 * 15, "too little audio");
    }
}
