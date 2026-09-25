//! GitHub activity for the `repo` panel (v0.185.0): pushes (Events API),
//! PRs opened/merged, issues opened/closed — per 24 h / 7 d window and the
//! period before. Pure counters over `serde_json::Value` + a thin ureq shell.
//!
//! Since 2025 a `PushEvent` payload carries no commit count (only
//! ref/before/head), so pushes are counted, not pushed commits. The Events
//! API returns at most 300 events / 90 days: three FULL pages whose oldest
//! event is still inside the 14-day horizon mean the count is incomplete
//! (`pushes_capped`). The token only ever travels as an Authorization header.

use crate::repo_activity::{in_window, DAY, WEEK};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const PAGE: usize = 100;
const MAX_PAGES: usize = 3;
const TIMEOUT_SECS: u64 = 8;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct GithubCounts {
    pub pushes: u64,
    pub prs_opened: u64,
    pub prs_merged: u64,
    pub issues_opened: u64,
    pub issues_closed: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct GithubActivity {
    pub day: GithubCounts,
    pub day_prev: GithubCounts,
    pub week: GithubCounts,
    pub week_prev: GithubCounts,
    pub pushes_capped: bool,
}

/// Four window counts for one kind of timestamp.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Win {
    pub day: u64,
    pub day_prev: u64,
    pub week: u64,
    pub week_prev: u64,
}

impl Win {
    fn add(&mut self, t: i64, now: i64) {
        if in_window(t, now - DAY, now) { self.day += 1; }
        if in_window(t, now - 2 * DAY, now - DAY) { self.day_prev += 1; }
        if in_window(t, now - WEEK, now) { self.week += 1; }
        if in_window(t, now - 2 * WEEK, now - WEEK) { self.week_prev += 1; }
    }
}

pub struct EventCounts {
    pub day: u64,
    pub day_prev: u64,
    pub week: u64,
    pub week_prev: u64,
    pub capped: bool,
}

pub struct TwoWin {
    pub opened: Win,
    pub merged: Win,
    pub closed: Win,
}

pub(crate) fn ts(v: &Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|d| d.timestamp())
}

pub fn count_events(pages: &[Vec<Value>], now: i64) -> EventCounts {
    let mut w = Win::default();
    for e in pages.iter().flatten() {
        if e["type"] == "PushEvent" {
            if let Some(t) = ts(&e["created_at"]) { w.add(t, now); }
        }
    }
    let oldest_inside_horizon = pages
        .last()
        .and_then(|p| p.iter().filter_map(|e| ts(&e["created_at"])).min())
        .is_some_and(|t| t > now - 2 * WEEK);
    let capped = pages.len() >= MAX_PAGES && pages.iter().all(|p| p.len() >= PAGE) && oldest_inside_horizon;
    EventCounts { day: w.day, day_prev: w.day_prev, week: w.week, week_prev: w.week_prev, capped }
}

pub fn count_pulls(pulls: &[Value], now: i64) -> TwoWin {
    let (mut opened, mut merged) = (Win::default(), Win::default());
    for p in pulls {
        if let Some(t) = ts(&p["created_at"]) { opened.add(t, now); }
        if let Some(t) = ts(&p["merged_at"]) { merged.add(t, now); }
    }
    TwoWin { opened, merged, closed: Win::default() }
}

pub fn count_issues(issues: &[Value], now: i64) -> TwoWin {
    let (mut opened, mut closed) = (Win::default(), Win::default());
    // The issues endpoint also returns pull requests — they carry a
    // `pull_request` key and are counted by `count_pulls` instead.
    for i in issues.iter().filter(|i| i.get("pull_request").is_none()) {
        if let Some(t) = ts(&i["created_at"]) { opened.add(t, now); }
        if let Some(t) = ts(&i["closed_at"]) { closed.add(t, now); }
    }
    TwoWin { opened, merged: Win::default(), closed }
}

pub fn api_url(owner: &str, repo: &str, path_and_query: &str) -> String {
    format!("https://api.github.com/repos/{owner}/{repo}/{path_and_query}")
}

pub fn classify_status(status: u16, remaining: Option<&str>) -> &'static str {
    match status {
        429 => "github.rate_limit",
        403 if remaining == Some("0") => "github.rate_limit",
        401 | 403 => "github.auth",
        404 => "github.not_found",
        _ => "github.http",
    }
}

/// Every header of an API request. The token travels ONLY here (never in the
/// URL), and only when there is one.
pub fn request_headers(token: Option<&str>) -> Vec<(&'static str, String)> {
    let mut h = vec![
        ("Accept", "application/vnd.github+json".to_string()),
        ("X-GitHub-Api-Version", "2022-11-28".to_string()),
        ("User-Agent", "inspector-rust".to_string()),
    ];
    if let Some(t) = token.map(str::trim).filter(|t| !t.is_empty()) {
        h.push(("Authorization", format!("Bearer {t}")));
    }
    h
}

fn get_page(url: &str, token: Option<&str>) -> Result<Vec<Value>, String> {
    let mut req = ureq::get(url).timeout(std::time::Duration::from_secs(TIMEOUT_SECS));
    for (k, v) in request_headers(token) {
        req = req.set(k, &v);
    }
    match req.call() {
        Ok(r) => {
            // ureq is built without its `json` feature — parse the text ourselves.
            let body = r.into_string().map_err(|e| format!("github.http: Antwort unlesbar: {e}"))?;
            serde_json::from_str::<Vec<Value>>(&body).map_err(|e| format!("github.http: Antwort unlesbar: {e}"))
        }
        Err(ureq::Error::Status(code, r)) => {
            let class = classify_status(code, r.header("x-ratelimit-remaining"));
            // `repo.auth` prefix so `retry_without_token` retries a stale token.
            let class = if class == "github.auth" { "repo.auth" } else { class };
            Err(format!("{class}: HTTP {code}"))
        }
        Err(e) => Err(format!("github.network: {e}")),
    }
}

/// Pages until a short page, MAX_PAGES, or `stop` says the rest is too old.
fn get_pages(owner: &str, repo: &str, path: &str, token: Option<&str>, stop: impl Fn(&[Value]) -> bool) -> Result<Vec<Vec<Value>>, String> {
    let sep = if path.contains('?') { '&' } else { '?' };
    let mut pages = Vec::new();
    for page in 1..=MAX_PAGES {
        let items = get_page(&api_url(owner, repo, &format!("{path}{sep}per_page={PAGE}&page={page}")), token)?;
        let done = items.len() < PAGE || stop(&items);
        pages.push(items);
        if done { break; }
    }
    Ok(pages)
}

pub fn fetch_activity(owner: &str, repo: &str, token: Option<&str>, now: i64) -> Result<GithubActivity, String> {
    let horizon = now - 2 * WEEK;
    let too_old = |field: &'static str| move |items: &[Value]| items.iter().filter_map(|v| ts(&v[field])).min().is_some_and(|t| t <= horizon);
    let since = chrono::DateTime::from_timestamp(horizon, 0).map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default();
    crate::repo_clone::retry_without_token(token, |t| {
        let events = get_pages(owner, repo, "events", t, too_old("created_at"))?;
        let pulls = get_pages(owner, repo, "pulls?state=all&sort=updated&direction=desc", t, too_old("updated_at"))?;
        let issues = get_pages(owner, repo, &format!("issues?state=all&since={since}"), t, |_| false)?;
        let e = count_events(&events, now);
        let p = count_pulls(&pulls.concat(), now);
        let i = count_issues(&issues.concat(), now);
        let mk = |pushes: u64, sel: fn(&Win) -> u64| GithubCounts {
            pushes,
            prs_opened: sel(&p.opened),
            prs_merged: sel(&p.merged),
            issues_opened: sel(&i.opened),
            issues_closed: sel(&i.closed),
        };
        Ok(GithubActivity {
            day: mk(e.day, |w| w.day),
            day_prev: mk(e.day_prev, |w| w.day_prev),
            week: mk(e.week, |w| w.week),
            week_prev: mk(e.week_prev, |w| w.week_prev),
            pushes_capped: e.capped,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_790_000_000;

    fn iso(t: i64) -> String {
        chrono::DateTime::from_timestamp(t, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }

    #[test]
    fn events_count_pushes_per_window() {
        let page = vec![
            json!({"type":"PushEvent","created_at": iso(NOW - 10)}),
            json!({"type":"PushEvent","created_at": iso(NOW - DAY - 10)}),
            json!({"type":"ReleaseEvent","created_at": iso(NOW - 20)}),
            json!({"type":"PushEvent","created_at": iso(NOW - 8 * DAY)}),
        ];
        let e = count_events(&[page], NOW);
        assert_eq!((e.day, e.day_prev, e.week, e.week_prev, e.capped), (1, 1, 2, 1, false));
    }

    #[test]
    fn full_pages_inside_the_horizon_mark_the_count_capped() {
        let page: Vec<Value> = (0..100).map(|i| json!({"type":"PushEvent","created_at": iso(NOW - 100 - i)})).collect();
        let e = count_events(&[page.clone(), page.clone(), page], NOW);
        assert!(e.capped, "300 events all within 14 days → incomplete");
        let short = vec![json!({"type":"PushEvent","created_at": iso(NOW - 5)})];
        assert!(!count_events(&[short], NOW).capped);
    }

    #[test]
    fn a_short_last_page_means_everything_was_fetched() {
        let full: Vec<Value> = (0..100).map(|i| json!({"type":"PushEvent","created_at": iso(NOW - 100 - i)})).collect();
        let short: Vec<Value> = (0..50).map(|i| json!({"type":"PushEvent","created_at": iso(NOW - 500 - i)})).collect();
        assert!(!count_events(&[full.clone(), full, short], NOW).capped, "250 events < 300 → complete");
    }

    #[test]
    fn the_token_goes_into_the_authorization_header_only() {
        let h = request_headers(Some("ghp_X"));
        assert!(h.contains(&("Authorization", "Bearer ghp_X".to_string())));
        assert!(h.iter().any(|(k, v)| *k == "User-Agent" && v == "inspector-rust"));
        assert!(!request_headers(None).iter().any(|(k, _)| *k == "Authorization"));
        assert!(!request_headers(Some("  ")).iter().any(|(k, _)| *k == "Authorization"));
    }

    #[test]
    fn old_pr_merged_yesterday() {
        let pulls = vec![
            json!({"created_at": iso(NOW - 60 * DAY), "merged_at": iso(NOW - 3600), "updated_at": iso(NOW - 3600)}),
            json!({"created_at": iso(NOW - 2 * 3600), "merged_at": null, "updated_at": iso(NOW - 2 * 3600)}),
        ];
        let p = count_pulls(&pulls, NOW);
        assert_eq!((p.opened.day, p.merged.day), (1, 1));
        assert_eq!(p.opened.week, 1);
    }

    #[test]
    fn pull_requests_are_not_issues() {
        let issues = vec![
            json!({"created_at": iso(NOW - 100), "closed_at": null}),
            json!({"created_at": iso(NOW - 100), "closed_at": null, "pull_request": {"url": "x"}}),
            json!({"created_at": iso(NOW - 20 * DAY), "closed_at": iso(NOW - 3 * DAY)}),
        ];
        let i = count_issues(&issues, NOW);
        assert_eq!((i.opened.day, i.opened.week, i.closed.week, i.closed.day), (1, 1, 1, 0));
    }

    #[test]
    fn the_token_never_goes_into_the_url() {
        let u = api_url("o", "r", "events?per_page=100&page=2");
        assert_eq!(u, "https://api.github.com/repos/o/r/events?per_page=100&page=2");
        assert!(!u.contains("token") && !u.contains("access_token"));
    }

    #[test]
    fn classify_http_status_names_rate_limit_and_not_found() {
        assert_eq!(classify_status(403, Some("0")), "github.rate_limit");
        assert_eq!(classify_status(429, None), "github.rate_limit");
        assert_eq!(classify_status(404, None), "github.not_found");
        assert_eq!(classify_status(401, None), "github.auth");
        assert_eq!(classify_status(403, Some("4000")), "github.auth");
        assert_eq!(classify_status(500, None), "github.http");
    }

    #[test]
    fn recorded_events_count_like_jq() {
        let v: Vec<Value> = serde_json::from_str(include_str!("testdata/github-events.json")).unwrap();
        let newest = v.iter().filter_map(|e| ts(&e["created_at"])).max().unwrap();
        // Everything inside one huge window: the parser must see exactly the
        // PushEvents jq counted when the fixture was recorded.
        let pushes = v.iter().filter(|e| e["type"] == "PushEvent").count() as u64;
        let e = count_events(std::slice::from_ref(&v), newest + 1);
        assert_eq!(e.week + e.week_prev + older_than_two_weeks(&v, newest + 1), pushes);
    }

    fn older_than_two_weeks(v: &[Value], now: i64) -> u64 {
        v.iter()
            .filter(|e| e["type"] == "PushEvent" && ts(&e["created_at"]).is_some_and(|t| t <= now - 2 * WEEK))
            .count() as u64
    }

    /// `IR_LIVE_GH=owner/repo cargo test -p inspector-rust-core --lib live_github_activity -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_github_activity() {
        let Ok(or) = std::env::var("IR_LIVE_GH") else { return };
        let (o, r) = or.split_once('/').unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let a = fetch_activity(o, r, crate::repo_clone::github_token().as_deref(), now).unwrap();
        eprintln!("{a:#?}");
    }
}
