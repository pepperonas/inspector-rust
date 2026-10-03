//! `limits` / `quota` — the usage limits of the Claude subscription (session,
//! week, per-model week, extra usage), as claude.ai → Settings → Usage shows
//! them.
//!
//! ⚠️ There is no official API. This calls the UNDOCUMENTED endpoint that
//! Claude Code's own `/usage` uses (`GET api.anthropic.com/api/oauth/usage`,
//! header `anthropic-beta: oauth-2025-04-20`). Its schema is not guaranteed, so
//! the parser is defensive: every field is optional, one broken entry never
//! drops the others, and an unknown limit kind is shown under its raw name
//! instead of being thrown away. A sample of a real response (sensitive values
//! masked) is `docs/usage-api-sample.json`; the tests run against it.
//!
//! **Token handling — read-only, by design.** The OAuth token belongs to Claude
//! Code. It is read FRESH on every fetch (Claude Code rotates it), from the
//! macOS keychain item `Claude Code-credentials` via `/usr/bin/security` (that
//! tool is already on the item's access list; reading through the keychain API
//! from this app would raise an access prompt) with `~/.claude/.credentials.json`
//! as fallback. There is NO refresh flow and NOTHING is ever written back — a
//! second writer could corrupt Claude Code's login. An expired token is
//! reported as such ("start Claude Code once"). The token is never logged,
//! persisted, or sent anywhere but `api.anthropic.com`.
//!
//! **Polling** lives in the process: the panel asks every few seconds, but the
//! network is only touched when the cached report is older than the configured
//! interval (default 5 min, min 2) and no 429 backoff is running. While backing
//! off, the last good report stays visible with its timestamp.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::db::DbHandle;

const ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
const BETA: &str = "oauth-2025-04-20";
const HTTP_TIMEOUT: Duration = Duration::from_secs(12);
const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

pub const KEY_POLL_MIN: &str = "climits.poll_minutes";
pub const DEFAULT_POLL_MIN: u32 = 5;
pub const MIN_POLL_MIN: u32 = 2;
pub const MAX_POLL_MIN: u32 = 60;
/// Ceiling of the 429 backoff.
const MAX_BACKOFF_MS: i64 = 60 * 60 * 1000;

// Error codes the frontend maps to human text.
pub const ERR_NO_TOKEN: &str = "limits.no_token";
pub const ERR_TOKEN_EXPIRED: &str = "limits.token_expired";
pub const ERR_RATE_LIMITED: &str = "limits.rate_limited";
pub const ERR_NETWORK: &str = "limits.network";
pub const ERR_SCHEMA: &str = "limits.schema";

// ── Data model ──────────────────────────────────────────────────────────────

/// One usage limit. `known == false` marks a limit type this build doesn't
/// recognise — shown under its raw name rather than dropped.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Limit {
    pub id: String,
    pub name: String,
    /// The API's `kind` (or the raw field name for top-level entries).
    pub kind: String,
    pub group: Option<String>,
    /// 0..=100 (may exceed 100 if the API says so; the UI clamps the bar).
    pub percent: f64,
    /// RFC 3339 as delivered.
    pub resets_at: Option<String>,
    pub severity: Option<String>,
    pub active: bool,
    pub known: bool,
    /// Money-denominated limits (some codename budgets carry dollars).
    pub money: Option<Money>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Money {
    pub used: f64,
    pub limit: f64,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExtraUsage {
    pub enabled: bool,
    pub used: f64,
    pub limit: f64,
    pub currency: String,
    pub percent: f64,
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BreakdownRow {
    pub name: String,
    pub percent: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct LimitsReport {
    pub limits: Vec<Limit>,
    pub extra: Option<ExtraUsage>,
    /// Share of the weekly usage per surface (Claude Code, chats, …).
    pub breakdown: Vec<BreakdownRow>,
    /// True when `limits[]` was absent/empty and the legacy fields were used.
    pub legacy: bool,
}

/// What the panel receives: the last good report (possibly stale) plus the
/// state of the most recent attempt.
#[derive(Debug, Clone, Serialize, Default)]
pub struct LimitsStatus {
    pub report: Option<LimitsReport>,
    /// When `report` was fetched (epoch ms).
    pub fetched_at_ms: Option<i64>,
    /// Error code of the LAST attempt (`None` = it succeeded or none ran).
    pub error: Option<String>,
    /// Human detail for the error (never contains the token).
    pub error_detail: Option<String>,
    /// While a 429 backoff runs: when the next attempt is allowed.
    pub retry_at_ms: Option<i64>,
    pub poll_minutes: u32,
}

// ── Pure parser ─────────────────────────────────────────────────────────────

fn num(v: Option<&Value>) -> Option<f64> {
    v.and_then(|x| x.as_f64()).filter(|f| f.is_finite())
}

fn string(v: Option<&Value>) -> Option<String> {
    v.and_then(|x| x.as_str()).map(str::to_owned)
}

/// Human name for a limit from the `limits[]` array.
fn limit_name(kind: &str, entry: &Value) -> (String, bool) {
    let scope_model = entry
        .pointer("/scope/model/display_name")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let scope_surface = entry
        .pointer("/scope/surface/display_name")
        .or_else(|| entry.pointer("/scope/surface"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    match kind {
        "session" => ("Aktuelle Sitzung".into(), true),
        "weekly_all" => ("Woche · alle Modelle".into(), true),
        "weekly_scoped" => {
            let what = scope_model.or(scope_surface).unwrap_or_else(|| "begrenzt".into());
            (format!("Woche · {what}"), true)
        }
        other => {
            // Unknown kind: keep the raw name, add a scope hint if any.
            let mut n = other.to_string();
            if let Some(m) = scope_model.or(scope_surface) {
                n = format!("{n} · {m}");
            }
            (n, false)
        }
    }
}

/// One entry of `limits[]`; `None` only if it carries no usable percentage.
fn parse_limit_entry(i: usize, entry: &Value) -> Option<Limit> {
    let obj = entry.as_object()?;
    let kind = string(obj.get("kind")).unwrap_or_else(|| "unknown".into());
    let percent = num(obj.get("percent")).or_else(|| num(obj.get("utilization")))?;
    let (name, known) = limit_name(&kind, entry);
    Some(Limit {
        id: format!("{kind}-{i}"),
        name,
        kind,
        group: string(obj.get("group")),
        percent,
        resets_at: string(obj.get("resets_at")),
        severity: string(obj.get("severity")),
        active: obj.get("is_active").and_then(Value::as_bool).unwrap_or(false),
        known,
        money: None,
    })
}

/// A top-level object like `{utilization, resets_at, limit_dollars, …}`.
fn parse_window(key: &str, v: &Value, name: &str, known: bool) -> Option<Limit> {
    let obj = v.as_object()?;
    let percent = num(obj.get("utilization")).or_else(|| num(obj.get("percent")))?;
    let money = match (num(obj.get("used_dollars")), num(obj.get("limit_dollars"))) {
        (Some(used), Some(limit)) => Some(Money { used, limit, currency: "USD".into() }),
        _ => None,
    };
    Some(Limit {
        id: key.to_string(),
        name: name.to_string(),
        kind: key.to_string(),
        group: None,
        percent,
        resets_at: string(obj.get("resets_at")),
        severity: None,
        active: false,
        known,
        money,
    })
}

/// Top-level keys that are NOT limits, or are the legacy forms of what
/// `limits[]` already carries (used only as fallback).
fn is_legacy_or_meta(key: &str) -> bool {
    matches!(
        key,
        "five_hour" | "seven_day" | "limits" | "extra_usage" | "spend" | "seven_day_breakdown"
            | "member_dashboard_available"
    ) || key.starts_with("seven_day_")
}

fn money_value(v: Option<&Value>) -> Option<(f64, String)> {
    let o = v?.as_object()?;
    let minor = num(o.get("amount_minor"))?;
    let exp = o.get("exponent").and_then(Value::as_i64).unwrap_or(2).clamp(0, 6) as i32;
    let cur = string(o.get("currency")).unwrap_or_else(|| "EUR".into());
    Some((minor / 10f64.powi(exp), cur))
}

fn parse_extra(root: &serde_json::Map<String, Value>) -> Option<ExtraUsage> {
    // Prefer the structured `spend` block, fall back to `extra_usage`.
    if let Some(s) = root.get("spend").and_then(Value::as_object) {
        if let (Some((used, cur)), Some((limit, _))) =
            (money_value(s.get("used")), money_value(s.get("limit")))
        {
            let percent = num(s.get("percent"))
                .unwrap_or(if limit > 0.0 { used / limit * 100.0 } else { 0.0 });
            return Some(ExtraUsage {
                enabled: s.get("enabled").and_then(Value::as_bool).unwrap_or(false),
                used,
                limit,
                currency: cur,
                percent,
                disabled_reason: string(s.get("disabled_reason")),
            });
        }
    }
    let e = root.get("extra_usage")?.as_object()?;
    let dp = e.get("decimal_places").and_then(Value::as_i64).unwrap_or(2).clamp(0, 6) as i32;
    let div = 10f64.powi(dp);
    let limit = num(e.get("monthly_limit"))? / div;
    let used = num(e.get("used_credits")).unwrap_or(0.0) / div;
    Some(ExtraUsage {
        enabled: e.get("is_enabled").and_then(Value::as_bool).unwrap_or(false),
        used,
        limit,
        currency: string(e.get("currency")).unwrap_or_else(|| "EUR".into()),
        percent: num(e.get("utilization"))
            .unwrap_or(if limit > 0.0 { used / limit * 100.0 } else { 0.0 }),
        disabled_reason: string(e.get("disabled_reason")),
    })
}

fn parse_breakdown(root: &serde_json::Map<String, Value>) -> Vec<BreakdownRow> {
    root.get("seven_day_breakdown")
        .and_then(|b| b.get("rows"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|r| {
                    let name = string(r.get("display_name")).or_else(|| string(r.get("key")))?;
                    Some(BreakdownRow { name, percent: num(r.get("percent"))? })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse a response body. Invalid JSON / a non-object root is an error; every
/// field inside is optional.
pub fn parse(body: &str) -> Result<LimitsReport, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("{ERR_SCHEMA}: {e}"))?;
    let root = v.as_object().ok_or_else(|| format!("{ERR_SCHEMA}: root is not an object"))?;

    let mut limits: Vec<Limit> = root
        .get("limits")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().enumerate().filter_map(|(i, e)| parse_limit_entry(i, e)).collect())
        .unwrap_or_default();

    let legacy = limits.is_empty();
    if legacy {
        if let Some(l) = root.get("five_hour").and_then(|v| parse_window("five_hour", v, "Aktuelle Sitzung", true)) {
            limits.push(l);
        }
        if let Some(l) = root.get("seven_day").and_then(|v| parse_window("seven_day", v, "Woche · alle Modelle", true)) {
            limits.push(l);
        }
    }

    // Filled codename budgets (e.g. `iguana_necktie`): not in `limits[]`, but
    // real — show them under their raw name. Sorted for a stable order.
    let mut keys: Vec<&String> = root.keys().filter(|k| !is_legacy_or_meta(k)).collect();
    keys.sort();
    for k in keys {
        if let Some(l) = parse_window(k, &root[k.as_str()], k, false) {
            limits.push(l);
        }
    }

    Ok(LimitsReport {
        limits,
        extra: parse_extra(root),
        breakdown: parse_breakdown(root),
        legacy,
    })
}

// ── Token (read-only) ───────────────────────────────────────────────────────

/// The access token + its expiry from a Claude Code credentials document.
#[derive(Debug, Clone)]
pub struct Credential {
    pub token: String,
    pub expires_at_ms: Option<i64>,
}

/// Pure: pull `claudeAiOauth.{accessToken, expiresAt}` out of a credentials
/// JSON document.
pub fn credential_from_json(doc: &str) -> Option<Credential> {
    let v: Value = serde_json::from_str(doc).ok()?;
    let o = v.get("claudeAiOauth")?;
    let token = o.get("accessToken")?.as_str()?.trim().to_string();
    if token.is_empty() {
        return None;
    }
    Some(Credential { token, expires_at_ms: o.get("expiresAt").and_then(Value::as_i64) })
}

/// Pure: of several candidates (keychain, file) take the one that expires
/// last — the file can hold an older login than the keychain.
pub fn freshest(cands: Vec<Credential>) -> Option<Credential> {
    cands.into_iter().max_by_key(|c| c.expires_at_ms.unwrap_or(i64::MIN))
}

/// Pure: is this credential expired at `now_ms`? A missing expiry counts as
/// usable (the server will say otherwise).
pub fn is_expired(c: &Credential, now_ms: i64) -> bool {
    c.expires_at_ms.map(|e| e <= now_ms).unwrap_or(false)
}

fn read_credential() -> Option<Credential> {
    let mut cands = Vec::new();
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("/usr/bin/security")
            .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"])
            .output()
        {
            if out.status.success() {
                if let Some(c) = credential_from_json(&String::from_utf8_lossy(&out.stdout)) {
                    cands.push(c);
                }
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        if let Ok(s) = std::fs::read_to_string(home.join(".claude/.credentials.json")) {
            if let Some(c) = credential_from_json(&s) {
                cands.push(c);
            }
        }
    }
    freshest(cands)
}

// ── Polling / backoff (pure) ────────────────────────────────────────────────

pub fn clamp_poll(min: u32) -> u32 {
    min.clamp(MIN_POLL_MIN, MAX_POLL_MIN)
}

/// Pure: should a network fetch happen now?
pub fn should_fetch(
    now_ms: i64,
    last_attempt_ms: Option<i64>,
    poll_ms: i64,
    retry_at_ms: Option<i64>,
    force: bool,
) -> bool {
    if let Some(r) = retry_at_ms {
        if now_ms < r {
            return false; // a 429 backoff runs — not even a forced refresh
        }
    }
    if force {
        return true;
    }
    match last_attempt_ms {
        None => true,
        Some(t) => now_ms - t >= poll_ms,
    }
}

/// Pure: delay before the next try after the `step`-th consecutive 429.
/// Doubles from the poll interval, honours a server `Retry-After`, caps at 1 h.
pub fn backoff_ms(step: u32, poll_ms: i64, retry_after_ms: Option<i64>) -> i64 {
    let exp = poll_ms.saturating_mul(1i64 << step.min(16));
    exp.max(retry_after_ms.unwrap_or(0)).min(MAX_BACKOFF_MS)
}

// ── Process state + fetch ───────────────────────────────────────────────────

#[derive(Default)]
struct State {
    report: Option<LimitsReport>,
    fetched_at_ms: Option<i64>,
    last_attempt_ms: Option<i64>,
    error: Option<String>,
    error_detail: Option<String>,
    retry_at_ms: Option<i64>,
    backoff_step: u32,
}

static STATE: Mutex<State> = Mutex::new(State {
    report: None,
    fetched_at_ms: None,
    last_attempt_ms: None,
    error: None,
    error_detail: None,
    retry_at_ms: None,
    backoff_step: 0,
});

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn poll_minutes(db: &DbHandle) -> u32 {
    crate::settings::get(db, KEY_POLL_MIN)
        .ok()
        .flatten()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .map(clamp_poll)
        .unwrap_or(DEFAULT_POLL_MIN)
}

pub fn set_poll_minutes(db: &DbHandle, min: u32) -> anyhow::Result<u32> {
    let m = clamp_poll(min);
    crate::settings::set(db, KEY_POLL_MIN, &m.to_string())?;
    Ok(m)
}

enum FetchError {
    NoToken,
    Expired,
    RateLimited(Option<i64>),
    Network(String),
    Schema(String),
}

fn fetch_once() -> Result<LimitsReport, FetchError> {
    let cred = read_credential().ok_or(FetchError::NoToken)?;
    if is_expired(&cred, now_ms()) {
        return Err(FetchError::Expired);
    }
    let resp = ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .build()
        .get(ENDPOINT)
        .set("Authorization", &format!("Bearer {}", cred.token))
        .set("anthropic-beta", BETA)
        .set("Accept", "application/json")
        .call();
    drop(cred);
    match resp {
        Ok(r) => {
            let body = r.into_string().map_err(|e| FetchError::Network(e.to_string()))?;
            parse(&body).map_err(FetchError::Schema)
        }
        Err(ureq::Error::Status(401 | 403, _)) => Err(FetchError::Expired),
        Err(ureq::Error::Status(429, r)) => {
            let ra = r
                .header("retry-after")
                .and_then(|s| s.trim().parse::<i64>().ok())
                .map(|s| s * 1000);
            Err(FetchError::RateLimited(ra))
        }
        // Never format the request — only the status / transport kind.
        Err(ureq::Error::Status(code, _)) => Err(FetchError::Network(format!("HTTP {code}"))),
        Err(ureq::Error::Transport(t)) => Err(FetchError::Network(t.kind().to_string())),
    }
}

/// The IPC entry point (blocking — call from `spawn_blocking`). Fetches only
/// when due; otherwise returns the cached state.
pub fn status(db: &DbHandle, force: bool) -> LimitsStatus {
    let poll = poll_minutes(db);
    let poll_ms = poll as i64 * 60_000;
    let now = now_ms();
    let due = {
        let s = STATE.lock().unwrap_or_else(|e| e.into_inner());
        should_fetch(now, s.last_attempt_ms, poll_ms, s.retry_at_ms, force)
    };
    if due {
        let result = fetch_once();
        let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
        s.last_attempt_ms = Some(now);
        match result {
            Ok(r) => {
                s.report = Some(r);
                s.fetched_at_ms = Some(now);
                s.error = None;
                s.error_detail = None;
                s.retry_at_ms = None;
                s.backoff_step = 0;
            }
            Err(e) => {
                let (code, detail): (&str, Option<String>) = match e {
                    FetchError::NoToken => (ERR_NO_TOKEN, None),
                    FetchError::Expired => (ERR_TOKEN_EXPIRED, None),
                    FetchError::RateLimited(ra) => {
                        let wait = backoff_ms(s.backoff_step, poll_ms, ra);
                        s.backoff_step = s.backoff_step.saturating_add(1);
                        s.retry_at_ms = Some(now + wait);
                        (ERR_RATE_LIMITED, None)
                    }
                    FetchError::Network(d) => (ERR_NETWORK, Some(d)),
                    FetchError::Schema(d) => (ERR_SCHEMA, Some(d)),
                };
                tracing::info!("claude limits: fetch failed ({code})");
                s.error = Some(code.to_string());
                s.error_detail = detail;
            }
        }
    }
    let s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    LimitsStatus {
        report: s.report.clone(),
        fetched_at_ms: s.fetched_at_ms,
        error: s.error.clone(),
        error_detail: s.error_detail.clone(),
        retry_at_ms: s.retry_at_ms.filter(|r| *r > now),
        poll_minutes: poll,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../../docs/usage-api-sample.json");

    #[test]
    fn real_sample_parses_into_the_settings_page_limits() {
        let r = parse(SAMPLE).unwrap();
        assert!(!r.legacy);
        let names: Vec<&str> = r.limits.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names[0], "Aktuelle Sitzung");
        assert_eq!(names[1], "Woche · alle Modelle");
        assert_eq!(names[2], "Woche · Fable");
        assert_eq!(r.limits[0].percent, 4.0);
        assert_eq!(r.limits[0].resets_at.as_deref(), Some("2026-10-04T04:00:00.754561+00:00"));
        assert!(r.limits[0].active);
        assert!(r.limits[..3].iter().all(|l| l.known));
    }

    #[test]
    fn filled_codename_budget_is_kept_under_its_raw_name() {
        let r = parse(SAMPLE).unwrap();
        let ig = r.limits.iter().find(|l| l.kind == "iguana_necktie").expect("codename kept");
        assert!(!ig.known);
        assert_eq!(ig.name, "iguana_necktie");
        let m = ig.money.as_ref().unwrap();
        assert_eq!(m.limit, 250.0);
        // null codenames and seven_day_* legacy fields never become rows
        assert!(!r.limits.iter().any(|l| l.kind == "tangelo" || l.kind.starts_with("seven_day")));
    }

    #[test]
    fn extra_usage_and_breakdown_come_from_the_sample() {
        let r = parse(SAMPLE).unwrap();
        let e = r.extra.unwrap();
        assert!(!e.enabled);
        assert_eq!(e.limit, 50.0);
        assert_eq!(e.currency, "EUR");
        assert_eq!(e.disabled_reason.as_deref(), Some("out_of_credits"));
        assert_eq!(r.breakdown[0], BreakdownRow { name: "Claude Code".into(), percent: 98.0 });
    }

    #[test]
    fn legacy_fields_only() {
        let r = parse(r#"{"five_hour":{"utilization":12.5,"resets_at":"2026-10-04T04:00:00Z"},
                          "seven_day":{"utilization":40,"resets_at":"2026-10-10T23:00:00Z"}}"#)
            .unwrap();
        assert!(r.legacy);
        assert_eq!(r.limits.len(), 2);
        assert_eq!(r.limits[0].name, "Aktuelle Sitzung");
        assert_eq!(r.limits[0].percent, 12.5);
        assert_eq!(r.limits[1].percent, 40.0);
    }

    #[test]
    fn empty_limits_array_falls_back_to_legacy() {
        let r = parse(r#"{"limits":[],"five_hour":{"utilization":7}}"#).unwrap();
        assert!(r.legacy);
        assert_eq!(r.limits.len(), 1);
        assert_eq!(r.limits[0].percent, 7.0);
    }

    #[test]
    fn legacy_fields_are_ignored_when_limits_exist() {
        let r = parse(r#"{"limits":[{"kind":"session","percent":3}],"five_hour":{"utilization":99}}"#)
            .unwrap();
        assert_eq!(r.limits.len(), 1);
        assert_eq!(r.limits[0].percent, 3.0);
    }

    #[test]
    fn unknown_limit_kind_is_shown_with_its_raw_name() {
        let r = parse(r#"{"limits":[{"kind":"monthly_frobnicate","percent":55,
            "scope":{"model":{"display_name":"Opus"}}}]}"#)
            .unwrap();
        assert_eq!(r.limits[0].name, "monthly_frobnicate · Opus");
        assert!(!r.limits[0].known);
    }

    #[test]
    fn one_broken_entry_does_not_drop_the_others() {
        let r = parse(r#"{"limits":[{"kind":"session","percent":"lots"},42,
            {"kind":"weekly_all","percent":10}]}"#)
            .unwrap();
        assert_eq!(r.limits.len(), 1);
        assert_eq!(r.limits[0].kind, "weekly_all");
    }

    #[test]
    fn broken_json_is_an_error_not_a_panic() {
        assert!(parse("{not json").unwrap_err().starts_with(ERR_SCHEMA));
        assert!(parse("[1,2]").is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn credential_parsing_and_freshest_pick() {
        let a = credential_from_json(r#"{"claudeAiOauth":{"accessToken":"t-old","expiresAt":100}}"#).unwrap();
        let b = credential_from_json(r#"{"claudeAiOauth":{"accessToken":"t-new","expiresAt":200}}"#).unwrap();
        assert_eq!(freshest(vec![a, b]).unwrap().token, "t-new");
        assert!(credential_from_json(r#"{"claudeAiOauth":{"accessToken":""}}"#).is_none());
        assert!(credential_from_json("garbage").is_none());
        let c = Credential { token: "x".into(), expires_at_ms: Some(1_000) };
        assert!(is_expired(&c, 1_000));
        assert!(!is_expired(&c, 999));
        assert!(!is_expired(&Credential { token: "x".into(), expires_at_ms: None }, 5));
    }

    #[test]
    fn fetch_only_when_due_and_never_inside_a_backoff() {
        let poll = 5 * 60_000;
        assert!(should_fetch(0, None, poll, None, false));
        assert!(!should_fetch(60_000, Some(0), poll, None, false));
        assert!(should_fetch(poll, Some(0), poll, None, false));
        assert!(should_fetch(60_000, Some(0), poll, None, true)); // forced
        assert!(!should_fetch(60_000, Some(0), poll, Some(90_000), true)); // backoff beats force
        assert!(should_fetch(90_000, Some(0), poll, Some(90_000), true));
    }

    #[test]
    fn backoff_doubles_honours_retry_after_and_caps() {
        let poll = 2 * 60_000;
        assert_eq!(backoff_ms(0, poll, None), poll);
        assert_eq!(backoff_ms(1, poll, None), 2 * poll);
        assert_eq!(backoff_ms(0, poll, Some(10 * 60_000)), 10 * 60_000);
        assert_eq!(backoff_ms(20, poll, None), MAX_BACKOFF_MS);
    }

    #[test]
    fn poll_interval_is_clamped() {
        assert_eq!(clamp_poll(0), MIN_POLL_MIN);
        assert_eq!(clamp_poll(5), 5);
        assert_eq!(clamp_poll(500), MAX_POLL_MIN);
    }

    /// Live check against the real endpoint. Prints names + percentages only.
    /// `cargo test -p inspector-rust-core --lib claude_limits_live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn claude_limits_live() {
        match fetch_once() {
            Ok(r) => {
                for l in &r.limits {
                    println!("{:<28} {:>6.1}%  {:?}", l.name, l.percent, l.resets_at);
                }
                println!("extra: {:?}\nbreakdown: {:?}", r.extra, r.breakdown);
            }
            Err(FetchError::NoToken) => println!("no token"),
            Err(FetchError::Expired) => println!("token expired"),
            Err(FetchError::RateLimited(ra)) => println!("429, retry-after {ra:?}"),
            Err(FetchError::Network(d)) | Err(FetchError::Schema(d)) => println!("error: {d}"),
        }
    }

    #[test]
    fn the_sample_holds_no_secret() {
        // The fixture is committed: no bearer token, no ids, no e-mails.
        for needle in ["sk-ant", "Bearer", "@", "accessToken", "refreshToken"] {
            assert!(!SAMPLE.contains(needle), "fixture contains {needle}");
        }
    }
}
