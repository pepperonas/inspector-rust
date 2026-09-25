//! `repo` / `repo export` — git repository activity stats (v0.123.0).
//!
//! Oriented on the maintainer's **repo2viz** project (`~/claude/repo2viz`,
//! Python): it clones a repo read-only, parses `git log --numstat` and renders
//! an interactive activity report. This module ports the analysis to Rust for
//! the popup preview + a self-contained HTML export.
//!
//! Sources, in order of what the caller resolves:
//!  * a GitHub (or any git) URL → **bare clone** to a temp dir (full history +
//!    blobs so `--numstat` churn is exact offline; no working tree), analysed,
//!    then deleted;
//!  * a **local path** / the Finder-selected folder that contains `.git` →
//!    analysed in place, no clone.
//!
//! The git-log invocation mirrors repo2viz: `--no-merges --numstat
//! --date=iso-strict` with a control-char-separated pretty format (RS `\x1e`
//! between records, US `\x1f` between fields) so commit subjects can't collide
//! with the delimiters. `parse_git_log` is pure and unit-tested against a
//! synthetic log; the clone/exec is the thin impure shell. `build_html`
//! renders the export as ONE self-contained file (inline SVG charts, no
//! external requests) and is tested structurally.

use serde::{Deserialize, Serialize};
use crate::report_style as rs;
use std::collections::BTreeMap;
use std::process::Command;

pub const REC: char = '\u{1e}';
pub const FLD: char = '\u{1f}';

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct RepoStats {
    pub name: String,
    pub source: String,
    pub commits: u64,
    pub contributors: u64,
    pub first_commit: String,
    pub last_commit: String,
    /// Distinct calendar days with at least one commit.
    pub active_days: u64,
    pub insertions: u64,
    pub deletions: u64,
    /// Commits per weekday, Mon..Sun (0..6).
    pub by_weekday: [u64; 7],
    /// Commits per hour of day, 0..23 (author-local time from %aI).
    pub by_hour: [u64; 24],
    /// Commits per YYYY-MM, chronological (activity timeline).
    pub by_month: Vec<MonthCount>,
    pub top_files: Vec<FileStat>,
    pub top_exts: Vec<ExtStat>,
    pub top_authors: Vec<AuthorStat>,
    /// Conventional-commit category → count (feat/fix/docs/…/other).
    pub categories: Vec<CatCount>,
    /// Longest run of consecutive days with commits.
    pub longest_streak: u64,
    pub avg_msg_len: u64,
    /// Weekday (Mon=0) × author-local hour.
    pub heatmap: [[u64; 24]; 7],
    /// Non-zero commit days within 365 days of the window's newest commit.
    pub calendar: Vec<DayCount>,
    /// Often-changed files with ≤ 2 authors — knowledge risk.
    pub hotspots: Vec<Hotspot>,
    /// Fewest authors that together hold ≥ 50 % of commits.
    pub bus_factor: u32,
    /// Per top-level directory ("(root)" for files at the root).
    pub dir_bus_factor: Vec<DirStat>,
    /// File pairs often changed together (commits with > 30 files skipped).
    pub co_change: Vec<CoChange>,
    /// Gapless time series for this range: commits + lines added/removed
    /// per bucket. Bucket size follows the range (see `granularity`) — a
    /// 30-day window used to be shown as "2 months" because it touched two
    /// calendar months.
    pub timeline: Vec<Bucket>,
    /// "day" (30 T) · "week" (90/180 T, Monday-start) · "month" (1 J, Gesamt);
    /// empty for a repo without commits.
    pub granularity: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MonthCount {
    pub month: String, // "YYYY-MM"
    pub commits: u64,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FileStat {
    pub path: String,
    pub changes: u64, // commits touching it
    pub churn: u64,   // insertions + deletions
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ExtStat {
    pub ext: String,
    pub commits: u64,
    pub churn: u64,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AuthorStat {
    pub name: String,
    pub commits: u64,
    pub churn: u64,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CatCount {
    pub cat: String,
    pub commits: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Bucket {
    /// First day of the bucket, `YYYY-MM-DD`.
    pub start: String,
    pub commits: u64,
    pub insertions: u64,
    pub deletions: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DayCount {
    pub date: String,
    pub commits: u64,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Hotspot {
    pub path: String,
    pub changes: u64,
    pub authors: u64,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DirStat {
    pub dir: String,
    pub commits: u64,
    pub authors: u64,
    pub bus_factor: u32,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct CoChange {
    pub a: String,
    pub b: String,
    pub count: u64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RangeKey {
    D30,
    D90,
    D180,
    Y1,
    All,
}

impl RangeKey {
    pub const ALL: [RangeKey; 5] = [RangeKey::D30, RangeKey::D90, RangeKey::D180, RangeKey::Y1, RangeKey::All];
    pub fn days(self) -> Option<i64> {
        match self {
            RangeKey::D30 => Some(30),
            RangeKey::D90 => Some(90),
            RangeKey::D180 => Some(180),
            RangeKey::Y1 => Some(365),
            RangeKey::All => None,
        }
    }
    pub fn parse(s: &str) -> Option<RangeKey> {
        Some(match s {
            "d30" => RangeKey::D30,
            "d90" => RangeKey::D90,
            "d180" => RangeKey::D180,
            "y1" => RangeKey::Y1,
            "all" => RangeKey::All,
            _ => return None,
        })
    }
    pub fn label(self) -> &'static str {
        match self {
            RangeKey::D30 => "letzte 30 Tage",
            RangeKey::D90 => "letzte 90 Tage",
            RangeKey::D180 => "letzte 180 Tage",
            RangeKey::Y1 => "letztes Jahr",
            RangeKey::All => "gesamt",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RangedStats {
    pub range: RangeKey,
    pub stats: RepoStats,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Commit {
    pub iso: String,
    pub day: Option<i64>,
    pub author_key: String,
    pub author_name: String,
    pub subject: String,
    /// Committer timestamp (Unix seconds, `%ct`) — "when it happened" for the
    /// 24 h / 7 d windows; `None` if git didn't print one.
    pub committed: Option<i64>,
    /// (path, insertions, deletions)
    pub files: Vec<(String, u64, u64)>,
}

/// Conventional-commit category from a subject line (feat/fix/docs/refactor/
/// perf/test/build/ci/chore/style/revert), else "other". Case-insensitive,
/// tolerates a scope: `feat(ui): …`.
pub fn classify_commit(subject: &str) -> &'static str {
    let s = subject.trim_start().to_ascii_lowercase();
    const CATS: [&str; 11] = [
        "feat", "fix", "docs", "refactor", "perf", "test", "build", "ci", "chore", "style",
        "revert",
    ];
    for c in CATS {
        if let Some(rest) = s.strip_prefix(c) {
            // Must be followed by ':' or '(' (scope) to count.
            let rest = rest.trim_start();
            if rest.starts_with(':') || rest.starts_with('(') {
                return match c {
                    "feat" => "feat",
                    "fix" => "fix",
                    "docs" => "docs",
                    "refactor" => "refactor",
                    "perf" => "perf",
                    "test" => "test",
                    "build" => "build",
                    "ci" => "ci",
                    "chore" => "chore",
                    "style" => "style",
                    _ => "revert",
                };
            }
        }
    }
    "other"
}

/// Extension of a path (lowercased, no dot), or "—" for extensionless files.
/// Only the final segment's extension, and only when it's short + alnum (so a
/// dotted directory like `.github/workflows/ci.yml` → "yml", not "github").
fn extension_of(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    match file.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 8 && ext.chars().all(|c| c.is_ascii_alphanumeric()) => {
            ext.to_ascii_lowercase()
        }
        _ => "—".to_string(),
    }
}

/// Weekday (0=Mon..6=Sun) + hour (0..23) from an ISO-8601 timestamp with
/// offset (`%aI`, e.g. "2026-08-24T14:30:00+02:00"). Uses the author-local
/// wall clock (the offset is part of the string, so we read it as-is). Pure —
/// a tiny Zeller-based weekday so no chrono parse of the tz is needed.
pub fn weekday_hour(iso: &str) -> Option<(usize, usize)> {
    // "YYYY-MM-DDTHH:MM:SS±HH:MM" — take the local wall-clock fields directly.
    let bytes = iso.as_bytes();
    if iso.len() < 16 || bytes[4] != b'-' || bytes[7] != b'-' || (bytes[10] != b'T' && bytes[10] != b' ') {
        return None;
    }
    let y: i64 = iso.get(0..4)?.parse().ok()?;
    let m: i64 = iso.get(5..7)?.parse().ok()?;
    let d: i64 = iso.get(8..10)?.parse().ok()?;
    let h: usize = iso.get(11..13)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 {
        return None;
    }
    // Zeller's congruence → 0=Sat..6=Fri; remap to 0=Mon..6=Sun.
    let (mm, yy) = if m < 3 { (m + 12, y - 1) } else { (m, y) };
    let k = yy % 100;
    let j = yy / 100;
    let zeller = (d + (13 * (mm + 1)) / 5 + k + k / 4 + j / 4 + 5 * j).rem_euclid(7);
    // Zeller: 0=Sat,1=Sun,2=Mon,...,6=Fri → Mon=0..Sun=6.
    let mon0 = ((zeller + 5) % 7) as usize;
    Some((mon0, h))
}

/// Day ordinal (days since a fixed epoch) for streak/active-day counting.
/// Pure; only needs YYYY-MM-DD from the iso string.
fn day_ordinal(iso: &str) -> Option<i64> {
    let y: i64 = iso.get(0..4)?.parse().ok()?;
    let m: i64 = iso.get(5..7)?.parse().ok()?;
    let d: i64 = iso.get(8..10)?.parse().ok()?;
    // Days from 0000-03-01 (Howard Hinnant's civil algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146097 + doe - 719468)
}

/// Pure: `git log --numstat` output (control-char separated) → commits,
/// newest first (git's order).
pub fn parse_commits(raw: &str) -> Vec<Commit> {
    let mut out = Vec::new();
    for rec in raw.split(REC) {
        let rec = rec.trim_matches('\n');
        if rec.is_empty() {
            continue;
        }
        let mut lines = rec.split('\n');
        let header = lines.next().unwrap_or("");
        let mut f = header.split(FLD);
        let _sha = f.next().unwrap_or("");
        let iso = f.next().unwrap_or("");
        let name = f.next().unwrap_or("");
        let email = f.next().unwrap_or("");
        let subject = f.next().unwrap_or("");
        let committed = f.next().and_then(|t| t.trim().parse::<i64>().ok());
        if iso.is_empty() {
            continue;
        }
        let mut files = Vec::new();
        for ln in lines {
            let ln = ln.trim();
            if ln.is_empty() {
                continue;
            }
            let mut cols = ln.splitn(3, '\t');
            let ins = cols.next().unwrap_or("0").parse().unwrap_or(0);
            let del = cols.next().unwrap_or("0").parse().unwrap_or(0);
            if let Some(path) = cols.next() {
                files.push((path.to_string(), ins, del));
            }
        }
        out.push(Commit {
            iso: iso.to_string(),
            day: day_ordinal(iso),
            author_key: if email.is_empty() { name.to_lowercase() } else { email.to_lowercase() },
            author_name: name.to_string(),
            subject: subject.to_string(),
            committed,
            files,
        });
    }
    out
}

/// Pure: the pre-v0.183 single-shot entry point; only the tests still use it
/// (they pin that the commit model reproduces the old numbers exactly).
#[cfg(test)]
pub fn parse_git_log(raw: &str) -> RepoStats {
    aggregate(&parse_commits(raw))
}

/// Fewest authors whose commits together reach ≥ 50 % of the total.
pub fn bus_factor(counts: &[u64]) -> u32 {
    let total: u64 = counts.iter().sum();
    if total == 0 {
        return 0;
    }
    let mut v: Vec<u64> = counts.to_vec();
    v.sort_unstable_by(|a, b| b.cmp(a));
    let (mut acc, mut n) = (0u64, 0u32);
    for c in v {
        acc += c;
        n += 1;
        if acc * 2 >= total {
            break;
        }
    }
    n
}

const CO_CHANGE_MAX_FILES: usize = 30;

fn top_dir(path: &str) -> String {
    match path.split_once('/') {
        Some((d, _)) if !d.is_empty() => d.to_string(),
        _ => "(root)".to_string(),
    }
}

/// Pure: aggregate any subset of commits (newest first) into stats.
pub fn aggregate<'a>(commits: impl IntoIterator<Item = &'a Commit>) -> RepoStats {
    use std::collections::{BTreeSet, HashMap};
    let commits: Vec<&Commit> = commits.into_iter().collect();
    let mut stats = RepoStats::default();
    let mut author_idx: BTreeMap<String, usize> = BTreeMap::new();
    let mut authors: Vec<(String, u64, u64)> = Vec::new();
    // Borrowed keys (&'a str from the commits) — no per-file String clones;
    // `analyze_ranges` runs this five times, so allocation dominates.
    let mut files: BTreeMap<&'a str, (u64, u64)> = BTreeMap::new();
    let mut file_authors: BTreeMap<&'a str, BTreeSet<usize>> = BTreeMap::new();
    let mut exts: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut months: BTreeMap<String, u64> = BTreeMap::new();
    let mut cats: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut dirs: BTreeMap<String, BTreeMap<usize, u64>> = BTreeMap::new();
    let mut pairs: HashMap<(&'a str, &'a str), u64> = HashMap::new();
    let mut day_counts: BTreeMap<i64, (String, u64)> = BTreeMap::new();
    let mut days: Vec<i64> = Vec::new();
    let mut msg_len_total: u64 = 0;

    for c in &commits {
        stats.commits += 1;
        msg_len_total += c.subject.chars().count() as u64;
        *cats.entry(classify_commit(&c.subject)).or_insert(0) += 1;
        if let Some((wd, hr)) = weekday_hour(&c.iso) {
            stats.by_weekday[wd] += 1;
            stats.by_hour[hr] += 1;
            stats.heatmap[wd][hr] += 1;
        }
        if let Some(ord) = c.day {
            days.push(ord);
            let e = day_counts.entry(ord).or_insert_with(|| (c.iso.get(0..10).unwrap_or("").to_string(), 0));
            e.1 += 1;
        }
        if c.iso.len() >= 7 {
            *months.entry(c.iso[0..7].to_string()).or_insert(0) += 1;
        }
        let a = *author_idx.entry(c.author_key.clone()).or_insert_with(|| {
            authors.push((c.author_name.clone(), 0, 0));
            authors.len() - 1
        });
        authors[a].1 += 1;

        let mut touched_exts: BTreeSet<String> = BTreeSet::new();
        let mut touched_dirs: BTreeSet<String> = BTreeSet::new();
        for (path, ins, del) in &c.files {
            let churn = ins + del;
            stats.insertions += ins;
            stats.deletions += del;
            authors[a].2 += churn;
            let fe = files.entry(path.as_str()).or_insert((0, 0));
            fe.0 += 1;
            fe.1 += churn;
            file_authors.entry(path.as_str()).or_default().insert(a);
            let ext = extension_of(path);
            exts.entry(ext.clone()).or_insert((0, 0)).1 += churn;
            touched_exts.insert(ext);
            touched_dirs.insert(top_dir(path));
        }
        for e in touched_exts {
            exts.entry(e).and_modify(|v| v.0 += 1);
        }
        for d in touched_dirs {
            *dirs.entry(d).or_default().entry(a).or_insert(0) += 1;
        }
        if (2..=CO_CHANGE_MAX_FILES).contains(&c.files.len()) {
            let mut ps: Vec<&'a str> = c.files.iter().map(|(p, _, _)| p.as_str()).collect();
            ps.sort_unstable();
            ps.dedup();
            for i in 0..ps.len() {
                for j in i + 1..ps.len() {
                    *pairs.entry((ps[i], ps[j])).or_insert(0) += 1;
                }
            }
        }
    }

    // git log is newest-first.
    stats.last_commit = commits.first().map(|c| c.iso.clone()).unwrap_or_default();
    stats.first_commit = commits.last().map(|c| c.iso.clone()).unwrap_or_default();
    stats.contributors = authors.len() as u64;
    stats.avg_msg_len = msg_len_total.checked_div(stats.commits).unwrap_or(0);

    days.sort_unstable();
    days.dedup();
    stats.active_days = days.len() as u64;
    stats.longest_streak = longest_streak(&days);

    if let Some(&anchor) = days.last() {
        stats.calendar = day_counts
            .into_iter()
            .filter(|(ord, _)| *ord > anchor - 365)
            .map(|(_, (date, commits))| DayCount { date, commits })
            .collect();
    }

    stats.by_month = months.into_iter().map(|(month, commits)| MonthCount { month, commits }).collect();

    let mut hs: Vec<Hotspot> = files
        .iter()
        .filter_map(|(path, (changes, _))| {
            let n = file_authors.get(path).map_or(0, |s| s.len() as u64);
            (*changes >= 3 && n <= 2).then(|| Hotspot { path: (*path).to_string(), changes: *changes, authors: n })
        })
        .collect();
    hs.sort_by(|a, b| b.changes.cmp(&a.changes).then(a.path.cmp(&b.path)));
    hs.truncate(10);
    stats.hotspots = hs;

    let mut fv: Vec<FileStat> = files
        .into_iter()
        .map(|(path, (changes, churn))| FileStat { path: path.to_string(), changes, churn })
        .collect();
    fv.sort_by(|a, b| b.changes.cmp(&a.changes).then(b.churn.cmp(&a.churn)).then(a.path.cmp(&b.path)));
    fv.truncate(15);
    stats.top_files = fv;

    let mut ev: Vec<ExtStat> = exts
        .into_iter()
        .map(|(ext, (commits, churn))| ExtStat { ext, commits, churn })
        .collect();
    ev.sort_by(|a, b| b.churn.cmp(&a.churn).then(a.ext.cmp(&b.ext)));
    ev.truncate(12);
    stats.top_exts = ev;

    stats.bus_factor = bus_factor(&authors.iter().map(|a| a.1).collect::<Vec<_>>());

    let mut dv: Vec<DirStat> = dirs
        .into_iter()
        .map(|(dir, by_author)| {
            let counts: Vec<u64> = by_author.values().copied().collect();
            DirStat {
                dir,
                commits: counts.iter().sum(),
                authors: counts.len() as u64,
                bus_factor: bus_factor(&counts),
            }
        })
        .collect();
    dv.sort_by(|a, b| b.commits.cmp(&a.commits).then(a.dir.cmp(&b.dir)));
    dv.truncate(10);
    stats.dir_bus_factor = dv;

    let mut cv: Vec<CoChange> = pairs
        .into_iter()
        .filter(|(_, n)| *n >= 2)
        .map(|((a, b), count)| CoChange { a: a.to_string(), b: b.to_string(), count })
        .collect();
    cv.sort_by(|x, y| y.count.cmp(&x.count).then(x.a.cmp(&y.a)).then(x.b.cmp(&y.b)));
    cv.truncate(10);
    stats.co_change = cv;

    let mut av: Vec<AuthorStat> = authors
        .into_iter()
        .map(|(name, commits, churn)| AuthorStat { name, commits, churn })
        .collect();
    av.sort_by(|a, b| b.commits.cmp(&a.commits).then(b.churn.cmp(&a.churn)).then(a.name.cmp(&b.name)));
    av.truncate(12);
    stats.top_authors = av;

    const ORDER: [&str; 12] = [
        "feat", "fix", "refactor", "perf", "docs", "test", "build", "ci", "chore", "style",
        "revert", "other",
    ];
    stats.categories = ORDER
        .iter()
        .filter_map(|c| cats.get(c).map(|&n| CatCount { cat: (*c).to_string(), commits: n }))
        .collect();

    stats
}

/// Stats for every range, each anchored at the NEWEST commit's day (a
/// dormant repo must not read "no commits in the last 30 days" for its whole
/// recent history).
pub fn analyze_ranges(commits: &[Commit]) -> Vec<RangedStats> {
    let anchor = commits.iter().filter_map(|c| c.day).max();
    RangeKey::ALL
        .iter()
        .map(|&range| {
            let sel: Vec<&Commit> = match (range.days(), anchor) {
                (Some(d), Some(a)) => commits.iter().filter(|c| c.day.is_some_and(|x| x > a - d)).collect(),
                _ => commits.iter().collect(),
            };
            let mut stats = aggregate(sel.iter().copied());
            if let Some(a) = anchor {
                let (granularity, timeline) = build_timeline(&sel, range, a);
                stats.granularity = granularity.to_string();
                stats.timeline = timeline;
            }
            RangedStats { range, stats }
        })
        .collect()
}

/// Inverse of `day_ordinal` (Howard Hinnant's civil_from_days) → `YYYY-MM-DD`.
fn date_from_ordinal(z: i64) -> String {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Ordinal of the first day of `o`'s calendar month.
fn month_start(o: i64) -> i64 {
    let date = date_from_ordinal(o);
    day_ordinal(&format!("{}-01", &date[..7])).unwrap_or(o)
}

/// Gapless buckets over the range window (`anchor` = newest commit's day):
/// days for 30 T, Monday-start weeks for 90/180 T, months for 1 J / Gesamt.
fn build_timeline(commits: &[&Commit], range: RangeKey, anchor: i64) -> (&'static str, Vec<Bucket>) {
    let granularity = match range {
        RangeKey::D30 => "day",
        RangeKey::D90 | RangeKey::D180 => "week",
        RangeKey::Y1 | RangeKey::All => "month",
    };
    let first = commits.iter().filter_map(|c| c.day).min().unwrap_or(anchor);
    let lo = range.days().map_or(first, |d| anchor - d + 1);
    let key = |o: i64| match granularity {
        "day" => o,
        "week" => o - (o + 3).rem_euclid(7), // 1970-01-01 was a Thursday
        _ => month_start(o),
    };
    let next = |k: i64| match granularity {
        "day" => k + 1,
        "week" => k + 7,
        _ => month_start(k + 31),
    };
    let mut starts = Vec::new();
    let (mut k, last) = (key(lo), key(anchor));
    while k <= last {
        starts.push(k);
        k = next(k);
    }
    let mut buckets: Vec<Bucket> = starts
        .iter()
        .map(|&o| Bucket { start: date_from_ordinal(o), commits: 0, insertions: 0, deletions: 0 })
        .collect();
    for c in commits {
        let Some(day) = c.day else { continue };
        let Ok(i) = starts.binary_search(&key(day)) else { continue };
        let b = &mut buckets[i];
        b.commits += 1;
        for (_, ins, del) in &c.files {
            b.insertions += ins;
            b.deletions += del;
        }
    }
    (granularity, buckets)
}

/// Longest run of consecutive day-ordinals in a sorted, deduped slice.
pub fn longest_streak(days: &[i64]) -> u64 {
    if days.is_empty() {
        return 0;
    }
    let (mut best, mut cur) = (1u64, 1u64);
    for w in days.windows(2) {
        if w[1] == w[0] + 1 {
            cur += 1;
            best = best.max(cur);
        } else {
            cur = 1;
        }
    }
    best
}

/// Derive a display name + export slug from a source (URL or path).
/// `github.com/pepperonas/inspector-rust(.git)` → ("inspector-rust",
/// "pepperonas-inspector-rust"); a local path → the folder name for both.
pub fn repo_identity(source: &str) -> (String, String) {
    let trimmed = source.trim_end_matches('/');
    if trimmed.contains("://") || trimmed.contains('@') && trimmed.contains(':') {
        // Looks like a URL/scp-style remote. Take the last two path segments.
        let cleaned = trimmed.trim_end_matches(".git");
        let segs: Vec<&str> = cleaned.rsplit(['/', ':']).filter(|s| !s.is_empty()).collect();
        let repo = segs.first().copied().unwrap_or("repo");
        let owner = segs.get(1).copied().unwrap_or("");
        let slug = if owner.is_empty() {
            sanitize_slug(repo)
        } else {
            format!("{}-{}", sanitize_slug(owner), sanitize_slug(repo))
        };
        (repo.to_string(), slug)
    } else {
        let name = trimmed.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("repo");
        (name.to_string(), sanitize_slug(name))
    }
}

fn sanitize_slug(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Run `git log` with the repo2viz format in `dir` (`rev` e.g. `origin/HEAD`
/// for the no-checkout cache, whose local branch is stale after a fetch).
fn git_log(dir: &std::path::Path, rev: Option<&str>, env: &[(String, String)]) -> Result<String, String> {
    let fmt = format!("{REC}%H{FLD}%aI{FLD}%aN{FLD}%aE{FLD}%s{FLD}%ct");
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(dir) // not `-C <dir>` — keeps an untrusted path out of argv
        .args(["log", "--no-merges", "--numstat", "--date=iso-strict"])
        .arg(format!("--pretty=format:{fmt}"));
    if let Some(r) = rev {
        cmd.arg(r);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().map_err(|e| format!("git nicht gefunden: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        let missing_ref = err.contains("does not have any commits")
            || err.contains("unknown revision")
            || err.contains("ambiguous argument");
        if missing_ref {
            // Truly empty (no remote refs at all) → "no commits". A FILLED
            // cache whose origin/HEAD went missing (default branch renamed,
            // set-head failed) must not read as empty: repair once, re-read.
            if let Some(r) = rev.filter(|r| r.starts_with("origin/")) {
                if has_remote_refs(dir) {
                    let _ = std::process::Command::new("git")
                        .current_dir(dir)
                        .args(["remote", "set-head", "origin", "--auto"])
                        .env("GIT_TERMINAL_PROMPT", "0")
                        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
                        .output();
                    let mut retry = std::process::Command::new("git");
                    retry
                        .current_dir(dir)
                        .args(["log", "--no-merges", "--numstat", "--date=iso-strict"])
                        .arg(format!("--pretty=format:{fmt}"))
                        .arg(r);
                    let again = retry.output().map_err(|e| format!("git nicht gefunden: {e}"))?;
                    if again.status.success() {
                        return Ok(String::from_utf8_lossy(&again.stdout).into_owned());
                    }
                    return Err(format!(
                        "repo.git: {r} fehlt im Cache — {}",
                        String::from_utf8_lossy(&again.stderr).trim()
                    ));
                }
            }
            return Ok(String::new());
        }
        return Err(format!("git log fehlgeschlagen: {}", err.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn has_remote_refs(dir: &std::path::Path) -> bool {
    std::process::Command::new("git")
        .current_dir(dir)
        .args(["for-each-ref", "--count=1", "refs/remotes/origin/"])
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Creation time (Unix s) of every tag: annotated → tag date, lightweight →
/// the tagged commit's date. Empty on any error (a repo without tags is normal).
pub fn tag_times_from_dir(dir: &std::path::Path) -> Vec<i64> {
    std::process::Command::new("git")
        .current_dir(dir)
        .args(["for-each-ref", "refs/tags", "--format=%(creatordate:unix)"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.trim().parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

pub fn commits_from_dir(
    dir: &std::path::Path,
    rev: Option<&str>,
    env: &[(String, String)],
) -> Result<Vec<Commit>, String> {
    Ok(parse_commits(&git_log(dir, rev, env)?))
}

#[derive(Serialize, Clone, Debug)]
pub struct RepoAnalysis {
    pub name: String,
    pub source: String,
    /// Set for GitHub repos — the panel offers "Klonen" only then.
    pub github: Option<crate::repo_url::RepoUrl>,
    pub ranges: Vec<RangedStats>,
    /// Rolling 24 h / 7 d windows from now (committer time), v0.185.0.
    pub recent: crate::repo_activity::RecentActivity,
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn finish(
    name: String,
    source: String,
    github: Option<crate::repo_url::RepoUrl>,
    commits: &[Commit],
    tag_times: &[i64],
) -> RepoAnalysis {
    let mut ranges = analyze_ranges(commits);
    for r in &mut ranges {
        r.stats.name = name.clone();
        r.stats.source = source.clone();
    }
    let recent = crate::repo_activity::recent_activity(commits, tag_times, now_unix());
    RepoAnalysis { name, source, github, ranges, recent }
}

pub fn analyze_local(dir: &std::path::Path) -> Result<RepoAnalysis, String> {
    if !dir.join(".git").exists() {
        return Err("Kein Git-Repository (kein .git gefunden).".into());
    }
    let commits = commits_from_dir(dir, None, &[])?;
    let (name, _slug) = repo_identity(&dir.to_string_lossy());
    Ok(finish(name, dir.to_string_lossy().into_owned(), None, &commits, &tag_times_from_dir(dir)))
}

/// GitHub: via the clone cache (fetch on repeat). Other hosts: bare temp
/// clone, analysed, deleted (the pre-v0.183 path).
pub fn analyze_remote(
    url: &str,
    on_progress: &mut dyn FnMut(&str, u8),
) -> Result<RepoAnalysis, String> {
    if url.starts_with('-') || !(url.contains("://") || (url.contains('@') && url.contains(':'))) {
        return Err("Keine gültige Repository-URL.".into());
    }
    if let Some(gh) = crate::repo_url::parse_repo_url(url) {
        let token = crate::repo_clone::github_token();
        let (dir, commits) = crate::repo_clone::cached_commits(&gh, token.as_deref(), on_progress)?;
        crate::repo_clone::prune_cache(&dir);
        let tags = tag_times_from_dir(&dir);
        return Ok(finish(gh.repo.clone(), gh.web_url.clone(), Some(gh), &commits, &tags));
    }
    let tmp = std::env::temp_dir().join(format!(
        "ir-repo-{}-{}",
        std::process::id(),
        sanitize_slug(url).chars().take(24).collect::<String>()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    let clone = Command::new("git")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["clone", "--bare", "--quiet", "--", url, &tmp.to_string_lossy()])
        .output()
        .map_err(|e| format!("git nicht gefunden: {e}"))?;
    if !clone.status.success() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("Klonen fehlgeschlagen: {}", String::from_utf8_lossy(&clone.stderr).trim()));
    }
    let result = commits_from_dir(&tmp, None, &[]);
    let tags = tag_times_from_dir(&tmp);
    let _ = std::fs::remove_dir_all(&tmp);
    let (name, _slug) = repo_identity(url);
    Ok(finish(name, url.to_string(), None, &result?, &tags))
}

// ── HTML export (self-contained, repo2viz-oriented) ─────────────────────────

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn bar_rows(items: &[(String, u64)], max: u64, color: &str) -> String {
    let mut out = String::new();
    for (label, val) in items {
        let pct = if max > 0 { (*val as f64 / max as f64 * 100.0).round() } else { 0.0 };
        out.push_str(&format!(
            "<div class=\"row\"><span class=\"lbl\">{}</span><span class=\"bar\"><i style=\"width:{}%;background:{}\"></i></span><span class=\"val\">{}</span></div>",
            esc(label), pct, color, val
        ));
    }
    out
}

/// Short label for a timeline bucket: `25.09.` (day), `ab 21.09.` (week),
/// `2026-09` (month).
fn bucket_label(start: &str, granularity: &str) -> String {
    match granularity {
        "day" if start.len() >= 10 => format!("{}.{}.", &start[8..10], &start[5..7]),
        "week" if start.len() >= 10 => format!("ab {}.{}.", &start[8..10], &start[5..7]),
        _ => start.get(..7).unwrap_or(start).to_string(),
    }
}

/// Lines added (green, up) / removed (red, down) per bucket plus the
/// cumulative net line — inline SVG so it survives the PDF render.
pub fn churn_svg(timeline: &[Bucket]) -> String {
    if timeline.is_empty() {
        return String::new();
    }
    let (w, h, pad) = (600.0_f64, 150.0_f64, 4.0_f64);
    let max_add = timeline.iter().map(|b| b.insertions).max().unwrap_or(0) as f64;
    let max_del = timeline.iter().map(|b| b.deletions).max().unwrap_or(0) as f64;
    // ONE scale for both directions (honest proportions); the zero line sits
    // where that scale puts it, so a mostly-additive history doesn't waste
    // the lower half of the chart. Each side keeps at least 12 % when it has data.
    let span = h - 2.0 * pad;
    let total = (max_add + max_del).max(1.0);
    let mut up = span * max_add / total;
    if max_del > 0.0 { up = up.min(span * 0.88); }
    if max_add > 0.0 { up = up.max(span * 0.12); }
    let y0 = pad + up;
    let k_add = if max_add > 0.0 { up / max_add } else { 0.0 };
    let k_del = if max_del > 0.0 { (span - up) / max_del } else { 0.0 };
    let k = if k_add > 0.0 && k_del > 0.0 { k_add.min(k_del) } else { k_add.max(k_del) };
    let bw = w / timeline.len() as f64;
    let mut s = format!("<svg class=\"churn\" viewBox=\"0 0 {w} {h}\" width=\"100%\" role=\"img\" aria-label=\"Zeilen hinzugefügt und gelöscht\">");
    s.push_str(&format!("<line x1=\"0\" y1=\"{y0:.1}\" x2=\"{w}\" y2=\"{y0:.1}\" stroke=\"#c9ced6\" stroke-width=\"0.6\"/>"));
    // Cumulative net line, scaled into the same band around y0.
    let mut net = 0i64;
    let (mut hi, mut lo) = (0i64, 0i64);
    for b in timeline {
        net += b.insertions as i64 - b.deletions as i64;
        hi = hi.max(net);
        lo = lo.min(net);
    }
    let kn_up = if hi > 0 { (y0 - pad) / hi as f64 } else { f64::INFINITY };
    let kn_dn = if lo < 0 { (h - pad - y0) / (-lo) as f64 } else { f64::INFINITY };
    let kn = kn_up.min(kn_dn);
    let kn = if kn.is_finite() { kn } else { 0.0 };
    net = 0;
    let mut pts = Vec::with_capacity(timeline.len());
    for (i, b) in timeline.iter().enumerate() {
        let x = i as f64 * bw + bw * 0.15;
        let bwi = (bw * 0.7).max(0.5);
        let ha = b.insertions as f64 * k;
        let hd = b.deletions as f64 * k;
        if b.insertions > 0 {
            s.push_str(&format!("<rect x=\"{x:.1}\" y=\"{:.1}\" width=\"{bwi:.1}\" height=\"{ha:.1}\" fill=\"#2e9e5b\"><title>{}: +{}</title></rect>", y0 - ha, esc(&b.start), b.insertions));
        }
        if b.deletions > 0 {
            s.push_str(&format!("<rect x=\"{x:.1}\" y=\"{y0:.1}\" width=\"{bwi:.1}\" height=\"{hd:.1}\" fill=\"#d0493f\"><title>{}: −{}</title></rect>", esc(&b.start), b.deletions));
        }
        net += b.insertions as i64 - b.deletions as i64;
        pts.push(format!("{:.1},{:.1}", i as f64 * bw + bw / 2.0, y0 - net as f64 * kn));
    }
    s.push_str(&format!("<polyline points=\"{}\" fill=\"none\" stroke=\"#3f6cd4\" stroke-width=\"1.4\"/>", pts.join(" ")));
    s.push_str("</svg>");
    s
}

/// Weekday×hour heatmap as inline SVG (survives the PDF render).
pub fn heatmap_svg(h: &[[u64; 24]; 7]) -> String {
    let max = h.iter().flatten().copied().max().unwrap_or(0).max(1) as f64;
    let days = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"];
    let (cw, ch, lx) = (14.0, 14.0, 22.0);
    let mut s = format!(
        "<svg class=\"heat\" viewBox=\"0 0 {} {}\" width=\"100%\" role=\"img\" aria-label=\"Heatmap Wochentag × Stunde\">",
        lx + 24.0 * cw,
        7.0 * ch + 12.0
    );
    for (d, row) in h.iter().enumerate() {
        s.push_str(&format!("<text x=\"0\" y=\"{}\" font-size=\"8\" fill=\"#6b7280\">{}</text>", d as f64 * ch + 10.0, days[d]));
        for (hr, &v) in row.iter().enumerate() {
            let op = if v == 0 { 0.06 } else { 0.18 + 0.82 * (v as f64 / max) };
            s.push_str(&format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"2\" fill=\"#3f6cd4\" fill-opacity=\"{op:.2}\"><title>{} {hr:02} Uhr: {v}</title></rect>",
                lx + hr as f64 * cw,
                d as f64 * ch,
                cw - 2.0,
                ch - 2.0,
                days[d]
            ));
        }
    }
    for hr in (0..24).step_by(6) {
        s.push_str(&format!("<text x=\"{}\" y=\"{}\" font-size=\"8\" fill=\"#6b7280\">{hr}</text>", lx + hr as f64 * cw, 7.0 * ch + 10.0));
    }
    s.push_str("</svg>");
    s
}

/// GitHub-style contribution calendar (weeks as columns, Mon..Sun rows).
pub fn calendar_svg(days: &[DayCount]) -> String {
    let parsed: Vec<(i64, &DayCount)> = days.iter().filter_map(|d| day_ordinal(&d.date).map(|o| (o, d))).collect();
    let Some(last) = parsed.iter().map(|(o, _)| *o).max() else { return String::new() };
    let first = parsed.iter().map(|(o, _)| *o).min().unwrap_or(last);
    // 1970-01-01 (ordinal 0) was a Thursday → Mon=0 weekday = (ord+3) mod 7.
    let wd = |o: i64| (o + 3).rem_euclid(7);
    let start = first - wd(first); // Monday of the first week
    let weeks = ((last - start) / 7 + 1) as f64;
    let max = parsed.iter().map(|(_, d)| d.commits).max().unwrap_or(1).max(1) as f64;
    let c = 10.0;
    let mut s = format!("<svg class=\"cal\" viewBox=\"0 0 {} {}\" width=\"100%\" role=\"img\" aria-label=\"Beitragskalender\">", weeks * c, 7.0 * c);
    for (o, d) in &parsed {
        let col = ((o - start) / 7) as f64;
        let row = wd(*o) as f64;
        let op = 0.2 + 0.8 * (d.commits as f64 / max);
        s.push_str(&format!(
            "<rect x=\"{:.0}\" y=\"{:.0}\" width=\"8\" height=\"8\" rx=\"1.5\" fill=\"#2e9e5b\" fill-opacity=\"{op:.2}\"><title>{}: {}</title></rect>",
            col * c,
            row * c,
            esc(&d.date),
            d.commits
        ));
    }
    s.push_str("</svg>");
    s
}

/// Build the self-contained HTML export for one range (no external requests —
/// inline CSS + inline SVG, both survive the PDF render). Pure; tested structurally.
/// Export without the activity table — only the tests still call it.
#[cfg(test)]
pub fn build_html(stats: &RepoStats, range: RangeKey) -> String {
    build_html_with(stats, range, None, None)
}

fn delta(cur: u64, prev: u64) -> String {
    match cur.cmp(&prev) {
        std::cmp::Ordering::Greater => format!("↑ {}", cur - prev),
        std::cmp::Ordering::Less => format!("↓ {}", prev - cur),
        std::cmp::Ordering::Equal => "±0".into(),
    }
}

fn activity_row(label: &str, d: u64, dp: u64, w: u64, wp: u64, capped: bool) -> String {
    let ge = if capped { "≥ " } else { "" };
    format!(
        "<tr><td>{label}</td><td>{ge}{d} <span class=\"dl\">{}</span></td><td>{ge}{w} <span class=\"dl\">{}</span></td></tr>",
        delta(d, dp),
        delta(w, wp)
    )
}

fn activity_section(r: &crate::repo_activity::RecentActivity, gh: Option<&crate::github_api::GithubActivity>) -> String {
    let mut rows = String::new();
    let g = |f: fn(&crate::repo_activity::ActivityCounts) -> u64| (f(&r.day), f(&r.day_prev), f(&r.week), f(&r.week_prev));
    for (label, (d, dp, w, wp)) in [
        ("Commits", g(|c| c.commits)),
        ("Zeilen +", g(|c| c.insertions)),
        ("Zeilen −", g(|c| c.deletions)),
        ("Dateien", g(|c| c.files)),
        ("Mitwirkende", g(|c| c.authors)),
        ("Tags", g(|c| c.tags)),
    ] {
        rows.push_str(&activity_row(label, d, dp, w, wp, false));
    }
    if let Some(gh) = gh {
        let h = |f: fn(&crate::github_api::GithubCounts) -> u64| (f(&gh.day), f(&gh.day_prev), f(&gh.week), f(&gh.week_prev));
        let (d, dp, w, wp) = h(|c| c.pushes);
        rows.push_str(&activity_row("Pushes", d, dp, w, wp, gh.pushes_capped));
        for (label, (d, dp, w, wp)) in [
            ("PRs geöffnet", h(|c| c.prs_opened)),
            ("PRs gemergt", h(|c| c.prs_merged)),
            ("Issues geöffnet", h(|c| c.issues_opened)),
            ("Issues geschlossen", h(|c| c.issues_closed)),
        ] {
            rows.push_str(&activity_row(label, d, dp, w, wp, false));
        }
    }
    format!(
        "<section><h2>Aktivität 24 h / 7 Tage</h2><table class=\"act\"><thead><tr><th>Wert</th><th>24 h</th><th>7 Tage</th></tr></thead><tbody>{rows}</tbody></table><p class=\"rp-lede\">Vergleich jeweils zur gleich langen Vorperiode. Git: Haupt-Branch ohne Merges · Pushes: alle Branches.</p></section>"
    )
}

/// Export with the optional 24 h / 7 d activity table at the top.
pub fn build_html_with(
    stats: &RepoStats,
    range: RangeKey,
    recent: Option<&crate::repo_activity::RecentActivity>,
    github: Option<&crate::github_api::GithubActivity>,
) -> String {
    if stats.commits == 0 {
        return rs::shell(
            "Repo-Aktivität",
            &esc(&stats.name),
            &format!("{} · {}", esc(&stats.source), range.label()),
            "<p class=\"rp-lede\">Keine Commits in diesem Zeitraum.</p>",
            "Erstellt mit Inspector Rust, orientiert an repo2viz.",
        );
    }
    let weekdays = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"];
    let wd_max = stats.by_weekday.iter().copied().max().unwrap_or(1).max(1);
    let wd_rows = bar_rows(
        &weekdays.iter().enumerate().map(|(i, d)| ((*d).to_string(), stats.by_weekday[i])).collect::<Vec<_>>(),
        wd_max,
        "#8ab4f8",
    );
    let hr_max = stats.by_hour.iter().copied().max().unwrap_or(1).max(1);
    let hr_rows = bar_rows(
        &(0..24).map(|h| (format!("{h:02}"), stats.by_hour[h])).collect::<Vec<_>>(),
        hr_max,
        "#c58af9",
    );
    let mo_max = stats.timeline.iter().map(|b| b.commits).max().unwrap_or(1).max(1);
    let mo_rows = bar_rows(
        &stats.timeline.iter().map(|b| (bucket_label(&b.start, &stats.granularity), b.commits)).collect::<Vec<_>>(),
        mo_max,
        "#81c995",
    );
    let activity_title = match stats.granularity.as_str() {
        "day" => "Aktivität pro Tag",
        "week" => "Aktivität pro Woche",
        _ => "Aktivität pro Monat",
    };
    let net = stats.insertions as i64 - stats.deletions as i64;
    let code_changes = format!(
        "<section><h2>Code-Änderungen</h2><div class=\"churn-kpis\"><span class=\"add\">+{}</span><span class=\"del\">−{}</span><span>{}{} netto</span></div>{}</section>",
        stats.insertions,
        stats.deletions,
        if net >= 0 { "+" } else { "−" },
        net.unsigned_abs(),
        churn_svg(&stats.timeline),
    );
    let cat_max = stats.categories.iter().map(|c| c.commits).max().unwrap_or(1).max(1);
    let cat_rows = bar_rows(
        &stats.categories.iter().map(|c| (c.cat.clone(), c.commits)).collect::<Vec<_>>(),
        cat_max,
        "#fcc934",
    );
    let file_rows: String = stats
        .top_files
        .iter()
        .map(|f| format!("<tr><td class=\"mono\">{}</td><td>{}</td><td>{}</td></tr>", esc(&f.path), f.changes, f.churn))
        .collect();
    let author_rows: String = stats
        .top_authors
        .iter()
        .map(|a| format!("<tr><td>{}</td><td>{}</td><td>{}</td></tr>", esc(&a.name), a.commits, a.churn))
        .collect();
    let ext_rows: String = stats
        .top_exts
        .iter()
        .map(|e| format!("<tr><td class=\"mono\">.{}</td><td>{}</td><td>{}</td></tr>", esc(&e.ext), e.commits, e.churn))
        .collect();
    let hot_rows: String = stats
        .hotspots
        .iter()
        .map(|h| format!("<tr><td class=\"mono\">{}</td><td>{}</td><td>{}</td></tr>", esc(&h.path), h.changes, h.authors))
        .collect();
    let dir_rows: String = stats
        .dir_bus_factor
        .iter()
        .map(|d| format!("<tr><td class=\"mono\">{}</td><td>{}</td><td>{}</td><td>{}</td></tr>", esc(&d.dir), d.commits, d.authors, d.bus_factor))
        .collect();
    let co_rows: String = stats
        .co_change
        .iter()
        .map(|p| format!("<tr><td class=\"mono\">{} ↔ {}</td><td>{}</td></tr>", esc(&p.a), esc(&p.b), p.count))
        .collect();
    let calendar = if matches!(range, RangeKey::Y1 | RangeKey::All) && !stats.calendar.is_empty() {
        format!("<section><h2>Beitragskalender</h2>{}</section>", calendar_svg(&stats.calendar))
    } else {
        String::new()
    };

    let body = format!(
        r#"{stats}
{recent}
{code_changes}
<section><h2>Commits nach Wochentag</h2>{wd}</section>
<section><h2>Commits nach Stunde</h2>{hr}</section>
<section><h2>Heatmap Wochentag × Stunde</h2>{heat}</section>
{calendar}
<section><h2>{activity_title}</h2>{mo}</section>
<section><h2>Commit-Kategorien</h2>{cat}</section>
<section><h2>Aktivste Dateien</h2><table>
<thead><tr><th>Datei</th><th>Änderungen</th><th>Churn</th></tr></thead><tbody>{files}</tbody></table></section>
<section><h2>Dateitypen</h2><table>
<thead><tr><th>Typ</th><th>Commits</th><th>Churn</th></tr></thead><tbody>{exts}</tbody></table></section>
<section><h2>Top-Mitwirkende</h2><table>
<thead><tr><th>Name</th><th>Commits</th><th>Churn</th></tr></thead><tbody>{authors}</tbody></table></section>
<section><h2>Hotspots (viel geändert, ≤ 2 Autoren)</h2><table>
<thead><tr><th>Datei</th><th>Änderungen</th><th>Autoren</th></tr></thead><tbody>{hot}</tbody></table></section>
<section><h2>Bus-Faktor je Verzeichnis</h2><table>
<thead><tr><th>Verzeichnis</th><th>Commits</th><th>Autoren</th><th>Bus-Faktor</th></tr></thead><tbody>{dirs}</tbody></table></section>
<section><h2>Co-Change</h2><table>
<thead><tr><th>Dateipaar</th><th>Gemeinsam</th></tr></thead><tbody>{co}</tbody></table></section>"#,
        stats = rs::stats(&[
            rs::Stat { label: "Commits", value: stats.commits.to_string(), unit: None },
            rs::Stat { label: "Mitwirkende", value: stats.contributors.to_string(), unit: None },
            rs::Stat { label: "Aktive Tage", value: stats.active_days.to_string(), unit: None },
            rs::Stat { label: "Längste Serie", value: stats.longest_streak.to_string(), unit: Some("Tage") },
            rs::Stat { label: "Zeilen ein", value: format!("+{}", stats.insertions), unit: None },
            rs::Stat { label: "Zeilen aus", value: format!("−{}", stats.deletions), unit: None },
            rs::Stat { label: "Bus-Faktor", value: stats.bus_factor.to_string(), unit: None },
        ]),
        wd = wd_rows,
        hr = hr_rows,
        mo = mo_rows,
        cat = cat_rows,
        files = file_rows,
        exts = ext_rows,
        authors = author_rows,
        heat = heatmap_svg(&stats.heatmap),
        code_changes = code_changes,
        recent = recent.map(|r| activity_section(r, github)).unwrap_or_default(),
        activity_title = activity_title,
        calendar = calendar,
        hot = hot_rows,
        dirs = dir_rows,
        co = co_rows,
    );

    let doc = rs::shell(
        "Repo-Aktivität",
        &esc(&stats.name),
        &format!("{} · {} · {} → {}", esc(&stats.source), range.label(), esc(&stats.first_commit), esc(&stats.last_commit)),
        &body,
        "Aus der Git-Historie gerechnet (Merges ausgenommen); <b>Churn</b> = geänderte Zeilen ein + aus.<br>Erstellt mit Inspector Rust, orientiert an repo2viz.",
    );
    doc.replace("</style>", &format!("{}\n</style>", REPO_CSS))
}

/// Bar-row rules on top of the shared stylesheet.
const REPO_CSS: &str = r#"
.row { display:flex; align-items:center; gap:11px; margin:4px 0; font-size:11.5px }
.lbl { width:58px; color:var(--muted); text-align:right; flex:none }
.bar { flex:1; height:8px; background:#f1f3f6; border-radius:4px; overflow:hidden }
.bar i { display:block; height:100%; border-radius:4px }
.val { width:56px; text-align:right; color:var(--muted); flex:none }
.mono { font-family:ui-monospace,SFMono-Regular,Menlo,monospace }
.heat, .cal, .churn { display:block; max-width:100% }
.churn-kpis { display:flex; gap:18px; font-size:15px; font-weight:600; margin:0 0 8px; font-variant-numeric:tabular-nums }
.dl { color:var(--muted); font-size:10.5px; margin-left:4px }
.churn-kpis .add { color:#2e9e5b } .churn-kpis .del { color:#d0493f }
td:nth-child(2), td:nth-child(3), td:nth-child(4), th:nth-child(2), th:nth-child(3), th:nth-child(4) { width:88px }
.act td, .act th { width:auto; white-space:nowrap }
.dl { white-space:nowrap }
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_commit_reads_conventional_prefixes_with_scope() {
        assert_eq!(classify_commit("feat: add x"), "feat");
        assert_eq!(classify_commit("fix(ui): y"), "fix");
        assert_eq!(classify_commit("FEAT: caps"), "feat");
        assert_eq!(classify_commit("refactor!: bang"), "other"); // '!' before ':' → not matched (conservative)
        assert_eq!(classify_commit("just a normal message"), "other");
        assert_eq!(classify_commit("feature: not a keyword"), "other"); // 'feature' != 'feat:'
    }

    #[test]
    fn extension_handles_dotfiles_and_paths() {
        assert_eq!(extension_of("src/main.rs"), "rs");
        assert_eq!(extension_of(".github/workflows/ci.yml"), "yml");
        assert_eq!(extension_of("Makefile"), "—");
        assert_eq!(extension_of("dir.with.dots/file"), "—");
        assert_eq!(extension_of("archive.tar.gz"), "gz");
    }

    #[test]
    fn weekday_hour_matches_known_dates() {
        // 2026-08-24 is a Monday.
        assert_eq!(weekday_hour("2026-08-24T09:15:00+02:00"), Some((0, 9)));
        // 2026-08-23 is a Sunday.
        assert_eq!(weekday_hour("2026-08-23T23:59:00Z"), Some((6, 23)));
        // 2000-01-01 is a Saturday.
        assert_eq!(weekday_hour("2000-01-01T00:00:00Z"), Some((5, 0)));
        assert_eq!(weekday_hour("garbage"), None);
    }

    #[test]
    fn longest_streak_counts_consecutive_days() {
        assert_eq!(longest_streak(&[]), 0);
        assert_eq!(longest_streak(&[5]), 1);
        assert_eq!(longest_streak(&[1, 2, 3, 5, 6]), 3);
        assert_eq!(longest_streak(&[10, 11, 12, 13]), 4);
        assert_eq!(longest_streak(&[1, 3, 5]), 1);
    }

    #[test]
    fn repo_identity_from_urls_and_paths() {
        assert_eq!(
            repo_identity("https://github.com/pepperonas/inspector-rust.git"),
            ("inspector-rust".into(), "pepperonas-inspector-rust".into())
        );
        assert_eq!(
            repo_identity("https://github.com/pepperonas/inspector-rust"),
            ("inspector-rust".into(), "pepperonas-inspector-rust".into())
        );
        assert_eq!(
            repo_identity("git@github.com:owner/repo.git"),
            ("repo".into(), "owner-repo".into())
        );
        assert_eq!(repo_identity("/Users/martin/claude/inspector-rust"), ("inspector-rust".into(), "inspector-rust".into()));
    }

    fn synth_log() -> String {
        // Two commits (newest first, as git log emits), with numstat.
        // Commit 2 (newer): feat, Mon 2026-08-24 09:xx, bob.
        // Commit 1 (older): fix, Sun 2026-08-23 23:xx, alice.
        format!(
            "{REC}sha2{FLD}2026-08-24T09:15:00+02:00{FLD}Bob{FLD}bob@x.io{FLD}feat(ui): add panel\n\
             10\t2\tsrc/ui.rs\n\
             5\t0\tsrc/lib.rs\n\
             {REC}sha1{FLD}2026-08-23T23:40:00+02:00{FLD}Alice{FLD}alice@x.io{FLD}fix: correct bug\n\
             1\t1\tsrc/ui.rs\n",
            REC = REC, FLD = FLD
        )
    }

    #[test]
    fn parse_git_log_computes_the_core_metrics() {
        let s = parse_git_log(&synth_log());
        assert_eq!(s.commits, 2);
        assert_eq!(s.contributors, 2);
        assert_eq!(s.insertions, 16); // 10+5+1
        assert_eq!(s.deletions, 3); //  2+0+1
        // Chronology: first = older (Alice), last = newer (Bob).
        assert!(s.first_commit.starts_with("2026-08-23"));
        assert!(s.last_commit.starts_with("2026-08-24"));
        // Weekday: one Mon (0), one Sun (6).
        assert_eq!(s.by_weekday[0], 1);
        assert_eq!(s.by_weekday[6], 1);
        assert_eq!(s.by_hour[9], 1);
        assert_eq!(s.by_hour[23], 1);
        // src/ui.rs touched twice → the top file.
        assert_eq!(s.top_files[0].path, "src/ui.rs");
        assert_eq!(s.top_files[0].changes, 2);
        assert_eq!(s.top_files[0].churn, 14); // (10+2)+(1+1)
        // Extensions: only "rs".
        assert_eq!(s.top_exts[0].ext, "rs");
        // Categories present: feat + fix.
        assert!(s.categories.iter().any(|c| c.cat == "feat" && c.commits == 1));
        assert!(s.categories.iter().any(|c| c.cat == "fix" && c.commits == 1));
        // Two consecutive days → streak 2, 2 active days.
        assert_eq!(s.active_days, 2);
        assert_eq!(s.longest_streak, 2);
        // Timeline has the month.
        assert_eq!(s.by_month.iter().find(|m| m.month == "2026-08").unwrap().commits, 2);
    }

    #[test]
    fn analyze_remote_rejects_flag_smuggling_and_junk() {
        // argv injection: a URL that is actually a git flag must be refused
        // BEFORE spawning git (the `--` guard is belt-and-braces on top).
        assert!(analyze_remote("--upload-pack=touch /tmp/pwn", &mut |_, _| {}).is_err());
        assert!(analyze_remote("-x", &mut |_, _| {}).is_err());
        // Plain non-URL junk is refused too (no scheme / scp form).
        assert!(analyze_remote("not a url", &mut |_, _| {}).is_err());
    }

    /// Offline sight check — see the timesheet dump for why.
    #[test]
    #[ignore]
    fn dump_for_a_sight_check() {
        let dir = std::path::PathBuf::from(
            std::env::var("IR_DUMP_DIR").unwrap_or_else(|_| "/tmp".into()),
        );
        // IR_DUMP_REPO=<path> renders a real repo (all ranges) instead of the synthetic log.
        if let Ok(repo) = std::env::var("IR_DUMP_REPO") {
            let a = analyze_local(std::path::Path::new(&repo)).unwrap();
            for r in &a.ranges {
                let name = format!("repo-report-{}.html", serde_json::to_string(&r.range).unwrap().trim_matches('"'));
                std::fs::write(dir.join(name), build_html_with(&r.stats, r.range, Some(&a.recent), None)).unwrap();
            }
            return;
        }
        let stats = parse_git_log(&synth_log());
        std::fs::write(dir.join("repo-report.html"), build_html(&stats, RangeKey::All)).unwrap();
    }

    #[test]
    fn build_html_is_self_contained_and_names_the_repo() {
        let mut s = parse_git_log(&synth_log());
        s.name = "inspector-rust".into();
        s.source = "https://github.com/pepperonas/inspector-rust".into();
        let html = build_html(&s, RangeKey::All);
        assert!(html.starts_with("<!doctype html>"), "gemeinsames Dokument-Gerüst");
        assert!(html.contains("inspector-rust"));
        // No external requests — the whole point of the repo2viz-style export.
        assert!(!html.contains("http://"));
        assert!(!html.to_lowercase().contains("<script"));
        assert!(!html.contains("src=\"http"));
        // Charts + tables rendered.
        assert!(html.contains("Commits nach Wochentag"));
        assert!(html.contains("Aktivste Dateien"));
        assert!(html.contains("src/ui.rs"));
    }

    #[test]
    fn html_escapes_injected_names() {
        let mut s = RepoStats { name: "<script>x</script>".into(), commits: 1, ..Default::default() };
        s.top_authors.push(AuthorStat { name: "a<b>&\"".into(), commits: 1, churn: 1 });
        let html = build_html(&s, RangeKey::All);
        assert!(!html.contains("<script>x</script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("a&lt;b&gt;&amp;&quot;"));
    }

    fn rec(iso: &str, name: &str, email: &str, subject: &str, files: &[(&str, u64, u64)]) -> String {
        let mut s = format!("{REC}sha{FLD}{iso}{FLD}{name}{FLD}{email}{FLD}{subject}\n");
        for (p, i, d) in files {
            s.push_str(&format!("{i}\t{d}\t{p}\n"));
        }
        s
    }

    #[test]
    fn parse_git_log_is_unchanged_by_the_commit_model() {
        // The legacy entry point must still produce identical core numbers.
        let s = parse_git_log(&synth_log());
        let c = aggregate(&parse_commits(&synth_log()));
        assert_eq!(s.commits, c.commits);
        assert_eq!(s.top_files, c.top_files);
        assert_eq!(s.by_weekday, c.by_weekday);
    }

    #[test]
    fn ranges_are_anchored_at_the_last_commit_not_today() {
        // Newest-first like git: 2020-06-30, 2020-06-10, 2019-06-30.
        let raw = [
            rec("2020-06-30T10:00:00+00:00", "A", "a@x", "feat: c", &[("a.rs", 1, 0)]),
            rec("2020-06-10T10:00:00+00:00", "A", "a@x", "fix: b", &[("a.rs", 1, 0)]),
            rec("2019-06-30T10:00:00+00:00", "B", "b@x", "chore: a", &[("b.rs", 1, 0)]),
        ]
        .concat();
        let r = analyze_ranges(&parse_commits(&raw));
        let get = |k: RangeKey| r.iter().find(|x| x.range == k).unwrap().stats.commits;
        assert_eq!(get(RangeKey::D30), 2); // 06-10 is 20 days before 06-30
        assert_eq!(get(RangeKey::D90), 2);
        assert_eq!(get(RangeKey::Y1), 2); // 2019-06-30 is 366 days back → outside
        assert_eq!(get(RangeKey::All), 3);
        assert_eq!(r.len(), 5);
    }

    #[test]
    fn empty_range_yields_a_zero_stats_block_without_panicking() {
        let s = aggregate(std::iter::empty::<&Commit>());
        assert_eq!(s.commits, 0);
        assert_eq!(s.bus_factor, 0);
        assert!(s.hotspots.is_empty() && s.calendar.is_empty() && s.co_change.is_empty());
        assert_eq!(s.avg_msg_len, 0);
    }

    #[test]
    fn bus_factor_is_the_smallest_group_covering_half() {
        assert_eq!(bus_factor(&[]), 0);
        assert_eq!(bus_factor(&[10]), 1);
        assert_eq!(bus_factor(&[5, 5]), 1); // 5 of 10 = 50 % → one person suffices
        assert_eq!(bus_factor(&[3, 3, 3, 3]), 2);
        assert_eq!(bus_factor(&[1, 9]), 1);
    }

    #[test]
    fn heatmap_calendar_hotspots_dirs_and_co_change() {
        // 2026-08-24 is a Monday.
        let raw = [
            rec("2026-08-24T14:00:00+02:00", "A", "a@x", "feat: 1", &[("src/a.rs", 1, 0), ("src/b.rs", 1, 0)]),
            rec("2026-08-24T14:30:00+02:00", "A", "a@x", "feat: 2", &[("src/a.rs", 1, 0), ("src/b.rs", 1, 0)]),
            rec("2026-08-23T09:00:00+02:00", "A", "a@x", "fix: 3", &[("src/a.rs", 1, 0), ("README.md", 1, 0)]),
            rec("2026-08-22T09:00:00+02:00", "B", "b@x", "fix: 4", &[("docs/x.md", 1, 0)]),
        ]
        .concat();
        let s = aggregate(&parse_commits(&raw));
        assert_eq!(s.heatmap[0][14], 2); // Monday 14h
        assert_eq!(s.heatmap[6][9], 1); // Sunday 2026-08-23 09h
        assert_eq!(s.calendar.iter().find(|d| d.date == "2026-08-24").unwrap().commits, 2);
        // src/a.rs: 3 changes, 1 author → hotspot.
        assert_eq!(s.hotspots[0], Hotspot { path: "src/a.rs".into(), changes: 3, authors: 1 });
        // Directories: src (3 commits, 1 author), (root) (1), docs (1).
        assert_eq!(s.dir_bus_factor[0], DirStat { dir: "src".into(), commits: 3, authors: 1, bus_factor: 1 });
        assert!(s.dir_bus_factor.iter().any(|d| d.dir == "(root)"));
        // a.rs+b.rs changed together twice.
        assert_eq!(s.co_change[0], CoChange { a: "src/a.rs".into(), b: "src/b.rs".into(), count: 2 });
        assert_eq!(s.bus_factor, 1); // A has 3 of 4
    }

    #[test]
    fn co_change_ignores_commits_with_more_than_thirty_files() {
        let many: Vec<(String, u64, u64)> = (0..31).map(|i| (format!("f{i}.rs"), 1, 0)).collect();
        let many_ref: Vec<(&str, u64, u64)> = many.iter().map(|(p, i, d)| (p.as_str(), *i, *d)).collect();
        let raw = [
            rec("2026-08-24T10:00:00+00:00", "A", "a@x", "chore: big", &many_ref),
            rec("2026-08-23T10:00:00+00:00", "A", "a@x", "chore: big", &many_ref),
        ]
        .concat();
        assert!(aggregate(&parse_commits(&raw)).co_change.is_empty());
    }

    #[test]
    fn range_key_serialises_lowercase_and_parses_back() {
        assert_eq!(serde_json::to_string(&RangeKey::D30).unwrap(), "\"d30\"");
        assert_eq!(serde_json::to_string(&RangeKey::All).unwrap(), "\"all\"");
        for k in RangeKey::ALL {
            let s = serde_json::to_string(&k).unwrap();
            assert_eq!(RangeKey::parse(s.trim_matches('"')), Some(k));
        }
        assert_eq!(RangeKey::parse("x"), None);
    }

    #[test]
    fn html_carries_range_heatmap_calendar_and_new_tables() {
        let mut s = parse_git_log(&synth_log());
        s.hotspots = vec![Hotspot { path: "src/<x>.rs".into(), changes: 5, authors: 1 }];
        s.co_change = vec![CoChange { a: "a.rs".into(), b: "b.rs".into(), count: 3 }];
        s.dir_bus_factor = vec![DirStat { dir: "src".into(), commits: 5, authors: 2, bus_factor: 1 }];
        s.bus_factor = 1;
        let html = build_html(&s, RangeKey::D90);
        assert!(html.contains("letzte 90 Tage"));
        assert!(html.contains("<svg") && html.contains("class=\"heat\""));
        assert!(html.contains("Hotspots") && html.contains("src/&lt;x&gt;.rs"));
        assert!(html.contains("Co-Change") && html.contains("Bus-Faktor"));
        assert!(!html.contains("<script"), "must stay self-contained");
    }

    #[test]
    fn calendar_svg_places_days_by_weekday_and_week() {
        let days = vec![
            DayCount { date: "2026-08-24".into(), commits: 2 }, // Monday
            DayCount { date: "2026-08-30".into(), commits: 1 }, // Sunday, same week
        ];
        let svg = calendar_svg(&days);
        assert_eq!(svg.matches("<rect").count(), 2);
        assert!(svg.contains("2026-08-24: 2"));
        // Monday lands in row 0 (y=0), Sunday in row 6 (y=60), same week column.
        assert!(svg.contains("x=\"0\" y=\"0\"") && svg.contains("x=\"0\" y=\"60\""), "{svg}");
        assert_eq!(calendar_svg(&[]), "");
    }

    #[test]
    fn empty_range_html_says_so() {
        let html = build_html(&aggregate(std::iter::empty::<&Commit>()), RangeKey::D30);
        assert!(html.contains("Keine Commits in diesem Zeitraum"));
    }

    fn ranged(r: &[RangedStats], k: RangeKey) -> &RepoStats {
        &r.iter().find(|x| x.range == k).unwrap().stats
    }

    /// Anchor 2026-09-25 (Friday); commits on 2026-09-25, 2026-09-10,
    /// 2026-08-27 (inside 30 days) and 2026-03-02.
    fn timeline_log() -> String {
        [
            rec("2026-09-25T10:00:00+02:00", "A", "a@x", "feat: 1", &[("a.rs", 10, 2)]),
            rec("2026-09-10T10:00:00+02:00", "A", "a@x", "feat: 2", &[("a.rs", 5, 0), ("b.rs", 1, 1)]),
            rec("2026-08-27T10:00:00+02:00", "B", "b@x", "fix: 3", &[("b.rs", 0, 7)]),
            rec("2026-03-02T10:00:00+02:00", "B", "b@x", "chore: 4", &[("c.rs", 100, 0)]),
        ]
        .concat()
    }

    #[test]
    fn thirty_days_are_thirty_daily_buckets_not_two_months() {
        let r = analyze_ranges(&parse_commits(&timeline_log()));
        let s = ranged(&r, RangeKey::D30);
        assert_eq!(s.granularity, "day");
        assert_eq!(s.timeline.len(), 30, "gapless, one bucket per day");
        assert_eq!(s.timeline.first().unwrap().start, "2026-08-27");
        assert_eq!(s.timeline.last().unwrap().start, "2026-09-25");
        assert_eq!(s.timeline.iter().filter(|b| b.commits > 0).count(), 3);
    }

    #[test]
    fn medium_ranges_bucket_by_monday_weeks() {
        let r = analyze_ranges(&parse_commits(&timeline_log()));
        let s = ranged(&r, RangeKey::D90);
        assert_eq!(s.granularity, "week");
        // Every bucket starts on a Monday; the last one holds 2026-09-25.
        for b in &s.timeline {
            let o = day_ordinal(&b.start).unwrap();
            assert_eq!((o + 3).rem_euclid(7), 0, "{} is not a Monday", b.start);
        }
        assert_eq!(s.timeline.last().unwrap().start, "2026-09-21");
        assert_eq!(ranged(&r, RangeKey::D180).granularity, "week");
    }

    #[test]
    fn long_ranges_bucket_by_month_including_empty_months() {
        let r = analyze_ranges(&parse_commits(&timeline_log()));
        let all = ranged(&r, RangeKey::All);
        assert_eq!(all.granularity, "month");
        let starts: Vec<&str> = all.timeline.iter().map(|b| b.start.as_str()).collect();
        assert_eq!(starts, ["2026-03-01", "2026-04-01", "2026-05-01", "2026-06-01", "2026-07-01", "2026-08-01", "2026-09-01"]);
        assert_eq!(ranged(&r, RangeKey::Y1).granularity, "month");
    }

    #[test]
    fn timeline_line_totals_match_the_kpis() {
        let r = analyze_ranges(&parse_commits(&timeline_log()));
        for x in &r {
            let s = &x.stats;
            assert_eq!(s.timeline.iter().map(|b| b.insertions).sum::<u64>(), s.insertions, "{:?}", x.range);
            assert_eq!(s.timeline.iter().map(|b| b.deletions).sum::<u64>(), s.deletions, "{:?}", x.range);
            assert_eq!(s.timeline.iter().map(|b| b.commits).sum::<u64>(), s.commits, "{:?}", x.range);
        }
        let d30 = ranged(&r, RangeKey::D30);
        let sep25 = d30.timeline.iter().find(|b| b.start == "2026-09-25").unwrap();
        assert_eq!((sep25.insertions, sep25.deletions), (10, 2));
    }

    #[test]
    fn date_from_ordinal_inverts_day_ordinal() {
        for d in ["1970-01-01", "2000-02-29", "2024-12-31", "2026-09-25", "1999-03-01"] {
            assert_eq!(date_from_ordinal(day_ordinal(d).unwrap()), d);
        }
    }

    #[test]
    fn html_leads_with_code_changes_and_follows_the_range_granularity() {
        let r = analyze_ranges(&parse_commits(&timeline_log()));
        let html = build_html(ranged(&r, RangeKey::D30), RangeKey::D30);
        let code = html.find("Code-Änderungen").expect("code-changes section");
        let weekday = html.find("Commits nach Wochentag").unwrap();
        assert!(code < weekday, "code changes must come first");
        assert!(html.contains("class=\"churn\""), "churn chart");
        assert!(html.contains("+16") && html.contains("−10") && html.contains("netto"), "totals");
        assert!(html.contains("Aktivität pro Tag"));
        assert!(!html.contains("Aktivität nach Monat"));
        let all = build_html(ranged(&r, RangeKey::All), RangeKey::All);
        assert!(all.contains("Aktivität pro Monat"));
    }

    #[test]
    fn committer_time_is_parsed_from_the_trailing_field() {
        let raw = format!("{REC}sha{FLD}2020-01-01T00:00:00+00:00{FLD}A{FLD}a@x{FLD}feat: x{FLD}1790000000\n1\t0\tf.rs\n");
        assert_eq!(parse_commits(&raw)[0].committed, Some(1_790_000_000));
        // Old-format record (no trailing field) still parses, time unknown.
        let old = format!("{REC}sha{FLD}2020-01-01T00:00:00+00:00{FLD}A{FLD}a@x{FLD}feat: x\n");
        assert_eq!(parse_commits(&old)[0].committed, None);
    }

    #[test]
    fn html_leads_with_recent_activity_when_given() {
        use crate::repo_activity::{ActivityCounts, RecentActivity};
        let s = parse_git_log(&synth_log());
        let recent = RecentActivity {
            now: 0,
            day: ActivityCounts { commits: 3, insertions: 10, deletions: 2, files: 4, authors: 1, tags: 0 },
            day_prev: ActivityCounts { commits: 1, ..Default::default() },
            week: ActivityCounts { commits: 9, ..Default::default() },
            week_prev: ActivityCounts { commits: 12, ..Default::default() },
        };
        let gh = crate::github_api::GithubActivity { pushes_capped: true, ..Default::default() };
        let html = build_html_with(&s, RangeKey::All, Some(&recent), Some(&gh));
        let a = html.find("Aktivität 24 h / 7 Tage").expect("section");
        assert!(a < html.find("Code-Änderungen").unwrap());
        assert!(html.contains("↑ 2") && html.contains("↓ 3"), "deltas vs previous period");
        assert!(html.contains("Pushes") && html.contains("≥"), "capped push count marked");
        let plain = build_html(&s, RangeKey::All);
        assert!(!plain.contains("Aktivität 24 h / 7 Tage"));
    }

    /// Manual benchmark: `cargo test --release -p inspector-rust-core --lib repo_bench -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn repo_bench_analyze_ranges() {
        let mut raw = String::new();
        for i in 0..30_000u32 {
            let day = 1 + (i % 28);
            let month = 1 + (i / 2500) % 12;
            let n = 3 + (i % 25) as usize; // up to 27 files per commit
            let files: Vec<(String, u64, u64)> =
                (0..n).map(|f| (format!("src/mod{}/file{}.rs", (i as usize + f) % 40, (i as usize * 7 + f) % 400), 3, 1)).collect();
            let fr: Vec<(&str, u64, u64)> = files.iter().map(|(p, a, d)| (p.as_str(), *a, *d)).collect();
            raw.push_str(&rec(&format!("2025-{month:02}-{day:02}T10:00:00+00:00"), &format!("A{}", i % 17), &format!("a{}@x", i % 17), "feat: x", &fr));
        }
        let commits = parse_commits(&raw);
        let t = std::time::Instant::now();
        let r = analyze_ranges(&commits);
        eprintln!("analyze_ranges: {} commits, {:?}, co_change[0]={:?}", commits.len(), t.elapsed(), r[4].stats.co_change.first());
    }
}
