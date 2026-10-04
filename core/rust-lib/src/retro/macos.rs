//! macOS retro overlay: ScreenCaptureKit → Metal shader → click-through
//! window per monitor, plus the panel's live preview.
//!
//! Pipeline per monitor:
//! 1. `SCStream` on that display. The content filter excludes ONLY our overlay
//!    windows (not the whole app — the popup must stay visible, it simply shows
//!    up retro-rendered underneath the overlay). The capture never contains
//!    the cursor: the real one is drawn by the system above every window, a
//!    captured copy would trail behind it as a second cursor.
//! 2. Each frame arrives as an IOSurface-backed `CVPixelBuffer` on one serial
//!    render queue and becomes an `MTLTexture` through `CVMetalTextureCache`
//!    (zero copy).
//! 3. `shader.metal` renders into the overlay's `CAMetalLayer`.
//!
//! Rendering happens on the render queue (Metal and `CAMetalLayer` drawables
//! are thread-safe); every draw runs inside `objc2::exception::catch` so an
//! Objective-C exception stops the overlay instead of aborting the process
//! (the EDR lesson). Windows are built and closed on the main thread only.
//!
//! Idle cost: ScreenCaptureKit only delivers a frame when the screen changed;
//! the ticker re-renders the last frame only when the mouse moved (lens /
//! sprite cursor) or a setting / the focus changed.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{
    CVImageBuffer, CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture, CVPixelBufferGetBaseAddress,
    CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSArray, NSError, NSString};
use objc2_metal::{
    MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue, MTLCreateSystemDefaultDevice, MTLDevice, MTLDrawable,
    MTLLibrary, MTLLoadAction, MTLPixelFormat, MTLPrimitiveType, MTLRenderCommandEncoder, MTLRenderPassDescriptor,
    MTLRenderPipelineDescriptor, MTLRenderPipelineState, MTLStoreAction,
};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamDelegate,
    SCStreamOutput, SCStreamOutputType, SCWindow,
};
use tauri::{AppHandle, Emitter, Manager};

use super::config::{ModeSettings, RetroConfig, Target as MonTarget};
use super::frame::{capture_size, frame_params, pack, render_params, scaled, Display, Live, WinInfo};
use super::{render, Rect};

pub const ERR_NO_PERMISSION: &str = "retro.no_permission";
pub const ERR_UNSUPPORTED: &str = "retro.unsupported";
const SHADER: &str = include_str!("shader.metal");
/// 'BGRA'
const PIXEL_BGRA: u32 = 0x4247_5241;

// ── Small helpers ───────────────────────────────────────────────────────────

/// Wrapper for Objective-C objects we hand between threads. Metal devices,
/// queues, pipelines, layers and SCK objects are documented thread-safe for
/// the calls we make; the NSWindow pointer is only ever touched on main.
struct Sendable<T>(T);
unsafe impl<T> Send for Sendable<T> {}
unsafe impl<T> Sync for Sendable<T> {}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSPoint {
    x: f64,
    y: f64,
}
unsafe impl objc2::Encode for NSPoint {
    const ENCODING: objc2::Encoding = objc2::Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}

/// ScreenCaptureKit exists (macOS 12.3+).
pub fn supported() -> bool {
    AnyClass::get(c"SCStream").is_some()
}

pub fn screen_permission() -> bool {
    crate::screen_recording::screen_recording_granted()
}

pub fn accessibility() -> bool {
    super::macos_focus::accessibility_granted()
}

pub fn note() -> Option<String> {
    (!supported()).then(|| "Braucht macOS 12.3 oder neuer (ScreenCaptureKit).".to_string())
}

/// Run `f` on the main thread and wait for its result.
fn on_main<T: Send + 'static>(app: &AppHandle, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .ok()?;
    rx.recv_timeout(Duration::from_secs(5)).ok()
}

// ── Displays (main thread) ──────────────────────────────────────────────────

/// All screens in global top-left points with their backing scale.
/// **Main thread.**
unsafe fn displays() -> Vec<Display> {
    let Some(cls) = AnyClass::get(c"NSScreen") else { return Vec::new() };
    let screens: *mut AnyObject = msg_send![cls, screens];
    if screens.is_null() {
        return Vec::new();
    }
    let n: usize = msg_send![screens, count];
    let key = NSString::from_str("NSScreenNumber");
    let mut raw = Vec::new();
    for i in 0..n {
        let s: *mut AnyObject = msg_send![screens, objectAtIndex: i];
        let f: CGRect = msg_send![s, frame];
        let scale: f64 = msg_send![s, backingScaleFactor];
        let desc: *mut AnyObject = msg_send![s, deviceDescription];
        let num: *mut AnyObject = msg_send![desc, objectForKey: &*key];
        let id: u32 = if num.is_null() { 0 } else { msg_send![num, unsignedIntValue] };
        raw.push((id, f, scale));
    }
    // Cocoa y grows upward from the primary screen's bottom edge.
    let primary_h = raw.first().map(|r| r.1.size.height).unwrap_or(0.0);
    raw.into_iter()
        .map(|(id, f, scale)| Display {
            id,
            x: f.origin.x,
            y: primary_h - (f.origin.y + f.size.height),
            w: f.size.width,
            h: f.size.height,
            scale,
        })
        .collect()
}

/// Mouse in global top-left points. Callable from any thread.
fn mouse_global(primary_h: f64) -> (f64, f64) {
    let Some(cls) = AnyClass::get(c"NSEvent") else { return (0.0, 0.0) };
    let p: NSPoint = unsafe { msg_send![cls, mouseLocation] };
    (p.x, primary_h - p.y)
}

// ── GPU (shared) ────────────────────────────────────────────────────────────

struct Gpu {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    pipeline: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    cache: CFRetained<CVMetalTextureCache>,
}
unsafe impl Send for Gpu {}
unsafe impl Sync for Gpu {}

fn gpu() -> Result<&'static Gpu, String> {
    static G: OnceLock<Result<Gpu, String>> = OnceLock::new();
    G.get_or_init(|| unsafe {
        let device = MTLCreateSystemDefaultDevice().ok_or("Kein Metal-Gerät")?;
        let queue = device.newCommandQueue().ok_or("Keine Metal-Queue")?;
        let lib = device
            .newLibraryWithSource_options_error(&NSString::from_str(SHADER), None)
            .map_err(|e| format!("Shader: {}", e.localizedDescription()))?;
        let vs = lib.newFunctionWithName(&NSString::from_str("retro_vs")).ok_or("retro_vs fehlt")?;
        let fs = lib.newFunctionWithName(&NSString::from_str("retro_fs")).ok_or("retro_fs fehlt")?;
        let desc = MTLRenderPipelineDescriptor::new();
        desc.setVertexFunction(Some(&vs));
        desc.setFragmentFunction(Some(&fs));
        desc.colorAttachments().objectAtIndexedSubscript(0).setPixelFormat(MTLPixelFormat::BGRA8Unorm);
        let pipeline = device
            .newRenderPipelineStateWithDescriptor_error(&desc)
            .map_err(|e| format!("Pipeline: {}", e.localizedDescription()))?;
        let mut out: *mut CVMetalTextureCache = std::ptr::null_mut();
        let rc = CVMetalTextureCache::create(None, None, &device, None, NonNull::from(&mut out));
        if rc != 0 || out.is_null() {
            return Err(format!("CVMetalTextureCache ({rc})"));
        }
        let cache = CFRetained::from_raw(NonNull::new_unchecked(out));
        Ok(Gpu { device, queue, pipeline, cache })
    })
    .as_ref()
    .map_err(|e| e.clone())
}

fn render_queue() -> &'static DispatchQueue {
    static Q: OnceLock<Sendable<DispatchRetained<DispatchQueue>>> = OnceLock::new();
    &Q.get_or_init(|| Sendable(DispatchQueue::new("io.celox.inspector-rust.retro", None))).0
}

// ── Engine state ────────────────────────────────────────────────────────────

/// One monitor's overlay.
struct Target {
    display: Display,
    layer: Sendable<Retained<CAMetalLayer>>,
    window: Sendable<*mut AnyObject>,
    last: Mutex<Option<Sendable<CFRetained<CVMetalTexture>>>>,
    shown: AtomicBool,
}

struct Shared {
    settings: Mutex<ModeSettings>,
    fps: Mutex<u32>,
    live: Mutex<Live>,
    targets: Mutex<Vec<Arc<Target>>>,
    running: AtomicBool,
    failed: AtomicBool,
    primary_h: Mutex<f64>,
    frames: AtomicU64,
    renders: AtomicU64,
    app: Mutex<Option<AppHandle>>,
}

fn shared() -> &'static Shared {
    static S: OnceLock<Shared> = OnceLock::new();
    S.get_or_init(|| Shared {
        settings: Mutex::new(ModeSettings::default()),
        fps: Mutex::new(60),
        live: Mutex::new(Live::default()),
        targets: Mutex::new(Vec::new()),
        running: AtomicBool::new(false),
        failed: AtomicBool::new(false),
        primary_h: Mutex::new(0.0),
        frames: AtomicU64::new(0),
        renders: AtomicU64::new(0),
        app: Mutex::new(None),
    })
}

struct Streams {
    streams: Vec<Sendable<Retained<SCStream>>>,
    _outputs: Vec<Sendable<Retained<RetroOutput>>>,
}
static STREAMS: Mutex<Option<Streams>> = Mutex::new(None);
static PREVIEW: Mutex<Option<Streams>> = Mutex::new(None);
static OP: Mutex<()> = Mutex::new(());

// ── SCStream output / delegate ──────────────────────────────────────────────

#[derive(Clone)]
enum OutKind {
    Overlay(Arc<Target>),
    Preview(Display),
}

pub struct OutIvars {
    kind: OutKind,
    last_preview: Mutex<Option<Instant>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "IRRetroOutput"]
    #[ivars = OutIvars]
    struct RetroOutput;

    unsafe impl NSObjectProtocol for RetroOutput {}

    unsafe impl SCStreamOutput for RetroOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn did_output(&self, _stream: &SCStream, sample: &CMSampleBuffer, ty: SCStreamOutputType) {
            if ty != SCStreamOutputType::Screen {
                return;
            }
            // Idle / blank status frames carry no image — nothing to redraw.
            let Some(buf) = (unsafe { sample.image_buffer() }) else { return };
            match &self.ivars().kind {
                OutKind::Overlay(t) => on_overlay_frame(t, &buf),
                OutKind::Preview(d) => self.on_preview_frame(*d, &buf),
            }
        }
    }

    unsafe impl SCStreamDelegate for RetroOutput {
        #[unsafe(method(stream:didStopWithError:))]
        fn did_stop(&self, _stream: &SCStream, error: &NSError) {
            let msg = error.localizedDescription().to_string();
            tracing::warn!("retro: capture stopped: {msg}");
            if let OutKind::Overlay(_) = self.ivars().kind {
                fail(&format!("Aufnahme beendet: {msg}"));
            }
        }
    }
);

impl RetroOutput {
    fn create(kind: OutKind) -> Retained<Self> {
        let this = Self::alloc().set_ivars(OutIvars { kind, last_preview: Mutex::new(None) });
        unsafe { msg_send![super(this), init] }
    }

    fn on_preview_frame(&self, d: Display, buf: &CVImageBuffer) {
        {
            let mut last = self.ivars().last_preview.lock().unwrap_or_else(|e| e.into_inner());
            if last.is_some_and(|t| t.elapsed() < Duration::from_millis(95)) {
                return; // ≤ 10 fps
            }
            *last = Some(Instant::now());
        }
        let Some((w, h, rgba)) = read_pixels(buf) else { return };
        let s = shared();
        let settings = s.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut live = s.live.lock().unwrap_or_else(|e| e.into_inner()).clone();
        live.mouse = Some(mouse_global(*s.primary_h.lock().unwrap_or_else(|e| e.into_inner())));
        let full = frame_params(&d, &settings, &live);
        let p = scaled(&full, w as f32 / full.size.0.max(1) as f32);
        let out = render(&rgba, w, h, &render_params(&p));
        use base64::Engine as _;
        let payload = serde_json::json!({
            "w": w, "h": h,
            "data": base64::engine::general_purpose::STANDARD.encode(out),
        });
        if let Some(app) = s.app.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = app.emit("retro-preview-frame", payload);
        }
    }
}

/// CPU copy of a (small, preview) pixel buffer as RGBA.
fn read_pixels(buf: &CVImageBuffer) -> Option<(usize, usize, Vec<u8>)> {
    unsafe {
        if CVPixelBufferLockBaseAddress(buf, CVPixelBufferLockFlags::ReadOnly) != 0 {
            return None;
        }
        let w = CVPixelBufferGetWidth(buf);
        let h = CVPixelBufferGetHeight(buf);
        let row = CVPixelBufferGetBytesPerRow(buf);
        let base = CVPixelBufferGetBaseAddress(buf) as *const u8;
        let mut out = Vec::with_capacity(w * h * 4);
        if !base.is_null() {
            for y in 0..h {
                let line = std::slice::from_raw_parts(base.add(y * row), w * 4);
                for px in line.as_chunks::<4>().0 {
                    out.extend_from_slice(&[px[2], px[1], px[0], 255]);
                }
            }
        }
        CVPixelBufferUnlockBaseAddress(buf, CVPixelBufferLockFlags::ReadOnly);
        (out.len() == w * h * 4).then_some((w, h, out))
    }
}

// ── Rendering (render queue) ────────────────────────────────────────────────

fn on_overlay_frame(t: &Arc<Target>, buf: &CVImageBuffer) {
    let Ok(g) = gpu() else { return };
    let w = CVPixelBufferGetWidth(buf);
    let h = CVPixelBufferGetHeight(buf);
    let mut out: *mut CVMetalTexture = std::ptr::null_mut();
    let rc = unsafe {
        CVMetalTextureCache::create_texture_from_image(
            None,
            &g.cache,
            buf,
            None,
            MTLPixelFormat::BGRA8Unorm,
            w,
            h,
            0,
            NonNull::from(&mut out),
        )
    };
    if rc != 0 || out.is_null() {
        return;
    }
    let tex = unsafe { CFRetained::from_raw(NonNull::new_unchecked(out)) };
    *t.last.lock().unwrap_or_else(|e| e.into_inner()) = Some(Sendable(tex));
    shared().frames.fetch_add(1, Ordering::Relaxed);
    draw(t);
}

/// Draw the last captured frame of `t` with the current settings.
fn draw(t: &Arc<Target>) {
    let s = shared();
    if !s.running.load(Ordering::SeqCst) || s.failed.load(Ordering::SeqCst) {
        return;
    }
    let Ok(g) = gpu() else { return };
    let last = t.last.lock().unwrap_or_else(|e| e.into_inner());
    let Some(tex) = last.as_ref() else { return };
    let settings = s.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut live = s.live.lock().unwrap_or_else(|e| e.into_inner()).clone();
    live.mouse = Some(mouse_global(*s.primary_h.lock().unwrap_or_else(|e| e.into_inner())));
    let frame = pack(&frame_params(&t.display, &settings, &live));
    let ok = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
        let Some(mtl_tex) = CVMetalTextureGetTexture(&tex.0) else { return false };
        let Some(drawable) = t.layer.0.nextDrawable() else { return false };
        let pass = MTLRenderPassDescriptor::new();
        let att = pass.colorAttachments().objectAtIndexedSubscript(0);
        att.setTexture(Some(&drawable.texture()));
        att.setLoadAction(MTLLoadAction::DontCare);
        att.setStoreAction(MTLStoreAction::Store);
        let Some(cb) = g.queue.commandBuffer() else { return false };
        let Some(enc) = cb.renderCommandEncoderWithDescriptor(&pass) else { return false };
        encode_frame(&enc, g, &mtl_tex, &frame);
        enc.endEncoding();
        let d: &ProtocolObject<dyn MTLDrawable> = ProtocolObject::from_ref(&*drawable);
        cb.presentDrawable(d);
        cb.commit();
        true
    }));
    match ok {
        Ok(true) => {
            s.renders.fetch_add(1, Ordering::Relaxed);
            if !t.shown.swap(true, Ordering::SeqCst) {
                // First frame is on the layer → fade the window in (main).
                let win = Sendable(t.window.0);
                if let Some(app) = s.app.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                    let _ = app.run_on_main_thread(move || unsafe {
                        let w = win;
                        if !w.0.is_null() {
                            let _: () = msg_send![w.0, setAlphaValue: 1.0f64];
                        }
                    });
                }
            }
        }
        Ok(false) => {}
        Err(e) => {
            tracing::error!("retro: render raised an Objective-C exception: {e:?}");
            fail("Darstellungsfehler");
        }
    }
}

/// Bind pipeline, source texture and every buffer, then draw the full-screen
/// triangle. Shared by the overlay and the offscreen test.
unsafe fn encode_frame(
    enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
    g: &Gpu,
    src: &ProtocolObject<dyn objc2_metal::MTLTexture>,
    frame: &super::frame::GpuFrame,
) {
    enc.setRenderPipelineState(&g.pipeline);
    enc.setFragmentTexture_atIndex(Some(src), 0);
    // setBytes is fine for ≤ 4 KB per buffer: palette ≤ 1 KB, windows ≤ 1 KB,
    // titles ≤ 1.5 KB, font 665 B.
    let bytes = |ptr: *const c_void, len: usize, idx: usize| {
        enc.setFragmentBytes_length_atIndex(NonNull::new_unchecked(ptr as *mut c_void), len.max(4), idx);
    };
    bytes((&frame.uniforms as *const _) as *const c_void, std::mem::size_of_val(&frame.uniforms), 0);
    bytes(frame.palette.as_ptr() as *const c_void, frame.palette.len() * 16, 1);
    bytes(frame.bayer.as_ptr() as *const c_void, frame.bayer.len() * 4, 2);
    bytes(frame.windows.as_ptr() as *const c_void, frame.windows.len() * 32, 3);
    bytes(frame.titles.as_ptr() as *const c_void, frame.titles.len(), 4);
    let font = super::frame::font();
    bytes(font.as_ptr() as *const c_void, font.len(), 5);
    enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
}

/// Re-render every overlay with its last frame (settings / mouse / focus).
fn redraw_all() {
    let targets = shared().targets.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if targets.is_empty() {
        return;
    }
    render_queue().exec_async(move || {
        for t in &targets {
            draw(t);
        }
    });
}

// ── Windows (main thread) ───────────────────────────────────────────────────

/// Build one overlay window on `d`. **Main thread.** Returns the window and
/// its Metal layer.
unsafe fn build_window(d: &Display, primary_h: f64) -> Option<(*mut AnyObject, Retained<CAMetalLayer>)> {
    let g = gpu().ok()?;
    let frame = CGRect {
        origin: CGPoint { x: d.x, y: primary_h - (d.y + d.h) },
        size: CGSize { width: d.w, height: d.h },
    };
    let cls = AnyClass::get(c"NSWindow")?;
    let alloc: *mut AnyObject = msg_send![cls, alloc];
    let win: *mut AnyObject = msg_send![alloc, initWithContentRect: frame, styleMask: 0u64, backing: 2u64, defer: false];
    if win.is_null() {
        return None;
    }
    let _: () = msg_send![win, setReleasedWhenClosed: false];
    let _: () = msg_send![win, setOpaque: false];
    let _: () = msg_send![win, setHasShadow: false];
    let _: () = msg_send![win, setIgnoresMouseEvents: true];
    // Above the menu bar (24) and the Dock (20).
    let _: () = msg_send![win, setLevel: 1000i64];
    // canJoinAllSpaces(1) | stationary(16) | ignoresCycle(64) | fullScreenAuxiliary(256)
    let _: () = msg_send![win, setCollectionBehavior: 337u64];
    // NSWindowSharingNone: also stays out of our own screenshots/recordings.
    // Diagnostics only: IR_RETRO_SHARE=1 keeps it visible to screenshots so
    // the overlay can be checked with `screencapture`.
    let share = std::env::var("IR_RETRO_SHARE").is_ok_and(|v| v == "1");
    let _: () = msg_send![win, setSharingType: if share { 1u64 } else { 0u64 }];
    // app.hide() (popup dismiss) hides every window of the process — not this one.
    let _: () = msg_send![win, setCanHide: false];
    if let Some(c) = AnyClass::get(c"NSColor") {
        let clear: *mut AnyObject = msg_send![c, clearColor];
        let _: () = msg_send![win, setBackgroundColor: clear];
    }
    let view_cls = AnyClass::get(c"NSView")?;
    let va: *mut AnyObject = msg_send![view_cls, alloc];
    let view: *mut AnyObject = msg_send![va, initWithFrame: frame];
    let _: () = msg_send![view, setWantsLayer: true];
    let layer = CAMetalLayer::new();
    layer.setDevice(Some(&g.device));
    layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
    layer.setFramebufferOnly(true);
    layer.setOpaque(false);
    layer.setContentsScale(d.scale);
    let (pw, ph) = d.px_size();
    layer.setDrawableSize(CGSize { width: pw as f64, height: ph as f64 });
    let _: () = msg_send![view, setLayer: &*layer];
    let _: () = msg_send![win, setContentView: view];
    let _: () = msg_send![view, release];
    let _: () = msg_send![win, setAlphaValue: 0.0f64];
    let _: () = msg_send![win, orderFrontRegardless];
    Some((win, layer))
}

unsafe fn close_window(win: *mut AnyObject) {
    if win.is_null() {
        return;
    }
    let _: () = msg_send![win, orderOut: std::ptr::null::<AnyObject>()];
    let _: () = msg_send![win, close];
    let _: () = msg_send![win, release];
}

unsafe fn window_number(win: *mut AnyObject) -> u32 {
    let n: isize = msg_send![win, windowNumber];
    n as u32
}

// ── ScreenCaptureKit plumbing ───────────────────────────────────────────────

fn shareable_content() -> Result<Sendable<Retained<SCShareableContent>>, String> {
    let (tx, rx) = mpsc::channel::<Result<Sendable<Retained<SCShareableContent>>, String>>();
    let block = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        let r = if let Some(c) = unsafe { Retained::retain(content) } {
            Ok(Sendable(c))
        } else if let Some(e) = unsafe { err.as_ref() } {
            Err(e.localizedDescription().to_string())
        } else {
            Err("keine Inhalte".into())
        };
        let _ = tx.send(r);
    });
    unsafe { SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(false, true, &block) };
    rx.recv_timeout(Duration::from_secs(5)).map_err(|_| "ScreenCaptureKit antwortet nicht".to_string())?
}

fn stream_config(w: u32, h: u32, fps: u32) -> Retained<SCStreamConfiguration> {
    unsafe {
        let c = SCStreamConfiguration::new();
        c.setWidth(w as usize);
        c.setHeight(h as usize);
        c.setPixelFormat(PIXEL_BGRA);
        c.setShowsCursor(false);
        c.setScalesToFit(true);
        c.setQueueDepth(3);
        c.setMinimumFrameInterval(CMTime::new(1, fps.max(1) as i32));
        c
    }
}

fn start_stream(
    display: &SCDisplay,
    exclude: &NSArray<SCWindow>,
    cfg: &SCStreamConfiguration,
    kind: OutKind,
) -> Result<(Retained<SCStream>, Retained<RetroOutput>), String> {
    unsafe {
        let filter = SCContentFilter::initWithDisplay_excludingWindows(SCContentFilter::alloc(), display, exclude);
        let out = RetroOutput::create(kind);
        let delegate: &ProtocolObject<dyn SCStreamDelegate> = ProtocolObject::from_ref(&*out);
        let stream = SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, cfg, Some(delegate));
        let output: &ProtocolObject<dyn SCStreamOutput> = ProtocolObject::from_ref(&*out);
        stream
            .addStreamOutput_type_sampleHandlerQueue_error(output, SCStreamOutputType::Screen, Some(render_queue()))
            .map_err(|e| e.localizedDescription().to_string())?;
        let (tx, rx) = mpsc::channel::<Option<String>>();
        let done = RcBlock::new(move |err: *mut NSError| {
            let _ = tx.send(err.as_ref().map(|e| e.localizedDescription().to_string()));
        });
        stream.startCaptureWithCompletionHandler(Some(&done));
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(None) => Ok((stream, out)),
            Ok(Some(e)) => Err(e),
            Err(_) => Err("Start der Aufnahme hängt".into()),
        }
    }
}

fn stop_streams(slot: &Mutex<Option<Streams>>) {
    let taken = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(s) = taken {
        for st in &s.streams {
            let (tx, rx) = mpsc::channel::<()>();
            let done = RcBlock::new(move |_e: *mut NSError| {
                let _ = tx.send(());
            });
            unsafe { st.0.stopCaptureWithCompletionHandler(Some(&done)) };
            let _ = rx.recv_timeout(Duration::from_secs(2));
        }
    }
}

// ── Public API ──────────────────────────────────────────────────────────────

pub fn set_app(app: &AppHandle) {
    *shared().app.lock().unwrap_or_else(|e| e.into_inner()) = Some(app.clone());
}

pub fn is_running() -> bool {
    shared().running.load(Ordering::SeqCst)
}

/// Start the overlay with `cfg`. Blocking (builds windows on main, waits for
/// ScreenCaptureKit) — call from a worker thread.
pub fn start(app: &AppHandle, cfg: &RetroConfig) -> Result<(), String> {
    let _op = OP.lock().unwrap_or_else(|e| e.into_inner());
    if is_running() {
        return Ok(());
    }
    if !supported() {
        return Err(ERR_UNSUPPORTED.into());
    }
    if !crate::screen_recording::screen_recording_granted() {
        return Err(ERR_NO_PERMISSION.into());
    }
    gpu()?;
    set_app(app);
    let s = shared();
    *s.settings.lock().unwrap_or_else(|e| e.into_inner()) = cfg.active().clone();
    *s.fps.lock().unwrap_or_else(|e| e.into_inner()) = cfg.fps;
    s.failed.store(false, Ordering::SeqCst);

    // 1. windows on main
    let target = cfg.target;
    let built = on_main(app, move || unsafe {
        let all = displays();
        let primary_h = all.first().map(|d| d.h).unwrap_or(0.0);
        let mouse = mouse_global(primary_h);
        let chosen: Vec<Display> = match target {
            MonTarget::All => all,
            MonTarget::Current => {
                let d = all.iter().find(|d| d.contains(mouse.0, mouse.1)).or(all.first()).copied();
                d.into_iter().collect()
            }
        };
        let mut out = Vec::new();
        for d in chosen {
            if let Some((win, layer)) = build_window(&d, primary_h) {
                out.push((d, Sendable(win), Sendable(layer), window_number(win)));
            }
        }
        (primary_h, out)
    })
    .ok_or("Fenster konnten nicht gebaut werden")?;
    *s.primary_h.lock().unwrap_or_else(|e| e.into_inner()) = built.0;
    if built.1.is_empty() {
        return Err("Kein Bildschirm gefunden".into());
    }
    let targets: Vec<Arc<Target>> = built
        .1
        .iter()
        .map(|(d, w, l, _)| {
            Arc::new(Target {
                display: *d,
                layer: Sendable(l.0.clone()),
                window: Sendable(w.0),
                last: Mutex::new(None),
                shown: AtomicBool::new(false),
            })
        })
        .collect();
    let our_ids: Vec<u32> = built.1.iter().map(|b| b.3).collect();

    // 2. capture (excluding our windows)
    let result = (|| -> Result<Streams, String> {
        let content = shareable_content()?;
        let (displays, windows) = unsafe { (content.0.displays(), content.0.windows()) };
        let excluded: Vec<Retained<SCWindow>> =
            windows.iter().filter(|w| our_ids.contains(&unsafe { w.windowID() })).collect();
        let exclude = NSArray::from_retained_slice(&excluded);
        tracing::info!("retro: excluding {} of {} overlay windows from capture", excluded.len(), our_ids.len());
        let mut streams = Vec::new();
        let mut outputs = Vec::new();
        let settings = cfg.active().clone();
        for t in &targets {
            let Some(scd) = displays.iter().find(|d| unsafe { d.displayID() } == t.display.id) else { continue };
            let (cw, ch) = capture_size(&t.display, &settings);
            tracing::info!(
                "retro: display {} {}x{} pt @{}x → capture {cw}x{ch} @ {} fps",
                t.display.id, t.display.w, t.display.h, t.display.scale, cfg.fps
            );
            let conf = stream_config(cw, ch, cfg.fps);
            let (st, out) = start_stream(&scd, &exclude, &conf, OutKind::Overlay(t.clone()))?;
            streams.push(Sendable(st));
            outputs.push(Sendable(out));
        }
        if streams.is_empty() {
            return Err("Kein Bildschirm für die Aufnahme".into());
        }
        Ok(Streams { streams, _outputs: outputs })
    })();
    match result {
        Ok(st) => {
            *STREAMS.lock().unwrap_or_else(|e| e.into_inner()) = Some(st);
            *s.targets.lock().unwrap_or_else(|e| e.into_inner()) = targets;
            s.running.store(true, Ordering::SeqCst);
            spawn_ticker();
            super::macos_focus::start();
            emit_state(app);
            Ok(())
        }
        Err(e) => {
            let wins: Vec<Sendable<*mut AnyObject>> = built.1.iter().map(|b| Sendable(b.1 .0)).collect();
            let _ = on_main(app, move || unsafe {
                for w in wins {
                    close_window(w.0);
                }
            });
            Err(e)
        }
    }
}

/// Stop the overlay. Blocking — call from a worker thread.
pub fn stop(app: &AppHandle) {
    let _op = OP.lock().unwrap_or_else(|e| e.into_inner());
    stop_locked(app);
}

fn stop_locked(app: &AppHandle) {
    let s = shared();
    if !s.running.swap(false, Ordering::SeqCst) {
        return;
    }
    let (f, r) = counters();
    tracing::info!("retro: stopping — {f} frames captured, {r} drawn since app start");
    stop_streams(&STREAMS);
    super::macos_focus::stop();
    // Drain the render queue so no draw holds a target past this point.
    let (tx, rx) = mpsc::channel::<()>();
    render_queue().exec_async(move || {
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(Duration::from_secs(2));
    let targets = std::mem::take(&mut *s.targets.lock().unwrap_or_else(|e| e.into_inner()));
    let wins: Vec<Sendable<*mut AnyObject>> = targets.iter().map(|t| Sendable(t.window.0)).collect();
    drop(targets);
    let _ = on_main(app, move || unsafe {
        for w in wins {
            close_window(w.0);
        }
    });
    emit_state(app);
}

/// Called when capture or rendering fails: stop and tell the user.
fn fail(reason: &str) {
    let s = shared();
    if s.failed.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(app) = s.app.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
    let reason = reason.to_string();
    std::thread::spawn(move || {
        stop(&app);
        toast(&app, "Retro-Overlay gestoppt", &reason);
    });
}

pub fn toast(app: &AppHandle, title: &str, subtitle: &str) {
    let app2 = app.clone();
    let t = crate::status_toast::StatusToast {
        kind: "retro".into(),
        on: false,
        title: title.into(),
        subtitle: subtitle.into(),
    };
    let _ = app.run_on_main_thread(move || crate::status_toast::show_passive(&app2, t));
}

/// Apply new settings to a running overlay. If the capture resolution has to
/// change (focus / lens toggled, pixel size changed) the running streams get
/// a new configuration — no restart.
pub fn apply(cfg: &RetroConfig) {
    let s = shared();
    let old = s.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let new = cfg.active().clone();
    *s.settings.lock().unwrap_or_else(|e| e.into_inner()) = new.clone();
    let fps_changed = {
        let mut f = s.fps.lock().unwrap_or_else(|e| e.into_inner());
        let changed = *f != cfg.fps;
        *f = cfg.fps;
        changed
    };
    if !is_running() {
        return;
    }
    let targets = s.targets.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let needs_reconf = fps_changed
        || targets.iter().any(|t| capture_size(&t.display, &old) != capture_size(&t.display, &new));
    if needs_reconf {
        if let Some(st) = STREAMS.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            for (stream, t) in st.streams.iter().zip(targets.iter()) {
                let (w, h) = capture_size(&t.display, &new);
                let conf = stream_config(w, h, cfg.fps);
                unsafe { stream.0.updateConfiguration_completionHandler(&conf, None) };
            }
        }
    }
    redraw_all();
}

// ── Live inputs (mouse, focus, windows) ─────────────────────────────────────

/// Update the focus / window data (called by the focus source).
pub fn set_live(focus: Option<Rect>, fullscreen: bool, windows: Vec<WinInfo>) {
    let s = shared();
    {
        let mut live = s.live.lock().unwrap_or_else(|e| e.into_inner());
        if live.focus == focus && live.focus_fullscreen == fullscreen && live.windows == windows {
            return;
        }
        live.focus = focus;
        live.focus_fullscreen = fullscreen;
        live.windows = windows;
    }
    redraw_all();
}

/// Settings the focus source needs (does it have to read anything at all?).
pub fn wants_focus_data() -> (bool, bool) {
    let s = shared().settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
    (s.focus, s.retro_frames)
}

fn spawn_ticker() {
    std::thread::Builder::new()
        .name("ir-retro-tick".into())
        .spawn(|| {
            let s = shared();
            let mut last_mouse = (f64::NAN, f64::NAN);
            while s.running.load(Ordering::SeqCst) {
                let fps = (*s.fps.lock().unwrap_or_else(|e| e.into_inner())).max(1);
                std::thread::sleep(Duration::from_millis(1000 / fps as u64));
                let st = s.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
                if !(st.lens || st.sprite_cursor) {
                    continue; // nothing follows the mouse → no re-render
                }
                let m = mouse_global(*s.primary_h.lock().unwrap_or_else(|e| e.into_inner()));
                if m != last_mouse {
                    last_mouse = m;
                    redraw_all();
                }
            }
        })
        .ok();
}

fn emit_state(app: &AppHandle) {
    let _ = app.emit("retro-state-changed", is_running());
    if let Some(item) = app.try_state::<crate::RetroTrayItem>() {
        let _ = item.inner().0.set_enabled(is_running());
    }
}

/// Counters for the performance measurement (`frames` = captured frames,
/// `renders` = drawn frames).
pub fn counters() -> (u64, u64) {
    let s = shared();
    (s.frames.load(Ordering::Relaxed), s.renders.load(Ordering::Relaxed))
}

// ── Preview stream ──────────────────────────────────────────────────────────

/// Start the ≤10 fps preview capture of the monitor under the cursor.
pub fn preview_start(app: &AppHandle, cfg: &RetroConfig) -> Result<(), String> {
    if !supported() {
        return Err(ERR_UNSUPPORTED.into());
    }
    if !crate::screen_recording::screen_recording_granted() {
        return Err(ERR_NO_PERMISSION.into());
    }
    set_app(app);
    let s = shared();
    if !is_running() {
        *s.settings.lock().unwrap_or_else(|e| e.into_inner()) = cfg.active().clone();
    }
    preview_stop();
    let (primary_h, d) = on_main(app, || unsafe {
        let all = displays();
        let ph = all.first().map(|d| d.h).unwrap_or(0.0);
        let m = mouse_global(ph);
        (ph, all.iter().find(|d| d.contains(m.0, m.1)).or(all.first()).copied())
    })
    .ok_or("Bildschirm unbekannt")?;
    let d = d.ok_or("Kein Bildschirm")?;
    *s.primary_h.lock().unwrap_or_else(|e| e.into_inner()) = primary_h;
    let content = shareable_content()?;
    let ours: Vec<u32> = s
        .targets
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|t| unsafe { window_number(t.window.0) })
        .collect();
    let (displays, windows) = unsafe { (content.0.displays(), content.0.windows()) };
    let excluded: Vec<Retained<SCWindow>> = windows.iter().filter(|w| ours.contains(&unsafe { w.windowID() })).collect();
    let exclude = NSArray::from_retained_slice(&excluded);
    let scd = displays.iter().find(|x| unsafe { x.displayID() } == d.id).ok_or("Bildschirm nicht erfassbar")?;
    let pw = 480u32;
    let ph = ((pw as f64) * d.h / d.w).round().max(1.0) as u32;
    let conf = stream_config(pw, ph, 10);
    let (st, out) = start_stream(&scd, &exclude, &conf, OutKind::Preview(d))?;
    *PREVIEW.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(Streams { streams: vec![Sendable(st)], _outputs: vec![Sendable(out)] });
    Ok(())
}

pub fn preview_stop() {
    stop_streams(&PREVIEW);
}

/// Settings changed while only the preview runs.
pub fn set_preview_settings(cfg: &RetroConfig) {
    if !is_running() {
        *shared().settings.lock().unwrap_or_else(|e| e.into_inner()) = cfg.active().clone();
    }
}

pub fn primary_h() -> f64 {
    *shared().primary_h.lock().unwrap_or_else(|e| e.into_inner())
}

// ── System lifecycle: sleep, lock, display changes ──────────────────────────

/// Register once at startup. Sleep / screen lock / session switch stop the
/// overlay (with a toast); a display change (hot-plug, resolution, scale)
/// rebuilds it.
pub fn install_lifecycle(app: &AppHandle) {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Ordering::SeqCst) {
        return;
    }
    set_app(app);
    unsafe {
        let observe = |center: *mut AnyObject, name: &str, f: Box<dyn Fn() + 'static>| {
            if center.is_null() {
                return;
            }
            let n = NSString::from_str(name);
            let block = RcBlock::new(move |_note: NonNull<AnyObject>| f());
            let token: *mut AnyObject = msg_send![
                center,
                addObserverForName: &*n,
                object: std::ptr::null::<AnyObject>(),
                queue: std::ptr::null::<AnyObject>(),
                usingBlock: &*block
            ];
            // The observer lives for the whole process.
            if !token.is_null() {
                let _: *mut AnyObject = msg_send![token, retain];
            }
        };
        let ws_center: *mut AnyObject = AnyClass::get(c"NSWorkspace")
            .map(|c| {
                let w: *mut AnyObject = msg_send![c, sharedWorkspace];
                msg_send![w, notificationCenter]
            })
            .unwrap_or(std::ptr::null_mut());
        let dist: *mut AnyObject = AnyClass::get(c"NSDistributedNotificationCenter")
            .map(|c| msg_send![c, defaultCenter])
            .unwrap_or(std::ptr::null_mut());
        let local: *mut AnyObject = AnyClass::get(c"NSNotificationCenter")
            .map(|c| msg_send![c, defaultCenter])
            .unwrap_or(std::ptr::null_mut());
        for (center, name, why) in [
            (ws_center, "NSWorkspaceWillSleepNotification", "Ruhezustand"),
            (ws_center, "NSWorkspaceScreensDidSleepNotification", "Bildschirm aus"),
            (ws_center, "NSWorkspaceSessionDidResignActiveNotification", "Benutzerwechsel"),
            (dist, "com.apple.screenIsLocked", "Bildschirm gesperrt"),
        ] {
            let a = app.clone();
            observe(center, name, Box::new(move || super::control::on_system_pause(&a, why)));
        }
        let a = app.clone();
        observe(
            local,
            "NSApplicationDidChangeScreenParametersNotification",
            Box::new(move || super::control::on_displays_changed(&a)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_metal::{MTLOrigin, MTLRegion, MTLSize, MTLStorageMode, MTLTexture, MTLTextureDescriptor, MTLTextureUsage};

    /// Render RGBA `src` through the real shader into an offscreen texture.
    fn gpu_render(src: &[u8], w: usize, h: usize, p: &super::super::frame::FrameParams) -> Vec<u8> {
        let g = gpu().expect("Metal + shader compile");
        unsafe {
            let make = |usage: MTLTextureUsage| {
                let d = MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                    MTLPixelFormat::BGRA8Unorm, w, h, false);
                d.setUsage(usage);
                d.setStorageMode(MTLStorageMode::Shared);
                g.device.newTextureWithDescriptor(&d).expect("texture")
            };
            let input = make(MTLTextureUsage::ShaderRead);
            let bgra: Vec<u8> = src.chunks(4).flat_map(|c| [c[2], c[1], c[0], 255]).collect();
            let region = MTLRegion { origin: MTLOrigin { x: 0, y: 0, z: 0 }, size: MTLSize { width: w, height: h, depth: 1 } };
            input.replaceRegion_mipmapLevel_withBytes_bytesPerRow(region, 0, NonNull::new_unchecked(bgra.as_ptr() as *mut c_void), w * 4);
            let target = make(MTLTextureUsage::RenderTarget);
            let pass = MTLRenderPassDescriptor::new();
            let att = pass.colorAttachments().objectAtIndexedSubscript(0);
            att.setTexture(Some(&target));
            att.setLoadAction(MTLLoadAction::DontCare);
            att.setStoreAction(MTLStoreAction::Store);
            let cb = g.queue.commandBuffer().unwrap();
            let enc = cb.renderCommandEncoderWithDescriptor(&pass).unwrap();
            encode_frame(&enc, g, &input, &pack(p));
            enc.endEncoding();
            cb.commit();
            cb.waitUntilCompleted();
            let mut out = vec![0u8; w * h * 4];
            target.getBytes_bytesPerRow_fromRegion_mipmapLevel(NonNull::new_unchecked(out.as_mut_ptr() as *mut c_void), w * 4, region, 0);
            out.chunks(4).flat_map(|c| [c[2], c[1], c[0], 255]).collect()
        }
    }

    fn photo(w: usize, h: usize) -> Vec<u8> {
        // deterministic "busy" image: gradients + a hash pattern
        (0..w * h)
            .flat_map(|i| {
                let (x, y) = (i % w, i / w);
                let n = ((x as u32).wrapping_mul(2_654_435_761) ^ (y as u32).wrapping_mul(40_503)) >> 24;
                [(x * 255 / w) as u8, (y * 255 / h) as u8, n as u8, 255]
            })
            .collect()
    }

    fn compare(name: &str, settings: super::super::config::ModeSettings, live: Live) {
        let d = Display { id: 0, x: 0.0, y: 0.0, w: 160.0, h: 100.0, scale: 2.0 };
        let p = frame_params(&d, &settings, &live);
        let (w, h) = (p.size.0 as usize, p.size.1 as usize);
        let src = photo(w, h);
        let cpu = render(&src, w, h, &render_params(&p));
        let gpu = gpu_render(&src, w, h, &p);
        let diff = cpu.chunks(4).zip(gpu.chunks(4)).filter(|(a, b)| (0..3).any(|k| (a[k] as i32 - b[k] as i32).abs() > 2)).count();
        let share = diff as f64 / (w * h) as f64;
        if share >= 0.01 {
            for (i, (a, b)) in cpu.chunks(4).zip(gpu.chunks(4)).enumerate().filter(|(_, (a, b))| (0..3).any(|k| (a[k] as i32 - b[k] as i32).abs() > 2)).take(6) {
                let (x, y) = (i % w, i / w);
                eprintln!("{name}: ({x},{y}) src {:?} cpu {:?} gpu {:?}", &src[i * 4..i * 4 + 3], &a[..3], &b[..3]);
            }
        }
        assert!(share < 0.01, "{name}: GPU and CPU reference differ on {:.2} % of pixels", share * 100.0);
    }

    /// The shader must match the CPU reference. Needs a Metal device, so it
    /// is ignored in CI (Linux). Run on a Mac:
    /// `cargo test -p inspector-rust-core --lib shader_matches -- --ignored`
    #[test]
    #[ignore]
    fn shader_matches_the_cpu_reference() {
        use super::super::config::{Dither, LensView, Mode, ModeSettings};
        let eight = ModeSettings::defaults(Mode::Eight);
        compare("8-bit default", eight.clone(), Live::default());
        compare("16-bit default", ModeSettings { focus: false, ..ModeSettings::defaults(Mode::Sixteen) }, Live::default());
        compare("gb no dither", ModeSettings { palette: "gb".into(), dither: Dither::Off, scanlines: false, ..eight.clone() }, Live::default());
        let focus = Live { focus: Some(Rect { x: 20.0, y: 10.0, w: 60.0, h: 40.0 }), mouse: Some((120.0, 70.0)), ..Live::default() };
        compare("focus + border + lens(original)", ModeSettings { focus: true, focus_pixel_pt: 1.5, lens: true, lens_radius_pt: 40, ..eight.clone() }, focus.clone());
        compare("lens(focus cells) + bayer8", ModeSettings { lens: true, lens_view: LensView::Focus, lens_radius_pt: 40, dither: Dither::Bayer8, ..eight }, focus);
    }
}
