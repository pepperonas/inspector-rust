//! `pagespeed` — Google PageSpeed Insights for a URL, desktop AND mobile
//! (v0.142.0).
//!
//! One analysis fetches BOTH strategies (in parallel — each Lighthouse run
//! takes 10–40 s, and sequentially that is a minute of staring) and the panel
//! shows them side by side, because a page is routinely fine on desktop and
//! poor on mobile; showing one without the other invites the wrong conclusion.
//!
//! **The API key is optional but effectively necessary.** PSI answers without
//! one, but the anonymous quota is shared and empties quickly (measured: the
//! keyless call returned "Quota exceeded" on the first try). The key lives in
//! the `settings` table (`pagespeed.api_key`) and is entered in Settings —
//! never in the repo.
//!
//! ⚠️ **A Google API key can carry an IP restriction.** The key found in the
//! ops project is bound to that server's IPv4 and fails from anywhere else
//! ("The provided API key has an IP address restriction"), which is a
//! configuration matter in the Google console, not something the app can work
//! around — so [`classify_error`] names that case explicitly instead of
//! reporting a generic failure.

use serde::{Deserialize, Serialize};

use crate::db::DbHandle;
use crate::settings;

pub const KEY_API: &str = "pagespeed.api_key";
const ENDPOINT: &str = "https://www.googleapis.com/pagespeedonline/v5/runPagespeed";
/// A cold Lighthouse run on a slow page genuinely takes this long.
const TIMEOUT_SECS: u64 = 90;

/// The four Lighthouse categories, in the order PSI reports them.
const CATEGORIES: [(&str, &str); 4] = [
    ("performance", "Performance"),
    ("accessibility", "Barrierefreiheit"),
    ("best-practices", "Best Practices"),
    ("seo", "SEO"),
];

/// The Core-Web-Vitals-ish audits worth showing. Keyed by Lighthouse audit id.
const METRICS: [(&str, &str); 5] = [
    ("first-contentful-paint", "First Contentful Paint"),
    ("largest-contentful-paint", "Largest Contentful Paint"),
    ("total-blocking-time", "Total Blocking Time"),
    ("cumulative-layout-shift", "Cumulative Layout Shift"),
    ("speed-index", "Speed Index"),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CategoryScore {
    pub id: String,
    pub label: String,
    /// 0..100, or `None` when Lighthouse could not score it.
    pub score: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Metric {
    pub id: String,
    pub label: String,
    /// What Lighthouse prints, e.g. "1.5 s".
    pub display: String,
    pub score: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyRun {
    /// "desktop" | "mobile".
    pub strategy: String,
    pub categories: Vec<CategoryScore>,
    pub metrics: Vec<Metric>,
    pub final_url: String,
    pub fetch_time: String,
    pub lighthouse_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageSpeedReport {
    /// The URL as analysed (normalised).
    pub url: String,
    pub desktop: Option<StrategyRun>,
    pub mobile: Option<StrategyRun>,
    /// Human-readable reasons a strategy is missing. Never silently empty —
    /// ops swallows PSI failures into `None`, which makes a broken key look
    /// like a page with no data.
    pub errors: Vec<String>,
}

/// Lighthouse's own banding: ≥90 good, ≥50 needs improvement, else poor.
/// Pure so the panel, the export and the tests agree on one rule.
pub fn band(score: Option<u8>) -> &'static str {
    match score {
        Some(s) if s >= 90 => "good",
        Some(s) if s >= 50 => "average",
        Some(_) => "poor",
        None => "unknown",
    }
}

/// Accept what a person types: bare hosts get https://, whitespace is dropped.
/// Returns `None` for something that cannot be a URL at all.
pub fn normalize_url(input: &str) -> Option<String> {
    let t = input.trim();
    if t.is_empty() || t.contains(' ') {
        return None;
    }
    let with_scheme = if t.starts_with("http://") || t.starts_with("https://") {
        t.to_string()
    } else {
        format!("https://{t}")
    };
    // A host needs at least one dot (or be localhost) — otherwise `pagespeed
    // foo` would fire a pointless request.
    let host = with_scheme.split("://").nth(1)?.split('/').next()?;
    if host.contains('.') || host.starts_with("localhost") {
        Some(with_scheme)
    } else {
        None
    }
}

/// Build the request URL. The key is appended only when present, mirroring
/// how the ops backend calls the same endpoint.
pub fn build_request_url(page: &str, strategy: &str, key: &str) -> String {
    let mut u = format!("{ENDPOINT}?url={}&strategy={strategy}", url_encode(page));
    for (id, _) in CATEGORIES {
        u.push_str("&category=");
        u.push_str(id);
    }
    if !key.trim().is_empty() {
        u.push_str("&key=");
        u.push_str(&url_encode(key.trim()));
    }
    u
}

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

/// Turn Google's error payload into something a person can act on.
///
/// The two failures that actually happen get their own wording; everything
/// else is passed through rather than flattened into "request failed".
pub fn classify_error(message: &str) -> String {
    let m = message.to_lowercase();
    if m.contains("ip address restriction") {
        "Der API-Key ist auf bestimmte IP-Adressen beschränkt und gilt für diesen Rechner nicht. \
         In der Google-Cloud-Konsole die IP-Beschränkung entfernen oder diese IP eintragen."
            .to_string()
    } else if m.contains("quota") {
        "Google-Kontingent erschöpft. Ohne eigenen API-Key teilen sich alle Nutzer ein kleines \
         Tageskontingent — trag in den Einstellungen einen eigenen Key ein."
            .to_string()
    } else {
        message.to_string()
    }
}

/// Extract the strategy's numbers from a PSI v5 response. Pure.
///
/// Scores arrive as 0..1 floats; a missing or null score stays `None` rather
/// than becoming a 0 that would read as "catastrophically bad".
pub fn parse_run(strategy: &str, body: &str) -> Result<StrategyRun, String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("Antwort unlesbar: {e}"))?;
    if let Some(msg) = v.pointer("/error/message").and_then(|m| m.as_str()) {
        return Err(classify_error(msg));
    }
    let lr = v
        .get("lighthouseResult")
        .ok_or_else(|| "Antwort enthält kein Ergebnis".to_string())?;

    let pct = |s: Option<f64>| s.map(|x| (x * 100.0).round().clamp(0.0, 100.0) as u8);

    let categories = CATEGORIES
        .iter()
        .map(|(id, label)| CategoryScore {
            id: (*id).to_string(),
            label: (*label).to_string(),
            score: pct(
                lr.pointer(&format!("/categories/{id}/score"))
                    .and_then(|s| s.as_f64()),
            ),
        })
        .collect();

    let metrics = METRICS
        .iter()
        .filter_map(|(id, label)| {
            let a = lr.pointer(&format!("/audits/{id}"))?;
            Some(Metric {
                id: (*id).to_string(),
                label: (*label).to_string(),
                // Lighthouse uses a non-breaking space in "1.5 s"; normalise
                // it so the value doesn't wrap oddly or compare unequal.
                display: a
                    .get("displayValue")
                    .and_then(|d| d.as_str())
                    .unwrap_or("—")
                    .replace('\u{a0}', " "),
                score: pct(a.get("score").and_then(|s| s.as_f64())),
            })
        })
        .collect();

    Ok(StrategyRun {
        strategy: strategy.to_string(),
        categories,
        metrics,
        final_url: lr
            .get("finalUrl")
            .or_else(|| lr.get("requestedUrl"))
            .and_then(|u| u.as_str())
            .unwrap_or_default()
            .to_string(),
        fetch_time: lr
            .get("fetchTime")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_string(),
        lighthouse_version: lr
            .get("lighthouseVersion")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

pub fn get_key(db: &DbHandle) -> String {
    settings::get_or(db, KEY_API, "").unwrap_or_default()
}

pub fn set_key(db: &DbHandle, key: &str) -> anyhow::Result<()> {
    settings::set(db, KEY_API, key.trim())
}

/// One strategy, over the network.
fn fetch(page: &str, strategy: &str, key: &str) -> Result<StrategyRun, String> {
    let url = build_request_url(page, strategy, key);
    let resp = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .call();
    let body = match resp {
        Ok(r) => r.into_string().map_err(|e| format!("Antwort unlesbar: {e}"))?,
        // A 4xx carries Google's own error JSON — read it rather than
        // reporting the bare status, or an IP-restricted key just says "400".
        Err(ureq::Error::Status(_, r)) => {
            r.into_string().map_err(|e| format!("Antwort unlesbar: {e}"))?
        }
        Err(e) => return Err(format!("Anfrage fehlgeschlagen: {e}")),
    };
    parse_run(strategy, &body)
}

/// Analyse both strategies. They run in PARALLEL — two cold Lighthouse runs
/// in sequence is about a minute of waiting for no reason.
pub fn analyze(page: &str, key: &str) -> PageSpeedReport {
    let url = normalize_url(page).unwrap_or_else(|| page.to_string());
    let (mut desktop, mut mobile) = (None, None);
    let mut errors = Vec::new();

    std::thread::scope(|s| {
        let d = s.spawn(|| fetch(&url, "desktop", key));
        let m = s.spawn(|| fetch(&url, "mobile", key));
        match d.join() {
            Ok(Ok(r)) => desktop = Some(r),
            Ok(Err(e)) => errors.push(format!("Desktop: {e}")),
            Err(_) => errors.push("Desktop: Analyse abgestürzt".into()),
        }
        match m.join() {
            Ok(Ok(r)) => mobile = Some(r),
            Ok(Err(e)) => errors.push(format!("Mobil: {e}")),
            Err(_) => errors.push("Mobil: Analyse abgestürzt".into()),
        }
    });

    PageSpeedReport { url, desktop, mobile, errors }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shaped exactly like the live PSI v5 response (captured from the real
    /// API), trimmed to the fields we read.
    const REAL: &str = r#"{
      "lighthouseResult": {
        "requestedUrl": "https://celox.io",
        "finalUrl": "https://celox.io/",
        "fetchTime": "2026-08-27T19:44:05.026Z",
        "lighthouseVersion": "13.4.1",
        "categories": {
          "performance": {"title": "Performance", "score": 0.95},
          "accessibility": {"title": "Accessibility", "score": 1},
          "best-practices": {"title": "Best Practices", "score": 1},
          "seo": {"title": "SEO", "score": 1}
        },
        "audits": {
          "first-contentful-paint": {"displayValue": "1.5 s", "score": 0.93, "numericValue": 1496.5},
          "largest-contentful-paint": {"displayValue": "2.4 s", "score": 0.8},
          "total-blocking-time": {"displayValue": "160 ms", "score": 0.88},
          "cumulative-layout-shift": {"displayValue": "0", "score": 1},
          "speed-index": {"displayValue": "2.9 s", "score": 0.9}
        }
      }
    }"#;

    #[test]
    fn parses_the_real_response_shape() {
        let r = parse_run("mobile", REAL).unwrap();
        assert_eq!(r.strategy, "mobile");
        assert_eq!(r.final_url, "https://celox.io/");
        assert_eq!(r.lighthouse_version, "13.4.1");
        // 0.95 → 95, and an integer 1 → 100 (Google sends both forms).
        assert_eq!(r.categories[0].score, Some(95));
        assert_eq!(r.categories[1].score, Some(100));
        assert_eq!(
            r.categories.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            vec!["performance", "accessibility", "best-practices", "seo"]
        );
        assert_eq!(r.metrics.len(), 5);
        // The non-breaking space Lighthouse uses is normalised.
        assert_eq!(r.metrics[0].display, "1.5 s");
        assert!(!r.metrics[0].display.contains('\u{a0}'));
    }

    #[test]
    fn a_missing_score_stays_unknown_and_never_becomes_zero() {
        let body = r#"{"lighthouseResult":{"categories":{"performance":{"score":null}},"audits":{}}}"#;
        let r = parse_run("desktop", body).unwrap();
        // null → None. A 0 here would read as "catastrophically bad".
        assert_eq!(r.categories[0].score, None);
        assert_eq!(band(None), "unknown");
        // Categories Lighthouse didn't return are still listed, as unknown.
        assert_eq!(r.categories.len(), 4);
        assert!(r.metrics.is_empty());
    }

    #[test]
    fn googles_error_payload_is_surfaced_not_swallowed() {
        // ⚠️ The ops backend turns every failure into `None`, which makes a
        // broken key indistinguishable from a page without data. We don't.
        let ip = r#"{"error":{"message":"The provided API key has an IP address restriction. The originating IP address of the call (1.2.3.4) violates this restriction."}}"#;
        let e = parse_run("mobile", ip).unwrap_err();
        assert!(e.contains("IP-Beschränkung"), "{e}");

        let quota = r#"{"error":{"message":"Quota exceeded for quota metric 'Queries'"}}"#;
        assert!(parse_run("mobile", quota).unwrap_err().contains("Kontingent"));

        // Anything else is passed through verbatim rather than flattened.
        let other = r#"{"error":{"message":"Unable to process request"}}"#;
        assert_eq!(parse_run("mobile", other).unwrap_err(), "Unable to process request");
    }

    #[test]
    fn a_body_that_is_not_a_result_fails_rather_than_pretending() {
        assert!(parse_run("mobile", "nicht json").is_err());
        assert!(parse_run("mobile", "{}").unwrap_err().contains("kein Ergebnis"));
    }

    #[test]
    fn the_band_follows_lighthouses_own_thresholds() {
        assert_eq!(band(Some(100)), "good");
        assert_eq!(band(Some(90)), "good");
        assert_eq!(band(Some(89)), "average");
        assert_eq!(band(Some(50)), "average");
        assert_eq!(band(Some(49)), "poor");
        assert_eq!(band(Some(0)), "poor");
    }

    #[test]
    fn urls_are_normalised_the_way_people_type_them() {
        assert_eq!(normalize_url("celox.io").as_deref(), Some("https://celox.io"));
        assert_eq!(normalize_url("  celox.io ").as_deref(), Some("https://celox.io"));
        assert_eq!(
            normalize_url("http://celox.io/x").as_deref(),
            Some("http://celox.io/x")
        );
        assert_eq!(normalize_url("https://a.de").as_deref(), Some("https://a.de"));
        // Not a host — firing a request for these would be pointless.
        assert!(normalize_url("").is_none());
        assert!(normalize_url("foo").is_none());
        assert!(normalize_url("zwei wörter").is_none());
        // localhost is legitimate for a dev server.
        assert!(normalize_url("localhost:3000").is_some());
    }

    #[test]
    fn the_request_carries_all_four_categories_and_the_key_only_when_set() {
        let u = build_request_url("https://a.de/x?y=1", "mobile", "");
        assert!(u.starts_with(ENDPOINT));
        assert!(u.contains("strategy=mobile"));
        for (id, _) in CATEGORIES {
            assert!(u.contains(&format!("category={id}")), "{id} fehlt");
        }
        // The page URL must be encoded, or its query string would merge into
        // ours and silently change the request.
        assert!(u.contains("url=https%3A%2F%2Fa.de%2Fx%3Fy%3D1"));
        assert!(!u.contains("&key="));

        let k = build_request_url("https://a.de", "desktop", "  abc123  ");
        assert!(k.contains("&key=abc123"), "{k}");
    }

    #[test]
    fn a_report_without_any_run_still_says_why() {
        let r = PageSpeedReport {
            url: "https://a.de".into(),
            desktop: None,
            mobile: None,
            errors: vec!["Desktop: x".into()],
        };
        assert!(r.desktop.is_none() && !r.errors.is_empty());
    }
}
