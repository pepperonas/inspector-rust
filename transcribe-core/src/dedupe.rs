//! Echo gate: without headphones the microphone hears the speakers, so a
//! sentence from the other side arrives twice — once as SYS, once as MIC. A
//! MIC segment that overlaps a SYS segment in time and reads almost the same
//! is dropped. MIC finals wait up to `HOLD_MS` for the matching SYS final,
//! because the two streams finish their segments at different moments.

use crate::protocol::Source;
use crate::text::similarity;
use std::collections::VecDeque;

pub const OVERLAP_TOLERANCE_MS: u64 = 1000;
pub const HOLD_MS: u64 = 3000;
pub const SYS_MEMORY_MS: u64 = 30_000;
pub const DEFAULT_THRESHOLD: f32 = 0.75;

#[derive(Debug, Clone, PartialEq)]
pub struct FinalSeg {
    pub source: Source,
    pub seq: u64,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub lang: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Release {
    Keep(FinalSeg),
    Drop(FinalSeg, String),
}

#[derive(Debug)]
pub struct EchoGate {
    pub threshold: f32,
    pub enabled: bool,
    held: VecDeque<(FinalSeg, u64)>,
    recent_sys: VecDeque<FinalSeg>,
}

fn overlaps(a: &FinalSeg, b: &FinalSeg) -> bool {
    a.start_ms <= b.end_ms + OVERLAP_TOLERANCE_MS && b.start_ms <= a.end_ms + OVERLAP_TOLERANCE_MS
}

impl EchoGate {
    pub fn new(threshold: f32, enabled: bool) -> Self {
        EchoGate { threshold, enabled, held: VecDeque::new(), recent_sys: VecDeque::new() }
    }

    /// Is `mic` an echo of a remembered SYS segment (or of two neighbouring
    /// ones — the streams cut at different places)?
    fn echo_of(&self, mic: &FinalSeg) -> Option<f32> {
        let near: Vec<&FinalSeg> = self.recent_sys.iter().filter(|s| overlaps(mic, s)).collect();
        let mut best = 0.0f32;
        for (i, s) in near.iter().enumerate() {
            best = best.max(similarity(&mic.text, &s.text));
            if let Some(next) = near.get(i + 1) {
                best = best.max(similarity(&mic.text, &format!("{} {}", s.text, next.text)));
            }
        }
        (best >= self.threshold).then_some(best)
    }

    fn judge(&self, mic: FinalSeg) -> Release {
        match self.echo_of(&mic) {
            Some(sim) => Release::Drop(mic, format!("echo of system audio ({sim:.2})")),
            None => Release::Keep(mic),
        }
    }

    /// A MIC segment can be judged once SYS has finished past its end.
    fn sys_caught_up(&self, mic: &FinalSeg) -> bool {
        self.recent_sys
            .back()
            .is_some_and(|s| s.end_ms + OVERLAP_TOLERANCE_MS >= mic.end_ms)
    }

    fn release_ready(&mut self, now_ms: u64, force: bool) -> Vec<Release> {
        let mut out = Vec::new();
        while let Some((mic, arrived)) = self.held.front() {
            let ready = force || now_ms >= arrived + HOLD_MS || self.sys_caught_up(mic);
            if !ready {
                break;
            }
            let (mic, _) = self.held.pop_front().expect("front exists");
            out.push(self.judge(mic));
        }
        out
    }

    pub fn offer(&mut self, seg: FinalSeg, now_ms: u64) -> Vec<Release> {
        match seg.source {
            Source::Sys => {
                self.recent_sys.push_back(seg.clone());
                while self
                    .recent_sys
                    .front()
                    .is_some_and(|s| s.end_ms + SYS_MEMORY_MS < seg.end_ms)
                {
                    self.recent_sys.pop_front();
                }
                let mut out = vec![Release::Keep(seg)];
                out.extend(self.release_ready(now_ms, false));
                out
            }
            Source::Mic if !self.enabled => vec![Release::Keep(seg)],
            Source::Mic => {
                self.held.push_back((seg, now_ms));
                self.release_ready(now_ms, false)
            }
        }
    }

    pub fn tick(&mut self, now_ms: u64) -> Vec<Release> {
        self.release_ready(now_ms, false)
    }

    /// Session end: judge everything still held.
    pub fn drain(&mut self) -> Vec<Release> {
        self.release_ready(0, true)
    }
}

/// Does the current output look like headphones (no echo possible)?
pub fn headphones_suggest_off(transport: &str, data_source: Option<&str>) -> bool {
    let t = transport.to_lowercase();
    if t.contains("bluetooth") || t.contains("usb") {
        return true;
    }
    data_source.is_some_and(|d| {
        let d = d.to_lowercase();
        d.contains("headphone") || d.contains("kopfhörer")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(source: Source, seq: u64, start: u64, end: u64, text: &str) -> FinalSeg {
        FinalSeg { source, seq, start_ms: start, end_ms: end, text: text.into(), lang: "de".into(), confidence: 0.9 }
    }

    fn kinds(r: &[Release]) -> Vec<(Source, bool)> {
        r.iter()
            .map(|x| match x {
                Release::Keep(s) => (s.source, true),
                Release::Drop(s, _) => (s.source, false),
            })
            .collect()
    }

    #[test]
    fn speaker_echo_on_mic_is_dropped() {
        let mut g = EchoGate::new(DEFAULT_THRESHOLD, true);
        let r1 = g.offer(seg(Source::Sys, 1, 10_000, 12_000, "Wir treffen uns um drei."), 12_100);
        assert_eq!(kinds(&r1), vec![(Source::Sys, true)]);
        let r2 = g.offer(seg(Source::Mic, 2, 10_200, 12_300, "wir treffen uns um drei"), 12_400);
        assert_eq!(kinds(&r2), vec![(Source::Mic, false)]);
    }

    #[test]
    fn own_speech_overlapping_sys_is_kept() {
        let mut g = EchoGate::new(DEFAULT_THRESHOLD, true);
        g.offer(seg(Source::Sys, 1, 10_000, 12_000, "Wir treffen uns um drei."), 12_100);
        let r = g.offer(seg(Source::Mic, 2, 10_200, 12_300, "Passt mir gut, bis dann"), 12_400);
        assert_eq!(kinds(&r), vec![(Source::Mic, true)]);
    }

    #[test]
    fn mic_waits_for_late_sys() {
        let mut g = EchoGate::new(DEFAULT_THRESHOLD, true);
        assert!(g.offer(seg(Source::Mic, 1, 10_000, 12_000, "Das Wetter wird morgen schön"), 12_000).is_empty());
        assert!(g.tick(13_000).is_empty());
        let r = g.offer(seg(Source::Sys, 2, 9_900, 12_100, "Das Wetter wird morgen schön."), 13_500);
        assert_eq!(kinds(&r), vec![(Source::Sys, true), (Source::Mic, false)]);
    }

    #[test]
    fn mic_is_released_after_hold_without_sys() {
        let mut g = EchoGate::new(DEFAULT_THRESHOLD, true);
        g.offer(seg(Source::Mic, 1, 10_000, 12_000, "Hallo"), 12_000);
        assert!(g.tick(14_900).is_empty());
        assert_eq!(kinds(&g.tick(15_100)), vec![(Source::Mic, true)]);
    }

    #[test]
    fn echo_spanning_two_sys_segments_is_dropped() {
        let mut g = EchoGate::new(DEFAULT_THRESHOLD, true);
        g.offer(seg(Source::Sys, 1, 10_000, 11_500, "Ich habe heute keine Zeit,"), 11_600);
        g.offer(seg(Source::Sys, 2, 11_600, 13_000, "aber morgen gerne."), 13_100);
        let r = g.offer(
            seg(Source::Mic, 3, 10_100, 13_100, "Ich habe heute keine Zeit aber morgen gerne"),
            13_200,
        );
        assert_eq!(kinds(&r), vec![(Source::Mic, false)]);
    }

    #[test]
    fn same_text_far_apart_in_time_is_kept() {
        let mut g = EchoGate::new(DEFAULT_THRESHOLD, true);
        g.offer(seg(Source::Sys, 1, 10_000, 11_000, "Ja genau"), 11_100);
        assert!(g.offer(seg(Source::Mic, 2, 20_000, 21_000, "ja genau"), 21_100).is_empty());
        assert_eq!(kinds(&g.tick(24_200)), vec![(Source::Mic, true)]);
    }

    #[test]
    fn disabled_gate_keeps_everything_immediately() {
        let mut g = EchoGate::new(DEFAULT_THRESHOLD, false);
        g.offer(seg(Source::Sys, 1, 10_000, 12_000, "gleich"), 12_000);
        assert_eq!(kinds(&g.offer(seg(Source::Mic, 2, 10_000, 12_000, "gleich"), 12_000)), vec![(Source::Mic, true)]);
    }

    #[test]
    fn drain_releases_held_mic() {
        let mut g = EchoGate::new(DEFAULT_THRESHOLD, true);
        g.offer(seg(Source::Mic, 1, 0, 1000, "x"), 1000);
        assert_eq!(kinds(&g.drain()), vec![(Source::Mic, true)]);
    }

    #[test]
    fn headphones_detection() {
        assert!(headphones_suggest_off("bluetooth", None));
        assert!(!headphones_suggest_off("builtin", Some("Internal Speakers")));
        assert!(headphones_suggest_off("builtin", Some("Headphones")));
        assert!(!headphones_suggest_off("hdmi", None));
    }
}
