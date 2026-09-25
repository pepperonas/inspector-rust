# Repo-Aktivität 24 h / 7 Tage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Das `repo`-Panel zeigt ganz oben, was in den letzten 24 h und 7 Tagen passiert ist (Git: Commits, Zeilen, Dateien, Mitwirkende, Tags; GitHub: Pushes, PRs, Issues), jeweils mit ↑/↓ zur Vorperiode.

**Architecture:** Neues reines Modul `repo_activity.rs` rechnet vier halboffene Fenster aus den schon geladenen Commits (neues Feld Commit-Zeitpunkt `%ct`) plus Tag-Daten und hängt `recent` an `RepoAnalysis`. Neues Modul `github_api.rs` holt Events/Pulls/Issues (reine Zähl-Parser + dünne HTTP-Schicht) über einen eigenen IPC `repo_github_activity`, den das Panel nach der Analyse nachlädt. Export bekommt beide Blöcke optional mitgereicht.

**Tech Stack:** Rust (ureq 2 mit `tls`, serde_json, keyring über `repo_clone::github_token`), React 19 + TS + Vitest.

**Spec:** `docs/superpowers/specs/2026-09-25-repo-recent-activity-design.md`

## Global Constraints

- Fenster halboffen links: `t > start && t <= end`; `day = (now−86400, now]`, `day_prev = (now−172800, now−86400]`, `week = (now−604800, now]`, `week_prev = (now−1209600, now−604800]`.
- Commit-Zeitpunkt = Committer-Zeit `%ct` (Unix-Sekunden); Zeiträume 30 T … Gesamt bleiben beim Autor-Datum.
- GitHub-Endpunkte je max. 3 Seiten à 100; Zeitlimit 8 s je Anfrage; Header `Accept: application/vnd.github+json`, `X-GitHub-Api-Version: 2022-11-28`, `User-Agent: inspector-rust`; Token nur als `Authorization: Bearer …`, nie in der URL; `retry_without_token` bei Auth-Fehler.
- Fehler-Präfixe: `github.rate_limit`, `github.not_found`, `github.network`, `github.http`.
- Differenzen neutral eingefärbt (gedämpft), nie rot/grün.
- Kein neues Crate, kein neues npm-Package. Blockierende IPC = `async` + `spawn_blocking`.
- Fremde Dateien der parallelen dezibel-Session nie mitcommitten (`CHANGELOG.md`/`commandDocs.ts` nur eigene Hunks via `git apply --cached`).
- Commit-Messages enden mit `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` und `Claude-Session: https://claude.ai/code/session_01WbJj9S9wRyEvzQE8REMWB4`.

## Review Focus

1. **Ein Commit genau auf der 24-h-Grenze** zählt zur Vorperiode, nicht doppelt. → Task 1 Test `boundaries_are_half_open`.
2. **Rebase-Fall:** alter Autor-Zeitstempel, heutiger Commit-Zeitpunkt → zählt in „24 h". → Task 1 Test `a_rewritten_old_commit_counts_today`.
3. **Ein PR, vor Monaten eröffnet und gestern gemergt,** zählt als „gemergt 24 h", aber nicht als „geöffnet". → Task 2 Test `old_pr_merged_yesterday`.
4. **Issues-Endpunkt liefert auch PRs** → werden nicht als Issues gezählt. → Task 2 Test `pull_requests_are_not_issues`.
5. **Events-Grenze:** drei volle Seiten, deren letztes Ereignis noch innerhalb von 14 Tagen liegt → `pushes_capped = true`. → Task 2 Test `full_pages_inside_the_horizon_mark_the_count_capped`.

---

### Task 1: Commit-Zeitpunkt, Tag-Daten, `repo_activity.rs`

**Files:**
- Create: `core/rust-lib/src/repo_activity.rs`
- Modify: `core/rust-lib/src/lib.rs` (`mod repo_activity;` neben `mod repo_stats;`)
- Modify: `core/rust-lib/src/repo_stats.rs` (Commit-Struct ~Z.191, `parse_commits` ~Z.300, `git_log`-Format ~Z.678, `RepoAnalysis`/`finish`/`analyze_local`/`analyze_remote` ~Z.748–810)

**Interfaces:**
- Produces: `Commit.committed: Option<i64>`; `repo_activity::{ActivityCounts {commits, insertions, deletions, files, authors, tags: u64}, RecentActivity {now: i64, day, day_prev, week, week_prev: ActivityCounts}, fn recent_activity(commits: &[Commit], tag_times: &[i64], now: i64) -> RecentActivity, fn in_window(t: i64, start: i64, end: i64) -> bool, const DAY: i64 = 86_400, const WEEK: i64 = 604_800}`; `repo_stats::tag_times_from_dir(dir: &Path) -> Vec<i64>`; `RepoAnalysis.recent: RecentActivity`. Alle Structs `Serialize + Deserialize + Clone + Debug + PartialEq + Default`.

- [ ] **Step 1: Tests schreiben** — `core/rust-lib/src/repo_activity.rs` nur mit Kopf, `use` und Tests:

```rust
//! "What happened in the last 24 h / 7 days" (v0.185.0) — four rolling,
//! half-open windows `(start, end]` from `now`, each compared with the
//! equally long period before it. Pure; the time source is the COMMITTER
//! timestamp (`%ct`), so a commit rewritten today (rebase/cherry-pick) counts
//! today even though its author date is old.

use crate::repo_stats::Commit;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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
```

`lib.rs`: `mod repo_activity;` unter `mod repo_stats;`. In `repo_stats.rs` am `Commit`-Struct das Feld ergänzen (nach `subject`):

```rust
    /// Committer timestamp (Unix seconds, `%ct`) — "when it happened" for the
    /// 24 h / 7 d windows; `None` if git didn't print one.
    pub committed: Option<i64>,
```

- [ ] **Step 2: Rot prüfen**

Run: `cargo test -p inspector-rust-core --lib repo_activity`
Expected: FAIL — Kompilierfehler (`in_window`, `recent_activity`, `ActivityCounts`, `DAY` fehlen; `parse_commits` setzt `committed` nicht).

- [ ] **Step 3: Implementieren.**

`repo_activity.rs` über den Tests:

```rust
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
```

`repo_stats.rs`:
- Format (`let fmt = format!(…)` ~Z.678) auf `format!("{REC}%H{FLD}%aI{FLD}%aN{FLD}%aE{FLD}%s{FLD}%ct")` ändern — `%ct` ans ENDE, damit alte Test-Logs ohne das Feld weiter parsen (Feld fehlt → `None`).
- In `parse_commits` nach `let subject = f.next().unwrap_or("");` einfügen: `let committed = f.next().and_then(|t| t.trim().parse::<i64>().ok());` und im `Commit { … }`-Literal `committed,` ergänzen.
- Tag-Daten:

```rust
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
```

- `RepoAnalysis` bekommt `pub recent: crate::repo_activity::RecentActivity,`; `finish(name, source, github, commits)` → `finish(name, source, github, commits, tag_times: &[i64])` und setzt `recent: crate::repo_activity::recent_activity(commits, tag_times, now_unix())` mit

```rust
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
```

- Aufrufer: `analyze_local` → `finish(…, &commits, &tag_times_from_dir(dir))`; GitHub-Zweig → `tag_times_from_dir(&dir)` nach `cached_commits`; Nicht-GitHub-Zweig → `let tags = tag_times_from_dir(&tmp);` VOR `remove_dir_all(&tmp)`.

- [ ] **Step 4: Grün + Parser-Test** — zusätzlich in `repo_stats` tests:

```rust
    #[test]
    fn committer_time_is_parsed_from_the_trailing_field() {
        let raw = format!("{REC}sha{FLD}2020-01-01T00:00:00+00:00{FLD}A{FLD}a@x{FLD}feat: x{FLD}1790000000\n1\t0\tf.rs\n");
        assert_eq!(parse_commits(&raw)[0].committed, Some(1_790_000_000));
        // Old-format record (no trailing field) still parses, time unknown.
        let old = format!("{REC}sha{FLD}2020-01-01T00:00:00+00:00{FLD}A{FLD}a@x{FLD}feat: x\n");
        assert_eq!(parse_commits(&old)[0].committed, None);
    }
```

Run: `cargo test -p inspector-rust-core --lib repo_`
Expected: PASS (alle bisherigen + 5 neue).

- [ ] **Step 5: Mutationsproben** (einzeln, zurücksetzen): `t > start` → `t >= start` (→ `boundaries_are_half_open` rot); in `counts` `c.committed` → aus `c.day` rechnen, z. B. `let Some(t) = c.day.map(|d| d * DAY) else` (→ `a_rewritten_old_commit_counts_today` rot).

- [ ] **Step 6: Commit** — `git add core/rust-lib/src/repo_activity.rs core/rust-lib/src/lib.rs core/rust-lib/src/repo_stats.rs` + Commit `feat(repo): recent activity windows (24 h / 7 d, committer time, tags)`.

---

### Task 2: `github_api.rs` — Pushes, PRs, Issues

**Files:**
- Create: `core/rust-lib/src/github_api.rs`
- Create: `core/rust-lib/src/testdata/github-events.json` (aufgezeichnet)
- Modify: `core/rust-lib/src/lib.rs` (`mod github_api;` + Handler `commands::repo_github_activity`)
- Modify: `core/rust-lib/src/commands.rs` (IPC nach `repo_clone`)

**Interfaces:**
- Consumes: `repo_activity::{in_window, DAY, WEEK}`, `repo_clone::{github_token, retry_without_token}`.
- Produces: `github_api::{GithubCounts {pushes, prs_opened, prs_merged, issues_opened, issues_closed: u64}, GithubActivity {day, day_prev, week, week_prev: GithubCounts, pushes_capped: bool}, fn api_url(owner, repo, path_and_query) -> String, fn count_events(pages: &[Vec<Value>], now: i64) -> (…4 push counts, capped: bool), fn fetch_activity(owner, repo, token, now) -> Result<GithubActivity, String>}`; IPC `repo_github_activity(owner: String, repo: String) -> GithubActivity`.

- [ ] **Step 1: Fixture aufzeichnen** (nur Felder, die gezählt werden — keine Nutzer-/Repo-Daten):

```bash
gh api "repos/pepperonas/inspector-rust/events?per_page=100" --jq '[.[] | {type, created_at}]' > core/rust-lib/src/testdata/github-events.json
jq 'length' core/rust-lib/src/testdata/github-events.json
jq '[.[] | select(.type=="PushEvent")] | length' core/rust-lib/src/testdata/github-events.json
```

Die zwei Zahlen notieren (Gesamt, Pushes) — der Test prüft den Parser gegen `jq`s Push-Zählung, indem er alle PushEvents mit `now` = jüngstes Ereignis + 1 und einem 10-Jahres-Fenster zählt (siehe Test `recorded_events_count_like_jq`, dort die notierte Zahl eintragen).

- [ ] **Step 2: Tests schreiben** — `github_api.rs` mit Kopf, `use`s und:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_790_000_000;

    fn iso(t: i64) -> String {
        chrono::DateTime::from_timestamp(t, 0).unwrap().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }

    #[test]
    fn events_count_pushes_per_window() {
        let page = vec![
            json!({"type":"PushEvent","created_at": iso(NOW - 10)}),
            json!({"type":"PushEvent","created_at": iso(NOW - DAY - 10)}),
            json!({"type":"ReleaseEvent","created_at": iso(NOW - 20)}),
            json!({"type":"PushEvent","created_at": iso(NOW - 8 * DAY)}),
        ];
        let e = count_events(&[page], NOW);
        assert_eq!((e.day, e.day_prev, e.week, e.week_prev, e.capped), (1, 1, 2, 1, false));
    }

    #[test]
    fn full_pages_inside_the_horizon_mark_the_count_capped() {
        let page: Vec<Value> = (0..100).map(|i| json!({"type":"PushEvent","created_at": iso(NOW - 100 - i)})).collect();
        let e = count_events(&[page.clone(), page.clone(), page], NOW);
        assert!(e.capped, "300 events all within 14 days → incomplete");
        let short = vec![json!({"type":"PushEvent","created_at": iso(NOW - 5)})];
        assert!(!count_events(&[short], NOW).capped);
    }

    #[test]
    fn old_pr_merged_yesterday() {
        let pulls = vec![
            json!({"created_at": iso(NOW - 60 * DAY), "merged_at": iso(NOW - 3600), "updated_at": iso(NOW - 3600)}),
            json!({"created_at": iso(NOW - 2 * 3600), "merged_at": null, "updated_at": iso(NOW - 2 * 3600)}),
        ];
        let p = count_pulls(&pulls, NOW);
        assert_eq!((p.opened.day, p.merged.day), (1, 1));
        assert_eq!(p.opened.week, 1);
    }

    #[test]
    fn pull_requests_are_not_issues() {
        let issues = vec![
            json!({"created_at": iso(NOW - 100), "closed_at": null}),
            json!({"created_at": iso(NOW - 100), "closed_at": null, "pull_request": {"url": "x"}}),
            json!({"created_at": iso(NOW - 20 * DAY), "closed_at": iso(NOW - 3 * DAY)}),
        ];
        let i = count_issues(&issues, NOW);
        assert_eq!((i.opened.day, i.opened.week, i.closed.week, i.closed.day), (1, 1, 1, 0));
    }

    #[test]
    fn the_token_never_goes_into_the_url() {
        let u = api_url("o", "r", "events?per_page=100&page=2");
        assert_eq!(u, "https://api.github.com/repos/o/r/events?per_page=100&page=2");
        assert!(!u.contains("token") && !u.contains("access_token"));
    }

    #[test]
    fn classify_http_status_names_rate_limit_and_not_found() {
        assert_eq!(classify_status(403, Some("0")), "github.rate_limit");
        assert_eq!(classify_status(429, None), "github.rate_limit");
        assert_eq!(classify_status(404, None), "github.not_found");
        assert_eq!(classify_status(401, None), "github.auth");
        assert_eq!(classify_status(403, Some("4000")), "github.auth");
        assert_eq!(classify_status(500, None), "github.http");
    }

    #[test]
    fn recorded_events_count_like_jq() {
        let v: Vec<Value> = serde_json::from_str(include_str!("testdata/github-events.json")).unwrap();
        let newest = v.iter().filter_map(|e| ts(&e["created_at"])).max().unwrap();
        // Everything inside one huge window: the parser must see exactly the
        // PushEvents jq counted when the fixture was recorded.
        let pushes = v.iter().filter(|e| e["type"] == "PushEvent").count() as u64;
        let e = count_events(&[v.clone()], newest + 1);
        assert_eq!(e.week + e.week_prev + older_than_two_weeks(&v, newest + 1), pushes);
    }

    fn older_than_two_weeks(v: &[Value], now: i64) -> u64 {
        v.iter()
            .filter(|e| e["type"] == "PushEvent" && ts(&e["created_at"]).is_some_and(|t| t <= now - 2 * WEEK))
            .count() as u64
    }
}
```

Hinweis: `recorded_events_count_like_jq` braucht keine hart eingetragene Zahl — er vergleicht den Parser mit einer unabhängigen Zählung derselben Datei (Fenster + ältere). Die in Step 1 notierten Zahlen sind nur zum Gegenlesen.

`lib.rs`: `mod github_api;` unter `mod repo_clone;`.

- [ ] **Step 3: Rot prüfen** — `cargo test -p inspector-rust-core --lib github_api` → Kompilierfehler (Funktionen fehlen).

- [ ] **Step 4: Implementieren** (über den Tests):

```rust
//! GitHub activity for the `repo` panel (v0.185.0): pushes (Events API),
//! PRs opened/merged, issues opened/closed — per 24 h / 7 d window and the
//! period before. Pure counters over `serde_json::Value` + a thin ureq shell.
//!
//! Since 2025 a `PushEvent` payload carries no commit count (only
//! ref/before/head), so pushes are counted, not pushed commits. The Events
//! API returns at most 300 events / 90 days: three FULL pages whose oldest
//! event is still inside the 14-day horizon mean the count is incomplete
//! (`pushes_capped`). The token only ever travels as an Authorization header.

use crate::repo_activity::{in_window, DAY, WEEK};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const PAGE: usize = 100;
const MAX_PAGES: usize = 3;
const TIMEOUT_SECS: u64 = 8;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct GithubCounts {
    pub pushes: u64,
    pub prs_opened: u64,
    pub prs_merged: u64,
    pub issues_opened: u64,
    pub issues_closed: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct GithubActivity {
    pub day: GithubCounts,
    pub day_prev: GithubCounts,
    pub week: GithubCounts,
    pub week_prev: GithubCounts,
    pub pushes_capped: bool,
}

/// Four window counts for one kind of timestamp.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Win {
    pub day: u64,
    pub day_prev: u64,
    pub week: u64,
    pub week_prev: u64,
}

impl Win {
    fn add(&mut self, t: i64, now: i64) {
        if in_window(t, now - DAY, now) { self.day += 1; }
        if in_window(t, now - 2 * DAY, now - DAY) { self.day_prev += 1; }
        if in_window(t, now - WEEK, now) { self.week += 1; }
        if in_window(t, now - 2 * WEEK, now - WEEK) { self.week_prev += 1; }
    }
}

pub struct EventCounts {
    pub day: u64,
    pub day_prev: u64,
    pub week: u64,
    pub week_prev: u64,
    pub capped: bool,
}

pub struct TwoWin {
    pub opened: Win,
    pub merged: Win,
    pub closed: Win,
}

pub(crate) fn ts(v: &Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(v.as_str()?).ok().map(|d| d.timestamp())
}

pub fn count_events(pages: &[Vec<Value>], now: i64) -> EventCounts {
    let mut w = Win::default();
    for e in pages.iter().flatten() {
        if e["type"] == "PushEvent" {
            if let Some(t) = ts(&e["created_at"]) { w.add(t, now); }
        }
    }
    let oldest_inside_horizon = pages
        .last()
        .and_then(|p| p.iter().filter_map(|e| ts(&e["created_at"])).min())
        .is_some_and(|t| t > now - 2 * WEEK);
    let capped = pages.len() >= MAX_PAGES && pages.iter().all(|p| p.len() >= PAGE) && oldest_inside_horizon;
    EventCounts { day: w.day, day_prev: w.day_prev, week: w.week, week_prev: w.week_prev, capped }
}

pub fn count_pulls(pulls: &[Value], now: i64) -> TwoWin {
    let (mut opened, mut merged) = (Win::default(), Win::default());
    for p in pulls {
        if let Some(t) = ts(&p["created_at"]) { opened.add(t, now); }
        if let Some(t) = ts(&p["merged_at"]) { merged.add(t, now); }
    }
    TwoWin { opened, merged, closed: Win::default() }
}

pub fn count_issues(issues: &[Value], now: i64) -> TwoWin {
    let (mut opened, mut closed) = (Win::default(), Win::default());
    // The issues endpoint also returns pull requests — they carry a
    // `pull_request` key and are counted by `count_pulls` instead.
    for i in issues.iter().filter(|i| i.get("pull_request").is_none()) {
        if let Some(t) = ts(&i["created_at"]) { opened.add(t, now); }
        if let Some(t) = ts(&i["closed_at"]) { closed.add(t, now); }
    }
    TwoWin { opened, merged: Win::default(), closed }
}

pub fn api_url(owner: &str, repo: &str, path_and_query: &str) -> String {
    format!("https://api.github.com/repos/{owner}/{repo}/{path_and_query}")
}

pub fn classify_status(status: u16, remaining: Option<&str>) -> &'static str {
    match status {
        429 => "github.rate_limit",
        403 if remaining == Some("0") => "github.rate_limit",
        401 | 403 => "github.auth",
        404 => "github.not_found",
        _ => "github.http",
    }
}

fn get_page(url: &str, token: Option<&str>) -> Result<Vec<Value>, String> {
    let mut req = ureq::get(url)
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .set("User-Agent", "inspector-rust");
    if let Some(t) = token {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    match req.call() {
        Ok(r) => r.into_json::<Vec<Value>>().map_err(|e| format!("github.http: Antwort unlesbar: {e}")),
        Err(ureq::Error::Status(code, r)) => {
            let class = classify_status(code, r.header("x-ratelimit-remaining"));
            // `repo.auth` prefix so `retry_without_token` retries a stale token.
            let class = if class == "github.auth" { "repo.auth" } else { class };
            Err(format!("{class}: HTTP {code}"))
        }
        Err(e) => Err(format!("github.network: {e}")),
    }
}

/// Pages until a short page, MAX_PAGES, or `stop` says the rest is too old.
fn get_pages(owner: &str, repo: &str, path: &str, token: Option<&str>, stop: impl Fn(&[Value]) -> bool) -> Result<Vec<Vec<Value>>, String> {
    let sep = if path.contains('?') { '&' } else { '?' };
    let mut pages = Vec::new();
    for page in 1..=MAX_PAGES {
        let items = get_page(&api_url(owner, repo, &format!("{path}{sep}per_page={PAGE}&page={page}")), token)?;
        let done = items.len() < PAGE || stop(&items);
        pages.push(items);
        if done { break; }
    }
    Ok(pages)
}

pub fn fetch_activity(owner: &str, repo: &str, token: Option<&str>, now: i64) -> Result<GithubActivity, String> {
    let horizon = now - 2 * WEEK;
    let too_old = |field: &'static str| move |items: &[Value]| items.iter().filter_map(|v| ts(&v[field])).min().is_some_and(|t| t <= horizon);
    let since = chrono::DateTime::from_timestamp(horizon, 0).map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string()).unwrap_or_default();
    crate::repo_clone::retry_without_token(token, |t| {
        let events = get_pages(owner, repo, "events", t, too_old("created_at"))?;
        let pulls = get_pages(owner, repo, "pulls?state=all&sort=updated&direction=desc", t, too_old("updated_at"))?;
        let issues = get_pages(owner, repo, &format!("issues?state=all&since={since}"), t, |_| false)?;
        let e = count_events(&events, now);
        let p = count_pulls(&pulls.concat(), now);
        let i = count_issues(&issues.concat(), now);
        let mk = |pushes: u64, sel: fn(&Win) -> u64| GithubCounts {
            pushes,
            prs_opened: sel(&p.opened),
            prs_merged: sel(&p.merged),
            issues_opened: sel(&i.opened),
            issues_closed: sel(&i.closed),
        };
        Ok(GithubActivity {
            day: mk(e.day, |w| w.day),
            day_prev: mk(e.day_prev, |w| w.day_prev),
            week: mk(e.week, |w| w.week),
            week_prev: mk(e.week_prev, |w| w.week_prev),
            pushes_capped: e.capped,
        })
    })
}
```

(`Win::add` braucht `#[derive]`-freie Methode — oben bereits enthalten; `count_events` in Tests nutzt die Felder `day`, `day_prev`, `week`, `week_prev`, `capped`.)

IPC in `commands.rs` (nach `repo_clone`):

```rust
/// GitHub pushes / PRs / issues for the last 24 h and 7 days (+ previous
/// periods). Separate from `repo_analyze` so the git numbers show at once.
#[tauri::command]
pub async fn repo_github_activity(owner: String, repo: String) -> Result<crate::github_api::GithubActivity, String> {
    // Same validation as a parsed URL — owner/repo go into the API path.
    let u = crate::repo_url::parse_repo_url(&format!("https://github.com/{owner}/{repo}"))
        .ok_or_else(|| "github.http: ungültiges Repo".to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        crate::github_api::fetch_activity(&u.owner, &u.repo, crate::repo_clone::github_token().as_deref(), now)
    })
    .await
    .map_err(|e| format!("github task: {e}"))?
}
```

`lib.rs`: `commands::repo_github_activity,` nach `commands::repo_clone,`.

- [ ] **Step 5: Grün** — `cargo test -p inspector-rust-core --lib github_api` → PASS (7 Tests). `cargo clippy -p inspector-rust-core --all-targets -- -D warnings` → sauber.

- [ ] **Step 6: Live-Probe (opt-in, ignored)** — Test anhängen und einmal laufen lassen:

```rust
    /// `IR_LIVE_GH=owner/repo cargo test -p inspector-rust-core --lib live_github_activity -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_github_activity() {
        let Ok(or) = std::env::var("IR_LIVE_GH") else { return };
        let (o, r) = or.split_once('/').unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
        let a = fetch_activity(o, r, crate::repo_clone::github_token().as_deref(), now).unwrap();
        eprintln!("{a:#?}");
    }
```

Run: `IR_LIVE_GH=pepperonas/inspector-rust cargo test -p inspector-rust-core --lib live_github_activity -- --ignored --nocapture` → Pushes 24 h ≥ 1 (heute wurde gepusht); gegenlesen mit `gh api repos/pepperonas/inspector-rust/events --jq '[.[]|select(.type=="PushEvent")|.created_at]' | head`.

- [ ] **Step 7: Mutationsproben**: in `count_issues` den `pull_request`-Filter entfernen (→ `pull_requests_are_not_issues` rot); in `count_events` `&& oldest_inside_horizon` entfernen und den Kurz-Fall prüfen — dafür ist `full_pages_…` mit `short` zuständig (bleibt grün → stattdessen `pages.iter().all(|p| p.len() >= PAGE)` entfernen → der `short`-Assert wird rot).

- [ ] **Step 8: Commit** — `git add core/rust-lib/src/github_api.rs core/rust-lib/src/testdata/github-events.json core/rust-lib/src/lib.rs core/rust-lib/src/commands.rs` + `feat(repo): GitHub pushes/PRs/issues per 24 h / 7 d window`.

---

### Task 3: Export mit Aktivitätstabelle

**Files:**
- Modify: `core/rust-lib/src/repo_stats.rs` (`build_html` → `build_html_with`)
- Modify: `core/rust-lib/src/commands.rs:1397-1417` (`repo_export`)

**Interfaces:**
- Consumes: `RecentActivity`, `GithubActivity`.
- Produces: `pub fn build_html_with(stats: &RepoStats, range: RangeKey, recent: Option<&RecentActivity>, github: Option<&GithubActivity>) -> String`; `build_html(s, r)` bleibt als `build_html_with(s, r, None, None)`; IPC `repo_export(stats, range, format, recent: Option<RecentActivity>, github: Option<GithubActivity>)`.

- [ ] **Step 1: Test**:

```rust
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
```

- [ ] **Step 2: Rot** — `cargo test -p inspector-rust-core --lib html_leads_with_recent` → Kompilierfehler (`build_html_with` fehlt).

- [ ] **Step 3: Implementieren.** `build_html` umbenennen zu `build_html_with` mit den zwei zusätzlichen Parametern; neue dünne Funktion:

```rust
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
        "<section><h2>Aktivität 24 h / 7 Tage</h2><table><thead><tr><th>Wert</th><th>24 h</th><th>7 Tage</th></tr></thead><tbody>{rows}</tbody></table><p class=\"rp-lede\">Vergleich jeweils zur gleich langen Vorperiode. Git: Haupt-Branch ohne Merges · Pushes: alle Branches.</p></section>"
    )
}
```

Im `body`-`format!` direkt nach `{stats}` den Platzhalter `{recent}` vor `{code_changes}` einsetzen, Argument `recent = recent.map(|r| activity_section(r, github)).unwrap_or_default()`. In `REPO_CSS`: `.dl { color:var(--muted); font-size:10.5px; margin-left:4px }`. Der Empty-Range-Frühausstieg bleibt unverändert (ohne Aktivität).

`commands.rs` `repo_export`: Parameter `recent: Option<crate::repo_activity::RecentActivity>, github: Option<crate::github_api::GithubActivity>,` ergänzen und `build_html(&stats, range)` → `build_html_with(&stats, range, recent.as_ref(), github.as_ref())`.

- [ ] **Step 4: Grün** — `cargo test -p inspector-rust-core --lib repo_stats` → PASS. Sichtprüfung: Dump-Test um `build_html_with(…, Some(&a.recent), None)` erweitern (in `dump_for_a_sight_check` für den IR_DUMP_REPO-Zweig), rendern, Screenshot ansehen.

- [ ] **Step 5: Mutation**: in `delta` die Zweige Greater/Less vertauschen → Test rot.

- [ ] **Step 6: Commit** — `feat(repo): export leads with 24 h / 7 d activity`.

---

### Task 4: Frontend — Typen, Helfer, Aktivitätskarte

**Files:**
- Modify: `core/frontend/src/lib/ipc.ts` (repo-Block), `core/frontend/src/lib/ipc.test.ts`
- Modify: `core/frontend/src/lib/repo.ts`, `core/frontend/src/lib/repo.test.ts`
- Modify: `core/frontend/src/components/RepoPanel.tsx`, `core/frontend/src/components/RepoPanel.test.tsx`

**Interfaces:**
- Produces (TS): `ActivityCounts`, `RecentActivity`, `GithubCounts`, `GithubActivity` (Feldnamen wie Rust); `RepoAnalysis.recent: RecentActivity`; `repoGithubActivity(owner: string, repo: string): Promise<GithubActivity>`; `repoExport(stats, range, format, recent?: RecentActivity, github?: GithubActivity | null)`; `deltaLabel(cur: number, prev: number): { text: string; title: string }`; `githubErrorHint(err: string): string`.

- [ ] **Step 1: Tests.** `repo.test.ts` anhängen (Import oben ergänzen: `deltaLabel, githubErrorHint`):

```ts
describe("recent activity helpers", () => {
  it("labels the change vs the previous period neutrally", () => {
    expect(deltaLabel(5, 2)).toEqual({ text: "↑ 3", title: "Vorperiode: 2" });
    expect(deltaLabel(1, 4)).toEqual({ text: "↓ 3", title: "Vorperiode: 4" });
    expect(deltaLabel(3, 3)).toEqual({ text: "±0", title: "Vorperiode: 3" });
  });
  it("turns GitHub API errors into hints", () => {
    expect(githubErrorHint("github.rate_limit: HTTP 403")).toMatch(/Abfragelimit/);
    expect(githubErrorHint("github.not_found: HTTP 404")).toMatch(/nicht gefunden|kein Zugriff/i);
    expect(githubErrorHint("github.network: timeout")).toMatch(/nicht erreichbar/);
    expect(githubErrorHint("repo.auth: HTTP 401")).toMatch(/Token/);
  });
});
```

`ipc.test.ts` anhängen:

```ts
describe("repo github activity wrapper (v0.185.0)", () => {
  it("repoGithubActivity passes owner + repo", async () => {
    await ipc.repoGithubActivity("o", "r");
    expect(mockInvoke).toHaveBeenCalledWith("repo_github_activity", { owner: "o", repo: "r" });
  });
  it("repoExport forwards recent + github", async () => {
    const stats = { commits: 1 } as unknown as ipc.RepoStats;
    const recent = { now: 1 } as unknown as ipc.RecentActivity;
    await ipc.repoExport(stats, "all", "html", recent, null);
    expect(mockInvoke).toHaveBeenCalledWith("repo_export", { stats, range: "all", format: "html", recent, github: null });
  });
});
```

`RepoPanel.test.tsx`: Mock um `repoGithubActivity: (o: string, r: string) => repoGithubActivity(o, r)` erweitern (`const repoGithubActivity = vi.fn<(o: string, r: string) => Promise<GithubActivity>>();`, Default in `beforeEach`: `repoGithubActivity.mockResolvedValue(gh)`), `analysis` bekommt

```ts
  recent: {
    now: 0,
    day: { commits: 3, insertions: 10, deletions: 1, files: 4, authors: 1, tags: 0 },
    day_prev: { commits: 1, insertions: 0, deletions: 0, files: 0, authors: 0, tags: 0 },
    week: { commits: 9, insertions: 50, deletions: 5, files: 7, authors: 3, tags: 1 },
    week_prev: { commits: 12, insertions: 0, deletions: 0, files: 0, authors: 0, tags: 0 },
  },
```

mit `const zero = { pushes: 0, prs_opened: 0, prs_merged: 0, issues_opened: 0, issues_closed: 0 }; const gh = { day: { ...zero, pushes: 4 }, day_prev: zero, week: { ...zero, pushes: 20 }, week_prev: zero, pushes_capped: true };` und neue Tests:

```tsx
  it("shows the 24 h / 7 day activity above the range chips, with deltas", async () => {
    const v = render(<RepoPanel arg="https://github.com/o/r" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => v.getByText("Aktivität 24 h / 7 Tage"));
    const html = v.container.innerHTML;
    expect(html.indexOf("Aktivität 24 h / 7 Tage")).toBeLessThan(html.indexOf('aria-label="Zeitraum"'));
    expect(v.getByText("↑ 2")).toBeTruthy(); // commits 24 h: 3 vs 1
    expect(v.getByText("↓ 3")).toBeTruthy(); // commits 7 d: 9 vs 12
  });
  it("loads GitHub numbers for a GitHub repo and marks a capped push count", async () => {
    const v = render(<RepoPanel arg="https://github.com/o/r" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => expect(repoGithubActivity).toHaveBeenCalledWith("o", "r"));
    await waitFor(() => v.getByText("Pushes"));
    // "≥ ", "20" and the delta are separate nodes inside one cell.
    expect(v.getByText(/^≥ 20\b/)).toBeTruthy();
  });
  it("no GitHub block for a local repo", async () => {
    repoAnalyze.mockResolvedValue({ ...analysis, github: null });
    const v = render(<RepoPanel arg="~/x" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => v.getByText("Aktivität 24 h / 7 Tage"));
    expect(repoGithubActivity).not.toHaveBeenCalled();
    expect(v.queryByText("Pushes")).toBeNull();
  });
  it("a GitHub error shows a hint and keeps the git numbers", async () => {
    repoGithubActivity.mockRejectedValue("github.rate_limit: HTTP 403");
    const v = render(<RepoPanel arg="https://github.com/o/r" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => v.getByText(/Abfragelimit/));
    expect(v.getByText("↑ 2")).toBeTruthy();
  });
```

- [ ] **Step 2: Rot** — `cd core/frontend && npx vitest run src/lib/repo.test.ts src/lib/ipc.test.ts src/components/RepoPanel.test.tsx` → Fehlschläge (fehlende Exporte/Karte).

- [ ] **Step 3: Implementieren.**

`ipc.ts` (repo-Block):

```ts
export interface ActivityCounts { commits: number; insertions: number; deletions: number; files: number; authors: number; tags: number }
export interface RecentActivity { now: number; day: ActivityCounts; day_prev: ActivityCounts; week: ActivityCounts; week_prev: ActivityCounts }
export interface GithubCounts { pushes: number; prs_opened: number; prs_merged: number; issues_opened: number; issues_closed: number }
export interface GithubActivity { day: GithubCounts; day_prev: GithubCounts; week: GithubCounts; week_prev: GithubCounts; pushes_capped: boolean }
```

`RepoAnalysis` um `recent: RecentActivity;` ergänzen;

```ts
/** GitHub pushes / PRs / issues for 24 h and 7 days (+ previous periods). */
export function repoGithubActivity(owner: string, repo: string): Promise<GithubActivity> {
  return invoke("repo_github_activity", { owner, repo });
}
```

`repoExport` → `(stats, range, format = "html", recent?: RecentActivity, github: GithubActivity | null = null)` mit `invoke("repo_export", { stats, range, format, recent, github })`. ⚠️ Bestehender ipc-Pin `repoExport passes the panel's stats + range + format` erwartet dann zusätzlich `recent: undefined, github: null` — anpassen (Vertrag bewusst erweitert).

`repo.ts`:

```ts
/** Neutral change vs the previous period — fewer isn't automatically worse. */
export function deltaLabel(cur: number, prev: number): { text: string; title: string } {
  const text = cur > prev ? `↑ ${cur - prev}` : cur < prev ? `↓ ${prev - cur}` : "±0";
  return { text, title: `Vorperiode: ${prev}` };
}

export function githubErrorHint(err: string): string {
  if (err.startsWith("github.rate_limit")) return "GitHub-Abfragelimit erreicht — `gh auth login` oder ein Token in Settings → Repositories erhöht es.";
  if (err.startsWith("github.not_found")) return "Repo bei GitHub nicht gefunden oder kein Zugriff (privat?).";
  if (err.startsWith("repo.auth")) return "GitHub lehnt das Token ab — Token in Settings → Repositories prüfen.";
  if (err.startsWith("github.network")) return "GitHub nicht erreichbar — Git-Werte oben sind trotzdem aktuell.";
  return "GitHub-Werte nicht verfügbar.";
}
```

`RepoPanel.tsx`: Imports `repoGithubActivity, type GithubActivity, type RecentActivity` bzw. `deltaLabel, githubErrorHint`. State + Nachladen:

```tsx
  const [gh, setGh] = useState<GithubActivity | null>(null);
  const [ghErr, setGhErr] = useState<string | null>(null);
  const ghRepo = analysis?.github ? `${analysis.github.owner}/${analysis.github.repo}` : null;
  useEffect(() => {
    setGh(null);
    setGhErr(null);
    if (!ghRepo) return;
    let dead = false;
    const [o, r] = ghRepo.split("/");
    repoGithubActivity(o, r)
      .then((a) => { if (!dead) setGh(a); })
      .catch((e) => { if (!dead) setGhErr(String(e)); });
    return () => { dead = true; };
  }, [ghRepo, analysis]);
```

(`analysis` in den Deps, damit „Neu analysieren" auch GitHub neu lädt.) `doExport` → `repoExport(stats, range, fmt === "pdf" ? "pdf" : "html", analysis?.recent, gh)` und `analysis`, `gh` in die Deps. Direkt VOR `<div className="flex flex-wrap items-center gap-1" role="group" aria-label="Zeitraum">`:

```tsx
      <RecentCard recent={analysis.recent} gh={gh} ghErr={ghErr} isGithub={!!analysis.github} />
```

Komponente (bei den übrigen Hilfskomponenten):

```tsx
function DeltaCell({ cur, prev, capped }: { cur: number; prev: number; capped?: boolean }) {
  const d = deltaLabel(cur, prev);
  return (
    <td className="py-0.5 text-right tabular-nums">
      {capped ? "≥ " : ""}
      {formatNum(cur)}{" "}
      <span className="text-[10px] text-[var(--color-muted)]" title={d.title}>{d.text}</span>
    </td>
  );
}

function RecentCard({ recent, gh, ghErr, isGithub }: { recent: RecentActivity; gh: GithubActivity | null; ghErr: string | null; isGithub: boolean }) {
  const git: [string, (c: RecentActivity["day"]) => number][] = [
    ["Commits", (c) => c.commits],
    ["Zeilen +", (c) => c.insertions],
    ["Zeilen −", (c) => c.deletions],
    ["Dateien", (c) => c.files],
    ["Mitwirkende", (c) => c.authors],
    ["Tags", (c) => c.tags],
  ];
  const hub: [string, (c: GithubActivity["day"]) => number, boolean][] = gh
    ? [
        ["Pushes", (c) => c.pushes, gh.pushes_capped],
        ["PRs geöffnet", (c) => c.prs_opened, false],
        ["PRs gemergt", (c) => c.prs_merged, false],
        ["Issues geöffnet", (c) => c.issues_opened, false],
        ["Issues geschlossen", (c) => c.issues_closed, false],
      ]
    : [];
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3 [contain:content]">
      <p className="mb-1 text-[11px] font-medium">Aktivität 24 h / 7 Tage</p>
      <table className="w-full text-[11px]">
        <thead>
          <tr className="text-[10px] text-[var(--color-muted)]">
            <th className="text-left font-normal" />
            <th className="text-right font-normal">24 h</th>
            <th className="text-right font-normal">7 Tage</th>
          </tr>
        </thead>
        <tbody>
          {git.map(([label, f]) => (
            <tr key={label}>
              <td className="py-0.5 text-[var(--color-muted)]">{label}</td>
              <DeltaCell cur={f(recent.day)} prev={f(recent.day_prev)} />
              <DeltaCell cur={f(recent.week)} prev={f(recent.week_prev)} />
            </tr>
          ))}
          {hub.map(([label, f, capped]) => (
            <tr key={label}>
              <td className="py-0.5 text-[var(--color-muted)]">{label}</td>
              <DeltaCell cur={f(gh!.day)} prev={f(gh!.day_prev)} capped={capped} />
              <DeltaCell cur={f(gh!.week)} prev={f(gh!.week_prev)} capped={capped} />
            </tr>
          ))}
        </tbody>
      </table>
      {isGithub && !gh && !ghErr && <p className="mt-1 text-[10px] text-[var(--color-muted)]">GitHub-Werte werden geladen…</p>}
      {ghErr && <p className="mt-1 text-[10px] text-[var(--color-muted)]">{githubErrorHint(ghErr)}</p>}
      <p className="mt-1 text-[10px] text-[var(--color-muted)]">
        Vergleich zur gleich langen Vorperiode · Git: Haupt-Branch ohne Merges{isGithub ? " · Pushes: alle Branches" : ""}
      </p>
    </div>
  );
}
```

⚠️ Der Test „shows … ↑ 2" darf nicht an mehrfach vorkommendem Text scheitern: bei den Fixture-Zahlen entstehen `↑ 2` (Commits 24 h) und `↓ 3` (Commits 7 d) je genau einmal; falls andere Zeilen denselben Text ergeben, auf `getAllByText(...).length` ≥ 1 umstellen.

- [ ] **Step 4: Grün** — Tests von Step 2 + `npx tsc --noEmit` + `npx eslint src/components/RepoPanel.tsx src/lib/repo.ts src/lib/ipc.ts` → sauber.

- [ ] **Step 5: Mutationsproben**: `RecentCard` hinter die Zeitraum-Chips verschieben (→ Reihenfolge-Test rot); in `deltaLabel` ↑/↓ vertauschen (→ rot); den `if (!ghRepo) return;` entfernen (→ „no GitHub block for a local repo" rot).

- [ ] **Step 6: Commit** — alle sechs Dateien + `feat(repo): 24 h / 7 day activity card with GitHub pushes, PRs, issues`.

---

### Task 5: Doku, Version, Installation, Push

- [ ] **Step 1:** CommandDoc `repo` (Beschreibung: Satz zur Aktivitätskarte; Tip: „Pushes zählen alle Branches, Git-Werte nur den Haupt-Branch; die Events-API liefert max. 300 Ereignisse — dann steht ≥"); `features.txt` Zeile (v0.185.0); `CHANGELOG.md` `## [0.185.0] - 2026-09-25` (Added); `docs/repo.md` Abschnitt „Aktivität 24 h / 7 Tage" (Fenster-Tabelle, Werte-Liste, GitHub-Grenzen, Commit-Zeitpunkt statt Autor-Datum); `CLAUDE.md`-Notiz unter dem repo-Abschnitt (halboffene Fenster, `%ct` am Ende des Formats, PushEvent ohne Commit-Zahl, Cap-Regel, `github.*`-Präfixe, `repo.auth` für den Token-Retry).
- [ ] **Step 2:** Version 0.184.0 → 0.185.0 in den neun Manifesten; `node scripts/gen-docs.mjs`; `bash scripts/check.sh`; `pnpm update-badges` (führt beide Suiten aus).
- [ ] **Step 3:** `bash scripts/install-macos.sh`; Live-Probe aus Task 2 Step 6 wiederholen.
- [ ] **Step 4:** Selektiv stagen (eigene Hunks), prüfen dass keine dezibel-Datei dabei ist, Commit `docs(repo): v0.185.0 — 24 h / 7 d activity`, `git push`.
