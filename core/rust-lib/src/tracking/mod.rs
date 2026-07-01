//! Timesheet / time-tracking feature.
//!
//! Delivery is incremental (see `docs/timesheet.md`).
//! - `db`  — persistence (encrypted at rest).
//! - `os`  — per-OS frontmost-window + idle queries.
//! - this module — the tracker core: session state machine + the focus/idle
//!   loop that opens/closes intervals.
//
// Some persistence/IPC helpers are exercised only by tests until their UI
// consumers land in later delivery steps — allow dead_code meanwhile.
#![allow(dead_code)]

pub mod bridge;
pub mod claude;
pub mod db;
pub mod export;
pub mod extension;
pub mod os;

use crate::db::DbHandle;
use db as tdb;
use os::FocusInfo;
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const TICK_MS: u64 = 1500;
const DEFAULT_IDLE_SECONDS: f64 = 300.0;

/// Runtime tracker state (managed Tauri state). The focus/idle loop thread and
/// the IPC commands share this behind a lock.
pub struct TrackerState(pub Arc<Mutex<Runtime>>);

impl Default for TrackerState {
    fn default() -> Self {
        TrackerState(Arc::new(Mutex::new(Runtime::default())))
    }
}

#[derive(Default)]
pub struct Runtime {
    pub session_id: Option<i64>,
    pub session_started: Option<i64>,
    /// True while idle-auto-paused.
    pub paused: bool,
    /// Last seen frontmost app (for status + labelling idle spans).
    pub active_app: Option<String>,
    open_event_id: Option<i64>,
    open_key: Option<String>,
    open_started_at: i64,
    /// True when the open interval matched the privacy denylist (its key is the
    /// collapsed `app\0\0source` form). Blocks in-place tab enrichment — denied
    /// time must never be retroactively attributed to the next tab's host.
    open_was_denied: bool,
    stop: Option<Arc<AtomicBool>>,
    claude_stop: Option<Arc<AtomicBool>>,
    bridge_stop: Option<Arc<AtomicBool>>,
    /// Most recent active browser tab reported by the extension (loopback WS).
    /// Used to enrich the open interval while a browser is frontmost.
    pub last_tab: Option<TabInfo>,
}

/// The active browser tab, pushed by the extension over the loopback bridge.
#[derive(Debug, Clone, Default)]
pub struct TabInfo {
    pub host: Option<String>,
    pub title: Option<String>,
    pub url: Option<String>,
}

/// Is `app` a web browser (→ enrich its interval with the reported tab)?
pub fn is_browser(app: &str) -> bool {
    let a = app.to_ascii_lowercase();
    ["chrome", "chromium", "safari", "firefox", "edge", "arc", "brave", "vivaldi", "opera"]
        .iter()
        .any(|b| a.contains(b))
}

/// Parse the comma/newline-separated `track.denylist` setting into lowercase
/// patterns (app names or hostnames). Pure + tested.
pub fn parse_denylist(raw: &str) -> Vec<String> {
    raw.split([',', '\n'])
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn load_denylist(db: &DbHandle) -> Vec<String> {
    crate::settings::get_or(db, "track.denylist", "")
        .map(|s| parse_denylist(&s))
        .unwrap_or_default()
}

/// Does the desired interval match the privacy denylist (by app, host, or url)?
fn is_denied(d: &Desired, denylist: &[String]) -> bool {
    if denylist.is_empty() {
        return false;
    }
    let app = d.app.to_ascii_lowercase();
    let host = d.host.as_deref().unwrap_or("").to_ascii_lowercase();
    let url = d.url.as_deref().unwrap_or("").to_ascii_lowercase();
    denylist.iter().any(|p| {
        app.contains(p.as_str())
            || (!host.is_empty() && host.contains(p.as_str()))
            || (!url.is_empty() && url.contains(p.as_str()))
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackStatus {
    pub active: bool,
    pub session_id: Option<i64>,
    pub paused: bool,
    pub since: Option<i64>,
    pub active_app: Option<String>,
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// ── Public API (driven by IPC) ───────────────────────────────────────────────

pub fn start(
    app: &AppHandle,
    db: &DbHandle,
    state: &TrackerState,
    label: Option<String>,
) -> Result<i64, String> {
    let mut rt = state.0.lock();
    if rt.session_id.is_some() {
        return Err("already tracking".into());
    }
    let now = now_ms();
    prune_by_retention(db, now);
    let sid = tdb::start_session(db, label.as_deref(), now).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    // Claude-Code watcher (default on; settings-gated) — its own stop flag.
    let claude_stop = if crate::settings::get_or(db, "track.claude_watcher", "1")
        .map(|v| v != "0")
        .unwrap_or(true)
    {
        Some(claude::start(db.clone(), sid))
    } else {
        None
    };
    // Browser bridge: loopback WS the extension reports the active tab to.
    let bridge_stop = bridge::start(db.clone(), state.0.clone());
    *rt = Runtime {
        session_id: Some(sid),
        session_started: Some(now),
        stop: Some(stop.clone()),
        claude_stop,
        bridge_stop: Some(bridge_stop),
        ..Default::default()
    };
    drop(rt);

    let (a, d, s) = (app.clone(), db.clone(), state.0.clone());
    std::thread::spawn(move || run_loop(a, d, s, sid, stop));
    let _ = app.emit("track-status-changed", ());
    Ok(sid)
}

/// Restore the last tracking state at startup: if a session wasn't cleanly
/// ended (status != "ended"), re-arm the focus loop + Claude watcher + bridge on
/// that **same** session so recording continues across an app restart/update.
/// Any dangling open event is finalized (heartbeat already ended it at the
/// last-alive tick), so the offline gap isn't counted; the loop opens fresh
/// events from now. No-op if nothing was active. Called once from `lib.rs`.
pub fn resume_if_active(app: &AppHandle, db: &DbHandle, state: &TrackerState) {
    // Recover from any unclean shutdown FIRST: close every dangling open event
    // (a still-NULL `ended_at` would otherwise count to *now* and overlap every
    // later event — the "today shows >2h after 1h" bug). Heartbeats keep live
    // events stamped, so this only catches truly-orphaned ones.
    let _ = tdb::finalize_all_open_events(db);
    // Retention must also apply on the resume path — with keep-alive on, a user
    // may never run `track on` again, and pruning would otherwise never fire.
    prune_by_retention(db, now_ms());

    let session = match tdb::active_session(db) {
        Ok(Some(s)) => s,
        _ => {
            // Nothing to resume — still end any stale "active" sessions so they
            // don't linger.
            let _ = tdb::end_stale_sessions(db, None);
            return;
        }
    };
    let mut rt = state.0.lock();
    if rt.session_id.is_some() {
        return; // already running (shouldn't happen at startup)
    }
    let sid = session.id;
    // End all OTHER non-ended sessions (stale duplicates from older builds).
    let _ = tdb::end_stale_sessions(db, Some(sid));
    // A resumed paused/active session is treated as active again.
    let _ = tdb::set_session_status(db, sid, "active");

    let stop = Arc::new(AtomicBool::new(false));
    let claude_stop = if crate::settings::get_or(db, "track.claude_watcher", "1")
        .map(|v| v != "0")
        .unwrap_or(true)
    {
        Some(claude::start(db.clone(), sid))
    } else {
        None
    };
    let bridge_stop = bridge::start(db.clone(), state.0.clone());
    *rt = Runtime {
        session_id: Some(sid),
        session_started: Some(session.started_at),
        stop: Some(stop.clone()),
        claude_stop,
        bridge_stop: Some(bridge_stop),
        ..Default::default()
    };
    drop(rt);

    let (a, d, s) = (app.clone(), db.clone(), state.0.clone());
    std::thread::spawn(move || run_loop(a, d, s, sid, stop));
    let _ = app.emit("track-status-changed", ());
    tracing::info!("timesheet: resumed active session {sid}");
}

/// Enforce `track.retention_days` (0 = keep forever): prune events + empty
/// sessions older than the cutoff. Called on `track on`, on resume, and hourly
/// from the run loop (so a never-restarted keep-alive session still prunes).
fn prune_by_retention(db: &DbHandle, now: i64) {
    if let Ok(days) = crate::settings::get_or(db, "track.retention_days", "0") {
        if let Ok(d) = days.parse::<i64>() {
            if d > 0 {
                let cutoff = now - d * 86_400_000;
                if let Err(e) = tdb::prune_before(db, cutoff) {
                    tracing::warn!("timesheet: retention prune failed: {e:#}");
                }
            }
        }
    }
}

/// The id of the currently-growing (live) event, if any. Used by the IPC layer
/// to protect the live row from delete/merge/cleanup — the run loop's heartbeat
/// writes to this id, and removing the row would silently stop persisting all
/// further time in the current focus span.
pub fn live_event_id(state: &TrackerState) -> Option<i64> {
    state.0.lock().open_event_id
}

pub fn stop(app: &AppHandle, db: &DbHandle, state: &TrackerState) -> Result<(), String> {
    let mut rt = state.0.lock();
    let Some(sid) = rt.session_id else {
        return Ok(());
    };
    if let Some(s) = &rt.stop {
        s.store(true, Ordering::SeqCst);
    }
    if let Some(s) = &rt.claude_stop {
        s.store(true, Ordering::SeqCst);
    }
    if let Some(s) = &rt.bridge_stop {
        s.store(true, Ordering::SeqCst);
    }
    let now = now_ms();
    if let Some(open) = rt.open_event_id.take() {
        let _ = tdb::close_event(db, open, now);
    }
    let _ = tdb::end_session(db, sid, now);
    *rt = Runtime::default();
    drop(rt);
    let _ = app.emit("track-status-changed", ());
    Ok(())
}

pub fn status(state: &TrackerState) -> TrackStatus {
    let rt = state.0.lock();
    TrackStatus {
        active: rt.session_id.is_some(),
        session_id: rt.session_id,
        paused: rt.paused,
        since: rt.session_started,
        active_app: rt.active_app.clone(),
    }
}

// ── The focus / idle loop ────────────────────────────────────────────────────

fn run_loop(app: AppHandle, db: DbHandle, rt: Arc<Mutex<Runtime>>, sid: i64, stop: Arc<AtomicBool>) {
    let mut last_prune = std::time::Instant::now();
    while !stop.load(Ordering::SeqCst) {
        // Hourly retention enforcement — a keep-alive session may run for weeks
        // without ever passing through `start()` again.
        if last_prune.elapsed().as_secs() >= 3600 {
            last_prune = std::time::Instant::now();
            prune_by_retention(&db, now_ms());
        }
        let threshold = crate::settings::get_or(&db, "track.idle_seconds", "300")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| *v > 0.0)
            .unwrap_or(DEFAULT_IDLE_SECONDS);
        let denylist = load_denylist(&db);
        let focus = os::frontmost();
        let idle_s = os::idle_seconds().unwrap_or(0.0);
        let now = now_ms();

        let (paused_changed, heartbeat_id) = {
            let mut g = rt.lock();
            if g.session_id != Some(sid) {
                break; // session ended / replaced
            }
            let was_paused = g.paused;
            apply_tick(&db, &mut g, sid, Tick {
                now,
                focus,
                idle_s,
                threshold_s: threshold,
                denylist: &denylist,
            });
            (g.paused != was_paused, g.open_event_id)
        };
        // Heartbeat the open event so a crash/quit leaves it ended at the last
        // live tick — the offline gap is never recorded as phantom usage, which
        // is what lets `resume_if_active` pick up cleanly after a restart.
        if let Some(id) = heartbeat_id {
            let _ = tdb::touch_event(&db, id, now);
        }
        if paused_changed {
            let _ = app.emit("track-status-changed", ());
        }

        // Responsive sleep so `stop` is honoured quickly.
        let mut slept = 0u64;
        while slept < TICK_MS && !stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(150));
            slept += 150;
        }
    }
}

/// Inputs sampled once per tick (bundled to keep `apply_tick`'s arity sane).
struct Tick<'a> {
    now: i64,
    focus: Option<FocusInfo>,
    idle_s: f64,
    threshold_s: f64,
    denylist: &'a [String],
}

/// The pure-ish per-tick state machine (no `AppHandle`, so it's unit-testable
/// against an in-memory DB): decide the *desired* interval and, on change,
/// close the open one + open the new. Idle transitions are retroactive — the
/// idle interval starts at the moment input actually stopped.
fn apply_tick(db: &DbHandle, rt: &mut Runtime, sid: i64, t: Tick) {
    let Tick { now, focus, idle_s, threshold_s, denylist } = t;
    let idle = threshold_s > 0.0 && idle_s >= threshold_s;

    // Desired interval for this tick (None = no info & not idle → keep current).
    let desired: Option<Desired> = if idle {
        let app = rt
            .active_app
            .clone()
            .or_else(|| focus.as_ref().map(|f| f.app_name.clone()))
            .unwrap_or_else(|| "Idle".to_string());
        let begin = now - (idle_s * 1000.0) as i64;
        Some(Desired {
            key: "\u{0}idle".to_string(),
            app,
            app_id: None,
            title: None,
            host: None,
            url: None,
            source: "focus".to_string(),
            is_idle: true,
            started_at: begin,
        })
    } else if let Some(f) = &focus {
        rt.active_app = Some(f.app_name.clone());
        if is_browser(&f.app_name) {
            // Browser frontmost → enrich with the extension-reported tab; the
            // tab URL is part of the key so a tab change splits the interval.
            let tab = rt.last_tab.clone().unwrap_or_default();
            Some(Desired {
                key: format!("{}\u{0}{}\u{0}browser", f.app_name, tab.url.clone().unwrap_or_default()),
                app: f.app_name.clone(),
                app_id: f.app_id.clone(),
                title: tab.title.clone().or_else(|| f.window_title.clone()),
                host: tab.host.clone(),
                url: tab.url.clone(),
                source: "browser".to_string(),
                is_idle: false,
                started_at: now,
            })
        } else {
            Some(Desired {
                key: format!(
                    "{}\u{0}{}\u{0}focus",
                    f.app_name,
                    f.window_title.as_deref().unwrap_or("")
                ),
                app: f.app_name.clone(),
                app_id: f.app_id.clone(),
                title: f.window_title.clone(),
                host: None,
                url: None,
                source: "focus".to_string(),
                is_idle: false,
                started_at: now,
            })
        }
    } else {
        None
    };

    let Some(mut d) = desired else {
        return;
    };
    // Privacy denylist: for matching apps/hosts/urls, keep only the app + time —
    // strip title/url/host and collapse the key so tabs don't even split by URL.
    let denied = !d.is_idle && is_denied(&d, denylist);
    if denied {
        d.title = None;
        d.host = None;
        d.url = None;
        d.key = format!("{}\u{0}\u{0}{}", d.app, d.source);
    }
    if rt.open_key.as_deref() == Some(d.key.as_str()) {
        return; // unchanged → leave the interval open
    }

    // Browser tab info usually arrives a tick AFTER the browser interval opened
    // (MV3 worker wake + WS round trip): the interval starts with empty
    // host/url, and the late report would then SPLIT it — a tiny "(unknown)"
    // fragment on every browser refocus. Instead, enrich the *young* open
    // interval in place (same app, tab info newly arrived). Denied intervals
    // are never enriched (their time must not be attributed to the next tab).
    const ENRICH_WITHIN_MS: i64 = 15_000;
    if !d.is_idle
        && d.source == "browser"
        && d.url.is_some()
        && !rt.open_was_denied
        && rt.open_key.as_deref() == Some(format!("{}\u{0}\u{0}browser", d.app).as_str())
        && now - rt.open_started_at <= ENRICH_WITHIN_MS
    {
        if let Some(open_id) = rt.open_event_id {
            let _ =
                tdb::enrich_event(db, open_id, d.host.as_deref(), d.title.as_deref(), d.url.as_deref());
            rt.open_key = Some(d.key);
            return;
        }
    }

    // Transition: close the open interval (clamped so it can't end before it
    // started), then open the new one.
    let close_at = d.started_at.max(rt.open_started_at);
    if let Some(open_id) = rt.open_event_id.take() {
        let _ = tdb::close_event(db, open_id, close_at);
    }
    // Auto-categorize from the saved app→category rule (idle spans stay
    // uncategorized; they're not "work").
    let category = if d.is_idle {
        None
    } else {
        tdb::category_for_app(db, &d.app).ok().flatten()
    };
    let ne = tdb::NewEvent {
        session_id: sid,
        app_name: d.app.clone(),
        app_id: d.app_id.clone(),
        window_title: d.title.clone(),
        url: d.url.clone(),
        host: d.host.clone(),
        category,
        project: None,
        source: d.source.clone(),
        is_idle: d.is_idle,
        started_at: d.started_at,
    };
    rt.open_event_id = tdb::open_event(db, &ne).ok();
    rt.open_key = Some(d.key);
    rt.open_started_at = d.started_at;
    rt.open_was_denied = denied;
    rt.paused = d.is_idle;
}

struct Desired {
    key: String,
    app: String,
    app_id: Option<String>,
    title: Option<String>,
    host: Option<String>,
    url: Option<String>,
    source: String,
    is_idle: bool,
    started_at: i64,
}

// ── Day report (aggregations for the Timesheet tab + export) ─────────────────

#[derive(Debug, Clone, Serialize)]
pub struct Bucket {
    pub key: String,
    pub seconds: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClaudeAgg {
    pub project: String,
    pub seconds: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
}

/// One detail line within an app's usage (a visited host for browsers, a window
/// title otherwise), grouped + summed.
#[derive(Debug, Clone, Serialize)]
pub struct AppDetail {
    pub label: String,
    pub seconds: i64,
    pub count: i64,
}

/// Per-app usage breakdown — total time + an expandable detail list (browser
/// history for browsers, window-title history for other apps). Shown as the
/// grouped "By app" view in the tab + HTML export.
#[derive(Debug, Clone, Serialize)]
pub struct AppBreakdown {
    pub app: String,
    pub seconds: i64,
    /// `"browser"` → details are hosts; otherwise window titles.
    pub source: String,
    /// The app's current category (from its events), if any.
    pub category: Option<String>,
    pub details: Vec<AppDetail>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayReport {
    pub date: String,
    pub events: Vec<tdb::TrackEvent>,
    pub total_active_s: i64,
    pub total_idle_s: i64,
    pub session_count: i64,
    /// Longest uninterrupted active run (no idle break), seconds.
    pub longest_focus_s: i64,
    /// Number of active focus segments (≈ context switches).
    pub focus_segments: i64,
    pub by_app: Vec<Bucket>,
    pub by_category: Vec<Bucket>,
    pub by_host: Vec<Bucket>,
    /// Time per **project** tag (manual entries + Claude projects). A cross-cut
    /// that may overlap the active total (Claude runs alongside terminal focus).
    pub by_project: Vec<Bucket>,
    /// Claude-Code usage per project (time + tokens) — a separate dimension,
    /// **not** included in `total_active_s`/`by_app` (those are focus/browser,
    /// to avoid double-counting time you spent in the terminal *and* Claude).
    pub claude: Vec<ClaudeAgg>,
    /// Per-app usage with an expandable detail list (browser → hosts; others →
    /// window titles). A grouped view of the same active time in `by_app`/
    /// `total_active_s` (not double-counted); idle + Claude are excluded.
    pub app_breakdown: Vec<AppBreakdown>,
}

/// Total seconds covered by the union of `intervals` (ms, may overlap). Merges
/// overlapping/adjacent ranges so the result never exceeds real elapsed time.
fn union_seconds(mut intervals: Vec<(i64, i64)>) -> i64 {
    if intervals.is_empty() {
        return 0;
    }
    intervals.sort_by_key(|iv| iv.0);
    let mut total_ms = 0i64;
    let (mut cur_s, mut cur_e) = intervals[0];
    for &(s, e) in &intervals[1..] {
        if s > cur_e {
            total_ms += cur_e - cur_s;
            cur_s = s;
            cur_e = e;
        } else if e > cur_e {
            cur_e = e;
        }
    }
    total_ms += cur_e - cur_s;
    total_ms / 1000
}

/// Longest single contiguous run (merging touching/overlapping intervals), in
/// seconds — i.e. the longest uninterrupted span with no gap. `intervals` in ms.
fn longest_run_seconds(mut intervals: Vec<(i64, i64)>) -> i64 {
    if intervals.is_empty() {
        return 0;
    }
    intervals.sort_by_key(|iv| iv.0);
    let mut best = 0i64;
    let (mut cur_s, mut cur_e) = intervals[0];
    for &(s, e) in &intervals[1..] {
        if s > cur_e {
            best = best.max(cur_e - cur_s);
            cur_s = s;
            cur_e = e;
        } else if e > cur_e {
            cur_e = e;
        }
    }
    best = best.max(cur_e - cur_s);
    best / 1000
}

/// Local-day `[midnight, next-midnight)` in unix ms for a `"YYYY-MM-DD"`.
///
/// The upper bound is the **next day's local midnight**, not `from + 24 h` — a
/// DST transition day is 23 or 25 real hours, and a fixed offset would double-
/// count (spring) or drop (fall) an hour between adjacent days' reports.
/// Invariant: `day_bounds(d).1 == day_bounds(d + 1).0` for every date.
pub fn day_bounds(date: &str) -> Result<(i64, i64), String> {
    use chrono::{Local, NaiveDate, TimeZone};
    // Local-midnight resolver, robust across DST: `.earliest()` picks the first
    // valid instant when midnight is ambiguous (fall-back) and, when midnight
    // itself is skipped (spring-forward at 00:00, e.g. historic Brazil), falls
    // back to 01:00 of the same day.
    let local_midnight = |nd: NaiveDate| -> Result<i64, String> {
        let mid = nd.and_hms_opt(0, 0, 0).ok_or_else(|| "invalid date".to_string())?;
        if let Some(dt) = Local.from_local_datetime(&mid).earliest() {
            return Ok(dt.timestamp_millis());
        }
        let one = nd.and_hms_opt(1, 0, 0).ok_or_else(|| "invalid date".to_string())?;
        Local
            .from_local_datetime(&one)
            .earliest()
            .map(|dt| dt.timestamp_millis())
            .ok_or_else(|| "unrepresentable local date".to_string())
    };
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|e| format!("bad date: {e}"))?;
    let next = d.succ_opt().ok_or_else(|| "date overflow".to_string())?;
    Ok((local_midnight(d)?, local_midnight(next)?))
}

fn to_buckets(map: std::collections::HashMap<String, i64>) -> Vec<Bucket> {
    let mut v: Vec<Bucket> = map
        .into_iter()
        .map(|(key, seconds)| Bucket { key, seconds })
        .collect();
    v.sort_by(|a, b| b.seconds.cmp(&a.seconds).then(a.key.cmp(&b.key)));
    v
}

/// Build the report for a local calendar day (`"YYYY-MM-DD"`): events
/// overlapping the day, totals (active vs idle), session count, and
/// app/category/host breakdowns (active time only). Open events count up to
/// "now". Pure aggregation over `events_in_range` — unit-tested.
pub fn day_report(db: &DbHandle, date: &str) -> Result<DayReport, String> {
    let (from, to) = day_bounds(date)?;
    let events = tdb::events_in_range(db, from, to).map_err(|e| e.to_string())?;
    let mut report = aggregate_day(date.to_string(), events, from, to, now_ms());
    // Merge token totals (per project) onto the time-only claude aggregation.
    if let Ok(tokens) = tdb::claude_tokens_by_project(db, from, to) {
        for c in &mut report.claude {
            if let Some((tin, tout)) = tokens.get(&c.project) {
                c.tokens_in = *tin;
                c.tokens_out = *tout;
            }
        }
    }
    Ok(report)
}

// ── Range / week report (multi-day overview) ─────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct DaySummary {
    pub date: String,
    pub active_s: i64,
    pub idle_s: i64,
    /// Active seconds per local hour 0..23 (union-correct) — for the heatmap.
    pub hours: Vec<i64>,
    /// This day's category breakdown — for the stacked daily bars.
    pub by_category: Vec<Bucket>,
}

/// Active seconds per local hour [0,24) for a day, from `events` clipped to
/// `[day_from, day_to)`. Unions overlapping active intervals first, so a leftover
/// open/overlapping event can't inflate an hour. Pure + unit-tested.
fn hour_buckets(events: &[tdb::TrackEvent], day_from: i64, day_to: i64, now: i64) -> Vec<i64> {
    let mut iv: Vec<(i64, i64)> = events
        .iter()
        .filter(|e| !e.is_idle && e.source != "claude")
        .filter_map(|e| {
            let s = e.started_at.max(day_from);
            let en = e.ended_at.unwrap_or(now).min(day_to);
            (en > s).then_some((s, en))
        })
        .collect();
    iv.sort_by_key(|x| x.0);
    // Merge overlaps.
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (s, e) in iv {
        if let Some(last) = merged.last_mut() {
            if s <= last.1 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }
    let mut hours = vec![0i64; 24];
    for (s, e) in merged {
        for (h, slot) in hours.iter_mut().enumerate() {
            let hs = day_from + h as i64 * 3_600_000;
            let he = hs + 3_600_000;
            let overlap = e.min(he) - s.max(hs);
            if overlap > 0 {
                *slot += overlap / 1000;
            }
        }
    }
    hours
}

#[derive(Debug, Clone, Serialize)]
pub struct RangeReport {
    pub from: String,
    pub to: String,
    /// Per-day active/idle (chronological) for the bar chart.
    pub days: Vec<DaySummary>,
    pub total_active_s: i64,
    pub total_idle_s: i64,
    pub by_category: Vec<Bucket>,
    pub by_app: Vec<Bucket>,
    pub by_project: Vec<Bucket>,
}

/// Aggregate the inclusive local-day range `[from_date, to_date]` (each
/// `"YYYY-MM-DD"`): per-day active/idle plus overall category/app/project
/// breakdowns. Reuses `aggregate_day` per day so totals stay union-correct.
pub fn range_report(db: &DbHandle, from_date: &str, to_date: &str) -> Result<RangeReport, String> {
    use chrono::{Duration, NaiveDate};
    let start = NaiveDate::parse_from_str(from_date, "%Y-%m-%d").map_err(|e| format!("bad date: {e}"))?;
    let end = NaiveDate::parse_from_str(to_date, "%Y-%m-%d").map_err(|e| format!("bad date: {e}"))?;
    if end < start {
        return Err("range end before start".into());
    }
    let now = now_ms();
    let mut days = Vec::new();
    let (mut total_active, mut total_idle) = (0i64, 0i64);
    let mut by_cat: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut by_app: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut by_project: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut d = start;
    while d <= end {
        let ds = d.format("%Y-%m-%d").to_string();
        let (from, to) = day_bounds(&ds)?;
        let events = tdb::events_in_range(db, from, to).map_err(|e| e.to_string())?;
        let hours = hour_buckets(&events, from, to, now);
        let r = aggregate_day(ds.clone(), events, from, to, now);
        total_active += r.total_active_s;
        total_idle += r.total_idle_s;
        for b in &r.by_category {
            *by_cat.entry(b.key.clone()).or_default() += b.seconds;
        }
        for b in &r.by_app {
            *by_app.entry(b.key.clone()).or_default() += b.seconds;
        }
        for b in &r.by_project {
            *by_project.entry(b.key.clone()).or_default() += b.seconds;
        }
        days.push(DaySummary {
            date: ds,
            active_s: r.total_active_s,
            idle_s: r.total_idle_s,
            hours,
            by_category: r.by_category,
        });
        d += Duration::days(1);
    }
    Ok(RangeReport {
        from: from_date.to_string(),
        to: to_date.to_string(),
        days,
        total_active_s: total_active,
        total_idle_s: total_idle,
        by_category: to_buckets(by_cat),
        by_app: to_buckets(by_app),
        by_project: to_buckets(by_project),
    })
}

/// Pure aggregation core (no DB/clock) for testability.
fn aggregate_day(
    date: String,
    events: Vec<tdb::TrackEvent>,
    from: i64,
    to: i64,
    now: i64,
) -> DayReport {
    use std::collections::{HashMap, HashSet};
    // Totals are the UNION of intervals (merged), not the raw sum — so even if
    // events overlap in time (e.g. a leftover open event from a crashed run),
    // the headline can never exceed real wall-clock. Per-app/category/host stay
    // raw sums (an app rarely overlaps itself).
    let mut active_iv: Vec<(i64, i64)> = Vec::new();
    let mut idle_iv: Vec<(i64, i64)> = Vec::new();
    let mut sessions: HashSet<i64> = HashSet::new();
    let mut by_app: HashMap<String, i64> = HashMap::new();
    let mut by_host: HashMap<String, i64> = HashMap::new();
    let mut by_cat: HashMap<String, i64> = HashMap::new();
    let mut by_project: HashMap<String, i64> = HashMap::new();
    let mut claude_secs: HashMap<String, i64> = HashMap::new();
    // Per app → (total seconds, source, category, detail label → (seconds, count)).
    type DetailStats = HashMap<String, (i64, i64)>;
    let mut apps_detail: HashMap<String, (i64, String, Option<String>, DetailStats)> = HashMap::new();
    for e in &events {
        sessions.insert(e.session_id);
        let end = e.ended_at.unwrap_or(now).min(to);
        let start = e.started_at.max(from);
        let dur = ((end - start).max(0)) / 1000;
        if dur == 0 {
            continue;
        }
        // Project tag cross-cut (manual + Claude + any tagged event), incl. claude.
        if !e.is_idle {
            if let Some(p) = &e.project {
                if !p.is_empty() {
                    *by_project.entry(p.clone()).or_default() += dur;
                }
            }
        }
        if e.source == "claude" {
            // Separate dimension — not part of the focus/browser active total
            // (you're also focused in the terminal during that time).
            let proj = e.project.clone().unwrap_or_else(|| "(unknown)".to_string());
            *claude_secs.entry(proj).or_default() += dur;
            continue;
        }
        if e.is_idle {
            idle_iv.push((start, end));
        } else {
            active_iv.push((start, end));
            *by_app.entry(e.app_name.clone()).or_default() += dur;
            if let Some(h) = &e.host {
                *by_host.entry(h.clone()).or_default() += dur;
            }
            let cat = e.category.clone().unwrap_or_else(|| "Uncategorized".to_string());
            *by_cat.entry(cat).or_default() += dur;
            // Per-app detail: browsers group by host, other apps by window title.
            let label = if e.source == "browser" {
                e.host
                    .clone()
                    .or_else(|| e.window_title.clone())
                    .unwrap_or_else(|| "(unknown)".to_string())
            } else {
                e.window_title.clone().unwrap_or_else(|| "(no title)".to_string())
            };
            let group = apps_detail
                .entry(e.app_name.clone())
                .or_insert_with(|| (0, e.source.clone(), None, HashMap::new()));
            group.0 += dur;
            if group.2.is_none() {
                if let Some(c) = &e.category {
                    if !c.is_empty() {
                        group.2 = Some(c.clone());
                    }
                }
            }
            let det = group.3.entry(label).or_insert((0, 0));
            det.0 += dur;
            det.1 += 1;
        }
    }
    let mut app_breakdown: Vec<AppBreakdown> = apps_detail
        .into_iter()
        .map(|(app, (seconds, source, category, details))| {
            let mut det: Vec<AppDetail> = details
                .into_iter()
                .map(|(label, (s, count))| AppDetail { label, seconds: s, count })
                .collect();
            det.sort_by(|a, b| b.seconds.cmp(&a.seconds).then(a.label.cmp(&b.label)));
            AppBreakdown { app, seconds, source, category, details: det }
        })
        .collect();
    app_breakdown.sort_by(|a, b| b.seconds.cmp(&a.seconds).then(a.app.cmp(&b.app)));
    let mut claude: Vec<ClaudeAgg> = claude_secs
        .into_iter()
        .map(|(project, seconds)| ClaudeAgg {
            project,
            seconds,
            tokens_in: 0,
            tokens_out: 0,
        })
        .collect();
    claude.sort_by(|a, b| b.seconds.cmp(&a.seconds).then(a.project.cmp(&b.project)));
    let focus_segments = active_iv.len() as i64;
    let longest_focus_s = longest_run_seconds(active_iv.clone());
    DayReport {
        date,
        events,
        total_active_s: union_seconds(active_iv),
        total_idle_s: union_seconds(idle_iv),
        session_count: sessions.len() as i64,
        longest_focus_s,
        focus_segments,
        by_app: to_buckets(by_app),
        by_category: to_buckets(by_cat),
        by_host: to_buckets(by_host),
        by_project: to_buckets(by_project),
        claude,
        app_breakdown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex as PMutex;
    use rusqlite::Connection;
    use std::sync::Arc;

    fn test_db() -> DbHandle {
        let conn = Connection::open_in_memory().unwrap();
        tdb::init_schema(&conn).unwrap();
        Arc::new(PMutex::new(conn))
    }

    fn focus(app: &str, title: Option<&str>) -> Option<FocusInfo> {
        Some(FocusInfo {
            app_name: app.into(),
            app_id: None,
            window_title: title.map(|t| t.into()),
        })
    }

    #[test]
    fn opens_and_switches_on_app_change_no_overlap() {
        let db = test_db();
        let sid = tdb::start_session(&db, None, 0).unwrap();
        let mut rt = Runtime {
            session_id: Some(sid),
            ..Default::default()
        };
        apply_tick(&db, &mut rt, sid, Tick { now: 1_000, focus: focus("Code", Some("a.rs")), idle_s: 0.0, threshold_s: 300.0, denylist: &[] });
        apply_tick(&db, &mut rt, sid, Tick { now: 2_500, focus: focus("Code", Some("a.rs")), idle_s: 0.0, threshold_s: 300.0, denylist: &[] }); // same → no new
        apply_tick(&db, &mut rt, sid, Tick { now: 4_000, focus: focus("Safari", Some("Docs")), idle_s: 0.0, threshold_s: 300.0, denylist: &[] }); // switch
        let evs = tdb::events_in_range(&db, -1, 1_000_000).unwrap();
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0].app_name, "Code");
        assert_eq!(evs[0].ended_at, Some(4_000)); // closed exactly when the next opened
        assert_eq!(evs[1].app_name, "Safari");
        assert_eq!(evs[1].ended_at, None); // still open
        assert!(!rt.paused);
    }

    #[test]
    fn idle_retroactively_closes_active_and_marks_idle_then_resumes() {
        let db = test_db();
        let sid = tdb::start_session(&db, None, 0).unwrap();
        let mut rt = Runtime {
            session_id: Some(sid),
            ..Default::default()
        };
        // Active from t=4000.
        apply_tick(&db, &mut rt, sid, Tick { now: 4_000, focus: focus("Code", None), idle_s: 0.0, threshold_s: 300.0, denylist: &[] });
        // At t=1_000_000 the user has been idle 400 s → idle began at 600_000.
        apply_tick(&db, &mut rt, sid, Tick { now: 1_000_000, focus: focus("Code", None), idle_s: 400.0, threshold_s: 300.0, denylist: &[] });
        assert!(rt.paused, "idle should pause");
        // Input resumes (idle 0) at t=1_200_000.
        apply_tick(&db, &mut rt, sid, Tick { now: 1_200_000, focus: focus("Code", None), idle_s: 0.0, threshold_s: 300.0, denylist: &[] });
        assert!(!rt.paused, "resume should unpause");

        let evs = tdb::events_in_range(&db, -1, 10_000_000).unwrap();
        assert_eq!(evs.len(), 3);
        // [0] active Code, closed at idle-begin 600_000 (retroactive).
        assert!(!evs[0].is_idle);
        assert_eq!(evs[0].ended_at, Some(600_000));
        // [1] idle span 600_000 → 1_200_000.
        assert!(evs[1].is_idle);
        assert_eq!(evs[1].started_at, 600_000);
        assert_eq!(evs[1].ended_at, Some(1_200_000));
        // [2] active again, open.
        assert!(!evs[2].is_idle);
        assert_eq!(evs[2].started_at, 1_200_000);
        assert_eq!(evs[2].ended_at, None);
    }

    #[test]
    fn aggregate_day_totals_and_buckets() {
        // Window [1000, 100_000); now well past `to`.
        let mk = |app: &str, cat: Option<&str>, host: Option<&str>, idle: bool, s: i64, e: i64| {
            tdb::TrackEvent {
                id: 0,
                session_id: 1,
                app_name: app.into(),
                app_id: None,
                window_title: None,
                url: None,
                host: host.map(|h| h.into()),
                category: cat.map(|c| c.into()),
                project: None,
                source: "focus".into(),
                is_idle: idle,
                started_at: s,
                ended_at: Some(e),
                duration_s: Some((e - s) / 1000),
            }
        };
        let events = vec![
            mk("Code", Some("Dev"), None, false, 1_000, 11_000),      // 10s active
            mk("Safari", Some("Research"), Some("github.com"), false, 11_000, 41_000), // 30s
            mk("Code", Some("Dev"), None, true, 41_000, 101_000),     // 60s idle (clipped to 100_000 → 59s)
        ];
        let r = aggregate_day("2026-06-20".into(), events, 1_000, 100_000, 9_999_999);
        assert_eq!(r.total_active_s, 40); // 10 + 30
        assert_eq!(r.total_idle_s, 59); // clipped to the window
        assert_eq!(r.session_count, 1);
        // by_app sorted desc: Safari(30) before Code(10).
        assert_eq!(r.by_app[0].key, "Safari");
        assert_eq!(r.by_app[0].seconds, 30);
        assert_eq!(r.by_app[1].key, "Code");
        assert_eq!(r.by_app[1].seconds, 10);
        // by_category Dev=10 (idle excluded), Research=30.
        let dev = r.by_category.iter().find(|b| b.key == "Dev").unwrap();
        assert_eq!(dev.seconds, 10);
        assert_eq!(r.by_host.iter().find(|b| b.key == "github.com").unwrap().seconds, 30);
    }

    #[test]
    fn union_seconds_merges_overlaps_never_exceeds_wallclock() {
        assert_eq!(union_seconds(vec![]), 0);
        assert_eq!(union_seconds(vec![(0, 10_000)]), 10); // 10s
        // Two overlapping 60s spans over the same minute → 60s, not 120s.
        assert_eq!(union_seconds(vec![(0, 60_000), (0, 60_000)]), 60);
        // Partial overlap [0,40] ∪ [30,60] = [0,60] = 60s.
        assert_eq!(union_seconds(vec![(0, 40_000), (30_000, 60_000)]), 60);
        // Disjoint [0,10] + [20,30] = 20s.
        assert_eq!(union_seconds(vec![(20_000, 30_000), (0, 10_000)]), 20);
    }

    #[test]
    fn hour_buckets_distribute_active_time_and_dedupe_overlap() {
        let mk = |idle: bool, src: &str, s: i64, e: i64| tdb::TrackEvent {
            id: 0,
            session_id: 1,
            app_name: "Code".into(),
            app_id: None,
            window_title: None,
            url: None,
            host: None,
            category: None,
            project: None,
            source: src.into(),
            is_idle: idle,
            started_at: s,
            ended_at: Some(e),
            duration_s: None,
        };
        let h = 3_600_000;
        let events = vec![
            mk(false, "focus", 0, h),          // hour 0 full (3600s)
            mk(false, "focus", 0, h / 2),      // overlaps hour 0 → no double count
            mk(false, "focus", h + h / 2, 2 * h), // hour 1: 1800s
            mk(true, "focus", 2 * h, 3 * h),   // idle → ignored
        ];
        let hb = hour_buckets(&events, 0, 86_400_000, 9_999_999);
        assert_eq!(hb[0], 3600); // union, not 5400
        assert_eq!(hb[1], 1800);
        assert_eq!(hb[2], 0);
        assert_eq!(hb.len(), 24);
    }

    #[test]
    fn aggregate_day_focus_metrics() {
        let mk = |app: &str, idle: bool, s: i64, e: i64| tdb::TrackEvent {
            id: 0,
            session_id: 1,
            app_name: app.into(),
            app_id: None,
            window_title: None,
            url: None,
            host: None,
            category: None,
            project: None,
            source: "focus".into(),
            is_idle: idle,
            started_at: s,
            ended_at: Some(e),
            duration_s: Some((e - s) / 1000),
        };
        // Run 1: Code 0–600s, Safari 600–900s (contiguous → 900s run).
        // idle 900–1200s breaks it. Run 2: Code 1200–1500s (300s).
        let events = vec![
            mk("Code", false, 0, 600_000),
            mk("Safari", false, 600_000, 900_000),
            mk("Code", true, 900_000, 1_200_000),
            mk("Code", false, 1_200_000, 1_500_000),
        ];
        let r = aggregate_day("2026-06-21".into(), events, 0, 10_000_000, 9_999_999);
        assert_eq!(r.focus_segments, 3); // 3 active events
        assert_eq!(r.longest_focus_s, 900); // the contiguous Code+Safari run
        assert_eq!(r.total_active_s, 1200); // 900 + 300
    }

    #[test]
    fn aggregate_day_totals_are_union_not_sum_for_overlapping_events() {
        // Two overlapping active events (e.g. a leftover open event) over the
        // same 60s → the headline total is 60s, not 120s.
        let ev = |s: i64, e: i64| tdb::TrackEvent {
            id: 0,
            session_id: 1,
            app_name: "Code".into(),
            app_id: None,
            window_title: None,
            url: None,
            host: None,
            category: None,
            project: None,
            source: "focus".into(),
            is_idle: false,
            started_at: s,
            ended_at: Some(e),
            duration_s: Some((e - s) / 1000),
        };
        let r = aggregate_day("2026-06-21".into(), vec![ev(0, 60_000), ev(0, 60_000)], 0, 1_000_000, 9_999_999);
        assert_eq!(r.total_active_s, 60); // union, not 120
    }

    #[test]
    fn aggregate_day_app_breakdown_groups_browser_by_host_and_apps_by_title() {
        let mk = |app: &str, src: &str, host: Option<&str>, title: Option<&str>, s: i64, e: i64| {
            tdb::TrackEvent {
                id: 0,
                session_id: 1,
                app_name: app.into(),
                app_id: None,
                window_title: title.map(|t| t.into()),
                url: None,
                host: host.map(|h| h.into()),
                category: None,
                project: None,
                source: src.into(),
                is_idle: false,
                started_at: s,
                ended_at: Some(e),
                duration_s: Some((e - s) / 1000),
            }
        };
        let events = vec![
            mk("Google Chrome", "browser", Some("github.com"), Some("PR #1"), 0, 10_000), // 10s
            mk("Google Chrome", "browser", Some("news.com"), Some("News"), 10_000, 16_000), // 6s
            mk("Google Chrome", "browser", Some("github.com"), Some("PR #2"), 16_000, 36_000), // 20s
            mk("Code", "focus", None, Some("main.rs"), 36_000, 96_000), // 60s
        ];
        let r = aggregate_day("2026-06-20".into(), events, 0, 100_000, 9_999_999);
        // Sorted by total time desc: Code (60s) before Chrome (36s).
        assert_eq!(r.app_breakdown.len(), 2);
        assert_eq!(r.app_breakdown[0].app, "Code");
        assert_eq!(r.app_breakdown[0].seconds, 60);
        assert_eq!(r.app_breakdown[0].details[0].label, "main.rs"); // non-browser → title
        let chrome = &r.app_breakdown[1];
        assert_eq!(chrome.app, "Google Chrome");
        assert_eq!(chrome.seconds, 36);
        assert_eq!(chrome.source, "browser");
        // Browser details grouped by host, time desc: github.com 30s/2 before news.com.
        assert_eq!(chrome.details[0].label, "github.com");
        assert_eq!(chrome.details[0].seconds, 30);
        assert_eq!(chrome.details[0].count, 2);
        assert_eq!(chrome.details[1].label, "news.com");
        // Still counted once in the active total + by_app.
        assert_eq!(r.total_active_s, 96);
    }

    #[test]
    fn no_focus_and_not_idle_keeps_current_interval() {
        let db = test_db();
        let sid = tdb::start_session(&db, None, 0).unwrap();
        let mut rt = Runtime {
            session_id: Some(sid),
            ..Default::default()
        };
        apply_tick(&db, &mut rt, sid, Tick { now: 1_000, focus: focus("Code", None), idle_s: 0.0, threshold_s: 300.0, denylist: &[] });
        let before = rt.open_event_id;
        apply_tick(&db, &mut rt, sid, Tick { now: 2_000, focus: None, idle_s: 0.0, threshold_s: 300.0, denylist: &[] }); // no info
        assert_eq!(rt.open_event_id, before, "should not churn the interval");
        assert_eq!(tdb::events_in_range(&db, -1, 1_000_000).unwrap().len(), 1);
    }

    #[test]
    fn late_tab_report_enriches_instead_of_splitting() {
        let db = test_db();
        let sid = tdb::start_session(&db, None, 0).unwrap();
        let mut rt = Runtime { session_id: Some(sid), ..Default::default() };
        let deny: Vec<String> = vec![];
        // Tick 1: browser frontmost, the extension hasn't reported the tab yet.
        apply_tick(&db, &mut rt, sid, Tick { now: 1_000, focus: focus("Safari", Some("Loading…")), idle_s: 0.0, threshold_s: 300.0, denylist: &deny });
        let evs = tdb::events_in_range(&db, -1, 1_000_000).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].host, None);
        let first_id = evs[0].id;
        // Tick 2: tab report arrived → the young interval is enriched IN PLACE
        // (no "(unknown)" fragment split).
        rt.last_tab = Some(TabInfo {
            host: Some("github.com".into()),
            title: Some("PR".into()),
            url: Some("https://github.com/pr".into()),
        });
        apply_tick(&db, &mut rt, sid, Tick { now: 2_500, focus: focus("Safari", Some("PR")), idle_s: 0.0, threshold_s: 300.0, denylist: &deny });
        let evs = tdb::events_in_range(&db, -1, 1_000_000).unwrap();
        assert_eq!(evs.len(), 1, "no fragment split — enriched in place");
        assert_eq!(evs[0].id, first_id);
        assert_eq!(evs[0].host.as_deref(), Some("github.com"));
        // A LATER tab change still splits normally (that's a real transition).
        rt.last_tab = Some(TabInfo {
            host: Some("news.com".into()),
            title: Some("News".into()),
            url: Some("https://news.com/".into()),
        });
        apply_tick(&db, &mut rt, sid, Tick { now: 4_000, focus: focus("Safari", Some("News")), idle_s: 0.0, threshold_s: 300.0, denylist: &deny });
        assert_eq!(tdb::events_in_range(&db, -1, 1_000_000).unwrap().len(), 2);
    }

    #[test]
    fn day_bounds_adjacent_days_share_the_boundary() {
        // The invariant that survives DST (day N's upper bound IS day N+1's
        // lower bound) — checked across both 2026 EU transition days + normal
        // days. With the old fixed `from + 24h` upper bound, the spring day
        // double-counted an hour and the fall day dropped one.
        for (a, b) in [
            ("2026-03-29", "2026-03-30"), // EU spring-forward day (23 h in CET)
            ("2026-10-25", "2026-10-26"), // EU fall-back day (25 h in CET)
            ("2026-07-01", "2026-07-02"),
            ("2026-12-31", "2027-01-01"),
        ] {
            let (fa, ta) = day_bounds(a).unwrap();
            let (fb, _) = day_bounds(b).unwrap();
            assert_eq!(ta, fb, "{a} upper bound must equal {b} lower bound");
            assert!(ta > fa, "{a} must be a non-empty day");
        }
    }

    #[test]
    fn parse_denylist_splits_and_lowercases() {
        let d = parse_denylist("1Password, KeePass\nbank.com\n , ");
        assert_eq!(d, vec!["1password", "keepass", "bank.com"]);
        assert!(parse_denylist("").is_empty());
    }

    #[test]
    fn denylist_strips_title_host_url_keeps_app_and_time() {
        let db = test_db();
        let sid = tdb::start_session(&db, None, 0).unwrap();
        let mut rt = Runtime {
            session_id: Some(sid),
            ..Default::default()
        };
        let deny = vec!["1password".to_string()];
        apply_tick(&db, &mut rt, sid, Tick { now: 1_000, focus: focus("1Password", Some("Secret vault")), idle_s: 0.0, threshold_s: 300.0, denylist: &deny });
        let evs = tdb::events_in_range(&db, -1, 1_000_000).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].app_name, "1Password"); // app + time kept
        assert_eq!(evs[0].window_title, None); // title stripped
    }

    #[test]
    fn prune_before_drops_old_events_and_empty_sessions() {
        let db = test_db();
        let s_old = tdb::start_session(&db, None, 0).unwrap();
        let s_new = tdb::start_session(&db, None, 10_000_000).unwrap();
        let old = tdb::open_event(&db, &tdb::NewEvent {
            session_id: s_old, app_name: "A".into(), app_id: None, window_title: None,
            url: None, host: None, category: None, project: None, source: "focus".into(),
            is_idle: false, started_at: 1_000,
        }).unwrap();
        tdb::close_event(&db, old, 2_000).unwrap();
        tdb::open_event(&db, &tdb::NewEvent {
            session_id: s_new, app_name: "B".into(), app_id: None, window_title: None,
            url: None, host: None, category: None, project: None, source: "focus".into(),
            is_idle: false, started_at: 10_001_000,
        }).unwrap();
        let n = tdb::prune_before(&db, 5_000_000).unwrap();
        assert_eq!(n, 1);
        let evs = tdb::events_in_range(&db, -1, 100_000_000).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].app_name, "B");
    }
}
