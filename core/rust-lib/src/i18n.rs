//! App language — English / German.
//!
//! ONE catalogue for the whole app: `core/frontend/src/locales/{en,de}.json`.
//! The frontend loads it for its own text; Rust embeds the very same files
//! for the text it produces itself (tray menu, toasts, notifications, native
//! dialogs, generated reports). Two lists would drift — the parity tests in
//! both languages' test suites pin that every key exists in both files.
//!
//! The preference is `appearance.language` = `system` | `en` | `de`
//! (`system` = follow the OS, the default). The EFFECTIVE language lives in an
//! atomic so `t()` is a lock-free read from any thread.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

const EN_JSON: &str = include_str!("../../frontend/src/locales/en.json");
const DE_JSON: &str = include_str!("../../frontend/src/locales/de.json");

pub const KEY_LANGUAGE: &str = "appearance.language";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    En,
    De,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
        }
    }
}

/// Coerce a stored preference to one of the three valid values. Unknown
/// values (a hand-edited DB, a future build's value) fall back to `system`.
pub fn normalise_setting(s: &str) -> &'static str {
    match s.trim() {
        "en" => "en",
        "de" => "de",
        _ => "system",
    }
}

/// The language a preference resolves to, given the OS language.
pub fn resolve(setting: &str, system: Lang) -> Lang {
    match normalise_setting(setting) {
        "en" => Lang::En,
        "de" => Lang::De,
        _ => system,
    }
}

/// Map an OS locale identifier (`de-DE`, `de_AT.UTF-8`, `en`, …) to a
/// supported language. Anything that is not German reads English — the
/// app's second language is German, English is the fallback for the world.
pub fn lang_from_locale(id: &str) -> Lang {
    let id = id.trim().to_ascii_lowercase();
    if id == "de" || id.starts_with("de-") || id.starts_with("de_") || id.starts_with("de.") {
        Lang::De
    } else {
        Lang::En
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0); // 0 = En, 1 = De

pub fn current() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == 1 {
        Lang::De
    } else {
        Lang::En
    }
}

pub fn set_current(lang: Lang) {
    CURRENT.store(if lang == Lang::De { 1 } else { 0 }, Ordering::Relaxed);
}

fn parse(json: &str) -> HashMap<String, String> {
    serde_json::from_str(json).expect("locale catalogue must be a flat JSON object of strings")
}

fn catalogue(lang: Lang) -> &'static HashMap<String, String> {
    static EN: OnceLock<HashMap<String, String>> = OnceLock::new();
    static DE: OnceLock<HashMap<String, String>> = OnceLock::new();
    match lang {
        Lang::En => EN.get_or_init(|| parse(EN_JSON)),
        Lang::De => DE.get_or_init(|| parse(DE_JSON)),
    }
}

/// Look up `key` in a given language: that language, then English, then the
/// key itself (a missing key is a test failure, never a crash).
pub fn t_in(lang: Lang, key: &str) -> String {
    catalogue(lang)
        .get(key)
        .or_else(|| catalogue(Lang::En).get(key))
        .cloned()
        .unwrap_or_else(|| key.to_string())
}

/// Look up `key` in the current language.
pub fn t(key: &str) -> String {
    t_in(current(), key)
}

/// `t` with `{name}` placeholders filled in.
pub fn tf(key: &str, vars: &[(&str, &str)]) -> String {
    fill(t(key), vars)
}

pub fn fill(mut s: String, vars: &[(&str, &str)]) -> String {
    for (k, v) in vars {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

/// The OS language. Best effort — any failure reads English.
pub fn system_lang() -> Lang {
    system_locale().map(|l| lang_from_locale(&l)).unwrap_or(Lang::En)
}

#[cfg(target_os = "macos")]
fn system_locale() -> Option<String> {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSString;
    // SAFETY: `NSLocale.preferredLanguages` is a class method returning an
    // autoreleased NSArray<NSString>; we only read its first element.
    unsafe {
        let cls = AnyClass::get(c"NSLocale")?;
        let langs: Option<Retained<AnyObject>> = msg_send![cls, preferredLanguages];
        let langs = langs?;
        let count: usize = msg_send![&*langs, count];
        if count == 0 {
            return None;
        }
        let first: Option<Retained<NSString>> = msg_send![&*langs, objectAtIndex: 0usize];
        first.map(|s| s.to_string())
    }
}

#[cfg(target_os = "windows")]
fn system_locale() -> Option<String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    // SAFETY: no arguments, returns a LANGID.
    let langid = unsafe { GetUserDefaultUILanguage() };
    // Primary language id 0x07 = German.
    Some(if langid & 0x3ff == 0x07 { "de".into() } else { "en".into() })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn system_locale() -> Option<String> {
    for var in ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"] {
        if let Ok(v) = std::env::var(var) {
            let v = v.split(':').next().unwrap_or("").to_string();
            if !v.is_empty() && v != "C" && v != "POSIX" {
                return Some(v);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_catalogues_have_the_same_keys() {
        let en = catalogue(Lang::En);
        let de = catalogue(Lang::De);
        let mut missing_de: Vec<_> = en.keys().filter(|k| !de.contains_key(*k)).collect();
        let mut missing_en: Vec<_> = de.keys().filter(|k| !en.contains_key(*k)).collect();
        missing_de.sort();
        missing_en.sort();
        assert!(missing_de.is_empty(), "keys missing in de.json: {missing_de:?}");
        assert!(missing_en.is_empty(), "keys missing in en.json: {missing_en:?}");
    }

    #[test]
    fn no_value_is_empty() {
        for lang in [Lang::En, Lang::De] {
            for (k, v) in catalogue(lang) {
                assert!(!v.trim().is_empty(), "{k} is empty in {}", lang.code());
            }
        }
    }

    #[test]
    fn placeholders_match_between_languages() {
        let names = |s: &str| {
            let mut v: Vec<String> = s
                .split('{')
                .skip(1)
                .filter_map(|p| p.split_once('}').map(|(n, _)| n.to_string()))
                .collect();
            v.sort();
            v
        };
        for (k, en) in catalogue(Lang::En) {
            if let Some(de) = catalogue(Lang::De).get(k) {
                assert_eq!(names(en), names(de), "placeholders differ for {k}");
            }
        }
    }

    #[test]
    fn locale_mapping() {
        for de in ["de", "de-DE", "de_AT.UTF-8", "DE-ch", "de.UTF-8"] {
            assert_eq!(lang_from_locale(de), Lang::De, "{de}");
        }
        for en in ["en", "en-US", "fr-FR", "", "dee", "C"] {
            assert_eq!(lang_from_locale(en), Lang::En, "{en}");
        }
    }

    #[test]
    fn settings_resolve() {
        assert_eq!(normalise_setting("klingon"), "system");
        assert_eq!(resolve("system", Lang::De), Lang::De);
        assert_eq!(resolve("system", Lang::En), Lang::En);
        assert_eq!(resolve("en", Lang::De), Lang::En);
        assert_eq!(resolve("de", Lang::En), Lang::De);
        assert_eq!(resolve("", Lang::De), Lang::De);
    }

    #[test]
    fn lookup_falls_back_to_english_then_the_key() {
        assert_eq!(t_in(Lang::De, "tray.quit"), "Inspector Rust beenden");
        assert_eq!(t_in(Lang::En, "tray.quit"), "Quit Inspector Rust");
        assert_eq!(t_in(Lang::De, "no.such.key"), "no.such.key");
        assert_eq!(fill("Open ({hotkey})".into(), &[("hotkey", "⌃Space")]), "Open (⌃Space)");
    }
}
