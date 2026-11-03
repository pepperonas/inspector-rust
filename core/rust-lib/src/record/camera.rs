//! `record` stage 5 — the webcam: quality, devices, permission (and, later, its capture).
//!
//! The pure part (quality ladder, device choice, the types the settings carry) compiles on
//! every target, because `RecordConfig`/`StartOptions` hold it. The AVFoundation edge lives
//! in the macOS-only `mod mac` below; nothing here opens a capture session yet.

use serde::{Deserialize, Serialize};

/// The camera quality the user asks for. "Ideal", never forced: the device picks the
/// nearest format it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CameraQuality {
    #[default]
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "1440p")]
    P1440,
    #[serde(rename = "2160p")]
    P2160,
}

impl CameraQuality {
    /// The frame size this quality asks the device for.
    pub fn ideal_size(self) -> (u32, u32) {
        match self {
            Self::P1080 => (1920, 1080),
            Self::P1440 => (2560, 1440),
            Self::P2160 => (3840, 2160),
        }
    }

    /// Stored settings value → quality; anything unknown is the default.
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "1440p" => Self::P1440,
            "2160p" => Self::P2160,
            _ => Self::P1080,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::P1080 => "1080p",
            Self::P1440 => "1440p",
            Self::P2160 => "2160p",
        }
    }
}

/// H.264 bitrate for a camera frame of `w`×`h`, measured on the shorter side (OpenScreen's
/// `webcamBitrateFor`): ≥ 2160 → 40 Mb/s, ≥ 1440 → 24, ≥ 1080 → 16, ≥ 720 → 8, else 4.
pub fn camera_bitrate(w: u32, h: u32) -> u32 {
    match w.min(h) {
        s if s >= 2160 => 40_000_000,
        s if s >= 1440 => 24_000_000,
        s if s >= 1080 => 16_000_000,
        s if s >= 720 => 8_000_000,
        _ => 4_000_000,
    }
}

/// A video capture device as the HUD and settings show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraDevice {
    /// `AVCaptureDevice.uniqueID` — stable across launches, stored in the settings.
    pub id: String,
    pub name: String,
    /// The Mac's own camera (`AVCaptureDeviceTypeBuiltInWideAngleCamera`).
    pub builtin: bool,
}

/// Camera permission (TCC) as AVFoundation reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CameraAuth {
    Authorized,
    Denied,
    Restricted,
    NotDetermined,
}

/// The device a recording uses: the wanted id, else the built-in camera, else the first.
pub fn pick_device<'a>(devices: &'a [CameraDevice], wanted: &str) -> Option<&'a CameraDevice> {
    let wanted = wanted.trim();
    (!wanted.is_empty())
        .then(|| devices.iter().find(|d| d.id == wanted))
        .flatten()
        .or_else(|| devices.iter().find(|d| d.builtin))
        .or_else(|| devices.first())
}

#[cfg(target_os = "macos")]
pub use mac::{authorization, devices, request_access};

#[cfg(target_os = "macos")]
mod mac {
    use super::{CameraAuth, CameraDevice};
    use block2::RcBlock;
    use objc2::rc::autoreleasepool;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{
        AVAuthorizationStatus, AVCaptureDevice, AVCaptureDeviceTypeBuiltInWideAngleCamera, AVMediaTypeVideo,
    };
    use std::sync::mpsc;
    use std::time::Duration;

    /// How long `request_access` waits for the user to answer the system dialog.
    const REQUEST_WAIT: Duration = Duration::from_secs(300);

    fn map(status: AVAuthorizationStatus) -> CameraAuth {
        match status {
            AVAuthorizationStatus::Authorized => CameraAuth::Authorized,
            AVAuthorizationStatus::Denied => CameraAuth::Denied,
            AVAuthorizationStatus::Restricted => CameraAuth::Restricted,
            _ => CameraAuth::NotDetermined,
        }
    }

    /// The camera permission. Reading it never shows a dialog.
    pub fn authorization() -> CameraAuth {
        // SAFETY: `AVMediaTypeVideo` is a framework-provided static NSString, valid for the
        // process lifetime; with a video/audio media type the class method does not throw.
        let media = unsafe { AVMediaTypeVideo }.expect("AVMediaTypeVideo is always exported");
        // SAFETY: plain class-method read with a valid media type (see above); the pool
        // drains anything AVFoundation autoreleases on this non-main thread.
        autoreleasepool(|_| map(unsafe { AVCaptureDevice::authorizationStatusForMediaType(media) }))
    }

    /// Asks for camera access when it is not decided yet and waits for the answer; a decided
    /// permission is returned as it is (macOS only asks once — afterwards only Settings help).
    /// Needs `NSCameraUsageDescription` in the bundle, else TCC ends the process.
    pub fn request_access() -> CameraAuth {
        let now = authorization();
        if now != CameraAuth::NotDetermined {
            return now;
        }
        let (tx, rx) = mpsc::channel::<bool>();
        let block = RcBlock::new(move |granted: Bool| {
            let _ = tx.send(granted.as_bool());
        });
        // SAFETY: see `authorization` for the media type.
        let media = unsafe { AVMediaTypeVideo }.expect("AVMediaTypeVideo is always exported");
        // SAFETY: valid media type; AVFoundation copies the block and calls it once on an
        // arbitrary queue — the closure only sends on a channel (Send + Sync), no UI work.
        autoreleasepool(|_| unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(media, &block) });
        match rx.recv_timeout(REQUEST_WAIT) {
            Ok(true) => CameraAuth::Authorized,
            // A "no" or a dialog left open: report what TCC says now.
            _ => authorization(),
        }
    }

    /// Every video capture device (built-in and external). Listing needs no permission.
    pub fn devices() -> Vec<CameraDevice> {
        // SAFETY: framework statics, valid for the process lifetime (see `authorization`).
        let media = unsafe { AVMediaTypeVideo }.expect("AVMediaTypeVideo is always exported");
        // SAFETY: as above; the type constant exists since macOS 10.15 (our minimum).
        let builtin_type = unsafe { AVCaptureDeviceTypeBuiltInWideAngleCamera };
        // `devicesWithMediaType:` instead of a discovery session: the external-camera type
        // constant is macOS 14+ and would be a hard link against older systems.
        autoreleasepool(|_| {
            #[allow(deprecated)]
            // SAFETY: class method with a valid media type; returns an NSArray we retain. The
            // pool drains the autoreleased objects of this (non-main) thread.
            let list = unsafe { AVCaptureDevice::devicesWithMediaType(media) };
            list.iter()
                .map(|d| {
                    // SAFETY: `d` is a live AVCaptureDevice from the array; these getters have
                    // no preconditions and return retained NSStrings.
                    let (id, name, kind) = unsafe { (d.uniqueID(), d.localizedName(), d.deviceType()) };
                    CameraDevice { id: id.to_string(), name: name.to_string(), builtin: *kind == *builtin_type }
                })
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, builtin: bool) -> CameraDevice {
        CameraDevice { id: id.into(), name: format!("cam {id}"), builtin }
    }

    #[test]
    fn camera_bitrate_ladder() {
        assert_eq!(camera_bitrate(3840, 2160), 40_000_000);
        assert_eq!(camera_bitrate(2560, 1440), 24_000_000);
        assert_eq!(camera_bitrate(1920, 1080), 16_000_000);
        assert_eq!(camera_bitrate(1280, 720), 8_000_000);
        assert_eq!(camera_bitrate(640, 480), 4_000_000);
        // Measured on the shorter side, so a portrait frame lands on the same rung.
        assert_eq!(camera_bitrate(1080, 1920), 16_000_000);
        assert_eq!(camera_bitrate(1920, 1079), 8_000_000, "just under 1080 drops a rung");
    }

    #[test]
    fn quality_sizes_and_names() {
        assert_eq!(CameraQuality::P1080.ideal_size(), (1920, 1080));
        assert_eq!(CameraQuality::P1440.ideal_size(), (2560, 1440));
        assert_eq!(CameraQuality::P2160.ideal_size(), (3840, 2160));
        assert_eq!(CameraQuality::default(), CameraQuality::P1080);
        for q in [CameraQuality::P1080, CameraQuality::P1440, CameraQuality::P2160] {
            assert_eq!(CameraQuality::parse(q.as_str()), q);
            assert_eq!(serde_json::to_string(&q).unwrap(), format!("\"{}\"", q.as_str()));
        }
        assert_eq!(CameraQuality::parse("8k"), CameraQuality::P1080, "unknown → default");
        assert_eq!(serde_json::to_string(&CameraAuth::NotDetermined).unwrap(), "\"not-determined\"");
    }

    #[test]
    fn pick_device_prefers_wanted_then_builtin() {
        let list = [dev("usb", false), dev("mac", true), dev("desk", false)];
        assert_eq!(pick_device(&list, "desk").unwrap().id, "desk", "the wanted id wins");
        assert_eq!(pick_device(&list, "").unwrap().id, "mac", "no wish → built-in");
        assert_eq!(pick_device(&list, "gone").unwrap().id, "mac", "unplugged wish → built-in");
        let no_builtin = [dev("usb", false), dev("desk", false)];
        assert_eq!(pick_device(&no_builtin, "gone").unwrap().id, "usb", "else the first");
    }

    #[test]
    fn pick_device_empty_is_none() {
        assert!(pick_device(&[], "").is_none());
        assert!(pick_device(&[], "mac").is_none());
    }

    /// Live: lists the cameras and reads the permission (never asks — the test binary has no
    /// usage string, a request would make TCC end the process).
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn camera_devices_live() {
        let auth = authorization();
        let list = devices();
        println!("auth {auth:?}, {} device(s): {list:?}", list.len());
        assert!(list.iter().any(|d| d.builtin), "expected the built-in camera");
    }
}
