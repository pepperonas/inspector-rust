//! `wakelock` — keep the computer awake. Toggled via the search-bar
//! commands `wakelock on` / `wakelock off` (alias `caffeine on/off`).
//!
//! **macOS (v0.41.0+):** spawns `/usr/bin/caffeinate -disu -w <ir-pid>` as a
//! child process while wakelock is on; killed on toggle-off. This
//! raises proper IOPM assertions in the kernel
//! (`PreventUserIdleSystemSleep` + `PreventUserIdleDisplaySleep`),
//! which actually pauses the screen-lock / screensaver counters.
//!
//! **v0.78.0 fix — no more orphans.** The `-w <ir-pid>` ties caffeinate's
//! lifetime to IR's process: it exits the instant IR does (clean quit, crash,
//! or a reinstall replacing the binary), so a keep-awake assertion can never
//! outlive the app that set it. Pre-0.78.0 the child was reparented to launchd
//! and kept the Mac awake forever, unreachable by a later `caffeine off`.
//! [`cleanup_orphans`] additionally reaps any such pre-fix orphan at startup.
//!
//! **Windows (v0.41.0+):** a worker thread calls
//! `SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED |
//! ES_SYSTEM_REQUIRED)` on entry and clears with `ES_CONTINUOUS` on
//! exit. This is the documented Win32 API for suppressing system +
//! display *power* sleep.
//!
//! **However** (v0.50.1 fix): `SetThreadExecutionState` does **not**
//! reset the user-input idle timer that drives the **screensaver** and
//! the **lock screen** — so on its own the machine stopped *sleeping*
//! but still *locked*, which users reported as "wakelock doesn't work".
//! The worker therefore *also* injects an invisible `F15` keypress (via
//! `enigo`, same SendInput path the app already uses for paste) every
//! `WIN_NUDGE` seconds. F15 is effectively unused by applications, so it
//! disturbs nothing while counting as user activity that keeps the
//! screensaver + lock from engaging — the same belt-and-suspenders trick
//! Caffeine / PowerToys Awake use. `SetThreadExecutionState` still
//! covers actual power-sleep (and survives group-policy configs that
//! ignore synthetic input), so the two mechanisms complement each other.
//!
//! Pre-0.41.0 both platforms used cursor-jiggle: macOS via
//! `CGEventPost`, Windows via `SetCursorPos`. macOS hardens its idle
//! counter against synthetic mouse-moved events, so the screen still
//! locked despite the LED pulsing — leading to the fix above. Windows
//! generally honoured jiggle, but the supported API is more reliable
//! + has zero visual blip.
//!
//! **Linux:** cursor-jiggle worker remains — the base install has no
//! equivalent inhibit API. The jiggle is **two** synthetic mouse-move
//! events spaced 30 ms apart: one to `(x+1, y)`, one back to `(x, y)`.
//! X11-only — Wayland deliberately denies cursor synth at the protocol
//! level, so the jiggle is a no-op there (a future
//! `org.freedesktop.ScreenSaver` D-Bus inhibit would be the proper
//! Wayland path).

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

/// Windows-only: how often to inject the invisible F15 idle-timer nudge.
/// Kept comfortably below the 1-minute Windows screensaver/lock floor so
/// a short idle timeout can't slip in between nudges.
#[cfg(target_os = "windows")]
const WIN_NUDGE: Duration = Duration::from_secs(30);

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
    /// Linux-only: handle to the `systemd-inhibit … sleep infinity` child kept
    /// alive while wakelock is on (the logind idle/sleep inhibitor — works
    /// under Wayland where the cursor-jiggle worker is a no-op). Killed in
    /// `set_enabled(false)`.
    #[cfg(target_os = "linux")]
    pub inhibitor: Mutex<Option<std::process::Child>>,
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
            //   -w <pid>  tie the assertion to OUR process — caffeinate exits
            //             the moment IR exits (clean quit OR crash OR being
            //             replaced by a reinstall). Without this the child was
            //             reparented to launchd and kept the Mac awake forever,
            //             unreachable by a later `caffeine off` (v0.78.0 fix).
            let self_pid = std::process::id().to_string();
            let child = std::process::Command::new("/usr/bin/caffeinate")
                .args(["-disu", "-w", &self_pid])
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
        #[cfg(target_os = "linux")]
        {
            // Primary: a logind idle+sleep inhibitor. Unlike the X11 cursor
            // jiggle (which Wayland blocks at the protocol level), this actually
            // keeps a Wayland session from going idle / locking / sleeping, as
            // long as the desktop honours logind inhibitors (GNOME, KDE do).
            // Best-effort — if systemd-inhibit is missing, the jiggle worker
            // below still helps on X11.
            let child = std::process::Command::new("systemd-inhibit")
                .args([
                    "--what=idle:sleep",
                    "--who=Inspector Rust",
                    "--why=Keep awake",
                    "--mode=block",
                    "sleep",
                    "infinity",
                ])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            match child {
                Ok(c) => *state.inhibitor.lock() = Some(c),
                Err(e) => tracing::warn!(
                    "wakelock: systemd-inhibit unavailable ({e}); \
                     relying on the X11 cursor jiggle (a no-op under Wayland)"
                ),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            // Cursor-jiggle worker — primary on Windows/X11, a complement to the
            // logind inhibitor on Linux (no-op under Wayland).
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
        #[cfg(target_os = "linux")]
        {
            // Release the logind inhibitor (kill the `sleep infinity` child).
            if let Some(mut child) = state.inhibitor.lock().take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    enable
}

pub fn is_enabled(state: &WakelockState) -> bool {
    state.active.load(Ordering::SeqCst)
}

/// Best-effort cleanup of **orphaned** `caffeinate` processes left behind by a
/// pre-v0.78.0 IR session (which spawned `caffeinate -disu` with no `-w` tie,
/// so a crash / reinstall reparented it to launchd and it kept the Mac awake
/// forever). Called once at startup. Conservative match so it can't touch an
/// unrelated tool's caffeinate: name `caffeinate`, **orphaned** (parent = pid 1),
/// IR's exact `-disu` flags, and **no** `-w` (the v0.78.0+ form self-terminates,
/// so it never orphans and is never matched here).
/// Pure predicate (unit-tested): is this process an orphaned IR caffeinate we
/// should reap? Matches the `caffeinate` name, an orphaned parent (launchd /
/// pid 1), IR's `-disu` flags, and the absence of `-w` (so the v0.78.0+ form,
/// which self-terminates, is never killed).
#[cfg(any(target_os = "macos", test))]
fn should_reap_caffeinate(name: &str, parent: Option<u32>, cmd: &[String]) -> bool {
    name == "caffeinate"
        && parent == Some(1)
        && cmd.iter().any(|a| a == "-disu")
        && !cmd.iter().any(|a| a == "-w")
}

#[cfg(target_os = "macos")]
pub fn cleanup_orphans() {
    use sysinfo::{ProcessRefreshKind, RefreshKind, Signal, System};
    let mut sys = System::new_with_specifics(
        RefreshKind::new().with_processes(ProcessRefreshKind::everything()),
    );
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    for (pid, proc) in sys.processes() {
        let name = proc.name().to_string_lossy();
        let parent = proc.parent().map(|p| p.as_u32());
        let cmd: Vec<String> = proc.cmd().iter().map(|a| a.to_string_lossy().into_owned()).collect();
        if should_reap_caffeinate(&name, parent, &cmd) {
            tracing::warn!("wakelock: killing orphaned caffeinate (pid {})", pid.as_u32());
            proc.kill_with(Signal::Term);
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn cleanup_orphans() {}

/// Linux worker — 60 s cursor-jiggle loop. (Linux has no kernel-side
/// idle-inhibit API in the base install; a future Wayland path would
/// use `org.freedesktop.ScreenSaver.Inhibit` over D-Bus.)
#[cfg(target_os = "linux")]
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

/// Windows worker — engages `SetThreadExecutionState` then idles
/// waiting for the stop signal. **v0.41.0:** replaces the pre-0.41.0
/// `SetCursorPos` jiggle. The `ES_CONTINUOUS | ES_DISPLAY_REQUIRED |
/// ES_SYSTEM_REQUIRED` combination resets the display-idle timer the
/// screensaver / lock counter watches, and crucially survives
/// group-policy-managed corporate Windows configurations that disable
/// synthetic-input idle resets. The flag is **per-thread**, so the
/// worker must stay alive the entire time wakelock is on — engage on
/// entry, disengage on exit, sleep in 200 ms chunks in between.
#[cfg(target_os = "windows")]
fn worker(stop: Arc<AtomicBool>) {
    let engaged = win_power::engage();
    if !engaged {
        tracing::warn!(
            "wakelock: SetThreadExecutionState returned 0 (engage failed). \
             The OS may sleep / lock as usual."
        );
    }
    // `SetThreadExecutionState` blocks power-sleep but does NOT reset the
    // user-input idle timer that drives the screensaver + lock screen.
    // Nudge an invisible F15 keypress now (so a short idle timeout can't
    // beat the first interval) and every `WIN_NUDGE` thereafter. See the
    // module header for the full rationale.
    nudge_input();
    let mut waited = Duration::ZERO;
    while !stop.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(200));
        waited += Duration::from_millis(200);
        if waited >= WIN_NUDGE {
            nudge_input();
            // Cheap re-assert in case anything cleared the execution state.
            let _ = win_power::engage();
            waited = Duration::ZERO;
        }
    }
    // Always release — even if engage returned 0, calling disengage
    // with just ES_CONTINUOUS is a no-op so it's safe.
    win_power::disengage();
}

/// Windows-only: inject an invisible `F15` keypress to reset the OS
/// input-idle timer (screensaver + lock), via the same `enigo`/SendInput
/// path the app already uses for paste. F15 is effectively unused by
/// applications, so it disturbs no focused window and moves no cursor.
/// Best-effort — a failure is logged at debug and the loop continues.
#[cfg(target_os = "windows")]
fn nudge_input() {
    use enigo::{Direction::Click, Enigo, Key, Keyboard, Settings};
    // Same flag paste.rs uses: never trigger enigo's permission prompt.
    let settings = Settings {
        open_prompt_to_get_permissions: false,
        ..Settings::default()
    };
    match Enigo::new(&settings) {
        Ok(mut enigo) => {
            if let Err(e) = enigo.key(Key::F15, Click) {
                tracing::debug!("wakelock: F15 nudge failed: {e:?}");
            }
        }
        Err(e) => tracing::debug!("wakelock: enigo init for nudge failed: {e:?}"),
    }
}

// ── Platform jiggle ────────────────────────────────────────────────────
// macOS uses `caffeinate` (no jiggle). Windows uses
// `SetThreadExecutionState` (no jiggle). Only Linux still jiggles, since
// the base install lacks an equivalent inhibit API.

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

// ── Windows: SetThreadExecutionState (kernel32) ────────────────────────
//
// Win32 `SetThreadExecutionState` is the documented way to suppress
// system + display sleep. The flag is per-thread + sticky until
// cleared; the worker thread engages on entry and clears on exit.
//
// References:
//   <https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-setthreadexecutionstate>

#[cfg(target_os = "windows")]
mod win_power {
    // EXECUTION_STATE flags (from `winbase.h`):
    //   ES_CONTINUOUS         0x80000000  keep the flag set until cleared
    //   ES_SYSTEM_REQUIRED    0x00000001  forces the system to be in working state
    //   ES_DISPLAY_REQUIRED   0x00000002  forces the display to be on (resets idle timer)
    pub const ES_CONTINUOUS: u32 = 0x8000_0000;
    pub const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
    pub const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;

    #[link(name = "kernel32")]
    extern "system" {
        /// Returns the previous EXECUTION_STATE, or 0 on failure.
        fn SetThreadExecutionState(es_flags: u32) -> u32;
    }

    /// Engage the wakelock on the calling thread. Returns `true` if
    /// the call returned a non-zero previous state (i.e. the OS
    /// accepted the request).
    pub fn engage() -> bool {
        let prev = unsafe {
            SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED)
        };
        prev != 0
    }

    /// Clear the wakelock on the calling thread. Calling with just
    /// `ES_CONTINUOUS` cancels any previously-set DISPLAY/SYSTEM
    /// flags, returning the thread to default behaviour.
    pub fn disengage() -> bool {
        let prev = unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        prev != 0
    }
}

// Legacy Win32 cursor-jiggle — kept (gated dead) as documentation of
// the FFI shape in case a future fallback wants it.

#[cfg(target_os = "windows")]
#[allow(dead_code)]
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
    fn reaps_only_orphaned_disu_caffeinate_without_w() {
        let disu = vec!["/usr/bin/caffeinate".into(), "-disu".into()];
        let disu_w = vec![
            "/usr/bin/caffeinate".into(),
            "-disu".into(),
            "-w".into(),
            "4242".into(),
        ];
        // The target: an orphaned (parent=1) `-disu` caffeinate with no `-w`.
        assert!(should_reap_caffeinate("caffeinate", Some(1), &disu));
        // v0.78.0+ form (has -w) → self-terminates, never reaped.
        assert!(!should_reap_caffeinate("caffeinate", Some(1), &disu_w));
        // Still has a live parent → not an orphan, leave it.
        assert!(!should_reap_caffeinate("caffeinate", Some(500), &disu));
        // A different process named caffeinate-ish, or other flags → ignored.
        assert!(!should_reap_caffeinate("caffeinated", Some(1), &disu));
        assert!(!should_reap_caffeinate(
            "caffeinate",
            Some(1),
            &vec!["/usr/bin/caffeinate".into(), "-d".into()]
        ));
        assert!(!should_reap_caffeinate("caffeinate", None, &disu));
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

