//! Clean-up suggestions for a recording: pauses (from the microphone's waveform) and filler
//! words (from the transcript). Pure — the editor shows the list and the user decides; a
//! suggestion is applied through the ordinary transcript cut.

use serde::{Deserialize, Serialize};

use super::editor::{source_to_programme, EditDoc, TranscriptWord};
use super::peaks::Peaks;

/// A pause shorter than this is speech rhythm, not dead air.
pub const DEFAULT_MIN_GAP_MS: u64 = 700;
/// Kept of a pause on each side of the cut.
pub const DEFAULT_PAD_MS: u64 = 150;
pub const MIN_GAP_RANGE_MS: (u64, u64) = (300, 5_000);
pub const PAD_RANGE_MS: (u64, u64) = (50, 500);
/// A cut shorter than this is not worth a suggestion.
pub const MIN_CUT_MS: u64 = 200;
/// A word is protected this far on both sides (recognised word edges are approximate).
pub const WORD_GUARD_MS: u64 = 80;
/// The quiet threshold sits this far above the recording's noise floor …
const FLOOR_MARGIN_DB: f32 = 10.0;
/// … and inside this band.
const THRESHOLD_BAND_DB: (f32, f32) = (-55.0, -28.0);
/// No suggestions beyond this many (a broken recording must not flood the list).
pub const MAX_SUGGESTIONS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AutocutOpts {
    pub silences: bool,
    pub fillers: bool,
    pub min_gap_ms: u64,
    pub pad_ms: u64,
}

impl Default for AutocutOpts {
    fn default() -> Self {
        Self { silences: true, fillers: true, min_gap_ms: DEFAULT_MIN_GAP_MS, pad_ms: DEFAULT_PAD_MS }
    }
}

impl AutocutOpts {
    pub fn normalized(self) -> Self {
        Self {
            min_gap_ms: self.min_gap_ms.clamp(MIN_GAP_RANGE_MS.0, MIN_GAP_RANGE_MS.1),
            pad_ms: self.pad_ms.clamp(PAD_RANGE_MS.0, PAD_RANGE_MS.1),
            ..self
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SuggestionKind {
    Silence,
    Filler,
}

/// One thing that could be cut: source ms `[start_ms, end_ms)` of clip `clip`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub id: String,
    pub kind: SuggestionKind,
    pub clip: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    /// Where it starts in the finished video (for the list and for seeking).
    pub programme_ms: u64,
    /// The filler word; empty for a pause.
    pub text: String,
}

fn block_db(min: f32, max: f32) -> f32 {
    let a = min.abs().max(max.abs());
    if a > 1e-5 { 20.0 * a.log10() } else { -100.0 }
}

/// The level below which a block counts as quiet: the recording's noise floor (its 15th
/// percentile block) plus a margin, kept inside a sane band. `None` without audio.
pub fn quiet_threshold_db(peaks: &[f32]) -> Option<f32> {
    let mut db: Vec<f32> = peaks.as_chunks::<2>().0.iter().map(|p| block_db(p[0], p[1])).collect();
    if db.is_empty() {
        return None;
    }
    db.sort_by(|a, b| a.total_cmp(b));
    let floor = db[db.len() * 15 / 100];
    Some((floor + FLOOR_MARGIN_DB).clamp(THRESHOLD_BAND_DB.0, THRESHOLD_BAND_DB.1))
}

/// Quiet stretches of a waveform in FILE ms, each at least `min_gap_ms` long, before any
/// padding. A recording that is quiet throughout (no speech at all) yields nothing.
pub fn quiet_runs(p: &Peaks, min_gap_ms: u64) -> Vec<(u64, u64)> {
    let blocks = p.data.len() / 2;
    let Some(threshold) = quiet_threshold_db(&p.data) else { return Vec::new() };
    if blocks == 0 || p.duration_ms == 0 {
        return Vec::new();
    }
    let ms = |i: usize| (i as u128 * p.duration_ms as u128 / blocks as u128) as u64;
    let quiet = |i: usize| block_db(p.data[2 * i], p.data[2 * i + 1]) < threshold;
    if (0..blocks).all(quiet) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for i in 0..=blocks {
        let q = i < blocks && quiet(i);
        match (q, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if ms(i) - ms(s) >= min_gap_ms {
                    out.push((ms(s), ms(i)));
                }
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// `range` minus every guarded word span, as the remaining pieces.
fn minus_words(range: (u64, u64), words: &[(u64, u64)]) -> Vec<(u64, u64)> {
    let mut pieces = vec![range];
    for &(ws, we) in words {
        let (gs, ge) = (ws.saturating_sub(WORD_GUARD_MS), we + WORD_GUARD_MS);
        pieces = pieces
            .into_iter()
            .flat_map(|(s, e)| {
                if ge <= s || gs >= e {
                    vec![(s, e)]
                } else {
                    let mut v = Vec::new();
                    if gs > s {
                        v.push((s, gs));
                    }
                    if ge < e {
                        v.push((ge, e));
                    }
                    v
                }
            })
            .collect();
    }
    pieces
}

/// The pauses worth cutting, in SOURCE ms: every quiet run of the microphone (file time +
/// `offset_ms`), shortened by `pad_ms` on both sides, never touching a transcribed word.
pub fn silences(p: &Peaks, offset_ms: i64, opts: &AutocutOpts, words: &[(u64, u64)]) -> Vec<(u64, u64)> {
    let to_source = |file_ms: u64| (file_ms as i64 + offset_ms).max(0) as u64;
    quiet_runs(p, opts.min_gap_ms)
        .into_iter()
        .filter_map(|(s, e)| {
            let (s, e) = (to_source(s) + opts.pad_ms, to_source(e).saturating_sub(opts.pad_ms));
            (e > s).then_some((s, e))
        })
        .flat_map(|r| minus_words(r, words))
        .filter(|(s, e)| e - s >= MIN_CUT_MS)
        .collect()
}

/// Lower case, letters only, runs of the same letter collapsed (`Ähhhm,` → `ähm`).
pub fn normalize_word(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().flat_map(char::to_lowercase).filter(|c| c.is_alphabetic()) {
        if !out.ends_with(c) {
            out.push(c);
        }
    }
    out
}

/// Hesitation sounds per language. Only sounds that are never a word of that language:
/// English `er`/`um` are German words (`er`, `um`), so each list applies to its language only.
fn filler_list(lang: &str) -> &'static [&'static str] {
    match lang.get(..2).unwrap_or(lang) {
        "de" => &["äh", "ähm", "öh", "öhm", "eh", "ehm", "hm", "mhm", "em"],
        "en" => &["um", "uh", "uhm", "er", "erm", "hm", "mhm", "ah"],
        _ => &["hm", "mhm"],
    }
}

/// Indices of the filler words in `words` (hidden words — empty text — are skipped).
pub fn fillers(words: &[TranscriptWord], lang: &str) -> Vec<usize> {
    let list = filler_list(lang);
    words
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            let n = normalize_word(&w.text);
            !n.is_empty() && list.contains(&n.as_str())
        })
        .map(|(i, _)| i)
        .collect()
}

/// Every suggestion for `doc`: per clip, the pauses of its take's microphone (`mic(source)` =
/// its waveform and offset, when there is one) and the fillers of its take's transcript, cut
/// to the clip's window, on the programme clock, in programme order.
pub fn suggest<'a>(
    doc: &EditDoc,
    opts: &AutocutOpts,
    mic: impl Fn(Option<&str>) -> Option<(&'a Peaks, i64)>,
) -> Vec<Suggestion> {
    let opts = opts.normalized();
    let mut out: Vec<Suggestion> = Vec::new();
    for (i, clip) in doc.clips.iter().enumerate() {
        if clip.end_ms <= clip.start_ms {
            continue;
        }
        let source = clip.source.as_deref();
        let transcript = doc.transcripts.iter().rev().find(|t| t.source.as_deref() == source);
        let words: &[TranscriptWord] = transcript.map_or(&[], |t| &t.words);
        let mut push = |kind: SuggestionKind, s: u64, e: u64, text: &str| {
            let (s, e) = (s.max(clip.start_ms), e.min(clip.end_ms));
            if e > s && e - s >= if kind == SuggestionKind::Silence { MIN_CUT_MS } else { 1 } {
                out.push(Suggestion {
                    id: String::new(),
                    kind,
                    clip: i,
                    start_ms: s,
                    end_ms: e,
                    programme_ms: source_to_programme(doc, i, s),
                    text: text.to_string(),
                });
            }
        };
        if opts.silences {
            if let Some((peaks, offset)) = mic(source) {
                let spans: Vec<(u64, u64)> =
                    words.iter().filter(|w| !w.text.trim().is_empty()).map(|w| (w.start_ms, w.end_ms)).collect();
                for (s, e) in silences(peaks, offset, &opts, &spans) {
                    push(SuggestionKind::Silence, s, e, "");
                }
            }
        }
        if opts.fillers {
            if let Some(t) = transcript {
                for wi in fillers(&t.words, &t.lang) {
                    let w = &t.words[wi];
                    push(SuggestionKind::Filler, w.start_ms, w.end_ms, w.text.trim());
                }
            }
        }
    }
    out.sort_by_key(|s| (s.programme_ms, s.end_ms));
    out.truncate(MAX_SUGGESTIONS);
    for (n, s) in out.iter_mut().enumerate() {
        s.id = format!("cut-{n}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::editor::{EditClip, Transcript};

    /// 200 blocks/s: `loud` spans (ms) at ±0.3 over a ±0.002 noise floor.
    fn wave(duration_ms: u64, loud: &[(u64, u64)]) -> Peaks {
        let blocks = (duration_ms / 5) as usize;
        let mut data = Vec::with_capacity(blocks * 2);
        for i in 0..blocks {
            let t = i as u64 * 5;
            let a = if loud.iter().any(|&(s, e)| t >= s && t < e) { 0.3 } else { 0.002 };
            data.extend_from_slice(&[-a, a]);
        }
        Peaks { data, duration_ms }
    }

    fn w(start_ms: u64, end_ms: u64, text: &str) -> TranscriptWord {
        TranscriptWord { id: format!("w{start_ms}"), start_ms, end_ms, text: text.into(), original_text: None }
    }

    fn doc(clips: Vec<EditClip>, words: Vec<TranscriptWord>, lang: &str) -> EditDoc {
        let mut d: EditDoc =
            serde_json::from_value(serde_json::json!({"version": 6, "clips": [{"startMs": 0, "endMs": 1}]})).unwrap();
        d.crop = vec![None; clips.len()];
        d.clips = clips;
        d.transcripts = vec![Transcript { source: None, lang: lang.into(), words }];
        d
    }

    #[test]
    fn a_long_pause_is_found_and_a_short_one_is_not() {
        // Speech 0–2 s, pause 2–4 s, speech 4–6 s, short pause 6–6.4 s, speech to 8 s.
        let p = wave(8_000, &[(0, 2_000), (4_000, 6_000), (6_400, 8_000)]);
        assert_eq!(quiet_runs(&p, 700), vec![(2_000, 4_000)]);
        let cuts = silences(&p, 0, &AutocutOpts::default(), &[]);
        assert_eq!(cuts, vec![(2_150, 3_850)], "150 ms of the pause stay on each side");
    }

    #[test]
    fn the_threshold_follows_the_noise_floor() {
        // The same pause over a much louder room (floor -34 dB instead of -54): still found.
        let mut p = wave(6_000, &[(0, 2_000), (4_000, 6_000)]);
        for v in p.data.iter_mut() {
            if v.abs() < 0.01 {
                *v *= 10.0;
            }
        }
        assert_eq!(quiet_runs(&p, 700), vec![(2_000, 4_000)]);
        assert_eq!(quiet_threshold_db(&[]), None);
    }

    #[test]
    fn a_recording_without_speech_suggests_nothing() {
        let silent = wave(10_000, &[]);
        assert!(quiet_runs(&silent, 700).is_empty(), "nothing to keep means nothing to cut");
        assert!(quiet_runs(&Peaks { data: Vec::new(), duration_ms: 0 }, 700).is_empty());
    }

    #[test]
    fn a_pause_never_cuts_into_a_word() {
        // The recogniser heard a quiet word at 2.8–3.2 s inside the "pause".
        let p = wave(8_000, &[(0, 2_000), (4_000, 8_000)]);
        let cuts = silences(&p, 0, &AutocutOpts::default(), &[(2_800, 3_200)]);
        assert_eq!(cuts, vec![(2_150, 2_720), (3_280, 3_850)]);
        // Too little left on one side: that piece is dropped, not cut to a sliver.
        let cuts = silences(&p, 0, &AutocutOpts::default(), &[(2_250, 3_200)]);
        assert_eq!(cuts, vec![(3_280, 3_850)]);
    }

    #[test]
    fn the_microphone_offset_moves_pauses_onto_the_video_clock() {
        let p = wave(8_000, &[(0, 2_000), (4_000, 8_000)]);
        assert_eq!(silences(&p, 120, &AutocutOpts::default(), &[]), vec![(2_270, 3_970)]);
    }

    #[test]
    fn fillers_per_language() {
        let words = vec![w(0, 100, "Also"), w(200, 300, "Ähhhm,"), w(400, 500, "um"), w(600, 700, "er"), w(800, 900, "äh.")];
        // German: `um` and `er` are words, the hesitations are not.
        assert_eq!(fillers(&words, "de"), vec![1, 4]);
        // English: `um` and `er` are hesitations.
        assert_eq!(fillers(&words, "en-US"), vec![2, 3]);
        assert_eq!(fillers(&words, "fr"), Vec::<usize>::new());
        assert_eq!(normalize_word("Ähhhm,"), "ähm");
        assert_eq!(normalize_word("  …"), "");
        // A hidden word (empty text) is not a filler.
        assert!(fillers(&[w(0, 10, "")], "de").is_empty());
    }

    #[test]
    fn suggestions_are_cut_to_the_clips_and_ordered_like_the_video() {
        // The take: speech 0–2, pause 2–4, speech 4–10 s; "ähm" at 5.0–5.3 s.
        let p = wave(10_000, &[(0, 2_000), (4_000, 10_000)]);
        let words = vec![w(500, 900, "Hallo"), w(5_000, 5_300, "ähm"), w(6_000, 6_400, "weiter")];
        // The programme plays 4.5–10 s first, then 0–3 s (which ends inside the pause).
        let d = doc(vec![EditClip::main(4_500, 10_000), EditClip::main(0, 3_000)], words, "de");
        let got = suggest(&d, &AutocutOpts::default(), |_| Some((&p, 0)));
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].kind, got[0].clip, got[0].start_ms, got[0].end_ms), (SuggestionKind::Filler, 0, 5_000, 5_300));
        assert_eq!((got[0].programme_ms, got[0].text.as_str()), (500, "ähm"));
        // The pause is cut at the second clip's end (3 s), not at its own end (3.85 s).
        assert_eq!((got[1].kind, got[1].clip, got[1].start_ms, got[1].end_ms), (SuggestionKind::Silence, 1, 2_150, 3_000));
        assert_eq!(got[1].programme_ms, 5_500 + 2_150);
        assert_eq!((got[0].id.as_str(), got[1].id.as_str()), ("cut-0", "cut-1"));
        // Each kind can be switched off; without a microphone only fillers remain.
        let only = suggest(&d, &AutocutOpts { silences: false, ..Default::default() }, |_| Some((&p, 0)));
        assert!(only.iter().all(|s| s.kind == SuggestionKind::Filler));
        assert_eq!(suggest(&d, &AutocutOpts::default(), |_| None).len(), 1);
        assert_eq!(suggest(&d, &AutocutOpts { fillers: false, silences: false, ..Default::default() }, |_| Some((&p, 0))), vec![]);
    }

    #[test]
    fn options_stay_in_range() {
        let o = AutocutOpts { min_gap_ms: 1, pad_ms: 99_999, ..Default::default() }.normalized();
        assert_eq!((o.min_gap_ms, o.pad_ms), (MIN_GAP_RANGE_MS.0, PAD_RANGE_MS.1));
        let o: AutocutOpts = serde_json::from_str(r#"{"minGapMs": 1200}"#).unwrap();
        assert_eq!((o.min_gap_ms, o.pad_ms, o.silences, o.fillers), (1_200, DEFAULT_PAD_MS, true, true));
    }
}
