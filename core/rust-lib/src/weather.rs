//! Weather — the `weather` search-bar command (v0.97.0).
//!
//! Renders inline in the popup's preview column (same inline-panel family as
//! `stats` / `hue` / `calendar`): the **current** conditions for your location
//! plus a **5-day forecast**, each with a weather-appropriate animation in the
//! frontend.
//!
//! **Provider: OpenWeatherMap** (the free 2.5 API — `/weather` + `/forecast`,
//! which a standard free key can call; One Call 3.0 needs a separate sub). The
//! API key lives in the settings table (`weather.api_key`), never committed.
//!
//! **"My location"** is resolved by IP geolocation (keyless ip-api.com, the
//! self-contained `ip_locate`) → lat/lon → OWM; a `weather <city>` argument
//! overrides it with an OWM `q=<city>` query. (Deliberately self-contained —
//! `weather` is cross-platform, so it must NOT depend on the macOS-only
//! `snitch` module, which is what broke the Linux CI in v0.97.0.)
//!
//! The JSON parsers (`parse_current`, `parse_forecast`), the icon→kind mapping
//! (`owm_icon_to_kind`), and the URL builders are pure and unit-tested; the
//! HTTP calls aren't (they need a live network + key).

use std::io::Read;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::db::DbHandle;
use crate::settings;

pub const KEY_API_KEY: &str = "weather.api_key";
pub const KEY_UNITS: &str = "weather.units";

/// Sentinel returned when no API key is configured — the frontend turns this
/// into a "add your OpenWeather key in Settings" connect card.
pub const ERR_NO_KEY: &str = "weather.no_key";

const OWM_BASE: &str = "https://api.openweathermap.org/data/2.5";
const HTTP_TIMEOUT: Duration = Duration::from_secs(6);
const DEFAULT_LANG: &str = "de";

// ── Weather kind (drives the frontend animation) ────────────────────────────

/// A coarse, animation-friendly classification of the sky, derived from the
/// OpenWeather icon code. Serialized as a lowercase string the `WeatherPanel`
/// switches on to pick its animated scene.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WeatherKind {
    ClearDay,
    ClearNight,
    Clouds,
    Drizzle,
    Rain,
    Thunderstorm,
    Snow,
    Mist,
}

/// Map an OpenWeather icon code (`01d`, `10n`, `50d`, …) to a `WeatherKind`.
///
/// The first two chars are the condition group; the trailing `d`/`n` is
/// day/night (only clear sky renders differently by that). Anything
/// unrecognised falls back to `Clouds` (a safe, neutral scene) — never panics.
pub fn owm_icon_to_kind(icon: &str) -> WeatherKind {
    let night = icon.ends_with('n');
    let group = icon.get(0..2).unwrap_or("");
    match group {
        "01" => {
            if night {
                WeatherKind::ClearNight
            } else {
                WeatherKind::ClearDay
            }
        }
        "02" | "03" | "04" => WeatherKind::Clouds,
        "09" => WeatherKind::Drizzle,
        "10" => WeatherKind::Rain,
        "11" => WeatherKind::Thunderstorm,
        "13" => WeatherKind::Snow,
        "50" => WeatherKind::Mist,
        _ => WeatherKind::Clouds,
    }
}

// ── Wire types ──────────────────────────────────────────────────────────────

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Current {
    /// Temperature in the report's units (°C for metric).
    pub temp: f64,
    pub feels_like: f64,
    pub temp_min: f64,
    pub temp_max: f64,
    /// Relative humidity, %.
    pub humidity: i64,
    /// Sea-level pressure, hPa.
    pub pressure: i64,
    /// Wind speed (m/s for metric — the frontend converts to km/h).
    pub wind_speed: f64,
    /// Wind direction, meteorological degrees (0 = N). `None` if absent.
    pub wind_deg: Option<i64>,
    /// Localized condition text (`lang=de` → "leichter Regen").
    pub description: String,
    /// OpenWeather icon code (kept for reference/debug).
    pub icon: String,
    /// Animation-friendly classification.
    pub kind: WeatherKind,
    /// Unix seconds; `0` if the response omitted them.
    pub sunrise: i64,
    pub sunset: i64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DailyForecast {
    /// Unix seconds of the representative (≈midday) slot for this day.
    pub dt: i64,
    /// `YYYY-MM-DD` (the grouping key; stable + timezone-of-the-API-local).
    pub date: String,
    pub temp_min: f64,
    pub temp_max: f64,
    pub icon: String,
    pub kind: WeatherKind,
    pub description: String,
}

/// One 3-hour slot of the near-term outlook (v0.106.0, the "next 12 h"
/// strip). Same OWM `/forecast` response the daily summary is built from —
/// no extra HTTP round trip.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct HourlyForecast {
    /// Unix seconds (UTC) of the slot start.
    pub dt: i64,
    pub temp: f64,
    pub icon: String,
    pub kind: WeatherKind,
    pub description: String,
    /// Probability of precipitation, 0.0–1.0 (OWM `pop`; 0 when absent).
    pub pop: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct WeatherReport {
    /// "City, CC" for the header.
    pub location: String,
    pub lat: f64,
    pub lon: f64,
    pub current: Current,
    /// The next ~12 hours as 3-hour slots (up to 5).
    pub hourly: Vec<HourlyForecast>,
    pub daily: Vec<DailyForecast>,
    /// The LOCATION's UTC offset in seconds (OWM `city.timezone`) — the
    /// frontend labels the hourly slots in the searched city's local time,
    /// not the machine's (`weather tokyo` shows Tokyo hours).
    pub tz_offset: i64,
    /// `metric` (°C) — the only supported unit set for now.
    pub units: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct WeatherConfig {
    /// Whether an API key is stored (the key itself never crosses IPC).
    pub has_key: bool,
    pub units: String,
}

// ── Pure parsers (unit-tested) ──────────────────────────────────────────────

fn num(v: &Value, path: &[&str]) -> Option<f64> {
    let mut cur = v;
    for p in path {
        cur = cur.get(p)?;
    }
    cur.as_f64()
}

/// Parse an OWM `/weather` (current conditions) response body.
pub fn parse_current(v: &Value) -> Result<Current, String> {
    let temp = num(v, &["main", "temp"]).ok_or("weather: missing main.temp")?;
    let w0 = v
        .get("weather")
        .and_then(|w| w.get(0))
        .ok_or("weather: missing weather[0]")?;
    let icon = w0
        .get("icon")
        .and_then(|x| x.as_str())
        .unwrap_or("01d")
        .to_string();
    let description = w0
        .get("description")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    Ok(Current {
        temp,
        feels_like: num(v, &["main", "feels_like"]).unwrap_or(temp),
        temp_min: num(v, &["main", "temp_min"]).unwrap_or(temp),
        temp_max: num(v, &["main", "temp_max"]).unwrap_or(temp),
        humidity: num(v, &["main", "humidity"]).unwrap_or(0.0) as i64,
        pressure: num(v, &["main", "pressure"]).unwrap_or(0.0) as i64,
        wind_speed: num(v, &["wind", "speed"]).unwrap_or(0.0),
        wind_deg: num(v, &["wind", "deg"]).map(|d| d as i64),
        kind: owm_icon_to_kind(&icon),
        icon,
        description,
        sunrise: num(v, &["sys", "sunrise"]).unwrap_or(0.0) as i64,
        sunset: num(v, &["sys", "sunset"]).unwrap_or(0.0) as i64,
    })
}

/// Extract "City, CC" from an OWM `/weather` response (both optional).
pub fn location_from_current(v: &Value) -> String {
    let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("");
    let country = v
        .get("sys")
        .and_then(|s| s.get("country"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    match (name.is_empty(), country.is_empty()) {
        (false, false) => format!("{name}, {country}"),
        (false, true) => name.to_string(),
        (true, false) => country.to_string(),
        (true, true) => String::new(),
    }
}

/// The hour (0–23) parsed from an OWM `dt_txt` (`"2026-07-29 12:00:00"`), or
/// `None` if malformed. Pure helper for picking the representative daily slot.
fn hour_of(dt_txt: &str) -> Option<u32> {
    dt_txt.get(11..13).and_then(|h| h.parse().ok())
}

/// Group an OWM `/forecast` (5-day / 3-hour) response into up to `max` daily
/// summaries: per calendar date (the API's local date, from `dt_txt`), the
/// min of all `temp_min` and max of all `temp_max`, and the icon/description
/// of the slot closest to **12:00** (a sensible "what does the day look like").
///
/// Order is preserved (earliest date first). Days with no usable entry are
/// skipped. Pure + deterministic (no clock read).
pub fn parse_forecast(v: &Value, max: usize) -> Vec<DailyForecast> {
    let list = match v.get("list").and_then(|l| l.as_array()) {
        Some(l) => l,
        None => return Vec::new(),
    };

    // Preserve first-seen date order.
    let mut order: Vec<String> = Vec::new();
    struct Acc {
        dt: i64,
        min: f64,
        max: f64,
        icon: String,
        description: String,
        best_hour_dist: u32,
    }
    let mut days: std::collections::HashMap<String, Acc> = std::collections::HashMap::new();

    for entry in list {
        let dt_txt = match entry.get("dt_txt").and_then(|x| x.as_str()) {
            Some(s) if s.len() >= 10 => s,
            _ => continue,
        };
        let date = dt_txt[0..10].to_string();
        let tmin = num(entry, &["main", "temp_min"]);
        let tmax = num(entry, &["main", "temp_max"]);
        let t = num(entry, &["main", "temp"]);
        // Need at least one temperature to be useful.
        let (tmin, tmax) = match (tmin.or(t), tmax.or(t)) {
            (Some(a), Some(b)) => (a, b),
            _ => continue,
        };
        let dt = num(entry, &["dt"]).unwrap_or(0.0) as i64;
        let icon = entry
            .get("weather")
            .and_then(|w| w.get(0))
            .and_then(|w| w.get("icon"))
            .and_then(|x| x.as_str())
            .unwrap_or("01d")
            .to_string();
        let description = entry
            .get("weather")
            .and_then(|w| w.get(0))
            .and_then(|w| w.get("description"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let hour = hour_of(dt_txt).unwrap_or(0);
        let hour_dist = hour.abs_diff(12);

        let acc = days.entry(date.clone()).or_insert_with(|| {
            order.push(date.clone());
            Acc {
                dt,
                min: tmin,
                max: tmax,
                icon: icon.clone(),
                description: description.clone(),
                best_hour_dist: u32::MAX,
            }
        });
        acc.min = acc.min.min(tmin);
        acc.max = acc.max.max(tmax);
        // The slot nearest midday defines the day's representative look.
        if hour_dist < acc.best_hour_dist {
            acc.best_hour_dist = hour_dist;
            acc.icon = icon;
            acc.description = description;
            acc.dt = dt;
        }
    }

    order
        .into_iter()
        .take(max)
        .filter_map(|date| {
            days.remove(&date).map(|a| DailyForecast {
                dt: a.dt,
                date,
                temp_min: (a.min * 10.0).round() / 10.0,
                temp_max: (a.max * 10.0).round() / 10.0,
                icon: a.icon.clone(),
                kind: owm_icon_to_kind(&a.icon),
                description: a.description,
            })
        })
        .collect()
}

/// The near-term slots for the "next 12 h" strip: every `/forecast` entry
/// whose slot is still RELEVANT (its 3-hour window hasn't fully passed —
/// `dt + 3 h > now`), first `max` of them. Pure + deterministic: `now` is
/// injected (unix seconds). Entries without a temperature are skipped;
/// `pop` defaults to 0 when absent.
pub fn parse_hourly(v: &Value, now: i64, max: usize) -> Vec<HourlyForecast> {
    const SLOT_S: i64 = 3 * 3600;
    let list = match v.get("list").and_then(|l| l.as_array()) {
        Some(l) => l,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in list {
        if out.len() >= max {
            break;
        }
        let dt = match num(entry, &["dt"]) {
            Some(d) => d as i64,
            None => continue,
        };
        if dt + SLOT_S <= now {
            continue;
        }
        let temp = match num(entry, &["main", "temp"]) {
            Some(t) => t,
            None => continue,
        };
        let icon = entry
            .get("weather")
            .and_then(|w| w.get(0))
            .and_then(|w| w.get("icon"))
            .and_then(|x| x.as_str())
            .unwrap_or("01d")
            .to_string();
        let description = entry
            .get("weather")
            .and_then(|w| w.get(0))
            .and_then(|w| w.get("description"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        out.push(HourlyForecast {
            dt,
            temp: (temp * 10.0).round() / 10.0,
            kind: owm_icon_to_kind(&icon),
            icon,
            description,
            pop: num(entry, &["pop"]).unwrap_or(0.0).clamp(0.0, 1.0),
        });
    }
    out
}

/// The location's UTC offset in seconds from an OWM `/forecast` response
/// (`city.timezone`); 0 when absent. Pure.
pub fn forecast_tz_offset(v: &Value) -> i64 {
    num(v, &["city", "timezone"]).unwrap_or(0.0) as i64
}

/// One of the two ways to name the place we want weather for.
pub enum Location {
    Coords { lat: f64, lon: f64 },
    City(String),
}

/// URL-encode a query value (city name) — RFC-3986 unreserved stay literal,
/// everything else is percent-encoded per UTF-8 byte. Small, dependency-free.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Build an OWM endpoint URL (`endpoint` = `"weather"` or `"forecast"`).
/// Pure — the key/units/lang are query params. Coordinates are formatted with
/// a stable precision so the tests are deterministic.
pub fn build_url(
    base: &str,
    endpoint: &str,
    loc: &Location,
    key: &str,
    units: &str,
    lang: &str,
) -> String {
    let place = match loc {
        Location::Coords { lat, lon } => format!("lat={lat}&lon={lon}"),
        Location::City(c) => format!("q={}", url_encode(c)),
    };
    format!(
        "{base}/{endpoint}?{place}&units={units}&lang={lang}&appid={}",
        url_encode(key)
    )
}

// ── HTTP (not unit-tested; needs a live network + key) ───────────────────────

/// Best-effort geolocation of this machine via ip-api.com's keyless `/json`
/// (no IP → locates the caller). Returns `(lat, lon)`. Self-contained so the
/// cross-platform `weather` command never depends on the macOS-only `snitch`.
fn ip_locate() -> Option<(f64, f64)> {
    let resp = ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .build()
        .get("http://ip-api.com/json?fields=status,lat,lon")
        .call()
        .ok()?;
    let mut body = String::new();
    resp.into_reader()
        .take(64 * 1024)
        .read_to_string(&mut body)
        .ok()?;
    let v: Value = serde_json::from_str(&body).ok()?;
    if v.get("status").and_then(|s| s.as_str()) != Some("success") {
        return None;
    }
    Some((v.get("lat")?.as_f64()?, v.get("lon")?.as_f64()?))
}

fn get_json(url: &str) -> Result<Value, String> {
    let resp = ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .build()
        .get(url)
        .call()
        .map_err(|e| match e {
            // OWM answers 401 for a bad/inactive key — give a clear message.
            ureq::Error::Status(401, _) => {
                "OpenWeather rejected the API key (401). Check it in Settings → Weather.".to_string()
            }
            ureq::Error::Status(404, _) => "Location not found.".to_string(),
            other => format!("weather request failed: {other}"),
        })?;
    let mut body = String::new();
    resp.into_reader()
        .take(4 * 1024 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| format!("weather read failed: {e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("weather bad JSON: {e}"))
}

/// Read the stored units, defaulting to `metric`.
pub fn stored_units(db: &DbHandle) -> String {
    settings::get(db, KEY_UNITS)
        .ok()
        .flatten()
        .filter(|u| u == "metric" || u == "imperial")
        .unwrap_or_else(|| "metric".to_string())
}

/// Fetch a full report. If `city` is `Some`, query by name; otherwise resolve
/// the caller's location by IP (ip-api) and query by coordinates.
pub fn fetch(db: &DbHandle, city: Option<String>) -> Result<WeatherReport, String> {
    let key = settings::get(db, KEY_API_KEY)
        .ok()
        .flatten()
        .filter(|k| !k.trim().is_empty())
        .ok_or(ERR_NO_KEY)?;
    let units = stored_units(db);
    let lang = DEFAULT_LANG;

    let loc = match city {
        Some(c) if !c.trim().is_empty() => Location::City(c.trim().to_string()),
        _ => {
            let (lat, lon) = ip_locate()
                .ok_or("Couldn't determine your location (IP geolocation failed).")?;
            Location::Coords { lat, lon }
        }
    };

    let cur_json = get_json(&build_url(OWM_BASE, "weather", &loc, &key, &units, lang))?;
    let current = parse_current(&cur_json)?;
    let location = location_from_current(&cur_json);
    let lat = num(&cur_json, &["coord", "lat"]).unwrap_or(0.0);
    let lon = num(&cur_json, &["coord", "lon"]).unwrap_or(0.0);

    // Forecast is best-effort — a current-only report is still useful. One
    // response feeds BOTH the daily summary and the next-12-h hourly strip.
    let (hourly, daily, tz_offset) =
        match get_json(&build_url(OWM_BASE, "forecast", &loc, &key, &units, lang)) {
            Ok(fj) => {
                let now = chrono::Utc::now().timestamp();
                (
                    parse_hourly(&fj, now, 5),
                    parse_forecast(&fj, 6),
                    forecast_tz_offset(&fj),
                )
            }
            Err(_) => (Vec::new(), Vec::new(), 0),
        };

    Ok(WeatherReport {
        location,
        lat,
        lon,
        current,
        hourly,
        daily,
        tz_offset,
        units,
    })
}

/// Persist the API key (trimmed). An empty string clears it.
pub fn set_api_key(db: &DbHandle, key: &str) -> Result<(), String> {
    settings::set(db, KEY_API_KEY, key.trim()).map_err(|e| e.to_string())
}

/// Persist the units preference (`metric` | `imperial`); anything else → metric.
pub fn set_units(db: &DbHandle, units: &str) -> Result<(), String> {
    let u = if units == "imperial" { "imperial" } else { "metric" };
    settings::set(db, KEY_UNITS, u).map_err(|e| e.to_string())
}

/// Current config for the settings UI (the key itself never leaves the backend).
pub fn config(db: &DbHandle) -> WeatherConfig {
    let has_key = settings::get(db, KEY_API_KEY)
        .ok()
        .flatten()
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false);
    WeatherConfig {
        has_key,
        units: stored_units(db),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn icon_codes_map_to_the_right_kind() {
        assert_eq!(owm_icon_to_kind("01d"), WeatherKind::ClearDay);
        assert_eq!(owm_icon_to_kind("01n"), WeatherKind::ClearNight);
        assert_eq!(owm_icon_to_kind("02d"), WeatherKind::Clouds);
        assert_eq!(owm_icon_to_kind("03n"), WeatherKind::Clouds);
        assert_eq!(owm_icon_to_kind("04d"), WeatherKind::Clouds);
        assert_eq!(owm_icon_to_kind("09d"), WeatherKind::Drizzle);
        assert_eq!(owm_icon_to_kind("10n"), WeatherKind::Rain);
        assert_eq!(owm_icon_to_kind("11d"), WeatherKind::Thunderstorm);
        assert_eq!(owm_icon_to_kind("13d"), WeatherKind::Snow);
        assert_eq!(owm_icon_to_kind("50d"), WeatherKind::Mist);
    }

    #[test]
    fn unknown_icon_falls_back_to_clouds_never_panics() {
        assert_eq!(owm_icon_to_kind(""), WeatherKind::Clouds);
        assert_eq!(owm_icon_to_kind("zz"), WeatherKind::Clouds);
        assert_eq!(owm_icon_to_kind("9"), WeatherKind::Clouds);
    }

    #[test]
    fn parse_current_reads_the_core_fields() {
        let v = json!({
            "coord": {"lat": 52.52, "lon": 13.41},
            "weather": [{"icon": "10d", "description": "leichter Regen"}],
            "main": {"temp": 18.4, "feels_like": 17.9, "temp_min": 16.0, "temp_max": 20.0,
                     "humidity": 72, "pressure": 1012},
            "wind": {"speed": 3.6, "deg": 210},
            "sys": {"country": "DE", "sunrise": 1000, "sunset": 2000},
            "name": "Berlin"
        });
        let c = parse_current(&v).unwrap();
        assert_eq!(c.temp, 18.4);
        assert_eq!(c.feels_like, 17.9);
        assert_eq!(c.humidity, 72);
        assert_eq!(c.pressure, 1012);
        assert_eq!(c.wind_speed, 3.6);
        assert_eq!(c.wind_deg, Some(210));
        assert_eq!(c.kind, WeatherKind::Rain);
        assert_eq!(c.description, "leichter Regen");
        assert_eq!(c.sunrise, 1000);
        assert_eq!(location_from_current(&v), "Berlin, DE");
    }

    #[test]
    fn parse_current_tolerates_missing_optional_fields() {
        // Only the truly-required temp + weather[0].icon present.
        let v = json!({
            "weather": [{"icon": "01d"}],
            "main": {"temp": 25.0}
        });
        let c = parse_current(&v).unwrap();
        assert_eq!(c.temp, 25.0);
        assert_eq!(c.feels_like, 25.0); // defaults to temp
        assert_eq!(c.temp_min, 25.0);
        assert_eq!(c.wind_deg, None);
        assert_eq!(c.humidity, 0);
        assert_eq!(c.description, "");
        assert_eq!(c.kind, WeatherKind::ClearDay);
        assert_eq!(location_from_current(&v), "");
    }

    #[test]
    fn parse_current_errors_without_temp_or_weather() {
        assert!(parse_current(&json!({"weather": [{"icon": "01d"}]})).is_err());
        assert!(parse_current(&json!({"main": {"temp": 1.0}})).is_err());
    }

    fn slot(dt_txt: &str, dt: i64, tmin: f64, tmax: f64, icon: &str) -> Value {
        json!({
            "dt": dt,
            "dt_txt": dt_txt,
            "main": {"temp_min": tmin, "temp_max": tmax},
            "weather": [{"icon": icon, "description": icon}]
        })
    }

    #[test]
    fn parse_forecast_groups_by_day_with_midday_icon_and_min_max() {
        let v = json!({"list": [
            // Day 1: three slots; the 12:00 one is snow, the extremes come from
            // the morning/evening slots.
            slot("2026-07-29 06:00:00", 1, 10.0, 12.0, "01d"),
            slot("2026-07-29 12:00:00", 2, 14.0, 20.0, "13d"),
            slot("2026-07-29 18:00:00", 3,  9.0, 22.0, "10d"),
            // Day 2: one slot at 15:00 (closest to noon by default).
            slot("2026-07-30 15:00:00", 4,  5.0,  8.0, "11d"),
        ]});
        let days = parse_forecast(&v, 6);
        assert_eq!(days.len(), 2);

        assert_eq!(days[0].date, "2026-07-29");
        assert_eq!(days[0].temp_min, 9.0); // min across all slots
        assert_eq!(days[0].temp_max, 22.0); // max across all slots
        assert_eq!(days[0].icon, "13d"); // 12:00 slot won
        assert_eq!(days[0].kind, WeatherKind::Snow);
        assert_eq!(days[0].dt, 2);

        assert_eq!(days[1].date, "2026-07-30");
        assert_eq!(days[1].kind, WeatherKind::Thunderstorm);
    }

    #[test]
    fn parse_forecast_respects_max_and_preserves_order() {
        let v = json!({"list": [
            slot("2026-07-29 12:00:00", 1, 1.0, 2.0, "01d"),
            slot("2026-07-30 12:00:00", 2, 1.0, 2.0, "01d"),
            slot("2026-07-31 12:00:00", 3, 1.0, 2.0, "01d"),
        ]});
        let days = parse_forecast(&v, 2);
        assert_eq!(days.len(), 2);
        assert_eq!(days[0].date, "2026-07-29");
        assert_eq!(days[1].date, "2026-07-30");
    }

    #[test]
    fn parse_forecast_handles_empty_or_missing_list() {
        assert!(parse_forecast(&json!({}), 6).is_empty());
        assert!(parse_forecast(&json!({"list": []}), 6).is_empty());
        // A slot without temps or dt_txt is skipped, not fatal.
        assert!(parse_forecast(&json!({"list": [{"weather": []}]}), 6).is_empty());
    }

    #[test]
    fn build_url_by_coords_and_by_city() {
        let u = build_url(
            "https://api.openweathermap.org/data/2.5",
            "weather",
            &Location::Coords { lat: 52.52, lon: 13.41 },
            "SECRET",
            "metric",
            "de",
        );
        assert_eq!(
            u,
            "https://api.openweathermap.org/data/2.5/weather?lat=52.52&lon=13.41&units=metric&lang=de&appid=SECRET"
        );

        let c = build_url(
            "https://api.openweathermap.org/data/2.5",
            "forecast",
            &Location::City("São Paulo".to_string()),
            "K",
            "metric",
            "de",
        );
        // Space + the non-ASCII 'ã' are percent-encoded.
        assert_eq!(
            c,
            "https://api.openweathermap.org/data/2.5/forecast?q=S%C3%A3o%20Paulo&units=metric&lang=de&appid=K"
        );
    }

    #[test]
    fn hour_of_parses_or_none() {
        assert_eq!(hour_of("2026-07-29 12:00:00"), Some(12));
        assert_eq!(hour_of("2026-07-29 00:00:00"), Some(0));
        assert_eq!(hour_of("bad"), None);
    }

    fn hourly_fixture() -> Value {
        // Slot starts every 3 h from t=0; "now" is injected per test.
        json!({
            "city": { "timezone": 7200 },
            "list": [
                { "dt": 0,     "main": { "temp": 10.04 }, "weather": [{ "icon": "01d", "description": "klar" }], "pop": 0.0 },
                { "dt": 10800, "main": { "temp": 12.06 }, "weather": [{ "icon": "10d", "description": "regen" }], "pop": 0.62 },
                { "dt": 21600, "main": { "temp": 14.0 },  "weather": [{ "icon": "04d", "description": "wolkig" }] },
                { "dt": 32400, "weather": [{ "icon": "01d" }] },              // no temp → skipped
                { "dt": 43200, "main": { "temp": 9.0 },   "weather": [{ "icon": "13d", "description": "schnee" }], "pop": 1.7 },
                { "dt": 54000, "main": { "temp": 8.0 },   "weather": [{ "icon": "01n", "description": "klar" }] },
            ]
        })
    }

    #[test]
    fn parse_hourly_keeps_the_running_slot_and_drops_fully_past_ones() {
        // now = 4000 → slot dt=0 still RUNS (its 3-h window ends at 10800);
        // it must stay — the strip's first cell is "right now".
        let h = parse_hourly(&hourly_fixture(), 4000, 5);
        assert_eq!(h[0].dt, 0);
        // now = 11000 → slot 0's window has fully passed → gone.
        let h = parse_hourly(&hourly_fixture(), 11_000, 5);
        assert_eq!(h[0].dt, 10800);
    }

    #[test]
    fn parse_hourly_caps_skips_templess_and_clamps_pop() {
        let h = parse_hourly(&hourly_fixture(), 0, 5);
        // dt=32400 has no temp → skipped; 5 usable slots remain.
        assert_eq!(
            h.iter().map(|s| s.dt).collect::<Vec<_>>(),
            vec![0, 10800, 21600, 43200, 54000]
        );
        assert_eq!(h[0].temp, 10.0); // rounded to 1 decimal
        assert_eq!(h[1].pop, 0.62);
        assert_eq!(h[2].pop, 0.0); // absent → 0
        assert_eq!(h[3].pop, 1.0); // out-of-range → clamped
        assert_eq!(h[3].kind, WeatherKind::Snow);
        // max is a hard cap.
        assert_eq!(parse_hourly(&hourly_fixture(), 0, 2).len(), 2);
    }

    #[test]
    fn parse_hourly_and_tz_handle_missing_fields() {
        assert!(parse_hourly(&json!({}), 0, 5).is_empty());
        assert!(parse_hourly(&json!({"list": []}), 0, 5).is_empty());
        assert_eq!(forecast_tz_offset(&hourly_fixture()), 7200);
        assert_eq!(forecast_tz_offset(&json!({})), 0);
    }
}
