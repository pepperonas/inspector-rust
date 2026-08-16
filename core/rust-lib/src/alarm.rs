//! Timer / countdown **alarm** — the loud, dismiss-to-stop overlay.
//!
//! This is the default when a timer/alarm fires (the legacy OS-notification
//! path stays available via the `timer.alarm_style` setting). When it fires:
//! 1. the system output volume is raised (macOS) so it's clearly audible —
//!    and restored when dismissed;
//! 2. a nice bell-arpeggio alarm sound (`assets/alarm.wav`) loops until stopped;
//! 3. a focused, always-on-top **overlay window** appears that the user must
//!    click (or press Esc/Enter/Space) to silence the alarm.
//!
//! `start`/`stop` are driven from the timer worker (`timer::run_timer`) and the
//! `stop_alarm` IPC (the overlay's Stop button). One alarm at a time — a second
//! firing replaces the first.

use parking_lot::Mutex;
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const ALARM_WAV: &[u8] = include_bytes!("../assets/alarm.wav");
/// The output volume is raised to at least this (0–100) while the alarm sounds,
/// then restored — so the alarm is clearly louder than a quiet/OS-notification.
const ALARM_VOLUME: u8 = 85;
pub const ALARM_OVERLAY_LABEL: &str = "alarm-overlay";

#[derive(Default)]
pub struct AlarmState(pub Mutex<Option<AlarmSession>>);

pub struct AlarmSession {
    pub label: String,
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
    /// The system volume before we raised it (macOS), to restore on stop.
    saved_volume: Option<u8>,
    /// Whether output was MUTED before the alarm (macOS) — restored on stop.
    /// The alarm deliberately overrides mute (an alarm must be heard), but a
    /// Mac the user muted for a meeting must be muted again afterwards.
    saved_muted: Option<bool>,
}

/// Fire the overlay alarm: raise volume, loop the sound, show the overlay.
pub fn start(app: &AppHandle, label: String) {
    // Only one alarm at a time — a new firing replaces any in progress.
    stop(app);

    let stop_flag = Arc::new(AtomicBool::new(false));
    let child: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));
    let (saved_volume, saved_muted) = raise_volume();

    spawn_loop(stop_flag.clone(), child.clone());

    if let Some(st) = app.try_state::<AlarmState>() {
        *st.0.lock() = Some(AlarmSession {
            label: label.clone(),
            stop: stop_flag,
            child,
            saved_volume,
            saved_muted,
        });
    }

    // Build the overlay from a worker thread so Tauri marshals the window
    // creation onto the event loop cleanly (the proven record-stop pattern).
    let app2 = app.clone();
    let label2 = label;
    std::thread::spawn(move || build_overlay(&app2, &label2));
}

/// Silence + dismiss the alarm: stop the loop, kill the player, restore volume,
/// close the overlay.
pub fn stop(app: &AppHandle) {
    let session = app
        .try_state::<AlarmState>()
        .and_then(|st| st.0.lock().take());
    if let Some(s) = session {
        s.stop.store(true, Ordering::SeqCst);
        if let Some(mut c) = s.child.lock().take() {
            let _ = c.kill();
        }
        restore_volume(s.saved_volume, s.saved_muted);
    }
    if let Some(w) = app.get_webview_window(ALARM_OVERLAY_LABEL) {
        let _ = w.close();
    }
    let _ = app.emit("timers-changed", ());
}

/// The overlay's label (the fired timer's name), for the overlay UI.
pub fn current_label(app: &AppHandle) -> Option<String> {
    app.try_state::<AlarmState>()
        .and_then(|st| st.0.lock().as_ref().map(|s| s.label.clone()))
}

// ── Looping sound ────────────────────────────────────────────────────────────

fn spawn_loop(stop: Arc<AtomicBool>, child_slot: Arc<Mutex<Option<Child>>>) {
    std::thread::spawn(move || {
        let Some(path) = materialize_alarm() else {
            tracing::warn!("alarm: couldn't materialise alarm.wav");
            return;
        };
        while !stop.load(Ordering::SeqCst) {
            let Some(c) = play_once(&path) else { break };
            *child_slot.lock() = Some(c);
            // Wait for this play to finish (or for a stop/kill) without holding
            // the lock, so `stop()` can take + kill the child promptly.
            loop {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let done = {
                    let mut g = child_slot.lock();
                    match g.as_mut() {
                        Some(c) => match c.try_wait() {
                            Ok(Some(_)) | Err(_) => {
                                *g = None;
                                true
                            }
                            Ok(None) => false,
                        },
                        None => true, // taken by stop()
                    }
                };
                if done {
                    break;
                }
                std::thread::sleep(Duration::from_millis(80));
            }
        }
    });
}

/// Materialise the embedded alarm WAV to the cache dir once.
fn materialize_alarm() -> Option<PathBuf> {
    static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let dir = dirs::cache_dir()?.join("InspectorRust");
            std::fs::create_dir_all(&dir).ok()?;
            let p = dir.join("alarm.wav");
            if !p.exists() {
                std::fs::write(&p, ALARM_WAV).ok()?;
            }
            Some(p)
        })
        .clone()
}

#[cfg(target_os = "macos")]
fn play_once(path: &std::path::Path) -> Option<Child> {
    std::process::Command::new("/usr/bin/afplay").arg(path).spawn().ok()
}

#[cfg(target_os = "windows")]
fn play_once(path: &std::path::Path) -> Option<Child> {
    let script = format!(
        "(New-Object System.Media.SoundPlayer '{}').PlaySync()",
        path.display()
    );
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .spawn()
        .ok()
}

#[cfg(target_os = "linux")]
fn play_once(path: &std::path::Path) -> Option<Child> {
    std::process::Command::new("paplay")
        .arg(path)
        .spawn()
        .or_else(|_| std::process::Command::new("aplay").arg(path).spawn())
        .ok()
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn play_once(_path: &std::path::Path) -> Option<Child> {
    None
}

// ── System volume (macOS): raise while the alarm sounds, restore after ───────

#[cfg(target_os = "macos")]
fn raise_volume() -> (Option<u8>, Option<bool>) {
    // Read BOTH halves of the state we are about to change. If the read fails
    // (Automation denied, osascript timeout) we deliberately touch NOTHING:
    // the old code forced 85 + unmute anyway and then had nothing to restore —
    // a Mac muted for a meeting was left unmuted at 85 % (fixed 2026-08-16).
    // The alarm still plays at whatever the current level is.
    let current = get_volume();
    let muted = get_muted();
    if current.is_none() {
        tracing::warn!("alarm: could not read the output volume — leaving it untouched");
        return (None, muted);
    }
    // Never *lower* the volume — raise to at least ALARM_VOLUME, and unmute
    // (an alarm must be heard; the saved mute state is restored on stop).
    let target = current.map(|c| c.max(ALARM_VOLUME)).unwrap_or(ALARM_VOLUME);
    set_volume(target);
    (current, muted)
}

#[cfg(target_os = "macos")]
fn restore_volume(saved: Option<u8>, saved_muted: Option<bool>) {
    if let Some(v) = saved {
        let _ = std::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(format!("set volume output volume {}", v.min(100)))
            .status();
    }
    if let Some(true) = saved_muted {
        let _ = std::process::Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg("set volume with output muted")
            .status();
    }
}

#[cfg(target_os = "macos")]
fn get_volume() -> Option<u8> {
    let out = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg("output volume of (get volume settings)")
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse::<u8>().ok()
}

#[cfg(target_os = "macos")]
fn get_muted() -> Option<bool> {
    let out = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg("output muted of (get volume settings)")
        .output()
        .ok()?;
    match String::from_utf8_lossy(&out.stdout).trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn set_volume(level: u8) {
    // Set the level and ensure output isn't muted (else a muted Mac stays silent).
    let _ = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(format!("set volume output volume {}", level.min(100)))
        .arg("-e")
        .arg("set volume without output muted")
        .status();
}

#[cfg(not(target_os = "macos"))]
fn raise_volume() -> (Option<u8>, Option<bool>) {
    (None, None)
}
#[cfg(not(target_os = "macos"))]
fn restore_volume(_saved: Option<u8>, _saved_muted: Option<bool>) {}

// ── Overlay window ───────────────────────────────────────────────────────────

fn build_overlay(app: &AppHandle, label: &str) {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    if let Some(existing) = app.get_webview_window(ALARM_OVERLAY_LABEL) {
        let _ = existing.close();
    }
    let win = match WebviewWindowBuilder::new(
        app,
        ALARM_OVERLAY_LABEL,
        WebviewUrl::App("index.html".into()),
    )
    .title("Alarm")
    .inner_size(440.0, 300.0)
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .center()
    .focused(true)
    .build()
    {
        Ok(w) => w,
        Err(e) => {
            // The overlay is the ONLY way to dismiss the alarm. Without this
            // fallback a failed build (label collision after an unclean close,
            // a transient webview failure) left the bell looping at raised
            // volume with no visible UI and no obvious way to stop it (fixed
            // 2026-08-16). Fall back to the legacy notification and end the
            // alarm cleanly — volume/mute restore included.
            tracing::warn!("alarm: build overlay failed: {e} — falling back to a notification");
            #[cfg(target_os = "macos")]
            {
                let msg = label.replace('"', "'");
                let _ = std::process::Command::new("/usr/bin/osascript")
                    .arg("-e")
                    .arg(format!(
                        "display notification \"{msg}\" with title \"Inspector Rust alarm\""
                    ))
                    .status();
            }
            #[cfg(not(target_os = "macos"))]
            let _ = label;
            stop(app);
            return;
        }
    };
    let _ = win.show();
    let _ = win.set_focus();
}
