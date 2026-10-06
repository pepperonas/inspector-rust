//! The `pulse` background sampler and the state the IPC reads.
//!
//! One thread (`ir-pulse`), started with the app, independent of any window:
//! every 5 s it reads the system counters, every 6th tick (30 s) it scans the
//! processes. Raw samples stay in a RAM ring (one hour). Once per wall-clock
//! minute it writes one aggregate row to `pulse.db` (see `store.rs`). The
//! sampler measures its own cost per tick so the overhead is reportable.

use parking_lot::Mutex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{
    aggregate_samples, bucket_start, forecast, merge_aggs, pressure_bucket, rank_causers, rebucket, store,
    tbw_bytes, Agg, Causer, Forecast, GroupStat, Sample, SmartInfo, LEVEL_15M, LEVEL_1M, LEVEL_DAY, PROC_EVERY,
    RING_LEN, SAMPLE_SECS, TOP_PROCS,
};
use crate::db::DbHandle;

// ── Config ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PulseConfig {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub notify: bool,
    #[serde(default = "default_notify_minutes")]
    pub notify_minutes: u32,
    /// Endurance rating in TB; 0 = automatic (600 TB per TB of capacity).
    #[serde(default)]
    pub tbw_tb: f64,
}

fn yes() -> bool {
    true
}
fn default_notify_minutes() -> u32 {
    10
}

impl Default for PulseConfig {
    fn default() -> Self {
        Self { enabled: true, notify: false, notify_minutes: 10, tbw_tb: 0.0 }
    }
}

impl PulseConfig {
    pub fn normalized(mut self) -> Self {
        self.notify_minutes = self.notify_minutes.clamp(1, 240);
        self.tbw_tb = if self.tbw_tb.is_finite() { self.tbw_tb.clamp(0.0, 100_000.0) } else { 0.0 };
        self
    }

    pub fn load(db: &DbHandle) -> Self {
        let g = |k: &str| crate::settings::get(db, k).ok().flatten();
        let d = Self::default();
        Self {
            enabled: g("pulse.enabled").map_or(d.enabled, |v| v != "false"),
            notify: g("pulse.notify").map_or(d.notify, |v| v == "true"),
            notify_minutes: g("pulse.notify_minutes").and_then(|v| v.parse().ok()).unwrap_or(d.notify_minutes),
            tbw_tb: g("pulse.tbw_tb").and_then(|v| v.parse().ok()).unwrap_or(d.tbw_tb),
        }
        .normalized()
    }

    pub fn save(&self, db: &DbHandle) -> anyhow::Result<()> {
        crate::settings::set(db, "pulse.enabled", if self.enabled { "true" } else { "false" })?;
        crate::settings::set(db, "pulse.notify", if self.notify { "true" } else { "false" })?;
        crate::settings::set(db, "pulse.notify_minutes", &self.notify_minutes.to_string())?;
        crate::settings::set(db, "pulse.tbw_tb", &self.tbw_tb.to_string())?;
        Ok(())
    }
}

// ── Shared state ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
pub struct Overhead {
    /// System samples taken since start.
    pub samples: u64,
    pub sample_avg_ms: f64,
    pub sample_max_ms: f64,
    pub scans: u64,
    pub scan_avg_ms: f64,
    pub scan_max_ms: f64,
    /// Rows written to pulse.db since start.
    pub rows_written: u64,
    /// CPU seconds the sampler thread has used, and that as a share of the
    /// wall time it has been running (the "< 0.5 %" budget).
    pub cpu_secs: f64,
    pub cpu_pct: f64,
    pub running_secs: f64,
}

#[derive(Default)]
struct State {
    config: PulseConfig,
    ring: VecDeque<Sample>,
    groups: Vec<GroupStat>,
    groups_at: i64,
    unreadable: usize,
    disk_found: bool,
    capacity: Option<u64>,
    overhead: Overhead,
    smart: Option<SmartInfo>,
    smartctl_present: Option<bool>,
    last_error: Option<String>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Default::default);
static ENABLED: AtomicBool = AtomicBool::new(false);
static PDB: OnceLock<(PathBuf, Mutex<Connection>)> = OnceLock::new();

fn now_ts() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

pub fn supported() -> bool {
    cfg!(target_os = "macos")
}

pub fn config() -> PulseConfig {
    STATE.lock().config.clone()
}

pub fn apply_config(cfg: PulseConfig) {
    ENABLED.store(cfg.enabled, Ordering::Relaxed);
    STATE.lock().config = cfg;
}

// ── Pure helpers (tested) ──────────────────────────────────────────────────

/// Notify when critical pressure has lasted `minutes` and the last
/// notification is at least an hour old.
pub fn should_notify(crit_since: Option<i64>, now: i64, minutes: u32, last_notified: Option<i64>) -> bool {
    let Some(since) = crit_since else { return false };
    now - since >= minutes as i64 * 60 && last_notified.is_none_or(|t| now - t >= 3600)
}

/// A tick whose monotonic gap is far beyond the cadence (thread stalled, or a
/// dark-wake jitter) is a re-baseline, not a sample: its rates would average
/// across an unknown period.
pub fn gap_is_rebaseline(elapsed_secs: f64) -> bool {
    !(0.5..=30.0).contains(&elapsed_secs)
}

/// How a history range is served: (level, look-back seconds, display bucket).
pub fn history_plan(range: &str) -> Option<(i64, i64, i64)> {
    Some(match range {
        "1h" => (LEVEL_1M, 3_600, 60),
        "24h" => (LEVEL_15M, 86_400, 900),
        "7d" => (LEVEL_15M, 7 * 86_400, 3_600),
        "30d" => (LEVEL_DAY, 30 * 86_400, 86_400),
        "90d" => (LEVEL_DAY, 90 * 86_400, 86_400),
        _ => return None,
    })
}

/// Fold a process scan into the running causers of the current minute.
pub fn accumulate_causers(acc: &mut HashMap<String, Causer>, groups: &[GroupStat], written: &HashMap<String, u64>) {
    for g in groups {
        let e = acc.entry(g.name.clone()).or_insert_with(|| Causer { name: g.name.clone(), ..Default::default() });
        e.peak_footprint = e.peak_footprint.max(g.footprint);
        e.written += written.get(&g.name).copied().unwrap_or(0);
    }
}

fn running_avg(avg: f64, n: u64, x: f64) -> f64 {
    avg + (x - avg) / n.max(1) as f64
}

// ── Start ──────────────────────────────────────────────────────────────────

pub fn start(app_db: DbHandle) {
    let cfg = PulseConfig::load(&app_db);
    apply_config(cfg);
    if !supported() {
        return;
    }
    let path = match store::default_path() {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("pulse: no data dir: {e:#}");
            return;
        }
    };
    let conn = match store::open(&path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("pulse: cannot open {}: {e:#}", path.display());
            STATE.lock().last_error = Some(format!("{e:#}"));
            return;
        }
    };
    if let Ok(Some(s)) = store::load_smart(&conn) {
        STATE.lock().smart = Some(s);
    }
    let _ = PDB.set((path, Mutex::new(conn)));
    #[cfg(target_os = "macos")]
    {
        let _ = std::thread::Builder::new().name("ir-pulse".into()).spawn(run);
        let _ = std::thread::Builder::new().name("ir-pulse-smart".into()).spawn(smart_loop);
    }
}

#[cfg(target_os = "macos")]
fn run() {
    use super::{derive_sample, group_scan, pick_internal_disk, Previous};

    let mut prev: Option<(Previous, Instant)> = None;
    let mut pending: Vec<Sample> = Vec::with_capacity(16);
    let mut minute_key: Option<i64> = None;
    let mut causers: HashMap<String, Causer> = HashMap::new();
    let mut prev_writes: HashMap<(i32, u64), u64> = HashMap::new();
    let mut last_scan: Option<Instant> = None;
    let mut arg_cache = HashMap::new();
    let mut crit_since: Option<i64> = None;
    let mut last_notified: Option<i64> = None;
    let mut last_prune = 0i64;
    let mut tick: u64 = 0;
    let thread_port = super::macos::current_thread_port();
    let started = Instant::now();

    loop {
        let t0 = Instant::now();
        if !ENABLED.load(Ordering::Relaxed) {
            prev = None;
            last_scan = None;
            prev_writes.clear();
            std::thread::sleep(Duration::from_secs(SAMPLE_SECS));
            continue;
        }
        let ts = now_ts();
        let reading = super::macos::read_system();
        let disk = pick_internal_disk(&reading.disks).cloned();
        let sample = prev.as_ref().and_then(|(p, at)| {
            let el = at.elapsed().as_secs_f64();
            (!gap_is_rebaseline(el)).then(|| derive_sample(p, &reading, disk.as_ref(), el, ts))
        });
        prev = Some((Previous::from_reading(&reading, disk.as_ref()), Instant::now()));
        let sample_ms = t0.elapsed().as_secs_f64() * 1000.0;

        // Minute rollover: aggregate what belongs to the previous minute.
        let m = bucket_start(ts, LEVEL_1M);
        if minute_key.is_some_and(|k| k != m) {
            let k = minute_key.unwrap_or(m);
            if !pending.is_empty() {
                let mut agg = aggregate_samples(k, &pending);
                agg.causers = rank_causers(causers.values().cloned().collect());
                flush(&agg);
            }
            pending.clear();
            causers.clear();
        }
        minute_key = Some(m);

        // Process scan every 30 s.
        let mut scan_ms = None;
        if tick.is_multiple_of(PROC_EVERY) {
            let s0 = Instant::now();
            let (procs, unreadable) = super::macos::read_processes(&mut arg_cache);
            let el = last_scan.map_or(30.0, |t| t.elapsed().as_secs_f64());
            let (groups, next, written) = group_scan(&procs, &prev_writes, el);
            prev_writes = next;
            last_scan = Some(Instant::now());
            accumulate_causers(&mut causers, &groups, &written);
            let mut st = STATE.lock();
            st.groups = groups.into_iter().take(TOP_PROCS).collect();
            st.groups_at = ts;
            st.unreadable = unreadable;
            scan_ms = Some(s0.elapsed().as_secs_f64() * 1000.0);
        }

        {
            let mut st = STATE.lock();
            st.disk_found = disk.is_some();
            st.capacity = disk.as_ref().and_then(|d| d.capacity);
            let o = &mut st.overhead;
            o.samples += 1;
            o.sample_avg_ms = running_avg(o.sample_avg_ms, o.samples, sample_ms);
            o.sample_max_ms = o.sample_max_ms.max(sample_ms);
            if let Some(cpu) = super::macos::thread_cpu_secs(thread_port) {
                o.cpu_secs = cpu;
                o.running_secs = started.elapsed().as_secs_f64();
                o.cpu_pct = if o.running_secs > 0.0 { cpu / o.running_secs * 100.0 } else { 0.0 };
            }
            if let Some(ms) = scan_ms {
                o.scans += 1;
                o.scan_avg_ms = running_avg(o.scan_avg_ms, o.scans, ms);
                o.scan_max_ms = o.scan_max_ms.max(ms);
            }
            if let Some(s) = sample {
                if st.ring.len() >= RING_LEN {
                    st.ring.pop_front();
                }
                st.ring.push_back(s);
            }
        }
        if let Some(s) = sample {
            pending.push(s);
            // Notification on sustained critical pressure.
            if pressure_bucket(s.pressure) == 2 {
                crit_since.get_or_insert(ts);
            } else {
                crit_since = None;
            }
            let cfg = config();
            if cfg.notify && should_notify(crit_since, ts, cfg.notify_minutes, last_notified) {
                last_notified = Some(ts);
                notify_critical(cfg.notify_minutes);
            }
        }

        // Self-report every 10 min: the overhead budget is checked in the
        // field log, not only in the panel.
        if tick > 0 && tick.is_multiple_of(120) {
            let o = STATE.lock().overhead.clone();
            tracing::info!(
                "pulse: overhead {:.3} % CPU ({:.2} s in {:.0} s), sample avg {:.2} ms max {:.2} ms, scan avg {:.2} ms max {:.2} ms, {} rows written",
                o.cpu_pct, o.cpu_secs, o.running_secs, o.sample_avg_ms, o.sample_max_ms, o.scan_avg_ms, o.scan_max_ms, o.rows_written
            );
        }

        if ts - last_prune >= 3600 {
            last_prune = ts;
            if let Some((_, c)) = PDB.get() {
                if let Err(e) = store::prune(&c.lock(), ts) {
                    tracing::warn!("pulse: prune failed: {e:#}");
                }
            }
        }

        tick = tick.wrapping_add(1);
        let spent = t0.elapsed();
        std::thread::sleep(Duration::from_secs(SAMPLE_SECS).saturating_sub(spent));
    }
}

fn flush(agg: &Agg) {
    let Some((_, c)) = PDB.get() else { return };
    let res = store::flush_minute(&mut c.lock(), agg);
    let mut st = STATE.lock();
    match res {
        Ok(()) => st.overhead.rows_written += 3,
        Err(e) => {
            tracing::warn!("pulse: flush failed: {e:#}");
            st.last_error = Some(format!("{e:#}"));
        }
    }
}

#[cfg(target_os = "macos")]
fn notify_critical(minutes: u32) {
    let top = STATE.lock().groups.first().cloned();
    let detail = match top {
        Some(g) => format!(
            "Größter Verbraucher: {} ({:.1} GB)",
            g.name.replace(['"', '\\'], "'"),
            g.footprint as f64 / 1e9
        ),
        None => "Verursacher unbekannt".into(),
    };
    let script = format!(
        r#"display notification "{detail}" with title "Speicherdruck kritisch" subtitle "seit über {minutes} Minuten""#
    );
    let _ = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(&script)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

// ── smartctl (daily, outside the sampling loop) ────────────────────────────

fn smartctl_path() -> Option<PathBuf> {
    ["/opt/homebrew/bin/smartctl", "/usr/local/bin/smartctl", "/opt/homebrew/sbin/smartctl", "/usr/local/sbin/smartctl"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

#[cfg(target_os = "macos")]
fn smart_loop() {
    std::thread::sleep(Duration::from_secs(90));
    loop {
        let path = smartctl_path();
        STATE.lock().smartctl_present = Some(path.is_some());
        if let Some(bin) = path {
            match std::process::Command::new(bin).args(["-a", "-j", "disk0"]).output() {
                Ok(out) => {
                    let text = String::from_utf8_lossy(&out.stdout);
                    if let Some(info) = super::parse_smartctl_json(&text, now_ts()) {
                        if let Some((_, c)) = PDB.get() {
                            let _ = store::save_smart(&c.lock(), &info);
                        }
                        STATE.lock().smart = Some(info);
                    }
                }
                Err(e) => tracing::warn!("pulse: smartctl failed: {e}"),
            }
        }
        std::thread::sleep(Duration::from_secs(86_400));
    }
}

// ── IPC payloads ───────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct PulseLive {
    pub supported: bool,
    pub enabled: bool,
    pub latest: Option<Sample>,
    /// Last 15 minutes of raw samples (for sparklines).
    pub recent: Vec<Sample>,
    pub groups: Vec<GroupStat>,
    pub groups_at: i64,
    /// Processes that could not be read (other users / root).
    pub unreadable: usize,
    pub disk_found: bool,
    pub capacity: Option<u64>,
    pub overhead: Overhead,
    pub db_bytes: u64,
    pub smart: Option<SmartInfo>,
    pub smartctl_present: Option<bool>,
    pub last_error: Option<String>,
}

pub fn live() -> PulseLive {
    let st = STATE.lock();
    let keep = (15 * 60 / SAMPLE_SECS) as usize;
    let skip = st.ring.len().saturating_sub(keep);
    PulseLive {
        supported: supported(),
        enabled: st.config.enabled,
        latest: st.ring.back().copied(),
        recent: st.ring.iter().skip(skip).copied().collect(),
        groups: st.groups.clone(),
        groups_at: st.groups_at,
        unreadable: st.unreadable,
        disk_found: st.disk_found,
        capacity: st.capacity,
        overhead: st.overhead.clone(),
        db_bytes: PDB.get().map_or(0, |(p, _)| store::file_bytes(p)),
        smart: st.smart.clone(),
        smartctl_present: st.smartctl_present,
        last_error: st.last_error.clone(),
    }
}

#[derive(Debug, Serialize)]
pub struct PulseHistory {
    pub range: String,
    pub bucket_secs: i64,
    pub buckets: Vec<Agg>,
    pub total: Agg,
    /// Day cards, newest first.
    pub days: Vec<Agg>,
    pub forecast: Option<Forecast>,
}

/// History for one range. "1h" comes from the RAM ring (aggregated per minute,
/// including the minute in progress); the rest from pulse.db.
pub fn history(range: &str) -> anyhow::Result<PulseHistory> {
    let (level, back, bucket) = history_plan(range).ok_or_else(|| anyhow::anyhow!("unknown range {range}"))?;
    let now = now_ts();
    let from = now - back;
    let (buckets, days, fc) = {
        let ring: Vec<Sample> = STATE.lock().ring.iter().copied().collect();
        let Some((_, c)) = PDB.get() else {
            return Ok(PulseHistory {
                range: range.into(),
                bucket_secs: bucket,
                buckets: Vec::new(),
                total: Agg::default(),
                days: Vec::new(),
                forecast: None,
            });
        };
        let conn = c.lock();
        let buckets = if level == LEVEL_1M {
            minute_buckets(&ring, from)
        } else if level == LEVEL_DAY {
            store::range(&conn, LEVEL_DAY, super::local_day_start(from), now + 1)?
        } else {
            rebucket(&store::range(&conn, level, bucket_start(from, LEVEL_15M), now + 1)?, bucket)
        };
        let day_from = super::local_day_start(now - back.max(7 * 86_400));
        let mut days = store::range(&conn, LEVEL_DAY, day_from, now + 1)?;
        days.reverse();
        days.truncate(30);
        let month = store::range(&conn, LEVEL_DAY, now - 30 * 86_400, now + 1)?;
        (buckets, days, month)
    };
    let total = merge_aggs(from, &buckets);
    let st = STATE.lock();
    let measured_days = fc.iter().map(|a| a.secs).sum::<f64>() / 86_400.0;
    let written = fc.iter().map(|a| a.written).sum::<f64>();
    let forecast = st.disk_found.then(|| {
        forecast(
            tbw_bytes(st.config.tbw_tb, st.capacity),
            written,
            measured_days,
            st.smart.as_ref().and_then(|s| s.data_written_bytes),
        )
    });
    Ok(PulseHistory { range: range.into(), bucket_secs: bucket, buckets, total, days, forecast })
}

fn minute_buckets(ring: &[Sample], from: i64) -> Vec<Agg> {
    let mut by_minute: std::collections::BTreeMap<i64, Vec<Sample>> = Default::default();
    for s in ring.iter().filter(|s| s.ts >= from) {
        by_minute.entry(bucket_start(s.ts, LEVEL_1M)).or_default().push(*s);
    }
    by_minute.into_iter().map(|(ts, v)| aggregate_samples(ts, &v)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifies_after_the_threshold_and_at_most_hourly() {
        assert!(!should_notify(None, 1000, 10, None));
        assert!(!should_notify(Some(1000), 1000 + 599, 10, None));
        assert!(should_notify(Some(1000), 1000 + 600, 10, None));
        assert!(!should_notify(Some(1000), 5000, 10, Some(5000 - 3599)));
        assert!(should_notify(Some(1000), 5000, 10, Some(5000 - 3600)));
    }

    #[test]
    fn stalls_rebaseline_instead_of_averaging() {
        assert!(!gap_is_rebaseline(5.0));
        assert!(!gap_is_rebaseline(29.0));
        assert!(gap_is_rebaseline(120.0));
        assert!(gap_is_rebaseline(0.1));
    }

    #[test]
    fn every_ui_range_has_a_plan() {
        for r in ["1h", "24h", "7d", "30d", "90d"] {
            assert!(history_plan(r).is_some(), "{r}");
        }
        assert_eq!(history_plan("1y"), None);
        assert_eq!(history_plan("7d"), Some((LEVEL_15M, 7 * 86_400, 3_600)));
    }

    #[test]
    fn causers_keep_the_peak_and_sum_writes_within_a_minute() {
        let mut acc = HashMap::new();
        let g = |fp| vec![GroupStat { name: "Safari".into(), footprint: fp, write_bps: 0.0, procs: 1 }];
        let w = |b| HashMap::from([("Safari".to_string(), b)]);
        accumulate_causers(&mut acc, &g(500), &w(10));
        accumulate_causers(&mut acc, &g(300), &w(20));
        assert_eq!(acc["Safari"], Causer { name: "Safari".into(), written: 30, peak_footprint: 500 });
    }

    #[test]
    fn ring_minutes_are_aggregated_with_the_running_minute() {
        let s = |ts| Sample { ts, dur: 5.0, ssd_write_bps: 2.0, ..Default::default() };
        let ring = [s(50), s(60), s(65), s(130)];
        let b = minute_buckets(&ring, 55);
        assert_eq!(b.iter().map(|a| a.ts).collect::<Vec<_>>(), vec![60, 120]);
        assert_eq!(b[0].written, 20.0);
    }

    #[test]
    fn config_is_clamped_and_old_payloads_keep_defaults() {
        let c: PulseConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(c, PulseConfig::default());
        let n = PulseConfig { notify_minutes: 0, tbw_tb: f64::NAN, ..Default::default() }.normalized();
        assert_eq!((n.notify_minutes, n.tbw_tb), (1, 0.0));
    }
}
