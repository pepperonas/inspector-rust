//! Public IP address and approximate IP geolocation for the `ip` command.
//!
//! The provider sees the request IP. Results are approximate (usually the ISP
//! or city centroid), never a device GPS location. No result is persisted.

use std::io::Read;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

const ENDPOINT: &str = "https://ipapi.co/json/";
const HTTP_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct IpReport {
    pub ip: String,
    pub city: Option<String>,
    pub region: Option<String>,
    pub country: Option<String>,
    pub country_code: Option<String>,
    pub postal: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub timezone: Option<String>,
    pub organization: Option<String>,
    pub asn: Option<String>,
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

pub fn parse_report(body: &str) -> Result<IpReport, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("ip: invalid response: {e}"))?;
    if let Some(reason) = text(&v, "error") {
        return Err(format!("ip lookup failed: {reason}"));
    }
    let ip = text(&v, "ip").ok_or_else(|| "ip lookup returned no public address".to_string())?;
    Ok(IpReport {
        ip,
        city: text(&v, "city"),
        region: text(&v, "region"),
        country: text(&v, "country_name"),
        country_code: text(&v, "country_code"),
        postal: text(&v, "postal"),
        latitude: v.get("latitude").and_then(Value::as_f64),
        longitude: v.get("longitude").and_then(Value::as_f64),
        timezone: text(&v, "timezone"),
        organization: text(&v, "org"),
        asn: text(&v, "asn"),
    })
}

pub fn fetch() -> Result<IpReport, String> {
    let response = ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .build()
        .get(ENDPOINT)
        .call()
        .map_err(|e| format!("ip lookup request failed: {e}"))?;
    let mut body = String::new();
    response
        .into_reader()
        .take(128 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| format!("ip lookup read failed: {e}"))?;
    parse_report(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_response_and_optional_fields() {
        let report = parse_report(r#"{"ip":"203.0.113.9","city":"Berlin","region":"Berlin","country_name":"Germany","country_code":"DE","postal":"10115","latitude":52.52,"longitude":13.405,"timezone":"Europe/Berlin","org":"Example ISP","asn":"AS64500"}"#).unwrap();
        assert_eq!(report.ip, "203.0.113.9");
        assert_eq!(report.city.as_deref(), Some("Berlin"));
        assert_eq!(report.latitude, Some(52.52));
    }

    #[test]
    fn tolerates_missing_optional_fields() {
        let report = parse_report(r#"{"ip":"198.51.100.7","city":"","latitude":1.5}"#).unwrap();
        assert_eq!(report.ip, "198.51.100.7");
        assert_eq!(report.city, None);
        assert_eq!(report.longitude, None);
    }

    #[test]
    fn rejects_provider_errors_and_missing_ip() {
        assert!(parse_report(r#"{"error":true,"reason":"Throttled"}"#).is_err());
        assert!(parse_report(r#"{"city":"Berlin"}"#).is_err());
        assert!(parse_report("not json").is_err());
    }
}
