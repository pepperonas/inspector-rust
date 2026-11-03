//! The app's side of the `ir-transcribe` helper: one long-lived process,
//! started on first use, commands as JSON lines on its stdin, events fanned
//! out to subscribers. whisper.cpp only ever runs in that process — an abort
//! inside it ends the helper, never the app.

use parking_lot::Mutex;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command as Proc, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use transcribe_protocol::{decode_event, encode, Command, Event};

pub const HELPER: &str = "ir-transcribe";
pub const ERR_NO_HELPER: &str = "transcribe.no_helper";
pub const ERR_HELPER_DIED: &str = "transcribe.helper_died";
pub const ERR_UNSUPPORTED: &str = "transcribe.unsupported";

type Listener = Arc<dyn Fn(&Event) + Send + Sync>;

struct Running {
    child: Child,
    stdin: ChildStdin,
    /// Which helper this is: a reader thread only ever cleans up its own.
    gen: u64,
}

static HELPER_PROC: Mutex<Option<Running>> = Mutex::new(None);
static LISTENERS: Mutex<Vec<(u64, Listener)>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(1);
/// A session (or file job) is running in the helper.
static BUSY: AtomicBool = AtomicBool::new(false);
/// A file job was sent and has not answered yet.
static FILE_BUSY: AtomicBool = AtomicBool::new(false);
/// A `Start` was sent and neither `Started` nor a refusal came back yet.
static STARTING: AtomicBool = AtomicBool::new(false);
static GEN: AtomicU64 = AtomicU64::new(1);

/// Transcription runs on Apple Silicon Macs only (whisper.cpp on Metal).
pub fn supported() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
}

/// `Contents/MacOS/ir-transcribe`, or `IR_TRANSCRIBE_HELPER` (development).
pub fn helper_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("IR_TRANSCRIBE_HELPER") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.join(HELPER);
    p.is_file().then_some(p)
}

fn dispatch(ev: &Event) {
    match ev {
        Event::Started { .. } => {
            STARTING.store(false, Ordering::SeqCst);
            BUSY.store(true, Ordering::SeqCst)
        }
        Event::Stopped { .. } => {
            STARTING.store(false, Ordering::SeqCst);
            BUSY.store(false, Ordering::SeqCst)
        }
        Event::Error { .. } => STARTING.store(false, Ordering::SeqCst),
        Event::FileDone { .. } | Event::FileWords { .. } | Event::FileError { .. } => FILE_BUSY.store(false, Ordering::SeqCst),
        _ => {}
    }
    let listeners: Vec<Listener> = LISTENERS.lock().iter().map(|(_, l)| l.clone()).collect();
    for l in listeners {
        l(ev);
    }
}

fn spawn() -> Result<Running, String> {
    if !supported() {
        return Err(ERR_UNSUPPORTED.into());
    }
    let path = helper_path().ok_or(ERR_NO_HELPER)?;
    let mut child = Proc::new(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let gen = GEN.fetch_add(1, Ordering::SeqCst);
    std::thread::Builder::new()
        .name("ir-transcribe-client".into())
        .spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                match decode_event(&line) {
                    Ok(ev) => dispatch(&ev),
                    Err(e) => tracing::warn!("transcribe: bad event from helper: {e}"),
                }
            }
            // The helper is gone. A session that was running ends with an error.
            tracing::warn!("transcribe: helper exited");
            let (dead, ours) = take_if_current(&mut HELPER_PROC.lock(), gen);
            if let Some(mut r) = dead {
                let _ = r.child.wait(); // outside the lock
            }
            if !ours {
                return; // a newer helper already runs; its state is not ours
            }
            if BUSY.swap(false, Ordering::SeqCst) || STARTING.load(Ordering::SeqCst) {
                dispatch(&Event::Stopped { session_dir: None, error: Some(ERR_HELPER_DIED.into()) });
            }
            if FILE_BUSY.load(Ordering::SeqCst) {
                dispatch(&Event::FileError { error: ERR_HELPER_DIED.into() });
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Running { child, stdin, gen })
}

/// The exiting reader of helper `gen` takes the process out only if it is
/// still the current one. Returns it and whether the exit concerns the current
/// helper (or none is running any more). Pure over the slot.
fn take_if_current(slot: &mut Option<Running>, gen: u64) -> (Option<Running>, bool) {
    match slot.as_ref().map(|r| r.gen) {
        Some(g) if g == gen => (slot.take(), true),
        Some(_) => (None, false),
        None => (None, true),
    }
}

/// Send one command, starting the helper if needed.
pub fn send(cmd: &Command) -> Result<(), String> {
    let mut guard = HELPER_PROC.lock();
    if guard.is_none() {
        *guard = Some(spawn()?);
    }
    let r = guard.as_mut().expect("just set");
    let line = encode(cmd);
    // Set before writing: the answer can arrive before `write_all` returns.
    let file = matches!(cmd, Command::TranscribeFile { .. } | Command::TranscribeWords { .. });
    if file {
        FILE_BUSY.store(true, Ordering::SeqCst);
    }
    if matches!(cmd, Command::Start(_)) {
        STARTING.store(true, Ordering::SeqCst);
    }
    if let Err(e) = r.stdin.write_all(line.as_bytes()).and_then(|_| r.stdin.flush()) {
        if file {
            FILE_BUSY.store(false, Ordering::SeqCst);
        }
        STARTING.store(false, Ordering::SeqCst);
        // A dead pipe: drop it; the next call starts a fresh helper.
        if let Some(mut dead) = guard.take() {
            let _ = dead.child.kill();
        }
        return Err(format!("{ERR_HELPER_DIED}: {e}"));
    }
    Ok(())
}

pub fn subscribe(f: impl Fn(&Event) + Send + Sync + 'static) -> u64 {
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    LISTENERS.lock().push((id, Arc::new(f)));
    id
}

pub fn unsubscribe(id: u64) {
    LISTENERS.lock().retain(|(i, _)| *i != id);
}

/// Send `cmd` and wait for the first event `pick` accepts.
pub fn request<T: Send + 'static>(
    cmd: &Command,
    timeout: Duration,
    pick: impl Fn(&Event) -> Option<T> + Send + Sync + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let id = subscribe(move |ev| {
        if let Some(v) = pick(ev) {
            let _ = tx.lock().send(v);
        }
    });
    let r = send(cmd).and_then(|_| rx.recv_timeout(timeout).map_err(|_| "transcribe.timeout".to_string()));
    unsubscribe(id);
    r
}

pub fn is_busy() -> bool {
    BUSY.load(Ordering::SeqCst) || FILE_BUSY.load(Ordering::SeqCst)
}

/// Kill a helper that did not exit within the grace time on app exit? Not
/// while it finishes a session: it decodes the queued segments, writes them
/// and marks the session completed on its own (the same path as an app
/// `kill -9`). Pure.
pub fn kill_on_exit(session_or_file_busy: bool) -> bool {
    !session_or_file_busy
}

/// App exit: end the helper (it closes a running session cleanly on EOF).
pub fn shutdown() {
    let busy = is_busy();
    if let Some(mut r) = HELPER_PROC.lock().take() {
        let _ = r.stdin.write_all(encode(&Command::Shutdown).as_bytes());
        drop(r.stdin);
        for _ in 0..40 {
            if matches!(r.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if kill_on_exit(busy) {
            let _ = r.child.kill();
        } else {
            tracing::info!("transcribe: helper still finishing the session — left to end on its own");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_follows_started_and_stopped() {
        dispatch(&Event::Started { session_dir: "/x".into(), sources: vec![] });
        assert!(is_busy());
        dispatch(&Event::Stopped { session_dir: None, error: None });
        assert!(!is_busy());
    }

    fn running(gen: u64) -> Running {
        let mut child = Proc::new("/bin/cat").stdin(Stdio::piped()).spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        Running { child, stdin, gen }
    }

    #[test]
    fn an_old_reader_never_takes_the_new_helper() {
        let mut slot = Some(running(7));
        let (taken, ours) = take_if_current(&mut slot, 6);
        assert!(taken.is_none() && !ours, "the reader of helper 6 leaves helper 7 alone");
        assert!(slot.is_some());
        let (taken, ours) = take_if_current(&mut slot, 7);
        assert!(ours);
        let mut r = taken.unwrap();
        assert!(slot.is_none());
        let _ = r.child.kill();
        let _ = r.child.wait();
        // A helper that send() already dropped: its reader still reports.
        assert!(take_if_current(&mut None, 3).1);
    }

    #[test]
    fn a_helper_finishing_a_session_is_not_killed_on_exit() {
        assert!(!kill_on_exit(true));
        assert!(kill_on_exit(false));
    }

    #[test]
    fn listeners_get_events_until_unsubscribed() {
        let n = Arc::new(AtomicU64::new(0));
        let n2 = n.clone();
        let id = subscribe(move |e| {
            if matches!(e, Event::Devices { .. }) {
                n2.fetch_add(1, Ordering::SeqCst);
            }
        });
        dispatch(&Event::Devices { mic: vec![] });
        unsubscribe(id);
        dispatch(&Event::Devices { mic: vec![] });
        assert_eq!(n.load(Ordering::SeqCst), 1);
    }
}
