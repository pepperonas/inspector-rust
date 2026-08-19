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
//! (clicks). A ~60 ms silence cushion is pre-filled for startup + drift slack.
//!
//! **Idle gate (battery, v0.84.240):** a running IOProc makes coreaudiod hold a
//! `PreventUserIdleSystemSleep` assertion — the Mac could never idle-sleep while
//! boom was on. After 60 s of true silence the gate stops both IOProcs
//! (assertions released, procs stay registered) — but only if, with our procs
//! stopped, no OTHER client runs on boom Audio (`DeviceIsRunningSomewhere`
//! probe; the wake listener is armed BEFORE the probe so a client starting in
//! the gap is never missed). Any app starting playback fires the listener →
//! resume in milliseconds (ring re-primed to the startup cushion). The webview's
//! warm AudioContext is parked/woken alongside via the `warm-audio-suspend` /
//! `warm-audio-resume` events (it is itself a client on boom Audio).
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

use super::{BoomConfig, DspChain, BANDS_10, DspParams};
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
/// Perceptual output gain (f32 bits) from the boom-Audio volume scalar — the
/// driver publishes the control but no longer applies it (its stock curve was
/// linear over −64..0 dB → "40 % is barely audible"); the playback bridge
/// multiplies by `boom::volume_gain(scalar)` (scalar²) instead. Updated by a
/// CoreAudio property listener + the initial read at start.
static VOLUME_GAIN_BITS: AtomicU32 = AtomicU32::new(1.0f32.to_bits());

// ── Idle gate (battery, v0.84.240) ───────────────────────────────────────────
// A running IOProc makes coreaudiod hold a PreventUserIdleSystemSleep assertion
// for our process — the Mac could never idle-sleep while boom was enabled. The
// gate suspends both IOProcs after `boom::GATE_SUSPEND_AFTER_MS` of true
// silence (BlackHole delivers exact zeros when nothing renders) and resumes on
// a `kAudioDevicePropertyDeviceIsRunningSomewhere` wake — with our own procs
// stopped, that property reflects OTHER clients only, so any app starting
// playback into boom Audio fires the listener within milliseconds.

/// Millis since process start, written by the capture IOProc on every audible
/// buffer. `Instant::elapsed` is a commpage read on macOS — RT-safe.
static LAST_AUDIBLE_MS: AtomicU64 = AtomicU64::new(0);
static GATE_EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
static GATE_THREAD_ON: AtomicBool = AtomicBool::new(false);
static RUNNING_LISTENER_ON: AtomicBool = AtomicBool::new(false);
/// App handle for the `warm-audio-suspend`/`warm-audio-resume` frontend events
/// (the webview's warm AudioContext is itself a client on boom Audio — it must
/// park alongside the bridge or the "no other clients" probe never passes).
static APP_HANDLE: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

fn now_ms() -> u64 {
    GATE_EPOCH.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
}

pub(crate) fn set_app_handle(app: tauri::AppHandle) {
    let _ = APP_HANDLE.set(app);
}

fn emit_frontend(event: &str) {
    if let Some(app) = APP_HANDLE.get() {
        use tauri::Emitter;
        let _ = app.emit(event, ());
    }
}

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
const PROP_MUTE: u32 = fourcc(b"mute"); // kAudioDevicePropertyMute
const PROP_VOLUME_SCALAR: u32 = fourcc(b"volm"); // kAudioDevicePropertyVolumeScalar
const PROP_IS_RUNNING_SOMEWHERE: u32 = fourcc(b"gone"); // kAudioDevicePropertyDeviceIsRunningSomewhere

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
    fn AudioObjectAddPropertyListener(
        id: AudioObjectID,
        addr: *const AudioObjectPropertyAddress,
        proc: AudioObjectPropertyListenerProc,
        client_data: *mut c_void,
    ) -> OSStatus;
    fn AudioObjectRemovePropertyListener(
        id: AudioObjectID,
        addr: *const AudioObjectPropertyAddress,
        proc: AudioObjectPropertyListenerProc,
        client_data: *mut c_void,
    ) -> OSStatus;
}

type AudioObjectPropertyListenerProc =
    extern "C" fn(AudioObjectID, u32, *const AudioObjectPropertyAddress, *mut c_void) -> OSStatus;

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
    ring: Arc<Ring>,
    /// The startup silence cushion in samples — the idle-gate resume re-primes
    /// the ring to exactly this, so bridge latency stays deterministic across
    /// suspend/resume cycles instead of creeping toward the ring capacity.
    cushion: usize,
    /// False while the idle gate has the IOProcs suspended (procs stay
    /// registered; only stopped — coreaudiod releases the sleep assertions).
    io_running: bool,
}

// SAFETY: the CoreAudio object ids are plain integers; the block is only invoked
// on the audio thread and we tear the session down before dropping it. We gate
// all access through a global Mutex.
unsafe impl Send for Session {}

struct Engine {
    session: Option<Session>,
    dsp: Arc<Mutex<DspChain>>,
    /// The last params pushed from the config. `start_locked` re-applies them to
    /// the fresh chain it swaps in — without this, enabling boom ran a DEFAULT
    /// (flat) chain until the next slider touch pushed params again (the "have
    /// to move a slider for settings to apply" bug).
    params: DspParams,
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
    /// Reset to exactly `cushion` samples of silence. ONLY safe while BOTH
    /// IOProcs are stopped (no concurrent producer/consumer) — used by the
    /// idle-gate resume so the bridge restarts with its deterministic startup
    /// latency instead of whatever fill level the suspend froze.
    unsafe fn reset_to_silence(&self, cushion: usize) {
        let w = self.write.load(Ordering::Relaxed);
        self.read.store(w, Ordering::Relaxed); // drain
        self.push(&vec![0.0f32; cushion.min(self.cap)]);
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

/// A device's stable UID (survives restarts, unlike `AudioObjectID`).
unsafe fn device_uid(dev: AudioObjectID) -> String {
    match string_prop(dev, PROP_DEVICE_UID) {
        Some(cf) => {
            let s = cfstring_to_string(cf).unwrap_or_default();
            CFRelease(cf);
            s
        }
        None => String::new(),
    }
}

/// Settings key: the UID of the real output device the bridge last targeted.
/// After an UNCLEAN exit the system default is stale-stuck on boom Audio and
/// carries no information about the user's device — this key does. It makes
/// "restart lands on the MacBook speakers instead of the BT box" impossible
/// whenever the remembered device is still present.
const KEY_LAST_OUTPUT_UID: &str = "boom.last_output_uid";

fn persist_last_output_uid(uid: &str) {
    if uid.is_empty() {
        return;
    }
    let Some(app) = APP_HANDLE.get() else { return };
    use tauri::Manager as _;
    if let Some(db) = app.try_state::<crate::db::DbHandle>() {
        let _ = crate::settings::set(&db, KEY_LAST_OUTPUT_UID, uid);
    }
}

fn saved_last_output_uid() -> String {
    let Some(app) = APP_HANDLE.get() else { return String::new() };
    use tauri::Manager as _;
    match app.try_state::<crate::db::DbHandle>() {
        Some(db) => crate::settings::get_or(&db, KEY_LAST_OUTPUT_UID, "").unwrap_or_default(),
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
    // If boom is actively bridging, the boom-Audio default is LEGITIMATE (not
    // stale) — leave it. Otherwise this fires on every config change / overlay
    // close and yanks the output device away mid-session.
    //
    // The ENGINE guard is held across the WHOLE body (not just this check):
    // otherwise a concurrent start_locked() could bring a session up between the
    // check and set_default_output below, and we'd yank the default away from a
    // just-started bridge (session "active" but the OS output bypassing the EQ).
    let guard = ENGINE.lock();
    if guard.as_ref().is_some_and(|e| e.session.is_some()) {
        return;
    }
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
    drop(guard);
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

/// A device's output-mute state — master element first, channel 1 fallback.
/// `None` = the device publishes no mute control (treat as audible).
unsafe fn device_mute(dev: AudioObjectID) -> Option<bool> {
    for element in [ELEMENT_MAIN, 1] {
        let a = AudioObjectPropertyAddress { selector: PROP_MUTE, scope: SCOPE_OUTPUT, element };
        let mut v: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        if AudioObjectGetPropertyData(dev, &a, 0, std::ptr::null(), &mut size, &mut v as *mut _ as *mut c_void) == 0 {
            return Some(v != 0);
        }
    }
    None
}

/// Set a device's output mute (both elements best-effort). `true` on success.
unsafe fn set_device_mute(dev: AudioObjectID, muted: bool) -> bool {
    let v: u32 = muted as u32;
    let mut ok = false;
    for element in [ELEMENT_MAIN, 1, 2] {
        let a = AudioObjectPropertyAddress { selector: PROP_MUTE, scope: SCOPE_OUTPUT, element };
        ok |= AudioObjectSetPropertyData(
            dev,
            &a,
            0,
            std::ptr::null(),
            std::mem::size_of::<u32>() as u32,
            &v as *const _ as *const c_void,
        ) == 0;
    }
    ok
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
        // Input level (RMS) for the meter + peak for the idle gate, one pass.
        let mut sum = 0f32;
        let mut peak = 0f32;
        for &x in s {
            sum += x * x;
            let a = x.abs();
            if a > peak {
                peak = a;
            }
        }
        CB_IN_RMS_BITS.store((sum / n.max(1) as f32).sqrt().to_bits(), Ordering::Relaxed);
        if super::gate_is_audible(peak) {
            LAST_AUDIBLE_MS.store(now_ms(), Ordering::Relaxed);
        }
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
        // System volume (perceptual taper) — applied here, post-DSP, since the
        // driver no longer applies its own (badly-tapered) gain. RT-safe: one
        // atomic load + a multiply.
        let gain = f32::from_bits(VOLUME_GAIN_BITS.load(Ordering::Relaxed));
        if gain != 1.0 {
            for x in slice.iter_mut() {
                *x *= gain;
            }
        }
        // Output level (RMS, post-DSP + volume) + clip detection for the meter.
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
        // Prefer the REMEMBERED device (the one the user last listened on —
        // e.g. a BT speaker), fall back to built-in only when it's gone.
        let saved_uid = saved_last_output_uid();
        if !saved_uid.is_empty() {
            let remembered = find_device_by_uid(&saved_uid);
            if remembered != 0 && remembered != boom_dev && device_has_output(remembered) {
                real_dev = remembered;
                tracing::info!(
                    "boom: default was stale boom Audio; restoring remembered output '{}'",
                    device_name(remembered)
                );
            }
        }
        if real_dev == 0 || real_dev == boom_dev {
            real_dev = pick_real_output(boom_dev);
            if real_dev != 0 {
                tracing::info!("boom: default was stale boom Audio; using real output {real_dev}");
            }
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
        // Fresh chain at the REAL device's sample rate — and immediately re-apply
        // the saved config params: the swap otherwise left a default (flat) chain
        // live until the next slider touch pushed params again.
        let mut chain = DspChain::new(sr, &BANDS_10);
        chain.set_params(&eng.params);
        std::mem::swap(&mut *eng.dsp.lock(), &mut chain);
    }
    let ring = Ring::new(1 << 15); // 32768 f32 ~= 0.34 s of stereo @ 48 kHz
    // Pre-fill a ~60 ms silence cushion so playback never underruns at startup
    // and has slack to absorb minor clock drift between the two devices AND
    // brief capture-IOProc stalls while CoreAudio reconfigures devices — e.g.
    // the webview opening the microphone for the BPM detector / disco, which
    // used to drain the old 30 ms cushion and stutter the music (v0.84.238).
    let cushion = ((sr * 0.06) as usize) * 2;
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

    // A failed start must NOT be reported as success — otherwise the default
    // output is switched to boom Audio with no running bridge and the user's
    // system audio goes silent while the UI claims boom is on. Check both, and
    // unwind everything created so far on failure (so retry_start can retry).
    let st_cap = AudioDeviceStart(boom_dev, cap_proc);
    if st_cap != 0 {
        tracing::warn!("boom: AudioDeviceStart(capture) failed: {st_cap}");
        AudioDeviceDestroyIOProcID(boom_dev, cap_proc);
        AudioDeviceDestroyIOProcID(real_dev, play_proc);
        return false;
    }
    let st_play = AudioDeviceStart(real_dev, play_proc);
    if st_play != 0 {
        tracing::warn!("boom: AudioDeviceStart(playback) failed: {st_play}");
        AudioDeviceStop(boom_dev, cap_proc);
        AudioDeviceDestroyIOProcID(boom_dev, cap_proc);
        AudioDeviceDestroyIOProcID(real_dev, play_proc);
        return false;
    }

    // Mute transparency (v0.112.1): right after enabling boom or switching
    // outputs, silence is never the desired state — clear a set mute on BOTH
    // boom Audio (where keys/HUD/gestures land while boom fronts the system;
    // it persisted invisibly and read as "boom is broken" three times in the
    // field) and the real device (its own device-level mute is equally
    // invisible behind boom, and a live probe showed those reads go stale).
    // The idle-gate resume deliberately does NOT clear (same device, no
    // user-visible transition — a deliberate mid-session mute survives).
    for (dev, label) in [(boom_dev, "boom Audio"), (real_dev, "real output")] {
        if super::should_clear_stale_mute(device_mute(dev)) {
            if set_device_mute(dev, false) {
                tracing::info!("boom: cleared stale mute on {label} '{}'", device_name(dev));
            } else {
                tracing::warn!("boom: could not clear mute on {label} '{}'", device_name(dev));
            }
        }
    }

    // Route the default output to boom Audio so apps render into it; we bridge
    // its loopback → real device. Saved + restored on stop / quit; the UID is
    // ALSO persisted so a restart after an unclean exit re-targets this device.
    SAVED_DEFAULT_OUTPUT.store(real_dev, Ordering::SeqCst);
    persist_last_output_uid(&device_uid(real_dev));
    set_default_output(boom_dev);

    LEVELS_BOOM_DEV.store(boom_dev, Ordering::Relaxed);
    LEVELS_REAL_DEV.store(real_dev, Ordering::Relaxed);
    eng.session = Some(Session {
        boom_dev,
        real_dev,
        cap_proc,
        play_proc,
        _cap_block: cap_block,
        _play_block: play_block,
        ring,
        cushion,
        io_running: true,
    });
    // Idle-gate grace: a fresh bridge gets a full silence window before the
    // gate may suspend it (LAST_AUDIBLE may be minutes stale from before).
    LAST_AUDIBLE_MS.store(now_ms(), Ordering::Relaxed);
    // Follow later output-device changes (e.g. the user picks Bluetooth) so boom
    // keeps EQ-ing the selected device instead of being pinned to this one.
    add_default_listener();
    // Track boom Audio's volume scalar (keys/HUD/our slider all write it) →
    // the bridge applies the perceptual gain (driver-side application is off).
    add_volume_listener(boom_dev);
    tracing::info!("boom: enabled — routing '{}' through the EQ (sr {sr})", device_name(real_dev));
    tracing::debug!("boom: real_dev={real_dev} boom_audio={boom_dev}");
    true
}

/// Whether the "boom Audio" driver is installed + loaded (the device exists).
pub(crate) fn driver_present() -> bool {
    unsafe { find_device_by_uid("BoomAudio_UID") != 0 }
}

/// Live (input RMS, output RMS, clipped-since-last-read, output-muted) for
/// the level meters. The clip flag latches then resets on read; `muted` reads
/// boom Audio's mute property live (µs — the panel polls ~8 Hz) so the UI can
/// say WHY the output is silent instead of looking broken.
pub(crate) fn levels() -> (f32, f32, bool, bool) {
    let muted = [&LEVELS_BOOM_DEV, &LEVELS_REAL_DEV].iter().any(|a| {
        let dev = a.load(Ordering::Relaxed);
        dev != 0 && unsafe { device_mute(dev) }.unwrap_or(false)
    });
    (
        f32::from_bits(CB_IN_RMS_BITS.load(Ordering::Relaxed)),
        f32::from_bits(CB_OUT_RMS_BITS.load(Ordering::Relaxed)),
        CLIP.swap(false, Ordering::Relaxed),
        muted,
    )
}

/// boom Audio's / the real output's device ids while a session runs (0 =
/// none) — lets `levels()` read the mute properties without a device-list
/// scan, and `unmute_outputs()` clear them from the panel banner's button.
static LEVELS_BOOM_DEV: AtomicU32 = AtomicU32::new(0);
static LEVELS_REAL_DEV: AtomicU32 = AtomicU32::new(0);

/// Clear the mute on both bridge devices (panel "Unmute" button). Returns
/// whether anything was actually cleared.
pub(crate) fn unmute_outputs() -> bool {
    let mut cleared = false;
    for a in [&LEVELS_BOOM_DEV, &LEVELS_REAL_DEV] {
        let dev = a.load(Ordering::Relaxed);
        if dev != 0 && unsafe { device_mute(dev) } == Some(true) {
            cleared |= unsafe { set_device_mute(dev, false) };
        }
    }
    cleared
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

/// Stop + destroy the IOProcs and drop the session WITHOUT restoring the default
/// output (used by the live re-bridge, which keeps the new device as default).
unsafe fn stop_ioprocs_only(eng: &mut Engine) {
    if let Some(s) = eng.session.take() {
        remove_running_listener(s.boom_dev); // idle-gate wake listener, if armed
        AudioDeviceStop(s.real_dev, s.play_proc);
        AudioDeviceStop(s.boom_dev, s.cap_proc);
        AudioDeviceDestroyIOProcID(s.real_dev, s.play_proc);
        AudioDeviceDestroyIOProcID(s.boom_dev, s.cap_proc);
        // blocks + ring drop here, after the IOProcs are destroyed.
    }
}

// ── Idle gate — suspend the bridge during silence so the Mac can sleep ───────

/// `kAudioDevicePropertyDeviceIsRunningSomewhere` — with our own IOProcs
/// stopped this reflects OTHER processes' clients only.
unsafe fn is_running_somewhere(dev: AudioObjectID) -> bool {
    let a = addr(PROP_IS_RUNNING_SOMEWHERE);
    let mut v: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    AudioObjectGetPropertyData(dev, &a, 0, std::ptr::null(), &mut size, &mut v as *mut u32 as *mut c_void) == 0
        && v != 0
}

/// Fires on any run-state change of boom Audio. Lightweight: hand off to a
/// worker (same pattern as `default_output_changed`); `gate_resume` re-checks
/// the actual state under the engine lock, so spurious fires are no-ops.
extern "C" fn boom_running_changed(
    _obj: AudioObjectID,
    _n: u32,
    _addrs: *const AudioObjectPropertyAddress,
    _data: *mut c_void,
) -> OSStatus {
    std::thread::spawn(gate_resume);
    0
}

unsafe fn add_running_listener(dev: AudioObjectID) {
    if RUNNING_LISTENER_ON.swap(true, Ordering::SeqCst) {
        return;
    }
    let a = addr(PROP_IS_RUNNING_SOMEWHERE);
    AudioObjectAddPropertyListener(dev, &a, boom_running_changed, std::ptr::null_mut());
}

fn remove_running_listener(dev: AudioObjectID) {
    if !RUNNING_LISTENER_ON.swap(false, Ordering::SeqCst) {
        return;
    }
    let a = addr(PROP_IS_RUNNING_SOMEWHERE);
    unsafe {
        AudioObjectRemovePropertyListener(dev, &a, boom_running_changed, std::ptr::null_mut());
    }
}

/// The gate's monitor thread — one per process, ticking every few seconds.
/// Cheap while there's nothing to do (two atomic loads).
fn ensure_gate_thread() {
    if GATE_THREAD_ON.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("boom-idle-gate".into())
        .spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(5));
            if !SHOULD_RUN.load(Ordering::SeqCst) {
                continue;
            }
            if !super::gate_should_suspend(LAST_AUDIBLE_MS.load(Ordering::Relaxed), now_ms()) {
                continue;
            }
            // Phase 1 (UNDER the lock, fast): stop the IOProcs and mark the
            // session suspended. Phase 2 (lock DROPPED): the 12 x 200 ms
            // no-other-clients probe. Holding ENGINE across that probe (the
            // pre-2026-08-16 shape) blocked gate_resume — the wake path whose
            // whole purpose is "a client starts -> immediate resume" — for up
            // to 2.4 s of dead audio, and set_boom_config/shutdown with it.
            let boom_dev = {
                let mut slot = ENGINE.lock();
                let Some(eng) = slot.as_mut() else { continue };
                let Some(s) = eng.session.as_mut() else { continue };
                if !s.io_running {
                    continue; // already suspended — the wake listener owns resume
                }
                unsafe { suspend_begin(s) };
                s.boom_dev
            };
            // Probe without the lock: give the (possibly timer-throttled)
            // webview a moment to park its context, then require boom Audio to
            // be client-free.
            let mut clean = false;
            for _ in 0..12 {
                std::thread::sleep(std::time::Duration::from_millis(200));
                if !unsafe { is_running_somewhere(boom_dev) } {
                    clean = true;
                    break;
                }
            }
            if clean {
                tracing::info!("boom: idle gate — bridge suspended (sleep assertions released)");
                continue;
            }
            // Abort — but re-check under the lock first: gate_resume may have
            // already resumed the bridge during the probe (that racing freely
            // is exactly what dropping the lock buys).
            let mut slot = ENGINE.lock();
            let Some(eng) = slot.as_mut() else { continue };
            let Some(s) = eng.session.as_mut() else { continue };
            if s.io_running {
                continue; // resumed while we probed — nothing to undo
            }
            unsafe { suspend_abort(s) };
        });
    if spawned.is_err() {
        GATE_THREAD_ON.store(false, Ordering::SeqCst);
    }
}

/// Phase 1 of the idle suspend (engine lock HELD — fast, no sleeps).
/// **Audio must never be lost:** the wake listener is armed BEFORE the
/// no-other-clients probe (a client starting in between fires it → immediate
/// resume, now truly immediate because the probe runs without the lock).
unsafe fn suspend_begin(s: &mut Session) {
    // Ask the webview to park its warm AudioContext — it is itself a client on
    // boom Audio and would otherwise always fail the probe. (The frontend
    // refuses while a mic is live, i.e. bpm/disco running → probe fails → we
    // stay running. Correct: the user is actively using audio.)
    emit_frontend("warm-audio-suspend");
    add_running_listener(s.boom_dev);
    AudioDeviceStop(s.real_dev, s.play_proc);
    AudioDeviceStop(s.boom_dev, s.cap_proc);
    s.io_running = false;
}

/// The failed-probe undo (engine lock HELD again, `io_running` re-checked by
/// the caller). Another process still has a running (silent) client on boom
/// Audio — or the webview couldn't park. Never risk inaudible playback.
unsafe fn suspend_abort(s: &mut Session) {
    remove_running_listener(s.boom_dev);
    let a = AudioDeviceStart(s.boom_dev, s.cap_proc);
    let b = AudioDeviceStart(s.real_dev, s.play_proc);
    s.io_running = true;
    emit_frontend("warm-audio-resume");
    // Back off a full silence window before the next attempt.
    LAST_AUDIBLE_MS.store(now_ms(), Ordering::Relaxed);
    if a != 0 || b != 0 {
        tracing::warn!("boom: idle-gate abort restart failed (cap {a}, play {b})");
    } else {
        tracing::info!("boom: idle-suspend aborted — another client still runs on boom Audio; retrying later");
    }
}

/// Resume the suspended bridge — spawned by the wake listener the moment any
/// app starts rendering into boom Audio again.
fn gate_resume() {
    let mut slot = ENGINE.lock();
    let Some(eng) = slot.as_mut() else { return };
    let Some(s) = eng.session.as_mut() else { return };
    if s.io_running || !SHOULD_RUN.load(Ordering::SeqCst) {
        return;
    }
    unsafe {
        if !is_running_somewhere(s.boom_dev) {
            return; // spurious property change (e.g. our own stop) — stay suspended
        }
        // Deterministic latency: restart from exactly the startup cushion.
        s.ring.reset_to_silence(s.cushion);
        let a = AudioDeviceStart(s.boom_dev, s.cap_proc);
        let b = AudioDeviceStart(s.real_dev, s.play_proc);
        s.io_running = true;
        remove_running_listener(s.boom_dev);
        if a != 0 || b != 0 {
            tracing::warn!("boom: idle-gate resume failed (cap {a}, play {b}) — re-bridging");
            // Last-ditch: full rebuild so the user is never left silent.
            stop_ioprocs_only(eng);
            start_locked(eng);
        } else {
            tracing::info!("boom: audio client active — bridge resumed");
        }
    }
    LAST_AUDIBLE_MS.store(now_ms(), Ordering::Relaxed);
    drop(slot);
    // Let the fresh bridge settle before the webview's silent context re-adds
    // its own client (its output-unit start can stall the capture for a beat).
    std::thread::sleep(std::time::Duration::from_millis(700));
    emit_frontend("warm-audio-resume");
}

unsafe fn stop_locked(eng: &mut Engine) {
    LEVELS_BOOM_DEV.store(0, Ordering::Relaxed);
    LEVELS_REAL_DEV.store(0, Ordering::Relaxed);
    remove_default_listener();
    // Restore the user's real output device FIRST.
    let prev = SAVED_DEFAULT_OUTPUT.swap(0, Ordering::SeqCst);
    if prev != 0 {
        set_default_output(prev);
    }
    let had = eng.session.is_some();
    stop_ioprocs_only(eng);
    if had {
        tracing::info!("boom: bridge stopped");
    }
}

static LISTENER_ON: AtomicBool = AtomicBool::new(false);

/// CoreAudio fires this when the system default output changes. Lightweight:
/// hand off to a worker so we never mutate audio state on the notify thread.
extern "C" fn default_output_changed(
    _obj: AudioObjectID,
    _n: u32,
    _addrs: *const AudioObjectPropertyAddress,
    _data: *mut c_void,
) -> OSStatus {
    std::thread::spawn(follow_default_change);
    0
}

/// The user picked a different output while boom is running → re-bridge to it so
/// boom keeps EQ-ing whatever device is selected (and SAVED tracks it).
fn follow_default_change() {
    let mut slot = ENGINE.lock();
    let Some(eng) = slot.as_mut() else { return };
    let (boom_dev, cur_real) = match &eng.session {
        Some(s) => (s.boom_dev, s.real_dev),
        None => return,
    };
    let new_def = unsafe { default_output() };
    // Ignore our own switch back to boom Audio + no-op (same real device).
    if new_def == 0 || new_def == boom_dev || new_def == cur_real {
        return;
    }
    tracing::info!(
        "boom: output changed → re-bridging to '{}' ({new_def})",
        unsafe { device_name(new_def) },
    );
    unsafe {
        stop_ioprocs_only(eng); // keep new_def as the default
        start_locked(eng); // captures default_output() == new_def as the real device
    }
}

/// Re-read boom Audio's volume scalar → store the perceptual gain. Called at
/// start + from the property listener on every volume change (keys, HUD, our
/// slider — they all write the device scalar).
unsafe fn refresh_volume_gain(boom_dev: AudioObjectID) {
    let a = AudioObjectPropertyAddress {
        selector: PROP_VOLUME_SCALAR,
        scope: SCOPE_OUTPUT,
        element: ELEMENT_MAIN,
    };
    let mut scalar: f32 = 1.0;
    let mut size = std::mem::size_of::<f32>() as u32;
    let st = AudioObjectGetPropertyData(
        boom_dev,
        &a,
        0,
        std::ptr::null(),
        &mut size,
        &mut scalar as *mut f32 as *mut c_void,
    );
    if st == 0 {
        let gain = super::volume_gain(scalar);
        VOLUME_GAIN_BITS.store(gain.to_bits(), Ordering::Relaxed);
        tracing::debug!("boom: volume scalar {scalar:.2} → gain {gain:.3}");
    }
}

extern "C" fn boom_volume_changed(
    object: AudioObjectID,
    _n: u32,
    _addrs: *const AudioObjectPropertyAddress,
    _client: *mut c_void,
) -> OSStatus {
    unsafe { refresh_volume_gain(object) };
    0
}

static VOLUME_LISTENER_ON: AtomicBool = AtomicBool::new(false);

unsafe fn add_volume_listener(boom_dev: AudioObjectID) {
    refresh_volume_gain(boom_dev);
    if VOLUME_LISTENER_ON.swap(true, Ordering::SeqCst) {
        return;
    }
    let a = AudioObjectPropertyAddress {
        selector: PROP_VOLUME_SCALAR,
        scope: SCOPE_OUTPUT,
        element: ELEMENT_MAIN,
    };
    AudioObjectAddPropertyListener(boom_dev, &a, boom_volume_changed, std::ptr::null_mut());
}

unsafe fn add_default_listener() {
    if LISTENER_ON.swap(true, Ordering::SeqCst) {
        return; // already registered (survives live re-bridges)
    }
    let a = addr(PROP_DEFAULT_OUTPUT);
    AudioObjectAddPropertyListener(SYSTEM_OBJECT, &a, default_output_changed, std::ptr::null_mut());
}

fn remove_default_listener() {
    if !LISTENER_ON.swap(false, Ordering::SeqCst) {
        return;
    }
    let a = addr(PROP_DEFAULT_OUTPUT);
    unsafe {
        AudioObjectRemovePropertyListener(SYSTEM_OBJECT, &a, default_output_changed, std::ptr::null_mut());
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
            params: DspParams::default(),
        });
    }
    let eng = slot.as_mut().unwrap();
    // Push params (applied whether or not audio is live) + remember them so
    // start_locked can re-apply to the fresh chain it swaps in.
    eng.params = cfg.dsp_params();
    eng.dsp.lock().set_params(&eng.params);

    if cfg.enabled && ENGINE_ENABLED {
        SHOULD_RUN.store(true, Ordering::SeqCst);
        ensure_gate_thread();
        if !unsafe { start_locked(eng) } {
            // Occasional device-not-ready miss right after enabling — retry in the
            // background so the user doesn't have to nudge a slider to kick it off.
            std::thread::spawn(retry_start);
        }
        // The bridge is (or is about to be) live — the webview may keep its warm
        // AudioContext running (it protects the ring from mic spin-up stalls).
        emit_frontend("warm-audio-resume");
    } else {
        SHOULD_RUN.store(false, Ordering::SeqCst);
        // Engine off (or disabled): make sure nothing is touching the audio path.
        unsafe { stop_locked(eng) };
        // Without boom there is no ring to protect — park the webview's warm
        // AudioContext too, or ITS silent output unit keeps holding a
        // PreventUserIdleSystemSleep assertion on the speakers.
        emit_frontend("warm-audio-suspend");
    }
}

static SHOULD_RUN: AtomicBool = AtomicBool::new(false);

/// Retry starting the bridge a few times (the audio device list can be briefly
/// not-ready right after enabling). Cancels if the user disables in the meantime.
fn retry_start() {
    for _ in 0..4 {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if !SHOULD_RUN.load(Ordering::SeqCst) {
            return;
        }
        let mut slot = ENGINE.lock();
        let Some(eng) = slot.as_mut() else { return };
        if eng.session.is_some() {
            return; // already running
        }
        if unsafe { start_locked(eng) } {
            tracing::info!("boom: bridge started on retry");
            return;
        }
    }
    tracing::warn!("boom: bridge failed to start after retries");
}

/// Tear the engine down (disable / app quit). Restores normal output.
pub(crate) fn shutdown() {
    let mut slot = ENGINE.lock();
    if let Some(eng) = slot.as_mut() {
        unsafe { stop_locked(eng) };
    }
}

#[cfg(test)]
mod probe_tests {
    use super::*;

    #[test]
    #[ignore] // live probe: prints every output device's mute state
    fn probe_device_mutes() {
        unsafe {
            let a = addr(PROP_DEVICES);
            let mut size: u32 = 0;
            assert_eq!(AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &a, 0, std::ptr::null(), &mut size), 0);
            let count = size as usize / std::mem::size_of::<AudioObjectID>();
            let mut ids = vec![0u32; count];
            assert_eq!(AudioObjectGetPropertyData(SYSTEM_OBJECT, &a, 0, std::ptr::null(), &mut size, ids.as_mut_ptr() as *mut c_void), 0);
            let def = default_output();
            for id in ids {
                if !device_has_output(id) {
                    continue;
                }
                eprintln!(
                    "dev {id}{} '{}' mute={:?}",
                    if id == def { " (DEFAULT)" } else { "" },
                    device_name(id),
                    device_mute(id)
                );
            }
        }
    }
}
