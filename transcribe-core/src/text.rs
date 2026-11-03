//! Text helpers: normalisation, similarity, LocalAgreement and the filters
//! that keep Whisper's inventions out of a transcript.

use std::io::Write;

/// Lower case, punctuation removed, whitespace collapsed. Umlauts stay.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            space = false;
        } else if (c.is_whitespace() || c == '-' || c == '/') && !space && !out.is_empty() {
            out.push(' ');
            space = true;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// `1 − distance / max length` over the normalised characters, 0.0..=1.0.
pub fn similarity(a: &str, b: &str) -> f32 {
    let a: Vec<char> = normalize(a).chars().collect();
    let b: Vec<char> = normalize(b).chars().collect();
    let max = a.len().max(b.len());
    if max == 0 {
        return 1.0;
    }
    1.0 - levenshtein(&a, &b) as f32 / max as f32
}

/// LocalAgreement-2: a word becomes stable once two consecutive hypotheses
/// agree on it (as a common prefix). Stable words are never taken back.
#[derive(Debug, Default, Clone)]
pub struct Agreement {
    committed: Vec<String>,
    last: Vec<String>,
}

impl Agreement {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed the newest hypothesis for the open segment; returns
    /// `(stable, pending)`.
    pub fn update(&mut self, hypothesis: &str) -> (String, String) {
        let words: Vec<String> = hypothesis.split_whitespace().map(str::to_string).collect();
        // Only what comes after the committed part can still be agreed on.
        let tail = skip_committed(&self.committed, &words);
        let last_tail = skip_committed(&self.committed, &self.last);
        let agreed = tail
            .iter()
            .zip(last_tail.iter())
            .take_while(|(a, b)| normalize(a) == normalize(b))
            .count();
        // The original spelling of the first hypothesis that had the word.
        self.committed.extend(last_tail.iter().take(agreed).cloned());
        let pending = tail[agreed.min(tail.len())..].join(" ");
        self.last = words;
        (self.committed.join(" "), pending)
    }

    pub fn reset(&mut self) {
        self.committed.clear();
        self.last.clear();
    }
}

/// The words of `words` after the ones already committed. When the new
/// hypothesis rewrote a committed word, the committed part still counts as
/// consumed (it is never taken back) and the rest follows it.
fn skip_committed<'a>(committed: &[String], words: &'a [String]) -> &'a [String] {
    let n = committed.len().min(words.len());
    &words[n..]
}

/// What Whisper writes for silence or noise (ported from `live_transcribe`,
/// plus the classic hallucinations on silent stretches).
pub fn is_noise_text(text: &str) -> bool {
    let t = text.trim();
    if t.chars().all(|c| !c.is_alphanumeric()) {
        return true;
    }
    let lower = normalize(t);
    [
        "untertitel im auftrag des zdf",
        "untertitel der amaraorg community",
        "untertitelung des zdf",
        "untertitelung im auftrag des zdf",
        "subtitles by the amaraorg community",
        "subtitles by",
        "thank you for watching",
        "thanks for watching",
        "vielen dank fürs zuschauen",
        "vielen dank für s zuschauen",
        "danke fürs zuschauen",
    ]
    .iter()
    .any(|p| lower.contains(p))
}

/// Whisper looping on music: a run of identical lines longer than
/// `max_repeat` is dropped entirely (a chorus line sung twice stays).
pub fn drop_loops(lines: Vec<String>, max_repeat: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let mut j = i;
        while j + 1 < lines.len() && lines[j + 1] == lines[i] {
            j += 1;
        }
        if j - i < max_repeat {
            out.extend_from_slice(&lines[i..=j]);
        }
        i = j + 1;
    }
    out
}

/// Bytes / zlib bytes — Whisper's own repetition measure (limit 2.4).
pub fn compression_ratio(s: &str) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    if enc.write_all(s.as_bytes()).is_err() {
        return 0.0;
    }
    match enc.finish() {
        Ok(z) if !z.is_empty() => s.len() as f32 / z.len() as f32,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarity_ignores_case_and_punctuation() {
        assert_eq!(similarity("Hallo, Welt!", "hallo welt"), 1.0);
        assert_eq!(similarity("abc", ""), 0.0);
        assert_eq!(similarity("", ""), 1.0);
        assert!(similarity("Das ist ein Test", "Das ist ein Fest") > 0.9);
        assert!(similarity("Wir treffen uns um drei", "Morgen regnet es") < 0.5);
    }

    #[test]
    fn agreement_commits_only_what_two_hypotheses_share() {
        let mut a = Agreement::new();
        assert_eq!(a.update("ich gehe heute"), ("".into(), "ich gehe heute".into()));
        assert_eq!(a.update("ich gehe heute nach"), ("ich gehe heute".into(), "nach".into()));
        // A later pass rewrites "gehe": committed words are never taken back.
        assert_eq!(a.update("ich ging heute nach Hause"), ("ich gehe heute nach".into(), "Hause".into()));
        assert_eq!(a.update("ich ging heute nach Hause jetzt"), ("ich gehe heute nach Hause".into(), "jetzt".into()));
        a.reset();
        assert_eq!(a.update("neu"), ("".into(), "neu".into()));
    }

    #[test]
    fn agreement_is_case_and_punctuation_insensitive() {
        let mut a = Agreement::new();
        a.update("Hallo, wie");
        assert_eq!(a.update("hallo wie geht's"), ("Hallo, wie".into(), "geht's".into()));
    }

    #[test]
    fn noise_texts_are_recognised() {
        assert!(is_noise_text("Untertitel im Auftrag des ZDF, 2021"));
        assert!(is_noise_text("Thanks for watching!"));
        assert!(is_noise_text(" … "));
        assert!(!is_noise_text("Wir sehen uns morgen"));
    }

    #[test]
    fn loops_are_dropped_like_the_python_worker() {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(drop_loops(v(&["a", "a", "a", "b"]), 2), v(&["b"]));
        assert_eq!(drop_loops(v(&["a", "a", "b"]), 2), v(&["a", "a", "b"]));
        assert_eq!(drop_loops(v(&[]), 2), v(&[]));
    }

    #[test]
    fn compression_ratio_flags_repetition() {
        assert!(compression_ratio(&"ja ".repeat(200)) > 2.4);
        assert!(compression_ratio("Das ist ein ganz normaler Satz ohne Wiederholung.") < 2.0);
    }
}
