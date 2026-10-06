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

fn reapply(app: &tauri::AppHandle, why: &str) {
    use tauri::Manager as _;
    let Some(db) = app.try_state::<crate::db::DbHandle>() else { return };
    tracing::info!("display-wake: {why} — re-applying brightness and the gesture capture");
    crate::brightness::restore_saved(app, &db);
    if let Some(state) = app.try_state::<crate::gestures::GestureState>() {
        // `apply` is a no-op when gestures are off; when on, it restarts the
        // capture, which re-registers with MultitouchSupport.
        crate::gestures::apply(app, &db, &state);
    }
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
    fn the_settle_outlasts_display_renegotiation() {
        assert!(SETTLE >= Duration::from_millis(1500));
    }
}
