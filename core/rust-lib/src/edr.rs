//! EDR (Extended Dynamic Range) brightness extension for the `brightness`
//! command — macOS-only, XDR/EDR-capable displays.
//!
//! macOS clamps SDR content to ~SDR-max. EDR-capable panels (14"/16" MBP XDR,
//! Pro Display XDR) can go to ~1000 nits sustained, but only while EDR content
//! (a `CAMetalLayer` with `wantsExtendedDynamicRangeContent` showing pixels above
//! 1.0) is on screen — then macOS releases the backlight headroom. This module
//! extends the existing brightness slider above its SDR max on such displays;
//! everywhere else it's inert (the slider behaves exactly as before).
//!
//! **Phase 1 (this): runtime EDR-capability detection** (per display, via
//! `NSScreen.maximumPotentialExtendedDynamicRangeColorComponentValue`) → the
//! slider's upper bound. **Phase 2 (next): the click-through CAMetalLayer
//! overlay + render loop** that actually lifts luminance into the EDR range.
//! All native code is `#[cfg(target_os = "macos")]`; other platforms get the
//! pure helpers (which always report "not capable") and nothing else.

#![allow(dead_code)] // the overlay (phase 2) consumes more of this

/// A display is treated as EDR-capable when its potential headroom exceeds this
/// (1.0 = no headroom). A tiny margin avoids float-noise false positives.
const EDR_MIN_HEADROOM: f32 = 1.05;
/// Cap the slider's EDR upper bound (%) regardless of the raw headroom — the
/// *usable* sustained boost is far below the raw color-component headroom (which
/// can read ~16 on XDR). Tunable once verified on hardware.
const EDR_MAX_CAP: u16 = 300;

/// Map a display's potential EDR headroom to the slider's upper bound in percent
/// (100 = SDR only). Pure + unit-tested. `> EDR_MIN_HEADROOM` ⇒ the slider runs
/// past 100 up to `min(headroom·100, EDR_MAX_CAP)`; otherwise it ends at 100.
pub fn edr_max_percent(headroom: f32) -> u16 {
    if headroom > EDR_MIN_HEADROOM {
        ((headroom * 100.0).round() as u16).clamp(110, EDR_MAX_CAP)
    } else {
        100
    }
}

/// The potential EDR headroom of the display with CoreGraphics id `cg_id`
/// (1.0 = none / not EDR-capable). Reads
/// `NSScreen.maximumPotentialExtendedDynamicRangeColorComponentValue` for the
/// `NSScreen` whose `NSScreenNumber` matches.
#[cfg(target_os = "macos")]
unsafe fn nsstring(s: &str) -> *mut objc2::runtime::AnyObject {
    use objc2::msg_send;
    use objc2::runtime::AnyClass;
    let Some(cls) = AnyClass::get(c"NSString") else {
        return std::ptr::null_mut();
    };
    let Ok(cstr) = std::ffi::CString::new(s) else {
        return std::ptr::null_mut();
    };
    msg_send![cls, stringWithUTF8String: cstr.as_ptr()]
}

#[cfg(target_os = "macos")]
pub fn display_headroom(cg_id: u32) -> f32 {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    unsafe {
        let Some(ns_screen) = AnyClass::get(c"NSScreen") else {
            return 1.0;
        };
        let screens: *mut AnyObject = msg_send![ns_screen, screens];
        if screens.is_null() {
            return 1.0;
        }
        let count: usize = msg_send![screens, count];
        let key = nsstring("NSScreenNumber");
        if key.is_null() {
            return 1.0;
        }
        for i in 0..count {
            let s: *mut AnyObject = msg_send![screens, objectAtIndex: i];
            if s.is_null() {
                continue;
            }
            let desc: *mut AnyObject = msg_send![s, deviceDescription];
            if desc.is_null() {
                continue;
            }
            let num: *mut AnyObject = msg_send![desc, objectForKey: key];
            if num.is_null() {
                continue;
            }
            let screen_no: u32 = msg_send![num, unsignedIntValue];
            if screen_no == cg_id {
                let v: f64 = msg_send![s, maximumPotentialExtendedDynamicRangeColorComponentValue];
                return v as f32;
            }
        }
        1.0
    }
}

#[cfg(not(target_os = "macos"))]
pub fn display_headroom(_cg_id: u32) -> f32 {
    1.0
}

/// Drive the EDR boost for `cg_id` to `percent` (the full slider value; ≤ 100 /
/// 0 ⇒ off → tear the overlay down). Above 100 the overlay is created/updated to
/// lift luminance into the EDR range, scaled by the above-SDR amount.
#[cfg(target_os = "macos")]
pub fn set_level(app: &tauri::AppHandle, cg_id: u32, percent: u16) {
    macos::set_level(app, cg_id, percent);
}

#[cfg(not(target_os = "macos"))]
pub fn set_level(_app: &tauri::AppHandle, _cg_id: u32, _percent: u16) {}

// ── The native EDR overlay (CAMetalLayer + render loop) ──────────────────────
#[cfg(target_os = "macos")]
mod macos {
    use objc2::encode::{Encode, Encoding};
    use objc2::msg_send;
    use objc2::rc::autoreleasepool;
    use objc2::runtime::{AnyClass, AnyObject};
    use parking_lot::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::Arc;
    use tauri::AppHandle;

    // Tuning (verify/tune on EDR hardware). `above` runs 0..2 for 100..300 %.
    // Preferred path: the overlay MULTIPLIES the desktop (CIMultiplyCompositing)
    // by `factor`, so black stays black (no wash) and brights lift into EDR.
    const MULT_GAIN: f32 = 3.0; // factor = 1 + above·MULT_GAIN → up to 7× at 300 %
                                // (clamped by the display's live EDR headroom)
    // Fallback (no CoreImage / multiply unavailable): an additive white veil
    // (brightens but washes). Never an opaque grey screen.
    const VEIL_GAIN: f32 = 1.6;
    const VEIL_ALPHA: f32 = 0.14;
    const MAX_ALPHA: f32 = 0.32;
    const TICK_MS: u64 = 100; // ~10 fps; the worker ticks, the main thread draws

    // True once the layer's CIMultiplyCompositing filter is in place.
    static MULTIPLY_OK: AtomicBool = AtomicBool::new(false);

    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct NSPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct NSSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct NSRect {
        origin: NSPoint,
        size: NSSize,
    }
    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct MTLClearColor {
        red: f64,
        green: f64,
        blue: f64,
        alpha: f64,
    }
    unsafe impl Encode for NSPoint {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for NSSize {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for NSRect {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[NSPoint::ENCODING, NSSize::ENCODING]);
    }
    unsafe impl Encode for MTLClearColor {
        const ENCODING: Encoding = Encoding::Struct(
            "MTLClearColor",
            &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING],
        );
    }

    // A retained Objective-C pointer that's safe to move across threads (we only
    // ever touch each from one place at a time, serialised by `STATE`).
    struct SendPtr(*mut AnyObject);
    unsafe impl Send for SendPtr {}

    struct Overlay {
        cg_id: u32,
        target: Arc<AtomicU32>, // above-SDR ×1000 (0 = off)
        running: Arc<AtomicBool>,
        window: SendPtr,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    static STATE: Mutex<Option<Overlay>> = Mutex::new(None);
    // Set when a Metal/CA draw raises an Obj-C exception — the ticker then stops
    // (the overlay window stays but is inert) instead of letting the exception
    // unwind through the run loop and abort the whole app.
    static RENDER_FAILED: AtomicBool = AtomicBool::new(false);

    unsafe fn ns_to_string(s: *mut AnyObject) -> String {
        if s.is_null() {
            return String::new();
        }
        let utf8: *const std::os::raw::c_char = msg_send![s, UTF8String];
        if utf8.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned()
    }

    #[link(name = "Metal", kind = "framework")]
    extern "C" {
        fn MTLCreateSystemDefaultDevice() -> *mut AnyObject;
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGColorSpaceCreateWithName(name: *const std::ffi::c_void) -> *const std::ffi::c_void;
        static kCGColorSpaceExtendedLinearDisplayP3: *const std::ffi::c_void;
    }

    pub fn set_level(app: &AppHandle, cg_id: u32, percent: u16) {
        let above = ((percent as f32 - 100.0) / 100.0).clamp(0.0, 2.0);
        if percent <= 100 || above <= 0.0 {
            teardown(app);
            return;
        }
        let guard = STATE.lock();
        match guard.as_ref() {
            // Already running on this display → just update the target.
            Some(o) if o.cg_id == cg_id => {
                o.target.store((above * 1000.0) as u32, Ordering::Relaxed);
            }
            _ => {
                // Different display or none → (re)build.
                drop(guard);
                teardown(app);
                build(app, cg_id, above);
            }
        }
    }

    fn build(app: &AppHandle, cg_id: u32, above: f32) {
        RENDER_FAILED.store(false, Ordering::Relaxed);
        let target = Arc::new(AtomicU32::new((above * 1000.0) as u32));
        let running = Arc::new(AtomicBool::new(true));
        let (t2, r2) = (target.clone(), running.clone());
        #[allow(clippy::type_complexity)]
        let (tx, rx) = std::sync::mpsc::channel::<Option<(SendPtr, SendPtr, SendPtr)>>();

        // Window + layer + queue creation must all be on the main thread (AppKit
        // is main-thread-only). The render thread only ever touches the resolved
        // CAMetalLayer + MTLCommandQueue (both safe off-main) — NOT the window.
        let _ = app.run_on_main_thread(move || {
            let res = unsafe { build_window(cg_id) };
            let _ = tx.send(res.map(|(w, l, q)| (SendPtr(w), SendPtr(l), SendPtr(q))));
        });
        let Ok(Some((window, layer, queue))) = rx.recv() else {
            running.store(false, Ordering::Relaxed);
            tracing::warn!("edr: overlay window build failed");
            return;
        };
        // Render ON THE MAIN THREAD. Drawing a CAMetalLayer (nextDrawable /
        // CALayer property access) from a background thread raises an Obj-C
        // exception that unwinds through our Rust frames → SIGABRT (the crash).
        // The worker thread only ticks + dispatches each frame to the main thread.
        let layer_ptr = layer.0 as usize;
        let queue_ptr = queue.0 as usize;
        let app2 = app.clone();
        let thread = std::thread::spawn(move || {
            while r2.load(Ordering::Relaxed) {
                if RENDER_FAILED.load(Ordering::Relaxed) {
                    break; // a draw raised an exception; stop ticking
                }
                let above = t2.load(Ordering::Relaxed) as f32 / 1000.0;
                let (l, q) = (layer_ptr, queue_ptr);
                let _ = app2.run_on_main_thread(move || draw_edr_frame(l, q, above));
                std::thread::sleep(std::time::Duration::from_millis(TICK_MS));
            }
            // Release the +1 command queue on the main thread, for symmetry.
            let q = queue_ptr;
            let _ = app2.run_on_main_thread(move || unsafe {
                let _: () = msg_send![q as *mut AnyObject, release];
            });
        });
        *STATE.lock() = Some(Overlay {
            cg_id,
            target,
            running,
            window,
            thread: Some(thread),
        });
        tracing::info!("edr: overlay active on display {cg_id}");

        // Diagnostic: is the system actually GRANTING EDR headroom? Log the
        // screen's *current* headroom now + again after EDR content has been on
        // screen for 3 s. If `current` stays ~1.0, the OS isn't granting EDR
        // (e.g. Tahoe "Automatically Adjust Brightness" off) and no overlay
        // compositing can brighten — that's the root cause, not our maths.
        log_headroom(app, cg_id, "at-build");
        let app3 = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            log_headroom(&app3, cg_id, "after-3s");
        });
    }

    fn log_headroom(app: &AppHandle, cg_id: u32, when: &'static str) {
        let _ = app.run_on_main_thread(move || {
            let (cur, pot) = unsafe { screen_headrooms(cg_id) };
            tracing::info!("edr: headroom [{when}] current={cur:.2} potential={pot:.2}");
        });
    }

    /// `(current, potential)` EDR headroom of the screen with id `cg_id`. The
    /// **current** value is what's actually granted right now (1.0 = none).
    unsafe fn screen_headrooms(cg_id: u32) -> (f32, f32) {
        let Some(ns_screen) = AnyClass::get(c"NSScreen") else {
            return (1.0, 1.0);
        };
        let screens: *mut AnyObject = msg_send![ns_screen, screens];
        if screens.is_null() {
            return (1.0, 1.0);
        }
        let count: usize = msg_send![screens, count];
        let key = super::nsstring("NSScreenNumber");
        for i in 0..count {
            let s: *mut AnyObject = msg_send![screens, objectAtIndex: i];
            if s.is_null() {
                continue;
            }
            let desc: *mut AnyObject = msg_send![s, deviceDescription];
            if desc.is_null() {
                continue;
            }
            let num: *mut AnyObject = msg_send![desc, objectForKey: key];
            if num.is_null() {
                continue;
            }
            let screen_no: u32 = msg_send![num, unsignedIntValue];
            if screen_no == cg_id {
                let cur: f64 = msg_send![s, maximumExtendedDynamicRangeColorComponentValue];
                let pot: f64 =
                    msg_send![s, maximumPotentialExtendedDynamicRangeColorComponentValue];
                return (cur as f32, pot as f32);
            }
        }
        (1.0, 1.0)
    }

    pub fn teardown(app: &AppHandle) {
        let Some(mut o) = STATE.lock().take() else {
            return;
        };
        o.running.store(false, Ordering::Relaxed);
        if let Some(t) = o.thread.take() {
            let _ = t.join();
        }
        let win = o.window.0 as usize;
        let _ = app.run_on_main_thread(move || unsafe {
            let w = win as *mut AnyObject;
            if !w.is_null() {
                let _: () = msg_send![w, orderOut: std::ptr::null::<AnyObject>()];
                let _: () = msg_send![w, close];
            }
        });
        tracing::info!("edr: overlay torn down");
    }

    /// Build the borderless, transparent, click-through, always-on-top window
    /// covering the screen with CoreGraphics id `cg_id`, hosting a CAMetalLayer.
    /// Returns `(NSWindow*, CAMetalLayer*, MTLCommandQueue*)` — the window is kept
    /// for teardown; the layer + queue go to the render thread (both safe
    /// off-main, unlike the window). **Main-thread only.**
    unsafe fn build_window(cg_id: u32) -> Option<(*mut AnyObject, *mut AnyObject, *mut AnyObject)> {
        let frame = screen_frame(cg_id)?;

        let ns_window = AnyClass::get(c"NSWindow")?;
        let alloc: *mut AnyObject = msg_send![ns_window, alloc];
        // styleMask 0 = borderless; backing 2 = buffered; defer NO.
        let win: *mut AnyObject = msg_send![
            alloc,
            initWithContentRect: frame,
            styleMask: 0u64,
            backing: 2u64,
            defer: false,
        ];
        if win.is_null() {
            return None;
        }
        let _: () = msg_send![win, setOpaque: false];
        let _: () = msg_send![win, setHasShadow: false];
        let _: () = msg_send![win, setIgnoresMouseEvents: true];
        // Above normal windows; kCGScreenSaverWindowLevel-ish (1000).
        let _: () = msg_send![win, setLevel: 1000i64];
        // Join all Spaces + don't show in mission control: canJoinAllSpaces(1)
        // | stationary(16) | ignoresCycle(64) = 81.
        let _: () = msg_send![win, setCollectionBehavior: 81u64];
        // Clear background so only the EDR layer contributes.
        let clear = ns_color_clear();
        if !clear.is_null() {
            let _: () = msg_send![win, setBackgroundColor: clear];
        }

        // CAMetalLayer host view.
        let ns_view = AnyClass::get(c"NSView")?;
        let v_alloc: *mut AnyObject = msg_send![ns_view, alloc];
        let view: *mut AnyObject = msg_send![v_alloc, initWithFrame: frame];
        let _: () = msg_send![view, setWantsLayer: true];

        let layer = build_metal_layer(frame.size)?;
        let _: () = msg_send![view, setLayer: layer];
        let _: () = msg_send![win, setContentView: view];

        let _: () = msg_send![win, orderFrontRegardless];

        // Resolve the command queue here on the main thread; the render thread
        // gets only the layer + queue (never the NSWindow/NSView).
        let device: *mut AnyObject = msg_send![layer, device];
        if device.is_null() {
            return None;
        }
        let queue: *mut AnyObject = msg_send![device, newCommandQueue];
        if queue.is_null() {
            return None;
        }
        Some((win, layer, queue))
    }

    unsafe fn build_metal_layer(size: NSSize) -> Option<*mut AnyObject> {
        let device = MTLCreateSystemDefaultDevice();
        if device.is_null() {
            return None;
        }
        let ca = AnyClass::get(c"CAMetalLayer")?;
        let layer: *mut AnyObject = msg_send![ca, layer];
        if layer.is_null() {
            return None;
        }
        let _: () = msg_send![layer, setDevice: device];
        // RGBA16Float = 115.
        let _: () = msg_send![layer, setPixelFormat: 115u64];
        let _: () = msg_send![layer, setWantsExtendedDynamicRangeContent: true];
        let _: () = msg_send![layer, setOpaque: false];
        let _: () = msg_send![layer, setFramebufferOnly: false];
        let r = NSRect { origin: NSPoint::default(), size };
        let _: () = msg_send![layer, setFrame: r];
        let _: () = msg_send![layer, setDrawableSize: size];
        // Extended-linear Display-P3 so component values > 1.0 mean EDR.
        let cs = CGColorSpaceCreateWithName(kCGColorSpaceExtendedLinearDisplayP3);
        if !cs.is_null() {
            let _: () = msg_send![layer, setColorspace: cs];
        }

        // Multiply the desktop behind the window instead of alpha-blending a
        // white veil → blacks stay black (no wash). Dimming apps use this same
        // compositingFilter trick (with factor < 1). If CoreImage / the filter
        // isn't available we fall back to the veil (MULTIPLY_OK stays false), so
        // the overlay is never an opaque grey rectangle.
        MULTIPLY_OK.store(false, Ordering::Relaxed);
        if let Some(ci) = AnyClass::get(c"CIFilter") {
            let name = super::nsstring("CIMultiplyCompositing");
            if !name.is_null() {
                let filter: *mut AnyObject = msg_send![ci, filterWithName: name];
                if !filter.is_null() {
                    let _: () = msg_send![layer, setCompositingFilter: filter];
                    MULTIPLY_OK.store(true, Ordering::Relaxed);
                }
            }
        }
        if !MULTIPLY_OK.load(Ordering::Relaxed) {
            tracing::warn!("edr: CIMultiplyCompositing unavailable — falling back to additive veil");
        }

        // Keep the device referenced via the layer; queue created in the loop.
        Some(layer)
    }

    /// Draw one EDR frame. **MUST run on the main thread** — Core-Animation layer
    /// access off-main raises an Obj-C exception that aborts the process.
    fn draw_edr_frame(layer_ptr: usize, queue_ptr: usize, above: f32) {
        let layer = layer_ptr as *mut AnyObject;
        let queue = queue_ptr as *mut AnyObject;
        if layer.is_null() || queue.is_null() {
            return;
        }
        // Catch any Obj-C exception from the Metal/CA calls so it can't unwind
        // through the run loop and abort the app; log the reason once.
        let result = unsafe {
            objc2::exception::catch(std::panic::AssertUnwindSafe(|| {
                autoreleasepool(|_| {
                    let (val, alpha) = if MULTIPLY_OK.load(Ordering::Relaxed) {
                        // Multiply: desktop × factor (≥ 1). Opaque; the filter
                        // does the blend. Blacks stay black, brights lift → EDR.
                        ((1.0 + above * MULT_GAIN) as f64, 1.0)
                    } else {
                        // Additive veil fallback (brightens but washes).
                        let lum = (1.0 + above * VEIL_GAIN) as f64;
                        let alpha = (above * VEIL_ALPHA).min(MAX_ALPHA) as f64;
                        (lum, alpha)
                    };
                    draw_frame(layer, queue, val, alpha);
                });
            }))
        };
        if let Err(exc) = result {
            if !RENDER_FAILED.swap(true, Ordering::Relaxed) {
                let (name, reason) = unsafe {
                    match exc.as_deref() {
                        Some(e) => {
                            let p = e as *const _ as *mut AnyObject;
                            let n: *mut AnyObject = msg_send![p, name];
                            let r: *mut AnyObject = msg_send![p, reason];
                            (ns_to_string(n), ns_to_string(r))
                        }
                        None => ("<none>".to_string(), "<none>".to_string()),
                    }
                };
                tracing::error!(
                    "edr: Metal draw raised Obj-C exception '{name}': {reason} — EDR overlay disabled"
                );
            }
        }
    }

    unsafe fn draw_frame(layer: *mut AnyObject, queue: *mut AnyObject, lum: f64, alpha: f64) {
        let drawable: *mut AnyObject = msg_send![layer, nextDrawable];
        if drawable.is_null() {
            return;
        }
        let texture: *mut AnyObject = msg_send![drawable, texture];
        if texture.is_null() {
            return;
        }
        let rpd_cls = match AnyClass::get(c"MTLRenderPassDescriptor") {
            Some(c) => c,
            None => return,
        };
        let rpd: *mut AnyObject = msg_send![rpd_cls, renderPassDescriptor];
        let attachments: *mut AnyObject = msg_send![rpd, colorAttachments];
        let att0: *mut AnyObject = msg_send![attachments, objectAtIndexedSubscript: 0usize];
        let _: () = msg_send![att0, setTexture: texture];
        let _: () = msg_send![att0, setLoadAction: 2u64]; // Clear
        let _: () = msg_send![att0, setStoreAction: 1u64]; // Store
        let _: () = msg_send![att0, setClearColor: MTLClearColor { red: lum, green: lum, blue: lum, alpha }];

        let cmd: *mut AnyObject = msg_send![queue, commandBuffer];
        let enc: *mut AnyObject = msg_send![cmd, renderCommandEncoderWithDescriptor: rpd];
        let _: () = msg_send![enc, endEncoding];
        let _: () = msg_send![cmd, presentDrawable: drawable];
        let _: () = msg_send![cmd, commit];
    }

    unsafe fn ns_color_clear() -> *mut AnyObject {
        match AnyClass::get(c"NSColor") {
            Some(c) => msg_send![c, clearColor],
            None => std::ptr::null_mut(),
        }
    }

    /// The Cocoa-points frame of the NSScreen whose `NSScreenNumber` is `cg_id`.
    unsafe fn screen_frame(cg_id: u32) -> Option<NSRect> {
        let ns_screen = AnyClass::get(c"NSScreen")?;
        let screens: *mut AnyObject = msg_send![ns_screen, screens];
        if screens.is_null() {
            return None;
        }
        let count: usize = msg_send![screens, count];
        let key = super::nsstring("NSScreenNumber");
        for i in 0..count {
            let s: *mut AnyObject = msg_send![screens, objectAtIndex: i];
            if s.is_null() {
                continue;
            }
            let desc: *mut AnyObject = msg_send![s, deviceDescription];
            if desc.is_null() {
                continue;
            }
            let num: *mut AnyObject = msg_send![desc, objectForKey: key];
            if num.is_null() {
                continue;
            }
            let screen_no: u32 = msg_send![num, unsignedIntValue];
            if screen_no == cg_id {
                let f: NSRect = msg_send![s, frame];
                return Some(f);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edr_max_percent_gates_on_headroom() {
        assert_eq!(edr_max_percent(1.0), 100, "no headroom → SDR only");
        assert_eq!(edr_max_percent(1.04), 100, "below margin → SDR only");
        assert_eq!(edr_max_percent(1.6), 160, "1.6× → 160 %");
        assert_eq!(edr_max_percent(16.0), EDR_MAX_CAP, "huge headroom is capped");
        // Just-capable displays still get a usable range above 100.
        assert!(edr_max_percent(1.06) >= 110);
    }
}
