//! Locale model for the faker command / expander.
//!
//! `fake`'s locales are **compile-time types** (`Name(DE_DE)` vs `Name(EN)`),
//! not runtime values, so we carry a runtime [`Locale`] enum and dispatch to
//! the typed faker via the `localized!` macro (see `registry.rs`).
//!
//! **Honest locale support (verified against the fake-rs source, not guessed):**
//! `fake` inherits unspecified data from EN via trait defaults, so *every*
//! faker compiles for *every* locale but silently returns English data where a
//! locale has no override. We therefore record, per generator, the locales that
//! actually localise it, and [`resolve`] collapses any unsupported request to
//! EN — surfaced to the UI as a fallback (never a silent lie).

use serde::{Deserialize, Serialize};

/// The 14 locales `fake` 5.1 ships (verified from `fake/src/locales/`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Locale {
    En,
    DeDe,
    FrFr,
    ItIt,
    PtBr,
    PtPt,
    NlNl,
    JaJp,
    ZhCn,
    ZhTw,
    ArSa,
    CyGb,
    FaIr,
    TrTr,
}

impl Locale {
    pub const ALL: [Locale; 14] = [
        Locale::En,
        Locale::DeDe,
        Locale::FrFr,
        Locale::ItIt,
        Locale::PtBr,
        Locale::PtPt,
        Locale::NlNl,
        Locale::JaJp,
        Locale::ZhCn,
        Locale::ZhTw,
        Locale::ArSa,
        Locale::CyGb,
        Locale::FaIr,
        Locale::TrTr,
    ];

    /// Canonical code (matches the `@flag` long form + the fake locale name).
    pub fn code(self) -> &'static str {
        match self {
            Locale::En => "EN",
            Locale::DeDe => "DE_DE",
            Locale::FrFr => "FR_FR",
            Locale::ItIt => "IT_IT",
            Locale::PtBr => "PT_BR",
            Locale::PtPt => "PT_PT",
            Locale::NlNl => "NL_NL",
            Locale::JaJp => "JA_JP",
            Locale::ZhCn => "ZH_CN",
            Locale::ZhTw => "ZH_TW",
            Locale::ArSa => "AR_SA",
            Locale::CyGb => "CY_GB",
            Locale::FaIr => "FA_IR",
            Locale::TrTr => "TR_TR",
        }
    }

    /// Human label for the Settings dropdown.
    pub fn label(self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::DeDe => "Deutsch",
            Locale::FrFr => "Français",
            Locale::ItIt => "Italiano",
            Locale::PtBr => "Português (Brasil)",
            Locale::PtPt => "Português (Portugal)",
            Locale::NlNl => "Nederlands",
            Locale::JaJp => "日本語",
            Locale::ZhCn => "简体中文",
            Locale::ZhTw => "繁體中文",
            Locale::ArSa => "العربية",
            Locale::CyGb => "Cymraeg",
            Locale::FaIr => "فارسی",
            Locale::TrTr => "Türkçe",
        }
    }

    /// Parse a `@flag` short or long form (case-insensitive). Accepts both the
    /// short language code (`de`, `en`, `fr`) and the full code (`de_de`,
    /// `pt_br`); ambiguous shorts map to the primary variant (`pt`→PT_BR,
    /// `zh`→ZH_CN). Returns `None` for an unknown code.
    pub fn from_flag(s: &str) -> Option<Locale> {
        let k = s.trim().trim_start_matches('@').to_ascii_lowercase();
        Some(match k.as_str() {
            "en" | "en_us" | "en_gb" => Locale::En,
            "de" | "de_de" => Locale::DeDe,
            "fr" | "fr_fr" => Locale::FrFr,
            "it" | "it_it" => Locale::ItIt,
            "pt" | "pt_br" | "br" => Locale::PtBr,
            "pt_pt" => Locale::PtPt,
            "nl" | "nl_nl" => Locale::NlNl,
            "ja" | "jp" | "ja_jp" => Locale::JaJp,
            "zh" | "zh_cn" | "cn" => Locale::ZhCn,
            "zh_tw" | "tw" => Locale::ZhTw,
            "ar" | "ar_sa" => Locale::ArSa,
            "cy" | "cy_gb" => Locale::CyGb,
            "fa" | "fa_ir" => Locale::FaIr,
            "tr" | "tr_tr" => Locale::TrTr,
            _ => return None,
        })
    }

    /// Parse a canonical code (`DE_DE`) back to a `Locale` — the inverse of
    /// [`code`](Locale::code), used when reading the settings default.
    pub fn from_code(s: &str) -> Option<Locale> {
        Self::from_flag(s)
    }
}

/// Resolve the effective locale for a generator: the requested locale if the
/// generator actually localises it, else EN (a visible fallback). Returns
/// `(effective, fell_back)`.
pub fn resolve(requested: Locale, supported: &[Locale]) -> (Locale, bool) {
    if requested == Locale::En || supported.contains(&requested) {
        (requested, false)
    } else {
        (Locale::En, true)
    }
}

// ── Shared support sets (verified from fake-rs source) ───────────────────────

/// Names are localised in all 14 locales.
pub const LOC_NAME: &[Locale] = &Locale::ALL;

/// Address data (city/street/state/country/zip) — all but JA_JP, ZH_TW, AR_SA.
pub const LOC_ADDRESS: &[Locale] = &[
    Locale::En,
    Locale::DeDe,
    Locale::FrFr,
    Locale::ItIt,
    Locale::PtBr,
    Locale::PtPt,
    Locale::NlNl,
    Locale::ZhCn,
    Locale::CyGb,
    Locale::FaIr,
    Locale::TrTr,
];

/// Phone formats — all but DE_DE(!), ZH_CN, ZH_TW, AR_SA.
pub const LOC_PHONE: &[Locale] = &[
    Locale::En,
    Locale::FrFr,
    Locale::ItIt,
    Locale::PtBr,
    Locale::PtPt,
    Locale::NlNl,
    Locale::JaJp,
    Locale::CyGb,
    Locale::FaIr,
    Locale::TrTr,
];

/// Lorem word lists — only EN, ZH_CN, FA_IR override in fake 5.1.
pub const LOC_LOREM: &[Locale] = &[Locale::En, Locale::ZhCn, Locale::FaIr];

// (Company *suffix* is localised in most locales, but fake's `company` faker
// returns the EN company *name*, so there is no standalone localised company
// generator to attach a LOC_COMPANY set to — `company` is LOC_EN.)

/// Locale-independent generators (internet, finance, numbers, …).
pub const LOC_EN: &[Locale] = &[Locale::En];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_parsing_round_trips_codes() {
        for l in Locale::ALL {
            assert_eq!(Locale::from_code(l.code()), Some(l), "{}", l.code());
        }
    }

    #[test]
    fn short_flags_map_to_primary_variants() {
        assert_eq!(Locale::from_flag("@de"), Some(Locale::DeDe));
        assert_eq!(Locale::from_flag("pt"), Some(Locale::PtBr));
        assert_eq!(Locale::from_flag("zh"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_flag("@ZH_TW"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_flag("nonsense"), None);
    }

    #[test]
    fn resolve_falls_back_to_en_for_unsupported() {
        // Phone is NOT localised for German → falls back, flagged.
        assert_eq!(resolve(Locale::DeDe, LOC_PHONE), (Locale::En, true));
        // Names ARE localised for German → used, not flagged.
        assert_eq!(resolve(Locale::DeDe, LOC_NAME), (Locale::DeDe, false));
        // EN is always itself.
        assert_eq!(resolve(Locale::En, LOC_LOREM), (Locale::En, false));
    }
}
