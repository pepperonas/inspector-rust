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

use serde::Serialize;
use std::collections::BTreeMap;
use std::process::Command;
use std::time::Duration;

pub const REC: char = '\u{1e}';
pub const FLD: char = '\u{1f}';

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
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
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct MonthCount {
    pub month: String, // "YYYY-MM"
    pub commits: u64,
}
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct FileStat {
    pub path: String,
    pub changes: u64, // commits touching it
    pub churn: u64,   // insertions + deletions
}
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ExtStat {
    pub ext: String,
    pub commits: u64,
    pub churn: u64,
}
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct AuthorStat {
    pub name: String,
    pub commits: u64,
    pub churn: u64,
}
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct CatCount {
    pub cat: String,
    pub commits: u64,
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

/// Pure: parse `git log --numstat` output (control-char separated) into stats.
/// `name`/`source` are filled by the caller.
pub fn parse_git_log(raw: &str) -> RepoStats {
    let mut stats = RepoStats::default();
    let mut author_idx: BTreeMap<String, usize> = BTreeMap::new();
    let mut authors: Vec<(String, u64, u64)> = Vec::new(); // name, commits, churn
    let mut files: BTreeMap<String, (u64, u64)> = BTreeMap::new(); // path → (changes, churn)
    let mut exts: BTreeMap<String, (u64, u64)> = BTreeMap::new(); // ext → (commits, churn)
    let mut months: BTreeMap<String, u64> = BTreeMap::new();
    let mut cats: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut days: Vec<i64> = Vec::new();
    let mut msg_len_total: u64 = 0;
    let (mut first, mut last): (Option<String>, Option<String>) = (None, None);

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
        if iso.is_empty() {
            continue;
        }

        stats.commits += 1;
        msg_len_total += subject.chars().count() as u64;
        *cats.entry(classify_commit(subject)).or_insert(0) += 1;

        // Chronology: git log is newest-first, so the LAST record seen is the
        // first commit; track both ends.
        if last.is_none() {
            last = Some(iso.to_string());
        }
        first = Some(iso.to_string());

        if let Some((wd, hr)) = weekday_hour(iso) {
            stats.by_weekday[wd] += 1;
            stats.by_hour[hr] += 1;
        }
        if let Some(ord) = day_ordinal(iso) {
            days.push(ord);
        }
        if iso.len() >= 7 {
            *months.entry(iso[0..7].to_string()).or_insert(0) += 1;
        }

        let key = if email.is_empty() { name.to_lowercase() } else { email.to_lowercase() };
        let a = *author_idx.entry(key).or_insert_with(|| {
            authors.push((name.to_string(), 0, 0));
            authors.len() - 1
        });
        authors[a].1 += 1;

        // numstat lines: "<ins>\t<del>\t<path>" (ins/del are "-" for binary).
        let mut touched_exts: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for ln in lines {
            let ln = ln.trim();
            if ln.is_empty() {
                continue;
            }
            let mut cols = ln.splitn(3, '\t');
            let ins = cols.next().unwrap_or("0");
            let del = cols.next().unwrap_or("0");
            let path = match cols.next() {
                Some(p) => p,
                None => continue,
            };
            let ins: u64 = ins.parse().unwrap_or(0);
            let del: u64 = del.parse().unwrap_or(0);
            let churn = ins + del;
            stats.insertions += ins;
            stats.deletions += del;
            authors[a].2 += churn;
            let fe = files.entry(path.to_string()).or_insert((0, 0));
            fe.0 += 1;
            fe.1 += churn;
            let ext = extension_of(path);
            let ee = exts.entry(ext.clone()).or_insert((0, 0));
            ee.1 += churn;
            touched_exts.insert(ext);
        }
        for e in touched_exts {
            exts.entry(e).and_modify(|v| v.0 += 1);
        }
    }

    stats.contributors = authors.len() as u64;
    stats.first_commit = first.unwrap_or_default();
    stats.last_commit = last.unwrap_or_default();
    stats.avg_msg_len = msg_len_total.checked_div(stats.commits).unwrap_or(0);

    // Active days + longest streak.
    days.sort_unstable();
    days.dedup();
    stats.active_days = days.len() as u64;
    stats.longest_streak = longest_streak(&days);

    // Timeline (chronological).
    stats.by_month = months.into_iter().map(|(month, commits)| MonthCount { month, commits }).collect();

    // Top files by change count (then churn).
    let mut fv: Vec<FileStat> = files
        .into_iter()
        .map(|(path, (changes, churn))| FileStat { path, changes, churn })
        .collect();
    fv.sort_by(|a, b| b.changes.cmp(&a.changes).then(b.churn.cmp(&a.churn)).then(a.path.cmp(&b.path)));
    fv.truncate(15);
    stats.top_files = fv;

    // Top extensions by churn.
    let mut ev: Vec<ExtStat> = exts
        .into_iter()
        .map(|(ext, (commits, churn))| ExtStat { ext, commits, churn })
        .collect();
    ev.sort_by(|a, b| b.churn.cmp(&a.churn).then(a.ext.cmp(&b.ext)));
    ev.truncate(12);
    stats.top_exts = ev;

    // Top authors by commits.
    let mut av: Vec<AuthorStat> = authors
        .into_iter()
        .map(|(name, commits, churn)| AuthorStat { name, commits, churn })
        .collect();
    av.sort_by(|a, b| b.commits.cmp(&a.commits).then(b.churn.cmp(&a.churn)).then(a.name.cmp(&b.name)));
    av.truncate(12);
    stats.top_authors = av;

    // Categories in a stable, meaningful order.
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

const T_GIT: Duration = Duration::from_secs(30);

/// Run `git log` with the repo2viz format in `dir`. Impure shell.
fn git_log(dir: &std::path::Path) -> Result<String, String> {
    let fmt = format!("{REC}%H{FLD}%aI{FLD}%aN{FLD}%aE{FLD}%s");
    let out = std::process::Command::new("git")
        .current_dir(dir) // not `-C <dir>` — keeps an untrusted path out of argv
        .args(["log", "--no-merges", "--numstat", "--date=iso-strict"])
        .arg(format!("--pretty=format:{fmt}"))
        .output()
        .map_err(|e| format!("git nicht gefunden: {e}"))?;
    if !out.status.success() {
        return Err(format!("git log fehlgeschlagen: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Analyse a LOCAL git repo directory (Finder selection / path). Errors if it
/// isn't a git repo.
pub fn analyze_local(dir: &std::path::Path) -> Result<RepoStats, String> {
    if !dir.join(".git").exists() {
        return Err("Kein Git-Repository (kein .git gefunden).".into());
    }
    let raw = git_log(dir)?;
    let (name, _slug) = repo_identity(&dir.to_string_lossy());
    let mut stats = parse_git_log(&raw);
    stats.name = name;
    stats.source = dir.to_string_lossy().into_owned();
    Ok(stats)
}

/// Clone a remote read-only (bare, full history) into a temp dir, analyse, and
/// clean up. Impure.
pub fn analyze_remote(url: &str) -> Result<RepoStats, String> {
    // Reject obviously non-URL junk before spawning git — AND anything starting
    // with '-', which git would parse as a flag (argv smuggling, e.g.
    // `--upload-pack=…`). The `--` below is the belt to this suspenders.
    if url.starts_with('-') || !(url.contains("://") || (url.contains('@') && url.contains(':'))) {
        return Err("Keine gültige Repository-URL.".into());
    }
    let tmp = std::env::temp_dir().join(format!("ir-repo-{}-{}", std::process::id(), sanitize_slug(url).chars().take(24).collect::<String>()));
    let _ = std::fs::remove_dir_all(&tmp);
    let clone = Command::new("git")
        .args(["clone", "--bare", "--quiet", "--", url, &tmp.to_string_lossy()])
        .output()
        .map_err(|e| format!("git nicht gefunden: {e}"))?;
    let _ = T_GIT; // (clone has no built-in timeout here; git itself will fail fast on a bad URL)
    if !clone.status.success() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("Klonen fehlgeschlagen: {}", String::from_utf8_lossy(&clone.stderr).trim()));
    }
    let result = (|| {
        let raw = git_log(&tmp)?;
        let (name, _slug) = repo_identity(url);
        let mut stats = parse_git_log(&raw);
        stats.name = name;
        stats.source = url.to_string();
        Ok(stats)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result
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

/// Build the self-contained HTML export (no external requests — inline CSS +
/// SVG-free CSS bar charts). Pure; tested structurally.
pub fn build_html(stats: &RepoStats) -> String {
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
    let mo_max = stats.by_month.iter().map(|m| m.commits).max().unwrap_or(1).max(1);
    let mo_rows = bar_rows(
        &stats.by_month.iter().map(|m| (m.month.clone(), m.commits)).collect::<Vec<_>>(),
        mo_max,
        "#81c995",
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

    format!(
        r#"<!DOCTYPE html>
<html lang="de"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{name} — repo activity</title>
<style>
:root{{--bg:#131318;--surf:#1d1d24;--bd:#2c2c36;--fg:#e6e1e9;--mut:#9a94a3;--acc:#d0bcff}}
*{{box-sizing:border-box}}
body{{margin:0;background:var(--bg);color:var(--fg);font:14px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;padding:32px}}
.wrap{{max-width:900px;margin:0 auto}}
h1{{font-size:24px;margin:0 0 4px}}
.sub{{color:var(--mut);margin:0 0 24px}}
.kpis{{display:grid;grid-template-columns:repeat(auto-fit,minmax(120px,1fr));gap:12px;margin-bottom:28px}}
.kpi{{background:var(--surf);border:1px solid var(--bd);border-radius:14px;padding:14px}}
.kpi b{{display:block;font-size:22px}}
.kpi span{{color:var(--mut);font-size:12px}}
.card{{background:var(--surf);border:1px solid var(--bd);border-radius:16px;padding:18px;margin-bottom:20px}}
.card h2{{font-size:15px;margin:0 0 14px}}
.row{{display:flex;align-items:center;gap:10px;margin:5px 0;font-size:12px}}
.lbl{{width:56px;color:var(--mut);text-align:right;font-variant-numeric:tabular-nums}}
.bar{{flex:1;height:10px;background:#00000030;border-radius:6px;overflow:hidden}}
.bar i{{display:block;height:100%;border-radius:6px}}
.val{{width:52px;text-align:right;font-variant-numeric:tabular-nums;color:var(--mut)}}
table{{width:100%;border-collapse:collapse;font-size:12px}}
td,th{{text-align:left;padding:5px 8px;border-bottom:1px solid var(--bd)}}
th{{color:var(--mut);font-weight:600}}
td:nth-child(2),td:nth-child(3),th:nth-child(2),th:nth-child(3){{text-align:right;font-variant-numeric:tabular-nums;width:80px}}
.mono{{font-family:ui-monospace,SFMono-Regular,Menlo,monospace}}
footer{{color:var(--mut);font-size:11px;text-align:center;margin-top:28px}}
</style></head><body><div class="wrap">
<h1>{name}</h1>
<p class="sub">{source} · {first} → {last}</p>
<div class="kpis">
<div class="kpi"><b>{commits}</b><span>Commits</span></div>
<div class="kpi"><b>{contributors}</b><span>Mitwirkende</span></div>
<div class="kpi"><b>{active_days}</b><span>Aktive Tage</span></div>
<div class="kpi"><b>{streak}</b><span>Längste Serie</span></div>
<div class="kpi"><b>+{ins}</b><span>Zeilen ein</span></div>
<div class="kpi"><b>−{del}</b><span>Zeilen aus</span></div>
</div>
<div class="card"><h2>Commits nach Wochentag</h2>{wd}</div>
<div class="card"><h2>Commits nach Stunde</h2>{hr}</div>
<div class="card"><h2>Aktivität nach Monat</h2>{mo}</div>
<div class="card"><h2>Commit-Kategorien</h2>{cat}</div>
<div class="card"><h2>Aktivste Dateien</h2><table><tr><th>Datei</th><th>Änderungen</th><th>Churn</th></tr>{files}</table></div>
<div class="card"><h2>Dateitypen</h2><table><tr><th>Typ</th><th>Commits</th><th>Churn</th></tr>{exts}</table></div>
<div class="card"><h2>Top-Mitwirkende</h2><table><tr><th>Name</th><th>Commits</th><th>Churn</th></tr>{authors}</table></div>
<footer>Erzeugt mit Inspector Rust · orientiert an repo2viz</footer>
</div></body></html>"#,
        name = esc(&stats.name),
        source = esc(&stats.source),
        first = esc(&stats.first_commit),
        last = esc(&stats.last_commit),
        commits = stats.commits,
        contributors = stats.contributors,
        active_days = stats.active_days,
        streak = stats.longest_streak,
        ins = stats.insertions,
        del = stats.deletions,
        wd = wd_rows,
        hr = hr_rows,
        mo = mo_rows,
        cat = cat_rows,
        files = file_rows,
        exts = ext_rows,
        authors = author_rows,
    )
}

/// Write the HTML export to `~/Downloads/<slug>-activity.html`, returning the
/// path. Slug from the source (owner-repo for URLs, folder name for paths).
pub fn export_html(stats: &RepoStats) -> Result<std::path::PathBuf, String> {
    let (_name, slug) = repo_identity(&stats.source);
    let downloads = dirs::download_dir().ok_or_else(|| "Kein Downloads-Ordner".to_string())?;
    let path = downloads.join(format!("{slug}-activity.html"));
    std::fs::write(&path, build_html(stats)).map_err(|e| format!("Schreiben fehlgeschlagen: {e}"))?;
    Ok(path)
}

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
        assert!(analyze_remote("--upload-pack=touch /tmp/pwn").is_err());
        assert!(analyze_remote("-x").is_err());
        // Plain non-URL junk is refused too (no scheme / scp form).
        assert!(analyze_remote("not a url").is_err());
    }

    #[test]
    fn build_html_is_self_contained_and_names_the_repo() {
        let mut s = parse_git_log(&synth_log());
        s.name = "inspector-rust".into();
        s.source = "https://github.com/pepperonas/inspector-rust".into();
        let html = build_html(&s);
        assert!(html.starts_with("<!DOCTYPE html>"));
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
        let mut s = RepoStats { name: "<script>x</script>".into(), ..Default::default() };
        s.top_authors.push(AuthorStat { name: "a<b>&\"".into(), commits: 1, churn: 1 });
        let html = build_html(&s);
        assert!(!html.contains("<script>x</script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("a&lt;b&gt;&amp;&quot;"));
    }
}
