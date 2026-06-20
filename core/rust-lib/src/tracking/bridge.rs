//! Browser bridge for the timesheet — a **loopback-only** (`127.0.0.1`),
//! token-authenticated WebSocket server the browser extension reports the active
//! tab to. The desktop tracker stays the single clock; the bridge only updates
//! `Runtime.last_tab`, which the focus loop reads to enrich the open interval
//! while a browser is frontmost (and to split it on tab change).
//!
//! **Security:** binds `127.0.0.1` only (never `0.0.0.0`); every connection must
//! send `{"type":"hello","token":"…"}` matching the app's bridge token before
//! any `tab`/`blur` frame is accepted; non-loopback peers are rejected. No
//! outbound traffic. Runs only while a tracking session is active.

use crate::db::DbHandle;
use crate::tracking::{Runtime, TabInfo};
use parking_lot::Mutex;
use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tungstenite::Message;

pub const DEFAULT_PORT: u16 = 47823;

pub fn bridge_port(db: &DbHandle) -> u16 {
    crate::settings::get_or(db, "track.bridge_port", &DEFAULT_PORT.to_string())
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|p| *p > 0)
        .unwrap_or(DEFAULT_PORT)
}

/// The bridge token, generated + persisted on first use.
pub fn bridge_token(db: &DbHandle) -> String {
    if let Ok(Some(t)) = crate::settings::get(db, "track.bridge_token") {
        if !t.is_empty() {
            return t;
        }
    }
    let tok = gen_token();
    let _ = crate::settings::set(db, "track.bridge_token", &tok);
    tok
}

pub fn regenerate_token(db: &DbHandle) -> String {
    let tok = gen_token();
    let _ = crate::settings::set(db, "track.bridge_token", &tok);
    tok
}

fn gen_token() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Start the bridge on a worker thread; returns its stop flag.
pub fn start(db: DbHandle, rt: Arc<Mutex<Runtime>>) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let _ = bridge_token(&db); // ensure a token exists (generated on first use)
    let port = bridge_port(&db);
    let stop_ret = stop.clone();
    std::thread::spawn(move || {
        let listener = match TcpListener::bind(("127.0.0.1", port)) {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!("timesheet bridge: bind 127.0.0.1:{port} failed: {e}");
                return;
            }
        };
        let _ = listener.set_nonblocking(true);
        while !stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, addr)) => {
                    if !addr.ip().is_loopback() {
                        continue; // defensive — bind is already loopback-only
                    }
                    let (rt2, db2, stop2) = (rt.clone(), db.clone(), stop.clone());
                    std::thread::spawn(move || handle_conn(stream, rt2, db2, stop2));
                }
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        }
    });
    stop_ret
}

fn handle_conn(stream: TcpStream, rt: Arc<Mutex<Runtime>>, db: DbHandle, stop: Arc<AtomicBool>) {
    // Read timeout so a blocked read wakes periodically to honour `stop`.
    let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
    let mut ws = match tungstenite::accept(stream) {
        Ok(w) => w,
        Err(_) => return,
    };
    // The token this socket authenticated with — so a `regenerate_token`
    // (which writes the DB) immediately invalidates the connection below.
    let mut authed_with: Option<String> = None;
    while !stop.load(Ordering::SeqCst) {
        match ws.read() {
            Ok(Message::Text(txt)) => {
                let v: serde_json::Value = match serde_json::from_str(&txt) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
                // Read the *live* token every frame so rotation takes effect
                // immediately — revocation doesn't wait for a session restart.
                let current = bridge_token(&db);
                if authed_with.is_none() {
                    if typ == "hello"
                        && v.get("token").and_then(|t| t.as_str()) == Some(current.as_str())
                    {
                        authed_with = Some(current);
                        let _ = ws.send(Message::Text("{\"type\":\"ok\"}".to_string()));
                    } else {
                        let _ = ws.close(None);
                        return;
                    }
                    continue;
                }
                // Token rotated since this socket authed → drop it (fail closed).
                if authed_with.as_deref() != Some(current.as_str()) {
                    let _ = ws.close(None);
                    return;
                }
                match typ {
                    "tab" => {
                        let url = v
                            .get("url")
                            .and_then(|u| u.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string());
                        let title = v
                            .get("title")
                            .and_then(|t| t.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string());
                        let host = url.as_deref().and_then(host_from_url);
                        rt.lock().last_tab = Some(TabInfo { host, title, url });
                    }
                    "blur" => {
                        rt.lock().last_tab = None;
                    }
                    _ => {}
                }
            }
            Ok(Message::Close(_)) => return,
            Ok(_) => {} // ping/pong/binary — tungstenite auto-handles ping
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {}
            Err(_) => return,
        }
    }
    let _ = ws.close(None);
}

/// Extract the lowercased host from a URL (no scheme/port/path/userinfo). Pure.
pub fn host_from_url(url: &str) -> Option<String> {
    let after = url.split("://").nth(1).unwrap_or(url);
    let host = after.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or(host); // strip userinfo
    let host = host.split(':').next().unwrap_or(host); // strip port
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_from_url_extracts_host() {
        assert_eq!(host_from_url("https://github.com/foo/bar?x=1"), Some("github.com".into()));
        assert_eq!(host_from_url("http://Example.COM:8080/page"), Some("example.com".into()));
        assert_eq!(host_from_url("https://user:pw@host.io/x"), Some("host.io".into()));
        assert_eq!(host_from_url("github.com/x"), Some("github.com".into()));
        assert_eq!(host_from_url("about:blank"), Some("about".into())); // best-effort
        assert_eq!(host_from_url(""), None);
    }
}
