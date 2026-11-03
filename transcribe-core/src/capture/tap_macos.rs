//! System audio through a Core Audio process tap (macOS 14.2+): a global,
//! private, unmuted tap of every process, put into a private aggregate device
//! whose IOProc hands us the mixed output. Needs only the "audio capture"
//! permission (`NSAudioCaptureUsageDescription`), no screen recording, and it
//! hears the output whatever the default device is (boom Audio included).
//!
//! Fails over to ScreenCaptureKit when a tap cannot be created.

use super::{AudioInput, Chunk, OnError};
use crate::resample::Resampler;
use block2::RcBlock;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use std::ffi::{c_void, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type AudioObjectID = u32;
type OSStatus = i32;
type CFStringRef = *const c_void;
type AudioDeviceIOProcID = *mut c_void;

const SYSTEM_OBJECT: AudioObjectID = 1;
const fn fourcc(b: &[u8; 4]) -> u32 {
    ((b[0] as u32) << 24) | ((b[1] as u32) << 16) | ((b[2] as u32) << 8) | (b[3] as u32)
}
const SCOPE_GLOBAL: u32 = fourcc(b"glob");
const PROP_DEFAULT_OUTPUT: u32 = fourcc(b"dOut");
const PROP_DEVICE_UID: u32 = fourcc(b"uid ");
const PROP_TAP_UID: u32 = fourcc(b"tuid");
const PROP_TAP_FORMAT: u32 = fourcc(b"tfmt");
const KCF_UTF8: u32 = 0x0800_0100;
/// `CATapMuteBehavior.unmuted`: the user still hears everything.
const UNMUTED: i64 = 0;

#[repr(C)]
struct Addr {
    selector: u32,
    scope: u32,
    element: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
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

#[repr(C)]
struct AudioBuffer {
    channels: u32,
    bytes: u32,
    data: *mut c_void,
}

#[repr(C)]
struct AudioBufferList {
    count: u32,
    buffers: [AudioBuffer; 1],
}

type IOBlock = block2::DynBlock<dyn Fn(*const c_void, *const c_void, *const c_void, *mut c_void, *const c_void)>;

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectGetPropertyData(id: AudioObjectID, a: *const Addr, qs: u32, q: *const c_void, size: *mut u32, out: *mut c_void) -> OSStatus;
    fn AudioHardwareCreateProcessTap(desc: *mut AnyObject, out: *mut AudioObjectID) -> OSStatus;
    fn AudioHardwareDestroyProcessTap(tap: AudioObjectID) -> OSStatus;
    fn AudioHardwareCreateAggregateDevice(dict: *const c_void, out: *mut AudioObjectID) -> OSStatus;
    fn AudioHardwareDestroyAggregateDevice(dev: AudioObjectID) -> OSStatus;
    fn AudioDeviceCreateIOProcIDWithBlock(out: *mut AudioDeviceIOProcID, dev: AudioObjectID, q: *mut c_void, block: &IOBlock) -> OSStatus;
    fn AudioDeviceDestroyIOProcID(dev: AudioObjectID, p: AudioDeviceIOProcID) -> OSStatus;
    fn AudioDeviceStart(dev: AudioObjectID, p: AudioDeviceIOProcID) -> OSStatus;
    fn AudioDeviceStop(dev: AudioObjectID, p: AudioDeviceIOProcID) -> OSStatus;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFAllocatorDefault: *const c_void;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    static kCFTypeArrayCallBacks: c_void;
    static kCFBooleanTrue: *const c_void;
    fn CFStringCreateWithCString(a: *const c_void, s: *const i8, enc: u32) -> CFStringRef;
    fn CFDictionaryCreate(a: *const c_void, k: *const *const c_void, v: *const *const c_void, n: isize, kcb: *const c_void, vcb: *const c_void) -> *const c_void;
    fn CFArrayCreate(a: *const c_void, v: *const *const c_void, n: isize, cb: *const c_void) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

fn cfstr(s: &str) -> CFStringRef {
    let c = CString::new(s).unwrap_or_default();
    // SAFETY: a valid NUL-terminated UTF-8 string; the caller releases the result.
    unsafe { CFStringCreateWithCString(kCFAllocatorDefault, c.as_ptr(), KCF_UTF8) }
}

fn addr(selector: u32) -> Addr {
    Addr { selector, scope: SCOPE_GLOBAL, element: 0 }
}

/// SAFETY: `id` is a live Core Audio object; `T` is the property's C type.
unsafe fn get<T: Default>(id: AudioObjectID, selector: u32) -> Option<T> {
    let mut v = T::default();
    let mut size = std::mem::size_of::<T>() as u32;
    let a = addr(selector);
    (AudioObjectGetPropertyData(id, &a, 0, std::ptr::null(), &mut size, &mut v as *mut T as *mut c_void) == 0).then_some(v)
}

/// A CFString property (retained; the caller releases it).
unsafe fn get_cfstring(id: AudioObjectID, selector: u32) -> Option<CFStringRef> {
    let mut s: CFStringRef = std::ptr::null();
    let mut size = std::mem::size_of::<CFStringRef>() as u32;
    let a = addr(selector);
    let ok = AudioObjectGetPropertyData(id, &a, 0, std::ptr::null(), &mut size, &mut s as *mut CFStringRef as *mut c_void) == 0;
    (ok && !s.is_null()).then_some(s)
}

pub fn default_output() -> AudioObjectID {
    // SAFETY: the system object always exists.
    unsafe { get::<u32>(SYSTEM_OBJECT, PROP_DEFAULT_OUTPUT).unwrap_or(0) }
}

const PROP_TRANSPORT: u32 = fourcc(b"tran");
const PROP_DATA_SOURCE: u32 = fourcc(b"ssrc");
const SCOPE_OUTPUT: u32 = fourcc(b"outp");

/// Transport of the default output as the words `dedupe` understands.
pub fn transport_word(t: u32) -> &'static str {
    match &t.to_be_bytes() {
        b"blue" | b"blea" => "bluetooth",
        b"usb " => "usb",
        b"bltn" => "builtin",
        b"hdmi" | b"dprt" => "hdmi",
        b"virt" => "virtual",
        _ => "other",
    }
}

/// Built-in data source: headphones jack vs. speakers.
pub fn data_source_word(d: u32) -> Option<&'static str> {
    match &d.to_be_bytes() {
        b"hdpn" => Some("Headphones"),
        b"ispk" => Some("Internal Speakers"),
        _ => None,
    }
}

/// Does the default output look like headphones (no speaker echo)?
pub fn headphones_hint() -> Option<bool> {
    let dev = default_output();
    if dev == 0 {
        return None;
    }
    // SAFETY: `dev` is the live default output device.
    let transport = unsafe { get::<u32>(dev, PROP_TRANSPORT)? };
    let ds = unsafe {
        let mut v = 0u32;
        let mut size = 4u32;
        let a = Addr { selector: PROP_DATA_SOURCE, scope: SCOPE_OUTPUT, element: 0 };
        (AudioObjectGetPropertyData(dev, &a, 0, std::ptr::null(), &mut size, &mut v as *mut u32 as *mut c_void) == 0)
            .then_some(v)
    };
    Some(crate::dedupe::headphones_suggest_off(transport_word(transport), ds.and_then(data_source_word)))
}

/// SAFETY: builds and returns a retained CATapDescription (or None).
unsafe fn tap_description() -> Option<*mut AnyObject> {
    let alloc: *mut AnyObject = msg_send![class!(CATapDescription), alloc];
    if alloc.is_null() {
        return None;
    }
    let empty: *mut AnyObject = msg_send![class!(NSArray), array];
    let desc: *mut AnyObject = msg_send![alloc, initStereoGlobalTapButExcludeProcesses: empty];
    if desc.is_null() {
        return None;
    }
    let _: () = msg_send![desc, setMuteBehavior: UNMUTED];
    let _: () = msg_send![desc, setPrivate: true];
    let name = cfstr("InspectorRust Transkription");
    let _: () = msg_send![desc, setName: name as *mut AnyObject];
    CFRelease(name);
    Some(desc)
}

/// SAFETY: `tap_uid` and `out_uid` are valid CFStrings.
unsafe fn aggregate(tap_uid: CFStringRef, out_uid: CFStringRef) -> Option<AudioObjectID> {
    let k_uid = cfstr("uid");
    let k_drift = cfstr("drift");
    let sub_tap = {
        let k = [k_uid, k_drift];
        let v = [tap_uid, kCFBooleanTrue];
        CFDictionaryCreate(kCFAllocatorDefault, k.as_ptr(), v.as_ptr(), 2, &kCFTypeDictionaryKeyCallBacks as *const _, &kCFTypeDictionaryValueCallBacks as *const _)
    };
    let sub_dev = {
        let k = [k_uid];
        let v = [out_uid];
        CFDictionaryCreate(kCFAllocatorDefault, k.as_ptr(), v.as_ptr(), 1, &kCFTypeDictionaryKeyCallBacks as *const _, &kCFTypeDictionaryValueCallBacks as *const _)
    };
    let taps = CFArrayCreate(kCFAllocatorDefault, [sub_tap].as_ptr(), 1, &kCFTypeArrayCallBacks as *const _);
    let subs = CFArrayCreate(kCFAllocatorDefault, [sub_dev].as_ptr(), 1, &kCFTypeArrayCallBacks as *const _);
    let uid = cfstr(&format!("io.celox.inspector-rust.transcribe.{}", std::process::id()));
    let name = cfstr("InspectorRust Transkription");
    let keys = [cfstr("uid"), cfstr("name"), cfstr("private"), cfstr("taps"), cfstr("subdevices"), cfstr("master"), cfstr("tapautostart")];
    let vals = [uid, name, kCFBooleanTrue, taps, subs, out_uid, kCFBooleanTrue];
    let dict = CFDictionaryCreate(kCFAllocatorDefault, keys.as_ptr(), vals.as_ptr(), keys.len() as isize, &kCFTypeDictionaryKeyCallBacks as *const _, &kCFTypeDictionaryValueCallBacks as *const _);
    let mut dev = 0;
    let err = AudioHardwareCreateAggregateDevice(dict, &mut dev);
    for r in [sub_tap, sub_dev, taps, subs, dict, uid, name, k_uid, k_drift] {
        CFRelease(r);
    }
    for k in keys {
        CFRelease(k);
    }
    if err == 0 && dev != 0 {
        Some(dev)
    } else {
        tracing::warn!("tap: aggregate device failed ({err})");
        None
    }
}

/// Mono samples from one IOProc buffer list (interleaved or planar f32).
/// SAFETY: `list` points at a valid AudioBufferList of f32 buffers.
unsafe fn mono_from(list: *const AudioBufferList, out: &mut Vec<f32>) {
    let l = &*list;
    let n = l.count as usize;
    let bufs = std::slice::from_raw_parts(l.buffers.as_ptr(), n);
    if n == 1 {
        let b = &bufs[0];
        let ch = b.channels.max(1) as usize;
        let data = std::slice::from_raw_parts(b.data as *const f32, b.bytes as usize / 4);
        out.extend(data.chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32));
    } else if n > 1 {
        let planes: Vec<&[f32]> = bufs
            .iter()
            .map(|b| std::slice::from_raw_parts(b.data as *const f32, b.bytes as usize / 4))
            .collect();
        let frames = planes.iter().map(|p| p.len()).min().unwrap_or(0);
        out.extend((0..frames).map(|i| planes.iter().map(|p| p[i]).sum::<f32>() / n as f32));
    }
}

type Block = RcBlock<dyn Fn(*const c_void, *const c_void, *const c_void, *mut c_void, *const c_void)>;

/// The IOProc block, moved to the thread that owns the IOProc.
struct SendBlock(#[allow(dead_code)] Block);
// SAFETY: the block is only kept alive on that thread, never called from Rust.
unsafe impl Send for SendBlock {}

struct Handles {
    tap: AudioObjectID,
    agg: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
}
// SAFETY: plain Core Audio object IDs, valid from any thread.
unsafe impl Send for Handles {}

pub struct TapInput {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    warning: Option<String>,
}

impl AudioInput for TapInput {
    fn stop(mut self: Box<Self>) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
    fn warning(&self) -> Option<String> {
        self.warning.clone()
    }
}

/// SAFETY: tears down what `open` built, in reverse order.
unsafe fn close(h: &Handles) {
    AudioDeviceStop(h.agg, h.proc_id);
    AudioDeviceDestroyIOProcID(h.agg, h.proc_id);
    AudioHardwareDestroyAggregateDevice(h.agg);
    AudioHardwareDestroyProcessTap(h.tap);
}

type Buf = Arc<Mutex<Vec<f32>>>;

/// SAFETY: plain Core Audio calls; every created object is destroyed on error.
unsafe fn open(buf: Buf) -> Result<(Handles, f64, Block), String> {
    let desc = tap_description().ok_or("tap: CATapDescription unavailable")?;
    let mut tap = 0;
    let err = AudioHardwareCreateProcessTap(desc, &mut tap);
    let _: () = msg_send![desc, release];
    if err != 0 || tap == 0 {
        return Err(format!("tap: create failed ({err})"));
    }
    let fmt: Asbd = get(tap, PROP_TAP_FORMAT).unwrap_or_default();
    let rate = if fmt.sample_rate > 0.0 { fmt.sample_rate } else { 48_000.0 };
    let (Some(tap_uid), Some(out_uid)) = (get_cfstring(tap, PROP_TAP_UID), get_cfstring(default_output(), PROP_DEVICE_UID)) else {
        AudioHardwareDestroyProcessTap(tap);
        return Err("tap: no device UID".into());
    };
    let agg = aggregate(tap_uid, out_uid);
    CFRelease(tap_uid);
    CFRelease(out_uid);
    let Some(agg) = agg else {
        AudioHardwareDestroyProcessTap(tap);
        return Err("tap: aggregate device failed".into());
    };
    let block = RcBlock::new(move |_now: *const c_void, input: *const c_void, _t: *const c_void, _out: *mut c_void, _t2: *const c_void| {
        if input.is_null() {
            return;
        }
        // Realtime thread: never block; a contended moment drops one buffer.
        if let Ok(mut b) = buf.try_lock() {
            // SAFETY: Core Audio hands a valid f32 buffer list for the duration of the call.
            unsafe { mono_from(input as *const AudioBufferList, &mut b) };
        }
    });
    let mut proc_id: AudioDeviceIOProcID = std::ptr::null_mut();
    if AudioDeviceCreateIOProcIDWithBlock(&mut proc_id, agg, std::ptr::null_mut(), &block) != 0 || proc_id.is_null() {
        AudioHardwareDestroyAggregateDevice(agg);
        AudioHardwareDestroyProcessTap(tap);
        return Err("tap: IOProc failed".into());
    }
    let h = Handles { tap, agg, proc_id };
    if AudioDeviceStart(agg, proc_id) != 0 {
        close(&h);
        return Err("tap: start failed".into());
    }
    Ok((h, rate, block))
}

/// Process tap, else ScreenCaptureKit (with a warning on the input).
pub fn start(on_chunk: Chunk, on_error: OnError) -> Result<Box<dyn AudioInput>, String> {
    let cell: Arc<Mutex<Option<(Chunk, OnError)>>> = Arc::new(Mutex::new(Some((on_chunk, on_error))));
    match start_tap(cell.clone()) {
        Ok(h) => Ok(h),
        Err(e) => {
            tracing::warn!("{e} — falling back to ScreenCaptureKit");
            let (c, err) = cell.lock().unwrap_or_else(|p| p.into_inner()).take().ok_or("tap: callbacks gone")?;
            let mut input = super::sck_macos::start(c, err)?;
            input.set_warning("sys.fallback_sck");
            Ok(input)
        }
    }
}

fn start_tap(cell: Arc<Mutex<Option<(Chunk, OnError)>>>) -> Result<Box<dyn AudioInput>, String> {
    let buf: Buf = Arc::new(Mutex::new(Vec::new()));
    // SAFETY: see `open`.
    let (handles, rate, block) = unsafe { open(buf.clone())? };
    let (mut on_chunk, mut on_error) = cell.lock().unwrap_or_else(|p| p.into_inner()).take().ok_or("tap: callbacks gone")?;
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let device = default_output();
    let block = SendBlock(block);
    let thread = std::thread::Builder::new()
        .name("ir-transcribe-tap".into())
        .spawn(move || {
            let _keep = block; // the IOProc block must outlive the IOProc
            let mut rs = Resampler::new(rate.round() as u32, 1);
            let mut last_check = Instant::now();
            while !stop2.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(40));
                let chunk = std::mem::take(&mut *buf.lock().unwrap_or_else(|e| e.into_inner()));
                if !chunk.is_empty() {
                    let out = rs.push(&chunk);
                    if !out.is_empty() {
                        on_chunk(&out);
                    }
                }
                if last_check.elapsed() >= Duration::from_secs(1) {
                    last_check = Instant::now();
                    if default_output() != device {
                        // The aggregate is bound to the old output: rebuild via restart.
                        // SAFETY: handles are live until here.
                        unsafe { close(&handles) };
                        on_error("sys.output_changed".into());
                        return;
                    }
                }
            }
            // SAFETY: handles are live until here.
            unsafe { close(&handles) };
        })
        .map_err(|e| e.to_string())?;
    Ok(Box::new(TapInput { stop, thread: Some(thread), warning: None }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_and_data_source_words() {
        assert_eq!(transport_word(fourcc(b"blue")), "bluetooth");
        assert_eq!(transport_word(fourcc(b"bltn")), "builtin");
        assert_eq!(data_source_word(fourcc(b"hdpn")), Some("Headphones"));
        assert_eq!(data_source_word(fourcc(b"xxxx")), None);
    }

    #[test]
    #[ignore = "needs audio capture permission; plays a sentence with `say`"]
    fn tap_live_hears_say() {
        let got = Arc::new(Mutex::new(Vec::<f32>::new()));
        let g = got.clone();
        let h = start(Box::new(move |s| g.lock().unwrap().extend_from_slice(s)), Box::new(|e| eprintln!("err {e}"))).unwrap();
        eprintln!("warning: {:?}", h.warning());
        std::process::Command::new("say").arg("test eins zwei drei").status().unwrap();
        std::thread::sleep(Duration::from_millis(500));
        h.stop();
        let s = got.lock().unwrap();
        let rms = (s.iter().map(|x| x * x).sum::<f32>() / s.len().max(1) as f32).sqrt();
        eprintln!("{} samples, rms {rms}", s.len());
        assert!(rms > 0.003);
    }
}
