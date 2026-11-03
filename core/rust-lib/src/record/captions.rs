//! Captions derived from the transcript (stage 6). Pure: nothing here is stored — every scene
//! build groups the words of each take's transcript into lines and places them in every clip
//! that plays them, as text annotations measured on the output frame (`space: "frame"`).
//!
//! Port of OpenScreen's `annotationsFromCaptions.ts` (line grouping, dedupe, overlap trim) and
//! `captions/settings.ts` + `cues.ts` (the box, the style, the projection through the clips);
//! values verbatim from the code (`RECON-stage6.md` §2, §6).

use super::editor::{CaptionAnchorH, CaptionAnchorV, CaptionSettings, EditDoc, TranscriptWord, TEXT_FONTS};
use serde_json::{json, Value};
use std::collections::HashMap;

/// `fontSize` is pixels at a 1080-high frame (`fontSizeRel = font_size / 1080`).
pub const CAPTION_REFERENCE_PX: f64 = 1080.0;
pub const DEFAULT_CAPTION_FONT_SIZE: f64 = 48.0;
/// What a stored font size is read into.
pub const CAPTION_FONT_SIZE_RANGE: (f64, f64) = (12.0, 200.0);
/// What the captions pane offers.
pub const CAPTION_FONT_SIZE_UI_RANGE: (f64, f64) = (16.0, 140.0);
pub const CAPTION_PLATE_OPACITY_RANGE: (f64, f64) = (0.4, 0.9);
/// A stored plate opacity below this reads as "no plate".
pub const CAPTION_PLATE_OFF_BELOW: f64 = 0.2;
pub const DEFAULT_CAPTION_PLATE_OPACITY: f64 = 0.55;
pub const DEFAULT_CAPTION_COLOR: &str = "#ffffff";
pub const CAPTION_PLATE_DARK: &str = "#000000";
pub const CAPTION_PLATE_LIGHT: &str = "#ffffff";
pub const CAPTION_INSET_Y_MAX: f64 = 50.0;
pub const CAPTION_INSET_X_MAX: f64 = 25.0;
pub const CAPTION_INSET_STEP: f64 = 0.5;
/// At or above this output aspect a frame counts as landscape.
pub const CAPTION_LANDSCAPE_ASPECT: f64 = 1.5;
pub const CAPTION_INSET_Y_LANDSCAPE: f64 = 1.5;
pub const CAPTION_INSET_Y_PORTRAIT: f64 = 12.5;
pub const CAPTION_INSET_X_LANDSCAPE: f64 = 10.0;
pub const CAPTION_INSET_X_PORTRAIT: f64 = 5.0;
/// The column captions wrap in, `(x, width)` in percent of the frame width.
pub const CAPTION_COLUMN_LANDSCAPE: (f64, f64) = (16.0, 68.0);
pub const CAPTION_COLUMN_PORTRAIT: (f64, f64) = (5.0, 90.0);
pub const CAPTION_LINE_HEIGHT_EM: f64 = 1.5;
/// Lines the box has room for (headroom, never a position).
pub const CAPTION_BOX_LINES: f64 = 3.0;
/// The plate's padding above and below the text, together, in em (2 × 0.1).
pub const CAPTION_PLATE_PAD_EM: f64 = 0.2;
pub const CAPTION_BOX_MIN_HEIGHT: f64 = 10.0;
pub const CAPTION_WORDS_RANGE: (u8, u8) = (1, 12);
pub const DEFAULT_CAPTION_MIN_WORDS: u8 = 2;
pub const DEFAULT_CAPTION_MAX_WORDS: u8 = 7;
/// Captions paint after every annotation: their `zIndex` starts here (or above the highest
/// annotation, whichever is larger).
pub const CAPTION_Z_BASE: i32 = 100_000;
/// A silence of at least this between two words starts a new run of lines.
pub const CAPTION_RUN_BREAK_MS: u64 = 240;
/// The same text again within this gap (or overlapping by more than [`CAPTION_DEDUPE_OVERLAP_MS`])
/// is one line, a Whisper chunk artefact.
pub const CAPTION_DEDUPE_GAP_MS: u64 = 550;
pub const CAPTION_DEDUPE_OVERLAP_MS: u64 = 15;
/// The shortest a line is shown.
pub const CAPTION_MIN_LINE_MS: u64 = 20;

/// A named look: font, weight, size, text colour and plate set together.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CaptionLook {
    pub font_family: &'static str,
    pub bold: bool,
    pub font_size: f64,
    pub color: &'static str,
    pub plate: bool,
    pub plate_color: &'static str,
    pub plate_opacity: f64,
}

/// `settings.ts:186-224`; classic is the default look.
pub const CAPTION_LOOKS: [(&str, CaptionLook); 4] = [
    ("classic", CaptionLook { font_family: "Inter", bold: true, font_size: 48.0, color: "#ffffff", plate: true, plate_color: "#000000", plate_opacity: 0.55 }),
    ("bold", CaptionLook { font_family: "Oswald", bold: true, font_size: 60.0, color: "#fde047", plate: true, plate_color: "#000000", plate_opacity: 0.85 }),
    ("minimal", CaptionLook { font_family: "Inter", bold: false, font_size: 40.0, color: "#ffffff", plate: true, plate_color: "#000000", plate_opacity: 0.4 }),
    ("light", CaptionLook { font_family: "Inter", bold: true, font_size: 48.0, color: "#111111", plate: true, plate_color: "#ffffff", plate_opacity: 0.9 }),
];

/// One caption line in SOURCE ms of its take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptionLine {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// `computeCaptionLineIndexRanges`: `count` words cut into lines of `min..=max` words, never
/// leaving a tail shorter than `min` when it can be avoided.
fn line_ranges(count: usize, min: usize, max: usize) -> Vec<(usize, usize)> {
    let min_w = min.min(max).max(1);
    let max_w = max.max(min_w);
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < count {
        let remaining = count - i;
        if remaining <= max_w {
            match out.last_mut() {
                Some(last) if remaining < min_w => last.1 = count,
                _ => out.push((i, count)),
            }
            break;
        }
        let mut take = max_w;
        let after = remaining - take;
        if after > 0 && after < min_w {
            take = remaining - min_w;
            if take < min_w {
                out.push((i, count));
                break;
            }
            take = take.min(max_w);
        }
        out.push((i, i + take));
        i += take;
    }
    out
}

/// `normalizeCaptionKey`: the text two lines are compared by.
fn dedupe_key(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mapped: String = collapsed
        .chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            c => c,
        })
        .collect();
    mapped.to_lowercase().trim_end_matches(['.', '!', '?', ',', ';', ':']).to_string()
}

/// The caption lines of one transcript: words with text, sorted; a gap of at least
/// [`CAPTION_RUN_BREAK_MS`] starts a new run; each run cut into lines of `min..=max` words;
/// a line spans `[first.start, max(first.start + 20, last.end)]`; then repeats merged and
/// overlaps trimmed (`groupTimedCaptionWordsIntoLines` → `dedupeAdjacentCaptionRepeats` →
/// `finalizeCaptionSegmentsForPlayback`).
pub fn caption_lines(words: &[TranscriptWord], min: u8, max: u8) -> Vec<CaptionLine> {
    let mut ws: Vec<&TranscriptWord> = words.iter().filter(|w| !w.text.trim().is_empty()).collect();
    ws.sort_by_key(|w| (w.start_ms, w.end_ms));
    if ws.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<CaptionLine> = Vec::new();
    let flush = |run: &[&TranscriptWord], lines: &mut Vec<CaptionLine>| {
        for (from, to) in line_ranges(run.len(), min as usize, max as usize) {
            let slice = &run[from..to];
            let start = slice[0].start_ms;
            let end = (start + CAPTION_MIN_LINE_MS).max(slice[slice.len() - 1].end_ms);
            let text = slice.iter().map(|w| w.text.trim()).collect::<Vec<_>>().join(" ");
            lines.push(CaptionLine { start_ms: start, end_ms: end, text });
        }
    };
    let mut run_start = 0;
    for i in 1..ws.len() {
        if ws[i].start_ms.saturating_sub(ws[i - 1].end_ms) >= CAPTION_RUN_BREAK_MS {
            flush(&ws[run_start..i], &mut lines);
            run_start = i;
        }
    }
    flush(&ws[run_start..], &mut lines);
    // Lines inside a run must not overlap (a word's end past the next line's start).
    for i in 0..lines.len().saturating_sub(1) {
        let next = lines[i + 1].start_ms;
        if lines[i].end_ms > next + 1 {
            lines[i].end_ms = (lines[i].start_ms + CAPTION_MIN_LINE_MS).max(next);
        }
    }

    // Dedupe adjacent repeats.
    lines.sort_by_key(|l| (l.start_ms, l.end_ms));
    let mut deduped: Vec<CaptionLine> = Vec::new();
    for line in lines {
        if let Some(prev) = deduped.last_mut() {
            if dedupe_key(&prev.text) == dedupe_key(&line.text) {
                let overlap = prev.end_ms.saturating_sub(line.start_ms);
                let gap = line.start_ms.saturating_sub(prev.end_ms);
                if overlap > CAPTION_DEDUPE_OVERLAP_MS || gap < CAPTION_DEDUPE_GAP_MS {
                    prev.start_ms = prev.start_ms.min(line.start_ms);
                    prev.end_ms = prev.end_ms.max(line.end_ms);
                    continue;
                }
            }
        }
        deduped.push(line);
    }
    // Trim real overlaps: the earlier line ends where the later begins.
    for i in 1..deduped.len() {
        let cur = deduped[i].start_ms;
        if cur + 2 < deduped[i - 1].end_ms {
            deduped[i - 1].end_ms = (deduped[i - 1].start_ms + 1).max(cur);
        }
    }
    deduped
}

/// The plate is drawn (on, and not at an opacity that reads as off).
pub fn plate_on(s: &CaptionSettings) -> bool {
    s.plate && s.plate_opacity >= CAPTION_PLATE_OFF_BELOW
}

/// The vertical inset in force: the stored one, else the aspect's standard.
pub fn inset_y(s: &CaptionSettings, aspect: f64) -> f64 {
    let landscape = aspect >= CAPTION_LANDSCAPE_ASPECT;
    s.inset_y
        .unwrap_or(if landscape { CAPTION_INSET_Y_LANDSCAPE } else { CAPTION_INSET_Y_PORTRAIT })
        .clamp(0.0, CAPTION_INSET_Y_MAX)
}

/// The horizontal inset in force: the stored one, else the aspect's standard.
pub fn inset_x(s: &CaptionSettings, aspect: f64) -> f64 {
    let landscape = aspect >= CAPTION_LANDSCAPE_ASPECT;
    s.inset_x
        .unwrap_or(if landscape { CAPTION_INSET_X_LANDSCAPE } else { CAPTION_INSET_X_PORTRAIT })
        .clamp(0.0, CAPTION_INSET_X_MAX)
}

/// The output aspect (`w / h`), 16:9 for a degenerate size.
fn aspect_of(out: (u32, u32)) -> f64 {
    if out.0 == 0 || out.1 == 0 { 16.0 / 9.0 } else { out.0 as f64 / out.1 as f64 }
}

/// `captionBoxRect`: the caption box `(x, y, w, h)` in percent of the output frame. The text
/// block is pinned to the anchored edge (`verticalAlign`), the height is only headroom.
pub fn caption_box(settings: &CaptionSettings, out: (u32, u32)) -> (f64, f64, f64, f64) {
    let aspect = aspect_of(out);
    // The column's own x is where a centred box lands anyway; only its width is a limit.
    let (_, col_w) =
        if aspect >= CAPTION_LANDSCAPE_ASPECT { CAPTION_COLUMN_LANDSCAPE } else { CAPTION_COLUMN_PORTRAIT };
    let iy = inset_y(settings, aspect);
    let ix = inset_x(settings, aspect);
    let plate = if plate_on(settings) { CAPTION_PLATE_PAD_EM } else { 0.0 };
    let font = settings.font_size.clamp(CAPTION_FONT_SIZE_RANGE.0, CAPTION_FONT_SIZE_RANGE.1);
    let capacity = font * (CAPTION_BOX_LINES * CAPTION_LINE_HEIGHT_EM + plate) / CAPTION_REFERENCE_PX * 100.0;
    let height = capacity.clamp(CAPTION_BOX_MIN_HEIGHT, 100.0 - iy);
    let width = match settings.anchor_h {
        CaptionAnchorH::Center => col_w,
        _ => col_w.min(100.0 - ix),
    };
    let x = match settings.anchor_h {
        CaptionAnchorH::Left => ix,
        CaptionAnchorH::Right => 100.0 - ix - width,
        CaptionAnchorH::Center => (100.0 - width) / 2.0,
    };
    let y = match settings.anchor_v {
        CaptionAnchorV::Bottom => 100.0 - iy - height,
        CaptionAnchorV::Top => iy,
    };
    (x, y, width, height)
}

/// `#rgb` / `#rrggbb` → its channels.
fn hex_rgb(c: &str) -> Option<(u8, u8, u8)> {
    let hex = c.trim().strip_prefix('#')?;
    if !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let ch = |s: &str| u8::from_str_radix(s, 16).ok();
    match hex.len() {
        3 => {
            let d: Vec<u8> = hex.chars().filter_map(|c| ch(&format!("{c}{c}"))).collect();
            Some((d[0], d[1], d[2]))
        }
        6 => Some((ch(&hex[0..2])?, ch(&hex[2..4])?, ch(&hex[4..6])?)),
        _ => None,
    }
}

/// A plate colour is `#rgb` or `#rrggbb` (the opacity is its own field).
pub fn valid_plate_color(c: &str) -> bool {
    hex_rgb(c).is_some()
}

/// The plate as the compositor's CSS colour: `rgba(r, g, b, opacity)`, or `transparent`.
pub fn plate_css(s: &CaptionSettings) -> String {
    if !plate_on(s) {
        return "transparent".into();
    }
    let (r, g, b) = hex_rgb(&s.plate_color).unwrap_or((0, 0, 0));
    format!("rgba({r}, {g}, {b}, {})", s.plate_opacity)
}

/// Clamps a stored caption block into what the editor offers (`readPlate` & co): font size into
/// [`CAPTION_FONT_SIZE_RANGE`], an unknown font → Inter, an invalid colour → the default, a plate
/// opacity below [`CAPTION_PLATE_OFF_BELOW`] kept as stored (it reads as no plate), any other into
/// [`CAPTION_PLATE_OPACITY_RANGE`]; insets into their ranges (`None` stays `None` — the aspect's
/// standard is never frozen); words per line into [`CAPTION_WORDS_RANGE`], min ≤ max.
pub fn normalize_settings(s: CaptionSettings) -> CaptionSettings {
    let finite = |v: f64, fallback: f64| if v.is_finite() { v } else { fallback };
    let font_size = finite(s.font_size, DEFAULT_CAPTION_FONT_SIZE).clamp(CAPTION_FONT_SIZE_RANGE.0, CAPTION_FONT_SIZE_RANGE.1);
    let font_family =
        if TEXT_FONTS.contains(&s.font_family.as_str()) { s.font_family } else { TEXT_FONTS[0].to_string() };
    let color = if super::editor::is_valid_color(&s.color) { s.color } else { DEFAULT_CAPTION_COLOR.to_string() };
    let plate_color = if valid_plate_color(&s.plate_color) { s.plate_color } else { CAPTION_PLATE_DARK.to_string() };
    let raw = finite(s.plate_opacity, DEFAULT_CAPTION_PLATE_OPACITY).clamp(0.0, 1.0);
    let plate_opacity = if raw < CAPTION_PLATE_OFF_BELOW {
        raw
    } else {
        raw.clamp(CAPTION_PLATE_OPACITY_RANGE.0, CAPTION_PLATE_OPACITY_RANGE.1)
    };
    let inset = |v: Option<f64>, max: f64| v.filter(|v| v.is_finite()).map(|v| v.clamp(0.0, max));
    let (lo, hi) = CAPTION_WORDS_RANGE;
    let a = s.min_words.clamp(lo, hi);
    let b = s.max_words.clamp(lo, hi);
    CaptionSettings {
        font_size,
        font_family,
        color,
        plate_color,
        plate_opacity,
        inset_y: inset(s.inset_y, CAPTION_INSET_Y_MAX),
        inset_x: inset(s.inset_x, CAPTION_INSET_X_MAX),
        min_words: a.min(b),
        max_words: a.max(b),
        ..s
    }
}

/// The captions as compositor text annotations (`V/src/scene.rs` `SceneAnnotation`), appended
/// after the document's own annotations. Per clip of the scene (`kept` maps a document clip to
/// its scene index; a clip that is not in the scene gets none): the lines of the transcript of
/// that clip's take, clamped to the clip's window — a duplicated clip repeats them, a line a
/// clip edge cuts gets one entry per clip. Times are the clip's source seconds (the compositor
/// gates by clip index, so speed applies). `zIndex` starts above every annotation. Empty when
/// captions are off or there is no transcript.
pub fn caption_entries(doc: &EditDoc, out: (u32, u32), kept: &dyn Fn(usize) -> Option<usize>) -> Vec<Value> {
    let s = &doc.captions;
    if !s.enabled || doc.transcripts.is_empty() {
        return Vec::new();
    }
    let (x, y, w, h) = caption_box(s, out);
    let top = doc.annotations.iter().map(|a| a.z_index).max().unwrap_or(0);
    let z_base = CAPTION_Z_BASE.max(top.saturating_add(1));
    let background = plate_css(s);
    let mut lines_by_source: HashMap<Option<&str>, Vec<CaptionLine>> = HashMap::new();
    let mut entries = Vec::new();
    for (i, clip) in doc.clips.iter().enumerate() {
        let Some(ci) = kept(i) else { continue };
        let source = clip.source.as_deref();
        let lines = lines_by_source.entry(source).or_insert_with(|| {
            doc.transcripts
                .iter()
                .rev()
                .find(|t| t.source.as_deref() == source)
                .map(|t| caption_lines(&t.words, s.min_words, s.max_words))
                .unwrap_or_default()
        });
        let mut n = 0;
        for line in lines.iter() {
            let start = line.start_ms.max(clip.start_ms);
            let end = line.end_ms.min(clip.end_ms);
            if end <= start {
                continue;
            }
            let z = z_base.saturating_add(entries.len() as i32);
            entries.push(json!({
                "id": format!("caption-{ci}-{n}"),
                "clipIndex": ci,
                "startSec": start as f64 / 1000.0,
                "endSec": end as f64 / 1000.0,
                "x": x / 100.0,
                "y": y / 100.0,
                "w": w / 100.0,
                "h": h / 100.0,
                "zIndex": z,
                "kind": "text",
                "space": "frame",
                "text": {
                    "content": line.text,
                    "color": s.color,
                    "backgroundColor": background,
                    "fontSizeRel": s.font_size / CAPTION_REFERENCE_PX,
                    "fontFamily": s.font_family,
                    "fontWeight": if s.bold { "bold" } else { "normal" },
                    "fontStyle": "normal",
                    "textDecoration": "none",
                    "textAlign": match s.anchor_h {
                        CaptionAnchorH::Left => "left",
                        CaptionAnchorH::Center => "center",
                        CaptionAnchorH::Right => "right",
                    },
                    "verticalAlign": match s.anchor_v {
                        CaptionAnchorV::Bottom => "bottom",
                        CaptionAnchorV::Top => "top",
                    },
                    "animation": "none",
                },
            }));
            n += 1;
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::editor::{Annotation, AnnotationBody, EditClip, TextBody, Transcript};

    fn w(start_ms: u64, end_ms: u64, text: &str) -> TranscriptWord {
        TranscriptWord { id: String::new(), start_ms, end_ms, text: text.into(), original_text: None }
    }

    /// `n` words of 100 ms, 50 ms apart.
    fn run(n: usize, from_ms: u64) -> Vec<TranscriptWord> {
        (0..n).map(|i| w(from_ms + i as u64 * 150, from_ms + i as u64 * 150 + 100, &format!("w{i}"))).collect()
    }

    fn doc(clips: Vec<EditClip>, transcripts: Vec<Transcript>) -> EditDoc {
        let mut d: EditDoc = serde_json::from_value(json!({"version": 5, "clips": [{"startMs": 0, "endMs": 1}]})).unwrap();
        d.crop = vec![None; clips.len()];
        d.clips = clips;
        d.transcripts = transcripts;
        d.captions.enabled = true;
        d
    }

    fn own(words: Vec<TranscriptWord>) -> Transcript {
        Transcript { source: None, lang: "de".into(), words }
    }

    fn all(i: usize) -> Option<usize> {
        Some(i)
    }

    fn spans(entries: &[Value]) -> Vec<(u64, f64, f64, String)> {
        entries
            .iter()
            .map(|e| {
                (
                    e["clipIndex"].as_u64().unwrap(),
                    e["startSec"].as_f64().unwrap(),
                    e["endSec"].as_f64().unwrap(),
                    e["text"]["content"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn groups_runs_at_a_quarter_second_gap() {
        // 0.24 s of silence breaks the run …
        let broken = caption_lines(&[w(0, 100, "a"), w(340, 440, "b")], 2, 7);
        assert_eq!(broken.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        // … 0.23 s does not.
        let joined = caption_lines(&[w(0, 100, "a"), w(330, 430, "b")], 2, 7);
        assert_eq!(joined, vec![CaptionLine { start_ms: 0, end_ms: 430, text: "a b".into() }]);
    }

    #[test]
    fn lines_respect_min_and_max_words() {
        for n in [11, 8, 9, 15] {
            let lines = caption_lines(&run(n, 0), 2, 7);
            let counts: Vec<usize> = lines.iter().map(|l| l.text.split(' ').count()).collect();
            assert_eq!(counts.iter().sum::<usize>(), n, "{n}: {counts:?}");
            assert!(counts.iter().all(|&c| (2..=7).contains(&c)), "{n} words: {counts:?}");
        }
        assert_eq!(caption_lines(&run(11, 0), 2, 7).iter().map(|l| l.text.split(' ').count()).collect::<Vec<_>>(), vec![7, 4]);
        assert_eq!(caption_lines(&run(8, 0), 2, 7).iter().map(|l| l.text.split(' ').count()).collect::<Vec<_>>(), vec![6, 2]);
        // Lines follow each other without overlap.
        let lines = caption_lines(&run(11, 0), 2, 7);
        assert!(lines[0].end_ms <= lines[1].start_ms);
        // A single word is always a line, whatever the minimum.
        assert_eq!(caption_lines(&run(1, 0), 3, 7).len(), 1);
    }

    #[test]
    fn line_span_is_at_least_20ms() {
        let l = caption_lines(&[w(1_000, 1_000, "kurz")], 2, 7);
        assert_eq!((l[0].start_ms, l[0].end_ms), (1_000, 1_020));
        let l = caption_lines(&[w(1_000, 1_500, "lang")], 2, 7);
        assert_eq!(l[0].end_ms, 1_500);
    }

    #[test]
    fn same_text_close_together_is_merged() {
        // Two runs (0.3 s apart) with the same text up to case and punctuation: one line.
        let l = caption_lines(&[w(0, 100, "Hallo"), w(400, 500, "hallo.")], 2, 7);
        assert_eq!(l, vec![CaptionLine { start_ms: 0, end_ms: 500, text: "Hallo".into() }]);
        // The same text after real silence is said again.
        let l = caption_lines(&[w(0, 100, "Hallo"), w(1_000, 1_100, "Hallo")], 2, 7);
        assert_eq!(l.len(), 2);
        // Different text close together stays apart.
        assert_eq!(caption_lines(&[w(0, 100, "eins"), w(400, 500, "zwei")], 2, 7).len(), 2);
    }

    #[test]
    fn empty_word_text_is_hidden() {
        let l = caption_lines(&[w(0, 100, "a"), w(150, 250, "  "), w(300, 400, "b")], 2, 7);
        assert_eq!(l, vec![CaptionLine { start_ms: 0, end_ms: 400, text: "a b".into() }]);
        assert!(caption_lines(&[w(0, 100, "")], 2, 7).is_empty());
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn box_landscape_bottom_centre() {
        let s = CaptionSettings::default();
        let (x, y, bw, h) = caption_box(&s, (1920, 1080));
        let expected_h = 48.0 * (3.0 * 1.5 + 0.2) / 1080.0 * 100.0;
        assert!(close(x, 16.0) && close(bw, 68.0), "{x} {bw}");
        assert!(close(h, expected_h), "{h}");
        assert!(close(y, 100.0 - 1.5 - h), "{y}");
        // Without a plate the box loses the plate's padding; at the top it sits at the inset.
        let top = CaptionSettings { plate: false, anchor_v: CaptionAnchorV::Top, ..s.clone() };
        let (_, ty, _, th) = caption_box(&top, (1920, 1080));
        assert!(close(ty, 1.5) && close(th, 48.0 * 4.5 / 10.8));
        // Left and right narrow the column from the inset.
        let left = CaptionSettings { anchor_h: CaptionAnchorH::Left, inset_x: Some(20.0), ..s.clone() };
        let (lx, _, lw, _) = caption_box(&left, (1920, 1080));
        assert!(close(lx, 20.0) && close(lw, 68.0));
        // An inset past its maximum is read as 25; the 90 % portrait column narrows to fit.
        let right = CaptionSettings { anchor_h: CaptionAnchorH::Right, inset_x: Some(40.0), ..s };
        let (rx, _, rw, _) = caption_box(&right, (1920, 1080));
        assert!(close(rw, 68.0) && close(rx, 7.0), "{rx} {rw}");
        let (px, _, pw, _) = caption_box(&right, (1080, 1920));
        assert!(close(pw, 75.0) && close(px, 0.0), "{px} {pw}");
    }

    #[test]
    fn box_portrait_uses_12_5_and_90_column() {
        // Another field changed: the aspect's standards are still resolved, not frozen.
        let s = CaptionSettings { font_size: 80.0, bold: false, ..CaptionSettings::default() };
        let (x, y, bw, h) = caption_box(&s, (1080, 1920));
        assert!(close(x, 5.0) && close(bw, 90.0), "{x} {bw}");
        assert!(close(h, 80.0 * 4.7 / 10.8), "{h}");
        assert!(close(y, 100.0 - 12.5 - h), "{y}");
        // The same settings in landscape get the landscape standard.
        let (lx, ly, _, lh) = caption_box(&s, (1920, 1080));
        assert!(close(lx, 16.0) && close(ly, 100.0 - 1.5 - lh));
        // Left in portrait uses the portrait standard inset (5).
        let left = CaptionSettings { anchor_h: CaptionAnchorH::Left, ..s };
        assert!(close(caption_box(&left, (1080, 1920)).0, 5.0));
        assert!(close(caption_box(&left, (1920, 1080)).0, 10.0));
    }

    #[test]
    fn entries_repeat_in_duplicated_clips() {
        let d = doc(vec![EditClip::main(0, 5_000), EditClip::main(0, 5_000)], vec![own(vec![w(1_000, 1_500, "hallo")])]);
        let e = caption_entries(&d, (1920, 1080), &all);
        assert_eq!(spans(&e), vec![(0, 1.0, 1.5, "hallo".into()), (1, 1.0, 1.5, "hallo".into())]);
        assert_eq!(e[0]["id"], "caption-0-0");
        assert_eq!(e[1]["id"], "caption-1-0");
    }

    #[test]
    fn entries_are_clamped_to_the_clip_window_and_split_at_clip_edges() {
        let words = vec![w(1_000, 1_400, "über"), w(1_500, 3_000, "die"), w(4_500, 4_800, "später")];
        let d = doc(
            vec![EditClip::main(0, 2_000), EditClip::main(2_000, 4_000), EditClip::main(6_000, 8_000)],
            vec![own(words)],
        );
        let e = caption_entries(&d, (1920, 1080), &all);
        assert_eq!(spans(&e), vec![(0, 1.0, 2.0, "über die".into()), (1, 2.0, 3.0, "über die".into())]);
        // A clip that is not in the scene gets none; the rest keep their scene index.
        let skip_first = |i: usize| if i == 0 { None } else { Some(i - 1) };
        assert_eq!(spans(&caption_entries(&d, (1920, 1080), &skip_first)), vec![(0, 2.0, 3.0, "über die".into())]);
    }

    #[test]
    fn entries_follow_each_takes_own_transcript() {
        let take = EditClip { start_ms: 0, end_ms: 5_000, source: Some("t1".into()) };
        let d = doc(
            vec![EditClip::main(0, 5_000), take, EditClip { start_ms: 0, end_ms: 5_000, source: Some("other".into()) }],
            vec![
                own(vec![w(100, 400, "eigen")]),
                Transcript { source: Some("t1".into()), lang: "en".into(), words: vec![w(2_000, 2_300, "take")] },
            ],
        );
        let e = caption_entries(&d, (1920, 1080), &all);
        assert_eq!(spans(&e), vec![(0, 0.1, 0.4, "eigen".into()), (1, 2.0, 2.3, "take".into())]);
    }

    #[test]
    fn entries_sort_after_every_annotation() {
        let mut d = doc(vec![EditClip::main(0, 5_000)], vec![own(vec![w(0, 100, "a"), w(150, 250, "b"), w(1_000, 1_100, "c")])]);
        let ann = Annotation::new("x".into(), 0, 0, 1_000, AnnotationBody::Text(TextBody::default()), &[]);
        d.annotations = vec![Annotation { z_index: 500_000, ..ann }];
        let e = caption_entries(&d, (1920, 1080), &all);
        let z: Vec<i64> = e.iter().map(|e| e["zIndex"].as_i64().unwrap()).collect();
        assert_eq!(z, vec![500_001, 500_002]);
        // Without a high annotation they start at the base.
        d.annotations.clear();
        assert_eq!(caption_entries(&d, (1920, 1080), &all)[0]["zIndex"], CAPTION_Z_BASE);
    }

    #[test]
    fn entries_carry_vertical_align_and_rgba_plate() {
        let mut d = doc(vec![EditClip::main(0, 5_000)], vec![own(vec![w(0, 100, "a")])]);
        let e = &caption_entries(&d, (1920, 1080), &all)[0];
        assert_eq!(e["kind"], "text");
        assert_eq!(e["space"], "frame");
        let t = &e["text"];
        assert_eq!(t["verticalAlign"], "bottom");
        assert_eq!(t["backgroundColor"], "rgba(0, 0, 0, 0.55)");
        assert_eq!(t["textAlign"], "center");
        assert_eq!(t["animation"], "none");
        assert_eq!(t["fontWeight"], "bold");
        assert_eq!(t["fontSizeRel"].as_f64().unwrap(), 48.0 / 1080.0);
        assert!(close(e["x"].as_f64().unwrap(), 0.16) && close(e["w"].as_f64().unwrap(), 0.68));
        d.captions.anchor_v = CaptionAnchorV::Top;
        d.captions.plate_color = "#fff".into();
        d.captions.plate_opacity = 0.9;
        let t = &caption_entries(&d, (1920, 1080), &all)[0]["text"];
        assert_eq!((t["verticalAlign"].as_str(), t["backgroundColor"].as_str()), (Some("top"), Some("rgba(255, 255, 255, 0.9)")));
        // No plate, or an opacity that reads as off: transparent.
        d.captions.plate = false;
        assert_eq!(caption_entries(&d, (1920, 1080), &all)[0]["text"]["backgroundColor"], "transparent");
        d.captions.plate = true;
        d.captions.plate_opacity = 0.1;
        assert_eq!(caption_entries(&d, (1920, 1080), &all)[0]["text"]["backgroundColor"], "transparent");
    }

    #[test]
    fn disabled_emits_nothing() {
        let mut d = doc(vec![EditClip::main(0, 5_000)], vec![own(vec![w(0, 100, "a")])]);
        d.captions.enabled = false;
        assert!(caption_entries(&d, (1920, 1080), &all).is_empty());
        d.captions.enabled = true;
        d.transcripts.clear();
        assert!(caption_entries(&d, (1920, 1080), &all).is_empty());
    }

    #[test]
    fn looks_match_upstream_and_classic_is_the_default() {
        let names: Vec<&str> = CAPTION_LOOKS.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["classic", "bold", "minimal", "light"]);
        let d = CaptionSettings::default();
        let classic = CAPTION_LOOKS[0].1;
        assert_eq!((d.font_family.as_str(), d.bold, d.font_size, d.color.as_str()), (classic.font_family, classic.bold, classic.font_size, classic.color));
        assert_eq!((d.plate, d.plate_color.as_str(), d.plate_opacity), (classic.plate, classic.plate_color, classic.plate_opacity));
        for (_, l) in CAPTION_LOOKS {
            assert!(TEXT_FONTS.contains(&l.font_family) && valid_plate_color(l.plate_color));
        }
    }
}
