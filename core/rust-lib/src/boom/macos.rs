//! macOS boom engine (phase 1b): Core-Audio **process tap** → private aggregate
//! device → realtime **IOProc** running the [`DspChain`] → real output. Driverless
//! (Apple's tap API, macOS 14.2+), no BlackHole/kext.
//!
//! **SAFE-FIRST:** the tap is created **UNMUTED** for now, so a bug can never
//! silence the Mac — worst case is doubled audio (you hear the original plus our
//! processed render). Once the chain is verified end-to-end on real hardware we
//! flip `MUTE_BEHAVIOR` to muted (only the processed signal audible). Teardown is
//! idempotent + runs on disable/quit; process taps are per-process, so even a
//! hard crash lets CoreAudio reclaim the tap (output un-mutes).
//!
//! The realtime IOProc closure must not allocate/lock-block: it copies the
//! tapped input into the output buffers and runs `DspChain` in place; params are
//! shared via a `Mutex<DspChain>` read with `try_lock` (a contended tweak just
//! passes that one block through untouched).

// The realtime process-tap engine is retained but currently un-armed via
// `ENGINE_ENABLED` (see below) — so most of it is intentionally dead code.
#![allow(dead_code)]

use super::{BoomConfig, DspChain, BANDS_10};
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use parking_lot::Mutex;
use std::ffi::{c_void, CString};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
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

/// CATapMuteBehavior: 0 = unmuted (doubles → echo), 1 = muted (also muted our
/// output device → silence), 2 = mutedWhenTapped. The tone probe proved our
/// output reaches the speakers, so the silence under (1) is its device-mute.
/// Try (2): mute the source processes without muting the device we render to.
const MUTE_BEHAVIOR: i64 = 2;

/// Master switch for the live audio engine.
const ENGINE_ENABLED: bool = false; // disabled: tap-mute also mutes our output device (see phase 1b notes)

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
    fn AudioObjectGetPropertyData(
        id: AudioObjectID,
        addr: *const AudioObjectPropertyAddress,
        qual_size: u32,
        qual: *const c_void,
        io_size: *mut u32,
        out_data: *mut c_void,
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
    fn CFRelease(cf: *const c_void);
}

fn cfstr(s: &str) -> CFStringRef {
    let c = CString::new(s).unwrap();
    unsafe { CFStringCreateWithCString(kCFAllocatorDefault, c.as_ptr(), KCF_UTF8) }
}

fn addr(selector: u32) -> AudioObjectPropertyAddress {
    AudioObjectPropertyAddress { selector, scope: SCOPE_GLOBAL, element: ELEMENT_MAIN }
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
    tap: AudioObjectID,
    agg: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    // Kept alive for the lifetime of the IOProc.
    _block: RcBlock<dyn Fn(*const c_void, *const c_void, *const c_void, *mut c_void, *const c_void)>,
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

unsafe fn start_locked(eng: &mut Engine) -> bool {
    if eng.session.is_some() {
        return true;
    }
    let Some(desc) = make_tap_description() else {
        tracing::warn!("boom: CATapDescription unavailable");
        return false;
    };
    let mut tap: AudioObjectID = 0;
    let err = AudioHardwareCreateProcessTap(Retained::as_ptr(&desc) as *mut AnyObject, &mut tap);
    if err != 0 || tap == 0 {
        tracing::warn!("boom: AudioHardwareCreateProcessTap failed (err {err}) — likely missing audio-capture permission");
        return false;
    }

    let out_dev = default_output();
    let tap_uid = string_prop(tap, PROP_TAP_UID);
    let out_uid = string_prop(out_dev, PROP_DEVICE_UID);
    let (Some(tap_uid), Some(out_uid)) = (tap_uid, out_uid) else {
        tracing::warn!("boom: could not read tap/output UID");
        AudioHardwareDestroyProcessTap(tap);
        if let Some(u) = tap_uid {
            CFRelease(u);
        }
        if let Some(u) = out_uid {
            CFRelease(u);
        }
        return false;
    };

    let sr = nominal_sample_rate(out_dev);
    eng.dsp.lock().reset();
    {
        // Rebuild the chain at the device sample rate (keeps biquad coeffs correct).
        let mut dsp = DspChain::new(sr, &BANDS_10);
        std::mem::swap(&mut *eng.dsp.lock(), &mut dsp);
    }

    let agg = build_aggregate(tap_uid, out_uid);
    CFRelease(tap_uid);
    CFRelease(out_uid);
    let Some(agg) = agg else {
        AudioHardwareDestroyProcessTap(tap);
        return false;
    };

    // The realtime IOProc: copy tapped input → output, run DSP in place.
    let dsp = eng.dsp.clone();
    let block = RcBlock::new(
        move |_now: *const c_void,
              input: *const c_void,
              _itime: *const c_void,
              output: *mut c_void,
              _otime: *const c_void| {
            io_callback(&dsp, input as *const AudioBufferList, output as *mut AudioBufferList);
        },
    );

    let mut proc_id: AudioDeviceIOProcID = std::ptr::null_mut();
    let err = AudioDeviceCreateIOProcIDWithBlock(&mut proc_id, agg, std::ptr::null_mut(), &block);
    if err != 0 || proc_id.is_null() {
        tracing::warn!("boom: AudioDeviceCreateIOProcIDWithBlock failed (err {err})");
        AudioHardwareDestroyAggregateDevice(agg);
        AudioHardwareDestroyProcessTap(tap);
        return false;
    }
    let err = AudioDeviceStart(agg, proc_id);
    if err != 0 {
        tracing::warn!("boom: AudioDeviceStart failed (err {err})");
        AudioDeviceDestroyIOProcID(agg, proc_id);
        AudioHardwareDestroyAggregateDevice(agg);
        AudioHardwareDestroyProcessTap(tap);
        return false;
    }

    eng.session = Some(Session { tap, agg, proc_id, _block: block });
    tracing::info!("boom: engine started (sr {sr}, mute={MUTE_BEHAVIOR})");
    // One-shot diagnostic: after ~1 s report whether the IOProc is firing + the
    // tap is actually delivering audio (helps diagnose "no sound").
    CB_COUNT.store(0, Ordering::Relaxed);
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(1000));
        let in_rms = f32::from_bits(CB_IN_RMS_BITS.load(Ordering::Relaxed));
        let out_rms = f32::from_bits(CB_OUT_RMS_BITS.load(Ordering::Relaxed));
        tracing::info!(
            "boom diag: calls={} silence={} | IN n={} bytes={} ch={} rms={:.5} | OUT n={} bytes={} ch={} rms={:.5}",
            CB_COUNT.load(Ordering::Relaxed),
            DIAG_SILENCE,
            CB_IN_N.load(Ordering::Relaxed),
            CB_IN_BYTES.load(Ordering::Relaxed),
            CB_IN_CH.load(Ordering::Relaxed),
            in_rms,
            CB_OUT_N.load(Ordering::Relaxed),
            CB_OUT_BYTES.load(Ordering::Relaxed),
            CB_OUT_CH.load(Ordering::Relaxed),
            out_rms,
        );
    });
    true
}

/// The realtime audio callback. No allocation; `try_lock` so a config tweak on
/// another thread never blocks the audio thread (that block just passes through).
fn io_callback(dsp: &Mutex<DspChain>, input: *const AudioBufferList, output: *mut AudioBufferList) {
    unsafe {
        if input.is_null() || output.is_null() {
            return;
        }
        let in_n = (*input).number_buffers as usize;
        let out_n = (*output).number_buffers as usize;
        let n = in_n.min(out_n);
        let in_bufs = (*input).buffers.as_ptr();
        let out_bufs = (*output).buffers.as_mut_ptr();
        // Diagnostics (lock-free): is the callback firing? is tap input present?
        CB_COUNT.fetch_add(1, Ordering::Relaxed);
        CB_IN_N.store(in_n as u32, Ordering::Relaxed);
        CB_OUT_N.store(out_n as u32, Ordering::Relaxed);
        CB_IN_NULL.store(in_n == 0 || (*in_bufs).data.is_null(), Ordering::Relaxed);
        if in_n > 0 {
            let b0 = &*in_bufs;
            CB_IN_BYTES.store(b0.data_byte_size, Ordering::Relaxed);
            CB_IN_CH.store(b0.number_channels, Ordering::Relaxed);
            if !b0.data.is_null() && b0.data_byte_size >= 4 {
                let frames = (b0.data_byte_size as usize / 4).min(256);
                let s = std::slice::from_raw_parts(b0.data as *const f32, frames);
                let rms = (s.iter().map(|x| x * x).sum::<f32>() / frames as f32).sqrt();
                CB_IN_RMS_BITS.store(rms.to_bits(), Ordering::Relaxed);
            }
        }
        // Record the output buffer structure (buffer 0) for diagnosis.
        if out_n > 0 {
            let ob0 = &*out_bufs;
            CB_OUT_BYTES.store(ob0.data_byte_size, Ordering::Relaxed);
            CB_OUT_CH.store(ob0.number_channels, Ordering::Relaxed);
        }

        // DIAGNOSTIC-TONE: write a quiet 440 Hz sine to the output (ignore the
        // tap) — a probe for whether our aggregate output reaches the speakers.
        if DIAG_TONE {
            for i in 0..out_n {
                let ob = &mut *out_bufs.add(i);
                if ob.data.is_null() || ob.data_byte_size < 4 {
                    continue;
                }
                let ch = ob.number_channels.max(1) as usize;
                let frames = ob.data_byte_size as usize / 4 / ch;
                let s = std::slice::from_raw_parts_mut(ob.data as *mut f32, frames * ch);
                for f in 0..frames {
                    let idx = TONE_IDX.fetch_add(1, Ordering::Relaxed);
                    let t = idx as f64 / 44100.0;
                    let v = (0.08 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()) as f32;
                    for c in 0..ch {
                        s[f * ch + c] = v;
                    }
                }
            }
            return;
        }

        // DIAGNOSTIC-SILENCE: zero every output buffer (pure silence) — never
        // copies/processes, so it cannot produce noise. Original audio keeps
        // playing (unmuted). We only collect the format diagnostics above.
        if DIAG_SILENCE {
            for i in 0..out_n {
                let ob = &mut *out_bufs.add(i);
                if !ob.data.is_null() && ob.data_byte_size > 0 {
                    std::ptr::write_bytes(ob.data as *mut u8, 0, ob.data_byte_size as usize);
                }
            }
            return;
        }

        let mut guard = dsp.try_lock();
        for i in 0..n {
            let ib = &*in_bufs.add(i);
            let ob = &mut *out_bufs.add(i);
            let bytes = ib.data_byte_size.min(ob.data_byte_size) as usize;
            if i == 0 {
                CB_OUT_BYTES.store(ob.data_byte_size, Ordering::Relaxed);
                CB_OUT_CH.store(ob.number_channels, Ordering::Relaxed);
            }
            // Defensive: zero the whole output buffer first, so even a short
            // copy (size mismatch) leaves silence, never garbage/noise.
            if !ob.data.is_null() && ob.data_byte_size > 0 {
                std::ptr::write_bytes(ob.data as *mut u8, 0, ob.data_byte_size as usize);
            }
            if ib.data.is_null() || ob.data.is_null() || bytes == 0 {
                continue;
            }
            // Copy tapped input → output buffer.
            std::ptr::copy_nonoverlapping(ib.data as *const u8, ob.data as *mut u8, bytes);
            // Run DSP in place on the output (float samples).
            if let Some(chain) = guard.as_mut() {
                let frames = bytes / std::mem::size_of::<f32>();
                let slice = std::slice::from_raw_parts_mut(ob.data as *mut f32, frames);
                chain.process_interleaved(slice, ob.number_channels.max(1) as usize);
            }
            // Output RMS of buffer 0 (post-DSP) — is our render non-silent?
            if i == 0 && !ob.data.is_null() {
                let frames = (bytes / 4).min(256);
                let s = std::slice::from_raw_parts(ob.data as *const f32, frames);
                let rms = (s.iter().map(|x| x * x).sum::<f32>() / frames as f32).sqrt();
                CB_OUT_RMS_BITS.store(rms.to_bits(), Ordering::Relaxed);
            }
        }
    }
}

unsafe fn stop_locked(eng: &mut Engine) {
    if let Some(s) = eng.session.take() {
        AudioDeviceStop(s.agg, s.proc_id);
        AudioDeviceDestroyIOProcID(s.agg, s.proc_id);
        AudioHardwareDestroyAggregateDevice(s.agg);
        AudioHardwareDestroyProcessTap(s.tap);
        tracing::info!("boom: engine stopped + torn down");
        // `s._block` drops here, after the IOProc is destroyed.
    }
}

// ── Public API (called from `boom::apply` + the IPC) ─────────────────────────

/// Start/stop the engine + push the latest DSP params to match `cfg`.
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
