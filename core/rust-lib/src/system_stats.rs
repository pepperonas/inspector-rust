//! Live system statistics for the `stats` search-bar command — CPU / memory /
//! disks / network / temperatures / fans / battery & power draw, gathered once
//! per poll and rendered read-only in the right preview column (`StatsPanel`).
//!
//! ## Data sources (best-effort, graceful per OS)
//!
//! - **CPU / memory / swap / disks / network / uptime / load / host info** come
//!   from [`sysinfo`] (already a workspace dep) and are reliable on macOS,
//!   Windows and Linux. CPU usage needs two samples spaced by
//!   `MINIMUM_CPU_UPDATE_INTERVAL`, so [`gather`] refreshes, sleeps ~200 ms, and
//!   refreshes again — the same window is reused to compute the network rate.
//! - **Temperatures** come from `sysinfo::Components` (hwmon on Linux, SMC on
//!   Intel macOS, WMI on Windows). On Apple-Silicon macOS `Components` is empty,
//!   so the macOS SMC reader below supplies a CPU temperature by averaging the
//!   per-core thermal sensors it discovers.
//! - **Fans** are not exposed by `sysinfo` on any OS, so they're read directly:
//!   macOS via the SMC (`F<n>Ac` keys), Linux via `/sys/class/hwmon/*/fan*_input`.
//!   Windows has no rootless fan API → none.
//! - **Battery & power draw** come from the `starship-battery` crate (`battery`),
//!   cross-platform via IOKit / WMI / sysfs. `energy_rate` is the instantaneous
//!   power in watts — the system power draw while on battery.
//!
//! Every source degrades to an empty/`None` value on failure; `gather` never
//! panics and always returns the `sysinfo`-backed core.

use serde::Serialize;

#[derive(Serialize, Default)]
pub struct SystemStats {
    // ── Host
    pub host_name: Option<String>,
    pub os_name: Option<String>,
    pub kernel: Option<String>,
    pub cpu_arch: Option<String>,
    pub uptime_secs: u64,
    // ── CPU
    pub cpu_brand: String,
    pub cpu_usage: f32,
    pub cpu_freq_mhz: u64,
    pub physical_cores: Option<usize>,
    pub logical_cores: usize,
    pub per_core: Vec<f32>,
    /// `[1m, 5m, 15m]` — Unix only (empty on Windows).
    pub load_avg: Option<[f64; 3]>,
    // ── Memory (bytes)
    pub mem_total: u64,
    pub mem_used: u64,
    pub mem_available: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    // ── Disks
    pub disks: Vec<DiskStat>,
    // ── Network (bytes per second, averaged over the sample window)
    pub net_rx_per_sec: u64,
    pub net_tx_per_sec: u64,
    // ── Sensors
    pub temps: Vec<TempStat>,
    pub fans: Vec<FanStat>,
    // ── Battery & power
    pub battery: Option<BatteryStat>,
}

#[derive(Serialize)]
pub struct DiskStat {
    pub name: String,
    pub mount: String,
    pub fs: String,
    pub total: u64,
    pub available: u64,
    pub removable: bool,
    pub kind: String,
}

#[derive(Serialize)]
pub struct TempStat {
    pub label: String,
    pub celsius: f32,
}

#[derive(Serialize)]
pub struct FanStat {
    pub label: String,
    pub rpm: u32,
}

#[derive(Serialize)]
pub struct BatteryStat {
    pub percent: f32,
    pub state: String,
    /// Instantaneous power in watts (discharge while on battery). `None` if the
    /// platform doesn't report it.
    pub power_watts: Option<f32>,
    pub time_to_empty_secs: Option<u64>,
    pub time_to_full_secs: Option<u64>,
    pub health_percent: Option<f32>,
    pub cycle_count: Option<u32>,
    pub temperature_c: Option<f32>,
    pub vendor: Option<String>,
    pub model: Option<String>,
}

/// Gather one snapshot of system stats. Blocks ~200 ms (the CPU/network sample
/// window); callers run it off the main thread (Tauri command worker).
pub fn gather() -> SystemStats {
    use sysinfo::{Disks, MemoryRefreshKind, Networks, RefreshKind, System};

    let mut sys = System::new_with_specifics(
        RefreshKind::new()
            .with_cpu(sysinfo::CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::everything()),
    );
    let mut networks = Networks::new_with_refreshed_list();

    // First sample, then wait the minimum CPU interval, then re-sample. The same
    // window doubles as the network-rate window.
    sys.refresh_cpu_all();
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    sys.refresh_cpu_all();
    sys.refresh_memory();
    networks.refresh();

    let window = sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.as_secs_f64().max(0.05);
    let (mut rx, mut tx) = (0u64, 0u64);
    for (_iface, data) in networks.iter() {
        rx += data.received();
        tx += data.transmitted();
    }
    let net_rx_per_sec = (rx as f64 / window) as u64;
    let net_tx_per_sec = (tx as f64 / window) as u64;

    let cpus = sys.cpus();
    let cpu_brand = cpus
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "CPU".to_string());
    let cpu_freq_mhz = cpus.first().map(|c| c.frequency()).unwrap_or(0);
    let per_core: Vec<f32> = cpus.iter().map(|c| c.cpu_usage()).collect();

    let disks = Disks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|d| DiskStat {
            name: d.name().to_string_lossy().to_string(),
            mount: d.mount_point().to_string_lossy().to_string(),
            fs: d.file_system().to_string_lossy().to_string(),
            total: d.total_space(),
            available: d.available_space(),
            removable: d.is_removable(),
            kind: match d.kind() {
                sysinfo::DiskKind::HDD => "HDD".to_string(),
                sysinfo::DiskKind::SSD => "SSD".to_string(),
                sysinfo::DiskKind::Unknown(_) => "Disk".to_string(),
            },
        })
        // Drop pseudo / zero-capacity mounts (devfs, etc.) that just clutter.
        .filter(|d| d.total > 0)
        .collect();

    let load = System::load_average();
    let load_avg = if cfg!(target_os = "windows") {
        None // Windows load_average() returns zeros.
    } else {
        Some([load.one, load.five, load.fifteen])
    };

    let mut temps = summarize_temps(&component_temps());
    let fans = read_fans();
    supplement_cpu_temp(&mut temps);

    SystemStats {
        host_name: System::host_name(),
        os_name: System::long_os_version().or_else(System::name),
        kernel: System::kernel_version(),
        cpu_arch: System::cpu_arch(),
        uptime_secs: System::uptime(),
        cpu_brand,
        cpu_usage: sys.global_cpu_usage(),
        cpu_freq_mhz,
        physical_cores: sys.physical_core_count(),
        logical_cores: cpus.len(),
        per_core,
        load_avg,
        mem_total: sys.total_memory(),
        mem_used: sys.used_memory(),
        mem_available: sys.available_memory(),
        swap_total: sys.total_swap(),
        swap_used: sys.used_swap(),
        disks,
        net_rx_per_sec,
        net_tx_per_sec,
        temps,
        fans,
        battery: read_battery(),
    }
}

/// Temperatures via `sysinfo::Components` (cross-platform: hwmon / SMC / WMI).
fn component_temps() -> Vec<TempStat> {
    let comps = sysinfo::Components::new_with_refreshed_list();
    comps
        .list()
        .iter()
        .filter_map(|c| {
            let t = c.temperature();
            if t.is_finite() && t > 0.0 && t < 150.0 {
                Some(TempStat {
                    label: c.label().to_string(),
                    celsius: t,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Collapse the raw (often dozens of cryptic, duplicated) `Components` sensors
/// into a few clean, named rows — CPU / GPU / Battery / SSD — averaging each
/// bucket. Pure; unit-tested. Cross-platform label heuristics: Apple-Silicon
/// `PMU tdie*`, Linux `Core N` / `Package` / `Tctl`, Intel `TC0P` → CPU; etc.
fn summarize_temps(raw: &[TempStat]) -> Vec<TempStat> {
    let (mut cpu, mut gpu, mut batt, mut ssd) = (vec![], vec![], vec![], vec![]);
    for t in raw {
        let l = t.label.to_lowercase();
        if l.contains("battery") || l.contains("gas gauge") {
            batt.push(t.celsius);
        } else if l.contains("nand") || l.contains("nvme") || l.contains("ssd") {
            ssd.push(t.celsius);
        } else if l.contains("gpu") || l.contains("tg0") {
            gpu.push(t.celsius);
        } else if l.contains("tdie")
            || l.contains("cpu")
            || l.contains("core")
            || l.contains("package")
            || l.contains("tctl") // AMD CPU control temp
            || l.contains("tccd") // AMD CCD
            || l.contains("tc0") // Intel Mac SMC CPU proximity (TC0P/TC0D)
        {
            cpu.push(t.celsius);
        }
    }
    let round1 = |xs: &[f32]| (xs.iter().sum::<f32>() / xs.len() as f32 * 10.0).round() / 10.0;
    let mut out = Vec::new();
    for (label, bucket) in [
        ("CPU", &cpu),
        ("GPU", &gpu),
        ("Battery", &batt),
        ("SSD", &ssd),
    ] {
        if !bucket.is_empty() {
            out.push(TempStat {
                label: label.to_string(),
                celsius: round1(bucket),
            });
        }
    }
    out
}

/// If summarising produced no CPU temperature, fall back to the SMC per-core
/// average (macOS). No-op on non-macOS / when a CPU temp already exists.
fn supplement_cpu_temp(temps: &mut Vec<TempStat>) {
    #[cfg(target_os = "macos")]
    {
        if !temps.iter().any(|t| t.label == "CPU") {
            if let Some(c) = macos_smc::cpu_temperature() {
                temps.insert(
                    0,
                    TempStat {
                        label: "CPU".to_string(),
                        celsius: (c * 10.0).round() / 10.0,
                    },
                );
            }
        }
    }
    let _ = temps;
}

/// Fans — macOS SMC, Linux hwmon; none elsewhere.
fn read_fans() -> Vec<FanStat> {
    #[cfg(target_os = "macos")]
    {
        return macos_smc::fans();
    }
    #[cfg(target_os = "linux")]
    {
        return linux_hwmon::fans();
    }
    #[allow(unreachable_code)]
    Vec::new()
}

/// Battery & power draw via the cross-platform `battery` (starship-battery) crate.
fn read_battery() -> Option<BatteryStat> {
    use battery::units::power::watt;
    use battery::units::ratio::percent;
    use battery::units::thermodynamic_temperature::degree_celsius;
    use battery::units::time::second;

    let manager = battery::Manager::new().ok()?;
    let bat = manager.batteries().ok()?.next()?.ok()?;

    let power = bat.energy_rate().get::<watt>();
    let health = bat.state_of_health().get::<percent>();
    let temp = bat.temperature().map(|t| t.get::<degree_celsius>());

    Some(BatteryStat {
        percent: bat.state_of_charge().get::<percent>(),
        state: match bat.state() {
            battery::State::Charging => "Charging",
            battery::State::Discharging => "Discharging",
            battery::State::Empty => "Empty",
            battery::State::Full => "Full",
            _ => "Unknown",
        }
        .to_string(),
        power_watts: if power.is_finite() && power > 0.0 {
            Some(power)
        } else {
            None
        },
        time_to_empty_secs: bat.time_to_empty().map(|t| t.get::<second>() as u64),
        time_to_full_secs: bat.time_to_full().map(|t| t.get::<second>() as u64),
        health_percent: if health.is_finite() { Some(health) } else { None },
        cycle_count: bat.cycle_count(),
        temperature_c: temp,
        vendor: bat.vendor().map(|s| s.trim().to_string()),
        model: bat.model().map(|s| s.trim().to_string()),
    })
}

// ───────────────────────────── Linux: hwmon fans ─────────────────────────────

#[cfg(target_os = "linux")]
mod linux_hwmon {
    use super::FanStat;

    /// Read `/sys/class/hwmon/hwmon*/fan*_input` (RPM). Labels prefer the
    /// sibling `fan*_label`, then the chip `name`, then `Fan N`.
    pub fn fans() -> Vec<FanStat> {
        let mut out = Vec::new();
        let Ok(dirs) = std::fs::read_dir("/sys/class/hwmon") else {
            return out;
        };
        for hwmon in dirs.flatten() {
            let base = hwmon.path();
            let chip = std::fs::read_to_string(base.join("name"))
                .ok()
                .map(|s| s.trim().to_string());
            let Ok(entries) = std::fs::read_dir(&base) else {
                continue;
            };
            let mut files: Vec<String> = entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.starts_with("fan") && n.ends_with("_input"))
                .collect();
            files.sort();
            for f in files {
                let Ok(raw) = std::fs::read_to_string(base.join(&f)) else {
                    continue;
                };
                let Ok(rpm) = raw.trim().parse::<u32>() else {
                    continue;
                };
                if rpm == 0 {
                    continue;
                }
                let n = f.trim_start_matches("fan").trim_end_matches("_input");
                let label = std::fs::read_to_string(base.join(format!("fan{n}_label")))
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .or_else(|| chip.clone())
                    .unwrap_or_else(|| format!("Fan {n}"));
                out.push(FanStat { label, rpm });
            }
        }
        out
    }
}

// ───────────────────────────── macOS: SMC reader ─────────────────────────────
//
// Minimal IOKit SMC client (no external crate). Reads `F<n>Ac` fan-speed keys
// and the per-core thermal sensors, decoding the SMC `flt`/`fpe2`/`sp78`/`ui*`
// value formats. Connection is opened per call and closed; the relevant key
// list is enumerated once and cached. All FFI is wrapped so any failure yields
// empty results — never a panic.

#[cfg(target_os = "macos")]
mod macos_smc {
    use super::FanStat;
    use std::ffi::c_void;
    use std::os::raw::c_char;
    use std::sync::OnceLock;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct SmcVers {
        major: u8,
        minor: u8,
        build: u8,
        reserved: u8,
        release: u16,
    }
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct SmcPLimit {
        version: u16,
        length: u16,
        cpu_plimit: u32,
        gpu_plimit: u32,
        mem_plimit: u32,
    }
    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct SmcKeyInfo {
        data_size: u32,
        data_type: u32,
        data_attributes: u8,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SmcKeyData {
        key: u32,
        vers: SmcVers,
        p_limit: SmcPLimit,
        key_info: SmcKeyInfo,
        result: u8,
        status: u8,
        data8: u8,
        data32: u32,
        bytes: [u8; 32],
    }
    impl Default for SmcKeyData {
        fn default() -> Self {
            // SAFETY: an all-zero `SmcKeyData` is a valid POD value.
            unsafe { std::mem::zeroed() }
        }
    }

    const KERNEL_INDEX_SMC: u32 = 2;
    const SMC_CMD_READ_BYTES: u8 = 5;
    const SMC_CMD_READ_INDEX: u8 = 8;
    const SMC_CMD_READ_KEYINFO: u8 = 9;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOServiceMatching(name: *const c_char) -> *mut c_void;
        fn IOServiceGetMatchingService(master: u32, matching: *mut c_void) -> u32;
        fn IOServiceOpen(service: u32, owning_task: u32, typ: u32, conn: *mut u32) -> i32;
        fn IOServiceClose(conn: u32) -> i32;
        fn IOObjectRelease(obj: u32) -> i32;
        fn IOConnectCallStructMethod(
            conn: u32,
            selector: u32,
            input: *const c_void,
            input_size: usize,
            output: *mut c_void,
            output_size: *mut usize,
        ) -> i32;
    }
    extern "C" {
        static mach_task_self_: u32;
    }

    /// Open the AppleSMC user client. Returns the connection handle.
    fn open() -> Option<u32> {
        unsafe {
            let matching = IOServiceMatching(c"AppleSMC".as_ptr());
            if matching.is_null() {
                return None;
            }
            // IOServiceGetMatchingService consumes the matching dict reference.
            let service = IOServiceGetMatchingService(0, matching);
            if service == 0 {
                return None;
            }
            let mut conn: u32 = 0;
            let kr = IOServiceOpen(service, mach_task_self_, 0, &mut conn);
            IOObjectRelease(service);
            if kr != 0 || conn == 0 {
                None
            } else {
                Some(conn)
            }
        }
    }

    fn close(conn: u32) {
        unsafe {
            IOServiceClose(conn);
        }
    }

    fn call(conn: u32, input: &SmcKeyData) -> Option<SmcKeyData> {
        let mut output = SmcKeyData::default();
        let mut out_size = std::mem::size_of::<SmcKeyData>();
        let kr = unsafe {
            IOConnectCallStructMethod(
                conn,
                KERNEL_INDEX_SMC,
                input as *const _ as *const c_void,
                std::mem::size_of::<SmcKeyData>(),
                &mut output as *mut _ as *mut c_void,
                &mut out_size,
            )
        };
        if kr == 0 && output.result == 0 {
            Some(output)
        } else {
            None
        }
    }

    /// `(data_type, value_bytes)` for an SMC key, or `None` if absent.
    fn read_key(conn: u32, key: u32) -> Option<(u32, [u8; 32], usize)> {
        let mut info_in = SmcKeyData {
            key,
            data8: SMC_CMD_READ_KEYINFO,
            ..Default::default()
        };
        let info = call(conn, &info_in)?;
        let size = info.key_info.data_size as usize;
        if size == 0 || size > 32 {
            return None;
        }
        info_in = SmcKeyData {
            key,
            data8: SMC_CMD_READ_BYTES,
            ..Default::default()
        };
        info_in.key_info.data_size = info.key_info.data_size;
        let val = call(conn, &info_in)?;
        Some((info.key_info.data_type, val.bytes, size))
    }

    /// Number of keys the SMC exposes (`#KEY`).
    fn key_count(conn: u32) -> Option<u32> {
        let (typ, bytes, size) = read_key(conn, fourcc(b"#KEY"))?;
        decode(typ, &bytes[..size]).map(|v| v as u32)
    }

    /// The key at index `i` (`SMC_CMD_READ_INDEX`).
    fn key_at(conn: u32, i: u32) -> Option<u32> {
        let input = SmcKeyData {
            data8: SMC_CMD_READ_INDEX,
            data32: i,
            ..Default::default()
        };
        let out = call(conn, &input)?;
        if out.key == 0 {
            None
        } else {
            Some(out.key)
        }
    }

    /// Enumerate all keys once, keep the fan (`F<n>Ac`) and per-core thermal
    /// (`Tp*`/`Te*`/`Tf*`/`TC*`) keys we care about. Cached for the process.
    fn relevant_keys(conn: u32) -> &'static RelevantKeys {
        static CACHE: OnceLock<RelevantKeys> = OnceLock::new();
        CACHE.get_or_init(|| {
            let mut fans = Vec::new();
            let mut cpu_temps = Vec::new();
            if let Some(count) = key_count(conn) {
                for i in 0..count {
                    let Some(k) = key_at(conn, i) else { continue };
                    let s = fourcc_str(k);
                    let b = s.as_bytes();
                    // Fan actual-speed keys: F<digit>Ac
                    if b.len() == 4 && b[0] == b'F' && b[1].is_ascii_digit() && &b[2..] == b"Ac" {
                        fans.push(k);
                    }
                    // Apple-Silicon core temps (Tp/Te/Tf*) + Intel CPU proximity (TC*).
                    else if b.len() == 4
                        && b[0] == b'T'
                        && (b[1] == b'p' || b[1] == b'e' || b[1] == b'f' || b[1] == b'C')
                    {
                        cpu_temps.push(k);
                    }
                }
            }
            fans.sort_unstable();
            RelevantKeys { fans, cpu_temps }
        })
    }

    struct RelevantKeys {
        fans: Vec<u32>,
        cpu_temps: Vec<u32>,
    }

    pub fn fans() -> Vec<FanStat> {
        let Some(conn) = open() else {
            return Vec::new();
        };
        let keys = relevant_keys(conn);
        let mut out = Vec::new();
        for (idx, &k) in keys.fans.iter().enumerate() {
            if let Some((typ, bytes, size)) = read_key(conn, k) {
                if let Some(v) = decode(typ, &bytes[..size]) {
                    let rpm = v.round();
                    // Include idle (0 rpm) fans too — Apple's fan curve keeps
                    // them off until warm, and "Fan 1: 0 rpm" is informative.
                    if rpm.is_finite() && (0.0..20_000.0).contains(&rpm) {
                        out.push(FanStat {
                            label: format!("Fan {}", idx + 1),
                            rpm: rpm as u32,
                        });
                    }
                }
            }
        }
        close(conn);
        out
    }

    /// Average the in-range per-core thermal sensors → a single CPU temperature.
    pub fn cpu_temperature() -> Option<f32> {
        let conn = open()?;
        let keys = relevant_keys(conn);
        let mut sum = 0.0f32;
        let mut n = 0u32;
        for &k in &keys.cpu_temps {
            if let Some((typ, bytes, size)) = read_key(conn, k) {
                if let Some(v) = decode(typ, &bytes[..size]) {
                    if v.is_finite() && v > 5.0 && v < 120.0 {
                        sum += v;
                        n += 1;
                    }
                }
            }
        }
        close(conn);
        if n > 0 {
            Some(sum / n as f32)
        } else {
            None
        }
    }

    fn fourcc(s: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*s)
    }
    fn fourcc_str(k: u32) -> String {
        k.to_be_bytes().iter().map(|&c| c as char).collect()
    }

    /// Decode an SMC value (the type is a FourCC). Returns the numeric value
    /// (RPM for fans, °C for temps). Pure — unit-tested.
    pub(super) fn decode(data_type: u32, bytes: &[u8]) -> Option<f32> {
        const FLT: u32 = u32::from_be_bytes(*b"flt ");
        const FPE2: u32 = u32::from_be_bytes(*b"fpe2");
        const FP1F: u32 = u32::from_be_bytes(*b"fp1f");
        const SP78: u32 = u32::from_be_bytes(*b"sp78");
        const UI8: u32 = u32::from_be_bytes(*b"ui8 ");
        const UI16: u32 = u32::from_be_bytes(*b"ui16");
        const UI32: u32 = u32::from_be_bytes(*b"ui32");
        match data_type {
            FLT if bytes.len() >= 4 => {
                Some(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            }
            FPE2 if bytes.len() >= 2 => {
                Some((((bytes[0] as u16) << 8 | bytes[1] as u16) >> 2) as f32)
            }
            FP1F if bytes.len() >= 2 => {
                Some((((bytes[0] as u16) << 8 | bytes[1] as u16) >> 1) as f32 / 16384.0)
            }
            SP78 if bytes.len() >= 2 => {
                Some(i16::from_be_bytes([bytes[0], bytes[1]]) as f32 / 256.0)
            }
            UI8 if !bytes.is_empty() => Some(bytes[0] as f32),
            UI16 if bytes.len() >= 2 => Some(u16::from_be_bytes([bytes[0], bytes[1]]) as f32),
            UI32 if bytes.len() >= 4 => {
                Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod summarize_tests {
    use super::{summarize_temps, TempStat};

    fn t(label: &str, c: f32) -> TempStat {
        TempStat {
            label: label.to_string(),
            celsius: c,
        }
    }

    #[test]
    fn buckets_and_averages_apple_silicon_labels() {
        let raw = vec![
            t("PMU tdie0", 46.0),
            t("PMU tdie1", 48.0),
            t("gas gauge battery", 31.0),
            t("NAND CH0 temp", 39.0),
            t("PMU tdev4", 35.0), // unclassified → dropped
        ];
        let out = summarize_temps(&raw);
        // CPU = mean(46,48)=47.0; ordered CPU, Battery, SSD; tdev dropped.
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].label, "CPU");
        assert_eq!(out[0].celsius, 47.0);
        assert_eq!(out[1].label, "Battery");
        assert_eq!(out[2].label, "SSD");
    }

    #[test]
    fn linux_intel_cpu_labels_classify_as_cpu() {
        let raw = vec![t("Core 0", 50.0), t("Package id 0", 52.0), t("TC0P", 48.0)];
        let out = summarize_temps(&raw);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "CPU");
        assert_eq!(out[0].celsius, 50.0); // mean(50,52,48)
    }

    #[test]
    fn empty_input_yields_empty() {
        assert!(summarize_temps(&[]).is_empty());
    }
}

#[cfg(test)]
mod gather_smoke {
    #[test]
    #[ignore = "hits real hardware; run with --ignored --nocapture to inspect"]
    fn print_live_stats() {
        let s = super::gather();
        eprintln!(
            "cpu={:.1}% cores={} brand={:?} freq={}MHz",
            s.cpu_usage, s.logical_cores, s.cpu_brand, s.cpu_freq_mhz
        );
        eprintln!("mem {}/{} MB", s.mem_used / 1048576, s.mem_total / 1048576);
        eprintln!(
            "disks={} net rx={}/s tx={}/s load={:?}",
            s.disks.len(),
            s.net_rx_per_sec,
            s.net_tx_per_sec,
            s.load_avg
        );
        eprintln!(
            "temps={:?}",
            s.temps
                .iter()
                .map(|t| format!("{}:{:.1}", t.label, t.celsius))
                .collect::<Vec<_>>()
        );
        eprintln!(
            "fans={:?}",
            s.fans
                .iter()
                .map(|f| format!("{}:{}", f.label, f.rpm))
                .collect::<Vec<_>>()
        );
        eprintln!(
            "battery={:?}",
            s.battery.as_ref().map(|b| format!(
                "{:.0}% {} {:?}W health={:?} cyc={:?}",
                b.percent, b.state, b.power_watts, b.health_percent, b.cycle_count
            ))
        );
        assert!(s.logical_cores > 0);
    }
}

#[cfg(all(test, target_os = "macos"))]
mod smc_tests {
    use super::macos_smc::decode;

    #[test]
    fn decode_flt_little_endian() {
        // 1234.0_f32 as little-endian bytes.
        let b = 1234.0f32.to_le_bytes();
        assert_eq!(decode(u32::from_be_bytes(*b"flt "), &b), Some(1234.0));
    }

    #[test]
    fn decode_fpe2_fan_rpm() {
        // fpe2: ((b0<<8)|b1) >> 2. 0x0F_A0 = 4000 → >>2 = 1000.
        let b = [0x0F, 0xA0];
        assert_eq!(decode(u32::from_be_bytes(*b"fpe2"), &b), Some(1000.0));
    }

    #[test]
    fn decode_sp78_temperature() {
        // sp78: i16 big-endian / 256. 0x2D00 = 45 * 256 → 45.0 °C.
        let b = [0x2D, 0x00];
        assert_eq!(decode(u32::from_be_bytes(*b"sp78"), &b), Some(45.0));
    }

    #[test]
    fn decode_unknown_type_is_none() {
        assert_eq!(decode(u32::from_be_bytes(*b"ch8*"), &[1, 2, 3, 4]), None);
    }

    #[test]
    fn decode_ui_ints_big_endian() {
        assert_eq!(decode(u32::from_be_bytes(*b"ui8 "), &[42]), Some(42.0));
        assert_eq!(decode(u32::from_be_bytes(*b"ui16"), &[0x01, 0x00]), Some(256.0));
    }
}
