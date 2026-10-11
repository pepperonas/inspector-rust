//! The `spotify_plays` table in `history.db`: one row per play.
//!
//! Rows are written while a play is open (`open = 1`, status `playing`) so a
//! crash or quit loses at most one tick; the tracker finalises them. Reads
//! are defensive: a damaged row is repaired on the way out, never dropped.

use super::{Change, Play, PlayStatus, PlayerState, Track, MIN_RECORD_MS};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS spotify_plays (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            started_at INTEGER NOT NULL,
            ended_at INTEGER NOT NULL,
            track_uri TEXT NOT NULL DEFAULT '',
            track TEXT NOT NULL DEFAULT '',
            artist TEXT NOT NULL DEFAULT '',
            album TEXT NOT NULL DEFAULT '',
            duration_ms INTEGER,
            played_ms INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'partial',
            artwork_url TEXT,
            open INTEGER NOT NULL DEFAULT 0,
            last_at INTEGER NOT NULL DEFAULT 0,
            position_ms INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_spotify_plays_started
            ON spotify_plays(started_at DESC);",
    )
}

/// One history row as the UI and the export see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlayRow {
    pub id: i64,
    pub started_at: i64,
    pub ended_at: i64,
    pub track_uri: String,
    pub track: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: Option<u64>,
    pub played_ms: u64,
    pub status: PlayStatus,
    pub artwork_url: Option<String>,
    /// Still playing / paused right now.
    pub open: bool,
}

/// Filters for `query`. All optional; text matches track, artist or album,
/// `artist` only the artist (both case-insensitive, Unicode-aware).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HistoryQuery {
    pub text: Option<String>,
    pub artist: Option<String>,
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryPage {
    pub rows: Vec<PlayRow>,
    /// Matching rows in total (for "load more").
    pub total: u32,
    /// Heard time across all matching rows.
    pub played_ms: u64,
}

const COLS: &str = "id, started_at, ended_at, track_uri, track, artist, album, duration_ms, played_ms, status, artwork_url, open";

fn row(r: &rusqlite::Row) -> rusqlite::Result<PlayRow> {
    let started_at: i64 = r.get(1)?;
    let ended_at: i64 = r.get(2)?;
    let played: i64 = r.get(8)?;
    let duration: Option<i64> = r.get(7)?;
    let status: String = r.get(9)?;
    let open: i64 = r.get(11)?;
    Ok(PlayRow {
        id: r.get(0)?,
        started_at,
        // Repair instead of hiding: an end before the start is the start.
        ended_at: ended_at.max(started_at),
        track_uri: r.get(3)?,
        track: r.get(4)?,
        artist: r.get(5)?,
        album: r.get(6)?,
        duration_ms: duration.filter(|d| *d > 0).map(|d| d as u64),
        played_ms: played.max(0) as u64,
        status: PlayStatus::parse(&status),
        artwork_url: r.get::<_, Option<String>>(10)?.filter(|s| !s.is_empty()),
        open: open != 0,
    })
}

fn write(conn: &Connection, play: &Play, open: bool) -> rusqlite::Result<i64> {
    let t = &play.track;
    let values = params![
        play.started_at,
        play.ended_at,
        t.uri,
        t.name,
        t.artist,
        t.album,
        t.duration_ms.map(|d| d as i64),
        play.played_ms as i64,
        play.status(open).as_str(),
        t.artwork_url,
        open as i64,
        play.last_at,
        play.position_ms.map(|v| v as i64),
        play.row_id,
    ];
    if play.row_id.is_some() {
        conn.execute(
            "UPDATE spotify_plays SET started_at=?1, ended_at=?2, track_uri=?3, track=?4, artist=?5,
             album=?6, duration_ms=?7, played_ms=?8, status=?9, artwork_url=?10, open=?11,
             last_at=?12, position_ms=?13 WHERE id=?14",
            values,
        )?;
        Ok(play.row_id.unwrap_or_default())
    } else {
        conn.execute(
            "INSERT INTO spotify_plays (started_at, ended_at, track_uri, track, artist, album,
             duration_ms, played_ms, status, artwork_url, open, last_at, position_ms)
             SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13 WHERE ?14 IS NULL",
            values,
        )?;
        Ok(conn.last_insert_rowid())
    }
}

/// Apply what the tracker decided. Returns the row id of the open play once
/// it exists, so the caller can hand it back to the tracker.
///
/// * an open play is only written once it reached `MIN_RECORD_MS`;
/// * a finished play that never got there is removed (or never written).
pub fn apply(conn: &Connection, change: &Change) -> rusqlite::Result<Option<i64>> {
    match change {
        Change::Update(p) => {
            if p.row_id.is_none() && p.played_ms < MIN_RECORD_MS {
                return Ok(None);
            }
            write(conn, p, true).map(Some)
        }
        Change::Finish(p) => {
            if p.played_ms < MIN_RECORD_MS {
                if let Some(id) = p.row_id {
                    conn.execute("DELETE FROM spotify_plays WHERE id=?1", [id])?;
                }
                return Ok(None);
            }
            write(conn, p, false)?;
            Ok(None)
        }
    }
}

/// The entry a previous run left open (the newest one). Older open rows —
/// only possible after a crash in an older build — are closed on the spot.
pub fn take_open(conn: &Connection) -> rusqlite::Result<Option<Play>> {
    let mut stmt = conn.prepare(
        "SELECT id, started_at, ended_at, track_uri, track, artist, album, duration_ms,
                played_ms, artwork_url, last_at, position_ms
         FROM spotify_plays WHERE open = 1 ORDER BY started_at DESC, id DESC",
    )?;
    let open: Vec<Play> = stmt
        .query_map([], |r| {
            let started_at: i64 = r.get(1)?;
            let ended_at: i64 = r.get(2)?;
            let last_at: i64 = r.get(10)?;
            Ok(Play {
                row_id: Some(r.get(0)?),
                track: Track {
                    uri: r.get(3)?,
                    name: r.get(4)?,
                    artist: r.get(5)?,
                    album: r.get(6)?,
                    duration_ms: r.get::<_, Option<i64>>(7)?.filter(|d| *d > 0).map(|d| d as u64),
                    artwork_url: r.get(9)?,
                },
                started_at,
                ended_at: ended_at.max(started_at),
                played_ms: r.get::<_, i64>(8)?.max(0) as u64,
                // Unknown whether it was playing; the tracker decides.
                state: PlayerState::Paused,
                last_at: last_at.max(ended_at).max(started_at),
                paused_since: None,
                position_ms: r.get::<_, Option<i64>>(11)?.map(|v| v.max(0) as u64),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut it = open.into_iter();
    let newest = it.next();
    for stale in it {
        apply(conn, &Change::Finish(stale))?;
    }
    Ok(newest)
}

fn matches(row: &PlayRow, text: Option<&str>, artist: Option<&str>) -> bool {
    let has = |hay: &str, needle: &str| hay.to_lowercase().contains(needle);
    if let Some(a) = artist {
        if !has(&row.artist, a) {
            return false;
        }
    }
    match text {
        None => true,
        Some(t) => t
            .split_whitespace()
            .all(|w| has(&row.track, w) || has(&row.artist, w) || has(&row.album, w)),
    }
}

/// Newest first, filtered, paged. Text filters run in Rust (Unicode case
/// folding — SQLite's `LIKE` only folds ASCII, so `ä` would miss `Ä`).
pub fn query(conn: &Connection, q: &HistoryQuery) -> rusqlite::Result<HistoryPage> {
    let since = q.since_ms.unwrap_or(i64::MIN);
    let until = q.until_ms.unwrap_or(i64::MAX);
    let norm = |s: &Option<String>| {
        s.as_deref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
    };
    let (text, artist) = (norm(&q.text), norm(&q.artist));
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM spotify_plays WHERE started_at >= ?1 AND started_at < ?2
         ORDER BY started_at DESC, id DESC"
    ))?;
    let all: Vec<PlayRow> = stmt
        .query_map(params![since, until], row)?
        .filter_map(Result::ok)
        .filter(|r| matches(r, text.as_deref(), artist.as_deref()))
        .collect();
    let total = all.len() as u32;
    let played_ms = all.iter().map(|r| r.played_ms).sum();
    let offset = q.offset.unwrap_or(0) as usize;
    let limit = q.limit.map(|l| l as usize).unwrap_or(usize::MAX);
    Ok(HistoryPage {
        rows: all.into_iter().skip(offset).take(limit).collect(),
        total,
        played_ms,
    })
}

/// Every row, oldest first — the export's order.
pub fn all(conn: &Connection, q: &HistoryQuery) -> rusqlite::Result<Vec<PlayRow>> {
    let mut page = query(
        conn,
        &HistoryQuery {
            limit: None,
            offset: None,
            ..q.clone()
        },
    )?;
    page.rows.reverse();
    Ok(page.rows)
}

pub fn delete(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM spotify_plays WHERE id=?1 AND open = 0", [id])
        .map(|_| ())
}

/// Remove all finished rows (the one playing right now stays).
pub fn clear(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM spotify_plays WHERE open = 0", [])
}

#[cfg(test)]
pub fn count(conn: &Connection) -> rusqlite::Result<u32> {
    conn.query_row("SELECT COUNT(*) FROM spotify_plays", [], |r| r.get(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spotify::{Observation, Tracker};

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        init_schema(&c).unwrap();
        c
    }

    fn track(uri: &str, name: &str, artist: &str, album: &str) -> Track {
        Track {
            uri: uri.into(),
            name: name.into(),
            artist: artist.into(),
            album: album.into(),
            duration_ms: Some(200_000),
            artwork_url: None,
        }
    }

    /// Drive a tracker and persist like the recorder does.
    fn feed(conn: &Connection, t: &mut Tracker, changes: Vec<Change>) {
        for c in changes {
            if let Some(id) = apply(conn, &c).unwrap() {
                t.set_row_id(id);
            }
        }
    }

    fn play(conn: &Connection, t: &mut Tracker, tr: Track, start: i64, secs: i64) {
        let o = |at, state| Observation {
            at_ms: at,
            state,
            track: Some(tr.clone()),
            position_ms: None,
        };
        let c = t.observe(&o(start, PlayerState::Playing));
        feed(conn, t, c);
        let mut now = start;
        while now < start + secs * 1000 {
            now = (now + 15_000).min(start + secs * 1000);
            let c = t.tick(now);
            feed(conn, t, c);
        }
    }

    #[test]
    fn plays_are_written_while_open_and_finalised_once() {
        let c = db();
        let mut t = Tracker::new();
        play(&c, &mut t, track("spotify:track:a", "Wicked Games", "Parra for Cuva", "Paper Planes"), 0, 120);
        let page = query(&c, &HistoryQuery::default()).unwrap();
        assert_eq!(page.total, 1);
        assert!(page.rows[0].open);
        assert_eq!(page.rows[0].status, PlayStatus::Playing);
        // The track changes: the same row is finalised, no duplicate.
        play(&c, &mut t, track("spotify:track:b", "B", "X", "Y"), 120_000, 60);
        let page = query(&c, &HistoryQuery::default()).unwrap();
        assert_eq!(page.total, 2);
        let a = page.rows.iter().find(|r| r.track == "Wicked Games").unwrap();
        assert!(!a.open);
        assert_eq!(a.played_ms, 120_000);
        assert_eq!(a.status, PlayStatus::Listened);
        assert_eq!(page.played_ms, 180_000);
    }

    #[test]
    fn touches_shorter_than_the_minimum_never_become_history() {
        let c = db();
        let mut t = Tracker::new();
        play(&c, &mut t, track("spotify:track:a", "A", "X", "Y"), 0, 3);
        // Not even while it is still open (a quick skip must not flash in).
        assert_eq!(count(&c).unwrap(), 0);
        play(&c, &mut t, track("spotify:track:b", "B", "X", "Y"), 3_000, 30);
        let page = query(&c, &HistoryQuery::default()).unwrap();
        assert_eq!(page.rows.iter().map(|r| r.track.as_str()).collect::<Vec<_>>(), ["B"]);
    }

    #[test]
    fn an_open_row_survives_a_restart_and_is_continued() {
        let c = db();
        let mut t = Tracker::new();
        let a = track("spotify:track:a", "A", "X", "Y");
        play(&c, &mut t, a.clone(), 0, 60);
        drop(t); // Inspector quits.
        let open = take_open(&c).unwrap().expect("open row");
        assert_eq!(open.played_ms, 60_000);
        let mut t = Tracker::with_open(Some(open));
        play(&c, &mut t, a, 90_000, 30);
        let page = query(&c, &HistoryQuery::default()).unwrap();
        assert_eq!(page.total, 1, "continued, not duplicated");
        assert_eq!(page.rows[0].played_ms, 90_000);
    }

    #[test]
    fn stale_open_rows_from_a_crash_are_closed() {
        let c = db();
        for (id, start) in [(1, 0), (2, 1_000_000)] {
            c.execute(
                "INSERT INTO spotify_plays (id, started_at, ended_at, track, played_ms, status, open, last_at)
                 VALUES (?1, ?2, ?2, 'T', 60000, 'playing', 1, ?2)",
                params![id, start],
            )
            .unwrap();
        }
        let newest = take_open(&c).unwrap().unwrap();
        assert_eq!(newest.row_id, Some(2));
        let r = query(&c, &HistoryQuery::default()).unwrap();
        let old = r.rows.iter().find(|r| r.id == 1).unwrap();
        assert!(!old.open);
        // 60 s, length unknown → partial.
        assert_eq!(old.status, PlayStatus::Partial);
    }

    #[test]
    fn damaged_rows_are_repaired_on_read() {
        let c = db();
        c.execute(
            "INSERT INTO spotify_plays (started_at, ended_at, track, duration_ms, played_ms, status, open)
             VALUES (5000, 10, '', -3, -50, 'garbage', 0)",
            [],
        )
        .unwrap();
        let r = &query(&c, &HistoryQuery::default()).unwrap().rows[0];
        assert_eq!(r.ended_at, 5000);
        assert_eq!(r.played_ms, 0);
        assert_eq!(r.duration_ms, None);
        assert_eq!(r.status, PlayStatus::Partial);
    }

    #[test]
    fn search_filters_paging_and_ranges() {
        let c = db();
        let mut t = Tracker::new();
        let tracks = [
            ("Wicked Games", "Parra for Cuva", "Paper Planes"),
            ("Runaway", "Kanye West", "MBDTF"),
            ("Über", "Ärzte", "Jazz"),
            ("Power", "Kanye West", "MBDTF"),
        ];
        for (i, (n, a, al)) in tracks.iter().enumerate() {
            play(&c, &mut t, track(&format!("spotify:track:{i}"), n, a, al), i as i64 * 100_000, 60);
        }
        let q = |text: Option<&str>, artist: Option<&str>| {
            query(
                &c,
                &HistoryQuery {
                    text: text.map(Into::into),
                    artist: artist.map(Into::into),
                    ..Default::default()
                },
            )
            .unwrap()
            .rows
            .into_iter()
            .map(|r| r.track)
            .collect::<Vec<_>>()
        };
        assert_eq!(q(Some("kanye"), None), ["Power", "Runaway"]);
        assert_eq!(q(Some("mbdtf power"), None), ["Power"]);
        assert_eq!(q(Some("ärzte"), None), ["Über"], "Unicode case folding");
        assert_eq!(q(None, Some("west")), ["Power", "Runaway"]);
        assert_eq!(q(Some("paper"), None), ["Wicked Games"], "album matches");
        let page = query(&c, &HistoryQuery { limit: Some(2), offset: Some(1), ..Default::default() }).unwrap();
        assert_eq!(page.total, 4);
        assert_eq!(page.rows.iter().map(|r| r.track.as_str()).collect::<Vec<_>>(), ["Über", "Runaway"]);
        let range = query(&c, &HistoryQuery { since_ms: Some(100_000), until_ms: Some(300_000), ..Default::default() }).unwrap();
        assert_eq!(range.rows.iter().map(|r| r.track.as_str()).collect::<Vec<_>>(), ["Über", "Runaway"]);
        assert_eq!(all(&c, &HistoryQuery::default()).unwrap()[0].track, "Wicked Games", "export is oldest first");
    }

    #[test]
    fn clear_and_delete_never_touch_the_open_play() {
        let c = db();
        let mut t = Tracker::new();
        play(&c, &mut t, track("spotify:track:a", "A", "X", "Y"), 0, 60);
        play(&c, &mut t, track("spotify:track:b", "B", "X", "Y"), 60_000, 60);
        let rows = query(&c, &HistoryQuery::default()).unwrap().rows;
        let open = rows.iter().find(|r| r.open).unwrap().id;
        delete(&c, open).unwrap();
        assert_eq!(clear(&c).unwrap(), 1);
        assert_eq!(count(&c).unwrap(), 1);
    }
}
