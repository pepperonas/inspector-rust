//! Internet speed test for the `speedtest` command (v0.199.0).
//!
//! Measures against Cloudflare's public speed-test endpoints
//! (`speed.cloudflare.com`) — no account, no extra program, the same method on
//! every OS. Cloudflare sees the caller's IP, nothing else leaves the machine.
//!
//! Method (deliberately close to Cloudflare's own speed test, so the numbers
//! are comparable):
//! * **Latency** — `LATENCY_SAMPLES` empty requests on one warm connection;
//!   median = latency, mean absolute difference of consecutive samples =
//!   jitter.
//! * **Download / upload** — transfers of growing size; each yields a
//!   bandwidth sample, the result is the 90th percentile of the samples. A
//!   stage whose transfers already take long ends the escalation, so a slow
//!   line doesn't sit through the 25 MB stage.
//!
//! The decisions (percentile, jitter, when to stop escalating, the bandwidth of
//! one transfer) are pure functions with tests; only `run` touches the network.
//! Results are kept in `speedtest_history` for comparison.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::db::DbHandle;

const BASE: &str = "https://speed.cloudflare.com";
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
pub const LATENCY_SAMPLES: usize = 20;
/// (bytes, count) per download stage, smallest first.
pub const DOWNLOAD_PLAN: &[(u64, usize)] = &[
    (100_000, 4),
    (1_000_000, 6),
    (10_000_000, 4),
    (25_000_000, 3),
];
/// (bytes, count) per upload stage.
pub const UPLOAD_PLAN: &[(u64, usize)] = &[(100_000, 4), (1_000_000, 6), (10_000_000, 3)];
/// The next (bigger) stage only runs if its transfers are predicted to stay
/// under this — on a slow line it would only add waiting, not precision.
pub const NEXT_STAGE_MAX_MS: f64 = 2_500.0;
/// Transfers shorter than this say more about request overhead than about the
/// line; they don't count as bandwidth samples (unless nothing else exists).
pub const MIN_SAMPLE_MS: f64 = 10.0;
/// Rows kept in the history table.
pub const HISTORY_KEEP: i64 = 200;

pub const ERR_BUSY: &str = "speedtest.busy";

static RUNNING: AtomicBool = AtomicBool::new(false);

pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

// ── Pure maths ──────────────────────────────────────────────────────────────

/// Linear-interpolated percentile (`p` in 0..=100). `None` for no data.
pub fn percentile(values: &[f64], p: f64) -> Option<f64> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let rank = (p.clamp(0.0, 100.0) / 100.0) * (v.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    Some(v[lo] + (v[hi] - v[lo]) * (rank - lo as f64))
}

pub fn median(values: &[f64]) -> Option<f64> {
    percentile(values, 50.0)
}

/// Mean absolute difference between consecutive samples (RFC 3550 style,
/// without the smoothing) — how much the latency wobbles.
pub fn jitter(samples: &[f64]) -> Option<f64> {
    if samples.len() < 2 {
        return None;
    }
    let sum: f64 = samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
    Some(sum / (samples.len() - 1) as f64)
}

/// Bits per second of one transfer. `None` for a zero/negative duration.
pub fn bandwidth_bps(bytes: u64, millis: f64) -> Option<f64> {
    (millis > 0.0 && millis.is_finite()).then(|| bytes as f64 * 8.0 / (millis / 1000.0))
}

/// The bandwidth result from a list of `(bytes, millis)` transfers: 90th
/// percentile of the samples long enough to mean something; if none is,
/// the 90th percentile of all of them.
pub fn bandwidth_result(transfers: &[(u64, f64)]) -> Option<f64> {
    let long: Vec<f64> = transfers
        .iter()
        .filter(|(_, ms)| *ms >= MIN_SAMPLE_MS)
        .filter_map(|&(b, ms)| bandwidth_bps(b, ms))
        .collect();
    if !long.is_empty() {
        return percentile(&long, 90.0);
    }
    let all: Vec<f64> = transfers.iter().filter_map(|&(b, ms)| bandwidth_bps(b, ms)).collect();
    percentile(&all, 90.0)
}

/// Should the plan stop after a stage of `bytes`-sized transfers that took
/// `stage_ms`, given that the next stage would move `next_bytes` each? The
/// next stage's duration is predicted linearly from this one's median.
pub fn stop_after_stage(stage_ms: &[f64], bytes: u64, next_bytes: u64) -> bool {
    let Some(m) = median(stage_ms) else { return true };
    if bytes == 0 {
        return true;
    }
    m * (next_bytes as f64 / bytes as f64) > NEXT_STAGE_MAX_MS
}

/// One field of Cloudflare's `/cdn-cgi/trace` (`key=value` per line).
pub fn trace_field(body: &str, key: &str) -> Option<String> {
    body.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key && !v.trim().is_empty()).then(|| v.trim().to_owned())
    })
}

// ── Types ───────────────────────────────────────────────────────────────────

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct SpeedtestResult {
    /// Unix ms when the test finished.
    pub at: i64,
    pub download_bps: Option<f64>,
    pub upload_bps: Option<f64>,
    pub latency_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    /// Cloudflare data centre (IATA code, e.g. "TXL").
    pub colo: Option<String>,
    /// Country the caller was located in (ISO code).
    pub country: Option<String>,
    pub ip: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct SpeedtestHistoryEntry {
    pub id: i64,
    pub at: i64,
    pub download_bps: Option<f64>,
    pub upload_bps: Option<f64>,
    pub latency_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub colo: Option<String>,
}

/// Live progress, emitted as `speedtest-progress`.
#[derive(Serialize, Clone, Debug)]
pub struct Progress {
    /// "meta" | "latency" | "download" | "upload"
    pub phase: &'static str,
    /// 0..=1 within the phase.
    pub fraction: f64,
    /// Current estimate for the phase (ms for latency, bps otherwise).
    pub value: Option<f64>,
}

// ── Network ─────────────────────────────────────────────────────────────────

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(HTTP_TIMEOUT).build()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Download `bytes`; returns the body-transfer time in ms (headers excluded —
/// they are latency, not bandwidth).
fn download_once(agent: &ureq::Agent, bytes: u64) -> Result<f64, String> {
    let resp = agent
        .get(&format!("{BASE}/__down?bytes={bytes}"))
        .call()
        .map_err(|e| format!("download: {e}"))?;
    let started = Instant::now();
    let mut reader = resp.into_reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("download: {e}"))?;
        if n == 0 {
            break;
        }
        total += n as u64;
    }
    if total < bytes {
        return Err(format!("download: got {total} of {bytes} bytes"));
    }
    Ok(started.elapsed().as_secs_f64() * 1000.0)
}

/// Upload `bytes`; returns the round-trip in ms minus one latency (the
/// response's own round trip), floored at 1 ms.
fn upload_once(agent: &ureq::Agent, payload: &[u8], latency_ms: f64) -> Result<f64, String> {
    let started = Instant::now();
    agent
        .post(&format!("{BASE}/__up"))
        .set("Content-Type", "application/octet-stream")
        .send_bytes(payload)
        .map_err(|e| format!("upload: {e}"))?;
    let ms = started.elapsed().as_secs_f64() * 1000.0 - latency_ms;
    Ok(ms.max(1.0))
}

fn latency_once(agent: &ureq::Agent) -> Result<f64, String> {
    let started = Instant::now();
    let resp = agent
        .get(&format!("{BASE}/__down?bytes=0"))
        .call()
        .map_err(|e| format!("latency: {e}"))?;
    let _ = resp.into_reader().read_to_end(&mut Vec::new());
    Ok(started.elapsed().as_secs_f64() * 1000.0)
}

/// Run the whole test. `progress` is called after every request. Fails only
/// if NO phase produced a value — a broken upload still returns the download.
pub fn run(mut progress: impl FnMut(Progress)) -> Result<SpeedtestResult, String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err(ERR_BUSY.into());
    }
    struct Release;
    impl Drop for Release {
        fn drop(&mut self) {
            RUNNING.store(false, Ordering::SeqCst);
        }
    }
    let _release = Release;

    let agent = agent();
    progress(Progress { phase: "meta", fraction: 0.0, value: None });
    let trace = agent
        .get(&format!("{BASE}/cdn-cgi/trace"))
        .call()
        .ok()
        .and_then(|r| r.into_string().ok())
        .unwrap_or_default();

    // Latency — the first request also opens the connection; drop it.
    let _ = latency_once(&agent);
    let mut pings = Vec::with_capacity(LATENCY_SAMPLES);
    let mut last_err = None;
    for i in 0..LATENCY_SAMPLES {
        match latency_once(&agent) {
            Ok(ms) => pings.push(ms),
            Err(e) => last_err = Some(e),
        }
        progress(Progress {
            phase: "latency",
            fraction: (i + 1) as f64 / LATENCY_SAMPLES as f64,
            value: median(&pings),
        });
    }
    let latency = median(&pings);

    let down = run_plan(DOWNLOAD_PLAN, "download", &mut progress, &mut last_err, |b| {
        download_once(&agent, b)
    });

    let up_latency = latency.unwrap_or(0.0);
    let biggest = UPLOAD_PLAN.iter().map(|&(b, _)| b).max().unwrap_or(0) as usize;
    let payload = vec![0u8; biggest];
    let up = run_plan(UPLOAD_PLAN, "upload", &mut progress, &mut last_err, |b| {
        upload_once(&agent, &payload[..b as usize], up_latency)
    });

    let result = SpeedtestResult {
        at: now_ms(),
        download_bps: bandwidth_result(&down),
        upload_bps: bandwidth_result(&up),
        latency_ms: latency,
        jitter_ms: jitter(&pings),
        colo: trace_field(&trace, "colo"),
        country: trace_field(&trace, "loc"),
        ip: trace_field(&trace, "ip"),
    };
    if result.download_bps.is_none() && result.upload_bps.is_none() && result.latency_ms.is_none() {
        return Err(last_err.unwrap_or_else(|| "speedtest: no measurement succeeded".into()));
    }
    Ok(result)
}

/// Walk a size plan, stopping early once a stage is slow. Returns the
/// `(bytes, millis)` transfers that succeeded.
fn run_plan(
    plan: &[(u64, usize)],
    phase: &'static str,
    progress: &mut impl FnMut(Progress),
    last_err: &mut Option<String>,
    mut transfer: impl FnMut(u64) -> Result<f64, String>,
) -> Vec<(u64, f64)> {
    let total: usize = plan.iter().map(|&(_, n)| n).sum();
    let mut done = 0usize;
    let mut out = Vec::new();
    for (i, &(bytes, count)) in plan.iter().enumerate() {
        let mut stage_ms = Vec::with_capacity(count);
        for _ in 0..count {
            match transfer(bytes) {
                Ok(ms) => {
                    out.push((bytes, ms));
                    stage_ms.push(ms);
                }
                Err(e) => *last_err = Some(e),
            }
            done += 1;
            progress(Progress {
                phase,
                fraction: done as f64 / total as f64,
                value: bandwidth_result(&out),
            });
        }
        let Some(&(next_bytes, _)) = plan.get(i + 1) else { break };
        if stop_after_stage(&stage_ms, bytes, next_bytes) {
            break;
        }
    }
    progress(Progress { phase, fraction: 1.0, value: bandwidth_result(&out) });
    out
}

// ── History ─────────────────────────────────────────────────────────────────

pub fn init_schema(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS speedtest_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            at INTEGER NOT NULL,
            download_bps REAL,
            upload_bps REAL,
            latency_ms REAL,
            jitter_ms REAL,
            colo TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_speedtest_at ON speedtest_history(at DESC);",
    )
}

pub fn history_insert(db: &DbHandle, r: &SpeedtestResult) -> rusqlite::Result<i64> {
    let conn = db.lock();
    conn.execute(
        "INSERT INTO speedtest_history (at, download_bps, upload_bps, latency_ms, jitter_ms, colo)
         VALUES (?1,?2,?3,?4,?5,?6)",
        rusqlite::params![r.at, r.download_bps, r.upload_bps, r.latency_ms, r.jitter_ms, r.colo],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "DELETE FROM speedtest_history WHERE id NOT IN
           (SELECT id FROM speedtest_history ORDER BY at DESC, id DESC LIMIT ?1)",
        [HISTORY_KEEP],
    )?;
    Ok(id)
}

pub fn history_list(db: &DbHandle, limit: u32) -> rusqlite::Result<Vec<SpeedtestHistoryEntry>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, at, download_bps, upload_bps, latency_ms, jitter_ms, colo
         FROM speedtest_history ORDER BY at DESC, id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit], |r| {
        Ok(SpeedtestHistoryEntry {
            id: r.get(0)?,
            at: r.get(1)?,
            download_bps: r.get(2)?,
            upload_bps: r.get(3)?,
            latency_ms: r.get(4)?,
            jitter_ms: r.get(5)?,
            colo: r.get(6)?,
        })
    })?;
    rows.collect()
}

pub fn history_clear(db: &DbHandle) -> rusqlite::Result<()> {
    db.lock().execute("DELETE FROM speedtest_history", []).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_db() -> DbHandle {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        std::sync::Arc::new(parking_lot::Mutex::new(conn))
    }

    #[test]
    fn percentile_interpolates_and_ignores_garbage() {
        assert_eq!(percentile(&[], 90.0), None);
        assert_eq!(percentile(&[5.0], 90.0), Some(5.0));
        assert_eq!(percentile(&[1.0, 2.0, 3.0, 4.0, 5.0], 50.0), Some(3.0));
        // rank 0.9*4 = 3.6 → 4 + 0.6*(5-4)
        let p = percentile(&[5.0, 1.0, 4.0, 2.0, 3.0], 90.0).unwrap();
        assert!((p - 4.6).abs() < 1e-9, "{p}");
        assert_eq!(percentile(&[f64::NAN, 2.0], 50.0), Some(2.0));
    }

    #[test]
    fn jitter_is_the_mean_step_between_samples() {
        assert_eq!(jitter(&[10.0]), None);
        assert_eq!(jitter(&[10.0, 12.0, 11.0, 15.0]), Some((2.0 + 1.0 + 4.0) / 3.0));
    }

    #[test]
    fn bandwidth_of_one_transfer() {
        // 1 MB in 100 ms = 80 Mbit/s.
        assert_eq!(bandwidth_bps(1_000_000, 100.0), Some(80_000_000.0));
        assert_eq!(bandwidth_bps(1_000_000, 0.0), None);
    }

    #[test]
    fn the_result_is_the_90th_percentile_not_the_median() {
        // 1 MB transfers at 10, 20, … 100 Mbit/s (800 ms down to 80 ms).
        let t: Vec<(u64, f64)> = (1..=10).map(|k| (1_000_000, 800.0 / k as f64)).collect();
        let r = bandwidth_result(&t).unwrap() / 1e6;
        // p90 of 10..100 = 91; the median would be 55.
        assert!((r - 91.0).abs() < 0.01, "{r}");
    }

    #[test]
    fn short_transfers_dont_count_when_long_ones_exist() {
        // A 2-ms transfer would claim 400 Mbit/s; the long ones say ~80.
        let t = [(100_000, 2.0), (1_000_000, 100.0), (1_000_000, 100.0)];
        let r = bandwidth_result(&t).unwrap();
        assert!((r - 80_000_000.0).abs() < 1.0, "{r}");
        // Only short ones → they are all there is.
        assert!(bandwidth_result(&[(100_000, 2.0)]).is_some());
        assert_eq!(bandwidth_result(&[]), None);
    }

    #[test]
    fn the_next_stage_runs_only_if_predicted_short() {
        // 1 MB in 80 ms → 10 MB predicted 800 ms → go on.
        assert!(!stop_after_stage(&[80.0, 90.0, 70.0], 1_000_000, 10_000_000));
        // 1 MB in 1000 ms → 10 MB predicted 10 s → stop.
        assert!(stop_after_stage(&[1_000.0, 1_000.0], 1_000_000, 10_000_000));
        // No successful transfer → nothing to predict from → stop.
        assert!(stop_after_stage(&[], 1_000_000, 10_000_000));
    }

    #[test]
    fn run_plan_escalates_until_a_stage_is_slow() {
        // Simulated 8 Mbit/s line: 1 MB takes 1000 ms, 10 MB 10 s.
        let mut seen = Vec::new();
        let mut err = None;
        let out = run_plan(DOWNLOAD_PLAN, "download", &mut |_p: Progress| {}, &mut err, |b| {
            seen.push(b);
            Ok(b as f64 * 8.0 / 8_000.0) // ms at 8 Mbit/s
        });
        // 100 kB (100 ms) and 1 MB (1000 ms) run; 10 MB is predicted at 10 s
        // and never starts.
        assert!(seen.contains(&1_000_000), "{seen:?}");
        assert!(!seen.contains(&10_000_000), "{seen:?}");
        let r = bandwidth_result(&out).unwrap();
        assert!((r - 8_000_000.0).abs() < 1.0, "{r}");
    }

    #[test]
    fn a_fast_line_runs_the_whole_plan() {
        let mut seen = Vec::new();
        let mut err = None;
        run_plan(DOWNLOAD_PLAN, "download", &mut |_p: Progress| {}, &mut err, |b| {
            seen.push(b);
            Ok(b as f64 * 8.0 / 500_000.0) // 500 Mbit/s
        });
        assert!(seen.contains(&25_000_000), "{seen:?}");
        let expected: usize = DOWNLOAD_PLAN.iter().map(|&(_, n)| n).sum();
        assert_eq!(seen.len(), expected);
    }

    #[test]
    fn run_plan_survives_failed_transfers() {
        let mut err = None;
        let mut n = 0;
        let out = run_plan(&[(1_000, 3)], "upload", &mut |_p: Progress| {}, &mut err, |_| {
            n += 1;
            if n == 2 { Err("boom".into()) } else { Ok(50.0) }
        });
        assert_eq!(out.len(), 2);
        assert_eq!(err.as_deref(), Some("boom"));
    }

    #[test]
    fn trace_fields_are_read_by_key() {
        let body = "fl=67f194\nip=203.0.113.9\ncolo=TXL\nloc=DE\nempty=\n";
        assert_eq!(trace_field(body, "colo").as_deref(), Some("TXL"));
        assert_eq!(trace_field(body, "loc").as_deref(), Some("DE"));
        assert_eq!(trace_field(body, "empty"), None);
        assert_eq!(trace_field(body, "missing"), None);
    }

    #[test]
    fn history_roundtrips_newest_first_and_is_capped() {
        let db = mem_db();
        for i in 0..(HISTORY_KEEP + 5) {
            history_insert(
                &db,
                &SpeedtestResult {
                    at: i,
                    download_bps: Some(i as f64),
                    upload_bps: None,
                    latency_ms: Some(10.0),
                    jitter_ms: None,
                    colo: Some("TXL".into()),
                    country: None,
                    ip: None,
                },
            )
            .unwrap();
        }
        let all = history_list(&db, 1_000).unwrap();
        assert_eq!(all.len() as i64, HISTORY_KEEP);
        assert_eq!(all[0].at, HISTORY_KEEP + 4, "newest first");
        assert_eq!(all[0].upload_bps, None, "a missing phase stays missing, not 0");
        history_clear(&db).unwrap();
        assert!(history_list(&db, 10).unwrap().is_empty());
    }

    /// Real network run: `cargo test -p inspector-rust-core --lib speedtest_live -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn speedtest_live() {
        let r = run(|p| println!("{} {:.0}% {:?}", p.phase, p.fraction * 100.0, p.value)).unwrap();
        println!("{r:?}");
        assert!(r.download_bps.is_some());
    }
}
