//! System-stats **history**: an always-on, lightweight background sampler that
//! persists a handful of time-series metrics so the Stats panel can show a
//! "last hours / days" view, not just the live readout.
//!
//! - **Collection** (`start_collector`): a background thread samples every
//!   [`SAMPLE_INTERVAL_SECS`] (reusing `system_stats::gather`, reduced to the
//!   core series by [`sample_from`]) and inserts a row, pruning rows older than
//!   [`RETENTION_SECS`] roughly hourly. Runs for the process lifetime (the app
//!   is tray-resident), so the history is gap-free across popup open/close.
//! - **Query** (`history`): reads the rows in a time range and **downsamples**
//!   them to ≤ [`MAX_POINTS`] bucket-averaged points (the pure, unit-tested
//!   [`downsample`]) so the chart payload stays small + the SVG stays smooth.
//!
//! Stored metrics (the "core set"): CPU %, memory %, network rx/tx (bytes/s),
//! power draw (W), CPU temperature (°C), battery %. The last three are nullable
//! (no battery on a desktop, no temp sensor on some hardware). Numeric telemetry
//! only — not encrypted (consistent with timestamps/sizes being plaintext).

use crate::db::DbHandle;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How often the background collector samples + stores.
pub const SAMPLE_INTERVAL_SECS: u64 = 60;
/// How long history is kept before pruning (7 days).
pub const RETENTION_SECS: i64 = 7 * 24 * 60 * 60;
/// Max points returned to the frontend per query (downsample target).
pub const MAX_POINTS: usize = 240;

/// One stored sample (core time-series metrics). f64 throughout to match
/// SQLite `REAL` cleanly; nullable where the sensor may be absent.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct StatsSample {
    pub cpu: f64,
    pub mem: f64,
    pub net_rx: f64,
    pub net_tx: f64,
    pub power: Option<f64>,
    pub cpu_temp: Option<f64>,
    pub battery: Option<f64>,
}

/// A row as read back from the DB (sample + its timestamp).
#[derive(Debug, Clone, Copy)]
pub struct Row {
    pub ts: i64,
    pub cpu: f64,
    pub mem: f64,
    pub net_rx: f64,
    pub net_tx: f64,
    pub power: Option<f64>,
    pub cpu_temp: Option<f64>,
    pub battery: Option<f64>,
}

/// One downsampled (bucket-averaged) point sent to the frontend.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct HistoryPoint {
    pub ts: i64,
    pub cpu: f64,
    pub mem: f64,
    pub net_rx: f64,
    pub net_tx: f64,
    pub power: Option<f64>,
    pub cpu_temp: Option<f64>,
    pub battery: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct StatsHistory {
    pub points: Vec<HistoryPoint>,
    /// The requested range (seconds), echoed back.
    pub range_secs: i64,
    /// Raw rows that were in range before downsampling (data density).
    pub sample_count: usize,
    /// The collector's sampling cadence (so the UI can hint at resolution).
    pub interval_secs: i64,
}

/// Reduce a full `SystemStats` snapshot to the stored core series. Pure +
/// unit-tested. CPU temperature is the summarised `"CPU"` bucket from
/// `system_stats::summarize_temps`; power/battery come from the battery stat.
pub fn sample_from(s: &crate::system_stats::SystemStats) -> StatsSample {
    let mem = if s.mem_total > 0 {
        s.mem_used as f64 / s.mem_total as f64 * 100.0
    } else {
        0.0
    };
    StatsSample {
        cpu: s.cpu_usage as f64,
        mem,
        net_rx: s.net_rx_per_sec as f64,
        net_tx: s.net_tx_per_sec as f64,
        power: s.battery.as_ref().and_then(|b| b.power_watts).map(|w| w as f64),
        cpu_temp: s
            .temps
            .iter()
            .find(|t| t.label == "CPU")
            .map(|t| t.celsius as f64),
        battery: s.battery.as_ref().map(|b| b.percent as f64),
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Create the history table (idempotent). Called from `db::open`.
pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS stats_history (
            ts        INTEGER NOT NULL,
            cpu       REAL    NOT NULL,
            mem       REAL    NOT NULL,
            net_rx    REAL    NOT NULL,
            net_tx    REAL    NOT NULL,
            power     REAL,
            cpu_temp  REAL,
            battery   REAL
        );
        CREATE INDEX IF NOT EXISTS idx_stats_history_ts ON stats_history(ts);
        "#,
    )
}

fn insert(db: &DbHandle, ts: i64, s: &StatsSample) {
    let conn = db.lock();
    let _ = conn.execute(
        "INSERT INTO stats_history (ts, cpu, mem, net_rx, net_tx, power, cpu_temp, battery)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![ts, s.cpu, s.mem, s.net_rx, s.net_tx, s.power, s.cpu_temp, s.battery],
    );
}

fn prune(db: &DbHandle, before_ts: i64) {
    let conn = db.lock();
    let _ = conn.execute("DELETE FROM stats_history WHERE ts < ?1", params![before_ts]);
}

fn query(db: &DbHandle, since_ts: i64) -> Vec<Row> {
    let conn = db.lock();
    let mut stmt = match conn.prepare(
        "SELECT ts, cpu, mem, net_rx, net_tx, power, cpu_temp, battery
         FROM stats_history WHERE ts >= ?1 ORDER BY ts ASC",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mapped = stmt.query_map(params![since_ts], |r| {
        Ok(Row {
            ts: r.get(0)?,
            cpu: r.get(1)?,
            mem: r.get(2)?,
            net_rx: r.get(3)?,
            net_tx: r.get(4)?,
            power: r.get(5)?,
            cpu_temp: r.get(6)?,
            battery: r.get(7)?,
        })
    });
    match mapped {
        Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Downsample sorted rows (ascending `ts`) into ≤ `max_points` bucket-averaged
/// points. Buckets are fixed-width over `[since, until]`; each metric is the
/// mean of the rows in the bucket, with the nullable metrics averaging only
/// their present values (None if the whole bucket lacked the sensor). Pure +
/// unit-tested. Assumes `rows` are sorted ascending by `ts`.
pub fn downsample(rows: &[Row], since: i64, until: i64, max_points: usize) -> Vec<HistoryPoint> {
    if rows.is_empty() || max_points == 0 {
        return Vec::new();
    }
    let span = (until - since).max(1);
    let width = (span / max_points as i64).max(1);
    let bucket_of = |ts: i64| (ts - since) / width;

    let mut out: Vec<HistoryPoint> = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let b = bucket_of(rows[i].ts);
        let mut ts_sum: i128 = 0;
        let (mut cpu, mut mem, mut rx, mut tx) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        let (mut pw, mut pw_n) = (0.0f64, 0usize);
        let (mut tp, mut tp_n) = (0.0f64, 0usize);
        let (mut bt, mut bt_n) = (0.0f64, 0usize);
        let mut n = 0usize;
        let mut j = i;
        while j < rows.len() && bucket_of(rows[j].ts) == b {
            let r = &rows[j];
            ts_sum += r.ts as i128;
            cpu += r.cpu;
            mem += r.mem;
            rx += r.net_rx;
            tx += r.net_tx;
            if let Some(v) = r.power {
                pw += v;
                pw_n += 1;
            }
            if let Some(v) = r.cpu_temp {
                tp += v;
                tp_n += 1;
            }
            if let Some(v) = r.battery {
                bt += v;
                bt_n += 1;
            }
            n += 1;
            j += 1;
        }
        let nf = n as f64;
        out.push(HistoryPoint {
            ts: (ts_sum / n as i128) as i64,
            cpu: cpu / nf,
            mem: mem / nf,
            net_rx: rx / nf,
            net_tx: tx / nf,
            power: (pw_n > 0).then(|| pw / pw_n as f64),
            cpu_temp: (tp_n > 0).then(|| tp / tp_n as f64),
            battery: (bt_n > 0).then(|| bt / bt_n as f64),
        });
        i = j;
    }
    out
}

/// Query + downsample the last `range_secs` of history for the frontend.
pub fn history(db: &DbHandle, range_secs: i64) -> StatsHistory {
    let now = now_secs();
    let range = range_secs.max(1);
    let since = now - range;
    let rows = query(db, since);
    let sample_count = rows.len();
    let points = downsample(&rows, since, now, MAX_POINTS);
    StatsHistory {
        points,
        range_secs: range,
        sample_count,
        interval_secs: SAMPLE_INTERVAL_SECS as i64,
    }
}

/// Start the always-on background sampler (one per process). Cheap: one
/// `gather()` per minute on a dedicated thread, plus an hourly prune.
pub fn start_collector(db: DbHandle) {
    std::thread::Builder::new()
        .name("ir-stats-history".into())
        .spawn(move || {
            let mut iter: u64 = 0;
            loop {
                let stats = crate::system_stats::gather();
                let sample = sample_from(&stats);
                let ts = now_secs();
                insert(&db, ts, &sample);
                // Prune roughly hourly (every 60 samples) so the table stays
                // bounded without a delete on every insert.
                if iter.is_multiple_of(60) {
                    prune(&db, ts - RETENTION_SECS);
                }
                iter = iter.wrapping_add(1);
                std::thread::sleep(Duration::from_secs(SAMPLE_INTERVAL_SECS));
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_stats::{BatteryStat, SystemStats, TempStat};

    fn row(ts: i64, cpu: f64, power: Option<f64>) -> Row {
        Row {
            ts,
            cpu,
            mem: 50.0,
            net_rx: 1000.0,
            net_tx: 500.0,
            power,
            cpu_temp: Some(45.0),
            battery: Some(80.0),
        }
    }

    #[test]
    fn sample_from_extracts_the_core_series() {
        let mut s = SystemStats {
            cpu_usage: 33.0,
            mem_total: 16_000,
            mem_used: 4_000,
            net_rx_per_sec: 2048,
            net_tx_per_sec: 1024,
            ..Default::default()
        };
        s.temps = vec![
            TempStat { label: "GPU".into(), celsius: 60.0 },
            TempStat { label: "CPU".into(), celsius: 48.5 },
        ];
        s.battery = Some(BatteryStat {
            percent: 77.0,
            state: "Discharging".into(),
            power_watts: Some(12.5),
            time_to_empty_secs: None,
            time_to_full_secs: None,
            health_percent: None,
            cycle_count: None,
            temperature_c: None,
            vendor: None,
            model: None,
        });
        let sample = sample_from(&s);
        assert!((sample.cpu - 33.0).abs() < 1e-9);
        assert!((sample.mem - 25.0).abs() < 1e-9); // 4000/16000
        assert_eq!(sample.net_rx, 2048.0);
        assert_eq!(sample.net_tx, 1024.0);
        assert_eq!(sample.power, Some(12.5));
        assert_eq!(sample.cpu_temp, Some(48.5)); // the "CPU" bucket, not GPU
        assert_eq!(sample.battery, Some(77.0));
    }

    #[test]
    fn sample_from_handles_missing_battery_and_temp() {
        let s = SystemStats {
            cpu_usage: 10.0,
            mem_total: 0, // guard against /0
            ..Default::default()
        };
        let sample = sample_from(&s);
        assert_eq!(sample.mem, 0.0);
        assert_eq!(sample.power, None);
        assert_eq!(sample.cpu_temp, None);
        assert_eq!(sample.battery, None);
    }

    #[test]
    fn downsample_empty_is_empty() {
        assert!(downsample(&[], 0, 100, 10).is_empty());
        assert!(downsample(&[row(5, 1.0, None)], 0, 100, 0).is_empty());
    }

    #[test]
    fn downsample_passes_through_when_fewer_rows_than_buckets() {
        let rows = [row(10, 10.0, Some(5.0)), row(20, 20.0, Some(7.0))];
        let pts = downsample(&rows, 0, 100, 240);
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0].cpu, 10.0);
        assert_eq!(pts[1].cpu, 20.0);
    }

    #[test]
    fn downsample_averages_within_a_bucket() {
        // Range 0..100, max_points 1 → width 100 → one bucket holding all rows.
        let rows = [
            row(10, 10.0, Some(2.0)),
            row(20, 20.0, Some(4.0)),
            row(30, 30.0, None), // power missing here
        ];
        let pts = downsample(&rows, 0, 100, 1);
        assert_eq!(pts.len(), 1);
        assert!((pts[0].cpu - 20.0).abs() < 1e-9); // (10+20+30)/3
        // Nullable averages only the present values: (2+4)/2 = 3.
        assert_eq!(pts[0].power, Some(3.0));
        // ts is the mean of the bucket's timestamps.
        assert_eq!(pts[0].ts, 20);
    }

    #[test]
    fn downsample_all_null_bucket_yields_none() {
        let rows = [
            Row { power: None, ..row(10, 10.0, None) },
            Row { power: None, ..row(20, 20.0, None) },
        ];
        let pts = downsample(&rows, 0, 100, 1);
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].power, None);
    }

    #[test]
    fn downsample_splits_into_distinct_buckets() {
        // Range 0..100, max_points 10 → width 10. Rows at 5, 15, 25 → 3 buckets.
        let rows = [row(5, 1.0, None), row(15, 2.0, None), row(25, 3.0, None)];
        let pts = downsample(&rows, 0, 100, 10);
        assert_eq!(pts.len(), 3);
        assert_eq!(pts.iter().map(|p| p.cpu).collect::<Vec<_>>(), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn downsample_single_point_echoes_the_row() {
        let rows = [row(42, 33.0, Some(9.0))];
        let pts = downsample(&rows, 0, 3600, 240);
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].ts, 42);
        assert!((pts[0].cpu - 33.0).abs() < 1e-9);
        assert_eq!(pts[0].power, Some(9.0));
        assert_eq!(pts[0].cpu_temp, Some(45.0));
        assert_eq!(pts[0].battery, Some(80.0));
    }

    #[test]
    fn downsample_mixed_nullable_metrics_average_independently() {
        // Within one bucket: power present in row 1 only, temp in row 2 only,
        // battery in neither — each nullable metric averages its own present set.
        let rows = [
            Row { cpu_temp: None, battery: None, ..row(10, 10.0, Some(6.0)) },
            Row { cpu_temp: Some(50.0), battery: None, ..row(20, 20.0, None) },
        ];
        let pts = downsample(&rows, 0, 100, 1);
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].power, Some(6.0)); // only row 1 had power
        assert_eq!(pts[0].cpu_temp, Some(50.0)); // only row 2 had temp
        assert_eq!(pts[0].battery, None); // nobody had battery
    }

    #[test]
    fn downsample_zero_span_range_still_buckets() {
        // until == since → span clamps to 1; must not panic or divide by zero.
        let rows = [row(5, 1.0, None), row(6, 2.0, None)];
        let pts = downsample(&rows, 5, 5, 10);
        assert!(!pts.is_empty());
        let total: f64 = pts.iter().map(|p| p.cpu).sum::<f64>();
        assert!(total > 0.0);
    }

    fn mem_db() -> DbHandle {
        let conn = Connection::open_in_memory().expect("in-memory db");
        init_schema(&conn).expect("schema");
        std::sync::Arc::new(parking_lot::Mutex::new(conn))
    }

    fn sample(cpu: f64, power: Option<f64>) -> StatsSample {
        StatsSample {
            cpu,
            mem: 40.0,
            net_rx: 100.0,
            net_tx: 50.0,
            power,
            cpu_temp: None,
            battery: None,
        }
    }

    #[test]
    fn db_round_trip_preserves_values_and_nullables() {
        let db = mem_db();
        insert(&db, 100, &sample(12.5, Some(7.5)));
        insert(&db, 200, &sample(25.0, None));
        let rows = query(&db, 0);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].ts, 100);
        assert!((rows[0].cpu - 12.5).abs() < 1e-9);
        assert_eq!(rows[0].power, Some(7.5));
        assert_eq!(rows[0].cpu_temp, None);
        assert_eq!(rows[1].power, None);
    }

    #[test]
    fn query_filters_by_since_and_orders_ascending() {
        let db = mem_db();
        // Insert out of order; query must return ascending + only ts >= since.
        insert(&db, 300, &sample(3.0, None));
        insert(&db, 100, &sample(1.0, None));
        insert(&db, 200, &sample(2.0, None));
        let rows = query(&db, 150);
        assert_eq!(rows.iter().map(|r| r.ts).collect::<Vec<_>>(), vec![200, 300]);
    }

    #[test]
    fn prune_deletes_only_rows_older_than_cutoff() {
        let db = mem_db();
        insert(&db, 100, &sample(1.0, None));
        insert(&db, 200, &sample(2.0, None));
        insert(&db, 300, &sample(3.0, None));
        prune(&db, 200); // strictly-older-than: ts < 200
        let rows = query(&db, 0);
        assert_eq!(rows.iter().map(|r| r.ts).collect::<Vec<_>>(), vec![200, 300]);
    }

    #[test]
    fn history_returns_recent_rows_with_metadata() {
        let db = mem_db();
        let now = now_secs();
        insert(&db, now - 10, &sample(10.0, None));
        insert(&db, now - 5, &sample(20.0, None));
        insert(&db, now - 7200, &sample(99.0, None)); // outside a 1h range
        let h = history(&db, 3600);
        assert_eq!(h.range_secs, 3600);
        assert_eq!(h.sample_count, 2);
        assert_eq!(h.interval_secs, SAMPLE_INTERVAL_SECS as i64);
        assert!(!h.points.is_empty());
        assert!(h.points.iter().all(|p| p.cpu < 99.0), "old row must be excluded");
    }

    #[test]
    fn history_clamps_non_positive_range() {
        let db = mem_db();
        let h = history(&db, 0);
        assert_eq!(h.range_secs, 1);
        assert!(h.points.is_empty());
        assert_eq!(h.sample_count, 0);
    }

    #[test]
    fn downsample_caps_at_max_points_for_dense_data() {
        // 1000 rows over a 1000 s span, max_points 100 → ≤ ~101 buckets.
        let rows: Vec<Row> = (0..1000).map(|k| row(k, k as f64, Some(k as f64))).collect();
        let pts = downsample(&rows, 0, 1000, 100);
        assert!(pts.len() <= 101, "expected ≤101 buckets, got {}", pts.len());
        assert!(pts.len() >= 90, "expected dense coverage, got {}", pts.len());
        // Monotonic timestamps preserved.
        assert!(pts.windows(2).all(|w| w[0].ts <= w[1].ts));
    }

    #[test]
    fn init_schema_can_run_again_over_existing_history() {
        // It runs on every `db::open`; a second pass must keep the collected
        // series rather than error out (or recreate the table empty).
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        let db: DbHandle = std::sync::Arc::new(parking_lot::Mutex::new(conn));
        insert(&db, 100, &sample(1.0, None));
        {
            let conn = db.lock();
            init_schema(&conn).expect("second run must succeed");
        }
        assert_eq!(query(&db, 0).len(), 1, "existing samples survive");
    }

    #[test]
    fn a_sensor_that_disappears_mid_window_leaves_a_hole_not_a_flat_line() {
        // Unplugging the charger stops the power readings. Each bucket averages
        // only what it actually has, so the chart's power line ends where the
        // data ends instead of carrying the last value forward.
        let rows = [
            row(5, 10.0, Some(20.0)),
            row(6, 12.0, Some(22.0)),
            row(55, 30.0, None),
            row(56, 32.0, None),
        ];
        // Range 0..100 with 2 buckets → width 50: rows 5/6 in bucket 0, 55/56 in 1.
        let pts = downsample(&rows, 0, 100, 2);
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0].power, Some(21.0));
        assert_eq!(pts[1].power, None, "the later bucket must not inherit the earlier average");
        assert!((pts[1].cpu - 31.0).abs() < 1e-9, "the always-present metrics keep averaging");
    }
}
