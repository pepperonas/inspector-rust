//! Claude Code token usage — the `tokens` search-bar command (v0.101.0).
//!
//! Pulls the live overview from the local **Token Tracker** dashboard
//! (`http://127.0.0.1:5010`, LaunchAgent `io.celox.token-tracker`) and surfaces
//! it as an inline preview panel: totals / cost · projects / sessions · models,
//! with a period picker (today / 7d / 30d / all).
//!
//! We deliberately call the running Node aggregator's HTTP API rather than
//! opening `tracker.db` or re-parsing `~/.claude/projects/**/*.jsonl` — Claude
//! prunes old JSONL, the SQLite file is WAL-live while the tracker runs, and
//! pricing/aggregation (~1.8k LOC) already lives in the tracker. Connection
//! refused → `ERR_UNREACHABLE` (frontend shows a "start Token Tracker" card).
//!
//! Pure cores (URL builder, JSON parsers, period → `from`/`to`) are unit-tested;
//! the HTTP call needs a live tracker and is not.

use std::io::Read;
use std::time::Duration;

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Default Token Tracker base URL (loopback, no auth in single-user mode).
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:5010";

/// Sentinel when nothing is listening on the tracker port.
pub const ERR_UNREACHABLE: &str = "tokens.unreachable";

const HTTP_TIMEOUT: Duration = Duration::from_secs(4);

/// Cap on sessions returned over IPC — a full history can be thousands of rows.
const SESSION_CAP: usize = 80;

// ── Period ──────────────────────────────────────────────────────────────────

/// Named windows the panel chips expose. `All` omits `from` (tracker returns
/// the earliest day it has). Dates are **local** `YYYY-MM-DD`, matching the
/// tracker's `toLocalDate` filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Period {
    Today,
    #[serde(rename = "7d")]
    Days7,
    #[serde(rename = "30d")]
    Days30,
    All,
}

/// Inclusive local-date window for a [`Period`], relative to `today`.
/// Returns `(from, to)` where `from` is `None` for [`Period::All`].
pub fn period_range(period: Period, today: NaiveDate) -> (Option<String>, String) {
    let to = today.format("%Y-%m-%d").to_string();
    match period {
        Period::Today => (Some(to.clone()), to),
        Period::Days7 => {
            let from = today - chrono::Duration::days(6);
            (Some(from.format("%Y-%m-%d").to_string()), to)
        }
        Period::Days30 => {
            let from = today - chrono::Duration::days(29);
            (Some(from.format("%Y-%m-%d").to_string()), to)
        }
        Period::All => (None, to),
    }
}

/// Build a tracker API URL. Pure + unit-tested.
pub fn build_url(base: &str, path: &str, from: Option<&str>, to: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    match from {
        Some(f) if !f.is_empty() => format!("{base}{path}?from={f}&to={to}"),
        _ => format!("{base}{path}?to={to}"),
    }
}

// ── Wire types ──────────────────────────────────────────────────────────────

#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Overview {
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_create_tokens: i64,
    pub estimated_cost: f64,
    pub input_cost: f64,
    pub output_cost: f64,
    pub cache_read_cost: f64,
    pub cache_create_cost: f64,
    pub sessions: i64,
    pub total_active_min: i64,
    pub avg_active_min_per_day: i64,
    pub active_days: i64,
    pub messages: i64,
    pub rate_limit_hits: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
    pub lines_written: i64,
    pub period_from: Option<String>,
    pub period_to: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ModelRow {
    pub model: String,
    pub label: String,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_create_tokens: i64,
    pub cost: f64,
    pub messages: i64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ProjectRow {
    pub name: String,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_create_tokens: i64,
    pub cost: f64,
    pub messages: i64,
    pub sessions: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
    pub lines_written: i64,
    pub first_ts: Option<String>,
    pub last_ts: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SessionRow {
    pub id: String,
    pub project: String,
    pub models: Vec<String>,
    pub first_ts: String,
    pub last_ts: String,
    pub duration_min: i64,
    pub active_min: i64,
    pub messages: i64,
    pub total_tokens: i64,
    pub cost: f64,
}

/// One refetch payload for the panel (overview + the three list tabs).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub overview: Overview,
    pub models: Vec<ModelRow>,
    pub projects: Vec<ProjectRow>,
    pub sessions: Vec<SessionRow>,
    /// Echo of the requested window (local YYYY-MM-DD).
    pub from: Option<String>,
    pub to: String,
}

// ── Parsers (pure) ──────────────────────────────────────────────────────────

fn i64_of(v: &Value, key: &str) -> i64 {
    v.get(key)
        .and_then(|x| x.as_i64().or_else(|| x.as_f64().map(|f| f as i64)))
        .unwrap_or(0)
}

fn f64_of(v: &Value, key: &str) -> f64 {
    v.get(key)
        .and_then(|x| x.as_f64().or_else(|| x.as_i64().map(|n| n as f64)))
        .unwrap_or(0.0)
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

fn opt_str(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Parse `/api/overview` JSON into [`Overview`]. Tolerant of missing fields.
pub fn parse_overview(v: &Value) -> Overview {
    Overview {
        total_tokens: i64_of(v, "totalTokens"),
        input_tokens: i64_of(v, "inputTokens"),
        output_tokens: i64_of(v, "outputTokens"),
        cache_read_tokens: i64_of(v, "cacheReadTokens"),
        cache_create_tokens: i64_of(v, "cacheCreateTokens"),
        estimated_cost: f64_of(v, "estimatedCost"),
        input_cost: f64_of(v, "inputCost"),
        output_cost: f64_of(v, "outputCost"),
        cache_read_cost: f64_of(v, "cacheReadCost"),
        cache_create_cost: f64_of(v, "cacheCreateCost"),
        sessions: i64_of(v, "sessions"),
        total_active_min: i64_of(v, "totalActiveMin"),
        avg_active_min_per_day: i64_of(v, "avgActiveMinPerDay"),
        active_days: i64_of(v, "activeDays"),
        messages: i64_of(v, "messages"),
        rate_limit_hits: i64_of(v, "rateLimitHits"),
        lines_added: i64_of(v, "linesAdded"),
        lines_removed: i64_of(v, "linesRemoved"),
        lines_written: i64_of(v, "linesWritten"),
        period_from: opt_str(v, "periodFrom"),
        period_to: opt_str(v, "periodTo"),
    }
}

/// Parse `/api/models` (array).
pub fn parse_models(v: &Value) -> Vec<ModelRow> {
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .map(|m| ModelRow {
            model: str_of(m, "model"),
            label: {
                let l = str_of(m, "label");
                if l.is_empty() {
                    str_of(m, "model")
                } else {
                    l
                }
            },
            total_tokens: i64_of(m, "totalTokens"),
            input_tokens: i64_of(m, "inputTokens"),
            output_tokens: i64_of(m, "outputTokens"),
            cache_read_tokens: i64_of(m, "cacheReadTokens"),
            cache_create_tokens: i64_of(m, "cacheCreateTokens"),
            cost: f64_of(m, "cost"),
            messages: i64_of(m, "messages"),
        })
        .collect()
}

/// Parse `/api/projects` (array).
pub fn parse_projects(v: &Value) -> Vec<ProjectRow> {
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .map(|p| ProjectRow {
            name: str_of(p, "name"),
            total_tokens: i64_of(p, "totalTokens"),
            input_tokens: i64_of(p, "inputTokens"),
            output_tokens: i64_of(p, "outputTokens"),
            cache_read_tokens: i64_of(p, "cacheReadTokens"),
            cache_create_tokens: i64_of(p, "cacheCreateTokens"),
            cost: f64_of(p, "cost"),
            messages: i64_of(p, "messages"),
            sessions: i64_of(p, "sessions"),
            lines_added: i64_of(p, "linesAdded"),
            lines_removed: i64_of(p, "linesRemoved"),
            lines_written: i64_of(p, "linesWritten"),
            first_ts: opt_str(p, "firstTs"),
            last_ts: opt_str(p, "lastTs"),
        })
        .collect()
}

/// Parse `/api/sessions` (array), truncated to [`SESSION_CAP`] (already
/// newest-first from the tracker).
pub fn parse_sessions(v: &Value) -> Vec<SessionRow> {
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .take(SESSION_CAP)
        .map(|s| {
            let models = s
                .get("models")
                .and_then(|m| m.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            SessionRow {
                id: str_of(s, "id"),
                project: str_of(s, "project"),
                models,
                first_ts: str_of(s, "firstTs"),
                last_ts: str_of(s, "lastTs"),
                duration_min: i64_of(s, "durationMin"),
                active_min: i64_of(s, "activeMin"),
                messages: i64_of(s, "messages"),
                total_tokens: i64_of(s, "totalTokens"),
                cost: f64_of(s, "cost"),
            }
        })
        .collect()
}

// ── HTTP ────────────────────────────────────────────────────────────────────

fn get_json(url: &str) -> Result<Value, String> {
    let resp = ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .build()
        .get(url)
        .call()
        .map_err(|e| match e {
            ureq::Error::Transport(_) => ERR_UNREACHABLE.to_string(),
            other => other.to_string(),
        })?;
    let mut body = String::new();
    resp.into_reader()
        .take(8 * 1024 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

/// Fetch a full panel snapshot for `period` from the tracker at `base_url`.
pub fn fetch(base_url: &str, period: Period) -> Result<Snapshot, String> {
    let today = Local::now().date_naive();
    let (from, to) = period_range(period, today);
    let from_ref = from.as_deref();

    let overview_v = get_json(&build_url(base_url, "/api/overview", from_ref, &to))?;
    let models_v = get_json(&build_url(base_url, "/api/models", from_ref, &to))?;
    let projects_v = get_json(&build_url(base_url, "/api/projects", from_ref, &to))?;
    let sessions_v = get_json(&build_url(base_url, "/api/sessions", from_ref, &to))?;

    Ok(Snapshot {
        overview: parse_overview(&overview_v),
        models: parse_models(&models_v),
        projects: parse_projects(&projects_v),
        sessions: parse_sessions(&sessions_v),
        from,
        to,
    })
}

/// Convenience: fetch from the default local tracker URL.
pub fn fetch_default(period: Period) -> Result<Snapshot, String> {
    fetch(DEFAULT_BASE_URL, period)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn period_range_today_and_windows() {
        let today = NaiveDate::from_ymd_opt(2026, 7, 31).unwrap();
        assert_eq!(
            period_range(Period::Today, today),
            (Some("2026-07-31".into()), "2026-07-31".into())
        );
        assert_eq!(
            period_range(Period::Days7, today),
            (Some("2026-07-25".into()), "2026-07-31".into())
        );
        assert_eq!(
            period_range(Period::Days30, today),
            (Some("2026-07-02".into()), "2026-07-31".into())
        );
        assert_eq!(
            period_range(Period::All, today),
            (None, "2026-07-31".into())
        );
    }

    #[test]
    fn build_url_with_and_without_from() {
        assert_eq!(
            build_url("http://127.0.0.1:5010", "/api/overview", Some("2026-07-01"), "2026-07-31"),
            "http://127.0.0.1:5010/api/overview?from=2026-07-01&to=2026-07-31"
        );
        assert_eq!(
            build_url("http://127.0.0.1:5010/", "api/models", None, "2026-07-31"),
            "http://127.0.0.1:5010/api/models?to=2026-07-31"
        );
    }

    #[test]
    fn parse_overview_full() {
        let v = json!({
            "totalTokens": 1000,
            "inputTokens": 100,
            "outputTokens": 200,
            "cacheReadTokens": 600,
            "cacheCreateTokens": 100,
            "estimatedCost": 12.34,
            "inputCost": 1.0,
            "outputCost": 2.0,
            "cacheReadCost": 8.0,
            "cacheCreateCost": 1.34,
            "sessions": 5,
            "totalActiveMin": 120,
            "avgActiveMinPerDay": 40,
            "activeDays": 3,
            "messages": 50,
            "rateLimitHits": 1,
            "linesAdded": 10,
            "linesRemoved": 2,
            "linesWritten": 8,
            "periodFrom": "2026-07-01",
            "periodTo": "2026-07-31"
        });
        let o = parse_overview(&v);
        assert_eq!(o.total_tokens, 1000);
        assert_eq!(o.sessions, 5);
        assert!((o.estimated_cost - 12.34).abs() < 1e-9);
        assert_eq!(o.period_from.as_deref(), Some("2026-07-01"));
    }

    #[test]
    fn parse_overview_tolerates_missing() {
        let o = parse_overview(&json!({}));
        assert_eq!(o.total_tokens, 0);
        assert!(o.period_from.is_none());
    }

    #[test]
    fn parse_models_projects_sessions() {
        let models = parse_models(&json!([
            {
                "model": "claude-opus-4",
                "label": "Opus 4",
                "totalTokens": 500,
                "inputTokens": 10,
                "outputTokens": 20,
                "cacheReadTokens": 400,
                "cacheCreateTokens": 70,
                "cost": 3.5,
                "messages": 9
            }
        ]));
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].label, "Opus 4");
        assert!((models[0].cost - 3.5).abs() < 1e-9);

        let projects = parse_projects(&json!([
            {
                "name": "claude/inspector/rust",
                "totalTokens": 100,
                "cost": 1.25,
                "messages": 4,
                "sessions": 2,
                "firstTs": "2026-07-01T00:00:00Z",
                "lastTs": "2026-07-31T00:00:00Z"
            }
        ]));
        assert_eq!(projects[0].name, "claude/inspector/rust");
        assert_eq!(projects[0].sessions, 2);

        let sessions = parse_sessions(&json!([
            {
                "id": "abc",
                "project": "x",
                "models": ["Opus 4"],
                "firstTs": "a",
                "lastTs": "b",
                "durationMin": 10,
                "activeMin": 8,
                "messages": 3,
                "totalTokens": 50,
                "cost": 0.5
            }
        ]));
        assert_eq!(sessions[0].id, "abc");
        assert_eq!(sessions[0].models, vec!["Opus 4".to_string()]);
    }

    #[test]
    fn parse_sessions_caps_at_limit() {
        let arr: Vec<_> = (0..200)
            .map(|i| {
                json!({
                    "id": format!("s{i}"),
                    "project": "p",
                    "models": [],
                    "firstTs": "",
                    "lastTs": "",
                    "durationMin": 0,
                    "activeMin": 0,
                    "messages": 0,
                    "totalTokens": 0,
                    "cost": 0
                })
            })
            .collect();
        let sessions = parse_sessions(&Value::Array(arr));
        assert_eq!(sessions.len(), SESSION_CAP);
        assert_eq!(sessions[0].id, "s0");
    }

    #[test]
    fn period_range_crosses_month_boundary() {
        let today = NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
        assert_eq!(
            period_range(Period::Days7, today),
            (Some("2026-02-27".into()), "2026-03-05".into())
        );
        assert_eq!(
            period_range(Period::Days30, today),
            (Some("2026-02-04".into()), "2026-03-05".into())
        );
    }

    #[test]
    fn period_range_leap_day_window() {
        let today = NaiveDate::from_ymd_opt(2024, 3, 1).unwrap();
        assert_eq!(
            period_range(Period::Days7, today),
            (Some("2024-02-24".into()), "2024-03-01".into())
        );
    }

    #[test]
    fn build_url_empty_from_omits_from_param() {
        assert_eq!(
            build_url("http://127.0.0.1:5010", "/api/overview", Some(""), "2026-07-31"),
            "http://127.0.0.1:5010/api/overview?to=2026-07-31"
        );
    }

    #[test]
    fn build_url_trims_trailing_slash_on_base() {
        assert_eq!(
            build_url("http://127.0.0.1:5010///", "/api/models", Some("a"), "b"),
            "http://127.0.0.1:5010/api/models?from=a&to=b"
        );
    }

    #[test]
    fn parse_overview_coerces_float_counts_and_int_costs() {
        let o = parse_overview(&json!({
            "totalTokens": 12.9,
            "sessions": 3.1,
            "estimatedCost": 7,
            "inputCost": 1,
            "outputCost": 2,
            "periodFrom": "",
            "periodTo": "2026-07-31"
        }));
        assert_eq!(o.total_tokens, 12);
        assert_eq!(o.sessions, 3);
        assert!((o.estimated_cost - 7.0).abs() < 1e-9);
        assert!(o.period_from.is_none()); // empty string → None
        assert_eq!(o.period_to.as_deref(), Some("2026-07-31"));
    }

    #[test]
    fn parse_models_skips_non_array_and_tolerates_sparse_rows() {
        assert!(parse_models(&json!({"nope": true})).is_empty());
        let rows = parse_models(&json!([{"label": "X"}, {"model": "m", "cost": "bad"}]));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "X");
        assert_eq!(rows[0].model, "");
        assert_eq!(rows[0].total_tokens, 0);
        assert_eq!(rows[1].model, "m");
        assert_eq!(rows[1].cost, 0.0); // non-numeric cost → 0
    }

    #[test]
    fn parse_projects_non_array_and_missing_timestamps() {
        assert!(parse_projects(&json!(null)).is_empty());
        let rows = parse_projects(&json!([{
            "name": "solo",
            "totalTokens": 9,
            "firstTs": "",
            "lastTs": null
        }]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "solo");
        assert_eq!(rows[0].total_tokens, 9);
        assert!(rows[0].first_ts.is_none());
        assert!(rows[0].last_ts.is_none());
    }

    #[test]
    fn parse_sessions_non_array_and_filters_non_string_models() {
        assert!(parse_sessions(&json!("nope")).is_empty());
        let rows = parse_sessions(&json!([{
            "id": "s1",
            "project": "p",
            "models": ["Opus", 42, null, "Sonnet"],
            "firstTs": "a",
            "lastTs": "b",
            "durationMin": 1,
            "activeMin": 1,
            "messages": 1,
            "totalTokens": 1,
            "cost": 0.1
        }]));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].models, vec!["Opus".to_string(), "Sonnet".to_string()]);
    }

    #[test]
    fn parse_sessions_missing_models_defaults_empty() {
        let rows = parse_sessions(&json!([{
            "id": "s",
            "project": "p",
            "firstTs": "",
            "lastTs": "",
            "durationMin": 0,
            "activeMin": 0,
            "messages": 0,
            "totalTokens": 0,
            "cost": 0
        }]));
        assert_eq!(rows[0].models, Vec::<String>::new());
    }

    #[test]
    fn period_serde_aliases() {
        assert_eq!(
            serde_json::from_str::<Period>(r#""today""#).unwrap(),
            Period::Today
        );
        assert_eq!(
            serde_json::from_str::<Period>(r#""7d""#).unwrap(),
            Period::Days7
        );
        assert_eq!(
            serde_json::from_str::<Period>(r#""30d""#).unwrap(),
            Period::Days30
        );
        assert_eq!(
            serde_json::from_str::<Period>(r#""all""#).unwrap(),
            Period::All
        );
    }

    #[test]
    fn constants_are_stable_contract() {
        assert_eq!(DEFAULT_BASE_URL, "http://127.0.0.1:5010");
        assert_eq!(ERR_UNREACHABLE, "tokens.unreachable");
        assert_eq!(SESSION_CAP, 80);
    }
}
