//! Windows UI Automation (UIA) implementation of `FieldAccess`.
//!
//! Reads the focused element's text and selection via the COM API
//! `IUIAutomation` (UIAutomationCore.dll). For the *replace* half of the
//! trait we deliberately go back to keystroke synthesis (Backspace×N +
//! type-body via `enigo`), because:
//!
//! - `IUIAutomationTextEditPattern2::Replace` is only implemented by a
//!   small set of controls (modern UIAuto-aware Edit / RichEdit). Most
//!   real-world apps don't expose it.
//! - `IUIAutomationValuePattern::SetValue` replaces the *entire* control
//!   value, not just the word range.
//! - Keystroke synthesis on Windows uses `SendInput` (via `enigo`),
//!   which has none of the macOS TSM main-thread restrictions and works
//!   reliably across every app that accepts keyboard input.
//!
//! So we use UIA for the *read* (the unreliable part on macOS without
//! AX) and fall back to `enigo` for the deterministic *write*.
//!
//! No additional permission required: UIA is part of the standard
//! Windows accessibility infrastructure and any user-process can
//! consume it.

use anyhow::{anyhow, Context, Result};
use enigo::{
    Direction::{Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use windows::core::Interface;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, TextUnit_Word, UIA_TextPatternId,
};

use super::{trim_word, FieldAccess, ReplaceOutcome};

pub struct UiaFieldAccess;

impl UiaFieldAccess {
    /// One-shot UIA query: returns the word immediately before the cursor,
    /// or `None` when the focused element doesn't expose a TextPattern at
    /// all. Each call sets up its own COM apartment because there's no
    /// guarantee the IPC thread that calls us has one already.
    fn read_word(&self) -> Result<Option<String>> {
        unsafe {
            // CoInitializeEx is reference-counted; calling it multiple
            // times is fine, and we don't bother to CoUninitialize on
            // success — Tauri's IPC threads stay alive for the app's
            // lifetime.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                    .context("CoCreateInstance(CUIAutomation)")?;

            let focused = match automation.GetFocusedElement() {
                Ok(e) => e,
                Err(_) => return Ok(None),
            };

            // TextPattern: read selection / move text-range cursors.
            let pattern_unknown = match focused.GetCurrentPattern(UIA_TextPatternId) {
                Ok(p) => p,
                Err(_) => return Ok(None),
            };
            let text_pattern: IUIAutomationTextPattern = match pattern_unknown.cast() {
                Ok(p) => p,
                Err(_) => return Ok(None),
            };

            // Get the current selection. For a bare cursor (no real
            // selection) this is a zero-length range positioned at the
            // caret.
            let selection = match text_pattern.GetSelection() {
                Ok(s) => s,
                Err(_) => return Ok(None),
            };
            let count = selection.Length().unwrap_or(0);
            if count == 0 {
                return Ok(None);
            }
            let range = match selection.GetElement(0) {
                Ok(r) => r,
                Err(_) => return Ok(None),
            };

            // Clone, then move the start of the cloned range backwards
            // by exactly one word. The end stays at the cursor. GetText
            // on that range gives us the previous word.
            let word_range = match range.Clone() {
                Ok(r) => r,
                Err(_) => return Ok(None),
            };
            let moved = word_range
                .MoveEndpointByUnit(
                    windows::Win32::UI::Accessibility::TextPatternRangeEndpoint_Start,
                    TextUnit_Word,
                    -1,
                )
                .unwrap_or(0);
            if moved == 0 {
                return Ok(None);
            }

            let bstr = match word_range.GetText(-1) {
                Ok(s) => s,
                Err(_) => return Ok(None),
            };
            let raw = bstr.to_string();
            let trimmed = trim_word(&raw);
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
    }
}

impl FieldAccess for UiaFieldAccess {
    fn is_focused_field_secure(&self) -> Result<bool> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                    .context("CoCreateInstance(CUIAutomation) for password probe")?;
            let focused = match automation.GetFocusedElement() {
                Ok(e) => e,
                Err(_) => return Ok(false),
            };
            // `CurrentIsPassword` is the standard UIA way to ask
            // "would Windows show dots instead of characters here?".
            // Modern WinUI / WPF / WinForms password fields all set
            // this; legacy Win32 EDIT with ES_PASSWORD style does
            // too. Returns BOOL → 0 / nonzero.
            let is_pw = focused.CurrentIsPassword().unwrap_or_default();
            Ok(is_pw.as_bool())
        }
    }

    fn read_word_before_cursor(&self) -> Result<Option<String>> {
        self.read_word()
    }

    /// Windows replace: **Backspace × char_count(word) + clipboard-paste**.
    /// We deliberately do NOT use `enigo.text(replacement)` — that
    /// translates each char into a key-event sequence, which (a) is
    /// slow for long bodies, (b) breaks dead-key layouts
    /// (US-International ", e → "e instead of ë), (c) doesn't work
    /// when an IME is active for CJK input, and (d) drops
    /// supplementary-plane Unicode (emoji, etc.). The clipboard-paste
    /// path matches the macOS in-place-fallback path and handles all
    /// of these correctly.
    ///
    /// Returns `Unsupported` only when the focused element exposes no
    /// UIA TextPattern at all (caller does the keystroke-select
    /// fallback); otherwise `Replaced`.
    fn try_replace_word_before_cursor(&self, replacement: &str) -> Result<ReplaceOutcome> {
        use clipboard_rs::{Clipboard, ClipboardContext};

        let word = match self.read_word()? {
            Some(w) => w,
            None => return Ok(ReplaceOutcome::Unsupported),
        };
        let backspaces = word.chars().count();

        let settings = Settings {
            open_prompt_to_get_permissions: false,
            ..Settings::default()
        };
        let mut e = Enigo::new(&settings)
            .map_err(|err| anyhow!("enigo init failed: {err:?}"))?;

        // Delete the abbreviation with a 4 ms pace gap — matches the
        // expander.rs::send_backspaces fix; some apps drop tightly
        // packed Backspaces.
        for i in 0..backspaces {
            e.key(Key::Backspace, Press)
                .map_err(|err| anyhow!("backspace press: {err:?}"))?;
            e.key(Key::Backspace, Release)
                .map_err(|err| anyhow!("backspace release: {err:?}"))?;
            if i + 1 < backspaces {
                std::thread::sleep(std::time::Duration::from_millis(4));
            }
        }

        // Save → write body → paste → restore. Caller has already
        // hidden the popup and dispatched the new word; this code
        // runs in the target app's keyboard-focus context.
        let saved = ClipboardContext::new()
            .ok()
            .and_then(|c| c.get_text().ok());
        {
            let ctx = ClipboardContext::new()
                .map_err(|err| anyhow!("clipboard ctx: {err:?}"))?;
            ctx.set_text(replacement.to_string())
                .map_err(|err| anyhow!("set_text: {err:?}"))?;
        }
        // Let the pasteboard write settle (Win32 + Catalyst-style apps
        // can briefly lag observing OpenClipboard events).
        std::thread::sleep(std::time::Duration::from_millis(30));

        // Synthesize Ctrl+V.
        e.key(Key::Control, Press)
            .map_err(|err| anyhow!("ctrl press: {err:?}"))?;
        e.key(Key::Unicode('v'), Press)
            .map_err(|err| anyhow!("v press: {err:?}"))?;
        e.key(Key::Unicode('v'), Release)
            .map_err(|err| anyhow!("v release: {err:?}"))?;
        e.key(Key::Control, Release)
            .map_err(|err| anyhow!("ctrl release: {err:?}"))?;

        // Restore the user's clipboard after the paste has consumed
        // our body. 180 ms matches the macOS path — too short risks
        // the app pasting the restored value instead.
        std::thread::sleep(std::time::Duration::from_millis(180));
        if let Some(text) = saved {
            if let Ok(ctx) = ClipboardContext::new() {
                let _ = ctx.set_text(text);
            }
        }

        Ok(ReplaceOutcome::Replaced)
    }
}
