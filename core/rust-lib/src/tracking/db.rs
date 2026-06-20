//! Timesheet (time-tracking) persistence — the SQLite layer for the
//! `track` feature. Lives alongside the other tables in the one app DB
//! (`DbHandle`), reuses the same AES-256-GCM-at-rest crypto path
//! (`crypto::encrypt`/`decrypt`) for the sensitive `window_title` + `url`
//! columns, and follows the repo's `CREATE TABLE IF NOT EXISTS` + lazy
//! migration convention.
//!
//! ## Encryption
//! `window_title` and `url` are encrypted at rest. We store the encrypted value
//! as **TEXT** (the `crypto::encrypt` `"v1:<base64>"` string) — not BLOB — to
//! match the existing `entries.content_text`/`snippets.body` convention and the
//! `String`-returning crypto API (no new crypto path). `app_name`, `host`,
//! `category`, `project` and all timestamps stay plaintext for aggregation.

use crate::crypto;
use crate::db::DbHandle;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Create the timesheet tables + indexes. Idempotent (`IF NOT EXISTS`); called
/// from `db::open` right after the core tables. Never crashes an existing DB.
pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS track_sessions (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            label       TEXT,
            started_at  INTEGER NOT NULL,
            ended_at    INTEGER,
            status      TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS track_events (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id    INTEGER NOT NULL REFERENCES track_sessions(id) ON DELETE CASCADE,
            app_name      TEXT NOT NULL,
            app_id        TEXT,
            window_title  TEXT,   -- AES-256-GCM ("v1:" string), nullable
            url           TEXT,   -- AES-256-GCM ("v1:" string), nullable
            host          TEXT,
            category      TEXT,
            project       TEXT,
            source        TEXT NOT NULL,           -- 'focus' | 'browser' | 'claude'
            is_idle       INTEGER NOT NULL DEFAULT 0,
            started_at    INTEGER NOT NULL,
            ended_at      INTEGER,
            duration_s    INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_track_events_started ON track_events(started_at);
        CREATE INDEX IF NOT EXISTS idx_track_events_session ON track_events(session_id);

        CREATE TABLE IF NOT EXISTS track_claude_turns (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            event_id      INTEGER NOT NULL REFERENCES track_events(id) ON DELETE CASCADE,
            ts            INTEGER NOT NULL,
            model         TEXT,
            tokens_in     INTEGER,
            tokens_out    INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_track_claude_event ON track_claude_turns(event_id);

        CREATE TABLE IF NOT EXISTS track_categories (
            app_name      TEXT PRIMARY KEY,
            category      TEXT NOT NULL
        );
        "#,
    )?;
    // Foreign-key cascade is opt-in per connection in SQLite.
    let _ = conn.pragma_update(None, "foreign_keys", "ON");
    Ok(())
}

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct TrackSession {
    pub id: i64,
    pub label: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub status: String,
}

/// An event as read back for the UI/export — `window_title`/`url` already
/// decrypted.
#[derive(Debug, Clone, Serialize)]
pub struct TrackEvent {
    pub id: i64,
    pub session_id: i64,
    pub app_name: String,
    pub app_id: Option<String>,
    pub window_title: Option<String>,
    pub url: Option<String>,
    pub host: Option<String>,
    pub category: Option<String>,
    pub project: Option<String>,
    pub source: String,
    pub is_idle: bool,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_s: Option<i64>,
}

/// A new focus/browser/claude interval to open.
#[derive(Debug, Clone)]
pub struct NewEvent {
    pub session_id: i64,
    pub app_name: String,
    pub app_id: Option<String>,
    pub window_title: Option<String>,
    pub url: Option<String>,
    pub host: Option<String>,
    pub category: Option<String>,
    pub project: Option<String>,
    pub source: String,
    pub is_idle: bool,
    pub started_at: i64,
}

/// Editable fields for `update_event` — every `Some` is applied, `None` leaves
/// the column untouched. For `category`/`project`/`window_title` an **empty
/// string clears** the column (set to NULL); a non-empty value sets it
/// (`window_title` re-encrypted). Using `Option<String>` (not the serde-fragile
/// `Option<Option<String>>`, which can't represent an explicit null over JSON).
#[derive(Debug, Default, Deserialize)]
pub struct EventPatch {
    pub app_name: Option<String>,
    pub category: Option<String>,
    pub project: Option<String>,
    pub window_title: Option<String>,
    pub is_idle: Option<bool>,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
}

/// `""` → `None` (clear the column), otherwise `Some(value)`.
fn blank_to_null(v: &str) -> Option<&str> {
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

// ── Sessions ─────────────────────────────────────────────────────────────────

pub fn start_session(db: &DbHandle, label: Option<&str>, now: i64) -> rusqlite::Result<i64> {
    let conn = db.lock();
    conn.execute(
        "INSERT INTO track_sessions (label, started_at, ended_at, status) VALUES (?1, ?2, NULL, 'active')",
        params![label, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn set_session_status(db: &DbHandle, id: i64, status: &str) -> rusqlite::Result<()> {
    let conn = db.lock();
    conn.execute(
        "UPDATE track_sessions SET status = ?2 WHERE id = ?1",
        params![id, status],
    )?;
    Ok(())
}

pub fn end_session(db: &DbHandle, id: i64, now: i64) -> rusqlite::Result<()> {
    let conn = db.lock();
    conn.execute(
        "UPDATE track_sessions SET status = 'ended', ended_at = ?2 WHERE id = ?1",
        params![id, now],
    )?;
    Ok(())
}

/// The newest still-`active` session, if any (used to resume after a relaunch
/// or to refuse a double-start).
pub fn active_session(db: &DbHandle) -> rusqlite::Result<Option<TrackSession>> {
    let conn = db.lock();
    conn.query_row(
        "SELECT id, label, started_at, ended_at, status FROM track_sessions \
         WHERE status != 'ended' ORDER BY id DESC LIMIT 1",
        [],
        row_to_session,
    )
    .optional()
}

fn row_to_session(row: &rusqlite::Row) -> rusqlite::Result<TrackSession> {
    Ok(TrackSession {
        id: row.get(0)?,
        label: row.get(1)?,
        started_at: row.get(2)?,
        ended_at: row.get(3)?,
        status: row.get(4)?,
    })
}

// ── Events ───────────────────────────────────────────────────────────────────

pub fn open_event(db: &DbHandle, ev: &NewEvent) -> rusqlite::Result<i64> {
    let conn = db.lock();
    let enc_title = ev.window_title.as_deref().map(crypto::encrypt);
    let enc_url = ev.url.as_deref().map(crypto::encrypt);
    conn.execute(
        "INSERT INTO track_events \
         (session_id, app_name, app_id, window_title, url, host, category, project, source, is_idle, started_at, ended_at, duration_s) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, NULL)",
        params![
            ev.session_id,
            ev.app_name,
            ev.app_id,
            enc_title,
            enc_url,
            ev.host,
            ev.category,
            ev.project,
            ev.source,
            ev.is_idle as i64,
            ev.started_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Close an open event: set `ended_at` + denormalised `duration_s`. If `ended_at`
/// is before `started_at` (clock skew) the duration is clamped to 0.
pub fn close_event(db: &DbHandle, id: i64, ended_at: i64) -> rusqlite::Result<()> {
    let conn = db.lock();
    conn.execute(
        "UPDATE track_events \
         SET ended_at = ?2, duration_s = MAX(0, (?2 - started_at) / 1000) \
         WHERE id = ?1 AND ended_at IS NULL",
        params![id, ended_at],
    )?;
    Ok(())
}

/// Heartbeat the currently-open event: keep its `ended_at` ≈ `now` (still open in
/// the runtime, but persisted so a crash leaves it ended at the last-alive time —
/// no phantom offline duration). Unlike `close_event` this updates even an
/// already-stamped `ended_at`, because the event is still live.
pub fn touch_event(db: &DbHandle, id: i64, now: i64) -> rusqlite::Result<()> {
    let conn = db.lock();
    conn.execute(
        "UPDATE track_events \
         SET ended_at = ?2, duration_s = MAX(0, (?2 - started_at) / 1000) \
         WHERE id = ?1",
        params![id, now],
    )?;
    Ok(())
}

/// Finalize any events of `session_id` that are still `ended_at IS NULL` (only
/// brand-new ones that crashed before the first heartbeat) by ending them at
/// their own `started_at` (≈0 duration — the lost time is < one heartbeat). Used
/// on resume so a recovered session has no dangling open events. Returns rows.
pub fn finalize_open_events(db: &DbHandle, session_id: i64) -> rusqlite::Result<usize> {
    let conn = db.lock();
    conn.execute(
        "UPDATE track_events SET ended_at = started_at, duration_s = 0 \
         WHERE session_id = ?1 AND ended_at IS NULL",
        params![session_id],
    )
}

/// Enrich the still-open event with browser tab metadata (host/title/url) — used
/// while a browser is frontmost and the extension reports the active tab.
pub fn enrich_event(
    db: &DbHandle,
    id: i64,
    host: Option<&str>,
    title: Option<&str>,
    url: Option<&str>,
) -> rusqlite::Result<()> {
    let conn = db.lock();
    let enc_title = title.map(crypto::encrypt);
    let enc_url = url.map(crypto::encrypt);
    conn.execute(
        "UPDATE track_events SET host = COALESCE(?2, host), \
         window_title = COALESCE(?3, window_title), url = COALESCE(?4, url) \
         WHERE id = ?1 AND ended_at IS NULL",
        params![id, host, enc_title, enc_url],
    )?;
    Ok(())
}

pub fn update_event(db: &DbHandle, id: i64, patch: &EventPatch) -> rusqlite::Result<()> {
    let conn = db.lock();
    if let Some(v) = &patch.app_name {
        conn.execute("UPDATE track_events SET app_name = ?2 WHERE id = ?1", params![id, v])?;
    }
    if let Some(v) = &patch.category {
        conn.execute(
            "UPDATE track_events SET category = ?2 WHERE id = ?1",
            params![id, blank_to_null(v)],
        )?;
    }
    if let Some(v) = &patch.project {
        conn.execute(
            "UPDATE track_events SET project = ?2 WHERE id = ?1",
            params![id, blank_to_null(v)],
        )?;
    }
    if let Some(v) = &patch.window_title {
        let enc = blank_to_null(v).map(crypto::encrypt);
        conn.execute("UPDATE track_events SET window_title = ?2 WHERE id = ?1", params![id, enc])?;
    }
    if let Some(v) = patch.is_idle {
        conn.execute("UPDATE track_events SET is_idle = ?2 WHERE id = ?1", params![id, v as i64])?;
    }
    if patch.started_at.is_some() || patch.ended_at.is_some() {
        conn.execute(
            "UPDATE track_events SET \
             started_at = COALESCE(?2, started_at), \
             ended_at = COALESCE(?3, ended_at) WHERE id = ?1",
            params![id, patch.started_at, patch.ended_at],
        )?;
        // Recompute duration from the (possibly) new bounds.
        conn.execute(
            "UPDATE track_events SET duration_s = MAX(0, (ended_at - started_at) / 1000) \
             WHERE id = ?1 AND ended_at IS NOT NULL",
            params![id],
        )?;
    }
    Ok(())
}

pub fn delete_event(db: &DbHandle, id: i64) -> rusqlite::Result<()> {
    let conn = db.lock();
    conn.execute("DELETE FROM track_events WHERE id = ?1", params![id])?;
    Ok(())
}

/// Merge `ids` into the earliest event (by `started_at`): the survivor spans
/// from the earliest start to the latest end; the others are deleted. Returns
/// the surviving event id. No-op for < 2 ids.
pub fn merge_events(db: &DbHandle, ids: &[i64]) -> rusqlite::Result<Option<i64>> {
    if ids.len() < 2 {
        return Ok(ids.first().copied());
    }
    let conn = db.lock();
    // Load the merge set (id, started_at, ended_at).
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, started_at, ended_at FROM track_events WHERE id IN ({placeholders}) ORDER BY started_at ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let params_vec: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
    let rows: Vec<(i64, i64, Option<i64>)> = stmt
        .query_map(params_vec.as_slice(), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);
    if rows.len() < 2 {
        return Ok(rows.first().map(|r| r.0));
    }
    let survivor = rows[0].0;
    let min_start = rows.iter().map(|r| r.1).min().unwrap();
    // Latest known end (None ends are treated as "still open" → keep open).
    let any_open = rows.iter().any(|r| r.2.is_none());
    let max_end = rows.iter().filter_map(|r| r.2).max();
    let losers: Vec<i64> = rows.iter().skip(1).map(|r| r.0).collect();
    let del_ph = losers.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let del_sql = format!("DELETE FROM track_events WHERE id IN ({del_ph})");
    let del_params: Vec<&dyn rusqlite::ToSql> =
        losers.iter().map(|i| i as &dyn rusqlite::ToSql).collect();
    conn.execute(&del_sql, del_params.as_slice())?;
    if any_open {
        conn.execute(
            "UPDATE track_events SET started_at = ?2, ended_at = NULL, duration_s = NULL WHERE id = ?1",
            params![survivor, min_start],
        )?;
    } else if let Some(end) = max_end {
        conn.execute(
            "UPDATE track_events SET started_at = ?2, ended_at = ?3, \
             duration_s = MAX(0, (?3 - ?2) / 1000) WHERE id = ?1",
            params![survivor, min_start, end],
        )?;
    }
    Ok(Some(survivor))
}

pub fn set_category(db: &DbHandle, app_name: &str, category: &str) -> rusqlite::Result<()> {
    let conn = db.lock();
    conn.execute(
        "INSERT INTO track_categories (app_name, category) VALUES (?1, ?2) \
         ON CONFLICT(app_name) DO UPDATE SET category = excluded.category",
        params![app_name, category],
    )?;
    // Back-fill the category onto existing events for that app.
    conn.execute(
        "UPDATE track_events SET category = ?2 WHERE app_name = ?1",
        params![app_name, category],
    )?;
    Ok(())
}

/// All events overlapping `[from_ms, to_ms)`, ordered by start, title/url
/// decrypted. Used by both the day view and the export (with a wider range).
pub fn events_in_range(db: &DbHandle, from_ms: i64, to_ms: i64) -> rusqlite::Result<Vec<TrackEvent>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, session_id, app_name, app_id, window_title, url, host, category, project, \
                source, is_idle, started_at, ended_at, duration_s \
         FROM track_events \
         WHERE started_at < ?2 AND (ended_at IS NULL OR ended_at > ?1) \
         ORDER BY started_at ASC",
    )?;
    let rows = stmt
        .query_map(params![from_ms, to_ms], |row| {
            let enc_title: Option<String> = row.get(4)?;
            let enc_url: Option<String> = row.get(5)?;
            Ok(TrackEvent {
                id: row.get(0)?,
                session_id: row.get(1)?,
                app_name: row.get(2)?,
                app_id: row.get(3)?,
                window_title: enc_title.map(|t| crypto::decrypt(&t)),
                url: enc_url.map(|u| crypto::decrypt(&u)),
                host: row.get(6)?,
                category: row.get(7)?,
                project: row.get(8)?,
                source: row.get(9)?,
                is_idle: row.get::<_, i64>(10)? != 0,
                started_at: row.get(11)?,
                ended_at: row.get(12)?,
                duration_s: row.get(13)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn insert_claude_turn(
    db: &DbHandle,
    event_id: i64,
    ts: i64,
    model: Option<&str>,
    tokens_in: Option<i64>,
    tokens_out: Option<i64>,
) -> rusqlite::Result<()> {
    let conn = db.lock();
    conn.execute(
        "INSERT INTO track_claude_turns (event_id, ts, model, tokens_in, tokens_out) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![event_id, ts, model, tokens_in, tokens_out],
    )?;
    Ok(())
}

/// Sum Claude token usage per project for turns in `[from, to)`.
pub fn claude_tokens_by_project(
    db: &DbHandle,
    from_ms: i64,
    to_ms: i64,
) -> rusqlite::Result<std::collections::HashMap<String, (i64, i64)>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT COALESCE(e.project, '(unknown)') AS project, \
                COALESCE(SUM(t.tokens_in), 0), COALESCE(SUM(t.tokens_out), 0) \
         FROM track_claude_turns t JOIN track_events e ON e.id = t.event_id \
         WHERE t.ts >= ?1 AND t.ts < ?2 AND e.source = 'claude' \
         GROUP BY project",
    )?;
    let rows = stmt.query_map(params![from_ms, to_ms], |r| {
        Ok((r.get::<_, String>(0)?, (r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))
    })?;
    let mut map = std::collections::HashMap::new();
    for row in rows {
        let (p, t) = row?;
        map.insert(p, t);
    }
    Ok(map)
}

/// Delete events (+ their claude turns + now-empty sessions) started before
/// `cutoff_ms` (the retention setting). Returns the events deleted.
pub fn prune_before(db: &DbHandle, cutoff_ms: i64) -> rusqlite::Result<usize> {
    let conn = db.lock();
    conn.execute(
        "DELETE FROM track_claude_turns WHERE event_id IN \
         (SELECT id FROM track_events WHERE started_at < ?1)",
        params![cutoff_ms],
    )?;
    let n = conn.execute("DELETE FROM track_events WHERE started_at < ?1", params![cutoff_ms])?;
    conn.execute(
        "DELETE FROM track_sessions WHERE id NOT IN (SELECT DISTINCT session_id FROM track_events)",
        [],
    )?;
    Ok(n)
}

/// Delete all timesheet data (Settings → "Clear timesheet data").
pub fn clear_all(db: &DbHandle) -> rusqlite::Result<()> {
    let conn = db.lock();
    conn.execute_batch(
        "DELETE FROM track_claude_turns; DELETE FROM track_events; DELETE FROM track_sessions;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DbHandle;
    use parking_lot::Mutex;
    use std::sync::Arc;

    fn test_db() -> DbHandle {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn ev(session: i64, app: &str, start: i64) -> NewEvent {
        NewEvent {
            session_id: session,
            app_name: app.into(),
            app_id: None,
            window_title: None,
            url: None,
            host: None,
            category: None,
            project: None,
            source: "focus".into(),
            is_idle: false,
            started_at: start,
        }
    }

    #[test]
    fn session_lifecycle() {
        let db = test_db();
        let sid = start_session(&db, Some("work"), 1000).unwrap();
        assert!(active_session(&db).unwrap().is_some());
        assert_eq!(active_session(&db).unwrap().unwrap().status, "active");
        set_session_status(&db, sid, "paused").unwrap();
        assert_eq!(active_session(&db).unwrap().unwrap().status, "paused");
        end_session(&db, sid, 5000).unwrap();
        assert!(active_session(&db).unwrap().is_none());
    }

    #[test]
    fn open_close_computes_duration_seconds() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let eid = open_event(&db, &ev(sid, "Code", 10_000)).unwrap();
        close_event(&db, eid, 70_000).unwrap(); // 60 s
        let evs = events_in_range(&db, 0, 1_000_000).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].duration_s, Some(60));
        assert_eq!(evs[0].ended_at, Some(70_000));
    }

    #[test]
    fn title_and_url_round_trip_through_the_crypto_path_host_stays_plaintext() {
        // NOTE: unit tests don't run `crypto::init` (it touches the OS keychain),
        // so `crypto::encrypt` is a passthrough here — the ciphertext-at-rest
        // guarantee is crypto.rs's job (its `cipher_roundtrip` test asserts the
        // `v1:` prefix). Here we verify the column plumbing: title/url go through
        // encrypt-on-write + decrypt-on-read and round-trip, and `host` is stored
        // as plaintext for aggregation.
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let mut e = ev(sid, "Safari", 0);
        e.window_title = Some("Secret Page".into());
        e.url = Some("https://example.com/secret".into());
        e.host = Some("example.com".into());
        let _ = open_event(&db, &e).unwrap();
        let evs = events_in_range(&db, -1, 1).unwrap();
        assert_eq!(evs[0].window_title.as_deref(), Some("Secret Page"));
        assert_eq!(evs[0].url.as_deref(), Some("https://example.com/secret"));
        assert_eq!(evs[0].host.as_deref(), Some("example.com"));
    }

    #[test]
    fn touch_event_heartbeats_ended_at_then_finalize_is_noop() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let eid = open_event(&db, &ev(sid, "Code", 1_000)).unwrap();
        // Heartbeat keeps ended_at fresh while the event is still live.
        touch_event(&db, eid, 31_000).unwrap();
        let evs = events_in_range(&db, 0, 1_000_000).unwrap();
        assert_eq!(evs[0].ended_at, Some(31_000));
        assert_eq!(evs[0].duration_s, Some(30));
        // A later heartbeat overrides the earlier one (unlike close_event).
        touch_event(&db, eid, 61_000).unwrap();
        // Resume finalize: nothing dangling (heartbeat already ended it).
        assert_eq!(finalize_open_events(&db, sid).unwrap(), 0);
        assert_eq!(events_in_range(&db, 0, 1_000_000).unwrap()[0].ended_at, Some(61_000));
    }

    #[test]
    fn finalize_open_events_closes_never_heartbeated_events() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let eid = open_event(&db, &ev(sid, "Code", 5_000)).unwrap(); // crashed before any heartbeat
        assert_eq!(finalize_open_events(&db, sid).unwrap(), 1);
        let evs = events_in_range(&db, 0, 1_000_000).unwrap();
        assert_eq!(evs[0].ended_at, Some(5_000)); // ended at its own start → 0 duration
        assert_eq!(evs[0].duration_s, Some(0));
        let _ = eid;
    }

    #[test]
    fn enrich_only_touches_open_event() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let eid = open_event(&db, &ev(sid, "Safari", 0)).unwrap();
        enrich_event(&db, eid, Some("github.com"), Some("Title"), Some("https://github.com")).unwrap();
        let evs = events_in_range(&db, -1, 100).unwrap();
        assert_eq!(evs[0].host.as_deref(), Some("github.com"));
        assert_eq!(evs[0].window_title.as_deref(), Some("Title"));
        // After close, enrich is a no-op.
        close_event(&db, eid, 50).unwrap();
        enrich_event(&db, eid, Some("other.com"), None, None).unwrap();
        let evs = events_in_range(&db, -1, 100).unwrap();
        assert_eq!(evs[0].host.as_deref(), Some("github.com"));
    }

    #[test]
    fn update_event_patches_fields_and_recomputes_duration() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let eid = open_event(&db, &ev(sid, "Code", 0)).unwrap();
        close_event(&db, eid, 10_000).unwrap();
        let patch = EventPatch {
            app_name: Some("VS Code".into()),
            category: Some("Dev".into()),
            is_idle: Some(true),
            ended_at: Some(40_000),
            ..Default::default()
        };
        update_event(&db, eid, &patch).unwrap();
        let evs = events_in_range(&db, -1, 1_000_000).unwrap();
        assert_eq!(evs[0].app_name, "VS Code");
        assert_eq!(evs[0].category.as_deref(), Some("Dev"));
        assert!(evs[0].is_idle);
        assert_eq!(evs[0].duration_s, Some(40)); // recomputed

        // Empty string clears the category (→ NULL).
        update_event(
            &db,
            eid,
            &EventPatch {
                category: Some(String::new()),
                ..Default::default()
            },
        )
        .unwrap();
        let evs = events_in_range(&db, -1, 1_000_000).unwrap();
        assert_eq!(evs[0].category, None);
    }

    #[test]
    fn merge_events_spans_min_start_to_max_end() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let a = open_event(&db, &ev(sid, "Code", 0)).unwrap();
        close_event(&db, a, 10_000).unwrap();
        let b = open_event(&db, &ev(sid, "Code", 10_000)).unwrap();
        close_event(&db, b, 25_000).unwrap();
        let survivor = merge_events(&db, &[a, b]).unwrap().unwrap();
        assert_eq!(survivor, a);
        let evs = events_in_range(&db, -1, 1_000_000).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].started_at, 0);
        assert_eq!(evs[0].ended_at, Some(25_000));
        assert_eq!(evs[0].duration_s, Some(25));
    }

    #[test]
    fn set_category_backfills_existing_events() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let eid = open_event(&db, &ev(sid, "Slack", 0)).unwrap();
        close_event(&db, eid, 1000).unwrap();
        set_category(&db, "Slack", "Comms").unwrap();
        let evs = events_in_range(&db, -1, 100_000).unwrap();
        assert_eq!(evs[0].category.as_deref(), Some("Comms"));
    }

    #[test]
    fn clear_all_wipes_everything() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let eid = open_event(&db, &ev(sid, "Code", 0)).unwrap();
        insert_claude_turn(&db, eid, 0, Some("opus"), Some(10), Some(20)).unwrap();
        clear_all(&db).unwrap();
        assert!(events_in_range(&db, -1, 1_000_000).unwrap().is_empty());
        assert!(active_session(&db).unwrap().is_none());
    }
}
