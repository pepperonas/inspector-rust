//! Logitech mouse settings over HID++ 2.0 — what Logi Options+ does for the
//! pointer speed, the scroll wheel and the buttons, without Logi Options+.
//!
//! * This file: the protocol as pure functions (message layout, feature
//!   decoders), the stored configuration and the button/gesture rules.
//! * `macos.rs`: the IOKit HID transport and the one thread that owns the
//!   device.
//!
//! Scope: a mouse connected **directly by Bluetooth** (vendor collection
//! `0xFF43/0x0202`, long reports only). Receivers (Unifying/Bolt) address
//! their mice by index on another collection and are not covered.
//!
//! Nothing here is stored on the mouse: DPI, wheel mode and button diversion
//! are volatile, so the configuration is applied again each time the mouse
//! connects.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::db::DbHandle;
use crate::gestures::bindings::{classify_open, parse_chord, BindingAction};

#[cfg(target_os = "macos")]
mod macos;

pub const SETTING: &str = "logi.config";
/// Emitted whenever the cached status changed (connect, battery, settings).
pub const EVENT: &str = "logi-changed";

// ── HID++ 2.0 wire format ────────────────────────────────────────────────────

pub const REPORT_LONG: u8 = 0x11;
pub const REPORT_LEN: usize = 20;
/// A Bluetooth mouse is addressed directly.
pub const DEVICE_DIRECT: u8 = 0xFF;
/// Our software id (low nibble of byte 3); device events carry 0.
pub const SWID: u8 = 0x0A;

pub const F_NAME: u16 = 0x0005;
pub const F_BATTERY: u16 = 0x1000;
pub const F_REPROG: u16 = 0x1b04;
pub const F_SMARTSHIFT: u16 = 0x2110;
pub const F_HIRES_WHEEL: u16 = 0x2121;
pub const F_DPI: u16 = 0x2201;

/// One request: `[0x11, device, feature index, function<<4 | swid, params…]`.
pub fn request(feature_index: u8, function: u8, params: &[u8]) -> [u8; REPORT_LEN] {
    let mut m = [0u8; REPORT_LEN];
    m[0] = REPORT_LONG;
    m[1] = DEVICE_DIRECT;
    m[2] = feature_index;
    m[3] = (function << 4) | SWID;
    let n = params.len().min(REPORT_LEN - 4);
    m[4..4 + n].copy_from_slice(&params[..n]);
    m
}

/// What an incoming report is, seen from a request that is waiting.
#[derive(Debug, PartialEq, Eq)]
pub enum Incoming<'a> {
    /// The answer to the request; the payload after the 4-byte header.
    Reply(&'a [u8]),
    /// The device refused the request (HID++ 2.0 error code).
    Error(u8),
    /// Something else: an event, or the answer to another program's request.
    Other,
}

pub fn classify_incoming<'a>(report: &'a [u8], feature_index: u8, function: u8) -> Incoming<'a> {
    if report.len() < 6 || report[0] != REPORT_LONG {
        return Incoming::Other;
    }
    let tag = (function << 4) | SWID;
    if report[2] == 0xFF && report[3] == feature_index && report[4] == tag {
        return Incoming::Error(report[5]);
    }
    if report[2] == feature_index && report[3] == tag {
        return Incoming::Reply(&report[4..]);
    }
    Incoming::Other
}

/// A device event: `(feature index, function, payload)`. Events carry
/// software id 0.
pub fn as_event(report: &[u8]) -> Option<(u8, u8, &[u8])> {
    if report.len() < 5 || report[0] != REPORT_LONG || report[2] == 0xFF || report[3] & 0x0F != 0 {
        return None;
    }
    Some((report[2], report[3] >> 4, &report[4..]))
}

pub fn error_text(code: u8) -> &'static str {
    match code {
        1 => "unknown",
        2 => "invalid argument",
        3 => "out of range",
        4 => "hardware error",
        5 => "not allowed",
        6 => "invalid feature index",
        7 => "invalid function",
        8 => "busy",
        9 => "unsupported",
        _ => "error",
    }
}

// ── Feature decoders ─────────────────────────────────────────────────────────

/// `getSensorDpiList` payload (after the sensor index): big-endian values,
/// ended by 0. A value ≥ 0xE000 is a step between its neighbours.
pub fn parse_dpi_list(payload: &[u8]) -> Vec<u16> {
    let mut raw = Vec::new();
    for pair in payload.as_chunks::<2>().0 {
        let v = u16::from_be_bytes(*pair);
        if v == 0 {
            break;
        }
        raw.push(v);
    }
    let mut out: Vec<u16> = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        let v = raw[i];
        if v >= 0xE000 {
            let step = v - 0xE000;
            if let (Some(&from), Some(&to)) = (out.last(), raw.get(i + 1)) {
                if step > 0 && to > from && to < 0xE000 {
                    let mut d = from + step;
                    while d < to && out.len() < 64 {
                        out.push(d);
                        d += step;
                    }
                }
            }
        } else {
            out.push(v);
        }
        i += 1;
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The option closest to `want` (ties go down).
pub fn nearest_dpi(options: &[u16], want: u16) -> Option<u16> {
    options.iter().copied().min_by_key(|o| ((*o as i32 - want as i32).abs(), *o))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

/// `0x1000 getBatteryLevelStatus`: `[level %, next level, status]`.
pub fn parse_battery(payload: &[u8]) -> Option<Battery> {
    let percent = *payload.first()?;
    if percent > 100 {
        return None;
    }
    let status = payload.get(2).copied().unwrap_or(0);
    Some(Battery { percent, charging: matches!(status, 1..=3) })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WheelMode {
    /// Always free-spinning.
    Free,
    /// Always line by line.
    Ratchet,
    /// Line by line, free-spinning when the wheel is flicked (SmartShift).
    Smart,
}

pub const SMART_MIN: u8 = 1;
pub const SMART_MAX: u8 = 50;
pub const SMART_DEFAULT: u8 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wheel {
    pub mode: WheelMode,
    /// Flick speed that releases the ratchet (lower = sooner); meaningful in
    /// `Smart`.
    pub threshold: u8,
}

/// `0x2110 getRatchetControlMode`: `[mode (1 free, 2 ratchet), auto-disengage,
/// default]`. Auto-disengage 0xFF (or 0) means the ratchet never lets go.
pub fn parse_smartshift(payload: &[u8]) -> Option<Wheel> {
    let (mode, auto) = (*payload.first()?, *payload.get(1)?);
    let default = payload.get(2).copied().filter(|d| (SMART_MIN..=SMART_MAX).contains(d)).unwrap_or(SMART_DEFAULT);
    Some(match mode {
        1 => Wheel { mode: WheelMode::Free, threshold: default },
        2 if (SMART_MIN..=SMART_MAX).contains(&auto) => Wheel { mode: WheelMode::Smart, threshold: auto },
        2 => Wheel { mode: WheelMode::Ratchet, threshold: default },
        _ => return None,
    })
}

/// The three bytes for `0x2110 setRatchetControlMode`.
pub fn smartshift_bytes(w: Wheel) -> [u8; 3] {
    match w.mode {
        WheelMode::Free => [1, 0, 0],
        WheelMode::Ratchet => [2, 0xFF, 0],
        WheelMode::Smart => [2, w.threshold.clamp(SMART_MIN, SMART_MAX), 0],
    }
}

/// `0x2121` wheel mode bit 2 = inverted direction.
pub const WHEEL_INVERT_BIT: u8 = 0x04;
/// `0x2121 getWheelCapability` flags bit 3 = the wheel can invert.
pub const WHEEL_CAP_INVERT: u8 = 0x08;

pub fn with_invert(mode: u8, invert: bool) -> u8 {
    if invert {
        mode | WHEEL_INVERT_BIT
    } else {
        mode & !WHEEL_INVERT_BIT
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Control {
    /// Control id (what the device reports when the button is diverted).
    pub cid: u16,
    /// Whether the button can be handed to us instead of doing its own thing.
    pub divertable: bool,
}

/// `0x1b04 getCidInfo`: `[cid, task id, flags, …]`; flags bit 5 = divertable.
pub fn parse_control(payload: &[u8]) -> Option<Control> {
    if payload.len() < 5 {
        return None;
    }
    let cid = u16::from_be_bytes([payload[0], payload[1]]);
    (cid != 0).then_some(Control { cid, divertable: payload[4] & 0x20 != 0 })
}

/// `0x1b04 setCidReporting` params: temporary diversion on or off.
pub fn divert_bytes(cid: u16, divert: bool) -> [u8; 5] {
    let [hi, lo] = cid.to_be_bytes();
    // bit 1 = "the divert bit is valid", bit 0 = divert.
    [hi, lo, if divert { 0x03 } else { 0x02 }, 0, 0]
}

/// `0x1b04` diverted-buttons event: up to four pressed control ids, 0-padded.
pub fn parse_pressed(payload: &[u8]) -> Vec<u16> {
    payload
        .as_chunks::<2>()
        .0
        .iter()
        .take(4)
        .map(|p| u16::from_be_bytes(*p))
        .filter(|c| *c != 0)
        .collect()
}

/// Known control ids → catalogue key suffix (`logi.btn.<name>`). The panel
/// names buttons from its own copy of this table; `logi.test.ts` reads this
/// one to keep the two equal.
#[cfg_attr(not(test), allow(dead_code))]
pub fn control_name(cid: u16) -> &'static str {
    match cid {
        0x50 => "left",
        0x51 => "right",
        0x52 => "middle",
        0x53 => "back",
        0x56 => "forward",
        0xC3 => "gesture",
        0xC4 => "wheel_mode",
        0xD7 => "host",
        _ => "other",
    }
}

// ── Configuration ────────────────────────────────────────────────────────────

/// What one button does once it is ours. A button with nothing set is left to
/// the mouse.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub press: Option<BindingAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up: Option<BindingAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub down: Option<BindingAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<BindingAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<BindingAction>,
}

impl ButtonConfig {
    pub fn is_empty(&self) -> bool {
        self.press.is_none() && !self.has_directions()
    }
    pub fn has_directions(&self) -> bool {
        self.up.is_some() || self.down.is_some() || self.left.is_some() || self.right.is_some()
    }
    fn actions_mut(&mut self) -> [&mut Option<BindingAction>; 5] {
        [&mut self.press, &mut self.up, &mut self.down, &mut self.left, &mut self.right]
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogiConfig {
    /// Pointer speed in DPI; `None` leaves the mouse as it is.
    #[serde(default)]
    pub dpi: Option<u16>,
    #[serde(default)]
    pub wheel: Option<Wheel>,
    /// Reverse the wheel on the mouse itself (independent of the trackpad).
    #[serde(default)]
    pub invert: Option<bool>,
    /// Control id (decimal) → what the button does.
    #[serde(default)]
    pub buttons: BTreeMap<String, ButtonConfig>,
}

impl LogiConfig {
    pub fn button(&self, cid: u16) -> Option<&ButtonConfig> {
        self.buttons.get(&cid.to_string()).filter(|b| !b.is_empty())
    }
}

fn check_action(a: &mut BindingAction) -> Result<(), String> {
    match a {
        BindingAction::Hotkey { action } => {
            *action = action.trim().to_string();
            if crate::hotkey::ActionId::from_key(action).is_none() {
                return Err(format!("unknown action “{action}”"));
            }
        }
        BindingAction::Shortcut { keys } => {
            *keys = keys.trim().to_string();
            parse_chord(keys)?;
        }
        BindingAction::Open { target } => {
            *target = target.trim().to_string();
            classify_open(target)?;
        }
        BindingAction::Task { id } if *id <= 0 => return Err("no task chosen".into()),
        _ => {}
    }
    Ok(())
}

/// Clean a configuration before it is saved: clamps, drops empty buttons and
/// ids that are not numbers, checks every action.
pub fn validate(mut cfg: LogiConfig) -> Result<LogiConfig, String> {
    if let Some(d) = cfg.dpi {
        if !(100..=32000).contains(&d) {
            return Err(format!("DPI {d} out of range"));
        }
    }
    if let Some(w) = cfg.wheel.as_mut() {
        w.threshold = w.threshold.clamp(SMART_MIN, SMART_MAX);
    }
    let mut buttons = BTreeMap::new();
    for (key, mut b) in std::mem::take(&mut cfg.buttons) {
        let Ok(cid) = key.trim().parse::<u16>() else { continue };
        for a in b.actions_mut().into_iter().flatten() {
            check_action(a)?;
        }
        if !b.is_empty() {
            buttons.insert(cid.to_string(), b);
        }
    }
    cfg.buttons = buttons;
    Ok(cfg)
}

pub fn load(db: &DbHandle) -> LogiConfig {
    crate::settings::get(db, SETTING)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str::<LogiConfig>(&s).ok())
        .and_then(|c| validate(c).ok())
        .unwrap_or_default()
}

pub fn save(db: &DbHandle, cfg: LogiConfig) -> Result<LogiConfig, String> {
    let cfg = validate(cfg)?;
    let json = serde_json::to_string(&cfg).map_err(|e| e.to_string())?;
    crate::settings::set(db, SETTING, &json).map_err(|e| e.to_string())?;
    Ok(cfg)
}

// ── Button rules ─────────────────────────────────────────────────────────────

/// How far the pointer must travel while a button is held for the release to
/// count as a direction (points).
pub const GESTURE_MIN_PT: f64 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// Screen coordinates: `dy < 0` is up. The dominant axis decides.
pub fn classify_move(dx: f64, dy: f64) -> Option<Direction> {
    if dx.abs().max(dy.abs()) < GESTURE_MIN_PT {
        return None;
    }
    Some(if dx.abs() >= dy.abs() {
        if dx > 0.0 { Direction::Right } else { Direction::Left }
    } else if dy > 0.0 {
        Direction::Down
    } else {
        Direction::Up
    })
}

/// A button without direction actions acts the moment it goes down; one with
/// them has to wait for the release to know which it was.
pub fn fires_on_press(b: &ButtonConfig) -> bool {
    b.press.is_some() && !b.has_directions()
}

/// What a release does, given how far the pointer moved while the button was
/// held. `None` for a button that already fired on press.
pub fn release_action(b: &ButtonConfig, dx: f64, dy: f64) -> Option<(&BindingAction, Option<Direction>)> {
    if fires_on_press(b) {
        return None;
    }
    match classify_move(dx, dy) {
        None => b.press.as_ref().map(|a| (a, None)),
        Some(d) => match d {
            Direction::Up => b.up.as_ref(),
            Direction::Down => b.down.as_ref(),
            Direction::Left => b.left.as_ref(),
            Direction::Right => b.right.as_ref(),
        }
        .map(|a| (a, Some(d))),
    }
}

/// Buttons that went down and came up between two pressed-sets.
pub fn press_edges(before: &[u16], now: &[u16]) -> (Vec<u16>, Vec<u16>) {
    let down = now.iter().copied().filter(|c| !before.contains(c)).collect();
    let up = before.iter().copied().filter(|c| !now.contains(c)).collect();
    (down, up)
}

// ── Status for the panel ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dpi {
    pub current: u16,
    pub default: u16,
    pub options: Vec<u16>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogiStatus {
    /// False off macOS.
    pub supported: bool,
    pub connected: bool,
    /// macOS refused to open the device (Input Monitoring).
    pub permission_denied: bool,
    pub name: String,
    pub battery: Option<Battery>,
    pub dpi: Option<Dpi>,
    pub wheel: Option<Wheel>,
    /// `None` when the wheel cannot invert.
    pub invert: Option<bool>,
    pub controls: Vec<Control>,
    /// Logi Options+ is running and will fight over the same settings.
    pub options_running: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogiView {
    pub status: LogiStatus,
    pub config: LogiConfig,
}

pub fn start(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    macos::start(app.clone());
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// The cached status (no device traffic).
pub fn status() -> LogiStatus {
    #[cfg(target_os = "macos")]
    return macos::status();
    #[cfg(not(target_os = "macos"))]
    LogiStatus::default()
}

/// Read battery and settings from the mouse again; blocks up to a few seconds.
pub fn refresh() -> LogiStatus {
    #[cfg(target_os = "macos")]
    macos::refresh();
    status()
}

/// Apply a saved configuration to the connected mouse; blocks like `refresh`.
pub fn apply(cfg: LogiConfig) -> LogiStatus {
    #[cfg(target_os = "macos")]
    macos::apply(cfg);
    #[cfg(not(target_os = "macos"))]
    let _ = cfg;
    status()
}

/// Ask macOS for Input Monitoring (shows the system prompt once).
pub fn request_access() -> bool {
    #[cfg(target_os = "macos")]
    return macos::request_access();
    #[cfg(not(target_os = "macos"))]
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_has_the_long_report_layout() {
        let m = request(9, 3, &[0, 0x03, 0xE8]);
        assert_eq!(&m[..7], &[0x11, 0xFF, 9, 0x3A, 0, 0x03, 0xE8]);
        assert_eq!(m.len(), 20);
        assert!(m[7..].iter().all(|b| *b == 0));
    }

    #[test]
    fn over_long_params_are_cut_not_overflowed() {
        let m = request(1, 0, &[7u8; 40]);
        assert_eq!(m[4..], [7u8; 16]);
    }

    #[test]
    fn replies_errors_and_foreign_traffic_are_told_apart() {
        // Captured from an MX Master: battery reply, and an error for 0x1d4b.
        let reply = [0x11, 0xFF, 0x06, 0x0A, 0x5A, 0x32, 0, 0];
        assert_eq!(classify_incoming(&reply, 6, 0), Incoming::Reply(&[0x5A, 0x32, 0, 0][..]));
        let err = [0x11, 0xFF, 0xFF, 0x04, 0x0A, 0x07, 0, 0];
        assert_eq!(classify_incoming(&err, 4, 0), Incoming::Error(7));
        // Same feature, another function / another program's software id.
        assert_eq!(classify_incoming(&reply, 6, 1), Incoming::Other);
        let foreign = [0x11, 0xFF, 0x06, 0x05, 0x5A, 0x32, 0, 0];
        assert_eq!(classify_incoming(&foreign, 6, 0), Incoming::Other);
        assert_eq!(classify_incoming(&[0x11, 0xFF], 6, 0), Incoming::Other);
    }

    #[test]
    fn events_carry_software_id_zero() {
        let ev = [0x11, 0xFF, 0x08, 0x00, 0x00, 0xC3, 0, 0];
        assert_eq!(as_event(&ev), Some((8, 0, &[0x00, 0xC3, 0, 0][..])));
        let reply = [0x11, 0xFF, 0x08, 0x0A, 0x00, 0xC3, 0, 0];
        assert_eq!(as_event(&reply), None);
        let err = [0x11, 0xFF, 0xFF, 0x00, 0x00, 0x07, 0, 0];
        assert_eq!(as_event(&err), None);
    }

    #[test]
    fn the_mx_master_dpi_list_expands_its_step() {
        // 400, step 200, 1600 — as the mouse sends it.
        let payload = [0x01, 0x90, 0xE0, 0xC8, 0x06, 0x40, 0, 0, 0, 0];
        assert_eq!(parse_dpi_list(&payload), vec![400, 600, 800, 1000, 1200, 1400, 1600]);
    }

    #[test]
    fn a_plain_dpi_list_is_kept_and_a_broken_step_is_ignored() {
        assert_eq!(parse_dpi_list(&[0x03, 0x20, 0x01, 0x90, 0x06, 0x40, 0, 0]), vec![400, 800, 1600]);
        // A step with no upper bound, and a zero step.
        assert_eq!(parse_dpi_list(&[0x01, 0x90, 0xE0, 0xC8, 0, 0]), vec![400]);
        assert_eq!(parse_dpi_list(&[0x01, 0x90, 0xE0, 0x00, 0x06, 0x40]), vec![400, 1600]);
        assert!(parse_dpi_list(&[]).is_empty());
    }

    #[test]
    fn the_nearest_dpi_is_picked() {
        let o = [400, 600, 800];
        assert_eq!(nearest_dpi(&o, 650), Some(600));
        assert_eq!(nearest_dpi(&o, 700), Some(600));
        assert_eq!(nearest_dpi(&o, 5000), Some(800));
        assert_eq!(nearest_dpi(&[], 800), None);
    }

    #[test]
    fn battery_level_and_charging() {
        assert_eq!(parse_battery(&[90, 50, 0]), Some(Battery { percent: 90, charging: false }));
        assert_eq!(parse_battery(&[40, 30, 1]), Some(Battery { percent: 40, charging: true }));
        assert_eq!(parse_battery(&[100, 90, 3]), Some(Battery { percent: 100, charging: true }));
        assert_eq!(parse_battery(&[200, 0, 0]), None);
        assert_eq!(parse_battery(&[]), None);
    }

    #[test]
    fn smartshift_reads_all_three_modes() {
        assert_eq!(parse_smartshift(&[2, 0x10, 0x10]), Some(Wheel { mode: WheelMode::Smart, threshold: 16 }));
        assert_eq!(parse_smartshift(&[2, 0xFF, 0x10]), Some(Wheel { mode: WheelMode::Ratchet, threshold: 16 }));
        assert_eq!(parse_smartshift(&[2, 0, 0]), Some(Wheel { mode: WheelMode::Ratchet, threshold: SMART_DEFAULT }));
        assert_eq!(parse_smartshift(&[1, 0x10, 0x10]), Some(Wheel { mode: WheelMode::Free, threshold: 16 }));
        assert_eq!(parse_smartshift(&[9, 0, 0]), None);
    }

    #[test]
    fn smartshift_bytes_round_trip() {
        for w in [
            Wheel { mode: WheelMode::Free, threshold: 16 },
            Wheel { mode: WheelMode::Ratchet, threshold: 16 },
            Wheel { mode: WheelMode::Smart, threshold: 30 },
        ] {
            let b = smartshift_bytes(w);
            // The device answers a get with [mode, auto, default].
            assert_eq!(parse_smartshift(&[b[0], b[1], 16]), Some(w));
        }
        // Out-of-range thresholds never reach the device.
        assert_eq!(smartshift_bytes(Wheel { mode: WheelMode::Smart, threshold: 200 })[1], SMART_MAX);
        assert_eq!(smartshift_bytes(Wheel { mode: WheelMode::Smart, threshold: 0 })[1], SMART_MIN);
    }

    #[test]
    fn invert_touches_only_its_bit() {
        assert_eq!(with_invert(0b0000_0010, true), 0b0000_0110);
        assert_eq!(with_invert(0b0000_0110, false), 0b0000_0010);
        assert_eq!(with_invert(0b0000_0100, true), 0b0000_0100);
    }

    #[test]
    fn controls_and_their_divertable_flag() {
        // Left button (not divertable) and the gesture button, as captured.
        assert_eq!(parse_control(&[0x00, 0x50, 0x00, 0x38, 0x11, 0, 1, 1, 0]), Some(Control { cid: 0x50, divertable: false }));
        assert_eq!(parse_control(&[0x00, 0xC3, 0x00, 0x9C, 0x31, 0, 3, 7, 0]), Some(Control { cid: 0xC3, divertable: true }));
        assert_eq!(parse_control(&[0, 0, 0, 0, 0x31]), None);
        assert_eq!(parse_control(&[0, 0x50]), None);
    }

    #[test]
    fn diversion_bytes_and_pressed_sets() {
        assert_eq!(divert_bytes(0xC3, true), [0x00, 0xC3, 0x03, 0, 0]);
        assert_eq!(divert_bytes(0xC3, false), [0x00, 0xC3, 0x02, 0, 0]);
        assert_eq!(parse_pressed(&[0x00, 0xC3, 0x00, 0x53, 0, 0, 0, 0, 9, 9]), vec![0xC3, 0x53]);
        assert!(parse_pressed(&[0; 16]).is_empty());
    }

    #[test]
    fn edges_between_pressed_sets() {
        assert_eq!(press_edges(&[], &[0xC3]), (vec![0xC3], vec![]));
        assert_eq!(press_edges(&[0xC3], &[0xC3, 0x53]), (vec![0x53], vec![]));
        assert_eq!(press_edges(&[0xC3, 0x53], &[0x53]), (vec![], vec![0xC3]));
        assert_eq!(press_edges(&[0xC3], &[]), (vec![], vec![0xC3]));
    }

    #[test]
    fn a_move_needs_distance_and_the_dominant_axis_wins() {
        assert_eq!(classify_move(10.0, -20.0), None);
        assert_eq!(classify_move(0.0, -GESTURE_MIN_PT), Some(Direction::Up));
        assert_eq!(classify_move(5.0, 80.0), Some(Direction::Down));
        assert_eq!(classify_move(-90.0, 30.0), Some(Direction::Left));
        assert_eq!(classify_move(90.0, -89.0), Some(Direction::Right));
    }

    fn act(a: BindingAction) -> Option<BindingAction> {
        Some(a)
    }

    #[test]
    fn a_plain_button_fires_on_press_and_not_again_on_release() {
        let b = ButtonConfig { press: act(BindingAction::MuteToggle), ..Default::default() };
        assert!(fires_on_press(&b));
        assert_eq!(release_action(&b, 0.0, 0.0), None);
        assert_eq!(release_action(&b, 300.0, 0.0), None);
    }

    #[test]
    fn a_gesture_button_decides_on_release() {
        let b = ButtonConfig {
            press: act(BindingAction::MuteToggle),
            up: act(BindingAction::VolumeUp),
            left: act(BindingAction::PrevTab),
            ..Default::default()
        };
        assert!(!fires_on_press(&b));
        assert_eq!(release_action(&b, 3.0, -2.0), Some((&BindingAction::MuteToggle, None)));
        assert_eq!(release_action(&b, 0.0, -120.0), Some((&BindingAction::VolumeUp, Some(Direction::Up))));
        assert_eq!(release_action(&b, -120.0, 0.0), Some((&BindingAction::PrevTab, Some(Direction::Left))));
        // A direction nothing is bound to does nothing — never the press action.
        assert_eq!(release_action(&b, 120.0, 0.0), None);
        assert_eq!(release_action(&b, 0.0, 120.0), None);
    }

    #[test]
    fn directions_without_a_press_action() {
        let b = ButtonConfig { down: act(BindingAction::VolumeDown), ..Default::default() };
        assert!(!fires_on_press(&b));
        assert_eq!(release_action(&b, 0.0, 0.0), None);
        assert_eq!(release_action(&b, 0.0, 99.0), Some((&BindingAction::VolumeDown, Some(Direction::Down))));
    }

    #[test]
    fn validation_drops_empty_buttons_and_bad_ids_and_clamps() {
        let mut cfg = LogiConfig {
            dpi: Some(1000),
            wheel: Some(Wheel { mode: WheelMode::Smart, threshold: 99 }),
            ..Default::default()
        };
        cfg.buttons.insert("195".into(), ButtonConfig { press: act(BindingAction::MuteToggle), ..Default::default() });
        cfg.buttons.insert("83".into(), ButtonConfig::default());
        cfg.buttons.insert("abc".into(), ButtonConfig { press: act(BindingAction::MuteToggle), ..Default::default() });
        let out = validate(cfg).unwrap();
        assert_eq!(out.wheel.unwrap().threshold, SMART_MAX);
        assert_eq!(out.buttons.keys().collect::<Vec<_>>(), vec!["195"]);
        assert!(out.button(0xC3).is_some());
        assert!(out.button(0x53).is_none());
    }

    #[test]
    fn validation_refuses_bad_values() {
        assert!(validate(LogiConfig { dpi: Some(50), ..Default::default() }).is_err());
        let mut cfg = LogiConfig::default();
        cfg.buttons.insert("83".into(), ButtonConfig { press: act(BindingAction::Task { id: 0 }), ..Default::default() });
        assert!(validate(cfg).is_err());
        let mut cfg = LogiConfig::default();
        cfg.buttons.insert("83".into(), ButtonConfig { up: act(BindingAction::Open { target: "-rf".into() }), ..Default::default() });
        assert!(validate(cfg).is_err());
        let mut cfg = LogiConfig::default();
        cfg.buttons.insert("83".into(), ButtonConfig { left: act(BindingAction::Hotkey { action: "nope".into() }), ..Default::default() });
        assert!(validate(cfg).is_err());
    }

    #[test]
    fn a_stored_config_survives_the_round_trip_and_old_shapes_load() {
        let mut cfg = LogiConfig { dpi: Some(1200), invert: Some(true), ..Default::default() };
        cfg.buttons.insert(
            "195".into(),
            ButtonConfig { up: act(BindingAction::Shortcut { keys: "Ctrl+ArrowUp".into() }), ..Default::default() },
        );
        let json = serde_json::to_string(&cfg).unwrap();
        assert_eq!(serde_json::from_str::<LogiConfig>(&json).unwrap(), cfg);
        assert_eq!(serde_json::from_str::<LogiConfig>("{}").unwrap(), LogiConfig::default());
    }

    #[test]
    fn known_buttons_have_names() {
        assert_eq!(control_name(0xC3), "gesture");
        assert_eq!(control_name(0x53), "back");
        assert_eq!(control_name(0x1234), "other");
    }
}
