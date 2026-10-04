//! Usage limits of the other coding agents, for the `limits` panel — read from
//! their LOCAL files only. No network, no tokens, nothing written.
//!
//! * **Codex** writes its rate limits into every session file: each
//!   `token_count` event in `~/.codex/sessions/**/*.jsonl` carries
//!   `rate_limits {limit_id:"codex", primary, secondary}` with `used_percent`,
//!   `window_minutes` and `resets_at` (epoch s). We take the newest such event.
//!   It is only as fresh as the last Codex turn — the panel shows its time.
//!   Events with `limit_id` other than `codex` (e.g. `premium`, with null
//!   windows) are skipped.
//! * **Antigravity** exposes no percentages locally. All it leaves behind is a
//!   log line when a request hits the wall:
//!   `E1003 06:09:48.211684 … Individual quota reached. … Resets in 88h4m0s.`
//!   in `~/.gemini/antigravity-cli/log/cli-YYYYMMDD_HHMMSS.log`. From the
//!   newest one we derive "blocked until …" — or that no block is known.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::claude_limits::Limit;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CodexLimits {
    pub plan: Option<String>,
    /// When the event was written (RFC 3339) — the data's age.
    pub as_of: Option<String>,
    pub limits: Vec<Limit>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AntigravityQuota {
    /// A quota block is still running.
    pub blocked: bool,
    /// When the last recorded block ends (RFC 3339), if any was recorded.
    pub resets_at: Option<String>,
    /// When it was hit (RFC 3339).
    pub hit_at: Option<String>,
}

// ── Codex (pure) ────────────────────────────────────────────────────────────

/// "5-Stunden-Fenster" / "Woche" / "Tag" / "N-Min.-Fenster".
pub fn window_name(minutes: i64) -> String {
    match minutes {
        10080 => "Woche".into(),
        1440 => "Tag".into(),
        m if m > 0 && m % 60 == 0 => format!("{}-Stunden-Fenster", m / 60),
        m => format!("{m}-Min.-Fenster"),
    }
}

fn epoch_to_rfc3339(secs: i64) -> Option<String> {
    Utc.timestamp_opt(secs, 0).single().map(|d| d.to_rfc3339())
}

fn codex_window(key: &str, w: &Value, now_s: i64) -> Option<Limit> {
    let o = w.as_object()?;
    let mut percent = o.get("used_percent")?.as_f64()?;
    let minutes = o.get("window_minutes").and_then(Value::as_i64).unwrap_or(0);
    let resets = o.get("resets_at").and_then(Value::as_i64);
    // The window has rolled over since Codex last wrote it — the usage that
    // was recorded no longer counts.
    if resets.is_some_and(|r| r <= now_s) {
        percent = 0.0;
    }
    Some(Limit {
        id: format!("codex-{key}"),
        name: window_name(minutes),
        kind: key.into(),
        group: None,
        percent,
        resets_at: resets.and_then(|r| if r > now_s { epoch_to_rfc3339(r) } else { None }),
        severity: None,
        active: false,
        known: true,
        money: None,
    })
}

/// Pure: one JSONL line → limits, if it is a Codex `token_count` event with
/// at least one populated window.
pub fn parse_codex_line(line: &str, now_s: i64) -> Option<CodexLimits> {
    // Cheap pre-filter — the files are large and almost every line is other.
    if !line.contains("\"limit_id\":\"codex\"") {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    let rl = v.pointer("/payload/rate_limits")?;
    if rl.get("limit_id")?.as_str()? != "codex" {
        return None;
    }
    let limits: Vec<Limit> = ["primary", "secondary"]
        .iter()
        .filter_map(|k| rl.get(*k).and_then(|w| codex_window(k, w, now_s)))
        .collect();
    if limits.is_empty() {
        return None;
    }
    Some(CodexLimits {
        plan: rl.get("plan_type").and_then(Value::as_str).map(str::to_owned),
        as_of: v.get("timestamp").and_then(Value::as_str).map(str::to_owned),
        limits,
    })
}

/// Pure: the newest Codex limits in a file's content (last matching line).
pub fn codex_from_content(content: &str, now_s: i64) -> Option<CodexLimits> {
    content.lines().rev().find_map(|l| parse_codex_line(l, now_s))
}

// ── Antigravity (pure) ──────────────────────────────────────────────────────

/// Pure: Go duration like `88h4m0s`, `14m18s`, `1h0m0.5s` → seconds.
pub fn parse_go_duration(s: &str) -> Option<i64> {
    let mut total = 0f64;
    let mut num = String::new();
    let mut any = false;
    for c in s.chars() {
        match c {
            '0'..='9' | '.' => num.push(c),
            'h' | 'm' | 's' => {
                let n: f64 = num.parse().ok()?;
                num.clear();
                total += n * match c {
                    'h' => 3600.0,
                    'm' => 60.0,
                    _ => 1.0,
                };
                any = true;
            }
            _ => return None,
        }
    }
    (any && num.is_empty()).then_some(total.round() as i64)
}

/// Pure: the year from `cli-20261003_060915.log`.
pub fn log_year(file_name: &str) -> Option<i32> {
    file_name.strip_prefix("cli-")?.get(..4)?.parse().ok()
}

/// Pure: a quota line → (hit time, reset time) as naive local datetimes.
/// glog prefix: `E1003 06:09:48.211684 …` (MMDD, local time).
pub fn parse_quota_line(line: &str, year: i32) -> Option<(NaiveDateTime, NaiveDateTime)> {
    if !line.contains("quota reached") {
        return None;
    }
    let mut parts = line.split_whitespace();
    let head = parts.next()?;
    let time = parts.next()?;
    let mmdd = head.get(1..5)?;
    let date = NaiveDate::from_ymd_opt(year, mmdd.get(..2)?.parse().ok()?, mmdd.get(2..)?.parse().ok()?)?;
    let t = NaiveTime::parse_from_str(time.split('.').next()?, "%H:%M:%S").ok()?;
    let hit = NaiveDateTime::new(date, t);
    let dur = line.split("Resets in ").nth(1)?.trim().trim_end_matches('.');
    let secs = parse_go_duration(dur)?;
    Some((hit, hit + chrono::Duration::seconds(secs)))
}

// ── Impure readers with a small cache ───────────────────────────────────────

fn newest_files(dir: &Path, prefix: Option<&str>, ext: &str, max: usize) -> Vec<(PathBuf, SystemTime)> {
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, SystemTime)>, prefix: Option<&str>, ext: &str, depth: u8) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(md) = e.metadata() else { continue };
            if md.is_dir() {
                if depth < 4 {
                    walk(&p, out, prefix, ext, depth + 1);
                }
            } else if p.extension().and_then(|x| x.to_str()) == Some(ext)
                && prefix.is_none_or(|pre| {
                    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(pre))
                })
            {
                if let Ok(m) = md.modified() {
                    out.push((p, m));
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, &mut out, prefix, ext, 0);
    out.sort_by_key(|a| std::cmp::Reverse(a.1));
    out.truncate(max);
    out
}

/// Cache of the last (file, mtime) we parsed, so a 30-s panel poll doesn't
/// re-read multi-megabyte session files that haven't changed.
static CODEX_CACHE: Mutex<Option<(PathBuf, SystemTime, Option<CodexLimits>)>> = Mutex::new(None);

pub fn codex() -> Option<CodexLimits> {
    let dir = dirs::home_dir()?.join(".codex/sessions");
    let now_s = Utc::now().timestamp();
    for (path, mtime) in newest_files(&dir, None, "jsonl", 8) {
        {
            let c = CODEX_CACHE.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((p, m, r)) = c.as_ref() {
                if *p == path && *m == mtime {
                    if let Some(r) = r {
                        // Re-evaluate expiry against now.
                        return Some(refresh_expiry(r.clone(), now_s));
                    }
                    continue;
                }
            }
        }
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        let found = codex_from_content(&content, now_s);
        *CODEX_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((path, mtime, found.clone()));
        if found.is_some() {
            return found;
        }
    }
    None
}

fn refresh_expiry(mut r: CodexLimits, now_s: i64) -> CodexLimits {
    for l in &mut r.limits {
        let expired = l
            .resets_at
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .is_some_and(|d| d.timestamp() <= now_s);
        if expired {
            l.percent = 0.0;
            l.resets_at = None;
        }
    }
    r
}

pub fn antigravity() -> Option<AntigravityQuota> {
    let dir = dirs::home_dir()?.join(".gemini/antigravity-cli");
    if !dir.is_dir() {
        return None; // not installed — hide the section
    }
    let now = Local::now().naive_local();
    for (path, _) in newest_files(&dir.join("log"), Some("cli-"), "log", 6) {
        let Some(year) = path.file_name().and_then(|n| n.to_str()).and_then(log_year) else { continue };
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        if let Some((hit, reset)) = content.lines().rev().find_map(|l| parse_quota_line(l, year)) {
            let to_rfc = |n: NaiveDateTime| Local.from_local_datetime(&n).single().map(|d| d.to_rfc3339());
            return Some(AntigravityQuota {
                blocked: reset > now,
                resets_at: to_rfc(reset),
                hit_at: to_rfc(hit),
            });
        }
    }
    Some(AntigravityQuota { blocked: false, resets_at: None, hit_at: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"{"timestamp":"2026-10-03T22:26:40.939Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","limit_name":null,"primary":{"used_percent":94.0,"window_minutes":300,"resets_at":1791080716},"secondary":{"used_percent":38.0,"window_minutes":10080,"resets_at":1791604899},"plan_type":"plus"}}}"#;
    const PREMIUM: &str = r#"{"timestamp":"2026-10-03T22:30:00Z","payload":{"type":"token_count","rate_limits":{"limit_id":"premium","primary":null,"secondary":null}}}"#;

    #[test]
    fn codex_line_yields_both_windows() {
        let r = parse_codex_line(LINE, 1_791_000_000).unwrap();
        assert_eq!(r.plan.as_deref(), Some("plus"));
        assert_eq!(r.as_of.as_deref(), Some("2026-10-03T22:26:40.939Z"));
        assert_eq!(r.limits[0].name, "5-Stunden-Fenster");
        assert_eq!(r.limits[0].percent, 94.0);
        assert_eq!(r.limits[1].name, "Woche");
        assert_eq!(r.limits[1].percent, 38.0);
    }

    #[test]
    fn rolled_over_window_counts_as_zero() {
        // now is past the 5-hour reset but before the weekly one
        let r = parse_codex_line(LINE, 1_791_080_716).unwrap();
        assert_eq!(r.limits[0].percent, 0.0);
        assert!(r.limits[0].resets_at.is_none());
        assert_eq!(r.limits[1].percent, 38.0);
    }

    #[test]
    fn premium_and_other_lines_are_skipped_newest_codex_wins() {
        assert!(parse_codex_line(PREMIUM, 0).is_none());
        let content = format!("{LINE}\n{PREMIUM}\n{{\"x\":1}}\nnot json");
        let r = codex_from_content(&content, 1_791_000_000).unwrap();
        assert_eq!(r.limits[0].percent, 94.0);
        assert!(codex_from_content("garbage\n{}", 0).is_none());
    }

    /// Reads the real local files. `cargo test … agent_limits_live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn agent_limits_live() {
        println!("codex: {:#?}", codex());
        println!("antigravity: {:#?}", antigravity());
    }

    #[test]
    fn window_names() {
        assert_eq!(window_name(300), "5-Stunden-Fenster");
        assert_eq!(window_name(10080), "Woche");
        assert_eq!(window_name(45), "45-Min.-Fenster");
    }

    #[test]
    fn go_durations() {
        assert_eq!(parse_go_duration("88h4m0s"), Some(88 * 3600 + 240));
        assert_eq!(parse_go_duration("14m18s"), Some(858));
        assert_eq!(parse_go_duration("1.5s"), Some(2));
        assert_eq!(parse_go_duration(""), None);
        assert_eq!(parse_go_duration("12"), None);
        assert_eq!(parse_go_duration("3d"), None);
    }

    #[test]
    fn antigravity_quota_line() {
        let l = "E1003 06:09:48.211684  258432 errorreport.go:224] generating and executing: RESOURCE_EXHAUSTED (code 429): Individual quota reached. Please upgrade your subscription to increase your limits. Resets in 88h4m0s.";
        let (hit, reset) = parse_quota_line(l, 2026).unwrap();
        assert_eq!(hit.to_string(), "2026-10-03 06:09:48");
        assert_eq!(reset.to_string(), "2026-10-06 22:13:48");
        assert!(parse_quota_line("I1003 06:09:48 something else", 2026).is_none());
        assert_eq!(log_year("cli-20261003_060915.log"), Some(2026));
        assert_eq!(log_year("other.log"), None);
    }
}
