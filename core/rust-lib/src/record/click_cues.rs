//! Mouse-click sound: the recorded clicks as press/release hits on the PROGRAMME clock.
//!
//! Port of OpenScreen v2.1.2 `src/lib/ai-edition/clickSound.ts` — `buildClickCues` (press per
//! click, a second press for a double-click, a release at the recorded `mouseup` or a synthetic
//! one) and `clickPlacement` (a cue a clip shows lands at its programme time through cuts and
//! speed; a cue no clip shows has no placement). The export mixes these in the compositor
//! (`clickSound.hits`), the editor preview plays the same list. Pure.

use super::editor::{source_to_programme, EditDoc};
use super::project::{CursorRecording, Interaction};
use serde::Serialize;

/// A press with no release inside this window gets a synthetic one `FALLBACK_RELEASE_MS` later.
pub const MAX_RELEASE_GAP_MS: u64 = 600;
pub const FALLBACK_RELEASE_MS: u64 = 110;
/// The second press of a double-click.
pub const DOUBLE_TAP_MS: u64 = 90;
/// Right and middle clicks sit a little quieter than the left.
pub const SIDE_CLICK_GAIN: f64 = 0.85;
/// A release is quieter than its press.
pub const RELEASE_GAIN: f64 = 0.7;

/// One hit on the programme. `gain` is the cue's own level (the user's dB is applied by the
/// scene builder / the preview).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickHit {
    pub prog_ms: u64,
    pub gain: f64,
    pub release: bool,
}

/// `buildClickCues`: `(source ms, gain, release)`, sorted by time.
fn cues(cursor: &CursorRecording) -> Vec<(u64, f64, bool)> {
    let mut sorted: Vec<_> = cursor.samples.iter().collect();
    sorted.sort_by_key(|s| s.time_ms);
    let releases: Vec<u64> =
        sorted.iter().filter(|s| s.interaction_type == Some(Interaction::Mouseup)).map(|s| s.time_ms).collect();
    let mut out = Vec::new();
    for s in &sorted {
        let kind = match s.interaction_type {
            Some(k @ (Interaction::Click | Interaction::DoubleClick | Interaction::RightClick | Interaction::MiddleClick)) => k,
            _ => continue,
        };
        let gain = if matches!(kind, Interaction::RightClick | Interaction::MiddleClick) { SIDE_CLICK_GAIN } else { 1.0 };
        let t = s.time_ms;
        out.push((t, gain, false));
        if kind == Interaction::DoubleClick {
            out.push((t + DOUBLE_TAP_MS, gain, false));
        }
        let release = releases.iter().copied().find(|&ms| ms > t && ms <= t + MAX_RELEASE_GAP_MS);
        out.push((release.unwrap_or(t + FALLBACK_RELEASE_MS), gain * RELEASE_GAIN, true));
    }
    out.sort_by_key(|c| c.0);
    out
}

/// Every click the edit plays, on the programme clock (cuts and speed applied), sorted.
/// A cue inside no clip's source window (cut out, or past the end) is dropped; a source moment
/// two clips both show sounds in each.
pub fn click_hits(doc: &EditDoc, cursor: &CursorRecording) -> Vec<ClickHit> {
    let mut hits = Vec::new();
    for (t, gain, release) in cues(cursor) {
        for (i, c) in doc.clips.iter().enumerate() {
            if t < c.start_ms || t >= c.end_ms {
                continue;
            }
            hits.push(ClickHit { prog_ms: source_to_programme(doc, i, t), gain, release });
        }
    }
    hits.sort_by_key(|h| h.prog_ms);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::editor::{Appearance, CursorStyle, EditClip, OutputShape, SpeedRegion, EDITOR_VERSION};
    use crate::record::project::{CursorProvider, CursorSample};

    fn doc(clips: &[(u64, u64)], speed: Vec<SpeedRegion>) -> EditDoc {
        EditDoc {
            version: EDITOR_VERSION,
            clips: clips.iter().map(|&(s, e)| EditClip::main(s, e)).collect(),
            speed,
            crop: vec![None; clips.len()],
            output: OutputShape::default(),
            mic_gain_db: 0.0,
            appearance: Appearance::plain(),
            cursor: CursorStyle::default(),
            zooms: Vec::new(),
            auto_zoom_done: true,
            annotations: Vec::new(),
            audio_tracks: Vec::new(),
            system_gain_db: 0.0,
        }
    }

    fn sample(t: u64, kind: Interaction) -> CursorSample {
        CursorSample { time_ms: t, cx: 0.5, cy: 0.5, visible: None, asset_id: None, cursor_type: None, interaction_type: Some(kind) }
    }

    fn rec(samples: Vec<CursorSample>) -> CursorRecording {
        CursorRecording { version: 1, provider: CursorProvider::Native, samples, assets: vec![] }
    }

    #[test]
    fn click_hits_project_through_speed() {
        let d = doc(&[(0, 10_000)], vec![SpeedRegion { clip: 0, start_ms: 1_000, end_ms: 3_000, speed: 2.0 }]);
        let hits = click_hits(&d, &rec(vec![sample(3_500, Interaction::Click), sample(3_600, Interaction::Mouseup)]));
        assert_eq!(
            hits,
            vec![
                ClickHit { prog_ms: 2_500, gain: 1.0, release: false },
                ClickHit { prog_ms: 2_600, gain: RELEASE_GAIN, release: true },
            ]
        );
    }

    #[test]
    fn double_click_adds_second_press_at_90ms() {
        let d = doc(&[(0, 10_000)], vec![]);
        let hits = click_hits(&d, &rec(vec![sample(1_000, Interaction::DoubleClick)]));
        let presses: Vec<u64> = hits.iter().filter(|h| !h.release).map(|h| h.prog_ms).collect();
        assert_eq!(presses, vec![1_000, 1_090]);
        // Side clicks are quieter, their releases ×0.7 on top.
        let hits = click_hits(&d, &rec(vec![sample(1_000, Interaction::RightClick)]));
        assert_eq!(hits[0].gain, SIDE_CLICK_GAIN);
        assert!((hits[1].gain - SIDE_CLICK_GAIN * RELEASE_GAIN).abs() < 1e-12);
        // Moves and bare mouseups make no sound.
        assert!(click_hits(&d, &rec(vec![sample(1_000, Interaction::Move), sample(1_200, Interaction::Mouseup)])).is_empty());
    }

    #[test]
    fn release_fallback_110ms() {
        let d = doc(&[(0, 10_000)], vec![]);
        // No mouseup at all.
        let hits = click_hits(&d, &rec(vec![sample(2_000, Interaction::Click)]));
        assert_eq!(hits[1], ClickHit { prog_ms: 2_110, gain: RELEASE_GAIN, release: true });
        // A mouseup past the 600 ms window does not count.
        let hits = click_hits(&d, &rec(vec![sample(2_000, Interaction::Click), sample(2_601, Interaction::Mouseup)]));
        assert_eq!(hits[1].prog_ms, 2_110);
        // A mouseup right at the window's edge does.
        let hits = click_hits(&d, &rec(vec![sample(2_000, Interaction::Click), sample(2_600, Interaction::Mouseup)]));
        assert_eq!(hits[1].prog_ms, 2_600);
    }

    #[test]
    fn cut_out_click_dropped() {
        // 0–4 s and 6.5–10 s: a click at 5 s is cut out; one at 7 s lands at 4 + 0.5 s.
        let d = doc(&[(0, 4_000), (6_500, 10_000)], vec![]);
        let hits = click_hits(&d, &rec(vec![sample(5_000, Interaction::Click), sample(7_000, Interaction::Click)]));
        let presses: Vec<u64> = hits.iter().filter(|h| !h.release).map(|h| h.prog_ms).collect();
        assert_eq!(presses, vec![4_500]);
        // A release cut out with its press still there is dropped too, and the clip end is
        // exclusive (a cue on the seam is not clamped onto it).
        let hits = click_hits(&d, &rec(vec![sample(3_950, Interaction::Click)]));
        assert_eq!(hits, vec![ClickHit { prog_ms: 3_950, gain: 1.0, release: false }]);
    }
}
