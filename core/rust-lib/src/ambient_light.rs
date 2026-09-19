//! Ambient-light sensor readings for the `lumen` command.
//!
//! The sensor is optional hardware. macOS exposes supported built-in sensors
//! through IOKit's `CurrentLux` registry property; Linux commonly exposes an
//! illuminance channel through IIO sysfs. Windows has no generally available,
//! permission-free desktop API in the current backend, so it reports no value.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AmbientLightReading {
    pub lux: f64,
    pub source: String,
}

pub fn read() -> Option<AmbientLightReading> {
    #[cfg(target_os = "macos")]
    {
        macos::read()
    }
    #[cfg(target_os = "linux")]
    {
        linux::read()
    }
    #[cfg(target_os = "windows")]
    {
        None
    }
}

fn reading(lux: f64, source: impl Into<String>) -> Option<AmbientLightReading> {
    if !lux.is_finite() || lux < 0.0 {
        return None;
    }
    Some(AmbientLightReading {
        lux: (lux * 10.0).round() / 10.0,
        source: source.into(),
    })
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{reading, AmbientLightReading};
    use std::ffi::{c_char, c_void};

    type CfTypeRef = *const c_void;
    type CfStringRef = *const c_void;

    const UTF8: u32 = 0x0800_0100;
    const CF_NUMBER_DOUBLE: i32 = 13;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOServiceMatching(name: *const c_char) -> *mut c_void;
        fn IOServiceGetMatchingService(master: u32, matching: *mut c_void) -> u32;
        fn IORegistryEntryCreateCFProperty(
            entry: u32,
            key: CfStringRef,
            allocator: *const c_void,
            options: u32,
        ) -> CfTypeRef;
        fn IOObjectRelease(obj: u32) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            value: *const c_char,
            encoding: u32,
        ) -> CfStringRef;
        fn CFGetTypeID(value: CfTypeRef) -> usize;
        fn CFNumberGetTypeID() -> usize;
        fn CFNumberGetValue(number: CfTypeRef, number_type: i32, value: *mut c_void) -> bool;
        fn CFRelease(value: CfTypeRef);
    }

    pub(super) fn read() -> Option<AmbientLightReading> {
        // Modern Apple-Silicon machines use the VD628x colour/ALS driver;
        // older Intel Macs commonly expose the same property on TCS3490/LMU.
        for class in [
            c"AppleSPUVD6286",
            c"AppleSPUVD6287",
            c"AppleTCS3490",
            c"AppleLMUController",
        ] {
            if let Some(lux) = read_property(class.as_ptr(), c"CurrentLux".as_ptr()) {
                return reading(lux, "Ambient light sensor");
            }
        }
        None
    }

    fn read_property(class: *const c_char, property: *const c_char) -> Option<f64> {
        unsafe {
            let matching = IOServiceMatching(class);
            if matching.is_null() {
                return None;
            }
            // IOServiceGetMatchingService consumes the matching dictionary.
            let service = IOServiceGetMatchingService(0, matching);
            if service == 0 {
                return None;
            }
            let key = CFStringCreateWithCString(std::ptr::null(), property, UTF8);
            if key.is_null() {
                IOObjectRelease(service);
                return None;
            }
            let value = IORegistryEntryCreateCFProperty(service, key, std::ptr::null(), 0);
            CFRelease(key);
            IOObjectRelease(service);
            if value.is_null() {
                return None;
            }
            let mut lux = 0.0_f64;
            let ok = CFGetTypeID(value) == CFNumberGetTypeID()
                && CFNumberGetValue(value, CF_NUMBER_DOUBLE, &mut lux as *mut f64 as *mut c_void);
            CFRelease(value);
            ok.then_some(lux)
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{reading, AmbientLightReading};
    use std::fs;
    use std::path::Path;

    pub(super) fn read() -> Option<AmbientLightReading> {
        let root = Path::new("/sys/bus/iio/devices");
        let devices = fs::read_dir(root).ok()?;
        for device in devices.flatten() {
            let path = device.path();
            if let Some(lux) = read_number(&path.join("in_illuminance_input")) {
                return reading(lux, sensor_name(&path));
            }
            if let Some(raw) = read_number(&path.join("in_illuminance_raw")) {
                let scale = read_number(&path.join("in_illuminance_scale")).unwrap_or(1.0);
                if let Some(value) = reading(raw * scale, sensor_name(&path)) {
                    return Some(value);
                }
            }
        }
        None
    }

    fn read_number(path: &Path) -> Option<f64> {
        fs::read_to_string(path).ok()?.trim().parse().ok()
    }

    fn sensor_name(path: &Path) -> String {
        fs::read_to_string(path.join("name"))
            .ok()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Ambient light sensor".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_zero_and_rounds_to_one_decimal() {
        assert_eq!(reading(0.0, "ALS").unwrap().lux, 0.0);
        assert_eq!(reading(12.36, "ALS").unwrap().lux, 12.4);
    }

    #[test]
    fn rejects_invalid_readings() {
        assert!(reading(-0.1, "ALS").is_none());
        assert!(reading(f64::NAN, "ALS").is_none());
        assert!(reading(f64::INFINITY, "ALS").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires a Mac with an exposed ambient-light sensor"]
    fn print_live_reading() {
        let value = read().expect("ambient-light sensor should be available");
        eprintln!("live ambient light: {value:?}");
    }
}
