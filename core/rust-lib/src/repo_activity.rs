//! "What happened in the last 24 h / 7 days" (v0.185.0) — four rolling,
//! half-open windows `(start, end]` from `now`, each compared with the
//! equally long period before it. Pure; the time source is the COMMITTER
//! timestamp (`%ct`), so a commit rewritten today (rebase/cherry-pick) counts
//! today even though its author date is old.

use crate::repo_stats::Commit;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const DAY: i64 = 86_400;
pub const WEEK: i64 = 7 * DAY;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ActivityCounts {
    pub commits: u64,
    pub insertions: u64,
    pub deletions: u64,
    /// Distinct paths touched.
    pub files: u64,
    /// Distinct authors.
    pub authors: u64,
    /// Tags created in the window.
    pub tags: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct RecentActivity {
    pub now: i64,
    pub day: ActivityCounts,
    pub day_prev: ActivityCounts,
    pub week: ActivityCounts,
    pub week_prev: ActivityCounts,
}

/// Half-open `(start, end]`: an event exactly on a boundary belongs to the
/// OLDER window, so nothing is counted twice.
pub fn in_window(t: i64, start: i64, end: i64) -> bool {
    t > start && t <= end
}

fn counts(commits: &[Commit], tag_times: &[i64], start: i64, end: i64) -> ActivityCounts {
    let mut out = ActivityCounts::default();
    let mut files: BTreeSet<&str> = BTreeSet::new();
    let mut authors: BTreeSet<&str> = BTreeSet::new();
    for c in commits {
        let Some(t) = c.committed else { continue };
        if !in_window(t, start, end) {
            continue;
        }
        out.commits += 1;
        authors.insert(&c.author_key);
        for (p, i, d) in &c.files {
            out.insertions += i;
            out.deletions += d;
            files.insert(p);
        }
    }
    out.files = files.len() as u64;
    out.authors = authors.len() as u64;
    out.tags = tag_times.iter().filter(|&&t| in_window(t, start, end)).count() as u64;
    out
}

pub fn recent_activity(commits: &[Commit], tag_times: &[i64], now: i64) -> RecentActivity {
    RecentActivity {
        now,
        day: counts(commits, tag_times, now - DAY, now),
        day_prev: counts(commits, tag_times, now - 2 * DAY, now - DAY),
        week: counts(commits, tag_times, now - WEEK, now),
        week_prev: counts(commits, tag_times, now - 2 * WEEK, now - WEEK),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_790_000_000;

    fn c(committed: Option<i64>, author: &str, files: &[(&str, u64, u64)]) -> Commit {
        Commit {
            iso: "2020-01-01T00:00:00+00:00".into(), // old author date on purpose
            day: None,
            author_key: author.into(),
            author_name: author.into(),
            subject: "x".into(),
            committed,
            files: files.iter().map(|(p, i, d)| (p.to_string(), *i, *d)).collect(),
        }
    }

    #[test]
    fn boundaries_are_half_open() {
        assert!(in_window(NOW, NOW - DAY, NOW));
        assert!(!in_window(NOW - DAY, NOW - DAY, NOW)); // exactly 24 h ago → previous day
        assert!(in_window(NOW - DAY, NOW - 2 * DAY, NOW - DAY));
        let r = recent_activity(&[c(Some(NOW - DAY), "a", &[("f", 1, 0)])], &[], NOW);
        assert_eq!((r.day.commits, r.day_prev.commits), (0, 1));
    }

    #[test]
    fn a_rewritten_old_commit_counts_today() {
        let r = recent_activity(&[c(Some(NOW - 60), "a", &[("f", 3, 1)])], &[], NOW);
        assert_eq!(r.day.commits, 1);
        assert_eq!((r.day.insertions, r.day.deletions), (3, 1));
    }

    #[test]
    fn counts_unique_files_authors_and_tags_per_window() {
        let commits = [
            c(Some(NOW - 100), "a", &[("x.rs", 1, 0), ("y.rs", 2, 2)]),
            c(Some(NOW - 200), "b", &[("x.rs", 1, 0)]),
            c(Some(NOW - 3 * DAY), "a", &[("z.rs", 5, 0)]),
            c(Some(NOW - 8 * DAY), "c", &[("q.rs", 1, 1)]),
            c(None, "d", &[("n.rs", 9, 9)]), // no commit time → in no window
        ];
        let r = recent_activity(&commits, &[NOW - 10, NOW - 2 * DAY, NOW - 9 * DAY], NOW);
        assert_eq!(r.now, NOW);
        assert_eq!((r.day.commits, r.day.files, r.day.authors, r.day.tags), (2, 2, 2, 1));
        assert_eq!((r.week.commits, r.week.files, r.week.authors, r.week.tags), (3, 3, 2, 2));
        assert_eq!((r.week_prev.commits, r.week_prev.authors, r.week_prev.tags), (1, 1, 1));
        assert_eq!((r.week.insertions, r.week.deletions), (9, 2));
    }

    #[test]
    fn nothing_recent_is_zero_not_missing() {
        let r = recent_activity(&[], &[], NOW);
        assert_eq!(r.day, ActivityCounts::default());
        assert_eq!(r.week_prev, ActivityCounts::default());
    }
}
