use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// UI-side flags shared between Tauri commands and the popup window-event
/// handler. Currently only carries `suppress_hide`, which the frontend toggles
/// while a modal child window (e.g., the native file-open dialog) owns focus,
/// so the popup's "hide on blur" behaviour doesn't tear the popup down while
/// the user is still picking a file.
#[derive(Default)]
pub struct UiState {
    pub suppress_hide: Arc<AtomicBool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn default_is_not_suppressing_hide() {
        let s = UiState::default();
        assert!(!s.suppress_hide.load(Ordering::Relaxed));
    }

    #[test]
    fn suppress_hide_flag_round_trips() {
        let s = UiState::default();
        s.suppress_hide.store(true, Ordering::Relaxed);
        assert!(s.suppress_hide.load(Ordering::Relaxed));
        s.suppress_hide.store(false, Ordering::Relaxed);
        assert!(!s.suppress_hide.load(Ordering::Relaxed));
    }

    #[test]
    fn suppress_hide_flag_shared_via_arc_clone() {
        let s = UiState::default();
        let clone = s.suppress_hide.clone();
        s.suppress_hide.store(true, Ordering::Relaxed);
        assert!(clone.load(Ordering::Relaxed));
        clone.store(false, Ordering::Relaxed);
        assert!(!s.suppress_hide.load(Ordering::Relaxed));
    }

    #[test]
    fn suppress_hide_survives_thread_handoff() {
        let s = UiState::default();
        let flag = s.suppress_hide.clone();
        let handle = std::thread::spawn(move || {
            flag.store(true, Ordering::SeqCst);
        });
        handle.join().expect("thread should complete");
        assert!(s.suppress_hide.load(Ordering::SeqCst));
    }

    #[test]
    fn ui_state_is_send_sync_safe_for_tauri_state() {
        // Compile-time guard: Tauri's `manage()` requires Send+Sync. If this
        // ever stops compiling we've added a non-thread-safe field.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<UiState>();
    }
}
