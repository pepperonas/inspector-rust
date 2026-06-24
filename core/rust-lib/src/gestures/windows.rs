//! Windows touchpad gesture capture via Raw Input + HID Precision Touchpad.
//!
//! **Status: stub.** The architecture is laid out (Raw Input `WM_INPUT`, HID
//! contact reports, frame assembly, `Recognizer` feed) but the Windows runtime
//! path is not yet exercised.  `start()` returns an error so the feature
//! degrades gracefully on Windows — gestures just don't fire.

use super::{GestureConfig, GestureSink, GestureSource};

pub struct WindowsGestureSource;

impl WindowsGestureSource {
    pub fn new() -> Self {
        WindowsGestureSource
    }
}

impl GestureSource for WindowsGestureSource {
    fn start(&mut self, _cfg: GestureConfig, _sink: GestureSink) -> Result<(), String> {
        Err("Windows Precision Touchpad gesture capture is not yet implemented".to_string())
    }

    fn stop(&mut self) {}
}
