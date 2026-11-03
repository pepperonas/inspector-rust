//! Auto-zoom planner: a faithful port of OpenScreen's
//! `src/lib/ai-edition/timeline/zoom-suggestions.ts` (with the doc glue of
//! `apply-auto-zooms.ts`). Pure, no IPC.
//!
//! Not an AI feature: a deterministic pass over the clicks recorded with a take, which
//! thinks in shots the way an editor would. The video opens on the whole screen. Clicks
//! close in time and in space share one zoom. A zoom never comes straight back after
//! leaving. The video ends on the whole screen. A still pointer is no reason to zoom:
//! a take without clicks gets no automatic zoom.
//!
//! Time axes. Cursor samples are in the recording's SOURCE ms; our zooms are stored in
//! source ms per clip. Upstream plans each clip on its own source window and checks the
//! gaps between zooms on the TIMELINE, tempo ignored. Here the timeline ("edit time") is
//! the sum of the source durations of the clips before (speed regions ignored, as
//! upstream). Every clip is planned on its own window, then one global `resolve_shots`
//! runs over all clips on edit time, and the result goes back to source ms per clip.

use super::editor::{zoom_scale, EditDoc, FocusMode, ZoomOrigin, ZoomRegion, ZOOM_DEPTHS};
use super::project::{CursorRecording, CursorSample, Interaction};

/// The video opens on the whole screen: no zoom starts moving in before this.
pub const OPENING_WIDE_MS: f64 = 2500.0;
/// And it ends on it: the last zoom is fully back out this long before the end.
pub const CLOSING_WIDE_MS: f64 = 1000.0;
/// A click this close to the end of the recording is the one that stopped it.
pub const STOP_CLICK_GUARD_MS: f64 = 1000.0;
/// Clicks closer than this in time share one zoom, when they fit in it together.
pub const SHOT_MERGE_GAP_MS: f64 = 3000.0;
/// A zoom is fully in this long before its first click...
pub const SHOT_LEAD_MS: f64 = 500.0;
/// ...and holds this long after its last one, to show what the click did.
pub const SHOT_TAIL_MS: f64 = 1500.0;
/// The shortest time a zoom holds, fully in.
pub const MIN_SHOT_HOLD_MS: f64 = 1800.0;
/// The shortest wide shot between two zooms, ramps excluded. Less reads as a yo-yo.
pub const MIN_WIDE_BETWEEN_MS: f64 = 1500.0;
/// The margin kept around the clicks a zoom frames, as a fraction of the frame.
pub const SHOT_FIT_MARGIN: f64 = 0.12;
/// The depths a zoom may take, shallowest first: 1.25×, 1.5×, 1.8×.
pub const SHOT_DEPTHS: [u8; 3] = [1, 2, 3];
/// A press without its own mouse-up: a hidden sample within this of the press means a
/// slider press; a much later hidden interval must not erase a completed click.
pub const UNRELEASED_PRESS_GRACE_MS: u64 = 150;
/// Upstream `DEFAULT_ZOOM_DEPTH`: a reserved zoom without a scale counts at its scale (1.8).
const DEFAULT_RESERVED_DEPTH: u8 = 3;

const ZOOM_TRANSITION_BASE_MS: f64 = 600.0;
const ZOOM_TRANSITION_PER_LN_MS: f64 = 550.0;

/// How long a zoom ramp lasts at `scale` (= compositor `zoom_transition_s`, in ms).
pub fn zoom_transition_ms(scale: f64) -> f64 {
    ZOOM_TRANSITION_BASE_MS + ZOOM_TRANSITION_PER_LN_MS * scale.max(1.0).ln()
}

fn depth_scale(depth: u8) -> f64 {
    ZOOM_DEPTHS[(depth.clamp(1, 6) - 1) as usize]
}

/// JavaScript's `Math.round` (half rounds towards +∞, unlike `f64::round`).
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// A click a viewer sees, at its own time and position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Click {
    pub time_ms: u64,
    pub cx: f64,
    pub cy: f64,
}

/// One planned zoom on a single axis (upstream `AutoZoomSuggestion`), in that axis' ms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Suggestion {
    pub start: u64,
    pub end: u64,
    pub cx: f64,
    pub cy: f64,
    pub depth: u8,
}

/// A zoom already on the timeline: the plan keeps clear of it and never moves it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReservedZoom {
    pub start_ms: f64,
    pub end_ms: f64,
    /// Its effective scale (sets its ramps). The default depth's if `None`.
    pub scale: Option<f64>,
}

/// Upstream `normalizeCursorTelemetry`: drop non-finite, sort by time (stably), clamp
/// time to `[0, total]` and position to `[0, 1]`.
fn normalize(samples: &[CursorSample], total_ms: u64) -> Vec<CursorSample> {
    let mut out: Vec<CursorSample> =
        samples.iter().filter(|s| s.cx.is_finite() && s.cy.is_finite()).cloned().collect();
    out.sort_by_key(|s| s.time_ms);
    for s in &mut out {
        s.time_ms = s.time_ms.min(total_ms);
        s.cx = s.cx.clamp(0.0, 1.0);
        s.cy = s.cy.clamp(0.0, 1.0);
    }
    out
}

fn is_click(kind: Option<Interaction>) -> bool {
    matches!(
        kind,
        Some(Interaction::Click | Interaction::DoubleClick | Interaction::RightClick | Interaction::MiddleClick)
    )
}

/// The clicks a viewer sees (upstream `detectClicks` on normalised samples). A press the
/// cursor hides during is a drag the app took over, a slider or a scrubbed field.
pub fn detect_clicks(samples: &[CursorSample], total_ms: u64) -> Vec<Click> {
    detect_normalized(&normalize(samples, total_ms))
}

fn detect_normalized(samples: &[CursorSample]) -> Vec<Click> {
    let mut clicks = Vec::new();
    let mut pending: Option<&CursorSample> = None;
    let mut hidden_at: Option<u64> = None;
    let mut finish = |pending: &mut Option<&CursorSample>, hidden_at: &mut Option<u64>, released: bool| {
        let Some(p) = pending.take() else { return };
        let hidden_during_press = match *hidden_at {
            Some(h) => released || h.saturating_sub(p.time_ms) <= UNRELEASED_PRESS_GRACE_MS,
            None => false,
        };
        if !hidden_during_press {
            clicks.push(Click { time_ms: p.time_ms, cx: p.cx, cy: p.cy });
        }
        *hidden_at = None;
    };
    for s in samples {
        if is_click(s.interaction_type) {
            finish(&mut pending, &mut hidden_at, false);
            pending = if s.visible == Some(false) { None } else { Some(s) };
        } else if s.interaction_type == Some(Interaction::Mouseup) {
            // Sampled after the release: its visibility says nothing about the press.
            finish(&mut pending, &mut hidden_at, true);
        } else if pending.is_some() && s.visible == Some(false) && hidden_at.is_none() {
            hidden_at = Some(s.time_ms);
        }
    }
    finish(&mut pending, &mut hidden_at, false);
    clicks
}

/// Upstream `visibleClicks`: detected clicks before `ignore_from` (the stop click).
fn visible_clicks(samples: &[CursorSample], total_ms: u64, ignore_from: f64) -> Vec<Click> {
    detect_clicks(samples, total_ms).into_iter().filter(|c| (c.time_ms as f64) < ignore_from).collect()
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BoxF {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

fn box_around(c: &Click) -> BoxF {
    BoxF { min_x: c.cx, max_x: c.cx, min_y: c.cy, max_y: c.cy }
}

fn union(a: BoxF, b: BoxF) -> BoxF {
    BoxF {
        min_x: a.min_x.min(b.min_x),
        max_x: a.max_x.max(b.max_x),
        min_y: a.min_y.min(b.min_y),
        max_y: a.max_y.max(b.max_y),
    }
}

/// The deepest of `SHOT_DEPTHS` that shows the whole box with its margin, or `None`.
fn fit_depth(b: BoxF) -> Option<u8> {
    let size = (b.max_x - b.min_x).max(b.max_y - b.min_y) + 2.0 * SHOT_FIT_MARGIN;
    let mut fit = None;
    for d in SHOT_DEPTHS {
        if depth_scale(d) * size <= 1.0 {
            fit = Some(d);
        }
    }
    fit
}

#[derive(Debug, Clone, PartialEq)]
struct Shot {
    start: f64,
    end: f64,
    depth: u8,
    bx: BoxF,
    clicks: usize,
    /// Two shots merge only on one clip: a zoom does not hold across a cut.
    clip: Option<usize>,
}

/// One axis's clicks as timed shots (upstream `planShots`).
fn plan_shots(clicks: &[Click], total_ms: f64, wide_until: f64, wide_from: f64) -> Vec<Shot> {
    let mut groups: Vec<(Vec<f64>, BoxF)> = Vec::new();
    for c in clicks {
        let t = c.time_ms as f64;
        if let Some((times, bx)) = groups.last_mut() {
            if t - times[times.len() - 1] <= SHOT_MERGE_GAP_MS {
                let merged = union(*bx, box_around(c));
                if fit_depth(merged).is_some() {
                    times.push(t);
                    *bx = merged;
                    continue;
                }
            }
        }
        groups.push((vec![t], box_around(c)));
    }

    let mut shots = Vec::new();
    for (times, bx) in groups {
        // A group only grows while it fits, so it always has a depth.
        let depth = fit_depth(bx).unwrap_or(SHOT_DEPTHS[0]);
        let ramp = zoom_transition_ms(depth_scale(depth));
        let earliest = 0f64.max((wide_until + ramp).ceil());
        let latest = total_ms.min((wide_from - ramp).floor());
        let first = times[0];
        let last = times[times.len() - 1];
        // Near the end of the axis, the zoom arrives earlier rather than holding too briefly.
        let start = js_round(first - SHOT_LEAD_MS).min(latest - MIN_SHOT_HOLD_MS).max(earliest);
        let end = js_round(last + SHOT_TAIL_MS).max(start + MIN_SHOT_HOLD_MS).min(latest);
        if end - start < MIN_SHOT_HOLD_MS {
            continue;
        }
        if !times.iter().any(|&t| t >= start && t <= end) {
            continue;
        }
        shots.push(Shot { start, end, depth, bx, clicks: times.len(), clip: None });
    }
    shots
}

/// No yo-yo (upstream `resolveShots`): between two zooms, a real wide shot or none at all.
/// Two shots too close merge when they share a clip and fit in one zoom together;
/// otherwise the one with fewer clicks goes, the later one on a tie. A reserved zoom
/// never moves: a shot too close to it goes.
fn resolve_shots(shots: Vec<Shot>, reserved: &[ReservedZoom]) -> Vec<Shot> {
    #[derive(Clone, Copy)]
    struct Span {
        start: f64,
        end: f64,
        scale: f64,
        /// Index into `plan`, or `None` for a reserved zoom.
        shot: Option<usize>,
    }
    let out_of = |s: &Span| s.end + zoom_transition_ms(s.scale);
    let fixed: Vec<Span> = reserved
        .iter()
        .map(|z| Span {
            start: z.start_ms,
            end: z.end_ms,
            scale: z.scale.unwrap_or(depth_scale(DEFAULT_RESERVED_DEPTH)),
            shot: None,
        })
        .collect();
    let mut plan = shots;
    loop {
        let mut spans: Vec<Span> = plan
            .iter()
            .enumerate()
            .map(|(i, s)| Span { start: s.start, end: s.end, scale: depth_scale(s.depth), shot: Some(i) })
            .chain(fixed.iter().copied())
            .collect();
        // Stable, like JavaScript's `Array.prototype.sort`.
        spans.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
        // Each span against the one before it that ends latest, ramp included.
        let mut reach: Option<Span> = None;
        let mut clash: Option<(Span, Span)> = None;
        for span in &spans {
            if let Some(r) = reach {
                if r.shot.is_some() || span.shot.is_some() {
                    let wide = span.start - out_of(&r) - zoom_transition_ms(span.scale);
                    if wide < MIN_WIDE_BETWEEN_MS {
                        clash = Some((r, *span));
                        break;
                    }
                }
            }
            match reach {
                Some(r) if out_of(span) <= out_of(&r) => {}
                _ => reach = Some(*span),
            }
        }
        let Some((ra, rb)) = clash else { return plan };

        match (ra.shot, rb.shot) {
            (Some(ia), Some(ib)) => {
                let (a, b) = (&plan[ia], &plan[ib]);
                let bx = union(a.bx, b.bx);
                let depth = if a.clip == b.clip { fit_depth(bx) } else { None };
                if let Some(depth) = depth {
                    let merged = Shot {
                        start: a.start,
                        end: a.end.max(b.end),
                        depth,
                        bx,
                        clicks: a.clicks + b.clicks,
                        clip: a.clip,
                    };
                    plan[ia] = merged;
                    plan.remove(ib);
                } else {
                    let drop = if a.clicks >= b.clicks { ib } else { ia };
                    plan.remove(drop);
                }
            }
            (Some(i), None) | (None, Some(i)) => {
                plan.remove(i);
            }
            // Unreachable: a clash always involves a shot.
            (None, None) => return plan,
        }
    }
}

fn by_start(plan: &mut [Shot]) {
    plan.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
}

fn to_suggestion(s: &Shot) -> Suggestion {
    Suggestion {
        start: s.start.max(0.0) as u64,
        end: s.end.max(0.0) as u64,
        cx: (s.bx.min_x + s.bx.max_x) / 2.0,
        cy: (s.bx.min_y + s.bx.max_y) / 2.0,
        depth: s.depth,
    }
}

/// Upstream `buildAutoZoomSuggestions`: one recording laid on an axis from 0. The bounds
/// are on the samples' axis; ±∞ means no bound.
fn plan_samples(
    samples: &[CursorSample],
    total_ms: u64,
    reserved: &[ReservedZoom],
    wide_until: f64,
    wide_from: f64,
    ignore_from: f64,
) -> Vec<Suggestion> {
    if total_ms == 0 || samples.is_empty() {
        return Vec::new();
    }
    let clicks = visible_clicks(samples, total_ms, ignore_from);
    let shots = plan_shots(&clicks, total_ms as f64, wide_until, wide_from);
    let mut plan = resolve_shots(shots, reserved);
    by_start(&mut plan);
    plan.iter().map(to_suggestion).collect()
}

/// When the recording stopped: its samples run until the take ends.
fn last_sample_ms(samples: &[CursorSample]) -> u64 {
    samples.iter().map(|s| s.time_ms).max().unwrap_or(0)
}

/// Plan automatic zooms for `doc` from the take's cursor samples (upstream
/// `buildAutoZoomSuggestionsForClips` + `appendAutoZoomSuggestions`). Returns NEW regions
/// only (`origin: Auto`, `focus_mode: Auto`, id `auto-<clip>-<start_ms>`, times in source
/// ms of their clip); `doc.zooms` are reserved and never changed. Each clip replaying a
/// click gets its own zoom.
pub fn plan_auto_zooms(doc: &EditDoc, cursor: &CursorRecording) -> Vec<ZoomRegion> {
    if doc.clips.is_empty() || cursor.samples.is_empty() {
        return Vec::new();
    }
    // Edit-time offset of each clip: the source durations of the clips before it.
    let mut offsets = Vec::with_capacity(doc.clips.len());
    let mut edit_end = 0f64;
    for c in &doc.clips {
        offsets.push(edit_end);
        edit_end += c.end_ms.saturating_sub(c.start_ms) as f64;
    }
    let edit_start = 0f64;
    let last = last_sample_ms(&cursor.samples) as f64;

    let mut shots = Vec::new();
    for (i, clip) in doc.clips.iter().enumerate() {
        let window = clip.end_ms.saturating_sub(clip.start_ms);
        if window == 0 {
            continue;
        }
        let source_offset = clip.start_ms as f64;
        let timeline_offset = offsets[i];
        let clip_samples: Vec<CursorSample> = cursor
            .samples
            .iter()
            .filter(|s| s.time_ms >= clip.start_ms && s.time_ms <= clip.end_ms)
            .map(|s| CursorSample { time_ms: s.time_ms - clip.start_ms, ..s.clone() })
            .collect();
        let clicks = visible_clicks(&clip_samples, window, last - STOP_CLICK_GUARD_MS - source_offset);
        let clip_shots = plan_shots(
            &clicks,
            window as f64,
            edit_start + OPENING_WIDE_MS - timeline_offset,
            edit_end - CLOSING_WIDE_MS - timeline_offset,
        );
        shots.extend(clip_shots.into_iter().map(|s| Shot {
            start: s.start + timeline_offset,
            end: s.end + timeline_offset,
            clip: Some(i),
            ..s
        }));
    }

    // Existing zooms, on edit time with their effective scale.
    let reserved: Vec<ReservedZoom> = doc
        .zooms
        .iter()
        .filter_map(|z| {
            let clip = doc.clips.get(z.clip)?;
            let off = offsets[z.clip] - clip.start_ms as f64;
            Some(ReservedZoom {
                start_ms: off + z.start_ms as f64,
                end_ms: off + z.end_ms as f64,
                scale: Some(zoom_scale(z)),
            })
        })
        .collect();

    let mut plan = resolve_shots(shots, &reserved);
    by_start(&mut plan);
    plan.iter()
        .filter_map(|s| {
            let ci = s.clip?;
            let clip = &doc.clips[ci];
            let local = |t: f64| (js_round(t - offsets[ci]).max(0.0) as u64) + clip.start_ms;
            let (start_ms, end_ms) = (local(s.start), local(s.end));
            let sug = to_suggestion(s);
            Some(ZoomRegion {
                id: format!("auto-{ci}-{start_ms}"),
                clip: ci,
                start_ms,
                end_ms,
                depth: s.depth,
                custom_scale: None,
                focus_x: sug.cx,
                focus_y: sug.cy,
                focus_mode: FocusMode::Auto,
                origin: ZoomOrigin::Auto,
                hide_cursor: false,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::editor::{default_doc, EditClip, FocusMode, ZoomOrigin};
    use crate::record::project::{CursorProvider, Interaction, RecordingMeta};

    // ---- helpers (mirror zoom-suggestions.test.ts:7-27) ----

    fn s(t: u64, cx: f64, cy: f64) -> CursorSample {
        CursorSample {
            time_ms: t,
            cx,
            cy,
            visible: None,
            asset_id: None,
            cursor_type: None,
            interaction_type: None,
        }
    }
    fn with(mut x: CursorSample, kind: Interaction, visible: Option<bool>) -> CursorSample {
        x.interaction_type = Some(kind);
        x.visible = visible;
        x
    }
    fn click(t: u64, cx: f64, cy: f64) -> CursorSample {
        with(s(t, cx, cy), Interaction::Click, None)
    }
    fn vclick(t: u64, cx: f64, cy: f64, visible: bool) -> CursorSample {
        with(s(t, cx, cy), Interaction::Click, Some(visible))
    }
    fn mv(t: u64, cx: f64, cy: f64, visible: bool) -> CursorSample {
        with(s(t, cx, cy), Interaction::Move, Some(visible))
    }
    fn up(t: u64, cx: f64, cy: f64, visible: bool) -> CursorSample {
        with(s(t, cx, cy), Interaction::Mouseup, Some(visible))
    }
    fn park(center: f64, cx: f64, cy: f64) -> Vec<CursorSample> {
        let (count, span) = (6, 900.0);
        let step = span / (count as f64 - 1.0);
        (0..count).map(|i| s((center - span / 2.0 + i as f64 * step) as u64, cx, cy)).collect()
    }

    /// `buildAutoZoomSuggestions` without bounds (upstream `plan`).
    fn plan(samples: &[CursorSample], total: u64) -> Vec<Suggestion> {
        plan_samples(samples, total, &[], f64::NEG_INFINITY, f64::INFINITY, f64::INFINITY)
    }
    fn plan_res(samples: &[CursorSample], total: u64, reserved: &[ReservedZoom]) -> Vec<Suggestion> {
        plan_samples(samples, total, reserved, f64::NEG_INFINITY, f64::INFINITY, f64::INFINITY)
    }
    fn spans(v: &[Suggestion]) -> Vec<(u64, u64)> {
        v.iter().map(|x| (x.start, x.end)).collect()
    }
    fn foci(v: &[Suggestion]) -> Vec<(f64, f64)> {
        v.iter().map(|x| (x.cx, x.cy)).collect()
    }
    fn res(start: f64, end: f64, scale: Option<f64>) -> ReservedZoom {
        ReservedZoom { start_ms: start, end_ms: end, scale }
    }

    fn rec(samples: Vec<CursorSample>) -> CursorRecording {
        CursorRecording { version: 1, provider: CursorProvider::Native, samples, assets: vec![] }
    }
    fn doc(duration_ms: u64, clips: &[(u64, u64)]) -> EditDoc {
        let mut d = default_doc(&RecordingMeta { duration_ms, ..Default::default() }, None);
        d.clips = clips.iter().map(|&(a, b)| EditClip::main(a, b)).collect();
        d.crop = vec![None; clips.len()];
        d
    }
    /// Planned zooms as (clip, source start, source end).
    fn zs(v: &[ZoomRegion]) -> Vec<(usize, u64, u64)> {
        v.iter().map(|z| (z.clip, z.start_ms, z.end_ms)).collect()
    }

    // ---- constants ----

    #[test]
    fn constants_match_upstream() {
        assert_eq!(OPENING_WIDE_MS, 2500.0);
        assert_eq!(CLOSING_WIDE_MS, 1000.0);
        assert_eq!(STOP_CLICK_GUARD_MS, 1000.0);
        assert_eq!(SHOT_MERGE_GAP_MS, 3000.0);
        assert_eq!(SHOT_LEAD_MS, 500.0);
        assert_eq!(SHOT_TAIL_MS, 1500.0);
        assert_eq!(MIN_SHOT_HOLD_MS, 1800.0);
        assert_eq!(MIN_WIDE_BETWEEN_MS, 1500.0);
        assert_eq!(SHOT_FIT_MARGIN, 0.12);
        assert_eq!(SHOT_DEPTHS, [1, 2, 3]);
        assert_eq!(UNRELEASED_PRESS_GRACE_MS, 150);
        assert!((zoom_transition_ms(1.8) - 923.29).abs() < 0.01);
        assert_eq!(zoom_transition_ms(0.5), 600.0);
        assert!((zoom_transition_ms(5.0) - (600.0 + 550.0 * 5f64.ln())).abs() < 1e-9);
    }

    // ---- which clicks count (zoom-suggestions.test.ts:29-147) ----

    #[test]
    fn hidden_click_ignored() {
        assert!(plan(&[vclick(2000, 0.5, 0.5, false)], 5000).is_empty());
    }

    #[test]
    fn press_hidden_during_drag_ignored() {
        let v = plan(
            &[vclick(1000, 0.5, 0.5, true), mv(1016, 0.5, 0.5, false), up(1600, 0.7, 0.5, false)],
            5000,
        );
        assert!(v.is_empty());
    }

    #[test]
    fn release_before_later_hide_kept() {
        let v = plan(
            &[vclick(1000, 0.5, 0.5, true), up(1050, 0.5, 0.5, true), mv(1400, 0.5, 0.5, false)],
            5000,
        );
        assert_eq!(foci(&v), vec![(0.5, 0.5)]);
    }

    #[test]
    fn later_visible_click_after_hidden_drag_kept() {
        let v = plan(
            &[
                vclick(1000, 0.5, 0.5, true),
                mv(1100, 0.6, 0.5, false),
                up(1600, 0.7, 0.5, true),
                vclick(4000, 0.2, 0.8, true),
            ],
            6000,
        );
        assert_eq!(foci(&v), vec![(0.2, 0.8)]);
    }

    #[test]
    fn legacy_click_without_mouseup_kept_when_hide_late() {
        let v = plan(
            &[vclick(1000, 0.2, 0.8, true), mv(1050, 0.3, 0.7, true), mv(2000, 0.7, 0.3, false)],
            4000,
        );
        assert_eq!(foci(&v), vec![(0.2, 0.8)]);
    }

    #[test]
    fn legacy_click_without_mouseup_rejected_when_hide_next_tick() {
        let v = plan(&[vclick(1000, 0.2, 0.8, true), mv(1033, 0.25, 0.75, false)], 4000);
        assert!(v.is_empty());
    }

    #[test]
    fn tap_folded_with_release_kept() {
        let v = plan(
            &[
                vclick(500, 0.8, 0.2, true),
                up(600, 0.8, 0.2, true),
                vclick(9000, 0.2, 0.8, true),
                mv(9500, 0.2, 0.8, false),
            ],
            12000,
        );
        assert_eq!(foci(&v), vec![(0.8, 0.2), (0.2, 0.8)]);
    }

    #[test]
    fn hidden_mouseup_sample_kept() {
        let v = plan(&[vclick(1000, 0.5, 0.5, true), up(1033, 0.5, 0.5, false)], 4000);
        assert_eq!(foci(&v), vec![(0.5, 0.5)]);
    }

    #[test]
    fn all_four_click_kinds_accepted() {
        for kind in
            [Interaction::Click, Interaction::DoubleClick, Interaction::RightClick, Interaction::MiddleClick]
        {
            let v = plan(&[with(s(2000, 0.3, 0.7), kind, None), s(4000, 0.4, 0.6)], 5000);
            assert_eq!(foci(&v), vec![(0.3, 0.7)], "{kind:?}");
        }
    }

    #[test]
    fn move_and_mouseup_are_not_clicks() {
        let t: Vec<CursorSample> = (0..10u64)
            .map(|i| {
                let kind = if i == 4 { Interaction::Mouseup } else { Interaction::Move };
                with(s(i * 500, i as f64 / 10.0, i as f64 / 10.0), kind, None)
            })
            .collect();
        assert!(plan(&t, 5000).is_empty());
    }

    #[test]
    fn parked_pointer_no_zoom() {
        assert!(plan(&park(2000.0, 0.5, 0.5), 5000).is_empty());
    }

    #[test]
    fn no_samples_plans_nothing() {
        assert!(plan(&[], 5000).is_empty());
        for kind in [Interaction::Move, Interaction::Mouseup] {
            assert!(plan(&[with(s(2000, 0.3, 0.7), kind, None)], 5000).is_empty(), "{kind:?}");
        }
        // And through the public entry point.
        assert!(plan_auto_zooms(&doc(5000, &[(0, 5000)]), &rec(vec![])).is_empty());
    }

    #[test]
    fn detect_clicks_reports_time_and_position() {
        let c = detect_clicks(
            &[mv(100, 0.1, 0.1, true), vclick(2000, 1.4, -0.2, true), up(2050, 0.5, 0.5, true)],
            5000,
        );
        // Positions are clamped to the frame, as upstream normalises them.
        assert_eq!(c, vec![Click { time_ms: 2000, cx: 1.0, cy: 0.0 }]);
        // Unsorted input is sorted first, and times clamp to the total.
        let c = detect_clicks(&[click(9000, 0.5, 0.5), click(1000, 0.2, 0.2)], 5000);
        assert_eq!(
            c,
            vec![Click { time_ms: 1000, cx: 0.2, cy: 0.2 }, Click { time_ms: 5000, cx: 0.5, cy: 0.5 }]
        );
        // Non-finite positions are dropped.
        assert!(detect_clicks(&[click(1000, f64::NAN, 0.5)], 5000).is_empty());
    }

    // ---- shots (zoom-suggestions.test.ts:149-265) ----

    #[test]
    fn single_click_at_2000_in_5000() {
        let t: Vec<CursorSample> = (0..10u64)
            .map(|i| {
                let kind = if i == 4 { Interaction::Click } else { Interaction::Move };
                with(s(i * 500, i as f64 / 10.0, i as f64 / 10.0), kind, None)
            })
            .collect();
        let v = plan(&t, 5000);
        assert_eq!(v, vec![Suggestion { start: 1500, end: 3500, cx: 0.4, cy: 0.4, depth: 3 }]);
    }

    #[test]
    fn focus_is_the_click_not_the_path() {
        let v = plan(&[s(1000, 0.1, 0.9), click(1200, 0.8, 0.2), s(1400, 0.12, 0.88)], 5000);
        assert_eq!(spans(&v), vec![(700, 2700)]);
        assert_eq!(foci(&v), vec![(0.8, 0.2)]);
    }

    #[test]
    fn burst_2000_3500_5000() {
        let v = plan(&[click(2000, 0.3, 0.3), click(3500, 0.36, 0.33), click(5000, 0.4, 0.35)], 9000);
        assert_eq!(v.len(), 1);
        assert_eq!(spans(&v), vec![(1500, 6500)]);
        assert_eq!(v[0].depth, 3);
        assert!((v[0].cx - 0.35).abs() < 1e-5);
        assert!((v[0].cy - 0.325).abs() < 1e-5);
    }

    #[test]
    fn spread_035_depth_2() {
        let v = plan(&[click(2000, 0.2, 0.5), click(3000, 0.55, 0.5)], 9000);
        assert_eq!(spans(&v), vec![(1500, 4500)]);
        assert_eq!(v.iter().map(|x| x.depth).collect::<Vec<_>>(), vec![2]);
    }

    #[test]
    fn spread_05_depth_1() {
        let v = plan(&[click(2000, 0.2, 0.5), click(3000, 0.7, 0.5)], 9000);
        assert_eq!(v.iter().map(|x| x.depth).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn clash_keeps_more_clicks() {
        let v = plan(&[click(2000, 0.1, 0.1), click(2600, 0.12, 0.12), click(3200, 0.9, 0.9)], 9000);
        assert_eq!(spans(&v), vec![(1500, 4100)]);
    }

    #[test]
    fn gap_4s_same_spot_merges() {
        let v = plan(&[click(2000, 0.3, 0.3), click(6000, 0.34, 0.3)], 9000);
        assert_eq!(spans(&v), vec![(1500, 7500)]);
        assert_eq!(v[0].depth, 3);
    }

    // Not in upstream's tests: the merge gap's boundary. Alone, a click past the closing
    // bound gets no shot; only grouped with an earlier click does its zoom hold up to it.
    // latest = floor(7000 − 923.3) = 6076, so a lone click at 6200 is dropped.
    #[test]
    fn merge_gap_is_inclusive_at_3000() {
        let bounded = |a: u64, b: u64| {
            plan_samples(
                &[click(a, 0.3, 0.3), click(b, 0.3, 0.3)],
                9000,
                &[],
                f64::NEG_INFINITY,
                7000.0,
                f64::INFINITY,
            )
        };
        assert_eq!(spans(&bounded(3200, 6200)), vec![(2700, 6076)]);
        assert_eq!(spans(&bounded(3199, 6200)), vec![(2699, 4699)]);
    }

    #[test]
    fn wide_shot_kept() {
        let v = plan(&[click(2000, 0.1, 0.1), click(9000, 0.9, 0.9)], 12000);
        assert_eq!(spans(&v), vec![(1500, 3500), (8500, 10500)]);
    }

    #[test]
    fn reserved_3000_5000_blocks_7000_allows_9000() {
        let reserved = [res(3000.0, 5000.0, None)];
        assert!(plan_res(&[click(7000, 0.5, 0.5)], 12000, &reserved).is_empty());
        let clear = plan_res(&[click(9000, 0.5, 0.5)], 12000, &reserved);
        assert_eq!(spans(&clear), vec![(8500, 10500)]);
    }

    #[test]
    fn reserved_overlapping_long_zoom_reaches_past() {
        let v = plan_res(
            &[click(12000, 0.5, 0.5)],
            30000,
            &[res(0.0, 20000.0, None), res(5000.0, 8000.0, None)],
        );
        assert!(v.is_empty());
    }

    #[test]
    fn reserved_5x_ramps() {
        let v = plan_res(&[click(9000, 0.5, 0.5)], 12000, &[res(3000.0, 5000.0, Some(5.0))]);
        assert!(v.is_empty());
    }

    #[test]
    fn axis_ends() {
        assert_eq!(spans(&plan(&[click(100, 0.1, 0.1)], 5000)), vec![(0, 1800)]);
        assert_eq!(spans(&plan(&[click(4800, 0.9, 0.9)], 5000)), vec![(3200, 5000)]);
    }

    // ---- clips (zoom-suggestions.test.ts:267-490), on our single-recording doc ----

    #[test]
    fn click_per_clip() {
        // One recording laid down twice: source 0-10 s, then again.
        let d = doc(10000, &[(0, 10000), (0, 10000)]);
        let v = plan_auto_zooms(&d, &rec(vec![click(4000, 0.3, 0.7), s(9500, 0.4, 0.6)]));
        assert_eq!(zs(&v), vec![(0, 3500, 5500), (1, 3500, 5500)]);
        for z in &v {
            assert_eq!((z.focus_x, z.focus_y), (0.3, 0.7));
            assert_eq!(z.origin, ZoomOrigin::Auto);
            assert_eq!(z.focus_mode, FocusMode::Auto);
            assert_eq!(z.depth, 3);
            assert_eq!(z.custom_scale, None);
            assert!(!z.hide_cursor);
        }
        assert_eq!(v[0].id, "auto-0-3500");
        assert_eq!(v[1].id, "auto-1-3500");
    }

    #[test]
    fn source_in_point_shift() {
        // A clip that starts 30 s into the recording: source 34 s is edit 4 s.
        let d = doc(60000, &[(30000, 40000)]);
        let v = plan_auto_zooms(&d, &rec(vec![click(34000, 0.5, 0.5), s(60000, 0.6, 0.4)]));
        assert_eq!(zs(&v), vec![(0, 33500, 35500)]);
        assert_eq!(v[0].id, "auto-0-33500");
    }

    #[test]
    fn single_click_after_trim() {
        let d = doc(60000, &[(30000, 40000)]);
        let v = plan_auto_zooms(
            &d,
            &rec(vec![s(5000, 0.1, 0.9), click(34000, 0.5, 0.5), s(60000, 0.1, 0.9)]),
        );
        assert_eq!(zs(&v), vec![(0, 33500, 35500)]);
        assert_eq!((v[0].focus_x, v[0].focus_y), (0.5, 0.5));
    }

    #[test]
    fn outside_window_ignored() {
        let d = doc(60000, &[(0, 10000)]);
        let v = plan_auto_zooms(&d, &rec(vec![click(45000, 0.5, 0.5), s(60000, 0.6, 0.4)]));
        assert!(v.is_empty());
    }

    #[test]
    fn reserve_per_clip() {
        let mut d = doc(10000, &[(0, 10000), (0, 10000)]);
        d.zooms.push(ZoomRegion {
            id: "mine".into(),
            clip: 0,
            start_ms: 3500,
            end_ms: 4500,
            depth: 3,
            custom_scale: None,
            focus_x: 0.5,
            focus_y: 0.5,
            focus_mode: FocusMode::Manual,
            origin: ZoomOrigin::Manual,
            hide_cursor: false,
        });
        let v = plan_auto_zooms(&d, &rec(vec![click(4000, 0.5, 0.5), s(9500, 0.6, 0.4)]));
        assert_eq!(zs(&v), vec![(1, 3500, 5500)]);
    }

    #[test]
    fn clip_without_samples_skipped() {
        // A zero-length clip window, as upstream's unprobed clip.
        let d = doc(10000, &[(4000, 4000)]);
        let v = plan_auto_zooms(&d, &rec(vec![click(4000, 0.5, 0.5), s(9500, 0.6, 0.4)]));
        assert!(v.is_empty());
        // A clip whose window holds no samples gets nothing, its neighbour still does.
        let d = doc(30000, &[(20000, 30000), (0, 10000)]);
        let v = plan_auto_zooms(&d, &rec(vec![click(4000, 0.5, 0.5), s(15000, 0.6, 0.4)]));
        assert_eq!(zs(&v), vec![(1, 3500, 5500)]);
    }

    #[test]
    fn no_hold_across_cut() {
        // Two clips cut back to back. Each clip's zoom is fine alone, but together they would
        // zoom out at the cut and straight back in, and a zoom does not merge across a cut.
        let d = doc(20000, &[(0, 10000), (10000, 20000)]);
        let v = plan_auto_zooms(
            &d,
            &rec(vec![click(7000, 0.5, 0.5), click(11000, 0.5, 0.5), s(19900, 0.5, 0.5)]),
        );
        assert_eq!(zs(&v), vec![(0, 6500, 8500)]);
    }

    // At depth 3 a ramp lasts 923 ms: on a 10 s edit a zoom is fully in at 3424 ms at the
    // earliest and by 8076 ms at the latest.
    #[test]
    fn opening_and_ending_wide() {
        let rest = s(30000, 0.5, 0.5);
        let d = doc(30000, &[(0, 10000)]);
        let early = plan_auto_zooms(&d, &rec(vec![click(1000, 0.3, 0.3), rest.clone()]));
        assert!(early.is_empty());
        let held = plan_auto_zooms(&d, &rec(vec![click(3600, 0.3, 0.3), rest.clone()]));
        assert_eq!(zs(&held), vec![(0, 3424, 5224)]);
        let zoomed = plan_auto_zooms(&d, &rec(vec![click(7500, 0.3, 0.3), rest.clone()]));
        assert_eq!(zs(&zoomed), vec![(0, 6276, 8076)]);
        let too_late = plan_auto_zooms(&d, &rec(vec![click(8500, 0.3, 0.3), rest]));
        assert!(too_late.is_empty());
    }

    #[test]
    fn clip_after_cut_may_open_on_zoom() {
        let d = doc(10000, &[(0, 10000), (0, 10000)]);
        let v = plan_auto_zooms(&d, &rec(vec![click(1000, 0.3, 0.3), s(9500, 0.5, 0.5)]));
        assert_eq!(zs(&v), vec![(1, 500, 2500)]);
    }

    #[test]
    fn stop_click_ignored() {
        // The take's last click, 100 ms before its telemetry ends: the HUD's Stop button.
        // Another clip follows, so the edit's own ending does not reach it.
        let d = doc(20000, &[(0, 20000), (0, 5000)]);
        let v = plan_auto_zooms(
            &d,
            &rec(vec![click(6000, 0.3, 0.3), click(19800, 0.5, 0.92), s(19900, 0.5, 0.92)]),
        );
        assert_eq!(v.iter().map(|z| (z.focus_x, z.focus_y)).collect::<Vec<_>>(), vec![(0.3, 0.3)]);
        // The pointer resting on Stop before that click does not zoom either.
        let mut samples = vec![click(6000, 0.3, 0.3)];
        samples.extend((0..34u64).map(|i| s(18800 + i * 33, 0.5, 0.92)));
        samples.push(click(19800, 0.5, 0.92));
        samples.push(s(19900, 0.5, 0.92));
        let v = plan_auto_zooms(&d, &rec(samples));
        assert_eq!(v.iter().map(|z| (z.focus_x, z.focus_y)).collect::<Vec<_>>(), vec![(0.3, 0.3)]);
    }

    // ---- our additions ----

    #[test]
    fn existing_zooms_never_change() {
        let mut d = doc(30000, &[(0, 30000)]);
        let mine = ZoomRegion {
            id: "mine".into(),
            clip: 0,
            start_ms: 3000,
            end_ms: 5000,
            depth: 5,
            custom_scale: Some(2.0),
            focus_x: 0.1,
            focus_y: 0.2,
            focus_mode: FocusMode::Manual,
            origin: ZoomOrigin::Manual,
            hide_cursor: true,
        };
        d.zooms.push(mine.clone());
        let before = d.clone();
        let v = plan_auto_zooms(
            &d,
            &rec(vec![click(4000, 0.5, 0.5), click(15000, 0.5, 0.5), s(29000, 0.5, 0.5)]),
        );
        assert_eq!(d, before);
        // Only new regions come back, and the click inside the reserved zoom yields to it.
        assert_eq!(zs(&v), vec![(0, 14500, 16500)]);
        assert!(v.iter().all(|z| z.id != "mine" && z.origin == ZoomOrigin::Auto));
    }

    #[test]
    fn double_click_counts_once_as_shot_click() {
        // A lone double-click sample is ONE click: the same shot as a single click.
        let one = plan(&[click(2000, 0.4, 0.4)], 5000);
        let dbl = plan(&[with(s(2000, 0.4, 0.4), Interaction::DoubleClick, None)], 5000);
        assert_eq!(dbl, one);
        // Our sampler records a real double-click as a click then a double-click sample:
        // one zoom, never two.
        let pair = plan(
            &[click(2000, 0.4, 0.4), with(s(2200, 0.4, 0.4), Interaction::DoubleClick, None)],
            5000,
        );
        assert_eq!(spans(&pair), vec![(1500, 3700)]);
    }

    #[test]
    fn position_only_provider_accepted() {
        // The planner reads the samples, not the provider: positions alone plan nothing,
        // and it does not reject the recording outright.
        let d = doc(30000, &[(0, 30000)]);
        let mut r = rec(park(5000.0, 0.5, 0.5));
        r.provider = CursorProvider::PositionOnly;
        assert!(plan_auto_zooms(&d, &r).is_empty());
        r.samples = vec![click(5000, 0.5, 0.5), s(29000, 0.5, 0.5)];
        assert_eq!(zs(&plan_auto_zooms(&d, &r)), vec![(0, 4500, 6500)]);
    }

    #[test]
    fn output_sorted_by_edit_time() {
        // Clip 1 comes first in source, second on the edit: order follows the edit.
        let d = doc(40000, &[(20000, 40000), (0, 20000)]);
        let v = plan_auto_zooms(
            &d,
            &rec(vec![click(6000, 0.5, 0.5), click(30000, 0.5, 0.5), s(39900, 0.5, 0.5)]),
        );
        assert_eq!(zs(&v), vec![(0, 29500, 31500), (1, 5500, 7500)]);
    }
}
