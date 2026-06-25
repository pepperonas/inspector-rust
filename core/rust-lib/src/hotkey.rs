use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Monitor, PhysicalPosition, WebviewWindow};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// Guards against spawning a second capture while one is already active.
/// A second Ctrl+Shift+S press while the region picker is open is just
/// ignored — there's nothing useful a "second" capture could do.
static SCREENSHOT_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Wall-clock of the most recent popup `show_and_position`. Read by the
/// `WindowEvent::Focused(false)` auto-hide handler to ignore a **spurious**
/// focus-loss that arrives in the first few hundred ms after the popup is
/// shown. On Windows, `show()` + `set_focus()` can emit a transient
/// `Focused(false)` immediately after the window appears (a focus flicker
/// that becomes reliable once another transient always-on-top window — the
/// status toast, the record overlay — has perturbed the foreground z-order
/// during the session), which made the popup "open briefly then close by
/// itself" until the app was restarted.
static LAST_SHOWN_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// Whether the popup has received a genuine `Focused(true)` event since the
/// last `mark_shown()`. On Windows, `SetForegroundWindow` can fail silently
/// (background-process restriction), so the popup may show without ever
/// receiving focus. A `Focused(false)` that arrives before any `Focused(true)`
/// is always spurious — we must not auto-hide on it.
static POPUP_HAS_FOCUS: AtomicBool = AtomicBool::new(false);

/// Short grace window: ignore `Focused(false)` for this many ms after show
/// even if `Focused(true)` somehow arrives very quickly and then flickers.
const SHOW_GRACE: Duration = Duration::from_millis(600);

/// Safety-net grace: if `Focused(true)` *never* arrives (e.g. `SetForeground-
/// Window` fails on Windows), ignore `Focused(false)` up to this deadline so
/// stale OS messages can't auto-close the popup. After this window the popup
/// is left open (user must press Esc / hotkey to dismiss) — better than
/// silently closing a popup the user can see.
const SHOW_GRACE_NOFOCUS: Duration = Duration::from_millis(3_000);

/// Pure core of the post-show grace check (testable without timers/globals).
fn is_within_grace(last: Option<Instant>, now: Instant, grace: Duration) -> bool {
    match last {
        Some(t) => now.duration_since(t) < grace,
        None => false,
    }
}

/// Returns true when the auto-hide handler should **ignore** a `Focused(false)`
/// event.  Two independent guards:
///
/// 1. **Focus-received guard** — if the popup has never received `Focused(true)`
///    since it was last shown, any `Focused(false)` is spurious (Windows
///    `SetForegroundWindow` can fail silently, leaving the popup visible but
///    unfocused; OS then sends a stale `WM_KILLFOCUS`). We ignore those events
///    up to [`SHOW_GRACE_NOFOCUS`] from the last show.
///
/// 2. **Short grace** — even after focus arrives, ignore a `Focused(false)` that
///    lands within [`SHOW_GRACE`] of the show (covers rapid focus-then-flicker).
///
/// On macOS this always returns `false` quickly (no spurious events there).
pub fn within_show_grace() -> bool {
    let last = *LAST_SHOWN_AT.lock();
    let now = Instant::now();
    if !POPUP_HAS_FOCUS.load(Ordering::Relaxed) {
        // No Focused(true) yet — suppress until the safety-net deadline.
        return is_within_grace(last, now, SHOW_GRACE_NOFOCUS);
    }
    // Focused(true) arrived; only suppress during the short flicker window.
    is_within_grace(last, now, SHOW_GRACE)
}

/// Record that the popup window just received `Focused(true)`.  Called from
/// the `on_window_event` handler in `lib.rs`.
pub fn mark_popup_focused() {
    POPUP_HAS_FOCUS.store(true, Ordering::Relaxed);
}

/// On Windows, WebView2 routes focus between its internal child HWNDs and the
/// host (our popup HWND), generating spurious `Focused(false)` events while
/// the popup is still effectively the foreground app.  This helper returns
/// `true` when the OS foreground window still belongs to our own process —
/// the focus change is an internal WebView2/Windows compositor bounce that
/// must NOT trigger auto-hide.
///
/// A genuine user-initiated dismiss (click on another app, Alt+Tab, etc.)
/// always brings a window from a *different* process to the foreground, so
/// this returns `false` exactly when a real hide is warranted.
#[cfg(target_os = "windows")]
pub fn foreground_belongs_to_our_process() -> bool {
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return false;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(fg, Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}

/// Stamp "popup shown now" so the auto-hide grace window starts. Resets the
/// focus-received flag so stale `Focused(false)` events can't auto-close the
/// freshly opened popup before it receives `Focused(true)`.
fn mark_shown() {
    POPUP_HAS_FOCUS.store(false, Ordering::Relaxed);
    *LAST_SHOWN_AT.lock() = Some(Instant::now());
}

use crate::db::DbHandle;
use crate::expander;

/// Parse a `Modifier+...+Code` shortcut string. We deliberately avoid
/// `Shortcut::from_str` from the Tauri plugin — its lookup table is
/// incomplete (no `IntlBackslash`, `IntlBackquote`, `IntlRo`, `IntlYen`,
/// no media keys beyond a handful, …). `keyboard_types::Code` (which the
/// plugin re-exports as `Code`) implements `FromStr` for the full W3C
/// `KeyboardEvent.code` spec, so we route everything through it.
pub fn parse_shortcut(s: &str) -> Result<Shortcut> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("empty shortcut string"));
    }

    let tokens: Vec<&str> = trimmed.split('+').map(str::trim).collect();
    if tokens.iter().any(|t| t.is_empty()) {
        return Err(anyhow!("empty token in shortcut: {trimmed:?}"));
    }

    let (code_token, mod_tokens) = tokens
        .split_last()
        .ok_or_else(|| anyhow!("missing key code in shortcut: {trimmed:?}"))?;

    let mut mods = Modifiers::empty();
    for raw in mod_tokens {
        match raw.to_ascii_uppercase().as_str() {
            "CTRL" | "CONTROL" => mods |= Modifiers::CONTROL,
            "SHIFT" => mods |= Modifiers::SHIFT,
            "ALT" | "OPTION" | "OPT" => mods |= Modifiers::ALT,
            "META" | "CMD" | "COMMAND" | "SUPER" | "WIN" => mods |= Modifiers::SUPER,
            #[cfg(target_os = "macos")]
            "CMDORCTRL" | "CMDORCONTROL" | "COMMANDORCONTROL" | "COMMANDORCTRL" => {
                mods |= Modifiers::SUPER
            }
            #[cfg(not(target_os = "macos"))]
            "CMDORCTRL" | "CMDORCONTROL" | "COMMANDORCONTROL" | "COMMANDORCTRL" => {
                mods |= Modifiers::CONTROL
            }
            other => return Err(anyhow!("unknown modifier {other:?} in {trimmed:?}")),
        }
    }

    let code = Code::from_str(code_token)
        .map_err(|_| anyhow!("unknown key code {code_token:?} in {trimmed:?}"))?;

    Ok(Shortcut::new(Some(mods), code))
}

pub const POPUP_LABEL: &str = "popup";

/// Cheap, TCC-free check: is our popup currently visible? Used by the
/// global hotkey handlers to bail out when the user is interacting with
/// our own window. **Crucially does not call AppleScript / NSWorkspace
/// frontmost-app probes**, so it works on a fresh install where Apple
/// Events ("System Events" automation) hasn't been granted — that grant
/// is independent from Finder's. Pre-0.43.3 we used
/// `inspector_rust_is_frontmost_public` which silently returned false
/// when the System Events grant was missing, leaving the hotkey gate
/// open and the expander cycle continuing into a spurious AX-permission
/// banner when the user pressed Alt+1 in the popup.
fn popup_is_visible(app: &AppHandle) -> bool {
    app.get_webview_window(POPUP_LABEL)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// Default popup-show hotkey: `Ctrl+Space` on every platform. Literal
/// Control (not Cmd) is deliberate — Cmd+Space is macOS Spotlight.
/// Settings can override this to anything `parse_shortcut` accepts
/// (e.g. `Ctrl+Shift+V`, `Alt+Space`, `F19`, …).
///
/// Note (macOS): `Ctrl+Space` is also macOS's default "select previous
/// input source" shortcut. If the popup doesn't open, free that up in
/// System Settings → Keyboard → Keyboard Shortcuts → Input Sources (or
/// pick a different hotkey in Settings).
pub const DEFAULT_POPUP_HOTKEY: &str = "Ctrl+Space";

/// The pre-0.67 default, kept only so [`migrate_legacy_popup_default`] can
/// recognise an un-customised old install and bump it to `Ctrl+Space`.
pub const LEGACY_POPUP_HOTKEY: &str = "Ctrl+Shift+KeyV";

/// Settings key for the user-configured popup hotkey. Stored as the
/// same `Modifier+...+Code` string format that `parse_shortcut`
/// accepts. Absent → default applies.
pub const KEY_POPUP_HOTKEY: &str = "popup.hotkey";

/// A SECOND, optional global hotkey that also opens the popup (clipboard
/// history). Default `Ctrl+Shift+V` (the popup's pre-0.67 default, now free
/// since the main popup migrated to `Ctrl+Space`). Configurable in Settings;
/// independent of the main popup hotkey. Empty string = disabled.
pub const DEFAULT_HISTORY_HOTKEY: &str = "Ctrl+Shift+KeyV";

/// Settings key for the second (clipboard-history) popup hotkey.
pub const KEY_HISTORY_HOTKEY: &str = "popup.history_hotkey";

/// One-shot flag: set once the legacy `Ctrl+Shift+V` popup default has
/// been migrated to [`DEFAULT_POPUP_HOTKEY`]. Idempotent; never clobbers a
/// value the user deliberately set.
pub const KEY_POPUP_HOTKEY_MIGRATED: &str = "popup.hotkey_migrated_v0_67";

/// If the stored popup hotkey is still the pre-0.67 default `Ctrl+Shift+V`
/// (i.e. the user never customised it), rewrite it to the new
/// [`DEFAULT_POPUP_HOTKEY`] (`Ctrl+Space`). A migration flag makes this
/// run once and prevents clobbering a value the user re-picks later.
/// Returns the hotkey string that should be used from here on.
pub fn migrate_legacy_popup_default(db: &crate::db::DbHandle) -> String {
    use crate::settings;
    let stored = settings::get_or(db, KEY_POPUP_HOTKEY, DEFAULT_POPUP_HOTKEY)
        .unwrap_or_else(|_| DEFAULT_POPUP_HOTKEY.to_string());
    if settings::get_bool(db, KEY_POPUP_HOTKEY_MIGRATED, false).unwrap_or(false) {
        return stored;
    }
    let upgraded = if stored == LEGACY_POPUP_HOTKEY {
        let _ = settings::set(db, KEY_POPUP_HOTKEY, DEFAULT_POPUP_HOTKEY);
        tracing::info!("migrated popup hotkey {LEGACY_POPUP_HOTKEY} → {DEFAULT_POPUP_HOTKEY}");
        DEFAULT_POPUP_HOTKEY.to_string()
    } else {
        stored
    };
    let _ = settings::set(db, KEY_POPUP_HOTKEY_MIGRATED, "true");
    upgraded
}

/// Holds the currently-registered expander shortcut (if any), so we can
/// unregister it cleanly when the user changes it from the settings panel.
/// Also tracks the direct hotkey→snippet slots. Tauri state.
#[derive(Default)]
pub struct ExpanderShortcutState {
    /// The abbreviation-based expander hotkey (the one in Settings → Hotkey).
    pub current: Arc<Mutex<Option<Shortcut>>>,
    /// Direct hotkey→snippet slots currently registered: `(shortcut, snippet_id)`.
    pub direct: Arc<Mutex<Vec<(Shortcut, i64)>>>,
}

/// Holds the currently-registered popup-show shortcut so we can swap
/// it cleanly when the user changes it from Settings. Tauri state.
#[derive(Default)]
pub struct PopupShortcutState {
    pub current: Arc<Mutex<Option<Shortcut>>>,
    /// The second, optional clipboard-history hotkey (also opens the popup).
    pub history: Arc<Mutex<Option<Shortcut>>>,
}

/// Register the popup-show hotkey from a string. If a previous popup
/// shortcut was registered, it is unregistered first. Default is
/// `Ctrl+Space` on every platform (see `DEFAULT_POPUP_HOTKEY`).
///
/// Validates that the new hotkey doesn't collide with the hard-coded
/// global shortcuts (`Ctrl+Shift+O` OCR, `Ctrl+Shift+S` screenshot,
/// `Ctrl+Shift+C` eyedropper, `Ctrl+Shift+F` Finder) or the
/// abbreviation expander / direct-slot hotkeys currently registered.
/// Returns a descriptive `Err` on collision and persists nothing.
pub fn register_popup(app: &AppHandle, state: &PopupShortcutState, hotkey: &str) -> Result<()> {
    let shortcut = parse_shortcut(hotkey)
        .with_context(|| format!("could not parse popup hotkey {hotkey:?}"))?;

    // ── Collision check against the still-hard-coded global shortcuts.
    let ocr = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO);
    let screenshot = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyS);
    let color = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyC);
    let finder = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyF);
    let markdown = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyM);
    let record = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::ALT), Code::KeyS);
    let aswap = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::ALT), Code::KeyM);
    let timesheet = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyT);
    let reserved = [
        (ocr, "OCR region (Ctrl+Shift+O)"),
        (screenshot, "Screenshot region (Ctrl+Shift+S)"),
        (color, "Eyedropper (Ctrl+Shift+C)"),
        (finder, "Finder selection (Ctrl+Shift+F)"),
        (markdown, "Markdown → PDF (Ctrl+Shift+M)"),
        (record, "Screen recording (Ctrl+Shift+Alt+S)"),
        (aswap, "Audio swap (Ctrl+Shift+Alt+M)"),
        (timesheet, "Timesheet (Ctrl+Shift+T)"),
    ];
    for (sc, name) in reserved {
        if shortcut == sc {
            return Err(anyhow!(
                "hotkey {hotkey} is reserved by {name} — pick another"
            ));
        }
    }

    // ── Collision check against the currently-armed expander + direct-slot
    //    hotkeys (read from the existing ExpanderShortcutState if any).
    if let Some(exp_state) = app.try_state::<ExpanderShortcutState>() {
        if let Some(abbr) = *exp_state.current.lock() {
            if shortcut == abbr {
                return Err(anyhow!(
                    "hotkey {hotkey} is bound to the text expander — pick another"
                ));
            }
        }
        for (sc, _id) in exp_state.direct.lock().iter() {
            if shortcut == *sc {
                return Err(anyhow!(
                    "hotkey {hotkey} is bound to a direct snippet slot — pick another"
                ));
            }
        }
    }

    // ── Don't let the main popup hotkey collide with the second
    //    (clipboard-history) hotkey.
    if *state.history.lock() == Some(shortcut) {
        return Err(anyhow!(
            "hotkey {hotkey} is bound to the clipboard-history shortcut — pick another"
        ));
    }

    // Unregister the previous popup hotkey (if any) only AFTER all
    // validation passes — that way a rejected change leaves the old
    // hotkey still working.
    {
        let mut current = state.current.lock();
        if let Some(prev) = current.take() {
            let _ = app.global_shortcut().unregister(prev);
        }
    }

    let app_for_popup = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, sc, event| {
            if event.state == ShortcutState::Pressed && *sc == shortcut {
                if let Err(e) = toggle_popup(&app_for_popup) {
                    tracing::warn!("toggle_popup failed: {e:#}");
                }
            }
        })
        .with_context(|| format!("failed to register popup hotkey {hotkey:?}"))?;

    *state.current.lock() = Some(shortcut);
    tracing::info!("popup hotkey armed: {hotkey}");
    Ok(())
}

/// Register the SECOND, optional clipboard-history hotkey — a separate global
/// shortcut that also toggles the popup. An **empty** string disables it
/// (unregisters any previous one). Same collision checks as [`register_popup`],
/// plus it must not equal the main popup hotkey. Default `Ctrl+Shift+V`.
pub fn register_history_hotkey(
    app: &AppHandle,
    state: &PopupShortcutState,
    hotkey: &str,
) -> Result<()> {
    // Empty = disabled: unregister whatever was there and store None.
    if hotkey.trim().is_empty() {
        let mut hist = state.history.lock();
        if let Some(prev) = hist.take() {
            let _ = app.global_shortcut().unregister(prev);
        }
        tracing::info!("clipboard-history hotkey disabled");
        return Ok(());
    }

    let shortcut = parse_shortcut(hotkey)
        .with_context(|| format!("could not parse clipboard-history hotkey {hotkey:?}"))?;

    // ── Reserved hard-coded globals (same set as register_popup).
    let reserved = [
        (Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO), "OCR region (Ctrl+Shift+O)"),
        (Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyS), "Screenshot region (Ctrl+Shift+S)"),
        (Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyC), "Eyedropper (Ctrl+Shift+C)"),
        (Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyF), "Finder selection (Ctrl+Shift+F)"),
        (Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyM), "Markdown → PDF (Ctrl+Shift+M)"),
        (Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::ALT), Code::KeyS), "Screen recording (Ctrl+Shift+Alt+S)"),
        (Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::ALT), Code::KeyM), "Audio swap (Ctrl+Shift+Alt+M)"),
        (Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyT), "Timesheet (Ctrl+Shift+T)"),
    ];
    for (sc, name) in reserved {
        if shortcut == sc {
            return Err(anyhow!("hotkey {hotkey} is reserved by {name} — pick another"));
        }
    }

    // ── Must not equal the main popup hotkey.
    if *state.current.lock() == Some(shortcut) {
        return Err(anyhow!(
            "hotkey {hotkey} is already the main popup hotkey — pick another"
        ));
    }

    // ── Expander + direct-slot collisions.
    if let Some(exp_state) = app.try_state::<ExpanderShortcutState>() {
        if *exp_state.current.lock() == Some(shortcut) {
            return Err(anyhow!("hotkey {hotkey} is bound to the text expander — pick another"));
        }
        if exp_state.direct.lock().iter().any(|(sc, _)| *sc == shortcut) {
            return Err(anyhow!("hotkey {hotkey} is bound to a direct snippet slot — pick another"));
        }
    }

    // Unregister the previous history hotkey only after validation passes.
    {
        let mut hist = state.history.lock();
        if let Some(prev) = hist.take() {
            let _ = app.global_shortcut().unregister(prev);
        }
    }

    let app_for_history = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, sc, event| {
            if event.state == ShortcutState::Pressed && *sc == shortcut {
                if let Err(e) = toggle_popup(&app_for_history) {
                    tracing::warn!("toggle_popup (history hotkey) failed: {e:#}");
                }
            }
        })
        .with_context(|| format!("failed to register clipboard-history hotkey {hotkey:?}"))?;

    *state.history.lock() = Some(shortcut);
    tracing::info!("clipboard-history hotkey armed: {hotkey}");
    Ok(())
}

/// All configurable global *action* hotkeys (everything except the popup /
/// history / expander / direct-slot hotkeys, which have their own state). One
/// registry: each variant has a stable settings key, a label, and a default
/// binding; the work lives in [`dispatch_action`]. Bindings persist under
/// settings `hotkey.<key>` (empty string = disabled) and are (re)applied by
/// [`apply_action_hotkeys`]. Previously these were hard-coded in `register`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActionId {
    Ocr,
    Screenshot,
    Color,
    Finder,
    Markdown,
    Record,
    AudioSwap,
    Timesheet,
}

impl ActionId {
    pub const ALL: [ActionId; 8] = [
        ActionId::Ocr,
        ActionId::Screenshot,
        ActionId::Color,
        ActionId::Finder,
        ActionId::Markdown,
        ActionId::Record,
        ActionId::AudioSwap,
        ActionId::Timesheet,
    ];

    pub fn key(self) -> &'static str {
        match self {
            ActionId::Ocr => "ocr",
            ActionId::Screenshot => "screenshot",
            ActionId::Color => "color",
            ActionId::Finder => "finder",
            ActionId::Markdown => "markdown",
            ActionId::Record => "record",
            ActionId::AudioSwap => "audioswap",
            ActionId::Timesheet => "timesheet",
        }
    }

    pub fn from_key(s: &str) -> Option<ActionId> {
        ActionId::ALL.into_iter().find(|a| a.key() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            ActionId::Ocr => "OCR region → text",
            ActionId::Screenshot => "Screenshot region",
            ActionId::Color => "Eyedropper (pick colour)",
            ActionId::Finder => "Finder selection",
            ActionId::Markdown => "Markdown → PDF",
            ActionId::Record => "Screen recording",
            ActionId::AudioSwap => "Audio swap",
            ActionId::Timesheet => "Open Timesheet",
        }
    }

    /// Default binding as a `parse_shortcut` spec.
    pub fn default_spec(self) -> &'static str {
        match self {
            ActionId::Ocr => "Ctrl+Shift+KeyO",
            ActionId::Screenshot => "Ctrl+Shift+KeyS",
            ActionId::Color => "Ctrl+Shift+KeyC",
            ActionId::Finder => "Ctrl+Shift+KeyF",
            ActionId::Markdown => "Ctrl+Shift+KeyM",
            ActionId::Record => "Ctrl+Shift+Alt+KeyS",
            ActionId::AudioSwap => "Ctrl+Shift+Alt+KeyM",
            ActionId::Timesheet => "Ctrl+Shift+KeyT",
        }
    }

    fn settings_key(self) -> String {
        format!("hotkey.{}", self.key())
    }
}

/// Currently-registered action shortcuts, so we can unregister cleanly before
/// re-applying. Tauri-managed state.
#[derive(Default)]
pub struct ActionShortcutState(pub Mutex<Vec<Shortcut>>);

/// View of one action hotkey for the Settings UI.
#[derive(Clone, serde::Serialize)]
pub struct ActionHotkeyView {
    pub id: String,
    pub label: String,
    /// Effective binding spec ("" = disabled).
    pub shortcut: String,
    pub default: String,
    pub is_default: bool,
}

/// Effective binding spec for `id`: the user override (settings `hotkey.<key>`,
/// which may be an empty string = disabled) or the built-in default.
pub fn effective_action_spec(db: &crate::db::DbHandle, id: ActionId) -> String {
    crate::settings::get_or(db, &id.settings_key(), id.default_spec())
        .unwrap_or_else(|_| id.default_spec().to_string())
}

/// Run the action bound to `id` (on the global-shortcut callback thread). Each
/// arm preserves its original threading: OCR/screenshot/eyedropper/finder/
/// audio-swap/markdown defer to a worker (and, where a window is built,
/// `run_on_main_thread`); record + timesheet go straight to the main thread.
pub fn dispatch_action(app: &AppHandle, id: ActionId) {
    use tauri::Manager as _;
    match id {
        ActionId::Ocr => {
            let app = app.clone();
            std::thread::spawn(move || match crate::commands::run_ocr_pipeline(&app) {
                Ok(r) if !r.cancelled && r.chars > 0 => {
                    tracing::info!("OCR captured {} chars", r.chars);
                }
                Ok(_) => tracing::debug!("OCR cancelled or empty"),
                Err(e) => {
                    tracing::warn!("OCR pipeline: {e}");
                    if e == "screen.permission_denied" {
                        let _ = show_popup(&app);
                        let _ = app.emit("ocr-permission-needed", ());
                    }
                }
            });
        }
        ActionId::Screenshot => {
            if SCREENSHOT_IN_PROGRESS.load(Ordering::SeqCst) {
                return;
            }
            SCREENSHOT_IN_PROGRESS.store(true, Ordering::SeqCst);
            let app = app.clone();
            std::thread::spawn(move || {
                let result = crate::commands::run_screenshot_pipeline(&app);
                SCREENSHOT_IN_PROGRESS.store(false, Ordering::SeqCst);
                match result {
                    Ok(r) if !r.cancelled && r.bytes > 0 => {
                        tracing::info!("screenshot captured {} bytes", r.bytes);
                    }
                    Ok(_) => tracing::debug!("screenshot cancelled or empty"),
                    Err(e) => {
                        tracing::warn!("screenshot pipeline: {e}");
                        if e == "screen.permission_denied" {
                            let _ = show_popup(&app);
                            let _ = app.emit("ocr-permission-needed", ());
                        }
                    }
                }
            });
        }
        ActionId::Color => {
            let app = app.clone();
            std::thread::spawn(move || {
                crate::commands::run_eyedropper_pipeline(&app);
            });
        }
        ActionId::Finder => {
            let app = app.clone();
            std::thread::spawn(move || {
                crate::commands::run_finder_selection_pipeline(&app);
            });
        }
        ActionId::Record => {
            let app_main = app.clone();
            let _ = app.run_on_main_thread(move || {
                if let Err(e) = crate::commands::screen_record_open_overlay(app_main.clone()) {
                    tracing::warn!("screen-record overlay: {e}");
                }
            });
        }
        ActionId::AudioSwap => {
            let app = app.clone();
            std::thread::spawn(move || {
                let video = crate::finder_selection::read()
                    .ok()
                    .and_then(|paths| paths.into_iter().find(|p| crate::audio_swap::is_video_path(p)));
                if let Some(state) = app.try_state::<crate::commands::AudioSwapState>() {
                    *state.video.lock() = video;
                }
                let app_main = app.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Err(e) = crate::commands::open_audio_swap_overlay(app_main.clone()) {
                        tracing::warn!("audio-swap overlay: {e}");
                    }
                });
            });
        }
        ActionId::Markdown => {
            let app = app.clone();
            std::thread::spawn(move || {
                let paths = match crate::finder_selection::read() {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("markdown→pdf: finder selection failed: {e}");
                        let summary = crate::md_to_pdf::ConvertSummary::default();
                        crate::md_to_pdf::notify(&summary);
                        return;
                    }
                };
                let (tx, rx) = std::sync::mpsc::channel::<crate::md_to_pdf::ConvertSummary>();
                let _ = app.run_on_main_thread(move || {
                    let summary = crate::md_to_pdf::convert_files(&paths);
                    let _ = tx.send(summary);
                });
                let summary = match rx.recv() {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("markdown→pdf: main-thread channel: {e}");
                        crate::md_to_pdf::ConvertSummary::default()
                    }
                };
                tracing::info!(
                    "markdown→pdf: {} converted, {} skipped, {} failed (backend_unavailable={})",
                    summary.converted.len(),
                    summary.skipped.len(),
                    summary.failed.len(),
                    summary.backend_unavailable,
                );
                crate::md_to_pdf::notify(&summary);
            });
        }
        ActionId::Timesheet => {
            let app_main = app.clone();
            let _ = app.run_on_main_thread(move || {
                if let Err(e) = show_popup(&app_main) {
                    tracing::warn!("show popup for timesheet: {e:#}");
                }
                let _ = app_main.emit("open-timesheet-tab", ());
            });
        }
    }
}

/// (Re)register every action hotkey from its effective binding. Unregisters the
/// previously-registered set first (tracked in [`ActionShortcutState`]). Called
/// at startup and after a settings change. **Never call from inside a
/// global-shortcut handler** (plugin-mutex re-entrancy → deadlock) — only from
/// startup / a Settings IPC.
pub fn apply_action_hotkeys(app: &AppHandle) {
    use tauri::Manager as _;
    let Some(db) = app.try_state::<crate::db::DbHandle>() else {
        return;
    };
    let Some(state) = app.try_state::<ActionShortcutState>() else {
        return;
    };
    let mut reg = state.0.lock();
    for sc in reg.drain(..) {
        let _ = app.global_shortcut().unregister(sc);
    }
    for id in ActionId::ALL {
        let spec = effective_action_spec(&db, id);
        if spec.trim().is_empty() {
            continue; // disabled by the user
        }
        let shortcut = match parse_shortcut(&spec) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("action hotkey {}: invalid spec {spec:?}: {e:#}", id.key());
                continue;
            }
        };
        if reg.contains(&shortcut) {
            tracing::warn!(
                "action hotkey {} ({spec}) collides with another binding — skipped",
                id.key()
            );
            continue;
        }
        let appc = app.clone();
        match app.global_shortcut().on_shortcut(shortcut, move |_a, sc, ev| {
            if ev.state == ShortcutState::Pressed && *sc == shortcut {
                dispatch_action(&appc, id);
            }
        }) {
            Ok(()) => reg.push(shortcut),
            Err(e) => tracing::warn!("register action hotkey {} ({spec}): {e}", id.key()),
        }
    }
}

/// Startup entry (kept under the old name) — now delegates to the registry.
pub fn register(app: &AppHandle) -> Result<()> {
    apply_action_hotkeys(app);
    Ok(())
}

/// The action hotkeys for the Settings UI (effective binding + default).
pub fn action_views(app: &AppHandle) -> Vec<ActionHotkeyView> {
    use tauri::Manager as _;
    let Some(db) = app.try_state::<crate::db::DbHandle>() else {
        return Vec::new();
    };
    ActionId::ALL
        .into_iter()
        .map(|id| {
            let spec = effective_action_spec(&db, id);
            ActionHotkeyView {
                id: id.key().to_string(),
                label: id.label().to_string(),
                is_default: spec == id.default_spec(),
                default: id.default_spec().to_string(),
                shortcut: spec,
            }
        })
        .collect()
}

/// Every shortcut bound anywhere (popup, history, expander, direct slots,
/// action hotkeys), each with a human label — for collision checks. `except`
/// excludes one action (when re-binding it).
pub fn all_bound_shortcuts(app: &AppHandle, except: Option<ActionId>) -> Vec<(Shortcut, String)> {
    use tauri::Manager as _;
    let mut out: Vec<(Shortcut, String)> = Vec::new();
    if let Some(p) = app.try_state::<PopupShortcutState>() {
        if let Some(s) = *p.current.lock() {
            out.push((s, "Popup".into()));
        }
        if let Some(s) = *p.history.lock() {
            out.push((s, "Clipboard history".into()));
        }
    }
    if let Some(e) = app.try_state::<ExpanderShortcutState>() {
        if let Some(s) = *e.current.lock() {
            out.push((s, "Text expander".into()));
        }
        for (s, _id) in e.direct.lock().iter() {
            out.push((*s, "Direct snippet".into()));
        }
    }
    if let Some(db) = app.try_state::<crate::db::DbHandle>() {
        for id in ActionId::ALL {
            if Some(id) == except {
                continue;
            }
            let spec = effective_action_spec(&db, id);
            if spec.trim().is_empty() {
                continue;
            }
            if let Ok(s) = parse_shortcut(&spec) {
                out.push((s, id.label().to_string()));
            }
        }
    }
    out
}

/// Set (or clear, with an empty `spec`) an action hotkey, validating against
/// every other binding, then re-apply. Returns a human error on conflict /
/// parse failure.
pub fn set_action_hotkey(app: &AppHandle, id_key: &str, spec: &str) -> std::result::Result<(), String> {
    use tauri::Manager as _;
    let id = ActionId::from_key(id_key).ok_or_else(|| format!("unknown action hotkey {id_key:?}"))?;
    let spec = spec.trim();
    if !spec.is_empty() {
        let sc = parse_shortcut(spec).map_err(|e| format!("invalid shortcut: {e}"))?;
        if let Some((_, label)) = all_bound_shortcuts(app, Some(id)).into_iter().find(|(s, _)| *s == sc) {
            return Err(format!("already used by {label}"));
        }
    }
    let db = app.try_state::<crate::db::DbHandle>().ok_or("no db")?;
    crate::settings::set(&db, &id.settings_key(), spec).map_err(|e| e.to_string())?;
    apply_action_hotkeys(app);
    Ok(())
}

/// Reset an action hotkey to its built-in default, then re-apply.
pub fn reset_action_hotkey(app: &AppHandle, id_key: &str) -> std::result::Result<(), String> {
    use tauri::Manager as _;
    let id = ActionId::from_key(id_key).ok_or_else(|| format!("unknown action hotkey {id_key:?}"))?;
    let db = app.try_state::<crate::db::DbHandle>().ok_or("no db")?;
    crate::settings::set(&db, &id.settings_key(), id.default_spec()).map_err(|e| e.to_string())?;
    apply_action_hotkeys(app);
    Ok(())
}


/// Register the text-expander hotkey from a string like `"Alt+Backquote"`.
/// If a previous expander shortcut was registered, it is unregistered
/// first. A no-op if `enabled` is false (any prior registration is
/// removed).
pub fn register_expander(
    app: &AppHandle,
    state: &ExpanderShortcutState,
    hotkey: &str,
    enabled: bool,
) -> Result<()> {
    // Always unregister whatever was previously registered first.
    {
        let mut current = state.current.lock();
        if let Some(prev) = current.take() {
            let _ = app.global_shortcut().unregister(prev);
        }
    }

    if !enabled {
        tracing::info!("text expander disabled");
        return Ok(());
    }

    let shortcut = parse_shortcut(hotkey)
        .with_context(|| format!("could not parse expander hotkey {hotkey:?}"))?;
    // Clone for the closure — the OS-level shortcut SWALLOWS the
    // keydown on macOS, so when Inspector Rust is frontmost the webview
    // never sees the keystroke. We forward the hotkey string as a
    // Tauri event so the frontend's in-popup handlers (e.g. pwgen
    // Alt+1…4) can react.
    let hotkey_str = hotkey.to_string();

    let app_for_handler = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, sc, event| {
            if event.state != ShortcutState::Pressed || *sc != shortcut {
                return;
            }
            let app = app_for_handler.clone();

            // **Order matters here.** Popup-visible gate runs BEFORE
            // the accessibility check. If the user has the popup open and
            // hits the expander hotkey — e.g. Alt+1 while a pwgen row is
            // selected — the global expander hotkey fires AND swallows
            // the keydown so the webview never sees it. We do two
            // things: (a) skip the expand-cycle entirely (it would only
            // type into our own search bar) and (b) emit
            // `expander-hotkey-forwarded` with the hotkey string so the
            // frontend can run whatever in-popup handler maps to it
            // (e.g. `Alt+Digit1` → pwgen mode "all").
            //
            // `popup_is_visible` is a pure Tauri window-state read —
            // no TCC grant of any kind required. Works on a fresh
            // install with zero permissions.
            if popup_is_visible(&app) {
                tracing::debug!(
                    "expander hotkey {hotkey_str:?} pressed while popup is visible — \
                     forwarding to in-popup handler"
                );
                let _ = app.emit("expander-hotkey-forwarded", hotkey_str.clone());
                return;
            }

            // Bail *loudly* when synthetic input isn't available (macOS
            // Accessibility). Without this the whole expand cycle silently
            // no-ops — enigo's keystrokes never reach the source app — and
            // the user concludes the hotkey is dead. Pop the popup + emit
            // an event the frontend turns into a "grant Accessibility"
            // banner. Mirrors the OCR `screen.permission_denied` path.
            // `accessibility_granted()` is a cheap, thread-safe CF call.
            // (Always `true` on Windows/Linux, so this never blocks there.)
            if !expander::accessibility_granted() {
                tracing::warn!(
                    "expander hotkey pressed but Accessibility not granted — \
                     showing permission banner instead of running the cycle"
                );
                let _ = show_popup(&app);
                let _ = app.emit("expander-permission-needed", ());
                return;
            }

            // CRITICAL: enigo's `Key::Unicode(...)` mapping calls the
            // macOS TSM (Text Services Manager) APIs, which assert
            // they're called on the main thread. Calling them from a
            // worker thread — as we used to with `std::thread::spawn`
            // — fires `_dispatch_assert_queue_fail` and crashes the
            // process with EXC_BREAKPOINT/SIGTRAP. Dispatch the whole
            // cycle to the main thread instead. ~330 ms blocking is
            // acceptable here: the popup is hidden during the cycle,
            // so the freeze is invisible to the user.
            let app_in_closure = app.clone();
            let app_for_err = app.clone();
            let _ = app.run_on_main_thread(move || {
                // Preferred path (v0.64.0): expand the abbreviation the user
                // just typed, read from the passive keystroke buffer. Works in
                // *any* app — including terminals — because it never reads the
                // focused field. Falls through to the legacy AX/UIA read path
                // when the monitor isn't tracking or the buffer didn't match.
                if crate::auto_expand::try_hotkey_expand() {
                    crate::sound::play_paste();
                    return;
                }
                if let Some(db) = app_in_closure.try_state::<DbHandle>() {
                    let watcher = app_in_closure.try_state::<crate::clipboard_watcher::WatcherState>();
                    match expander::expand_at_cursor(&db, watcher.as_deref()) {
                        Ok(()) => {}
                        Err(e) => match expander::BlockReason::from_error(&e) {
                            Some(expander::BlockReason::NoAccessibility) => {
                                let _ = show_popup(&app_for_err);
                                let _ = app_for_err.emit("expander-permission-needed", ());
                            }
                            Some(expander::BlockReason::PasswordField) => {
                                let _ = app_for_err.emit("expander-blocked", "password");
                            }
                            Some(expander::BlockReason::SecureInput) => {
                                let _ = app_for_err.emit("expander-blocked", "secure_input");
                            }
                            Some(expander::BlockReason::InspectorFrontmost) => {
                                tracing::debug!("expander hotkey: Inspector Rust frontmost");
                            }
                            Some(expander::BlockReason::TerminalUnsupported) => {
                                // Terminals can't be expanded into via
                                // the AX/clipboard cycle. Open the
                                // popup as a fallback so the user can
                                // search + paste a snippet manually.
                                let _ = show_popup(&app_for_err);
                                let _ = app_for_err.emit("expander-blocked", "terminal");
                            }
                            None => tracing::warn!("expand_at_cursor failed: {e:#}"),
                        },
                    }
                }
            });
        })
        .with_context(|| format!("failed to register expander hotkey {hotkey:?}"))?;

    *state.current.lock() = Some(shortcut);
    tracing::info!("text expander armed: {hotkey}");
    Ok(())
}

/// (Re-)register the direct hotkey→snippet slots. Unregisters whatever was
/// registered before. Validates that no slot's hotkey collides with the
/// popup hotkey (`Ctrl+Shift+V`), the OCR hotkey, the abbreviation
/// expander hotkey, or another slot — returns a descriptive `Err` on a
/// collision (and persists nothing; the caller does that only on success).
///
/// Each slot's handler dispatches to the main thread (enigo's `Cmd+V` on
/// macOS needs it) and is gated on the Accessibility grant, mirroring the
/// abbreviation expander: on a miss it shows the popup + emits
/// `expander-permission-needed`.
pub fn register_direct_slots(
    app: &AppHandle,
    state: &ExpanderShortcutState,
    slots: &[crate::expander::DirectSlot],
) -> Result<()> {
    // 1) Unregister whatever was registered before.
    {
        let mut cur = state.direct.lock();
        for (sc, _) in cur.drain(..) {
            let _ = app.global_shortcut().unregister(sc);
        }
    }

    // 2) Parse + validate against the reserved shortcuts and each other.
    let popup = Shortcut::new(Some(Modifiers::CONTROL), Code::Space);
    let ocr = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyO);
    let screenshot = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyS);
    let color = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyC);
    let markdown = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyM);
    let record = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::ALT), Code::KeyS);
    let aswap = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT | Modifiers::ALT), Code::KeyM);
    let abbr_hotkey: Option<Shortcut> = *state.current.lock();
    // The optional second (clipboard-history) popup hotkey, if armed.
    let history_hotkey: Option<Shortcut> =
        app.try_state::<PopupShortcutState>().and_then(|s| *s.history.lock());

    let mut parsed: Vec<(Shortcut, i64)> = Vec::with_capacity(slots.len());
    for slot in slots {
        let sc = parse_shortcut(&slot.hotkey)
            .with_context(|| format!("invalid direct-slot hotkey {:?}", slot.hotkey))?;
        if sc == popup
            || sc == ocr
            || sc == screenshot
            || sc == color
            || sc == markdown
            || sc == record
            || sc == aswap
            || abbr_hotkey == Some(sc)
            || history_hotkey == Some(sc)
        {
            return Err(anyhow!(
                "hotkey {} is reserved (popup / clipboard-history / OCR / screenshot / color picker / markdown / recording / audio-swap / text-expander) — pick another",
                slot.hotkey
            ));
        }
        if parsed.iter().any(|(s, _)| *s == sc) {
            return Err(anyhow!("hotkey {} is bound to more than one slot", slot.hotkey));
        }
        parsed.push((sc, slot.snippet_id));
    }

    // 3) Register each.
    for &(sc, snippet_id) in &parsed {
        let app_h = app.clone();
        // Find this slot's hotkey string so we can forward it when
        // Inspector is frontmost — needed because the global shortcut
        // swallows the macOS keydown before the webview sees it.
        let slot_hotkey = slots
            .iter()
            .find(|s| s.snippet_id == snippet_id)
            .map(|s| s.hotkey.clone())
            .unwrap_or_default();
        app.global_shortcut()
            .on_shortcut(sc, move |_app, fired, event| {
                if event.state != ShortcutState::Pressed || *fired != sc {
                    return;
                }
                let app = app_h.clone();
                // Same forwarding as the abbreviation expander: when
                // the popup is visible, the slot's hotkey is most
                // likely meant for an in-popup handler (e.g. pwgen
                // Alt+1…4 mode-switch). Don't fire the paste pipeline;
                // instead emit the hotkey string so the frontend can
                // route it. TCC-free check; see `popup_is_visible`.
                if popup_is_visible(&app) {
                    tracing::debug!(
                        "direct-slot hotkey {slot_hotkey:?} pressed while popup visible — \
                         forwarding to in-popup handler"
                    );
                    let _ = app.emit("expander-hotkey-forwarded", slot_hotkey.clone());
                    return;
                }
                // Paste needs the Accessibility grant on macOS — same gate +
                // banner as the abbreviation expander.
                if !crate::expander::accessibility_granted() {
                    let _ = show_popup(&app);
                    let _ = app.emit("expander-permission-needed", ());
                    return;
                }
                let app_main = app.clone();
                let app_err = app.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Some(db) = app_main.try_state::<DbHandle>() {
                        let watcher = app_main.try_state::<crate::clipboard_watcher::WatcherState>();
                        match crate::expander::paste_snippet_body(&db, snippet_id, watcher.as_deref()) {
                            Ok(()) => {}
                            Err(e) => match crate::expander::BlockReason::from_error(&e) {
                                Some(crate::expander::BlockReason::NoAccessibility) => {
                                    let _ = show_popup(&app_err);
                                    let _ = app_err.emit("expander-permission-needed", ());
                                }
                                Some(crate::expander::BlockReason::PasswordField) => {
                                    let _ = app_err.emit("expander-blocked", "password");
                                }
                                Some(crate::expander::BlockReason::SecureInput) => {
                                    let _ = app_err.emit("expander-blocked", "secure_input");
                                }
                                Some(crate::expander::BlockReason::InspectorFrontmost) => {
                                    tracing::debug!("direct-slot: Inspector Rust frontmost");
                                }
                                Some(crate::expander::BlockReason::TerminalUnsupported) => {
                                    // Direct slots themselves DO work in
                                    // terminals (they don't read anything,
                                    // just Backspace + paste). This branch
                                    // is dead but the enum is exhaustive
                                    // so we keep it for completeness.
                                    tracing::debug!("direct-slot in terminal — shouldn't fire");
                                }
                                None => tracing::warn!("direct-slot paste failed: {e:#}"),
                            },
                        }
                    }
                });
            })
            .with_context(|| format!("failed to register direct-slot hotkey {sc:?}"))?;
    }

    *state.direct.lock() = parsed;
    tracing::info!("registered {} direct hotkey slot(s)", slots.len());
    Ok(())
}

// Several popup helpers (`toggle_popup`, `show_and_position`, …) live after this
// test module; they're grouped by topic, not position. Suppress the lint rather
// than relocate them.
#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_registry_defaults_parse_unique_and_roundtrip() {
        use std::collections::HashSet;
        let mut keys = HashSet::new();
        let mut specs = HashSet::new();
        for id in ActionId::ALL {
            assert!(keys.insert(id.key()), "duplicate action key {}", id.key());
            assert_eq!(ActionId::from_key(id.key()), Some(id), "from_key roundtrip");
            assert!(!id.label().is_empty(), "empty label for {}", id.key());
            assert!(
                parse_shortcut(id.default_spec()).is_ok(),
                "default spec {:?} for {} does not parse",
                id.default_spec(),
                id.key()
            );
            assert!(
                specs.insert(id.default_spec()),
                "duplicate default shortcut {} ({})",
                id.default_spec(),
                id.key()
            );
        }
        assert_eq!(keys.len(), 8, "expected 8 action hotkeys");
        assert_eq!(ActionId::from_key("nope"), None);
    }

    #[test]
    fn show_grace_ignores_focus_loss_only_right_after_show() {
        let now = Instant::now();
        // No show recorded yet → never in grace (auto-hide allowed).
        assert!(!is_within_grace(None, now, SHOW_GRACE));
        // Shown well within the grace window → still in grace (spurious flicker).
        assert!(is_within_grace(
            Some(now - SHOW_GRACE / 2),
            now,
            SHOW_GRACE
        ));
        // Shown well past the grace → a genuine click-away hides. Relative to
        // SHOW_GRACE so it stays correct if the constant is retuned.
        assert!(!is_within_grace(
            Some(now - SHOW_GRACE - Duration::from_millis(100)),
            now,
            SHOW_GRACE
        ));
    }

    #[test]
    fn parse_shortcut_accepts_alt_backquote() {
        let s = parse_shortcut("Alt+Backquote").expect("should parse");
        // Modifiers tolerate any case — verify via a different mod casing.
        assert_eq!(s, parse_shortcut("ALT+Backquote").unwrap());
        assert_eq!(s, parse_shortcut("alt+Backquote").unwrap());
        // Codes must match the W3C `KeyboardEvent.code` PascalCase exactly,
        // since that's what the frontend always sends.
        assert!(parse_shortcut("Alt+backquote").is_err());
    }

    #[test]
    fn parse_shortcut_accepts_intl_backslash() {
        // The whole reason we don't use the Tauri plugin's parser — its
        // table doesn't include IntlBackslash, but `keyboard_types` does.
        // German ISO macOS keyboards report the top-left `^/°` key as
        // IntlBackslash via WebKit, so this is a real path users hit.
        parse_shortcut("Alt+IntlBackslash").expect("IntlBackslash must parse");
        parse_shortcut("Ctrl+Shift+IntlBackslash").expect("with multiple mods");
    }

    #[test]
    fn parse_shortcut_supports_every_letter_and_function_key() {
        for code in ["KeyA", "KeyZ", "Digit0", "Digit9", "F1", "F12", "F19"] {
            parse_shortcut(&format!("Alt+{code}"))
                .unwrap_or_else(|e| panic!("{code} did not parse: {e:#}"));
        }
    }

    #[test]
    fn parse_shortcut_supports_modifier_aliases() {
        // CmdOrCtrl is the cross-platform alias the Tauri plugin advertises.
        parse_shortcut("CmdOrCtrl+KeyV").expect("alias must parse");
        parse_shortcut("Cmd+KeyV").expect("Cmd alias must parse");
        parse_shortcut("Option+KeyV").expect("Option alias must parse");
    }

    #[test]
    fn parse_shortcut_rejects_unknown_modifier() {
        let err = parse_shortcut("Hyper+KeyA")
            .expect_err("Hyper is not a Tauri modifier")
            .to_string();
        assert!(err.contains("unknown modifier"), "got: {err}");
    }

    #[test]
    fn parse_shortcut_rejects_unknown_code() {
        let err = parse_shortcut("Alt+NotAKey")
            .expect_err("garbage code must error")
            .to_string();
        assert!(err.contains("unknown key code"), "got: {err}");
    }

    #[test]
    fn parse_shortcut_rejects_empty_input() {
        assert!(parse_shortcut("").is_err());
        assert!(parse_shortcut("   ").is_err());
    }

    #[test]
    fn parse_shortcut_rejects_dangling_plus() {
        // "Alt+" or "+KeyA" produce empty tokens — should fail loudly.
        assert!(parse_shortcut("Alt+").is_err());
        assert!(parse_shortcut("+KeyA").is_err());
    }

    #[test]
    fn parse_shortcut_accepts_single_key_no_modifier() {
        // Single-key shortcuts (e.g. F19) are valid. The plugin happily
        // registers them — we should accept them too.
        parse_shortcut("F19").expect("bare F19 must parse");
    }

    #[test]
    fn parse_shortcut_tolerates_whitespace_around_tokens() {
        // Users editing settings.json by hand naturally drift in spaces;
        // accepting them makes the parser more forgiving. Key codes must
        // still be valid W3C `Code` strings (KeyV, not bare V).
        parse_shortcut(" Ctrl + Shift + KeyV ").expect("padded chord should parse");
        parse_shortcut("Alt+ Digit1").expect("inner space after + should parse");
    }

    #[test]
    fn parse_shortcut_rejects_empty_token_between_pluses() {
        // `Ctrl++V` is a typo, not a chord with the literal plus key.
        assert!(parse_shortcut("Ctrl++V").is_err());
        assert!(parse_shortcut("++").is_err());
        assert!(parse_shortcut("+").is_err());
    }

    #[test]
    fn parse_shortcut_supports_all_advertised_modifiers() {
        // Every modifier mentioned in the docs/README must round-trip.
        // Listed alphabetically — if a future bump breaks one of these, the
        // README claim is now a lie.
        for combo in &[
            "Alt+KeyA",
            "Control+KeyA",
            "Ctrl+KeyA",
            "Meta+KeyA",
            "Shift+KeyA",
            "Super+KeyA",
        ] {
            parse_shortcut(combo).unwrap_or_else(|e| panic!("{combo} should parse: {e}"));
        }
    }

    #[test]
    fn parse_shortcut_modifier_alias_normalisation_is_stable() {
        // ctrl == Control == cTrL — case must not matter for modifier names
        // (the W3C `KeyboardEvent.code` for the *key* itself is still
        // case-sensitive, but modifiers are looser).
        let a = parse_shortcut("ctrl+KeyV").unwrap();
        let b = parse_shortcut("Control+KeyV").unwrap();
        let c = parse_shortcut("CTRL+KeyV").unwrap();
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    #[test]
    fn parse_shortcut_distinguishes_key_o_from_digit_0() {
        // Critical: `O` (letter) vs `0` (digit). The OCR hotkey uses KeyO;
        // a typo to Digit0 would still parse but register a different chord.
        let with_letter = parse_shortcut("Ctrl+Shift+KeyO").unwrap();
        let with_digit = parse_shortcut("Ctrl+Shift+Digit0").unwrap();
        assert_ne!(with_letter, with_digit);
    }

    #[test]
    fn parse_shortcut_handles_intl_backslash() {
        // `IntlBackslash` is the layout-stable Code that German ISO MacBooks
        // report for the physical `^` key — used to be the expander default
        // pre-v0.12.0. Must keep parsing for backward compatibility with
        // settings written by older builds. Other `Intl*` codes
        // (Backquote/Ro/Yen) aren't in the keyboard_types::Code enum and so
        // can't be hotkeys today; documented here as a known limitation.
        parse_shortcut("IntlBackslash").expect("IntlBackslash must parse");
        parse_shortcut("Alt+IntlBackslash").expect("Alt+IntlBackslash must parse");
    }

    #[test]
    fn parse_shortcut_accepts_arrow_keys() {
        for k in &["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"] {
            parse_shortcut(k).unwrap_or_else(|e| panic!("{k} should parse: {e}"));
        }
        parse_shortcut("Ctrl+ArrowDown").expect("modifier + arrow should parse");
    }

    #[test]
    fn parse_shortcut_accepts_named_control_keys() {
        for k in &["Enter", "Escape", "Tab", "Space", "Backspace", "Delete"] {
            parse_shortcut(k).unwrap_or_else(|e| panic!("{k} should parse: {e}"));
        }
    }

    #[test]
    fn parse_shortcut_accepts_all_digit_row_keys() {
        for n in 0..=9 {
            let s = format!("Alt+Digit{n}");
            parse_shortcut(&s).unwrap_or_else(|e| panic!("{s} should parse: {e}"));
        }
    }

    #[test]
    fn parse_shortcut_lookalike_keys_are_distinct() {
        // KeyL (the letter) vs Digit1 (the number row) vs IntlYen — must
        // produce three different shortcuts, not collide via any normalisation.
        let l = parse_shortcut("KeyL").unwrap();
        let one = parse_shortcut("Digit1").unwrap();
        assert_ne!(l, one);
    }

    #[test]
    fn default_popup_hotkey_is_ctrl_space_and_parses() {
        assert_eq!(DEFAULT_POPUP_HOTKEY, "Ctrl+Space");
        let sc = parse_shortcut(DEFAULT_POPUP_HOTKEY).expect("default popup hotkey must parse");
        assert_eq!(sc, Shortcut::new(Some(Modifiers::CONTROL), Code::Space));
    }

    fn mem_db() -> crate::db::DbHandle {
        use parking_lot::Mutex;
        use rusqlite::Connection;
        use std::sync::Arc;
        let db = Arc::new(Mutex::new(Connection::open_in_memory().unwrap()));
        crate::settings::init_table(&db).unwrap();
        db
    }

    #[test]
    fn migrate_popup_default_upgrades_untouched_install() {
        use crate::settings;
        let db = mem_db();
        // Un-customised old install: stored value is the legacy default.
        settings::set(&db, KEY_POPUP_HOTKEY, LEGACY_POPUP_HOTKEY).unwrap();
        assert_eq!(migrate_legacy_popup_default(&db), DEFAULT_POPUP_HOTKEY);
        assert_eq!(
            settings::get(&db, KEY_POPUP_HOTKEY).unwrap().as_deref(),
            Some(DEFAULT_POPUP_HOTKEY)
        );
        // Idempotent — and won't clobber a value re-picked afterwards.
        settings::set(&db, KEY_POPUP_HOTKEY, LEGACY_POPUP_HOTKEY).unwrap();
        assert_eq!(migrate_legacy_popup_default(&db), LEGACY_POPUP_HOTKEY);
    }

    #[test]
    fn migrate_popup_default_leaves_custom_hotkey_alone() {
        use crate::settings;
        let db = mem_db();
        settings::set(&db, KEY_POPUP_HOTKEY, "Cmd+KeyJ").unwrap();
        assert_eq!(migrate_legacy_popup_default(&db), "Cmd+KeyJ");
        assert_eq!(
            settings::get(&db, KEY_POPUP_HOTKEY).unwrap().as_deref(),
            Some("Cmd+KeyJ")
        );
    }

    #[test]
    fn migrate_popup_default_on_fresh_install_returns_new_default() {
        // Nothing stored → returns the new default, doesn't write the legacy.
        let db = mem_db();
        assert_eq!(migrate_legacy_popup_default(&db), DEFAULT_POPUP_HOTKEY);
    }
}

pub fn toggle_popup(app: &AppHandle) -> Result<()> {
    let window = app
        .get_webview_window(POPUP_LABEL)
        .context("popup window not found")?;

    let visible = window.is_visible().unwrap_or(false);
    tracing::info!(visible, "toggle_popup");
    if visible {
        let _ = window.hide();
        return Ok(());
    }

    show_and_position(&window)?;
    let _ = app.emit("window-shown", ());
    tracing::info!("toggle_popup: shown");
    Ok(())
}

/// Show the popup unconditionally and center it on the cursor monitor.
pub fn show_popup(app: &AppHandle) -> Result<()> {
    let window = app
        .get_webview_window(POPUP_LABEL)
        .context("popup window not found")?;
    show_and_position(&window)
}

pub fn hide_popup(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(POPUP_LABEL) {
        let _ = w.hide();
    }
    // Tell the frontend the popup is gone so it can drop transient state
    // (e.g., close any open modal so the next "open popup" shows the
    // default History view, not the modal that was up at hide-time).
    let _ = app.emit("popup-hidden", ());
    // On macOS, hiding the window alone does not reliably return key focus
    // to the previously active app — especially with `ActivationPolicy::
    // Accessory`. Hiding the whole app (NSApp.hide(nil)) makes the OS
    // restore the prior frontmost app, which is what `enigo`'s synthesized
    // Cmd+V needs in order to land in the right window.
    #[cfg(target_os = "macos")]
    let _ = app.hide();
}

/// Move the (currently hidden) window onto the cursor's monitor, **then**
/// show, **then** center. Reading `outer_size()` before `show()` returns
/// stale or zero values on macOS for hidden windows; doing it in this order
/// guarantees the centering math has the real window size.
fn show_and_position(window: &WebviewWindow) -> Result<()> {
    let target = pick_cursor_monitor(window);

    // Position the window while still HIDDEN, so no WM_KILLFOCUS fires.
    //
    // Background: calling SetWindowPos on a *visible, focused* window (which
    // is what the old post-show clamp_into_monitor did) sends WM_KILLFOCUS to
    // the window.  If Tauri's async event queue delivers that WindowEvent
    // slightly late — after the 600 ms short-grace window — the auto-hide
    // handler would close the popup ("flickers / sometimes closes" on Windows).
    // Doing all positioning here, before show(), sidesteps that entirely.
    if let Some(m) = &target {
        if let Err(e) = clamp_into_monitor(window, m) {
            tracing::debug!("pre-show clamp_into_monitor: {e:#}");
        }
    }

    // Arm the grace + focus-received flag before show() so every Focused event
    // that fires during the show/set_focus sequence is evaluated correctly.
    mark_shown();

    window.show()?;
    window.set_focus()?;
    Ok(())
}

/// Park the (typically hidden) popup window onto the monitor that
/// currently contains the OS cursor. Used by features whose system
/// overlay (NSColorSampler eyedropper, screencapture region selector)
/// renders on the **active app's primary screen** — without this, the
/// loupe / marquee can appear on a different display than the one the
/// user's cursor is actually on, breaking the multi-screen workflow.
///
/// Cheap: a single `set_position` call. Idempotent — if the popup is
/// already on the cursor's monitor, nothing meaningfully changes.
pub fn park_on_cursor_monitor(window: &WebviewWindow) {
    if let Some(m) = pick_cursor_monitor(window) {
        let mpos = m.position();
        let msize = m.size();
        // Park at the monitor's centre — symmetric, doesn't bias to a
        // corner if the next `show()` doesn't run.
        let parked_x = mpos.x + (msize.width as i32 / 2);
        let parked_y = mpos.y + (msize.height as i32 / 2);
        let _ = window.set_position(PhysicalPosition::new(parked_x, parked_y));
    }
}

/// Find the monitor that contains the OS cursor; fall back to primary.
///
/// Uses the **global** cursor query (`screenshot_preview::pick_cursor_monitor_globally`
/// → `CGEventGetLocation`, point-space / mixed-DPI aware) — NOT
/// `WebviewWindow::cursor_position()`, which only refreshes when the popup
/// window itself receives a mouse event and so returns a STALE position. With
/// the stale query the popup opened on the wrong monitor (typically the
/// primary) when the cursor was on a secondary screen — so it looked like
/// "Ctrl+Space doesn't open" while the popup was actually appearing on another
/// display. Same root cause + fix as the record overlay.
fn pick_cursor_monitor(window: &WebviewWindow) -> Option<Monitor> {
    let monitors = window.available_monitors().ok()?;
    crate::screenshot_preview::pick_cursor_monitor_globally(&monitors)
        .or_else(|| window.primary_monitor().ok().flatten())
}

/// Center horizontally, place ~⅓ down vertically, then clamp so the window
/// can never extend past any edge of the monitor.
fn clamp_into_monitor(window: &WebviewWindow, monitor: &Monitor) -> Result<()> {
    let mpos = monitor.position();
    let msize = monitor.size();
    let wsize = window.outer_size().unwrap_or_default();

    // If outer_size is still bogus (zero), bail rather than placing wrongly.
    if wsize.width == 0 || wsize.height == 0 {
        return Ok(());
    }

    let mw = msize.width as i32;
    let mh = msize.height as i32;
    let ww = wsize.width as i32;
    let wh = wsize.height as i32;

    // Desired position: horizontally centered, ~⅓ down.
    let mut x = mpos.x + (mw - ww) / 2;
    let mut y = mpos.y + (mh - wh) / 3;

    // Clamp to monitor bounds. If the window is somehow larger than the
    // monitor (extreme zoom, etc.), `max_x < min_x` — pin to top-left.
    let min_x = mpos.x;
    let max_x = mpos.x + (mw - ww).max(0);
    let min_y = mpos.y;
    let max_y = mpos.y + (mh - wh).max(0);
    x = x.clamp(min_x, max_x);
    y = y.clamp(min_y, max_y);

    window.set_position(PhysicalPosition::new(x, y))?;
    Ok(())
}
