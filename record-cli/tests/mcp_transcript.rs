//! `ir-record mcp` end to end: the real binary on stdio against a fake of the
//! app's control socket (`IR_DATA_DIR`). `IR_RECORD_APP` points at a missing
//! bundle so no test can ever start the real app.
#![cfg(unix)]

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

const TOKEN: &str = "fedcba9876543210fedcba9876543210";
const WAIT: Duration = Duration::from_secs(10);

struct Td(PathBuf);
impl Drop for Td {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn td() -> Td {
    static N: AtomicUsize = AtomicUsize::new(0);
    // Short: unix socket paths are limited to ~104 bytes on macOS.
    let p = PathBuf::from(format!("/tmp/irm-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    Td(p)
}

/// Counts what the fake saw: connections closed during an export, methods called.
#[derive(Default)]
struct Seen {
    export_closed: AtomicUsize,
    calls: std::sync::Mutex<Vec<(String, Value)>>,
}

fn fake_app(dir: &Path) -> Arc<Seen> {
    let seen = Arc::new(Seen::default());
    let sock = dir.join("control.sock");
    let l = UnixListener::bind(&sock).unwrap();
    let d = json!({"pid": 999_999, "socket": sock.to_string_lossy(), "token": TOKEN});
    std::fs::write(dir.join("control.json"), d.to_string()).unwrap();
    let s = seen.clone();
    std::thread::spawn(move || {
        for c in l.incoming() {
            let Ok(c) = c else { return };
            let s = s.clone();
            std::thread::spawn(move || serve(c, s));
        }
    });
    seen
}

fn serve(c: UnixStream, seen: Arc<Seen>) {
    let mut out = c.try_clone().unwrap();
    let mut r = BufReader::new(c.try_clone().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let v: Value = serde_json::from_str(&line).unwrap();
        let id = v["id"].clone();
        let send = |o: &mut UnixStream, x: Value| writeln!(o, "{x}").is_ok();
        assert_eq!(v["token"], TOKEN);
        let method = v["method"].as_str().unwrap().to_string();
        if method != "control.ping" {
            seen.calls.lock().unwrap().push((method.clone(), v["params"].clone()));
        }
        match method.as_str() {
            "control.ping" => {
                send(&mut out, json!({"id": id, "result": {"version": "9.9.9", "platform": "macos"}}));
            }
            "record.projects" => {
                send(&mut out, json!({"id": id, "result": [{"folder": "take", "title": "Demo", "durationMs": 4000}]}));
            }
            "record.doc" if v["params"]["project"] == "busy" => {
                send(&mut out, json!({"id": id, "error": {"code": "record.control.other_open", "message": "another project is open"}}));
            }
            "record.doc" => {
                send(&mut out, json!({"id": id, "result": {"doc": {"zooms": [{"id": "z1"}]}, "view": {"durationMs": 4000}}}));
            }
            "record.frames" => {
                send(&mut out, json!({"id": id, "result": [{"atProgrammeMs": 0, "jpegBase64": "/9j/AA=="}, {"atProgrammeMs": 1000, "jpegBase64": "/9j/BB=="}]}));
            }
            "record.export" => {
                send(&mut out, json!({"id": id, "event": {"phase": "export", "frame": 1, "total": 10}}));
                send(&mut out, json!({"id": id, "event": {"phase": "export", "frame": 2, "total": 10}}));
                let _ = c.set_read_timeout(Some(Duration::from_secs(5)));
                let mut b = [0u8; 1];
                if matches!((&c).read(&mut b), Ok(0)) {
                    seen.export_closed.fetch_add(1, Ordering::SeqCst);
                    return;
                }
                send(&mut out, json!({"id": id, "result": {"path": "/x.mp4"}}));
            }
            "record.edit" => {
                send(&mut out, json!({"id": id, "result": {"view": {}, "created": []}}));
            }
            m => {
                send(&mut out, json!({"id": id, "error": {"code": "control.unknown_method", "message": m}}));
            }
        }
    }
}

struct Mcp {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<Value>,
}

impl Mcp {
    fn start(data_dir: &Path) -> Mcp {
        let mut child = Command::new(env!("CARGO_BIN_EXE_ir-record"))
            .arg("mcp")
            .env("IR_DATA_DIR", data_dir)
            .env("IR_RECORD_APP", "/nonexistent/IR-Test.app")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for l in BufReader::new(stdout).lines() {
                let Ok(l) = l else { return };
                let v: Value = serde_json::from_str(&l).unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {l}"));
                if tx.send(v).is_err() {
                    return;
                }
            }
        });
        Mcp { stdin: child.stdin.take(), child, lines: rx }
    }

    fn send(&mut self, v: Value) {
        let s = self.stdin.as_mut().unwrap();
        writeln!(s, "{v}").unwrap();
        s.flush().unwrap();
    }

    fn next(&self) -> Value {
        self.lines.recv_timeout(WAIT).expect("no line from ir-record mcp")
    }

    /// The reply for `id`; other lines (notifications) are collected into `skipped`.
    fn reply(&self, id: &Value, skipped: &mut Vec<Value>) -> Value {
        loop {
            let v = self.next();
            if &v["id"] == id && v.get("method").is_none() {
                return v;
            }
            skipped.push(v);
        }
    }

    fn request(&mut self, id: i64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        self.reply(&json!(id), &mut Vec::new())
    }

    fn call(&mut self, id: i64, name: &str, args: Value) -> Value {
        self.request(id, "tools/call", json!({"name": name, "arguments": args}))
    }

    /// Close stdin; the process must exit with 0. Returns the remaining lines.
    fn finish(mut self) -> Vec<Value> {
        drop(self.stdin.take());
        let start = Instant::now();
        let status = loop {
            if let Some(st) = self.child.try_wait().unwrap() {
                break st;
            }
            assert!(start.elapsed() < WAIT, "ir-record mcp did not exit after EOF");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(status.code(), Some(0));
        let mut rest = Vec::new();
        while let Ok(v) = self.lines.recv_timeout(Duration::from_millis(300)) {
            rest.push(v);
        }
        rest
    }
}

fn handshake(m: &mut Mcp, version: &str) -> Value {
    let r = m.request(
        1,
        "initialize",
        json!({"protocolVersion": version, "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}),
    );
    m.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    r
}

#[test]
fn handshake_tools_list_and_a_call() {
    let d = td();
    let seen = fake_app(&d.0);
    let mut m = Mcp::start(&d.0);
    let init = handshake(&mut m, "2025-06-18");
    assert_eq!(init["jsonrpc"], "2.0");
    let r = &init["result"];
    assert_eq!(r["protocolVersion"], "2025-06-18");
    assert_eq!(r["capabilities"], json!({"tools": {}}));
    assert_eq!(r["serverInfo"]["name"], "inspector-rust-record");
    assert!(r["instructions"].as_str().unwrap().contains("record_doc"));

    assert_eq!(m.request(2, "ping", json!({}))["result"], json!({}));

    let list = m.request(3, "tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 17);
    for t in tools {
        assert!(t["name"].as_str().unwrap().starts_with("record_"));
        assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
        assert!(t["inputSchema"]["properties"].is_object(), "{}", t["name"]);
        assert!(t["annotations"]["readOnlyHint"].is_boolean());
    }

    let r = m.call(4, "record_projects", json!({}));
    assert!(r["result"].get("isError").is_none(), "{r}");
    assert_eq!(r["result"]["structuredContent"]["projects"][0]["folder"], "take");
    let text: Value = serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, r["result"]["structuredContent"]);

    // Auto-zoom is an edit with one zoom.auto op.
    let r = m.call(5, "record_auto_zoom", json!({"project": "take"}));
    assert!(r["result"].get("isError").is_none(), "{r}");
    let calls = seen.calls.lock().unwrap().clone();
    assert_eq!(calls[0].0, "record.projects");
    assert_eq!(calls[1], ("record.edit".to_string(), json!({"project": "take", "ops": [{"op": "zoom.auto"}]})));

    assert!(m.finish().is_empty());
}

#[test]
fn version_negotiation() {
    let d = td();
    for (asked, answered) in [("2024-11-05", "2024-11-05"), ("2025-03-26", "2025-03-26"), ("2030-01-01", "2025-06-18")] {
        let mut m = Mcp::start(&d.0);
        let r = handshake(&mut m, asked);
        assert_eq!(r["result"]["protocolVersion"], answered, "asked {asked}");
        let list = m.request(2, "tools/list", json!({}));
        let annotated = list["result"]["tools"][0].get("annotations").is_some();
        assert_eq!(annotated, asked != "2024-11-05", "asked {asked}");
        m.finish();
    }
}

#[test]
fn frames_come_back_as_jpeg_images() {
    let d = td();
    let _seen = fake_app(&d.0);
    let mut m = Mcp::start(&d.0);
    handshake(&mut m, "2025-06-18");
    let r = m.call(2, "record_frames", json!({"project": "take", "atProgrammeMs": [0, 1000], "maxWidth": 320}));
    let c = r["result"]["content"].as_array().unwrap();
    assert_eq!(c[0]["type"], "text");
    assert_eq!(c[1], json!({"type": "image", "data": "/9j/AA==", "mimeType": "image/jpeg"}));
    assert_eq!(c[2]["data"], "/9j/BB==");
    m.finish();
}

#[test]
fn errors_unknown_tools_and_bad_arguments() {
    let d = td();
    let seen = fake_app(&d.0);
    let mut m = Mcp::start(&d.0);
    handshake(&mut m, "2025-06-18");

    let r = m.call(2, "record_nope", json!({}));
    assert_eq!(r["error"]["code"], -32602, "{r}");
    assert!(r.get("result").is_none());

    let r = m.request(3, "resources/list", json!({}));
    assert_eq!(r["error"]["code"], -32601);

    for (i, args) in [
        json!({"project": "take", "fps": "thirty"}),
        json!({"project": "take", "fps": 1e300}),
        json!({"project": -1}),
        json!({"project": "take", "extra": true}),
    ]
    .into_iter()
    .enumerate()
    {
        let r = m.call(10 + i as i64, "record_export", args.clone());
        assert_eq!(r["result"]["isError"], true, "{args}");
        assert_eq!(r["result"]["structuredContent"]["error"]["code"], "record.control.bad_params", "{args}");
    }
    let r = m.call(20, "record_frames", json!({"project": "take", "atProgrammeMs": [1, 2, 3, 4, 5, 6, 7, 8, 9]}));
    assert_eq!(r["result"]["isError"], true);

    // The app's sentinel passes through as a tool error.
    let r = m.call(21, "record_doc", json!({"project": "busy"}));
    assert_eq!(r["result"]["isError"], true);
    assert_eq!(r["result"]["structuredContent"]["error"]["code"], "record.control.other_open");
    assert_eq!(r["result"]["content"][0]["text"], "another project is open (record.control.other_open)");

    // Nothing invalid reached the app.
    let calls = seen.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].0, "record.doc");
    m.finish();
}

#[test]
fn delete_needs_confirm() {
    let d = td();
    let seen = fake_app(&d.0);
    let mut m = Mcp::start(&d.0);
    handshake(&mut m, "2025-06-18");
    for args in [json!({"project": "take"}), json!({"project": "take", "confirm": false}), json!({"project": "take", "confirm": "true"})] {
        let r = m.call(2, "record_delete", args);
        assert_eq!(r["result"]["structuredContent"]["error"]["code"], "record.control.confirm_required");
    }
    assert!(seen.calls.lock().unwrap().is_empty());
    m.finish();
}

#[test]
fn progress_is_forwarded_and_cancel_closes_the_connection() {
    let d = td();
    let seen = fake_app(&d.0);
    let mut m = Mcp::start(&d.0);
    handshake(&mut m, "2025-06-18");
    m.send(json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call",
        "params": {"name": "record_export", "arguments": {"project": "take"}, "_meta": {"progressToken": "tok"}}}));
    let p1 = m.next();
    assert_eq!(p1["method"], "notifications/progress");
    assert_eq!(p1["params"], json!({"progressToken": "tok", "progress": 1.0, "total": 10.0, "message": "export 1/10"}));
    assert_eq!(m.next()["params"]["progress"], 2.0);

    // stdin is still read while the export runs.
    assert_eq!(m.request(8, "ping", json!({}))["result"], json!({}));

    m.send(json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 7, "reason": "user"}}));
    let start = Instant::now();
    while seen.export_closed.load(Ordering::SeqCst) == 0 {
        assert!(start.elapsed() < Duration::from_secs(3), "the export connection was not closed");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(m.request(9, "ping", json!({}))["result"], json!({}));
    // A cancelled request gets no answer.
    assert!(m.finish().iter().all(|v| v["id"] != 7));
}

#[test]
fn no_progress_token_no_notifications() {
    let d = td();
    let seen = fake_app(&d.0);
    let mut m = Mcp::start(&d.0);
    handshake(&mut m, "2025-06-18");
    m.send(json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": "record_export", "arguments": {"project": "take"}}}));
    // Wait until the fake is inside the export, then end the session: EOF
    // stops the call, which closes the connection (cancelling the export).
    let start = Instant::now();
    while !seen.calls.lock().unwrap().iter().any(|c| c.0 == "record.export") {
        assert!(start.elapsed() < WAIT);
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(200));
    let rest = m.finish();
    assert!(rest.is_empty(), "{rest:?}");
    let start = Instant::now();
    while seen.export_closed.load(Ordering::SeqCst) == 0 {
        assert!(start.elapsed() < Duration::from_secs(3), "EOF did not cancel the export");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn app_not_running_is_a_tool_error() {
    let d = td(); // no control.json; the launch points at a missing bundle
    let mut m = Mcp::start(&d.0);
    handshake(&mut m, "2025-06-18");
    let started = Instant::now();
    let r = m.call(2, "record_projects", json!({}));
    assert!(started.elapsed() < Duration::from_secs(11));
    assert_eq!(r["result"]["isError"], true);
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("Inspector Rust is not running and could not be started"), "{text}");
    // status never launches: plain "not running".
    let r = m.call(3, "record_status", json!({}));
    assert_eq!(r["result"]["structuredContent"]["error"]["code"], "client.not_running");
    m.finish();
}

#[test]
fn eof_right_away_exits_zero() {
    let d = td();
    let m = Mcp::start(&d.0);
    assert!(m.finish().is_empty());
}
