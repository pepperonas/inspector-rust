//! Windows active-window + idle queries (**runtime-unverified** — compile-clean
//! against `windows` 0.61, like the rest of the repo's Windows code).

use super::FocusInfo;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, MAX_PATH};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
};

pub fn frontmost() -> Option<FocusInfo> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        // Front window title (best-effort).
        let len = GetWindowTextLengthW(hwnd);
        let title = if len > 0 {
            let mut buf = vec![0u16; len as usize + 1];
            let n = GetWindowTextW(hwnd, &mut buf);
            (n > 0).then(|| String::from_utf16_lossy(&buf[..n as usize]))
        } else {
            None
        };
        // Owning process → executable name.
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let app_name = process_name(pid).unwrap_or_else(|| "Unknown".to_string());
        Some(FocusInfo {
            app_name,
            app_id: None,
            window_title: title.filter(|s| !s.is_empty()),
        })
    }
}

fn process_name(pid: u32) -> Option<String> {
    if pid == 0 {
        return None;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; MAX_PATH as usize];
        let mut size = buf.len() as u32;
        let res = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        res.ok()?;
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        std::path::Path::new(&path)
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    }
}

pub fn idle_seconds() -> Option<f64> {
    unsafe {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut info).as_bool() {
            let now = GetTickCount();
            let idle_ms = now.wrapping_sub(info.dwTime);
            Some(idle_ms as f64 / 1000.0)
        } else {
            None
        }
    }
}
