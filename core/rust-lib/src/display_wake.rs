//! Re-arm what macOS silently drops when the screens come back (2026-10-06).
//!
//! Field case: the display slept for ~30 min while the Mac stayed awake. After
//! the display came back, touchpad gestures were dead and the saved brightness
//! was gone, although the app was still running. Two separate effects:
//!
//! * macOS resets every display's gamma table on display wake / reconfigure —
//!   the software dimming in `brightness` lives in exactly that table;
//! * the private MultitouchSupport registration goes deaf. The gesture
//!   watchdog only knows *system* sleep (wall vs monotonic clock), which a
//!   display-only sleep never shows, and its scroll proofs need a scroll.
//!
//! So we listen for the screens waking, the system waking and display
//! reconfiguration, and re-apply both after a short settle. A burst of
//! notifications (wake usually fires two or three) collapses into one run.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// Wait this long after the LAST notification before re-applying: displays
/// are still renegotiating right after wake, and a gamma write that lands
/// before that finishes is overwritten again.
pub const SETTLE: Duration = Duration::from_millis(2000);

/// The notifications we react to, with the centre they are posted on.
pub const NOTIFICATIONS: &[(Center, &str)] = &[
    (Center::Workspace, "NSWorkspaceScreensDidWakeNotification"),
    (Center::Workspace, "NSWorkspaceDidWakeNotification"),
    (Center::Local, "NSApplicationDidChangeScreenParametersNotification"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Center {
    Workspace,
    Local,
}

static GEN: AtomicU64 = AtomicU64::new(0);

/// Pure: does a settle timer started at `scheduled` still own the run? Only
/// the timer of the LAST notification in a burst fires.
pub fn owns_run(scheduled: u64, current: u64) -> bool {
    scheduled == current
}

/// A notification arrived: (re)start the settle timer.
fn schedule(app: &tauri::AppHandle, why: &'static str) {
    let mine = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("ir-display-wake".into())
        .spawn(move || {
            std::thread::sleep(SETTLE);
            if !owns_run(mine, GEN.load(Ordering::SeqCst)) {
                return; // a later notification will do it
            }
            reapply(&app, why);
        });
}

/// The name of the one notification that is NOT a wake.
pub const SCREEN_PARAMS: &str = "NSApplicationDidChangeScreenParametersNotification";

/// Pure: does this notification call for the full re-arm (gesture capture +
/// OCR warm-up), or only for the brightness tables?
///
/// A wake always does. A screen-parameter change does only when the set of
/// displays or their geometry really changed: an external display that
/// renegotiates its link posts that notification every few minutes with
/// nothing different (35–220 a day in the field logs), and tearing down the
/// touch capture and warming the OCR model each time was pure churn. The
/// brightness tables are re-applied either way — macOS resets them on every
/// reconfiguration, and writing them is cheap.
pub fn full_rearm(why: &str, layout_changed: bool) -> bool {
    why != SCREEN_PARAMS || layout_changed
}

/// The display layout last seen (physical rects of every active display).
static LAYOUT: parking_lot::Mutex<Option<Layout>> = parking_lot::Mutex::new(None);

/// Physical rect (x, y, width, height) of every active display.
type Layout = Vec<(i32, i32, u32, u32)>;

#[cfg(target_os = "macos")]
fn current_layout() -> Layout {
    crate::display_rects::physical_rects()
}
#[cfg(not(target_os = "macos"))]
fn current_layout() -> Layout {
    Vec::new()
}

/// Remember the layout and say whether it differs from the last one seen.
fn layout_changed() -> bool {
    let now = current_layout();
    let mut last = LAYOUT.lock();
    let changed = last.as_ref() != Some(&now);
    *last = Some(now);
    changed
}

fn reapply(app: &tauri::AppHandle, why: &str) {
    use tauri::Manager as _;
    let Some(db) = app.try_state::<crate::db::DbHandle>() else { return };
    let full = full_rearm(why, layout_changed());
    if full {
        tracing::info!("display-wake: {why} — re-applying brightness and the gesture capture");
    } else {
        tracing::debug!("display-wake: {why}, same displays — brightness only");
    }
    crate::brightness::restore_saved(app, &db);
    if !full {
        return;
    }
    if let Some(state) = app.try_state::<crate::gestures::GestureState>() {
        // `apply` is a no-op when gestures are off; when on, it restarts the
        // capture, which re-registers with MultitouchSupport.
        crate::gestures::apply(app, &db, &state);
    }
    // After sleep macOS often has dropped the OCR text model; reloading it
    // now keeps the next OCR from waiting for it (cheap when still loaded).
    crate::ocr::prewarm();
}

/// Register the observers once. Must run on the main thread (setup).
#[cfg(target_os = "macos")]
pub fn install(app: &tauri::AppHandle) {
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSString;
    use std::ptr::NonNull;

    static DONE: AtomicBool = AtomicBool::new(false);
    if DONE.swap(true, Ordering::SeqCst) {
        return;
    }
    unsafe {
        let workspace: *mut AnyObject = AnyClass::get(c"NSWorkspace")
            .map(|c| {
                let w: *mut AnyObject = msg_send![c, sharedWorkspace];
                msg_send![w, notificationCenter]
            })
            .unwrap_or(std::ptr::null_mut());
        let local: *mut AnyObject = AnyClass::get(c"NSNotificationCenter")
            .map(|c| msg_send![c, defaultCenter])
            .unwrap_or(std::ptr::null_mut());
        for &(center, name) in NOTIFICATIONS {
            let target = match center {
                Center::Workspace => workspace,
                Center::Local => local,
            };
            if target.is_null() {
                continue;
            }
            let a = app.clone();
            let block = RcBlock::new(move |_note: NonNull<AnyObject>| schedule(&a, name));
            let n = NSString::from_str(name);
            let token: *mut AnyObject = msg_send![
                target,
                addObserverForName: &*n,
                object: std::ptr::null::<AnyObject>(),
                queue: std::ptr::null::<AnyObject>(),
                usingBlock: &*block
            ];
            // The observer lives for the whole process.
            if !token.is_null() {
                let _: *mut AnyObject = msg_send![token, retain];
            }
        }
    }
    // The layout at start is the baseline for "did the displays change?".
    let _ = layout_changed();
    tracing::info!("display-wake: observers armed");
}

#[cfg(not(target_os = "macos"))]
pub fn install(_app: &tauri::AppHandle) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_last_notification_of_a_burst_runs() {
        // Three notifications → generations 1, 2, 3; only 3 owns the run.
        assert!(!owns_run(1, 3));
        assert!(!owns_run(2, 3));
        assert!(owns_run(3, 3));
    }

    #[test]
    fn display_only_wake_is_covered() {
        // The field case: screens woke, the system never slept.
        assert!(NOTIFICATIONS
            .iter()
            .any(|&(c, n)| c == Center::Workspace && n == "NSWorkspaceScreensDidWakeNotification"));
        assert!(NOTIFICATIONS.iter().any(|&(_, n)| n == "NSWorkspaceDidWakeNotification"));
        assert!(NOTIFICATIONS
            .iter()
            .any(|&(c, n)| c == Center::Local && n == "NSApplicationDidChangeScreenParametersNotification"));
    }

    #[test]
    fn a_renegotiating_display_does_not_rebuild_the_touch_capture() {
        // Same displays, only the screen-parameter notification: brightness only.
        assert!(!full_rearm(SCREEN_PARAMS, false));
        // A display was added, removed or changed size: full re-arm.
        assert!(full_rearm(SCREEN_PARAMS, true));
        // A wake always re-arms, changed layout or not.
        assert!(full_rearm("NSWorkspaceScreensDidWakeNotification", false));
        assert!(full_rearm("NSWorkspaceDidWakeNotification", false));
        // The constant names a notification we actually listen for.
        assert!(NOTIFICATIONS.iter().any(|&(_, n)| n == SCREEN_PARAMS));
    }

    #[test]
    fn the_settle_outlasts_display_renegotiation() {
        assert!(SETTLE >= Duration::from_millis(1500));
    }
}
