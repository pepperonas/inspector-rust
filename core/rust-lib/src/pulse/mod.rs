//! `pulse` — background monitoring of memory pressure, swap and SSD writes.
//!
//! Layout:
//! - `macos.rs`  — the readers (sysctl, host_statistics64, IOKit, proc_*).
//!   No process spawns; the only `unsafe` in the feature lives there.
//! - `mod.rs`    — the pure model (this file): counter deltas with reset
//!   handling, disk choice, process grouping, aggregation, roll-ups and the
//!   endurance forecast. Everything here is unit-tested without hardware.
//! - `store.rs`  — the separate `pulse.db` (WAL): aggregates + retention.
//! - `sampler.rs`— the background thread, the in-RAM ring buffer, the live
//!   state the IPC reads, notifications and the daily smartctl probe.
//!
//! Honest limits (also in the UI and docs): swap is written by the kernel and
//! cannot be attributed to a process; `ri_diskio_byteswritten` excludes swap;
//! without root, other users' processes (kernel_task, root daemons) are
//! unreadable; the disk counters reset at boot.

#[cfg(target_os = "macos")]
pub(crate) mod macos;
pub mod sampler;
pub mod store;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Live sampling cadence (system metrics).
pub const SAMPLE_SECS: u64 = 5;
/// Process scan cadence (in samples): every 6th sample = 30 s.
pub const PROC_EVERY: u64 = 6;
/// Ring buffer of raw samples: one hour at 5 s.
pub const RING_LEN: usize = 720;
/// Processes kept per scan.
pub const TOP_PROCS: usize = 10;
/// Causers kept per aggregate.
pub const TOP_CAUSERS: usize = 8;
/// Aggregate levels (bucket size in minutes) and their retention.
pub const LEVEL_1M: i64 = 1;
pub const LEVEL_15M: i64 = 15;
pub const LEVEL_DAY: i64 = 1440;
pub const KEEP_1M_SECS: i64 = 7 * 86_400;
pub const KEEP_15M_SECS: i64 = 90 * 86_400;
/// Endurance default: TBW per TB of capacity (common consumer-NVMe rating).
pub const DEFAULT_TBW_PER_TB: f64 = 600.0;

// ── Raw readings (filled by the platform reader) ───────────────────────────

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CpuTicks {
    pub busy: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiskCandidate {
    pub id: u64,
    pub bytes_written: u64,
    pub bytes_read: u64,
    /// "Internal" / "External" (IOKit Protocol Characteristics).
    pub location: Option<String>,
    /// "Apple Fabric", "PCI-Express", "USB", "Secure Digital", …
    pub interconnect: Option<String>,
    pub capacity: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct SystemReading {
    /// 1 normal, 2 warn, 4 critical (0 = unknown).
    pub pressure: u8,
    pub swap_used: u64,
    pub swap_total: u64,
    pub page_size: u64,
    pub swapouts: Option<u64>,
    pub swapins: Option<u64>,
    pub pageouts: Option<u64>,
    pub compressions: Option<u64>,
    pub decompressions: Option<u64>,
    pub wired_bytes: u64,
    pub active_bytes: u64,
    pub inactive_bytes: u64,
    pub free_bytes: u64,
    pub compressor_bytes: u64,
    pub cpu: Option<CpuTicks>,
    /// 0 nominal … 3 critical.
    pub thermal: u8,
    pub disks: Vec<DiskCandidate>,
}

#[derive(Debug, Clone, Default)]
pub struct ProcReading {
    pub pid: i32,
    /// Process start (mach abstime) — with the pid it identifies a process
    /// across pid reuse.
    pub start: u64,
    pub name: String,
    pub path: Option<String>,
    pub responsible_path: Option<String>,
    pub args: Option<Vec<String>>,
    pub footprint: u64,
    pub bytes_written: u64,
}

// ── Counter deltas ─────────────────────────────────────────────────────────

/// Delta of a monotonically increasing counter. A smaller current value means
/// the counter restarted (reboot, driver reload, wrap) — the true amount since
/// the restart is unknown, so the interval contributes 0 rather than a huge
/// wrapped number or a negative one.
pub fn counter_delta(prev: u64, cur: u64) -> u64 {
    cur.saturating_sub(prev)
}

fn opt_delta(prev: Option<u64>, cur: Option<u64>) -> u64 {
    match (prev, cur) {
        (Some(p), Some(c)) => counter_delta(p, c),
        _ => 0,
    }
}

/// Values kept from the previous sample to form deltas.
#[derive(Debug, Clone, Default)]
pub struct Previous {
    pub swapouts: Option<u64>,
    pub swapins: Option<u64>,
    pub pageouts: Option<u64>,
    pub compressions: Option<u64>,
    pub decompressions: Option<u64>,
    pub disk_id: Option<u64>,
    pub disk_written: u64,
    pub disk_read: u64,
    pub cpu: Option<CpuTicks>,
}

impl Previous {
    pub fn from_reading(r: &SystemReading, disk: Option<&DiskCandidate>) -> Self {
        Self {
            swapouts: r.swapouts,
            swapins: r.swapins,
            pageouts: r.pageouts,
            compressions: r.compressions,
            decompressions: r.decompressions,
            disk_id: disk.map(|d| d.id),
            disk_written: disk.map_or(0, |d| d.bytes_written),
            disk_read: disk.map_or(0, |d| d.bytes_read),
            cpu: r.cpu,
        }
    }
}

/// One live sample (rates are per second over the last interval).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct Sample {
    /// Unix seconds.
    pub ts: i64,
    /// Seconds this sample stands for (used for time-in-pressure shares).
    pub dur: f64,
    pub pressure: u8,
    pub swap_used: u64,
    pub swap_total: u64,
    pub swapout_bps: f64,
    pub swapin_bps: f64,
    pub pageout_bps: f64,
    pub compress_ps: f64,
    pub decompress_ps: f64,
    pub ssd_write_bps: f64,
    pub ssd_read_bps: f64,
    /// Swap share of the SSD writes: swapouts × page size (an estimate — the
    /// kernel may batch or compress before writing).
    pub swap_write_bps: f64,
    /// 0..100, None on the first sample.
    pub cpu_pct: Option<f64>,
    pub thermal: u8,
    pub wired: u64,
    pub active: u64,
    pub inactive: u64,
    pub free: u64,
    pub compressor: u64,
}

/// Build a sample from the previous counters and the current reading.
/// `elapsed` is the real time since the previous reading (monotonic clock);
/// rates are divided by it, so a late tick never inflates a rate. A disk whose
/// identity changed (re-enumerated) yields 0 for that interval.
pub fn derive_sample(
    prev: &Previous,
    cur: &SystemReading,
    disk: Option<&DiskCandidate>,
    elapsed: f64,
    ts: i64,
) -> Sample {
    let secs = elapsed.max(0.5);
    let page = cur.page_size.max(1) as f64;
    let (dw, dr) = match disk {
        Some(d) if prev.disk_id == Some(d.id) => (
            counter_delta(prev.disk_written, d.bytes_written),
            counter_delta(prev.disk_read, d.bytes_read),
        ),
        _ => (0, 0),
    };
    let swapouts = opt_delta(prev.swapouts, cur.swapouts) as f64;
    let cpu_pct = match (prev.cpu, cur.cpu) {
        (Some(p), Some(c)) => {
            let busy = counter_delta(p.busy, c.busy) as f64;
            let total = counter_delta(p.total, c.total) as f64;
            (total > 0.0).then(|| (busy / total * 100.0).clamp(0.0, 100.0))
        }
        _ => None,
    };
    let ssd_write_bps = dw as f64 / secs;
    Sample {
        ts,
        dur: secs,
        pressure: cur.pressure,
        swap_used: cur.swap_used,
        swap_total: cur.swap_total,
        swapout_bps: swapouts * page / secs,
        swapin_bps: opt_delta(prev.swapins, cur.swapins) as f64 * page / secs,
        pageout_bps: opt_delta(prev.pageouts, cur.pageouts) as f64 * page / secs,
        compress_ps: opt_delta(prev.compressions, cur.compressions) as f64 / secs,
        decompress_ps: opt_delta(prev.decompressions, cur.decompressions) as f64 / secs,
        ssd_write_bps,
        ssd_read_bps: dr as f64 / secs,
        // Never more than the disk actually took in this interval — when the
        // disk counter is known, the swap estimate is capped by it.
        swap_write_bps: {
            let est = swapouts * page / secs;
            if disk.is_some() && prev.disk_id.is_some() {
                est.min(ssd_write_bps.max(0.0))
            } else {
                est
            }
        },
        cpu_pct,
        thermal: cur.thermal,
        wired: cur.wired_bytes,
        active: cur.active_bytes,
        inactive: cur.inactive_bytes,
        free: cur.free_bytes,
        compressor: cur.compressor_bytes,
    }
}

/// Which pressure bucket a level falls in: 0 normal, 1 warn, 2 critical.
/// Unknown (0) counts as normal so it never inflates the red share.
pub fn pressure_bucket(level: u8) -> usize {
    match level {
        4.. => 2,
        2 | 3 => 1,
        _ => 0,
    }
}

// ── Disk choice ────────────────────────────────────────────────────────────

fn is_removable_bus(d: &DiskCandidate) -> bool {
    matches!(d.interconnect.as_deref(), Some("Secure Digital") | Some("USB") | Some("Thunderbolt"))
}

/// The internal SSD among all block-storage drivers: an "Internal" device on a
/// non-removable bus (the SD reader is reported as Internal, with zero
/// counters); among several, the largest. None when nothing qualifies — then
/// SSD rates are simply not shown, rather than tracking a USB stick.
pub fn pick_internal_disk(disks: &[DiskCandidate]) -> Option<&DiskCandidate> {
    disks
        .iter()
        .filter(|d| d.location.as_deref() == Some("Internal") && !is_removable_bus(d))
        .max_by_key(|d| (d.capacity.unwrap_or(0), d.bytes_written))
}

// ── KERN_PROCARGS2 ─────────────────────────────────────────────────────────

/// Parse a KERN_PROCARGS2 buffer: argc (native i32), exec path, NUL padding,
/// then argc NUL-terminated arguments. Malformed input → None.
pub fn parse_procargs2(buf: &[u8]) -> Option<Vec<String>> {
    if buf.len() < 4 {
        return None;
    }
    let argc = i32::from_ne_bytes(buf[0..4].try_into().ok()?);
    if !(0..=4096).contains(&argc) {
        return None;
    }
    let mut i = 4;
    // exec path
    while i < buf.len() && buf[i] != 0 {
        i += 1;
    }
    while i < buf.len() && buf[i] == 0 {
        i += 1;
    }
    let mut args = Vec::with_capacity(argc as usize);
    for _ in 0..argc {
        if i >= buf.len() {
            break;
        }
        let start = i;
        while i < buf.len() && buf[i] != 0 {
            i += 1;
        }
        args.push(String::from_utf8_lossy(&buf[start..i]).into_owned());
        i += 1;
    }
    Some(args)
}

// ── Grouping ───────────────────────────────────────────────────────────────

/// Name of the OUTERMOST `.app` bundle in a path — a helper deep inside
/// `Google Chrome.app/…/Google Chrome Helper.app` belongs to "Google Chrome".
pub fn outer_app_name(path: &str) -> Option<String> {
    path.split('/')
        .find(|seg| seg.len() > 4 && seg.ends_with(".app"))
        .map(|seg| seg.trim_end_matches(".app").to_string())
}

/// Terminal emulators: never used as a group (see `group_for`).
pub fn is_terminal_app(app: &str) -> bool {
    matches!(
        app,
        "iTerm" | "iTerm2" | "Terminal" | "Warp" | "kitty" | "Ghostty" | "WezTerm" | "Alacritty" | "Hyper" | "Tabby"
    )
}

/// Group label for a process. Order matters: the explicit rules for the known
/// memory hogs first (they're all `java` or helpers without a useful bundle),
/// then the responsible app (Activity Monitor's grouping), then the bundle the
/// binary itself sits in, then the bare name.
pub fn group_for(p: &ProcReading) -> String {
    let name = p.name.as_str();
    let lname = name.to_ascii_lowercase();
    if lname == "java" || lname.starts_with("java") {
        if let Some(args) = &p.args {
            let joined = args.join(" ");
            if joined.contains("KotlinCompileDaemon") || joined.contains("kotlin.daemon") {
                return "Kotlin-Daemon".into();
            }
            if joined.contains("GradleDaemon") || joined.contains("org.gradle") {
                return "Gradle-Daemon".into();
            }
        }
    }
    let path = p.path.as_deref().unwrap_or("");
    if path.contains("/Docker.app/")
        || lname.starts_with("com.docker")
        || matches!(lname.as_str(), "docker" | "vpnkit" | "qemu-system-aarch64" | "virtualization")
    {
        return "Docker".into();
    }
    if lname == "java" || lname.starts_with("java") {
        return "Java".into();
    }
    // Claude Code ships as `…/claude/versions/2.1.292` — named by version.
    if path.contains("/claude/versions/") || lname == "claude" {
        return "Claude Code".into();
    }
    if let Some(app) = p.responsible_path.as_deref().and_then(outer_app_name) {
        // A terminal is "responsible" for everything started in it (cargo,
        // node, Claude Code …) — lumping those under "iTerm" hides exactly
        // the process the user is looking for, so terminals don't group.
        if !is_terminal_app(&app) {
            return app;
        }
    }
    if let Some(app) = outer_app_name(path) {
        return app;
    }
    if name.is_empty() {
        format!("pid {}", p.pid)
    } else {
        name.to_string()
    }
}

/// A group's live figures.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GroupStat {
    pub name: String,
    pub footprint: u64,
    pub write_bps: f64,
    pub procs: u32,
}

/// Result of a process scan: groups (by footprint), the new cumulative-write
/// map, and the bytes each group wrote in the interval.
pub type ScanResult = (Vec<GroupStat>, HashMap<(i32, u64), u64>, HashMap<String, u64>);

/// Fold one process scan into groups. `prev_writes` maps (pid, start) → the
/// cumulative bytes seen last scan; a process seen for the first time
/// contributes no write rate (its counter covers its whole life, not the
/// interval). Returns the groups sorted by footprint and the new write map.
pub fn group_scan(
    procs: &[ProcReading],
    prev_writes: &HashMap<(i32, u64), u64>,
    elapsed: f64,
) -> ScanResult {
    let secs = elapsed.max(1.0);
    let mut groups: HashMap<String, GroupStat> = HashMap::new();
    let mut next = HashMap::with_capacity(procs.len());
    let mut written: HashMap<String, u64> = HashMap::new();
    for p in procs {
        let key = (p.pid, p.start);
        let delta = prev_writes.get(&key).map_or(0, |&prev| counter_delta(prev, p.bytes_written));
        next.insert(key, p.bytes_written);
        let g = group_for(p);
        let entry = groups.entry(g.clone()).or_insert_with(|| GroupStat { name: g.clone(), ..Default::default() });
        entry.footprint += p.footprint;
        entry.write_bps += delta as f64 / secs;
        entry.procs += 1;
        *written.entry(g).or_default() += delta;
    }
    let mut out: Vec<GroupStat> = groups.into_values().collect();
    out.sort_by(|a, b| b.footprint.cmp(&a.footprint).then_with(|| a.name.cmp(&b.name)));
    (out, next, written)
}

// ── Aggregates ─────────────────────────────────────────────────────────────

/// A causer within an aggregate: bytes it wrote (excluding swap) and its peak
/// footprint in the period.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Causer {
    pub name: String,
    pub written: u64,
    pub peak_footprint: u64,
}

/// One aggregate bucket (1 min, 15 min or a day).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Agg {
    /// Bucket start, unix seconds.
    pub ts: i64,
    /// Seconds of measured time inside the bucket.
    pub secs: f64,
    /// Seconds spent at pressure normal / warn / critical.
    pub p_normal: f64,
    pub p_warn: f64,
    pub p_crit: f64,
    pub swap_min: u64,
    pub swap_avg: f64,
    pub swap_max: u64,
    pub swap_total: u64,
    /// Bytes written to / read from the internal SSD.
    pub written: f64,
    pub read: f64,
    /// Estimated bytes of swap written (swapouts × page size).
    pub swap_written: f64,
    pub swapped_in: f64,
    pub compressions: f64,
    pub cpu_avg: Option<f64>,
    pub causers: Vec<Causer>,
}

/// Aggregate raw samples into one bucket. Sums are rate × duration, so a
/// bucket with missing samples sums only what was measured.
pub fn aggregate_samples(ts: i64, samples: &[Sample]) -> Agg {
    let mut a = Agg { ts, swap_min: u64::MAX, ..Default::default() };
    let mut swap_weighted = 0.0;
    let (mut cpu_sum, mut cpu_secs) = (0.0, 0.0);
    for s in samples {
        let d = s.dur;
        a.secs += d;
        match pressure_bucket(s.pressure) {
            2 => a.p_crit += d,
            1 => a.p_warn += d,
            _ => a.p_normal += d,
        }
        a.swap_min = a.swap_min.min(s.swap_used);
        a.swap_max = a.swap_max.max(s.swap_used);
        a.swap_total = a.swap_total.max(s.swap_total);
        swap_weighted += s.swap_used as f64 * d;
        a.written += s.ssd_write_bps * d;
        a.read += s.ssd_read_bps * d;
        a.swap_written += s.swap_write_bps * d;
        a.swapped_in += s.swapin_bps * d;
        a.compressions += s.compress_ps * d;
        if let Some(c) = s.cpu_pct {
            cpu_sum += c * d;
            cpu_secs += d;
        }
    }
    if samples.is_empty() {
        a.swap_min = 0;
    } else if a.secs > 0.0 {
        a.swap_avg = swap_weighted / a.secs;
    }
    a.cpu_avg = (cpu_secs > 0.0).then(|| cpu_sum / cpu_secs);
    a
}

/// Merge lower-level buckets into one (the roll-up 1m → 15m → day).
pub fn merge_aggs(ts: i64, parts: &[Agg]) -> Agg {
    let mut a = Agg { ts, swap_min: u64::MAX, ..Default::default() };
    let mut swap_weighted = 0.0;
    let (mut cpu_sum, mut cpu_secs) = (0.0, 0.0);
    let mut causers: Vec<Causer> = Vec::new();
    for p in parts {
        a.secs += p.secs;
        a.p_normal += p.p_normal;
        a.p_warn += p.p_warn;
        a.p_crit += p.p_crit;
        if p.secs > 0.0 {
            a.swap_min = a.swap_min.min(p.swap_min);
        }
        a.swap_max = a.swap_max.max(p.swap_max);
        a.swap_total = a.swap_total.max(p.swap_total);
        swap_weighted += p.swap_avg * p.secs;
        a.written += p.written;
        a.read += p.read;
        a.swap_written += p.swap_written;
        a.swapped_in += p.swapped_in;
        a.compressions += p.compressions;
        if let Some(c) = p.cpu_avg {
            cpu_sum += c * p.secs;
            cpu_secs += p.secs;
        }
        causers = merge_causers(&causers, &p.causers);
    }
    if a.swap_min == u64::MAX {
        a.swap_min = 0;
    }
    if a.secs > 0.0 {
        a.swap_avg = swap_weighted / a.secs;
    }
    a.cpu_avg = (cpu_secs > 0.0).then(|| cpu_sum / cpu_secs);
    a.causers = causers;
    a
}

/// Combine two causer lists: written bytes add up, the peak footprint is the
/// larger one; the result is ranked and capped at TOP_CAUSERS.
pub fn merge_causers(a: &[Causer], b: &[Causer]) -> Vec<Causer> {
    let mut map: HashMap<&str, Causer> = HashMap::new();
    for c in a.iter().chain(b) {
        let e = map.entry(c.name.as_str()).or_insert_with(|| Causer { name: c.name.clone(), ..Default::default() });
        e.written += c.written;
        e.peak_footprint = e.peak_footprint.max(c.peak_footprint);
    }
    rank_causers(map.into_values().collect())
}

/// Rank by footprint first (memory pressure is the main question), writes as
/// the tie-break; stable by name.
pub fn rank_causers(mut v: Vec<Causer>) -> Vec<Causer> {
    v.sort_by(|a, b| {
        b.peak_footprint
            .cmp(&a.peak_footprint)
            .then(b.written.cmp(&a.written))
            .then_with(|| a.name.cmp(&b.name))
    });
    v.truncate(TOP_CAUSERS);
    v
}

/// Start of the bucket containing `ts` for a fixed-size level (unix-aligned).
pub fn bucket_start(ts: i64, level_minutes: i64) -> i64 {
    let size = level_minutes * 60;
    ts - ts.rem_euclid(size)
}

/// Start of the LOCAL day containing `ts` (a day card should mean the user's
/// calendar day, not a UTC one). DST-safe via chrono.
pub fn local_day_start(ts: i64) -> i64 {
    use chrono::{Local, TimeZone};
    let Some(dt) = Local.timestamp_opt(ts, 0).single() else { return bucket_start(ts, LEVEL_DAY) };
    let midnight = dt.date_naive().and_hms_opt(0, 0, 0).expect("midnight exists");
    Local
        .from_local_datetime(&midnight)
        .earliest()
        .map(|d| d.timestamp())
        .unwrap_or_else(|| bucket_start(ts, LEVEL_DAY))
}

/// Merge buckets of one series into wider buckets (e.g. 15-min rows into
/// hourly bars). Input need not be sorted; output is sorted by ts.
pub fn rebucket(rows: &[Agg], bucket_secs: i64) -> Vec<Agg> {
    let mut groups: std::collections::BTreeMap<i64, Vec<Agg>> = Default::default();
    for r in rows {
        groups.entry(r.ts - r.ts.rem_euclid(bucket_secs)).or_default().push(r.clone());
    }
    groups.into_iter().map(|(ts, v)| merge_aggs(ts, &v)).collect()
}

// ── Forecast ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Forecast {
    /// Rated endurance in bytes.
    pub tbw_bytes: f64,
    /// Average bytes written per day in the window used.
    pub per_day: f64,
    /// Days the average is based on (≤ 30).
    pub days: f64,
    /// Bytes already written over the drive's life, when smartctl knows it.
    pub lifetime_written: Option<f64>,
    /// Years until the rating is reached at this pace (from today when the
    /// lifetime figure is known, from zero otherwise).
    pub years: Option<f64>,
}

/// Endurance forecast. Needs at least a day of measured data — a projection
/// from a few minutes would be pure noise.
pub fn forecast(tbw_bytes: f64, written_window: f64, days_window: f64, lifetime_written: Option<f64>) -> Forecast {
    let days = days_window.min(30.0);
    let per_day = if days > 0.0 { written_window / days } else { 0.0 };
    let years = if days >= 1.0 && per_day > 0.0 && tbw_bytes > 0.0 {
        let remaining = (tbw_bytes - lifetime_written.unwrap_or(0.0)).max(0.0);
        Some(remaining / per_day / 365.0)
    } else {
        None
    };
    Forecast { tbw_bytes, per_day, days, lifetime_written, years }
}

/// TBW for a drive: an explicit setting wins; otherwise 600 TB per TB of
/// capacity (decimal TB, as drives are sold).
pub fn tbw_bytes(setting_tb: f64, capacity: Option<u64>) -> f64 {
    if setting_tb > 0.0 {
        return setting_tb * 1e12;
    }
    let cap_tb = capacity.map_or(1.0, |c| (c as f64 / 1e12).max(0.1));
    DEFAULT_TBW_PER_TB * cap_tb * 1e12
}

// ── smartctl ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SmartInfo {
    pub percentage_used: Option<u32>,
    /// NVMe data units are 512 000 bytes each.
    pub data_written_bytes: Option<f64>,
    pub model: Option<String>,
    /// Unix seconds of the reading.
    pub at: i64,
}

/// Parse `smartctl -a -j <disk>` output (NVMe health log).
pub fn parse_smartctl_json(text: &str, at: i64) -> Option<SmartInfo> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let log = v.get("nvme_smart_health_information_log")?;
    Some(SmartInfo {
        percentage_used: log.get("percentage_used").and_then(|x| x.as_u64()).map(|x| x as u32),
        data_written_bytes: log.get("data_units_written").and_then(|x| x.as_f64()).map(|u| u * 512_000.0),
        model: v.get("model_name").and_then(|x| x.as_str()).map(str::to_string),
        at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading() -> SystemReading {
        SystemReading {
            pressure: 1,
            swap_used: 1 << 30,
            swap_total: 2 << 30,
            page_size: 16_384,
            swapouts: Some(100),
            swapins: Some(50),
            pageouts: Some(10),
            compressions: Some(1000),
            decompressions: Some(500),
            cpu: Some(CpuTicks { busy: 100, total: 400 }),
            ..Default::default()
        }
    }

    fn disk(id: u64, w: u64) -> DiskCandidate {
        DiskCandidate {
            id,
            bytes_written: w,
            bytes_read: w / 2,
            location: Some("Internal".into()),
            interconnect: Some("Apple Fabric".into()),
            capacity: Some(1_000_000_000_000),
        }
    }

    #[test]
    fn counter_delta_treats_a_smaller_value_as_a_reset() {
        assert_eq!(counter_delta(10, 25), 15);
        assert_eq!(counter_delta(25, 25), 0);
        assert_eq!(counter_delta(1_000_000, 3), 0, "reboot/reset must not wrap");
    }

    #[test]
    fn rates_are_divided_by_real_elapsed_time() {
        let r0 = reading();
        let d0 = disk(7, 1_000_000);
        let prev = Previous::from_reading(&r0, Some(&d0));
        let mut r1 = r0.clone();
        r1.swapouts = Some(110); // +10 pages
        r1.compressions = Some(1100);
        r1.cpu = Some(CpuTicks { busy: 150, total: 600 });
        let d1 = disk(7, 11_000_000); // +10 MB
        let s = derive_sample(&prev, &r1, Some(&d1), 10.0, 0);
        assert!((s.ssd_write_bps - 1_000_000.0).abs() < 1e-6);
        assert!((s.swapout_bps - 10.0 * 16_384.0 / 10.0).abs() < 1e-6);
        assert!((s.compress_ps - 10.0).abs() < 1e-9);
        assert_eq!(s.cpu_pct, Some(25.0));
        // Same counters over twice the time = half the rate.
        let s2 = derive_sample(&prev, &r1, Some(&d1), 20.0, 0);
        assert!((s2.ssd_write_bps - 500_000.0).abs() < 1e-6);
    }

    #[test]
    fn a_reset_or_a_new_disk_contributes_zero() {
        let r0 = reading();
        let prev = Previous::from_reading(&r0, Some(&disk(7, 9_000_000)));
        let mut r1 = r0.clone();
        r1.swapouts = Some(3); // counter restarted
        let s = derive_sample(&prev, &r1, Some(&disk(7, 100)), 5.0, 0);
        assert_eq!(s.swapout_bps, 0.0);
        assert_eq!(s.ssd_write_bps, 0.0);
        // Re-enumerated disk (different id) with a larger counter: still 0.
        let s = derive_sample(&prev, &r0, Some(&disk(8, 99_000_000)), 5.0, 0);
        assert_eq!(s.ssd_write_bps, 0.0);
    }

    #[test]
    fn first_sample_has_no_cpu_and_no_rates() {
        let s = derive_sample(&Previous::default(), &reading(), Some(&disk(7, 5)), 5.0, 0);
        assert_eq!(s.cpu_pct, None);
        assert_eq!(s.ssd_write_bps, 0.0);
        assert_eq!(s.swapout_bps, 0.0);
    }

    #[test]
    fn swap_estimate_never_exceeds_what_the_disk_took() {
        let r0 = reading();
        let prev = Previous::from_reading(&r0, Some(&disk(7, 0)));
        let mut r1 = r0.clone();
        r1.swapouts = Some(100 + 1000); // 16 MB estimated
        let s = derive_sample(&prev, &r1, Some(&disk(7, 1_000_000)), 1.0, 0);
        assert!((s.swap_write_bps - 1_000_000.0).abs() < 1e-6);
        assert!(s.swapout_bps > s.swap_write_bps, "the raw swap-out rate stays visible");
    }

    #[test]
    fn pressure_levels_map_to_three_buckets() {
        assert_eq!(pressure_bucket(1), 0);
        assert_eq!(pressure_bucket(0), 0);
        assert_eq!(pressure_bucket(2), 1);
        assert_eq!(pressure_bucket(4), 2);
    }

    #[test]
    fn the_internal_ssd_is_chosen_not_the_sd_reader_or_usb() {
        let ssd = disk(1, 500);
        let sd = DiskCandidate {
            id: 2,
            interconnect: Some("Secure Digital".into()),
            capacity: Some(4_000_000_000_000),
            ..disk(2, 0)
        };
        let usb = DiskCandidate {
            id: 3,
            location: Some("External".into()),
            interconnect: Some("USB".into()),
            ..disk(3, 900)
        };
        let all = [sd.clone(), usb.clone(), ssd.clone()];
        assert_eq!(pick_internal_disk(&all).map(|d| d.id), Some(1));
        assert_eq!(pick_internal_disk(&[sd, usb]).map(|d| d.id), None);
    }

    #[test]
    fn procargs2_is_parsed() {
        let mut buf = 3i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/usr/bin/java\0\0\0\0java\0-Xmx2g\0org.gradle.launcher.daemon.bootstrap.GradleDaemon\0HOME=/x\0");
        let args = parse_procargs2(&buf).unwrap();
        assert_eq!(args, vec!["java", "-Xmx2g", "org.gradle.launcher.daemon.bootstrap.GradleDaemon"]);
        assert_eq!(parse_procargs2(&[1, 2]), None);
        assert_eq!(parse_procargs2(&(-5i32).to_ne_bytes()), None);
    }

    fn proc(name: &str, path: Option<&str>, resp: Option<&str>, args: Option<&[&str]>) -> ProcReading {
        ProcReading {
            pid: 42,
            name: name.into(),
            path: path.map(str::to_string),
            responsible_path: resp.map(str::to_string),
            args: args.map(|a| a.iter().map(|s| s.to_string()).collect()),
            ..Default::default()
        }
    }

    #[test]
    fn grouping_follows_rules_then_responsible_app_then_bundle() {
        assert_eq!(
            group_for(&proc("java", None, None, Some(&["java", "org.gradle.launcher.daemon.bootstrap.GradleDaemon"]))),
            "Gradle-Daemon"
        );
        assert_eq!(
            group_for(&proc("java", None, None, Some(&["java", "org.jetbrains.kotlin.daemon.KotlinCompileDaemon"]))),
            "Kotlin-Daemon"
        );
        assert_eq!(group_for(&proc("java", None, None, None)), "Java");
        assert_eq!(
            group_for(&proc("com.docker.backend", Some("/Applications/Docker.app/Contents/MacOS/com.docker.backend"), None, None)),
            "Docker"
        );
        let helper = "/Applications/Google Chrome.app/Contents/Frameworks/X.framework/Helpers/Google Chrome Helper (Renderer).app/Contents/MacOS/Google Chrome Helper (Renderer)";
        assert_eq!(group_for(&proc("Google Chrome Helper", Some(helper), None, None)), "Google Chrome");
        // The responsible app wins over the binary's own location.
        assert_eq!(
            group_for(&proc("node", Some("/opt/homebrew/bin/node"), Some("/Applications/Visual Studio Code.app/Contents/MacOS/Electron"), None)),
            "Visual Studio Code"
        );
        assert_eq!(group_for(&proc("node", Some("/opt/homebrew/bin/node"), None, None)), "node");
        assert_eq!(
            group_for(&proc("2.1.292", Some("/Users/x/.local/share/claude/versions/2.1.292"), Some("/Applications/iTerm.app/Contents/MacOS/iTerm2"), None)),
            "Claude Code"
        );
        // Started from a terminal: the process itself, not "iTerm".
        assert_eq!(
            group_for(&proc("cargo", Some("/Users/x/.cargo/bin/cargo"), Some("/Applications/iTerm.app/Contents/MacOS/iTerm2"), None)),
            "cargo"
        );
        assert_eq!(group_for(&proc("", None, None, None)), "pid 42");
    }

    #[test]
    fn group_scan_sums_footprint_and_only_counts_known_writers() {
        let mk = |pid: i32, name: &str, fp: u64, w: u64| ProcReading {
            pid,
            start: 1,
            name: name.into(),
            path: Some(format!("/Applications/{name}.app/Contents/MacOS/{name}")),
            footprint: fp,
            bytes_written: w,
            ..Default::default()
        };
        let scan1 = [mk(1, "Safari", 100, 1000), mk(2, "Safari", 50, 0), mk(3, "Mail", 300, 5)];
        let (g, prev, _) = group_scan(&scan1, &HashMap::new(), 30.0);
        assert_eq!(g[0].name, "Mail");
        assert_eq!(g[1].footprint, 150);
        assert_eq!(g.iter().map(|x| x.write_bps).sum::<f64>(), 0.0, "no baseline yet");
        let scan2 = [mk(1, "Safari", 100, 4000), mk(2, "Safari", 50, 0), mk(3, "Mail", 300, 5)];
        let (g, _, written) = group_scan(&scan2, &prev, 30.0);
        let safari = g.iter().find(|x| x.name == "Safari").unwrap();
        assert!((safari.write_bps - 100.0).abs() < 1e-9);
        assert_eq!(written["Safari"], 3000);
        assert_eq!(safari.procs, 2);
    }

    fn sample(pressure: u8, swap: u64, w: f64, swap_w: f64, dur: f64) -> Sample {
        Sample {
            dur,
            pressure,
            swap_used: swap,
            swap_total: 10,
            ssd_write_bps: w,
            swap_write_bps: swap_w,
            cpu_pct: Some(50.0),
            ..Default::default()
        }
    }

    #[test]
    fn aggregation_sums_bytes_and_splits_time_by_pressure() {
        let s = [sample(1, 2, 100.0, 10.0, 5.0), sample(2, 6, 200.0, 0.0, 5.0), sample(4, 4, 0.0, 0.0, 10.0)];
        let a = aggregate_samples(60, &s);
        assert_eq!(a.secs, 20.0);
        assert_eq!((a.p_normal, a.p_warn, a.p_crit), (5.0, 5.0, 10.0));
        assert_eq!(a.written, 1500.0);
        assert_eq!(a.swap_written, 50.0);
        assert_eq!((a.swap_min, a.swap_max), (2, 6));
        assert!((a.swap_avg - (2.0 * 5.0 + 6.0 * 5.0 + 4.0 * 10.0) / 20.0).abs() < 1e-9);
        assert_eq!(a.cpu_avg, Some(50.0));
        let empty = aggregate_samples(0, &[]);
        assert_eq!((empty.swap_min, empty.secs, empty.cpu_avg), (0, 0.0, None));
    }

    #[test]
    fn roll_up_preserves_sums_and_extremes() {
        let a = Agg {
            secs: 60.0,
            p_crit: 30.0,
            p_normal: 30.0,
            swap_min: 5,
            swap_max: 9,
            swap_avg: 7.0,
            written: 1000.0,
            causers: vec![Causer { name: "A".into(), written: 10, peak_footprint: 100 }],
            ..Default::default()
        };
        let b = Agg {
            secs: 60.0,
            p_normal: 60.0,
            swap_min: 3,
            swap_max: 4,
            swap_avg: 3.5,
            written: 500.0,
            causers: vec![
                Causer { name: "A".into(), written: 5, peak_footprint: 300 },
                Causer { name: "B".into(), written: 99, peak_footprint: 50 },
            ],
            ..Default::default()
        };
        let m = merge_aggs(0, &[a, b]);
        assert_eq!(m.written, 1500.0);
        assert_eq!((m.secs, m.p_crit, m.p_normal), (120.0, 30.0, 90.0));
        assert_eq!((m.swap_min, m.swap_max), (3, 9));
        assert!((m.swap_avg - 5.25).abs() < 1e-9);
        assert_eq!(m.causers[0], Causer { name: "A".into(), written: 15, peak_footprint: 300 });
        assert_eq!(m.causers[1].name, "B");
    }

    #[test]
    fn causers_are_capped() {
        let many: Vec<Causer> =
            (0..20).map(|i| Causer { name: format!("p{i}"), written: 0, peak_footprint: i }).collect();
        let r = merge_causers(&many, &[]);
        assert_eq!(r.len(), TOP_CAUSERS);
        assert_eq!(r[0].name, "p19");
    }

    #[test]
    fn rebucket_merges_into_wider_aligned_buckets() {
        let rows: Vec<Agg> =
            (0..8).map(|i| Agg { ts: i * 900, secs: 900.0, written: 1.0, ..Default::default() }).collect();
        let hours = rebucket(&rows, 3600);
        assert_eq!(hours.len(), 2);
        assert_eq!(hours[1].ts, 3600);
        assert_eq!(hours[0].written, 4.0);
        assert_eq!(bucket_start(3599, 15), 2700);
    }

    #[test]
    fn local_day_start_is_midnight_and_idempotent() {
        let ts = 1_790_000_000;
        let d = local_day_start(ts);
        assert!(d <= ts && ts - d < 86_400 + 3_600);
        assert_eq!(local_day_start(d), d);
    }

    #[test]
    fn forecast_needs_a_day_and_counts_down_from_lifetime_writes() {
        let tb = 1e12;
        // 30 days at 100 GB/day against 600 TBW → 6000 days ≈ 16.4 years.
        let f = forecast(600.0 * tb, 30.0 * 100e9, 30.0, None);
        assert!((f.per_day - 100e9).abs() < 1.0);
        assert!((f.years.unwrap() - 6000.0 / 365.0).abs() < 1e-6);
        // Half the rating already used → half the time left.
        let g = forecast(600.0 * tb, 30.0 * 100e9, 30.0, Some(300.0 * tb));
        assert!((g.years.unwrap() - 3000.0 / 365.0).abs() < 1e-6);
        // Less than a day of data → no projection.
        assert_eq!(forecast(600.0 * tb, 5e9, 0.2, None).years, None);
        // Windows longer than 30 days are capped.
        assert_eq!(forecast(600.0 * tb, 90.0 * 1e9, 90.0, None).days, 30.0);
    }

    #[test]
    fn tbw_defaults_to_600_per_tb_of_capacity() {
        assert_eq!(tbw_bytes(0.0, Some(2_000_000_000_000)), 1200.0 * 1e12);
        assert_eq!(tbw_bytes(0.0, None), 600.0 * 1e12);
        assert_eq!(tbw_bytes(150.0, Some(2_000_000_000_000)), 150.0 * 1e12);
    }

    #[test]
    fn smartctl_json_is_parsed() {
        let j = r#"{"model_name":"APPLE SSD AP0512Z","nvme_smart_health_information_log":{"percentage_used":3,"data_units_written":100000}}"#;
        let s = parse_smartctl_json(j, 9).unwrap();
        assert_eq!(s.percentage_used, Some(3));
        assert_eq!(s.data_written_bytes, Some(100000.0 * 512_000.0));
        assert_eq!(s.model.as_deref(), Some("APPLE SSD AP0512Z"));
        assert_eq!(parse_smartctl_json("{}", 0), None);
        assert_eq!(parse_smartctl_json("not json", 0), None);
    }
}
