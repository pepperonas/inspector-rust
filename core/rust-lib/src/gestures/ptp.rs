//! Windows Precision Touchpad (PTP) → platform-neutral [`Touch`]es (gesture
//! guard Phase 6). Pure — no Windows API — so it is tested on every platform;
//! `windows.rs` only reads the HID fields and hands them here.
//!
//! Three jobs:
//!
//! * [`PtpAssembler`]: a PTP frame can arrive split over several HID reports
//!   ("hybrid mode"): the first carries the Contact Count, the follow-ups
//!   carry 0. Frames are only handed on once complete, otherwise a contact
//!   that is merely in the next report would look lifted.
//! * [`PtpTracker`]: contact ids → lifecycle phases (make / touching / break),
//!   velocity, and the **confidence bit**: a contact the touchpad itself
//!   reports as "not a finger" becomes [`Touch::palm`], which level 1 treats
//!   as a palm until it lifts.
//! * [`physical_mm`]: the descriptor's physical extents → millimetres, for
//!   the pad size (live view, edge zones) and the contact ellipse.
//!
//! What PTP does NOT report: a contact size comparable to macOS's `size`.
//! Classification on Windows therefore rests on the confidence bit (and
//! `palm_major` when the touchpad reports widths); the size-based palm and
//! thumb rules stay inactive there. Runtime-unverified like all Windows code
//! in this repo — the parsing is exercised by these tests only.

#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use super::trace::{Touch, TouchPhase};
use std::collections::HashMap;

/// One contact as a PTP report states it (HID logical units).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PtpContact {
    /// Contact Identifier (usage 0x51) — stable while the finger is down.
    pub id: u32,
    pub x: i32,
    pub y: i32,
    /// Tip Switch (0x42): down. A lifting contact is reported once with `false`.
    pub tip: bool,
    /// Confidence (0x47): `Some(false)` = the touchpad says it is not a
    /// finger (palm, cheek). `None` when the device has no such usage.
    pub confidence: Option<bool>,
    /// Width / Height (0x48 / 0x49), when the device reports them.
    pub width: Option<i32>,
    pub height: Option<i32>,
}

/// The pad's logical ranges and, when the descriptor gives them, its size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PtpGeometry {
    pub x_min: i32,
    pub x_max: i32,
    pub y_min: i32,
    pub y_max: i32,
    pub width_mm: Option<f64>,
    pub height_mm: Option<f64>,
}

impl PtpGeometry {
    fn norm_x(&self, x: i32) -> f64 {
        ((x - self.x_min) as f64 / (self.x_max - self.x_min).max(1) as f64).clamp(0.0, 1.0)
    }
    fn norm_y(&self, y: i32) -> f64 {
        ((y - self.y_min) as f64 / (self.y_max - self.y_min).max(1) as f64).clamp(0.0, 1.0)
    }
    fn mm_per_x(&self) -> Option<f64> {
        self.width_mm.map(|w| w / (self.x_max - self.x_min).max(1) as f64)
    }
    fn mm_per_y(&self) -> Option<f64> {
        self.height_mm.map(|h| h / (self.y_max - self.y_min).max(1) as f64)
    }
}

/// Physical span of a HID value in millimetres, from the descriptor's
/// physical extents and units. `units` is the HID Unit item (low nibble = the
/// system: 1 = SI linear → cm, 3 = English linear → inch; next nibble = the
/// length exponent, which must be 1), `units_exp` the Unit Exponent nibble
/// (two's complement: 0xE = −2). `None` when the descriptor doesn't say.
pub fn physical_mm(phys_min: i32, phys_max: i32, units: u32, units_exp: u32) -> Option<f64> {
    if (units >> 4) & 0xF != 1 {
        return None;
    }
    let mm_per_unit = match units & 0xF {
        1 => 10.0,
        3 => 25.4,
        _ => return None,
    };
    let nib = (units_exp & 0xF) as i32;
    let exp = if nib >= 8 { nib - 16 } else { nib };
    let span = (phys_max - phys_min) as f64;
    (span > 0.0).then(|| span * 10f64.powi(exp) * mm_per_unit)
}

/// Collects the reports of one frame (see the module doc).
#[derive(Debug, Default)]
pub struct PtpAssembler {
    expected: usize,
    pending: Vec<PtpContact>,
}

impl PtpAssembler {
    /// One HID report: its contact slots in order and its Contact Count.
    /// Returns the complete frame once all expected contacts are in. Slots
    /// beyond the count are padding and are dropped.
    pub fn push(&mut self, slots: &[PtpContact], contact_count: u32) -> Option<Vec<PtpContact>> {
        if contact_count > 0 {
            // A new frame — whatever was pending never completed.
            self.pending.clear();
            self.expected = contact_count as usize;
        } else if self.expected == 0 {
            // Neither a frame start nor an expected follow-up: a device that
            // never sets the count. Take what it says is down, as a frame.
            return Some(slots.iter().copied().filter(|c| c.tip).collect());
        }
        let room = self.expected - self.pending.len();
        self.pending.extend(slots.iter().copied().take(room));
        if self.pending.len() >= self.expected {
            self.expected = 0;
            return Some(std::mem::take(&mut self.pending));
        }
        None
    }
}

#[derive(Debug, Clone, Copy)]
struct Seen {
    x: f64,
    y: f64,
    t_ms: u64,
    major: f32,
    minor: f32,
    angle: f32,
    palm: bool,
}

/// Contact ids → phases, velocity and the sticky confidence verdict.
#[derive(Debug, Default)]
pub struct PtpTracker {
    down: HashMap<u32, Seen>,
}

impl PtpTracker {
    /// One complete frame at `t_ms` → touches. A contact missing from the
    /// frame without having been reported lifted is lifted here (break at its
    /// last position), so the pipeline never keeps a ghost finger.
    pub fn frame(&mut self, t_ms: u64, contacts: &[PtpContact], g: &PtpGeometry) -> Vec<Touch> {
        let mut out = Vec::with_capacity(contacts.len());
        let mut present = Vec::with_capacity(contacts.len());
        for c in contacts {
            let x = g.norm_x(c.x);
            let y = g.norm_y(c.y);
            let prev = self.down.get(&c.id).copied();
            let (major, minor, angle) = ellipse(c, g).or(prev.map(|p| (p.major, p.minor, p.angle))).unwrap_or((0.0, 0.0, 0.0));
            if !c.tip {
                if self.down.remove(&c.id).is_some() {
                    out.push(touch(c.id, x, y, (0.0, 0.0), (major, minor, angle), TouchPhase::Break, prev.is_some_and(|p| p.palm)));
                }
                continue;
            }
            present.push(c.id);
            // Sticky: once the touchpad doubted it, it stays a palm until lift.
            let palm = c.confidence == Some(false) || prev.is_some_and(|p| p.palm);
            let (vx, vy, phase) = match prev {
                Some(p) if t_ms > p.t_ms => {
                    let dt = (t_ms - p.t_ms) as f64 / 1000.0;
                    ((x - p.x) / dt, (y - p.y) / dt, TouchPhase::Touching)
                }
                Some(_) => (0.0, 0.0, TouchPhase::Touching),
                None => (0.0, 0.0, TouchPhase::Make),
            };
            self.down.insert(c.id, Seen { x, y, t_ms, major, minor, angle, palm });
            out.push(touch(c.id, x, y, (vx, vy), (major, minor, angle), phase, palm));
        }
        let vanished: Vec<u32> = self.down.keys().copied().filter(|id| !present.contains(id)).collect();
        for id in vanished {
            if let Some(p) = self.down.remove(&id) {
                out.push(touch(id, p.x, p.y, (0.0, 0.0), (p.major, p.minor, p.angle), TouchPhase::Break, p.palm));
            }
        }
        out
    }
}

/// Width/height → ellipse axes in mm, the major axis' angle (0 = along x).
fn ellipse(c: &PtpContact, g: &PtpGeometry) -> Option<(f32, f32, f32)> {
    let w = c.width? as f64 * g.mm_per_x()?;
    let h = c.height? as f64 * g.mm_per_y()?;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let angle = if h > w { std::f32::consts::FRAC_PI_2 } else { 0.0 };
    Some((w.max(h) as f32, w.min(h) as f32, angle))
}

fn touch(id: u32, x: f64, y: f64, v: (f64, f64), e: (f32, f32, f32), phase: TouchPhase, palm: bool) -> Touch {
    Touch {
        id: id as i32,
        x,
        y,
        vx: v.0,
        vy: v.1,
        major: e.0,
        minor: e.1,
        angle: e.2,
        size: 0.0,
        phase,
        palm,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: PtpGeometry =
        PtpGeometry { x_min: 0, x_max: 1000, y_min: 0, y_max: 500, width_mm: Some(100.0), height_mm: Some(50.0) };

    fn c(id: u32, x: i32, y: i32, tip: bool) -> PtpContact {
        PtpContact { id, x, y, tip, confidence: Some(true), width: None, height: None }
    }

    #[test]
    fn physical_extents_become_millimetres() {
        // The usual PTP descriptor: centimetres (0x11), exponent −2 (0xE).
        assert_eq!(physical_mm(0, 1100, 0x11, 0xE), Some(110.0));
        // Inches, exponent −3: 4.000 in.
        assert!((physical_mm(0, 4000, 0x13, 0xD).unwrap() - 101.6).abs() < 1e-9);
        assert_eq!(physical_mm(0, 1100, 0x00, 0xE), None, "no unit = no size, not a guess");
        assert_eq!(physical_mm(0, 1100, 0x21, 0xE), None, "area, not length");
        assert_eq!(physical_mm(500, 500, 0x11, 0xE), None);
    }

    #[test]
    fn a_frame_in_one_report_passes_and_padding_is_dropped() {
        let mut a = PtpAssembler::default();
        let slots = [c(1, 10, 10, true), c(2, 20, 20, true), c(0, 0, 0, false)];
        assert_eq!(a.push(&slots, 2), Some(vec![slots[0], slots[1]]));
    }

    #[test]
    fn a_hybrid_frame_is_handed_on_only_when_complete() {
        let mut a = PtpAssembler::default();
        let first = [c(1, 10, 10, true), c(2, 20, 20, true)];
        let second = [c(3, 30, 30, true), c(0, 0, 0, false)];
        assert_eq!(a.push(&first, 3), None, "the third contact is still to come");
        assert_eq!(a.push(&second, 0), Some(vec![first[0], first[1], second[0]]));
    }

    #[test]
    fn an_incomplete_frame_is_dropped_when_the_next_one_starts() {
        let mut a = PtpAssembler::default();
        assert_eq!(a.push(&[c(1, 1, 1, true)], 3), None);
        assert_eq!(a.push(&[c(5, 5, 5, true)], 1), Some(vec![c(5, 5, 5, true)]));
    }

    #[test]
    fn a_device_without_a_count_still_delivers_what_is_down() {
        let mut a = PtpAssembler::default();
        assert_eq!(a.push(&[c(1, 1, 1, true), c(0, 0, 0, false)], 0), Some(vec![c(1, 1, 1, true)]));
    }

    #[test]
    fn contacts_land_move_and_lift() {
        let mut t = PtpTracker::default();
        let f1 = t.frame(0, &[c(7, 500, 250, true)], &G);
        assert_eq!((f1[0].phase, f1[0].x, f1[0].y), (TouchPhase::Make, 0.5, 0.5));
        let f2 = t.frame(100, &[c(7, 600, 250, true)], &G);
        assert_eq!(f2[0].phase, TouchPhase::Touching);
        assert!((f2[0].vx - 1.0).abs() < 1e-9, "0.1 of the pad in 0.1 s");
        let f3 = t.frame(110, &[c(7, 600, 250, false)], &G);
        assert_eq!(f3[0].phase, TouchPhase::Break);
        assert!(t.frame(120, &[], &G).is_empty(), "lifted once, not twice");
    }

    #[test]
    fn a_contact_that_vanishes_unreported_is_lifted_not_left_as_a_ghost() {
        let mut t = PtpTracker::default();
        t.frame(0, &[c(1, 100, 100, true), c(2, 900, 100, true)], &G);
        let f = t.frame(10, &[c(1, 100, 100, true)], &G);
        let gone = f.iter().find(|x| x.id == 2).expect("contact 2 reported");
        assert_eq!(gone.phase, TouchPhase::Break);
        assert!((gone.x - 0.9).abs() < 1e-9, "at its last position");
        assert_eq!(t.frame(20, &[c(1, 100, 100, true)], &G).len(), 1);
    }

    #[test]
    fn low_confidence_marks_a_palm_and_it_sticks_until_lift() {
        let mut t = PtpTracker::default();
        let doubt = PtpContact { confidence: Some(false), ..c(3, 500, 450, true) };
        assert!(t.frame(0, &[doubt], &G)[0].palm);
        assert!(t.frame(10, &[c(3, 500, 450, true)], &G)[0].palm, "regained confidence doesn't clear it");
        assert!(t.frame(20, &[c(3, 500, 450, false)], &G)[0].palm, "the lift still says palm");
        assert!(!t.frame(30, &[c(3, 500, 450, true)], &G)[0].palm, "a new landing starts clean");
        let unknown = PtpContact { confidence: None, ..c(4, 1, 1, true) };
        assert!(!t.frame(40, &[unknown], &G).iter().any(|x| x.id == 4 && x.palm), "no usage = no verdict");
    }

    #[test]
    fn widths_become_an_ellipse_in_millimetres() {
        let mut t = PtpTracker::default();
        // 100 units wide = 10 mm, 40 units high = 4 mm.
        let wide = PtpContact { width: Some(100), height: Some(40), ..c(1, 10, 10, true) };
        let e = t.frame(0, &[wide], &G)[0];
        assert_eq!((e.major, e.minor, e.angle), (10.0, 4.0, 0.0));
        let tall = PtpContact { width: Some(40), height: Some(200), ..c(2, 10, 10, true) };
        let e = t.frame(0, &[tall], &G).into_iter().find(|x| x.id == 2).unwrap();
        assert_eq!((e.major, e.minor), (20.0, 4.0));
        assert!((e.angle - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert_eq!(e.size, 0.0, "PTP has no comparable size — never invented");
    }

    #[test]
    fn positions_are_clamped_to_the_pad() {
        let mut t = PtpTracker::default();
        let out = t.frame(0, &[c(1, -50, 900, true)], &G);
        assert_eq!((out[0].x, out[0].y), (0.0, 1.0));
    }
}
