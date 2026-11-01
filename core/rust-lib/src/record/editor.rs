//! The editor document (`project.json → editor`): cuts, speed regions, crop per clip,
//! output shape and mic gain — plus its normalisation and the mapping between
//! programme time (what the export plays) and source time (the recording).
//!
//! Pure: no compositor dependency. The mapping ports the math of the compositor's
//! `regions::speed_segments_for_window` (regions sorted by start, gaps play at 1×), so the
//! editor timeline and the export agree on where every frame lands. `view()` is the ONE
//! place the UI gets its timeline geometry from — there must be no second implementation.

use super::project::{Project, RecordingMeta};
use serde::{Deserialize, Serialize};

pub const EDITOR_VERSION: u32 = 1;
/// Clips shorter than this are dropped by `normalize` (a sliver nobody can see or grab).
pub const MIN_CLIP_MS: u64 = 100;
pub const MIN_SPEED: f64 = 0.25;
pub const MAX_SPEED: f64 = 16.0;
/// Smallest crop edge, as a fraction of the source.
pub const MIN_CROP_EDGE: f64 = 0.05;
pub const MIN_MIC_GAIN_DB: f64 = -60.0;
pub const MAX_MIC_GAIN_DB: f64 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditClip {
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedRegion {
    /// Index into `EditDoc::clips`. Times are SOURCE ms of that clip's recording.
    pub clip: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub speed: f64,
}

/// A crop rectangle in fractions (0..1) of the source frame, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CropRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Aspect {
    #[default]
    #[serde(rename = "source")]
    Source,
    #[serde(rename = "16:9")]
    W16H9,
    #[serde(rename = "9:16")]
    W9H16,
    #[serde(rename = "1:1")]
    W1H1,
    #[serde(rename = "4:3")]
    W4H3,
    #[serde(rename = "3:4")]
    W3H4,
    #[serde(rename = "21:9")]
    W21H9,
}

impl Aspect {
    /// Width / height, or `None` for "as the source".
    pub fn ratio(self) -> Option<f64> {
        let (w, h) = match self {
            Aspect::Source => return None,
            Aspect::W16H9 => (16.0, 9.0),
            Aspect::W9H16 => (9.0, 16.0),
            Aspect::W1H1 => (1.0, 1.0),
            Aspect::W4H3 => (4.0, 3.0),
            Aspect::W3H4 => (3.0, 4.0),
            Aspect::W21H9 => (21.0, 9.0),
        };
        Some(w / h)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputTarget {
    P720,
    P1080,
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OutputShape {
    pub aspect: Aspect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditDoc {
    pub version: u32,
    pub clips: Vec<EditClip>,
    #[serde(default)]
    pub speed: Vec<SpeedRegion>,
    /// One entry per clip (`None` = no crop).
    #[serde(default)]
    pub crop: Vec<Option<CropRect>>,
    #[serde(default)]
    pub output: OutputShape,
    #[serde(default)]
    pub mic_gain_db: f64,
}

/// One clip on the programme timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipSpan {
    pub prog_start_ms: u64,
    pub prog_end_ms: u64,
}

/// One speed region on the programme timeline. `index` points into `EditDoc::speed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedSpan {
    pub index: usize,
    pub prog_start_ms: u64,
    pub prog_end_ms: u64,
}

/// What the editor UI draws its timeline from.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorView {
    pub doc: EditDoc,
    pub programme_ms: u64,
    pub clips: Vec<ClipSpan>,
    pub speed: Vec<SpeedSpan>,
}

/// One clip spanning the whole recording, speed 1, no crop.
pub fn default_doc(meta: &RecordingMeta) -> EditDoc {
    EditDoc {
        version: EDITOR_VERSION,
        clips: vec![EditClip { start_ms: 0, end_ms: meta.duration_ms }],
        speed: Vec::new(),
        crop: vec![None],
        output: OutputShape::default(),
        mic_gain_db: 0.0,
    }
}

/// The stored document, normalised. Empty, unparseable or unknown-version blocks fall back
/// to `default_doc` — an editor that cannot read its block must still open the recording.
pub fn load(project: &Project) -> EditDoc {
    let meta = &project.recording;
    match serde_json::from_value::<EditDoc>(project.editor.clone()) {
        Ok(doc) if doc.version == EDITOR_VERSION && !doc.clips.is_empty() => {
            normalize(doc, meta.duration_ms)
        }
        _ => default_doc(meta),
    }
}

pub fn store(project: &mut Project, doc: &EditDoc) {
    // EditDoc is plain data (no maps with non-string keys); serialisation cannot fail.
    project.editor = serde_json::to_value(doc).unwrap_or_else(|_| serde_json::json!({}));
}

fn finite_or(v: f64, fallback: f64) -> f64 {
    if v.is_finite() { v } else { fallback }
}

fn normalize_crop(c: CropRect) -> Option<CropRect> {
    if ![c.x, c.y, c.w, c.h].iter().all(|v| v.is_finite()) {
        return None;
    }
    let w = c.w.clamp(MIN_CROP_EDGE, 1.0);
    let h = c.h.clamp(MIN_CROP_EDGE, 1.0);
    Some(CropRect { x: c.x.clamp(0.0, 1.0 - w), y: c.y.clamp(0.0, 1.0 - h), w, h })
}

/// Clamp everything into range, drop clips under `MIN_CLIP_MS` (one clip always stays),
/// clamp speed regions into their clip and resolve overlaps (a later region in the list
/// cuts its range out of earlier ones). Regions come out sorted by (clip, start).
pub fn normalize(doc: EditDoc, duration_ms: u64) -> EditDoc {
    let EditDoc { clips, speed, mut crop, output, mic_gain_db, .. } = doc;
    crop.resize(clips.len(), None);

    // Clamp clips into the source; remember each survivor's old index.
    let clamped: Vec<(usize, EditClip)> = clips
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let start = c.start_ms.min(duration_ms);
            let end = c.end_ms.min(duration_ms).max(start);
            (i, EditClip { start_ms: start, end_ms: end })
        })
        .collect();
    let mut kept: Vec<(usize, EditClip)> =
        clamped.iter().copied().filter(|(_, c)| c.end_ms - c.start_ms >= MIN_CLIP_MS).collect();
    if kept.is_empty() {
        // Keep the longest clip that has any length (first wins on a tie); else the whole source.
        let longest = clamped
            .iter()
            .copied()
            .filter(|(_, c)| c.end_ms > c.start_ms)
            .fold(None::<(usize, EditClip)>, |best, cur| match best {
                Some(b) if b.1.end_ms - b.1.start_ms >= cur.1.end_ms - cur.1.start_ms => Some(b),
                _ => Some(cur),
            });
        kept = match longest {
            Some(c) => vec![c],
            None => vec![(usize::MAX, EditClip { start_ms: 0, end_ms: duration_ms })],
        };
    }
    let new_index = |old: usize| kept.iter().position(|(o, _)| *o == old);
    let new_clips: Vec<EditClip> = kept.iter().map(|(_, c)| *c).collect();
    let new_crop: Vec<Option<CropRect>> = kept
        .iter()
        .map(|(old, _)| crop.get(*old).copied().flatten().and_then(normalize_crop))
        .collect();

    // Speed regions: re-index, clamp, then later-wins overlap resolution per clip.
    let mut resolved: Vec<SpeedRegion> = Vec::new();
    for r in speed {
        let Some(clip) = new_index(r.clip) else { continue };
        let c = new_clips[clip];
        let start = r.start_ms.clamp(c.start_ms, c.end_ms);
        let end = r.end_ms.clamp(c.start_ms, c.end_ms);
        if end <= start {
            continue;
        }
        let speed = finite_or(r.speed, 1.0).clamp(MIN_SPEED, MAX_SPEED);
        let mut next = Vec::with_capacity(resolved.len() + 1);
        for e in resolved {
            if e.clip != clip || e.end_ms <= start || e.start_ms >= end {
                next.push(e);
                continue;
            }
            if e.start_ms < start {
                next.push(SpeedRegion { end_ms: start, ..e });
            }
            if e.end_ms > end {
                next.push(SpeedRegion { start_ms: end, ..e });
            }
        }
        next.push(SpeedRegion { clip, start_ms: start, end_ms: end, speed });
        resolved = next;
    }
    resolved.sort_by_key(|r| (r.clip, r.start_ms));

    EditDoc {
        version: EDITOR_VERSION,
        clips: new_clips,
        speed: resolved,
        crop: new_crop,
        output,
        mic_gain_db: finite_or(mic_gain_db, 0.0).clamp(MIN_MIC_GAIN_DB, MAX_MIC_GAIN_DB),
    }
}

/// One clip's source window cut into speed segments `(start_ms, end_ms, speed)` — the port
/// of `regions::speed_segments_for_window`: regions of this clip sorted by start, clipped to
/// the window and to what earlier regions already covered; gaps play at 1×.
fn segments(doc: &EditDoc, clip: usize) -> Vec<(f64, f64, f64)> {
    let Some(c) = doc.clips.get(clip) else { return Vec::new() };
    let (ws, we) = (c.start_ms as f64, c.end_ms as f64);
    if we <= ws {
        return Vec::new();
    }
    let mut regions: Vec<&SpeedRegion> = doc
        .speed
        .iter()
        .filter(|r| r.clip == clip && (r.start_ms as f64) < we && (r.end_ms as f64) > ws)
        .collect();
    regions.sort_by_key(|r| r.start_ms);
    let mut out = Vec::new();
    let mut cursor = ws;
    for r in regions {
        let start = (r.start_ms as f64).max(ws).max(cursor);
        let end = (r.end_ms as f64).min(we);
        if end <= start {
            continue;
        }
        if start > cursor {
            out.push((cursor, start, 1.0));
        }
        let speed = if r.speed.is_finite() && r.speed > 0.0 { r.speed } else { 1.0 };
        out.push((start, end, speed));
        cursor = end;
    }
    if cursor < we {
        out.push((cursor, we, 1.0));
    }
    out
}

fn clip_programme_ms(doc: &EditDoc, clip: usize) -> f64 {
    segments(doc, clip).iter().map(|(s, e, v)| (e - s) / v).sum()
}

fn round_ms(v: f64) -> u64 {
    if v <= 0.0 { 0 } else { v.round() as u64 }
}

pub fn programme_duration_ms(doc: &EditDoc) -> u64 {
    round_ms((0..doc.clips.len()).map(|i| clip_programme_ms(doc, i)).sum())
}

/// Where programme time `t_ms` lands in the source: `(clip, source ms)`. The very end of
/// the programme maps to the last clip's end; anything past it is `None`.
pub fn programme_to_source(doc: &EditDoc, t_ms: u64) -> Option<(usize, u64)> {
    let t = t_ms as f64;
    let mut acc = 0.0;
    let mut last = None;
    for clip in 0..doc.clips.len() {
        for (s, e, v) in segments(doc, clip) {
            let len = (e - s) / v;
            if t < acc + len {
                return Some((clip, round_ms(s + (t - acc) * v)));
            }
            acc += len;
            last = Some((clip, round_ms(e)));
        }
    }
    if t <= acc + 0.5 { last } else { None }
}

/// Programme time of `source_ms` within `clip` (clamped into the clip's window).
pub fn source_to_programme(doc: &EditDoc, clip: usize, source_ms: u64) -> u64 {
    let Some(c) = doc.clips.get(clip) else { return programme_duration_ms(doc) };
    let src = (source_ms.clamp(c.start_ms, c.end_ms.max(c.start_ms))) as f64;
    let mut acc: f64 = (0..clip).map(|i| clip_programme_ms(doc, i)).sum();
    for (s, e, v) in segments(doc, clip) {
        if src <= s {
            break;
        }
        acc += (src.min(e) - s) / v;
    }
    round_ms(acc)
}

/// The timeline geometry the editor UI draws: clip and speed spans in programme ms.
pub fn view(doc: &EditDoc) -> EditorView {
    let clips = doc
        .clips
        .iter()
        .enumerate()
        .map(|(i, c)| ClipSpan {
            prog_start_ms: source_to_programme(doc, i, c.start_ms),
            prog_end_ms: source_to_programme(doc, i, c.end_ms),
        })
        .collect();
    let speed = doc
        .speed
        .iter()
        .enumerate()
        .filter(|(_, r)| r.clip < doc.clips.len())
        .map(|(index, r)| SpeedSpan {
            index,
            prog_start_ms: source_to_programme(doc, r.clip, r.start_ms),
            prog_end_ms: source_to_programme(doc, r.clip, r.end_ms),
        })
        .collect();
    EditorView { doc: doc.clone(), programme_ms: programme_duration_ms(doc), clips, speed }
}

fn even_round(v: f64) -> u32 {
    (((v / 2.0).round() as u32) * 2).max(2)
}

fn even_floor(v: f64) -> u32 {
    (((v / 2.0).floor() as u32) * 2).max(2)
}

/// Export frame size. Even on both axes (H.264/HEVC need it). For 720p/1080p the SHORT side
/// is 720/1080; at source resolution it is the largest rect of the aspect inside the source.
pub fn output_size(meta: &RecordingMeta, aspect: Aspect, target: OutputTarget) -> (u32, u32) {
    let (sw, sh) = if meta.width > 0 && meta.height > 0 {
        (meta.width as f64, meta.height as f64)
    } else {
        (1920.0, 1080.0)
    };
    let ratio = aspect.ratio().unwrap_or(sw / sh);
    let short = match target {
        OutputTarget::P720 => 720.0,
        OutputTarget::P1080 => 1080.0,
        OutputTarget::Source => {
            let (w, h) = if sw / sh > ratio { (sh * ratio, sh) } else { (sw, sw / ratio) };
            return (even_floor(w + 1e-6), even_floor(h + 1e-6));
        }
    };
    if ratio >= 1.0 {
        (even_round(short * ratio), even_round(short))
    } else {
        (even_round(short), even_round(short / ratio))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::project::{MicMeta, ProjectStatus, PROJECT_SCHEMA};

    pub(crate) fn meta(duration_ms: u64) -> RecordingMeta {
        RecordingMeta {
            width: 3024,
            height: 1964,
            fps: 60,
            duration_ms,
            system_audio: true,
            mic: Some(MicMeta { offset_ms: 120, sample_rate: 48_000 }),
            ..RecordingMeta::default()
        }
    }

    fn project_with(editor: serde_json::Value) -> Project {
        Project {
            schema: PROJECT_SCHEMA,
            id: "take".into(),
            title: String::new(),
            created_at: 0,
            updated_at: 0,
            status: ProjectStatus::Ready,
            failure: None,
            recording: meta(10_000),
            editor,
        }
    }

    fn clip(start_ms: u64, end_ms: u64) -> EditClip {
        EditClip { start_ms, end_ms }
    }

    fn sr(clip: usize, start_ms: u64, end_ms: u64, speed: f64) -> SpeedRegion {
        SpeedRegion { clip, start_ms, end_ms, speed }
    }

    fn doc(clips: Vec<EditClip>, speed: Vec<SpeedRegion>) -> EditDoc {
        let n = clips.len();
        EditDoc {
            version: EDITOR_VERSION,
            clips,
            speed,
            crop: vec![None; n],
            output: OutputShape::default(),
            mic_gain_db: 0.0,
        }
    }

    /// The brief's example: one clip 0–10 s, ×2 on 2–6 s.
    fn example() -> EditDoc {
        doc(vec![clip(0, 10_000)], vec![sr(0, 2_000, 6_000, 2.0)])
    }

    #[test]
    fn default_doc_from_meta() {
        let d = default_doc(&meta(7_500));
        assert_eq!(d.version, EDITOR_VERSION);
        assert_eq!(d.clips, vec![clip(0, 7_500)]);
        assert!(d.speed.is_empty());
        assert_eq!(d.crop, vec![None]);
        assert_eq!(d.output.aspect, Aspect::Source);
        assert_eq!(d.mic_gain_db, 0.0);
    }

    #[test]
    fn load_falls_back_on_garbage() {
        let def = default_doc(&meta(10_000));
        for editor in [
            serde_json::json!({}),
            serde_json::json!(null),
            serde_json::json!("garbage"),
            serde_json::json!({"version": 1, "clips": "nope"}),
            serde_json::json!({"version": 2, "clips": [{"startMs": 0, "endMs": 5000}]}),
            serde_json::json!({"version": 1, "clips": []}),
        ] {
            assert_eq!(load(&project_with(editor.clone())), def, "{editor}");
        }
    }

    #[test]
    fn store_then_load_roundtrips() {
        let mut p = project_with(serde_json::json!({}));
        let mut d = example();
        d.crop = vec![Some(CropRect { x: 0.1, y: 0.2, w: 0.5, h: 0.5 })];
        d.output.aspect = Aspect::W9H16;
        d.mic_gain_db = -6.0;
        store(&mut p, &d);
        assert_eq!(p.editor["version"], 1);
        assert_eq!(p.editor["output"]["aspect"], "9:16");
        assert_eq!(p.editor["micGainDb"], -6.0);
        assert_eq!(load(&p), d);
    }

    #[test]
    fn normalize_clamps_and_drops_tiny_clips() {
        let mut d = doc(
            vec![clip(0, 50), clip(2_000, 99_000), clip(4_000, 4_000)],
            vec![sr(1, 1_000, 50_000, 40.0), sr(0, 0, 50, 2.0), sr(1, 3_000, 4_000, 0.1)],
        );
        d.crop = vec![
            None,
            Some(CropRect { x: -0.5, y: 0.98, w: 0.01, h: 2.0 }),
            Some(CropRect { x: 0.0, y: 0.0, w: 0.5, h: 0.5 }),
        ];
        d.mic_gain_db = 40.0;
        let n = normalize(d, 10_000);
        // Clip 0 (50 ms) and clip 2 (empty) go; clip 1 is clamped into the source.
        assert_eq!(n.clips, vec![clip(2_000, 10_000)]);
        // Regions follow their clip's new index, are clamped into it, speed into 0.25..16;
        // the region on the dropped clip goes with it.
        assert_eq!(n.speed, vec![sr(0, 2_000, 3_000, 16.0), sr(0, 3_000, 4_000, 0.25), sr(0, 4_000, 10_000, 16.0)]);
        // Crop of the surviving clip: min edge 0.05, inside 0..1.
        assert_eq!(n.crop, vec![Some(CropRect { x: 0.0, y: 0.0, w: MIN_CROP_EDGE, h: 1.0 })]);
        assert_eq!(n.mic_gain_db, MAX_MIC_GAIN_DB);
        let quiet = normalize(EditDoc { mic_gain_db: -200.0, ..example() }, 10_000);
        assert_eq!(quiet.mic_gain_db, MIN_MIC_GAIN_DB);
    }

    #[test]
    fn normalize_rejects_non_finite_numbers() {
        let mut d = doc(vec![clip(0, 10_000)], vec![sr(0, 0, 1_000, f64::NAN)]);
        d.crop = vec![Some(CropRect { x: f64::NAN, y: 0.0, w: 0.5, h: 0.5 })];
        d.mic_gain_db = f64::NAN;
        let n = normalize(d, 10_000);
        assert_eq!(n.speed[0].speed, 1.0);
        assert_eq!(n.crop, vec![None]);
        assert_eq!(n.mic_gain_db, 0.0);
    }

    #[test]
    fn normalize_overlap_later_wins() {
        let d = doc(
            vec![clip(0, 10_000), clip(0, 10_000)],
            vec![
                sr(0, 1_000, 5_000, 2.0),
                sr(0, 3_000, 4_000, 4.0),  // cuts a hole into the first
                sr(1, 1_000, 5_000, 2.0),  // other clip: untouched by clip 0's regions
                sr(0, 0, 2_000, 0.5),      // eats the head of the first
            ],
        );
        let n = normalize(d, 10_000);
        assert_eq!(
            n.speed,
            vec![
                sr(0, 0, 2_000, 0.5),
                sr(0, 2_000, 3_000, 2.0),
                sr(0, 3_000, 4_000, 4.0),
                sr(0, 4_000, 5_000, 2.0),
                sr(1, 1_000, 5_000, 2.0),
            ]
        );
    }

    #[test]
    fn normalize_keeps_one_clip() {
        // Every clip too short: the longest survives rather than leaving nothing to play.
        let n = normalize(doc(vec![clip(0, 30), clip(500, 580), clip(900, 950)], vec![]), 10_000);
        assert_eq!(n.clips, vec![clip(500, 580)]);
        assert_eq!(n.crop, vec![None]);
        // Nothing usable at all: the whole source.
        let n = normalize(doc(vec![clip(20_000, 30_000)], vec![]), 10_000);
        assert_eq!(n.clips, vec![clip(0, 10_000)]);
        let n = normalize(doc(vec![], vec![]), 10_000);
        assert_eq!(n.clips, vec![clip(0, 10_000)]);
        assert_eq!(n.version, EDITOR_VERSION);
    }

    #[test]
    fn programme_mapping_roundtrip() {
        let d = example();
        assert_eq!(programme_duration_ms(&d), 8_000);
        assert_eq!(programme_to_source(&d, 0), Some((0, 0)));
        assert_eq!(programme_to_source(&d, 1_000), Some((0, 1_000)));
        assert_eq!(programme_to_source(&d, 3_000), Some((0, 4_000)));
        assert_eq!(programme_to_source(&d, 5_000), Some((0, 7_000)));
        assert_eq!(programme_to_source(&d, 8_000), Some((0, 10_000)));
        assert_eq!(programme_to_source(&d, 8_001), None);
        assert_eq!(source_to_programme(&d, 0, 7_000), 5_000);
        assert_eq!(source_to_programme(&d, 0, 4_000), 3_000);
        // Out-of-clip source times clamp to the clip's edges.
        assert_eq!(source_to_programme(&d, 0, 99_000), 8_000);
        for t in (0..=8_000).step_by(37) {
            let (c, s) = programme_to_source(&d, t).unwrap();
            let back = source_to_programme(&d, c, s);
            assert!(back.abs_diff(t) <= 1, "t={t} s={s} back={back}");
        }
    }

    #[test]
    fn programme_mapping_across_clips() {
        // Clip A = source 1–3 s at 1×, clip B = source 6–10 s with ×4 on 6–8 s and ×0.5 on 9–10 s.
        let d = doc(
            vec![clip(1_000, 3_000), clip(6_000, 10_000)],
            vec![sr(1, 6_000, 8_000, 4.0), sr(1, 9_000, 10_000, 0.5)],
        );
        // 2000 + (500 + 1000 + 2000) = 5500
        assert_eq!(programme_duration_ms(&d), 5_500);
        assert_eq!(programme_to_source(&d, 0), Some((0, 1_000)));
        assert_eq!(programme_to_source(&d, 1_999), Some((0, 2_999)));
        assert_eq!(programme_to_source(&d, 2_000), Some((1, 6_000)));
        assert_eq!(programme_to_source(&d, 2_250), Some((1, 7_000)));
        assert_eq!(programme_to_source(&d, 3_000), Some((1, 8_500)));
        assert_eq!(programme_to_source(&d, 4_500), Some((1, 9_500)));
        assert_eq!(source_to_programme(&d, 1, 8_000), 2_500);
        assert_eq!(source_to_programme(&d, 1, 10_000), 5_500);
        assert_eq!(source_to_programme(&d, 0, 3_000), 2_000);
        // The same source time in two clips maps to two programme times.
        let twice = doc(vec![clip(0, 2_000), clip(0, 2_000)], vec![]);
        assert_eq!(source_to_programme(&twice, 0, 1_000), 1_000);
        assert_eq!(source_to_programme(&twice, 1, 1_000), 3_000);
    }

    #[test]
    fn view_spans_match_mapping() {
        let v = view(&example());
        assert_eq!(v.programme_ms, 8_000);
        assert_eq!(v.clips, vec![ClipSpan { prog_start_ms: 0, prog_end_ms: 8_000 }]);
        assert_eq!(v.speed, vec![SpeedSpan { index: 0, prog_start_ms: 2_000, prog_end_ms: 4_000 }]);
        assert_eq!(v.doc, example());

        let d = doc(
            vec![clip(1_000, 3_000), clip(6_000, 10_000)],
            vec![sr(1, 9_000, 10_000, 0.5), sr(1, 6_000, 8_000, 4.0)],
        );
        let v = view(&d);
        assert_eq!(v.programme_ms, programme_duration_ms(&d));
        for (i, c) in v.clips.iter().enumerate() {
            assert_eq!(c.prog_start_ms, source_to_programme(&d, i, d.clips[i].start_ms));
            assert_eq!(c.prog_end_ms, source_to_programme(&d, i, d.clips[i].end_ms));
        }
        for s in &v.speed {
            let r = d.speed[s.index];
            assert_eq!(s.prog_start_ms, source_to_programme(&d, r.clip, r.start_ms));
            assert_eq!(s.prog_end_ms, source_to_programme(&d, r.clip, r.end_ms));
            assert_eq!(programme_to_source(&d, s.prog_start_ms), Some((r.clip, r.start_ms)));
        }
        // Indices point back into the document as given (not re-sorted).
        assert_eq!(v.speed.iter().map(|s| s.index).collect::<Vec<_>>(), vec![0, 1]);
        // camelCase on the wire.
        let j = serde_json::to_value(&v).unwrap();
        assert!(j.get("programmeMs").is_some() && j["clips"][0].get("progStartMs").is_some());
    }

    #[test]
    fn output_size_presets() {
        let m = meta(1_000);
        assert_eq!(output_size(&m, Aspect::W16H9, OutputTarget::P1080), (1920, 1080));
        assert_eq!(output_size(&m, Aspect::W9H16, OutputTarget::P1080), (1080, 1920));
        assert_eq!(output_size(&m, Aspect::Source, OutputTarget::Source), (3024, 1964));
        assert_eq!(output_size(&m, Aspect::W16H9, OutputTarget::P720), (1280, 720));
        assert_eq!(output_size(&m, Aspect::W1H1, OutputTarget::P720), (720, 720));
        assert_eq!(output_size(&m, Aspect::W21H9, OutputTarget::P1080), (2520, 1080));
        // Source aspect at 1080p: short side 1080, long side scaled and even.
        assert_eq!(output_size(&m, Aspect::Source, OutputTarget::P1080), (1662, 1080));
        // A fixed aspect at source resolution: the largest such rect inside the source.
        assert_eq!(output_size(&m, Aspect::W9H16, OutputTarget::Source), (1104, 1964));
        assert_eq!(output_size(&m, Aspect::W16H9, OutputTarget::Source), (3024, 1700));
        // Every size is even (H.264/HEVC need it).
        let odd = RecordingMeta { width: 1001, height: 777, ..m.clone() };
        for a in [Aspect::Source, Aspect::W16H9, Aspect::W9H16, Aspect::W4H3, Aspect::W3H4, Aspect::W21H9] {
            for t in [OutputTarget::P720, OutputTarget::P1080, OutputTarget::Source] {
                let (w, h) = output_size(&odd, a, t);
                assert!(w % 2 == 0 && h % 2 == 0 && w >= 2 && h >= 2, "{a:?} {t:?} → {w}×{h}");
            }
        }
        // An unknown source size falls back to 16:9 rather than dividing by zero.
        let unknown = RecordingMeta { width: 0, height: 0, ..m };
        assert_eq!(output_size(&unknown, Aspect::Source, OutputTarget::P1080), (1920, 1080));
    }
}
