//! Linux touchpad gesture capture via **libinput** (the `input` crate).
//!
//! libinput already classifies multi-finger gestures, so this is the simpler
//! path: we filter `GESTURE_SWIPE_*` to 3-finger, accumulate `dx`/`dy` over the
//! Update phase, and classify the total at `End` via the shared
//! [`classify_swipe`]; a short, non-cancelled 3-finger `GESTURE_HOLD` maps to a
//! Tap. 3-finger swipe up/down → volume, tap → mute (same bindings as macOS /
//! Windows).
//!
//! **Gesture guard (Phase 6):** libinput hands out no per-contact data, so the
//! guard's levels 1–2 are libinput's own: on every touchpad that offers them
//! we switch on **disable-while-typing** (DWT) and **disable-while-trackpointing**
//! (DWTP) — in OUR libinput context only, the desktop's own context and its
//! cursor stay untouched; palm detection is always on in libinput. Level 3
//! gets every key press of the same context (time + shortcut flag, never the
//! key — Linux has no keyboard hook of the app's own to borrow), level 4 gets
//! libinput's "cancelled" swipes: libinput cancels a gesture when the finger
//! count changes, which is exactly the constant-count rule.
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

use super::keys::EvdevKeys;
use super::{classify_swipe, ExternalGuard, GestureConfig, GestureEvent, GestureKind, GestureSink, GestureSource};
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use input::event::gesture::{
    GestureEndEvent, GestureEventCoordinates, GestureEventTrait, GestureHoldEvent,
    GestureSwipeEvent,
};
use input::event::device::DeviceEvent;
use input::event::keyboard::{KeyState, KeyboardEvent, KeyboardEventTrait};
use input::event::{Event, EventTrait};
use input::{DeviceCapability, Libinput, LibinputInterface};

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
    fn start(&mut self, cfg: GestureConfig, sink: GestureSink) -> Result<(), String> {
        if RUNNING.swap(true, Ordering::SeqCst) {
            return Ok(()); // already running
        }
        // libinput recognises the gestures; they still pass the guard's typing
        // level, plausibility, cooldown and config.
        let guard = ExternalGuard::new(cfg, sink);
        std::thread::spawn(move || {
            run_loop(guard);
            RUNNING.store(false, Ordering::SeqCst);
        });
        Ok(())
    }

    fn stop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
        // The poll loop wakes on its 200 ms timeout and exits.
    }
}

fn run_loop(mut guard: ExternalGuard) {
    let mut li = Libinput::new_with_udev(Interface);
    if li.udev_assign_seat("seat0").is_err() {
        tracing::warn!("gestures: libinput could not assign seat0 (no /dev/input access?)");
        return;
    }
    let fd = li.as_raw_fd();

    // Per-gesture accumulators (single-threaded loop, so locals suffice).
    let (mut acc_dx, mut acc_dy) = (0.0f64, 0.0f64);
    let mut hold_start: Option<Instant> = None;
    let mut keys = EvdevKeys::default();

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
            let g = match event {
                Event::Device(DeviceEvent::Added(a)) => {
                    let dev = a.device();
                    if dev.has_capability(DeviceCapability::Gesture) {
                        enable_typing_guards(&dev);
                    }
                    continue;
                }
                Event::Keyboard(KeyboardEvent::Key(k)) => {
                    if let Some(shortcut) = keys.on_key(k.key(), k.key_state() == KeyState::Pressed) {
                        guard.key(shortcut);
                    }
                    continue;
                }
                Event::Gesture(g) => g,
                _ => continue,
            };
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
                            let ev = GestureEvent { kind, fingers };
                            if e.cancelled() {
                                // libinput cancels when the finger count changes.
                                guard.count_changed(ev);
                            } else {
                                guard.event(ev);
                            }
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
                            guard.event(GestureEvent { kind: GestureKind::Tap, fingers });
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

/// Switch on libinput's own typing guards for a touchpad in our context.
fn enable_typing_guards(dev: &input::Device) {
    if dev.config_dwt_is_available() {
        if let Err(e) = dev.config_dwt_set_enabled(true) {
            tracing::debug!("gestures: DWT on {}: {e:?}", dev.name());
        }
    }
    if dev.config_dwtp_is_available() {
        if let Err(e) = dev.config_dwtp_set_enabled(true) {
            tracing::debug!("gestures: DWTP on {}: {e:?}", dev.name());
        }
    }
    tracing::info!(
        "gestures: touchpad {} — disable-while-typing {}, while-trackpointing {}",
        dev.name(),
        dev.config_dwt_is_available(),
        dev.config_dwtp_is_available()
    );
}
