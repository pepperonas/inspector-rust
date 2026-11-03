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
}

static HELPER_PROC: Mutex<Option<Running>> = Mutex::new(None);
static LISTENERS: Mutex<Vec<(u64, Listener)>> = Mutex::new(Vec::new());
static NEXT: AtomicU64 = AtomicU64::new(1);
/// A session (or file job) is running in the helper.
static BUSY: AtomicBool = AtomicBool::new(false);

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
        Event::Started { .. } => BUSY.store(true, Ordering::SeqCst),
        Event::Stopped { .. } | Event::FileDone { .. } | Event::FileError { .. } => BUSY.store(false, Ordering::SeqCst),
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
            if let Some(mut r) = HELPER_PROC.lock().take() {
                let _ = r.child.wait();
            }
            if BUSY.swap(false, Ordering::SeqCst) {
                dispatch(&Event::Stopped { session_dir: None, error: Some(ERR_HELPER_DIED.into()) });
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Running { child, stdin })
}

/// Send one command, starting the helper if needed.
pub fn send(cmd: &Command) -> Result<(), String> {
    let mut guard = HELPER_PROC.lock();
    if guard.is_none() {
        *guard = Some(spawn()?);
    }
    let r = guard.as_mut().expect("just set");
    let line = encode(cmd);
    if let Err(e) = r.stdin.write_all(line.as_bytes()).and_then(|_| r.stdin.flush()) {
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
    BUSY.load(Ordering::SeqCst)
}

/// App exit: end the helper (it closes a running session cleanly on EOF).
pub fn shutdown() {
    if let Some(mut r) = HELPER_PROC.lock().take() {
        let _ = r.stdin.write_all(encode(&Command::Shutdown).as_bytes());
        drop(r.stdin);
        for _ in 0..40 {
            if matches!(r.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = r.child.kill();
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
