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
    translate_with(&[&GoogleGtx, &MyMemory], text, sl, tl, TIMEOUT)
}

/// The provider-independent half of [`translate`]: input validation, the
/// try-in-order fallback and the error that survives when everything failed.
///
/// Split out so the orchestration can be unit-tested with stub providers — the
/// real ones need the network, but *which* provider gets asked, in what order,
/// and what happens when one is unavailable is exactly the logic worth pinning
/// down.
fn translate_with(
    providers: &[&dyn Provider],
    text: &str,
    sl: &str,
    tl: &str,
    timeout: Duration,
) -> Result<Translation, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("empty text".into());
    }
    let mut last_err = String::from("no provider available");
    for p in providers {
        if !p.supports_source(sl) {
            continue;
        }
        match p.translate(text, sl, tl, timeout) {
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
    use std::cell::Cell;

    /// A scripted provider: records whether it was asked, and answers with a
    /// canned success or failure. Lets the fallback order be asserted without
    /// touching the network.
    struct Stub {
        name: &'static str,
        /// `false` → the orchestrator must skip it entirely (the MyMemory
        /// "can't do `auto`" case).
        handles_auto: bool,
        ok: bool,
        asked: Cell<bool>,
    }

    impl Stub {
        fn new(name: &'static str, ok: bool) -> Self {
            Self { name, handles_auto: true, ok, asked: Cell::new(false) }
        }
        fn no_auto(name: &'static str, ok: bool) -> Self {
            Self { name, handles_auto: false, ok, asked: Cell::new(false) }
        }
    }

    impl Provider for Stub {
        fn name(&self) -> &'static str {
            self.name
        }
        fn supports_source(&self, sl: &str) -> bool {
            self.handles_auto || sl != "auto"
        }
        fn translate(
            &self,
            text: &str,
            _sl: &str,
            _tl: &str,
            _t: Duration,
        ) -> Result<Translation, String> {
            self.asked.set(true);
            if self.ok {
                Ok(Translation {
                    text: format!("{text} [{}]", self.name),
                    detected_source: String::new(),
                    provider: self.name.into(),
                })
            } else {
                Err(format!("{} exploded", self.name))
            }
        }
    }

    const T: Duration = Duration::from_millis(10);

    #[test]
    fn the_first_provider_wins_and_the_rest_are_never_asked() {
        let first = Stub::new("first", true);
        let second = Stub::new("second", true);
        let out = translate_with(&[&first, &second], "hi", "en", "de", T).unwrap();
        assert_eq!(out.provider, "first");
        assert!(first.asked.get());
        assert!(!second.asked.get(), "the fallback must stay unused on success");
    }

    #[test]
    fn a_failing_provider_falls_through_to_the_next() {
        let broken = Stub::new("broken", false);
        let backup = Stub::new("backup", true);
        let out = translate_with(&[&broken, &backup], "hi", "en", "de", T).unwrap();
        assert_eq!(out.provider, "backup");
        assert!(broken.asked.get() && backup.asked.get());
    }

    #[test]
    fn a_provider_that_cannot_handle_the_source_is_skipped_not_failed() {
        // `trauto` sends sl="auto"; MyMemory needs a concrete language, so it
        // must be passed over rather than asked and counted as a failure.
        let picky = Stub::no_auto("picky", true);
        let general = Stub::new("general", true);
        let out = translate_with(&[&picky, &general], "hi", "auto", "de", T).unwrap();
        assert_eq!(out.provider, "general");
        assert!(!picky.asked.get(), "an unsupported source must not be attempted");

        // …but with a concrete source it is asked first again.
        let picky2 = Stub::no_auto("picky", true);
        let out2 = translate_with(&[&picky2, &general], "hi", "en", "de", T).unwrap();
        assert_eq!(out2.provider, "picky");
    }

    #[test]
    fn the_last_error_survives_when_every_provider_failed() {
        // The caller shows the browser fallback on `Err`, so the message has to
        // describe the final attempt rather than a generic placeholder.
        let a = Stub::new("a", false);
        let b = Stub::new("b", false);
        let err = translate_with(&[&a, &b], "hi", "en", "de", T).unwrap_err();
        assert_eq!(err, "b exploded");
    }

    #[test]
    fn no_usable_provider_still_reports_an_error() {
        let picky = Stub::no_auto("picky", true);
        let err = translate_with(&[&picky], "hi", "auto", "de", T).unwrap_err();
        assert!(!err.is_empty());
        assert!(!picky.asked.get());
    }

    #[test]
    fn blank_input_is_rejected_before_any_provider_is_contacted() {
        // The frontend debounces per keystroke — whitespace must never become a
        // network request.
        for blank in ["", "   ", "\n\t "] {
            let p = Stub::new("p", true);
            assert!(translate_with(&[&p], blank, "en", "de", T).is_err());
            assert!(!p.asked.get(), "blank input must not reach a provider");
        }
    }

    #[test]
    fn the_text_is_trimmed_before_being_sent() {
        let p = Stub::new("p", true);
        let out = translate_with(&[&p], "  hello  ", "en", "de", T).unwrap();
        assert_eq!(out.text, "hello [p]");
    }

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
