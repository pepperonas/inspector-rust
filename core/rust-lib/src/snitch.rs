//! `snitch` — a lightweight Little-Snitch-style network monitor + best-effort
//! per-app outbound blocker (macOS).
//!
//! **Honest scope.** A *real* per-app outbound firewall (intercept every
//! connection, allow/deny per app) needs a NetworkExtension **system
//! extension** with the `com.apple.developer.networking.networkextension`
//! entitlement — obtainable only with a Developer-ID + Apple approval, which a
//! self-signed tray app cannot have. So blocking here is **best-effort**: a
//! privileged watcher (`snitch-blockd`) polls each blocked app's live
//! connections (via `lsof`) and pushes their remote IPs into a **pf** block
//! table. New connections leak their first packets before the next poll adds
//! them, and blocking only holds while the watcher runs — this is documented
//! to the user, never presented as a hard firewall.
//!
//! **Two features, one command family:**
//! - `snitch` → the app list + per-app allow/block toggles (this blocker).
//! - `snitch map` → the live connections plotted on a world map (read-only;
//!   geo-location is resolved online by the frontend).
//!
//! The pure parsers (`parse_lsof`, `apps_from_connections`) are unit-tested;
//! the pf/daemon plumbing needs root + a live machine.

#![cfg(target_os = "macos")]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One live network connection of a process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub pid: i32,
    /// The `lsof` COMMAND (truncated process name).
    pub command: String,
    pub proto: String, // "TCP" | "UDP"
    pub remote_ip: String,
    pub remote_port: u16,
    /// True for IPv6 endpoints.
    pub v6: bool,
}

/// A process/app grouped with its connections (for the app list + block UI).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConnections {
    /// Grouping key — the process command (stable enough for the blocker: the
    /// watcher matches blocked entries against the same `lsof` COMMAND).
    pub key: String,
    pub command: String,
    pub pids: Vec<i32>,
    pub connection_count: usize,
    /// Distinct remote endpoints (deduped ip:port).
    pub remotes: Vec<String>,
    pub blocked: bool,
}

/// Parse `lsof -nP -iTCP -sTCP:ESTABLISHED -iUDP` output into connections with a
/// resolvable remote endpoint (skips listening / `*:*` UDP rows). Pure.
pub fn parse_lsof(output: &str) -> Vec<Connection> {
    let mut out = Vec::new();
    for line in output.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 9 {
            continue;
        }
        // COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME...
        if cols[0] == "COMMAND" {
            continue; // header
        }
        let command = cols[0].to_string();
        let Ok(pid) = cols[1].parse::<i32>() else { continue };
        let node = cols[7]; // "TCP" | "UDP"
        if node != "TCP" && node != "UDP" {
            continue;
        }
        // NAME's endpoint is a single token (col 8); a trailing state like
        // `(ESTABLISHED)` is a SEPARATE column, so join-ing would break the
        // port parse — take just the endpoint token.
        let name = cols[8];
        // We only want connections with a remote endpoint: "local->remote".
        let Some((_, remote)) = name.split_once("->") else { continue };
        let Some((ip, port, v6)) = split_host_port(remote) else { continue };
        if port == 0 {
            continue;
        }
        out.push(Connection {
            pid,
            command,
            proto: node.to_string(),
            remote_ip: ip,
            remote_port: port,
            v6,
        });
    }
    out
}

/// Split an lsof endpoint into (ip, port, is_v6). Handles `1.2.3.4:443` and
/// `[2600:1901::1]:443`. Returns `None` for wildcard (`*`) hosts/ports. Pure.
pub fn split_host_port(s: &str) -> Option<(String, u16, bool)> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('[') {
        // [v6]:port
        let (host, port) = rest.split_once("]:")?;
        if host.contains('*') {
            return None;
        }
        let port = port.parse::<u16>().ok()?;
        Some((host.to_string(), port, true))
    } else {
        let (host, port) = s.rsplit_once(':')?;
        if host.contains('*') || port.contains('*') {
            return None;
        }
        let port = port.parse::<u16>().ok()?;
        Some((host.to_string(), port, false))
    }
}

/// Group connections by process command, marking which are in `blocked`. Pure.
pub fn apps_from_connections(conns: &[Connection], blocked: &[String]) -> Vec<AppConnections> {
    let mut by_key: BTreeMap<String, AppConnections> = BTreeMap::new();
    for c in conns {
        let entry = by_key.entry(c.command.clone()).or_insert_with(|| AppConnections {
            key: c.command.clone(),
            command: c.command.clone(),
            pids: Vec::new(),
            connection_count: 0,
            remotes: Vec::new(),
            blocked: blocked.iter().any(|b| b == &c.command),
        });
        if !entry.pids.contains(&c.pid) {
            entry.pids.push(c.pid);
        }
        entry.connection_count += 1;
        let ep = format!("{}:{}", c.remote_ip, c.remote_port);
        if !entry.remotes.contains(&ep) {
            entry.remotes.push(ep);
        }
    }
    let mut v: Vec<AppConnections> = by_key.into_values().collect();
    // Blocked first, then busiest.
    v.sort_by(|a, b| {
        b.blocked
            .cmp(&a.blocked)
            .then_with(|| b.connection_count.cmp(&a.connection_count))
            .then_with(|| a.command.cmp(&b.command))
    });
    v
}

// ── Live capture (impure) ────────────────────────────────────────────────────

/// Run `lsof` and parse the current established/active connections.
pub fn live_connections() -> Vec<Connection> {
    let out = std::process::Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:ESTABLISHED", "-iUDP"])
        .output();
    match out {
        Ok(o) => parse_lsof(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => Vec::new(),
    }
}

// ── Blocked-set persistence ──────────────────────────────────────────────────

pub const KEY_BLOCKED: &str = "snitch.blocked";

/// The daemon reads the blocked set from a plain file in the data dir (no root
/// needed to write it), so the user can toggle apps without a fresh admin
/// prompt. This is that path.
pub fn blocklist_file() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("InspectorRust").join("snitch-blocklist.txt"))
}

pub fn load_blocked(db: &crate::db::DbHandle) -> Vec<String> {
    crate::settings::get(db, KEY_BLOCKED)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

/// Persist the blocked set to the settings table AND the daemon-readable file.
pub fn save_blocked(db: &crate::db::DbHandle, blocked: &[String]) {
    let _ = crate::settings::set(db, KEY_BLOCKED, &serde_json::to_string(blocked).unwrap_or_default());
    if let Some(path) = blocklist_file() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, blocked.join("\n"));
    }
}

// ── Geo-IP (online, batched, cached) ────────────────────────────────────────

/// A resolved server location for the world map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLocation {
    pub ip: String,
    pub lat: f64,
    pub lon: f64,
    pub country: String,
    pub city: String,
    pub isp: String,
}

/// Parse ip-api.com's `/batch` JSON response into locations (only `success`
/// rows with coordinates). Pure — the HTTP call is separate + cached.
pub fn parse_ipapi_batch(json: &str) -> Vec<GeoLocation> {
    let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for v in arr {
        if v.get("status").and_then(|s| s.as_str()) != Some("success") {
            continue;
        }
        let (Some(lat), Some(lon)) = (
            v.get("lat").and_then(|x| x.as_f64()),
            v.get("lon").and_then(|x| x.as_f64()),
        ) else {
            continue;
        };
        out.push(GeoLocation {
            ip: v.get("query").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            lat,
            lon,
            country: v.get("country").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            city: v.get("city").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            isp: v.get("isp").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        });
    }
    out
}

/// Whether an IP is private/loopback/link-local — never sent to the geo API
/// (no point, and it keeps LAN addresses off a third-party service). Pure.
pub fn is_private_ip(ip: &str) -> bool {
    if let Ok(v4) = ip.parse::<std::net::Ipv4Addr>() {
        return v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified();
    }
    if let Ok(v6) = ip.parse::<std::net::Ipv6Addr>() {
        let seg = v6.segments();
        return v6.is_loopback()
            || v6.is_unspecified()
            || (seg[0] & 0xfe00) == 0xfc00 // unique-local fc00::/7
            || (seg[0] & 0xffc0) == 0xfe80; // link-local fe80::/10
    }
    false
}

/// Resolve public IPs to locations via ip-api.com's free batch endpoint
/// (≤100/call, no key). Private IPs are filtered out first. Best-effort:
/// network failure → empty. The frontend caches, so this is called only for
/// not-yet-seen IPs.
pub fn geolocate(ips: &[String]) -> Vec<GeoLocation> {
    let public: Vec<&String> = ips.iter().filter(|ip| !is_private_ip(ip)).collect();
    if public.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for chunk in public.chunks(100) {
        let body = serde_json::to_string(
            &chunk.iter().map(|ip| serde_json::json!({"query": ip})).collect::<Vec<_>>(),
        )
        .unwrap_or_default();
        let resp = ureq::post("http://ip-api.com/batch?fields=status,country,city,lat,lon,isp,query")
            .timeout(std::time::Duration::from_secs(8))
            .send_string(&body);
        if let Ok(r) = resp {
            if let Ok(txt) = r.into_string() {
                out.extend(parse_ipapi_batch(&txt));
            }
        }
    }
    out
}

// ── Live activity (per-process throughput via nettop) ───────────────────────

/// Bytes/s a process is currently moving (both directions), for highlighting
/// connections that are actively transferring on the map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetActivity {
    pub pid: i32,
    pub bytes_per_sec: u64,
}

/// Parse two `nettop -P -x -n -l 2` samples into a per-PID byte delta (≈ 1 s
/// interval → bytes/s). Each data row is `<time> <name>.<pid> … <bytes_in>
/// <bytes_out> …`; the process name can contain spaces, so we locate the token
/// ending in `.<digits>` that is followed by two all-numeric columns and read
/// the pid + byte counters from there. First sighting of a pid = baseline,
/// second = current; delta is saturating (survives a counter reset). Pure.
pub fn parse_nettop_deltas(output: &str) -> Vec<NetActivity> {
    use std::collections::BTreeMap;
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    // pid → (baseline_total, Option<current_total>)
    let mut seen: BTreeMap<i32, (u64, Option<u64>)> = BTreeMap::new();
    for line in output.lines() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.len() < 4 {
            continue;
        }
        // Find the proc.pid token followed by bytes_in + bytes_out.
        for i in 1..toks.len().saturating_sub(2) {
            let t = toks[i];
            let Some((_, pidstr)) = t.rsplit_once('.') else { continue };
            if !all_digits(pidstr) || !all_digits(toks[i + 1]) || !all_digits(toks[i + 2]) {
                continue;
            }
            let Ok(pid) = pidstr.parse::<i32>() else { break };
            let total = toks[i + 1].parse::<u64>().unwrap_or(0)
                + toks[i + 2].parse::<u64>().unwrap_or(0);
            // Baseline = first sighting; current = the largest later total.
            let e = seen.entry(pid).or_insert((total, None));
            if total > e.0 {
                e.1 = Some(e.1.map_or(total, |c| c.max(total)));
            }
            break;
        }
    }
    seen.into_iter()
        .filter_map(|(pid, (base, cur))| {
            let cur = cur?;
            Some(NetActivity { pid, bytes_per_sec: cur.saturating_sub(base) })
        })
        .filter(|a| a.bytes_per_sec > 0)
        .collect()
}

/// Run nettop twice (≈1 s) and return per-PID throughput. Blocks ~1 s; call it
/// off the main thread (the IPC command is async). No root needed.
pub fn activity() -> Vec<NetActivity> {
    let out = std::process::Command::new("nettop")
        .args(["-P", "-x", "-n", "-l", "2"])
        .output();
    match out {
        Ok(o) => parse_nettop_deltas(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => Vec::new(),
    }
}

/// Best-effort geolocation of THIS machine's own public IP (for the map's
/// "home" origin that connection arcs radiate from). ip-api's `/json` with no
/// IP locates the caller. `None` on any failure → the map just omits arcs.
pub fn geolocate_self() -> Option<GeoLocation> {
    let r = ureq::get("http://ip-api.com/json?fields=status,country,city,lat,lon,isp,query")
        .timeout(std::time::Duration::from_secs(6))
        .call()
        .ok()?;
    let txt = r.into_string().ok()?;
    let v: serde_json::Value = serde_json::from_str(&txt).ok()?;
    if v.get("status").and_then(|s| s.as_str()) != Some("success") {
        return None;
    }
    Some(GeoLocation {
        ip: v.get("query").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        lat: v.get("lat").and_then(|x| x.as_f64())?,
        lon: v.get("lon").and_then(|x| x.as_f64())?,
        country: v.get("country").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        city: v.get("city").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        isp: v.get("isp").and_then(|x| x.as_str()).unwrap_or("").to_string(),
    })
}

// ── Best-effort per-app blocker (pf watcher daemon) ─────────────────────────

/// The embedded root watcher script (see its header for the honest-scope docs).
const BLOCKD_SCRIPT: &str = include_str!("../assets/snitch-blockd.sh");

fn data_dir() -> Option<std::path::PathBuf> {
    dirs::data_dir().map(|d| d.join("InspectorRust"))
}

fn blockd_paths() -> Option<(std::path::PathBuf, std::path::PathBuf, std::path::PathBuf, std::path::PathBuf)> {
    let d = data_dir()?;
    Some((
        d.join("snitch-blockd.sh"),
        d.join("snitch-blockd.pid"),
        d.join("snitch-blockd.stop"),
        d,
    ))
}

/// Is the blocker daemon running (pidfile → live process)?
pub fn is_armed() -> bool {
    let Some((_, pidfile, stopfile, _)) = blockd_paths() else { return false };
    if stopfile.exists() {
        return false;
    }
    let Ok(pid) = std::fs::read_to_string(&pidfile) else { return false };
    let Ok(pid) = pid.trim().parse::<i32>() else { return false };
    // Signal 0 = liveness probe (no new crate — one-line FFI, `libc` is a
    // Linux-only dep in this workspace).
    extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe { kill(pid, 0) == 0 }
}

/// Arm the blocker: materialise the script and launch it as **root** via one
/// admin prompt (`osascript ... with administrator privileges`). Idempotent —
/// a second arm while running is a no-op.
pub fn arm() -> Result<(), String> {
    if is_armed() {
        return Ok(());
    }
    let (script, _pid, stopfile, dir) = blockd_paths().ok_or("no data dir")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&script, BLOCKD_SCRIPT).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&stopfile);
    // Make it executable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755));
    }
    let script_s = script.to_string_lossy();
    let dir_s = dir.to_string_lossy();
    // Launch detached as root; nohup + & so it survives the osascript return.
    let shell = format!(
        "nohup '{script}' '{dir}' >/dev/null 2>&1 &",
        script = script_s,
        dir = dir_s
    );
    let osa = format!(
        "do shell script \"{}\" with administrator privileges",
        shell.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let out = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&osa)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.contains("-128") || err.contains("cancel") {
            "cancelled".into()
        } else {
            err
        })
    }
}

/// Disarm: drop the stop-file. The root daemon sees it next poll, flushes the
/// pf table, restores `/etc/pf.conf` from its backup and exits — **no second
/// admin prompt** (the daemon is already root). Best-effort.
pub fn disarm() -> Result<(), String> {
    let (_, _, stopfile, dir) = blockd_paths().ok_or("no data dir")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&stopfile, b"stop").map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nettop_delta_parsing() {
        // Two samples ~1 s apart; process names with spaces; a header line.
        let sample = "\
time                    interface state bytes_in bytes_out rx_dupe rx_ooo re-tx
11:15:05.335 Spotify.842                 2773532  75080     418     12545  1428
11:15:05.335 Google Chrome H.1137        56495424 2555245   24744   3444   177649
11:15:05.335 idle.999                    1000     2000      0       0      0
11:15:06.335 Spotify.842                 2774434  75091     418     12545  1428
11:15:06.335 Google Chrome H.1137        56499424 2555245   24744   3444   177649
11:15:06.335 idle.999                    1000     2000      0       0      0";
        let acts = parse_nettop_deltas(sample);
        // Spotify: (2774434+75091)-(2773532+75080) = 913
        let sp = acts.iter().find(|a| a.pid == 842).unwrap();
        assert_eq!(sp.bytes_per_sec, 913);
        // Chrome: (56499424)-(56495424) = 4000 (bytes_out unchanged)
        let ch = acts.iter().find(|a| a.pid == 1137).unwrap();
        assert_eq!(ch.bytes_per_sec, 4000);
        // idle process had zero delta → filtered out.
        assert!(acts.iter().all(|a| a.pid != 999));
    }

    #[test]
    fn private_ip_detection() {
        assert!(is_private_ip("192.168.1.5"));
        assert!(is_private_ip("10.0.0.1"));
        assert!(is_private_ip("127.0.0.1"));
        assert!(is_private_ip("169.254.1.1"));
        assert!(is_private_ip("fe80::1"));
        assert!(is_private_ip("fc00::1"));
        assert!(!is_private_ip("104.199.65.9"));
        assert!(!is_private_ip("2600:1901::1"));
        assert!(!is_private_ip("8.8.8.8"));
    }

    #[test]
    fn ipapi_batch_parsing_skips_failures() {
        let json = r#"[
          {"status":"success","country":"United States","city":"Mountain View","lat":37.4,"lon":-122.0,"isp":"Google","query":"8.8.8.8"},
          {"status":"fail","query":"10.0.0.1"},
          {"status":"success","country":"Germany","city":"Berlin","lat":52.52,"lon":13.4,"isp":"X","query":"1.2.3.4"}
        ]"#;
        let locs = parse_ipapi_batch(json);
        assert_eq!(locs.len(), 2);
        assert_eq!(locs[0].ip, "8.8.8.8");
        assert_eq!(locs[0].city, "Mountain View");
        assert_eq!(locs[1].country, "Germany");
    }

    #[test]
    fn split_host_port_v4_v6_and_wildcards() {
        assert_eq!(split_host_port("1.2.3.4:443"), Some(("1.2.3.4".into(), 443, false)));
        assert_eq!(
            split_host_port("[2600:1901:1:ca7::]:443"),
            Some(("2600:1901:1:ca7::".into(), 443, true))
        );
        assert_eq!(split_host_port("*:*"), None);
        assert_eq!(split_host_port("192.168.1.5:*"), None);
        assert_eq!(split_host_port("garbage"), None);
    }

    #[test]
    fn parse_lsof_extracts_remote_connections() {
        let sample = "\
COMMAND     PID   USER   FD   TYPE             DEVICE SIZE/OFF NODE NAME
Spotify     842 martin   30u  IPv4 0x00              0t0  TCP 192.168.178.25:61465->104.199.65.9:443 (ESTABLISHED)
cloudd      791 martin   10u  IPv6 0x00              0t0  UDP [2a02::bf3c]:54576->[2a01:b740::33]:443
identitys   790 martin    7u  IPv4 0x00              0t0  UDP *:*
mdns        123 martin    5u  IPv4 0x00              0t0  TCP *:5353 (LISTEN)";
        let conns = parse_lsof(sample);
        assert_eq!(conns.len(), 2);
        assert_eq!(conns[0].command, "Spotify");
        assert_eq!(conns[0].remote_ip, "104.199.65.9");
        assert_eq!(conns[0].remote_port, 443);
        assert!(!conns[0].v6);
        assert_eq!(conns[1].command, "cloudd");
        assert!(conns[1].v6);
        assert_eq!(conns[1].remote_ip, "2a01:b740::33");
    }

    #[test]
    fn apps_group_dedupe_and_sort_blocked_first() {
        let conns = vec![
            Connection { pid: 1, command: "A".into(), proto: "TCP".into(), remote_ip: "1.1.1.1".into(), remote_port: 443, v6: false },
            Connection { pid: 1, command: "A".into(), proto: "TCP".into(), remote_ip: "1.1.1.1".into(), remote_port: 443, v6: false },
            Connection { pid: 2, command: "A".into(), proto: "TCP".into(), remote_ip: "8.8.8.8".into(), remote_port: 53, v6: false },
            Connection { pid: 3, command: "B".into(), proto: "TCP".into(), remote_ip: "9.9.9.9".into(), remote_port: 443, v6: false },
        ];
        let apps = apps_from_connections(&conns, &["B".to_string()]);
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].command, "B"); // blocked floats up
        assert!(apps[0].blocked);
        let a = apps.iter().find(|x| x.command == "A").unwrap();
        assert_eq!(a.connection_count, 3);
        assert_eq!(a.pids, vec![1, 2]); // deduped
        assert_eq!(a.remotes.len(), 2); // 1.1.1.1:443 deduped, 8.8.8.8:53
    }
}
