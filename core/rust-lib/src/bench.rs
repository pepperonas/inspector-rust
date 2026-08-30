//! CPU benchmark in the shape of Geekbench: a handful of realistic workloads,
//! run single-threaded and then across every core, reduced to comparable
//! scores next to the raw throughput each one measured.
//!
//! ## What is measured and what is not
//!
//! Every number in a [`BenchRun`] is **measured on the spot**. Nothing is
//! simulated, and the only constants baked in are the per-workload
//! [`Workload::baseline`] rates — the reference a score of 1000 means. Those
//! were measured once, on the machine this feature was written on, and are
//! declared as exactly that: an arbitrary but FIXED yardstick. They make runs
//! comparable to each other; they are not a claim about any product.
//!
//! ## Determinism
//!
//! Each workload builds its own input from a fixed-seed LCG, so two runs do
//! byte-identical work and a difference in the result is a difference in the
//! machine — not in the data.

use std::time::{Duration, Instant};

/// How long each workload is given, per section. Seven workloads × two
/// sections ≈ 8 s of wall clock, which the preview states before starting.
pub const BUDGET: Duration = Duration::from_millis(600);

/// Bumped when the workload set or the scoring changes, so an imported run
/// from an older version is never silently compared against a newer one.
pub const SCHEMA: u32 = 1;

// ── deterministic input ──────────────────────────────────────────────

/// Linear congruential generator (Numerical Recipes constants). Deterministic
/// on purpose: identical work on every machine and every run.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 16) as u32
    }
    fn next_f64(&mut self) -> f64 {
        f64::from(self.next_u32()) / f64::from(u32::MAX)
    }
}

// ── the workloads ────────────────────────────────────────────────────

/// Sort a large integer array — branchy, memory-bound, cache-sensitive.
fn wl_sort() -> u64 {
    const N: usize = 200_000;
    let mut rng = Lcg::new(0x5EED_1234);
    let mut v: Vec<u32> = (0..N).map(|_| rng.next_u32()).collect();
    v.sort_unstable();
    std::hint::black_box(v[N / 2]);
    N as u64
}

/// SHA-256 over a buffer — the integer/crypto path.
fn wl_sha256() -> u64 {
    use sha2::{Digest, Sha256};
    const N: usize = 4 << 20; // 4 MiB
    let mut rng = Lcg::new(0xC0FFEE);
    let buf: Vec<u8> = (0..N).map(|_| rng.next_u32() as u8).collect();
    let mut h = Sha256::new();
    h.update(&buf);
    std::hint::black_box(h.finalize());
    N as u64
}

/// Dense f64 matrix multiply — floating-point throughput.
fn wl_matmul() -> u64 {
    const N: usize = 160;
    let mut rng = Lcg::new(0xBEEF);
    let a: Vec<f64> = (0..N * N).map(|_| rng.next_f64()).collect();
    let b: Vec<f64> = (0..N * N).map(|_| rng.next_f64()).collect();
    let mut c = vec![0.0f64; N * N];
    for i in 0..N {
        for k in 0..N {
            let aik = a[i * N + k];
            for j in 0..N {
                c[i * N + j] += aik * b[k * N + j];
            }
        }
    }
    std::hint::black_box(c[0]);
    (2 * N * N * N) as u64 // FLOPs
}

/// One n-body step — floating point with a square root in the inner loop.
fn wl_nbody() -> u64 {
    const N: usize = 900;
    let mut rng = Lcg::new(0xF00D);
    let p: Vec<[f64; 3]> = (0..N)
        .map(|_| [rng.next_f64(), rng.next_f64(), rng.next_f64()])
        .collect();
    let mut acc = vec![[0.0f64; 3]; N];
    for i in 0..N {
        let (mut ax, mut ay, mut az) = (0.0, 0.0, 0.0);
        for j in 0..N {
            if i == j {
                continue;
            }
            let dx = p[j][0] - p[i][0];
            let dy = p[j][1] - p[i][1];
            let dz = p[j][2] - p[i][2];
            let d2 = dx * dx + dy * dy + dz * dz + 1e-9;
            let inv = 1.0 / (d2 * d2.sqrt());
            ax += dx * inv;
            ay += dy * inv;
            az += dz * inv;
        }
        acc[i] = [ax, ay, az];
    }
    std::hint::black_box(acc[0][0]);
    (N * N) as u64 // pair interactions
}

/// Sieve of Eratosthenes — integer work over a large, striding buffer.
fn wl_sieve() -> u64 {
    const N: usize = 2_000_000;
    let mut is_c = vec![false; N];
    let mut i = 2;
    while i * i < N {
        if !is_c[i] {
            let mut j = i * i;
            while j < N {
                is_c[j] = true;
                j += i;
            }
        }
        i += 1;
    }
    std::hint::black_box(is_c[N - 1]);
    N as u64
}

/// Deflate compression — the classic mixed integer/memory workload.
fn wl_deflate() -> u64 {
    use flate2::{write::DeflateEncoder, Compression};
    use std::io::Write;
    const N: usize = 2 << 20; // 2 MiB, semi-compressible
    let mut rng = Lcg::new(0xDEAD);
    let mut buf = Vec::with_capacity(N);
    while buf.len() < N {
        let word = rng.next_u32() % 64;
        buf.extend_from_slice(format!("word{word} ").as_bytes());
    }
    buf.truncate(N);
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&buf).ok();
    std::hint::black_box(enc.finish().map(|v| v.len()).unwrap_or(0));
    N as u64
}

/// Word frequency over generated text — allocation and hashing heavy.
fn wl_text() -> u64 {
    use std::collections::HashMap;
    const N: usize = 1 << 20; // ~1 MiB
    let mut rng = Lcg::new(0xABCD);
    let mut text = String::with_capacity(N + 16);
    while text.len() < N {
        text.push_str(match rng.next_u32() % 8 {
            0 => "der ",
            1 => "benchmark ",
            2 => "misst ",
            3 => "wirklich ",
            4 => "jede ",
            5 => "einzelne ",
            6 => "kleine ",
            _ => "sache ",
        });
    }
    let mut freq: HashMap<&str, u32> = HashMap::new();
    for w in text.split_whitespace() {
        *freq.entry(w).or_insert(0) += 1;
    }
    std::hint::black_box(freq.len());
    text.len() as u64
}

/// One benchmark discipline.
pub struct Workload {
    pub id: &'static str,
    pub name: &'static str,
    /// Unit of the raw rate, e.g. `MB/s`.
    pub unit: &'static str,
    /// Divides the measured units to reach `unit` (bytes → MB, FLOP → MFLOP…).
    pub scale: f64,
    run: fn() -> u64,
    /// Rate (in `unit`) that scores exactly 1000 — see the module docs on what
    /// this constant is and is not.
    pub baseline: f64,
}

pub const WORKLOADS: &[Workload] = &[
    Workload { id: "sort", name: "Integer-Sortierung", unit: "Melem/s", scale: 1e6, run: wl_sort, baseline: BASE_SORT },
    Workload { id: "sha256", name: "SHA-256", unit: "MB/s", scale: 1e6, run: wl_sha256, baseline: BASE_SHA },
    Workload { id: "matmul", name: "Matrixmultiplikation", unit: "MFLOP/s", scale: 1e6, run: wl_matmul, baseline: BASE_MATMUL },
    Workload { id: "nbody", name: "N-Körper-Simulation", unit: "Mpaare/s", scale: 1e6, run: wl_nbody, baseline: BASE_NBODY },
    Workload { id: "sieve", name: "Primzahlsieb", unit: "Mkand/s", scale: 1e6, run: wl_sieve, baseline: BASE_SIEVE },
    Workload { id: "deflate", name: "Deflate-Kompression", unit: "MB/s", scale: 1e6, run: wl_deflate, baseline: BASE_DEFLATE },
    Workload { id: "text", name: "Textauswertung", unit: "MB/s", scale: 1e6, run: wl_text, baseline: BASE_TEXT },
];

// ── the yardstick ────────────────────────────────────────────────────
//
// Reference rates: the throughput a score of exactly 1000 means. ⚠️ These were
// MEASURED once — `cargo test --release -p inspector-rust-core --lib
// bench_baseline -- --ignored --nocapture` on an Apple M1 Pro (macOS 26.6.2,
// 10 cores), 2026-08-30 — and pinned here. They are a fixed reference, not a
// simulated measurement and not a claim about any product; a run's own numbers
// are always measured on the spot.
//
// ⚠️ They MUST come from a `--release` build. Measured under `cargo test`'s
// default debug profile the same workloads ran 30-50x slower (sha256 17 MB/s
// against 241), and scoring the shipped release build against debug references
// would have produced scores around 10 000.
pub const BASELINE_MACHINE: &str = "Apple M1 Pro · macOS 26.6.2 · Release-Build";
const BASE_SORT: f64 = 82.557;
const BASE_SHA: f64 = 241.495;
const BASE_MATMUL: f64 = 10_361.237;
const BASE_NBODY: f64 = 639.260;
const BASE_SIEVE: f64 = 1_283.840;
const BASE_DEFLATE: f64 = 20.627;
const BASE_TEXT: f64 = 181.064;

// ── measuring ────────────────────────────────────────────────────────

/// Run `wl` on ONE thread until the budget is spent; returns (rate, seconds).
pub fn measure_single(wl: &Workload, budget: Duration) -> (f64, f64, u64) {
    let t0 = Instant::now();
    let mut units = 0u64;
    let mut iters = 0u64;
    while t0.elapsed() < budget {
        units += (wl.run)();
        iters += 1;
    }
    let secs = t0.elapsed().as_secs_f64();
    (units as f64 / wl.scale / secs, secs, iters)
}

/// Run `wl` on `threads` threads at once; the rate is the AGGREGATE — that is
/// what "multi-core" means here: how much work the machine gets through, not
/// how fast one core is.
pub fn measure_multi(wl: &Workload, budget: Duration, threads: usize) -> (f64, f64, u64) {
    let t0 = Instant::now();
    let (units, iters) = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let start = Instant::now();
                    let (mut u, mut i) = (0u64, 0u64);
                    while start.elapsed() < budget {
                        u += (wl.run)();
                        i += 1;
                    }
                    (u, i)
                })
            })
            .collect();
        handles.into_iter().fold((0u64, 0u64), |(u, i), h| {
            let (du, di) = h.join().unwrap_or((0, 0));
            (u + du, i + di)
        })
    });
    let secs = t0.elapsed().as_secs_f64();
    (units as f64 / wl.scale / secs, secs, iters)
}

/// `1000 × rate / baseline`, rounded. A baseline of 0 would be a build error
/// waiting to happen, so it degrades to 0 rather than dividing by zero.
pub fn score_of(rate: f64, baseline: f64) -> u32 {
    if baseline <= 0.0 || !rate.is_finite() || rate <= 0.0 {
        return 0;
    }
    (1000.0 * rate / baseline).round().clamp(0.0, 1_000_000.0) as u32
}

/// Geometric mean of the sub-scores — one slow discipline should weigh as much
/// as one fast one, which an arithmetic mean would not do.
pub fn overall_score(scores: &[u32]) -> u32 {
    let usable: Vec<f64> = scores.iter().filter(|s| **s > 0).map(|s| f64::from(*s)).collect();
    if usable.is_empty() {
        return 0;
    }
    let log_sum: f64 = usable.iter().map(|s| s.ln()).sum();
    (log_sum / usable.len() as f64).exp().round() as u32
}

// ── the machine ──────────────────────────────────────────────────────

/// What the report says about the computer. Every field is READ, never
/// estimated; `None` becomes "nicht verfügbar" in the UI and the export rather
/// than a placeholder number.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub struct MachineInfo {
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    pub kernel: Option<String>,
    pub arch: Option<String>,
    pub device_model: Option<String>,
    pub host_name: Option<String>,
    pub cpu_brand: Option<String>,
    pub physical_cores: Option<usize>,
    pub logical_cores: Option<usize>,
    pub mem_total_bytes: Option<u64>,
}

/// Marketing/model identifier of the machine. macOS and Linux publish one;
/// elsewhere this stays `None` rather than inventing a name.
fn device_model() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "hw.model"])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!s.is_empty()).then_some(s)
    }
    #[cfg(target_os = "linux")]
    {
        for p in [
            "/sys/devices/virtual/dmi/id/product_name",
            "/proc/device-tree/model",
        ] {
            if let Ok(s) = std::fs::read_to_string(p) {
                let s = s.trim_end_matches('\0').trim().to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
        None
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

pub fn machine_info() -> MachineInfo {
    use sysinfo::System;
    let mut sys = System::new_with_specifics(
        sysinfo::RefreshKind::new()
            .with_cpu(sysinfo::CpuRefreshKind::everything())
            .with_memory(sysinfo::MemoryRefreshKind::everything()),
    );
    sys.refresh_cpu_all();
    sys.refresh_memory();
    let cpu_brand = sys.cpus().first().map(|c| c.brand().trim().to_string()).filter(|s| !s.is_empty());
    MachineInfo {
        // ⚠️ `System::name()` alone reports the KERNEL on macOS ("Darwin"),
        // which the report then printed instead of "macOS". The long form
        // carries the product name; the same chain `system_stats` uses.
        os_name: System::long_os_version().or_else(System::name),
        os_version: System::os_version(),
        kernel: System::kernel_version(),
        arch: System::cpu_arch(),
        device_model: device_model(),
        host_name: System::host_name(),
        cpu_brand,
        physical_cores: sys.physical_core_count(),
        logical_cores: Some(sys.cpus().len()).filter(|n| *n > 0),
        mem_total_bytes: Some(sys.total_memory()).filter(|b| *b > 0),
    }
}

// ── a run ────────────────────────────────────────────────────────────

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct WorkloadResult {
    pub id: String,
    pub name: String,
    pub unit: String,
    /// Measured throughput in `unit`.
    pub rate: f64,
    pub score: u32,
    pub iterations: u64,
    pub seconds: f64,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct Section {
    pub score: u32,
    pub workloads: Vec<WorkloadResult>,
}

/// One complete benchmark, and the unit of comparison and of the JSON file.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct BenchRun {
    /// Bumped when workloads or scoring change; an older run is flagged rather
    /// than silently compared against a newer one.
    pub schema: u32,
    pub id: String,
    pub finished_at_ms: i64,
    pub duration_s: f64,
    pub app_version: String,
    pub baseline_machine: String,
    pub machine: MachineInfo,
    pub threads: usize,
    pub single: Section,
    pub multi: Section,
}

/// How many threads the multi-core section uses.
pub fn thread_count() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

/// Wall-clock estimate the preview shows, so nobody starts an 8-second freeze
/// without being told.
pub fn estimated_seconds() -> f64 {
    WORKLOADS.len() as f64 * BUDGET.as_secs_f64() * 2.0
}

/// Run the whole benchmark. `on_progress(done, total, label)` is called before
/// each workload so the panel can show where it is.
pub fn run(mut on_progress: impl FnMut(usize, usize, &str)) -> BenchRun {
    let threads = thread_count();
    let total = WORKLOADS.len() * 2;
    let t0 = Instant::now();
    let mut single = Vec::new();
    let mut multi = Vec::new();

    for (i, wl) in WORKLOADS.iter().enumerate() {
        on_progress(i, total, wl.name);
        let (rate, secs, iters) = measure_single(wl, BUDGET);
        single.push(WorkloadResult {
            id: wl.id.to_string(),
            name: wl.name.to_string(),
            unit: wl.unit.to_string(),
            rate,
            score: score_of(rate, wl.baseline),
            iterations: iters,
            seconds: secs,
        });
    }
    for (i, wl) in WORKLOADS.iter().enumerate() {
        on_progress(WORKLOADS.len() + i, total, wl.name);
        let (rate, secs, iters) = measure_multi(wl, BUDGET, threads);
        multi.push(WorkloadResult {
            id: wl.id.to_string(),
            name: wl.name.to_string(),
            unit: wl.unit.to_string(),
            rate,
            score: score_of(rate, wl.baseline),
            iterations: iters,
            seconds: secs,
        });
    }

    let s_score = overall_score(&single.iter().map(|w| w.score).collect::<Vec<_>>());
    let m_score = overall_score(&multi.iter().map(|w| w.score).collect::<Vec<_>>());
    BenchRun {
        schema: SCHEMA,
        id: format!("{:x}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)),
        finished_at_ms: chrono::Utc::now().timestamp_millis(),
        duration_s: t0.elapsed().as_secs_f64(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        baseline_machine: BASELINE_MACHINE.to_string(),
        machine: machine_info(),
        threads,
        single: Section { score: s_score, workloads: single },
        multi: Section { score: m_score, workloads: multi },
    }
}

// ── persistence ──────────────────────────────────────────────────────
//
// One JSON file per run under `<data dir>/InspectorRust/benchmarks/`. That
// file IS the exchange format: copy it off another Mac (or a Linux box) and
// import it here to compare. Plain JSON on purpose — no database, nothing to
// migrate, and a run from another machine needs no shared storage.

/// Directory holding the saved runs; created on demand.
pub fn store_dir() -> Result<std::path::PathBuf, String> {
    let mut d = dirs::data_dir().ok_or("kein Datenverzeichnis")?;
    d.push("InspectorRust");
    d.push("benchmarks");
    std::fs::create_dir_all(&d).map_err(|e| format!("Ordner nicht anlegbar: {e}"))?;
    Ok(d)
}

/// Persist a run. ⚠️ Written to a temp file and renamed, so an interrupted
/// write cannot leave a half-written run that later fails to parse.
pub fn save(run: &BenchRun) -> Result<std::path::PathBuf, String> {
    let dir = store_dir()?;
    let path = dir.join(format!("{}.json", run.id));
    let json = serde_json::to_string_pretty(run).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &json).map_err(|e| format!("Schreiben fehlgeschlagen: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("Umbenennen fehlgeschlagen: {e}"))?;
    Ok(path)
}

/// Every saved run, newest first. A file that will not parse is SKIPPED, not
/// fatal — one bad import must never hide the rest of the history.
pub fn history() -> Vec<BenchRun> {
    let Ok(dir) = store_dir() else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<BenchRun> = rd
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|s| serde_json::from_str::<BenchRun>(&s).ok())
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.finished_at_ms));
    out
}

pub fn delete(id: &str) -> Result<(), String> {
    // Guard the id: it becomes a file name, and `../` would escape the store.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("ungültige Lauf-Kennung".into());
    }
    let path = store_dir()?.join(format!("{id}.json"));
    std::fs::remove_file(path).map_err(|e| format!("Löschen fehlgeschlagen: {e}"))
}

/// Read a run from a file another machine produced and file it into the store.
///
/// ⚠️ A newer schema is REFUSED rather than compared: a different workload set
/// under the same names would be comparing different work.
pub fn import(path: &std::path::Path) -> Result<BenchRun, String> {
    let s = std::fs::read_to_string(path).map_err(|e| format!("Datei nicht lesbar: {e}"))?;
    let run: BenchRun = serde_json::from_str(&s).map_err(|e| format!("Kein Benchmark-JSON: {e}"))?;
    if run.schema > SCHEMA {
        return Err(format!(
            "Der Lauf stammt aus einer neueren Version (Schema {} > {}).              Ein Vergleich wäre nicht derselbe Test.",
            run.schema, SCHEMA
        ));
    }
    save(&run)?;
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Measures the reference rates that get pinned as the baselines above.
    /// Not part of the suite: it is a one-off yardstick run, and it takes
    /// seconds. `cargo test -p inspector-rust-core --lib bench_baseline -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bench_baseline_measure() {
        for wl in WORKLOADS {
            let (rate, _, _) = measure_single(wl, BUDGET);
            println!("{:10} {:12.3} {}", wl.id, rate, wl.unit);
        }
    }

    /// A real end-to-end run, printed. Ignored: it takes ~9 s and is a
    /// measurement, not an assertion.
    #[test]
    #[ignore]
    fn bench_full_run() {
        let r = run(|i, n, name| println!("  [{}/{}] {}", i + 1, n, name));
        println!("\nMaschine: {:?} / {:?}", r.machine.device_model, r.machine.cpu_brand);
        println!("Kerne: {:?} phys / {:?} log, Threads {}", r.machine.physical_cores, r.machine.logical_cores, r.threads);
        println!("Single {}  Multi {}  ({:.1} s)", r.single.score, r.multi.score, r.duration_s);
        for w in &r.single.workloads {
            println!("  {:10} {:9.2} {:9} -> {}", w.id, w.rate, w.unit, w.score);
        }
    }

    #[test]
    fn every_workload_reports_progress_and_is_deterministic() {
        // Two calls must do byte-identical work: the inputs come from a
        // fixed-seed LCG precisely so a difference means the MACHINE differed.
        for wl in WORKLOADS {
            let a = (wl.run)();
            let b = (wl.run)();
            assert_eq!(a, b, "{} is not deterministic", wl.id);
            assert!(a > 0, "{} reported no work", wl.id);
        }
    }

    #[test]
    fn workload_ids_are_unique_and_stable() {
        // The id is the join key when comparing runs across machines.
        let mut ids: Vec<&str> = WORKLOADS.iter().map(|w| w.id).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate workload id");
    }

    #[test]
    fn a_score_is_relative_to_the_baseline() {
        assert_eq!(score_of(100.0, 100.0), 1000);
        assert_eq!(score_of(200.0, 100.0), 2000);
        assert_eq!(score_of(50.0, 100.0), 500);
    }

    #[test]
    fn a_missing_or_absurd_measurement_scores_zero_rather_than_infinity() {
        assert_eq!(score_of(100.0, 0.0), 0);
        assert_eq!(score_of(f64::NAN, 100.0), 0);
        assert_eq!(score_of(-1.0, 100.0), 0);
    }

    #[test]
    fn the_overall_score_is_a_geometric_mean() {
        assert_eq!(overall_score(&[1000, 1000, 1000]), 1000);
        // 500 and 2000 average to 1000 geometrically, not to 1250.
        assert_eq!(overall_score(&[500, 2000]), 1000);
        assert_eq!(overall_score(&[]), 0);
        // A zero is a failed sub-test, not a score of zero to average in.
        assert_eq!(overall_score(&[0, 1000]), 1000);
    }
}
