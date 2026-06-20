//! Claude-Code usage detector for the timesheet. Watches
//! `~/.claude/projects/**/*.jsonl` (the Claude Code session logs) for appends;
//! each appended **assistant turn** extends a per-project (`cwd`) `claude`
//! interval, and after a gap (no append for [`GAP_MS`]) the next append starts a
//! fresh interval. Token usage per turn is recorded for the Claude charts.
//!
//! Runs only while a tracking session is active. JSONL parsing is **defensive**
//! (the format can change): unknown fields are ignored, malformed lines skipped.
//! Only `type` / `timestamp` / `cwd` / `message.model` / `message.usage.*` are read.

use crate::db::DbHandle;
use crate::tracking::db as tdb;
use notify::{RecursiveMode, Watcher};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Inactivity gap (ms) after which the next Claude turn for a project starts a
/// new interval instead of extending the previous one.
const GAP_MS: i64 = 180_000;
const APP_NAME: &str = "Claude Code";

/// Start the watcher on a worker thread; returns its stop flag. Caller keeps it
/// in the tracker runtime and sets it on session end.
pub fn start(db: DbHandle, session_id: i64) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_ret = stop.clone();
    std::thread::spawn(move || run(db, session_id, stop));
    stop_ret
}

fn projects_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("projects"))
}

struct OpenInterval {
    event_id: i64,
    last_turn: i64,
}

fn run(db: DbHandle, session_id: i64, stop: Arc<AtomicBool>) {
    let Some(root) = projects_dir() else { return };
    if !root.exists() {
        return;
    }

    // Skip pre-existing content: remember current sizes so we only react to
    // appends made *after* tracking started.
    let mut offsets: HashMap<PathBuf, u64> = HashMap::new();
    collect_jsonl(&root, &mut |p| {
        if let Ok(meta) = std::fs::metadata(&p) {
            offsets.insert(p, meta.len());
        }
    });

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    }) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!("claude watcher: create failed: {e}");
            return;
        }
    };
    if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
        tracing::warn!("claude watcher: watch failed: {e}");
        return;
    }

    // Per-project (cwd) currently-open interval.
    let mut open: HashMap<String, OpenInterval> = HashMap::new();

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(1000)) {
            Ok(Ok(event)) => {
                for path in event.paths {
                    if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                        let lines = read_appended(&path, &mut offsets);
                        for line in lines {
                            if let Some(turn) = parse_turn(&line) {
                                handle_turn(&db, session_id, &mut open, turn);
                            }
                        }
                    }
                }
            }
            Ok(Err(_)) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(watcher);
}

/// Read newly-appended complete lines from `path` (since the stored offset),
/// advancing the offset to the last newline. Handles truncation/rotation.
fn read_appended(path: &Path, offsets: &mut HashMap<PathBuf, u64>) -> Vec<String> {
    let Ok(mut f) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = offsets.get(path).copied().unwrap_or(0);
    let from = if start > len { 0 } else { start }; // truncated/rotated → re-read
    if len <= from {
        offsets.insert(path.to_path_buf(), len);
        return Vec::new();
    }
    if f.seek(SeekFrom::Start(from)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    // Only consume up to the last newline; keep a partial trailing line for next time.
    let last_nl = buf.iter().rposition(|&b| b == b'\n');
    let consumed = match last_nl {
        Some(i) => i + 1,
        None => {
            // No complete line yet — don't advance.
            return Vec::new();
        }
    };
    offsets.insert(path.to_path_buf(), from + consumed as u64);
    String::from_utf8_lossy(&buf[..consumed])
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.to_string())
        .collect()
}

struct Turn {
    ts: i64,
    cwd: String,
    model: Option<String>,
    tokens_in: Option<i64>,
    tokens_out: Option<i64>,
}

/// Parse a JSONL line into a Claude assistant turn (defensive). Returns `None`
/// for non-assistant lines, malformed JSON, or a missing timestamp/cwd.
fn parse_turn(line: &str) -> Option<Turn> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
        return None;
    }
    let ts_str = v.get("timestamp").and_then(|t| t.as_str())?;
    let ts = chrono::DateTime::parse_from_rfc3339(ts_str)
        .ok()?
        .timestamp_millis();
    let cwd = v.get("cwd").and_then(|c| c.as_str())?.to_string();
    if cwd.is_empty() {
        return None;
    }
    let model = v
        .pointer("/message/model")
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());
    let tokens_in = v.pointer("/message/usage/input_tokens").and_then(|n| n.as_i64());
    let tokens_out = v.pointer("/message/usage/output_tokens").and_then(|n| n.as_i64());
    Some(Turn {
        ts,
        cwd,
        model,
        tokens_in,
        tokens_out,
    })
}

fn project_name(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .and_then(|n| n.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| cwd.to_string())
}

fn handle_turn(
    db: &DbHandle,
    session_id: i64,
    open: &mut HashMap<String, OpenInterval>,
    turn: Turn,
) {
    let extend = matches!(open.get(&turn.cwd), Some(o) if turn.ts - o.last_turn <= GAP_MS);
    let event_id = if extend {
        let o = open.get_mut(&turn.cwd).unwrap();
        // Extend the interval's end to this turn.
        let _ = tdb::update_event(
            db,
            o.event_id,
            &tdb::EventPatch {
                ended_at: Some(turn.ts),
                ..Default::default()
            },
        );
        o.last_turn = turn.ts;
        o.event_id
    } else {
        // Start a fresh interval (started == ended == ts initially).
        let ne = tdb::NewEvent {
            session_id,
            app_name: APP_NAME.to_string(),
            app_id: None,
            window_title: None,
            url: None,
            host: None,
            category: None,
            project: Some(project_name(&turn.cwd)),
            source: "claude".to_string(),
            is_idle: false,
            started_at: turn.ts,
        };
        match tdb::open_event(db, &ne) {
            Ok(id) => {
                let _ = tdb::update_event(
                    db,
                    id,
                    &tdb::EventPatch {
                        ended_at: Some(turn.ts),
                        ..Default::default()
                    },
                );
                open.insert(turn.cwd.clone(), OpenInterval { event_id: id, last_turn: turn.ts });
                id
            }
            Err(_) => return,
        }
    };
    let _ = tdb::insert_claude_turn(db, event_id, turn.ts, turn.model.as_deref(), turn.tokens_in, turn.tokens_out);
}

/// Recursively visit `*.jsonl` files under `root`, calling `f` for each.
fn collect_jsonl(root: &Path, f: &mut impl FnMut(PathBuf)) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_jsonl(&p, f);
        } else if p.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            f(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_turn_reads_assistant_fields_defensively() {
        let line = r#"{"type":"assistant","timestamp":"2026-06-20T10:00:00.000Z","cwd":"/Users/m/proj","message":{"model":"claude-opus-4-8","usage":{"input_tokens":120,"output_tokens":340}}}"#;
        let t = parse_turn(line).unwrap();
        assert_eq!(t.cwd, "/Users/m/proj");
        assert_eq!(t.model.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(t.tokens_in, Some(120));
        assert_eq!(t.tokens_out, Some(340));
        assert!(t.ts > 0);
    }

    #[test]
    fn parse_turn_skips_non_assistant_and_malformed() {
        assert!(parse_turn(r#"{"type":"user","timestamp":"2026-06-20T10:00:00Z","cwd":"/x"}"#).is_none());
        assert!(parse_turn("not json").is_none());
        assert!(parse_turn(r#"{"type":"assistant"}"#).is_none()); // no ts/cwd
        // Missing usage is tolerated (tokens None).
        let t = parse_turn(r#"{"type":"assistant","timestamp":"2026-06-20T10:00:00Z","cwd":"/x"}"#).unwrap();
        assert_eq!(t.tokens_in, None);
    }

    #[test]
    fn project_name_is_the_cwd_basename() {
        assert_eq!(project_name("/Users/m/code/inspector-rust"), "inspector-rust");
        assert_eq!(project_name("plain"), "plain");
    }

    #[test]
    fn read_appended_returns_only_new_complete_lines() {
        let dir = std::env::temp_dir().join(format!("ir-claude-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("s.jsonl");
        std::fs::write(&f, "line1\nline2\n").unwrap();
        let mut offsets = std::collections::HashMap::new();
        offsets.insert(f.clone(), 0u64);
        let lines = read_appended(&f, &mut offsets);
        assert_eq!(lines, vec!["line1".to_string(), "line2".to_string()]);
        // No new content → empty.
        assert!(read_appended(&f, &mut offsets).is_empty());
        // Append a partial line (no newline) → not yet returned.
        std::fs::write(&f, "line1\nline2\npartial").unwrap();
        assert!(read_appended(&f, &mut offsets).is_empty());
        // Complete it.
        std::fs::write(&f, "line1\nline2\npartial done\n").unwrap();
        assert_eq!(read_appended(&f, &mut offsets), vec!["partial done".to_string()]);
        std::fs::remove_dir_all(&dir).ok();
    }
}
