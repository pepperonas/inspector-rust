//! Search-bar timer (`timer 12` / `timer 12s` / `timer 12 min` / …).
//!
//! Each `start_timer` IPC spawns a worker thread that sleeps for the
//! requested duration, then fires THREE notifications in parallel:
//!
//!   1. macOS native notification (top-right of screen, visible
//!      even when Inspector Rust's popup is closed) via `osascript
//!      -e 'display notification ...'`.
//!   2. System sound (`afplay /System/Library/Sounds/Glass.aiff`)
//!      detached so the worker exits while audio is still playing.
//!   3. Tauri `timer-fired` event → the popup, if open, shows a
//!      banner.
//!
//! Cancellation: the worker polls a per-timer `AtomicBool` every
//! ~200 ms, so cancelling is responsive but never blocks the IPC.
//!
//! State is a `HashMap<TimerId, TimerSlot>` behind a Mutex; the
//! frontend uses `list_timers` to count active timers for the
//! footer LED indicator.

use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

/// Stable identity for an in-flight timer. Frontend round-trips the
/// id when cancelling.
pub type TimerId = u64;

/// One active timer.
pub struct TimerSlot {
    pub id: TimerId,
    pub label: String,
    pub fires_at: Instant,
    /// Set to `true` by `cancel_timer` — the worker polls this and
    /// returns early on cancel.
    pub stop: Arc<AtomicBool>,
    /// Held so we don't drop the handle (and trigger join) before
    /// the cancel signal lands. Worker thread is detached in spirit;
    /// we just keep the handle for tracking.
    pub _handle: JoinHandle<()>,
}

/// Lightweight frontend-facing snapshot. The actual `TimerSlot` holds
/// runtime-only fields (Arc, JoinHandle) that we can't serialise.
#[derive(Clone, Debug, Serialize)]
pub struct TimerView {
    pub id: TimerId,
    pub label: String,
    /// Seconds remaining (negative if already fired but not yet
    /// pruned — shouldn't normally happen since the worker cleans up).
    pub remaining_secs: i64,
}

/// Tauri-managed singleton.
#[derive(Default)]
pub struct TimerRegistry {
    pub timers: Mutex<HashMap<TimerId, TimerSlot>>,
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Start a new timer. Returns its id.
pub fn start(
    reg: &TimerRegistry,
    app: AppHandle,
    seconds: u64,
    label: String,
) -> TimerId {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_worker = stop.clone();
    let label_for_worker = label.clone();

    let handle = std::thread::Builder::new()
        .name(format!("timer-{id}"))
        .spawn(move || {
            run_timer(app, id, seconds, label_for_worker, stop_for_worker);
        })
        .expect("spawn timer worker");

    let fires_at = Instant::now() + Duration::from_secs(seconds);
    reg.timers.lock().insert(
        id,
        TimerSlot {
            id,
            label,
            fires_at,
            stop,
            _handle: handle,
        },
    );
    id
}

/// Cancel an in-flight timer. No-op if `id` is unknown (already
/// fired + pruned, or never existed).
pub fn cancel(reg: &TimerRegistry, id: TimerId) -> bool {
    let mut t = reg.timers.lock();
    if let Some(slot) = t.remove(&id) {
        slot.stop.store(true, Ordering::SeqCst);
        true
    } else {
        false
    }
}

/// Snapshot of active timers for the frontend footer.
pub fn list(reg: &TimerRegistry) -> Vec<TimerView> {
    let now = Instant::now();
    reg.timers
        .lock()
        .values()
        .map(|s| TimerView {
            id: s.id,
            label: s.label.clone(),
            remaining_secs: s.fires_at.saturating_duration_since(now).as_secs() as i64,
        })
        .collect()
}

fn run_timer(
    app: AppHandle,
    id: TimerId,
    seconds: u64,
    label: String,
    stop: Arc<AtomicBool>,
) {
    // Poll-sleep so cancellation is responsive (~200 ms granularity)
    // without burning CPU.
    let total = Duration::from_secs(seconds);
    let started = Instant::now();
    while started.elapsed() < total {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    if stop.load(Ordering::SeqCst) {
        return;
    }

    // Remove ourselves from the registry first so the footer count
    // updates ASAP (even if the notification syscalls hang briefly).
    if let Some(reg) = app.try_state::<TimerRegistry>() {
        reg.timers.lock().remove(&id);
    }

    // Fire visual + audio notifications + the Tauri event in
    // parallel — none of them should block the others.
    notify_visual(&label);
    notify_audio();
    let _ = app.emit("timer-fired", serde_json::json!({ "id": id, "label": label }));
    let _ = app.emit("timers-changed", ());
}

#[cfg(target_os = "macos")]
fn notify_visual(label: &str) {
    // `display notification` via osascript shows a native macOS
    // notification (top-right corner, with sound effect if the
    // system has one). Works regardless of whether our popup is
    // open. Sanitise the label so a quote in the user's input
    // can't break the AppleScript string literal.
    let safe_label = label.replace('"', "'").replace('\\', "/");
    let script = format!(
        r#"display notification "Timer done — {safe_label}" with title "Inspector Rust" subtitle "Timer fired""#
    );
    let _ = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(&script)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(not(target_os = "macos"))]
fn notify_visual(_label: &str) {
    // Windows + Linux native notifications: not in v1.
    // The `timer-fired` event + popup banner are the cross-platform
    // surface.
}

#[cfg(target_os = "macos")]
fn notify_audio() {
    // `afplay` is the standard macOS CLI sound player; the Glass
    // sound is a default in /System/Library/Sounds, always present.
    // `spawn` (not `status`) so we don't block — afplay can take
    // ~500 ms to finish playing and we want the worker to exit
    // immediately.
    let _ = std::process::Command::new("/usr/bin/afplay")
        .arg("/System/Library/Sounds/Glass.aiff")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(not(target_os = "macos"))]
fn notify_audio() {
    // Defer to a v2 with a cross-platform audio crate.
}
