//! `pulse.db` — a separate SQLite file next to `history.db` (WAL).
//!
//! Data model: one table `pulse_agg` keyed by (level, ts), level = bucket size
//! in minutes (1, 15, 1440). Raw 5-s samples never reach the disk — they live
//! in the sampler's RAM ring for the last hour. Once a minute the sampler
//! writes ONE 1-min row and re-derives the 15-min and day rows that contain it
//! (two upserts), all in one transaction: the logger's own write load is three
//! small rows a minute.
//!
//! Retention: 1-min rows 7 days, 15-min rows 90 days, day rows forever.
//! A day row is derived from 15-min rows, so it is final before those expire.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;

use super::{local_day_start, merge_aggs, Agg, Causer, SmartInfo, KEEP_15M_SECS, KEEP_1M_SECS, LEVEL_15M, LEVEL_1M, LEVEL_DAY};

pub fn default_path() -> Result<PathBuf> {
    let mut p = crate::db::default_db_path()?;
    p.set_file_name("pulse.db");
    Ok(p)
}

pub fn open(path: &PathBuf) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    init(&conn)?;
    Ok(conn)
}

pub fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS pulse_agg (
            level        INTEGER NOT NULL,
            ts           INTEGER NOT NULL,
            secs         REAL NOT NULL,
            p_normal     REAL NOT NULL,
            p_warn       REAL NOT NULL,
            p_crit       REAL NOT NULL,
            swap_min     INTEGER NOT NULL,
            swap_avg     REAL NOT NULL,
            swap_max     INTEGER NOT NULL,
            swap_total   INTEGER NOT NULL,
            written      REAL NOT NULL,
            read         REAL NOT NULL,
            swap_written REAL NOT NULL,
            swapped_in   REAL NOT NULL,
            compressions REAL NOT NULL,
            cpu_avg      REAL,
            causers      TEXT NOT NULL DEFAULT '[]',
            PRIMARY KEY (level, ts)
        ) WITHOUT ROWID;
        CREATE TABLE IF NOT EXISTS pulse_smart (
            id    INTEGER PRIMARY KEY CHECK (id = 1),
            json  TEXT NOT NULL
        );
        "#,
    )?;
    Ok(())
}

pub fn upsert(conn: &Connection, level: i64, a: &Agg) -> Result<()> {
    let causers = serde_json::to_string(&a.causers)?;
    conn.execute(
        "INSERT OR REPLACE INTO pulse_agg
         (level, ts, secs, p_normal, p_warn, p_crit, swap_min, swap_avg, swap_max, swap_total,
          written, read, swap_written, swapped_in, compressions, cpu_avg, causers)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        params![
            level,
            a.ts,
            a.secs,
            a.p_normal,
            a.p_warn,
            a.p_crit,
            a.swap_min as i64,
            a.swap_avg,
            a.swap_max as i64,
            a.swap_total as i64,
            a.written,
            a.read,
            a.swap_written,
            a.swapped_in,
            a.compressions,
            a.cpu_avg,
            causers,
        ],
    )?;
    Ok(())
}

/// Rows of one level with `from <= ts < to`, oldest first.
pub fn range(conn: &Connection, level: i64, from: i64, to: i64) -> Result<Vec<Agg>> {
    let mut stmt = conn.prepare_cached(
        "SELECT ts, secs, p_normal, p_warn, p_crit, swap_min, swap_avg, swap_max, swap_total,
                written, read, swap_written, swapped_in, compressions, cpu_avg, causers
         FROM pulse_agg WHERE level = ?1 AND ts >= ?2 AND ts < ?3 ORDER BY ts",
    )?;
    let rows = stmt.query_map(params![level, from, to], |r| {
        let causers: String = r.get(15)?;
        Ok(Agg {
            ts: r.get(0)?,
            secs: r.get(1)?,
            p_normal: r.get(2)?,
            p_warn: r.get(3)?,
            p_crit: r.get(4)?,
            swap_min: r.get::<_, i64>(5)?.max(0) as u64,
            swap_avg: r.get(6)?,
            swap_max: r.get::<_, i64>(7)?.max(0) as u64,
            swap_total: r.get::<_, i64>(8)?.max(0) as u64,
            written: r.get(9)?,
            read: r.get(10)?,
            swap_written: r.get(11)?,
            swapped_in: r.get(12)?,
            compressions: r.get(13)?,
            cpu_avg: r.get(14)?,
            causers: serde_json::from_str::<Vec<Causer>>(&causers).unwrap_or_default(),
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Store a finished minute and refresh the 15-min and day rows containing it.
pub fn flush_minute(conn: &mut Connection, minute: &Agg) -> Result<()> {
    let tx = conn.transaction()?;
    upsert(&tx, LEVEL_1M, minute)?;
    let q = super::bucket_start(minute.ts, LEVEL_15M);
    let quarter = merge_aggs(q, &range(&tx, LEVEL_1M, q, q + LEVEL_15M * 60)?);
    upsert(&tx, LEVEL_15M, &quarter)?;
    let day = local_day_start(minute.ts);
    // A local day is 23–25 h long around DST; the next midnight bounds it.
    let next_day = local_day_start(day + 26 * 3600);
    let day_agg = merge_aggs(day, &range(&tx, LEVEL_15M, day, next_day)?);
    upsert(&tx, LEVEL_DAY, &day_agg)?;
    tx.commit()?;
    Ok(())
}

/// Drop rows past their retention. Day rows are kept forever.
pub fn prune(conn: &Connection, now: i64) -> Result<usize> {
    let a = conn.execute("DELETE FROM pulse_agg WHERE level = ?1 AND ts < ?2", params![LEVEL_1M, now - KEEP_1M_SECS])?;
    let b = conn.execute("DELETE FROM pulse_agg WHERE level = ?1 AND ts < ?2", params![LEVEL_15M, now - KEEP_15M_SECS])?;
    Ok(a + b)
}

pub fn save_smart(conn: &Connection, s: &SmartInfo) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO pulse_smart (id, json) VALUES (1, ?1)",
        params![serde_json::to_string(s)?],
    )?;
    Ok(())
}

pub fn load_smart(conn: &Connection) -> Result<Option<SmartInfo>> {
    let j: Option<String> =
        conn.query_row("SELECT json FROM pulse_smart WHERE id = 1", [], |r| r.get(0)).optional()?;
    Ok(j.and_then(|j| serde_json::from_str(&j).ok()))
}

pub fn file_bytes(path: &std::path::Path) -> u64 {
    let mut total = 0;
    for suffix in ["", "-wal", "-shm"] {
        let mut p = path.as_os_str().to_owned();
        p.push(suffix);
        total += std::fs::metadata(PathBuf::from(p)).map(|m| m.len()).unwrap_or(0);
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        c
    }

    fn minute(ts: i64, written: f64, crit: f64) -> Agg {
        Agg {
            ts,
            secs: 60.0,
            p_crit: crit,
            p_normal: 60.0 - crit,
            swap_min: 1,
            swap_avg: 2.0,
            swap_max: 3,
            written,
            causers: vec![Causer { name: "X".into(), written: 1, peak_footprint: ts as u64 }],
            ..Default::default()
        }
    }

    #[test]
    fn a_minute_round_trips_and_feeds_the_quarter_and_the_day() {
        let mut c = mem();
        let base = 1_790_000_100; // a 15-min boundary (divisible by 900)
        for i in 0..3 {
            flush_minute(&mut c, &minute(base + i * 60, 10.0, 6.0)).unwrap();
        }
        let m = range(&c, LEVEL_1M, base, base + 900).unwrap();
        assert_eq!(m.len(), 3);
        assert_eq!(m[0], minute(base, 10.0, 6.0));
        let q = range(&c, LEVEL_15M, base, base + 1).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!((q[0].written, q[0].p_crit, q[0].secs), (30.0, 18.0, 180.0));
        let day = local_day_start(base);
        let d = range(&c, LEVEL_DAY, day, day + 1).unwrap();
        assert_eq!(d[0].written, 30.0);
        // Re-flushing a minute replaces it, never double-counts.
        flush_minute(&mut c, &minute(base, 10.0, 6.0)).unwrap();
        assert_eq!(range(&c, LEVEL_15M, base, base + 1).unwrap()[0].written, 30.0);
    }

    #[test]
    fn retention_keeps_days_forever() {
        let mut c = mem();
        let old = 1_700_000_000 - 1_700_000_000 % 900;
        flush_minute(&mut c, &minute(old, 5.0, 0.0)).unwrap();
        let now = old + 100 * 86_400;
        prune(&c, now).unwrap();
        assert!(range(&c, LEVEL_1M, 0, i64::MAX).unwrap().is_empty());
        assert!(range(&c, LEVEL_15M, 0, i64::MAX).unwrap().is_empty());
        assert_eq!(range(&c, LEVEL_DAY, 0, i64::MAX).unwrap().len(), 1);
        // Within the window nothing goes.
        flush_minute(&mut c, &minute(now - 60, 5.0, 0.0)).unwrap();
        prune(&c, now).unwrap();
        assert_eq!(range(&c, LEVEL_1M, 0, i64::MAX).unwrap().len(), 1);
    }

    #[test]
    fn smart_info_round_trips() {
        let c = mem();
        assert_eq!(load_smart(&c).unwrap(), None);
        let s = SmartInfo { percentage_used: Some(4), data_written_bytes: Some(1e12), model: None, at: 5 };
        save_smart(&c, &s).unwrap();
        assert_eq!(load_smart(&c).unwrap(), Some(s));
    }
}
