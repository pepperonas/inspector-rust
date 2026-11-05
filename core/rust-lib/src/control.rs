//! Local control socket (record stage 8): a Unix socket inside the app that the
//! `ir-record` command line and its MCP server talk to.
//!
//! Wire format, both directions one JSON object per line:
//! request `{"id":n,"token":"…","method":"…","params":{…}}`; reply
//! `{"id":n,"result":…}` or `{"id":n,"error":{"code":"…","message":"…"}}`;
//! progress `{"id":n,"event":{…}}` lines before the reply of a long method.
//!
//! Discovery: `<data dir>/control.json` `{pid, socket, token}` + `control.sock`,
//! both mode 0600, removed on exit. A wrong token answers `control.bad_token`
//! and closes the connection. When the requesting connection closes while a
//! handler runs, `Call::cancelled` flips so long handlers (export) stop.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::AppHandle;

/// Stable error sentinel (`control.*`, `record.*`).
#[derive(Debug, Clone, PartialEq)]
pub struct ControlError {
    pub code: String,
    pub message: String,
}

impl ControlError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }

    /// An app error string as a control error: a `record.*`/`control.*` sentinel (optionally
    /// followed by `:detail`) keeps its code; anything else is `record.control.failed`.
    pub fn sentinel(e: impl Into<String>) -> Self {
        let e = e.into();
        let head = e.split([':', ' ']).next().unwrap_or("");
        let is_code = (head.starts_with("record.") || head.starts_with("control."))
            && head.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
        let code = if is_code { head.to_string() } else { "record.control.failed".to_string() };
        Self { code, message: e }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub id: Value,
    pub token: String,
    pub method: String,
    pub params: Value,
}

/// What a handler gets besides the app handle.
pub struct Call<'a> {
    pub params: &'a Value,
    /// Set when the requesting connection closed; long handlers poll it.
    pub cancelled: &'a AtomicBool,
    emit: &'a mut dyn FnMut(Value),
}

impl Call<'_> {
    /// Send a progress event (`{"id":n,"event":ev}`) to the client.
    pub fn emit(&mut self, ev: Value) {
        (self.emit)(ev)
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

pub type HandlerFn<C> = fn(&C, &mut Call) -> Result<Value, ControlError>;
/// The app-level handler type.
pub type Handler = HandlerFn<AppHandle>;

// ---- pure framing ----------------------------------------------------------

pub fn parse_request(line: &str) -> Result<Request, ControlError> {
    let v: Value = serde_json::from_str(line.trim())
        .map_err(|e| ControlError::new("control.bad_request", format!("invalid JSON: {e}")))?;
    let o = v
        .as_object()
        .ok_or_else(|| ControlError::new("control.bad_request", "request must be an object"))?;
    let method = o
        .get("method")
        .and_then(Value::as_str)
        .ok_or_else(|| ControlError::new("control.bad_request", "missing method"))?;
    Ok(Request {
        id: o.get("id").cloned().unwrap_or(Value::Null),
        token: o.get("token").and_then(Value::as_str).unwrap_or("").to_string(),
        method: method.to_string(),
        params: o.get("params").cloned().unwrap_or_else(|| json!({})),
    })
}

/// The id of a (possibly malformed) request line, `null` when unknown.
pub fn request_id(line: &str) -> Value {
    serde_json::from_str::<Value>(line.trim())
        .ok()
        .and_then(|v| v.get("id").cloned())
        .unwrap_or(Value::Null)
}

pub fn reply_line(id: &Value, result: &Value) -> String {
    json!({"id": id, "result": result}).to_string()
}
pub fn error_line(id: &Value, err: &ControlError) -> String {
    json!({"id": id, "error": {"code": err.code, "message": err.message}}).to_string()
}
pub fn event_line(id: &Value, ev: &Value) -> String {
    json!({"id": id, "event": ev}).to_string()
}

/// Constant-time comparison (length is not secret: tokens are fixed-size).
pub fn token_ok(given: &str, expected: &str) -> bool {
    let (a, b) = (given.as_bytes(), expected.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn discovery_json(pid: u32, socket: &str, token: &str) -> Value {
    json!({"pid": pid, "socket": socket, "token": token})
}

// ---- registry --------------------------------------------------------------

type AppTable = Arc<Mutex<HashMap<&'static str, Handler>>>;
static APP_TABLE: OnceLock<AppTable> = OnceLock::new();

fn app_table() -> &'static AppTable {
    APP_TABLE.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

/// Register an app-level method (later tasks call this before/after `start`).
pub fn register(method: &'static str, h: Handler) {
    app_table().lock().unwrap().insert(method, h);
}

#[cfg(unix)]
mod server {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    /// Longest request line accepted (a document edit fits many times over).
    pub(super) const MAX_LINE: usize = 4 << 20;
    /// How long a write to a client may block before the call counts as cancelled.
    const WRITE_TIMEOUT: Duration = Duration::from_secs(10);

    pub type Table<C> = Arc<Mutex<HashMap<&'static str, HandlerFn<C>>>>;

    pub struct Running {
        pub dir: PathBuf,
        pub stop: Arc<AtomicBool>,
    }

    fn random_token() -> String {
        use rand::RngCore;
        let mut b = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut b);
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        use std::io::Write as _;
        let tmp = path.with_extension("tmp");
        let _ = std::fs::remove_file(&tmp);
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode_0600()
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    }

    trait Mode0600 {
        fn mode_0600(&mut self) -> &mut Self;
    }
    impl Mode0600 for std::fs::OpenOptions {
        fn mode_0600(&mut self) -> &mut Self {
            use std::os::unix::fs::OpenOptionsExt;
            self.mode(0o600)
        }
    }

    /// Start the server in `dir`. Returns the token and the running handle.
    pub fn start_in<C: Send + Sync + 'static>(
        dir: &Path,
        ctx: Arc<C>,
        table: Table<C>,
    ) -> std::io::Result<(String, Running)> {
        std::fs::create_dir_all(dir)?;
        let sock = dir.join("control.sock");
        // A stale socket file from a crashed run is replaced.
        let _ = std::fs::remove_file(&sock);
        let listener = UnixListener::bind(&sock)?;
        std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600))?;
        let token = random_token();
        let disc = discovery_json(std::process::id(), &sock.to_string_lossy(), &token);
        write_private(&dir.join("control.json"), disc.to_string().as_bytes())?;

        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let token2 = token.clone();
        std::thread::Builder::new().name("ir-control".into()).spawn(move || {
            for conn in listener.incoming() {
                if stop2.load(Ordering::Relaxed) {
                    break;
                }
                // A failing accept (EMFILE: out of file descriptors) fails again at
                // once — without a pause this loop would spin a core at 100 %.
                let Ok(conn) = conn else {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                };
                let (ctx, table, token) = (ctx.clone(), table.clone(), token2.clone());
                let _ = std::thread::Builder::new()
                    .name("ir-control-conn".into())
                    .spawn(move || serve_conn(conn, &*ctx, &table, &token));
            }
        })?;
        Ok((token, Running { dir: dir.to_path_buf(), stop }))
    }

    pub fn remove_files(dir: &Path) {
        let _ = std::fs::remove_file(dir.join("control.sock"));
        let _ = std::fs::remove_file(dir.join("control.json"));
    }

    fn send(w: &mut UnixStream, line: &str) -> bool {
        w.write_all(line.as_bytes()).is_ok() && w.write_all(b"\n").is_ok()
    }

    fn serve_conn<C>(conn: UnixStream, ctx: &C, table: &Table<C>, token: &str) {
        let Ok(mut out) = conn.try_clone() else { return };
        // A client that stops reading must not wedge a handler thread in `send`/`emit`;
        // the timeout is socket-wide, so it covers every clone.
        let _ = out.set_write_timeout(Some(WRITE_TIMEOUT));
        let Ok(read_half) = conn.try_clone() else { return };
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        loop {
            line.clear();
            match (&mut reader).take(MAX_LINE as u64).read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(n) if n >= MAX_LINE && !line.ends_with('\n') => {
                    let e = ControlError::new("control.bad_request", "request line too long");
                    let _ = send(&mut out, &error_line(&Value::Null, &e));
                    return;
                }
                Ok(_) => {}
            }
            if line.trim().is_empty() {
                continue;
            }
            let req = match parse_request(&line) {
                Ok(r) => r,
                Err(e) => {
                    if !send(&mut out, &error_line(&request_id(&line), &e)) {
                        return;
                    }
                    continue;
                }
            };
            if !token_ok(&req.token, token) {
                let e = ControlError::new("control.bad_token", "missing or wrong token");
                let _ = send(&mut out, &error_line(&req.id, &e));
                return; // the connection is closed
            }
            let handler = table.lock().unwrap().get(req.method.as_str()).copied();
            let Some(handler) = handler else {
                let e = ControlError::new(
                    "control.unknown_method",
                    format!("unknown method {}", req.method),
                );
                if !send(&mut out, &error_line(&req.id, &e)) {
                    return;
                }
                continue;
            };

            let cancelled = Arc::new(AtomicBool::new(false));
            let done = Arc::new(AtomicBool::new(false));
            let (Ok(watch_conn), Ok(mut emit_out)) = (conn.try_clone(), out.try_clone()) else { return };
            let watcher = {
                let (c, d) = (cancelled.clone(), done.clone());
                std::thread::spawn(move || watch_close(watch_conn, &c, &d))
            };
            let id = req.id.clone();
            let emit_cancel = cancelled.clone();
            let mut emit = |ev: Value| {
                // A client that can't take events any more (gone, or stuck past the write
                // timeout) cancels the call, like a closed connection.
                if !send(&mut emit_out, &event_line(&id, &ev)) {
                    emit_cancel.store(true, Ordering::Relaxed);
                }
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut call = Call { params: &req.params, cancelled: &cancelled, emit: &mut emit };
                handler(ctx, &mut call)
            }));
            done.store(true, Ordering::Relaxed);
            let _ = watcher.join();
            let _ = conn.set_read_timeout(None);
            let reply = match result {
                Ok(Ok(v)) => reply_line(&req.id, &v),
                Ok(Err(e)) => error_line(&req.id, &e),
                Err(_) => error_line(
                    &req.id,
                    &ControlError::new("control.internal", "handler panicked"),
                ),
            };
            if !send(&mut out, &reply) || cancelled.load(Ordering::Relaxed) {
                return;
            }
        }
    }

    /// Flips `cancelled` when the peer closes. Peeks with a short timeout so it
    /// never consumes pipelined requests; stops once `done` is set.
    fn watch_close(conn: UnixStream, cancelled: &AtomicBool, done: &AtomicBool) {
        let _ = conn.set_read_timeout(Some(Duration::from_millis(100)));
        use std::os::unix::io::AsRawFd;
        let mut b = [0u8; 1];
        while !done.load(Ordering::Relaxed) {
            // MSG_PEEK never consumes pipelined requests; SO_RCVTIMEO bounds the wait.
            let n = unsafe {
                libc::recv(conn.as_raw_fd(), b.as_mut_ptr().cast(), 1, libc::MSG_PEEK)
            };
            if n == 0 {
                cancelled.store(true, Ordering::Relaxed);
                return;
            } else if n > 0 {
                std::thread::sleep(Duration::from_millis(50));
            } else {
                let e = std::io::Error::last_os_error();
                if !matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) {
                    cancelled.store(true, Ordering::Relaxed);
                    return;
                }
            }
        }
    }
}

fn ping(app: &AppHandle, _c: &mut Call) -> Result<Value, ControlError> {
    Ok(json!({"version": app.package_info().version.to_string(), "platform": std::env::consts::OS}))
}

#[cfg(unix)]
static RUNNING: Mutex<Option<server::Running>> = Mutex::new(None);

fn data_dir() -> Option<std::path::PathBuf> {
    if let Ok(d) = std::env::var("IR_DATA_DIR") {
        if !d.is_empty() {
            return Some(d.into());
        }
    }
    Some(dirs::data_dir()?.join("InspectorRust"))
}

/// Start the control socket. Failure is for the caller to log, never fatal.
#[cfg(unix)]
pub fn start(app: AppHandle) -> std::io::Result<()> {
    let dir = data_dir().ok_or_else(|| std::io::Error::other("no data dir"))?;
    register("control.ping", ping);
    // The live table is shared: methods registered after `start` are served too.
    let (_t, running) = server::start_in(&dir, Arc::new(app), app_table().clone())?;
    *RUNNING.lock().unwrap() = Some(running);
    Ok(())
}

#[cfg(not(unix))]
pub fn start(_app: AppHandle) -> std::io::Result<()> {
    Ok(())
}

pub fn stop() {
    #[cfg(unix)]
    if let Some(r) = RUNNING.lock().unwrap().take() {
        r.stop.store(true, Ordering::Relaxed);
        server::remove_files(&r.dir);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::time::Duration;

    fn table() -> server::Table<()> {
        let mut m: HashMap<&'static str, HandlerFn<()>> = HashMap::new();
        m.insert("t.echo", |_, c| Ok(c.params.clone()));
        m.insert("t.panic", |_, _| panic!("boom"));
        m.insert("t.events", |_, c| {
            c.emit(json!({"phase": "a"}));
            c.emit(json!({"phase": "b"}));
            Ok(json!("done"))
        });
        m.insert("t.wait", |_, c| {
            for _ in 0..100 {
                if c.is_cancelled() {
                    FLAG.store(true, Ordering::SeqCst);
                    return Ok(json!("cancelled"));
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            Ok(json!("timeout"))
        });
        Arc::new(Mutex::new(m))
    }
    struct Td(std::path::PathBuf);
    impl Td {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for Td {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn td() -> Td {
        use std::sync::atomic::AtomicUsize;
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "ir-control-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Td(p)
    }
    static FLAG: AtomicBool = AtomicBool::new(false);

    fn up(dir: &std::path::Path) -> (String, UnixStream) {
        let (tok, _r) = server::start_in(dir, Arc::new(()), table()).unwrap();
        let s = UnixStream::connect(dir.join("control.sock")).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        (tok, s)
    }
    fn rt(s: &mut UnixStream, r: &mut BufReader<UnixStream>, line: &str) -> Value {
        s.write_all(format!("{line}\n").as_bytes()).unwrap();
        let mut o = String::new();
        r.read_line(&mut o).unwrap();
        serde_json::from_str(&o).unwrap()
    }

    #[test]
    fn framing_round_trip() {
        let r = parse_request(r#"{"id":3,"token":"t","method":"m","params":{"a":1}}"#).unwrap();
        assert_eq!((r.id, r.token.as_str(), r.method.as_str()), (json!(3), "t", "m"));
        assert_eq!(r.params, json!({"a": 1}));
        let v: Value = serde_json::from_str(&reply_line(&json!(3), &json!({"x": 1}))).unwrap();
        assert_eq!(v, json!({"id": 3, "result": {"x": 1}}));
        let e = ControlError::new("record.x", "msg");
        let v: Value = serde_json::from_str(&error_line(&json!(4), &e)).unwrap();
        assert_eq!(v["error"], json!({"code": "record.x", "message": "msg"}));
        let v: Value = serde_json::from_str(&event_line(&json!(5), &json!({"p": 1}))).unwrap();
        assert_eq!(v, json!({"id": 5, "event": {"p": 1}}));
    }

    #[test]
    fn bad_json_and_missing_method() {
        assert_eq!(parse_request("{nope").unwrap_err().code, "control.bad_request");
        assert_eq!(parse_request("[1]").unwrap_err().code, "control.bad_request");
        assert_eq!(parse_request(r#"{"id":1}"#).unwrap_err().code, "control.bad_request");
        assert_eq!(request_id("{nope"), Value::Null);
        assert_eq!(request_id(r#"{"id":9}"#), json!(9));
    }

    #[test]
    fn sentinel_keeps_codes_and_wraps_text() {
        assert_eq!(ControlError::sentinel("record.editor.no_session").code, "record.editor.no_session");
        assert_eq!(ControlError::sentinel("record.session.invalid:gone").code, "record.session.invalid");
        assert_eq!(ControlError::sentinel("render helper is gone").code, "record.control.failed");
        assert_eq!(ControlError::sentinel("record helper: x").code, "record.control.failed");
    }

    #[test]
    fn token_compare() {
        assert!(token_ok("abcd", "abcd"));
        assert!(!token_ok("abcd", "abce"));
        assert!(!token_ok("abc", "abcd"));
        assert!(!token_ok("", "abcd"));
    }

    #[test]
    fn discovery_shape() {
        assert_eq!(
            discovery_json(7, "/s", "tok"),
            json!({"pid": 7, "socket": "/s", "token": "tok"})
        );
    }

    #[test]
    fn files_are_private_and_token_is_hex32() {
        let d = td();
        let (tok, _s) = up(d.path());
        assert_eq!(tok.len(), 32);
        assert!(tok.chars().all(|c| c.is_ascii_hexdigit()));
        for f in ["control.sock", "control.json"] {
            let m = std::fs::metadata(d.path().join(f)).unwrap().permissions().mode();
            assert_eq!(m & 0o777, 0o600, "{f}");
        }
        let disc: Value =
            serde_json::from_slice(&std::fs::read(d.path().join("control.json")).unwrap()).unwrap();
        assert_eq!(disc["token"], json!(tok));
        assert_eq!(disc["pid"], json!(std::process::id()));
    }

    #[test]
    fn stale_socket_file_is_replaced() {
        let d = td();
        std::fs::write(d.path().join("control.sock"), b"stale").unwrap();
        let (tok, mut s) = up(d.path());
        let mut r = BufReader::new(s.try_clone().unwrap());
        let v = rt(&mut s, &mut r, &format!(r#"{{"id":1,"token":"{tok}","method":"t.echo","params":5}}"#));
        assert_eq!(v["result"], json!(5));
    }

    #[test]
    fn dispatch_errors_and_events() {
        let d = td();
        let (tok, mut s) = up(d.path());
        let mut r = BufReader::new(s.try_clone().unwrap());
        let v = rt(&mut s, &mut r, "{garbage");
        assert_eq!((v["id"].clone(), v["error"]["code"].clone()), (Value::Null, json!("control.bad_request")));
        let v = rt(&mut s, &mut r, &format!(r#"{{"id":2,"token":"{tok}","method":"nope"}}"#));
        assert_eq!(v["error"]["code"], json!("control.unknown_method"));
        // a panicking handler answers control.internal and the connection keeps serving
        let v = rt(&mut s, &mut r, &format!(r#"{{"id":3,"token":"{tok}","method":"t.panic"}}"#));
        assert_eq!(v["error"]["code"], json!("control.internal"));
        // events come before the reply
        s.write_all(format!("{{\"id\":4,\"token\":\"{tok}\",\"method\":\"t.events\"}}\n").as_bytes()).unwrap();
        let mut lines = vec![];
        for _ in 0..3 {
            let mut o = String::new();
            r.read_line(&mut o).unwrap();
            lines.push(serde_json::from_str::<Value>(&o).unwrap());
        }
        assert_eq!(lines[0]["event"], json!({"phase": "a"}));
        assert_eq!(lines[1]["event"], json!({"phase": "b"}));
        assert_eq!(lines[2]["result"], json!("done"));
    }

    #[test]
    fn an_endless_line_is_refused_not_buffered() {
        let d = td();
        let (_tok, mut s) = up(d.path());
        let mut r = BufReader::new(s.try_clone().unwrap());
        let big = vec![b'x'; server::MAX_LINE + 10];
        let _ = s.write_all(&big); // the server may close before taking it all
        let mut o = String::new();
        r.read_line(&mut o).unwrap();
        let v: Value = serde_json::from_str(&o).unwrap();
        assert_eq!(v["error"]["code"], json!("control.bad_request"));
        o.clear();
        assert_eq!(r.read_line(&mut o).unwrap(), 0, "the connection is closed");
    }

    #[test]
    fn wrong_or_missing_token_closes_the_connection() {
        let d = td();
        let (_tok, mut s) = up(d.path());
        let mut r = BufReader::new(s.try_clone().unwrap());
        let v = rt(&mut s, &mut r, r#"{"id":1,"token":"wrong","method":"t.echo"}"#);
        assert_eq!(v["error"]["code"], json!("control.bad_token"));
        let mut o = String::new();
        assert_eq!(r.read_line(&mut o).unwrap(), 0, "connection closed");
        let d2 = td();
        let (_tok, mut s) = up(d2.path());
        let mut r = BufReader::new(s.try_clone().unwrap());
        let v = rt(&mut s, &mut r, r#"{"id":1,"method":"t.echo"}"#);
        assert_eq!(v["error"]["code"], json!("control.bad_token"));
    }

    #[test]
    fn closing_the_connection_cancels_the_running_handler() {
        let d = td();
        let (tok, mut s) = up(d.path());
        FLAG.store(false, Ordering::SeqCst);
        s.write_all(format!("{{\"id\":1,\"token\":\"{tok}\",\"method\":\"t.wait\"}}\n").as_bytes()).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        drop(s);
        for _ in 0..50 {
            if FLAG.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("handler was not cancelled");
    }
}
