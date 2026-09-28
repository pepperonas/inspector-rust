//! Exchange rates for the `convert` / `cv` command.
//!
//! Two keyless sources:
//! * **ECB reference rates** via frankfurter (`api.frankfurter.dev`) — about
//!   thirty currencies, published on working days around 16:00 CET.
//! * **Bitcoin / Ether** via CoinGecko's public `simple/price` endpoint.
//!
//! Only the rate URLs leave the machine — never an amount. Results are cached
//! in the `settings` table (`fx.cache`, JSON): the ECB set for 6 h, crypto for
//! 10 min. When a refresh fails, the last cached set is returned with
//! `stale: true` and the error text, so the UI can say how old the numbers are
//! instead of silently converting nothing.
//!
//! The frontend gets every rate as **EUR value of one unit** (`eur_per`):
//! `EUR = 1`, `USD = 1 / 1.08…`, `BTC = 73 319`. One convention for fiat and
//! crypto keeps the conversion maths in `units.ts` a single multiplication.

use std::collections::BTreeMap;
use std::io::Read;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::DbHandle;

const ECB_URL: &str = "https://api.frankfurter.dev/v1/latest?base=EUR";
const CRYPTO_URL: &str =
    "https://api.coingecko.com/api/v3/simple/price?ids=bitcoin,ethereum&vs_currencies=eur";
const HTTP_TIMEOUT: Duration = Duration::from_secs(6);
const CACHE_KEY: &str = "fx.cache";
pub const ECB_TTL_MS: i64 = 6 * 60 * 60 * 1000;
pub const CRYPTO_TTL_MS: i64 = 10 * 60 * 1000;

/// CoinGecko id → the code the frontend registry uses.
const CRYPTO_IDS: &[(&str, &str)] = &[("bitcoin", "BTC"), ("ethereum", "ETH")];

/// What is persisted between runs.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Cache {
    /// ECB publication date, e.g. `2026-09-25`.
    pub ecb_date: Option<String>,
    /// Units of each currency per ONE euro (as the ECB publishes them).
    pub ecb: BTreeMap<String, f64>,
    pub ecb_fetched_ms: Option<i64>,
    /// EUR price of one coin.
    pub crypto: BTreeMap<String, f64>,
    pub crypto_fetched_ms: Option<i64>,
}

/// What the frontend receives.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct FxRates {
    /// EUR value of one unit of each currency (EUR itself = 1).
    pub eur_per: BTreeMap<String, f64>,
    pub ecb_date: Option<String>,
    pub ecb_fetched_ms: Option<i64>,
    pub crypto_fetched_ms: Option<i64>,
    /// A refresh failed and (some) numbers come from an older cache.
    pub stale: bool,
    pub error: Option<String>,
}

/// ECB publication date + units per euro.
pub type EcbRates = (String, BTreeMap<String, f64>);

pub fn parse_frankfurter(body: &str) -> Result<EcbRates, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("ECB: invalid response: {e}"))?;
    let date = v
        .get("date")
        .and_then(Value::as_str)
        .ok_or("ECB: response has no date")?
        .to_string();
    let rates = v
        .get("rates")
        .and_then(Value::as_object)
        .ok_or("ECB: response has no rates")?;
    let mut out = BTreeMap::new();
    for (code, r) in rates {
        if let Some(x) = r.as_f64().filter(|x| x.is_finite() && *x > 0.0) {
            out.insert(code.to_ascii_uppercase(), x);
        }
    }
    if out.is_empty() {
        return Err("ECB: response has no usable rates".into());
    }
    Ok((date, out))
}

pub fn parse_coingecko(body: &str) -> Result<BTreeMap<String, f64>, String> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| format!("CoinGecko: invalid response: {e}"))?;
    let mut out = BTreeMap::new();
    for (id, code) in CRYPTO_IDS {
        if let Some(x) = v
            .get(*id)
            .and_then(|c| c.get("eur"))
            .and_then(Value::as_f64)
            .filter(|x| x.is_finite() && *x > 0.0)
        {
            out.insert((*code).to_string(), x);
        }
    }
    if out.is_empty() {
        // CoinGecko answers a rate limit with a JSON object that has no prices.
        return Err("CoinGecko: response has no prices (rate limited?)".into());
    }
    Ok(out)
}

/// A timestamp is fresh while it is younger than `ttl`. A timestamp in the
/// future (clock moved back) counts as stale, so the cache self-heals.
pub fn is_fresh(now_ms: i64, fetched_ms: Option<i64>, ttl_ms: i64) -> bool {
    match fetched_ms {
        Some(t) => t <= now_ms && now_ms - t < ttl_ms,
        None => false,
    }
}

/// The frontend view of a cache: every rate as EUR per unit.
pub fn to_rates(cache: &Cache, error: Option<String>) -> FxRates {
    let mut eur_per = BTreeMap::new();
    let have_any = !cache.ecb.is_empty() || !cache.crypto.is_empty();
    if have_any {
        eur_per.insert("EUR".to_string(), 1.0);
    }
    for (code, per_eur) in &cache.ecb {
        eur_per.insert(code.clone(), 1.0 / per_eur);
    }
    for (code, eur) in &cache.crypto {
        eur_per.insert(code.clone(), *eur);
    }
    FxRates {
        eur_per,
        ecb_date: cache.ecb_date.clone(),
        ecb_fetched_ms: cache.ecb_fetched_ms,
        crypto_fetched_ms: cache.crypto_fetched_ms,
        stale: error.is_some() && have_any,
        error,
    }
}

/// Fold one refresh round into the cache: a success replaces its half, a
/// failure keeps the old half and contributes its message. Pure.
pub fn apply_refresh(
    mut cache: Cache,
    now_ms: i64,
    ecb: Option<Result<EcbRates, String>>,
    crypto: Option<Result<BTreeMap<String, f64>, String>>,
) -> (Cache, Option<String>) {
    let mut errors = Vec::new();
    match ecb {
        Some(Ok((date, rates))) => {
            cache.ecb_date = Some(date);
            cache.ecb = rates;
            cache.ecb_fetched_ms = Some(now_ms);
        }
        Some(Err(e)) => errors.push(e),
        None => {}
    }
    match crypto {
        Some(Ok(prices)) => {
            cache.crypto = prices;
            cache.crypto_fetched_ms = Some(now_ms);
        }
        Some(Err(e)) => errors.push(e),
        None => {}
    }
    let err = if errors.is_empty() { None } else { Some(errors.join(" · ")) };
    (cache, err)
}

fn http_get(url: &str) -> Result<String, String> {
    let response = ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        .build()
        .get(url)
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("HTTP {code}"),
            other => format!("offline? ({other})"),
        })?;
    let mut body = String::new();
    response
        .into_reader()
        .take(256 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| format!("read failed: {e}"))?;
    Ok(body)
}

fn load_cache(db: &DbHandle) -> Cache {
    crate::settings::get(db, CACHE_KEY)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Current rates: cached halves that are still fresh are reused, the rest is
/// fetched (`force` refetches both). Blocking — call from `spawn_blocking`.
pub fn get(db: &DbHandle, force: bool) -> FxRates {
    let now = now_ms();
    let cache = load_cache(db);
    let need_ecb = force || !is_fresh(now, cache.ecb_fetched_ms, ECB_TTL_MS);
    let need_crypto = force || !is_fresh(now, cache.crypto_fetched_ms, CRYPTO_TTL_MS);
    if !need_ecb && !need_crypto {
        return to_rates(&cache, None);
    }
    let ecb = need_ecb.then(|| {
        http_get(ECB_URL)
            .map_err(|e| format!("ECB: {e}"))
            .and_then(|b| parse_frankfurter(&b))
    });
    let crypto = need_crypto.then(|| {
        http_get(CRYPTO_URL)
            .map_err(|e| format!("CoinGecko: {e}"))
            .and_then(|b| parse_coingecko(&b))
    });
    let (cache, err) = apply_refresh(cache, now, ecb, crypto);
    if let Some(e) = &err {
        tracing::warn!("fx_rates: {e}");
    }
    if let Ok(json) = serde_json::to_string(&cache) {
        let _ = crate::settings::set(db, CACHE_KEY, &json);
    }
    to_rates(&cache, err)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from the live APIs on 2026-09-28 (trimmed).
    const ECB: &str = r#"{"amount":1.0,"base":"EUR","date":"2026-09-25","rates":{"CHF":0.9445,"GBP":0.86045,"JPY":179.7,"USD":1.1712}}"#;
    const CG: &str = r#"{"bitcoin":{"eur":73319,"last_updated_at":1790597440},"ethereum":{"eur":2361.43,"last_updated_at":1790597440}}"#;

    #[test]
    fn parses_the_ecb_response() {
        let (date, rates) = parse_frankfurter(ECB).unwrap();
        assert_eq!(date, "2026-09-25");
        assert_eq!(rates.get("USD"), Some(&1.1712));
        assert_eq!(rates.len(), 4);
    }

    #[test]
    fn parses_the_coingecko_response() {
        let p = parse_coingecko(CG).unwrap();
        assert_eq!(p.get("BTC"), Some(&73319.0));
        assert_eq!(p.get("ETH"), Some(&2361.43));
    }

    #[test]
    fn rejects_empty_and_broken_responses() {
        assert!(parse_frankfurter("nope").is_err());
        assert!(parse_frankfurter(r#"{"date":"2026-09-25","rates":{}}"#).is_err());
        assert!(parse_frankfurter(r#"{"rates":{"USD":1.1}}"#).is_err());
        // CoinGecko's rate-limit answer carries no prices.
        assert!(parse_coingecko(r#"{"status":{"error_code":429}}"#).is_err());
        // Zero / negative rates are dropped, never divided by.
        let (_, r) = parse_frankfurter(r#"{"date":"d","rates":{"USD":0,"GBP":0.86}}"#).unwrap();
        assert!(!r.contains_key("USD"));
    }

    #[test]
    fn freshness() {
        assert!(is_fresh(1_000, Some(500), 600));
        assert!(!is_fresh(1_200, Some(500), 600));
        assert!(!is_fresh(1_000, None, 600));
        assert!(!is_fresh(1_000, Some(5_000), 600)); // clock moved back
    }

    #[test]
    fn rates_are_eur_per_unit_for_fiat_and_crypto() {
        let (cache, err) = apply_refresh(
            Cache::default(),
            42,
            Some(parse_frankfurter(ECB)),
            Some(parse_coingecko(CG)),
        );
        assert!(err.is_none());
        let r = to_rates(&cache, err);
        assert_eq!(r.eur_per["EUR"], 1.0);
        assert!((r.eur_per["USD"] - 1.0 / 1.1712).abs() < 1e-12);
        assert_eq!(r.eur_per["BTC"], 73319.0);
        assert_eq!(r.ecb_date.as_deref(), Some("2026-09-25"));
        assert!(!r.stale);
    }

    #[test]
    fn a_failed_refresh_keeps_the_old_numbers_and_says_so() {
        let (old, _) = apply_refresh(Cache::default(), 1, Some(parse_frankfurter(ECB)), None);
        let (cache, err) =
            apply_refresh(old.clone(), 2, Some(Err("ECB: offline?".into())), None);
        assert_eq!(cache.ecb, old.ecb);
        assert_eq!(cache.ecb_fetched_ms, Some(1)); // not bumped by the failure
        let r = to_rates(&cache, err);
        assert!(r.stale);
        assert_eq!(r.error.as_deref(), Some("ECB: offline?"));
        assert!(r.eur_per.contains_key("USD"));
    }

    #[test]
    fn a_failure_without_any_cache_is_an_error_not_stale() {
        let (cache, err) = apply_refresh(Cache::default(), 1, Some(Err("ECB: HTTP 500".into())), None);
        let r = to_rates(&cache, err);
        assert!(!r.stale);
        assert!(r.eur_per.is_empty(), "no EUR anchor without any rate");
        assert!(r.error.is_some());
    }

    #[test]
    fn untouched_halves_stay_as_they_were() {
        let (old, _) = apply_refresh(Cache::default(), 1, Some(parse_frankfurter(ECB)), None);
        let (cache, _) = apply_refresh(old.clone(), 9, None, Some(parse_coingecko(CG)));
        assert_eq!(cache.ecb_fetched_ms, Some(1));
        assert_eq!(cache.crypto_fetched_ms, Some(9));
    }
}
