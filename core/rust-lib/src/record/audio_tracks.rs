//! Music and voiceover tracks on the programme clock — the port of OpenScreen
//! `src/native/sceneDescription.ts:915-1047` (the `audioTracks` the compositor mixes).
//!
//! OpenScreen keeps a track on a RAW ruler (every clip at full length, trims as removed
//! stretches). Our document has no ruler: a clip is a window of its recording and a cut is
//! the gap between two windows of the same recording. So a track's raw clock starts at its
//! anchor `(clip, start_ms)` and walks the clips in programme order at 1× source time; where
//! the next clip continues the same recording after a gap, that gap is removed time (what
//! OpenScreen's trims are); a clip of another recording, or one that starts earlier (a
//! reordered or duplicated clip), simply follows on. Past the last clip the raw clock keeps
//! running on the programme clock (OpenScreen: "unfilmed", not removed).
//!
//! The rules, as upstream:
//! - every track is mixed once, from its head (our document stores one entry per track);
//! - a voiceover without loop is split around cuts (its words go with the video); the
//!   pieces play at 1×, each stops where the next one begins, fades only on the outer edges;
//! - music without loop is one entry, shortened by the cuts it spans;
//! - a loop is one entry per repeat (≤ [`MAX_LOOP_ENTRIES`]), fades only on the outer edges;
//! - speed regions move where a track starts, they never stretch it;
//! - `trimEndSec` is always set (the mixer skips a track without one);
//! - muted tracks and missing files are left out (`project_tracks`; the timeline still shows
//!   them, from [`track_entries`]).
//!
//! R12: this is the only place a track's programme position is computed.

use super::editor::{programme_duration_ms, source_to_programme, track_span_ms, AudioTrack, EditDoc, TrackKind};
use super::scene_json::project_asset;
use serde::Serialize;
use std::path::Path;

/// Most entries one looping track becomes (`sceneDescription.ts:1033`).
pub const MAX_LOOP_ENTRIES: usize = 1000;

/// One entry of the compositor's `audioTracks` (`V/src/scene.rs` `SceneAudioTrack`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneTrack {
    pub path: String,
    /// On the OUTPUT programme.
    pub start_sec: f64,
    pub gain_db: f64,
    pub trim_start_sec: f64,
    pub trim_end_sec: f64,
    pub fade_in_sec: f64,
    pub fade_out_sec: f64,
    pub kind: TrackKind,
}

/// One entry of a track on the programme clock, in ms, with the track it comes from — what
/// the scene mixes and what the timeline and the preview play (`EditorView.tracks`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackSpan {
    /// Index into `EditDoc::audio_tracks`.
    pub index: usize,
    /// Which repeat of a looping track (0 otherwise).
    pub repeat: u32,
    pub prog_start_ms: f64,
    pub prog_end_ms: f64,
    /// The window of the file this entry plays (file ms).
    pub file_start_ms: f64,
    pub file_end_ms: f64,
    pub fade_in_ms: f64,
    pub fade_out_ms: f64,
    pub gain_db: f64,
    pub kind: TrackKind,
    pub muted: bool,
}

/// One stretch of a track's raw clock: `play` = heard at `prog_ms`, else removed by a cut.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Piece {
    play: bool,
    raw_start: f64,
    raw_end: f64,
    prog_ms: f64,
}

/// The raw clock of a track anchored at `(clip, start_ms)`, cut into play / removed stretches,
/// up to `raw_len` ms. `None` when the anchor clip does not play (missing, or zero length).
fn walk(doc: &EditDoc, clip: usize, start_ms: u64, raw_len: f64) -> Option<Vec<Piece>> {
    let anchor = doc.clips.get(clip)?;
    if anchor.end_ms <= anchor.start_ms {
        return None;
    }
    let mut pieces = Vec::new();
    let mut raw = 0.0;
    let mut i = clip;
    let mut pos = start_ms.clamp(anchor.start_ms, anchor.end_ms);
    // The clip the walk came from (for the gap check), skipping clips that do not play.
    let mut prev: Option<usize> = None;
    while raw < raw_len {
        let Some(c) = doc.clips.get(i) else {
            // Past the last clip: the rest plays on the programme clock.
            let end = programme_duration_ms(doc) as f64;
            pieces.push(Piece { play: true, raw_start: raw, raw_end: raw_len, prog_ms: end });
            break;
        };
        if c.end_ms <= c.start_ms {
            i += 1;
            continue;
        }
        if let Some(p) = prev {
            let pc = &doc.clips[p];
            if pc.source == c.source && c.start_ms >= pc.end_ms {
                let gap = (c.start_ms - pc.end_ms) as f64;
                if gap > 0.0 {
                    let end = (raw + gap).min(raw_len);
                    pieces.push(Piece { play: false, raw_start: raw, raw_end: end, prog_ms: 0.0 });
                    raw = end;
                    if raw >= raw_len {
                        break;
                    }
                }
            }
            pos = c.start_ms;
        }
        let len = (c.end_ms - pos) as f64;
        let end = (raw + len).min(raw_len);
        if end > raw {
            pieces.push(Piece { play: true, raw_start: raw, raw_end: end, prog_ms: source_to_programme(doc, i, pos) as f64 });
        }
        raw = end;
        prev = Some(i);
        i += 1;
    }
    Some(pieces)
}

/// Removed time inside the first `raw_len` ms of the walk.
fn removed_ms(pieces: &[Piece]) -> f64 {
    pieces.iter().filter(|p| !p.play).map(|p| p.raw_end - p.raw_start).sum()
}

/// Every track's entries on the programme clock. `doc` is the document as it plays
/// (`editor::playable`: a missing take's clip lasts no time, its tracks are left out). Muted
/// tracks are included (flagged); files are not checked here.
pub fn track_entries(doc: &EditDoc) -> Vec<TrackSpan> {
    let programme = programme_duration_ms(doc) as f64;
    let mut out = Vec::new();
    for (index, t) in doc.audio_tracks.iter().enumerate() {
        out.extend(entries_of(doc, index, t, programme));
    }
    out
}

fn entries_of(doc: &EditDoc, index: usize, t: &AudioTrack, programme: f64) -> Vec<TrackSpan> {
    let Some(head) = doc.clips.get(t.clip) else { return Vec::new() };
    if head.end_ms <= head.start_ms {
        return Vec::new();
    }
    let offset = t.offset_ms as f64;
    let duration = t.duration_ms as f64;
    let head_prog = source_to_programme(doc, t.clip, t.start_ms) as f64;
    let entry = |repeat: u32, prog: f64, file_start: f64, file_end: f64, fade_in: f64, fade_out: f64| TrackSpan {
        index,
        repeat,
        prog_start_ms: prog,
        prog_end_ms: prog + (file_end - file_start),
        file_start_ms: file_start,
        file_end_ms: file_end,
        fade_in_ms: fade_in,
        fade_out_ms: fade_out,
        gain_db: t.gain_db,
        kind: t.kind,
        muted: t.muted,
    };
    let (fade_in, fade_out) = (t.fade_in_ms as f64, t.fade_out_ms as f64);

    if t.kind == TrackKind::Voiceover && !t.looped {
        let raw_len = track_span_ms(t).unwrap_or(0) as f64;
        // The file left after the offset; an unprobed file (duration 0) is capped by the span.
        let window = if duration > 0.0 { (duration - offset).max(0.0) } else { raw_len };
        let Some(pieces) = walk(doc, t.clip, t.start_ms, raw_len) else { return Vec::new() };
        // Pieces split only at cuts: consecutive heard stretches are one 1× piece.
        let mut kept: Vec<(f64, f64, f64)> = Vec::new(); // (prog, file_start, file_end)
        let mut after_cut = true;
        for p in &pieces {
            if !p.play {
                after_cut = true;
                continue;
            }
            let (fs, fe) = (offset + p.raw_start, (offset + p.raw_end).min(offset + window));
            if after_cut || kept.is_empty() {
                kept.push((p.prog_ms, fs, fe));
            } else if let Some(last) = kept.last_mut() {
                last.2 = fe;
            }
            after_cut = false;
        }
        kept.retain(|(_, fs, fe)| fe > fs);
        let n = kept.len();
        return (0..n)
            .map(|i| {
                let (prog, fs, mut fe) = kept[i];
                // A piece plays at 1×; it stops where the next one begins on the programme.
                if let Some(next) = kept.get(i + 1) {
                    fe = fe.min(fs + (next.0 - prog));
                }
                entry(0, prog, fs, fe, if i == 0 { fade_in } else { 0.0 }, if i == n - 1 { fade_out } else { 0.0 })
            })
            .filter(|e| e.file_end_ms > e.file_start_ms)
            .collect();
    }

    // Music (and a looping voiceover): the span is the raw length less the cuts it covers. A
    // loop without a set span fills the programme from its head.
    let raw_len = match track_span_ms(t) {
        Some(s) => s as f64,
        None => (programme - head_prog).max(0.0),
    };
    let Some(pieces) = walk(doc, t.clip, t.start_ms, raw_len) else { return Vec::new() };
    let span = if t.looped && t.span_ms.is_none() { raw_len } else { raw_len - removed_ms(&pieces) };
    if span <= 0.0 {
        return Vec::new();
    }
    let window = if duration > 0.0 { (duration - offset).max(0.0) } else { span };
    if window <= 0.0 {
        return Vec::new();
    }
    if !t.looped {
        return vec![entry(0, head_prog, offset, offset + window.min(span), fade_in, fade_out)];
    }
    let mut out = Vec::new();
    let mut played = 0.0;
    while played < span && out.len() < MAX_LOOP_ENTRIES {
        let this = window.min(span - played);
        out.push(entry(
            out.len() as u32,
            head_prog + played,
            offset,
            offset + this,
            if played == 0.0 { fade_in } else { 0.0 },
            if played + this >= span { fade_out } else { 0.0 },
        ));
        played += window;
    }
    out
}

/// The compositor's `audioTracks`: every unmuted entry whose file is a plain name that exists
/// under `<project>/assets/` (anything else is left out, never a helper error).
pub fn project_tracks(doc: &EditDoc, project: &Path) -> Vec<SceneTrack> {
    track_entries(doc)
        .into_iter()
        .filter(|e| !e.muted)
        .filter_map(|e| {
            let path = project_asset(project, &doc.audio_tracks[e.index].file)?;
            Some(SceneTrack {
                path: path.to_string_lossy().into_owned(),
                start_sec: e.prog_start_ms / 1000.0,
                gain_db: e.gain_db,
                trim_start_sec: e.file_start_ms / 1000.0,
                trim_end_sec: e.file_end_ms / 1000.0,
                fade_in_sec: e.fade_in_ms / 1000.0,
                fade_out_sec: e.fade_out_ms / 1000.0,
                kind: e.kind,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::editor::{EditClip, SpeedRegion};

    fn doc(clips: &[(u64, u64)]) -> EditDoc {
        let mut d = crate::record::editor::default_doc(&crate::record::editor::tests::meta(60_000));
        d.clips = clips.iter().map(|&(s, e)| EditClip::main(s, e)).collect();
        d.crop = vec![None; clips.len()];
        d
    }

    fn track(kind: TrackKind, clip: usize, start_ms: u64, duration_ms: u64) -> AudioTrack {
        AudioTrack {
            id: "t".into(),
            kind,
            file: "a.wav".into(),
            clip,
            start_ms,
            duration_ms,
            offset_ms: 0,
            span_ms: if kind == TrackKind::Voiceover { Some(duration_ms) } else { None },
            gain_db: -18.0,
            looped: false,
            fade_in_ms: 1000,
            fade_out_ms: 1000,
            muted: false,
            label: String::new(),
        }
    }

    fn tmp_project(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ir-audio-tracks-{name}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("assets/a.wav"), b"x").unwrap();
        dir
    }

    #[test]
    fn music_loop_fills_programme() {
        let mut d = doc(&[(0, 35_000)]);
        let mut t = track(TrackKind::Music, 0, 0, 10_000);
        t.looped = true;
        d.audio_tracks = vec![t];
        let e = track_entries(&d);
        assert_eq!(e.len(), 4);
        assert_eq!(e.iter().map(|e| e.prog_start_ms).collect::<Vec<_>>(), vec![0.0, 10_000.0, 20_000.0, 30_000.0]);
        assert_eq!(e[3].file_end_ms, 5_000.0, "the last repeat is cut where the programme ends");
        assert_eq!(e.iter().map(|e| e.fade_in_ms).collect::<Vec<_>>(), vec![1000.0, 0.0, 0.0, 0.0]);
        assert_eq!(e.iter().map(|e| e.fade_out_ms).collect::<Vec<_>>(), vec![0.0, 0.0, 0.0, 1000.0]);
        assert_eq!(e.iter().map(|e| e.repeat).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
    }

    #[test]
    fn music_without_loop_single_entry() {
        let mut d = doc(&[(0, 35_000)]);
        d.audio_tracks = vec![track(TrackKind::Music, 0, 2_000, 10_000)];
        let e = track_entries(&d);
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].prog_start_ms, e[0].file_start_ms, e[0].file_end_ms), (2_000.0, 0.0, 10_000.0));
        assert_eq!((e[0].fade_in_ms, e[0].fade_out_ms), (1000.0, 1000.0));
        // A cut inside its span shortens it (a bed plays through and ends early).
        let mut d = doc(&[(0, 5_000), (8_000, 35_000)]);
        d.audio_tracks = vec![track(TrackKind::Music, 0, 2_000, 10_000)];
        let e = track_entries(&d);
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].prog_start_ms, e[0].file_end_ms), (2_000.0, 7_000.0));
    }

    #[test]
    fn voiceover_split_around_cut() {
        // Clips 0–10 s and 15–30 s of one recording: 10–15 s was cut. A 10 s voiceover from
        // 8 s plays 2 s, loses the 5 s that were cut, plays the last 3 s from programme 10 s.
        let mut d = doc(&[(0, 10_000), (15_000, 30_000)]);
        let mut t = track(TrackKind::Voiceover, 0, 8_000, 10_000);
        t.offset_ms = 500;
        t.duration_ms = 20_000;
        d.audio_tracks = vec![t];
        let e = track_entries(&d);
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].prog_start_ms, e[0].file_start_ms, e[0].file_end_ms), (8_000.0, 500.0, 2_500.0));
        assert_eq!((e[1].prog_start_ms, e[1].file_start_ms, e[1].file_end_ms), (10_000.0, 7_500.0, 10_500.0));
        assert_eq!((e[0].fade_in_ms, e[0].fade_out_ms), (1000.0, 0.0));
        assert_eq!((e[1].fade_in_ms, e[1].fade_out_ms), (0.0, 1000.0));
        // Two clips that follow on without a gap (a split) are one piece.
        let mut d = doc(&[(0, 10_000), (10_000, 30_000)]);
        d.audio_tracks = vec![track(TrackKind::Voiceover, 0, 8_000, 10_000)];
        assert_eq!(track_entries(&d).len(), 1);
    }

    #[test]
    fn speed_shifts_track_start_only() {
        // 2× over 0–4 s: a track at source 6 s starts at programme 4 s, still 10 s long.
        let mut d = doc(&[(0, 35_000)]);
        d.speed = vec![SpeedRegion { clip: 0, start_ms: 0, end_ms: 4_000, speed: 2.0 }];
        d.audio_tracks = vec![track(TrackKind::Music, 0, 6_000, 10_000), track(TrackKind::Voiceover, 0, 6_000, 10_000)];
        for e in track_entries(&d) {
            assert_eq!(e.prog_start_ms, 4_000.0);
            assert_eq!(e.file_end_ms - e.file_start_ms, 10_000.0);
        }
        // A voiceover crossing a sped-up stretch ends where its next piece starts.
        let mut d = doc(&[(0, 10_000), (12_000, 30_000)]);
        d.speed = vec![SpeedRegion { clip: 0, start_ms: 2_000, end_ms: 10_000, speed: 2.0 }];
        d.audio_tracks = vec![track(TrackKind::Voiceover, 0, 2_000, 20_000)];
        let e = track_entries(&d);
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].prog_start_ms, e[1].prog_start_ms), (2_000.0, 6_000.0));
        assert_eq!(e[0].file_end_ms - e[0].file_start_ms, 4_000.0, "stops where the next piece begins");
    }

    #[test]
    fn missing_music_is_skipped() {
        let dir = tmp_project("missing");
        let mut d = doc(&[(0, 35_000)]);
        let mut gone = track(TrackKind::Music, 0, 0, 10_000);
        gone.file = "gone.wav".into();
        let mut escape = track(TrackKind::Music, 0, 0, 10_000);
        escape.file = "../a.wav".into();
        d.audio_tracks = vec![gone, escape, track(TrackKind::Music, 0, 0, 10_000)];
        let s = project_tracks(&d, &dir);
        assert_eq!(s.len(), 1);
        assert!(s[0].path.ends_with("assets/a.wav"));
        // The timeline still has all three.
        assert_eq!(track_entries(&d).len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn muted_track_skipped() {
        let dir = tmp_project("muted");
        let mut d = doc(&[(0, 35_000)]);
        let mut t = track(TrackKind::Music, 0, 0, 10_000);
        t.muted = true;
        d.audio_tracks = vec![t];
        assert!(project_tracks(&d, &dir).is_empty());
        assert!(track_entries(&d)[0].muted);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trim_end_always_set() {
        let dir = tmp_project("trim-end");
        let mut d = doc(&[(0, 35_000)]);
        // Unprobed file (duration 0): the span is the window, still a concrete end.
        let mut looped = track(TrackKind::Music, 0, 0, 0);
        looped.looped = true;
        d.audio_tracks = vec![looped, track(TrackKind::Voiceover, 0, 0, 4_000)];
        let s = project_tracks(&d, &dir);
        assert!(!s.is_empty());
        for t in &s {
            assert!(t.trim_end_sec > t.trim_start_sec, "{t:?}");
        }
        let v = serde_json::to_value(&s[0]).unwrap();
        assert!(v["trimEndSec"].is_number());
        assert_eq!(v["kind"], "music");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn loop_capped_at_1000() {
        let mut d = doc(&[(0, 3_600_000)]);
        let mut t = track(TrackKind::Music, 0, 0, 1_000);
        t.looped = true;
        d.audio_tracks = vec![t];
        let e = track_entries(&d);
        assert_eq!(e.len(), MAX_LOOP_ENTRIES);
        // The fade-out belongs to the real end, which the cap never reaches.
        assert!(e.iter().all(|e| e.fade_out_ms == 0.0));
    }

    #[test]
    fn track_of_a_missing_take_is_left_out() {
        let mut d = doc(&[(0, 10_000), (0, 10_000)]);
        d.clips[1].source = Some("gone".into());
        d.audio_tracks = vec![track(TrackKind::Music, 1, 0, 5_000), track(TrackKind::Music, 0, 0, 5_000)];
        let play = crate::record::editor::playable(&d, &Default::default());
        let e = track_entries(&play);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].index, 1);
    }
}
