//! CLI actions for desktop environments where Tauri global shortcuts do not
//! receive key events (common on GNOME + Wayland). Bind these flags in
//! Settings → Keyboard → Custom Shortcuts, e.g. `inspector-rust --toggle-popup`.

use tauri::{AppHandle, Manager};

use crate::{commands, db::DbHandle, hotkey};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliAction {
    TogglePopup,
    Ocr,
    Screenshot,
    PickColor,
    /// Re-scan gsettings conflicts and reinstall desktop shortcuts (Linux).
    SetupShortcuts,
    /// Text expander: capture word before cursor and paste snippet (Linux Wayland).
    ExpandAtCursor,
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
            #[cfg(target_os = "linux")]
            "--setup-shortcuts" => return Some(CliAction::SetupShortcuts),
            "--expand-at-cursor" | "--expand" => return Some(CliAction::ExpandAtCursor),
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
           --setup-shortcuts  (Linux) Re-scan shortcut conflicts and reinstall bindings\n\
           --expand-at-cursor (Linux) Run text expander once (if global hotkey does not fire)\n\
         \n\
         On GNOME/Cinnamon + Wayland, shortcuts are installed automatically on first start\n\
         (conflict scan moves Terminal to Ctrl+C/V when needed; fallbacks if keys are taken).\n"
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
                Ok(r) if r.cancelled => tracing::info!("--ocr: cancelled by user"),
                Ok(_) => tracing::warn!("--ocr: empty result (no text in region?)"),
                Err(e) => tracing::warn!("--ocr failed: {e}"),
            });
        }
        CliAction::Screenshot => {
            let app = app.clone();
            std::thread::spawn(move || match commands::run_screenshot_pipeline(&app) {
                Ok(r) if !r.cancelled && r.bytes > 0 => {
                    tracing::info!("--screenshot: {} bytes", r.bytes);
                }
                Ok(r) if r.cancelled => tracing::info!("--screenshot: cancelled by user"),
                Ok(_) => tracing::warn!("--screenshot: empty capture"),
                Err(e) => tracing::warn!("--screenshot failed: {e}"),
            });
        }
        CliAction::PickColor => {
            let app = app.clone();
            std::thread::spawn(move || commands::run_eyedropper_pipeline(&app));
        }
        #[cfg(target_os = "linux")]
        CliAction::SetupShortcuts => {
            if let Some(db) = app.try_state::<DbHandle>() {
                match crate::desktop_shortcuts::force_reinstall(&db) {
                    Ok(()) => tracing::info!("--setup-shortcuts: desktop shortcuts reinstalled"),
                    Err(e) => tracing::warn!("--setup-shortcuts failed: {e:#}"),
                }
            } else {
                tracing::warn!("--setup-shortcuts: database not ready yet");
            }
        }
        #[cfg(not(target_os = "linux"))]
        CliAction::SetupShortcuts => {
            tracing::warn!("--setup-shortcuts is only available on Linux");
        }
        CliAction::ExpandAtCursor => {
            hotkey::hide_popup(app);
            let app2 = app.clone();
            let _ = app.run_on_main_thread(move || {
                std::thread::sleep(std::time::Duration::from_millis(250));
                if let Some(db) = app2.try_state::<DbHandle>() {
                    match crate::expander::expand_at_cursor(&db) {
                        Ok(()) => tracing::info!("--expand-at-cursor: expansion completed"),
                        Err(e) => tracing::warn!("--expand-at-cursor failed: {e:#}"),
                    }
                }
            });
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `parse_args` always skips the first arg (the program name); the
    /// rest is what the tests care about. The helper builds an argv with
    /// a dummy `inspector-rust` head so the call sites read naturally.
    fn parse(rest: &[&str]) -> Option<CliAction> {
        let mut argv = vec!["inspector-rust"];
        argv.extend_from_slice(rest);
        parse_args(argv)
    }

    #[test]
    fn empty_or_program_only_returns_none() {
        assert_eq!(parse_args::<_, &str>(std::iter::empty()), None);
        assert_eq!(parse(&[]), None);
    }

    #[test]
    fn toggle_popup_has_three_aliases() {
        assert_eq!(parse(&["--toggle-popup"]), Some(CliAction::TogglePopup));
        assert_eq!(parse(&["--open"]), Some(CliAction::TogglePopup));
        assert_eq!(parse(&["-o"]), Some(CliAction::TogglePopup));
    }

    #[test]
    fn ocr_flag() {
        assert_eq!(parse(&["--ocr"]), Some(CliAction::Ocr));
    }

    #[test]
    fn screenshot_has_two_aliases() {
        assert_eq!(parse(&["--screenshot"]), Some(CliAction::Screenshot));
        assert_eq!(parse(&["--shot"]), Some(CliAction::Screenshot));
    }

    #[test]
    fn pick_color_has_two_aliases() {
        assert_eq!(parse(&["--pick-color"]), Some(CliAction::PickColor));
        assert_eq!(parse(&["--color"]), Some(CliAction::PickColor));
    }

    #[test]
    fn help_flags_return_none() {
        // Help is handled separately by `exit_if_help_requested`; the
        // parser returns None so no action fires when `--help` is given.
        assert_eq!(parse(&["--help"]), None);
        assert_eq!(parse(&["-h"]), None);
    }

    #[test]
    fn unknown_flag_is_ignored() {
        // Logs a warning, doesn't dispatch — and doesn't crash the app.
        assert_eq!(parse(&["--definitely-not-a-flag"]), None);
        assert_eq!(parse(&["-x"]), None);
    }

    #[test]
    fn first_recognized_action_wins() {
        // Two actions on one command line — the leftmost wins.
        // Documented behaviour so callers can chain flags predictably.
        assert_eq!(parse(&["--ocr", "--screenshot"]), Some(CliAction::Ocr));
        assert_eq!(
            parse(&["--screenshot", "--ocr"]),
            Some(CliAction::Screenshot),
        );
    }

    #[test]
    fn unknown_flags_before_a_known_one_are_skipped() {
        assert_eq!(parse(&["--bogus", "--ocr"]), Some(CliAction::Ocr));
    }

    #[test]
    fn non_flag_positionals_are_silently_ignored() {
        // Stray positional words (e.g. a path the user accidentally
        // dragged onto the binary) don't match a flag and don't crash.
        assert_eq!(parse(&["some-positional-token"]), None);
        assert_eq!(parse(&["foo", "--ocr"]), Some(CliAction::Ocr));
    }

    #[test]
    fn does_not_match_short_substring_of_a_known_flag() {
        // Guard against accidental prefix matching: `--ocrx` is NOT
        // `--ocr`; `--toggle-popup-foo` is NOT `--toggle-popup`.
        assert_eq!(parse(&["--ocrx"]), None);
        assert_eq!(parse(&["--toggle-popup-foo"]), None);
    }
}
