//! Live text translation for the `tr*` search-bar commands.
//!
//! A tiny **provider (strategy) abstraction** over keyless, free translation
//! endpoints so more engines (LibreTranslate, DeepL, …) can be added later
//! without touching the caller: the unofficial **Google gtx** endpoint is the
//! primary (same engine as the "open Google Translate in the browser"
//! command, so the quality — and the privacy trade-off — are identical), and
//! **MyMemory** is an automatic fallback when Google is unreachable. Both are
//! keyless and free; only the text + language pair leave the machine.
//!
//! The browser-open command (`translateUrl` in the frontend) remains the
//! ultimate fallback and is never removed — this module only powers the *live
//! preview*. Blocking (`ureq`); call from a `spawn_blocking` task.
//!
//! The response **parsers are pure + unit-tested**; the HTTP calls need the
//! network and are not.

use std::time::Duration;

/// A successful translation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Translation {
    /// The translated text.
    pub text: String,
    /// Detected source language (may be empty when the provider doesn't report
    /// one — e.g. MyMemory, or Google without an `auto` source).
    pub detected_source: String,
    /// Which provider produced it (`"google"` / `"mymemory"`) — for the UI hint
    /// + logging.
    pub provider: String,
}

/// Per-provider request timeout — short, because this drives a *live* preview
/// (the frontend also debounces). A timeout just falls through to the next
/// provider, and ultimately to the browser-open fallback.
const TIMEOUT: Duration = Duration::from_millis(2500);

/// A translation backend. New engines implement this + are added to
/// [`translate`]'s ordered list.
trait Provider {
    fn name(&self) -> &'static str;
    /// Whether the provider can handle this source language. Some (MyMemory)
    /// need a concrete source and can't do `"auto"`.
    fn supports_source(&self, sl: &str) -> bool;
    fn translate(&self, text: &str, sl: &str, tl: &str, timeout: Duration) -> Result<Translation, String>;
}

// ── Google (unofficial gtx endpoint, keyless) ────────────────────────────────

struct GoogleGtx;

impl Provider for GoogleGtx {
    fn name(&self) -> &'static str {
        "google"
    }
    fn supports_source(&self, _sl: &str) -> bool {
        true // handles "auto" too
    }
    fn translate(&self, text: &str, sl: &str, tl: &str, timeout: Duration) -> Result<Translation, String> {
        let resp = ureq::get("https://translate.googleapis.com/translate_a/single")
            .query("client", "gtx")
            .query("sl", sl)
            .query("tl", tl)
            .query("dt", "t")
            .query("q", text)
            .timeout(timeout)
            .call()
            .map_err(|e| format!("google: {e}"))?;
        let body = resp.into_string().map_err(|e| format!("google read: {e}"))?;
        parse_google(&body).ok_or_else(|| "google: unparseable response".to_string())
    }
}

/// Parse the Google gtx response: a nested array whose `[0]` is a list of
/// sentence segments `[translated, original, …]`; the full translation is the
/// concatenation of every `translated` chunk. `[2]` is the detected source
/// language. Pure + unit-tested.
pub fn parse_google(body: &str) -> Option<Translation> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let sentences = v.get(0)?.as_array()?;
    let mut text = String::new();
    for s in sentences {
        if let Some(chunk) = s.as_array().and_then(|a| a.first()).and_then(|x| x.as_str()) {
            text.push_str(chunk);
        }
    }
    if text.trim().is_empty() {
        return None;
    }
    let detected_source = v.get(2).and_then(|x| x.as_str()).unwrap_or("").to_string();
    Some(Translation { text, detected_source, provider: "google".into() })
}

// ── MyMemory (keyless fallback) ──────────────────────────────────────────────

struct MyMemory;

impl Provider for MyMemory {
    fn name(&self) -> &'static str {
        "mymemory"
    }
    fn supports_source(&self, sl: &str) -> bool {
        sl != "auto" // needs a concrete source language
    }
    fn translate(&self, text: &str, sl: &str, tl: &str, timeout: Duration) -> Result<Translation, String> {
        let resp = ureq::get("https://api.mymemory.translated.net/get")
            .query("q", text)
            .query("langpair", &format!("{sl}|{tl}"))
            .timeout(timeout)
            .call()
            .map_err(|e| format!("mymemory: {e}"))?;
        let body = resp.into_string().map_err(|e| format!("mymemory read: {e}"))?;
        parse_mymemory(&body).ok_or_else(|| "mymemory: unparseable response".to_string())
    }
}

/// Parse the MyMemory response (`{responseData:{translatedText},responseStatus}`).
/// `responseStatus` may be a number (200) or a string ("200"); anything else
/// (e.g. a quota message) yields `None`. Pure + unit-tested.
pub fn parse_mymemory(body: &str) -> Option<Translation> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let ok = v
        .get("responseStatus")
        .map(|s| s.as_i64() == Some(200) || s.as_str() == Some("200"))
        .unwrap_or(false);
    if !ok {
        return None;
    }
    let text = v.get("responseData")?.get("translatedText")?.as_str()?.to_string();
    if text.trim().is_empty() {
        return None;
    }
    Some(Translation { text, detected_source: String::new(), provider: "mymemory".into() })
}

// ── Orchestrator ─────────────────────────────────────────────────────────────

/// Translate `text` from `sl` → `tl`, trying each provider in order until one
/// succeeds (Google gtx, then MyMemory). Returns the last error if all fail —
/// the caller then relies on the browser-open fallback. Blocking.
pub fn translate(text: &str, sl: &str, tl: &str) -> Result<Translation, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("empty text".into());
    }
    let providers: [&dyn Provider; 2] = [&GoogleGtx, &MyMemory];
    let mut last_err = String::from("no provider available");
    for p in providers {
        if !p.supports_source(sl) {
            continue;
        }
        match p.translate(text, sl, tl, TIMEOUT) {
            Ok(t) => {
                tracing::debug!(
                    "translate {sl}->{tl} via {}: {} chars",
                    t.provider,
                    t.text.chars().count()
                );
                return Ok(t);
            }
            Err(e) => {
                tracing::info!("translate: provider {} failed: {e}", p.name());
                last_err = e;
            }
        }
    }
    Err(last_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_google_joins_sentence_segments_and_reads_source() {
        // Two segments → concatenated; detected source "en".
        let body = r#"[[["Verbesserung ","enhancement ",null,null,1],["des Codes","of the code",null,null,1]],null,"en"]"#;
        let t = parse_google(body).expect("parses");
        assert_eq!(t.text, "Verbesserung des Codes");
        assert_eq!(t.detected_source, "en");
        assert_eq!(t.provider, "google");
    }

    #[test]
    fn parse_google_rejects_garbage_and_empty() {
        assert!(parse_google("not json").is_none());
        assert!(parse_google("[]").is_none());
        assert!(parse_google(r#"[[],null,"en"]"#).is_none()); // no segments
        assert!(parse_google(r#"[[["   ","x"]],null,"en"]"#).is_none()); // whitespace only
    }

    #[test]
    fn parse_google_tolerates_a_missing_source_lang() {
        let body = r#"[[["Hallo","Hi"]]]"#;
        let t = parse_google(body).expect("parses");
        assert_eq!(t.text, "Hallo");
        assert_eq!(t.detected_source, "");
    }

    #[test]
    fn parse_mymemory_reads_translated_text_on_status_200() {
        // status as a number and as a string both count as OK.
        for status in ["200", "\"200\""] {
            let body = format!(
                r#"{{"responseData":{{"translatedText":"Verbesserung"}},"responseStatus":{status}}}"#
            );
            let t = parse_mymemory(&body).expect("parses");
            assert_eq!(t.text, "Verbesserung");
            assert_eq!(t.provider, "mymemory");
        }
    }

    #[test]
    fn parse_mymemory_rejects_non_200_and_garbage() {
        assert!(parse_mymemory("nope").is_none());
        assert!(parse_mymemory(r#"{"responseData":{"translatedText":"x"},"responseStatus":429}"#).is_none());
        assert!(parse_mymemory(r#"{"responseStatus":200}"#).is_none()); // no responseData
        assert!(parse_mymemory(r#"{"responseData":{"translatedText":"  "},"responseStatus":200}"#).is_none());
    }
}
