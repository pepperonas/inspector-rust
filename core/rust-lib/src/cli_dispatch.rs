//! CLI actions for desktop environments where Tauri global shortcuts do not
//! receive key events (common on GNOME + Wayland). Bind these flags in
//! Settings → Keyboard → Custom Shortcuts, e.g. `inspector-rust --toggle-popup`.

use tauri::AppHandle;

use crate::{commands, hotkey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliAction {
    TogglePopup,
    Ocr,
    Screenshot,
    PickColor,
}

/// Parse `argv` (program name + flags). Returns the first recognized action.
pub fn parse_args<I, S>(args: I) -> Option<CliAction>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    for arg in args.into_iter().skip(1) {
        match arg.as_ref() {
            "--toggle-popup" | "--open" | "-o" => return Some(CliAction::TogglePopup),
            "--ocr" => return Some(CliAction::Ocr),
            "--screenshot" | "--shot" => return Some(CliAction::Screenshot),
            "--pick-color" | "--color" => return Some(CliAction::PickColor),
            "--help" | "-h" => {
                print_help();
                return None;
            }
            other if other.starts_with('-') => {
                tracing::warn!("unknown CLI flag: {other} (try --help)");
            }
            _ => {}
        }
    }
    None
}

/// If `--help` was passed, print usage and return true (caller should exit).
pub fn exit_if_help_requested() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return true;
    }
    false
}

pub fn print_help() {
    eprintln!(
        "Inspector Rust CLI actions (for GNOME/KDE custom shortcuts under Wayland):\n\
         \n\
           --toggle-popup   Open/close clipboard popup (Ctrl+Shift+V)\n\
           --ocr            OCR a screen region (Ctrl+Shift+O)\n\
           --screenshot     Capture region to clipboard (Ctrl+Shift+S)\n\
           --pick-color     Pick pixel color to clipboard (Ctrl+Shift+C)\n\
         \n\
         On GNOME/Cinnamon + Wayland, shortcuts are usually installed automatically.\n\
         Manual re-apply: restart the app after clearing setting `linux.desktop_shortcuts_profile`.\n"
    );
}

/// Run an action on the main app handle (same behavior as the tray menu).
pub fn dispatch(app: &AppHandle, action: CliAction) {
    match action {
        CliAction::TogglePopup => {
            if let Err(e) = hotkey::toggle_popup(app) {
                tracing::warn!("--toggle-popup: {e:#}");
            }
        }
        CliAction::Ocr => {
            let app = app.clone();
            std::thread::spawn(move || match commands::run_ocr_pipeline(&app) {
                Ok(r) if !r.cancelled && r.chars > 0 => {
                    tracing::info!("--ocr: {} chars", r.chars);
                }
                Ok(_) => tracing::debug!("--ocr: cancelled or empty"),
                Err(e) => tracing::warn!("--ocr failed: {e}"),
            });
        }
        CliAction::Screenshot => {
            let app = app.clone();
            std::thread::spawn(move || match commands::run_screenshot_pipeline(&app) {
                Ok(r) if !r.cancelled && r.bytes > 0 => {
                    tracing::info!("--screenshot: {} bytes", r.bytes);
                }
                Ok(_) => tracing::debug!("--screenshot: cancelled or empty"),
                Err(e) => tracing::warn!("--screenshot failed: {e}"),
            });
        }
        CliAction::PickColor => {
            let app = app.clone();
            std::thread::spawn(move || commands::run_eyedropper_pipeline(&app));
        }
    }
}

#[cfg(target_os = "linux")]
pub fn log_wayland_shortcut_hint() {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return;
    }
    tracing::info!(
        "Wayland: if Ctrl+Shift+V/O/S/C do not work, GNOME/Cinnamon shortcuts are registered \
         automatically on first start (see Settings → Keyboard → Custom Shortcuts)."
    );
}
