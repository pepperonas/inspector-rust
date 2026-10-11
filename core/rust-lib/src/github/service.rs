//! The stateful side of GitHub analytics: which token to use, the device-flow
//! login, refreshing (gated by the interval and the rate limit), the
//! background refresher, and the views the panel renders.
//!
//! Tokens live ONLY in the keychain (service `io.celox.inspector-rust`):
//! `github-oauth-v1` (device flow) and `github-token-v1` (personal access
//! token, shared with `repo`). Order: device-flow token → PAT → `gh` CLI.
//! No token is ever logged, stored in `settings`, returned over IPC or
//! written into an error.
//!
//! What the UI needs between refreshes comes from `history.db` — a refresh is
//! never on the render path, and offline the panel keeps showing the cache.

use super::client::{self, CallInfo, Transport};
use super::store::{self, AccountField};
use super::{
    delta, refresh_gate, sort_rows, Gate, Point, PollResult, RateLimit, Repo, RepoNumbers, RepoRow,
    SortKey, DEFAULT_INTERVAL_MS, ERR_NOT_CONNECTED, ERR_NOT_FOUND, GROWTH_CHART_TOP,
    INTERVAL_CHOICES_H, WINDOWS,
};
use crate::db::DbHandle;
use crate::settings;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const KEYRING_SERVICE: &str = "io.celox.inspector-rust";
const OAUTH_USER: &str = "github-oauth-v1";

/// Public client id of the Inspector Rust OAuth app (device flow enabled).
/// Public by design — the device flow has no client secret. Empty until the
/// app is registered; `IR_GITHUB_CLIENT_ID` overrides it for development.
const BUILTIN_CLIENT_ID: &str = "";

/// Emitted whenever status or data changed (login, refresh, disconnect).
pub const EVENT: &str = "github-changed";
pub const ERR_NO_CLIENT_ID: &str = "github.no_client_id";
pub const ERR_BUSY: &str = "github.busy";

/// How often the background thread looks whether a refresh is due. The
/// interval itself (1–24 h) decides when GitHub is actually asked.
const BACKGROUND_TICK: Duration = Duration::from_secs(300);
const BACKGROUND_FIRST: Duration = Duration::from_secs(20);

const K_AUTO: &str = "github.auto_refresh";
const K_INTERVAL: &str = "github.interval_h";
const K_PRIVATE: &str = "github.private_repos";
const K_USER: &str = "github.user_id";
const K_LOGIN: &str = "github.login";
const K_CHECKED: &str = "github.last_checked_ms";
const K_OK: &str = "github.last_ok_ms";
const K_ERROR: &str = "github.last_error";
const K_RATE: &str = "github.rate";
const K_SCOPES: &str = "github.scopes";

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ── Tokens ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenSource {
    /// Signed in through the browser (device flow).
    Oauth,
    /// A personal access token entered in Settings.
    Pat,
    /// The GitHub CLI's login.
    Gh,
}

fn oauth_entry() -> Option<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, OAUTH_USER).ok()
}

fn oauth_token() -> Option<String> {
    oauth_entry()?
        .get_password()
        .ok()
        .filter(|t| !t.trim().is_empty())
}

/// The first token that exists, in the documented order.
pub fn resolve_token() -> Option<(String, TokenSource)> {
    pick_token(
        oauth_token,
        crate::repo_clone::stored_token,
        crate::repo_clone::gh_token,
    )
}

/// The order, testable without a keychain. Later sources are only asked when
/// the earlier ones have nothing (`gh` costs a process spawn).
fn pick_token(
    oauth: impl FnOnce() -> Option<String>,
    pat: impl FnOnce() -> Option<String>,
    gh: impl FnOnce() -> Option<String>,
) -> Option<(String, TokenSource)> {
    oauth()
        .map(|t| (t, TokenSource::Oauth))
        .or_else(|| pat().map(|t| (t, TokenSource::Pat)))
        .or_else(|| gh().map(|t| (t, TokenSource::Gh)))
}

pub fn client_id() -> Option<String> {
    std::env::var("IR_GITHUB_CLIENT_ID")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| (!BUILTIN_CLIENT_ID.is_empty()).then(|| BUILTIN_CLIENT_ID.to_string()))
}

/// The scope asked for at login. Empty = public data only (GitHub's default
/// for OAuth apps). OAuth apps have no read-only scope for private
/// repositories — `repo` is the only one, so it's opt-in and the UI says so;
/// a fine-grained PAT with "Metadata: read" is the least-privilege way.
pub fn scope_for(private_repos: bool) -> &'static str {
    if private_repos {
        "repo"
    } else {
        ""
    }
}

// ── Config ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubConfig {
    #[serde(default = "yes")]
    pub auto_refresh: bool,
    #[serde(default = "one")]
    pub interval_h: i64,
    #[serde(default)]
    pub private_repos: bool,
}

fn yes() -> bool {
    true
}
fn one() -> i64 {
    1
}

impl Default for GithubConfig {
    fn default() -> Self {
        Self {
            auto_refresh: true,
            interval_h: DEFAULT_INTERVAL_MS / 3_600_000,
            private_repos: false,
        }
    }
}

/// An interval GitHub's rate limit and the history granularity can live
/// with: one of `INTERVAL_CHOICES_H`, else the default.
pub fn clamp_interval_h(h: i64) -> i64 {
    if INTERVAL_CHOICES_H.contains(&h) {
        h
    } else {
        DEFAULT_INTERVAL_MS / 3_600_000
    }
}

impl GithubConfig {
    pub fn interval_ms(&self) -> i64 {
        clamp_interval_h(self.interval_h) * 3_600_000
    }

    pub fn load(db: &DbHandle) -> Self {
        let d = Self::default();
        Self {
            auto_refresh: settings::get_bool(db, K_AUTO, d.auto_refresh).unwrap_or(d.auto_refresh),
            interval_h: clamp_interval_h(get_i64(db, K_INTERVAL).unwrap_or(d.interval_h)),
            private_repos: settings::get_bool(db, K_PRIVATE, d.private_repos)
                .unwrap_or(d.private_repos),
        }
    }

    pub fn save(&self, db: &DbHandle) -> Result<(), String> {
        let e = |e: anyhow::Error| e.to_string();
        settings::set(db, K_AUTO, if self.auto_refresh { "true" } else { "false" }).map_err(e)?;
        settings::set(
            db,
            K_INTERVAL,
            &clamp_interval_h(self.interval_h).to_string(),
        )
        .map_err(e)?;
        settings::set(
            db,
            K_PRIVATE,
            if self.private_repos { "true" } else { "false" },
        )
        .map_err(e)
    }
}

fn get_i64(db: &DbHandle, k: &str) -> Option<i64> {
    settings::get(db, k)
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse().ok())
}

fn get_str(db: &DbHandle, k: &str) -> Option<String> {
    settings::get(db, k)
        .ok()
        .flatten()
        .filter(|v| !v.is_empty())
}

fn put(db: &DbHandle, k: &str, v: &str) {
    let _ = settings::set(db, k, v);
}

fn rate_of(db: &DbHandle) -> Option<RateLimit> {
    get_str(db, K_RATE).and_then(|v| serde_json::from_str(&v).ok())
}

// ── Refresh ─────────────────────────────────────────────────────────────────

static REFRESHING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
pub struct RefreshResult {
    pub gate: Gate,
    /// Whether GitHub was actually asked.
    pub fetched: bool,
    pub account_snapshot: bool,
    pub repo_snapshots: usize,
}

/// Refresh if the gate allows it. `force` = the user asked (`github refresh`).
pub fn refresh(db: &DbHandle, force: bool) -> Result<RefreshResult, String> {
    if REFRESHING.swap(true, Ordering::SeqCst) {
        return Err(ERR_BUSY.into());
    }
    let res = match resolve_token() {
        Some((token, _)) => refresh_with(&client::Ureq, db, &token, now_ms(), force),
        None => Err(ERR_NOT_CONNECTED.into()),
    };
    REFRESHING.store(false, Ordering::SeqCst);
    emit();
    res
}

/// One refresh against a transport — the whole policy, testable with a fake.
pub fn refresh_with(
    t: &dyn Transport,
    db: &DbHandle,
    token: &str,
    now: i64,
    force: bool,
) -> Result<RefreshResult, String> {
    let cfg = GithubConfig::load(db);
    let gate = refresh_gate(
        now,
        get_i64(db, K_CHECKED),
        cfg.interval_ms(),
        force,
        rate_of(db),
    );
    if gate != Gate::Go {
        return Ok(RefreshResult {
            gate,
            fetched: false,
            account_snapshot: false,
            repo_snapshots: 0,
        });
    }
    put(db, K_CHECKED, &now.to_string());
    let mut info = CallInfo::default();
    let fetched = client::fetch_account(t, token, &mut info)
        .and_then(|a| client::fetch_repos(t, token, &mut info).map(|r| (a, r)));
    // The rate limit is worth keeping even from a failed call.
    if let Some(r) = info.rate {
        put(db, K_RATE, &serde_json::to_string(&r).unwrap_or_default());
    }
    let (account, repos) = match fetched {
        Ok(v) => v,
        Err(e) => {
            put(db, K_ERROR, &e);
            return Err(e);
        }
    };
    let recorded = {
        let mut conn = db.lock();
        store::record(&mut conn, &account, &repos, now, cfg.interval_ms())
            .map_err(|e| e.to_string())?
    };
    put(db, K_USER, &account.id.to_string());
    put(db, K_LOGIN, &account.login);
    put(db, K_OK, &now.to_string());
    put(db, K_SCOPES, info.scopes.as_deref().unwrap_or(""));
    let _ = settings::delete(db, K_ERROR);
    Ok(RefreshResult {
        gate,
        fetched: true,
        account_snapshot: recorded.account_snapshot,
        repo_snapshots: recorded.repo_snapshots,
    })
}

// ── Device flow ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum DeviceView {
    Idle,
    Waiting {
        user_code: String,
        verification_uri: String,
        expires_at_ms: i64,
    },
    Failed {
        message: String,
    },
}

static DEVICE: Mutex<DeviceView> = Mutex::new(DeviceView::Idle);
/// Bumped by every start/cancel: a poll thread of an older generation stops.
static DEVICE_GEN: AtomicU64 = AtomicU64::new(0);

/// Ask GitHub for a user code and poll for the token in the background.
pub fn device_start(db: &DbHandle) -> Result<DeviceView, String> {
    let id = client_id().ok_or_else(|| ERR_NO_CLIENT_ID.to_string())?;
    let scope = scope_for(GithubConfig::load(db).private_repos);
    let code = client::device_start(&client::Ureq, &id, scope)?;
    let gen = DEVICE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let view = DeviceView::Waiting {
        user_code: code.user_code.clone(),
        verification_uri: code.verification_uri.clone(),
        expires_at_ms: now_ms() + code.expires_in_s * 1000,
    };
    *DEVICE.lock() = view.clone();
    let db = db.clone();
    std::thread::Builder::new()
        .name("ir-github-device".into())
        .spawn(move || {
            poll_loop(
                &db,
                &id,
                code.device_code,
                code.interval_s,
                code.expires_in_s,
                gen,
            )
        })
        .map_err(|e| e.to_string())?;
    Ok(view)
}

fn poll_loop(
    db: &DbHandle,
    id: &str,
    device_code: String,
    mut interval_s: i64,
    expires_in_s: i64,
    gen: u64,
) {
    let deadline = now_ms() + expires_in_s * 1000;
    let current = || DEVICE_GEN.load(Ordering::SeqCst) == gen;
    loop {
        std::thread::sleep(Duration::from_secs(interval_s.max(5) as u64));
        if !current() {
            return;
        }
        if now_ms() > deadline {
            finish(
                gen,
                DeviceView::Failed {
                    message: "github.device_expired".into(),
                },
            );
            return;
        }
        match client::device_poll(&client::Ureq, id, &device_code, interval_s) {
            PollResult::Pending => {}
            PollResult::SlowDown(n) => interval_s = n,
            PollResult::Token { token, .. } => {
                let saved = oauth_entry()
                    .ok_or_else(|| "keychain".to_string())
                    .and_then(|e| {
                        e.set_password(token.trim())
                            .map_err(|_| "github.keychain".to_string())
                    });
                match saved {
                    Ok(()) => {
                        finish(gen, DeviceView::Idle);
                        // A new login must not wait for the interval.
                        let _ = settings::delete(db, K_CHECKED);
                        let _ = refresh(db, true);
                    }
                    Err(e) => finish(gen, DeviceView::Failed { message: e }),
                }
                return;
            }
            PollResult::Expired => {
                finish(
                    gen,
                    DeviceView::Failed {
                        message: "github.device_expired".into(),
                    },
                );
                return;
            }
            PollResult::Denied => {
                finish(
                    gen,
                    DeviceView::Failed {
                        message: "github.device_denied".into(),
                    },
                );
                return;
            }
            PollResult::Failed(m) => {
                finish(gen, DeviceView::Failed { message: m });
                return;
            }
        }
    }
}

fn finish(gen: u64, view: DeviceView) {
    if DEVICE_GEN.load(Ordering::SeqCst) == gen {
        *DEVICE.lock() = view;
        emit();
    }
}

pub fn device_cancel() {
    DEVICE_GEN.fetch_add(1, Ordering::SeqCst);
    *DEVICE.lock() = DeviceView::Idle;
    emit();
}

/// Forget the device-flow token. The local history stays (it's the user's
/// data); a PAT or `gh` login, if present, still connects — the status says so.
pub fn disconnect(db: &DbHandle) {
    device_cancel();
    if let Some(e) = oauth_entry() {
        let _ = e.delete_credential();
    }
    let _ = settings::delete(db, K_ERROR);
    emit();
}

// ── Status ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub connected: bool,
    pub source: Option<TokenSource>,
    pub client_id_configured: bool,
    pub pat_stored: bool,
    pub gh_available: bool,
    pub user_id: Option<i64>,
    pub login: Option<String>,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub html_url: Option<String>,
    pub last_ok_ms: Option<i64>,
    pub last_checked_ms: Option<i64>,
    pub last_error: Option<String>,
    pub rate: Option<RateLimit>,
    /// `x-oauth-scopes` of the last call; empty = public data only.
    pub scopes: Option<String>,
    /// When an automatic refresh is due next (None = due now / never ran).
    pub next_refresh_ms: Option<i64>,
    pub rate_limited_until: Option<i64>,
    pub refreshing: bool,
    pub device: DeviceView,
    pub config: GithubConfig,
}

pub fn status(db: &DbHandle) -> Status {
    let token = resolve_token();
    let cfg = GithubConfig::load(db);
    let user_id = get_i64(db, K_USER);
    let profile = user_id.and_then(|id| store::account(&db.lock(), id).ok().flatten());
    let now = now_ms();
    let rate = rate_of(db);
    let last_checked = get_i64(db, K_CHECKED);
    let (next_refresh_ms, rate_limited_until) =
        match refresh_gate(now, last_checked, cfg.interval_ms(), false, rate) {
            Gate::Go => (None, None),
            Gate::TooSoon { next_at_ms } => (Some(next_at_ms), None),
            Gate::RateLimited { reset_at_ms } => (Some(reset_at_ms), Some(reset_at_ms)),
        };
    Status {
        connected: token.is_some(),
        source: token.map(|(_, s)| s),
        client_id_configured: client_id().is_some(),
        pat_stored: crate::repo_clone::has_stored_token(),
        gh_available: crate::repo_clone::gh_available(),
        user_id,
        html_url: get_str(db, K_LOGIN).map(|l| format!("https://github.com/{l}")),
        login: get_str(db, K_LOGIN),
        name: profile.as_ref().and_then(|p| p.1.clone()),
        avatar_url: profile.and_then(|p| p.2),
        last_ok_ms: get_i64(db, K_OK),
        last_checked_ms: last_checked,
        last_error: get_str(db, K_ERROR),
        rate,
        scopes: settings::get(db, K_SCOPES).ok().flatten(),
        next_refresh_ms,
        rate_limited_until,
        refreshing: REFRESHING.load(Ordering::SeqCst),
        device: DEVICE.lock().clone(),
        config: cfg,
    }
}

/// The current user's id from the last successful refresh.
pub fn user_id(db: &DbHandle) -> Option<i64> {
    get_i64(db, K_USER)
}

// ── Views ───────────────────────────────────────────────────────────────────

/// A chart range in ms; `None` = all history.
pub fn range_ms(name: &str) -> Option<i64> {
    const D: i64 = 86_400_000;
    match name.trim().to_ascii_lowercase().as_str() {
        "7d" => Some(7 * D),
        "30d" => Some(30 * D),
        "90d" => Some(90 * D),
        "1y" => Some(365 * D),
        _ => None,
    }
}

/// The points inside the range — real snapshots only, nothing invented at
/// the edges.
pub fn in_range(series: &[Point], now: i64, range: Option<i64>) -> Vec<Point> {
    match range {
        None => series.to_vec(),
        Some(r) => series
            .iter()
            .copied()
            .filter(|(t, _)| *t >= now - r)
            .collect(),
    }
}

/// 24h / 7d / 30d / 90d changes; None where the history is too short.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct Deltas {
    pub d24h: Option<i64>,
    pub d7d: Option<i64>,
    pub d30d: Option<i64>,
    pub d90d: Option<i64>,
}

pub fn deltas(series: &[Point], now: i64) -> Deltas {
    let w = |name: &str| {
        WINDOWS
            .iter()
            .find(|(n, _)| *n == name)
            .and_then(|(_, ms)| delta(series, now, *ms))
    };
    Deltas {
        d24h: w("24h"),
        d7d: w("7d"),
        d30d: w("30d"),
        d90d: w("90d"),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Kpi {
    pub key: &'static str,
    pub value: i64,
    pub deltas: Deltas,
}

#[derive(Debug, Clone, Serialize)]
pub struct GrowthLine {
    pub id: i64,
    pub name: String,
    pub points: Vec<Point>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dashboard {
    pub user_id: i64,
    pub kpis: Vec<Kpi>,
    pub followers: Vec<Point>,
    pub stars: Vec<Point>,
    pub rows: Vec<RepoRow>,
    pub growth: Vec<GrowthLine>,
    /// First stored snapshot — "history since …".
    pub history_since_ms: Option<i64>,
    pub snapshot_count: usize,
    pub private_count: usize,
}

fn stars_points(series: &[(i64, RepoNumbers)]) -> Vec<Point> {
    series.iter().map(|(t, n)| (*t, n.stars)).collect()
}

/// Everything the dashboard shows, from the local history only.
pub fn build_dashboard(
    conn: &rusqlite::Connection,
    user_id: i64,
    now: i64,
    range: Option<i64>,
    sort: SortKey,
) -> rusqlite::Result<Option<Dashboard>> {
    let Some((_, last)) = store::last_account_snapshot(conn, user_id)? else {
        return Ok(None);
    };
    let followers = store::account_series(conn, user_id, AccountField::Followers)?;
    let stars = store::account_series(conn, user_id, AccountField::TotalStars)?;
    let repo_count = store::account_series(conn, user_id, AccountField::RepoCount)?;
    let forks = store::account_series(conn, user_id, AccountField::TotalForks)?;
    let kpis = vec![
        Kpi {
            key: "followers",
            value: last.followers,
            deltas: deltas(&followers, now),
        },
        Kpi {
            key: "stars",
            value: last.total_stars,
            deltas: deltas(&stars, now),
        },
        Kpi {
            key: "repos",
            value: last.repo_count,
            deltas: deltas(&repo_count, now),
        },
        Kpi {
            key: "forks",
            value: last.total_forks,
            deltas: deltas(&forks, now),
        },
    ];
    let repos: Vec<Repo> = store::current_repos(conn, user_id)?;
    let private_count = repos.iter().filter(|r| r.private).count();
    let mut rows = Vec::with_capacity(repos.len());
    let mut lines = Vec::new();
    for r in repos {
        let s = stars_points(&store::repo_series(conn, r.id)?);
        rows.push(RepoRow {
            growth_7d: delta(&s, now, 7 * 86_400_000),
            growth_30d: delta(&s, now, 30 * 86_400_000),
            repo: r.clone(),
        });
        lines.push((
            r.stars,
            GrowthLine {
                id: r.id,
                name: r.name,
                points: in_range(&s, now, range),
            },
        ));
    }
    sort_rows(&mut rows, sort);
    lines.sort_by_key(|l| std::cmp::Reverse(l.0));
    let growth: Vec<GrowthLine> = lines
        .into_iter()
        .take(GROWTH_CHART_TOP)
        .map(|(_, l)| l)
        .collect();
    Ok(Some(Dashboard {
        user_id,
        kpis,
        history_since_ms: followers.first().map(|p| p.0),
        snapshot_count: followers.len(),
        followers: in_range(&followers, now, range),
        stars: in_range(&stars, now, range),
        rows,
        growth,
        private_count,
    }))
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoDetail {
    pub repo: Repo,
    pub stars: Vec<Point>,
    pub forks: Vec<Point>,
    pub open_issues: Vec<Point>,
    pub star_deltas: Deltas,
    pub fork_deltas: Deltas,
    pub first_seen_ms: Option<i64>,
}

pub fn build_repo_detail(
    conn: &rusqlite::Connection,
    user_id: i64,
    query: &str,
    now: i64,
    range: Option<i64>,
) -> rusqlite::Result<Option<RepoDetail>> {
    let Some(repo) = store::find_repo(conn, user_id, query)? else {
        return Ok(None);
    };
    let series = store::repo_series(conn, repo.id)?;
    let pick = |f: fn(&RepoNumbers) -> i64| -> Vec<Point> {
        series.iter().map(|(t, n)| (*t, f(n))).collect()
    };
    let stars = pick(|n| n.stars);
    let forks = pick(|n| n.forks);
    let issues = pick(|n| n.open_issues);
    Ok(Some(RepoDetail {
        star_deltas: deltas(&stars, now),
        fork_deltas: deltas(&forks, now),
        first_seen_ms: series.first().map(|s| s.0),
        stars: in_range(&stars, now, range),
        forks: in_range(&forks, now, range),
        open_issues: in_range(&issues, now, range),
        repo,
    }))
}

/// The detail view, plus the watchers count fetched once on demand (the
/// list endpoint doesn't carry it). Offline or rate-limited: the cached view.
pub fn repo_detail(db: &DbHandle, query: &str, range: Option<i64>) -> Result<RepoDetail, String> {
    let user = user_id(db).ok_or_else(|| ERR_NOT_CONNECTED.to_string())?;
    let now = now_ms();
    let load = || build_repo_detail(&db.lock(), user, query, now, range).map_err(|e| e.to_string());
    let detail = load()?.ok_or_else(|| ERR_NOT_FOUND.to_string())?;
    if detail.repo.watchers.is_some() {
        return Ok(detail);
    }
    let limited = rate_of(db).is_some_and(|r| r.remaining <= 0 && now < r.reset_at_ms);
    if let (Some((token, _)), false) = (resolve_token(), limited) {
        let mut info = CallInfo::default();
        if let Ok(fresh) =
            client::fetch_repo(&client::Ureq, &token, &detail.repo.full_name, &mut info)
        {
            if let Some(w) = fresh.watchers {
                let _ = store::set_watchers(&db.lock(), detail.repo.id, w);
            }
        }
        if let Some(r) = info.rate {
            put(db, K_RATE, &serde_json::to_string(&r).unwrap_or_default());
        }
        return Ok(load()?.unwrap_or(detail));
    }
    Ok(detail)
}

// ── Export ──────────────────────────────────────────────────────────────────

/// JSON or CSV of the stored data. Private repositories only when asked.
pub fn export_text(
    conn: &rusqlite::Connection,
    user_id: i64,
    login: &str,
    format: &str,
    include_private: bool,
    now: i64,
) -> Result<(String, &'static str), String> {
    let e = |e: rusqlite::Error| e.to_string();
    let history = store::account_snapshots(conn, user_id).map_err(e)?;
    let repos = store::current_repos(conn, user_id).map_err(e)?;
    let repo_history = store::all_repo_snapshots(conn, user_id).map_err(e)?;
    let data = super::export::ExportData {
        login,
        current: history.last().map(|h| h.1),
        account_history: &history,
        repos: &repos,
        repo_history: &repo_history,
    };
    match format.trim().to_ascii_lowercase().as_str() {
        "json" => Ok((super::export::json(&data, include_private, now), "json")),
        "csv" => Ok((super::export::csv(&data, include_private), "csv")),
        other => Err(format!("Unbekanntes Format: {other}")),
    }
}

// ── Background ──────────────────────────────────────────────────────────────

static APP: OnceLock<AppHandle> = OnceLock::new();

fn emit() {
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, ());
    }
}

/// Start the refresher: shortly after launch, then every few minutes it
/// checks whether a refresh is due (the interval gate decides). Nothing runs
/// without a token or with auto refresh off.
pub fn start(app: &AppHandle, db: &DbHandle) {
    let _ = APP.set(app.clone());
    let db = db.clone();
    let _ = std::thread::Builder::new()
        .name("ir-github".into())
        .spawn(move || {
            std::thread::sleep(BACKGROUND_FIRST);
            loop {
                if GithubConfig::load(&db).auto_refresh && resolve_token().is_some() {
                    match refresh(&db, false) {
                        Ok(r) if r.fetched => tracing::info!(
                            "github: refreshed ({} repo snapshots, account snapshot {})",
                            r.repo_snapshots,
                            r.account_snapshot
                        ),
                        Ok(_) => {}
                        // The sentinel only — never anything token-bearing.
                        Err(e) => tracing::info!("github: refresh failed: {e}"),
                    }
                }
                std::thread::sleep(BACKGROUND_TICK);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::client::fake::Fake;
    use crate::github::client::API;
    use crate::github::{ERR_RATE_LIMITED, ERR_UNAUTHORIZED};
    use std::sync::Arc;

    const H: i64 = 3_600_000;
    const D: i64 = 86_400_000;

    fn db() -> DbHandle {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let db = Arc::new(Mutex::new(conn));
        settings::init_table(&db).unwrap();
        store::init_schema(&db.lock()).unwrap();
        db
    }

    fn repos_url() -> String {
        format!("{API}/user/repos?affiliation=owner&per_page=100&sort=full_name")
    }

    fn api(followers: i64, stars: i64, remaining: i64) -> Fake {
        let rl = [
            ("x-ratelimit-limit", "5000"),
            ("x-ratelimit-remaining", "4990"),
            ("x-ratelimit-reset", "2000000000"),
        ];
        let rem = remaining.to_string();
        let rl2 = [
            ("x-ratelimit-limit", "5000"),
            ("x-ratelimit-remaining", rem.as_str()),
            ("x-ratelimit-reset", "2000000000"),
        ];
        Fake::default()
            .route(
                &format!("{API}/user"),
                200,
                &format!(r#"{{"id":7,"login":"octo","name":"Octo","followers":{followers},"following":1,"public_repos":2}}"#),
                &rl,
            )
            .route(
                &repos_url(),
                200,
                &format!(
                    r#"[{{"id":1,"name":"a","full_name":"octo/a","stargazers_count":{stars},"forks_count":2}},
                        {{"id":2,"name":"b","full_name":"octo/b","private":true,"stargazers_count":1}}]"#
                ),
                &rl2,
            )
    }

    #[test]
    fn token_order_is_oauth_then_pat_then_gh() {
        let some = |s: &str| {
            let s = s.to_string();
            move || Some(s)
        };
        assert_eq!(
            pick_token(some("o"), some("p"), some("g")),
            Some(("o".into(), TokenSource::Oauth))
        );
        assert_eq!(
            pick_token(|| None, some("p"), some("g")),
            Some(("p".into(), TokenSource::Pat))
        );
        assert_eq!(
            pick_token(|| None, || None, some("g")),
            Some(("g".into(), TokenSource::Gh))
        );
        assert_eq!(pick_token(|| None, || None, || None), None);
        // `gh` (a process spawn) is not even asked when a token exists.
        let asked = std::cell::Cell::new(false);
        let _ = pick_token(
            some("o"),
            || None,
            || {
                asked.set(true);
                None
            },
        );
        assert!(!asked.get());
    }

    #[test]
    fn scope_is_empty_unless_private_repos_are_wanted() {
        assert_eq!(scope_for(false), "");
        assert_eq!(scope_for(true), "repo");
    }

    #[test]
    fn interval_is_one_of_the_choices() {
        assert_eq!(clamp_interval_h(3), 3);
        assert_eq!(clamp_interval_h(24), 24);
        assert_eq!(clamp_interval_h(0), 1);
        assert_eq!(clamp_interval_h(5), 1);
        assert_eq!(
            GithubConfig {
                interval_h: 99,
                ..Default::default()
            }
            .interval_ms(),
            H
        );
    }

    #[test]
    fn refresh_records_once_per_interval_and_keeps_the_rate() {
        let db = db();
        let r = refresh_with(&api(10, 5, 4990), &db, "tok", 1_000 * H, false).unwrap();
        assert!(r.fetched && r.account_snapshot);
        assert_eq!(r.repo_snapshots, 2);
        assert_eq!(user_id(&db), Some(7));
        assert_eq!(rate_of(&db).unwrap().remaining, 4990);
        // Inside the interval an automatic refresh doesn't ask GitHub.
        let fake = api(10, 5, 4990);
        let r = refresh_with(&fake, &db, "tok", 1_000 * H + 10 * 60_000, false).unwrap();
        assert!(!r.fetched);
        assert!(fake.calls.borrow().is_empty());
        assert_eq!(
            r.gate,
            Gate::TooSoon {
                next_at_ms: 1_001 * H
            }
        );
        // A forced refresh does; with unchanged numbers it stores nothing.
        let r = refresh_with(&api(10, 5, 4990), &db, "tok", 1_000 * H + 10 * 60_000, true).unwrap();
        assert!(r.fetched && !r.account_snapshot);
        assert_eq!(r.repo_snapshots, 0);
        // A change is stored even inside the interval.
        let r = refresh_with(&api(11, 6, 4990), &db, "tok", 1_000 * H + 20 * 60_000, true).unwrap();
        assert!(r.account_snapshot);
        assert_eq!(r.repo_snapshots, 1);
    }

    #[test]
    fn an_exhausted_rate_limit_blocks_until_the_reset() {
        let db = db();
        let _ = refresh_with(&api(10, 5, 0), &db, "tok", 1_000 * H, false).unwrap();
        let fake = api(10, 5, 0);
        let r = refresh_with(&fake, &db, "tok", 1_000 * H + 2 * 60_000, true).unwrap();
        assert_eq!(
            r.gate,
            Gate::RateLimited {
                reset_at_ms: 2_000_000_000_000
            }
        );
        assert!(
            fake.calls.borrow().is_empty(),
            "no request into a known wall"
        );
        let _ = ERR_RATE_LIMITED;
    }

    #[test]
    fn errors_are_kept_as_sentinels_and_the_cache_survives() {
        let db = db();
        refresh_with(&api(10, 5, 4990), &db, "tok", 1_000 * H, false).unwrap();
        let dead = Fake {
            offline: true,
            ..Default::default()
        };
        let e = refresh_with(&dead, &db, "tok", 1_002 * H, false).unwrap_err();
        assert_eq!(e, crate::github::ERR_NETWORK);
        assert_eq!(
            get_str(&db, K_ERROR).as_deref(),
            Some(crate::github::ERR_NETWORK)
        );
        // The dashboard still renders from the cache.
        let d = build_dashboard(&db.lock(), 7, 1_002 * H, None, SortKey::Stars)
            .unwrap()
            .unwrap();
        assert_eq!(d.kpis[0].value, 10);
        // A revoked token: 401 → unauthorized, never the token in the text.
        let revoked = Fake::default().route(&format!("{API}/user"), 401, "{}", &[]);
        let e = refresh_with(&revoked, &db, "secret-token", 1_004 * H, false).unwrap_err();
        assert_eq!(e, ERR_UNAUTHORIZED);
        assert!(!get_str(&db, K_ERROR).unwrap().contains("secret"));
        // Success clears the error again.
        refresh_with(&api(10, 5, 4990), &db, "tok", 1_006 * H, false).unwrap();
        assert_eq!(get_str(&db, K_ERROR), None);
    }

    #[test]
    fn dashboard_deltas_need_real_history() {
        let db = db();
        let t0 = 1_000 * D;
        refresh_with(&api(100, 50, 4990), &db, "tok", t0, false).unwrap();
        let d = build_dashboard(&db.lock(), 7, t0, None, SortKey::Stars)
            .unwrap()
            .unwrap();
        assert_eq!(d.kpis[0].key, "followers");
        assert_eq!(
            d.kpis[0].deltas,
            Deltas::default(),
            "one snapshot: no delta, not zero"
        );
        refresh_with(&api(108, 60, 4990), &db, "tok", t0 + 8 * D, false).unwrap();
        let now = t0 + 8 * D;
        let d = build_dashboard(&db.lock(), 7, now, None, SortKey::Stars)
            .unwrap()
            .unwrap();
        assert_eq!(d.kpis[0].deltas.d7d, Some(8));
        assert_eq!(d.kpis[0].deltas.d30d, None);
        assert_eq!(d.kpis[1].key, "stars");
        assert_eq!(d.kpis[1].value, 61);
        assert_eq!(d.rows[0].repo.name, "a");
        assert_eq!(d.rows[0].growth_7d, Some(10));
        assert_eq!(d.private_count, 1);
        assert_eq!(d.history_since_ms, Some(t0));
        // The range keeps only real points inside it.
        let d = build_dashboard(&db.lock(), 7, now, range_ms("7d"), SortKey::Stars)
            .unwrap()
            .unwrap();
        assert_eq!(d.followers, vec![(now, 108)]);
    }

    #[test]
    fn repo_detail_by_name_with_its_history() {
        let db = db();
        let t0 = 1_000 * D;
        refresh_with(&api(1, 50, 4990), &db, "tok", t0, false).unwrap();
        refresh_with(&api(1, 57, 4990), &db, "tok", t0 + 2 * D, false).unwrap();
        let d = build_repo_detail(&db.lock(), 7, "A", t0 + 2 * D, None)
            .unwrap()
            .unwrap();
        assert_eq!(d.stars, vec![(t0, 50), (t0 + 2 * D, 57)]);
        assert_eq!(d.star_deltas.d24h, Some(7));
        assert_eq!(d.star_deltas.d7d, None);
        assert_eq!(d.first_seen_ms, Some(t0));
        assert!(build_repo_detail(&db.lock(), 7, "nope", t0, None)
            .unwrap()
            .is_none());
    }

    #[test]
    fn export_leaves_private_repositories_out_unless_asked() {
        let db = db();
        refresh_with(&api(1, 5, 4990), &db, "tok", 1_000 * D, false).unwrap();
        let (csv, ext) = export_text(&db.lock(), 7, "octo", "csv", false, 1_000 * D).unwrap();
        assert_eq!(ext, "csv");
        assert!(csv.starts_with("timestamp,repository,stars,forks,watchers,open_issues\n"));
        assert!(csv.contains("octo/a") && !csv.contains("octo/b"));
        let (json, _) = export_text(&db.lock(), 7, "octo", "json", true, 1_000 * D).unwrap();
        assert!(json.contains("octo/b"));
        assert!(!json.contains("tok\""));
        assert!(export_text(&db.lock(), 7, "octo", "xml", false, 0).is_err());
    }

    #[test]
    fn ranges() {
        assert_eq!(range_ms("7D"), Some(7 * D));
        assert_eq!(range_ms("1y"), Some(365 * D));
        assert_eq!(range_ms("all"), None);
        assert_eq!(in_range(&[(0, 1), (10, 2)], 10, Some(5)), vec![(10, 2)]);
    }
}
