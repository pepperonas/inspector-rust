//! Bluetooth device management — the `bluetooth` / `bt` command (v0.159.0,
//! macOS).
//!
//! List paired devices with their live connection state, disconnect/connect
//! one, and unpair ("remove") it. Talks to the **IOBluetooth framework via
//! FFI** (linked in build.rs), the same house pattern as CoreAudio/AX — no
//! external tool: macOS ships no Bluetooth CLI and `blueutil` is a Homebrew
//! install this app must not depend on.
//!
//! ⚠️ **Unpairing has NO public API.** `-[IOBluetoothDevice remove]` is a
//! private selector (the same one `blueutil` uses). House precedent for
//! private API exists (MultitouchSupport for gestures), but the call is
//! guarded with `respondsToSelector:` and fails with a clear message rather
//! than crashing if Apple ever drops it.
//!
//! ⚠️ **A device can be paired TWICE** (classic BR/EDR + LE — measured live:
//! the reference speaker holds two MAC addresses). The address is therefore
//! part of the surface, never just the name.
//!
//! House style: the pure logic (address normalisation, sorting, class
//! labels) is free functions with tests; the impure FFI shell stays thin.

#![allow(dead_code)] // non-macOS builds see only the pure helpers

/// One paired device, as the panel shows it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BtDevice {
    pub name: String,
    /// Normalised `aa:bb:cc:dd:ee:ff` (IOBluetooth reports dashes).
    pub address: String,
    pub connected: bool,
    /// Human label from the Class-of-Device major ("Audio", "Eingabegerät",
    /// …). ⚠️ LE devices broadcast NO class (major 0 — measured: an MX Master
    /// reports 0/0), so this is best-effort and never invented: unknown stays
    /// "Gerät".
    pub kind: String,
}

pub const ERR_NOT_FOUND: &str = "bluetooth.device_not_found";
pub const ERR_NO_REMOVE: &str = "bluetooth.remove_unavailable";

/// Normalise an address for comparison/display: lowercase, dashes → colons.
/// IOBluetooth prints `f4-2b-7d-14-ed-c0`, system_profiler `F4:2B:7D:14:ED:C0`
/// — one canonical form or lookups quietly miss.
pub fn normalize_addr(raw: &str) -> String {
    raw.trim().to_lowercase().replace('-', ":")
}

/// Human label for a Class-of-Device major number. 0 = the device did not
/// say (typical for LE) — an honest generic, never a guess.
pub fn class_label(major: u32) -> &'static str {
    match major {
        1 => "Computer",
        2 => "Telefon",
        3 => "Netzwerk",
        4 => "Audio",
        5 => "Eingabegerät",
        6 => "Bildgebung",
        7 => "Wearable",
        _ => "Gerät",
    }
}

/// Sort for the panel: connected first, then by name, then by address so the
/// twice-paired speaker (classic + LE) keeps a stable order.
pub fn sort_devices(devices: &mut [BtDevice]) {
    devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.address.cmp(&b.address))
    });
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{class_label, normalize_addr, BtDevice, ERR_NOT_FOUND, ERR_NO_REMOVE};
    use objc2::runtime::{AnyClass, AnyObject, Sel};
    use objc2::msg_send;

    unsafe fn ns_string_to_rust(s: *mut AnyObject) -> String {
        if s.is_null() {
            return String::new();
        }
        let c: *const std::os::raw::c_char = msg_send![s, UTF8String];
        if c.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr(c).to_string_lossy().into_owned()
    }

    /// All paired devices. IOBluetooth's sync API — cheap (no radio scan).
    pub fn list() -> Result<Vec<BtDevice>, String> {
        unsafe {
            let cls = AnyClass::get(c"IOBluetoothDevice")
                .ok_or("IOBluetooth nicht verfügbar")?;
            let arr: *mut AnyObject = msg_send![cls, pairedDevices];
            if arr.is_null() {
                return Ok(Vec::new());
            }
            let count: usize = msg_send![arr, count];
            let mut out = Vec::with_capacity(count);
            for i in 0..count {
                let d: *mut AnyObject = msg_send![arr, objectAtIndex: i];
                if d.is_null() {
                    continue;
                }
                let name: *mut AnyObject = msg_send![d, name];
                let addr: *mut AnyObject = msg_send![d, addressString];
                let connected: bool = msg_send![d, isConnected];
                let major: u32 = msg_send![d, deviceClassMajor];
                let address = normalize_addr(&ns_string_to_rust(addr));
                let mut name = ns_string_to_rust(name);
                if name.is_empty() {
                    // A nameless row would be unusable next to its twin —
                    // the address is the honest fallback.
                    name = address.clone();
                }
                out.push(BtDevice {
                    name,
                    address,
                    connected,
                    kind: class_label(major).to_string(),
                });
            }
            super::sort_devices(&mut out);
            Ok(out)
        }
    }

    /// Find the paired device with `address` (normalised compare).
    unsafe fn device_for(address: &str) -> Result<*mut AnyObject, String> {
        let want = normalize_addr(address);
        let cls = AnyClass::get(c"IOBluetoothDevice").ok_or("IOBluetooth nicht verfügbar")?;
        let arr: *mut AnyObject = msg_send![cls, pairedDevices];
        if arr.is_null() {
            return Err(ERR_NOT_FOUND.into());
        }
        let count: usize = msg_send![arr, count];
        for i in 0..count {
            let d: *mut AnyObject = msg_send![arr, objectAtIndex: i];
            if d.is_null() {
                continue;
            }
            let addr: *mut AnyObject = msg_send![d, addressString];
            if normalize_addr(&ns_string_to_rust(addr)) == want {
                return Ok(d);
            }
        }
        Err(ERR_NOT_FOUND.into())
    }

    /// Open the baseband connection. ⚠️ BLOCKS until connected or the OS
    /// timeout fires (an off device takes ~10 s to fail) — callers run this on
    /// `spawn_blocking`, never the main thread.
    pub fn connect(address: &str) -> Result<(), String> {
        unsafe {
            let d = device_for(address)?;
            let rc: i32 = msg_send![d, openConnection];
            if rc == 0 {
                Ok(())
            } else {
                Err(format!("Verbinden fehlgeschlagen (IOReturn {rc}) — ist das Gerät eingeschaltet?"))
            }
        }
    }

    pub fn disconnect(address: &str) -> Result<(), String> {
        unsafe {
            let d = device_for(address)?;
            let rc: i32 = msg_send![d, closeConnection];
            if rc == 0 {
                Ok(())
            } else {
                Err(format!("Trennen fehlgeschlagen (IOReturn {rc})"))
            }
        }
    }

    /// Unpair. Private `remove` selector — guarded, and the pairing record is
    /// gone afterwards (re-pairing needs the device's pairing mode again).
    pub fn unpair(address: &str) -> Result<(), String> {
        unsafe {
            let d = device_for(address)?;
            let sel = Sel::register(c"remove");
            let responds: bool = msg_send![d, respondsToSelector: sel];
            if !responds {
                // Apple removed the private selector — say so instead of
                // crashing or silently doing nothing.
                return Err(ERR_NO_REMOVE.into());
            }
            let _: () = msg_send![d, remove];
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::{connect, disconnect, list, unpair};

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "live: talks to the real IOBluetooth stack"]
    #[cfg(target_os = "macos")]
    fn live_list_matches_the_system() {
        let devices = super::list().expect("list");
        assert!(!devices.is_empty(), "this machine has paired devices");
        for d in &devices {
            println!("{} | {} | connected={} | {}", d.name, d.address, d.connected, d.kind);
            assert!(d.address.contains(':'), "normalised address");
        }
    }

    use super::*;

    fn dev(name: &str, address: &str, connected: bool) -> BtDevice {
        BtDevice { name: name.into(), address: address.into(), connected, kind: "Audio".into() }
    }

    #[test]
    fn addresses_normalise_to_one_canonical_form() {
        // ⚠️ IOBluetooth prints dashes, system_profiler colons+uppercase —
        // without one form, lookups quietly miss (measured on real output).
        assert_eq!(normalize_addr("F4:2B:7D:14:ED:C0"), "f4:2b:7d:14:ed:c0");
        assert_eq!(normalize_addr("f4-2b-7d-14-ed-c0"), "f4:2b:7d:14:ed:c0");
        assert_eq!(normalize_addr("  d3-0B-79-8C-a4-17 "), "d3:0b:79:8c:a4:17");
    }

    #[test]
    fn class_labels_are_honest_about_the_le_zero() {
        assert_eq!(class_label(4), "Audio");
        assert_eq!(class_label(5), "Eingabegerät");
        // ⚠️ LE devices broadcast no class: measured, an MX Master reports
        // major 0. That must read as a generic, never as a guessed category.
        assert_eq!(class_label(0), "Gerät");
        assert_eq!(class_label(99), "Gerät");
    }

    #[test]
    fn connected_devices_sort_first_and_twins_stay_stable() {
        let mut v = vec![
            dev("Zeta", "aa:aa:aa:aa:aa:02", false),
            // The twice-paired speaker: same name, two addresses (real case).
            dev("Boom", "f4:2b:7d:14:ed:c0", false),
            dev("Boom", "f4:2b:7d:13:78:15", false),
            dev("Maus", "d3:0b:79:8c:a4:17", true),
        ];
        sort_devices(&mut v);
        assert_eq!(v[0].name, "Maus", "connected first");
        // Twins ordered by address so the list never flickers between polls.
        assert_eq!(v[1].address, "f4:2b:7d:13:78:15");
        assert_eq!(v[2].address, "f4:2b:7d:14:ed:c0");
        assert_eq!(v[3].name, "Zeta");
    }
}
