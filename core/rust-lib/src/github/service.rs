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
    cumulative, delta, gains_by_month, merge_history, parse_commit_activity,
    parse_languages, parse_releases, parse_traffic, plan_backfill, refresh_gate, sort_rows, window,
    EventKind, ExtraKind, Gate, Point, PollResult, RateLimit, ReleaseSummary, Repo, RepoNumbers,
    RepoRow, SortKey, Traffic, BACKFILL_MIN_RATE, BACKFILL_PAGE_BUDGET, DEFAULT_INTERVAL_MS,
    ERR_FORBIDDEN, ERR_NETWORK, ERR_NOT_CONNECTED, ERR_NOT_FOUND, EVENT_PAGE_SIZE, EXTRAS_TTL_MS,
    GROWTH_CHART_TOP, INTERVAL_CHOICES_H, MAX_EVENT_PAGES, WINDOWS,
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
/// Public by design — the device flow uses no client secret, so none is
/// stored anywhere. `IR_GITHUB_CLIENT_ID` overrides it for development.
const BUILTIN_CLIENT_ID: &str = "Ov23liC708D0C6jc8iH4";

/// Emitted whenever status or data changed (login, refresh, disconnect).
pub const EVENT: &str = "github-changed";
pub const ERR_NO_CLIENT_ID: &str = "github.no_client_id";
pub const ERR_BUSY: &str = "github.busy";

/// How often the background thread looks whether a refresh is due. The
/// interval itself (1–24 h) decides when GitHub is actually asked.
const BACKGROUND_TICK: Duration = Duration::from_secs(300);
const BACKGROUND_FIRST: Duration = Duration::from_secs(20);
/// After a network error the next automatic try comes this soon, not a
/// whole interval later.
pub const RETRY_AFTER_ERROR_MS: i64 = 5 * 60_000;
/// Watchers (single-repository endpoint) checked per refresh, and how old a
/// check may get before it's repeated.
pub const WATCHERS_PER_REFRESH: usize = 5;
pub const WATCHERS_RECHECK_MS: i64 = 86_400_000;

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
    pub watchers_checked: usize,
    pub backfill: BackfillRun,
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
            watchers_checked: 0,
            backfill: BackfillRun::default(),
        });
    }
    put(db, K_CHECKED, &now.to_string());
    let mut info = CallInfo::default();
    let fetched = client::fetch_account(t, token, &mut info)
        .and_then(|a| client::fetch_repos(t, token, &mut info).map(|r| (a, r)));
    // The rate limit is worth keeping even from a failed call.
    store_rate(db, info.rate);
    let (account, (repos, complete)) = match fetched {
        Ok(v) => v,
        Err(e) => {
            put(db, K_ERROR, &e);
            if e == ERR_NETWORK {
                // Offline for a moment: try again soon, not a full interval later.
                let retry_from = now - cfg.interval_ms() + RETRY_AFTER_ERROR_MS;
                put(db, K_CHECKED, &retry_from.min(now).to_string());
            }
            return Err(e);
        }
    };
    let recorded = {
        let mut conn = db.lock();
        store::record(&mut conn, &account, &repos, now, cfg.interval_ms(), complete)
    };
    let recorded = match recorded {
        Ok(r) => r,
        Err(e) => {
            put(db, K_ERROR, "github.db");
            return Err(e.to_string());
        }
    };
    put(db, K_USER, &account.id.to_string());
    put(db, K_LOGIN, &account.login);
    put(db, K_OK, &now.to_string());
    put(db, K_SCOPES, info.scopes.as_deref().unwrap_or(""));
    let _ = settings::delete(db, K_ERROR);
    // The fresh numbers are visible now; history and watchers follow.
    emit();
    let watchers = refresh_watchers(t, db, token, account.id, now);
    let backfill = backfill_with(t, db, token, account.id, now);
    Ok(RefreshResult {
        gate,
        fetched: true,
        account_snapshot: recorded.account_snapshot,
        repo_snapshots: recorded.repo_snapshots,
        watchers_checked: watchers,
        backfill,
    })
}

fn store_rate(db: &DbHandle, rate: Option<RateLimit>) {
    if let Some(r) = rate {
        put(db, K_RATE, &serde_json::to_string(&r).unwrap_or_default());
    }
}

fn rate_low(rate: Option<RateLimit>) -> bool {
    rate.is_some_and(|r| r.remaining < BACKFILL_MIN_RATE)
}

/// Check the watchers of a few repositories (the list endpoint lacks them);
/// a change becomes a snapshot. Stops at the first error.
fn refresh_watchers(t: &dyn Transport, db: &DbHandle, token: &str, user: i64, now: i64) -> usize {
    if rate_low(rate_of(db)) {
        return 0;
    }
    let due = store::watchers_due(&db.lock(), user, now, WATCHERS_RECHECK_MS, WATCHERS_PER_REFRESH)
        .unwrap_or_default();
    let mut info = CallInfo::default();
    let mut checked = 0;
    for (id, full_name) in due {
        match client::fetch_repo(t, token, &full_name, &mut info) {
            Ok(r) => {
                if let Some(w) = r.watchers {
                    let _ = store::record_watchers(&mut db.lock(), id, w, now);
                    checked += 1;
                }
            }
            Err(_) => break,
        }
        if rate_low(info.rate) {
            break;
        }
    }
    store_rate(db, info.rate);
    checked
}

/// What the history reconstruction did in one refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct BackfillRun {
    /// Lists (stars or forks of one repository) read completely and stored.
    pub lists: usize,
    /// Requests it took.
    pub pages: i64,
    /// Stopped because few requests are left.
    pub stopped_by_rate: bool,
}

/// Read the stargazer / fork lists the plan picks, within the page budget.
/// A list is stored only when read completely — an error or a rate-limit
/// stop drops the half-read list and keeps the previous one.
pub fn backfill_with(t: &dyn Transport, db: &DbHandle, token: &str, user: i64, now: i64) -> BackfillRun {
    let mut run = BackfillRun::default();
    if rate_low(rate_of(db)) {
        run.stopped_by_rate = true;
        return run;
    }
    let cands = store::backfill_candidates(&db.lock(), user).unwrap_or_default();
    let mut info = CallInfo::default();
    'lists: for c in plan_backfill(cands, now, BACKFILL_PAGE_BUDGET) {
        let mut times = Vec::new();
        let mut truncated = false;
        let mut page = 1;
        loop {
            let got = client::fetch_event_page(t, token, &c.full_name, c.kind, page, &mut info);
            run.pages += 1;
            let Ok(got) = got else {
                // A failing list (deleted meanwhile, network) — skip it.
                break;
            };
            let n = got.len() as i64;
            times.extend(got);
            if rate_low(info.rate) {
                run.stopped_by_rate = true;
                break 'lists;
            }
            if n < EVENT_PAGE_SIZE || times.len() as i64 >= c.value {
                let _ = store::replace_events(
                    &mut db.lock(),
                    c.repo_id,
                    c.kind,
                    &times,
                    c.value,
                    truncated,
                    now,
                );
                run.lists += 1;
                break;
            }
            if page >= MAX_EVENT_PAGES {
                // GitHub lists no further — keep what exists, flagged.
                truncated = true;
                let _ = store::replace_events(
                    &mut db.lock(),
                    c.repo_id,
                    c.kind,
                    &times,
                    c.value,
                    truncated,
                    now,
                );
                run.lists += 1;
                break;
            }
            page += 1;
        }
    }
    store_rate(db, info.rate);
    run
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
        name: profile.as_ref().and_then(|p| p.name.clone()),
        avatar_url: profile.and_then(|p| p.avatar_url),
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

/// A chart range in ms; `None` = everything known (since creation where the
/// history is reconstructed).
pub fn range_ms(name: &str) -> Option<i64> {
    const D: i64 = 86_400_000;
    match name.trim().to_ascii_lowercase().as_str() {
        "7d" => Some(7 * D),
        "30d" => Some(30 * D),
        "90d" => Some(90 * D),
        "1y" => Some(365 * D),
        "5y" => Some(5 * 365 * D),
        _ => None,
    }
}

/// 24h / 7d / 30d / 90d / 1y changes; None where the history is too short.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct Deltas {
    pub d24h: Option<i64>,
    pub d7d: Option<i64>,
    pub d30d: Option<i64>,
    pub d90d: Option<i64>,
    pub d1y: Option<i64>,
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
        d1y: w("1y"),
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

/// How far the history reconstruction got: lists (stars + forks per own
/// repository with a count > 0) read at least once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct BackfillStatus {
    pub done: usize,
    pub total: usize,
    /// Lists cut at GitHub's listing limit.
    pub truncated: usize,
    /// All star lists read — months before observation are real, not unknown.
    pub stars_complete: bool,
    /// All fork lists read.
    pub forks_complete: bool,
}

pub fn backfill_status(conn: &rusqlite::Connection, user_id: i64) -> rusqlite::Result<BackfillStatus> {
    let mut st = BackfillStatus {
        stars_complete: true,
        forks_complete: true,
        ..Default::default()
    };
    for c in store::backfill_candidates(conn, user_id)? {
        if c.value <= 0 {
            continue;
        }
        st.total += 1;
        match store::synced(conn, c.repo_id, c.kind)? {
            Some(s) => {
                st.done += 1;
                if s.truncated {
                    st.truncated += 1;
                }
            }
            None if c.kind == EventKind::Stars => st.stars_complete = false,
            None => st.forks_complete = false,
        }
    }
    Ok(st)
}

/// A month and what it gained (None = unknown, before the history).
pub type MonthGain = (i64, Option<i64>);

#[derive(Debug, Clone, Serialize)]
pub struct Dashboard {
    pub user_id: i64,
    pub profile: Option<store::Profile>,
    pub kpis: Vec<Kpi>,
    pub followers: Vec<Point>,
    pub stars: Vec<Point>,
    pub forks: Vec<Point>,
    pub repos: Vec<Point>,
    pub rows: Vec<RepoRow>,
    pub growth: Vec<GrowthLine>,
    /// First stored snapshot — real observation starts here.
    pub history_since_ms: Option<i64>,
    /// First point of the reconstructed history (first star / repository),
    /// when it reaches back further than the observation.
    pub reconstructed_from_ms: Option<i64>,
    pub snapshot_count: usize,
    pub private_count: usize,
    /// Stars gained per month, the last 12 months (UTC month starts).
    pub star_gains: Vec<MonthGain>,
    pub backfill: BackfillStatus,
}

fn pick(series: &[(i64, RepoNumbers)], f: fn(&RepoNumbers) -> i64) -> Vec<Point> {
    series.iter().map(|(t, n)| (*t, f(n))).collect()
}

/// A repository's stars or forks over its whole life: the stored event list
/// before the first snapshot, the snapshots after.
fn repo_history(
    conn: &rusqlite::Connection,
    repo_id: i64,
    kind: EventKind,
    snapshots: &[Point],
) -> rusqlite::Result<Vec<Point>> {
    let ev = store::events(conn, repo_id, kind)?;
    Ok(merge_history(&cumulative(&ev), snapshots))
}

/// Growth ranking for the multi-line chart: the most stars gained in 30 days,
/// unknown growth last, ties by stars — "growing", not "big".
fn by_growth(a: &(Option<i64>, i64, GrowthLine), b: &(Option<i64>, i64, GrowthLine)) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let g = match (a.0, b.0) {
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    g.then(b.1.cmp(&a.1)).then(a.2.name.cmp(&b.2.name))
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
    let repos: Vec<Repo> = store::current_repos(conn, user_id)?;
    let backfill = backfill_status(conn, user_id)?;
    // The account total is a SUM over repositories: with only some lists read
    // the reconstruction would sit far below the first snapshot and every
    // change across that edge would be a fake jump. Until all lists of a kind
    // are read, the account curve is snapshots only.
    let account_curve = |kind: EventKind, field: AccountField, complete: bool| -> rusqlite::Result<Vec<Point>> {
        let snaps = store::account_series(conn, user_id, field)?;
        if !complete {
            return Ok(snaps);
        }
        Ok(merge_history(&cumulative(&store::owner_events(conn, user_id, kind)?), &snaps))
    };
    let stars = account_curve(EventKind::Stars, AccountField::TotalStars, backfill.stars_complete)?;
    let forks = account_curve(EventKind::Forks, AccountField::TotalForks, backfill.forks_complete)?;
    let created: Vec<i64> = repos.iter().filter_map(|r| r.created_at_ms).collect();
    let repo_count = merge_history(
        &cumulative(&created),
        &store::account_series(conn, user_id, AccountField::RepoCount)?,
    );
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
    let private_count = repos.iter().filter(|r| r.private).count();
    let mut rows = Vec::with_capacity(repos.len());
    let mut lines = Vec::new();
    for r in repos {
        let snaps = pick(&store::repo_series(conn, r.id)?, |n| n.stars);
        let s = repo_history(conn, r.id, EventKind::Stars, &snaps)?;
        let g30 = delta(&s, now, 30 * 86_400_000);
        rows.push(RepoRow {
            growth_7d: delta(&s, now, 7 * 86_400_000),
            growth_30d: g30,
            repo: r.clone(),
        });
        lines.push((
            g30,
            r.stars,
            GrowthLine {
                id: r.id,
                name: r.name,
                points: window(&s, now, range),
            },
        ));
    }
    sort_rows(&mut rows, sort);
    lines.sort_by(by_growth);
    let growth: Vec<GrowthLine> = lines
        .into_iter()
        .filter(|l| !l.2.points.is_empty())
        .take(GROWTH_CHART_TOP)
        .map(|l| l.2)
        .collect();
    let reconstructed_from_ms = [stars.first(), repo_count.first()]
        .into_iter()
        .flatten()
        .map(|p| p.0)
        .min()
        .filter(|t| followers.first().is_none_or(|f| *t < f.0));
    Ok(Some(Dashboard {
        user_id,
        profile: store::account(conn, user_id)?,
        kpis,
        history_since_ms: followers.first().map(|p| p.0),
        reconstructed_from_ms,
        snapshot_count: followers.len(),
        star_gains: gains_by_month(&stars, now, 12, backfill.stars_complete),
        followers: window(&followers, now, range),
        stars: window(&stars, now, range),
        forks: window(&forks, now, range),
        repos: window(&repo_count, now, range),
        rows,
        growth,
        private_count,
        backfill,
    }))
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoDetail {
    pub repo: Repo,
    pub stars: Vec<Point>,
    pub forks: Vec<Point>,
    pub watchers: Vec<Point>,
    pub open_issues: Vec<Point>,
    pub star_deltas: Deltas,
    pub fork_deltas: Deltas,
    /// First stored snapshot — real observation starts here.
    pub first_seen_ms: Option<i64>,
    /// What the reconstruction read (None = not yet).
    pub stars_synced: Option<store::Synced>,
    pub forks_synced: Option<store::Synced>,
    /// Stars gained per month, the last 12 months.
    pub star_gains: Vec<MonthGain>,
}

/// A full reconstruction starts at zero at the repository's creation.
fn from_creation(mut series: Vec<Point>, created: Option<i64>, complete: bool) -> Vec<Point> {
    if let (true, Some(c)) = (complete, created) {
        if series.first().is_none_or(|p| p.0 > c) {
            series.insert(0, (c, 0));
        }
    }
    series
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
    let stars_synced = store::synced(conn, repo.id, EventKind::Stars)?;
    let forks_synced = store::synced(conn, repo.id, EventKind::Forks)?;
    let full = |s: &Option<store::Synced>| s.is_some_and(|s| !s.truncated);
    let stars = from_creation(
        repo_history(conn, repo.id, EventKind::Stars, &pick(&series, |n| n.stars))?,
        repo.created_at_ms,
        full(&stars_synced),
    );
    let forks = from_creation(
        repo_history(conn, repo.id, EventKind::Forks, &pick(&series, |n| n.forks))?,
        repo.created_at_ms,
        full(&forks_synced),
    );
    let watchers: Vec<Point> = series
        .iter()
        .filter_map(|(t, n)| n.watchers.map(|w| (*t, w)))
        .collect();
    let issues = pick(&series, |n| n.open_issues);
    Ok(Some(RepoDetail {
        star_deltas: deltas(&stars, now),
        fork_deltas: deltas(&forks, now),
        first_seen_ms: series.first().map(|s| s.0),
        star_gains: gains_by_month(&stars, now, 12, full(&stars_synced)),
        stars: window(&stars, now, range),
        forks: window(&forks, now, range),
        watchers: window(&watchers, now, range),
        open_issues: window(&issues, now, range),
        stars_synced,
        forks_synced,
        repo,
    }))
}

/// The detail view — from the local data only (watchers come from the
/// background refresh; nothing here waits for GitHub).
pub fn repo_detail(db: &DbHandle, query: &str, range: Option<i64>) -> Result<RepoDetail, String> {
    let user = user_id(db).ok_or_else(|| ERR_NOT_CONNECTED.to_string())?;
    build_repo_detail(&db.lock(), user, query, now_ms(), range)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| ERR_NOT_FOUND.to_string())
}

/// Extra data of the detail view, each part optional: a missing permission
/// (traffic needs push access) just leaves its card out.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct RepoExtras {
    /// Commits per week, last 52 weeks.
    pub commits: Option<Vec<Point>>,
    /// GitHub is still computing the statistics (202) — ask again shortly.
    pub commits_pending: bool,
    pub releases: Option<ReleaseSummary>,
    /// Bytes per language, largest first.
    pub languages: Option<Vec<(String, i64)>>,
    pub views: Option<Traffic>,
    pub clones: Option<Traffic>,
    /// Oldest fetch time among the parts shown.
    pub fetched_ms: Option<i64>,
}

fn apply_extra(out: &mut RepoExtras, kind: ExtraKind, json: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return;
    };
    match kind {
        ExtraKind::CommitActivity => out.commits = parse_commit_activity(&v),
        ExtraKind::Releases => out.releases = parse_releases(&v),
        ExtraKind::Languages => out.languages = parse_languages(&v),
        ExtraKind::TrafficViews => out.views = parse_traffic(&v, "views"),
        ExtraKind::TrafficClones => out.clones = parse_traffic(&v, "clones"),
    }
}

/// The extras: cached ones younger than a day are used as they are, older
/// or missing ones fetched when a token and requests are available. An
/// answer that says "not for you" (403/404) is cached as nothing for a day —
/// it would only fail again. Offline: whatever is cached.
pub fn repo_extras_with(
    t: &dyn Transport,
    db: &DbHandle,
    token: Option<&str>,
    query: &str,
    now: i64,
) -> Result<RepoExtras, String> {
    let user = user_id(db).ok_or_else(|| ERR_NOT_CONNECTED.to_string())?;
    let repo = store::find_repo(&db.lock(), user, query)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| ERR_NOT_FOUND.to_string())?;
    let limited = rate_of(db).is_some_and(|r| r.remaining <= 0 && now < r.reset_at_ms);
    let mut out = RepoExtras::default();
    let mut info = CallInfo::default();
    let mut online = true;
    for kind in ExtraKind::ALL {
        let cached = store::extra(&db.lock(), repo.id, kind).ok().flatten();
        let fresh = cached
            .as_ref()
            .is_some_and(|(at, _)| now - at < EXTRAS_TTL_MS);
        let mut json = cached.as_ref().map(|c| (c.0, c.1.clone()));
        if !fresh && online && !limited {
            if let Some(tok) = token {
                match client::fetch_extra(t, tok, &repo.full_name, kind, &mut info) {
                    Ok(client::ExtraAnswer::Data(v)) => {
                        let text = v.to_string();
                        let _ = store::put_extra(&db.lock(), repo.id, kind, &text, now);
                        json = Some((now, text));
                    }
                    Ok(client::ExtraAnswer::Pending) => out.commits_pending = true,
                    Err(e) if e == ERR_FORBIDDEN || e == ERR_NOT_FOUND => {
                        let _ = store::put_extra(&db.lock(), repo.id, kind, "null", now);
                        json = Some((now, "null".into()));
                    }
                    // Network or rate limit: keep the cache, stop asking.
                    Err(_) => online = false,
                }
            }
        }
        if let Some((at, text)) = json {
            apply_extra(&mut out, kind, &text);
            out.fetched_ms = Some(out.fetched_ms.map_or(at, |f| f.min(at)));
        }
    }
    store_rate(db, info.rate);
    Ok(out)
}

pub fn repo_extras(db: &DbHandle, query: &str) -> Result<RepoExtras, String> {
    let token = resolve_token().map(|(t, _)| t);
    repo_extras_with(&client::Ureq, db, token.as_deref(), query, now_ms())
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
    let profile = store::account(conn, user_id).map_err(e)?;
    let data = super::export::ExportData {
        login,
        profile: profile.as_ref(),
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
        // The range carries the value in at its start (a value holds until
        // it changes) and ends at now.
        let d = build_dashboard(&db.lock(), 7, now, range_ms("7d"), SortKey::Stars)
            .unwrap()
            .unwrap();
        assert_eq!(d.followers, vec![(now - 7 * D, 100), (now, 108)]);
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
        assert_eq!(range_ms("5y"), Some(5 * 365 * D));
    }

    const RL: [(&str, &str); 3] = [
        ("x-ratelimit-limit", "5000"),
        ("x-ratelimit-remaining", "4900"),
        ("x-ratelimit-reset", "2000000000"),
    ];

    fn star_page(times: &[&str]) -> String {
        serde_json::to_string(
            &times
                .iter()
                .map(|t| serde_json::json!({"starred_at": t, "user": {"login": "x"}}))
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }

    fn stars_url(repo: &str, page: i64) -> String {
        format!("{API}/repos/{repo}/stargazers?per_page=100&page={page}")
    }

    fn forks_url(repo: &str, page: i64) -> String {
        format!("{API}/repos/{repo}/forks?sort=oldest&per_page=100&page={page}")
    }

    #[test]
    fn a_refresh_reconstructs_the_star_history_with_the_star_media_type() {
        let db = db();
        let now = 20_000 * D; // 2024-10-03
        let fake = api(10, 3, 4990)
            .route(
                &stars_url("octo/a", 1),
                200,
                &star_page(&["2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z", "2022-01-01T00:00:00Z"]),
                &RL,
            )
            .route(&forks_url("octo/a", 1), 200, r#"[{"created_at":"2023-05-01T00:00:00Z"},{"created_at":"2024-01-01T00:00:00Z"}]"#, &RL)
            .route(&stars_url("octo/b", 1), 200, &star_page(&["2019-01-01T00:00:00Z"]), &RL);
        let r = refresh_with(&fake, &db, "tok", now, false).unwrap();
        assert_eq!(r.backfill.lists, 3, "stars + forks of a, stars of b (no forks)");
        // The stargazer request asked for the media type that carries `starred_at`.
        let calls = fake.calls.borrow();
        let (_, hs) = calls.iter().find(|(u, _)| u.contains("stargazers")).unwrap();
        assert!(hs.contains(&("Accept".into(), client::STAR_ACCEPT.into())));
        drop(calls);
        // The dashboard reaches back to the first star, not to today.
        let d = build_dashboard(&db.lock(), 7, now, None, SortKey::Stars).unwrap().unwrap();
        assert_eq!(d.stars.first().map(|p| p.1), Some(1));
        assert_eq!(d.stars.first().map(|p| p.0), Some(1_546_300_800_000), "b's star in 2019");
        assert!(d.reconstructed_from_ms.unwrap() < now - 1000 * D);
        assert_eq!(d.backfill, BackfillStatus { done: 3, total: 3, truncated: 0, stars_complete: true, forks_complete: true });
        // Year-over-year delta comes from the reconstruction (1 star in 2022→now).
        assert_eq!(d.kpis[1].deltas.d1y, Some(0));
        // Second refresh: nothing changed, nothing re-read.
        let fake2 = api(10, 3, 4990);
        let r = refresh_with(&fake2, &db, "tok", now + 2 * H, false).unwrap();
        assert_eq!(r.backfill.lists, 0);
        assert!(!fake2.calls.borrow().iter().any(|(u, _)| u.contains("stargazers")));
    }

    #[test]
    fn a_failing_or_rate_limited_list_is_never_stored_half() {
        let db = db();
        let now = 20_000 * D;
        // Stars of `a` are 150: page 1 full, page 2 errors → nothing stored.
        let full: Vec<String> = (0..100).map(|i| format!("2020-01-01T00:{:02}:{:02}Z", i / 60, i % 60)).collect();
        let full: Vec<&str> = full.iter().map(String::as_str).collect();
        let fake = api(1, 150, 4990)
            .route(&stars_url("octo/a", 1), 200, &star_page(&full), &RL)
            .route(&stars_url("octo/a", 2), 500, "", &RL);
        refresh_with(&fake, &db, "tok", now, false).unwrap();
        assert!(store::events(&db.lock(), 1, EventKind::Stars).unwrap().is_empty());
        assert!(store::synced(&db.lock(), 1, EventKind::Stars).unwrap().is_none());
        // With few requests left nothing is backfilled at all.
        let db = db_fresh();
        let fake = api(1, 5, 400).route(&stars_url("octo/a", 1), 200, &star_page(&["2020-01-01T00:00:00Z"]), &RL);
        let r = refresh_with(&fake, &db, "tok", now, false).unwrap();
        assert!(r.backfill.stopped_by_rate && r.backfill.lists == 0);
        assert!(!fake.calls.borrow().iter().any(|(u, _)| u.contains("stargazers")));
    }

    #[test]
    fn a_partial_reconstruction_never_bends_the_account_curve() {
        let db = db();
        let now = 20_000 * D;
        // Only a's stars are readable; b's list fails → the account total
        // would be 3 of 4 before the first snapshot: a fake +1 jump.
        let fake = api(10, 3, 4990).route(
            &stars_url("octo/a", 1),
            200,
            &star_page(&["2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z", "2022-01-01T00:00:00Z"]),
            &RL,
        );
        refresh_with(&fake, &db, "tok", now, false).unwrap();
        let d = build_dashboard(&db.lock(), 7, now, None, SortKey::Stars).unwrap().unwrap();
        assert!(!d.backfill.stars_complete);
        assert_eq!(d.stars, vec![(now, 4)], "snapshots only until every list is read");
        assert_eq!(d.kpis[1].deltas, Deltas::default());
        // The repository itself is complete and keeps its own history.
        let r = build_repo_detail(&db.lock(), 7, "a", now, None).unwrap().unwrap();
        assert_eq!(r.stars.len(), 4);
    }

    #[test]
    fn running_low_on_requests_mid_list_stops_without_storing_it() {
        let db = db();
        let low = [
            ("x-ratelimit-limit", "5000"),
            ("x-ratelimit-remaining", "450"),
            ("x-ratelimit-reset", "2000000000"),
        ];
        let fake = api(1, 5, 4990)
            .route(&stars_url("octo/a", 1), 200, &star_page(&["2020-01-01T00:00:00Z"]), &low)
            .route(&stars_url("octo/b", 1), 200, &star_page(&["2020-01-01T00:00:00Z"]), &RL);
        let r = refresh_with(&fake, &db, "tok", 20_000 * D, false).unwrap();
        assert!(r.backfill.stopped_by_rate);
        assert_eq!(r.backfill.lists, 0, "the list that hit the floor is dropped, the rest waits");
        assert!(store::synced(&db.lock(), 1, EventKind::Stars).unwrap().is_none());
        assert!(!fake.calls.borrow().iter().any(|(u, _)| u.contains("octo/b/stargazers")));
    }

    fn db_fresh() -> DbHandle {
        db()
    }

    #[test]
    fn the_repo_detail_starts_at_creation_and_holds_its_value() {
        let db = db();
        let now = 20_000 * D;
        let repos = r#"[{"id":1,"name":"a","full_name":"octo/a","stargazers_count":2,"forks_count":0,"created_at":"2019-06-01T00:00:00Z"}]"#;
        let fake = Fake::default()
            .route(&format!("{API}/user"), 200, r#"{"id":7,"login":"octo","followers":1}"#, &RL)
            .route(&repos_url(), 200, repos, &RL)
            .route(&stars_url("octo/a", 1), 200, &star_page(&["2020-01-01T00:00:00Z", "2021-01-01T00:00:00Z"]), &RL);
        refresh_with(&fake, &db, "tok", now, false).unwrap();
        let d = build_repo_detail(&db.lock(), 7, "a", now, None).unwrap().unwrap();
        assert_eq!(d.stars.first(), Some(&(1_559_347_200_000, 0)), "zero at creation");
        assert_eq!(d.stars.last(), Some(&(now, 2)));
        // A 30-day window over a quiet repository still has a (flat) line.
        let d = build_repo_detail(&db.lock(), 7, "a", now + 40 * D, range_ms("30d")).unwrap().unwrap();
        assert_eq!(d.stars, vec![(now + 10 * D, 2), (now + 40 * D, 2)]);
        assert!(d.stars_synced.is_some());
    }

    #[test]
    fn the_growth_chart_shows_who_grows_not_who_is_biggest() {
        let db = db();
        let t0 = 1_000 * D;
        let two = |a: i64, b: i64| {
            Fake::default()
                .route(&format!("{API}/user"), 200, r#"{"id":7,"login":"octo"}"#, &RL)
                .route(
                    &repos_url(),
                    200,
                    &format!(
                        r#"[{{"id":1,"name":"big","full_name":"octo/big","stargazers_count":{a},"fork":true}},
                            {{"id":2,"name":"rising","full_name":"octo/rising","stargazers_count":{b},"fork":true}}]"#
                    ),
                    &RL,
                )
        };
        refresh_with(&two(1000, 1), &db, "tok", t0, false).unwrap();
        refresh_with(&two(1000, 40), &db, "tok", t0 + 31 * D, false).unwrap();
        let d = build_dashboard(&db.lock(), 7, t0 + 31 * D, None, SortKey::Stars).unwrap().unwrap();
        assert_eq!(d.growth[0].name, "rising");
    }

    #[test]
    fn after_a_network_error_the_next_try_comes_in_minutes() {
        let db = db();
        let t0 = 1_000 * H;
        refresh_with(&api(1, 1, 4990), &db, "tok", t0, false).unwrap();
        let dead = Fake { offline: true, ..Default::default() };
        refresh_with(&dead, &db, "tok", t0 + 2 * H, false).unwrap_err();
        let gate = refresh_gate(t0 + 2 * H + 4 * 60_000, get_i64(&db, K_CHECKED), H, false, None);
        assert!(matches!(gate, Gate::TooSoon { .. }));
        let gate = refresh_gate(t0 + 2 * H + RETRY_AFTER_ERROR_MS, get_i64(&db, K_CHECKED), H, false, None);
        assert_eq!(gate, Gate::Go);
    }

    #[test]
    fn extras_are_cached_and_a_missing_permission_hides_its_card() {
        let db = db();
        refresh_with(&api(1, 1, 4990), &db, "tok", 1_000 * D, false).unwrap();
        let base = format!("{API}/repos/octo/a");
        let fake = || {
            Fake::default()
                .route(&format!("{base}/stats/commit_activity"), 202, "{}", &RL)
                .route(&format!("{base}/releases?per_page=100"), 200, "[]", &RL)
                .route(&format!("{base}/languages"), 200, r#"{"Rust":10}"#, &RL)
                .route(&format!("{base}/traffic/views"), 403, r#"{"message":"Must have push access"}"#, &RL)
                .route(&format!("{base}/traffic/clones"), 403, "{}", &RL)
        };
        let f = fake();
        let x = repo_extras_with(&f, &db, Some("tok"), "a", 1_000 * D).unwrap();
        assert!(x.commits_pending && x.commits.is_none());
        assert_eq!(x.languages, Some(vec![("Rust".into(), 10)]));
        assert_eq!(x.releases.unwrap().count, 0);
        assert!(x.views.is_none() && x.clones.is_none());
        assert_eq!(f.calls.borrow().len(), 5);
        // An hour later only the still-pending statistics are asked again.
        let f = fake();
        repo_extras_with(&f, &db, Some("tok"), "a", 1_000 * D + H).unwrap();
        let calls: Vec<String> = f.calls.borrow().iter().map(|c| c.0.clone()).collect();
        assert_eq!(calls, vec![format!("{base}/stats/commit_activity")]);
        // Offline and without a token: the cache, no request.
        let x = repo_extras_with(&Fake::default(), &db, None, "a", 1_000 * D + H).unwrap();
        assert_eq!(x.languages, Some(vec![("Rust".into(), 10)]));
    }

    /// Live check against the real account (uses the `gh` login; never runs
    /// in CI): `cargo test -p inspector-rust-core --lib github_live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn github_live() {
        let db = db();
        let token = crate::repo_clone::gh_token().expect("gh auth login first");
        let now = now_ms();
        let r = refresh_with(&client::Ureq, &db, &token, now, true).unwrap();
        println!("refresh: {r:?}");
        let user = user_id(&db).unwrap();
        let d = build_dashboard(&db.lock(), user, now, None, SortKey::Stars).unwrap().unwrap();
        let p = d.profile.clone().unwrap();
        println!("account created: {:?} · backfill {:?}", p.created_at_ms.map(super::super::export::iso), d.backfill);
        println!("stars since: {:?} · first {:?} · last {:?}", d.reconstructed_from_ms.map(super::super::export::iso), d.stars.first(), d.stars.last());
        println!("star kpi: {:?}", d.kpis[1]);
        let events = store::owner_events(&db.lock(), user, EventKind::Stars).unwrap().len();
        let fork_stars: i64 = d.rows.iter().filter(|r| r.repo.fork).map(|r| r.repo.stars).sum();
        println!("events {events} vs total {} (fork-repo stars {fork_stars})", d.kpis[1].value);
        println!("gains: {:?}", d.star_gains.iter().map(|g| g.1).collect::<Vec<_>>());
        let top = d.rows.iter().max_by_key(|r| r.repo.stars).unwrap().repo.name.clone();
        let x = repo_extras_with(&client::Ureq, &db, Some(&token), &top, now).unwrap();
        println!(
            "{top}: commits {:?} (pending {}), releases {:?}, languages {:?}, views {:?}",
            x.commits.as_ref().map(|c| c.len()),
            x.commits_pending,
            x.releases.as_ref().map(|r| (r.count, r.downloads)),
            x.languages.as_ref().map(|l| l.len()),
            x.views.as_ref().map(|v| v.count)
        );
        let det = build_repo_detail(&db.lock(), user, &top, now, None).unwrap().unwrap();
        println!("{top} stars first {:?} last {:?} synced {:?}", det.stars.first(), det.stars.last(), det.stars_synced);
    }
}
