//! iris — microphone-triggered red screen vignette.
//!
//! Ported in spirit from the raspi5 dB-analysis page's `body.over-iris` wash
//! (disco-controller `public/stats.html`): same palette, same 1.8 s
//! `iris-breathe` alternate pulse on `cubic-bezier(.2,0,0,1)`, same 0.55 s
//! release fade. Two deliberate departures, both because this runs over a
//! *working* desktop instead of a dashboard page:
//!
//! * **Inverted geometry** — an edge vignette instead of a full-page wash, so
//!   the middle of the screen (where you read and type) stays untouched and
//!   the machine remains usable. No text is drawn at all.
//! * **Hysteresis + minimum hold** — the reference is purely level-triggered
//!   (`round(spl,1) > warn_thr`), which is fine for a 1 Hz server-side log but
//!   strobes at the boundary on a live mic feed sampled ~25×/s.
//!
//! The threshold is expressed in **SPL on the disco-controller convention**
//! (`spl = dBFS + DISCO_SPL_OFFSET`, offset 90), so a number that works on
//! raspi5 means the same thing here.
//!
//! Design notes that matter if you touch this:
//!
//! * The **decision lives in Rust, not the overlay**. `iris-over` is emitted
//!   **on the edge only** — mirroring the raspi5 rule that the server's
//!   `warn_over` flag is the single source of truth and the client never
//!   re-derives it from a displayed level. The overlay is a dumb renderer.
//! * Capture is [`crate::mic_capture::start_level`], which hands us an RMS per
//!   chunk. Streaming base64 PCM to a webview just to square it would be waste.
//! * The overlay windows stay **visible and fully transparent** while idle
//!   rather than being shown/hidden per event: a transparent click-through
//!   window is invisible anyway, and window ordering churn twice a second is
//!   both janky and a focus hazard. Only CSS opacity animates.

use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, WebviewWindowBuilder, WebviewUrl};

use crate::db::DbHandle;
use crate::mic_capture::MicStream;

/// dBFS → SPL offset. Identical to the disco-controller's `DISCO_SPL_OFFSET`
/// so thresholds are portable between raspi5 and here.
pub const SPL_OFFSET: f32 = 90.0;
/// Same default as the raspi5 `warn_thr`.
pub const DEFAULT_THRESHOLD_SPL: f32 = 55.0;
/// Matches the raspi5 dB-analysis number field's range.
pub const MIN_THRESHOLD_SPL: f32 = 30.0;
pub const MAX_THRESHOLD_SPL: f32 = 100.0;
/// Release margin: once lit, the level must fall this far below the threshold
/// before it goes dark again. Without it the vignette flickers on any signal
/// hovering at the threshold.
pub const HYSTERESIS_SPL: f32 = 2.0;
/// Once lit, stay lit at least this long — a single loud transient should read
/// as a visible pulse, not a one-frame flicker.
pub const MIN_HOLD_MS: u64 = 400;
/// Attack/release smoothing on the measured SPL. Fast attack so a clap
/// registers; slow release so the readout doesn't twitch.
pub const ATTACK: f32 = 0.55;
pub const RELEASE: f32 = 0.12;
/// Live-level events for the calibration panel, throttled to ~10 Hz.
const LEVEL_EMIT_MS: u64 = 100;

pub const OVERLAY_LABEL_PREFIX: &str = "iris-overlay-";
const SETTING_THRESHOLD: &str = "iris.threshold_spl";
pub const EVENT_OVER: &str = "iris-over";
pub const EVENT_LEVEL: &str = "iris-level";

// ---------------------------------------------------------------- pure core

/// Linear RMS amplitude → dBFS, with a finite floor instead of `-inf`.
/// Mirrors the frontend's `audio-level.ts::rmsToDbfs`.
pub fn rms_to_dbfs(amplitude: f32, silent_db: f32) -> f32 {
    if amplitude > 1e-5 {
        20.0 * amplitude.log10()
    } else {
        silent_db
    }
}

/// dBFS → SPL on the disco-controller convention.
pub fn dbfs_to_spl(dbfs: f32) -> f32 {
    dbfs + SPL_OFFSET
}

/// Clamp a user-supplied threshold into the supported band.
pub fn clamp_threshold(spl: f32) -> f32 {
    if !spl.is_finite() {
        return DEFAULT_THRESHOLD_SPL;
    }
    spl.clamp(MIN_THRESHOLD_SPL, MAX_THRESHOLD_SPL)
}

/// One attack/release smoothing step (rises fast, falls slow).
pub fn smooth_step(current: f32, target: f32, attack: f32, release: f32) -> f32 {
    let a = if target > current { attack } else { release };
    current + (target - current) * a
}

/// The level→lit decision, pure so the hysteresis is testable without a mic.
///
/// * dark → lit as soon as the level exceeds the threshold (no lag: the whole
///   point is to notice *immediately*).
/// * lit → dark only after `MIN_HOLD_MS` **and** once the level has dropped a
///   full [`HYSTERESIS_SPL`] below the threshold.
pub fn decide_over(spl: f32, threshold: f32, currently_over: bool, held_ms: u64) -> bool {
    if !currently_over {
        return spl > threshold;
    }
    if held_ms < MIN_HOLD_MS {
        return true;
    }
    spl > threshold - HYSTERESIS_SPL
}

// ------------------------------------------------------------------- state

struct Shared {
    threshold_spl: f32,
    over: bool,
    changed_at: Instant,
    smoothed_spl: f32,
    last_level_emit: Instant,
}

struct Running {
    /// Dropping this stops the capture and releases the input device.
    _stream: MicStream,
    shared: Arc<Mutex<Shared>>,
    overlays: Vec<String>,
}

#[derive(Default)]
pub struct IrisState {
    inner: Mutex<Option<Running>>,
}

/// Serialised status for the frontend.
#[derive(serde::Serialize, Clone)]
pub struct IrisStatus {
    pub active: bool,
    pub threshold_spl: f32,
    pub over: bool,
    pub min_spl: f32,
    pub max_spl: f32,
}

pub fn status(state: &IrisState, db: &DbHandle) -> IrisStatus {
    let guard = state.inner.lock();
    match guard.as_ref() {
        Some(r) => {
            let s = r.shared.lock();
            IrisStatus {
                active: true,
                threshold_spl: s.threshold_spl,
                over: s.over,
                min_spl: MIN_THRESHOLD_SPL,
                max_spl: MAX_THRESHOLD_SPL,
            }
        }
        None => IrisStatus {
            active: false,
            threshold_spl: saved_threshold(db),
            over: false,
            min_spl: MIN_THRESHOLD_SPL,
            max_spl: MAX_THRESHOLD_SPL,
        },
    }
}

/// Last threshold the user set, or the raspi5-parity default.
pub fn saved_threshold(db: &DbHandle) -> f32 {
    crate::settings::get(db, SETTING_THRESHOLD)
        .ok()
        .flatten()
        .and_then(|v| v.parse::<f32>().ok())
        .map(clamp_threshold)
        .unwrap_or(DEFAULT_THRESHOLD_SPL)
}

fn persist_threshold(db: &DbHandle, spl: f32) {
    let _ = crate::settings::set(db, SETTING_THRESHOLD, &format!("{spl}"));
}

/// Update the threshold of a running session (and persist it either way).
pub fn set_threshold(state: &IrisState, db: &DbHandle, spl: f32) -> f32 {
    let spl = clamp_threshold(spl);
    persist_threshold(db, spl);
    if let Some(r) = state.inner.lock().as_ref() {
        r.shared.lock().threshold_spl = spl;
    }
    spl
}

// ------------------------------------------------------------------ overlay

/// Build one overlay per monitor, **on the main thread**, and return the labels.
///
/// ⚠️ Every window call below is AppKit, and on macOS 26 the WindowManagement
/// framework hard-asserts the main thread (`_WMWindow applyTags:mask:` →
/// "Must only be used from the main thread" → `EXC_BREAKPOINT`, an instant
/// process kill with no Rust panic and therefore no `crash.log`). The `iris_*`
/// IPC commands are `async`, so they run on a **tokio worker** — calling this
/// directly from there crashed the app the moment iris was armed (v0.102.0).
/// Hence the `run_on_main_thread` bounce in [`build_overlays`]; do not inline
/// it back.
///
/// Building a webview window *inside* `run_on_main_thread` is fine — the same
/// pattern the snap overlay uses. What deadlocks is a **sync** command building
/// a window while it occupies the main thread; that is a different situation.
fn build_overlays_on_main(app: &AppHandle) -> Vec<String> {
    let anchor = match app.get_webview_window(crate::hotkey::POPUP_LABEL) {
        Some(w) => w,
        None => return Vec::new(),
    };
    let monitors = anchor.available_monitors().unwrap_or_default();
    let mut labels = Vec::new();
    for (i, m) in monitors.iter().enumerate() {
        let label = format!("{OVERLAY_LABEL_PREFIX}{i}");
        // A stale window from an earlier run would sit at the old geometry.
        if let Some(old) = app.get_webview_window(&label) {
            let _ = old.close();
        }
        let pos = m.position();
        let size = m.size();
        let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
            .title("Iris")
            .inner_size(size.width as f64, size.height as f64)
            .position(pos.x as f64, pos.y as f64)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .shadow(false)
            .focused(false)
            .visible(false)
            .build();
        match built {
            Ok(w) => {
                // Click-through: the vignette must never intercept a click.
                let _ = w.set_ignore_cursor_events(true);
                // Follow the user across Spaces / virtual desktops.
                let _ = w.set_visible_on_all_workspaces(true);
                // Physical px — `inner_size` above is logical, so a HiDPI
                // monitor would otherwise get a half-covered overlay.
                let _ = w.set_position(tauri::PhysicalPosition::new(pos.x, pos.y));
                let _ = w.set_size(tauri::PhysicalSize::new(size.width, size.height));
                show_overlay_window(&w);
                labels.push(label);
            }
            Err(e) => tracing::warn!("iris: overlay build failed for monitor {i}: {e:#}"),
        }
    }
    labels
}

/// Show an overlay **without** taking key focus, and make it survive the
/// app-wide hide.
///
/// Two macOS-specific things, both load-bearing:
///
/// * **`setCanHide:NO`.** `hotkey::hide_popup` ends in `app.hide()`
///   (`NSApp.hide(nil)`) — that is what hands key focus back to the app you
///   were working in, and it hides *every* window this process owns. Without
///   this flag the vignette vanished the moment the popup closed, which is
///   precisely when it needs to be visible (the whole feature is a background
///   monitor). This is the one API that exempts a window from that.
/// * **`orderFrontRegardless`, not Tauri `show()`.** `show()` is
///   `makeKeyAndOrderFront:` — it would steal key from the app underneath on
///   every arm. Same reasoning as the snap overlay.
#[cfg(target_os = "macos")]
fn show_overlay_window(win: &tauri::WebviewWindow) {
    use objc2::runtime::AnyObject;
    use objc2::msg_send;
    let mut ordered = false;
    if let Ok(ns) = win.ns_window() {
        let w = ns as *mut AnyObject;
        if !w.is_null() {
            unsafe {
                let _: () = msg_send![w, setCanHide: false];
                let _: () = msg_send![w, orderFrontRegardless];
            }
            ordered = true;
        }
    }
    if !ordered {
        let _ = win.show();
    }
}

#[cfg(not(target_os = "macos"))]
fn show_overlay_window(win: &tauri::WebviewWindow) {
    let _ = win.show();
}

/// How long a main-thread bounce may take before we give up. The event loop
/// services these in milliseconds; the timeout exists only so a wedged main
/// thread can never hang the caller forever.
const MAIN_THREAD_TIMEOUT: Duration = Duration::from_secs(5);

/// Worker-thread-safe wrapper around [`build_overlays_on_main`] — see its note
/// for why the bounce is mandatory.
fn build_overlays(app: &AppHandle) -> Vec<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let app2 = app.clone();
    if app
        .run_on_main_thread(move || {
            let _ = tx.send(build_overlays_on_main(&app2));
        })
        .is_err()
    {
        tracing::warn!("iris: could not reach the main thread to build overlays");
        return Vec::new();
    }
    rx.recv_timeout(MAIN_THREAD_TIMEOUT).unwrap_or_else(|_| {
        tracing::warn!("iris: overlay build timed out");
        Vec::new()
    })
}

/// Closing a window is AppKit too — same main-thread rule as the build.
fn close_overlays(app: &AppHandle, labels: &[String]) {
    let labels = labels.to_vec();
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        for label in &labels {
            if let Some(w) = app2.get_webview_window(label) {
                let _ = w.close();
            }
        }
    });
}

// ------------------------------------------------------------- start / stop

/// Start monitoring. Returns the effective (clamped) threshold.
///
/// Call from an `async` command — it builds windows.
pub fn start(
    app: &AppHandle,
    db: &DbHandle,
    state: &IrisState,
    threshold_spl: Option<f32>,
) -> Result<f32, String> {
    let threshold = clamp_threshold(threshold_spl.unwrap_or_else(|| saved_threshold(db)));
    persist_threshold(db, threshold);

    // Already running → just retune, don't rebuild the capture or the windows.
    if state.inner.lock().is_some() {
        set_threshold(state, db, threshold);
        return Ok(threshold);
    }

    let overlays = build_overlays(app);
    if overlays.is_empty() {
        return Err("iris.no_overlay".into());
    }

    let now = Instant::now();
    let shared = Arc::new(Mutex::new(Shared {
        threshold_spl: threshold,
        over: false,
        changed_at: now,
        // Start at the floor so the first frames can't false-trigger.
        smoothed_spl: MIN_THRESHOLD_SPL - 20.0,
        last_level_emit: now,
    }));

    let stream = {
        let app = app.clone();
        let shared = shared.clone();
        crate::mic_capture::start_level(app.clone(), move |rms| {
            let spl_now = dbfs_to_spl(rms_to_dbfs(rms, -120.0));
            let (spl, over, threshold, edge, emit_level) = {
                let mut s = shared.lock();
                s.smoothed_spl = smooth_step(s.smoothed_spl, spl_now, ATTACK, RELEASE);
                let held = s.changed_at.elapsed().as_millis() as u64;
                let next = decide_over(s.smoothed_spl, s.threshold_spl, s.over, held);
                let edge = next != s.over;
                if edge {
                    s.over = next;
                    s.changed_at = Instant::now();
                }
                let emit_level = s.last_level_emit.elapsed() >= Duration::from_millis(LEVEL_EMIT_MS);
                if emit_level {
                    s.last_level_emit = Instant::now();
                }
                (s.smoothed_spl, s.over, s.threshold_spl, edge, emit_level)
            };
            // Edge-triggered only — see the module note.
            if edge {
                let _ = app.emit(EVENT_OVER, serde_json::json!({ "over": over }));
            }
            if emit_level {
                let _ = app.emit(
                    EVENT_LEVEL,
                    serde_json::json!({ "spl": spl, "over": over, "threshold": threshold }),
                );
            }
        })
    };

    *state.inner.lock() = Some(Running {
        _stream: stream,
        shared,
        overlays,
    });
    tracing::info!("iris: armed at {threshold} SPL");
    Ok(threshold)
}

/// Stop monitoring and tear the overlays down. Idempotent.
pub fn stop(app: &AppHandle, state: &IrisState) {
    let running = state.inner.lock().take();
    if let Some(r) = running {
        close_overlays(app, &r.overlays);
        // `_stream` drops here → capture stops, input device released.
        tracing::info!("iris: disarmed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Arc;

    fn test_db() -> DbHandle {
        let conn = Connection::open_in_memory().unwrap();
        let db = Arc::new(Mutex::new(conn));
        crate::settings::init_table(&db).unwrap();
        db
    }

    #[test]
    fn spl_matches_the_disco_controller_convention() {
        // raspi5: spl = dbfs + 90, so the default 55 SPL is -35 dBFS.
        assert!((dbfs_to_spl(-35.0) - 55.0).abs() < 1e-6);
        assert!((dbfs_to_spl(0.0) - 90.0).abs() < 1e-6);
    }

    #[test]
    fn rms_to_dbfs_has_a_finite_floor() {
        assert_eq!(rms_to_dbfs(0.0, -120.0), -120.0);
        assert_eq!(rms_to_dbfs(1e-9, -120.0), -120.0);
        // Full scale is 0 dBFS; half amplitude is ~-6 dBFS.
        assert!((rms_to_dbfs(1.0, -120.0) - 0.0).abs() < 1e-4);
        assert!((rms_to_dbfs(0.5, -120.0) + 6.0206).abs() < 1e-3);
    }

    #[test]
    fn threshold_is_clamped_into_the_supported_band() {
        assert_eq!(clamp_threshold(55.0), 55.0);
        assert_eq!(clamp_threshold(5.0), MIN_THRESHOLD_SPL);
        assert_eq!(clamp_threshold(999.0), MAX_THRESHOLD_SPL);
        assert_eq!(clamp_threshold(f32::NAN), DEFAULT_THRESHOLD_SPL);
    }

    #[test]
    fn lights_up_the_moment_the_threshold_is_exceeded() {
        assert!(decide_over(56.0, 55.0, false, 0));
        assert!(!decide_over(55.0, 55.0, false, 0)); // strictly greater, as raspi5
        assert!(!decide_over(54.9, 55.0, false, 0));
    }

    #[test]
    fn minimum_hold_prevents_a_one_frame_flicker() {
        // Level collapses immediately after a transient, but we stay lit.
        assert!(decide_over(10.0, 55.0, true, 0));
        assert!(decide_over(10.0, 55.0, true, MIN_HOLD_MS - 1));
        // Past the hold, a level that low releases.
        assert!(!decide_over(10.0, 55.0, true, MIN_HOLD_MS));
    }

    #[test]
    fn hysteresis_keeps_it_lit_just_below_the_threshold() {
        let held = MIN_HOLD_MS + 1;
        // Within the release margin → stays lit (this is the anti-flap).
        assert!(decide_over(54.0, 55.0, true, held));
        assert!(decide_over(53.1, 55.0, true, held));
        // Beyond it → releases.
        assert!(!decide_over(52.9, 55.0, true, held));
        // ...and that same 54.0 would NOT have lit it from dark.
        assert!(!decide_over(54.0, 55.0, false, held));
    }

    #[test]
    fn smoothing_rises_faster_than_it_falls() {
        let up = smooth_step(0.0, 10.0, ATTACK, RELEASE);
        let down = smooth_step(10.0, 0.0, ATTACK, RELEASE);
        assert!(up > 10.0 - down, "attack should outpace release");
        // Never overshoots the target.
        assert!(up <= 10.0 && down >= 0.0);
    }

    #[test]
    fn a_sustained_loud_signal_stays_lit_across_many_frames() {
        // Regression guard for the flap the hysteresis exists to prevent:
        // a level parked exactly at the threshold must not toggle every frame.
        let mut over = false;
        let mut flips = 0;
        for i in 0..200u64 {
            let next = decide_over(55.05, 55.0, over, i * 40);
            if next != over {
                flips += 1;
                over = next;
            }
        }
        assert_eq!(flips, 1, "should light once and stay lit, got {flips} flips");
    }
    // ── persistence ─────────────────────────────────────────────────────
    // The stored threshold is a plain settings row, so it can be absent,
    // blank, hand-edited or garbage. Reading it must never arm at a nonsense
    // level — same defensive contract as `db::effective_max_entries`.

    #[test]
    fn an_unset_threshold_falls_back_to_the_reference_default() {
        let db = test_db();
        assert_eq!(saved_threshold(&db), DEFAULT_THRESHOLD_SPL);
    }

    #[test]
    fn a_stored_threshold_round_trips() {
        let db = test_db();
        assert_eq!(set_threshold(&IrisState::default(), &db, 62.5), 62.5);
        assert_eq!(saved_threshold(&db), 62.5);
    }

    #[test]
    fn a_hand_edited_out_of_band_threshold_is_clamped_on_read() {
        let db = test_db();
        crate::settings::set(&db, "iris.threshold_spl", "9999").unwrap();
        assert_eq!(saved_threshold(&db), MAX_THRESHOLD_SPL);
        crate::settings::set(&db, "iris.threshold_spl", "-40").unwrap();
        assert_eq!(saved_threshold(&db), MIN_THRESHOLD_SPL);
    }

    #[test]
    fn a_blank_or_garbage_threshold_falls_back_instead_of_arming_wrong() {
        let db = test_db();
        for bad in ["", "   ", "loud", "NaN", "55dB"] {
            crate::settings::set(&db, "iris.threshold_spl", bad).unwrap();
            assert_eq!(
                saved_threshold(&db),
                DEFAULT_THRESHOLD_SPL,
                "{bad:?} should have fallen back to the default"
            );
        }
    }

    #[test]
    fn set_threshold_persists_the_clamped_value_not_the_raw_one() {
        let db = test_db();
        // A caller passing an out-of-band value must not leave a poisoned row
        // behind that a later read has to clean up again.
        assert_eq!(set_threshold(&IrisState::default(), &db, 500.0), MAX_THRESHOLD_SPL);
        let raw = crate::settings::get(&db, "iris.threshold_spl").unwrap().unwrap();
        assert_eq!(raw.parse::<f32>().unwrap(), MAX_THRESHOLD_SPL);
    }

    #[test]
    fn status_of_an_idle_session_reports_the_saved_threshold_and_the_band() {
        let db = test_db();
        set_threshold(&IrisState::default(), &db, 61.0);
        let st = status(&IrisState::default(), &db);
        assert!(!st.active);
        assert!(!st.over);
        assert_eq!(st.threshold_spl, 61.0);
        assert_eq!(st.min_spl, MIN_THRESHOLD_SPL);
        assert_eq!(st.max_spl, MAX_THRESHOLD_SPL);
    }

    // ── the full pure chain: mic amplitude → dBFS → SPL → decision ───────

    /// What the capture callback does, minus the smoothing state.
    fn spl_of(rms: f32) -> f32 {
        dbfs_to_spl(rms_to_dbfs(rms, -120.0))
    }

    #[test]
    fn silence_never_triggers_at_any_supported_threshold() {
        for thr in [MIN_THRESHOLD_SPL, DEFAULT_THRESHOLD_SPL, MAX_THRESHOLD_SPL] {
            assert!(
                !decide_over(spl_of(0.0), thr, false, 0),
                "silence triggered at {thr}"
            );
        }
    }

    #[test]
    fn a_full_scale_signal_triggers_at_every_supported_threshold() {
        // 0 dBFS = 90 SPL, which is below MAX_THRESHOLD_SPL (100) — so the
        // loudest possible input does NOT trip a threshold set at the top of
        // the band. That is a real property of the scale, worth pinning so
        // nobody "fixes" the band without noticing.
        assert!(decide_over(spl_of(1.0), MIN_THRESHOLD_SPL, false, 0));
        assert!(decide_over(spl_of(1.0), DEFAULT_THRESHOLD_SPL, false, 0));
        assert!(!decide_over(spl_of(1.0), MAX_THRESHOLD_SPL, false, 0));
    }

    #[test]
    fn the_default_threshold_sits_where_the_reference_puts_it() {
        // raspi5 warn_thr 55 SPL == -35 dBFS == rms ~0.0178.
        let rms = 10f32.powf(-35.0 / 20.0);
        assert!((spl_of(rms) - DEFAULT_THRESHOLD_SPL).abs() < 0.05);
    }

    #[test]
    fn halving_the_amplitude_drops_about_six_db() {
        let loud = spl_of(0.2);
        let half = spl_of(0.1);
        assert!((loud - half - 6.0206).abs() < 0.01, "{loud} vs {half}");
    }

    #[test]
    fn smoothing_converges_on_a_held_level_without_overshooting() {
        let mut v = 0.0f32;
        for _ in 0..200 {
            v = smooth_step(v, 70.0, ATTACK, RELEASE);
            assert!(v <= 70.0, "overshot to {v}");
        }
        assert!((v - 70.0).abs() < 0.01, "did not converge: {v}");
    }

    #[test]
    fn smoothing_is_monotonic_toward_the_target() {
        let mut up = 0.0f32;
        for _ in 0..40 {
            let next = smooth_step(up, 80.0, ATTACK, RELEASE);
            assert!(next >= up);
            up = next;
        }
        let mut down = 80.0f32;
        for _ in 0..40 {
            let next = smooth_step(down, 0.0, ATTACK, RELEASE);
            assert!(next <= down);
            down = next;
        }
    }

    #[test]
    fn a_quiet_room_with_one_transient_lights_once_and_releases_cleanly() {
        // Simulated 25 Hz feed: quiet, one loud burst, then quiet again.
        let mut smoothed = MIN_THRESHOLD_SPL - 20.0;
        let mut over = false;
        let mut held_ms = 0u64;
        let mut rises = 0;
        for frame in 0..100u64 {
            let target = if (20..24).contains(&frame) { 78.0 } else { 22.0 };
            smoothed = smooth_step(smoothed, target, ATTACK, RELEASE);
            let next = decide_over(smoothed, DEFAULT_THRESHOLD_SPL, over, held_ms);
            if next != over {
                if next {
                    rises += 1;
                }
                over = next;
                held_ms = 0;
            } else {
                held_ms += 40;
            }
        }
        assert_eq!(rises, 1, "the transient should light exactly once");
        assert!(!over, "it should be dark again by the end");
    }

    #[test]
    fn the_release_never_beats_the_minimum_hold() {
        // Even a level that collapses instantly stays lit for the hold.
        let mut held = 0u64;
        let mut lit = true;
        while held < MIN_HOLD_MS {
            assert!(decide_over(0.0, DEFAULT_THRESHOLD_SPL, lit, held));
            held += 40;
        }
        lit = decide_over(0.0, DEFAULT_THRESHOLD_SPL, lit, held);
        assert!(!lit);
    }

    #[test]
    fn clamp_and_the_band_agree_with_the_frontend_mirror() {
        // lib/iris.ts mirrors these three numbers; if one side moves, the
        // other must too (the TS side has the same assertions).
        assert_eq!(MIN_THRESHOLD_SPL, 30.0);
        assert_eq!(MAX_THRESHOLD_SPL, 100.0);
        assert_eq!(DEFAULT_THRESHOLD_SPL, 55.0);
        assert_eq!(SPL_OFFSET, 90.0);
    }
}
