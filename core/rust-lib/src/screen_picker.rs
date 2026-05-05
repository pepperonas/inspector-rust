//! System-wide screen color picker (eyedropper).
//!
//! Both platform implementations are *asynchronous from the caller's
//! perspective*: the IPC command in [`crate::commands::pick_screen_color`]
//! kicks them off and returns immediately, then emits a Tauri event
//! `"color-picked"` carrying either the hex string (e.g. `"#3366FF"`) or
//! `null` when the user cancelled / an error occurred.
//!
//! - **macOS**: `NSColorSampler` (AppKit, 10.15+). Apple's own system
//!   eyedropper — same one Pages, Keynote, and Sketch use. Shows a
//!   magnified loupe under the cursor. Must be invoked on the main
//!   thread; the selection handler block is invoked back on the main
//!   thread when the user clicks (or with `nil` on Escape).
//!
//! - **Windows**: a layered fullscreen overlay window plus `GetPixel`
//!   on the desktop DC. Spawns its own message loop on a dedicated
//!   thread, so the UI stays responsive. Press Escape to cancel.

#[cfg(target_os = "macos")]
pub use macos_impl::pick_color_async;

#[cfg(target_os = "windows")]
pub use windows_impl::pick_color_blocking;

// ── macOS ────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
mod macos_impl {
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    /// Show the system color sampler. `on_result` is invoked once, on the
    /// main thread, with `Some(hex)` on success or `None` if the user
    /// cancelled. Must be called on the main thread.
    pub fn pick_color_async<F>(on_result: F) -> Result<(), String>
    where
        F: Fn(Option<String>) + Send + 'static,
    {
        unsafe {
            // NSColorSampler doesn't render its loupe when the calling
            // app isn't a "regular" foreground app. ClipSnap normally
            // runs as Accessory (Dock-hidden tray app), so we have to
            // briefly promote it to Regular for the duration of the
            // pick. We restore Accessory in the selection handler.
            let app_cls = AnyClass::get(c"NSApplication")
                .ok_or_else(|| "NSApplication not available".to_string())?;
            let shared_app: *mut AnyObject = msg_send![app_cls, sharedApplication];
            // Activation policy enums: Regular = 0, Accessory = 1, Prohibited = 2.
            let _: bool = msg_send![shared_app, setActivationPolicy: 0i64];
            let _: () = msg_send![shared_app, activateIgnoringOtherApps: true];

            let cls = AnyClass::get(c"NSColorSampler")
                .ok_or_else(|| "NSColorSampler class not available (macOS 10.15+)".to_string())?;
            let sampler: *mut AnyObject = msg_send![cls, new];
            if sampler.is_null() {
                let _: bool = msg_send![shared_app, setActivationPolicy: 1i64];
                return Err("failed to allocate NSColorSampler".to_string());
            }

            // The block is `Block_copy`'d by NSColorSampler before it
            // returns from showSampler:; our RcBlock can drop after the
            // msg_send call without leaking.
            let app_cls_for_block = app_cls;
            let block = RcBlock::new(move |color: *mut AnyObject| {
                let hex = if color.is_null() {
                    None
                } else {
                    extract_hex_from_nscolor(color)
                };
                // Demote app back to Accessory so the Dock icon goes away.
                let shared_app: *mut AnyObject = msg_send![app_cls_for_block, sharedApplication];
                let _: bool = msg_send![shared_app, setActivationPolicy: 1i64];
                on_result(hex);
            });

            let _: () = msg_send![
                sampler,
                showSamplerWithSelectionHandler: &*block,
            ];
        }
        Ok(())
    }

    /// Convert an NSColor to a `#RRGGBB` hex string in sRGB space.
    /// Returns `None` if the color can't be converted (extremely rare —
    /// would mean a non-RGB pattern color).
    unsafe fn extract_hex_from_nscolor(color: *mut AnyObject) -> Option<String> {
        let srgb_cls = AnyClass::get(c"NSColorSpace")?;
        let srgb_space: *mut AnyObject = msg_send![srgb_cls, sRGBColorSpace];
        if srgb_space.is_null() {
            return None;
        }
        let converted: *mut AnyObject = msg_send![color, colorUsingColorSpace: srgb_space];
        if converted.is_null() {
            return None;
        }
        let r: f64 = msg_send![converted, redComponent];
        let g: f64 = msg_send![converted, greenComponent];
        let b: f64 = msg_send![converted, blueComponent];
        Some(format!(
            "#{:02X}{:02X}{:02X}",
            (r.clamp(0.0, 1.0) * 255.0).round() as u8,
            (g.clamp(0.0, 1.0) * 255.0).round() as u8,
            (b.clamp(0.0, 1.0) * 255.0).round() as u8,
        ))
    }
}

// ── Windows ─────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::sync::OnceLock;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
        GetMessageW, GetSystemMetrics, LoadCursorW, RegisterClassExW, SetLayeredWindowAttributes,
        ShowWindow, TranslateMessage, CW_USEDEFAULT, IDC_CROSS, LWA_ALPHA, MSG, SM_CXVIRTUALSCREEN,
        SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOW, WM_KEYDOWN,
        WM_LBUTTONDOWN, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOPMOST, WS_POPUP,
    };

    const VK_ESCAPE: u32 = 0x1B;

    /// Synchronous blocking pick. Spawn this on a worker thread —
    /// it runs its own message loop until the user clicks or hits Esc.
    pub fn pick_color_blocking() -> Result<String, String> {
        unsafe { run_picker() }
    }

    unsafe fn run_picker() -> Result<String, String> {
        let class_name = wide_str("ClipSnapEyeDropper\0");
        register_class_once(class_name.as_ptr());

        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(wide_str("ClipSnap Eyedropper\0").as_ptr()),
            WS_POPUP,
            x,
            y,
            w,
            h,
            None,
            None,
            None,
            None,
        )
        .map_err(|e| format!("CreateWindowExW failed: {e}"))?;

        // Near-zero alpha — visually unobtrusive but still hit-testable.
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 1, LWA_ALPHA);
        let _ = ShowWindow(hwnd, SW_SHOW);

        let mut result: Option<String> = None;
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_LBUTTONDOWN {
                let mut pt = windows::Win32::Foundation::POINT::default();
                let _ = GetCursorPos(&mut pt);
                let hdc = GetDC(None);
                let pixel = GetPixel(hdc, pt.x, pt.y);
                let _ = ReleaseDC(None, hdc);
                let r = (pixel.0 & 0xFF) as u8;
                let g = ((pixel.0 >> 8) & 0xFF) as u8;
                let b = ((pixel.0 >> 16) & 0xFF) as u8;
                result = Some(format!("#{:02X}{:02X}{:02X}", r, g, b));
                break;
            }
            if msg.message == WM_KEYDOWN && msg.wParam.0 as u32 == VK_ESCAPE {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = DestroyWindow(hwnd);
        result.ok_or_else(|| "user cancelled".to_string())
    }

    /// `RegisterClassExW` only needs to happen once per process; second
    /// call would fail with ERROR_CLASS_ALREADY_EXISTS, which is fine
    /// but noisy. Cache success in a `OnceLock`.
    fn register_class_once(name: *const u16) {
        static REGISTERED: OnceLock<()> = OnceLock::new();
        REGISTERED.get_or_init(|| unsafe {
            let cursor = LoadCursorW(None, IDC_CROSS).unwrap_or_default();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wnd_proc),
                hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(PCWSTR::null())
                    .unwrap_or_default()
                    .into(),
                hCursor: cursor,
                lpszClassName: PCWSTR(name),
                ..Default::default()
            };
            RegisterClassExW(&wc);
        });
    }

    extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }

    fn wide_str(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }
}
