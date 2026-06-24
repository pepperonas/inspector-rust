//! Linux touchpad gesture capture via **libinput** (the `input` crate).
//!
//! libinput already classifies multi-finger gestures, so this is the simpler
//! path: we filter `GESTURE_SWIPE_*` to 3-finger, accumulate `dx`/`dy` over the
//! Update phase, and classify the total at `End` via the shared
//! [`classify_swipe`]; a short, non-cancelled 3-finger `GESTURE_HOLD` maps to a
//! Tap. 3-finger swipe up/down → volume, tap → mute (same bindings as macOS /
//! Windows).
//!
//! **Access:** reading `/dev/input/event*` needs the user in the `input` group
//! (or a logind seat). Without it, libinput opens no devices and gestures
//! silently don't fire — see `linux/README.md`.
//!
//! **Wayland/GNOME caveat:** the compositor often *also* owns the 3-finger
//! swipe (workspace switch / overview). libinput events still arrive here, but
//! the desktop reacts in parallel — disable the desktop gesture if it conflicts.
//!
//! **Unverified:** the `input` crate links libinput via pkg-config, so this
//! module can't be built or run from the maintainer's macOS host. Written
//! against the documented `input` 0.9 API; verify on real Linux.

use super::{classify_swipe, GestureConfig, GestureEvent, GestureKind, GestureSink, GestureSource};
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use input::event::gesture::{
    GestureEndEvent, GestureEventCoordinates, GestureEventTrait, GestureHoldEvent,
    GestureSwipeEvent,
};
use input::event::Event;
use input::{Libinput, LibinputInterface};

/// A 3-finger swipe must accumulate at least this much libinput motion (in its
/// device-independent units) to count — separate from the normalized Windows
/// threshold since libinput's units differ.
const LINUX_SWIPE_THRESHOLD: f64 = 50.0;
/// A 3-finger hold counts as a Tap only if it lifts within this long.
const HOLD_TAP_MAX_MS: u128 = 300;

/// libinput device-open hook. Rootless: open the evdev node directly (works
/// when the user is in the `input` group); logind seats would route through
/// `open_restricted` too.
struct Interface;

impl LibinputInterface for Interface {
    fn open_restricted(&mut self, path: &Path, flags: i32) -> Result<OwnedFd, i32> {
        use std::os::unix::ffi::OsStrExt;
        let cstr = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| libc::EINVAL)?;
        // SAFETY: cstr is a valid NUL-terminated path; flags come from libinput.
        let fd = unsafe { libc::open(cstr.as_ptr(), flags) };
        if fd < 0 {
            Err(unsafe { *libc::__errno_location() })
        } else {
            // SAFETY: fd is a fresh, owned, valid descriptor.
            Ok(unsafe { OwnedFd::from_raw_fd(fd) })
        }
    }
    fn close_restricted(&mut self, fd: OwnedFd) {
        // SAFETY: we own fd; hand it back to close().
        unsafe {
            libc::close(fd.into_raw_fd());
        }
    }
}

static RUNNING: AtomicBool = AtomicBool::new(false);

pub struct LinuxGestureSource;

impl LinuxGestureSource {
    pub fn new() -> Self {
        LinuxGestureSource
    }
}

impl GestureSource for LinuxGestureSource {
    fn start(&mut self, _cfg: GestureConfig, sink: GestureSink) -> Result<(), String> {
        if RUNNING.swap(true, Ordering::SeqCst) {
            return Ok(()); // already running
        }
        std::thread::spawn(move || {
            run_loop(sink);
            RUNNING.store(false, Ordering::SeqCst);
        });
        Ok(())
    }

    fn stop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
        // The poll loop wakes on its 200 ms timeout and exits.
    }
}

fn run_loop(sink: GestureSink) {
    let mut li = Libinput::new_with_udev(Interface);
    if li.udev_assign_seat("seat0").is_err() {
        tracing::warn!("gestures: libinput could not assign seat0 (no /dev/input access?)");
        return;
    }
    let fd = li.as_raw_fd();

    // Per-gesture accumulators (single-threaded loop, so locals suffice).
    let (mut acc_dx, mut acc_dy) = (0.0f64, 0.0f64);
    let mut hold_start: Option<Instant> = None;

    while RUNNING.load(Ordering::Relaxed) {
        // Block up to 200 ms for input, then re-check RUNNING so stop() is prompt.
        let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let r = unsafe { libc::poll(&mut pfd, 1, 200) };
        if r <= 0 {
            continue;
        }
        if li.dispatch().is_err() {
            continue;
        }
        for event in &mut li {
            let Event::Gesture(g) = event else { continue };
            match g {
                input::event::GestureEvent::Swipe(s) => match s {
                    GestureSwipeEvent::Begin(_) => {
                        acc_dx = 0.0;
                        acc_dy = 0.0;
                    }
                    GestureSwipeEvent::Update(u) => {
                        acc_dx += u.dx();
                        acc_dy += u.dy();
                    }
                    GestureSwipeEvent::End(e) => {
                        let fingers = e.finger_count() as u8;
                        // libinput y grows downward → dy < 0 = up, matches classify_swipe.
                        if let Some(kind) = classify_swipe(acc_dx, acc_dy, LINUX_SWIPE_THRESHOLD) {
                            sink(GestureEvent { kind, fingers });
                        }
                        acc_dx = 0.0;
                        acc_dy = 0.0;
                    }
                    _ => {}
                },
                input::event::GestureEvent::Hold(h) => match h {
                    GestureHoldEvent::Begin(_) => hold_start = Some(Instant::now()),
                    GestureHoldEvent::End(e) => {
                        let quick = hold_start
                            .map(|t| t.elapsed().as_millis() <= HOLD_TAP_MAX_MS)
                            .unwrap_or(false);
                        if !e.cancelled() && quick {
                            let fingers = e.finger_count() as u8;
                            sink(GestureEvent { kind: GestureKind::Tap, fingers });
                        }
                        hold_start = None;
                    }
                    _ => {}
                },
                _ => {}
            }
        }
    }
}
