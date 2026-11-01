//! The app side of the `ir-record-render` helper process (ruling R4/R11).
//!
//! One [`RenderClient`] is one helper process. It owns the child, a reader thread that sorts
//! the helper's frames into replies (to the request waiting for them) and notices (export
//! progress, death), and a stderr thread that forwards the helper's log lines to `tracing`
//! (target `record::render`).
//!
//! Requests are serialised: the helper answers in command order, so at most one request is in
//! flight. A request that gets no reply within its timeout, or a helper that closes its stdout,
//! makes the client `Dead { reason }`: the child is killed and reaped (no zombie), every waiting
//! and later request fails, and the sink hears [`Notice::Dead`] exactly once. The caller drops a
//! dead client and spawns a new one on the next command.
//!
//! Platform-free (std processes only), so the crash/timeout tests run with a shell-script
//! stand-in for the helper on any Unix.

use record_render_protocol::{
    decode_event, decode_preview, decode_thumb, read_frame, write_command, Command, Event, ExportKind,
    PreviewFrame, TAG_EVENT, TAG_PREVIEW, TAG_THUMB,
};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as Proc, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// File name of the helper inside `Contents/MacOS` (and `target/{debug,release}` in dev).
pub const HELPER_NAME: &str = "ir-record-render";
/// `Open` builds the preview (decoders, Metal device, shader compile on first use).
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(30);
/// One preview frame is a copy out of the preview thread; seconds mean the helper is wedged.
pub const FRAME_TIMEOUT: Duration = Duration::from_secs(5);
/// Thumbnails render on the GPU one by one: a base plus a budget per frame.
pub const THUMBS_TIMEOUT_BASE: Duration = Duration::from_secs(15);
pub const THUMBS_TIMEOUT_PER_FRAME: Duration = Duration::from_secs(2);
/// `Shutdown` may hang on the preview thread; after this the helper is killed.
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

/// Notices the client pushes on its own (from the reader thread).
#[derive(Debug, Clone, PartialEq)]
pub enum Notice {
    ExportProgress { frames: u64, total: u64 },
    ExportDone { path: String },
    ExportFailed { reason: String, cancelled: bool },
    /// The helper is gone. Sent once per client, never after an orderly `shutdown`.
    Dead { reason: String },
}

pub type Sink = Arc<dyn Fn(Notice) + Send + Sync>;

/// `RenderClient::frame`'s answer.
#[derive(Debug, Clone, PartialEq)]
pub enum FrameAnswer {
    Frame(PreviewFrame),
    /// The helper answered: nothing newer than `since_gen`.
    NoneNewer,
    /// No request was made: the helper is busy (another request, an export).
    Busy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientState {
    Running,
    Dead { reason: String },
}

#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    pub open: Duration,
    pub frame: Duration,
    pub thumbs_base: Duration,
    pub thumbs_per_frame: Duration,
    pub shutdown: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            open: OPEN_TIMEOUT,
            frame: FRAME_TIMEOUT,
            thumbs_base: THUMBS_TIMEOUT_BASE,
            thumbs_per_frame: THUMBS_TIMEOUT_PER_FRAME,
            shutdown: SHUTDOWN_TIMEOUT,
        }
    }
}

/// What the reader thread hands to the request currently waiting.
#[derive(Debug)]
enum Reply {
    Preview(PreviewFrame),
    Thumb(u64, Vec<u8>),
    Event(Event),
}

struct Shared {
    child: Mutex<Child>,
    dead: Mutex<Option<String>>,
    /// Set by an orderly `shutdown`: the end of the stream is then expected, not a death.
    closing: AtomicBool,
    exporting: AtomicBool,
    /// The running export's target (its staging file sits next to it).
    export_target: Mutex<Option<PathBuf>>,
    pid: u32,
    sink: Sink,
}

/// An export the helper can no longer finish (killed, crashed, shut down mid-export).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanedExport {
    /// Staging files of THIS helper process for this export's type (`.openscreen-<ext>-<pid>-
    /// <n>.partial.<ext>`, the compositor's `with_staged_output` name) — never the target.
    pub remove: Vec<PathBuf>,
    /// The target exists: the helper published it (atomic rename) before it died, so the
    /// export is complete even though its `ExportDone` never arrived.
    pub published: bool,
}

/// Pure decision for [`OrphanedExport`]: `names` are the file names in the target's folder.
pub fn orphaned_export(target: &Path, helper_pid: u32, names: &[String]) -> OrphanedExport {
    let dir = target.parent().unwrap_or(Path::new("."));
    let ext = target.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
    let prefix = format!(".openscreen-{ext}-{helper_pid}-");
    let suffix = format!(".partial.{ext}");
    let target_name = target.file_name().map(|n| n.to_string_lossy().into_owned());
    let remove = names
        .iter()
        .filter(|n| {
            n.strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix(&suffix))
                .is_some_and(|nonce| !nonce.is_empty() && nonce.bytes().all(|b| b.is_ascii_digit()))
        })
        .map(|n| dir.join(n))
        .collect();
    let published = target_name.is_some_and(|t| names.contains(&t));
    OrphanedExport { remove, published }
}

impl Shared {
    /// Marks the client dead (first reason wins), kills and reaps the child, tells the sink.
    fn die(&self, reason: String) {
        {
            let mut dead = self.dead.lock().unwrap_or_else(|p| p.into_inner());
            if dead.is_some() {
                return;
            }
            *dead = Some(reason.clone());
        }
        {
            let mut child = self.child.lock().unwrap_or_else(|p| p.into_inner());
            let _ = child.kill();
            let _ = child.wait();
        }
        let closing = self.closing.load(Ordering::SeqCst);
        // The child is gone: nothing writes to the export any more. Clean up after it and tell
        // the UI — also on an orderly close, where the export simply ends as cancelled.
        if self.exporting.swap(false, Ordering::SeqCst) {
            let target = self.export_target.lock().unwrap_or_else(|p| p.into_inner()).take();
            let mut published = None;
            if let Some(target) = target {
                let names: Vec<String> = target
                    .parent()
                    .and_then(|d| std::fs::read_dir(d).ok())
                    .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
                    .unwrap_or_default();
                let orphan = orphaned_export(&target, self.pid, &names);
                for f in &orphan.remove {
                    if let Err(e) = std::fs::remove_file(f) {
                        tracing::warn!(target: "record::render", "removing {}: {e}", f.display());
                    }
                }
                if orphan.published {
                    published = Some(target);
                }
            }
            match published {
                Some(path) => (self.sink)(Notice::ExportDone { path: path.to_string_lossy().into_owned() }),
                None => (self.sink)(Notice::ExportFailed { reason: reason.clone(), cancelled: closing }),
            }
        }
        if closing {
            return;
        }
        tracing::warn!(target: "record::render", "render helper dead: {reason}");
        (self.sink)(Notice::Dead { reason });
    }

    fn dead_reason(&self) -> Option<String> {
        self.dead.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

pub struct RenderClient {
    shared: Arc<Shared>,
    stdin: Mutex<Option<ChildStdin>>,
    /// Held for the whole of one request: replies arrive in command order.
    replies: Mutex<Receiver<Reply>>,
    timeouts: Timeouts,
}

/// Where the helper lives: next to the app binary (bundle `Contents/MacOS`, or `target/debug`
/// when the app runs from cargo), one level up (`target/debug/deps` for tests), else the
/// workspace's `target/{debug,release}`. `dev` is true outside an app bundle — only then does
/// the helper need `DYLD_FALLBACK_LIBRARY_PATH` to find the ffmpeg dylibs.
pub fn helper_path() -> Option<(PathBuf, bool)> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let in_bundle = dir.ends_with("Contents/MacOS");
    let mut candidates = vec![dir.join(HELPER_NAME)];
    if let Some(up) = dir.parent() {
        candidates.push(up.join(HELPER_NAME));
    }
    // The workspace's own build folders only for a debug build outside a bundle: a release
    // bundle missing its helper must fail, not pick up a developer's local build.
    if cfg!(debug_assertions) && !in_bundle {
        let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target");
        candidates.push(target.join("debug").join(HELPER_NAME));
        candidates.push(target.join("release").join(HELPER_NAME));
    }
    candidates.into_iter().find(|p| p.is_file()).map(|p| (p, !in_bundle))
}

/// Environment for a dev helper: the LGPL ffmpeg build of `scripts/build-ffmpeg-macos.sh`.
pub fn dev_env() -> Vec<(String, String)> {
    let lib = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../third_party/ffmpeg/8.1.2/lib");
    vec![("DYLD_FALLBACK_LIBRARY_PATH".into(), lib.to_string_lossy().into_owned())]
}

impl RenderClient {
    /// Starts the real helper (see [`helper_path`]).
    pub fn spawn(sink: Sink) -> Result<Self, String> {
        let (path, dev) = helper_path().ok_or_else(|| "record.editor.helper_missing".to_string())?;
        let env = if dev { dev_env() } else { Vec::new() };
        Self::spawn_with(path, env, sink)
    }

    pub fn spawn_with(path: PathBuf, env: Vec<(String, String)>, sink: Sink) -> Result<Self, String> {
        Self::spawn_with_timeouts(path, env, sink, Timeouts::default())
    }

    pub fn spawn_with_timeouts(
        path: PathBuf,
        env: Vec<(String, String)>,
        sink: Sink,
        timeouts: Timeouts,
    ) -> Result<Self, String> {
        let mut cmd = Proc::new(&path);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().map_err(|e| format!("start {}: {e}", path.display()))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let (Some(stdin), Some(stdout), Some(stderr)) = (stdin, stdout, stderr) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("render helper has no pipes".into());
        };
        let pid = child.id();
        let shared = Arc::new(Shared {
            pid,
            export_target: Mutex::new(None),
            child: Mutex::new(child),
            dead: Mutex::new(None),
            closing: AtomicBool::new(false),
            exporting: AtomicBool::new(false),
            sink,
        });
        let (tx, rx) = mpsc::channel();
        {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("ir-render-reader".into())
                .spawn(move || read_loop(stdout, tx, shared))
                .map_err(|e| format!("reader thread: {e}"))?;
        }
        std::thread::Builder::new()
            .name("ir-render-stderr".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    match line {
                        Ok(l) => tracing::info!(target: "record::render", "{l}"),
                        Err(_) => break,
                    }
                }
            })
            .map_err(|e| format!("stderr thread: {e}"))?;
        Ok(Self { shared, stdin: Mutex::new(Some(stdin)), replies: Mutex::new(rx), timeouts })
    }

    pub fn state(&self) -> ClientState {
        match self.shared.dead_reason() {
            Some(reason) => ClientState::Dead { reason },
            None => ClientState::Running,
        }
    }

    pub fn is_dead(&self) -> bool {
        self.shared.dead_reason().is_some()
    }

    pub fn exporting(&self) -> bool {
        self.shared.exporting.load(Ordering::SeqCst)
    }

    fn dead_err(&self) -> String {
        self.shared.dead_reason().unwrap_or_else(|| "render helper is gone".into())
    }

    /// Sends one command without waiting (the setters answer only on failure).
    pub fn send(&self, cmd: &Command) -> Result<(), String> {
        if let Some(reason) = self.shared.dead_reason() {
            return Err(reason);
        }
        let mut stdin = self.stdin.lock().unwrap_or_else(|p| p.into_inner());
        let Some(pipe) = stdin.as_mut() else { return Err(self.dead_err()) };
        if let Err(e) = write_command(pipe, cmd).and_then(|_| pipe.flush()) {
            drop(stdin);
            self.shared.die(format!("writing to the render helper failed: {e}"));
            return Err(self.dead_err());
        }
        Ok(())
    }

    /// Takes the request slot, drops replies a previous request left behind (a setter's late
    /// `Error`, a reply after a failed request), sends `cmd`.
    fn begin<'a>(&'a self, rx: MutexGuard<'a, Receiver<Reply>>, cmd: &Command) -> Result<MutexGuard<'a, Receiver<Reply>>, String> {
        while let Ok(stale) = rx.try_recv() {
            tracing::debug!(target: "record::render", "dropping stale reply: {stale:?}");
        }
        self.send(cmd)?;
        Ok(rx)
    }

    /// Waits for the next reply until `deadline`. Timeout or a closed stream kill the helper.
    fn next(&self, rx: &Receiver<Reply>, deadline: Instant, what: &str) -> Result<Reply, String> {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(r) => Ok(r),
            Err(RecvTimeoutError::Timeout) => {
                self.shared.die(format!("render helper did not answer {what} in time"));
                Err(self.dead_err())
            }
            Err(RecvTimeoutError::Disconnected) => {
                // The reader thread ends only after it marked the client dead.
                Err(self.dead_err())
            }
        }
    }

    /// `Open` → `Ready`.
    pub fn open(&self, scene: Value, preview_w: u32, preview_h: u32) -> Result<(), String> {
        // The render thread is busy until the export ends: an `Open` would wait behind it,
        // time out and kill the helper — and the export with it.
        if self.exporting() {
            return Err("record.editor.open_exporting".into());
        }
        let rx = self.replies.lock().unwrap_or_else(|p| p.into_inner());
        let rx = self.begin(rx, &Command::Open { scene, preview_w, preview_h })?;
        let deadline = Instant::now() + self.timeouts.open;
        loop {
            match self.next(&rx, deadline, "Open")? {
                Reply::Event(Event::Ready) => return Ok(()),
                Reply::Event(Event::Error { msg }) => return Err(msg),
                other => tracing::debug!(target: "record::render", "ignoring {other:?} while opening"),
            }
        }
    }

    /// The newest preview frame after `since_gen`. `Busy` when another request (thumbnails, a
    /// re-open) holds the helper or an export blocks its render thread — no frame can come then,
    /// which the stall watchdog must not mistake for a wedged preview.
    pub fn frame(&self, since_gen: u64) -> Result<FrameAnswer, String> {
        if let Some(reason) = self.shared.dead_reason() {
            return Err(reason);
        }
        if self.exporting() {
            return Ok(FrameAnswer::Busy);
        }
        let rx = match self.replies.try_lock() {
            Ok(rx) => rx,
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return Ok(FrameAnswer::Busy),
        };
        let rx = self.begin(rx, &Command::Frame { since_gen })?;
        let deadline = Instant::now() + self.timeouts.frame;
        loop {
            match self.next(&rx, deadline, "Frame")? {
                Reply::Preview(f) if f.is_empty() => return Ok(FrameAnswer::NoneNewer),
                Reply::Preview(f) => return Ok(FrameAnswer::Frame(f)),
                Reply::Event(Event::Error { msg }) => {
                    tracing::debug!(target: "record::render", "frame: {msg}");
                    return Ok(FrameAnswer::NoneNewer);
                }
                other => tracing::debug!(target: "record::render", "ignoring {other:?} while waiting for a frame"),
            }
        }
    }

    /// Renders `frames` (indices on the `out_fps` grid) as JPEGs, returned in request order.
    pub fn thumbs(&self, scene: Value, out_fps: u32, frames: Vec<u64>, max_width: u32) -> Result<Vec<(u64, Vec<u8>)>, String> {
        if self.exporting() {
            return Err("record.editor.busy".into());
        }
        let count = frames.len() as u32;
        let rx = self.replies.lock().unwrap_or_else(|p| p.into_inner());
        let rx = self.begin(rx, &Command::Thumbs { scene, out_fps, frames, max_width })?;
        let deadline = Instant::now() + self.timeouts.thumbs_base + self.timeouts.thumbs_per_frame * count;
        let mut out = Vec::new();
        loop {
            match self.next(&rx, deadline, "Thumbs")? {
                Reply::Thumb(i, jpeg) => out.push((i, jpeg)),
                Reply::Event(Event::ThumbsDone) => return Ok(out),
                Reply::Event(Event::Error { msg }) => return Err(msg),
                other => tracing::debug!(target: "record::render", "ignoring {other:?} while rendering thumbnails"),
            }
        }
    }

    /// Starts an export; progress and the outcome arrive as [`Notice`]s. One at a time.
    pub fn export(&self, scene: Value, kind: ExportKind, out_path: String) -> Result<(), String> {
        if self.shared.exporting.swap(true, Ordering::SeqCst) {
            return Err("record.editor.export_busy".into());
        }
        *self.shared.export_target.lock().unwrap_or_else(|p| p.into_inner()) = Some(PathBuf::from(&out_path));
        if let Err(e) = self.send(&Command::Export { scene, kind, out_path }) {
            self.shared.exporting.store(false, Ordering::SeqCst);
            self.shared.export_target.lock().unwrap_or_else(|p| p.into_inner()).take();
            return Err(e);
        }
        Ok(())
    }

    /// Gives up on the helper (e.g. a stalled preview): killed, reaped, reported as dead.
    pub fn abandon(&self, reason: String) {
        self.shared.die(reason);
    }

    pub fn cancel_export(&self) -> Result<(), String> {
        if !self.exporting() {
            return Ok(());
        }
        self.send(&Command::Cancel)
    }

    /// Orderly end: `Shutdown`, wait up to the shutdown timeout, then kill. Always reaps.
    pub fn shutdown(&self) {
        self.shared.closing.store(true, Ordering::SeqCst);
        let _ = self.send(&Command::Shutdown);
        // Closing stdin is the helper's second "stop" signal.
        self.stdin.lock().unwrap_or_else(|p| p.into_inner()).take();
        let deadline = Instant::now() + self.timeouts.shutdown;
        loop {
            let exited = {
                let mut child = self.shared.child.lock().unwrap_or_else(|p| p.into_inner());
                matches!(child.try_wait(), Ok(Some(_)))
            };
            if exited || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.shared.die("render helper shut down".into());
    }
}

impl Drop for RenderClient {
    fn drop(&mut self) {
        if !self.is_dead() {
            self.shared.closing.store(true, Ordering::SeqCst);
            self.shared.die("render client dropped".into());
        }
    }
}

fn read_loop(mut stdout: impl std::io::Read, tx: Sender<Reply>, shared: Arc<Shared>) {
    let mut fatal: Option<String> = None;
    let end = loop {
        let (tag, body) = match read_frame(&mut stdout) {
            Ok(f) => f,
            Err(e) => break e,
        };
        let reply = match tag {
            TAG_PREVIEW => match decode_preview(&body) {
                Ok(f) => Reply::Preview(f),
                Err(e) => {
                    tracing::warn!(target: "record::render", "bad preview frame: {e}");
                    continue;
                }
            },
            TAG_THUMB => match decode_thumb(&body) {
                Ok((i, jpeg)) => Reply::Thumb(i, jpeg.to_vec()),
                Err(e) => {
                    tracing::warn!(target: "record::render", "bad thumbnail frame: {e}");
                    continue;
                }
            },
            TAG_EVENT => match decode_event(&body) {
                Ok(Event::ExportProgress { frames, total }) => {
                    (shared.sink)(Notice::ExportProgress { frames, total });
                    continue;
                }
                Ok(Event::ExportDone { path }) => {
                    shared.export_target.lock().unwrap_or_else(|p| p.into_inner()).take();
                    shared.exporting.store(false, Ordering::SeqCst);
                    (shared.sink)(Notice::ExportDone { path });
                    continue;
                }
                Ok(Event::ExportFailed { msg, cancelled }) => {
                    shared.export_target.lock().unwrap_or_else(|p| p.into_inner()).take();
                    shared.exporting.store(false, Ordering::SeqCst);
                    (shared.sink)(Notice::ExportFailed { reason: msg, cancelled });
                    continue;
                }
                Ok(Event::Fatal { msg }) => {
                    fatal = Some(msg);
                    continue;
                }
                Ok(ev) => Reply::Event(ev),
                Err(e) => {
                    tracing::warn!(target: "record::render", "bad event: {e}");
                    continue;
                }
            },
            other => {
                tracing::warn!(target: "record::render", "unknown frame tag {other:#04x}");
                continue;
            }
        };
        // A send fails only once the client itself is gone; nothing left to answer.
        let _ = tx.send(reply);
    };
    // Reap first so the reason can carry the exit status.
    let status = {
        let mut child = shared.child.lock().unwrap_or_else(|p| p.into_inner());
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
                _ => break None,
            }
        }
    };
    let reason = match (fatal, status) {
        (Some(msg), _) => msg,
        (None, Some(s)) => format!("render helper exited ({s})"),
        (None, None) => format!("render helper closed its output ({end})"),
    };
    drop(tx);
    shared.die(reason);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A stand-in helper: a shell script in a fresh temp dir.
    fn script(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ir-render-client-{}-{name}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("helper.sh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// `printf` body writing the `Ready` event frame (tag 2, length 17, `{"event":"ready"}`).
    const READY: &str = r#"printf '\002\021\000\000\000{"event":"ready"}'"#;

    fn collecting() -> (Sink, Arc<Mutex<Vec<Notice>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        (Arc::new(move |n| s.lock().unwrap().push(n)), seen)
    }

    fn wait_until(mut f: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !f() {
            assert!(Instant::now() < deadline, "condition not reached in time");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn client_survives_helper_crash() {
        let crash = script("crash", &format!("{READY}\nsleep 0.2\nexit 1"));
        let (sink, seen) = collecting();
        let client = RenderClient::spawn_with(crash, Vec::new(), sink).unwrap();
        client.open(serde_json::json!({}), 64, 36).expect("Ready arrives before the crash");
        wait_until(|| client.is_dead());
        let ClientState::Dead { reason } = client.state() else { panic!("not dead") };
        assert!(reason.contains("exited"), "reason names the exit: {reason}");
        assert!(client.frame(0).is_err(), "a dead client refuses requests");
        assert!(client.send(&Command::SetPlaying { playing: true }).is_err());
        wait_until(|| seen.lock().unwrap().iter().any(|n| matches!(n, Notice::Dead { .. })));
        let deaths = seen.lock().unwrap().iter().filter(|n| matches!(n, Notice::Dead { .. })).count();
        assert_eq!(deaths, 1, "the sink hears the death exactly once");

        // The next spawn works: the caller simply starts a fresh helper.
        let alive = script("alive", &format!("{READY}\ncat > /dev/null"));
        let (sink2, seen2) = collecting();
        let again = RenderClient::spawn_with(alive, Vec::new(), sink2).unwrap();
        again.open(serde_json::json!({}), 64, 36).unwrap();
        assert_eq!(again.state(), ClientState::Running);
        again.shutdown();
        assert!(again.is_dead());
        assert!(seen2.lock().unwrap().is_empty(), "an orderly shutdown is not a death notice");
    }

    #[test]
    fn unanswered_request_times_out_and_kills_the_helper() {
        let mute = script("mute", "exec sleep 30");
        let (sink, seen) = collecting();
        let t = Timeouts { open: Duration::from_millis(300), ..Timeouts::default() };
        let client = RenderClient::spawn_with_timeouts(mute, Vec::new(), sink, t).unwrap();
        let started = Instant::now();
        let err = client.open(serde_json::json!({}), 64, 36).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(5), "the timeout, not the sleep, ended it");
        assert!(err.contains("in time"), "{err}");
        assert!(client.is_dead());
        // Killed and reaped: the child has an exit status.
        let mut child = client.shared.child.lock().unwrap();
        assert!(matches!(child.try_wait(), Ok(Some(_))), "child reaped");
        drop(child);
        assert!(seen.lock().unwrap().iter().any(|n| matches!(n, Notice::Dead { .. })));
    }

    #[test]
    fn shutdown_kills_a_helper_that_ignores_it() {
        // Ignores both Shutdown and its closed stdin.
        let stubborn = script("stubborn", "trap '' TERM\nexec sleep 30");
        let (sink, _) = collecting();
        let t = Timeouts { shutdown: Duration::from_millis(200), ..Timeouts::default() };
        let client = RenderClient::spawn_with_timeouts(stubborn, Vec::new(), sink, t).unwrap();
        let started = Instant::now();
        client.shutdown();
        assert!(started.elapsed() < Duration::from_secs(3));
        let mut child = client.shared.child.lock().unwrap();
        assert!(matches!(child.try_wait(), Ok(Some(_))), "killed and reaped");
    }

    #[test]
    fn orphaned_export_removes_only_this_helpers_staging() {
        let target = Path::new("/d/Demo 2026-11-02 09.05.mp4");
        let names: Vec<String> = [
            ".openscreen-mp4-42-0.partial.mp4",  // ours
            ".openscreen-mp4-42-17.partial.mp4", // ours, later nonce
            ".openscreen-mp4-43-0.partial.mp4",  // another helper (a second app instance)
            ".openscreen-gif-42-0.partial.gif",  // another export type
            ".openscreen-mp4-42-x.partial.mp4",  // not a nonce
            ".openscreen-mp4-42-.partial.mp4",   // empty nonce
            "Demo 2026-11-02 09.05 2.mp4",       // the user's earlier export
            "notes.txt",
        ]
        .map(String::from)
        .to_vec();
        let o = orphaned_export(target, 42, &names);
        assert_eq!(
            o.remove,
            vec![Path::new("/d/.openscreen-mp4-42-0.partial.mp4"), Path::new("/d/.openscreen-mp4-42-17.partial.mp4")]
        );
        assert!(!o.published, "the target was never published");
        let mut with_target = names.clone();
        with_target.push("Demo 2026-11-02 09.05.mp4".into());
        let o = orphaned_export(target, 42, &with_target);
        assert!(o.published, "a published target is a finished export");
        assert!(!o.remove.iter().any(|p| p == target), "the target itself is never removed");
    }

    #[test]
    fn death_mid_export_removes_the_staging_file() {
        let dir = std::env::temp_dir().join(format!("ir-render-client-{}-staging", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let staging = |pid: u32| dir.join(format!(".openscreen-gif-{pid}-0.partial.gif"));
        // Writes its staging file (named after its own pid), then crashes.
        let crash = script(
            "staging-crash",
            &format!("touch \"{}/.openscreen-gif-$$-0.partial.gif\"\nsleep 0.3\nexit 9", dir.display()),
        );
        let (sink, seen) = collecting();
        let client = RenderClient::spawn_with(crash, Vec::new(), sink).unwrap();
        let pid = client.shared.pid;
        let kind = ExportKind::Gif { w: 2, h: 2, fps: 15, loop_forever: true };
        client.export(serde_json::json!({}), kind.clone(), dir.join("out.gif").to_string_lossy().into()).unwrap();
        wait_until(|| staging(pid).exists() || client.is_dead());
        wait_until(|| client.is_dead());
        assert!(!staging(pid).exists(), "the partial file is gone");
        assert!(seen.lock().unwrap().iter().any(|n| matches!(n, Notice::ExportFailed { cancelled: false, .. })));

        // Editor closed mid-export: an orderly close still cleans up and reports a cancel.
        let busy = script(
            "staging-close",
            &format!("touch \"{}/.openscreen-gif-$$-0.partial.gif\"\ntrap '' TERM\nexec sleep 30", dir.display()),
        );
        let (sink, seen) = collecting();
        let t = Timeouts { shutdown: Duration::from_millis(300), ..Timeouts::default() };
        let client = RenderClient::spawn_with_timeouts(busy, Vec::new(), sink, t).unwrap();
        let pid = client.shared.pid;
        client.export(serde_json::json!({}), kind, dir.join("out2.gif").to_string_lossy().into()).unwrap();
        wait_until(|| staging(pid).exists());
        client.shutdown();
        assert!(!staging(pid).exists());
        let seen = seen.lock().unwrap();
        assert!(seen.iter().any(|n| matches!(n, Notice::ExportFailed { cancelled: true, .. })), "{seen:?}");
        assert!(!seen.iter().any(|n| matches!(n, Notice::Dead { .. })), "an orderly close is not a death");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_during_an_export_is_refused_without_killing_the_helper() {
        // A helper busy exporting never answers Open; waiting would time out and kill it.
        let busy = script("open-busy", "sleep 3");
        let (sink, _seen) = collecting();
        let client = RenderClient::spawn_with(busy, Vec::new(), sink).unwrap();
        let kind = ExportKind::Gif { w: 2, h: 2, fps: 15, loop_forever: true };
        client.export(serde_json::json!({}), kind, "/tmp/ir-open-busy.gif".into()).unwrap();
        let started = Instant::now();
        assert_eq!(client.open(serde_json::json!({}), 64, 36).unwrap_err(), "record.editor.open_exporting");
        assert!(started.elapsed() < Duration::from_millis(500), "refused at once, not after a timeout");
        assert!(!client.is_dead(), "the export keeps running");
        assert!(client.exporting());
        client.abandon("test over".into());
    }

    #[test]
    fn export_is_one_at_a_time_and_death_fails_it() {
        let alive = script("export", "sleep 0.3\nexit 3");
        let (sink, seen) = collecting();
        let client = RenderClient::spawn_with(alive, Vec::new(), sink).unwrap();
        let kind = ExportKind::Gif { w: 2, h: 2, fps: 15, loop_forever: true };
        client.export(serde_json::json!({}), kind.clone(), "/tmp/x.gif".into()).unwrap();
        assert_eq!(
            client.export(serde_json::json!({}), kind, "/tmp/y.gif".into()).unwrap_err(),
            "record.editor.export_busy"
        );
        assert_eq!(client.frame(0).unwrap(), FrameAnswer::Busy, "no frame requests while an export blocks the helper");
        wait_until(|| client.is_dead());
        wait_until(|| seen.lock().unwrap().iter().any(|n| matches!(n, Notice::ExportFailed { .. })));
        assert!(!client.exporting());
    }
}
