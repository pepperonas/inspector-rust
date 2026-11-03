//! The session folder format and crash recovery. The app reads this without
//! the helper (session list, "N sessions were aborted" at start).
//!
//! A folder `YYYY-MM-DD_HHMMSS/` holds `transkript.txt` (one readable line per
//! final segment), `transkript.jsonl` (the same as JSON) and `session.json`
//! (metadata, status `running|completed|aborted`). The two transcript files
//! are only ever appended; `session.json` is replaced atomically.

use crate::{Lang, Mode, Source};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const TXT: &str = "transkript.txt";
pub const JSONL: &str = "transkript.jsonl";
pub const META: &str = "session.json";
pub const AUDIO_DIR: &str = "audio";
pub const META_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Running,
    Completed,
    Aborted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMeta {
    pub version: u32,
    pub started_ms: u64,
    pub ended_ms: Option<u64>,
    pub status: SessionStatus,
    pub model: String,
    pub lang: Lang,
    pub sources: Vec<Source>,
    pub mode: Mode,
    /// Set when the session moved here because the chosen folder failed.
    #[serde(default)]
    pub fallback_from: Option<PathBuf>,
    #[serde(default)]
    pub note: Option<String>,
}

/// One final segment, as a `transkript.jsonl` line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub start_ms: u64,
    pub end_ms: u64,
    pub source: Source,
    pub lang: String,
    pub confidence: f32,
    pub text: String,
}

/// `YYYY-MM-DD_HHMMSS` for the local start time.
pub fn session_dir_name(started_local: chrono::NaiveDateTime) -> String {
    started_local.format("%Y-%m-%d_%H%M%S").to_string()
}

/// `root/name`, or `name-2`, `name-3` … when taken (two sessions in one second).
pub fn free_session_dir(root: &Path, name: &str) -> PathBuf {
    let first = root.join(name);
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| root.join(format!("{name}-{n}")))
        .find(|p| !p.exists())
        .expect("an unbounded range finds a free name")
}

/// `[19:45:12] MIC: text\n` — local time of the segment start; line breaks in
/// the text become spaces so one segment stays one line.
pub fn txt_line(l: &Line, local_offset_s: i32) -> String {
    let secs = (l.start_ms / 1000) as i64 + local_offset_s as i64;
    let day = secs.rem_euclid(86_400);
    let text: String = l.text.split_whitespace().collect::<Vec<_>>().join(" ");
    format!(
        "[{:02}:{:02}:{:02}] {}: {}\n",
        day / 3600,
        day / 60 % 60,
        day % 60,
        l.source.label(),
        text
    )
}

/// Replace `session.json` atomically (tmp + fsync + rename).
pub fn write_meta_atomic(dir: &Path, meta: &SessionMeta) -> io::Result<()> {
    let tmp = dir.join(format!("{META}.tmp"));
    let body = serde_json::to_vec_pretty(meta).map_err(io::Error::other)?;
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&body)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, dir.join(META))
}

pub fn read_meta(dir: &Path) -> Option<SessionMeta> {
    serde_json::from_slice(&fs::read(dir.join(META)).ok()?).ok()
}

/// Cut a file back to its last line break: a crash in the middle of an append
/// leaves half a line, which is removed rather than parsed.
fn trim_partial_line(path: &Path) -> io::Result<()> {
    let Ok(data) = fs::read(path) else { return Ok(()) };
    let keep = data.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    if keep < data.len() {
        let f = fs::OpenOptions::new().write(true).open(path)?;
        f.set_len(keep as u64)?;
        f.sync_all()?;
    }
    Ok(())
}

/// Every complete line of a session's `transkript.jsonl`.
pub fn read_lines(dir: &Path) -> Vec<Line> {
    let Ok(text) = fs::read_to_string(dir.join(JSONL)) else { return Vec::new() };
    let complete = match text.rfind('\n') {
        Some(i) => &text[..=i],
        None => "",
    };
    complete.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Recovered {
    pub dir: PathBuf,
    pub lines: usize,
    pub ended_ms: Option<u64>,
}

/// Mark every `running` session under `root` as `aborted` (the process that
/// wrote it is gone), trim a half-written last line and record where it ended.
/// Call once at start, before a new session begins.
pub fn recover(root: &Path) -> Vec<Recovered> {
    let Ok(entries) = fs::read_dir(root) else { return Vec::new() };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let dir = e.path();
        let Some(mut meta) = read_meta(&dir) else { continue };
        if meta.status != SessionStatus::Running {
            continue;
        }
        let _ = trim_partial_line(&dir.join(JSONL));
        let _ = trim_partial_line(&dir.join(TXT));
        let lines = read_lines(&dir);
        let ended_ms = lines.last().map(|l| l.end_ms).or(meta.ended_ms);
        meta.status = SessionStatus::Aborted;
        meta.ended_ms = ended_ms;
        if write_meta_atomic(&dir, &meta).is_ok() {
            out.push(Recovered { dir, lines: lines.len(), ended_ms });
        }
    }
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    out
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionSummary {
    pub dir: PathBuf,
    pub meta: SessionMeta,
    pub lines: usize,
}

/// All sessions under the given roots, newest first.
pub fn list_sessions(roots: &[PathBuf]) -> Vec<SessionSummary> {
    let mut out = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else { continue };
        for e in entries.flatten() {
            let dir = e.path();
            if let Some(meta) = read_meta(&dir) {
                let lines = read_lines(&dir).len();
                out.push(SessionSummary { dir, meta, lines });
            }
        }
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.meta.started_ms));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn meta(status: SessionStatus) -> SessionMeta {
        SessionMeta {
            version: META_VERSION,
            started_ms: 1_000,
            ended_ms: None,
            status,
            model: "large-v3-turbo-q5".into(),
            lang: Lang::Auto,
            sources: vec![Source::Mic, Source::Sys],
            mode: Mode::Meeting,
            fallback_from: None,
            note: None,
        }
    }

    fn line(end: u64) -> Line {
        Line { start_ms: end - 500, end_ms: end, source: Source::Mic, lang: "de".into(), confidence: 0.9, text: "x".into() }
    }

    #[test]
    fn txt_line_format() {
        let l = Line {
            start_ms: (19 * 3600 + 45 * 60 + 12) * 1000,
            end_ms: 0,
            source: Source::Mic,
            lang: "de".into(),
            confidence: 1.0,
            text: "Hallo\nWelt".into(),
        };
        assert_eq!(txt_line(&l, 0), "[19:45:12] MIC: Hallo Welt\n");
        assert_eq!(txt_line(&l, 3600), "[20:45:12] MIC: Hallo Welt\n");
    }

    #[test]
    fn session_dir_names_never_collide() {
        let root = tempfile::tempdir().unwrap();
        let name = session_dir_name(
            chrono::NaiveDate::from_ymd_opt(2026, 11, 3).unwrap().and_hms_opt(19, 4, 5).unwrap(),
        );
        assert_eq!(name, "2026-11-03_190405");
        let a = free_session_dir(root.path(), &name);
        fs::create_dir(&a).unwrap();
        let b = free_session_dir(root.path(), &name);
        assert_ne!(a, b);
        assert!(b.ends_with("2026-11-03_190405-2"));
    }

    #[test]
    fn recovery_trims_half_a_line_and_marks_aborted() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("s");
        fs::create_dir(&dir).unwrap();
        write_meta_atomic(&dir, &meta(SessionStatus::Running)).unwrap();
        let mut j = String::new();
        for e in [2_000, 3_000, 4_000] {
            j.push_str(&serde_json::to_string(&line(e)).unwrap());
            j.push('\n');
        }
        j.push_str("{\"start_ms\":45"); // the crash
        fs::write(dir.join(JSONL), &j).unwrap();
        fs::write(dir.join(TXT), "[a]\n[b]\n[c]\n[d").unwrap();

        let r = recover(root.path());
        assert_eq!(r, vec![Recovered { dir: dir.clone(), lines: 3, ended_ms: Some(4_000) }]);
        let m = read_meta(&dir).unwrap();
        assert_eq!(m.status, SessionStatus::Aborted);
        assert_eq!(m.ended_ms, Some(4_000));
        assert!(fs::read_to_string(dir.join(JSONL)).unwrap().ends_with("}\n"));
        assert_eq!(fs::read_to_string(dir.join(TXT)).unwrap(), "[a]\n[b]\n[c]\n");
        // A second start finds nothing more to recover.
        assert!(recover(root.path()).is_empty());
    }

    #[test]
    fn completed_sessions_are_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("s");
        fs::create_dir(&dir).unwrap();
        write_meta_atomic(&dir, &meta(SessionStatus::Completed)).unwrap();
        assert!(recover(root.path()).is_empty());
        assert_eq!(read_meta(&dir).unwrap().status, SessionStatus::Completed);
    }

    #[test]
    fn sessions_are_listed_newest_first() {
        let root = tempfile::tempdir().unwrap();
        for (name, t) in [("a", 1), ("b", 3), ("c", 2)] {
            let dir = root.path().join(name);
            fs::create_dir(&dir).unwrap();
            let mut m = meta(SessionStatus::Completed);
            m.started_ms = t;
            write_meta_atomic(&dir, &m).unwrap();
        }
        let l = list_sessions(&[root.path().to_path_buf(), root.path().join("missing")]);
        let order: Vec<u64> = l.iter().map(|s| s.meta.started_ms).collect();
        assert_eq!(order, vec![3, 2, 1]);
    }
}
