//! Word timings from Whisper's DTW token times (pure; the engine feeds it).
//!
//! `t_dtw` is the END of a token, in centiseconds from the start of the
//! decoded chunk. A word therefore starts where the previous text token ended
//! (the first word of a chunk at the chunk's start) and ends at its own last
//! token's `t_dtw`. Taking a word's first token's own `t_dtw` as its start made
//! words ~175 ms late upstream. Tokens join into a word unless they begin with
//! a space; a token that is only punctuation sticks to the word before it.

use crate::decode::DecodeOpts;
use crate::protocol::Word;
use crate::text::normalize;

/// One decoded token as the engine hands it over.
#[derive(Debug, Clone, PartialEq)]
pub struct Tok {
    pub text: String,
    /// End of the token in centiseconds from the chunk start, −1 = unknown.
    pub t_dtw_cs: i64,
    pub p: f32,
    /// Control/timestamp tokens (`[_BEG_]`, `<|en|>` …): never part of a word.
    pub special: bool,
}

/// What one chunk decode returns for word timing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TokDecoded {
    /// Tokens of the accepted Whisper segments, in order.
    pub tokens: Vec<Tok>,
    pub lang: String,
}

/// The engine side of word timing (a DTW-capable Whisper context in the
/// helper, a fake in tests).
pub trait WordTranscriber: Send {
    fn decode_tokens(&mut self, samples: &[f32], opts: &DecodeOpts) -> Result<TokDecoded, String>;
}

/// Raw token bytes from whisper.cpp, which splits multi-byte characters
/// (umlauts) across tokens: an incomplete UTF-8 tail is carried into the
/// next token, which then gives the merged token its (later) end time.
pub fn merge_utf8(raw: Vec<(Vec<u8>, i64, f32, bool)>) -> Vec<Tok> {
    let mut out = Vec::with_capacity(raw.len());
    let mut carry: Vec<u8> = Vec::new();
    let mut carry_p: Vec<f32> = Vec::new();
    for (bytes, t, p, special) in raw {
        if special {
            out.push(Tok { text: String::from_utf8_lossy(&bytes).into_owned(), t_dtw_cs: t, p, special });
            continue;
        }
        carry.extend_from_slice(&bytes);
        carry_p.push(p);
        let incomplete = matches!(std::str::from_utf8(&carry), Err(e) if e.error_len().is_none());
        if incomplete && carry.len() < 16 {
            continue;
        }
        let p = carry_p.iter().sum::<f32>() / carry_p.len() as f32;
        out.push(Tok { text: String::from_utf8_lossy(&carry).into_owned(), t_dtw_cs: t, p, special: false });
        carry.clear();
        carry_p.clear();
    }
    if !carry.is_empty() {
        let p = carry_p.iter().sum::<f32>() / carry_p.len() as f32;
        out.push(Tok { text: String::from_utf8_lossy(&carry).into_owned(), t_dtw_cs: -1, p, special: false });
    }
    out
}

fn has_word_chars(s: &str) -> bool {
    s.chars().any(char::is_alphanumeric)
}

/// Words of one decoded chunk `[seg_start_ms, seg_end_ms]` of the file.
pub fn words_from_tokens(seg_start_ms: u64, seg_end_ms: u64, tokens: &[Tok]) -> Vec<Word> {
    let seg_end_ms = seg_end_ms.max(seg_start_ms);
    let text_toks: Vec<&Tok> = tokens.iter().filter(|t| !t.special && !t.text.is_empty()).collect();
    // (text, first token index, last token index, probabilities)
    let mut groups: Vec<(String, usize, usize, Vec<f32>)> = Vec::new();
    for (i, t) in text_toks.iter().enumerate() {
        let starts_new = t.text.starts_with(char::is_whitespace) && has_word_chars(&t.text);
        match groups.last_mut() {
            Some(g) if !starts_new || !has_word_chars(&g.0) => {
                g.0.push_str(&t.text);
                g.2 = i;
                g.3.push(t.p);
            }
            _ => groups.push((t.text.clone(), i, i, vec![t.p])),
        }
    }
    groups.retain(|g| has_word_chars(&g.0));
    if groups.is_empty() {
        return Vec::new();
    }
    let dtw_ok = text_toks.iter().all(|t| t.t_dtw_cs >= 0);
    let span = seg_end_ms - seg_start_ms;
    let n = groups.len() as u64;
    let at = |cs: i64| (seg_start_ms + cs.max(0) as u64 * 10).min(seg_end_ms);
    let mut out = Vec::with_capacity(groups.len());
    let mut prev_end = seg_start_ms;
    for (k, (text, first, last, probs)) in groups.into_iter().enumerate() {
        let (mut start, mut end) = if dtw_ok {
            let start = if first == 0 { seg_start_ms } else { at(text_toks[first - 1].t_dtw_cs) };
            (start, at(text_toks[last].t_dtw_cs))
        } else {
            (seg_start_ms + span * k as u64 / n, seg_start_ms + span * (k as u64 + 1) / n)
        };
        start = start.max(prev_end).min(seg_end_ms);
        end = end.max(start).min(seg_end_ms);
        prev_end = end;
        let p = probs.iter().sum::<f32>() / probs.len() as f32;
        out.push(Word { text: text.trim().to_string(), start_ms: start, end_ms: end, p });
    }
    out
}

/// The longest phrase checked for loops.
const MAX_LOOP_WORDS: usize = 8;

/// `drop_loops` on words: empty words go, and a word or phrase (up to
/// `MAX_LOOP_WORDS`) repeated more than `MAX_REPEAT` times in a row is
/// Whisper looping — the whole run is dropped.
pub fn filter_words(words: Vec<Word>) -> Vec<Word> {
    let max_repeat = crate::file::MAX_REPEAT;
    let words: Vec<Word> = words.into_iter().filter(|w| !normalize(&w.text).is_empty()).collect();
    let keys: Vec<String> = words.iter().map(|w| normalize(&w.text)).collect();
    let mut keep = vec![true; words.len()];
    let mut i = 0;
    'outer: while i < words.len() {
        for n in 1..=MAX_LOOP_WORDS {
            if i + n > words.len() {
                break;
            }
            let mut reps = 1;
            while i + (reps + 1) * n <= words.len() && keys[i + reps * n..i + (reps + 1) * n] == keys[i..i + n] {
                reps += 1;
            }
            if reps > max_repeat {
                keep[i..i + reps * n].iter_mut().for_each(|k| *k = false);
                i += reps * n;
                continue 'outer;
            }
        }
        i += 1;
    }
    words.into_iter().zip(keep).filter_map(|(w, k)| k.then_some(w)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(text: &str, cs: i64) -> Tok {
        Tok { text: text.into(), t_dtw_cs: cs, p: 0.8, special: false }
    }

    fn special(text: &str) -> Tok {
        Tok { text: text.into(), t_dtw_cs: 0, p: 1.0, special: true }
    }

    fn texts(w: &[Word]) -> Vec<&str> {
        w.iter().map(|w| w.text.as_str()).collect()
    }

    fn w(text: &str, s: u64, e: u64) -> Word {
        Word { text: text.into(), start_ms: s, end_ms: e, p: 1.0 }
    }

    #[test]
    fn joins_bpe_tokens_at_a_leading_space() {
        let ws = words_from_tokens(0, 5000, &[tok(" Hal", 30), tok("lo", 50), tok(" Welt", 90)]);
        assert_eq!(texts(&ws), ["Hallo", "Welt"]);
        assert_eq!(ws[0].end_ms, 500, "a word ends at its LAST token");
    }

    #[test]
    fn start_is_the_previous_tokens_end() {
        let ws = words_from_tokens(1000, 5000, &[tok(" Hal", 50), tok("lo", 80), tok(" Welt", 120)]);
        assert_eq!((ws[1].start_ms, ws[1].end_ms), (1000 + 800, 1000 + 1200));
    }

    #[test]
    fn first_word_starts_at_segment_start() {
        let ws = words_from_tokens(2000, 5000, &[tok(" Ja", 60)]);
        assert_eq!((ws[0].start_ms, ws[0].end_ms), (2000, 2600));
    }

    #[test]
    fn special_tokens_are_skipped() {
        let ws = words_from_tokens(0, 5000, &[special("[_BEG_]"), tok(" Guten", 40), special("[_TT_50]"), tok(" Tag", 70)]);
        assert_eq!(texts(&ws), ["Guten", "Tag"]);
        assert_eq!(ws[1].start_ms, 400, "the special token between them doesn't count as the previous token");
    }

    #[test]
    fn missing_dtw_falls_back_to_even_spread() {
        let ws = words_from_tokens(1000, 4000, &[tok(" eins", -1), tok(" zwei", -1), tok(" drei", -1)]);
        assert_eq!(
            ws.iter().map(|w| (w.start_ms, w.end_ms)).collect::<Vec<_>>(),
            [(1000, 2000), (2000, 3000), (3000, 4000)]
        );
    }

    #[test]
    fn times_are_monotonic_and_clamped_to_the_segment() {
        // DTW past the chunk end and running backwards.
        let ws = words_from_tokens(1000, 2000, &[tok(" a", 50), tok(" b", 30), tok(" c", 500)]);
        let mut prev = 1000;
        for w in &ws {
            assert!(w.start_ms >= prev && w.end_ms >= w.start_ms, "{ws:?}");
            assert!(w.start_ms >= 1000 && w.end_ms <= 2000, "{ws:?}");
            prev = w.end_ms;
        }
        assert_eq!(ws[2].end_ms, 2000);
    }

    #[test]
    fn punctuation_only_token_attaches_to_previous_word() {
        let ws = words_from_tokens(0, 5000, &[tok(" Hallo", 40), tok(" ,", 45), tok(" Welt", 80), tok(".", 85)]);
        assert_eq!(texts(&ws), ["Hallo ,", "Welt."]);
        assert_eq!(ws[0].end_ms, 450);
        assert_eq!(ws[1].start_ms, 450);
    }

    #[test]
    fn leading_punctuation_joins_the_next_word() {
        let ws = words_from_tokens(0, 5000, &[tok(" „", 10), tok(" Hallo", 40)]);
        assert_eq!(texts(&ws), ["„ Hallo"]);
        assert_eq!(ws[0].start_ms, 0);
    }

    #[test]
    fn split_umlauts_are_merged_with_the_later_end() {
        let ue = "ü".as_bytes();
        let raw = vec![
            (b" Gr".to_vec(), 20, 0.5, false),
            (vec![ue[0]], 30, 0.7, false),
            (vec![ue[1], b'n'], 40, 0.9, false),
        ];
        let toks = merge_utf8(raw);
        assert_eq!(toks.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), [" Gr", "ün"]);
        assert_eq!(toks[1].t_dtw_cs, 40);
        assert!((toks[1].p - 0.8).abs() < 1e-6);
        assert_eq!(texts(&words_from_tokens(0, 1000, &toks)), ["Grün"]);
    }

    #[test]
    fn filter_drops_looping_words_and_phrases_but_keeps_doubles() {
        let ws = vec![w("die", 0, 1), w("die", 1, 2), w("Katze", 2, 3)];
        assert_eq!(texts(&filter_words(ws.clone())), ["die", "die", "Katze"]);
        let looped = vec![w("ja", 0, 1), w("Danke.", 1, 2), w("danke", 2, 3), w("Danke", 3, 4), w("Ende", 4, 5)];
        assert_eq!(texts(&filter_words(looped)), ["ja", "Ende"]);
        let phrase: Vec<Word> = ["Vielen", "Dank", "vielen", "Dank", "Vielen", "Dank", "tschüss"]
            .iter()
            .enumerate()
            .map(|(i, t)| w(t, i as u64, i as u64 + 1))
            .collect();
        assert_eq!(texts(&filter_words(phrase)), ["tschüss"]);
        assert!(filter_words(vec![w(" ", 0, 1), w("…", 1, 2)]).is_empty());
    }
}
