use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// Monotonic "popup lifecycle" generation, bumped on every show **and** every
/// `Focused(true)`. The Windows settle-confirm captures it on a `Focused(false)`
/// and, ~`SETTLE_MS` later, only hides if it is **unchanged** — so any
/// intervening re-show or focus-regain (a resolving WebView2 focus bounce)
/// cancels the pending hide. Replaces the brittle fixed-grace-window race.
static SHOW_GEN: AtomicU64 = AtomicU64::new(0);

/// How long the Windows `Focused(false)` handler waits before *confirming* a
/// hide. A transient focus bounce resolves (foreground returns to our process /
/// `Focused(true)` re-fires) within this window → the hide is cancelled. A real
/// click-away keeps a foreign window foreground past it → the hide proceeds.
#[cfg(target_os = "windows")]
const SETTLE_MS: u64 = 250;

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
/// the `on_window_event` handler in `lib.rs`. Bumps [`SHOW_GEN`] so a pending
/// Windows settle-confirm (from a just-prior bounce) is cancelled.
pub fn mark_popup_focused() {
    POPUP_HAS_FOCUS.store(true, Ordering::Relaxed);
    SHOW_GEN.fetch_add(1, Ordering::Relaxed);
}

/// Has the popup received a genuine `Focused(true)` since the last show? The
/// Windows handler ignores a `Focused(false)` that arrives before the first
/// focus (the `SetForegroundWindow`-failed show-race), since it's never a
/// real dismiss.
#[cfg(target_os = "windows")]
pub fn popup_was_focused() -> bool {
    POPUP_HAS_FOCUS.load(Ordering::Relaxed)
}

/// One activation retry per show (see [`settle_verdict`]) — reset by
/// [`mark_shown`], consumed by the settle-confirm.
static SETTLE_RETRIED: AtomicBool = AtomicBool::new(false);

/// How long after a show the popup is considered *intentionally opened*: a
/// settle-confirm that wants to hide within this window is treated as the
/// foreground-lock snap-back (activation stolen back right after the hotkey
/// press), NOT a user dismiss — the user literally just asked for the popup.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const SHOW_INTENT: Duration = Duration::from_millis(2_000);

/// What the Windows settle-confirm should do (pure; unit-tested).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
enum SettleVerdict {
    /// Bounce resolved / modal armed / already hidden — nothing to do.
    Keep,
    /// Foreign foreground right after an intentional show → the activation was
    /// snapped back, not dismissed. Re-run `force_foreground` once.
    RetryActivate,
    /// Activation failed twice within the intent window → leave the popup
    /// visible (unfocused). Closing a popup the user summoned 200 ms ago is
    /// never right; they can click it / press Esc.
    StayVisible,
    /// Real dismiss (foreign foreground, past the intent window) → hide.
    Hide,
}

/// Pure decision for the Windows settle-confirm. Hide requires: nothing
/// happened since the `Focused(false)` (generation unchanged), no modal armed
/// (`suppressed`), the OS foreground **still** foreign, the popup still
/// visible — and, new in v0.84.225, the show **not** being ~2 s fresh: a hide
/// verdict inside the intent window means the OS snapped foreground back after
/// a failed activation (the chronic "opens then closes in 200 ms" bug), so we
/// retry activation once and otherwise keep the popup on screen.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn settle_verdict(
    captured_gen: u64,
    current_gen: u64,
    suppressed: bool,
    foreground_is_ours: bool,
    visible: bool,
    just_shown: bool,
    already_retried: bool,
) -> SettleVerdict {
    let wants_hide = captured_gen == current_gen && !suppressed && !foreground_is_ours && visible;
    if !wants_hide {
        return SettleVerdict::Keep;
    }
    if just_shown {
        if already_retried {
            return SettleVerdict::StayVisible;
        }
        return SettleVerdict::RetryActivate;
    }
    SettleVerdict::Hide
}

/// Windows: defer the hide decision. On a `Focused(false)`, capture the current
/// generation and re-evaluate ~`SETTLE_MS` later on the main thread — confirming
/// a real dismiss vs. a transient WebView2 focus bounce (see [`settle_verdict`]).
#[cfg(target_os = "windows")]
pub fn schedule_settle_hide(app: &AppHandle) {
    use tauri::Manager as _;
    let captured = SHOW_GEN.load(Ordering::Relaxed);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(SETTLE_MS));
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            let suppressed = app2
                .try_state::<crate::ui_state::UiState>()
                .map(|s| s.suppress_hide.load(Ordering::Relaxed))
                .unwrap_or(false);
            let visible = app2
                .get_webview_window(POPUP_LABEL)
                .and_then(|w| w.is_visible().ok())
                .unwrap_or(false);
            let just_shown =
                is_within_grace(*LAST_SHOWN_AT.lock(), Instant::now(), SHOW_INTENT);
            match settle_verdict(
                captured,
                SHOW_GEN.load(Ordering::Relaxed),
                suppressed,
                foreground_belongs_to_our_process(),
                visible,
                just_shown,
                SETTLE_RETRIED.load(Ordering::Relaxed),
            ) {
                SettleVerdict::Hide => {
                    tracing::debug!("settle-confirm: real dismiss → hiding popup");
                    hide_popup(&app2);
                }
                SettleVerdict::RetryActivate => {
                    tracing::info!(
                        "settle-confirm: foreground snapped back right after show — retrying activation"
                    );
                    SETTLE_RETRIED.store(true, Ordering::Relaxed);
                    if let Some(w) = app2.get_webview_window(POPUP_LABEL) {
                        force_foreground(&w);
                    }
                    // Re-arm: a successful retry bumps SHOW_GEN via
                    // Focused(true) and cancels this; a failed one lands in
                    // StayVisible next time.
                    schedule_settle_hide(&app2);
                }
                SettleVerdict::StayVisible => {
                    tracing::info!(
                        "settle-confirm: activation failed twice — leaving the popup visible (no auto-hide)"
                    );
                }
                SettleVerdict::Keep => {
                    tracing::debug!("settle-confirm: focus bounce / re-show → keeping popup");
                }
            }
        });
    });
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

/// Windows: reliably bring the popup to the foreground from our background
/// (tray) process — the chronic "popup opens and closes itself within ~200 ms
/// after a while of use" bug lived here.
///
/// Why every previous attempt failed: once the user has been interacting with
/// other apps, the OS **foreground lock** refuses a background process's
/// `SetForegroundWindow`. `AttachThreadInput` alone is unreliable on Win10/11,
/// and the v0.84.201 `SPI_SETFOREGROUNDLOCKTIMEOUT`-to-0 trick was **circular**:
/// per the `SystemParametersInfo` docs that call itself only succeeds for a
/// process that is *already allowed* to change the foreground window — so
/// exactly when we needed it, it silently no-oped. Worse, the attach trick
/// often activated the popup for a *moment* before the OS snapped foreground
/// back to the previous app → `Focused(true)` then `Focused(false)` → the
/// settle-confirm saw a foreign foreground and (correctly, per its rules) hid
/// the popup ~250 ms after it appeared.
///
/// The fix is the battle-tested launcher technique (PowerToys Run PR #1282,
/// Flow Launcher / Wox lineage): **synthesize a benign ALT press+release via
/// `SendInput` first**. Receiving that input makes *our* process "the process
/// that received the last input event" — one of the documented conditions
/// under which `SetForegroundWindow` is honoured — and Windows additionally
/// unlocks foreground changes on an ALT press. Then `SetForegroundWindow` +
/// `BringWindowToTop` (with the `AttachThreadInput` bridge kept as
/// belt-and-braces), verify, and log the outcome so field debugging finally
/// has a signal. Returns whether the popup is the foreground window afterwards.
#[cfg(target_os = "windows")]
fn force_foreground(window: &WebviewWindow) -> bool {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_MENU,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
    };
    let Ok(hwnd) = window.hwnd() else {
        let _ = window.set_focus();
        return false;
    };
    unsafe {
        if GetForegroundWindow() == hwnd {
            let _ = window.set_focus();
            return true;
        }

        // The ALT tap. Down+up as one batch; no modifier state leaks (the up
        // event lands in the same call). Only sent when we actually need to
        // take foreground (checked above).
        let key = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_MENU,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let taps = [key(Default::default()), key(KEYEVENTF_KEYUP)];
        let _ = SendInput(&taps, std::mem::size_of::<INPUT>() as i32);

        let fg = GetForegroundWindow();
        let our_thread = GetCurrentThreadId();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let attached = !fg.0.is_null()
            && fg_thread != 0
            && fg_thread != our_thread
            && AttachThreadInput(fg_thread, our_thread, true).as_bool();
        let _ = SetForegroundWindow(hwnd);
        let _ = BringWindowToTop(hwnd);
        if attached {
            let _ = AttachThreadInput(fg_thread, our_thread, false);
        }

        let _ = window.set_focus();
        let ok = GetForegroundWindow() == hwnd;
        if !ok {
            tracing::info!("force_foreground: still not foreground after ALT-tap + attach");
        }
        ok
    }
}

/// Stamp "popup shown now" so the auto-hide grace window starts. Resets the
/// focus-received flag so stale `Focused(false)` events can't auto-close the
/// freshly opened popup before it receives `Focused(true)`.
fn mark_shown() {
    POPUP_HAS_FOCUS.store(false, Ordering::Relaxed);
    SETTLE_RETRIED.store(false, Ordering::Relaxed);
    *LAST_SHOWN_AT.lock() = Some(Instant::now());
    // Bump the generation so a pending settle-confirm from a previous lifecycle
    // can never hide the freshly-shown popup.
    SHOW_GEN.fetch_add(1, Ordering::Relaxed);
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

    // ── Collision check against every LIVE binding (action hotkeys as
    //    currently configured, expander, direct slots, history) — not the old
    //    hard-coded default list (v0.84.221).
    if let Some(label) = find_conflict(shortcut, &live_bindings(app, Some(BindingOwner::Popup))) {
        return Err(anyhow!("hotkey {hotkey} is already used by {label} — pick another"));
    }
    // Belt-and-braces via the state param too (live_bindings needs the managed
    // state, which is absent in early startup edge cases).
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

    // ── Collision check against every LIVE binding (v0.84.221 — see
    //    register_popup).
    if let Some(label) = find_conflict(shortcut, &live_bindings(app, Some(BindingOwner::History))) {
        return Err(anyhow!("hotkey {hotkey} is already used by {label} — pick another"));
    }
    if *state.current.lock() == Some(shortcut) {
        return Err(anyhow!(
            "hotkey {hotkey} is already the main popup hotkey — pick another"
        ));
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
    TrackToggle,
}

impl ActionId {
    pub const ALL: [ActionId; 9] = [
        ActionId::Ocr,
        ActionId::Screenshot,
        ActionId::Color,
        ActionId::Finder,
        ActionId::Markdown,
        ActionId::Record,
        ActionId::AudioSwap,
        ActionId::Timesheet,
        ActionId::TrackToggle,
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
            ActionId::TrackToggle => "tracktoggle",
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
            ActionId::TrackToggle => "Toggle time tracking",
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
            // Every toggle fires a status toast, so an accidental press is
            // visible (never silent background recording).
            ActionId::TrackToggle => "Ctrl+Shift+Alt+KeyT",
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
        ActionId::TrackToggle => {
            // Start/stop time tracking from anywhere. Worker thread (start/stop
            // do DB work + spawn the run loop); the status toast makes every
            // toggle visible so an accidental press is never silent.
            let app = app.clone();
            std::thread::spawn(move || {
                let db = app.state::<crate::db::DbHandle>().inner().clone();
                let tracker = app.state::<crate::tracking::TrackerState>();
                let active = crate::tracking::status(&tracker).active;
                let result = if active {
                    crate::tracking::stop(&app, &db, &tracker)
                } else {
                    crate::tracking::start(&app, &db, &tracker, None).map(|_| ())
                };
                match result {
                    Ok(()) => crate::status_toast::announce(
                        &app,
                        crate::status_toast::StatusToast {
                            kind: "track".into(),
                            on: !active,
                            title: if active { "Tracking off" } else { "Tracking on" }.into(),
                            subtitle: if active {
                                "Session ended".into()
                            } else {
                                "Recording app usage".into()
                            },
                        },
                    ),
                    Err(e) => tracing::warn!("track toggle hotkey: {e}"),
                }
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

/// Which binding "slot" a collision check runs FOR — that owner's own current
/// entries are excluded from the live set (re-binding replaces them anyway).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BindingOwner {
    Popup,
    History,
    DirectSlots,
    Action(ActionId),
}

/// Every shortcut bound anywhere **right now** (popup, history, expander,
/// direct slots, the 9 rebindable action hotkeys — read from settings, NOT
/// their hard-coded defaults), each with a human label — the single collision
/// set every hotkey validator checks against (v0.84.221; previously
/// popup/history/direct-slots validated against a stale hard-coded default
/// list, so a freed default was still blocked and a re-bound action wasn't).
pub fn live_bindings(app: &AppHandle, owner: Option<BindingOwner>) -> Vec<(Shortcut, String)> {
    use tauri::Manager as _;
    let mut out: Vec<(Shortcut, String)> = Vec::new();
    if let Some(p) = app.try_state::<PopupShortcutState>() {
        if owner != Some(BindingOwner::Popup) {
            if let Some(s) = *p.current.lock() {
                out.push((s, "the popup hotkey".into()));
            }
        }
        if owner != Some(BindingOwner::History) {
            if let Some(s) = *p.history.lock() {
                out.push((s, "the clipboard-history hotkey".into()));
            }
        }
    }
    if let Some(e) = app.try_state::<ExpanderShortcutState>() {
        if let Some(s) = *e.current.lock() {
            out.push((s, "the text expander".into()));
        }
        if owner != Some(BindingOwner::DirectSlots) {
            for (s, _id) in e.direct.lock().iter() {
                out.push((*s, "a direct snippet slot".into()));
            }
        }
    }
    if let Some(db) = app.try_state::<crate::db::DbHandle>() {
        for id in ActionId::ALL {
            if owner == Some(BindingOwner::Action(id)) {
                continue;
            }
            let spec = effective_action_spec(&db, id);
            if spec.trim().is_empty() {
                continue;
            }
            if let Ok(s) = parse_shortcut(&spec) {
                out.push((s, format!("{} ({spec})", id.label())));
            }
        }
    }
    out
}

/// First binding in `set` equal to `shortcut` → its label. Pure + unit-tested.
pub fn find_conflict(shortcut: Shortcut, set: &[(Shortcut, String)]) -> Option<&str> {
    set.iter().find(|(s, _)| *s == shortcut).map(|(_, l)| l.as_str())
}

/// Back-compat shim for the Settings action-hotkey validator.
pub fn all_bound_shortcuts(app: &AppHandle, except: Option<ActionId>) -> Vec<(Shortcut, String)> {
    live_bindings(app, except.map(BindingOwner::Action))
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

    // 2) Parse + validate against every LIVE binding (popup + history as
    //    currently bound, action hotkeys as configured, the abbreviation
    //    expander — v0.84.221; the slots themselves are excluded since this
    //    call replaces them) and against each other.
    let live = live_bindings(app, Some(BindingOwner::DirectSlots));
    let mut parsed: Vec<(Shortcut, i64)> = Vec::with_capacity(slots.len());
    for slot in slots {
        let sc = parse_shortcut(&slot.hotkey)
            .with_context(|| format!("invalid direct-slot hotkey {:?}", slot.hotkey))?;
        if let Some(label) = find_conflict(sc, &live) {
            return Err(anyhow!(
                "hotkey {} is already used by {label} — pick another",
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
    fn settle_verdict_hides_only_on_a_stable_foreign_dismiss_past_the_intent_window() {
        use SettleVerdict::*;
        // Real click-away: gen unchanged, foreground foreign, visible, not
        // modal, show not fresh → hide.
        assert_eq!(settle_verdict(5, 5, false, false, true, false, false), Hide);
        // Focus bounce resolved: foreground returned to our process → keep.
        assert_eq!(settle_verdict(5, 5, false, true, true, false, false), Keep);
        // Re-shown / re-focused during the settle (generation bumped) → keep.
        assert_eq!(settle_verdict(5, 6, false, false, true, false, false), Keep);
        // A modal (file dialog) is up → keep.
        assert_eq!(settle_verdict(5, 5, true, false, true, false, false), Keep);
        // Already hidden somehow → nothing to do.
        assert_eq!(settle_verdict(5, 5, false, false, false, false, false), Keep);
    }

    #[test]
    fn settle_verdict_never_hides_a_freshly_summoned_popup() {
        use SettleVerdict::*;
        // The chronic Windows bug: activation snapped back right after the
        // hotkey show. First settle inside the intent window → retry the
        // activation instead of hiding.
        assert_eq!(settle_verdict(5, 5, false, false, true, true, false), RetryActivate);
        // Retry also failed → the popup STAYS on screen (visible, unfocused).
        assert_eq!(settle_verdict(5, 5, false, false, true, true, true), StayVisible);
        // Fresh show but the bounce resolved (foreground ours) → plain keep,
        // no retry churn.
        assert_eq!(settle_verdict(5, 5, false, true, true, true, false), Keep);
        // Past the intent window the retry flag is irrelevant — foreign
        // foreground = the user really left → hide.
        assert_eq!(settle_verdict(5, 5, false, false, true, false, true), Hide);
    }

    #[test]
    fn settle_verdict_hide_requires_the_conjunction_of_all_guards() {
        // Exhaustive truth table: with the show not fresh, hide IFF the
        // generation is unchanged AND not suppressed AND the foreground is
        // foreign AND the window is visible — and a fresh show NEVER hides.
        for &gen_changed in &[false, true] {
            for &suppressed in &[false, true] {
                for &fg_ours in &[false, true] {
                    for &visible in &[false, true] {
                        for &just_shown in &[false, true] {
                            for &retried in &[false, true] {
                                let captured = 7;
                                let current = if gen_changed { 8 } else { 7 };
                                let v = settle_verdict(
                                    captured, current, suppressed, fg_ours, visible, just_shown,
                                    retried,
                                );
                                let wants = !gen_changed && !suppressed && !fg_ours && visible;
                                let expected = match (wants, just_shown, retried) {
                                    (false, _, _) => SettleVerdict::Keep,
                                    (true, false, _) => SettleVerdict::Hide,
                                    (true, true, false) => SettleVerdict::RetryActivate,
                                    (true, true, true) => SettleVerdict::StayVisible,
                                };
                                assert_eq!(
                                    v, expected,
                                    "gen_changed={gen_changed} suppressed={suppressed} fg_ours={fg_ours} visible={visible} just_shown={just_shown} retried={retried}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

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
        assert_eq!(keys.len(), 9, "expected 9 action hotkeys");
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

    #[test]
    fn find_conflict_matches_only_equal_shortcuts() {
        let set = vec![
            (parse_shortcut("Ctrl+Shift+KeyO").unwrap(), "OCR region (Ctrl+Shift+O)".to_string()),
            (parse_shortcut("Alt+Digit1").unwrap(), "the text expander".to_string()),
        ];
        let hit = find_conflict(parse_shortcut("Ctrl+Shift+KeyO").unwrap(), &set);
        assert_eq!(hit, Some("OCR region (Ctrl+Shift+O)"));
        assert_eq!(find_conflict(parse_shortcut("Alt+Digit1").unwrap(), &set), Some("the text expander"));
        // Same key, different modifiers → no conflict.
        assert_eq!(find_conflict(parse_shortcut("Ctrl+Alt+KeyO").unwrap(), &set), None);
        assert_eq!(find_conflict(parse_shortcut("Ctrl+Shift+KeyP").unwrap(), &set), None);
    }

    #[test]
    fn find_conflict_empty_set_is_free() {
        assert_eq!(find_conflict(parse_shortcut("Ctrl+Space").unwrap(), &[]), None);
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
    // Position the window while still HIDDEN, so no WM_KILLFOCUS fires.
    //
    // Background: calling SetWindowPos on a *visible, focused* window (which
    // is what the old post-show clamp_into_monitor did) sends WM_KILLFOCUS to
    // the window.  If Tauri's async event queue delivers that WindowEvent
    // slightly late — after the 600 ms short-grace window — the auto-hide
    // handler would close the popup ("flickers / sometimes closes" on Windows).
    // Doing all positioning here, before show(), sidesteps that entirely.
    //
    // macOS: position SYNCHRONOUSLY via the native NSWindow (applies before
    // show()). Tauri's set_position queues onto the event loop → applies after
    // show → the popup flashes on the previous monitor then jumps. Only fall back
    // to the Tauri path if the native call fails.
    #[cfg(target_os = "macos")]
    let positioned = position_popup_native_macos(window);
    #[cfg(not(target_os = "macos"))]
    let positioned = false;
    if !positioned {
        if let Some(m) = pick_cursor_monitor(window) {
            if let Err(e) = clamp_into_monitor(window, &m) {
                tracing::debug!("pre-show clamp_into_monitor: {e:#}");
            }
        }
    }

    // Arm the grace + focus-received flag before show() so every Focused event
    // that fires during the show/set_focus sequence is evaluated correctly.
    mark_shown();

    window.show()?;
    // On Windows, force the foreground via the AttachThreadInput trick so the
    // popup reliably activates (a background-process SetForegroundWindow is
    // otherwise blocked); elsewhere a plain set_focus is correct.
    #[cfg(target_os = "windows")]
    force_foreground(window);
    #[cfg(not(target_os = "windows"))]
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

/// macOS: the popup's logical (point) size read straight off the **NSWindow
/// frame** — which is valid even while the window is hidden, unlike Tauri's
/// `outer_size()`, which returns 0/stale for a hidden macOS window. That zero
/// made `clamp_into_monitor` bail (no positioning → the popup opened on whatever
/// screen it was last on). Returns `None` if AppKit isn't reachable / not ready.
#[cfg(target_os = "macos")]
fn native_window_logical_size(window: &WebviewWindow) -> Option<(f64, f64)> {
    use objc2::encode::{Encode, Encoding};
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct NSPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct NSSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct NSRect {
        origin: NSPoint,
        size: NSSize,
    }
    unsafe impl Encode for NSPoint {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for NSSize {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for NSRect {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[NSPoint::ENCODING, NSSize::ENCODING]);
    }

    let ptr = window.ns_window().ok()?;
    let nswindow = ptr as *mut AnyObject;
    if nswindow.is_null() {
        return None;
    }
    let wf: NSRect = unsafe { msg_send![nswindow, frame] };
    if wf.size.width <= 1.0 || wf.size.height <= 1.0 {
        return None;
    }
    Some((wf.size.width, wf.size.height))
}

/// macOS: position the popup **synchronously** via `NSWindow setFrameOrigin:` in
/// Cocoa points — centred horizontally + ~⅓ down on the NSScreen under the live
/// cursor. Critically, this is a *direct* AppKit call (applies immediately),
/// unlike Tauri's `set_position`, which **queues** the move onto the main-thread
/// event loop — so it doesn't apply until after `show()` returns and the popup
/// flashes on the *previous* monitor before jumping (the "opens twice" /
/// "open-close-open" bug; proven by the diag logs). Keeps the window's size.
/// Returns `false` if AppKit isn't reachable (caller falls back to Tauri).
#[cfg(target_os = "macos")]
fn position_popup_native_macos(window: &WebviewWindow) -> bool {
    use objc2::encode::{Encode, Encoding};
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct NSPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct NSSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Copy, Clone, Default)]
    struct NSRect {
        origin: NSPoint,
        size: NSSize,
    }
    unsafe impl Encode for NSPoint {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for NSSize {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for NSRect {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[NSPoint::ENCODING, NSSize::ENCODING]);
    }

    let Ok(ptr) = window.ns_window() else {
        return false;
    };
    let nswindow = ptr as *mut AnyObject;
    if nswindow.is_null() {
        return false;
    }
    unsafe {
        let (Some(ns_event), Some(ns_screen)) =
            (AnyClass::get(c"NSEvent"), AnyClass::get(c"NSScreen"))
        else {
            return false;
        };
        let cursor: NSPoint = msg_send![ns_event, mouseLocation];
        let screens: *mut AnyObject = msg_send![ns_screen, screens];
        if screens.is_null() {
            return false;
        }
        let count: usize = msg_send![screens, count];
        let mut target: *mut AnyObject = std::ptr::null_mut();
        for i in 0..count {
            let s: *mut AnyObject = msg_send![screens, objectAtIndex: i];
            if s.is_null() {
                continue;
            }
            let f: NSRect = msg_send![s, frame];
            if cursor.x >= f.origin.x
                && cursor.x < f.origin.x + f.size.width
                && cursor.y >= f.origin.y
                && cursor.y < f.origin.y + f.size.height
            {
                target = s;
                break;
            }
        }
        if target.is_null() {
            target = msg_send![ns_screen, mainScreen];
        }
        if target.is_null() {
            return false;
        }
        let sf: NSRect = msg_send![target, frame];
        let wf: NSRect = msg_send![nswindow, frame];
        let (ww, wh) = (wf.size.width, wf.size.height);
        if ww <= 1.0 || wh <= 1.0 {
            return false;
        }
        let gap_w = (sf.size.width - ww).max(0.0);
        let gap_h = (sf.size.height - wh).max(0.0);
        let x = sf.origin.x + gap_w / 2.0;
        let y = sf.origin.y + gap_h * 2.0 / 3.0;
        let _: () = msg_send![nswindow, setFrameOrigin: NSPoint { x, y }];
    }
    true
}

/// Center horizontally, place ~⅓ down vertically, then clamp so the window
/// can never extend past any edge of the monitor.
fn clamp_into_monitor(window: &WebviewWindow, monitor: &Monitor) -> Result<()> {
    let mpos = monitor.position();
    let msize = monitor.size();
    let mscale = monitor.scale_factor();

    // The window's PHYSICAL size **on the target monitor**. Computing this from
    // the logical (point) size × the target monitor's scale avoids two macOS
    // traps: (a) `outer_size()` is 0/stale for a hidden window (→ the popup used
    // to open on the wrong screen when the bail-on-zero guard tripped), and (b)
    // `outer_size()` is in the window's *current* scale, which is wrong right
    // after the cursor crosses to a different-DPI display. The point size comes
    // from the native NSWindow frame (reliable even when hidden); the Tauri
    // `outer_size()/scale_factor()` is the cross-platform fallback.
    let logical = {
        #[cfg(target_os = "macos")]
        {
            native_window_logical_size(window)
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }
    .or_else(|| {
        let s = window.outer_size().ok()?;
        if s.width == 0 || s.height == 0 {
            return None;
        }
        let sf = window.scale_factor().unwrap_or(1.0);
        Some((s.width as f64 / sf, s.height as f64 / sf))
    });
    let Some((lw, lh)) = logical else {
        return Ok(()); // size genuinely not ready — bail rather than misplace
    };

    let mw = msize.width as i32;
    let mh = msize.height as i32;
    let ww = (lw * mscale).round() as i32;
    let wh = (lh * mscale).round() as i32;

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
