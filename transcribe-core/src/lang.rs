//! Language choice (German vs. English only) and the confidence rules that
//! decide whether a decoded segment is kept.

use crate::protocol::Lang;
use crate::text::{compression_ratio, is_noise_text};

/// The likelier of German and English. Every other language is ignored:
/// short segments misdetect (an English song came back as Shona), and this
/// app only ever needs these two. A tie goes to German.
pub fn pick_de_en(probs: &[f32], de_id: usize, en_id: usize) -> &'static str {
    let de = probs.get(de_id).copied().unwrap_or(0.0);
    let en = probs.get(en_id).copied().unwrap_or(0.0);
    if en > de {
        "en"
    } else {
        "de"
    }
}

/// A forced setting wins; otherwise what was detected for this segment, then
/// what the source spoke last, then German.
pub fn resolve_lang(setting: Lang, detected: Option<&str>, last: Option<&str>) -> String {
    setting
        .forced()
        .or(detected)
        .or(last)
        .unwrap_or("de")
        .to_string()
}

/// Mean natural-log probability of a segment's tokens.
pub fn mean_logprob(token_probs: &[f32]) -> f32 {
    if token_probs.is_empty() {
        return 0.0;
    }
    token_probs.iter().map(|p| p.max(1e-6).ln()).sum::<f32>() / token_probs.len() as f32
}

pub const MIN_LOGPROB: f32 = -1.0;
pub const MAX_COMPRESSION: f32 = 2.4;
pub const NO_SPEECH: f32 = 0.6;

/// Keep or reject one decoded segment (the rules of the old Python worker,
/// measured on dictation over music: real speech scores −0.2 … −0.5).
pub fn accept(text: &str, mean_lp: f32, no_speech: f32) -> Result<(), &'static str> {
    if text.trim().is_empty() || is_noise_text(text) {
        return Err("noise");
    }
    if mean_lp < MIN_LOGPROB {
        return Err("low_confidence");
    }
    if compression_ratio(text) > MAX_COMPRESSION {
        return Err("loop");
    }
    if no_speech > NO_SPEECH && mean_lp < -0.5 {
        return Err("no_speech");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_wins_when_more_likely_even_if_shona_is_top() {
        let mut probs = vec![0.0; 100];
        probs[0] = 0.10; // en
        probs[2] = 0.05; // de
        probs[50] = 0.70; // sn
        assert_eq!(pick_de_en(&probs, 2, 0), "en");
        probs[2] = 0.2;
        assert_eq!(pick_de_en(&probs, 2, 0), "de");
        assert_eq!(pick_de_en(&[], 2, 0), "de");
    }

    #[test]
    fn forced_language_always_wins() {
        assert_eq!(resolve_lang(Lang::En, Some("de"), Some("de")), "en");
        assert_eq!(resolve_lang(Lang::De, Some("en"), None), "de");
    }

    #[test]
    fn hypothesis_uses_last_final_language() {
        assert_eq!(resolve_lang(Lang::Auto, None, Some("en")), "en");
        assert_eq!(resolve_lang(Lang::Auto, Some("de"), Some("en")), "de");
        assert_eq!(resolve_lang(Lang::Auto, None, None), "de");
    }

    #[test]
    fn mean_logprob_of_certain_tokens_is_zero() {
        assert_eq!(mean_logprob(&[]), 0.0);
        assert!(mean_logprob(&[1.0, 1.0]).abs() < 1e-6);
        assert!((mean_logprob(&[0.5]) - 0.5f32.ln()).abs() < 1e-6);
        assert!(mean_logprob(&[0.0]).is_finite());
    }

    #[test]
    fn accept_rules() {
        assert_eq!(accept("Hallo zusammen", -0.3, 0.1), Ok(()));
        assert_eq!(accept("  ", -0.1, 0.0), Err("noise"));
        assert_eq!(accept("Untertitel im Auftrag des ZDF", -0.1, 0.0), Err("noise"));
        assert_eq!(accept("Hallo zusammen", -1.01, 0.0), Err("low_confidence"));
        assert_eq!(accept("Hallo zusammen", -1.0, 0.0), Ok(()));
        assert_eq!(accept(&"na ".repeat(100), -0.2, 0.0), Err("loop"));
        assert_eq!(accept("Hallo zusammen", -0.6, 0.7), Err("no_speech"));
        assert_eq!(accept("Hallo zusammen", -0.4, 0.7), Ok(()));
    }
}
