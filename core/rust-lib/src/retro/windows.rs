//! Windows retro overlay: Windows.Graphics.Capture → D3D11 shader
//! (`shader.hlsl`, a port of `shader.metal`) → DirectComposition swap chain in
//! a click-through topmost window per monitor. **Runtime-unverified**: written
//! and cross-compiled from macOS (`cargo check --target x86_64-pc-windows-gnu`)
//! like the other Windows backends of this repo; the HLSL is validated with
//! glslang's HLSL front end, not with fxc.
//!
//! * Feedback loop: the overlay windows are excluded from every capture with
//!   `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`. That flag exists from
//!   Windows 10 2004 (build 19041); older builds report the feature as
//!   unsupported instead of starting a self-capturing loop.
//! * WGC cannot scale, so Windows always captures at native resolution; the
//!   shader does the cell sampling (macOS additionally captures low-res when
//!   neither focus nor lens need detail).
//! * Focus and the stage-2 window list come from WinEvent hooks
//!   (`EVENT_SYSTEM_FOREGROUND`, `EVENT_OBJECT_LOCATIONCHANGE`) — event-driven,
//!   on the overlay thread's message loop — plus `EnumWindows` for frames.
//! * Lock, sleep and display changes arrive on the overlay's message window
//!   (WTS session notifications, `WM_POWERBROADCAST`, `WM_DISPLAYCHANGE`).
//!
//! Coordinates: Windows works in physical pixels (per-monitor DPI aware). The
//! shared frame math works in points + scale, so every monitor is described
//! as `x = left / scale` etc., and global pixel inputs are mapped through the
//! monitor they fall on.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};
use windows::core::{Interface, BOOL, PCSTR, PCWSTR};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, TRUE, WPARAM};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::{ID3DBlob, D3D_DRIVER_TYPE_HARDWARE, D3D11_SRV_DIMENSION_BUFFEREX, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::DirectComposition::{DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::config::{ModeSettings, RetroConfig, Target as MonTarget};
use super::frame::{frame_params, pack, render_params, scaled, Display, GpuFrame, GpuUniforms, GpuWin, Live, WinInfo, MAX_PALETTE, MAX_TITLE, MAX_WINDOWS};
use super::{render, Rect};

pub const ERR_UNSUPPORTED: &str = "retro.unsupported";
const SHADER: &str = include_str!("shader.hlsl");
/// Windows 10 2004 — first build with WDA_EXCLUDEFROMCAPTURE.
const MIN_BUILD: u32 = 19041;

const WM_RETRO_QUIT: u32 = WM_APP + 0x31;
const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const EVENT_OBJECT_LOCATIONCHANGE: u32 = 0x800B;
const WINEVENT_OUTOFCONTEXT: u32 = 0;
const WTS_SESSION_LOCK: usize = 7;
const PBT_APMSUSPEND: usize = 4;
const OBJID_WINDOW: i32 = 0;

struct Sendable<T>(T);
unsafe impl<T> Send for Sendable<T> {}
unsafe impl<T> Sync for Sendable<T> {}

// ── Platform checks ─────────────────────────────────────────────────────────

fn windows_build() -> u32 {
    // RtlGetVersion is not shimmed by the manifest like GetVersionEx.
    #[repr(C)]
    struct OsVersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        csd: [u16; 128],
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(info: *mut OsVersionInfo) -> i32;
    }
    let mut v = OsVersionInfo { size: std::mem::size_of::<OsVersionInfo>() as u32, major: 0, minor: 0, build: 0, platform: 0, csd: [0; 128] };
    unsafe { RtlGetVersion(&mut v) };
    v.build
}

pub fn supported() -> bool {
    windows_build() >= MIN_BUILD && GraphicsCaptureSession::IsSupported().unwrap_or(false)
}

pub fn screen_permission() -> bool {
    true // Windows has no capture permission gate.
}

pub fn accessibility() -> bool {
    true // WinEvent hooks need no grant.
}

pub fn note() -> Option<String> {
    (!supported()).then(|| "Braucht Windows 10 Version 2004 oder neuer (Bildschirmaufnahme ohne Rückkopplung).".to_string())
}

// ── Monitors ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Mon {
    handle: isize,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
    scale: f64,
}

impl Mon {
    fn display(&self, id: u32) -> Display {
        Display {
            id,
            x: self.left as f64 / self.scale,
            y: self.top as f64 / self.scale,
            w: (self.right - self.left) as f64 / self.scale,
            h: (self.bottom - self.top) as f64 / self.scale,
            scale: self.scale,
        }
    }
    fn contains_px(&self, x: i32, y: i32) -> bool {
        x >= self.left && y >= self.top && x < self.right && y < self.bottom
    }
}

fn monitors() -> Vec<Mon> {
    unsafe extern "system" fn cb(h: HMONITOR, _dc: HDC, _r: *mut RECT, data: LPARAM) -> BOOL {
        let v = &mut *(data.0 as *mut Vec<Mon>);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(h, &mut info).as_bool() {
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let r = info.rcMonitor;
            v.push(Mon { handle: h.0 as isize, left: r.left, top: r.top, right: r.right, bottom: r.bottom, scale: dx.max(48) as f64 / 96.0 });
        }
        TRUE
    }
    let mut v: Vec<Mon> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(cb), LPARAM(&mut v as *mut _ as isize));
    }
    v
}

/// Global physical px → global "points" (through the monitor it falls on).
fn px_to_pt(mons: &[Mon], x: i32, y: i32) -> (f64, f64) {
    match mons.iter().find(|m| m.contains_px(x, y)).or(mons.first()) {
        Some(m) => (m.left as f64 / m.scale + (x - m.left) as f64 / m.scale, m.top as f64 / m.scale + (y - m.top) as f64 / m.scale),
        None => (x as f64, y as f64),
    }
}

fn rect_to_pt(mons: &[Mon], r: RECT) -> Rect {
    let (x, y) = px_to_pt(mons, r.left, r.top);
    let scale = mons.iter().find(|m| m.contains_px(r.left, r.top)).map(|m| m.scale).unwrap_or(1.0);
    Rect { x: x as f32, y: y as f32, w: ((r.right - r.left) as f64 / scale) as f32, h: ((r.bottom - r.top) as f64 / scale) as f32 }
}

fn mouse_pt() -> Option<(f64, f64)> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p).ok()? };
    Some(px_to_pt(&monitors(), p.x, p.y))
}

// ── GPU ─────────────────────────────────────────────────────────────────────

struct Gpu {
    device: ID3D11Device,
    ctx: Mutex<ID3D11DeviceContext>,
    winrt_device: IDirect3DDevice,
    vs: ID3D11VertexShader,
    ps: ID3D11PixelShader,
    cbuf: ID3D11Buffer,
    pal: (ID3D11Buffer, ID3D11ShaderResourceView),
    bayer: (ID3D11Buffer, ID3D11ShaderResourceView),
    wins: (ID3D11Buffer, ID3D11ShaderResourceView),
    titles: (ID3D11Buffer, ID3D11ShaderResourceView),
    font: (ID3D11Buffer, ID3D11ShaderResourceView),
}
unsafe impl Send for Gpu {}
unsafe impl Sync for Gpu {}

fn compile(entry: &str, target: &str) -> Result<ID3DBlob, String> {
    let mut code: Option<ID3DBlob> = None;
    let mut errs: Option<ID3DBlob> = None;
    let entry_c = std::ffi::CString::new(entry).unwrap();
    let target_c = std::ffi::CString::new(target).unwrap();
    let r = unsafe {
        D3DCompile(
            SHADER.as_ptr() as *const _,
            SHADER.len(),
            PCSTR(c"shader.hlsl".as_ptr() as *const u8),
            None,
            None,
            PCSTR(entry_c.as_ptr() as *const u8),
            PCSTR(target_c.as_ptr() as *const u8),
            0,
            0,
            &mut code,
            Some(&mut errs),
        )
    };
    if r.is_err() {
        let msg = errs
            .map(|e| unsafe {
                let s = std::slice::from_raw_parts(e.GetBufferPointer() as *const u8, e.GetBufferSize());
                String::from_utf8_lossy(s).into_owned()
            })
            .unwrap_or_default();
        return Err(format!("HLSL {entry}: {msg}"));
    }
    code.ok_or_else(|| "HLSL: kein Code".into())
}

fn blob_bytes(b: &ID3DBlob) -> &[u8] {
    unsafe { std::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize()) }
}

/// A dynamic structured buffer with its SRV.
fn structured(device: &ID3D11Device, stride: u32, count: u32) -> Result<(ID3D11Buffer, ID3D11ShaderResourceView), String> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: stride * count,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: D3D11_RESOURCE_MISC_BUFFER_STRUCTURED.0 as u32,
        StructureByteStride: stride,
    };
    let mut buf = None;
    unsafe { device.CreateBuffer(&desc, None, Some(&mut buf)) }.map_err(|e| e.to_string())?;
    let buf = buf.ok_or("buffer")?;
    let srv_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_UNKNOWN,
        ViewDimension: D3D11_SRV_DIMENSION_BUFFEREX,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            BufferEx: D3D11_BUFFEREX_SRV { FirstElement: 0, NumElements: count, Flags: 0 },
        },
    };
    let mut srv = None;
    unsafe { device.CreateShaderResourceView(&buf, Some(&srv_desc), Some(&mut srv)) }.map_err(|e| e.to_string())?;
    Ok((buf, srv.ok_or("srv")?))
}

fn write_buffer<T: Copy>(ctx: &ID3D11DeviceContext, buf: &ID3D11Buffer, data: &[T], max: usize) {
    let n = data.len().min(max);
    unsafe {
        let mut m = D3D11_MAPPED_SUBRESOURCE::default();
        if ctx.Map(buf, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut m)).is_ok() {
            std::ptr::copy_nonoverlapping(data.as_ptr(), m.pData as *mut T, n);
            ctx.Unmap(buf, 0);
        }
    }
}

fn gpu() -> Result<&'static Gpu, String> {
    static G: OnceLock<Result<Gpu, String>> = OnceLock::new();
    G.get_or_init(|| unsafe {
        let mut device = None;
        let mut ctx = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut ctx),
        )
        .map_err(|e| format!("D3D11: {e}"))?;
        let device: ID3D11Device = device.ok_or("D3D11-Gerät")?;
        let ctx: ID3D11DeviceContext = ctx.ok_or("D3D11-Kontext")?;
        let dxgi: IDXGIDevice = device.cast().map_err(|e| e.to_string())?;
        let winrt_device: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi).map_err(|e| e.to_string())?.cast().map_err(|e| e.to_string())?;
        let vsb = compile("retro_vs", "vs_5_0")?;
        let psb = compile("retro_ps", "ps_5_0")?;
        let mut vs = None;
        device.CreateVertexShader(blob_bytes(&vsb), None, Some(&mut vs)).map_err(|e| e.to_string())?;
        let mut ps = None;
        device.CreatePixelShader(blob_bytes(&psb), None, Some(&mut ps)).map_err(|e| e.to_string())?;
        let cdesc = D3D11_BUFFER_DESC {
            ByteWidth: std::mem::size_of::<GpuUniforms>() as u32,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            ..Default::default()
        };
        let mut cbuf = None;
        device.CreateBuffer(&cdesc, None, Some(&mut cbuf)).map_err(|e| e.to_string())?;
        let font = structured(&device, 4, super::frame::font().len() as u32)?;
        let font_u32: Vec<u32> = super::frame::font().iter().map(|b| *b as u32).collect();
        write_buffer(&ctx, &font.0, &font_u32, font_u32.len());
        Ok(Gpu {
            pal: structured(&device, 16, MAX_PALETTE as u32)?,
            bayer: structured(&device, 4, 64)?,
            wins: structured(&device, 32, MAX_WINDOWS as u32)?,
            titles: structured(&device, 4, (MAX_WINDOWS * MAX_TITLE) as u32)?,
            font,
            device,
            ctx: Mutex::new(ctx),
            winrt_device,
            vs: vs.ok_or("vs")?,
            ps: ps.ok_or("ps")?,
            cbuf: cbuf.ok_or("cbuf")?,
        })
    })
    .as_ref()
    .map_err(|e| e.clone())
}

// ── Engine state ────────────────────────────────────────────────────────────

struct Target {
    display: Display,
    hwnd: HWND,
    swap: IDXGISwapChain1,
    _dcomp: (IDCompositionDevice, IDCompositionTarget, IDCompositionVisual),
    rtv: ID3D11RenderTargetView,
    /// Our copy of the last captured frame (the pool recycles its surfaces).
    last: Mutex<Option<(ID3D11Texture2D, ID3D11ShaderResourceView)>>,
    shown: AtomicBool,
    _capture: Mutex<Option<(Direct3D11CaptureFramePool, GraphicsCaptureSession)>>,
}
unsafe impl Send for Target {}
unsafe impl Sync for Target {}

struct Shared {
    settings: Mutex<ModeSettings>,
    fps: Mutex<u32>,
    live: Mutex<Live>,
    targets: Mutex<Vec<Arc<Target>>>,
    running: AtomicBool,
    thread_id: AtomicU32,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    app: Mutex<Option<AppHandle>>,
    preview: Mutex<Option<Sendable<(Direct3D11CaptureFramePool, GraphicsCaptureSession)>>>,
}

fn shared() -> &'static Shared {
    static S: OnceLock<Shared> = OnceLock::new();
    S.get_or_init(|| Shared {
        settings: Mutex::new(ModeSettings::default()),
        fps: Mutex::new(60),
        live: Mutex::new(Live::default()),
        targets: Mutex::new(Vec::new()),
        running: AtomicBool::new(false),
        thread_id: AtomicU32::new(0),
        thread: Mutex::new(None),
        app: Mutex::new(None),
        preview: Mutex::new(None),
    })
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn is_running() -> bool {
    shared().running.load(Ordering::SeqCst)
}

// ── Rendering ───────────────────────────────────────────────────────────────

fn draw(t: &Target) {
    let s = shared();
    if !s.running.load(Ordering::SeqCst) {
        return;
    }
    let Ok(g) = gpu() else { return };
    let last = lock(&t.last);
    let Some((_, srv)) = last.as_ref() else { return };
    let settings = lock(&s.settings).clone();
    let mut live = lock(&s.live).clone();
    live.mouse = mouse_pt();
    let frame = pack(&frame_params(&t.display, &settings, &live));
    let ctx = lock(&g.ctx);
    unsafe { encode(&ctx, g, srv, &t.rtv, &frame, t.display.px_size()) };
    let _ = unsafe { t.swap.Present(1, DXGI_PRESENT(0)) };
    if !t.shown.swap(true, Ordering::SeqCst) {
        unsafe {
            let _ = ShowWindow(t.hwnd, SW_SHOWNOACTIVATE);
        }
    }
}

unsafe fn encode(
    ctx: &ID3D11DeviceContext,
    g: &Gpu,
    src: &ID3D11ShaderResourceView,
    rtv: &ID3D11RenderTargetView,
    frame: &GpuFrame,
    (w, h): (u32, u32),
) {
    write_buffer(ctx, &g.cbuf, std::slice::from_ref(&frame.uniforms), 1);
    write_buffer(ctx, &g.pal.0, &frame.palette, MAX_PALETTE);
    write_buffer(ctx, &g.bayer.0, &frame.bayer, 64);
    write_buffer::<GpuWin>(ctx, &g.wins.0, &frame.windows, MAX_WINDOWS);
    let titles: Vec<u32> = frame.titles.iter().map(|b| *b as u32).collect();
    write_buffer(ctx, &g.titles.0, &titles, MAX_WINDOWS * MAX_TITLE);
    ctx.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
    ctx.RSSetViewports(Some(&[D3D11_VIEWPORT { TopLeftX: 0.0, TopLeftY: 0.0, Width: w as f32, Height: h as f32, MinDepth: 0.0, MaxDepth: 1.0 }]));
    ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    ctx.IASetInputLayout(None);
    ctx.VSSetShader(&g.vs, None);
    ctx.PSSetShader(&g.ps, None);
    ctx.PSSetConstantBuffers(0, Some(&[Some(g.cbuf.clone())]));
    ctx.PSSetShaderResources(
        0,
        Some(&[
            Some(src.clone()),
            Some(g.pal.1.clone()),
            Some(g.bayer.1.clone()),
            Some(g.wins.1.clone()),
            Some(g.titles.1.clone()),
            Some(g.font.1.clone()),
        ]),
    );
    ctx.Draw(3, 0);
}

fn redraw_all() {
    let targets = lock(&shared().targets).clone();
    std::thread::spawn(move || {
        for t in &targets {
            draw(t);
        }
    });
}

/// Copy a captured surface into the target's own texture (the frame pool
/// recycles its surfaces) and draw.
fn on_frame(t: &Arc<Target>, pool: &Direct3D11CaptureFramePool) {
    let Ok(g) = gpu() else { return };
    let Ok(frame) = pool.TryGetNextFrame() else { return };
    let Ok(surface) = frame.Surface() else { return };
    let Ok(access) = surface.cast::<IDirect3DDxgiInterfaceAccess>() else { return };
    let Ok(tex) = (unsafe { access.GetInterface::<ID3D11Texture2D>() }) else { return };
    {
        let ctx = lock(&g.ctx);
        let mut last = lock(&t.last);
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { tex.GetDesc(&mut desc) };
        let fits = last.as_ref().is_some_and(|(own, _)| {
            let mut d = D3D11_TEXTURE2D_DESC::default();
            unsafe { own.GetDesc(&mut d) };
            d.Width == desc.Width && d.Height == desc.Height
        });
        if !fits {
            let own_desc = D3D11_TEXTURE2D_DESC {
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                CPUAccessFlags: 0,
                MiscFlags: 0,
                ..desc
            };
            let mut own = None;
            let mut srv = None;
            unsafe {
                if g.device.CreateTexture2D(&own_desc, None, Some(&mut own)).is_err() {
                    return;
                }
                let own = own.unwrap();
                if g.device.CreateShaderResourceView(&own, None, Some(&mut srv)).is_err() {
                    return;
                }
                *last = Some((own, srv.unwrap()));
            }
        }
        if let Some((own, _)) = last.as_ref() {
            unsafe { ctx.CopyResource(own, &tex) };
        }
    }
    let _ = frame.Close();
    draw(t);
}

// ── Windows ─────────────────────────────────────────────────────────────────

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let app = lock(&shared().app).clone();
    match msg {
        WM_WTSSESSION_CHANGE if wp.0 == WTS_SESSION_LOCK => {
            if let Some(a) = app {
                super::control::on_system_pause(&a, "Bildschirm gesperrt");
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST if wp.0 == PBT_APMSUSPEND => {
            if let Some(a) = app {
                super::control::on_system_pause(&a, "Ruhezustand");
            }
            LRESULT(1)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            if let Some(a) = app {
                super::control::on_displays_changed(&a);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn class_name() -> PCWSTR {
    windows::core::w!("IRRetroOverlay")
}

unsafe fn register_class() {
    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Ordering::SeqCst) {
        return;
    }
    let hinst = GetModuleHandleW(None).unwrap_or_default();
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: hinst.into(),
        lpszClassName: class_name(),
        ..Default::default()
    };
    RegisterClassExW(&wc);
}

/// Create the overlay window + DirectComposition swap chain for `m`.
unsafe fn build_target(g: &Gpu, m: &Mon, d: Display) -> Result<Arc<Target>, String> {
    register_class();
    let hinst = GetModuleHandleW(None).unwrap_or_default();
    let ex = WS_EX_NOREDIRECTIONBITMAP | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
    let hwnd = CreateWindowExW(
        ex,
        class_name(),
        windows::core::w!("Retro-Overlay"),
        WS_POPUP,
        m.left,
        m.top,
        m.right - m.left,
        m.bottom - m.top,
        None,
        None,
        Some(hinst.into()),
        None,
    )
    .map_err(|e| e.to_string())?;
    // Keeps the overlay out of every capture — ours (no feedback loop) and
    // screenshots / recordings.
    if SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).is_err() {
        let _ = DestroyWindow(hwnd);
        return Err(ERR_UNSUPPORTED.into());
    }
    let _ = SetLayeredWindowAttributes(hwnd, windows::Win32::Foundation::COLORREF(0), 255, LWA_ALPHA);
    let (w, h) = d.px_size();
    let dxgi: IDXGIDevice = g.device.cast().map_err(|e| e.to_string())?;
    let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).map_err(|e| e.to_string())?;
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: w,
        Height: h,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        ..Default::default()
    };
    let swap = factory.CreateSwapChainForComposition(&g.device, &desc, None).map_err(|e| e.to_string())?;
    let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi).map_err(|e| e.to_string())?;
    let target = dcomp.CreateTargetForHwnd(hwnd, true).map_err(|e| e.to_string())?;
    let visual = dcomp.CreateVisual().map_err(|e| e.to_string())?;
    visual.SetContent(&swap).map_err(|e| e.to_string())?;
    target.SetRoot(&visual).map_err(|e| e.to_string())?;
    dcomp.Commit().map_err(|e| e.to_string())?;
    let back: ID3D11Texture2D = swap.GetBuffer(0).map_err(|e| e.to_string())?;
    let mut rtv = None;
    g.device.CreateRenderTargetView(&back, None, Some(&mut rtv)).map_err(|e| e.to_string())?;
    Ok(Arc::new(Target {
        display: d,
        hwnd,
        swap,
        _dcomp: (dcomp, target, visual),
        rtv: rtv.ok_or("rtv")?,
        last: Mutex::new(None),
        shown: AtomicBool::new(false),
        _capture: Mutex::new(None),
    }))
}

fn capture_item(m: &Mon) -> Result<GraphicsCaptureItem, String> {
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(|e| e.to_string())?;
    unsafe { interop.CreateForMonitor(HMONITOR(m.handle as *mut _)) }.map_err(|e| e.to_string())
}

fn start_capture(
    g: &Gpu,
    m: &Mon,
    on_frame: impl Fn(&Direct3D11CaptureFramePool) + Send + Sync + 'static,
) -> Result<(Direct3D11CaptureFramePool, GraphicsCaptureSession), String> {
    let item = capture_item(m)?;
    let size = item.Size().map_err(|e| e.to_string())?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&g.winrt_device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)
        .map_err(|e| e.to_string())?;
    pool.FrameArrived(&TypedEventHandler::new(move |p: windows::core::Ref<Direct3D11CaptureFramePool>, _| {
        if let Some(p) = p.as_ref() {
            on_frame(p);
        }
        Ok(())
    }))
    .map_err(|e| e.to_string())?;
    let session = pool.CreateCaptureSession(&item).map_err(|e| e.to_string())?;
    let _ = session.SetIsCursorCaptureEnabled(false);
    let _ = session.SetIsBorderRequired(false); // Windows 11; harmless where unsupported
    session.StartCapture().map_err(|e| e.to_string())?;
    Ok((pool, session))
}

// ── Focus + window list (WinEvent) ──────────────────────────────────────────

fn window_rect(hwnd: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    unsafe {
        DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32).ok()?;
    }
    (r.right > r.left && r.bottom > r.top).then_some(r)
}

fn is_ours(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid == std::process::id()
}

fn refresh_live() {
    let s = shared();
    let st = lock(&s.settings).clone();
    let mons = monitors();
    let fg = unsafe { GetForegroundWindow() };
    let (focus, full) = if st.focus && !fg.is_invalid() && !is_ours(fg) {
        match window_rect(fg) {
            Some(r) => {
                let full = mons.iter().any(|m| r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom);
                (Some(rect_to_pt(&mons, r)), full)
            }
            None => (None, false),
        }
    } else {
        (lock(&s.live).focus, lock(&s.live).focus_fullscreen)
    };
    let windows = if st.retro_frames { window_list(&mons, fg) } else { Vec::new() };
    {
        let mut live = lock(&s.live);
        if live.focus == focus && live.focus_fullscreen == full && live.windows == windows {
            return;
        }
        live.focus = if st.focus { focus } else { None };
        live.focus_fullscreen = full;
        live.windows = windows;
    }
    redraw_all();
}

fn window_list(mons: &[Mon], fg: HWND) -> Vec<WinInfo> {
    unsafe extern "system" fn cb(hwnd: HWND, data: LPARAM) -> BOOL {
        let v = &mut *(data.0 as *mut Vec<HWND>);
        v.push(hwnd);
        TRUE
    }
    let mut all: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut all as *mut _ as isize));
    }
    let mut out = Vec::new();
    for hwnd in all {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || is_ours(hwnd) {
                continue;
            }
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            if ex & WS_EX_TOOLWINDOW.0 != 0 {
                continue;
            }
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4);
            if cloaked != 0 {
                continue;
            }
            let Some(r) = window_rect(hwnd) else { continue };
            if r.right - r.left < 40 || r.bottom - r.top < 30 {
                continue;
            }
            let mut buf = [0u16; 256];
            let n = GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);
            if title.is_empty() {
                continue;
            }
            out.push(WinInfo { rect: rect_to_pt(mons, r), title, focused: hwnd == fg });
            if out.len() >= MAX_WINDOWS {
                break;
            }
        }
    }
    out
}

unsafe extern "system" fn on_win_event(_h: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32, _child: i32, _t: u32, _ms: u32) {
    if event == EVENT_OBJECT_LOCATIONCHANGE && (id_object != OBJID_WINDOW || hwnd != GetForegroundWindow()) {
        return; // only moves/resizes of the foreground window matter
    }
    refresh_live();
}

// ── Start / stop ────────────────────────────────────────────────────────────

pub fn start(app: &AppHandle, cfg: &RetroConfig) -> Result<(), String> {
    if is_running() {
        return Ok(());
    }
    if !supported() {
        return Err(ERR_UNSUPPORTED.into());
    }
    gpu()?;
    let s = shared();
    *lock(&s.app) = Some(app.clone());
    *lock(&s.settings) = cfg.active().clone();
    *lock(&s.fps) = cfg.fps;
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let target_mode = cfg.target;
    let handle = std::thread::Builder::new()
        .name("ir-retro-win".into())
        .spawn(move || unsafe { run_thread(target_mode, tx) })
        .map_err(|e| e.to_string())?;
    match rx.recv_timeout(Duration::from_secs(8)) {
        Ok(Ok(())) => {
            *lock(&s.thread) = Some(handle);
            emit_state(app);
            Ok(())
        }
        Ok(Err(e)) => {
            let _ = handle.join();
            Err(e)
        }
        Err(_) => Err("Overlay-Start hängt".into()),
    }
}

unsafe fn run_thread(target_mode: MonTarget, ready: mpsc::Sender<Result<(), String>>) {
    let s = shared();
    let g = match gpu() {
        Ok(g) => g,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let mons = monitors();
    let mut cur = POINT::default();
    let _ = GetCursorPos(&mut cur);
    let chosen: Vec<(u32, Mon)> = match target_mode {
        MonTarget::All => mons.iter().copied().enumerate().map(|(i, m)| (i as u32, m)).collect(),
        MonTarget::Current => mons
            .iter()
            .copied()
            .enumerate()
            .find(|(_, m)| m.contains_px(cur.x, cur.y))
            .or(mons.first().copied().map(|m| (0, m)))
            .map(|(i, m)| vec![(i as u32, m)])
            .unwrap_or_default(),
    };
    let mut targets = Vec::new();
    for (i, m) in &chosen {
        match build_target(g, m, m.display(*i)) {
            Ok(t) => targets.push((t, *m)),
            Err(e) => {
                for (t, _) in &targets {
                    let _ = DestroyWindow(t.hwnd);
                }
                let _ = ready.send(Err(e));
                return;
            }
        }
    }
    for (t, m) in &targets {
        let tt = t.clone();
        match start_capture(g, m, move |pool| on_frame(&tt, pool)) {
            Ok(c) => *lock(&t._capture) = Some(c),
            Err(e) => {
                for (t, _) in &targets {
                    let _ = DestroyWindow(t.hwnd);
                }
                let _ = ready.send(Err(e));
                return;
            }
        }
    }
    let first_hwnd = targets.first().map(|t| t.0.hwnd);
    *lock(&s.targets) = targets.into_iter().map(|t| t.0).collect();
    s.thread_id.store(GetCurrentThreadId(), Ordering::SeqCst);
    s.running.store(true, Ordering::SeqCst);
    if let Some(h) = first_hwnd {
        let _ = WTSRegisterSessionNotification(h, NOTIFY_FOR_THIS_SESSION);
    }
    let hooks = [
        SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_win_event), 0, 0, WINEVENT_OUTOFCONTEXT),
        SetWinEventHook(EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE, None, Some(on_win_event), 0, 0, WINEVENT_OUTOFCONTEXT),
    ];
    refresh_live();
    let _ = ready.send(Ok(()));

    // Ticker for mouse-following layers + the 1-s window-list refresh.
    std::thread::spawn(|| {
        let s = shared();
        let mut last_mouse = None;
        let mut last_list = Instant::now();
        while s.running.load(Ordering::SeqCst) {
            let fps = (*lock(&s.fps)).max(1);
            std::thread::sleep(Duration::from_millis(1000 / fps as u64));
            let st = lock(&s.settings).clone();
            if st.retro_frames && last_list.elapsed() >= Duration::from_secs(1) {
                last_list = Instant::now();
                refresh_live();
            }
            if st.lens || st.sprite_cursor {
                let m = mouse_pt();
                if m != last_mouse {
                    last_mouse = m;
                    redraw_all();
                }
            }
        }
    });

    let mut msg = MSG::default();
    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
        if msg.message == WM_RETRO_QUIT {
            break;
        }
        let _ = TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }

    s.running.store(false, Ordering::SeqCst);
    for h in hooks {
        let _ = UnhookWinEvent(h);
    }
    if let Some(h) = first_hwnd {
        let _ = WTSUnRegisterSessionNotification(h);
    }
    let targets = std::mem::take(&mut *lock(&s.targets));
    for t in &targets {
        if let Some((pool, session)) = lock(&t._capture).take() {
            let _ = session.Close();
            let _ = pool.Close();
        }
        let _ = DestroyWindow(t.hwnd);
    }
}

pub fn stop(app: &AppHandle) {
    let s = shared();
    if !is_running() {
        return;
    }
    let tid = s.thread_id.load(Ordering::SeqCst);
    unsafe {
        let _ = PostThreadMessageW(tid, WM_RETRO_QUIT, WPARAM(0), LPARAM(0));
    }
    if let Some(h) = lock(&s.thread).take() {
        let _ = h.join();
    }
    emit_state(app);
}

fn emit_state(app: &AppHandle) {
    let _ = app.emit("retro-state-changed", is_running());
    if let Some(item) = app.try_state::<crate::RetroTrayItem>() {
        let _ = item.inner().0.set_enabled(is_running());
    }
}

pub fn apply(cfg: &RetroConfig) {
    let s = shared();
    *lock(&s.settings) = cfg.active().clone();
    *lock(&s.fps) = cfg.fps;
    if is_running() {
        refresh_live();
        redraw_all();
    }
}

pub fn set_preview_settings(cfg: &RetroConfig) {
    if !is_running() {
        *lock(&shared().settings) = cfg.active().clone();
    }
}

// ── Preview ─────────────────────────────────────────────────────────────────

pub fn preview_start(app: &AppHandle, cfg: &RetroConfig) -> Result<(), String> {
    if !supported() {
        return Err(ERR_UNSUPPORTED.into());
    }
    let g = gpu()?;
    let s = shared();
    *lock(&s.app) = Some(app.clone());
    set_preview_settings(cfg);
    preview_stop();
    let mons = monitors();
    let mut cur = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut cur);
    }
    let (idx, m) = mons
        .iter()
        .copied()
        .enumerate()
        .find(|(_, m)| m.contains_px(cur.x, cur.y))
        .or(mons.first().copied().map(|m| (0, m)))
        .ok_or("Kein Bildschirm")?;
    let d = m.display(idx as u32);
    let last = Arc::new(Mutex::new(None::<Instant>));
    let cap = start_capture(g, &m, move |pool| {
        let Ok(frame) = pool.TryGetNextFrame() else { return };
        {
            let mut l = lock(&last);
            if l.is_some_and(|t| t.elapsed() < Duration::from_millis(95)) {
                let _ = frame.Close();
                return;
            }
            *l = Some(Instant::now());
        }
        if let Some((w, h, rgba)) = read_frame(&frame) {
            let _ = frame.Close();
            preview_emit(d, w, h, &rgba);
        }
    })?;
    *lock(&s.preview) = Some(Sendable(cap));
    Ok(())
}

pub fn preview_stop() {
    if let Some(Sendable((pool, session))) = lock(&shared().preview).take() {
        let _ = session.Close();
        let _ = pool.Close();
    }
}

/// Copy a frame to a staging texture, downscale nearest-neighbour to the
/// 480-px preview width, return RGBA.
fn read_frame(frame: &windows::Graphics::Capture::Direct3D11CaptureFrame) -> Option<(usize, usize, Vec<u8>)> {
    let g = gpu().ok()?;
    let surface = frame.Surface().ok()?;
    let tex: ID3D11Texture2D = unsafe { surface.cast::<IDirect3DDxgiInterfaceAccess>().ok()?.GetInterface().ok()? };
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { tex.GetDesc(&mut desc) };
    let staging_desc = D3D11_TEXTURE2D_DESC {
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
        ..desc
    };
    let mut staging = None;
    unsafe { g.device.CreateTexture2D(&staging_desc, None, Some(&mut staging)).ok()? };
    let staging = staging?;
    let ctx = lock(&g.ctx);
    unsafe { ctx.CopyResource(&staging, &tex) };
    let mut m = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut m)).ok()? };
    let (sw, sh) = (desc.Width as usize, desc.Height as usize);
    let pw = 480usize.min(sw).max(1);
    let ph = ((sh as f64) * pw as f64 / sw as f64).round().max(1.0) as usize;
    let mut out = Vec::with_capacity(pw * ph * 4);
    unsafe {
        let base = m.pData as *const u8;
        for y in 0..ph {
            let sy = (y * sh / ph).min(sh - 1);
            for x in 0..pw {
                let sx = (x * sw / pw).min(sw - 1);
                let p = base.add(sy * m.RowPitch as usize + sx * 4);
                out.extend_from_slice(&[*p.add(2), *p.add(1), *p, 255]);
            }
        }
        ctx.Unmap(&staging, 0);
    }
    Some((pw, ph, out))
}

fn preview_emit(d: Display, w: usize, h: usize, rgba: &[u8]) {
    let s = shared();
    let settings = lock(&s.settings).clone();
    let mut live = lock(&s.live).clone();
    live.mouse = mouse_pt();
    let full = frame_params(&d, &settings, &live);
    let p = scaled(&full, w as f32 / full.size.0.max(1) as f32);
    let out = render(rgba, w, h, &render_params(&p));
    use base64::Engine as _;
    let payload = serde_json::json!({ "w": w, "h": h, "data": base64::engine::general_purpose::STANDARD.encode(out) });
    if let Some(app) = lock(&s.app).as_ref() {
        let _ = app.emit("retro-preview-frame", payload);
    }
}
