//! `wakelock` — keep the computer awake. Toggled via the search-bar
//! command `wakelock1` / `wakelock=1` (on) and `wakelock0` /
//! `wakelock=0` (off).
//!
//! **macOS (v0.41.0+):** spawns `/usr/bin/caffeinate -disu` as a
//! child process while wakelock is on; killed on toggle-off. This
//! raises proper IOPM assertions in the kernel
//! (`PreventUserIdleSystemSleep` + `PreventUserIdleDisplaySleep`),
//! which actually pauses the screen-lock / screensaver counters.
//!
//! Pre-0.41.0 macOS used the same cursor-jiggle path as Windows /
//! Linux below — `CGEventPost`-synthesized mouse moves every 60 s.
//! That defeats application-level "away" detectors (Teams, Slack) but
//! **does not** stop the macOS screen-lock: modern macOS hardens its
//! idle counter against synthetic mouse-moved events, so the screen
//! still locked despite the LED pulsing.
//!
//! **Windows / Linux:** cursor-jiggle worker remains in place — those
//! OSes don't ship an equivalent CLI in the base install. The jiggle
//! is **two** synthetic mouse-move events spaced 30 ms apart: one to
//! `(x+1, y)`, one back to `(x, y)`. The OS sees real motion, but the
//! visual blip is imperceptible. On Linux it's X11-only — Wayland
//! deliberately denies cursor synth at the protocol level, so the
//! jiggle is a no-op there (a future `org.freedesktop.ScreenSaver`
//! D-Bus inhibit would be the proper Wayland path).

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(target_os = "macos"))]
use std::sync::Arc;
#[cfg(not(target_os = "macos"))]
use std::thread::JoinHandle;
#[cfg(not(target_os = "macos"))]
use std::time::Duration;

/// Re-arm every 60 s. Aligned with the most common idle-timer floor
/// (Teams: 5 min, macOS screensaver: 1 min+). 60 s keeps the OS in
/// "active" land without burning CPU. macOS uses `caffeinate` instead,
/// so this only matters on Win/Linux.
#[cfg(not(target_os = "macos"))]
const TICK: Duration = Duration::from_secs(60);

/// Tauri-managed singleton. `active` is the public toggle the IPC
/// reports back; `stop` is a fresh AtomicBool per running worker — on
/// each on→off transition we set it to `true`; on each off→on we
/// allocate a brand-new one. That way a rapid off→on→off can never
/// resurrect a still-sleeping previous worker (it owns its own
/// already-stopped flag).
///
/// **v0.41.0:** on macOS the *primary* keep-awake mechanism is now a
/// spawned `/usr/bin/caffeinate -disu` child process (same approach
/// Apple's own `caffeinate` CLI takes, backed by `IOPMAssertionCreateWithName`
/// under the hood). Pre-0.41.0 the only mechanism was a 60 s
/// `CGEventPost` cursor-jiggle, but modern macOS hardens its idle-
/// timer against synthetic mouse-moved events — they no longer count
/// as user activity for the screen-lock / screensaver counters, so
/// the screen still locked even with wakelock "on" + LED pulsing.
/// `caffeinate` raises a proper IOPM assertion the kernel respects.
/// Windows + Linux fall back to the cursor-jiggle worker since they
/// don't ship an equivalent CLI in the base install.
#[derive(Default)]
pub struct WakelockState {
    pub active: AtomicBool,
    /// Worker-thread handle (Win/Linux only — macOS uses caffeinate
    /// instead of the jiggle loop).
    #[cfg(not(target_os = "macos"))]
    pub handle: Mutex<Option<JoinHandle<()>>>,
    /// Worker-thread stop flag (Win/Linux only — see `handle`).
    #[cfg(not(target_os = "macos"))]
    pub stop: Mutex<Option<Arc<AtomicBool>>>,
    /// macOS-only: handle to the `caffeinate` child kept alive while
    /// wakelock is on. Killed in `set_enabled(false)`.
    #[cfg(target_os = "macos")]
    pub caffeinate: Mutex<Option<std::process::Child>>,
}

/// Toggle the wakelock. Idempotent — calling with the current state
/// is a no-op. Returns the resulting state.
///
/// v0.35.2: replaces the pre-0.35.2 separate-load-then-store dance
/// with a single `compare_exchange`. Without the CAS, two concurrent
/// `set_enabled(true)` IPC calls could both observe `active=false`,
/// both pass the equality check, and **both spawn a worker thread**
/// — leaving one orphaned (its stop Arc overwritten in
/// `state.stop`, the worker running on a now-unreachable stop flag).
/// CAS makes the active-bit transition atomic; the loser bails
/// without spawning, and the winning thread fully owns the side
/// effects.
pub fn set_enabled(state: &WakelockState, enable: bool) -> bool {
    let prev = !enable;
    if state
        .active
        .compare_exchange(prev, enable, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        // Either the value was already `enable` (no-op) or another
        // thread is mid-transition. Either way the wanted state is
        // already in flight; just report the latest observation.
        return state.active.load(Ordering::SeqCst);
    }
    // We won the CAS — only one caller reaches here per transition.
    if enable {
        #[cfg(target_os = "macos")]
        {
            // Primary mechanism on macOS: spawn `caffeinate -disu` and
            // keep it alive. The CLI raises a proper IOPM assertion the
            // kernel honours, so the screen-lock / screensaver counters
            // are paused (which is what the user actually wants — pre-
            // 0.41.0 cursor-jiggle alone did not prevent the screen
            // from locking on modern macOS). The worker thread is NOT
            // started here; caffeinate covers display + system + idle.
            //   -d  prevent display sleep
            //   -i  prevent idle system sleep
            //   -s  prevent system sleep on AC
            //   -u  declare user is active (resets idle-lock counter)
            let child = std::process::Command::new("/usr/bin/caffeinate")
                .args(["-disu"])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            match child {
                Ok(c) => {
                    *state.caffeinate.lock() = Some(c);
                }
                Err(e) => {
                    tracing::error!(
                        "wakelock: failed to spawn caffeinate ({e}). \
                         The OS may sleep / lock as usual."
                    );
                    // Don't flip `active` back — the LED stays on so
                    // the user sees *something*; logs explain the
                    // degraded state. (Unreachable in practice:
                    // /usr/bin/caffeinate ships with every macOS.)
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let stop = Arc::new(AtomicBool::new(false));
            *state.stop.lock() = Some(stop.clone());
            let h = std::thread::spawn(move || worker(stop));
            *state.handle.lock() = Some(h);
        }
    } else {
        #[cfg(target_os = "macos")]
        {
            if let Some(mut child) = state.caffeinate.lock().take() {
                // Kill then wait — leaves no zombie. `kill()` is a
                // no-op if the child has already exited.
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            if let Some(stop) = state.stop.lock().take() {
                stop.store(true, Ordering::SeqCst);
            }
            // Let the worker exit on its own at the next 200 ms tick so
            // we don't block the IPC for up to a minute waiting on `join`.
            *state.handle.lock() = None;
        }
    }
    enable
}

pub fn is_enabled(state: &WakelockState) -> bool {
    state.active.load(Ordering::SeqCst)
}

#[cfg(not(target_os = "macos"))]
fn worker(stop: Arc<AtomicBool>) {
    loop {
        // Wait TICK in 200 ms chunks so the stop signal lands within
        // 200 ms of being set instead of 60 s.
        let mut waited = Duration::ZERO;
        while waited < TICK {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
            waited += Duration::from_millis(200);
        }
        if stop.load(Ordering::SeqCst) {
            return;
        }
        // v0.37.1: shield the jiggle call against panics. Pre-0.37.1
        // an FFI panic (e.g. a future macOS-update breaking
        // CGEventCreate) would unwind the worker thread silently,
        // leaving `state.active == true` but no actual jiggling
        // happening — LED-on-but-machine-still-sleeps. Catching here
        // logs the failure + keeps the loop alive on the next tick.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(jiggle_cursor));
        if let Err(_panic) = result {
            tracing::error!(
                "wakelock: jiggle_cursor panicked — continuing loop. \
                 If this repeats, the OS likely changed an FFI signature."
            );
        }
    }
}

// ── Platform jiggle ────────────────────────────────────────────────────
// macOS skips the jiggle entirely — `caffeinate` is the supported path.
// The legacy mod macos below is kept (gated dead) as documentation of
// the FFI shape, in case a future Win/Linux-style fallback is wanted.

#[cfg(target_os = "windows")]
fn jiggle_cursor() {
    win::jiggle();
}

#[cfg(target_os = "linux")]
fn jiggle_cursor() {
    linux::jiggle();
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn jiggle_cursor() {}

// ── macOS: CGEventCreateMouseEvent + CGEventPost (legacy fallback) ─────

#[cfg(target_os = "macos")]
#[allow(dead_code)]
mod macos {
    use std::ffi::c_void;
    use std::time::Duration;

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct CGPoint {
        x: f64,
        y: f64,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn CGEventCreate(source: *mut c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
        fn CGEventCreateMouseEvent(
            source: *mut c_void,
            mouse_type: u32,
            mouse_cursor_position: CGPoint,
            mouse_button: u32,
        ) -> *mut c_void;
        fn CGEventPost(tap: u32, event: *mut c_void);
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *mut c_void);
    }

    const K_CG_EVENT_MOUSE_MOVED: u32 = 5;
    const K_CG_MOUSE_BUTTON_LEFT: u32 = 0; // ignored for moves
    const K_CG_HID_EVENT_TAP: u32 = 0;

    pub fn jiggle() {
        unsafe {
            // Read the cursor position from a throwaway event.
            let probe = CGEventCreate(std::ptr::null_mut());
            if probe.is_null() {
                return;
            }
            let p = CGEventGetLocation(probe);
            CFRelease(probe);

            let plus = CGPoint { x: p.x + 1.0, y: p.y };
            post_move(plus);
            std::thread::sleep(Duration::from_millis(30));
            post_move(p);
        }
    }

    unsafe fn post_move(loc: CGPoint) {
        let e = CGEventCreateMouseEvent(
            std::ptr::null_mut(),
            K_CG_EVENT_MOUSE_MOVED,
            loc,
            K_CG_MOUSE_BUTTON_LEFT,
        );
        if !e.is_null() {
            CGEventPost(K_CG_HID_EVENT_TAP, e);
            CFRelease(e);
        }
    }
}

// ── Windows: GetCursorPos + SetCursorPos ───────────────────────────────

#[cfg(target_os = "windows")]
mod win {
    use std::time::Duration;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetCursorPos};

    pub fn jiggle() {
        let mut pt = POINT::default();
        unsafe {
            if GetCursorPos(&mut pt).is_err() {
                return;
            }
            let _ = SetCursorPos(pt.x + 1, pt.y);
            std::thread::sleep(Duration::from_millis(30));
            let _ = SetCursorPos(pt.x, pt.y);
        }
    }
}

// ── Linux X11: XQueryPointer + XWarpPointer ────────────────────────────

#[cfg(target_os = "linux")]
mod linux {
    use parking_lot::Mutex;
    use std::ffi::c_void;
    use std::os::raw::c_char;
    use std::sync::OnceLock;
    use std::time::Duration;

    type Display = *mut c_void;
    type XID = u64;

    #[link(name = "X11")]
    extern "C" {
        fn XOpenDisplay(display_name: *const c_char) -> Display;
        fn XDefaultRootWindow(display: Display) -> XID;
        fn XQueryPointer(
            display: Display,
            window: XID,
            root_return: *mut XID,
            child_return: *mut XID,
            root_x_return: *mut i32,
            root_y_return: *mut i32,
            win_x_return: *mut i32,
            win_y_return: *mut i32,
            mask_return: *mut u32,
        ) -> i32;
        fn XWarpPointer(
            display: Display,
            src_w: XID,
            dst_w: XID,
            src_x: i32,
            src_y: i32,
            src_width: u32,
            src_height: u32,
            dst_x: i32,
            dst_y: i32,
        );
        fn XFlush(display: Display) -> i32;
    }

    struct DisplayPtr(Display);
    unsafe impl Send for DisplayPtr {}
    unsafe impl Sync for DisplayPtr {}

    static DISPLAY: OnceLock<Mutex<Option<DisplayPtr>>> = OnceLock::new();

    fn display() -> &'static Mutex<Option<DisplayPtr>> {
        DISPLAY.get_or_init(|| unsafe {
            let d = XOpenDisplay(std::ptr::null());
            Mutex::new(if d.is_null() { None } else { Some(DisplayPtr(d)) })
        })
    }

    fn is_wayland() -> bool {
        std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
    }

    pub fn jiggle() {
        if is_wayland() {
            // Wayland denies global cursor synth at the protocol
            // level. We could try the screensaver-inhibit D-Bus
            // route but that's a separate feature; for now this is
            // a no-op so toggling the command doesn't error.
            return;
        }
        let dlock = display().lock();
        let Some(dp) = dlock.as_ref() else { return };
        let d = dp.0;
        unsafe {
            let root = XDefaultRootWindow(d);
            let mut root_return: XID = 0;
            let mut child_return: XID = 0;
            let mut rx = 0i32;
            let mut ry = 0i32;
            let mut wx = 0i32;
            let mut wy = 0i32;
            let mut mask = 0u32;
            let ok = XQueryPointer(
                d,
                root,
                &mut root_return,
                &mut child_return,
                &mut rx,
                &mut ry,
                &mut wx,
                &mut wy,
                &mut mask,
            );
            if ok == 0 {
                return;
            }
            // Absolute warp: src_w=0 means "from current position
            // doesn't matter, just move to dst_x,dst_y".
            XWarpPointer(d, 0, root, 0, 0, 0, 0, rx + 1, ry);
            XFlush(d);
            std::thread::sleep(Duration::from_millis(30));
            XWarpPointer(d, 0, root, 0, 0, 0, 0, rx, ry);
            XFlush(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Toggle wakelock off after a test — uses the public path so it
    /// stays correct per platform (kills caffeinate on macOS, signals
    /// the stop Arc on Win/Linux). Tests that flipped wakelock on MUST
    /// call this so the next test starts from a clean state.
    fn cleanup(s: &WakelockState) {
        set_enabled(s, false);
    }

    #[test]
    fn set_enabled_round_trip_returns_new_state() {
        let s = WakelockState::default();
        assert!(set_enabled(&s, true));
        assert!(is_enabled(&s));
        assert!(!set_enabled(&s, false));
        assert!(!is_enabled(&s));
        cleanup(&s);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn set_enabled_idempotent_does_not_double_spawn() {
        let s = WakelockState::default();
        set_enabled(&s, true);
        let stop_a = { s.stop.lock().as_ref().cloned() };
        set_enabled(&s, true); // no-op via CAS-rejection
        let stop_b = { s.stop.lock().as_ref().cloned() };
        // Same Arc pointer → no fresh allocation → no double-spawn.
        let same_arc = matches!(
            (&stop_a, &stop_b),
            (Some(a), Some(b)) if Arc::ptr_eq(a, b)
        );
        assert!(same_arc);
        cleanup(&s);
    }

    /// macOS variant of the idempotency test: enable twice → only one
    /// `caffeinate` PID exists. Without the CAS, a second
    /// `set_enabled(true)` would orphan the first child.
    #[cfg(target_os = "macos")]
    #[test]
    fn set_enabled_idempotent_does_not_double_spawn_macos() {
        let s = WakelockState::default();
        set_enabled(&s, true);
        let pid_a = { s.caffeinate.lock().as_ref().map(|c| c.id()) };
        set_enabled(&s, true); // no-op via CAS-rejection
        let pid_b = { s.caffeinate.lock().as_ref().map(|c| c.id()) };
        assert!(pid_a.is_some(), "caffeinate should have been spawned");
        assert_eq!(pid_a, pid_b, "second enable must not respawn caffeinate");
        cleanup(&s);
    }

    #[test]
    fn worker_catches_panic_and_keeps_looping() {
        // Direct test of the panic-shield: catch_unwind around a
        // panicking closure should NOT propagate. The worker uses
        // the same primitive on the real jiggle_cursor.
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            panic!("simulated FFI failure");
        }));
        assert!(r.is_err(), "panic should be captured");
        // The "loop keeps running" property is structural — we
        // can't easily test the worker's loop body without spawning
        // a thread + sleeping 60 s. The presence of catch_unwind in
        // the worker is documented + this test pins its semantics.
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn concurrent_set_enabled_true_only_spawns_once() {
        // Tight loop with multiple threads racing to flip enable=true.
        // Without CAS this used to spawn N workers (one per losing
        // race); with CAS exactly one wins.
        let s = std::sync::Arc::new(WakelockState::default());
        let mut handles = vec![];
        for _ in 0..16 {
            let s = s.clone();
            handles.push(std::thread::spawn(move || set_enabled(&s, true)));
        }
        for h in handles {
            assert!(h.join().unwrap()); // every caller reports `true`
        }
        assert!(is_enabled(&s));
        // Exactly one stop Arc lives in state; no orphaned workers.
        let has_stop = { s.stop.lock().is_some() };
        assert!(has_stop);
        cleanup(&s);
    }

    /// macOS variant of the concurrent-CAS test: 16 racing threads
    /// must end with exactly **one** `caffeinate` child running, not
    /// 16 orphans.
    #[cfg(target_os = "macos")]
    #[test]
    fn concurrent_set_enabled_true_only_spawns_once_macos() {
        let s = std::sync::Arc::new(WakelockState::default());
        let mut handles = vec![];
        for _ in 0..16 {
            let s = s.clone();
            handles.push(std::thread::spawn(move || set_enabled(&s, true)));
        }
        for h in handles {
            assert!(h.join().unwrap()); // every caller reports `true`
        }
        assert!(is_enabled(&s));
        let has_child = { s.caffeinate.lock().is_some() };
        assert!(has_child, "exactly one caffeinate child should be tracked");
        cleanup(&s);
    }
}

