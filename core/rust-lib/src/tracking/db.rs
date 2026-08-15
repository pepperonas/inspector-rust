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

/// End every non-`ended` session **except** `keep` (the one being resumed),
/// stamping `ended_at` from its last event (else its own start). Cleans up
/// stale duplicate "active" sessions left by older builds / unclean shutdowns.
/// Returns rows changed.
pub fn end_stale_sessions(db: &DbHandle, keep: Option<i64>) -> rusqlite::Result<usize> {
    let conn = db.lock();
    let keep = keep.unwrap_or(-1);
    conn.execute(
        "UPDATE track_sessions \
         SET status = 'ended', \
             ended_at = COALESCE( \
                 (SELECT MAX(ended_at) FROM track_events WHERE session_id = track_sessions.id), \
                 started_at) \
         WHERE status != 'ended' AND id != ?1",
        params![keep],
    )
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

/// Finalize **every** dangling open event (any session) at its `started_at`.
/// Run at startup: a still-`NULL` `ended_at` means an unclean shutdown left it
/// open, and the day report counts such events up to *now* — which silently
/// over-counts (overlapping all later events). Heartbeats keep live events
/// stamped, so legitimately-running events are never NULL here. Returns rows.
pub fn finalize_all_open_events(db: &DbHandle) -> rusqlite::Result<usize> {
    let conn = db.lock();
    conn.execute(
        "UPDATE track_events SET ended_at = started_at, duration_s = 0 WHERE ended_at IS NULL",
        [],
    )
}

/// Enrich the tracker's live event with browser tab metadata (host/title/url) —
/// called by `apply_tick` when the extension's tab report arrives a tick after
/// the browser interval opened (instead of splitting off an "(unknown)"
/// fragment). The caller guarantees `id` is the live event; no `ended_at IS
/// NULL` guard here — the heartbeat stamps `ended_at` every tick, so that
/// condition would make enrichment a permanent no-op.
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
         WHERE id = ?1",
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

/// Insert a **complete** (already-ended) event — for manual time entry. Mirrors
/// `open_event` but stamps `ended_at` + `duration_s`. `window_title`/`url`
/// encrypted as usual. Returns the new id.
pub fn insert_event(db: &DbHandle, ev: &NewEvent, ended_at: i64) -> rusqlite::Result<i64> {
    let conn = db.lock();
    let enc_title = ev.window_title.as_deref().map(crypto::encrypt);
    let enc_url = ev.url.as_deref().map(crypto::encrypt);
    let dur = ((ended_at - ev.started_at).max(0)) / 1000;
    conn.execute(
        "INSERT INTO track_events \
         (session_id, app_name, app_id, window_title, url, host, category, project, source, is_idle, started_at, ended_at, duration_s) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            ev.session_id, ev.app_name, ev.app_id, enc_title, enc_url, ev.host,
            ev.category, ev.project, ev.source, ev.is_idle as i64, ev.started_at, ended_at, dur,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// A session id to attach manual entries to: the active one if tracking, else a
/// reusable already-ended "Manual entries" container session (kept out of the
/// resume path because it's `ended`).
pub fn manual_session_id(db: &DbHandle, now: i64) -> rusqlite::Result<i64> {
    if let Some(s) = active_session(db)? {
        return Ok(s.id);
    }
    {
        let conn = db.lock();
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM track_sessions WHERE label = 'Manual entries' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            return Ok(id);
        }
        conn.execute(
            "INSERT INTO track_sessions (label, started_at, ended_at, status) \
             VALUES ('Manual entries', ?1, ?1, 'ended')",
            params![now],
        )?;
        Ok(conn.last_insert_rowid())
    }
}

/// Delete cleanup-able events in `[from, to)`: all `idle` spans plus any
/// non-idle, non-claude event shorter than `min_seconds` (quick-switch noise).
/// `exclude` protects the tracker's **live** (still-growing) event — its
/// heartbeat-stamped `ended_at` makes it look closed (and a freshly-opened row
/// is naturally short), so without the exclusion a cleanup during tracking
/// would delete the row the heartbeat writes to and silently stop persisting
/// the rest of the focus span. Returns rows deleted.
pub fn cleanup_day(
    db: &DbHandle,
    from: i64,
    to: i64,
    min_seconds: i64,
    exclude: Option<i64>,
) -> rusqlite::Result<usize> {
    let conn = db.lock();
    conn.execute(
        "DELETE FROM track_events \
         WHERE started_at >= ?1 AND started_at < ?2 \
           AND id != ?4 \
           AND ( is_idle = 1 \
                 OR (source != 'claude' AND ended_at IS NOT NULL \
                     AND (ended_at - started_at) / 1000 < ?3) )",
        params![from, to, min_seconds, exclude.unwrap_or(-1)],
    )
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

/// The category rule for an app, if any (used to auto-categorize new events).
pub fn category_for_app(db: &DbHandle, app_name: &str) -> rusqlite::Result<Option<String>> {
    let conn = db.lock();
    conn.query_row(
        "SELECT category FROM track_categories WHERE app_name = ?1",
        params![app_name],
        |r| r.get::<_, String>(0),
    )
    .optional()
}

/// All app→category rules (Settings → Timesheet manager).
pub fn list_category_rules(db: &DbHandle) -> rusqlite::Result<Vec<(String, String)>> {
    let conn = db.lock();
    let mut stmt =
        conn.prepare("SELECT app_name, category FROM track_categories ORDER BY app_name")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    rows.collect()
}

/// Delete an app→category rule (does not un-categorize existing events).
pub fn delete_category_rule(db: &DbHandle, app_name: &str) -> rusqlite::Result<()> {
    let conn = db.lock();
    conn.execute("DELETE FROM track_categories WHERE app_name = ?1", params![app_name])?;
    Ok(())
}

/// Assign a project to the given events (empty/None clears it). Returns rows.
pub fn set_project_for_events(
    db: &DbHandle,
    ids: &[i64],
    project: Option<&str>,
) -> rusqlite::Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    let conn = db.lock();
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("UPDATE track_events SET project = ? WHERE id IN ({placeholders})");
    let proj = project.filter(|s| !s.is_empty());
    let mut p: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(ids.len() + 1);
    p.push(&proj);
    for id in ids {
        p.push(id);
    }
    conn.execute(&sql, p.as_slice())
}

/// Distinct project names ever used (incl. Claude cwd projects) — autocomplete.
pub fn distinct_projects(db: &DbHandle) -> rusqlite::Result<Vec<String>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT DISTINCT project FROM track_events \
         WHERE project IS NOT NULL AND project != '' ORDER BY project",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
}

/// Distinct category names ever used (rules ∪ events) — for assign autocomplete.
pub fn distinct_categories(db: &DbHandle) -> rusqlite::Result<Vec<String>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT category FROM track_categories \
         UNION SELECT category FROM track_events WHERE category IS NOT NULL AND category != '' \
         ORDER BY category",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    rows.collect()
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
pub fn prune_before(db: &DbHandle, cutoff_ms: i64, exclude: Option<i64>) -> rusqlite::Result<usize> {
    // `exclude` protects the currently-open (live) event — same rationale as
    // `cleanup_day`'s exclude: its `started_at` can age past an aggressive
    // retention cutoff (e.g. a long idle span), and deleting the row the run
    // loop's heartbeat writes to silently stops persisting all further time
    // in the current focus span.
    let ex = exclude.unwrap_or(-1);
    let conn = db.lock();
    conn.execute(
        "DELETE FROM track_claude_turns WHERE event_id IN \
         (SELECT id FROM track_events WHERE started_at < ?1 AND id != ?2)",
        params![cutoff_ms, ex],
    )?;
    let n = conn.execute(
        "DELETE FROM track_events WHERE started_at < ?1 AND id != ?2",
        params![cutoff_ms, ex],
    )?;
    // Only ENDED sessions may be swept. The unscoped form deleted any
    // event-less session — including the one currently being recorded into:
    // with retention enabled, a resumed session whose events all aged past the
    // cutoff lost its own row, and because `init_schema` turns foreign keys ON,
    // every later `open_event` for that session_id failed with FOREIGN KEY
    // constraint failed. Tracking then LOOKED alive (no error surfaces to the
    // user) while recording nothing until the next restart. `resume_if_active`
    // prunes with exclude=None right before resuming, so it was reachable.
    conn.execute(
        "DELETE FROM track_sessions \
         WHERE status = 'ended' \
           AND id NOT IN (SELECT DISTINCT session_id FROM track_events)",
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
    fn insert_event_stamps_duration_and_cleanup_removes_idle_and_fragments() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        // A real 10-min entry (kept), a 5s fragment (removed), an idle span (removed).
        let mut keep = ev(sid, "Code", 0);
        keep.window_title = Some("main.rs".into());
        let keep_id = insert_event(&db, &keep, 600_000).unwrap(); // 10 min
        assert_eq!(events_in_range(&db, -1, 10_000_000).unwrap()
            .iter().find(|e| e.id == keep_id).unwrap().duration_s, Some(600));
        let frag = ev(sid, "Finder", 600_000);
        insert_event(&db, &frag, 605_000).unwrap(); // 5 s fragment
        let mut idle = ev(sid, "Code", 605_000);
        idle.is_idle = true;
        insert_event(&db, &idle, 900_000).unwrap(); // idle
        assert_eq!(events_in_range(&db, -1, 10_000_000).unwrap().len(), 3);
        // Clean up: idle + sub-15s → removes the fragment + the idle span.
        let removed = cleanup_day(&db, -1, 10_000_000, 15, None).unwrap();
        assert_eq!(removed, 2);
        let left = events_in_range(&db, -1, 10_000_000).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].app_name, "Code");
        assert!(!left[0].is_idle);
    }

    #[test]
    fn cleanup_day_spares_the_excluded_live_event() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        // A live idle span (heartbeat-stamped ended_at makes it look closed) +
        // a genuinely-old idle span. Excluding the live id must spare it.
        let mut live = ev(sid, "Idle", 0);
        live.is_idle = true;
        let live_id = insert_event(&db, &live, 5_000).unwrap();
        let mut old = ev(sid, "Idle", 10_000);
        old.is_idle = true;
        insert_event(&db, &old, 20_000).unwrap();
        let removed = cleanup_day(&db, -1, 10_000_000, 15, Some(live_id)).unwrap();
        assert_eq!(removed, 1, "only the non-live idle span is deleted");
        let left = events_in_range(&db, -1, 10_000_000).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, live_id);
    }

    #[test]
    fn manual_session_id_reuses_one_container_when_not_tracking() {
        let db = test_db();
        let a = manual_session_id(&db, 1_000).unwrap();
        let b = manual_session_id(&db, 2_000).unwrap();
        assert_eq!(a, b); // same reused "Manual entries" session
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
    fn enrich_updates_event_even_after_heartbeat() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let eid = open_event(&db, &ev(sid, "Safari", 0)).unwrap();
        enrich_event(&db, eid, Some("github.com"), Some("Title"), Some("https://github.com")).unwrap();
        let evs = events_in_range(&db, -1, 100).unwrap();
        assert_eq!(evs[0].host.as_deref(), Some("github.com"));
        assert_eq!(evs[0].window_title.as_deref(), Some("Title"));
        // The live event's ended_at is heartbeat-stamped every tick — enrichment
        // must still work then (the caller guarantees the id is the live event;
        // the old `ended_at IS NULL` guard made it a permanent no-op).
        touch_event(&db, eid, 50).unwrap();
        enrich_event(&db, eid, Some("other.com"), None, None).unwrap();
        let evs = events_in_range(&db, -1, 100).unwrap();
        assert_eq!(evs[0].host.as_deref(), Some("other.com"));
        // COALESCE keeps existing values when the report carries None.
        assert_eq!(evs[0].window_title.as_deref(), Some("Title"));
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

    /// Count helper for the tables without a typed reader.
    fn count(db: &DbHandle, table: &str) -> i64 {
        db.lock()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    fn session_row(db: &DbHandle, id: i64) -> Option<(String, Option<i64>)> {
        db.lock()
            .query_row(
                "SELECT status, ended_at FROM track_sessions WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .unwrap()
    }

    #[test]
    fn init_schema_can_run_again_on_an_existing_database() {
        // It runs on every `db::open`, so a second call over live data must be
        // a no-op rather than an error (or a wipe).
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn.execute(
            "INSERT INTO track_sessions (label, started_at, ended_at, status) VALUES ('x', 1, NULL, 'active')",
            [],
        )
        .unwrap();
        init_schema(&conn).expect("second run must succeed");
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM track_sessions", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "existing rows must survive re-initialisation");
    }

    #[test]
    fn prune_before_deletes_old_events_together_with_their_claude_turns() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let old = insert_event(&db, &ev(sid, "Code", 1_000), 2_000).unwrap();
        insert_claude_turn(&db, old, 1_500, Some("opus"), Some(10), Some(20)).unwrap();
        let recent = insert_event(&db, &ev(sid, "Code", 50_000), 60_000).unwrap();
        insert_claude_turn(&db, recent, 55_000, Some("opus"), Some(1), Some(2)).unwrap();

        assert_eq!(prune_before(&db, 10_000, None).unwrap(), 1);

        let left = events_in_range(&db, -1, 1_000_000).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, recent);
        assert_eq!(count(&db, "track_claude_turns"), 1, "the old event's turns go with it");
    }

    #[test]
    fn prune_before_removes_sessions_it_emptied_and_keeps_the_populated_ones() {
        let db = test_db();
        let old_session = start_session(&db, Some("last month"), 0).unwrap();
        insert_event(&db, &ev(old_session, "Code", 1_000), 2_000).unwrap();
        // A month-old session is ENDED in reality (`end_stale_sessions` closes
        // every leftover on the next resume) — and only ended sessions may be
        // swept, see the live-session test below.
        end_session(&db, old_session, 2_000).unwrap();
        let live_session = start_session(&db, Some("today"), 100_000).unwrap();
        insert_event(&db, &ev(live_session, "Code", 100_000), 110_000).unwrap();

        prune_before(&db, 50_000, None).unwrap();

        assert!(session_row(&db, old_session).is_none(), "an emptied session is cleaned up");
        assert!(session_row(&db, live_session).is_some(), "a session with events stays");
    }

    #[test]
    fn prune_before_never_deletes_the_session_that_is_still_recording() {
        // REGRESSION (found 2026-08-15): the session sweep was unscoped, so a
        // session whose events had all aged past the cutoff lost its own row —
        // including the one being recorded into. With foreign keys ON, every
        // later open_event on that id then failed and tracking silently
        // recorded NOTHING until the next app restart. Reachable because
        // `resume_if_active` prunes (exclude=None) right before resuming.
        let db = test_db();
        let live = start_session(&db, Some("resumed"), 0).unwrap();
        insert_event(&db, &ev(live, "Code", 1_000), 2_000).unwrap();

        prune_before(&db, 50_000, None).unwrap();

        assert!(
            session_row(&db, live).is_some(),
            "an ACTIVE session must survive even once the prune emptied it"
        );
        // The load-bearing half: it can still be recorded into.
        assert!(
            insert_event(&db, &ev(live, "Code", 60_000), 61_000).is_ok(),
            "recording into the surviving session must not hit a FK error"
        );
    }

    #[test]
    fn end_stale_sessions_closes_every_other_session_at_its_last_known_moment() {
        // Unclean shutdowns (and older builds) leave several "active" rows; on
        // resume exactly one may stay open, and the others must be closed at a
        // truthful time — never at "now", which would invent hours of work.
        let db = test_db();
        let with_events = start_session(&db, None, 1_000).unwrap();
        insert_event(&db, &ev(with_events, "Code", 1_000), 4_000).unwrap();
        let empty = start_session(&db, None, 5_000).unwrap();
        let resumed = start_session(&db, None, 9_000).unwrap();

        assert_eq!(end_stale_sessions(&db, Some(resumed)).unwrap(), 2);

        assert_eq!(session_row(&db, with_events).unwrap(), ("ended".into(), Some(4_000)));
        assert_eq!(session_row(&db, empty).unwrap(), ("ended".into(), Some(5_000)));
        assert_eq!(active_session(&db).unwrap().unwrap().id, resumed);
    }

    #[test]
    fn finalize_all_open_events_closes_danglers_from_every_session() {
        // The per-session variant only reaches the resumed session; a crash of
        // an older build can leave open rows elsewhere, and an open row is
        // counted up to *now* by the day report — overlapping everything after.
        let db = test_db();
        let a = start_session(&db, None, 0).unwrap();
        let b = start_session(&db, None, 0).unwrap();
        let dangling_a = open_event(&db, &ev(a, "Code", 1_000)).unwrap();
        let dangling_b = open_event(&db, &ev(b, "Safari", 2_000)).unwrap();
        let closed = insert_event(&db, &ev(b, "Mail", 3_000), 9_000).unwrap();

        assert_eq!(finalize_all_open_events(&db).unwrap(), 2);

        let evs = events_in_range(&db, -1, 1_000_000).unwrap();
        let by_id = |id: i64| evs.iter().find(|e| e.id == id).unwrap();
        assert_eq!((by_id(dangling_a).ended_at, by_id(dangling_a).duration_s), (Some(1_000), Some(0)));
        assert_eq!((by_id(dangling_b).ended_at, by_id(dangling_b).duration_s), (Some(2_000), Some(0)));
        assert_eq!(by_id(closed).duration_s, Some(6), "an already-closed event is untouched");
        assert_eq!(finalize_all_open_events(&db).unwrap(), 0, "second run finds nothing");
    }

    #[test]
    fn close_event_clamps_a_backwards_clock_and_refuses_to_reopen_the_books() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let eid = open_event(&db, &ev(sid, "Code", 10_000)).unwrap();
        // Clock skew / a DST jump: the end lands before the start.
        close_event(&db, eid, 5_000).unwrap();
        let e = events_in_range(&db, -1, 1_000_000).unwrap().remove(0);
        assert_eq!(e.ended_at, Some(5_000));
        assert_eq!(e.duration_s, Some(0), "never a negative duration");

        // A second close is ignored (the `ended_at IS NULL` guard) — unlike the
        // heartbeat, which deliberately keeps moving a live event's end.
        close_event(&db, eid, 90_000).unwrap();
        let e = events_in_range(&db, -1, 1_000_000).unwrap().remove(0);
        assert_eq!(e.ended_at, Some(5_000), "a closed event stays closed where it was");
    }

    #[test]
    fn merging_a_still_open_event_leaves_the_survivor_open() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let a = open_event(&db, &ev(sid, "Code", 0)).unwrap();
        close_event(&db, a, 10_000).unwrap();
        let b = open_event(&db, &ev(sid, "Code", 10_000)).unwrap(); // still running

        assert_eq!(merge_events(&db, &[a, b]).unwrap(), Some(a));
        let evs = events_in_range(&db, -1, 1_000_000).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].started_at, 0, "spans from the earliest start");
        assert_eq!(evs[0].ended_at, None, "and is still open, so the heartbeat keeps it growing");
        assert_eq!(evs[0].duration_s, None);
    }

    #[test]
    fn merge_never_deletes_anything_when_fewer_than_two_rows_are_selected() {
        // A stale id in the selection (the row was deleted in another view)
        // must not turn "merge these two" into "delete the good one".
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let a = insert_event(&db, &ev(sid, "Code", 0), 10_000).unwrap();

        assert_eq!(merge_events(&db, &[]).unwrap(), None);
        assert_eq!(merge_events(&db, &[a]).unwrap(), Some(a));
        assert_eq!(merge_events(&db, &[a, 9_999]).unwrap(), Some(a));
        let evs = events_in_range(&db, -1, 1_000_000).unwrap();
        assert_eq!(evs.len(), 1);
        assert_eq!((evs[0].started_at, evs[0].ended_at), (0, Some(10_000)));
    }

    #[test]
    fn events_in_range_takes_overlaps_but_not_merely_touching_neighbours() {
        // Half-open [from, to): the day report must not count an event twice on
        // two consecutive days, and must still show a block that spans midnight.
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let before = insert_event(&db, &ev(sid, "Ends at from", 0), 1_000).unwrap();
        let spanning = insert_event(&db, &ev(sid, "Spans", 500), 3_000).unwrap();
        let inside = insert_event(&db, &ev(sid, "Inside", 1_200), 1_800).unwrap();
        let after = insert_event(&db, &ev(sid, "Starts at to", 2_000), 2_500).unwrap();
        let open = open_event(&db, &ev(sid, "Open since before", 900)).unwrap();

        let ids: Vec<i64> = events_in_range(&db, 1_000, 2_000)
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect();
        assert!(ids.contains(&spanning) && ids.contains(&inside) && ids.contains(&open));
        assert!(!ids.contains(&before), "an event ending exactly at `from` is outside");
        assert!(!ids.contains(&after), "an event starting exactly at `to` is outside");
    }

    #[test]
    fn cleanup_spares_claude_turns_and_events_that_are_still_running() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let mut claude = ev(sid, "Claude", 0);
        claude.source = "claude".into();
        let claude_id = insert_event(&db, &claude, 3_000).unwrap(); // 3 s, but claude
        let running = open_event(&db, &ev(sid, "Code", 4_000)).unwrap(); // no end yet
        insert_event(&db, &ev(sid, "Finder", 5_000), 8_000).unwrap(); // 3 s fragment

        assert_eq!(cleanup_day(&db, -1, 1_000_000, 15, None).unwrap(), 1);
        let ids: Vec<i64> = events_in_range(&db, -1, 1_000_000).unwrap().iter().map(|e| e.id).collect();
        assert!(ids.contains(&claude_id), "short Claude turns are real work");
        assert!(ids.contains(&running), "an event without an end is not a fragment yet");
    }

    #[test]
    fn claude_tokens_are_summed_per_project_only_for_claude_turns_in_range() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let mut tagged = ev(sid, "Claude", 0);
        tagged.source = "claude".into();
        tagged.project = Some("alpha".into());
        let tagged_id = insert_event(&db, &tagged, 10_000).unwrap();
        insert_claude_turn(&db, tagged_id, 1_000, Some("opus"), Some(10), Some(20)).unwrap();
        insert_claude_turn(&db, tagged_id, 2_000, Some("opus"), Some(1), Some(2)).unwrap();
        insert_claude_turn(&db, tagged_id, 99_000, Some("opus"), Some(500), Some(500)).unwrap(); // out of range

        let mut untagged = ev(sid, "Claude", 0);
        untagged.source = "claude".into();
        let untagged_id = insert_event(&db, &untagged, 10_000).unwrap();
        insert_claude_turn(&db, untagged_id, 1_500, None, Some(7), None).unwrap();

        // A focus event that somehow carries a turn is NOT Claude usage.
        let focus_id = insert_event(&db, &ev(sid, "Code", 0), 10_000).unwrap();
        insert_claude_turn(&db, focus_id, 1_500, None, Some(999), Some(999)).unwrap();

        let map = claude_tokens_by_project(&db, 0, 50_000).unwrap();
        assert_eq!(map.get("alpha"), Some(&(11, 22)));
        assert_eq!(map.get("(unknown)"), Some(&(7, 0)), "a missing tokens_out counts as 0");
        assert_eq!(map.len(), 2, "only source='claude' events contribute");
    }

    #[test]
    fn deleting_a_category_rule_leaves_already_categorised_events_alone() {
        // Documented behaviour: the rule only decides what NEW events get; past
        // bookings keep the classification the user already reviewed.
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let eid = insert_event(&db, &ev(sid, "Slack", 0), 1_000).unwrap();
        set_category(&db, "Slack", "Comms").unwrap();
        assert_eq!(category_for_app(&db, "Slack").unwrap().as_deref(), Some("Comms"));

        delete_category_rule(&db, "Slack").unwrap();

        assert_eq!(category_for_app(&db, "Slack").unwrap(), None);
        assert_eq!(category_for_app(&db, "Never seen").unwrap(), None);
        let e = events_in_range(&db, -1, 1_000_000).unwrap().remove(0);
        assert_eq!(e.id, eid);
        assert_eq!(e.category.as_deref(), Some("Comms"), "the event keeps its category");
    }

    #[test]
    fn projects_can_be_assigned_in_bulk_and_cleared_with_an_empty_name() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let a = insert_event(&db, &ev(sid, "Code", 0), 1_000).unwrap();
        let b = insert_event(&db, &ev(sid, "Code", 1_000), 2_000).unwrap();

        assert_eq!(set_project_for_events(&db, &[a, b], Some("alpha")).unwrap(), 2);
        assert_eq!(distinct_projects(&db).unwrap(), vec!["alpha".to_string()]);

        assert_eq!(set_project_for_events(&db, &[a], Some("")).unwrap(), 1);
        let evs = events_in_range(&db, -1, 1_000_000).unwrap();
        assert_eq!(evs.iter().find(|e| e.id == a).unwrap().project, None);
        assert_eq!(evs.iter().find(|e| e.id == b).unwrap().project.as_deref(), Some("alpha"));

        assert_eq!(set_project_for_events(&db, &[], Some("alpha")).unwrap(), 0, "no ids, no writes");
    }

    #[test]
    fn distinct_categories_unions_the_rules_with_what_events_actually_carry() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let eid = insert_event(&db, &ev(sid, "Cursor", 0), 1_000).unwrap();
        // A category that exists only on an event (assigned by hand)…
        update_event(&db, eid, &EventPatch { category: Some("Deep work".into()), ..Default::default() })
            .unwrap();
        // …and one that exists only as a rule (no event of that app yet).
        set_category(&db, "Zoom", "Meetings").unwrap();

        assert_eq!(
            distinct_categories(&db).unwrap(),
            vec!["Deep work".to_string(), "Meetings".to_string()],
            "rules ∪ events, deduped and sorted"
        );
    }

    #[test]
    fn manual_entries_attach_to_the_running_session_while_tracking() {
        let db = test_db();
        let sid = start_session(&db, Some("work"), 1_000).unwrap();
        assert_eq!(manual_session_id(&db, 2_000).unwrap(), sid);
        assert_eq!(count(&db, "track_sessions"), 1, "no container session is created");
        // Once tracking stops, manual entries get their own reusable container.
        end_session(&db, sid, 3_000).unwrap();
        let container = manual_session_id(&db, 4_000).unwrap();
        assert_ne!(container, sid);
        assert_eq!(manual_session_id(&db, 5_000).unwrap(), container);
    }

    #[test]
    fn a_manual_entry_ending_before_it_starts_gets_a_zero_duration() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let eid = insert_event(&db, &ev(sid, "Code", 10_000), 5_000).unwrap();
        let e = events_in_range(&db, -1, 1_000_000).unwrap().remove(0);
        assert_eq!(e.id, eid);
        assert_eq!(e.duration_s, Some(0));
    }

    #[test]
    fn moving_only_the_start_recomputes_the_duration_from_the_stored_end() {
        let db = test_db();
        let sid = start_session(&db, None, 0).unwrap();
        let eid = insert_event(&db, &ev(sid, "Code", 60_000), 120_000).unwrap(); // 60 s
        update_event(&db, eid, &EventPatch { started_at: Some(0), ..Default::default() }).unwrap();
        let e = events_in_range(&db, -1, 1_000_000).unwrap().remove(0);
        assert_eq!((e.started_at, e.ended_at), (0, Some(120_000)));
        assert_eq!(e.duration_s, Some(120), "the duration follows the edited bounds");
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

