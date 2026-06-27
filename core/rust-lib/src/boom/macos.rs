//! macOS boom engine: **system-wide audio EQ via a virtual audio device** (the
//! "boom Audio" `AudioServerPlugIn` in `../../boom-driver/`, a rebranded BlackHole
//! loopback). No process tap, no muting — the only architecture that actually
//! re-outputs processed system audio (see `../../docs/boom-driver-plan.md`).
//!
//! Pipeline: enabling boom routes the **system default output** to "boom Audio"
//! so apps render into it. A **capture IOProc** on boom Audio pushes its loopback
//! input into a lock-free SPSC **ring buffer**; a **playback IOProc** on the real
//! output device pops it, runs the (unit-tested) `DspChain` (pre-amp → EQ → boost
//! → limiter) in place, and outputs. The capture side zeroes boom Audio's own
//! output (no feedback). The saved default output is restored on disable /
//! app-quit (`RunEvent::Exit`).
//!
//! **Single clock rate:** boom Audio's sample rate is matched to the real device
//! at start (`set_device_sample_rate`) — otherwise apps render at boom Audio's
//! rate while we play out at the real rate → slow playback + ring overruns
//! (clicks). A ~30 ms silence cushion is pre-filled for startup + drift slack.
//!
//! Realtime safety: the IOProc closures don't allocate/lock-block — the ring is
//! lock-free and `DspChain` params are read with `try_lock` (a contended tweak
//! passes that block through untouched).
//!
//! **History:** the first driverless attempt used Core-Audio **process taps**
//! (macOS 14.2+) — but those are a *capture* API: re-outputting to the same
//! device fails because muting the source to avoid doubling silences the shared
//! device output (verified across every mute variant). That tap code
//! (`make_tap_description` / `build_aggregate`, the `MUTE_BEHAVIOR` / `DIAG_*`
//! consts) is retained **dead** for reference — hence `#![allow(dead_code)]`.

#![allow(dead_code)]

use super::{BoomConfig, DspChain, BANDS_10};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use parking_lot::Mutex;
use std::ffi::{c_void, CString};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

// ── Realtime diagnostics (written from the audio thread, read by a logger) ───
static CB_COUNT: AtomicU64 = AtomicU64::new(0);
static CB_IN_NULL: AtomicBool = AtomicBool::new(false);
static CB_IN_BYTES: AtomicU32 = AtomicU32::new(0);
static CB_IN_N: AtomicU32 = AtomicU32::new(0);
static CB_IN_CH: AtomicU32 = AtomicU32::new(0);
static CB_IN_RMS_BITS: AtomicU32 = AtomicU32::new(0);
static CB_OUT_N: AtomicU32 = AtomicU32::new(0);
static CB_OUT_BYTES: AtomicU32 = AtomicU32::new(0);
static CB_OUT_CH: AtomicU32 = AtomicU32::new(0);
static CB_OUT_RMS_BITS: AtomicU32 = AtomicU32::new(0);
static CLIP: AtomicBool = AtomicBool::new(false);

/// CATapMuteBehavior: 0 = unmuted (doubles → echo), 1 = muted (also muted our
/// output device → silence), 2 = mutedWhenTapped. The tone probe proved our
/// output reaches the speakers, so the silence under (1) is its device-mute.
/// Try (2): mute the source processes without muting the device we render to.
const MUTE_BEHAVIOR: i64 = 1;

/// Master switch for the live audio engine.
const ENGINE_ENABLED: bool = true; // B3: virtual-driver loopback bridge

/// Diagnostic-silence mode (renders pure silence). Off now that the format is
/// confirmed; the real copy+DSP path runs (with defensive output zeroing so a
/// short copy can never leave garbage → no noise).
const DIAG_SILENCE: bool = false;

/// **Diagnostic test-tone mode.** When true the IOProc ignores the tap and
/// writes a quiet 440 Hz sine to the output — a definitive probe for "does our
/// aggregate output actually reach the speakers?". Safe (a soft beep over your
/// audio, unmuted). If you hear the beep → output routing works (so the muted
/// silence is the tap muting the device); if not → the aggregate isn't driving
/// the hardware output.
const DIAG_TONE: bool = false; // confirmed: beep heard (output reaches speakers)
static TONE_IDX: AtomicU64 = AtomicU64::new(0);

// ── FFI ──────────────────────────────────────────────────────────────────────

type OSStatus = i32;
type AudioObjectID = u32;
type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFArrayRef = *const c_void;
type AudioDeviceIOProcID = *mut c_void;

const SYSTEM_OBJECT: AudioObjectID = 1;
const SCOPE_GLOBAL: u32 = fourcc(b"glob");
const ELEMENT_MAIN: u32 = 0;
const PROP_DEFAULT_OUTPUT: u32 = fourcc(b"dOut"); // kAudioHardwarePropertyDefaultOutputDevice
const PROP_DEVICES: u32 = fourcc(b"dev#"); // kAudioHardwarePropertyDevices
const PROP_DEVICE_UID: u32 = fourcc(b"uid "); // kAudioDevicePropertyDeviceUID
const PROP_TAP_UID: u32 = fourcc(b"tuid"); // kAudioTapPropertyUID
const PROP_NOMINAL_SR: u32 = fourcc(b"nsrt"); // kAudioDevicePropertyNominalSampleRate

const KCF_UTF8: u32 = 0x0800_0100;

const fn fourcc(b: &[u8; 4]) -> u32 {
    ((b[0] as u32) << 24) | ((b[1] as u32) << 16) | ((b[2] as u32) << 8) | (b[3] as u32)
}

#[repr(C)]
struct AudioObjectPropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}
#[repr(C)]
struct AudioBuffer {
    number_channels: u32,
    data_byte_size: u32,
    data: *mut c_void,
}
#[repr(C)]
struct AudioBufferList {
    number_buffers: u32,
    buffers: [AudioBuffer; 1], // variable-length; index by `number_buffers`
}

/// The IOProc block type: `^(const AudioTimeStamp*, const AudioBufferList* in,
/// const AudioTimeStamp*, AudioBufferList* out, const AudioTimeStamp*)`. All args
/// are opaque `c_void` pointers (block2 needs `Encode` args) — cast inside.
type IOBlock =
    block2::DynBlock<dyn Fn(*const c_void, *const c_void, *const c_void, *mut c_void, *const c_void)>;

#[link(name = "CoreAudio", kind = "framework")]
extern "C" {
    fn AudioObjectGetPropertyDataSize(
        id: AudioObjectID,
        addr: *const AudioObjectPropertyAddress,
        qual_size: u32,
        qual: *const c_void,
        out_size: *mut u32,
    ) -> OSStatus;
    fn AudioObjectGetPropertyData(
        id: AudioObjectID,
        addr: *const AudioObjectPropertyAddress,
        qual_size: u32,
        qual: *const c_void,
        io_size: *mut u32,
        out_data: *mut c_void,
    ) -> OSStatus;
    fn AudioObjectSetPropertyData(
        id: AudioObjectID,
        addr: *const AudioObjectPropertyAddress,
        qual_size: u32,
        qual: *const c_void,
        in_size: u32,
        in_data: *const c_void,
    ) -> OSStatus;
    fn AudioHardwareCreateProcessTap(desc: *mut AnyObject, out_tap: *mut AudioObjectID) -> OSStatus;
    fn AudioHardwareDestroyProcessTap(tap: AudioObjectID) -> OSStatus;
    fn AudioHardwareCreateAggregateDevice(dict: CFDictionaryRef, out_dev: *mut AudioObjectID) -> OSStatus;
    fn AudioHardwareDestroyAggregateDevice(dev: AudioObjectID) -> OSStatus;
    fn AudioDeviceCreateIOProcIDWithBlock(
        out_proc: *mut AudioDeviceIOProcID,
        dev: AudioObjectID,
        queue: *mut c_void,
        block: &IOBlock,
    ) -> OSStatus;
    fn AudioDeviceDestroyIOProcID(dev: AudioObjectID, proc: AudioDeviceIOProcID) -> OSStatus;
    fn AudioDeviceStart(dev: AudioObjectID, proc: AudioDeviceIOProcID) -> OSStatus;
    fn AudioDeviceStop(dev: AudioObjectID, proc: AudioDeviceIOProcID) -> OSStatus;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFAllocatorDefault: *const c_void;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    static kCFTypeArrayCallBacks: c_void;
    static kCFBooleanTrue: CFTypeRef;
    fn CFStringCreateWithCString(a: *const c_void, s: *const i8, enc: u32) -> CFStringRef;
    fn CFDictionaryCreate(
        a: *const c_void,
        keys: *const *const c_void,
        vals: *const *const c_void,
        n: isize,
        kcb: *const c_void,
        vcb: *const c_void,
    ) -> CFDictionaryRef;
    fn CFArrayCreate(a: *const c_void, vals: *const *const c_void, n: isize, cb: *const c_void) -> CFArrayRef;
    fn CFEqual(a: *const c_void, b: *const c_void) -> u8;
    fn CFStringGetCString(s: CFStringRef, buf: *mut std::ffi::c_char, size: isize, enc: u32) -> bool;
    fn CFRelease(cf: *const c_void);
}

fn cfstr(s: &str) -> CFStringRef {
    let c = CString::new(s).unwrap();
    unsafe { CFStringCreateWithCString(kCFAllocatorDefault, c.as_ptr(), KCF_UTF8) }
}

fn addr(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress { selector, scope: SCOPE_GLOBAL, element: ELEMENT_MAIN }
}

/// The system default output device saved before we redirect it to our
/// aggregate (restored on stop). 0 = nothing saved.
static SAVED_DEFAULT_OUTPUT: AtomicU32 = AtomicU32::new(0);

unsafe fn set_default_output(dev: AudioObjectID) {
    let a = addr(PROP_DEFAULT_OUTPUT);
    let d = dev;
    let err = AudioObjectSetPropertyData(
        SYSTEM_OBJECT,
        &a,
        0,
        std::ptr::null(),
        std::mem::size_of::<AudioObjectID>() as u32,
        &d as *const _ as *const c_void,
    );
    if err != 0 {
        tracing::warn!("boom: set default output to {dev} failed (err {err})");
    }
}

unsafe fn default_output() -> AudioObjectID {
    let a = addr(PROP_DEFAULT_OUTPUT);
    let mut dev: AudioObjectID = 0;
    let mut size = std::mem::size_of::<AudioObjectID>() as u32;
    AudioObjectGetPropertyData(SYSTEM_OBJECT, &a, 0, std::ptr::null(), &mut size, &mut dev as *mut _ as *mut c_void);
    dev
}

unsafe fn string_prop(obj: AudioObjectID, selector: u32) -> Option<CFStringRef> {
    let a = addr(selector);
    let mut s: CFStringRef = std::ptr::null();
    let mut size = std::mem::size_of::<CFStringRef>() as u32;
    let err = AudioObjectGetPropertyData(obj, &a, 0, std::ptr::null(), &mut size, &mut s as *mut _ as *mut c_void);
    if err == 0 && !s.is_null() {
        Some(s)
    } else {
        None
    }
}

unsafe fn nominal_sample_rate(dev: AudioObjectID) -> f64 {
    let a = addr(PROP_NOMINAL_SR);
    let mut sr: f64 = 0.0;
    let mut size = std::mem::size_of::<f64>() as u32;
    AudioObjectGetPropertyData(dev, &a, 0, std::ptr::null(), &mut size, &mut sr as *mut _ as *mut c_void);
    if sr > 0.0 {
        sr
    } else {
        48000.0
    }
}

// ── Engine ───────────────────────────────────────────────────────────────────

struct Session {
    boom_dev: AudioObjectID,
    real_dev: AudioObjectID,
    cap_proc: AudioDeviceIOProcID,  // capture IOProc on boom Audio
    play_proc: AudioDeviceIOProcID, // playback IOProc on the real device
    // Kept alive for the lifetime of the IOProcs.
    _cap_block: RcBlock<dyn Fn(*const c_void, *const c_void, *const c_void, *mut c_void, *const c_void)>,
    _play_block: RcBlock<dyn Fn(*const c_void, *const c_void, *const c_void, *mut c_void, *const c_void)>,
    _ring: Arc<Ring>,
}

// SAFETY: the CoreAudio object ids are plain integers; the block is only invoked
// on the audio thread and we tear the session down before dropping it. We gate
// all access through a global Mutex.
unsafe impl Send for Session {}

struct Engine {
    session: Option<Session>,
    dsp: Arc<Mutex<DspChain>>,
}

static ENGINE: Mutex<Option<Engine>> = Mutex::new(None);

/// Build the `CATapDescription` (global tap of all processes, UNMUTED for now).
unsafe fn make_tap_description() -> Option<Retained<AnyObject>> {
    let cls = class!(CATapDescription);
    let alloc: *mut AnyObject = msg_send![cls, alloc];
    if alloc.is_null() {
        return None;
    }
    // Empty exclude list → a global tap of every process.
    let empty: *mut AnyObject = msg_send![class!(NSArray), array];
    let desc: *mut AnyObject = msg_send![alloc, initStereoGlobalTapButExcludeProcesses: empty];
    if desc.is_null() {
        return None;
    }
    let _: () = msg_send![desc, setMuteBehavior: MUTE_BEHAVIOR];
    let _: () = msg_send![desc, setPrivate: true];
    let name = cfstr("InspectorRust boom");
    let _: () = msg_send![desc, setName: name as *mut AnyObject];
    CFRelease(name);
    Retained::from_raw(desc)
}

unsafe fn build_aggregate(tap_uid: CFStringRef, out_uid: CFStringRef) -> Option<AudioObjectID> {
    // Sub-tap dict: { "uid": tapUID }
    let key_uid = cfstr("uid");
    let sub_tap = {
        let keys = [key_uid];
        let vals = [tap_uid];
        CFDictionaryCreate(
            kCFAllocatorDefault,
            keys.as_ptr(),
            vals.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        )
    };
    let sub_dev = {
        let keys = [key_uid];
        let vals = [out_uid];
        CFDictionaryCreate(
            kCFAllocatorDefault,
            keys.as_ptr(),
            vals.as_ptr(),
            1,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        )
    };
    let tap_list = {
        let v = [sub_tap];
        CFArrayCreate(kCFAllocatorDefault, v.as_ptr(), 1, &kCFTypeArrayCallBacks)
    };
    let dev_list = {
        let v = [sub_dev];
        CFArrayCreate(kCFAllocatorDefault, v.as_ptr(), 1, &kCFTypeArrayCallBacks)
    };

    let k_uid = cfstr("uid");
    let k_name = cfstr("name");
    let k_priv = cfstr("private");
    let k_taps = cfstr("taps");
    let k_subs = cfstr("subdevices");
    let k_main = cfstr("master");
    let k_autostart = cfstr("tapautostart"); // feed the tap into the IOProc
    let v_uid = cfstr("io.celox.inspector-rust.boom");
    let v_name = cfstr("InspectorRust boom");

    let keys = [k_uid, k_name, k_priv, k_taps, k_subs, k_main, k_autostart];
    let vals = [v_uid, v_name, kCFBooleanTrue, tap_list, dev_list, out_uid, kCFBooleanTrue];
    let dict = CFDictionaryCreate(
        kCFAllocatorDefault,
        keys.as_ptr(),
        vals.as_ptr(),
        keys.len() as isize,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks,
    );

    let mut agg: AudioObjectID = 0;
    let err = AudioHardwareCreateAggregateDevice(dict, &mut agg);

    // Release everything we created (the aggregate retains what it needs).
    for r in [
        sub_tap, sub_dev, tap_list, dev_list, dict, k_uid, k_name, k_priv, k_taps, k_subs, k_main,
        k_autostart, v_uid, v_name, key_uid,
    ] {
        if !r.is_null() {
            CFRelease(r);
        }
    }

    if err == 0 && agg != 0 {
        Some(agg)
    } else {
        tracing::warn!("boom: aggregate device creation failed (err {err})");
        None
    }
}

// ── Lock-free SPSC ring buffer (capture IOProc → playback IOProc) ────────────
struct Ring {
    buf: *mut f32,
    cap: usize,
    mask: usize,
    write: AtomicUsize,
    read: AtomicUsize,
}
// SAFETY: single-producer (capture) / single-consumer (playback); each side only
// advances its own atomic index.
unsafe impl Send for Ring {}
unsafe impl Sync for Ring {}
impl Ring {
    fn new(cap: usize) -> Arc<Ring> {
        let b = vec![0f32; cap].into_boxed_slice();
        let buf = Box::into_raw(b) as *mut f32;
        Arc::new(Ring { buf, cap, mask: cap - 1, write: AtomicUsize::new(0), read: AtomicUsize::new(0) })
    }
    #[inline]
    unsafe fn push(&self, src: &[f32]) {
        let r = self.read.load(Ordering::Acquire);
        let mut w = self.write.load(Ordering::Relaxed);
        for &val in src {
            if w.wrapping_sub(r) >= self.cap {
                break; // full → drop (overrun); playback catches up
            }
            *self.buf.add(w & self.mask) = val;
            w = w.wrapping_add(1);
        }
        self.write.store(w, Ordering::Release);
    }
    #[inline]
    unsafe fn pop_into(&self, dst: &mut [f32]) {
        let w = self.write.load(Ordering::Acquire);
        let r0 = self.read.load(Ordering::Relaxed);
        let avail = w.wrapping_sub(r0);
        let count = dst.len().min(avail);
        for (i, d) in dst.iter_mut().enumerate() {
            *d = if i < count { *self.buf.add((r0 + i) & self.mask) } else { 0.0 };
        }
        self.read.store(r0.wrapping_add(count), Ordering::Release);
    }
}
impl Drop for Ring {
    fn drop(&mut self) {
        unsafe {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(self.buf, self.cap)));
        }
    }
}

unsafe fn cfstring_to_string(s: CFStringRef) -> Option<String> {
    let mut buf = [0 as std::ffi::c_char; 256];
    if CFStringGetCString(s, buf.as_mut_ptr(), 256, KCF_UTF8) {
        Some(std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
    } else {
        None
    }
}

/// A device's display name (for diagnostics).
unsafe fn device_name(dev: AudioObjectID) -> String {
    match string_prop(dev, fourcc(b"lnam")) {
        Some(cf) => {
            let s = cfstring_to_string(cf).unwrap_or_default();
            CFRelease(cf);
            s
        }
        None => String::new(),
    }
}

const PROP_STREAMS: u32 = fourcc(b"stm#"); // kAudioDevicePropertyStreams
const SCOPE_OUTPUT: u32 = fourcc(b"outp"); // kAudioObjectPropertyScopeOutput
const PROP_TRANSPORT: u32 = fourcc(b"tran"); // kAudioDevicePropertyTransportType
const TRANSPORT_VIRTUAL: u32 = fourcc(b"virt"); // kAudioDeviceTransportTypeVirtual
const TRANSPORT_BUILTIN: u32 = fourcc(b"bltn"); // kAudioDeviceTransportTypeBuiltIn

fn addr_scope(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress { selector, scope, element: ELEMENT_MAIN }
}

unsafe fn device_has_output(dev: AudioObjectID) -> bool {
    let a = addr_scope(PROP_STREAMS, SCOPE_OUTPUT);
    let mut size = 0u32;
    AudioObjectGetPropertyDataSize(dev, &a, 0, std::ptr::null(), &mut size) == 0 && size > 0
}

unsafe fn device_transport(dev: AudioObjectID) -> u32 {
    let a = addr(PROP_TRANSPORT);
    let mut t = 0u32;
    let mut size = 4u32;
    AudioObjectGetPropertyData(dev, &a, 0, std::ptr::null(), &mut size, &mut t as *mut _ as *mut c_void);
    t
}

/// Pick a real (non-virtual) output device, excluding `exclude` (boom Audio) —
/// used when the default output is stale-stuck on boom Audio itself.
unsafe fn pick_real_output(exclude: AudioObjectID) -> AudioObjectID {
    let a = addr(PROP_DEVICES);
    let mut size = 0u32;
    if AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &a, 0, std::ptr::null(), &mut size) != 0 {
        return 0;
    }
    let count = size as usize / std::mem::size_of::<AudioObjectID>();
    let mut ids = vec![0u32; count];
    if AudioObjectGetPropertyData(SYSTEM_OBJECT, &a, 0, std::ptr::null(), &mut size, ids.as_mut_ptr() as *mut c_void) != 0 {
        return 0;
    }
    // Prefer the built-in output (the safe, always-present fallback); else any
    // non-virtual output. Never an arbitrary monitor when built-in exists.
    let mut fallback = 0;
    for &id in &ids {
        if id == exclude || !device_has_output(id) {
            continue;
        }
        let t = device_transport(id);
        if t == TRANSPORT_VIRTUAL {
            continue;
        }
        if t == TRANSPORT_BUILTIN {
            return id;
        }
        if fallback == 0 {
            fallback = id;
        }
    }
    fallback
}

/// If the default output is stuck on boom Audio (e.g. after an unclean exit
/// while boom was on), reset it to a real device so audio isn't silent — the
/// built-in output (safe, always present), never an arbitrary monitor. When
/// boom is toggled normally (from off), the default is already the user's real
/// device, so this is only a crash-recovery safety net.
pub(crate) fn reset_stale_default() {
    unsafe {
        let boom_dev = find_device_by_uid("BoomAudio_UID");
        if boom_dev == 0 || default_output() != boom_dev {
            return;
        }
        let target = pick_real_output(boom_dev);
        if target != 0 {
            set_default_output(target);
            tracing::info!("boom: reset stale default → {target}");
        }
    }
}

/// Set a device's nominal sample rate (used to match boom Audio to the real
/// output so the bridge runs at one rate — no resampling, no speed change).
unsafe fn set_device_sample_rate(dev: AudioObjectID, sr: f64) {
    let a = addr(PROP_NOMINAL_SR);
    let v = sr;
    let err = AudioObjectSetPropertyData(
        dev,
        &a,
        0,
        std::ptr::null(),
        std::mem::size_of::<f64>() as u32,
        &v as *const _ as *const c_void,
    );
    if err != 0 {
        tracing::warn!("boom: set boom Audio sample rate to {sr} failed (err {err})");
    }
}

/// Find an audio device by its UID (our driver → "BoomAudio_UID").
unsafe fn find_device_by_uid(uid: &str) -> AudioObjectID {
    let a = addr(PROP_DEVICES);
    let mut size: u32 = 0;
    if AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &a, 0, std::ptr::null(), &mut size) != 0 {
        return 0;
    }
    let count = size as usize / std::mem::size_of::<AudioObjectID>();
    if count == 0 {
        return 0;
    }
    let mut ids = vec![0u32; count];
    if AudioObjectGetPropertyData(SYSTEM_OBJECT, &a, 0, std::ptr::null(), &mut size, ids.as_mut_ptr() as *mut c_void) != 0 {
        return 0;
    }
    let want = cfstr(uid);
    let mut found = 0;
    for &id in &ids {
        if let Some(u) = string_prop(id, PROP_DEVICE_UID) {
            let eq = CFEqual(u, want) != 0;
            CFRelease(u);
            if eq {
                found = id;
                break;
            }
        }
    }
    CFRelease(want);
    found
}

/// Capture IOProc on "boom Audio": push its loopback input into the ring + zero
/// our contribution to its output (so we never feed back into the loopback).
fn capture_cb(ring: &Ring, input: *const AudioBufferList, output: *mut AudioBufferList) {
    unsafe {
        if !output.is_null() {
            let on = (*output).number_buffers as usize;
            let ob = (*output).buffers.as_mut_ptr();
            for i in 0..on {
                let b = &mut *ob.add(i);
                if !b.data.is_null() && b.data_byte_size > 0 {
                    std::ptr::write_bytes(b.data as *mut u8, 0, b.data_byte_size as usize);
                }
            }
        }
        if input.is_null() || (*input).number_buffers == 0 {
            return;
        }
        let ib = &*(*input).buffers.as_ptr();
        if ib.data.is_null() || ib.data_byte_size < 4 {
            return;
        }
        let n = ib.data_byte_size as usize / 4;
        let s = std::slice::from_raw_parts(ib.data as *const f32, n);
        ring.push(s);
        CB_COUNT.fetch_add(1, Ordering::Relaxed);
        // Input level (RMS) for the meter.
        let rms = (s.iter().map(|x| x * x).sum::<f32>() / n.max(1) as f32).sqrt();
        CB_IN_RMS_BITS.store(rms.to_bits(), Ordering::Relaxed);
    }
}

/// Playback IOProc on the real device: pop the ring into the output, then DSP in
/// place (`try_lock` so a settings tweak never blocks the audio thread).
fn playback_cb(ring: &Ring, dsp: &Mutex<DspChain>, output: *mut AudioBufferList) {
    unsafe {
        if output.is_null() || (*output).number_buffers == 0 {
            return;
        }
        let ob = &mut *(*output).buffers.as_mut_ptr();
        if ob.data.is_null() || ob.data_byte_size < 4 {
            return;
        }
        let n = ob.data_byte_size as usize / 4;
        let slice = std::slice::from_raw_parts_mut(ob.data as *mut f32, n);
        ring.pop_into(slice);
        let mut guard = dsp.try_lock();
        if let Some(chain) = guard.as_mut() {
            chain.process_interleaved(slice, ob.number_channels.max(1) as usize);
        }
        // Output level (RMS, post-DSP) + clip detection for the meter.
        let mut sum = 0f32;
        let mut peak = 0f32;
        for &x in slice.iter() {
            sum += x * x;
            let a = x.abs();
            if a > peak {
                peak = a;
            }
        }
        CB_OUT_RMS_BITS.store((sum / n.max(1) as f32).sqrt().to_bits(), Ordering::Relaxed);
        if peak > 0.99 {
            CLIP.store(true, Ordering::Relaxed);
        }
    }
}

unsafe fn start_locked(eng: &mut Engine) -> bool {
    if eng.session.is_some() {
        return true;
    }
    let boom_dev = find_device_by_uid("BoomAudio_UID");
    if boom_dev == 0 {
        tracing::warn!("boom: 'boom Audio' driver not installed (run scripts/boom-driver-install.sh)");
        return false;
    }
    let mut real_dev = default_output();
    if real_dev == 0 || real_dev == boom_dev {
        // Default is boom Audio (stale from a prior unclean exit) → bridge to a
        // real output instead, so we never play to our own silent device.
        real_dev = pick_real_output(boom_dev);
        if real_dev != 0 {
            tracing::info!("boom: default was stale boom Audio; using real output {real_dev}");
        }
    }
    if real_dev == 0 || real_dev == boom_dev {
        tracing::warn!("boom: no usable real output device found");
        return false;
    }
    let sr = nominal_sample_rate(real_dev);
    // Match boom Audio's rate to the real device. Otherwise apps render to boom
    // Audio at *its* rate (48k) while we play out at the real rate (44.1k) → the
    // music plays slow + the ring overruns (clicks). Let the change settle.
    set_device_sample_rate(boom_dev, sr);
    std::thread::sleep(std::time::Duration::from_millis(80));
    {
        let mut chain = DspChain::new(sr, &BANDS_10);
        std::mem::swap(&mut *eng.dsp.lock(), &mut chain);
    }
    let ring = Ring::new(1 << 15); // 32768 f32 ~= 0.34 s of stereo @ 48 kHz
    // Pre-fill a ~30 ms silence cushion so playback never underruns at startup
    // and has slack to absorb minor clock drift between the two devices.
    let cushion = ((sr * 0.03) as usize) * 2;
    ring.push(&vec![0.0f32; cushion]);

    let ring_c = ring.clone();
    let cap_block: RcBlock<dyn Fn(*const c_void, *const c_void, *const c_void, *mut c_void, *const c_void)> =
        RcBlock::new(move |_n: *const c_void, input: *const c_void, _it: *const c_void, output: *mut c_void, _ot: *const c_void| {
            capture_cb(&ring_c, input as *const AudioBufferList, output as *mut AudioBufferList);
        });
    let mut cap_proc: AudioDeviceIOProcID = std::ptr::null_mut();
    if AudioDeviceCreateIOProcIDWithBlock(&mut cap_proc, boom_dev, std::ptr::null_mut(), &cap_block) != 0 || cap_proc.is_null() {
        tracing::warn!("boom: capture IOProc creation failed");
        return false;
    }

    let ring_p = ring.clone();
    let dsp = eng.dsp.clone();
    let play_block: RcBlock<dyn Fn(*const c_void, *const c_void, *const c_void, *mut c_void, *const c_void)> =
        RcBlock::new(move |_n: *const c_void, _input: *const c_void, _it: *const c_void, output: *mut c_void, _ot: *const c_void| {
            playback_cb(&ring_p, &dsp, output as *mut AudioBufferList);
        });
    let mut play_proc: AudioDeviceIOProcID = std::ptr::null_mut();
    if AudioDeviceCreateIOProcIDWithBlock(&mut play_proc, real_dev, std::ptr::null_mut(), &play_block) != 0 || play_proc.is_null() {
        tracing::warn!("boom: playback IOProc creation failed");
        AudioDeviceDestroyIOProcID(boom_dev, cap_proc);
        return false;
    }

    AudioDeviceStart(boom_dev, cap_proc);
    AudioDeviceStart(real_dev, play_proc);

    // Route the default output to boom Audio so apps render into it; we bridge
    // its loopback → real device. Saved + restored on stop / quit.
    SAVED_DEFAULT_OUTPUT.store(real_dev, Ordering::SeqCst);
    set_default_output(boom_dev);

    eng.session = Some(Session {
        boom_dev,
        real_dev,
        cap_proc,
        play_proc,
        _cap_block: cap_block,
        _play_block: play_block,
        _ring: ring,
    });
    tracing::info!("boom: enabled — routing '{}' through the EQ (sr {sr})", device_name(real_dev));
    tracing::debug!("boom: real_dev={real_dev} boom_audio={boom_dev}");
    true
}

/// Whether the "boom Audio" driver is installed + loaded (the device exists).
pub(crate) fn driver_present() -> bool {
    unsafe { find_device_by_uid("BoomAudio_UID") != 0 }
}

/// Live (input RMS, output RMS, clipped-since-last-read) for the level meters.
/// The clip flag latches then resets on read.
pub(crate) fn levels() -> (f32, f32, bool) {
    (
        f32::from_bits(CB_IN_RMS_BITS.load(Ordering::Relaxed)),
        f32::from_bits(CB_OUT_RMS_BITS.load(Ordering::Relaxed)),
        CLIP.swap(false, Ordering::Relaxed),
    )
}

const HAL_DIR: &str = "/Library/Audio/Plug-Ins/HAL";

fn run_admin(script: &str) -> Result<(), String> {
    let out = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        // -128 = user cancelled the admin prompt.
        Err(if err.is_empty() { "cancelled".into() } else { err })
    }
}

/// Install the bundled "boom Audio" driver into the HAL plug-ins dir + restart
/// coreaudiod (one admin prompt). The `.driver` ships in the app's Resources.
pub(crate) fn install_driver(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let res = app.path().resource_dir().map_err(|e| e.to_string())?;
    let src = res.join("boom-driver.driver");
    if !src.exists() {
        return Err(format!("driver not bundled in app resources ({})", src.display()));
    }
    let src = src.to_string_lossy();
    let script = format!(
        "do shell script \"mkdir -p '{HAL_DIR}' && rm -rf '{HAL_DIR}/boom-driver.driver' && cp -R '{src}' '{HAL_DIR}/' && killall coreaudiod\" with administrator privileges"
    );
    run_admin(&script)
}

/// Remove the driver + restart coreaudiod (one admin prompt).
pub(crate) fn uninstall_driver() -> Result<(), String> {
    let script = format!(
        "do shell script \"rm -rf '{HAL_DIR}/boom-driver.driver' && killall coreaudiod\" with administrator privileges"
    );
    run_admin(&script)
}

unsafe fn stop_locked(eng: &mut Engine) {
    // Restore the user's real output device FIRST.
    let prev = SAVED_DEFAULT_OUTPUT.swap(0, Ordering::SeqCst);
    if prev != 0 {
        set_default_output(prev);
    }
    if let Some(s) = eng.session.take() {
        AudioDeviceStop(s.real_dev, s.play_proc);
        AudioDeviceStop(s.boom_dev, s.cap_proc);
        AudioDeviceDestroyIOProcID(s.real_dev, s.play_proc);
        AudioDeviceDestroyIOProcID(s.boom_dev, s.cap_proc);
        tracing::info!("boom: bridge stopped");
        // blocks + ring drop here, after the IOProcs are destroyed.
    }
}

// ── Public API (called from `boom::apply` + the IPC) ─────────────────────────

/// Start/stop the engine + push the latest DSP params to match `cfg`.
/// Start/stop the engine + push params.
pub(crate) fn set_active(cfg: &BoomConfig) {
    let mut slot = ENGINE.lock();
    if slot.is_none() {
        *slot = Some(Engine {
            session: None,
            dsp: Arc::new(Mutex::new(DspChain::new(48000.0, &BANDS_10))),
        });
    }
    let eng = slot.as_mut().unwrap();
    // Push params (applied whether or not audio is live).
    eng.dsp.lock().set_params(&cfg.dsp_params());

    if cfg.enabled && ENGINE_ENABLED {
        unsafe { start_locked(eng) };
    } else {
        // Engine off (or disabled): make sure nothing is touching the audio path.
        unsafe { stop_locked(eng) };
    }
}

/// Tear the engine down (disable / app quit). Restores normal output.
pub(crate) fn shutdown() {
    let mut slot = ENGINE.lock();
    if let Some(eng) = slot.as_mut() {
        unsafe { stop_locked(eng) };
    }
}
