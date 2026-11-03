//! The inference queue: one GPU, two streams. Finals (what ends up in the
//! transcript) always go first; a waiting hypothesis is replaced by a newer
//! one of the same source, so a busy GPU never works through stale previews.

use crate::protocol::Source;
use std::collections::VecDeque;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Final,
    Hypothesis,
}

#[derive(Debug, Clone)]
pub struct Job {
    pub kind: JobKind,
    pub source: Source,
    pub seq: u64,
    /// Absolute stream position of the first sample (ms).
    pub start_ms: u64,
    pub samples: Arc<Vec<f32>>,
    pub prompt: String,
    pub lang: Option<String>,
}

#[derive(Debug, Default)]
pub struct JobQueue {
    finals: VecDeque<Job>,
    hypotheses: VecDeque<Job>,
    last_hypothesis_source: Option<Source>,
}

impl JobQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, job: Job) {
        match job.kind {
            JobKind::Final => {
                // A final supersedes any preview of the same source.
                self.hypotheses.retain(|h| h.source != job.source);
                self.finals.push_back(job);
            }
            JobKind::Hypothesis => {
                if let Some(slot) = self.hypotheses.iter_mut().find(|h| h.source == job.source) {
                    *slot = job;
                } else {
                    self.hypotheses.push_back(job);
                }
            }
        }
    }

    pub fn pop(&mut self) -> Option<Job> {
        if let Some(f) = self.finals.pop_front() {
            return Some(f);
        }
        // Alternate between sources so one busy stream cannot starve the other.
        let idx = match self.last_hypothesis_source {
            Some(last) => self.hypotheses.iter().position(|h| h.source != last).unwrap_or(0),
            None => 0,
        };
        let job = self.hypotheses.remove(idx)?;
        self.last_hypothesis_source = Some(job.source);
        Some(job)
    }

    pub fn len(&self) -> usize {
        self.finals.len() + self.hypotheses.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(kind: JobKind, source: Source, seq: u64) -> Job {
        Job { kind, source, seq, start_ms: 0, samples: Arc::new(vec![]), prompt: String::new(), lang: None }
    }

    #[test]
    fn finals_beat_hypotheses() {
        let mut q = JobQueue::new();
        q.push(job(JobKind::Hypothesis, Source::Mic, 1));
        q.push(job(JobKind::Final, Source::Sys, 2));
        assert_eq!(q.pop().unwrap().seq, 2);
        assert_eq!(q.pop().unwrap().seq, 1);
        assert!(q.pop().is_none());
    }

    #[test]
    fn newer_hypothesis_replaces_waiting_one() {
        let mut q = JobQueue::new();
        q.push(job(JobKind::Hypothesis, Source::Mic, 1));
        q.push(job(JobKind::Hypothesis, Source::Mic, 2));
        assert_eq!(q.len(), 1);
        assert_eq!(q.pop().unwrap().seq, 2);
    }

    #[test]
    fn a_final_removes_the_preview_of_its_source() {
        let mut q = JobQueue::new();
        q.push(job(JobKind::Hypothesis, Source::Mic, 1));
        q.push(job(JobKind::Hypothesis, Source::Sys, 2));
        q.push(job(JobKind::Final, Source::Mic, 3));
        assert_eq!(q.pop().unwrap().seq, 3);
        assert_eq!(q.pop().unwrap().seq, 2);
        assert!(q.is_empty());
    }

    #[test]
    fn sources_alternate_between_hypotheses() {
        let mut q = JobQueue::new();
        q.push(job(JobKind::Hypothesis, Source::Mic, 1));
        q.push(job(JobKind::Hypothesis, Source::Sys, 2));
        assert_eq!(q.pop().unwrap().source, Source::Mic);
        q.push(job(JobKind::Hypothesis, Source::Mic, 3));
        assert_eq!(q.pop().unwrap().source, Source::Sys);
        assert_eq!(q.pop().unwrap().source, Source::Mic);
    }
}
