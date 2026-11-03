//! Cuts a 16 kHz stream into speech segments, driven by per-frame VAD
//! probabilities (Silero, 512-sample frames).
//!
//! Hysteresis: speech starts at `p ≥ SPEECH_ON` and ends after `MIN_SILENCE`
//! of `p < SPEECH_OFF`. A segment carries `PAD` before its first and after its
//! last speech frame (word onsets and endings are quiet). One longer than
//! `max_seg` is cut at the quietest frame of its last `CUT_SEARCH` samples and
//! continues without a gap. Outside speech only `PAD` samples are kept, so
//! hours of silence never grow a buffer.

pub const RATE: usize = 16_000;
pub const VAD_FRAME: usize = 512;
pub const PAD: usize = RATE * 3 / 10;
pub const MAX_SEG: usize = RATE * 15;
pub const CUT_SEARCH: usize = RATE * 3;
pub const SPEECH_ON: f32 = 0.5;
pub const SPEECH_OFF: f32 = 0.35;
pub const MIN_SILENCE: usize = RATE * 6 / 10;
pub const MIN_SPEECH: usize = RATE / 4;

/// Absolute sample indices `[start, end)` of the stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seg {
    pub start: u64,
    pub end: u64,
}

#[derive(Debug)]
pub struct Segmenter {
    /// Absolute index of `buf[0]`.
    pos: u64,
    buf: Vec<f32>,
    in_speech: bool,
    start: u64,
    speech: usize,
    silence: usize,
    last_speech_end: u64,
    max_seg: usize,
}

impl Default for Segmenter {
    fn default() -> Self {
        Self::new(MAX_SEG)
    }
}

fn rms(s: &[f32]) -> f32 {
    if s.is_empty() {
        return 0.0;
    }
    (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt()
}

impl Segmenter {
    pub fn new(max_seg: usize) -> Self {
        Segmenter {
            pos: 0,
            buf: Vec::new(),
            in_speech: false,
            start: 0,
            speech: 0,
            silence: 0,
            last_speech_end: 0,
            max_seg: max_seg.max(CUT_SEARCH + VAD_FRAME),
        }
    }

    fn end(&self) -> u64 {
        self.pos + self.buf.len() as u64
    }

    fn slice(&self, a: u64, b: u64) -> Vec<f32> {
        let a = (a.saturating_sub(self.pos)) as usize;
        let b = ((b.saturating_sub(self.pos)) as usize).min(self.buf.len());
        self.buf[a.min(b)..b].to_vec()
    }

    /// Forget everything before absolute index `at`.
    fn drop_before(&mut self, at: u64) {
        let n = (at.saturating_sub(self.pos) as usize).min(self.buf.len());
        self.buf.drain(..n);
        self.pos += n as u64;
    }

    /// Feed samples and their VAD probabilities (one per `VAD_FRAME`
    /// samples); returns the segments that closed, with their samples.
    pub fn push(&mut self, samples: &[f32], probs: &[f32]) -> Vec<(Seg, Vec<f32>)> {
        let mut out = Vec::new();
        for (i, &p) in probs.iter().enumerate() {
            let a = i * VAD_FRAME;
            let b = (a + VAD_FRAME).min(samples.len());
            if a >= b {
                break;
            }
            let frame_start = self.end();
            self.buf.extend_from_slice(&samples[a..b]);
            let frame_end = self.end();
            let n = b - a;

            if !self.in_speech {
                if p >= SPEECH_ON {
                    self.in_speech = true;
                    self.start = frame_start.saturating_sub(PAD as u64).max(self.pos);
                    self.speech = n;
                    self.silence = 0;
                    self.last_speech_end = frame_end;
                } else if self.buf.len() > PAD + VAD_FRAME {
                    let keep_from = frame_end - PAD as u64;
                    self.drop_before(keep_from);
                }
                continue;
            }

            if p >= SPEECH_OFF {
                self.speech += n;
                self.silence = 0;
                self.last_speech_end = frame_end;
            } else {
                self.silence += n;
            }

            if self.silence >= MIN_SILENCE {
                let end = (self.last_speech_end + PAD as u64).min(frame_end);
                if self.speech >= MIN_SPEECH {
                    out.push((Seg { start: self.start, end }, self.slice(self.start, end)));
                }
                self.in_speech = false;
                let keep_from = frame_end.saturating_sub(PAD as u64).max(end);
                self.drop_before(keep_from);
                continue;
            }

            if (frame_end - self.start) as usize >= self.max_seg {
                let cut = self.quietest(frame_end);
                out.push((Seg { start: self.start, end: cut }, self.slice(self.start, cut)));
                self.drop_before(cut);
                self.start = cut;
                self.speech = (frame_end - cut) as usize;
            }
        }
        out
    }

    /// Start of the quietest frame in the last `CUT_SEARCH` samples.
    fn quietest(&self, frame_end: u64) -> u64 {
        let from = frame_end - CUT_SEARCH as u64;
        let mut best = (f32::MAX, frame_end);
        let mut at = from;
        while at + VAD_FRAME as u64 <= frame_end {
            let r = rms(&self.slice(at, at + VAD_FRAME as u64));
            if r < best.0 {
                best = (r, at);
            }
            at += (VAD_FRAME / 4) as u64;
        }
        // Cut in the middle of the quietest frame.
        best.1 + (VAD_FRAME / 2) as u64
    }

    /// The segment being collected, for live hypotheses.
    pub fn open(&self) -> Option<(Seg, &[f32])> {
        if !self.in_speech {
            return None;
        }
        let a = (self.start - self.pos) as usize;
        Some((Seg { start: self.start, end: self.end() }, &self.buf[a..]))
    }

    /// End of stream: close the open segment.
    pub fn flush(&mut self) -> Option<(Seg, Vec<f32>)> {
        if !self.in_speech {
            return None;
        }
        self.in_speech = false;
        if self.speech < MIN_SPEECH {
            return None;
        }
        let end = (self.last_speech_end + PAD as u64).min(self.end());
        let samples = self.slice(self.start, end);
        Some((Seg { start: self.start, end }, samples))
    }

    pub fn buffered(&self) -> usize {
        self.buf.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: usize = VAD_FRAME;

    /// `frames` of loud or silent audio with matching probabilities.
    fn chunk(frames: usize, speech: bool, p: f32) -> (Vec<f32>, Vec<f32>) {
        let s: Vec<f32> = (0..frames * F)
            .map(|i| if speech { ((i as f32) * 0.07).sin() * 0.5 } else { 0.0 })
            .collect();
        (s, vec![p; frames])
    }

    fn feed(seg: &mut Segmenter, parts: &[(usize, bool, f32)]) -> Vec<(Seg, Vec<f32>)> {
        let mut out = Vec::new();
        for &(n, speech, p) in parts {
            let (s, pr) = chunk(n, speech, p);
            out.extend(seg.push(&s, &pr));
        }
        out
    }

    #[test]
    fn speech_between_silence_is_one_segment_with_padding() {
        let mut s = Segmenter::default();
        let out = feed(&mut s, &[(32, false, 0.0), (62, true, 0.9), (32, false, 0.0)]);
        assert_eq!(out.len(), 1);
        let seg = out[0].0;
        assert_eq!(seg.start, (32 * F - PAD) as u64);
        assert_eq!(seg.end, (94 * F + PAD) as u64);
        assert_eq!(out[0].1.len() as u64, seg.end - seg.start);
        assert!(s.open().is_none());
    }

    #[test]
    fn short_blip_is_ignored() {
        let mut s = Segmenter::default();
        assert!(feed(&mut s, &[(10, false, 0.0), (3, true, 0.9), (40, false, 0.0)]).is_empty());
    }

    #[test]
    fn hysteresis_does_not_split_on_short_dips() {
        let mut s = Segmenter::default();
        // 0.5 s of p = 0.4 (below ON, above OFF) and 0.2 s below OFF.
        let out = feed(
            &mut s,
            &[(20, true, 0.9), (15, true, 0.4), (6, false, 0.1), (20, true, 0.9), (40, false, 0.0)],
        );
        assert_eq!(out.len(), 1, "{:?}", out.iter().map(|o| o.0).collect::<Vec<_>>());
    }

    #[test]
    fn long_speech_is_cut_at_the_quietest_frame() {
        let mut s = Segmenter::default();
        let n = RATE * 20;
        let quiet = RATE * 27 / 2; // 13.5 s
        let samples: Vec<f32> = (0..n)
            .map(|i| if (quiet..quiet + 2 * F).contains(&i) { 0.0 } else { ((i as f32) * 0.07).sin() * 0.5 })
            .collect();
        let probs = vec![0.9; n / F];
        let out = s.push(&samples[..probs.len() * F], &probs);
        assert_eq!(out.len(), 1);
        let first = out[0].0;
        assert_eq!(first.start, 0);
        assert!((first.end as i64 - quiet as i64).abs() <= 2 * F as i64, "cut at {}", first.end);
        assert!(first.end - first.start <= MAX_SEG as u64);
        let (open, _) = s.open().unwrap();
        assert_eq!(open.start, first.end, "no gap");
    }

    #[test]
    fn two_hours_of_silence_keeps_the_buffer_small() {
        let mut s = Segmenter::default();
        let (sil, p) = chunk(31, false, 0.0);
        for _ in 0..7200 {
            assert!(s.push(&sil, &p).is_empty());
        }
        assert!(s.buffered() <= PAD + F, "{}", s.buffered());
    }

    #[test]
    fn open_segment_is_visible_while_speaking_and_flush_closes_it() {
        let mut s = Segmenter::default();
        feed(&mut s, &[(5, false, 0.0), (20, true, 0.9)]);
        let (seg, samples) = s.open().unwrap();
        assert_eq!(samples.len() as u64, seg.end - seg.start);
        let (closed, _) = s.flush().unwrap();
        assert_eq!(closed.start, seg.start);
        assert!(s.flush().is_none());
    }
}
