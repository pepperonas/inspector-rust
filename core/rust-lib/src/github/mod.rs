//! GitHub analytics: account + repository statistics of the signed-in user,
//! kept as local snapshots so growth can be shown over time.
//!
//! Layout (reference: docs/github.md):
//! * this file — the pure core: API types and parsing, pagination, the
//!   snapshot policy, deltas, sorting and the device-flow poll states.
//!   Deterministic, fully unit-tested, no network.
//! * `client` — HTTP behind the `Transport` trait (ureq in the app, a fake
//!   in tests): account, repositories (paginated), device flow.
//! * `store` — tables in `history.db`; repositories keyed by GitHub's stable
//!   id, so a rename never starts a new history.
//! * `export` — JSON / CSV (private repositories only on request).
//! * `service` — tokens (keychain), refresh with interval + rate-limit
//!   gating, the background refresher and what the UI shows.

pub mod client;
pub mod export;
pub mod service;
pub mod store;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── The rules ───────────────────────────────────────────────────────────────

/// Default minimum time between two automatic refreshes (and so between two
/// regular snapshots). Configurable in Settings → GitHub.
pub const DEFAULT_INTERVAL_MS: i64 = 3_600_000;
/// Allowed refresh intervals (1 h … 24 h).
pub const INTERVAL_CHOICES_H: [i64; 4] = [1, 3, 6, 24];
/// A forced refresh (`github refresh`) still never hits GitHub more often
/// than this.
pub const MIN_FORCED_GAP_MS: i64 = 60_000;
/// Pages of 100 repositories fetched at most (3 000 repos).
pub const MAX_REPO_PAGES: usize = 30;
/// Repositories in the multi-line growth chart.
pub const GROWTH_CHART_TOP: usize = 5;

/// Delta windows shown on the dashboard.
pub const WINDOWS: [(&str, i64); 5] = [
    ("24h", 86_400_000),
    ("7d", 7 * 86_400_000),
    ("30d", 30 * 86_400_000),
    ("90d", 90 * 86_400_000),
    ("1y", 365 * 86_400_000),
];

// ── API types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Account {
    pub id: i64,
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub html_url: Option<String>,
    pub followers: i64,
    pub following: i64,
    /// Public repositories only (GitHub's `public_repos`).
    pub public_repos: i64,
    /// `total_private_repos` — 0 when the token may not see private ones.
    #[serde(default)]
    pub private_repos: i64,
    #[serde(default)]
    pub public_gists: i64,
    /// When the account was created on GitHub (its age).
    #[serde(default)]
    pub created_at_ms: Option<i64>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub company: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub blog: Option<String>,
    #[serde(default)]
    pub hireable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Repo {
    /// GitHub's stable id — the identity, not the name.
    pub id: i64,
    pub name: String,
    pub full_name: String,
    pub private: bool,
    pub archived: bool,
    pub fork: bool,
    pub language: Option<String>,
    pub html_url: String,
    pub description: Option<String>,
    pub license: Option<String>,
    pub stars: i64,
    pub forks: i64,
    /// Subscribers ("watchers" in the GitHub UI). Only the single-repository
    /// endpoint carries it — None from the list.
    pub watchers: Option<i64>,
    pub open_issues: i64,
    pub updated_at_ms: Option<i64>,
    pub pushed_at_ms: Option<i64>,
    /// When the repository was created — its age and the start of its history.
    #[serde(default)]
    pub created_at_ms: Option<i64>,
    /// GitHub's `size` (KB).
    #[serde(default)]
    pub size_kb: Option<i64>,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub default_branch: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimit {
    pub limit: i64,
    pub remaining: i64,
    pub reset_at_ms: i64,
}

fn iso_ms(v: &Value) -> Option<i64> {
    v.as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis())
}

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string).filter(|s| !s.is_empty())
}

/// `GET /user`. None when the answer lacks the identity.
pub fn parse_account(v: &Value) -> Option<Account> {
    Some(Account {
        id: v.get("id")?.as_i64()?,
        login: v.get("login")?.as_str()?.to_string(),
        name: opt_str(&v["name"]),
        avatar_url: opt_str(&v["avatar_url"]),
        html_url: opt_str(&v["html_url"]),
        followers: v["followers"].as_i64().unwrap_or(0),
        following: v["following"].as_i64().unwrap_or(0),
        public_repos: v["public_repos"].as_i64().unwrap_or(0),
        private_repos: v["total_private_repos"].as_i64().unwrap_or(0),
        public_gists: v["public_gists"].as_i64().unwrap_or(0),
        created_at_ms: iso_ms(&v["created_at"]),
        bio: opt_str(&v["bio"]).map(|b| b.trim().to_string()),
        company: opt_str(&v["company"]),
        location: opt_str(&v["location"]),
        blog: opt_str(&v["blog"]),
        hireable: v["hireable"].as_bool(),
    })
}

/// One repository object (list or single endpoint). Entries without an id
/// or name are dropped — incomplete data never becomes a row.
pub fn parse_repo(v: &Value) -> Option<Repo> {
    Some(Repo {
        id: v.get("id")?.as_i64()?,
        name: v.get("name")?.as_str()?.to_string(),
        full_name: v["full_name"].as_str().unwrap_or_default().to_string(),
        private: v["private"].as_bool().unwrap_or(false),
        archived: v["archived"].as_bool().unwrap_or(false),
        fork: v["fork"].as_bool().unwrap_or(false),
        language: opt_str(&v["language"]),
        html_url: v["html_url"].as_str().unwrap_or_default().to_string(),
        description: opt_str(&v["description"]),
        license: opt_str(&v["license"]["spdx_id"]).filter(|s| s != "NOASSERTION"),
        stars: v["stargazers_count"].as_i64().unwrap_or(0),
        forks: v["forks_count"].as_i64().unwrap_or(0),
        watchers: v["subscribers_count"].as_i64(),
        open_issues: v["open_issues_count"].as_i64().unwrap_or(0),
        updated_at_ms: iso_ms(&v["updated_at"]),
        pushed_at_ms: iso_ms(&v["pushed_at"]),
        created_at_ms: iso_ms(&v["created_at"]),
        size_kb: v["size"].as_i64(),
        topics: v["topics"]
            .as_array()
            .map(|a| a.iter().filter_map(opt_str).collect())
            .unwrap_or_default(),
        homepage: opt_str(&v["homepage"]),
        default_branch: opt_str(&v["default_branch"]),
    })
}

pub fn parse_repos(v: &Value) -> Vec<Repo> {
    v.as_array()
        .map(|a| a.iter().filter_map(parse_repo).collect())
        .unwrap_or_default()
}

/// The `next` URL of a `Link` header, if any.
pub fn next_link(link: &str) -> Option<String> {
    link.split(',').find_map(|part| {
        let (url, rel) = part.split_once(';')?;
        rel.split(';').any(|p| p.trim() == "rel=\"next\"").then(|| {
            url.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string()
        })
    })
}

/// Rate limit from the `x-ratelimit-*` headers (lowercase lookups).
pub fn rate_from_headers(get: impl Fn(&str) -> Option<String>) -> Option<RateLimit> {
    let num = |k: &str| get(k).and_then(|v| v.trim().parse::<i64>().ok());
    Some(RateLimit {
        limit: num("x-ratelimit-limit")?,
        remaining: num("x-ratelimit-remaining")?,
        reset_at_ms: num("x-ratelimit-reset")? * 1000,
    })
}

/// Error sentinels the UI turns into words. Tokens never appear in them.
pub const ERR_NOT_CONNECTED: &str = "github.not_connected";
pub const ERR_UNAUTHORIZED: &str = "github.unauthorized";
pub const ERR_FORBIDDEN: &str = "github.forbidden";
pub const ERR_RATE_LIMITED: &str = "github.rate_limited";
pub const ERR_NETWORK: &str = "github.network";
pub const ERR_NOT_FOUND: &str = "github.not_found";
pub const ERR_BAD_DATA: &str = "github.bad_data";

/// What a non-2xx answer means. A 403 with no requests left (or a 429) is
/// the rate limit, any other 403 a missing permission.
pub fn classify_status(status: u16, remaining: Option<i64>) -> &'static str {
    match status {
        401 => ERR_UNAUTHORIZED,
        403 if remaining == Some(0) => ERR_RATE_LIMITED,
        429 => ERR_RATE_LIMITED,
        403 => ERR_FORBIDDEN,
        404 => ERR_NOT_FOUND,
        _ => ERR_NETWORK,
    }
}

// ── Device flow ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceCode {
    #[serde(skip_serializing)]
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in_s: i64,
    pub interval_s: i64,
}

pub fn parse_device_code(v: &Value) -> Option<DeviceCode> {
    Some(DeviceCode {
        device_code: v.get("device_code")?.as_str()?.to_string(),
        user_code: v.get("user_code")?.as_str()?.to_string(),
        verification_uri: v.get("verification_uri")?.as_str()?.to_string(),
        expires_in_s: v["expires_in"].as_i64().unwrap_or(900),
        // GitHub's minimum; never poll faster.
        interval_s: v["interval"].as_i64().unwrap_or(5).max(5),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum PollResult {
    Token {
        token: String,
        scope: String,
    },
    Pending,
    /// Poll slower: the new interval in seconds.
    SlowDown(i64),
    Expired,
    Denied,
    Failed(String),
}

/// One answer of `POST /login/oauth/access_token` during the device flow.
pub fn parse_poll(v: &Value, current_interval_s: i64) -> PollResult {
    if let Some(t) = v["access_token"].as_str().filter(|t| !t.is_empty()) {
        return PollResult::Token {
            token: t.to_string(),
            scope: v["scope"].as_str().unwrap_or_default().to_string(),
        };
    }
    match v["error"].as_str().unwrap_or("") {
        "authorization_pending" => PollResult::Pending,
        // GitHub says how long to wait; else +5 s as the spec requires.
        "slow_down" => {
            PollResult::SlowDown(v["interval"].as_i64().unwrap_or(current_interval_s + 5))
        }
        "expired_token" | "token_expired" => PollResult::Expired,
        "access_denied" => PollResult::Denied,
        other => PollResult::Failed(
            v["error_description"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| format!("GitHub: {other}")),
        ),
    }
}

// ── Snapshots ───────────────────────────────────────────────────────────────

/// The account numbers one snapshot keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AccountTotals {
    pub followers: i64,
    pub following: i64,
    pub public_repos: i64,
    pub total_stars: i64,
    pub total_forks: i64,
    pub repo_count: i64,
}

/// Totals over the account's own repositories (forks included — their stars
/// are the user's too; archived ones still count).
pub fn totals(account: &Account, repos: &[Repo]) -> AccountTotals {
    AccountTotals {
        followers: account.followers,
        following: account.following,
        public_repos: account.public_repos,
        total_stars: repos.iter().map(|r| r.stars).sum(),
        total_forks: repos.iter().map(|r| r.forks).sum(),
        repo_count: repos.len() as i64,
    }
}

/// The snapshot policy, in one place: a regular snapshot once per interval;
/// a refresh inside the interval (forced) only stores one when something
/// changed. Never two with identical numbers in the same interval.
pub fn should_snapshot(
    last: Option<(i64, AccountTotals)>,
    current: &AccountTotals,
    now: i64,
    interval_ms: i64,
) -> bool {
    match last {
        None => true,
        Some((ts, prev)) => now - ts >= interval_ms || prev != *current,
    }
}

/// The repository numbers kept per snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoNumbers {
    pub stars: i64,
    pub forks: i64,
    pub watchers: Option<i64>,
    pub open_issues: i64,
}

impl RepoNumbers {
    pub fn of(r: &Repo) -> Self {
        Self {
            stars: r.stars,
            forks: r.forks,
            watchers: r.watchers,
            open_issues: r.open_issues,
        }
    }

    /// Changed in what a repository snapshot stores. A missing watchers count
    /// (list endpoint) is not a change.
    pub fn differs(&self, other: &RepoNumbers) -> bool {
        self.stars != other.stars
            || self.forks != other.forks
            || self.open_issues != other.open_issues
            || (self.watchers.is_some()
                && other.watchers.is_some()
                && self.watchers != other.watchers)
    }
}

/// A point of a stored series: time (ms) and value.
pub type Point = (i64, i64);

/// The value at `t`: the newest point at or before it. None before the
/// first point — there was no observation yet.
pub fn value_at(series: &[Point], t: i64) -> Option<i64> {
    series
        .iter()
        .rev()
        .find(|(ts, _)| *ts <= t)
        .map(|(_, v)| *v)
}

/// Change over `window` up to `now`, from REAL points only. None when the
/// history does not reach back that far — never a fake zero.
pub fn delta(series: &[Point], now: i64, window: i64) -> Option<i64> {
    let current = value_at(series, now)?;
    let base = value_at(series, now - window)?;
    Some(current - base)
}

// ── Reconstructed history ───────────────────────────────────────────────────
//
// Snapshots only start the day the account was connected. GitHub keeps two
// timestamped lists that reach back to the start: when each CURRENT
// stargazer starred (`starred_at`) and when each fork was created. Turned
// into a cumulative count they give the history before the first snapshot.
// What they can't show: stars that were removed again — the reconstruction
// counts today's stargazers only, and the UI says so.

/// Which timestamped list an event series comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Stars,
    Forks,
}

impl EventKind {
    pub const ALL: [EventKind; 2] = [EventKind::Stars, EventKind::Forks];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stars => "stars",
            Self::Forks => "forks",
        }
    }

    /// The timestamp field of one list entry.
    pub fn time_field(self) -> &'static str {
        match self {
            Self::Stars => "starred_at",
            Self::Forks => "created_at",
        }
    }

    /// The count on the repository this list belongs to.
    pub fn value_of(self, r: &Repo) -> i64 {
        match self {
            Self::Stars => r.stars,
            Self::Forks => r.forks,
        }
    }
}

/// Entries per page of the event lists.
pub const EVENT_PAGE_SIZE: i64 = 100;
/// Pages read at most for ONE repository (GitHub stops listing there too).
pub const MAX_EVENT_PAGES: i64 = 400;
/// Pages read at most per refresh, over all repositories (3 % of the
/// hourly limit; most lists are one page, so an account with 100 small
/// repositories is reconstructed in one or two refreshes).
pub const BACKFILL_PAGE_BUDGET: i64 = 150;
/// Below this many requests left, nothing is backfilled — the regular
/// refresh must never run into the wall because of the history.
pub const BACKFILL_MIN_RATE: i64 = 500;
/// A changed repository is re-read at most this often (a busy repository
/// would otherwise be re-read on every refresh).
pub const BACKFILL_RESYNC_MS: i64 = 6 * 3_600_000;

/// The timestamps of one page; None when the answer is not a list.
pub fn parse_event_times(v: &Value, kind: EventKind) -> Option<Vec<i64>> {
    Some(
        v.as_array()?
            .iter()
            .filter_map(|e| iso_ms(&e[kind.time_field()]))
            .collect(),
    )
}

/// Pages one full read of a list of `value` entries takes.
pub fn pages_for(value: i64) -> i64 {
    ((value.max(1) + EVENT_PAGE_SIZE - 1) / EVENT_PAGE_SIZE).min(MAX_EVENT_PAGES)
}

/// One list that may need (re-)reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackfillCandidate {
    pub repo_id: i64,
    pub full_name: String,
    pub kind: EventKind,
    /// The repository's current count (stars / forks).
    pub value: i64,
    /// The count when the list was last read completely; None = never.
    pub synced_value: Option<i64>,
    pub synced_ms: Option<i64>,
}

impl BackfillCandidate {
    /// Due: never read, or the count changed and the last read is old enough.
    pub fn due(&self, now: i64) -> bool {
        if self.value <= 0 {
            return false;
        }
        match (self.synced_value, self.synced_ms) {
            (None, _) | (_, None) => true,
            (Some(v), Some(t)) => v != self.value && now - t >= BACKFILL_RESYNC_MS,
        }
    }
}

/// Which lists this refresh reads, within `budget` pages. Never-read lists
/// first, the biggest first (they matter most); a list is only taken when it
/// fits completely — a half-read list is never stored. The one exception: a
/// list bigger than the whole budget may take a refresh of its own, or it
/// would never be read.
pub fn plan_backfill(
    mut cands: Vec<BackfillCandidate>,
    now: i64,
    budget: i64,
) -> Vec<BackfillCandidate> {
    cands.retain(|c| c.due(now));
    cands.sort_by(|a, b| {
        a.synced_ms
            .is_some()
            .cmp(&b.synced_ms.is_some())
            .then(b.value.cmp(&a.value))
            .then(a.repo_id.cmp(&b.repo_id))
            .then(a.kind.as_str().cmp(b.kind.as_str()))
    });
    let mut left = budget;
    let mut out: Vec<BackfillCandidate> = Vec::new();
    for c in cands {
        let need = pages_for(c.value);
        if need <= left {
            left -= need;
            out.push(c);
        } else if out.is_empty() {
            out.push(c);
            break;
        }
    }
    out
}

/// Event times → a cumulative count, oldest first (one point per event).
pub fn cumulative(events: &[i64]) -> Vec<Point> {
    let mut ts = events.to_vec();
    ts.sort_unstable();
    ts.into_iter()
        .enumerate()
        .map(|(i, t)| (t, i as i64 + 1))
        .collect()
}

/// The reconstruction before the first real snapshot, the snapshots from
/// then on. Snapshots win where both exist: they also see removed stars.
pub fn merge_history(reconstructed: &[Point], snapshots: &[Point]) -> Vec<Point> {
    match snapshots.first() {
        None => reconstructed.to_vec(),
        Some(&(first, _)) => reconstructed
            .iter()
            .copied()
            .filter(|(t, _)| *t < first)
            .chain(snapshots.iter().copied())
            .collect(),
    }
}

/// A series for a step chart over `[now - range, now]`: the value carried
/// in at the start, every change inside, and the current value at `now` —
/// a value holds until it changes, so a quiet repository still has a line.
/// Empty when nothing is known by `now`.
pub fn window(series: &[Point], now: i64, range: Option<i64>) -> Vec<Point> {
    let start = range.map(|r| now - r);
    let mut out: Vec<Point> = Vec::new();
    if let Some(s) = start {
        if let Some(v) = value_at(series, s) {
            out.push((s, v));
        }
    }
    for &(t, v) in series {
        if t > now {
            break;
        }
        if start.is_some_and(|s| t <= s) {
            continue;
        }
        out.push((t, v));
    }
    if let Some(&(t, v)) = out.last() {
        if t < now {
            out.push((now, v));
        }
    }
    out
}

/// Gain per calendar month (UTC), the last `months` up to `now`, oldest
/// first. `zero_before` = the series is a full reconstruction, so before its
/// first point the count was 0; otherwise an unknown month is None.
pub fn gains_by_month(
    series: &[Point],
    now: i64,
    months: u32,
    zero_before: bool,
) -> Vec<(i64, Option<i64>)> {
    use chrono::{Datelike, TimeZone, Utc};
    let Some(n) = Utc.timestamp_millis_opt(now).single() else {
        return Vec::new();
    };
    let month_start = |y: i32, m: u32| {
        Utc.with_ymd_and_hms(y, m, 1, 0, 0, 0)
            .single()
            .map(|d| d.timestamp_millis())
    };
    let mut starts = Vec::new();
    let (mut y, mut m) = (n.year(), n.month());
    for _ in 0..months {
        if let Some(s) = month_start(y, m) {
            starts.push(s);
        }
        if m == 1 {
            y -= 1;
            m = 12;
        } else {
            m -= 1;
        }
    }
    starts.reverse();
    let at = |t: i64| {
        value_at(series, t).or(if zero_before && !series.is_empty() {
            Some(0)
        } else {
            None
        })
    };
    starts
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let end = starts.get(i + 1).copied().unwrap_or(now);
            (s, at(end).zip(at(s)).map(|(e, b)| e - b))
        })
        .collect()
}

// ── Repository extras (detail view, cached) ─────────────────────────────────

/// Extra data the repository detail fetches on demand (cached for a day).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtraKind {
    CommitActivity,
    Releases,
    Languages,
    TrafficViews,
    TrafficClones,
}

impl ExtraKind {
    pub const ALL: [ExtraKind; 5] = [
        ExtraKind::CommitActivity,
        ExtraKind::Releases,
        ExtraKind::Languages,
        ExtraKind::TrafficViews,
        ExtraKind::TrafficClones,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CommitActivity => "commit_activity",
            Self::Releases => "releases",
            Self::Languages => "languages",
            Self::TrafficViews => "traffic_views",
            Self::TrafficClones => "traffic_clones",
        }
    }

    pub fn path(self) -> &'static str {
        match self {
            Self::CommitActivity => "stats/commit_activity",
            Self::Releases => "releases?per_page=100",
            Self::Languages => "languages",
            Self::TrafficViews => "traffic/views",
            Self::TrafficClones => "traffic/clones",
        }
    }
}

/// How long a fetched extra is reused.
pub const EXTRAS_TTL_MS: i64 = 86_400_000;

/// `stats/commit_activity`: commits per week, last 52 weeks (week start ms).
pub fn parse_commit_activity(v: &Value) -> Option<Vec<Point>> {
    Some(
        v.as_array()?
            .iter()
            .filter_map(|w| Some((w["week"].as_i64()? * 1000, w["total"].as_i64()?)))
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReleaseSummary {
    /// Releases on the first page (up to 100).
    pub count: i64,
    pub latest_tag: Option<String>,
    pub latest_ms: Option<i64>,
    /// Asset downloads over those releases.
    pub downloads: i64,
    /// Release dates, newest first — the cadence.
    pub dates: Vec<i64>,
}

pub fn parse_releases(v: &Value) -> Option<ReleaseSummary> {
    let list = v.as_array()?;
    let mut s = ReleaseSummary::default();
    for r in list {
        if r["draft"].as_bool() == Some(true) {
            continue;
        }
        s.count += 1;
        let when = iso_ms(&r["published_at"]).or_else(|| iso_ms(&r["created_at"]));
        if let Some(t) = when {
            s.dates.push(t);
            if s.latest_ms.is_none_or(|l| t > l) {
                s.latest_ms = Some(t);
                s.latest_tag = opt_str(&r["tag_name"]);
            }
        }
        s.downloads += r["assets"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x["download_count"].as_i64()).sum::<i64>())
            .unwrap_or(0);
    }
    s.dates.sort_unstable_by(|a, b| b.cmp(a));
    Some(s)
}

/// `languages`: bytes per language, largest first.
pub fn parse_languages(v: &Value) -> Option<Vec<(String, i64)>> {
    let mut out: Vec<(String, i64)> = v
        .as_object()?
        .iter()
        .filter_map(|(k, b)| Some((k.clone(), b.as_i64()?)))
        .filter(|(_, b)| *b > 0)
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Some(out)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Traffic {
    pub count: i64,
    pub uniques: i64,
    /// Per day: (day ms, count).
    pub days: Vec<Point>,
}

/// `traffic/views` or `traffic/clones` (last 14 days).
pub fn parse_traffic(v: &Value, list: &str) -> Option<Traffic> {
    Some(Traffic {
        count: v["count"].as_i64()?,
        uniques: v["uniques"].as_i64().unwrap_or(0),
        days: v[list]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|d| Some((iso_ms(&d["timestamp"])?, d["count"].as_i64()?)))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

// ── Refresh gating ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Gate {
    Go,
    /// Data is fresh enough; the next refresh is due at this time.
    TooSoon {
        next_at_ms: i64,
    },
    /// No requests left; GitHub resets the limit at this time.
    RateLimited {
        reset_at_ms: i64,
    },
}

/// Whether a refresh may hit GitHub now. Automatic refreshes wait for the
/// interval; a forced one (`github refresh`) only for `MIN_FORCED_GAP_MS`.
/// An exhausted rate limit blocks both until its reset — no polling into a
/// wall.
pub fn refresh_gate(
    now: i64,
    last_checked: Option<i64>,
    interval_ms: i64,
    force: bool,
    rate: Option<RateLimit>,
) -> Gate {
    if let Some(r) = rate {
        if r.remaining <= 0 && now < r.reset_at_ms {
            return Gate::RateLimited {
                reset_at_ms: r.reset_at_ms,
            };
        }
    }
    let Some(last) = last_checked else {
        return Gate::Go;
    };
    let gap = if force {
        MIN_FORCED_GAP_MS
    } else {
        interval_ms
    };
    if now - last < gap {
        Gate::TooSoon {
            next_at_ms: last + gap,
        }
    } else {
        Gate::Go
    }
}

// ── Sorting ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    Stars,
    Forks,
    Updated,
    Name,
    Growth7d,
    Growth30d,
}

/// A ranking row (current numbers + growth from the history).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepoRow {
    #[serde(flatten)]
    pub repo: Repo,
    pub growth_7d: Option<i64>,
    pub growth_30d: Option<i64>,
}

/// Sort for the ranking. Descending for numbers and dates, ascending for the
/// name. Unknown growth ("insufficient history") goes last, never as 0.
/// Ties fall back to stars, then name — a stable, predictable order.
pub fn sort_rows(rows: &mut [RepoRow], key: SortKey) {
    use std::cmp::Ordering;
    let by_growth = |a: Option<i64>, b: Option<i64>| match (a, b) {
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    rows.sort_by(|a, b| {
        let primary = match key {
            SortKey::Stars => b.repo.stars.cmp(&a.repo.stars),
            SortKey::Forks => b.repo.forks.cmp(&a.repo.forks),
            SortKey::Updated => b
                .repo
                .pushed_at_ms
                .or(b.repo.updated_at_ms)
                .cmp(&a.repo.pushed_at_ms.or(a.repo.updated_at_ms)),
            SortKey::Name => a.repo.name.to_lowercase().cmp(&b.repo.name.to_lowercase()),
            SortKey::Growth7d => by_growth(a.growth_7d, b.growth_7d),
            SortKey::Growth30d => by_growth(a.growth_30d, b.growth_30d),
        };
        primary
            .then_with(|| b.repo.stars.cmp(&a.repo.stars))
            .then_with(|| a.repo.name.to_lowercase().cmp(&b.repo.name.to_lowercase()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub fn repo(id: i64, name: &str, stars: i64) -> Repo {
        Repo {
            id,
            name: name.into(),
            full_name: format!("pepperonas/{name}"),
            private: false,
            archived: false,
            fork: false,
            language: Some("Rust".into()),
            html_url: format!("https://github.com/pepperonas/{name}"),
            description: None,
            license: None,
            stars,
            forks: stars / 10,
            watchers: None,
            open_issues: 0,
            updated_at_ms: Some(id * 1000),
            pushed_at_ms: None,
            ..Default::default()
        }
    }

    fn row(r: Repo, g7: Option<i64>, g30: Option<i64>) -> RepoRow {
        RepoRow {
            repo: r,
            growth_7d: g7,
            growth_30d: g30,
        }
    }

    #[test]
    fn account_and_repos_parse_and_drop_incomplete_entries() {
        let a = parse_account(
            &json!({"id": 7, "login": "pepperonas", "name": "Martin", "followers": 142,
            "following": 3, "public_repos": 47, "total_private_repos": 5}),
        )
        .unwrap();
        assert_eq!(
            (a.id, a.login.as_str(), a.followers, a.public_repos, a.private_repos),
            (7, "pepperonas", 142, 47, 5),
            "public stays public — private repositories are counted apart"
        );
        assert!(parse_account(&json!({"login": "x"})).is_none());
        let repos = parse_repos(&json!([
            {"id": 1, "name": "inspector-rust", "full_name": "pepperonas/inspector-rust", "stargazers_count": 128,
             "forks_count": 17, "open_issues_count": 3, "language": "Rust", "archived": true, "private": false,
             "license": {"spdx_id": "MIT"}, "pushed_at": "2026-10-10T12:00:00Z"},
            {"name": "no-id"},
            {"id": 2, "name": "x", "license": {"spdx_id": "NOASSERTION"}}
        ]));
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].stars, 128);
        assert!(repos[0].archived);
        assert_eq!(repos[0].license.as_deref(), Some("MIT"));
        assert_eq!(repos[0].pushed_at_ms, Some(1_791_633_600_000));
        assert_eq!(
            repos[0].watchers, None,
            "the list endpoint has no subscribers"
        );
        assert_eq!(repos[1].license, None);
        assert!(parse_repos(&json!({"message": "Bad credentials"})).is_empty());
    }

    #[test]
    fn link_header_pagination() {
        let link = r#"<https://api.github.com/user/repos?page=2>; rel="next", <https://api.github.com/user/repos?page=5>; rel="last""#;
        assert_eq!(
            next_link(link).as_deref(),
            Some("https://api.github.com/user/repos?page=2")
        );
        let last = r#"<https://api.github.com/user/repos?page=4>; rel="prev", <https://api.github.com/user/repos?page=1>; rel="first""#;
        assert_eq!(next_link(last), None);
        assert_eq!(next_link(""), None);
    }

    #[test]
    fn rate_limit_headers_and_status_classes() {
        let h = |k: &str| match k {
            "x-ratelimit-limit" => Some("5000".to_string()),
            "x-ratelimit-remaining" => Some("4821".to_string()),
            "x-ratelimit-reset" => Some("1791680000".to_string()),
            _ => None,
        };
        assert_eq!(
            rate_from_headers(h),
            Some(RateLimit {
                limit: 5000,
                remaining: 4821,
                reset_at_ms: 1_791_680_000_000
            })
        );
        assert_eq!(rate_from_headers(|_| None), None);
        assert_eq!(classify_status(401, None), ERR_UNAUTHORIZED);
        assert_eq!(classify_status(403, Some(0)), ERR_RATE_LIMITED);
        assert_eq!(classify_status(403, Some(12)), ERR_FORBIDDEN);
        assert_eq!(classify_status(429, None), ERR_RATE_LIMITED);
        assert_eq!(classify_status(404, None), ERR_NOT_FOUND);
        assert_eq!(classify_status(502, None), ERR_NETWORK);
    }

    #[test]
    fn device_flow_answers() {
        let d = parse_device_code(&json!({"device_code": "dc", "user_code": "WDJB-MJHT",
            "verification_uri": "https://github.com/login/device", "expires_in": 899, "interval": 1})).unwrap();
        assert_eq!(d.user_code, "WDJB-MJHT");
        assert_eq!(d.interval_s, 5, "never faster than GitHub's minimum");
        assert!(
            !serde_json::to_string(&d).unwrap().contains("dc"),
            "the device code never goes to the UI"
        );
        assert_eq!(
            parse_poll(&json!({"error": "authorization_pending"}), 5),
            PollResult::Pending
        );
        assert_eq!(
            parse_poll(&json!({"error": "slow_down", "interval": 10}), 5),
            PollResult::SlowDown(10)
        );
        assert_eq!(
            parse_poll(&json!({"error": "slow_down"}), 5),
            PollResult::SlowDown(10)
        );
        assert_eq!(
            parse_poll(&json!({"error": "expired_token"}), 5),
            PollResult::Expired
        );
        assert_eq!(
            parse_poll(&json!({"error": "access_denied"}), 5),
            PollResult::Denied
        );
        assert_eq!(
            parse_poll(&json!({"access_token": "gho_x", "scope": ""}), 5),
            PollResult::Token {
                token: "gho_x".into(),
                scope: "".into()
            }
        );
        assert!(matches!(
            parse_poll(&json!({"error": "incorrect_client_credentials"}), 5),
            PollResult::Failed(_)
        ));
    }

    #[test]
    fn snapshot_policy_regular_and_on_change() {
        let t = AccountTotals {
            followers: 142,
            total_stars: 384,
            ..Default::default()
        };
        assert!(should_snapshot(None, &t, 0, DEFAULT_INTERVAL_MS));
        // Same numbers within the interval: no duplicate.
        assert!(!should_snapshot(
            Some((0, t)),
            &t,
            10 * 60_000,
            DEFAULT_INTERVAL_MS
        ));
        // Changed within the interval: yes.
        let changed = AccountTotals {
            followers: 143,
            ..t
        };
        assert!(should_snapshot(
            Some((0, t)),
            &changed,
            10 * 60_000,
            DEFAULT_INTERVAL_MS
        ));
        // Interval elapsed: the regular snapshot even if nothing changed.
        assert!(should_snapshot(
            Some((0, t)),
            &t,
            DEFAULT_INTERVAL_MS,
            DEFAULT_INTERVAL_MS
        ));
    }

    #[test]
    fn totals_sum_the_own_repositories() {
        let a = parse_account(&json!({"id": 1, "login": "p", "followers": 9, "public_repos": 2}))
            .unwrap();
        let t = totals(&a, &[repo(1, "a", 100), repo(2, "b", 20)]);
        assert_eq!(
            (t.total_stars, t.total_forks, t.repo_count, t.followers),
            (120, 12, 2, 9)
        );
    }

    #[test]
    fn deltas_come_only_from_real_history() {
        const D: i64 = 86_400_000;
        let s = [(0, 120), (10 * D, 130), (25 * D, 135), (40 * D, 142)];
        let now = 40 * D;
        assert_eq!(delta(&s, now, 7 * D), Some(7)); // base at day 33 = 135 (from day 25)
        assert_eq!(delta(&s, now, 30 * D), Some(12)); // base at day 10 = 130
        assert_eq!(delta(&s, now, 40 * D), Some(22)); // base at day 0 = 120
                                                      // 90 days back there was no observation: insufficient, not +0.
        assert_eq!(delta(&s, now, 90 * D), None);
        assert_eq!(delta(&[], now, D), None);
        // A single point: "now" is known, any window before it is not.
        assert_eq!(delta(&[(now, 5)], now, D), None);
        // Unchanged and observed: a real zero.
        assert_eq!(delta(&[(0, 5), (now, 5)], now, D), Some(0));
        assert_eq!(value_at(&s, 12 * D), Some(130));
        assert_eq!(value_at(&s, -1), None);
    }

    #[test]
    fn refresh_gating() {
        let h = DEFAULT_INTERVAL_MS;
        assert_eq!(refresh_gate(0, None, h, false, None), Gate::Go);
        assert_eq!(
            refresh_gate(30 * 60_000, Some(0), h, false, None),
            Gate::TooSoon { next_at_ms: h }
        );
        assert_eq!(refresh_gate(h, Some(0), h, false, None), Gate::Go);
        // Forced: only the short gap applies.
        assert_eq!(
            refresh_gate(30_000, Some(0), h, true, None),
            Gate::TooSoon {
                next_at_ms: MIN_FORCED_GAP_MS
            }
        );
        assert_eq!(refresh_gate(2 * 60_000, Some(0), h, true, None), Gate::Go);
        // Rate limit exhausted: blocked until the reset, even when forced.
        let empty = RateLimit {
            limit: 60,
            remaining: 0,
            reset_at_ms: 10 * 60_000,
        };
        assert_eq!(
            refresh_gate(5 * 60_000, None, h, true, Some(empty)),
            Gate::RateLimited {
                reset_at_ms: 10 * 60_000
            }
        );
        // After the reset it may go again.
        assert_eq!(
            refresh_gate(11 * 60_000, None, h, true, Some(empty)),
            Gate::Go
        );
        // Requests left: the limit doesn't block.
        assert_eq!(
            refresh_gate(
                0,
                None,
                h,
                false,
                Some(RateLimit {
                    remaining: 3,
                    ..empty
                })
            ),
            Gate::Go
        );
    }

    #[test]
    fn repo_number_changes() {
        let a = RepoNumbers {
            stars: 5,
            forks: 1,
            watchers: None,
            open_issues: 0,
        };
        assert!(!a.differs(&a));
        assert!(a.differs(&RepoNumbers { stars: 6, ..a }));
        assert!(a.differs(&RepoNumbers {
            open_issues: 2,
            ..a
        }));
        // A watchers count appearing from the detail endpoint is not a change.
        assert!(!a.differs(&RepoNumbers {
            watchers: Some(8),
            ..a
        }));
        let w = RepoNumbers {
            watchers: Some(8),
            ..a
        };
        assert!(w.differs(&RepoNumbers {
            watchers: Some(9),
            ..a
        }));
    }

    #[test]
    fn sorting_defaults_to_stars_and_puts_unknown_growth_last() {
        let mut rows = vec![
            row(repo(1, "beatbyte", 84), Some(2), Some(9)),
            row(repo(2, "inspector-rust", 128), None, Some(31)),
            row(repo(3, "Marklook", 52), Some(5), None),
            row(repo(4, "kapsel", 52), Some(0), Some(0)),
        ];
        sort_rows(&mut rows, SortKey::Stars);
        let names = |r: &[RepoRow]| r.iter().map(|x| x.repo.name.clone()).collect::<Vec<_>>();
        assert_eq!(
            names(&rows),
            ["inspector-rust", "beatbyte", "kapsel", "Marklook"],
            "ties by name"
        );
        sort_rows(&mut rows, SortKey::Growth7d);
        assert_eq!(
            names(&rows),
            ["Marklook", "beatbyte", "kapsel", "inspector-rust"],
            "unknown last, 0 is real"
        );
        sort_rows(&mut rows, SortKey::Growth30d);
        assert_eq!(names(&rows)[0], "inspector-rust");
        assert_eq!(names(&rows)[3], "Marklook");
        sort_rows(&mut rows, SortKey::Name);
        assert_eq!(
            names(&rows),
            ["beatbyte", "inspector-rust", "kapsel", "Marklook"],
            "case-insensitive"
        );
        sort_rows(&mut rows, SortKey::Updated);
        assert_eq!(names(&rows)[0], "kapsel");
    }

    #[test]
    fn repo_parse_keeps_age_size_and_topics() {
        let r = parse_repo(&json!({"id": 1, "name": "x", "created_at": "2020-03-01T00:00:00Z",
            "size": 2048, "topics": ["rust", "", "tauri"], "homepage": "", "default_branch": "main"}))
        .unwrap();
        assert_eq!(r.created_at_ms, Some(1_583_020_800_000));
        assert_eq!(r.size_kb, Some(2048));
        assert_eq!(r.topics, vec!["rust".to_string(), "tauri".to_string()]);
        assert_eq!(r.homepage, None, "an empty homepage is no homepage");
        assert_eq!(r.default_branch.as_deref(), Some("main"));
        let a = parse_account(&json!({"id": 1, "login": "p", "created_at": "2013-03-04T10:00:00Z",
            "bio": " Builder \n", "public_gists": 4, "hireable": null}))
        .unwrap();
        assert_eq!(a.created_at_ms, Some(1_362_391_200_000));
        assert_eq!(a.bio.as_deref(), Some("Builder"));
        assert_eq!(a.public_gists, 4);
        assert_eq!(a.hireable, None);
    }

    #[test]
    fn events_become_a_cumulative_history_that_snapshots_take_over() {
        let ev = cumulative(&[30, 10, 20]);
        assert_eq!(ev, vec![(10, 1), (20, 2), (30, 3)], "sorted, counting up");
        // Snapshots win from their first point on (they also see removed stars).
        assert_eq!(
            merge_history(&ev, &[(25, 1), (40, 2)]),
            vec![(10, 1), (20, 2), (25, 1), (40, 2)]
        );
        assert_eq!(merge_history(&ev, &[]), ev);
        assert_eq!(merge_history(&[], &[(5, 9)]), vec![(5, 9)]);
    }

    #[test]
    fn a_window_carries_the_value_in_and_ends_now() {
        let s = [(10, 1), (20, 2), (30, 3)];
        // A quiet range still has a line: the value at its start, held to now.
        assert_eq!(window(&s, 100, Some(50)), vec![(50, 3), (100, 3)]);
        assert_eq!(window(&s, 100, Some(85)), vec![(15, 1), (20, 2), (30, 3), (100, 3)]);
        // Before the first point there was nothing: no invented start.
        assert_eq!(window(&s, 25, Some(20)), vec![(10, 1), (20, 2), (25, 2)]);
        assert_eq!(window(&s, 100, None), vec![(10, 1), (20, 2), (30, 3), (100, 3)]);
        assert_eq!(window(&s, 30, None), vec![(10, 1), (20, 2), (30, 3)]);
        assert!(window(&s, 5, Some(3)).is_empty(), "nothing known yet");
        assert!(window(&[], 5, None).is_empty());
    }

    #[test]
    fn monthly_gains_are_unknown_before_the_history_unless_reconstructed() {
        use chrono::{TimeZone, Utc};
        let ms = |y, m, d| Utc.with_ymd_and_hms(y, m, d, 12, 0, 0).unwrap().timestamp_millis();
        let s = [(ms(2026, 8, 5), 1), (ms(2026, 9, 10), 2), (ms(2026, 9, 20), 4)];
        let now = ms(2026, 10, 15);
        let g = gains_by_month(&s, now, 3, true);
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].0, Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap().timestamp_millis());
        assert_eq!(g.iter().map(|x| x.1).collect::<Vec<_>>(), vec![Some(1), Some(3), Some(0)]);
        // Without a full reconstruction August is unknown, not "+1".
        let g = gains_by_month(&s, now, 3, false);
        assert_eq!(g[0].1, None);
        assert_eq!(g[1].1, Some(3));
        // Across a year boundary.
        let g = gains_by_month(&s, ms(2027, 1, 2), 3, true);
        assert_eq!(g[0].0, Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap().timestamp_millis());
    }

    fn cand(id: i64, value: i64, synced: Option<(i64, i64)>) -> BackfillCandidate {
        BackfillCandidate {
            repo_id: id,
            full_name: format!("o/r{id}"),
            kind: EventKind::Stars,
            value,
            synced_value: synced.map(|s| s.0),
            synced_ms: synced.map(|s| s.1),
        }
    }

    #[test]
    fn backfill_plan_reads_whole_lists_within_the_budget() {
        const H: i64 = 3_600_000;
        let now = 100 * H;
        // Due: never read; changed and old enough. Not due: unchanged, too
        // recent, or nothing to read.
        assert!(cand(1, 5, None).due(now));
        assert!(cand(1, 6, Some((5, now - 7 * H))).due(now));
        assert!(!cand(1, 5, Some((5, 0))).due(now));
        assert!(!cand(1, 6, Some((5, now - H))).due(now));
        assert!(!cand(1, 0, None).due(now));
        assert_eq!(pages_for(1), 1);
        assert_eq!(pages_for(100), 1);
        assert_eq!(pages_for(101), 2);
        assert_eq!(pages_for(10_000_000), MAX_EVENT_PAGES);
        // Never-read first, biggest first; one that doesn't fit is skipped,
        // a smaller one still fits.
        let plan = plan_backfill(
            vec![cand(1, 250, Some((200, 0))), cand(2, 3000, None), cand(3, 150, None), cand(4, 40, None)],
            now,
            33,
        );
        let ids: Vec<i64> = plan.iter().map(|c| c.repo_id).collect();
        assert_eq!(ids, vec![2, 3, 4], "30 + 2 + 1 pages; repo 1 (3 pages) no longer fits");
        // A list bigger than the whole budget gets a refresh of its own.
        let plan = plan_backfill(vec![cand(1, 9000, None), cand(2, 10, None)], now, 60);
        assert_eq!(plan.iter().map(|c| c.repo_id).collect::<Vec<_>>(), vec![1]);
        assert!(plan_backfill(vec![cand(1, 5, Some((5, 0)))], now, 60).is_empty());
    }

    #[test]
    fn extras_parse() {
        let times = parse_event_times(
            &json!([{"starred_at": "2026-01-01T00:00:00Z", "user": {}}, {"starred_at": null}]),
            EventKind::Stars,
        )
        .unwrap();
        assert_eq!(times, vec![1_767_225_600_000]);
        assert!(parse_event_times(&json!({"message": "x"}), EventKind::Forks).is_none());
        assert_eq!(
            parse_commit_activity(&json!([{"week": 100, "total": 4, "days": []}])),
            Some(vec![(100_000, 4)])
        );
        let r = parse_releases(&json!([
            {"tag_name": "v1", "published_at": "2026-01-01T00:00:00Z", "assets": [{"download_count": 3}, {"download_count": 4}]},
            {"tag_name": "v2", "published_at": "2026-02-01T00:00:00Z", "assets": []},
            {"tag_name": "draft", "draft": true, "assets": [{"download_count": 99}]}
        ]))
        .unwrap();
        assert_eq!((r.count, r.downloads, r.latest_tag.as_deref()), (2, 7, Some("v2")));
        assert!(r.dates[0] > r.dates[1], "newest first");
        assert_eq!(
            parse_languages(&json!({"TypeScript": 10, "Rust": 90, "Empty": 0})),
            Some(vec![("Rust".into(), 90), ("TypeScript".into(), 10)])
        );
        let t = parse_traffic(
            &json!({"count": 12, "uniques": 5, "views": [{"timestamp": "2026-01-01T00:00:00Z", "count": 12}]}),
            "views",
        )
        .unwrap();
        assert_eq!((t.count, t.uniques, t.days.len()), (12, 5, 1));
        assert!(parse_traffic(&json!({"message": "Must have push access"}), "views").is_none());
    }
}
