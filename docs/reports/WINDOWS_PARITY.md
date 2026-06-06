# Windows Parity — feature matrix & status

> Phase C of the feature plan. Audits every platform-divergent feature, the
> Windows path taken (or recommended), and current status.
> Generated: 2026-06-06 · base version 0.61.0.
>
> **Verification caveat:** the build host is macOS. All Windows code in this
> repo is written compile-clean behind `#[cfg(target_os = "windows")]` but is
> **runtime-unverified** until exercised on real Windows hardware. Items
> marked ✅ compile and follow documented Win32/CLI mechanisms; they still
> need a smoke test on a Windows box.

## Matrix

| Feature | macOS | Windows | Linux | Windows mechanism | Status |
|---|---|---|---|---|---|
| Clipboard history / capture | ✅ | ✅ | ✅ | clipboard-rs + plugin | shipping |
| Popup + global hotkeys | ✅ | ✅ | ⚠️ Wayland CLI | tauri-plugin-global-shortcut | shipping |
| Text expander (search / hotkey / direct-slot) | ✅ | ✅ | ⚠️ clipboard-only | UIA + enigo | shipping |
| **Auto-expansion (passive, aText)** | ✅ CGEventTap | ✅ `WH_KEYBOARD_LL` | ❌ no rootless tap | low-level keyboard hook | **v0.56.0, runtime-unverified on Win** |
| OCR region | ✅ Vision | ✅ WinRT OCR | ✅ tesseract | `Windows.Media.Ocr` | shipping |
| Screenshot region | ✅ screencapture | ✅ GDI overlay | ✅ grim/scrot | GDI marquee | shipping |
| **Screenshot fullscreen / window** | ✅ screencapture -x/-w | ✅ GDI blit / fg-window | ⚠️ full only | `extract_png` reuse | **v0.57.0, runtime-unverified on Win** |
| Screenshot self-timer / repeat-last | ✅ | ✅ | ✅ | platform-neutral | v0.57.0 |
| Screenshot editor (annotate) | ✅ | ✅ | ✅ | canvas (frontend) | v0.58.0 (line/ellipse/redact/step) |
| Screenshot pin-to-screen | ✅ | ✅ | ✅ | Tauri window | v0.58.0/0.59.0 |
| Eyedropper | ✅ NSColorSampler | ✅ GDI overlay | ❌ | GDI | shipping |
| Markdown → PDF | ✅ WKWebView | ✅ Edge headless | ❌ | `msedge --headless --print-to-pdf` | v0.55.0, runtime-unverified on Win |
| **System: reboot / shutdown / lock** | ✅ osascript/pmset | ✅ `shutdown /r\|/s`, `rundll32 LockWorkStation` | ❌ | CLI shell-out | **v0.61.0, runtime-unverified on Win** |
| **System: mute / volume** | ✅ osascript | ✅ `VK_VOLUME_*` via `keybd_event` | ❌ | multimedia keys | **v0.61.0, runtime-unverified on Win** |
| Process kill picker | ✅ libc kill | ✅ TerminateProcess | ❌ | sysinfo + Win32 | shipping |
| Input lock (`freeze`) | ✅ CGEventTap | ❌ | ❌ | `WH_KEYBOARD_LL`+`WH_MOUSE_LL` | **TODO (Win)** |
| Cleaning (`clean`) | ✅ | ✅ | ✅ | dirs allowlist | v0.60.0 |
| Wakelock / caffeine | ✅ caffeinate | ✅ SetThreadExecutionState + F15 | ⚠️ X11 jiggle | v0.50.2 | shipping |
| Timer / alarm + status toast | ✅ | ✅ | ✅ | platform-neutral | shipping |
| App launcher | ✅ /Applications walk | ❌ | ❌ | Start-Menu `.lnk` index + ShellExecute | **TODO (Win)** |
| Finder/Explorer selection + touch/mkdir/terminal | ✅ osascript | ❌ | ❌ | Shell COM (`IShellWindows`/`IFolderView`) | **TODO (Win)** |
| Image recolor / cutout (ONNX) | ✅ | ✅ | ✅ | ort static | shipping |
| TOTP / 2FA | ✅ | ✅ | ✅ | platform-neutral | shipping |

## Remaining Windows gaps (prioritised)

1. **App launcher (Windows)** — index `*.lnk` under
   `%ProgramData%\Microsoft\Windows\Start Menu` and
   `%AppData%\Microsoft\Windows\Start Menu`; launch via `ShellExecuteW`.
   Keep the existing IPC (`list_apps`/`launch_app`/`refresh_apps`) — only the
   `#[cfg(windows)]` body changes; the frontend is untouched. **Effort: M.**
2. **Explorer selection + file ops** — read the active Explorer window's
   selection via Shell COM (`IShellWindows` → `IShellFolderViewDual` →
   `SelectedItems`), or fall back to the active Explorer path. `touch`/`mkdir`
   target that folder; `terminal` opens Windows Terminal/`cmd` there. Keep the
   `get_finder_selection`/`finder_touch`/`finder_mkdir`/`finder_open_terminal`
   IPC names (platform `cfg`). **Effort: L** (COM-heavy).
3. **Input lock (Windows)** — `SetWindowsHookEx(WH_KEYBOARD_LL + WH_MOUSE_LL)`
   on a dedicated message-loop thread that swallows events until the unlock
   chord (the auto-expand hook in `auto_expand.rs` is a working reference for
   the keyboard half). **Effort: M.**

## Done in this phase (v0.61.0)

- System reboot / shutdown / lock on Windows (CLI shell-outs).
- Mute + volume on Windows (multimedia VK keys via `keybd_event`).

All other previously-divergent features already had Windows paths landed in
earlier versions (see the matrix). The three TODO items above are tracked for
follow-up; each keeps the existing IPC contract so no frontend change is
needed when implemented.
