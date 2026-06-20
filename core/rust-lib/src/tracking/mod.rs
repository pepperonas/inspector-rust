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

pub mod db;
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
    stop: Option<Arc<AtomicBool>>,
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
    let sid = tdb::start_session(db, label.as_deref(), now).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    *rt = Runtime {
        session_id: Some(sid),
        session_started: Some(now),
        stop: Some(stop.clone()),
        ..Default::default()
    };
    drop(rt);

    let (a, d, s) = (app.clone(), db.clone(), state.0.clone());
    std::thread::spawn(move || run_loop(a, d, s, sid, stop));
    let _ = app.emit("track-status-changed", ());
    Ok(sid)
}

pub fn stop(app: &AppHandle, db: &DbHandle, state: &TrackerState) -> Result<(), String> {
    let mut rt = state.0.lock();
    let Some(sid) = rt.session_id else {
        return Ok(());
    };
    if let Some(s) = &rt.stop {
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
    while !stop.load(Ordering::SeqCst) {
        let threshold = crate::settings::get_or(&db, "track.idle_seconds", "300")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| *v > 0.0)
            .unwrap_or(DEFAULT_IDLE_SECONDS);
        let focus = os::frontmost();
        let idle_s = os::idle_seconds().unwrap_or(0.0);
        let now = now_ms();

        let paused_changed = {
            let mut g = rt.lock();
            if g.session_id != Some(sid) {
                break; // session ended / replaced
            }
            let was_paused = g.paused;
            apply_tick(&db, &mut g, sid, now, focus, idle_s, threshold);
            g.paused != was_paused
        };
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

/// The pure-ish per-tick state machine (no `AppHandle`, so it's unit-testable
/// against an in-memory DB): decide the *desired* interval and, on change,
/// close the open one + open the new. Idle transitions are retroactive — the
/// idle interval starts at the moment input actually stopped.
fn apply_tick(
    db: &DbHandle,
    rt: &mut Runtime,
    sid: i64,
    now: i64,
    focus: Option<FocusInfo>,
    idle_s: f64,
    threshold_s: f64,
) {
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
            is_idle: true,
            started_at: begin,
        })
    } else if let Some(f) = &focus {
        rt.active_app = Some(f.app_name.clone());
        Some(Desired {
            key: format!(
                "{}\u{0}{}\u{0}focus",
                f.app_name,
                f.window_title.as_deref().unwrap_or("")
            ),
            app: f.app_name.clone(),
            app_id: f.app_id.clone(),
            title: f.window_title.clone(),
            is_idle: false,
            started_at: now,
        })
    } else {
        None
    };

    let Some(d) = desired else {
        return;
    };
    if rt.open_key.as_deref() == Some(d.key.as_str()) {
        return; // unchanged → leave the interval open
    }

    // Transition: close the open interval (clamped so it can't end before it
    // started), then open the new one.
    let close_at = d.started_at.max(rt.open_started_at);
    if let Some(open_id) = rt.open_event_id.take() {
        let _ = tdb::close_event(db, open_id, close_at);
    }
    let ne = tdb::NewEvent {
        session_id: sid,
        app_name: d.app.clone(),
        app_id: d.app_id.clone(),
        window_title: d.title.clone(),
        url: None,
        host: None,
        category: None,
        project: None,
        source: "focus".to_string(),
        is_idle: d.is_idle,
        started_at: d.started_at,
    };
    rt.open_event_id = tdb::open_event(db, &ne).ok();
    rt.open_key = Some(d.key);
    rt.open_started_at = d.started_at;
    rt.paused = d.is_idle;
}

struct Desired {
    key: String,
    app: String,
    app_id: Option<String>,
    title: Option<String>,
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
pub struct DayReport {
    pub date: String,
    pub events: Vec<tdb::TrackEvent>,
    pub total_active_s: i64,
    pub total_idle_s: i64,
    pub session_count: i64,
    pub by_app: Vec<Bucket>,
    pub by_category: Vec<Bucket>,
    pub by_host: Vec<Bucket>,
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
    use chrono::{Local, NaiveDate, TimeZone};
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|e| format!("bad date: {e}"))?;
    let midnight = d
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "invalid date".to_string())?;
    let from = Local
        .from_local_datetime(&midnight)
        .single()
        .ok_or_else(|| "ambiguous local date".to_string())?
        .timestamp_millis();
    let to = from + 86_400_000;
    let events = tdb::events_in_range(db, from, to).map_err(|e| e.to_string())?;
    Ok(aggregate_day(date.to_string(), events, from, to, now_ms()))
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
    let (mut active, mut idle) = (0i64, 0i64);
    let mut sessions: HashSet<i64> = HashSet::new();
    let mut by_app: HashMap<String, i64> = HashMap::new();
    let mut by_host: HashMap<String, i64> = HashMap::new();
    let mut by_cat: HashMap<String, i64> = HashMap::new();
    for e in &events {
        sessions.insert(e.session_id);
        let end = e.ended_at.unwrap_or(now).min(to);
        let start = e.started_at.max(from);
        let dur = ((end - start).max(0)) / 1000;
        if dur == 0 {
            continue;
        }
        if e.is_idle {
            idle += dur;
        } else {
            active += dur;
            *by_app.entry(e.app_name.clone()).or_default() += dur;
            if let Some(h) = &e.host {
                *by_host.entry(h.clone()).or_default() += dur;
            }
            let cat = e.category.clone().unwrap_or_else(|| "Uncategorized".to_string());
            *by_cat.entry(cat).or_default() += dur;
        }
    }
    DayReport {
        date,
        events,
        total_active_s: active,
        total_idle_s: idle,
        session_count: sessions.len() as i64,
        by_app: to_buckets(by_app),
        by_category: to_buckets(by_cat),
        by_host: to_buckets(by_host),
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
        apply_tick(&db, &mut rt, sid, 1_000, focus("Code", Some("a.rs")), 0.0, 300.0);
        apply_tick(&db, &mut rt, sid, 2_500, focus("Code", Some("a.rs")), 0.0, 300.0); // same → no new
        apply_tick(&db, &mut rt, sid, 4_000, focus("Safari", Some("Docs")), 0.0, 300.0); // switch
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
        apply_tick(&db, &mut rt, sid, 4_000, focus("Code", None), 0.0, 300.0);
        // At t=1_000_000 the user has been idle 400 s → idle began at 600_000.
        apply_tick(&db, &mut rt, sid, 1_000_000, focus("Code", None), 400.0, 300.0);
        assert!(rt.paused, "idle should pause");
        // Input resumes (idle 0) at t=1_200_000.
        apply_tick(&db, &mut rt, sid, 1_200_000, focus("Code", None), 0.0, 300.0);
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
    fn no_focus_and_not_idle_keeps_current_interval() {
        let db = test_db();
        let sid = tdb::start_session(&db, None, 0).unwrap();
        let mut rt = Runtime {
            session_id: Some(sid),
            ..Default::default()
        };
        apply_tick(&db, &mut rt, sid, 1_000, focus("Code", None), 0.0, 300.0);
        let before = rt.open_event_id;
        apply_tick(&db, &mut rt, sid, 2_000, None, 0.0, 300.0); // no info
        assert_eq!(rt.open_event_id, before, "should not churn the interval");
        assert_eq!(tdb::events_in_range(&db, -1, 1_000_000).unwrap().len(), 1);
    }
}
