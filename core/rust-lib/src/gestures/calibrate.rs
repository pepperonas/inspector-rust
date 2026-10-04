//! Guided calibration of the classification thresholds (gesture guard
//! Phase 5, `gestures calibrate`).
//!
//! The panel walks the user through three steps of [`STEP_MS`] each after a
//! short countdown:
//!
//! 1. both hands on the keyboard as when typing, palm heels on the trackpad;
//! 2. hands off, only the thumb resting low on the pad;
//! 3. thumb off, tap and swipe with three fingers several times.
//!
//! The frames are recorded as an ordinary trace (in memory only — a
//! calibration is never written to disk) and [`propose`] turns them into new
//! values for the level-1 thresholds. Nothing is applied here: the proposal is
//! shown, and only the user's confirmation saves it.
//!
//! Pure — no OS calls; the platform records, this module decides.

use super::guard::GuardConfig;
use super::trace::{DeviceInfo, Frame, Trace};
use serde::Serialize;
use std::collections::HashMap;

/// Countdown before the first step, so the hands can get into place.
pub const LEAD_MS: u64 = 3_000;
/// Length of one step.
pub const STEP_MS: u64 = 6_000;
/// Number of steps.
pub const STEPS: usize = 3;
/// The start of every step is ignored — the hands are still moving there.
pub const STEP_SETTLE_MS: u64 = 1_000;
/// A contact that stays at least this long counts as "resting" (steps 1, 2).
pub const REST_MIN_MS: u64 = 400;
/// A contact that stays longer than this is not a tap or swipe (step 3) — a
/// palm left on the pad must not be measured as a finger.
pub const GESTURE_MAX_MS: u64 = 1_500;
/// Finger contacts needed in step 3 (two three-finger gestures).
pub const MIN_FINGER_CONTACTS: usize = 6;

/// Total length of a calibration after the countdown.
pub fn total_ms() -> u64 {
    STEP_MS * STEPS as u64
}

/// Where a running calibration is, from the time since it was started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "phase")]
pub enum Phase {
    Countdown { remaining_ms: u64 },
    /// `step` counts from 1 of `steps`; `step_ms` is one step's length (the
    /// panel draws its progress bar from it instead of a copied constant).
    Step { step: usize, steps: usize, remaining_ms: u64, step_ms: u64 },
    Finished,
}

pub fn phase_at(elapsed_ms: u64) -> Phase {
    if elapsed_ms < LEAD_MS {
        return Phase::Countdown { remaining_ms: LEAD_MS - elapsed_ms };
    }
    let t = elapsed_ms - LEAD_MS;
    if t >= total_ms() {
        return Phase::Finished;
    }
    let i = (t / STEP_MS) as usize;
    Phase::Step { step: i + 1, steps: STEPS, remaining_ms: STEP_MS - t % STEP_MS, step_ms: STEP_MS }
}

/// The step a frame at `t_ms` (relative to the end of the countdown) is
/// measured for, or `None` inside the settle part of a step.
pub fn step_of(t_ms: u64) -> Option<usize> {
    if t_ms >= total_ms() || t_ms % STEP_MS < STEP_SETTLE_MS {
        return None;
    }
    Some((t_ms / STEP_MS) as usize)
}

/// One contact's life inside one step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Life {
    pub max_size: f32,
    /// Largest major/minor axis ratio seen (0 when the driver sends no axes).
    pub max_ratio: f32,
    /// Where it landed (y grows downwards).
    pub start_y: f64,
    pub duration_ms: u64,
}

/// Contact lives of each step. A contact that goes missing for a frame, or
/// crosses into the next step, starts a new life — the classifier also judges
/// a contact from its landing on.
pub fn lives_per_step(frames: &[Frame], device: u32) -> [Vec<Life>; STEPS] {
    struct Open {
        life: Life,
        first: u64,
        last: u64,
        seen: bool,
    }
    let mut out: [Vec<Life>; STEPS] = Default::default();
    let mut open: HashMap<i32, (usize, Open)> = HashMap::new();
    let close = |out: &mut [Vec<Life>; STEPS], step: usize, o: Open| {
        let mut l = o.life;
        l.duration_ms = o.last - o.first;
        out[step].push(l);
    };
    for f in frames.iter().filter(|f| f.device == device) {
        let step = step_of(f.t_ms);
        for (_, o) in open.values_mut() {
            o.seen = false;
        }
        if let Some(step) = step {
            for t in f.touches.iter().filter(|t| t.phase.on_pad()) {
                let ratio = if t.minor > 0.0 { t.major / t.minor } else { 0.0 };
                match open.get_mut(&t.id) {
                    Some((s, o)) if *s == step => {
                        o.life.max_size = o.life.max_size.max(t.size);
                        o.life.max_ratio = o.life.max_ratio.max(ratio);
                        o.last = f.t_ms;
                        o.seen = true;
                    }
                    _ => {
                        if let Some((s, o)) = open.remove(&t.id) {
                            close(&mut out, s, o);
                        }
                        let life = Life { max_size: t.size, max_ratio: ratio, start_y: t.y, duration_ms: 0 };
                        open.insert(t.id, (step, Open { life, first: f.t_ms, last: f.t_ms, seen: true }));
                    }
                }
            }
        }
        let gone: Vec<i32> = open.iter().filter(|(_, (_, o))| !o.seen).map(|(id, _)| *id).collect();
        for id in gone {
            if let Some((s, o)) = open.remove(&id) {
                close(&mut out, s, o);
            }
        }
    }
    for (_, (s, o)) in open {
        close(&mut out, s, o);
    }
    out
}

/// Nearest-rank percentile of a non-empty slice (`q` in 0..=1).
fn percentile(mut v: Vec<f64>, q: f64) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    let i = ((q * v.len() as f64).ceil() as usize).clamp(1, v.len()) - 1;
    v[i]
}

/// What one step measured.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct StepStats {
    /// Contacts that qualified for the step.
    pub count: usize,
    pub size_p10: f64,
    pub size_p90: f64,
    pub ratio_p10: f64,
    pub ratio_p90: f64,
    pub y_p10: f64,
}

fn stats(lives: &[Life]) -> Option<StepStats> {
    if lives.is_empty() {
        return None;
    }
    let sizes: Vec<f64> = lives.iter().map(|l| l.max_size as f64).collect();
    let ratios: Vec<f64> = lives.iter().map(|l| l.max_ratio as f64).collect();
    let ys: Vec<f64> = lives.iter().map(|l| l.start_y).collect();
    Some(StepStats {
        count: lives.len(),
        size_p10: percentile(sizes.clone(), 0.1),
        size_p90: percentile(sizes, 0.9),
        ratio_p10: percentile(ratios.clone(), 0.1),
        ratio_p90: percentile(ratios, 0.9),
        y_p10: percentile(ys, 0.1),
    })
}

/// One proposed threshold change. `key` is the field of [`GuardConfig`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    pub key: &'static str,
    pub current: f64,
    pub proposed: f64,
}

/// The result shown to the user.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Proposal {
    pub device: Option<DeviceInfo>,
    /// Palm (step 1), thumb (step 2), fingers (step 3); `None` = nothing usable.
    pub palm: Option<StepStats>,
    pub thumb: Option<StepStats>,
    pub finger: Option<StepStats>,
    /// Only the values that actually change.
    pub changes: Vec<Change>,
    /// Plain-language notes: what could not be measured and why a value stays.
    pub warnings: Vec<String>,
}

/// `v` on a grid of `step` (divides by the inverse so 0.1-steps come out as
/// exact decimals, not 1.9000000000000001).
fn round_to(v: f64, step: f64) -> f64 {
    let inv = (1.0 / step).round();
    (v * inv).round() / inv
}

/// Halfway between what the fingers reached and where the other class starts,
/// on the slider's grid, never at or below the fingers.
fn between(finger_hi: f64, other_lo: f64, grid: f64) -> f64 {
    let mid = round_to((finger_hi + other_lo) / 2.0, grid);
    if mid <= finger_hi { finger_hi + grid } else { mid }
}

/// Classes count as separable only with a margin — two sets of numbers that
/// merely touch would give a threshold the next touch crosses either way.
const MARGIN: f64 = 1.1;

/// Turn a calibration recording into proposed thresholds. Errs when step 3
/// yields too few finger contacts: without fingers there is nothing to
/// separate the other classes from.
pub fn propose(trace: &Trace, current: &GuardConfig) -> Result<Proposal, String> {
    // The device that saw most of the calibration is the one calibrated.
    let mut per_device: HashMap<u32, usize> = HashMap::new();
    for f in &trace.frames {
        if f.touches.iter().any(|t| t.phase.on_pad()) {
            *per_device.entry(f.device).or_default() += 1;
        }
    }
    let device = per_device.iter().max_by_key(|(d, n)| (**n, std::cmp::Reverse(**d))).map(|(d, _)| *d);
    let Some(device) = device else {
        return Err("Während der Kalibrierung lag nichts auf dem Trackpad.".into());
    };
    let [s1, s2, s3] = lives_per_step(&trace.frames, device);
    let palms: Vec<Life> = s1.into_iter().filter(|l| l.duration_ms >= REST_MIN_MS).collect();
    let thumbs: Vec<Life> = s2.into_iter().filter(|l| l.duration_ms >= REST_MIN_MS).collect();
    let fingers: Vec<Life> = s3.into_iter().filter(|l| l.duration_ms <= GESTURE_MAX_MS).collect();
    if fingers.len() < MIN_FINGER_CONTACTS {
        return Err(format!(
            "Schritt 3 hat nur {} Fingerkontakte erkannt (gebraucht: {MIN_FINGER_CONTACTS}) — bitte mehrmals mit drei Fingern tippen und wischen.",
            fingers.len()
        ));
    }
    let palm = stats(&palms);
    let thumb = stats(&thumbs);
    let finger = stats(&fingers).expect("non-empty");
    let mut warnings = Vec::new();
    let mut changes = Vec::new();
    let mut change = |key: &'static str, current: f64, proposed: f64| {
        if (proposed - current).abs() > 1e-6 {
            changes.push(Change { key, current, proposed });
        }
    };

    match palm {
        None => warnings.push("Schritt 1: kein ruhender Handballen erkannt — Handballen-Schwelle bleibt.".into()),
        Some(p) if p.size_p10 > finger.size_p90 * MARGIN => {
            let v = between(finger.size_p90, p.size_p10, 0.1).clamp(0.5, 10.0);
            change("palm_size", current.palm_size as f64, round_to(v, 0.1));
        }
        Some(_) => warnings.push(
            "Handballen und Finger waren auf diesem Gerät zu ähnlich groß — Handballen-Schwelle bleibt.".into(),
        ),
    }

    match thumb {
        None => warnings.push("Schritt 2: kein ruhender Daumen erkannt — Daumen-Werte bleiben.".into()),
        Some(t) => {
            let ratio_sep = t.ratio_p10 > finger.ratio_p90 * MARGIN;
            let size_sep = t.size_p10 > finger.size_p90 * MARGIN;
            if !ratio_sep && !size_sep {
                warnings.push(
                    "Der Daumen war weder länglicher noch größer als die Finger — Daumen-Werte bleiben.".into(),
                );
            } else {
                // The thumb rule needs ratio AND size AND a low landing. What
                // separates is placed between the classes; what doesn't is
                // lowered just under the thumb so it can't block the other.
                let ratio = if ratio_sep {
                    between(finger.ratio_p90, t.ratio_p10, 0.1)
                } else {
                    round_to(t.ratio_p10 * 0.95, 0.1).min(current.thumb_ratio as f64)
                };
                let size = if size_sep {
                    between(finger.size_p90, t.size_p10, 0.05)
                } else {
                    round_to(t.size_p10 * 0.9, 0.05).min(current.thumb_min_size as f64)
                };
                change("thumb_ratio", current.thumb_ratio as f64, round_to(ratio.clamp(0.1, 5.0), 0.1));
                change("thumb_min_size", current.thumb_min_size as f64, round_to(size.clamp(0.0, 10.0), 0.05));
                let zone = round_to(t.y_p10 - 0.05, 0.05).clamp(0.3, 0.9);
                change("thumb_zone", current.thumb_zone, zone);
            }
        }
    }

    let device_info = trace.devices.get(device as usize).cloned();
    Ok(Proposal { device: device_info, palm, thumb, finger: Some(finger), changes, warnings })
}

/// The proposal applied to a config — what the panel saves after the user
/// confirms. Unknown keys are ignored.
pub fn apply(g: &GuardConfig, changes: &[Change]) -> GuardConfig {
    let mut out = *g;
    for c in changes {
        match c.key {
            "palm_size" => out.palm_size = c.proposed as f32,
            "thumb_ratio" => out.thumb_ratio = c.proposed as f32,
            "thumb_min_size" => out.thumb_min_size = c.proposed as f32,
            "thumb_zone" => out.thumb_zone = c.proposed,
            _ => {}
        }
    }
    out.normalized()
}

#[cfg(test)]
mod tests {
    use super::super::trace::{Touch, TouchPhase, TRACE_VERSION};
    use super::*;

    fn touch(id: i32, y: f64, size: f32, major: f32, minor: f32) -> Touch {
        Touch { id, x: 0.5, y, vx: 0.0, vy: 0.0, major, minor, angle: 0.0, size, phase: TouchPhase::Touching }
    }

    /// Absolute trace time of `ms` into step `step` (0-based).
    fn at(step: u64, ms: u64) -> u64 {
        step * STEP_MS + ms
    }

    /// A contact held from `from` to `to` (every 10 ms) in step `step`.
    fn hold(frames: &mut Vec<Frame>, step: u64, from: u64, to: u64, t: Touch) {
        let mut ms = from;
        while ms <= to {
            frames.push(Frame { t_ms: at(step, ms), device: 0, touches: vec![t] });
            ms += 10;
        }
    }

    fn trace(frames: Vec<Frame>) -> Trace {
        let mut frames = frames;
        frames.sort_by_key(|f| f.t_ms);
        // Merge frames of the same instant so concurrent contacts share one.
        let mut merged: Vec<Frame> = Vec::new();
        for f in frames {
            match merged.last_mut() {
                Some(l) if l.t_ms == f.t_ms => l.touches.extend(f.touches),
                _ => merged.push(f),
            }
        }
        Trace {
            version: TRACE_VERSION,
            source: "test".into(),
            note: String::new(),
            devices: vec![DeviceInfo { builtin: Some(true), width_mm: None, height_mm: None }],
            muted_at_start: false,
            frames: merged,
            keys: vec![],
            expect: None,
        }
    }

    /// A user whose palm is 2.9–3.1, thumb ratio 2.4 / size 1.6 / landing at
    /// y 0.8, and fingers 0.9 with ratio 1.2.
    fn session() -> Vec<Frame> {
        let mut f = Vec::new();
        hold(&mut f, 0, 1_200, 5_500, touch(1, 0.9, 3.1, 30.0, 20.0));
        hold(&mut f, 0, 1_300, 5_400, touch(2, 0.85, 2.9, 28.0, 20.0));
        hold(&mut f, 1, 1_500, 5_000, touch(3, 0.8, 1.6, 24.0, 10.0));
        for g in 0..3u64 {
            let start = 1_200 + g * 1_400;
            for k in 0..3 {
                hold(&mut f, 2, start, start + 150, touch(10 + (g * 3 + k) as i32, 0.4, 0.9, 12.0, 10.0));
            }
        }
        f
    }

    #[test]
    fn phases_count_down_then_walk_the_steps() {
        assert_eq!(phase_at(0), Phase::Countdown { remaining_ms: LEAD_MS });
        let step = |step, remaining_ms| Phase::Step { step, steps: STEPS, remaining_ms, step_ms: STEP_MS };
        assert_eq!(phase_at(LEAD_MS), step(1, STEP_MS));
        assert_eq!(phase_at(LEAD_MS + STEP_MS + 1_000), step(2, STEP_MS - 1_000));
        assert_eq!(phase_at(LEAD_MS + total_ms() - 1), step(3, 1));
        assert_eq!(phase_at(LEAD_MS + total_ms()), Phase::Finished);
    }

    #[test]
    fn the_start_of_each_step_is_not_measured() {
        assert_eq!(step_of(0), None);
        assert_eq!(step_of(STEP_SETTLE_MS - 1), None);
        assert_eq!(step_of(STEP_SETTLE_MS), Some(0));
        assert_eq!(step_of(STEP_MS + 500), None);
        assert_eq!(step_of(2 * STEP_MS + STEP_SETTLE_MS), Some(2));
        assert_eq!(step_of(total_ms()), None);
    }

    #[test]
    fn a_contact_that_crosses_into_the_next_step_starts_a_new_life() {
        let mut f = Vec::new();
        hold(&mut f, 0, 4_000, STEP_MS + 2_000, touch(1, 0.5, 1.0, 10.0, 10.0));
        let lives = lives_per_step(&trace(f).frames, 0);
        assert_eq!(lives[0].len(), 1);
        assert_eq!(lives[1].len(), 1, "measured again from the end of step 2's settle");
        assert_eq!(lives[1][0].duration_ms, 1_000);
    }

    #[test]
    fn a_lifted_contact_whose_id_returns_is_two_lives() {
        let mut f = Vec::new();
        hold(&mut f, 2, 1_100, 1_200, touch(5, 0.5, 1.0, 10.0, 10.0));
        hold(&mut f, 2, 1_500, 1_600, touch(5, 0.5, 1.0, 10.0, 10.0));
        hold(&mut f, 2, 1_300, 1_400, touch(6, 0.5, 1.0, 10.0, 10.0)); // a frame in the gap
        let lives = lives_per_step(&trace(f).frames, 0);
        assert_eq!(lives[2].iter().filter(|l| l.duration_ms == 100).count(), 3);
    }

    #[test]
    fn a_clear_session_proposes_all_four_values_between_the_classes() {
        let p = propose(&trace(session()), &GuardConfig::default()).unwrap();
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        let v = |k: &str| p.changes.iter().find(|c| c.key == k).map(|c| c.proposed);
        // palm: halfway between finger 0.9 and palm 2.9
        assert_eq!(v("palm_size"), Some(1.9));
        // thumb ratio: finger 1.2, thumb 2.4 → 1.8
        assert_eq!(v("thumb_ratio"), Some(1.8));
        // thumb size: finger 0.9, thumb 1.6 → 1.25
        assert!((v("thumb_min_size").unwrap() - 1.25).abs() < 1e-9);
        // landing 0.8, minus the 0.05 margin
        assert!((v("thumb_zone").unwrap() - 0.75).abs() < 1e-9);
        assert_eq!(p.finger.unwrap().count, 9);
        assert_eq!(p.palm.unwrap().count, 2);
        assert_eq!(p.device.unwrap().builtin, Some(true));
    }

    #[test]
    fn the_proposal_classifies_the_session_it_came_from() {
        // The real check: with the proposed values, each measured class falls
        // on its own side of the thresholds.
        let p = propose(&trace(session()), &GuardConfig::default()).unwrap();
        let g = apply(&GuardConfig::default(), &p.changes);
        let (palm, thumb, finger) = (p.palm.unwrap(), p.thumb.unwrap(), p.finger.unwrap());
        assert!(palm.size_p10 >= g.palm_size as f64, "palms are palms");
        assert!(finger.size_p90 < g.palm_size as f64, "fingers are no palms");
        assert!(thumb.ratio_p10 >= g.thumb_ratio as f64 && thumb.size_p10 >= g.thumb_min_size as f64);
        assert!(thumb.y_p10 >= g.thumb_zone, "the thumb lands inside the thumb zone");
        assert!(finger.ratio_p90 < g.thumb_ratio as f64, "fingers are no thumbs");
    }

    #[test]
    fn a_palm_left_resting_in_step_three_is_not_measured_as_a_finger() {
        let mut f = session();
        hold(&mut f, 2, 1_050, 5_900, touch(99, 0.9, 3.0, 30.0, 20.0));
        let p = propose(&trace(f), &GuardConfig::default()).unwrap();
        // The count, not a percentile: one outlier in ten can't move a p90.
        assert_eq!(p.finger.unwrap().count, 9, "the long contact is filtered out");
    }

    #[test]
    fn classes_that_overlap_keep_the_value_and_say_why() {
        let mut f = Vec::new();
        hold(&mut f, 0, 1_200, 5_000, touch(1, 0.9, 0.95, 12.0, 10.0)); // "palm" as small as a finger
        hold(&mut f, 1, 1_500, 5_000, touch(3, 0.8, 0.9, 12.0, 10.0)); // round, small thumb
        for k in 0..6 {
            hold(&mut f, 2, 1_200 + k * 200, 1_300 + k * 200, touch(10 + k as i32, 0.4, 0.9, 12.0, 10.0));
        }
        let p = propose(&trace(f), &GuardConfig::default()).unwrap();
        assert!(p.changes.is_empty(), "{:?}", p.changes);
        assert_eq!(p.warnings.len(), 2);
    }

    #[test]
    fn a_thumb_separated_only_by_its_shape_lowers_the_size_gate_under_it() {
        let mut f = session();
        f.retain(|fr| !(STEP_MS..2 * STEP_MS).contains(&fr.t_ms));
        hold(&mut f, 1, 1_500, 5_000, touch(3, 0.8, 0.95, 24.0, 10.0)); // long, but finger-sized
        let p = propose(&trace(f), &GuardConfig::default()).unwrap();
        let v = |k: &str| p.changes.iter().find(|c| c.key == k).map(|c| c.proposed);
        let size = v("thumb_min_size").unwrap();
        assert!(size <= 0.95, "the thumb must still pass the size gate, got {size}");
        assert_eq!(v("thumb_ratio"), Some(1.8));
    }

    #[test]
    fn too_few_finger_contacts_is_an_error_not_a_guess() {
        let mut f = session();
        f.retain(|fr| fr.t_ms < 2 * STEP_MS);
        let e = propose(&trace(f), &GuardConfig::default()).unwrap_err();
        assert!(e.contains("Schritt 3"), "{e}");
        assert!(propose(&trace(vec![]), &GuardConfig::default()).is_err());
    }

    #[test]
    fn missing_steps_one_and_two_only_warn() {
        let mut f = session();
        f.retain(|fr| fr.t_ms >= 2 * STEP_MS);
        let p = propose(&trace(f), &GuardConfig::default()).unwrap();
        assert!(p.changes.is_empty());
        assert_eq!(p.warnings.len(), 2);
    }

    #[test]
    fn apply_sets_exactly_the_proposed_fields() {
        let g = GuardConfig::default();
        let out = apply(
            &g,
            &[
                Change { key: "palm_size", current: 2.0, proposed: 1.5 },
                Change { key: "nonsense", current: 0.0, proposed: 9.0 },
            ],
        );
        assert_eq!(out.palm_size, 1.5);
        assert_eq!(GuardConfig { palm_size: 2.0, ..out }, g);
    }

    /// The panel's copy: one text per step, one label per proposed key. Read
    /// from the TypeScript source so neither side can drift alone.
    #[test]
    fn the_panel_has_a_text_for_every_step_and_a_label_for_every_key() {
        let ts = include_str!("../../../frontend/src/lib/gesture-calibrate.ts");
        // From the array literal on — the type annotation names `title:` too.
        let decl = ts.find("export const CALIBRATION_STEPS").expect("step list");
        let start = decl + ts[decl..].find("= [").expect("step array");
        let end = start + ts[start..].find("];").expect("end of step list");
        assert_eq!(ts[start..end].matches("title:").count(), STEPS);
        let labels = &ts[ts.find("const CHANGE_LABEL").expect("label table")..];
        let labels = &labels[..labels.find("};").expect("end of label table")];
        for key in ["palm_size", "thumb_ratio", "thumb_min_size", "thumb_zone"] {
            assert!(labels.contains(&format!("{key}:")), "no panel label for {key}");
            let mut g = GuardConfig::default();
            let before = g;
            g = apply(&g, &[Change { key, current: 0.0, proposed: 0.42 }]);
            assert_ne!(g, before, "apply ignores {key}");
        }
    }
}
