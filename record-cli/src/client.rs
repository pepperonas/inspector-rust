//! Client side of the app's control socket.
//!
//! Wire format (one JSON object per line, both directions), from
//! `core/rust-lib/src/control.rs`:
//! request `{"id":n,"token":"…","method":"…","params":{…}}`, reply
//! `{"id":n,"result":…}` or `{"id":n,"error":{"code","message"}}`, progress
//! `{"id":n,"event":{…}}` lines before the reply of a long method.
//!
//! Discovery: `<data dir>/control.json` `{pid, socket, token}`. The file can be
//! stale (the app crashed, the pid was reused): it is only a hint. A connection
//! counts once `control.ping` answered on it — the pid is never trusted.

use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How long `connect` waits for the app by default (start-up included).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// One `control.ping` round trip on a fresh connection.
const PING_TIMEOUT: Duration = Duration::from_secs(2);
/// Granularity of the read loop: how often a blocked call checks `stop`.
const POLL: Duration = Duration::from_millis(200);
/// Between connection attempts while the app starts.
const RETRY: Duration = Duration::from_millis(200);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// Largest reply line accepted (frames come back base64-encoded).
const MAX_LINE: usize = 64 << 20;
/// Bundle id of the installed app (`macos/src-tauri/tauri.conf.json`).
pub const BUNDLE_ID: &str = "io.celox.inspector-rust";

/// Codes of errors produced by the client itself (never sent by the app).
pub const ERR_CLOSED: &str = "client.closed";
pub const ERR_INTERRUPTED: &str = "client.interrupted";
pub const ERR_IO: &str = "client.io";
pub const ERR_PROTOCOL: &str = "client.protocol";

/// Why no connection could be made.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientError {
    /// No live app (no/stale discovery file) and launching was not allowed.
    NotRunning,
    /// Launching the app failed (not installed, `open` refused, …).
    LaunchFailed(String),
    /// The app was started but did not answer within the timeout.
    StartTimeout(Duration),
    /// The socket answered, but refused our token (another app instance).
    BadToken,
    /// Platform without unix sockets.
    Unsupported,
}

impl ClientError {
    pub fn code(&self) -> &'static str {
        match self {
            ClientError::NotRunning => "client.not_running",
            ClientError::LaunchFailed(_) => "client.launch_failed",
            ClientError::StartTimeout(_) => "client.start_timeout",
            ClientError::BadToken => "control.bad_token",
            ClientError::Unsupported => "record.unsupported",
        }
    }
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::NotRunning => write!(f, "Inspector Rust is not running"),
            ClientError::LaunchFailed(d) if d.is_empty() => {
                write!(f, "Inspector Rust is not running and could not be started")
            }
            ClientError::LaunchFailed(d) => {
                write!(f, "Inspector Rust is not running and could not be started ({d})")
            }
            ClientError::StartTimeout(t) => write!(
                f,
                "Inspector Rust is not running and could not be started (no answer within {} s)",
                t.as_secs().max(1)
            ),
            ClientError::BadToken => {
                write!(f, "the control socket refused the token from control.json")
            }
            ClientError::Unsupported => write!(f, "ir-record needs macOS or Linux"),
        }
    }
}

impl std::error::Error for ClientError {}

/// An error answer of a call: the app's sentinel (`record.*`, `control.*`) or a
/// client-side one (`client.*`).
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteError {
    pub code: String,
    pub message: String,
}

impl RemoteError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        RemoteError { code: code.to_string(), message: message.into() }
    }
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.message.is_empty() {
            write!(f, "{}", self.code)
        } else {
            write!(f, "{} ({})", self.message, self.code)
        }
    }
}

impl std::error::Error for RemoteError {}

/// `$IR_DATA_DIR`, else the app's data dir (same rule as the app's `dirs::data_dir()`).
pub fn data_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("IR_DATA_DIR").filter(|d| !d.is_empty()) {
        return Some(d.into());
    }
    default_data_dir()
}

fn default_data_dir() -> Option<PathBuf> {
    let home = || std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return Some(home()?.join("Library/Application Support/InspectorRust"));
    }
    if cfg!(windows) {
        return Some(PathBuf::from(std::env::var_os("APPDATA")?).join("InspectorRust"));
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| Some(home()?.join(".local/share")))?;
    Some(base.join("InspectorRust"))
}

/// Parsed `control.json`.
#[derive(Clone, PartialEq)]
pub struct Discovery {
    pub pid: u32,
    pub socket: PathBuf,
    pub token: String,
}

/// Never prints the token.
impl std::fmt::Debug for Discovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Discovery").field("pid", &self.pid).field("socket", &self.socket).field("token", &"<redacted>").finish()
    }
}

/// Pure: parse the discovery file's contents.
pub fn parse_discovery(text: &str) -> Option<Discovery> {
    let v: Value = serde_json::from_str(text).ok()?;
    let socket = v.get("socket")?.as_str().filter(|s| !s.is_empty())?;
    let token = v.get("token")?.as_str().filter(|s| !s.is_empty())?;
    let pid = v.get("pid").and_then(Value::as_u64).unwrap_or(0) as u32;
    Some(Discovery { pid, socket: socket.into(), token: token.to_string() })
}

/// Pure: one request line (without the trailing newline).
pub fn request_line(id: u64, token: &str, method: &str, params: &Value) -> String {
    json!({"id": id, "token": token, "method": method, "params": params}).to_string()
}

/// One parsed line from the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Event(Value),
    Result(Value),
    Error(RemoteError),
    /// A line for another request id.
    Other,
}

/// Pure: classify a line of the app for request `id`. An error with id `null`
/// (a request the app could not parse) belongs to the current call.
pub fn parse_incoming(line: &str, id: u64) -> Result<Incoming, RemoteError> {
    let v: Value = serde_json::from_str(line)
        .map_err(|e| RemoteError::new(ERR_PROTOCOL, format!("bad line from the app: {e}")))?;
    let line_id = v.get("id").cloned().unwrap_or(Value::Null);
    let ours = line_id.as_u64() == Some(id);
    if let Some(e) = v.get("error") {
        if !ours && !line_id.is_null() {
            return Ok(Incoming::Other);
        }
        let code = e.get("code").and_then(Value::as_str).unwrap_or("control.internal");
        let message = e.get("message").and_then(Value::as_str).unwrap_or("");
        return Ok(Incoming::Error(RemoteError::new(code, message)));
    }
    if !ours {
        return Ok(Incoming::Other);
    }
    if let Some(ev) = v.get("event") {
        return Ok(Incoming::Event(ev.clone()));
    }
    if let Some(r) = v.get("result") {
        return Ok(Incoming::Result(r.clone()));
    }
    Err(RemoteError::new(ERR_PROTOCOL, "line without result, error or event"))
}

// ---------------------------------------------------------------------------
// Transport

trait Stream: Read + Write + Send {
    fn set_read_timeout(&self, t: Option<Duration>) -> std::io::Result<()>;
    fn set_write_timeout(&self, t: Option<Duration>) -> std::io::Result<()>;
    fn shutdown(&self);
}

#[cfg(unix)]
impl Stream for std::os::unix::net::UnixStream {
    fn set_read_timeout(&self, t: Option<Duration>) -> std::io::Result<()> {
        std::os::unix::net::UnixStream::set_read_timeout(self, t)
    }
    fn set_write_timeout(&self, t: Option<Duration>) -> std::io::Result<()> {
        std::os::unix::net::UnixStream::set_write_timeout(self, t)
    }
    fn shutdown(&self) {
        let _ = std::os::unix::net::UnixStream::shutdown(self, std::net::Shutdown::Both);
    }
}

#[cfg(unix)]
fn open_socket(path: &Path) -> std::io::Result<Box<dyn Stream>> {
    Ok(Box::new(std::os::unix::net::UnixStream::connect(path)?))
}

#[cfg(not(unix))]
fn open_socket(_path: &Path) -> std::io::Result<Box<dyn Stream>> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "no unix sockets"))
}

/// A live connection to the app (ping answered).
pub struct Conn {
    stream: Box<dyn Stream>,
    buf: Vec<u8>,
    token: String,
    next_id: u64,
    dead: bool,
    /// What `control.ping` answered: `{version, platform}`.
    pub ping: Value,
}

impl Conn {
    /// Call `method`; progress events go to `on_event` before the result.
    pub fn call(
        &mut self,
        method: &str,
        params: Value,
        on_event: &mut dyn FnMut(Value),
    ) -> Result<Value, RemoteError> {
        self.call_until(method, params, on_event, &|| false)
    }

    /// Like [`Conn::call`], but gives up when `stop()` turns true (checked at
    /// least every 200 ms while waiting): the connection is closed — which
    /// cancels a long method in the app — and the call answers
    /// `client.interrupted`. The `Conn` is unusable afterwards.
    pub fn call_until(
        &mut self,
        method: &str,
        params: Value,
        on_event: &mut dyn FnMut(Value),
        stop: &dyn Fn() -> bool,
    ) -> Result<Value, RemoteError> {
        self.call_inner(method, params, on_event, stop, None)
    }

    fn call_inner(
        &mut self,
        method: &str,
        params: Value,
        on_event: &mut dyn FnMut(Value),
        stop: &dyn Fn() -> bool,
        deadline: Option<Instant>,
    ) -> Result<Value, RemoteError> {
        if self.dead {
            return Err(RemoteError::new(ERR_CLOSED, "the connection is closed"));
        }
        self.next_id += 1;
        let id = self.next_id;
        let mut line = request_line(id, &self.token, method, &params);
        line.push('\n');
        if let Err(e) = self.stream.write_all(line.as_bytes()).and_then(|_| self.stream.flush()) {
            self.close();
            return Err(RemoteError::new(ERR_IO, format!("sending {method}: {e}")));
        }
        loop {
            let line = match self.next_line(stop, deadline) {
                Ok(l) => l,
                Err(e) => {
                    self.close();
                    return Err(e);
                }
            };
            match parse_incoming(&line, id) {
                Ok(Incoming::Event(ev)) => on_event(ev),
                Ok(Incoming::Result(v)) => return Ok(v),
                Ok(Incoming::Error(e)) => {
                    if e.code == "control.bad_token" || e.code == "control.bad_request" {
                        self.close(); // the app closes the connection after these
                    }
                    return Err(e);
                }
                Ok(Incoming::Other) => {}
                Err(e) => {
                    self.close();
                    return Err(e);
                }
            }
        }
    }

    fn next_line(
        &mut self,
        stop: &dyn Fn() -> bool,
        deadline: Option<Instant>,
    ) -> Result<String, RemoteError> {
        let mut chunk = [0u8; 16 * 1024];
        loop {
            if let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
                let rest = self.buf.split_off(pos + 1);
                let mut line = std::mem::replace(&mut self.buf, rest);
                line.pop();
                return String::from_utf8(line)
                    .map_err(|_| RemoteError::new(ERR_PROTOCOL, "line is not UTF-8"));
            }
            if self.buf.len() > MAX_LINE {
                return Err(RemoteError::new(ERR_PROTOCOL, "reply line too long"));
            }
            if stop() {
                return Err(RemoteError::new(ERR_INTERRUPTED, "interrupted"));
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Err(RemoteError::new(ERR_IO, "no answer in time"));
            }
            match self.stream.read(&mut chunk) {
                Ok(0) => {
                    return Err(RemoteError::new(ERR_CLOSED, "the app closed the connection"))
                }
                Ok(n) => self.buf.extend_from_slice(&chunk[..n]),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(RemoteError::new(ERR_IO, e.to_string())),
            }
        }
    }

    /// Close the connection (idempotent). A long method running for it is cancelled.
    pub fn close(&mut self) {
        if !self.dead {
            self.dead = true;
            self.stream.shutdown();
        }
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.close();
    }
}

/// Try the discovery file once: read → connect → ping.
fn try_connect(dir: &Path, ping_timeout: Duration) -> Result<Conn, ClientError> {
    let text = std::fs::read_to_string(dir.join("control.json")).map_err(|_| ClientError::NotRunning)?;
    let disc = parse_discovery(&text).ok_or(ClientError::NotRunning)?;
    let stream = open_socket(&disc.socket).map_err(|e| {
        if e.kind() == std::io::ErrorKind::Unsupported {
            ClientError::Unsupported
        } else {
            ClientError::NotRunning // refused / missing socket = stale file
        }
    })?;
    let _ = stream.set_read_timeout(Some(POLL));
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let mut conn =
        Conn { stream, buf: Vec::new(), token: disc.token, next_id: 0, dead: false, ping: Value::Null };
    let deadline = Instant::now() + ping_timeout;
    match conn.call_inner("control.ping", json!({}), &mut |_| {}, &|| false, Some(deadline)) {
        Ok(v) => {
            conn.ping = v;
            Ok(conn)
        }
        Err(e) if e.code == "control.bad_token" => Err(ClientError::BadToken),
        // No answer / closed / garbage: not our app (pid reused, socket of a dead
        // process that something else holds) — same as no app.
        Err(_) => Err(ClientError::NotRunning),
    }
}

/// Start the app in the background: `open -g -b io.celox.inspector-rust`, or
/// `open -g "$IR_RECORD_APP"` when that is set. Bounded by `timeout`.
pub fn launch_app(timeout: Duration) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("starting the app automatically is only supported on macOS".into());
    }
    let mut cmd = std::process::Command::new("/usr/bin/open");
    cmd.arg("-g");
    match std::env::var_os("IR_RECORD_APP").filter(|a| !a.is_empty()) {
        Some(app) => cmd.arg(app),
        None => cmd.args(["-b", BUNDLE_ID]),
    };
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("open: {e}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(st)) if st.success() => return Ok(()),
            Ok(Some(_)) => {
                let mut err = String::new();
                if let Some(mut s) = child.stderr.take() {
                    let _ = s.read_to_string(&mut err);
                }
                let err = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
                return Err(if err.is_empty() { "the app is not installed".into() } else { err.into() });
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("open did not return".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Connect to the running app; with `launch`, start it when needed and wait up
/// to `timeout` for its socket.
pub fn connect(launch: bool, timeout: Duration) -> Result<Conn, ClientError> {
    let dir = data_dir().ok_or(ClientError::NotRunning)?;
    connect_with(&dir, launch, timeout, &|t| launch_app(t))
}

/// [`connect`] with an explicit data dir and launcher (tests).
pub fn connect_with(
    dir: &Path,
    launch: bool,
    timeout: Duration,
    launcher: &dyn Fn(Duration) -> Result<(), String>,
) -> Result<Conn, ClientError> {
    let start = Instant::now();
    let deadline = start + timeout;
    let ping_for = |now: Instant| PING_TIMEOUT.min(deadline.saturating_duration_since(now));
    match try_connect(dir, ping_for(start)) {
        Ok(c) => return Ok(c),
        Err(ClientError::Unsupported) => return Err(ClientError::Unsupported),
        Err(e) if !launch => return Err(e),
        Err(_) => {}
    }
    launcher(deadline.saturating_duration_since(Instant::now())).map_err(ClientError::LaunchFailed)?;
    let mut last = ClientError::StartTimeout(timeout);
    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err(match last {
                ClientError::BadToken => ClientError::BadToken,
                _ => ClientError::StartTimeout(timeout),
            });
        }
        std::thread::sleep(RETRY.min(deadline - now));
        match try_connect(dir, ping_for(Instant::now())) {
            Ok(c) => return Ok(c),
            Err(e) => last = e,
        }
    }
}

// ---------------------------------------------------------------------------
// Ctrl+C

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_sigint(_sig: i32) {
    // Only an atomic store: async-signal-safe.
    INTERRUPTED.store(true, Ordering::SeqCst);
}

#[cfg(unix)]
extern "C" {
    // From the C library std already links; avoids a `libc` dependency.
    fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
}

/// SIGINT sets a flag (see [`interrupted`]) instead of killing the process, so
/// a recording can be stopped cleanly. Installed once per process.
pub fn install_interrupt_handler() {
    #[cfg(unix)]
    {
        const SIGINT: i32 = 2;
        // SAFETY: the handler only performs an atomic store.
        unsafe {
            signal(SIGINT, on_sigint);
        }
    }
}

/// Whether Ctrl+C was pressed since start (or the last [`set_interrupted`]).
pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

/// Set/clear the flag (tests; also lets callers simulate Ctrl+C).
pub fn set_interrupted(v: bool) {
    INTERRUPTED.store(v, Ordering::SeqCst);
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    use crate::test_support::*;

    fn no_launch(_: Duration) -> Result<(), String> {
        panic!("must not launch")
    }

    #[test]
    fn ping_ok_and_calls_work() {
        let d = td();
        fake_server(&d.0, Arc::default());
        let mut c = connect_with(&d.0, false, CONNECT_TIMEOUT, &no_launch).unwrap();
        assert_eq!(c.ping["version"], "9.9.9");
        let mut evs = Vec::new();
        let r = c.call("t.events", json!({}), &mut |e| evs.push(e)).unwrap();
        assert_eq!(r, json!({"path": "/x.mp4"}));
        // Events arrive, in order, before the result.
        assert_eq!(evs, vec![json!({"frame": 0}), json!({"frame": 1}), json!({"frame": 2})]);
        let e = c.call("t.fail", json!({}), &mut |_| {}).unwrap_err();
        assert_eq!(e.code, "record.busy");
        // The connection survives an error answer.
        assert!(c.call("t.events", json!({}), &mut |_| {}).is_ok());
    }

    #[test]
    fn wrong_token_is_an_error() {
        let d = td();
        let sock = fake_server(&d.0, Arc::default());
        write_disc(&d.0, &sock, "ffffffffffffffffffffffffffffffff");
        let e = connect_with(&d.0, false, CONNECT_TIMEOUT, &no_launch).err().unwrap();
        assert_eq!(e, ClientError::BadToken);
    }

    #[test]
    fn stale_file_with_dead_socket_is_not_running_fast() {
        let d = td();
        // A socket file nobody listens on any more (the app crashed) + a
        // reused pid: our own, which is certainly alive.
        let sock = d.0.join("control.sock");
        drop(UnixListener::bind(&sock).unwrap());
        assert!(sock.exists());
        let disc = json!({"pid": std::process::id(), "socket": sock.to_string_lossy(), "token": TOKEN});
        std::fs::write(d.0.join("control.json"), disc.to_string()).unwrap();
        let t = Instant::now();
        let e = connect_with(&d.0, false, CONNECT_TIMEOUT, &no_launch).err().unwrap();
        assert_eq!(e, ClientError::NotRunning);
        assert!(t.elapsed() < Duration::from_secs(1), "took {:?}", t.elapsed());
    }

    #[test]
    fn a_socket_that_never_answers_is_not_running() {
        // Something accepts on the path but never speaks the protocol: the pid
        // check would pass, only the ping tells.
        let d = td();
        let sock = d.0.join("control.sock");
        let l = UnixListener::bind(&sock).unwrap();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for c in l.incoming().flatten() {
                held.push(c);
            }
        });
        write_disc(&d.0, &sock, TOKEN);
        let t = Instant::now();
        let e = connect_with(&d.0, false, CONNECT_TIMEOUT, &no_launch).err().unwrap();
        assert_eq!(e, ClientError::NotRunning);
        assert!(t.elapsed() < Duration::from_secs(4), "took {:?}", t.elapsed());
    }

    #[test]
    fn missing_file_is_not_running() {
        let d = td();
        let e = connect_with(&d.0, false, CONNECT_TIMEOUT, &no_launch).err().unwrap();
        assert_eq!(e, ClientError::NotRunning);
    }

    #[test]
    fn stale_file_launches_and_waits_for_the_app() {
        let d = td();
        write_disc(&d.0, &d.0.join("gone.sock"), TOKEN);
        let calls = AtomicUsize::new(0);
        let dir = d.0.clone();
        let launcher = |_: Duration| {
            calls.fetch_add(1, Ordering::SeqCst);
            let dir = dir.clone();
            // The "app" comes up a little later.
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(400));
                fake_server(&dir, Arc::default());
            });
            Ok(())
        };
        let c = connect_with(&d.0, true, CONNECT_TIMEOUT, &launcher).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(c.ping["platform"], "macos");
    }

    #[test]
    fn a_failing_launch_fails_fast_with_a_readable_line() {
        let d = td();
        let t = Instant::now();
        let e = connect_with(&d.0, true, CONNECT_TIMEOUT, &|_| Err("not installed".into())).err().unwrap();
        assert_eq!(e, ClientError::LaunchFailed("not installed".into()));
        assert!(t.elapsed() < Duration::from_secs(1));
        assert_eq!(
            e.to_string(),
            "Inspector Rust is not running and could not be started (not installed)"
        );
    }

    #[test]
    fn a_launched_app_that_never_answers_times_out() {
        let d = td();
        let t = Instant::now();
        let e = connect_with(&d.0, true, Duration::from_millis(700), &|_| Ok(())).err().unwrap();
        assert!(matches!(e, ClientError::StartTimeout(_)));
        let el = t.elapsed();
        assert!(el >= Duration::from_millis(650) && el < Duration::from_secs(2), "{el:?}");
        assert!(e.to_string().starts_with("Inspector Rust is not running and could not be started"));
    }

    #[test]
    fn interrupting_a_call_closes_the_connection_without_panicking() {
        let d = td();
        let closed = Arc::new(AtomicUsize::new(0));
        fake_server(&d.0, closed.clone());
        let mut c = connect_with(&d.0, false, CONNECT_TIMEOUT, &no_launch).unwrap();
        let stop = AtomicBool::new(false);
        let mut evs = 0;
        let e = c
            .call_until("t.slow", json!({}), &mut |_| {
                evs += 1;
                stop.store(true, Ordering::SeqCst); // "Ctrl+C" after the first event
            }, &|| stop.load(Ordering::SeqCst))
            .unwrap_err();
        assert_eq!(e.code, ERR_INTERRUPTED);
        assert_eq!(evs, 1);
        // The server sees the close (that is what cancels an export in the app).
        let t = Instant::now();
        while closed.load(Ordering::SeqCst) == 0 && t.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(closed.load(Ordering::SeqCst), 1);
        // Later calls on the dead connection answer an error, never panic.
        assert_eq!(c.call("t.events", json!({}), &mut |_| {}).unwrap_err().code, ERR_CLOSED);
    }

    #[test]
    fn the_app_going_away_mid_call_is_an_error() {
        let d = td();
        let sock = d.0.join("control.sock");
        let l = UnixListener::bind(&sock).unwrap();
        write_disc(&d.0, &sock, TOKEN);
        std::thread::spawn(move || {
            let (c, _) = l.accept().unwrap();
            let mut out = c.try_clone().unwrap();
            let mut r = BufReader::new(c);
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let id = serde_json::from_str::<Value>(&line).unwrap()["id"].clone();
            writeln!(out, "{}", json!({"id": id, "result": {}})).unwrap();
            line.clear();
            r.read_line(&mut line).unwrap();
            // half a line, then gone
            write!(out, "{{\"id\":2,\"res").unwrap();
        });
        let mut c = connect_with(&d.0, false, CONNECT_TIMEOUT, &no_launch).unwrap();
        let e = c.call("x", json!({}), &mut |_| {}).unwrap_err();
        assert_eq!(e.code, ERR_CLOSED);
    }

    #[test]
    fn incoming_lines_are_classified() {
        assert_eq!(parse_incoming(r#"{"id":3,"event":{"a":1}}"#, 3).unwrap(), Incoming::Event(json!({"a":1})));
        assert_eq!(parse_incoming(r#"{"id":3,"result":null}"#, 3).unwrap(), Incoming::Result(Value::Null));
        assert_eq!(parse_incoming(r#"{"id":4,"result":1}"#, 3).unwrap(), Incoming::Other);
        assert_eq!(parse_incoming(r#"{"id":4,"error":{"code":"x"}}"#, 3).unwrap(), Incoming::Other);
        assert_eq!(
            parse_incoming(r#"{"id":null,"error":{"code":"control.bad_request","message":"m"}}"#, 3).unwrap(),
            Incoming::Error(RemoteError::new("control.bad_request", "m"))
        );
        assert_eq!(parse_incoming("nope", 3).unwrap_err().code, ERR_PROTOCOL);
        assert_eq!(parse_incoming(r#"{"id":3}"#, 3).unwrap_err().code, ERR_PROTOCOL);
    }

    #[test]
    fn discovery_and_request_shapes() {
        let d = parse_discovery(r#"{"pid":7,"socket":"/s","token":"t"}"#).unwrap();
        assert_eq!(d, Discovery { pid: 7, socket: "/s".into(), token: "t".into() });
        assert!(parse_discovery(r#"{"pid":7,"socket":"","token":"t"}"#).is_none());
        assert!(parse_discovery(r#"{"pid":7,"socket":"/s"}"#).is_none());
        assert!(parse_discovery("garbage").is_none());
        let v: Value = serde_json::from_str(&request_line(5, "tok", "m", &json!({"a":1}))).unwrap();
        assert_eq!(v, json!({"id":5,"token":"tok","method":"m","params":{"a":1}}));
    }

    #[test]
    fn data_dir_honours_the_override() {
        // Only this test touches IR_DATA_DIR.
        std::env::set_var("IR_DATA_DIR", "/tmp/ir-data-x");
        assert_eq!(data_dir(), Some(PathBuf::from("/tmp/ir-data-x")));
        std::env::remove_var("IR_DATA_DIR");
        if cfg!(target_os = "macos") {
            let home = std::env::var("HOME").unwrap();
            assert_eq!(data_dir(), Some(PathBuf::from(home).join("Library/Application Support/InspectorRust")));
        }
    }
}
