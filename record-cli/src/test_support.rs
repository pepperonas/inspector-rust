//! A small fake of the app's control server, for tests.
#![cfg(all(test, unix))]

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct Td(pub PathBuf);
impl Drop for Td {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub fn td() -> Td {
    static N: AtomicUsize = AtomicUsize::new(0);
    // Short: unix socket paths are limited to ~104 bytes on macOS.
    let p = PathBuf::from(format!("/tmp/irc-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    Td(p)
}

pub const TOKEN: &str = "0123456789abcdef0123456789abcdef";

pub fn write_disc(dir: &Path, sock: &Path, token: &str) {
    let d = json!({"pid": 999_999, "socket": sock.to_string_lossy(), "token": token});
    std::fs::write(dir.join("control.json"), d.to_string()).unwrap();
}

/// A small fake of the app's control server. `closed` counts connections
/// whose peer closed while a `t.slow` call ran.
pub fn fake_server(dir: &Path, closed: Arc<AtomicUsize>) -> PathBuf {
    let sock = dir.join("control.sock");
    let l = UnixListener::bind(&sock).unwrap();
    write_disc(dir, &sock, TOKEN);
    std::thread::spawn(move || {
        for c in l.incoming() {
            let Ok(c) = c else { return };
            let closed = closed.clone();
            std::thread::spawn(move || serve(c, closed));
        }
    });
    sock
}

pub fn serve(c: UnixStream, closed: Arc<AtomicUsize>) {
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
        if v["token"] != TOKEN {
            send(&mut out, json!({"id": id, "error": {"code": "control.bad_token", "message": "wrong"}}));
            return;
        }
        match v["method"].as_str().unwrap() {
            "control.ping" => {
                send(&mut out, json!({"id": id, "result": {"version": "9.9.9", "platform": "macos"}}));
            }
            "t.events" => {
                for i in 0..3 {
                    send(&mut out, json!({"id": id, "event": {"frame": i}}));
                }
                send(&mut out, json!({"id": id, "result": {"path": "/x.mp4"}}));
            }
            "t.slow" => {
                send(&mut out, json!({"id": id, "event": {"phase": "export"}}));
                let _ = c.set_read_timeout(Some(Duration::from_secs(5)));
                let mut b = [0u8; 1];
                if matches!((&c).read(&mut b), Ok(0)) {
                    closed.fetch_add(1, Ordering::SeqCst);
                }
                return;
            }
            "record.start" if v["params"]["title"] == "slow-start" => {
                // A countdown that only ends when the client goes away.
                send(&mut out, json!({"id": id, "event": {"phase": "countdown", "seconds": 3}}));
                let _ = c.set_read_timeout(Some(Duration::from_secs(5)));
                let mut b = [0u8; 1];
                if matches!((&c).read(&mut b), Ok(0)) {
                    closed.fetch_add(1, Ordering::SeqCst);
                }
                return;
            }
            "record.start" => {
                send(&mut out, json!({"id": id, "event": {"phase": "countdown", "seconds": 3}}));
                send(&mut out, json!({"id": id, "result": {"project": "/r/take", "folder": "take", "status": {"phase": "recording"}}}));
            }
            "record.status" => {
                send(&mut out, json!({"id": id, "result": {"phase": "recording", "elapsedMs": 1000}}));
            }
            "record.stop" => {
                closed.fetch_add(1000, Ordering::SeqCst); // counts stops
                send(&mut out, json!({"id": id, "result": {"project": "/r/take", "folder": "take", "ready": true, "status": "ready"}}));
            }
            "t.fail" => {
                send(&mut out, json!({"id": id, "error": {"code": "record.busy", "message": "busy"}}));
            }
            m => {
                send(&mut out, json!({"id": id, "error": {"code": "control.unknown_method", "message": m}}));
            }
        }
    }
}

