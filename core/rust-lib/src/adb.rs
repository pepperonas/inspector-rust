//! `adb` — Android device control for the `adb` command (v0.119.0).
//!
//! A popup-sized companion to the maintainer's ADBOSS desktop app
//! (`~/claude/adboss`, PySide6): dashboard + quick controls + remote input +
//! screenshot/record + app manager light + WiFi-ADB, rendered in the preview
//! column. The heavyweight ADBOSS features (logcat viewer, Bluetooth HCI
//! analyzer, dual-pane file transfer, settings browser) deliberately stay in
//! ADBOSS — the popup's value is glance-and-grab in under two seconds.
//!
//! **Every device-facing command form is taken 1:1 from ADBOSS'
//! `core/adb_client.py`** — those invocations are battle-tested on real
//! devices (svc wifi/bluetooth, `media volume --stream N --set V --show`,
//! `settings put system screen_brightness` after disabling adaptive mode,
//! zen_mode for DND, the airplane-mode settings-put + broadcast pair,
//! `monkey -p <pkg> -c LAUNCHER 1` for launching, `exec-out screencap -p`,
//! device-side `screenrecord` + pull). The reference parsers in ADBOSS'
//! `utils/helpers.py` are ported as the pure cores below and unit-tested
//! against the same shapes.
//!
//! ⚠️ Shell-injection safety: `adb shell` takes ONE string, so every
//! interpolated value passes a strict validator first — `valid_package`
//! (`[A-Za-z0-9._]`), `valid_keycode` (`[A-Z0-9_]`), and text input goes
//! through `escape_input_text`, which additionally REJECTS non-ASCII with a
//! clear error (`input text` cannot deliver unicode reliably; an honest
//! refusal beats silently garbled characters on the device).
//!
//! House style: the subprocess shell is thin + untested; parsers, validators
//! and arg-builders are pure with a file-final test module. All IPC wrappers
//! are async + `spawn_blocking` (subprocess spawns; some adb calls block for
//! seconds on a sleeping WiFi device — `run` enforces a hard timeout via a
//! watchdog so a wedged adb can never hang a worker forever).

use serde::Serialize;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Sentinel the frontend maps to the "install adb" card.
pub const ERR_NO_ADB: &str = "adb.not_found";
/// Sentinel for text input containing characters `input text` can't deliver.
pub const ERR_TEXT_NOT_ASCII: &str = "adb.text_not_ascii";

// ── Locating adb ────────────────────────────────────────────────────────────

/// Probe PATH plus the usual install locations (Homebrew, Android SDK).
pub fn adb_path() -> Option<std::path::PathBuf> {
    if let Ok(path) = std::env::var("PATH") {
        if let Some(found) = std::env::split_paths(&path)
            .map(|dir| dir.join("adb"))
            .find(|cand| cand.is_file())
        {
            return Some(found);
        }
    }
    let home = dirs::home_dir().unwrap_or_default();
    [
        std::path::PathBuf::from("/opt/homebrew/bin/adb"),
        std::path::PathBuf::from("/usr/local/bin/adb"),
        home.join("Library/Android/sdk/platform-tools/adb"),
        home.join("Android/Sdk/platform-tools/adb"),
    ]
    .into_iter()
    .find(|cand| cand.is_file())
}

/// Run `adb <args>` with a hard timeout (watchdog kills a wedged adb — a
/// sleeping WiFi device can block `shell` for minutes otherwise). Returns
/// stdout; a non-zero exit yields Err with stderr (trimmed).
fn run(args: &[&str], timeout: Duration) -> Result<String, String> {
    let adb = adb_path().ok_or_else(|| ERR_NO_ADB.to_string())?;
    let mut child = Command::new(&adb)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("adb spawn: {e}"))?;

    // Watchdog: kill after `timeout`. Reading AFTER wait would deadlock on a
    // full pipe, so drain stdout/stderr on threads while waiting.
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let out_t = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = stdout.read_to_end(&mut s);
        s
    });
    let err_t = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = stderr.read_to_end(&mut s);
        s
    });
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("adb timed out after {}s", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            Err(e) => return Err(format!("adb wait: {e}")),
        }
    };
    let out = String::from_utf8_lossy(&out_t.join().unwrap_or_default()).into_owned();
    let err = String::from_utf8_lossy(&err_t.join().unwrap_or_default()).into_owned();
    if !status.success() && out.trim().is_empty() {
        return Err(if err.trim().is_empty() {
            format!("adb exited with {status}")
        } else {
            err.trim().to_string()
        });
    }
    Ok(out)
}

const T_FAST: Duration = Duration::from_secs(6);
const T_SLOW: Duration = Duration::from_secs(20);

/// `adb -s <serial> shell <command>`.
fn shell(serial: &str, command: &str, timeout: Duration) -> Result<String, String> {
    run(&["-s", serial, "shell", command], timeout)
}

// ── Validators (injection safety — pure, tested) ────────────────────────────

/// Android package names: letters, digits, dots, underscores. Anything else
/// (spaces, quotes, `;`, `$(`, …) is rejected before it can reach a shell.
pub fn valid_package(pkg: &str) -> bool {
    !pkg.is_empty()
        && pkg.len() <= 256
        && pkg.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}

/// Keycodes: `KEYCODE_HOME` style or bare numbers.
pub fn valid_keycode(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 32
        && code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Device serials as printed by `adb devices` (USB serials, `IP:port`).
pub fn valid_serial(serial: &str) -> bool {
    !serial.is_empty()
        && serial.len() <= 64
        && serial
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '_'))
}

/// `ip[:port]` for WiFi connect.
pub fn valid_ip_port(s: &str) -> bool {
    let (ip, port) = match s.split_once(':') {
        Some((i, p)) => (i, Some(p)),
        None => (s, None),
    };
    let ip_ok = ip.split('.').count() == 4
        && ip.split('.').all(|o| o.parse::<u8>().is_ok() && !o.is_empty());
    let port_ok = port.is_none_or(|p| p.parse::<u16>().is_ok_and(|n| n > 0));
    ip_ok && port_ok
}

/// Space → `%s` (the `input` convention, from ADBOSS) + POSIX-safe quote
/// handling (`'` → `'\''`, close-reopen — see the security note below; the
/// ADBOSS backslash form was an injection vector) — PLUS an ASCII gate:
/// `input text` cannot deliver non-ASCII reliably, so we refuse with a
/// sentinel instead of sending garbage to the device.
pub fn escape_input_text(text: &str) -> Result<String, String> {
    if text.is_empty() {
        return Err("empty text".into());
    }
    if !text.is_ascii() {
        return Err(ERR_TEXT_NOT_ASCII.to_string());
    }
    if text.chars().any(|c| c.is_ascii_control()) {
        return Err("control characters are not sendable".into());
    }
    // ⚠️ POSIX close-quote-reopen, NICHT Backslash-Escape: in Single-Quotes
    // gibt es kein \\' — der Quote beendet das Quoting IMMER. `it\\'s` erzeugte
    // ein unterminiertes Kommando (tippte den Apostroph gar nicht), und
    // `x'$(id)'` haette $(id) AUSSERHALB der Quotes auf der Geraete-Shell
    // ausgefuehrt (adb reicht den String an /system/bin/sh). Mit '\\'' bleibt
    // ALLES andere durch das umschliessende '...' literal — nur der Quote
    // selbst braucht Behandlung. (Sicherheitsreview 2026-08-24.)
    // %s bleibt die `input`-Konvention fuers Leerzeichen (Geraete-Semantik,
    // nichts mit Shell-Sicherheit zu tun).
    Ok(text.replace(' ', "%s").replace('\'', "'\\''"))
}

// ── Devices ─────────────────────────────────────────────────────────────────

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct AdbDevice {
    pub serial: String,
    /// `device` | `unauthorized` | `offline` (verbatim from adb).
    pub state: String,
    pub model: String,
    /// True when the serial is `ip:port` (a WiFi-ADB connection).
    pub wifi: bool,
}

/// Port of ADBOSS `parse_devices_output` (+ the wifi flag).
pub fn parse_devices(out: &str) -> Vec<AdbDevice> {
    let mut devices = Vec::new();
    for line in out.trim().lines().skip(1) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('*') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(serial) = parts.next() else { continue };
        let Some(state) = parts.next() else { continue };
        let mut model = String::new();
        for p in parts {
            if let Some((k, v)) = p.split_once(':') {
                if k == "model" {
                    model = v.replace('_', " ");
                }
            }
        }
        devices.push(AdbDevice {
            serial: serial.to_string(),
            state: state.to_string(),
            model: if model.is_empty() { "unknown".into() } else { model },
            wifi: serial.contains(':'),
        });
    }
    devices
}

pub fn list_devices() -> Result<Vec<AdbDevice>, String> {
    Ok(parse_devices(&run(&["devices", "-l"], T_FAST)?))
}

// ── Dashboard ───────────────────────────────────────────────────────────────

#[derive(Serialize, Clone, Debug, Default)]
pub struct AdbDashboard {
    pub model: String,
    pub manufacturer: String,
    pub android_version: String,
    pub sdk: String,
    pub build_id: String,
    pub uptime_secs: u64,
    pub battery_level: Option<i64>,
    pub battery_status: String,
    pub battery_health: String,
    pub battery_temp_c: Option<f64>,
    pub battery_voltage_mv: Option<i64>,
    pub mem_total_kb: u64,
    pub mem_used_kb: u64,
    pub storage_total_kb: u64,
    pub storage_used_kb: u64,
    pub wifi_ssid: String,
    pub ip: String,
    pub rssi_dbm: Option<i64>,
    pub resolution: String,
    pub dpi: String,
    pub brightness: Option<i64>,
    pub volume_media: Option<i64>,
}

/// Battery fields from `dumpsys battery` — ADBOSS' status/health maps,
/// temperature is tenths of °C on the wire.
pub fn parse_battery(out: &str) -> (Option<i64>, String, String, Option<f64>, Option<i64>) {
    let (mut level, mut status, mut health, mut temp, mut volt) =
        (None, String::new(), String::new(), None, None);
    for line in out.lines() {
        let line = line.trim();
        let Some((k, v)) = line.split_once(':') else { continue };
        let v = v.trim();
        match k.trim() {
            "level" => level = v.parse().ok(),
            "status" => {
                status = match v {
                    "2" => "Lädt".into(),
                    "3" => "Entlädt".into(),
                    "4" => "Lädt nicht".into(),
                    "5" => "Voll".into(),
                    other => other.into(),
                }
            }
            "health" => {
                health = match v {
                    "2" => "Gut".into(),
                    "3" => "Überhitzt".into(),
                    "4" => "Defekt".into(),
                    "5" => "Überspannung".into(),
                    other => other.into(),
                }
            }
            "temperature" => temp = v.parse::<f64>().ok().map(|t| t / 10.0),
            "voltage" => volt = v.parse().ok(),
            _ => {}
        }
    }
    (level, status, health, temp, volt)
}

/// `MemTotal`/`MemAvailable` in kB (ADBOSS `parse_meminfo`).
pub fn parse_meminfo(out: &str) -> (u64, u64) {
    let grab = |prefix: &str| -> u64 {
        out.lines()
            .find(|l| l.starts_with(prefix))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    };
    let total = grab("MemTotal:");
    let avail = grab("MemAvailable:").max(grab("MemFree:"));
    (total, total.saturating_sub(avail))
}

/// `/data` row of `df` → (total_kb, used_kb) (ADBOSS `parse_df_output`).
pub fn parse_df(out: &str) -> (u64, u64) {
    for line in out.trim().lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 && (parts[0] == "/data" || parts.last() == Some(&"/data")) {
            let total = parts[1].parse().unwrap_or(0);
            let used = parts[2].parse().unwrap_or(0);
            return (total, used);
        }
    }
    (0, 0)
}

/// `wm size` → "1080x2400" (physical; override wins when present, matching
/// what the user actually sees).
pub fn parse_wm_size(out: &str) -> String {
    let pick = |label: &str| {
        out.lines()
            .find(|l| l.contains(label))
            .and_then(|l| l.split(':').nth(1))
            .map(|s| s.trim().to_string())
    };
    pick("Override size").or_else(|| pick("Physical size")).unwrap_or_default()
}

/// SSID / IP / RSSI (ADBOSS `parse_network_info`, regex-free port).
pub fn parse_network(wifi_out: &str, ip_out: &str) -> (String, String, Option<i64>) {
    let mut ssid = String::new();
    for line in wifi_out.lines() {
        if let Some(i) = line.find("SSID: ") {
            let raw = line[i + 6..].split(',').next().unwrap_or("").trim();
            let cleaned = raw.trim_matches('"');
            if !cleaned.is_empty() && cleaned != "<unknown ssid>" {
                ssid = cleaned.to_string();
                break;
            }
        }
    }
    let mut ip = String::new();
    for line in ip_out.lines() {
        let line = line.trim();
        if line.starts_with("inet ") {
            if let Some(addr) = line.split_whitespace().nth(1) {
                ip = addr.split('/').next().unwrap_or("").to_string();
                break;
            }
        }
    }
    let mut rssi = None;
    for line in wifi_out.lines() {
        for marker in ["RSSI: ", "rssi=", "mRssi="] {
            if let Some(i) = line.find(marker) {
                let tail = &line[i + marker.len()..];
                let num: String = tail
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '-')
                    .collect();
                if let Ok(v) = num.parse::<i64>() {
                    if v < 0 {
                        rssi = Some(v);
                    }
                }
            }
        }
        if rssi.is_some() {
            break;
        }
    }
    (ssid, ip, rssi)
}

/// Gather the full dashboard (one IPC → ~10 fast shell calls; each tolerant —
/// a missing field renders as absent, never fails the whole panel).
pub fn dashboard(serial: &str) -> Result<AdbDashboard, String> {
    if !valid_serial(serial) {
        return Err("invalid serial".into());
    }
    let getprop = |p: &str| shell(serial, &format!("getprop {p}"), T_FAST).unwrap_or_default().trim().to_string();
    let mut d = AdbDashboard {
        model: getprop("ro.product.model"),
        manufacturer: getprop("ro.product.manufacturer"),
        android_version: getprop("ro.build.version.release"),
        sdk: getprop("ro.build.version.sdk"),
        build_id: getprop("ro.build.display.id"),
        ..Default::default()
    };
    if let Ok(up) = shell(serial, "cat /proc/uptime", T_FAST) {
        d.uptime_secs = up
            .split_whitespace()
            .next()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|s| s as u64)
            .unwrap_or(0);
    }
    if let Ok(b) = shell(serial, "dumpsys battery", T_FAST) {
        let (level, status, health, temp, volt) = parse_battery(&b);
        d.battery_level = level;
        d.battery_status = status;
        d.battery_health = health;
        d.battery_temp_c = temp;
        d.battery_voltage_mv = volt;
    }
    if let Ok(m) = shell(serial, "cat /proc/meminfo", T_FAST) {
        (d.mem_total_kb, d.mem_used_kb) = parse_meminfo(&m);
    }
    if let Ok(df) = shell(serial, "df /data", T_FAST) {
        (d.storage_total_kb, d.storage_used_kb) = parse_df(&df);
    }
    if let Ok(s) = shell(serial, "wm size", T_FAST) {
        d.resolution = parse_wm_size(&s);
    }
    if let Ok(dens) = shell(serial, "wm density", T_FAST) {
        d.dpi = dens
            .lines()
            .last()
            .and_then(|l| l.split(':').nth(1))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
    }
    let wifi_out = shell(serial, "dumpsys wifi | grep -m 3 'mWifiInfo\\|SSID'", T_FAST)
        .unwrap_or_default();
    let ip_out = shell(serial, "ip addr show wlan0", T_FAST).unwrap_or_default();
    (d.wifi_ssid, d.ip, d.rssi_dbm) = parse_network(&wifi_out, &ip_out);
    d.brightness = shell(serial, "settings get system screen_brightness", T_FAST)
        .ok()
        .and_then(|s| s.trim().parse().ok());
    d.volume_media = shell(serial, "media volume --stream 3 --get", T_FAST)
        .ok()
        .and_then(|s| parse_volume_get(&s));
    Ok(d)
}

/// `media volume --get` prints "volume is N in range [0..M]".
pub fn parse_volume_get(out: &str) -> Option<i64> {
    let i = out.find("volume is ")?;
    let tail = &out[i + 10..];
    let num: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
    num.parse().ok()
}

// ── Controls (command forms 1:1 from ADBOSS) ────────────────────────────────

/// A control mutation. The enum keeps the IPC surface at ONE command and the
/// shell strings in ONE audited place.
pub fn set_control(serial: &str, what: &str, value: i64) -> Result<(), String> {
    if !valid_serial(serial) {
        return Err("invalid serial".into());
    }
    let cmds: Vec<String> = control_commands(what, value)?;
    for c in cmds {
        shell(serial, &c, T_FAST)?;
    }
    Ok(())
}

/// Pure: control name + value → the shell command(s) to run (tested — this IS
/// the ADBOSS command inventory, pinned so it can't drift).
pub fn control_commands(what: &str, value: i64) -> Result<Vec<String>, String> {
    let on = value != 0;
    Ok(match what {
        "brightness" => {
            let v = value.clamp(0, 255);
            vec![
                "settings put system screen_brightness_mode 0".into(),
                format!("settings put system screen_brightness {v}"),
            ]
        }
        "volume_media" => vec![format!("media volume --stream 3 --set {} --show", value.clamp(0, 15))],
        "volume_ring" => vec![format!("media volume --stream 2 --set {} --show", value.clamp(0, 15))],
        "volume_alarm" => vec![format!("media volume --stream 4 --set {} --show", value.clamp(0, 15))],
        "wifi" => vec![format!("svc wifi {}", if on { "enable" } else { "disable" })],
        "bluetooth" => vec![format!("svc bluetooth {}", if on { "enable" } else { "disable" })],
        "airplane" => vec![
            format!("settings put global airplane_mode_on {}", i64::from(on)),
            format!(
                "am broadcast -a android.intent.action.AIRPLANE_MODE --ez state {}",
                if on { "true" } else { "false" }
            ),
        ],
        "dnd" => vec![format!("settings put global zen_mode {}", if on { 2 } else { 0 })],
        "screen_wake" => vec!["input keyevent KEYCODE_WAKEUP".into()],
        "screen_sleep" => vec!["input keyevent KEYCODE_SLEEP".into()],
        "screen_lock" => vec!["input keyevent KEYCODE_POWER".into()],
        other => return Err(format!("unknown control: {other}")),
    })
}

// ── Remote input ────────────────────────────────────────────────────────────

pub fn press_key(serial: &str, keycode: &str) -> Result<(), String> {
    if !valid_serial(serial) || !valid_keycode(keycode) {
        return Err("invalid key".into());
    }
    shell(serial, &format!("input keyevent {keycode}"), T_FAST).map(|_| ())
}

pub fn input_text(serial: &str, text: &str) -> Result<(), String> {
    if !valid_serial(serial) {
        return Err("invalid serial".into());
    }
    let escaped = escape_input_text(text)?;
    shell(serial, &format!("input text '{escaped}'"), T_FAST).map(|_| ())
}

pub fn tap(serial: &str, x: i64, y: i64) -> Result<(), String> {
    if !valid_serial(serial) || !(0..=9999).contains(&x) || !(0..=9999).contains(&y) {
        return Err("invalid coordinates".into());
    }
    shell(serial, &format!("input tap {x} {y}"), T_FAST).map(|_| ())
}

pub fn swipe(serial: &str, x1: i64, y1: i64, x2: i64, y2: i64, dur_ms: i64) -> Result<(), String> {
    let ok = [x1, y1, x2, y2].iter().all(|v| (0..=9999).contains(v)) && (50..=5000).contains(&dur_ms);
    if !valid_serial(serial) || !ok {
        return Err("invalid coordinates".into());
    }
    shell(serial, &format!("input swipe {x1} {y1} {x2} {y2} {dur_ms}"), T_FAST).map(|_| ())
}

// ── Screenshot / screenrecord ───────────────────────────────────────────────

/// `adb exec-out screencap -p` → raw PNG bytes.
pub fn screenshot_png(serial: &str) -> Result<Vec<u8>, String> {
    if !valid_serial(serial) {
        return Err("invalid serial".into());
    }
    let adb = adb_path().ok_or_else(|| ERR_NO_ADB.to_string())?;
    let out = Command::new(&adb)
        .args(["-s", serial, "exec-out", "screencap", "-p"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("adb screencap: {e}"))?;
    if !out.status.success() || out.stdout.len() < 8 {
        return Err("screencap failed (Bildschirm gesperrt?)".into());
    }
    if &out.stdout[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("screencap returned no PNG".into());
    }
    Ok(out.stdout)
}

/// Device-side recording path (fixed name; validated ours on stop/pull).
pub const RECORD_REMOTE: &str = "/sdcard/ir-record.mp4";

/// Recording session state (managed by Tauri). The local `adb shell
/// screenrecord` child is the recording — killing it stops the device-side
/// recorder (adb forwards the hangup), the ADBOSS model.
#[derive(Default)]
pub struct AdbRecordState {
    pub child: parking_lot::Mutex<Option<(std::process::Child, String)>>,
}

pub fn record_start(state: &AdbRecordState, serial: &str) -> Result<(), String> {
    if !valid_serial(serial) {
        return Err("invalid serial".into());
    }
    let mut guard = state.child.lock();
    if guard.is_some() {
        return Err("already recording".into());
    }
    let adb = adb_path().ok_or_else(|| ERR_NO_ADB.to_string())?;
    let child = Command::new(&adb)
        .args(["-s", serial, "shell", "screenrecord", RECORD_REMOTE])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("screenrecord: {e}"))?;
    *guard = Some((child, serial.to_string()));
    Ok(())
}

/// Stop, pull to `~/Downloads/android-record-<ts>.mp4`, clean up on-device.
pub fn record_stop(state: &AdbRecordState) -> Result<std::path::PathBuf, String> {
    let (mut child, serial) = state
        .child
        .lock()
        .take()
        .ok_or_else(|| "not recording".to_string())?;
    // Ask the device-side recorder to finish first (SIGINT via pkill — the
    // ADBOSS fallback), then hang up our channel.
    let _ = shell(&serial, "pkill -2 screenrecord", T_FAST);
    std::thread::sleep(Duration::from_millis(700)); // let it finalise the moov atom
    let _ = child.kill();
    let _ = child.wait();
    let downloads = dirs::download_dir().ok_or_else(|| "no Downloads dir".to_string())?;
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let local = downloads.join(format!("android-record-{ts}.mp4"));
    run(
        &["-s", &serial, "pull", RECORD_REMOTE, &local.to_string_lossy()],
        Duration::from_secs(120),
    )?;
    let _ = shell(&serial, &format!("rm {RECORD_REMOTE}"), T_FAST);
    Ok(local)
}

pub fn record_active(state: &AdbRecordState) -> bool {
    state.child.lock().is_some()
}

// ── App manager (light) ─────────────────────────────────────────────────────

/// `pm list packages [-3]` → sorted names (ADBOSS `parse_packages`).
pub fn list_packages(serial: &str, include_system: bool) -> Result<Vec<String>, String> {
    if !valid_serial(serial) {
        return Err("invalid serial".into());
    }
    let cmd = if include_system { "pm list packages" } else { "pm list packages -3" };
    let out = shell(serial, cmd, T_SLOW)?;
    let mut pkgs: Vec<String> = out
        .lines()
        .filter_map(|l| l.trim().strip_prefix("package:"))
        .map(|s| s.to_string())
        .collect();
    pkgs.sort();
    Ok(pkgs)
}

/// Pure: app action → shell command (the audited ADBOSS forms). `uninstall`
/// is NOT a shell command (top-level `adb uninstall`) — handled in `app_action`.
pub fn app_action_command(action: &str, package: &str) -> Result<Option<String>, String> {
    if !valid_package(package) {
        return Err("invalid package name".into());
    }
    Ok(match action {
        "launch" => Some(format!("monkey -p {package} -c android.intent.category.LAUNCHER 1")),
        "stop" => Some(format!("am force-stop {package}")),
        "clear" => Some(format!("pm clear {package}")),
        "disable" => Some(format!("pm disable-user --user 0 {package}")),
        "enable" => Some(format!("pm enable {package}")),
        "uninstall" => None,
        other => return Err(format!("unknown app action: {other}")),
    })
}

pub fn app_action(serial: &str, action: &str, package: &str) -> Result<String, String> {
    if !valid_serial(serial) {
        return Err("invalid serial".into());
    }
    match app_action_command(action, package)? {
        Some(cmd) => shell(serial, &cmd, T_SLOW).map(|o| o.trim().to_string()),
        None => run(&["-s", serial, "uninstall", package], T_SLOW).map(|o| o.trim().to_string()),
    }
}

// ── WiFi-ADB ────────────────────────────────────────────────────────────────

/// Switch a USB device to TCP/IP mode (ADBOSS `enable_tcpip`); returns the
/// device's WLAN IP for the connect step (best-effort).
pub fn wifi_enable_tcpip(serial: &str, port: u16) -> Result<String, String> {
    if !valid_serial(serial) {
        return Err("invalid serial".into());
    }
    // Grab the IP BEFORE tcpip — the adbd restart can drop the connection.
    let ip_out = shell(serial, "ip addr show wlan0", T_FAST).unwrap_or_default();
    let (_, ip, _) = parse_network("", &ip_out);
    run(&["-s", serial, "tcpip", &port.to_string()], Duration::from_secs(10))?;
    Ok(ip)
}

pub fn wifi_connect(ip_port: &str) -> Result<String, String> {
    if !valid_ip_port(ip_port) {
        return Err("invalid ip:port".into());
    }
    let target = if ip_port.contains(':') {
        ip_port.to_string()
    } else {
        format!("{ip_port}:5555")
    };
    let out = run(&["connect", &target], Duration::from_secs(10))?;
    if out.contains("connected") {
        Ok(out.trim().to_string())
    } else {
        Err(out.trim().to_string())
    }
}

pub fn wifi_disconnect(serial: &str) -> Result<(), String> {
    if !valid_serial(serial) {
        return Err("invalid serial".into());
    }
    run(&["disconnect", serial], T_FAST).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Validators — the injection gate ──────────────────────────────

    #[test]
    fn validators_reject_shell_metacharacters() {
        assert!(valid_package("com.example.app_2"));
        for evil in ["com.a; rm -rf /", "a$(reboot)", "a b", "a'b", "", "a|b"] {
            assert!(!valid_package(evil), "{evil:?} must be rejected");
        }
        assert!(valid_keycode("KEYCODE_HOME"));
        assert!(valid_keycode("26"));
        for evil in ["KEYCODE_HOME; reboot", "home", "", "KEY CODE"] {
            assert!(!valid_keycode(evil), "{evil:?} must be rejected");
        }
        assert!(valid_serial("R5CT30XXXX"));
        assert!(valid_serial("192.168.178.42:5555"));
        for evil in ["a b", "x;y", "", "🙂"] {
            assert!(!valid_serial(evil), "{evil:?} must be rejected");
        }
    }

    #[test]
    fn ip_port_validation() {
        assert!(valid_ip_port("192.168.178.42"));
        assert!(valid_ip_port("192.168.178.42:5555"));
        for bad in ["192.168.178", "192.168.178.256", "host:5555", "1.2.3.4:0", "1.2.3.4:abc", ""] {
            assert!(!valid_ip_port(bad), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn input_text_escaping_is_the_adboss_form_and_ascii_gated() {
        assert_eq!(escape_input_text("hello world").unwrap(), "hello%sworld");
        assert_eq!(escape_input_text("it's").unwrap(), "it'\\''s");
        // Non-ASCII → the sentinel, never garbled bytes on the device.
        assert_eq!(escape_input_text("grüße").unwrap_err(), ERR_TEXT_NOT_ASCII);
        assert!(escape_input_text("").is_err());
        assert!(escape_input_text("a\tb").is_err()); // control chars
    }

    /// Minimaler POSIX-sh-Parser fuer eine Single-Quote-gequotete Zeichenkette:
    /// gibt das Wort zurueck, das die Geraete-Shell aus `'<escaped>'` bilden
    /// wuerde — oder None, sobald ein GEFAEHRLICHES Metazeichen UNGEQUOTED
    /// auftaucht. Genau das fordert der Sicherheitsreview: beweisen, dass
    /// $(id) niemals ausserhalb der Quotes landen kann.
    #[cfg(test)]
    fn sh_unquote(s: &str) -> Option<String> {
        let mut out = String::new();
        let mut it = s.chars().peekable();
        let mut in_single = false;
        while let Some(c) = it.next() {
            if in_single {
                if c == '\'' { in_single = false; } else { out.push(c); }
            } else {
                match c {
                    '\'' => in_single = true,
                    '\\' => out.push(it.next()?),           // \x -> literal x
                    // ungequotet gefaehrlich -> die Shell wuerde es deuten:
                    '$' | '`' | ';' | '|' | '&' | '<' | '>' | '(' | ')'
                    | '*' | '?' | '[' | ']' | '{' | '}' | '#' | '~' | '\n' => return None,
                    other => out.push(other),
                }
            }
        }
        if in_single { None } else { Some(out) }
    }

    #[test]
    fn input_text_cannot_break_out_of_the_device_shell_quotes() {
        // Der genau vom Review verlangte Fall + weitere Injektionsversuche.
        for angriff in [
            "x'$(id)'", "'; reboot; '", "a`whoami`b", "$(rm -rf /)",
            "end' && echo pwned && '", "pipe | cat", "it's a \"test\"",
        ] {
            let escaped = escape_input_text(angriff).unwrap();
            let wrapped = format!("'{escaped}'");        // wie in input_text()
            // Die Geraete-Shell bildet daraus GENAU den Originaltext — mit
            // Leerzeichen als %s (input-Konvention), nie ausgefuehrte Substitution.
            let erwartet = angriff.replace(' ', "%s");
            assert_eq!(sh_unquote(&wrapped), Some(erwartet),
                       "Ausbruch aus den Quotes bei: {angriff:?}");
        }
    }

    #[test]
    fn sh_unquote_flags_an_unquoted_metachar() {
        // Gegenprobe: der Parser erkennt einen ECHTEN Ausbruch (sonst
        // waere der Test oben blind). Das ist der alte, kaputte Backslash-Stil.
        let kaputt = format!("'{}'", "x\'$(id)\'");    // altes escape_input_text
        assert_eq!(sh_unquote(&kaputt), None);
    }

    // ── Parsers (shapes from ADBOSS' helpers/tests) ──────────────────

    #[test]
    fn parses_devices_with_states_models_and_wifi_transport() {
        let out = "List of devices attached\n\
                   R5CT30ABCDE\tdevice usb:339738624X product:a54xeea model:SM_A546B device:a54x transport_id:1\n\
                   192.168.178.42:5555\tdevice product:a54xeea model:SM_A546B device:a54x transport_id:2\n\
                   0123456789\tunauthorized transport_id:3\n\
                   * daemon started successfully *\n";
        let d = parse_devices(out);
        assert_eq!(d.len(), 3);
        assert_eq!(d[0].serial, "R5CT30ABCDE");
        assert_eq!(d[0].model, "SM A546B");
        assert!(!d[0].wifi);
        assert!(d[1].wifi);
        assert_eq!(d[2].state, "unauthorized");
        assert_eq!(d[2].model, "unknown");
    }

    #[test]
    fn parses_battery_with_status_health_maps_and_tenth_degrees() {
        let out = "Current Battery Service state:\n  AC powered: false\n  USB powered: true\n  \
                   status: 2\n  health: 2\n  level: 85\n  voltage: 4123\n  temperature: 273\n";
        let (level, status, health, temp, volt) = parse_battery(out);
        assert_eq!(level, Some(85));
        assert_eq!(status, "Lädt");
        assert_eq!(health, "Gut");
        assert_eq!(temp, Some(27.3)); // wire format is tenths of °C
        assert_eq!(volt, Some(4123));
    }

    #[test]
    fn parses_meminfo_df_wm_and_volume() {
        let mem = "MemTotal:        7994052 kB\nMemFree:          234560 kB\nMemAvailable:    3456789 kB\n";
        assert_eq!(parse_meminfo(mem), (7994052, 7994052 - 3456789));
        let df = "Filesystem      1K-blocks     Used Available Use% Mounted on\n\
                  /dev/block/dm-52 115740656 98123456  17617200  85% /data\n";
        assert_eq!(parse_df(df), (115740656, 98123456));
        assert_eq!(parse_wm_size("Physical size: 1080x2340\n"), "1080x2340");
        // An override (e.g. changed via wm size) is what the user SEES.
        assert_eq!(
            parse_wm_size("Physical size: 1080x2340\nOverride size: 720x1560\n"),
            "720x1560"
        );
        assert_eq!(parse_volume_get("volume is 9 in range [0..15]\n"), Some(9));
        assert_eq!(parse_volume_get("garbage"), None);
    }

    #[test]
    fn parses_network_ssid_ip_rssi() {
        let wifi = "mWifiInfo SSID: \"elliptic-curve\", BSSID: aa:bb, RSSI: -52, ...\n";
        let ip = "36: wlan0: <BROADCAST,MULTICAST,UP>\n    inet 192.168.178.42/24 brd ...\n";
        let (ssid, addr, rssi) = parse_network(wifi, ip);
        assert_eq!(ssid, "elliptic-curve");
        assert_eq!(addr, "192.168.178.42");
        assert_eq!(rssi, Some(-52));
        // Unknown SSID marker is not a name.
        let (ssid2, _, _) = parse_network("SSID: <unknown ssid>, RSSI: -50", "");
        assert_eq!(ssid2, "");
    }

    // ── The ADBOSS command inventory, pinned ─────────────────────────

    #[test]
    fn control_commands_are_the_battle_tested_adboss_forms() {
        assert_eq!(
            control_commands("brightness", 128).unwrap(),
            vec![
                "settings put system screen_brightness_mode 0".to_string(),
                "settings put system screen_brightness 128".to_string(),
            ]
        );
        assert_eq!(
            control_commands("volume_media", 9).unwrap(),
            vec!["media volume --stream 3 --set 9 --show".to_string()]
        );
        assert_eq!(control_commands("wifi", 1).unwrap(), vec!["svc wifi enable".to_string()]);
        assert_eq!(control_commands("bluetooth", 0).unwrap(), vec!["svc bluetooth disable".to_string()]);
        // Airplane mode needs the settings-put AND the broadcast (ADBOSS pair).
        let air = control_commands("airplane", 1).unwrap();
        assert_eq!(air[0], "settings put global airplane_mode_on 1");
        assert!(air[1].starts_with("am broadcast -a android.intent.action.AIRPLANE_MODE"));
        assert_eq!(control_commands("dnd", 1).unwrap(), vec!["settings put global zen_mode 2".to_string()]);
        assert_eq!(control_commands("screen_wake", 0).unwrap(), vec!["input keyevent KEYCODE_WAKEUP".to_string()]);
        assert!(control_commands("nonsense", 1).is_err());
        // Ranges clamp instead of leaking wild values into shell strings.
        assert_eq!(
            control_commands("brightness", 9999).unwrap()[1],
            "settings put system screen_brightness 255"
        );
    }

    #[test]
    fn app_actions_are_the_adboss_forms_and_uninstall_is_top_level() {
        assert_eq!(
            app_action_command("launch", "com.spotify.music").unwrap().unwrap(),
            "monkey -p com.spotify.music -c android.intent.category.LAUNCHER 1"
        );
        assert_eq!(
            app_action_command("stop", "com.spotify.music").unwrap().unwrap(),
            "am force-stop com.spotify.music"
        );
        assert_eq!(
            app_action_command("clear", "com.spotify.music").unwrap().unwrap(),
            "pm clear com.spotify.music"
        );
        // Uninstall is `adb uninstall`, not a shell string → None marks it.
        assert_eq!(app_action_command("uninstall", "com.spotify.music").unwrap(), None);
        assert!(app_action_command("launch", "com.a; reboot").is_err());
        assert!(app_action_command("explode", "com.a").is_err());
    }
}
