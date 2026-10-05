//! Pace + projection for the `limits` panel (v0.196.0).
//!
//! Design: `docs/superpowers/specs/2026-10-05-claude-limits-forecast-design.md`.
//! The calibrated projection (cost history, previous weeks, window chart) is
//! computed by the Token Tracker and read from its HTTP API
//! (`GET /api/usage-limits`, field `forecast`, contract `version: 1`).
//! Inspector Rust does NOT recompute it — it only displays it.
//!
//! Without the tracker (port closed, or a tracker that predates `forecast`)
//! each limit gets a local **linear** projection from its own report:
//! pace against the even line through the window, and the current rate
//! extrapolated to the reset (±40 % band). Same formulas as the tracker's
//! linear fallback, so both sources agree where they overlap.

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;

use crate::claude_limits::Limit;

/// The contract version this build understands.
pub const CONTRACT_VERSION: u32 = 1;
/// Below this share of the window there is no projection (`too_early`).
pub const MIN_ELAPSED_SHARE: f64 = 0.10;
/// `ahead` once usage leads the even line by more than this many points.
pub const AHEAD_POINTS: f64 = 5.0;
/// Linear band: the growth to the reset varies by ±40 %.
pub const LINEAR_BAND: f64 = 0.4;
/// Tracker answers are cached this long.
const TRACKER_CACHE_MS: i64 = 60_000;
const TRACKER_TIMEOUT_S: u64 = 4;

// ── Contract types (serde: tolerant, every field optional) ──────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Forecast {
    pub version: u32,
    /// `calibrated` | `snapshots` | `linear` | `none`
    pub basis: String,
    /// `good` | `rough` | `none`
    pub confidence: String,
    /// `reserve` | `ahead` | `exhausts` | `idle` | `unknown`
    pub status: String,
    pub window: Option<Window>,
    pub now: Option<String>,
    pub pace: Option<Pace>,
    pub at_reset: Option<Band>,
    pub exhausts_at: Option<ExhaustsAt>,
    pub k: Option<f64>,
    pub notes: Vec<String>,
    pub series: Option<Series>,
    /// Not part of the tracker contract: where this forecast came from
    /// (`tracker` | `local`). Set by Inspector Rust.
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Window {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Pace {
    pub plan_percent: f64,
    pub delta_points: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Band {
    pub median: f64,
    pub low: f64,
    pub high: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ExhaustsAt {
    pub median: Option<String>,
    pub early: Option<String>,
    pub late: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Series {
    pub actual: Vec<(String, f64)>,
    pub measured: Vec<(String, f64)>,
    pub forecast: Vec<(String, f64, f64, f64)>,
    pub ghosts: Vec<Ghost>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Ghost {
    pub start: String,
    /// `[offsetMinutes, percent]`
    pub points: Vec<(f64, f64)>,
}

/// One limit as the tracker reports it — just what matching needs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrackerLimit {
    /// `claude` | `codex` | `antigravity`
    pub provider: String,
    pub kind: String,
    pub resets_at: Option<String>,
    pub scope_label: Option<String>,
    pub window_minutes: Option<i64>,
    pub forecast: Option<Forecast>,
}

// ── Time helpers ────────────────────────────────────────────────────────────

fn parse_ms(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis())
}

fn iso(ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .unwrap_or_default()
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Same instant to the minute (the APIs disagree on sub-second digits).
fn same_minute(a: &str, b: &str) -> bool {
    match (parse_ms(a), parse_ms(b)) {
        (Some(x), Some(y)) => x.div_euclid(60_000) == y.div_euclid(60_000),
        _ => false,
    }
}

/// Window length of a Claude limit, from its `kind`.
pub fn claude_window_minutes(kind: &str) -> Option<i64> {
    match kind {
        "session" | "five_hour" => Some(300),
        k if k.starts_with("weekly") || k.starts_with("seven_day") => Some(10_080),
        _ => None,
    }
}

// ── Pure: local linear forecast ─────────────────────────────────────────────

fn status_for(percent: f64, delta: f64, exhausts_median: Option<i64>, end: i64) -> &'static str {
    if exhausts_median.is_some_and(|t| t < end) {
        "exhausts"
    } else if percent <= 0.0 {
        "idle"
    } else if delta > AHEAD_POINTS {
        "ahead"
    } else {
        "reserve"
    }
}

/// Pace + linear projection for one limit. `None` without a reset time or a
/// window length; a passed reset yields `status: unknown` without numbers.
pub fn linear_forecast(
    percent: f64,
    resets_at: Option<&str>,
    window_minutes: Option<i64>,
    now_ms: i64,
) -> Option<Forecast> {
    let end = parse_ms(resets_at?)?;
    let win = window_minutes.filter(|m| *m > 0)? * 60_000;
    let start = end - win;
    let mut f = Forecast {
        version: CONTRACT_VERSION,
        basis: "none".into(),
        confidence: "none".into(),
        status: "unknown".into(),
        window: Some(Window { start: iso(start), end: iso(end) }),
        now: Some(iso(now_ms)),
        source: "local".into(),
        ..Default::default()
    };
    if now_ms >= end {
        return Some(f); // reset already passed
    }
    let elapsed = (now_ms - start).max(0);
    let plan = 100.0 * elapsed as f64 / win as f64;
    let delta = percent - plan;
    f.pace = Some(Pace { plan_percent: plan, delta_points: delta });

    if (elapsed as f64) < MIN_ELAPSED_SHARE * win as f64 {
        f.notes.push("too_early".into());
        f.status = status_for(percent, delta, None, end).into();
        return Some(f);
    }

    let remaining = (end - now_ms) as f64;
    let rate = percent.max(0.0) / elapsed as f64; // points per ms
    let growth = rate * remaining;
    f.basis = "linear".into();
    f.confidence = "rough".into();
    f.at_reset = Some(Band {
        median: percent + growth,
        low: percent + growth * (1.0 - LINEAR_BAND),
        high: percent + growth * (1.0 + LINEAR_BAND),
    });
    // When does a rate of `r` reach 100 from `percent`? Already there → now.
    let hit = |r: f64| -> Option<i64> {
        if percent >= 100.0 {
            return Some(now_ms);
        }
        if r <= 0.0 {
            return None;
        }
        let t = now_ms + ((100.0 - percent) / r).round() as i64;
        (t < end).then_some(t)
    };
    let median = hit(rate);
    let early = hit(rate * (1.0 + LINEAR_BAND));
    let late = hit(rate * (1.0 - LINEAR_BAND));
    if early.is_some() {
        f.exhausts_at = Some(ExhaustsAt {
            median: median.map(iso),
            early: early.map(iso),
            late: late.map(iso),
        });
    }
    f.status = status_for(percent, delta, median, end).into();
    Some(f)
}

// ── Pure: tracker contract + matching ───────────────────────────────────────

/// A tracker `forecast` object → our type; `None` for a version this build
/// doesn't understand (that limit then falls back to linear).
pub fn parse_forecast(v: &Value) -> Option<Forecast> {
    let ver = v.get("version").and_then(Value::as_u64)?;
    if ver != CONTRACT_VERSION as u64 {
        return None;
    }
    let mut f: Forecast = serde_json::from_value(v.clone()).ok()?;
    f.source = "tracker".into();
    Some(f)
}

/// The tracker's `/api/usage-limits` body → flat list over all providers.
/// Unknown shapes yield an empty list, never an error.
pub fn parse_tracker(body: &str) -> Vec<TrackerLimit> {
    let Ok(root) = serde_json::from_str::<Value>(body) else { return vec![] };
    let mut out = vec![];
    for provider in ["claude", "codex", "antigravity"] {
        let Some(list) = root.pointer(&format!("/{provider}/data/limits")).and_then(Value::as_array) else {
            continue;
        };
        for l in list {
            out.push(TrackerLimit {
                provider: provider.into(),
                kind: l.get("kind").and_then(Value::as_str).unwrap_or_default().into(),
                resets_at: l.get("resetsAt").and_then(Value::as_str).map(str::to_owned),
                scope_label: l.get("scopeLabel").and_then(Value::as_str).map(str::to_owned),
                window_minutes: l.get("windowMinutes").and_then(Value::as_i64),
                forecast: l.get("forecast").and_then(parse_forecast),
            });
        }
    }
    out
}

/// Find the tracker's entry for one of our limits. Claude: same `kind` and
/// reset minute, plus the model for `weekly_scoped` (our name carries it,
/// e.g. „Woche · Fable"). Codex: same window length and reset minute (the
/// kinds differ: ours are `primary`/`secondary`, the tracker's
/// `session`/`weekly`).
pub fn match_limit<'a>(limit: &Limit, provider: &str, trackers: &'a [TrackerLimit]) -> Option<&'a TrackerLimit> {
    let reset = limit.resets_at.as_deref()?;
    trackers.iter().find(|t| {
        if t.provider != provider || !t.resets_at.as_deref().is_some_and(|r| same_minute(r, reset)) {
            return false;
        }
        match provider {
            "codex" => t.window_minutes.is_some() && t.window_minutes == limit.window_minutes,
            _ => {
                if t.kind != limit.kind {
                    return false;
                }
                if limit.kind == "weekly_scoped" {
                    return t
                        .scope_label
                        .as_deref()
                        .is_some_and(|s| limit.name.to_lowercase().contains(&s.to_lowercase()));
                }
                true
            }
        }
    })
}

/// Give every limit its forecast: the tracker's when it has a usable one,
/// else the local linear projection.
pub fn attach(limits: &mut [Limit], provider: &str, trackers: &[TrackerLimit], now_ms: i64) {
    for l in limits.iter_mut() {
        let from_tracker = match_limit(l, provider, trackers).and_then(|t| t.forecast.clone());
        l.forecast = from_tracker.or_else(|| linear_forecast(l.percent, l.resets_at.as_deref(), l.window_minutes, now_ms));
    }
}

// ── Impure: tracker fetch with a 60 s cache ─────────────────────────────────

struct Cache {
    at_ms: i64,
    data: Vec<TrackerLimit>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// The tracker's limits, at most one request per minute. Unreachable or a
/// broken answer → empty list (every limit then falls back to linear).
pub fn tracker_limits(now_ms: i64) -> Vec<TrackerLimit> {
    {
        let c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = c.as_ref() {
            if now_ms - c.at_ms < TRACKER_CACHE_MS {
                return c.data.clone();
            }
        }
    }
    let url = format!("{}/api/usage-limits", crate::token_usage::DEFAULT_BASE_URL);
    let data = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(TRACKER_TIMEOUT_S))
        .call()
        .ok()
        .and_then(|r| r.into_string().ok())
        .map(|b| parse_tracker(&b))
        .unwrap_or_default();
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Cache { at_ms: now_ms, data: data.clone() });
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3_600_000;
    const WEEK_MIN: i64 = 10_080;

    fn at(s: &str) -> i64 {
        parse_ms(s).unwrap()
    }

    fn limit(kind: &str, name: &str, percent: f64, reset: &str, win: Option<i64>) -> Limit {
        Limit {
            id: format!("{kind}-0"),
            name: name.into(),
            kind: kind.into(),
            group: None,
            percent,
            resets_at: Some(reset.into()),
            severity: None,
            active: false,
            known: true,
            money: None,
            window_minutes: win,
            forecast: None,
        }
    }

    // Week ends Sat 2026-10-10 23:00Z → starts Sat 2026-10-03 23:00Z.
    const END: &str = "2026-10-10T23:00:00Z";

    #[test]
    fn pace_is_measured_against_the_even_line() {
        // 3 of 7 days elapsed → plan 42.86 %.
        let now = at("2026-10-06T23:00:00Z");
        let f = linear_forecast(49.0, Some(END), Some(WEEK_MIN), now).unwrap();
        let p = f.pace.unwrap();
        assert!((p.plan_percent - 300.0 / 7.0).abs() < 1e-9);
        assert!((p.delta_points - (49.0 - 300.0 / 7.0)).abs() < 1e-9);
        // Linear: anyone ahead of the even line reaches 100 before the reset
        // (49 % after 3/7 → 114 %), so the local status is `exhausts`, never
        // `ahead` — `ahead` only appears from the tracker or while too early.
        assert_eq!(f.status, "exhausts");
    }

    #[test]
    fn linear_projection_extrapolates_the_current_rate() {
        // Half the week gone, 30 % used → 60 % at the reset, band 48..72.
        let now = at("2026-10-07T11:00:00Z");
        let f = linear_forecast(30.0, Some(END), Some(WEEK_MIN), now).unwrap();
        let b = f.at_reset.unwrap();
        assert!((b.median - 60.0).abs() < 1e-9);
        assert!((b.low - 48.0).abs() < 1e-9);
        assert!((b.high - 72.0).abs() < 1e-9);
        assert_eq!(f.basis, "linear");
        assert_eq!(f.confidence, "rough");
        assert_eq!(f.status, "reserve");
        assert!(f.exhausts_at.is_none(), "even the fast end stays under 100");
    }

    #[test]
    fn exhaustion_before_the_reset_is_dated() {
        // Half the week gone, 60 % used → 100 % reached after another 2/3 of
        // the elapsed time (56 h), well before the reset.
        let now = at("2026-10-07T11:00:00Z");
        let f = linear_forecast(60.0, Some(END), Some(WEEK_MIN), now).unwrap();
        assert_eq!(f.status, "exhausts");
        let ex = f.exhausts_at.unwrap();
        assert_eq!(parse_ms(ex.median.as_deref().unwrap()).unwrap(), now + 56 * HOUR);
        let early = parse_ms(ex.early.as_deref().unwrap()).unwrap();
        assert!(early < now + 56 * HOUR, "the fast end runs out sooner");
        assert!(ex.late.is_none(), "the slow end makes it to the reset");
    }

    #[test]
    fn only_the_fast_end_exhausting_is_not_yet_exhausts() {
        // 48 % at half time → median 96 (no), fast end 48 + 48·1.4 = 115 (yes).
        let now = at("2026-10-07T11:00:00Z");
        let f = linear_forecast(48.0, Some(END), Some(WEEK_MIN), now).unwrap();
        assert!(f.exhausts_at.as_ref().unwrap().early.is_some());
        assert!(f.exhausts_at.as_ref().unwrap().median.is_none());
        assert_ne!(f.status, "exhausts");
    }

    #[test]
    fn too_early_gives_pace_but_no_projection() {
        // 5 % of the week elapsed.
        let now = at("2026-10-03T23:00:00Z") + (WEEK_MIN * 60_000) / 20;
        let f = linear_forecast(20.0, Some(END), Some(WEEK_MIN), now).unwrap();
        assert_eq!(f.basis, "none");
        assert!(f.at_reset.is_none());
        assert!(f.notes.contains(&"too_early".to_string()));
        assert!(f.pace.is_some());
        assert_eq!(f.status, "ahead");
    }

    #[test]
    fn nothing_used_is_idle() {
        let now = at("2026-10-07T11:00:00Z");
        let f = linear_forecast(0.0, Some(END), Some(WEEK_MIN), now).unwrap();
        assert_eq!(f.status, "idle");
        assert!(f.exhausts_at.is_none());
    }

    #[test]
    fn already_full_exhausts_now() {
        let now = at("2026-10-07T11:00:00Z");
        let f = linear_forecast(100.0, Some(END), Some(WEEK_MIN), now).unwrap();
        assert_eq!(f.status, "exhausts");
        assert_eq!(f.exhausts_at.unwrap().median.as_deref(), Some(iso(now).as_str()));
    }

    #[test]
    fn passed_reset_is_unknown_and_missing_inputs_give_nothing() {
        let f = linear_forecast(40.0, Some(END), Some(WEEK_MIN), at("2026-10-11T00:00:00Z")).unwrap();
        assert_eq!(f.status, "unknown");
        assert!(f.pace.is_none());
        assert!(linear_forecast(40.0, None, Some(WEEK_MIN), 0).is_none());
        assert!(linear_forecast(40.0, Some(END), None, 0).is_none());
        assert!(linear_forecast(40.0, Some("kaputt"), Some(WEEK_MIN), 0).is_none());
    }

    #[test]
    fn window_lengths_by_kind() {
        assert_eq!(claude_window_minutes("session"), Some(300));
        assert_eq!(claude_window_minutes("weekly_scoped"), Some(10_080));
        assert_eq!(claude_window_minutes("seven_day_opus"), Some(10_080));
        assert_eq!(claude_window_minutes("iguana_necktie"), None);
    }

    // ── contract ──

    fn full_forecast() -> Value {
        serde_json::json!({
            "version": 1, "basis": "calibrated", "confidence": "good", "status": "ahead",
            "window": {"start": "2026-10-03T23:00:00Z", "end": END},
            "now": "2026-10-07T11:00:00Z",
            "pace": {"planPercent": 50.0, "deltaPoints": 8.0},
            "atReset": {"median": 96.0, "low": 80.0, "high": 112.0},
            "exhaustsAt": {"median": null, "early": "2026-10-10T08:00:00Z", "late": null},
            "k": 0.0123, "notes": ["few_weeks"],
            "series": {
                "actual": [["2026-10-04T00:00:00Z", 1.0]],
                "measured": [["2026-10-05T00:00:00Z", 12]],
                "forecast": [["2026-10-07T12:00:00Z", 58.2, 57.0, 59.5]],
                "ghosts": [{"start": "2026-09-26T23:00:00Z", "points": [[60, 2.0]]}]
            },
            "somethingNew": {"ignored": true}
        })
    }

    #[test]
    fn full_contract_deserialises_and_ignores_unknown_fields() {
        let f = parse_forecast(&full_forecast()).unwrap();
        assert_eq!(f.basis, "calibrated");
        assert_eq!(f.source, "tracker");
        assert_eq!(f.pace.unwrap().delta_points, 8.0);
        assert_eq!(f.at_reset.unwrap().high, 112.0);
        let s = f.series.unwrap();
        assert_eq!(s.measured[0].1, 12.0);
        assert_eq!(s.forecast[0].3, 59.5);
        assert_eq!(s.ghosts[0].points[0], (60.0, 2.0));
    }

    #[test]
    fn minimal_contract_and_version_gate() {
        let f = parse_forecast(&serde_json::json!({"version": 1, "status": "unknown"})).unwrap();
        assert_eq!(f.status, "unknown");
        assert!(f.series.is_none());
        assert!(parse_forecast(&serde_json::json!({"version": 2, "status": "ahead"})).is_none());
        assert!(parse_forecast(&serde_json::json!({"status": "ahead"})).is_none());
    }

    /// The tracker's real answer on 2026-10-05 (0.7.2, before `forecast`).
    const TRACKER_072: &str = r#"{"claude":{"enabled":true,"status":"ok","data":{"source":"limits","limits":[
      {"id":"session","kind":"session","percentUsed":46,"resetsAt":"2026-10-05T01:40:00.728490+00:00","scopeLabel":null},
      {"id":"weekly_all","kind":"weekly_all","percentUsed":49,"resetsAt":"2026-10-10T23:00:00.728516+00:00","scopeLabel":null},
      {"id":"weekly_scoped:fable","kind":"weekly_scoped","percentUsed":0,"resetsAt":"2026-10-10T23:00:00+00:00","scopeLabel":"Fable"}]}},
      "codex":{"data":{"limits":[{"id":"codex:10080","kind":"weekly","windowMinutes":10080,"percentUsed":55,"resetsAt":"2026-10-10T04:01:40.000Z"}]}},
      "antigravity":{"data":{"limits":[{"id":"quota","kind":"exhausted","percentUsed":100,"resetsAt":"2026-10-06T20:13:48.000Z"}]}}}"#;

    #[test]
    fn tracker_body_parses_across_providers() {
        let t = parse_tracker(TRACKER_072);
        assert_eq!(t.len(), 5);
        assert!(t.iter().all(|l| l.forecast.is_none()), "0.7.2 has no forecast");
        assert_eq!(t[2].scope_label.as_deref(), Some("Fable"));
        assert_eq!(t[3].provider, "codex");
        assert_eq!(t[3].window_minutes, Some(10_080));
        assert!(parse_tracker("not json").is_empty());
        assert!(parse_tracker("{}").is_empty());
    }

    #[test]
    fn matching_by_kind_reset_minute_and_scope_model() {
        let t = parse_tracker(TRACKER_072);
        // Our reset carries different sub-second digits — same minute.
        let all = limit("weekly_all", "Woche · alle Modelle", 49.0, "2026-10-10T23:00:00.9Z", Some(WEEK_MIN));
        assert_eq!(match_limit(&all, "claude", &t).unwrap().kind, "weekly_all");
        let fable = limit("weekly_scoped", "Woche · Fable", 0.0, "2026-10-10T23:00:00Z", Some(WEEK_MIN));
        assert_eq!(match_limit(&fable, "claude", &t).unwrap().scope_label.as_deref(), Some("Fable"));
        let opus = limit("weekly_scoped", "Woche · Opus", 0.0, "2026-10-10T23:00:00Z", Some(WEEK_MIN));
        assert!(match_limit(&opus, "claude", &t).is_none(), "another model's scope never matches");
        let shifted = limit("weekly_all", "Woche", 49.0, "2026-10-10T23:01:00Z", Some(WEEK_MIN));
        assert!(match_limit(&shifted, "claude", &t).is_none(), "a different reset is a different window");
    }

    #[test]
    fn codex_matches_by_window_length() {
        let t = parse_tracker(TRACKER_072);
        let wk = limit("secondary", "Woche", 55.0, "2026-10-10T04:01:40+00:00", Some(WEEK_MIN));
        assert_eq!(match_limit(&wk, "codex", &t).unwrap().provider, "codex");
        let five = limit("primary", "5-Stunden-Fenster", 55.0, "2026-10-10T04:01:40+00:00", Some(300));
        assert!(match_limit(&five, "codex", &t).is_none());
        assert!(match_limit(&wk, "claude", &t).is_none(), "providers never cross");
    }

    #[test]
    fn attach_prefers_the_tracker_and_falls_back_to_linear() {
        let now = at("2026-10-07T11:00:00Z");
        let mut t = parse_tracker(TRACKER_072);
        t[1].forecast = parse_forecast(&full_forecast());
        let mut ls = vec![
            limit("weekly_all", "Woche · alle Modelle", 49.0, "2026-10-10T23:00:00Z", Some(WEEK_MIN)),
            limit("weekly_scoped", "Woche · Fable", 10.0, "2026-10-10T23:00:00Z", Some(WEEK_MIN)),
        ];
        attach(&mut ls, "claude", &t, now);
        assert_eq!(ls[0].forecast.as_ref().unwrap().source, "tracker");
        assert_eq!(ls[0].forecast.as_ref().unwrap().basis, "calibrated");
        assert_eq!(ls[1].forecast.as_ref().unwrap().source, "local");
        assert_eq!(ls[1].forecast.as_ref().unwrap().basis, "linear");
        // Unreachable tracker = empty list → everything linear.
        attach(&mut ls, "claude", &[], now);
        assert!(ls.iter().all(|l| l.forecast.as_ref().unwrap().source == "local"));
    }
}
