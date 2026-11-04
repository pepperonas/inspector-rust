//! `ir-record mcp`: an MCP server on stdio (JSON-RPC 2.0, one message per
//! line on stdin/stdout; nothing but protocol messages on stdout, logs go to
//! stderr). Tools only — see [`crate::tools`].
//!
//! Concurrency: the main thread reads stdin and answers `initialize`, `ping`
//! and `tools/list` itself. Every `tools/call` runs on its own thread with its
//! OWN control connection, so stdin keeps being read while an export runs:
//! `notifications/cancelled` for that request flips its flag, the call's
//! `stop` sees it within 200 ms and closes the connection — which cancels the
//! method in the app (no orphaned export). A cancelled request gets no answer
//! (MCP spec). EOF on stdin = the client is gone: every running call is
//! stopped the same way (calls that finish on their own are still answered),
//! the threads are joined, exit 0. `connect` itself (≤ 10 s, app start) is
//! not interruptible; the stop is checked right after it.

use crate::client::{self, ClientError, Conn, RemoteError, CONNECT_TIMEOUT};
use crate::tools::{self, Shape, Tool};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

pub const SERVER_NAME: &str = "inspector-rust-record";
/// Newest first; [`negotiate`] answers the client's version when it is one of these.
pub const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
pub const LATEST: &str = PROTOCOL_VERSIONS[0];

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

/// Largest stdin line accepted (a frame-free request is far smaller).
const MAX_LINE: u64 = 16 << 20;

pub const INSTRUCTIONS: &str = "Drives the screen recorder of Inspector Rust, a macOS app; the app is started in the background when needed. \
Projects are named by their folder (from record_projects) or an absolute path. Read record_doc before editing: region ids and clip indices come from it, and every edit returns the new view. \
Times are programme ms (the edited timeline) unless a field says source ms (time inside one clip's recording, e.g. transcript words and `clip`+`startMs` placement). \
Use record_frames to look at the result. Only one export runs at a time; exports and transcriptions can take minutes and are cancelled when the call is cancelled. \
record_delete needs confirm: true.";

/// Pure: the protocol version to answer for the client's `protocolVersion`.
pub fn negotiate(requested: Option<&str>) -> &'static str {
    requested.and_then(|r| PROTOCOL_VERSIONS.iter().copied().find(|v| *v == r)).unwrap_or(LATEST)
}

/// Tool annotations exist from 2025-03-26 on.
pub fn has_annotations(version: &str) -> bool {
    version != "2024-11-05"
}

/// `structuredContent` exists from 2025-06-18 on.
pub fn has_structured(version: &str) -> bool {
    version == "2025-06-18"
}

/// Opens a control connection; `true` = may start the app.
pub type Connector = Arc<dyn Fn(bool) -> Result<Conn, ClientError> + Send + Sync>;

struct Shared {
    out: Mutex<Box<dyn Write + Send>>,
    version: Mutex<&'static str>,
    shutdown: AtomicBool,
    /// Running tool calls by request id (JSON text) → cancel flag.
    running: Mutex<HashMap<String, Arc<AtomicBool>>>,
    connect: Connector,
}

impl Shared {
    fn send(&self, v: &Value) {
        let mut o = self.out.lock().unwrap_or_else(|e| e.into_inner());
        // A closed stdout (client gone) is not an error worth dying for.
        let _ = writeln!(o, "{v}").and_then(|_| o.flush());
    }
    fn version(&self) -> &'static str {
        *self.version.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn reply(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn rpc_error(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// `ir-record mcp`.
pub fn run() -> i32 {
    let stdin = std::io::stdin();
    serve(stdin.lock(), Box::new(std::io::stdout()), Arc::new(|launch| client::connect(launch, CONNECT_TIMEOUT)))
}

/// The server loop: until EOF on `input`. Returns the exit code (0).
pub fn serve(mut input: impl BufRead, out: Box<dyn Write + Send>, connect: Connector) -> i32 {
    let shared = Arc::new(Shared {
        out: Mutex::new(out),
        version: Mutex::new(LATEST),
        shutdown: AtomicBool::new(false),
        running: Mutex::new(HashMap::new()),
        connect,
    });
    let mut threads: Vec<JoinHandle<()>> = Vec::new();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match (&mut input).take(MAX_LINE + 1).read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                eprintln!("ir-record mcp: reading stdin: {e}");
                break;
            }
        }
        if buf.len() as u64 > MAX_LINE && buf.last() != Some(&b'\n') {
            // Skip the rest of the oversized line.
            let mut rest = Vec::new();
            let _ = input.read_until(b'\n', &mut rest);
            shared.send(&rpc_error(&Value::Null, PARSE_ERROR, "Parse error: line too long"));
            continue;
        }
        let line = String::from_utf8_lossy(&buf);
        if line.trim().is_empty() {
            continue;
        }
        if let Some(h) = handle_line(&shared, &line) {
            threads.push(h);
        }
        threads.retain(|h| !h.is_finished());
    }
    // The client is gone: stop every running call (closes its connection,
    // which cancels the method in the app) and wait for the threads.
    shared.shutdown.store(true, Ordering::SeqCst);
    for h in threads {
        let _ = h.join();
    }
    0
}

/// One stdin line. Returns the thread of a started tool call.
fn handle_line(shared: &Arc<Shared>, line: &str) -> Option<JoinHandle<()>> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => {
            shared.send(&rpc_error(&Value::Null, PARSE_ERROR, "Parse error"));
            return None;
        }
    };
    let Some(obj) = msg.as_object() else {
        let m = if msg.is_array() { "Invalid Request: batches are not supported" } else { "Invalid Request" };
        shared.send(&rpc_error(&Value::Null, INVALID_REQUEST, m));
        return None;
    };
    let id = obj.get("id").cloned();
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        if obj.contains_key("result") || obj.contains_key("error") {
            return None; // a response to a request we never send
        }
        shared.send(&rpc_error(id.as_ref().unwrap_or(&Value::Null), INVALID_REQUEST, "Invalid Request"));
        return None;
    };
    let params = obj.get("params").cloned().unwrap_or(Value::Null);
    let Some(id) = id else {
        notification(shared, method, &params);
        return None;
    };
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        shared.send(&rpc_error(&id, INVALID_REQUEST, "Invalid Request: jsonrpc must be \"2.0\""));
        return None;
    }
    if !(params.is_null() || params.is_object()) {
        shared.send(&rpc_error(&id, INVALID_PARAMS, "Invalid params: params must be an object"));
        return None;
    }
    match method {
        "initialize" => {
            let v = negotiate(params.get("protocolVersion").and_then(Value::as_str));
            *shared.version.lock().unwrap_or_else(|e| e.into_inner()) = v;
            shared.send(&reply(&id, initialize_result(v)));
        }
        "ping" => shared.send(&reply(&id, json!({}))),
        "tools/list" => shared.send(&reply(&id, tools_list(shared.version()))),
        "tools/call" => return start_call(shared, id, &params),
        m => shared.send(&rpc_error(&id, METHOD_NOT_FOUND, &format!("Method not found: {m}"))),
    }
    None
}

fn notification(shared: &Shared, method: &str, params: &Value) {
    if method == "notifications/cancelled" {
        if let Some(rid) = params.get("requestId") {
            if let Some(flag) = shared.running.lock().unwrap_or_else(|e| e.into_inner()).get(&rid.to_string()) {
                flag.store(true, Ordering::SeqCst);
            }
        }
    }
    // notifications/initialized and anything else: nothing to do, never answered.
}

pub fn initialize_result(version: &str) -> Value {
    json!({
        "protocolVersion": version,
        "capabilities": {"tools": {}},
        "serverInfo": {"name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION")},
        "instructions": INSTRUCTIONS
    })
}

pub fn tools_list(version: &str) -> Value {
    let with = has_annotations(version);
    json!({"tools": tools::TOOLS.iter().map(|t| t.listing(with)).collect::<Vec<_>>()})
}

fn start_call(shared: &Arc<Shared>, id: Value, params: &Value) -> Option<JoinHandle<()>> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        shared.send(&rpc_error(&id, INVALID_PARAMS, "Invalid params: `name` is required"));
        return None;
    };
    let Some(tool) = tools::find(name) else {
        shared.send(&rpc_error(&id, INVALID_PARAMS, &format!("Unknown tool: {name}")));
        return None;
    };
    let args = match params.get("arguments") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(m)) => m.clone(),
        Some(_) => {
            shared.send(&rpc_error(&id, INVALID_PARAMS, "Invalid params: `arguments` must be an object"));
            return None;
        }
    };
    let token = params.get("_meta").and_then(|m| m.get("progressToken")).cloned();
    let cancel = Arc::new(AtomicBool::new(false));
    let key = id.to_string();
    shared.running.lock().unwrap_or_else(|e| e.into_inner()).insert(key.clone(), cancel.clone());
    let spawned = std::thread::Builder::new().name(format!("ir-mcp-{name}")).spawn({
        let shared = shared.clone();
        let key = key.clone();
        let id = id.clone();
        move || {
            let result = call_tool(&shared, tool, &args, token.as_ref(), &cancel);
            shared.running.lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
            // A cancelled request gets no answer (MCP spec); a call stopped by
            // EOF neither. One that finished on its own is still answered.
            if let Some(result) = result {
                if !cancel.load(Ordering::SeqCst) {
                    shared.send(&reply(&id, result));
                }
            }
        }
    });
    match spawned {
        Ok(h) => Some(h),
        Err(e) => {
            shared.running.lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
            let v = shared.version();
            shared.send(&reply(&id, tool_error(v, "client.io", &format!("could not start a thread: {e}"))));
            None
        }
    }
}

/// A tool error result (`isError`): the message names the sentinel.
pub fn tool_error(version: &str, code: &str, message: &str) -> Value {
    let text = RemoteError::new(code, message).to_string();
    let mut v = json!({"content": [{"type": "text", "text": text}], "isError": true});
    if has_structured(version) {
        v["structuredContent"] = json!({"error": {"code": code, "message": message}});
    }
    v
}

/// Pure: the MCP result for a control result.
pub fn tool_result(version: &str, tool: &Tool, result: Value) -> Value {
    match tool.shape {
        Shape::Frames => {
            let frames = result.as_array().cloned().unwrap_or_default();
            let summary: Vec<Value> =
                frames.iter().map(|f| json!({"atProgrammeMs": f.get("atProgrammeMs").cloned().unwrap_or(Value::Null)})).collect();
            let structured = json!({"frames": summary});
            let mut content = vec![json!({"type": "text", "text": pretty(&structured)})];
            for f in &frames {
                if let Some(data) = f.get("jpegBase64").and_then(Value::as_str) {
                    content.push(json!({"type": "image", "data": data, "mimeType": "image/jpeg"}));
                }
            }
            let mut v = json!({"content": content});
            if has_structured(version) {
                v["structuredContent"] = structured;
            }
            v
        }
        Shape::Json(key) => {
            let structured = if result.is_object() { result } else { json!({ key: result }) };
            let mut v = json!({"content": [{"type": "text", "text": pretty(&structured)}]});
            if has_structured(version) {
                v["structuredContent"] = structured;
            }
            v
        }
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

/// The tool result; `None` when the call was stopped (cancelled or EOF).
fn call_tool(
    shared: &Shared,
    tool: &Tool,
    args: &Map<String, Value>,
    token: Option<&Value>,
    cancel: &AtomicBool,
) -> Option<Value> {
    let version = shared.version();
    if tool.name == "record_delete" && args.get("confirm") != Some(&Value::Bool(true)) {
        return Some(tool_error(version, "record.control.confirm_required", "record_delete needs `confirm: true`"));
    }
    if let Err(m) = tools::validate(&Value::Object(args.clone()), &(tool.schema)(), "arguments") {
        return Some(tool_error(version, "record.control.bad_params", &m));
    }
    let stop = || cancel.load(Ordering::SeqCst) || shared.shutdown.load(Ordering::SeqCst);
    let mut conn = match (shared.connect)(tool.launch) {
        Ok(c) => c,
        Err(e) => return Some(tool_error(version, e.code(), &e.to_string())),
    };
    if stop() {
        return None;
    }
    let mut last = 0.0;
    let mut on_event = |ev: Value| {
        if let Some(tok) = token {
            if let Some((progress, total, message)) = progress_of(&ev, &mut last) {
                let mut p = json!({"progressToken": tok, "progress": progress, "message": message});
                if let Some(t) = total {
                    p["total"] = json!(t);
                }
                shared.send(&json!({"jsonrpc": "2.0", "method": "notifications/progress", "params": p}));
            }
        }
    };
    let deadline = tool.deadline.map(|d| Instant::now() + d);
    match conn.call_within(tool.method, tool.params(args), &mut on_event, &stop, deadline) {
        Ok(r) => Some(tool_result(version, tool, r)),
        Err(e) if e.code == client::ERR_INTERRUPTED => None,
        Err(e) => Some(tool_error(version, &e.code, &e.message)),
    }
}

/// Pure: a control event as MCP progress `(progress, total, message)`.
/// Progress must grow with every notification: an event whose `frame` does
/// not move past the last value is dropped; events without `frame` count up.
pub fn progress_of(ev: &Value, last: &mut f64) -> Option<(f64, Option<f64>, String)> {
    let frame = ev.get("frame").and_then(Value::as_f64);
    let total = ev.get("total").and_then(Value::as_f64);
    let phase = ev.get("phase").and_then(Value::as_str);
    let message = match (phase, frame, total, ev.get("seconds").and_then(Value::as_f64)) {
        (Some("countdown"), _, _, Some(s)) => format!("starting in {s} s"),
        (Some(p), Some(f), Some(t), _) => format!("{p} {f}/{t}"),
        (Some(p), _, _, _) => p.to_string(),
        _ => ev.to_string(),
    };
    let progress = match frame {
        Some(f) if f > *last => f,
        Some(_) => return None,
        None => *last + 1.0,
    };
    *last = progress;
    Some((progress, total, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::mpsc;

    #[test]
    fn negotiation_answers_a_known_version_else_the_latest() {
        assert_eq!(negotiate(Some("2024-11-05")), "2024-11-05");
        assert_eq!(negotiate(Some("2025-03-26")), "2025-03-26");
        assert_eq!(negotiate(Some("2025-06-18")), "2025-06-18");
        assert_eq!(negotiate(Some("2099-01-01")), LATEST);
        assert_eq!(negotiate(Some("")), LATEST);
        assert_eq!(negotiate(None), LATEST);
        assert_eq!(LATEST, "2025-06-18");
        assert!(!has_annotations("2024-11-05") && has_annotations("2025-03-26"));
        assert!(has_structured("2025-06-18") && !has_structured("2025-03-26"));
    }

    #[test]
    fn progress_grows_and_names_the_phase() {
        let mut last = 0.0;
        assert_eq!(
            progress_of(&json!({"phase": "countdown", "seconds": 3}), &mut last),
            Some((1.0, None, "starting in 3 s".into()))
        );
        assert_eq!(
            progress_of(&json!({"phase": "export", "frame": 50, "total": 200}), &mut last),
            Some((50.0, Some(200.0), "export 50/200".into()))
        );
        assert_eq!(progress_of(&json!({"phase": "export", "frame": 50, "total": 200}), &mut last), None);
        assert_eq!(progress_of(&json!({"phase": "transcribe"}), &mut last).unwrap().0, 51.0);
        assert_eq!(progress_of(&json!({"x": 1}), &mut last).unwrap().2, "{\"x\":1}");
    }

    #[test]
    fn results_wrap_arrays_and_frames_become_images() {
        let t = tools::find("record_projects").unwrap();
        let r = tool_result("2025-06-18", t, json!([{"folder": "a"}]));
        assert_eq!(r["structuredContent"], json!({"projects": [{"folder": "a"}]}));
        let text: Value = serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text, r["structuredContent"]);
        assert!(r.get("isError").is_none());
        // Older protocol: no structuredContent.
        assert!(tool_result("2025-03-26", t, json!([])).get("structuredContent").is_none());

        let f = tools::find("record_frames").unwrap();
        let r = tool_result("2025-06-18", f, json!([{"atProgrammeMs": 0, "jpegBase64": "AAA"}, {"atProgrammeMs": 40, "jpegBase64": "BBB"}]));
        let c = r["content"].as_array().unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(c[1], json!({"type": "image", "data": "AAA", "mimeType": "image/jpeg"}));
        assert_eq!(r["structuredContent"], json!({"frames": [{"atProgrammeMs": 0}, {"atProgrammeMs": 40}]}));
    }

    #[test]
    fn tool_errors_carry_the_sentinel() {
        let e = tool_error("2025-06-18", "record.busy", "busy");
        assert_eq!(e["isError"], true);
        assert_eq!(e["content"][0]["text"], "busy (record.busy)");
        assert_eq!(e["structuredContent"]["error"]["code"], "record.busy");
    }

    /// Run `serve` on in-memory input with a connector that never reaches an
    /// app; return the output lines.
    fn session(input: &str) -> Vec<Value> {
        struct Pipe(mpsc::Sender<Vec<u8>>);
        impl Write for Pipe {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                let _ = self.0.send(b.to_vec());
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let (tx, rx) = mpsc::channel();
        let code = serve(Cursor::new(input.to_string()), Box::new(Pipe(tx)), Arc::new(|_| Err(ClientError::NotRunning)));
        assert_eq!(code, 0);
        let bytes: Vec<u8> = rx.try_iter().flatten().collect();
        String::from_utf8(bytes).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    #[test]
    fn protocol_errors_and_notifications() {
        let out = session(concat!(
            "not json\n",
            "[]\n",
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"nope\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":\"s\",\"method\":\"ping\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"record_nope\"}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":5,\"method\":\"tools/call\",\"params\":{\"name\":\"record_doc\",\"arguments\":[1]}}\n",
            "{\"jsonrpc\":\"1.0\",\"id\":6,\"method\":\"ping\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{}}\n",
            "\n",
        ));
        let codes: Vec<(Value, Value)> = out.iter().map(|m| (m["id"].clone(), m["error"]["code"].clone())).collect();
        assert_eq!(
            codes,
            vec![
                (Value::Null, json!(PARSE_ERROR)),
                (Value::Null, json!(INVALID_REQUEST)),
                (json!(1), json!(METHOD_NOT_FOUND)),
                (json!("s"), Value::Null),
                (json!(3), json!(INVALID_PARAMS)),
                (json!(4), json!(INVALID_PARAMS)),
                (json!(5), json!(INVALID_PARAMS)),
                (json!(6), json!(INVALID_REQUEST)),
            ]
        );
        assert_eq!(out[3]["result"], json!({}));
        assert_eq!(out[4]["error"]["message"], "Unknown tool: record_nope");
    }

    #[test]
    fn a_known_tool_is_a_call_not_an_unknown_tool() {
        // Guards the unknown-tool check against matching too much.
        let out = session("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"record_status\"}}\n");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["result"]["isError"], true);
        assert_eq!(out[0]["result"]["structuredContent"]["error"]["code"], "client.not_running");
    }

    #[test]
    fn wrong_argument_types_are_a_tool_error_not_a_protocol_error() {
        let out = session(concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"record_export\",\"arguments\":{\"project\":\"a\",\"fps\":\"x\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"record_delete\",\"arguments\":{\"project\":\"a\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"record_frames\",\"arguments\":{\"project\":\"a\",\"atProgrammeMs\":-1e308}}}\n",
        ));
        let mut out = out;
        out.sort_by_key(|m| m["id"].as_i64());
        assert_eq!(out[0]["result"]["structuredContent"]["error"]["code"], "record.control.bad_params");
        assert_eq!(out[0]["result"]["content"][0]["text"], "arguments.fps: expected integer, got string (record.control.bad_params)");
        assert_eq!(out[1]["result"]["structuredContent"]["error"]["code"], "record.control.confirm_required");
        assert_eq!(out[2]["result"]["isError"], true);
    }

    #[test]
    fn initialize_negotiates_and_older_clients_get_no_annotations() {
        let out = session(concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{},\"clientInfo\":{\"name\":\"t\",\"version\":\"1\"}}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
        ));
        assert_eq!(out[0]["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(out[0]["result"]["serverInfo"]["name"], SERVER_NAME);
        assert!(out[1]["result"]["tools"][0].get("annotations").is_none());
        let out = session("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"1999-01-01\"}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n");
        assert_eq!(out[0]["result"]["protocolVersion"], LATEST);
        assert_eq!(out[1]["result"]["tools"][0]["annotations"]["readOnlyHint"], true);
    }
}
