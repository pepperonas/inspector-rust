//! Interactive screen-region picker for the OCR feature.
//!
//! macOS uses Apple's own `screencapture(1)` with `-i -t png` — Cmd+Shift+4
//! is *literally* this binary, so the UX is the polished one users
//! already know: drag a marquee, Esc cancels, hold Space to drag the
//! whole rect, etc. Way more reliable than reinventing an overlay
//! window in `objc2`. The captured PNG is written to a temp file we
//! then read back into memory and delete.
//!
//! Windows is stubbed for now — implementation will use either the
//! `ms-screenclip:` URI handler or a direct GDI overlay later.

// Context is used by the macOS implementation only; Linux / Windows
// stubs don't need it. Per-platform import keeps clippy happy on all
// targets without sprinkling allow attributes.
#[cfg(target_os = "macos")]
use anyhow::Context;
use anyhow::Result;

/// User pressed Esc / clicked away — distinct error so the IPC layer
/// can return success-with-no-text instead of bubbling up a real error.
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "region capture cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// Show the interactive region picker, return the captured PNG bytes.
/// Blocks until the user finishes drawing the rect or cancels (Esc).
pub fn capture() -> Result<Vec<u8>> {
    capture_impl()
}

#[cfg(target_os = "macos")]
fn capture_impl() -> Result<Vec<u8>> {
    use chrono::Utc;
    use std::process::Command;

    let tmp = std::env::temp_dir().join(format!(
        "inspector-rust-ocr-{}.png",
        Utc::now().timestamp_millis()
    ));

    // -i = interactive selection (drag rectangle)
    // -t png = output format (default is also PNG, but be explicit)
    // -x = silent (no shutter sound)
    // -o = no shadow/window chrome capture (irrelevant for region but harmless)
    // We do NOT pass `-c` (clipboard) because we want the file to read
    // back; -c would leave us guessing at the clipboard format.
    let status = Command::new("/usr/sbin/screencapture")
        .args(["-i", "-x", "-t", "png"])
        .arg(&tmp)
        .status()
        .context("spawn /usr/sbin/screencapture")?;

    if !status.success() {
        // Clean up if anything was written despite non-zero exit.
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("screencapture exited with status {:?}", status.code());
    }

    // screencapture exits 0 even on cancel — the only signal is "did
    // the file get written?". A zero-byte file is also considered a
    // cancel (some macOS versions create the file then write nothing).
    if !tmp.exists() {
        return Err(Cancelled.into());
    }
    let bytes = std::fs::read(&tmp).context("read captured png")?;
    let _ = std::fs::remove_file(&tmp);
    if bytes.is_empty() {
        return Err(Cancelled.into());
    }
    Ok(bytes)
}

#[cfg(target_os = "windows")]
fn capture_impl() -> Result<Vec<u8>> {
    unsafe { win_impl::capture() }
}

/// Windows GDI fullscreen overlay region picker.
///
/// Flow:
///   1. Capture the entire virtual screen into a memory DC (freeze-frame).
///   2. Show a fullscreen WS_POPUP window that paints the freeze-frame.
///   3. User drags a rectangle; DrawFocusRect shows the selection.
///   4. On mouse-up: extract the selected region as a PNG and return it.
///   5. On Esc: return `Cancelled`.
#[cfg(target_os = "windows")]
mod win_impl {
    use std::cell::RefCell;
    use std::sync::OnceLock;

    use anyhow::Result;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen, DeleteDC,
        DeleteObject, EndPaint, GetDC, GetDIBits, GetStockObject, InvalidateRect, ReleaseDC,
        SelectObject, SetROP2, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HDC, HGDIOBJ,
        NULL_BRUSH, PAINTSTRUCT, PS_SOLID, R2_NOT, RGBQUAD, SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
        GetSystemMetrics, LoadCursorW, PostQuitMessage, RegisterClassExW, SetForegroundWindow,
        ShowWindow, TranslateMessage, IDC_CROSS, MSG, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
        SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOW, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
        WM_MOUSEMOVE, WM_PAINT, WNDCLASSEXW, WS_EX_TOPMOST, WS_POPUP,
    };

    const VK_ESCAPE: usize = 0x1B;

    struct State {
        mem_dc: HDC,
        vw: i32,
        vh: i32,
        start: Option<(i32, i32)>,
        cur: Option<(i32, i32)>,
        result: Option<(i32, i32, i32, i32)>, // x, y, w, h in bitmap coords
        cancelled: bool,
    }

    thread_local! {
        static S: RefCell<Option<State>> = const { RefCell::new(None) };
    }

    pub unsafe fn capture() -> Result<Vec<u8>> {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);

        // Grab the full virtual screen into an off-screen DC before any
        // overlay appears, so the freeze-frame shows real screen content.
        let desk_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(desk_dc));
        let bmp = CreateCompatibleBitmap(desk_dc, vw, vh);
        let old = SelectObject(mem_dc, HGDIOBJ(bmp.0));
        let _ = BitBlt(mem_dc, 0, 0, vw, vh, Some(desk_dc), vx, vy, SRCCOPY);
        ReleaseDC(None, desk_dc);

        S.with(|s| {
            *s.borrow_mut() = Some(State {
                mem_dc,
                vw,
                vh,
                start: None,
                cur: None,
                result: None,
                cancelled: false,
            });
        });

        let class: Vec<u16> = "InspectorRustRegionPicker\0".encode_utf16().collect();
        register_once(&class);

        let title: Vec<u16> = "Select Region\0".encode_utf16().collect();
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST,
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_POPUP,
            vx,
            vy,
            vw,
            vh,
            None,
            None,
            None,
            None,
        )
        .map_err(|e| anyhow::anyhow!("CreateWindowExW: {e}"))?;

        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = DestroyWindow(hwnd);

        let (result, cancelled) = S.with(|s| {
            let b = s.borrow();
            let st = b.as_ref().unwrap();
            (st.result, st.cancelled)
        });

        // Extract PNG while mem_dc (and its bitmap) are still live.
        let png = if cancelled {
            Err(crate::region_picker::Cancelled.into())
        } else {
            match result {
                Some((x, y, w, h)) if w > 0 && h > 0 => extract_png(mem_dc, x, y, w, h),
                _ => Err(crate::region_picker::Cancelled.into()),
            }
        };

        SelectObject(mem_dc, old);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        DeleteDC(mem_dc);
        S.with(|s| *s.borrow_mut() = None);

        png
    }

    /// Copy a sub-region out of `src` DC and encode it as PNG bytes.
    unsafe fn extract_png(src: HDC, x: i32, y: i32, w: i32, h: i32) -> Result<Vec<u8>> {
        let desk_dc = GetDC(None);
        let tmp_dc = CreateCompatibleDC(Some(desk_dc));
        let bmp = CreateCompatibleBitmap(desk_dc, w, h);
        ReleaseDC(None, desk_dc);
        let old = SelectObject(tmp_dc, HGDIOBJ(bmp.0));
        let _ = BitBlt(tmp_dc, 0, 0, w, h, Some(src), x, y, SRCCOPY);

        // DWORD-aligned row stride for 24-bit DIB.
        let stride = (w as usize * 3 + 3) & !3usize;
        let mut raw = vec![0u8; stride * h as usize];

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // negative = top-down scan order
                biPlanes: 1,
                biBitCount: 24,
                ..Default::default()
            },
            bmiColors: [RGBQUAD::default()],
        };
        GetDIBits(
            tmp_dc,
            bmp,
            0,
            h as u32,
            Some(raw.as_mut_ptr().cast()),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        SelectObject(tmp_dc, old);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        DeleteDC(tmp_dc);

        // GDI 24-bit DIB layout is BGR; image crate expects RGB.
        let mut img = image::RgbImage::new(w as u32, h as u32);
        for row in 0..h as usize {
            for col in 0..w as usize {
                let off = row * stride + col * 3;
                img.put_pixel(
                    col as u32,
                    row as u32,
                    image::Rgb([raw[off + 2], raw[off + 1], raw[off]]),
                );
            }
        }

        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .map_err(|e| anyhow::anyhow!("PNG encode: {e}"))?;
        Ok(out)
    }

    fn register_once(class_name: &[u16]) {
        static DONE: OnceLock<()> = OnceLock::new();
        DONE.get_or_init(|| unsafe {
            let cursor = LoadCursorW(None, IDC_CROSS).unwrap_or_default();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wnd_proc),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                hInstance: windows::Win32::System::LibraryLoader::GetModuleHandleW(PCWSTR::null())
                    .unwrap_or_default()
                    .into(),
                hCursor: cursor,
                ..Default::default()
            };
            RegisterClassExW(&wc);
        });
    }

    /// Extract mouse client-coordinates from a WM_MOUSEMOVE / WM_LBUTTON* LPARAM.
    /// Uses signed 16-bit halves so negative coords (multi-monitor) are handled.
    #[inline]
    fn mouse_xy(lp: LPARAM) -> (i32, i32) {
        let x = (lp.0 as u32 & 0xFFFF) as i16 as i32;
        let y = ((lp.0 as u32 >> 16) & 0xFFFF) as i16 as i32;
        (x, y)
    }

    extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe {
            match msg {
                WM_PAINT => {
                    let mut ps = PAINTSTRUCT::default();
                    let hdc = BeginPaint(hwnd, &mut ps);
                    S.with(|s| {
                        if let Some(ref st) = *s.borrow() {
                            // Paint the freeze-frame screenshot.
                            let _ = BitBlt(hdc, 0, 0, st.vw, st.vh, Some(st.mem_dc), 0, 0, SRCCOPY);
                            // Draw selection rectangle while dragging.
                            if let (Some((x1, y1)), Some((x2, y2))) = (st.start, st.cur) {
                                let pen = CreatePen(
                                    PS_SOLID,
                                    2,
                                    windows::Win32::Foundation::COLORREF(0xFFFFFFu32),
                                );
                                let null_brush = GetStockObject(NULL_BRUSH);
                                let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
                                let old_brush = SelectObject(hdc, HGDIOBJ(null_brush.0));
                                SetROP2(hdc, R2_NOT);
                                windows::Win32::Graphics::Gdi::Rectangle(
                                    hdc,
                                    x1.min(x2),
                                    y1.min(y2),
                                    x1.max(x2),
                                    y1.max(y2),
                                );
                                SelectObject(hdc, old_pen);
                                SelectObject(hdc, old_brush);
                                let _ = DeleteObject(HGDIOBJ(pen.0));
                            }
                        }
                    });
                    EndPaint(hwnd, &ps);
                    LRESULT(0)
                }
                WM_LBUTTONDOWN => {
                    let (x, y) = mouse_xy(lp);
                    S.with(|s| {
                        if let Some(ref mut st) = *s.borrow_mut() {
                            st.start = Some((x, y));
                            st.cur = Some((x, y));
                        }
                    });
                    LRESULT(0)
                }
                WM_MOUSEMOVE => {
                    let (x, y) = mouse_xy(lp);
                    S.with(|s| {
                        if let Some(ref mut st) = *s.borrow_mut() {
                            if st.start.is_some() {
                                st.cur = Some((x, y));
                            }
                        }
                    });
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    LRESULT(0)
                }
                WM_LBUTTONUP => {
                    let (x, y) = mouse_xy(lp);
                    S.with(|s| {
                        if let Some(ref mut st) = *s.borrow_mut() {
                            if let Some((x1, y1)) = st.start {
                                let lx = x1.min(x).max(0);
                                let ly = y1.min(y).max(0);
                                let rw = (x1 - x).abs().min(st.vw - lx);
                                let rh = (y1 - y).abs().min(st.vh - ly);
                                st.result = Some((lx, ly, rw, rh));
                            }
                        }
                    });
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                WM_KEYDOWN if wp.0 == VK_ESCAPE => {
                    S.with(|s| {
                        if let Some(ref mut st) = *s.borrow_mut() {
                            st.cancelled = true;
                        }
                    });
                    PostQuitMessage(0);
                    LRESULT(0)
                }
                _ => DefWindowProcW(hwnd, msg, wp, lp),
            }
        }
    }
}

/// Linux: xdg-desktop-portal on GNOME/Cinnamon Wayland, else grim+slurp
/// (wlroots), else X11 scrot.
#[cfg(target_os = "linux")]
fn capture_impl() -> Result<Vec<u8>> {
    use anyhow::Context;
    use chrono::Utc;
    use std::process::Command;

    if crate::linux_portal::prefer_portal_capture() {
        match crate::linux_portal::capture_region() {
            Ok(bytes) => return Ok(bytes),
            Err(e) if crate::linux_portal::is_portal_cancelled(&e) => {
                return Err(Cancelled.into());
            }
            Err(e) => {
                tracing::warn!("portal region capture failed ({e:#}); trying grim+slurp fallback");
            }
        }
    }

    let tmp = std::env::temp_dir().join(format!(
        "inspector-rust-region-{}.png",
        Utc::now().timestamp_millis()
    ));

    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        && which_exists("grim")
        && which_exists("slurp");

    let status = if wayland {
        let region = Command::new("slurp")
            .output()
            .context("spawn slurp (Wayland region picker)")?;
        if !region.status.success() {
            let _ = std::fs::remove_file(&tmp);
            return Err(Cancelled.into());
        }
        let geom = String::from_utf8_lossy(&region.stdout).trim().to_string();
        if geom.is_empty() {
            return Err(Cancelled.into());
        }
        Command::new("grim")
            .args(["-g", &geom])
            .arg(&tmp)
            .status()
            .context("spawn grim")?
    } else if which_exists("scrot") {
        Command::new("scrot")
            .args(["-s", "-o"])
            .arg(&tmp)
            .status()
            .context("spawn scrot")?
    } else {
        anyhow::bail!(
            "region capture needs xdg-desktop-portal (GNOME Wayland), scrot (X11), \
             or grim+slurp (wlroots Wayland). \
             Install: sudo apt install scrot   # X11\n\
             or: sudo apt install grim slurp   # Sway/Hyprland"
        );
    };

    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(Cancelled.into());
    }
    if !tmp.exists() {
        return Err(Cancelled.into());
    }
    let bytes = std::fs::read(&tmp).context("read captured png")?;
    let _ = std::fs::remove_file(&tmp);
    if bytes.is_empty() {
        return Err(Cancelled.into());
    }
    Ok(bytes)
}

#[cfg(target_os = "linux")]
fn which_exists(cmd: &str) -> bool {
    use std::process::Command;
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// Other Unix targets (not Linux / macOS / Windows).
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn capture_impl() -> Result<Vec<u8>> {
    anyhow::bail!("region capture is not implemented on this platform")
}
