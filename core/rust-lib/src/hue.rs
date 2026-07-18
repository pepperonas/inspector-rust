//! Philips Hue lamp control — the `hue` search-bar command (v0.84.40).
//!
//! Renders inline in the popup's preview column (same pattern as `brightness`
//! / `sound`): a global on/off + brightness control plus a row per lamp, with
//! 8 colour-preset swatches on colour-capable bulbs.
//!
//! **Local-first.** All bridge traffic is plain HTTP on the LAN (the classic
//! Hue local API on port 80) and discovery is **SSDP** (UDP multicast) — no
//! Philips cloud call, no TLS. The bridge IP + the paired API username are
//! persisted in the settings table (`hue.bridge_ip` / `hue.username`).
//!
//! Connect flow: discover (or enter) the bridge IP → press the bridge's link
//! button → `pair()` creates a whitelisted username → it's stored and reused.
//!
//! The colour maths (`hex_to_rgb`, `rgb_to_xy`) and the brightness mapping
//! (`percent_to_bri` / `bri_to_percent`) are pure and unit-tested; the HTTP
//! calls aren't (they need a live bridge).

use std::io::Read;
use std::net::UdpSocket;
use std::time::Duration;

use serde::Serialize;

use crate::db::DbHandle;
use crate::settings;

pub const KEY_BRIDGE_IP: &str = "hue.bridge_ip";
pub const KEY_USERNAME: &str = "hue.username";

/// Sentinel returned by `pair` when the bridge replies "link button not
/// pressed" — the frontend turns this into a friendly "press the button" hint.
pub const ERR_LINK_BUTTON: &str = "hue.link_button";

const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

// ── Pure helpers (unit-tested) ──────────────────────────────────────────────

/// Parse a `#rrggbb` / `rrggbb` hex string into 8-bit RGB. Returns `None` on
/// any malformed input (wrong length / non-hex).
pub fn hex_to_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some((r, g, b))
}

/// Convert 8-bit sRGB to the CIE 1931 xy chromaticity the Hue API expects,
/// using Philips' documented gamma-correction + Wide-RGB-D65 matrix. Returns
/// `(x, y)` each in `[0,1]`, rounded to 4 dp. Pure.
pub fn rgb_to_xy(r: u8, g: u8, b: u8) -> (f64, f64) {
    let gamma = |c: f64| {
        if c > 0.04045 {
            ((c + 0.055) / 1.055).powf(2.4)
        } else {
            c / 12.92
        }
    };
    let red = gamma(f64::from(r) / 255.0);
    let green = gamma(f64::from(g) / 255.0);
    let blue = gamma(f64::from(b) / 255.0);

    let x = red * 0.649_926 + green * 0.103_455 + blue * 0.197_109;
    let y = red * 0.234_327 + green * 0.743_075 + blue * 0.022_598;
    let z = green * 0.053_077 + blue * 1.035_763;

    let sum = x + y + z;
    if sum <= 0.0 {
        return (0.0, 0.0);
    }
    let cx = (x / sum * 10_000.0).round() / 10_000.0;
    let cy = (y / sum * 10_000.0).round() / 10_000.0;
    (cx, cy)
}

/// Map a 0–100 % brightness to the Hue `bri` range (1–254, rounded).
pub fn percent_to_bri(percent: u8) -> u8 {
    let p = u32::from(percent.min(100));
    (1 + (p * 253 + 50) / 100) as u8
}

/// Map a Hue `bri` (1–254) back to 0–100 % (rounded, clamped).
pub fn bri_to_percent(bri: u8) -> u8 {
    let b = i32::from(bri.max(1)) - 1;
    (((b * 100 + 126) / 253).clamp(0, 100)) as u8
}

// ── Wire types ──────────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct HueStatus {
    /// We have a stored bridge IP + username and the bridge answered.
    pub connected: bool,
    /// Stored bridge IP, if any (shown in the connect UI).
    pub bridge_ip: Option<String>,
    /// We have a username (paired) — but may still be unreachable.
    pub paired: bool,
}

#[derive(Serialize, Clone)]
pub struct HueLight {
    pub id: String,
    pub name: String,
    pub on: bool,
    /// 0–100 %.
    pub brightness: u8,
    pub reachable: bool,
    /// Whether the bulb can show colour (→ render the swatches).
    pub supports_color: bool,
    /// Whether the bulb is dimmable (→ render the brightness slider).
    pub dimmable: bool,
}

// ── HTTP plumbing (LAN, plain HTTP) ─────────────────────────────────────────

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .build()
}

fn get_json(url: &str) -> Result<serde_json::Value, String> {
    let resp = agent().get(url).call().map_err(|e| format!("hue GET failed: {e}"))?;
    let mut body = String::new();
    resp.into_reader()
        .take(8 * 1024 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| format!("hue read failed: {e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("hue bad JSON: {e}"))
}

fn send_json(method: &str, url: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    let resp = agent()
        .request(method, url)
        .send_string(&body.to_string())
        .map_err(|e| format!("hue {method} failed: {e}"))?;
    let mut text = String::new();
    resp.into_reader()
        .take(1024 * 1024)
        .read_to_string(&mut text)
        .map_err(|e| format!("hue read failed: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("hue bad JSON: {e}"))
}

/// First error description in a Hue API array response (`[{"error":{...}}]`),
/// with the link-button case mapped to the [`ERR_LINK_BUTTON`] sentinel.
fn first_error(v: &serde_json::Value) -> Option<String> {
    let err = v.as_array()?.iter().find_map(|e| e.get("error"))?;
    let typ = err.get("type").and_then(|t| t.as_i64());
    if typ == Some(101) {
        return Some(ERR_LINK_BUTTON.to_string());
    }
    Some(
        err.get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("unknown hue error")
            .to_string(),
    )
}

// ── Discovery (local SSDP) ──────────────────────────────────────────────────

/// Best-effort local discovery of a Hue bridge IP via SSDP (UDP multicast to
/// 239.255.255.250:1900). Returns the first IP whose `/api/config` looks like a
/// Hue bridge. Local-only — no cloud. `None` if nothing answered in time.
pub fn discover_bridge() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.set_read_timeout(Some(Duration::from_millis(1200))).ok()?;
    let msg = "M-SEARCH * HTTP/1.1\r\n\
        HOST: 239.255.255.250:1900\r\n\
        MAN: \"ssdp:discover\"\r\n\
        MX: 2\r\n\
        ST: ssdp:all\r\n\r\n";
    socket
        .send_to(msg.as_bytes(), "239.255.255.250:1900")
        .ok()?;

    let mut buf = [0u8; 2048];
    let mut tried: Vec<String> = Vec::new();
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(3) {
        let Ok((n, addr)) = socket.recv_from(&mut buf) else { break };
        let text = String::from_utf8_lossy(&buf[..n]);
        let lower = text.to_lowercase();
        // Hue bridges advertise "IpBridge" in SERVER and a hue-bridgeid header.
        if !(lower.contains("ipbridge") || lower.contains("hue-bridgeid")) {
            continue;
        }
        let ip = addr.ip().to_string();
        if tried.contains(&ip) {
            continue;
        }
        tried.push(ip.clone());
        if looks_like_bridge(&ip) {
            return Some(ip);
        }
    }
    None
}

/// Probe a candidate IP's unauthenticated `/api/config`: a Hue bridge returns
/// JSON carrying a `bridgeid`.
fn looks_like_bridge(ip: &str) -> bool {
    match get_json(&format!("http://{ip}/api/config")) {
        Ok(v) => v.get("bridgeid").is_some(),
        Err(_) => false,
    }
}

// ── Pairing ─────────────────────────────────────────────────────────────────

/// Create a whitelisted API username on the bridge. The user must have pressed
/// the bridge's physical link button first; otherwise the bridge returns error
/// 101 and we surface [`ERR_LINK_BUTTON`]. On success the username is returned.
pub fn pair(bridge_ip: &str) -> Result<String, String> {
    let body = serde_json::json!({ "devicetype": "inspector_rust#desktop" });
    let resp = send_json("POST", &format!("http://{bridge_ip}/api"), &body)?;
    if let Some(user) = resp
        .as_array()
        .and_then(|a| a.first())
        .and_then(|e| e.get("success"))
        .and_then(|s| s.get("username"))
        .and_then(|u| u.as_str())
    {
        return Ok(user.to_string());
    }
    Err(first_error(&resp).unwrap_or_else(|| "pairing failed".to_string()))
}

// ── Light listing / control ─────────────────────────────────────────────────

fn parse_light(id: &str, v: &serde_json::Value) -> Option<HueLight> {
    let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("Lamp").to_string();
    let state = v.get("state")?;
    let on = state.get("on").and_then(|o| o.as_bool()).unwrap_or(false);
    let reachable = state.get("reachable").and_then(|r| r.as_bool()).unwrap_or(true);
    let bri = state.get("bri").and_then(|b| b.as_u64());
    let dimmable = bri.is_some();
    let brightness = bri.map(|b| bri_to_percent(b as u8)).unwrap_or(100);
    // Colour-capable if the state exposes xy / hue, or the type says so.
    let typ = v.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
    let supports_color =
        state.get("xy").is_some() || state.get("hue").is_some() || typ.contains("color");
    Some(HueLight {
        id: id.to_string(),
        name,
        on,
        brightness,
        reachable,
        supports_color,
        dimmable,
    })
}

/// List all lamps on the bridge with their current state, sorted by name.
pub fn list_lights(bridge_ip: &str, username: &str) -> Result<Vec<HueLight>, String> {
    let v = get_json(&format!("http://{bridge_ip}/api/{username}/lights"))?;
    if let Some(err) = first_error(&v) {
        return Err(err);
    }
    let obj = v.as_object().ok_or("hue: unexpected /lights response")?;
    let mut lights: Vec<HueLight> = obj
        .iter()
        .filter_map(|(id, light)| parse_light(id, light))
        .collect();
    lights.sort_by_key(|l| l.name.to_lowercase());
    Ok(lights)
}

/// Build the state body for a PUT — `on`, optional brightness %, optional hex
/// colour (converted to xy). Pure (used by both single-light + group writes).
pub fn build_state_body(on: bool, brightness: Option<u8>, hex: Option<&str>) -> serde_json::Value {
    let mut body = serde_json::Map::new();
    body.insert("on".into(), serde_json::Value::Bool(on));
    if on {
        if let Some(p) = brightness {
            body.insert("bri".into(), serde_json::json!(percent_to_bri(p)));
        }
        if let Some((r, g, b)) = hex.and_then(hex_to_rgb) {
            let (x, y) = rgb_to_xy(r, g, b);
            body.insert("xy".into(), serde_json::json!([x, y]));
        }
    }
    serde_json::Value::Object(body)
}

/// Set a single lamp's state.
pub fn set_light(
    bridge_ip: &str,
    username: &str,
    id: &str,
    on: bool,
    brightness: Option<u8>,
    hex: Option<&str>,
) -> Result<(), String> {
    let body = build_state_body(on, brightness, hex);
    let url = format!("http://{bridge_ip}/api/{username}/lights/{id}/state");
    let resp = send_json("PUT", &url, &body)?;
    match first_error(&resp) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Set **all** lamps at once via group 0 (`/groups/0/action`).
pub fn set_all(
    bridge_ip: &str,
    username: &str,
    on: bool,
    brightness: Option<u8>,
    hex: Option<&str>,
) -> Result<(), String> {
    let body = build_state_body(on, brightness, hex);
    let url = format!("http://{bridge_ip}/api/{username}/groups/0/action");
    let resp = send_json("PUT", &url, &body)?;
    match first_error(&resp) {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

// ── Settings-backed config ──────────────────────────────────────────────────

pub fn bridge_ip(db: &DbHandle) -> Option<String> {
    settings::get(db, KEY_BRIDGE_IP).ok().flatten().filter(|s| !s.trim().is_empty())
}

pub fn username(db: &DbHandle) -> Option<String> {
    settings::get(db, KEY_USERNAME).ok().flatten().filter(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(hex_to_rgb("#ff0000"), Some((255, 0, 0)));
        assert_eq!(hex_to_rgb("00ff00"), Some((0, 255, 0)));
        assert_eq!(hex_to_rgb("#FFFFFF"), Some((255, 255, 255)));
        assert_eq!(hex_to_rgb("#fff"), None); // wrong length
        assert_eq!(hex_to_rgb("#gggggg"), None); // non-hex
        assert_eq!(hex_to_rgb(""), None);
    }

    #[test]
    fn rgb_to_xy_red_green_blue() {
        // Known Philips reference points (≈), within rounding tolerance.
        let (rx, ry) = rgb_to_xy(255, 0, 0);
        // Raw Wide-RGB-D65 xy for pure red ≈ (0.735, 0.265); the bulb clamps
        // this into its own gamut. Just assert it lands firmly in the red region.
        assert!(rx > 0.6 && ry < 0.35, "red xy={rx},{ry}");
        let (gx, gy) = rgb_to_xy(0, 255, 0);
        assert!(gx > 0.1 && gx < 0.3 && gy > 0.6, "green xy={gx},{gy}");
        let (bx, by) = rgb_to_xy(0, 0, 255);
        assert!(bx < 0.2 && by < 0.1, "blue xy={bx},{by}");
        // Black → guarded to (0,0), never NaN.
        assert_eq!(rgb_to_xy(0, 0, 0), (0.0, 0.0));
    }

    #[test]
    fn brightness_mapping_roundtrips_endpoints() {
        assert_eq!(percent_to_bri(0), 1);
        assert_eq!(percent_to_bri(100), 254);
        assert_eq!(bri_to_percent(1), 0);
        assert_eq!(bri_to_percent(254), 100);
        // Mid stays mid-ish through the round-trip.
        let mid = bri_to_percent(percent_to_bri(50));
        assert!((mid as i32 - 50).abs() <= 1, "mid={mid}");
    }

    #[test]
    fn state_body_on_off_and_color() {
        // Off → only `on:false`, no bri/xy even if provided.
        let off = build_state_body(false, Some(80), Some("#ff0000"));
        assert_eq!(off.get("on").unwrap(), &serde_json::Value::Bool(false));
        assert!(off.get("bri").is_none() && off.get("xy").is_none());

        // On with brightness + colour → carries bri + xy.
        let on = build_state_body(true, Some(50), Some("#00ff00"));
        assert_eq!(on.get("on").unwrap(), &serde_json::Value::Bool(true));
        assert!(on.get("bri").is_some());
        assert!(on.get("xy").is_some());

        // On, no colour → no xy.
        let plain = build_state_body(true, Some(50), None);
        assert!(plain.get("xy").is_none());
    }

    #[test]
    fn first_error_maps_link_button() {
        let v = serde_json::json!([{"error": {"type": 101, "description": "link button not pressed"}}]);
        assert_eq!(first_error(&v).as_deref(), Some(ERR_LINK_BUTTON));
        let ok = serde_json::json!([{"success": {"username": "abc"}}]);
        assert_eq!(first_error(&ok), None);
    }

    #[test]
    fn parse_light_reads_state() {
        let v = serde_json::json!({
            "name": "Couch",
            "type": "Extended color light",
            "state": { "on": true, "bri": 254, "xy": [0.3, 0.3], "reachable": true }
        });
        let l = parse_light("3", &v).unwrap();
        assert_eq!(l.id, "3");
        assert_eq!(l.name, "Couch");
        assert!(l.on && l.supports_color && l.dimmable && l.reachable);
        assert_eq!(l.brightness, 100);

        // A plug (on/off only) → not dimmable, not colour.
        let plug = serde_json::json!({
            "name": "Plug", "type": "On/Off plug-in unit",
            "state": { "on": false, "reachable": true }
        });
        let p = parse_light("9", &plug).unwrap();
        assert!(!p.dimmable && !p.supports_color);
    }

    #[test]
    fn first_error_returns_the_description_for_non_101_codes() {
        // A non-101 bridge error surfaces its human description, not the
        // link-button sentinel.
        let v = serde_json::json!([{"error": {"type": 7, "description": "invalid value"}}]);
        assert_eq!(first_error(&v).as_deref(), Some("invalid value"));
        // Error object without a description → a generic fallback string.
        let no_desc = serde_json::json!([{"error": {"type": 3}}]);
        assert_eq!(first_error(&no_desc).as_deref(), Some("unknown hue error"));
        // Not even an array → None.
        assert_eq!(first_error(&serde_json::json!({"error": "x"})), None);
    }

    #[test]
    fn percent_to_bri_clamps_above_100() {
        // Values > 100 % are clamped, not overflowed.
        assert_eq!(percent_to_bri(200), percent_to_bri(100));
        assert_eq!(percent_to_bri(101), 254);
    }

    #[test]
    fn parse_light_dimmable_only_light_is_not_colour() {
        // A plain white "Dimmable light" exposes bri but no xy/hue → dimmable
        // true, colour false.
        let v = serde_json::json!({
            "name": "Desk", "type": "Dimmable light",
            "state": { "on": true, "bri": 127, "reachable": true }
        });
        let l = parse_light("5", &v).unwrap();
        assert!(l.dimmable, "has bri → dimmable");
        assert!(!l.supports_color, "no xy/hue/'color' in type → not colour");
        assert!((l.brightness as i32 - 50).abs() <= 1);
    }

    #[test]
    fn parse_light_without_a_state_object_is_none() {
        // No `state` key → cannot describe the lamp → None (skipped by the caller).
        let v = serde_json::json!({ "name": "Ghost", "type": "Extended color light" });
        assert!(parse_light("1", &v).is_none());
    }

    #[test]
    fn rgb_to_xy_white_is_near_d65() {
        // Pure white should map close to the D65 white point (0.3127, 0.3290).
        let (x, y) = rgb_to_xy(255, 255, 255);
        assert!((x - 0.3127).abs() < 0.01, "white x={x}");
        assert!((y - 0.3290).abs() < 0.01, "white y={y}");
    }

    #[test]
    fn bri_to_percent_clamps_at_the_top() {
        // bri 255 (a u8 max, above the nominal 254) still maps to 100, never > 100.
        assert_eq!(bri_to_percent(255), 100);
        // Every bri maps into [0,100].
        for b in 0u8..=255 {
            let p = bri_to_percent(b);
            assert!(p <= 100, "bri {b} → {p}");
        }
    }

    #[test]
    fn percent_to_bri_is_monotonic() {
        // Increasing brightness % never decreases the mapped bri.
        let mut prev = 0u8;
        for p in 0u8..=100 {
            let bri = percent_to_bri(p);
            assert!(bri >= prev, "bri dropped at {p}% ({prev} → {bri})");
            prev = bri;
        }
    }

    #[test]
    fn state_body_on_without_brightness_or_with_bad_hex() {
        // On, no brightness, no colour → just {on:true}.
        let plain = build_state_body(true, None, None);
        assert_eq!(plain.get("on").unwrap(), &serde_json::Value::Bool(true));
        assert!(plain.get("bri").is_none());
        assert!(plain.get("xy").is_none());
        // On with an unparseable hex → no xy (hex_to_rgb returns None).
        let bad = build_state_body(true, None, Some("#zzz"));
        assert!(bad.get("xy").is_none());
    }

    #[test]
    fn parse_light_unreachable_and_hue_only_is_colour() {
        // reachable=false surfaces; a bulb exposing `hue` (but no xy) is colour.
        let v = serde_json::json!({
            "name": "Nook", "type": "Color light",
            "state": { "on": true, "bri": 100, "hue": 12000, "reachable": false }
        });
        let l = parse_light("7", &v).unwrap();
        assert!(!l.reachable);
        assert!(l.supports_color, "hue field → colour-capable");
        assert!(l.dimmable);
    }

    #[test]
    fn parse_light_defaults_name_and_reachable_when_absent() {
        // Missing name → "Lamp"; missing reachable → assumed reachable (true).
        let v = serde_json::json!({ "state": { "on": true } });
        let l = parse_light("2", &v).unwrap();
        assert_eq!(l.name, "Lamp");
        assert!(l.reachable);
        assert_eq!(l.brightness, 100); // no bri → 100 default
    }
}
