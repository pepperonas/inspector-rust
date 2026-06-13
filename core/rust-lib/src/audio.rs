//! Audio **output device** selection — the `sound` command. Lists the system
//! render/output devices and switches the default, mirroring the macOS Sound
//! pane's Output list. UI is inline in the popup (`SoundPanel.tsx`), the same
//! arrow-key picker pattern as `brightness`.
//!
//! - **macOS:** raw CoreAudio FFI (`#[link(... "CoreAudio")]`, same pattern as
//!   `input_lock.rs`). Enumerate `kAudioHardwarePropertyDevices`, keep the ones
//!   with output channels, read each name, and get/set
//!   `kAudioHardwarePropertyDefaultOutputDevice`.
//! - **Windows (runtime-unverified):** the public MMDevice API
//!   (`IMMDeviceEnumerator`) lists render endpoints; switching the default uses
//!   the undocumented-but-ubiquitous `IPolicyConfig` COM interface (the same
//!   one NirCmd / SoundVolumeView use — there's no public "set default device"
//!   API). Defined by hand since the `windows` metadata doesn't ship it.
//! - **Linux:** PulseAudio / PipeWire via the `pactl` CLI (PipeWire ships a
//!   `pipewire-pulse` shim so `pactl` works on both). Lists sinks from
//!   `pactl list sinks`, reads the default from `pactl get-default-sink`, and
//!   switches with `pactl set-default-sink`. The sink-list parser is pure +
//!   unit-tested.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AudioDevice {
    /// Opaque per-platform id: the CoreAudio `AudioDeviceID` (decimal) on
    /// macOS, the MMDevice endpoint-id string on Windows.
    pub id: String,
    /// Human-readable device name (e.g. "MacBook Pro Speakers").
    pub name: String,
    /// Whether this is the current default output device.
    pub is_default: bool,
}

/// List the system audio **output** devices, marking the current default.
pub fn list_outputs() -> Result<Vec<AudioDevice>, String> {
    #[cfg(target_os = "macos")]
    {
        macos::list_outputs()
    }
    #[cfg(target_os = "windows")]
    {
        win::list_outputs()
    }
    #[cfg(target_os = "linux")]
    {
        linux::list_outputs()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        Err("audio output switching isn't supported on this platform yet".into())
    }
}

/// Set the default audio **output** device by its [`AudioDevice::id`].
pub fn set_output(id: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos::set_output(id)
    }
    #[cfg(target_os = "windows")]
    {
        win::set_output(id)
    }
    #[cfg(target_os = "linux")]
    {
        linux::set_output(id)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = id;
        Err("audio output switching isn't supported on this platform yet".into())
    }
}

/// Parse the `Name:` / `Description:` pairs out of `pactl list sinks` (long
/// form). Each sink block starts with a `Sink #N` header. Pure so it's
/// unit-testable without a running PulseAudio/PipeWire server. Returns
/// `(name, description)` in listing order; the name is the id `set-default-sink`
/// expects, the description is the human-readable label.
#[cfg(any(target_os = "linux", test))]
pub fn parse_pactl_sinks(output: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut name: Option<String> = None;
    let mut desc: Option<String> = None;
    let flush = |out: &mut Vec<(String, String)>, n: &mut Option<String>, d: &mut Option<String>| {
        if let Some(name) = n.take() {
            let description = d.take().filter(|s| !s.is_empty()).unwrap_or_else(|| name.clone());
            out.push((name, description));
        } else {
            *d = None;
        }
    };
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Sink #") {
            flush(&mut out, &mut name, &mut desc);
        } else if let Some(rest) = trimmed.strip_prefix("Name:") {
            name = Some(rest.trim().to_string());
        } else if let Some(rest) = trimmed.strip_prefix("Description:") {
            desc = Some(rest.trim().to_string());
        }
    }
    flush(&mut out, &mut name, &mut desc);
    out
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{parse_pactl_sinks, AudioDevice};

    fn pactl(args: &[&str]) -> Result<String, String> {
        let out = std::process::Command::new("pactl")
            .args(args)
            .output()
            .map_err(|e| format!("pactl not available ({e}); install pulseaudio-utils"))?;
        if !out.status.success() {
            return Err(format!(
                "pactl {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    pub fn list_outputs() -> Result<Vec<AudioDevice>, String> {
        let default = pactl(&["get-default-sink"]).unwrap_or_default();
        let default = default.trim();
        let sinks = parse_pactl_sinks(&pactl(&["list", "sinks"])?);
        Ok(sinks
            .into_iter()
            .map(|(name, description)| AudioDevice {
                is_default: name == default,
                id: name,
                name: description,
            })
            .collect())
    }

    pub fn set_output(id: &str) -> Result<(), String> {
        pactl(&["set-default-sink", id]).map(|_| ())
    }
}

#[cfg(test)]
mod pactl_tests {
    use super::parse_pactl_sinks;

    #[test]
    fn parses_name_and_description_pairs() {
        let sample = "\
Sink #0
	State: RUNNING
	Name: alsa_output.pci-0000_00_1f.3.analog-stereo
	Description: Built-in Audio Analog Stereo
	Mute: no
Sink #1
	State: SUSPENDED
	Name: alsa_output.usb-Generic-00.analog-stereo
	Description: USB Headset
";
        let sinks = parse_pactl_sinks(sample);
        assert_eq!(sinks.len(), 2);
        assert_eq!(sinks[0].0, "alsa_output.pci-0000_00_1f.3.analog-stereo");
        assert_eq!(sinks[0].1, "Built-in Audio Analog Stereo");
        assert_eq!(sinks[1].0, "alsa_output.usb-Generic-00.analog-stereo");
        assert_eq!(sinks[1].1, "USB Headset");
    }

    #[test]
    fn falls_back_to_name_when_description_missing() {
        let sample = "Sink #0\n\tName: only_name\n";
        let sinks = parse_pactl_sinks(sample);
        assert_eq!(sinks, vec![("only_name".to_string(), "only_name".to_string())]);
    }

    #[test]
    fn empty_input_yields_no_sinks() {
        assert!(parse_pactl_sinks("").is_empty());
    }
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use super::{list_outputs, set_output};

    #[test]
    fn lists_at_least_one_output_with_a_single_default() {
        // Touches real CoreAudio (read-only). Every Mac has at least the
        // built-in output, so the list is non-empty and exactly one device is
        // flagged default.
        let devs = list_outputs().expect("CoreAudio enumeration");
        assert!(!devs.is_empty(), "expected at least one output device");
        let defaults = devs.iter().filter(|d| d.is_default).count();
        assert_eq!(defaults, 1, "exactly one default output expected");
        for d in &devs {
            assert!(!d.id.is_empty());
            assert!(!d.name.is_empty());
            assert!(d.id.parse::<u32>().is_ok(), "macOS id is a numeric AudioDeviceID");
        }
    }

    #[test]
    fn set_output_to_current_default_is_a_noop_ok() {
        // Exercise the set path without disturbing the user's audio: re-set the
        // default to whatever it already is. Validates the CoreAudio write FFI.
        let devs = list_outputs().expect("CoreAudio enumeration");
        let def = devs.iter().find(|d| d.is_default).expect("a default device");
        set_output(&def.id).expect("set default output (no-op) should succeed");
        // Still exactly one default, unchanged.
        let after = list_outputs().unwrap();
        assert_eq!(after.iter().filter(|d| d.is_default).count(), 1);
    }

    #[test]
    fn set_output_rejects_a_bad_id() {
        assert!(set_output("not-a-number").is_err());
    }
}

// ── macOS — CoreAudio ────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod macos {
    use super::AudioDevice;
    use std::ffi::c_void;
    use std::os::raw::c_char;

    type OSStatus = i32;
    type AudioObjectID = u32;
    type CFStringRef = *const c_void;

    const SYSTEM_OBJECT: AudioObjectID = 1; // kAudioObjectSystemObject
    const SCOPE_GLOBAL: u32 = u32_fourcc(b"glob"); // kAudioObjectPropertyScopeGlobal
    const SCOPE_OUTPUT: u32 = u32_fourcc(b"outp"); // kAudioObjectPropertyScopeOutput
    const ELEMENT_MAIN: u32 = 0; // kAudioObjectPropertyElementMain
    const PROP_DEVICES: u32 = u32_fourcc(b"dev#"); // kAudioHardwarePropertyDevices
    const PROP_DEFAULT_OUTPUT: u32 = u32_fourcc(b"dOut"); // kAudioHardwarePropertyDefaultOutputDevice
    const PROP_NAME: u32 = u32_fourcc(b"lnam"); // kAudioObjectPropertyName
    const PROP_STREAM_CONFIG: u32 = u32_fourcc(b"slay"); // kAudioDevicePropertyStreamConfiguration
    const CF_UTF8: u32 = 0x0800_0100; // kCFStringEncodingUTF8

    const fn u32_fourcc(s: &[u8; 4]) -> u32 {
        ((s[0] as u32) << 24) | ((s[1] as u32) << 16) | ((s[2] as u32) << 8) | (s[3] as u32)
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
        buffers: [AudioBuffer; 1], // variable-length; we index by count below
    }

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
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, size: isize, enc: u32) -> bool;
        fn CFRelease(cf: *const c_void);
    }

    fn addr(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            selector,
            scope,
            element: ELEMENT_MAIN,
        }
    }

    fn device_name(dev: AudioObjectID) -> Option<String> {
        let a = addr(PROP_NAME, SCOPE_GLOBAL);
        let mut cfstr: CFStringRef = std::ptr::null();
        let mut size = std::mem::size_of::<CFStringRef>() as u32;
        let st = unsafe {
            AudioObjectGetPropertyData(
                dev,
                &a,
                0,
                std::ptr::null(),
                &mut size,
                &mut cfstr as *mut _ as *mut c_void,
            )
        };
        if st != 0 || cfstr.is_null() {
            return None;
        }
        let mut buf = [0i8; 256];
        let ok = unsafe { CFStringGetCString(cfstr, buf.as_mut_ptr(), 256, CF_UTF8) };
        unsafe { CFRelease(cfstr as *const c_void) };
        if !ok {
            return None;
        }
        let cstr = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) };
        Some(cstr.to_string_lossy().into_owned())
    }

    /// True if the device exposes at least one output channel.
    fn has_output(dev: AudioObjectID) -> bool {
        let a = addr(PROP_STREAM_CONFIG, SCOPE_OUTPUT);
        let mut size: u32 = 0;
        let st = unsafe {
            AudioObjectGetPropertyDataSize(dev, &a, 0, std::ptr::null(), &mut size)
        };
        if st != 0 || size == 0 {
            return false;
        }
        let mut buf = vec![0u8; size as usize];
        let st = unsafe {
            AudioObjectGetPropertyData(
                dev,
                &a,
                0,
                std::ptr::null(),
                &mut size,
                buf.as_mut_ptr() as *mut c_void,
            )
        };
        if st != 0 {
            return false;
        }
        let list = buf.as_ptr() as *const AudioBufferList;
        let n = unsafe { (*list).number_buffers } as usize;
        let first = unsafe { &(*list).buffers[0] } as *const AudioBuffer;
        let mut channels = 0u32;
        for i in 0..n {
            channels += unsafe { (*first.add(i)).number_channels };
        }
        channels > 0
    }

    fn default_output() -> AudioObjectID {
        let a = addr(PROP_DEFAULT_OUTPUT, SCOPE_GLOBAL);
        let mut dev: AudioObjectID = 0;
        let mut size = std::mem::size_of::<AudioObjectID>() as u32;
        unsafe {
            AudioObjectGetPropertyData(
                SYSTEM_OBJECT,
                &a,
                0,
                std::ptr::null(),
                &mut size,
                &mut dev as *mut _ as *mut c_void,
            );
        }
        dev
    }

    pub fn list_outputs() -> Result<Vec<AudioDevice>, String> {
        let a = addr(PROP_DEVICES, SCOPE_GLOBAL);
        let mut size: u32 = 0;
        let st =
            unsafe { AudioObjectGetPropertyDataSize(SYSTEM_OBJECT, &a, 0, std::ptr::null(), &mut size) };
        if st != 0 {
            return Err(format!("CoreAudio device list size failed: {st}"));
        }
        let count = (size as usize) / std::mem::size_of::<AudioObjectID>();
        let mut ids = vec![0u32; count];
        let st = unsafe {
            AudioObjectGetPropertyData(
                SYSTEM_OBJECT,
                &a,
                0,
                std::ptr::null(),
                &mut size,
                ids.as_mut_ptr() as *mut c_void,
            )
        };
        if st != 0 {
            return Err(format!("CoreAudio device list failed: {st}"));
        }
        let def = default_output();
        let mut out = Vec::new();
        for dev in ids {
            if !has_output(dev) {
                continue;
            }
            let name = device_name(dev).unwrap_or_else(|| format!("Device {dev}"));
            out.push(AudioDevice {
                id: dev.to_string(),
                name,
                is_default: dev == def,
            });
        }
        Ok(out)
    }

    pub fn set_output(id: &str) -> Result<(), String> {
        let dev: AudioObjectID = id.parse().map_err(|_| format!("bad device id: {id}"))?;
        let a = addr(PROP_DEFAULT_OUTPUT, SCOPE_GLOBAL);
        let st = unsafe {
            AudioObjectSetPropertyData(
                SYSTEM_OBJECT,
                &a,
                0,
                std::ptr::null(),
                std::mem::size_of::<AudioObjectID>() as u32,
                &dev as *const _ as *const c_void,
            )
        };
        if st != 0 {
            return Err(format!("set default output failed: {st}"));
        }
        Ok(())
    }
}

// ── Windows — MMDevice + IPolicyConfig ───────────────────────────────────────

#[cfg(target_os = "windows")]
mod win {
    //! **Runtime-unverified on real Windows.** Listing uses the public
    //! MMDevice API; setting the default uses the undocumented `IPolicyConfig`
    //! created via raw `ole32!CoCreateInstance` directly on its IID (no manual
    //! QueryInterface) and a hand-declared vtable — exactly what NirCmd /
    //! SoundVolumeView do, since Windows ships no public "set default" API.

    use super::AudioDevice;
    use std::ffi::c_void;
    use windows::core::{GUID, PCWSTR, PWSTR};
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED, STGM_READ,
    };

    // CLSID_CPolicyConfigClient / IID_IPolicyConfig (Vista+; the reverse-
    // engineered values used by every "set default audio device" tool).
    const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
    const IID_IPOLICY_CONFIG: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
    // ERole: Console, Multimedia, Communications.
    const ROLES: [i32; 3] = [0, 1, 2];

    // Only the methods we touch are typed; the rest are opaque slots so the
    // vtable offsets line up. SetDefaultEndpoint is the 14th entry (3 IUnknown
    // + 10 IPolicyConfig methods before it).
    #[repr(C)]
    struct IPolicyConfigVtbl {
        query_interface:
            unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
        add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        get_mix_format: unsafe extern "system" fn() -> i32,
        get_device_format: unsafe extern "system" fn() -> i32,
        reset_device_format: unsafe extern "system" fn() -> i32,
        set_device_format: unsafe extern "system" fn() -> i32,
        get_processing_period: unsafe extern "system" fn() -> i32,
        set_processing_period: unsafe extern "system" fn() -> i32,
        get_share_mode: unsafe extern "system" fn() -> i32,
        set_share_mode: unsafe extern "system" fn() -> i32,
        get_property_value: unsafe extern "system" fn() -> i32,
        set_property_value: unsafe extern "system" fn() -> i32,
        set_default_endpoint: unsafe extern "system" fn(*mut c_void, PCWSTR, i32) -> i32,
        set_endpoint_visibility: unsafe extern "system" fn() -> i32,
    }
    #[repr(C)]
    struct IPolicyConfig {
        vtbl: *const IPolicyConfigVtbl,
    }

    // Raw, untyped CoCreateInstance — distinct from the `windows` crate's
    // typed generic one (used above for the enumerator) so we can create the
    // hand-declared IPolicyConfig directly on its IID.
    #[link(name = "ole32")]
    extern "system" {
        #[link_name = "CoCreateInstance"]
        fn co_create_instance_raw(
            rclsid: *const GUID,
            punkouter: *mut c_void,
            dwclscontext: u32,
            riid: *const GUID,
            ppv: *mut *mut c_void,
        ) -> i32;
    }

    fn ensure_com() {
        unsafe {
            // Ignore RPC_E_CHANGED_MODE — another part of the app may already
            // have initialised COM with a different model.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
    }

    fn pwstr_to_string(p: PWSTR) -> String {
        if p.is_null() {
            return String::new();
        }
        unsafe {
            let mut len = 0usize;
            while *p.0.add(len) != 0 {
                len += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(p.0, len))
        }
    }

    pub fn list_outputs() -> Result<Vec<AudioDevice>, String> {
        ensure_com();
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| format!("MMDeviceEnumerator: {e}"))?;
            let default_id = enumerator
                .GetDefaultAudioEndpoint(eRender, eMultimedia)
                .and_then(|d| d.GetId())
                .map(pwstr_to_string)
                .unwrap_or_default();
            let coll = enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
                .map_err(|e| format!("EnumAudioEndpoints: {e}"))?;
            let n = coll.GetCount().map_err(|e| format!("GetCount: {e}"))?;
            let mut out = Vec::new();
            for i in 0..n {
                let Ok(dev) = coll.Item(i) else { continue };
                let id = dev.GetId().map(pwstr_to_string).unwrap_or_default();
                let name = dev
                    .OpenPropertyStore(STGM_READ)
                    .ok()
                    .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName).ok())
                    .and_then(|prop| PropVariantToStringAlloc(&prop).ok())
                    .map(pwstr_to_string)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "Audio device".to_string());
                let is_default = !id.is_empty() && id == default_id;
                out.push(AudioDevice { id, name, is_default });
            }
            Ok(out)
        }
    }

    pub fn set_output(id: &str) -> Result<(), String> {
        ensure_com();
        let wide: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let mut raw: *mut c_void = std::ptr::null_mut();
            let hr = co_create_instance_raw(
                &CLSID_POLICY_CONFIG,
                std::ptr::null_mut(),
                CLSCTX_ALL.0,
                &IID_IPOLICY_CONFIG,
                &mut raw,
            );
            if hr < 0 || raw.is_null() {
                return Err(format!("IPolicyConfig create failed: 0x{hr:08x}"));
            }
            let vtbl = (*(raw as *mut IPolicyConfig)).vtbl;
            let set = (*vtbl).set_default_endpoint;
            let release = (*vtbl).release;
            let pcwstr = PCWSTR(wide.as_ptr());
            let mut result = Ok(());
            for role in ROLES {
                let hr = set(raw, pcwstr, role);
                if hr < 0 {
                    result = Err(format!("SetDefaultEndpoint failed: 0x{hr:08x}"));
                    break;
                }
            }
            release(raw);
            result
        }
    }
}
