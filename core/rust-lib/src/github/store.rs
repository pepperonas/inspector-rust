//! GitHub tables in `history.db`. Repositories are keyed by GitHub's stable
//! id: a renamed repository keeps its history, a deleted one is marked `gone`
//! (its history stays), a private/public switch just updates the flag.
//!
//! Snapshot storage (policy in `super::should_snapshot`):
//! * account — one per refresh interval, or when a number changed;
//! * repository — only when its numbers changed (or the first time), so 50
//!   repositories don't add 50 rows an hour. The value at any time is the
//!   newest snapshot before it; the first snapshot marks where observation
//!   starts — earlier there is no history, not a zero.

use super::{should_snapshot, Account, AccountTotals, EventKind, ExtraKind, Point, Repo, RepoNumbers};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS github_accounts (
            github_user_id INTEGER PRIMARY KEY,
            login TEXT NOT NULL,
            name TEXT,
            avatar_url TEXT,
            html_url TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS github_repositories (
            github_repository_id INTEGER PRIMARY KEY,
            owner_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            full_name TEXT NOT NULL,
            private INTEGER NOT NULL DEFAULT 0,
            archived INTEGER NOT NULL DEFAULT 0,
            fork INTEGER NOT NULL DEFAULT 0,
            language TEXT,
            html_url TEXT NOT NULL DEFAULT '',
            description TEXT,
            license TEXT,
            stars INTEGER NOT NULL DEFAULT 0,
            forks INTEGER NOT NULL DEFAULT 0,
            watchers INTEGER,
            open_issues INTEGER NOT NULL DEFAULT 0,
            updated_at_ms INTEGER,
            pushed_at_ms INTEGER,
            last_seen INTEGER NOT NULL,
            gone INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_github_repositories_owner ON github_repositories(owner_id);
        CREATE TABLE IF NOT EXISTS github_account_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            github_user_id INTEGER NOT NULL,
            ts INTEGER NOT NULL,
            followers INTEGER NOT NULL,
            following INTEGER NOT NULL,
            public_repos INTEGER NOT NULL,
            total_stars INTEGER NOT NULL,
            total_forks INTEGER NOT NULL,
            repo_count INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_github_account_snapshots ON github_account_snapshots(github_user_id, ts);
        CREATE TABLE IF NOT EXISTS github_repository_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            github_repository_id INTEGER NOT NULL,
            ts INTEGER NOT NULL,
            stars INTEGER NOT NULL,
            forks INTEGER NOT NULL,
            watchers INTEGER,
            open_issues INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_github_repository_snapshots ON github_repository_snapshots(github_repository_id, ts);
        CREATE TABLE IF NOT EXISTS github_events (
            github_repository_id INTEGER NOT NULL,
            kind TEXT NOT NULL,
            ts INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_github_events ON github_events(github_repository_id, kind, ts);
        CREATE TABLE IF NOT EXISTS github_backfill (
            github_repository_id INTEGER NOT NULL,
            kind TEXT NOT NULL,
            synced_ms INTEGER NOT NULL,
            synced_value INTEGER NOT NULL,
            count INTEGER NOT NULL,
            truncated INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (github_repository_id, kind)
        );
        CREATE TABLE IF NOT EXISTS github_repo_extras (
            github_repository_id INTEGER NOT NULL,
            kind TEXT NOT NULL,
            fetched_ms INTEGER NOT NULL,
            json TEXT NOT NULL,
            PRIMARY KEY (github_repository_id, kind)
        );",
    )?;
    // Columns added after 0.210 — additive, an existing column is fine.
    for sql in [
        "ALTER TABLE github_accounts ADD COLUMN gh_created_ms INTEGER",
        "ALTER TABLE github_accounts ADD COLUMN bio TEXT",
        "ALTER TABLE github_accounts ADD COLUMN company TEXT",
        "ALTER TABLE github_accounts ADD COLUMN location TEXT",
        "ALTER TABLE github_accounts ADD COLUMN blog TEXT",
        "ALTER TABLE github_accounts ADD COLUMN hireable INTEGER",
        "ALTER TABLE github_accounts ADD COLUMN public_gists INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE github_accounts ADD COLUMN public_repos INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE github_accounts ADD COLUMN private_repos INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE github_accounts ADD COLUMN followers INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE github_accounts ADD COLUMN following INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE github_repositories ADD COLUMN created_at_ms INTEGER",
        "ALTER TABLE github_repositories ADD COLUMN size_kb INTEGER",
        "ALTER TABLE github_repositories ADD COLUMN topics TEXT",
        "ALTER TABLE github_repositories ADD COLUMN homepage TEXT",
        "ALTER TABLE github_repositories ADD COLUMN default_branch TEXT",
        "ALTER TABLE github_repositories ADD COLUMN watchers_checked_ms INTEGER",
    ] {
        let _ = conn.execute(sql, []);
    }
    Ok(())
}

pub fn upsert_account(conn: &Connection, a: &Account, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO github_accounts (github_user_id, login, name, avatar_url, html_url, created_at, updated_at,
            gh_created_ms, bio, company, location, blog, hireable, public_gists, public_repos, private_repos,
            followers, following)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
         ON CONFLICT(github_user_id) DO UPDATE SET login=?2, name=?3, avatar_url=?4, html_url=?5, updated_at=?6,
            gh_created_ms=?7, bio=?8, company=?9, location=?10, blog=?11, hireable=?12, public_gists=?13,
            public_repos=?14, private_repos=?15, followers=?16, following=?17",
        params![
            a.id, a.login, a.name, a.avatar_url, a.html_url, now, a.created_at_ms, a.bio, a.company,
            a.location, a.blog, a.hireable, a.public_gists, a.public_repos, a.private_repos, a.followers,
            a.following
        ],
    )?;
    Ok(())
}

/// The stored account (profile + the counts of the last refresh).
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct Profile {
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub html_url: Option<String>,
    pub created_at_ms: Option<i64>,
    pub bio: Option<String>,
    pub company: Option<String>,
    pub location: Option<String>,
    pub blog: Option<String>,
    pub hireable: Option<bool>,
    pub public_gists: i64,
    pub public_repos: i64,
    pub private_repos: i64,
    pub followers: i64,
    pub following: i64,
}

pub fn account(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<Profile>> {
    conn.query_row(
        "SELECT login, name, avatar_url, html_url, gh_created_ms, bio, company, location, blog, hireable,
            public_gists, public_repos, private_repos, followers, following
         FROM github_accounts WHERE github_user_id=?1",
        [user_id],
        |r| {
            Ok(Profile {
                login: r.get(0)?,
                name: r.get(1)?,
                avatar_url: r.get(2)?,
                html_url: r.get(3)?,
                created_at_ms: r.get(4)?,
                bio: r.get(5)?,
                company: r.get(6)?,
                location: r.get(7)?,
                blog: r.get(8)?,
                hireable: r.get(9)?,
                public_gists: r.get(10)?,
                public_repos: r.get(11)?,
                private_repos: r.get(12)?,
                followers: r.get(13)?,
                following: r.get(14)?,
            })
        },
    )
    .optional()
}

/// Store the current repositories: insert new ones, update renamed /
/// re-flagged ones by id, mark those missing from this fetch as gone — but
/// only when the fetch was `complete`: a list cut at the page cap says
/// nothing about the repositories beyond it.
pub fn upsert_repos(
    conn: &Connection,
    owner_id: i64,
    repos: &[Repo],
    now: i64,
    complete: bool,
) -> rusqlite::Result<()> {
    for r in repos {
        let topics = serde_json::to_string(&r.topics).unwrap_or_else(|_| "[]".into());
        conn.execute(
            "INSERT INTO github_repositories (github_repository_id, owner_id, name, full_name, private, archived, fork,
                language, html_url, description, license, stars, forks, watchers, open_issues, updated_at_ms,
                pushed_at_ms, last_seen, gone, created_at_ms, size_kb, topics, homepage, default_branch)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,0,?19,?20,?21,?22,?23)
             ON CONFLICT(github_repository_id) DO UPDATE SET owner_id=?2, name=?3, full_name=?4, private=?5,
                archived=?6, fork=?7, language=?8, html_url=?9, description=?10, license=?11, stars=?12, forks=?13,
                watchers=COALESCE(?14, watchers), open_issues=?15, updated_at_ms=?16, pushed_at_ms=?17,
                last_seen=?18, gone=0, created_at_ms=COALESCE(?19, created_at_ms), size_kb=?20, topics=?21,
                homepage=?22, default_branch=?23",
            params![
                r.id, owner_id, r.name, r.full_name, r.private, r.archived, r.fork, r.language, r.html_url,
                r.description, r.license, r.stars, r.forks, r.watchers, r.open_issues, r.updated_at_ms,
                r.pushed_at_ms, now, r.created_at_ms, r.size_kb, topics, r.homepage, r.default_branch
            ],
        )?;
    }
    if complete {
        conn.execute(
            "UPDATE github_repositories SET gone=1 WHERE owner_id=?1 AND last_seen < ?2",
            params![owner_id, now],
        )?;
    }
    Ok(())
}

/// The stored watchers count (list refreshes don't carry it).
fn stored_watchers(conn: &Connection, repo_id: i64) -> rusqlite::Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT watchers FROM github_repositories WHERE github_repository_id=?1",
            [repo_id],
            |r| r.get::<_, Option<i64>>(0),
        )
        .optional()?
        .flatten())
}

pub fn last_account_snapshot(
    conn: &Connection,
    user_id: i64,
) -> rusqlite::Result<Option<(i64, AccountTotals)>> {
    conn.query_row(
        "SELECT ts, followers, following, public_repos, total_stars, total_forks, repo_count
         FROM github_account_snapshots WHERE github_user_id=?1 ORDER BY ts DESC LIMIT 1",
        [user_id],
        |r| {
            Ok((
                r.get(0)?,
                AccountTotals {
                    followers: r.get(1)?,
                    following: r.get(2)?,
                    public_repos: r.get(3)?,
                    total_stars: r.get(4)?,
                    total_forks: r.get(5)?,
                    repo_count: r.get(6)?,
                },
            ))
        },
    )
    .optional()
}

fn last_repo_numbers(conn: &Connection, repo_id: i64) -> rusqlite::Result<Option<RepoNumbers>> {
    conn.query_row(
        "SELECT stars, forks, watchers, open_issues FROM github_repository_snapshots
         WHERE github_repository_id=?1 ORDER BY ts DESC LIMIT 1",
        [repo_id],
        |r| {
            Ok(RepoNumbers {
                stars: r.get(0)?,
                forks: r.get(1)?,
                watchers: r.get(2)?,
                open_issues: r.get(3)?,
            })
        },
    )
    .optional()
}

/// What one refresh stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Recorded {
    pub account_snapshot: bool,
    pub repo_snapshots: usize,
}

/// Store a fetched state: account + repositories, and snapshots according to
/// the policy. One transaction — a crash never leaves half a snapshot.
pub fn record(
    conn: &mut Connection,
    account: &Account,
    repos: &[Repo],
    now: i64,
    interval_ms: i64,
    complete: bool,
) -> rusqlite::Result<Recorded> {
    let tx = conn.transaction()?;
    upsert_account(&tx, account, now)?;
    upsert_repos(&tx, account.id, repos, now, complete)?;
    let totals = super::totals(account, repos);
    let mut out = Recorded::default();
    if should_snapshot(
        last_account_snapshot(&tx, account.id)?,
        &totals,
        now,
        interval_ms,
    ) {
        tx.execute(
            "INSERT INTO github_account_snapshots (github_user_id, ts, followers, following, public_repos,
                total_stars, total_forks, repo_count) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![account.id, now, totals.followers, totals.following, totals.public_repos,
                totals.total_stars, totals.total_forks, totals.repo_count],
        )?;
        out.account_snapshot = true;
    }
    for r in repos {
        let mut n = RepoNumbers::of(r);
        // The list has no watchers: carry the stored count, or every
        // snapshot after a list refresh would break the watchers series.
        if n.watchers.is_none() {
            n.watchers = stored_watchers(&tx, r.id)?;
        }
        let changed = last_repo_numbers(&tx, r.id)?.is_none_or(|prev| n.differs(&prev));
        if changed {
            tx.execute(
                "INSERT INTO github_repository_snapshots (github_repository_id, ts, stars, forks, watchers, open_issues)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![r.id, now, n.stars, n.forks, n.watchers, n.open_issues],
            )?;
            out.repo_snapshots += 1;
        }
    }
    tx.commit()?;
    Ok(out)
}

/// Every account snapshot (oldest first) — the export.
pub fn account_snapshots(
    conn: &Connection,
    user_id: i64,
) -> rusqlite::Result<Vec<(i64, AccountTotals)>> {
    let mut stmt = conn.prepare(
        "SELECT ts, followers, following, public_repos, total_stars, total_forks, repo_count
         FROM github_account_snapshots WHERE github_user_id=?1 ORDER BY ts",
    )?;
    let rows = stmt
        .query_map([user_id], |r| {
            Ok((
                r.get(0)?,
                AccountTotals {
                    followers: r.get(1)?,
                    following: r.get(2)?,
                    public_repos: r.get(3)?,
                    total_stars: r.get(4)?,
                    total_forks: r.get(5)?,
                    repo_count: r.get(6)?,
                },
            ))
        })?
        .collect();
    rows
}

/// An account metric over time (oldest first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountField {
    Followers,
    TotalStars,
    TotalForks,
    RepoCount,
}

impl AccountField {
    fn column(self) -> &'static str {
        match self {
            Self::Followers => "followers",
            Self::TotalStars => "total_stars",
            Self::TotalForks => "total_forks",
            Self::RepoCount => "repo_count",
        }
    }
}

pub fn account_series(
    conn: &Connection,
    user_id: i64,
    field: AccountField,
) -> rusqlite::Result<Vec<Point>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT ts, {} FROM github_account_snapshots WHERE github_user_id=?1 ORDER BY ts",
        field.column()
    ))?;
    let rows = stmt
        .query_map([user_id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect();
    rows
}

/// A repository's stars over time (oldest first).
pub fn repo_series(conn: &Connection, repo_id: i64) -> rusqlite::Result<Vec<(i64, RepoNumbers)>> {
    let mut stmt = conn.prepare(
        "SELECT ts, stars, forks, watchers, open_issues FROM github_repository_snapshots
         WHERE github_repository_id=?1 ORDER BY ts",
    )?;
    let rows = stmt
        .query_map([repo_id], |r| {
            Ok((
                r.get(0)?,
                RepoNumbers {
                    stars: r.get(1)?,
                    forks: r.get(2)?,
                    watchers: r.get(3)?,
                    open_issues: r.get(4)?,
                },
            ))
        })?
        .collect();
    rows
}

fn repo_from_row(r: &rusqlite::Row) -> rusqlite::Result<Repo> {
    repo_at(r, 0)
}

/// A repository from `REPO_COLS`, starting at column `o`.
fn repo_at(r: &rusqlite::Row, o: usize) -> rusqlite::Result<Repo> {
    let topics: Option<String> = r.get(o + 16)?;
    Ok(Repo {
        id: r.get(o)?,
        name: r.get(o + 1)?,
        full_name: r.get(o + 2)?,
        private: r.get(o + 3)?,
        archived: r.get(o + 4)?,
        fork: r.get(o + 5)?,
        language: r.get(o + 6)?,
        html_url: r.get(o + 7)?,
        description: r.get(o + 8)?,
        license: r.get(o + 9)?,
        stars: r.get(o + 10)?,
        forks: r.get(o + 11)?,
        watchers: r.get(o + 12)?,
        open_issues: r.get(o + 13)?,
        updated_at_ms: r.get(o + 14)?,
        pushed_at_ms: r.get(o + 15)?,
        topics: topics
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default(),
        created_at_ms: r.get(o + 17)?,
        size_kb: r.get(o + 18)?,
        homepage: r.get(o + 19)?,
        default_branch: r.get(o + 20)?,
    })
}

const REPO_COLS: &str =
    "github_repository_id, name, full_name, private, archived, fork, language, html_url,
    description, license, stars, forks, watchers, open_issues, updated_at_ms, pushed_at_ms,
    topics, created_at_ms, size_kb, homepage, default_branch";
const REPO_COL_COUNT: usize = 21;

/// The account's current repositories (deleted ones excluded).
pub fn current_repos(conn: &Connection, owner_id: i64) -> rusqlite::Result<Vec<Repo>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {REPO_COLS} FROM github_repositories WHERE owner_id=?1 AND gone=0"
    ))?;
    let rows = stmt.query_map([owner_id], repo_from_row)?.collect();
    rows
}

/// A repository by name, full name or id (case-insensitive) — renamed ones
/// are found under their current name, their history under their id.
pub fn find_repo(conn: &Connection, owner_id: i64, query: &str) -> rusqlite::Result<Option<Repo>> {
    let q = query.trim().to_lowercase();
    conn.query_row(
        &format!(
            "SELECT {REPO_COLS} FROM github_repositories WHERE owner_id=?1
             AND (lower(name)=?2 OR lower(full_name)=?2 OR CAST(github_repository_id AS TEXT)=?2)
             ORDER BY gone LIMIT 1"
        ),
        params![owner_id, q],
        repo_from_row,
    )
    .optional()
}

/// Store the subscribers count from the single-repository endpoint. A
/// changed count gets a snapshot (with the other current numbers), so
/// watchers have a history like stars and forks. Returns whether it did.
pub fn record_watchers(
    conn: &mut Connection,
    repo_id: i64,
    watchers: i64,
    now: i64,
) -> rusqlite::Result<bool> {
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE github_repositories SET watchers=?2, watchers_checked_ms=?3 WHERE github_repository_id=?1",
        params![repo_id, watchers, now],
    )?;
    let current: Option<(i64, i64, i64)> = tx
        .query_row(
            "SELECT stars, forks, open_issues FROM github_repositories WHERE github_repository_id=?1",
            [repo_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let mut wrote = false;
    if let Some((stars, forks, open_issues)) = current {
        let last = last_repo_numbers(&tx, repo_id)?;
        if last.is_none_or(|l| l.watchers != Some(watchers)) {
            tx.execute(
                "INSERT INTO github_repository_snapshots (github_repository_id, ts, stars, forks, watchers, open_issues)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![repo_id, now, stars, forks, watchers, open_issues],
            )?;
            wrote = true;
        }
    }
    tx.commit()?;
    Ok(wrote)
}

/// Repositories whose watchers are due for a check (never, or older than
/// `older_than`), the most-starred first. Full names.
pub fn watchers_due(
    conn: &Connection,
    owner_id: i64,
    now: i64,
    older_than: i64,
    limit: usize,
) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT github_repository_id, full_name FROM github_repositories
         WHERE owner_id=?1 AND gone=0 AND (watchers_checked_ms IS NULL OR watchers_checked_ms <= ?2)
         ORDER BY watchers_checked_ms IS NOT NULL, stars DESC, github_repository_id LIMIT ?3",
    )?;
    let rows = stmt
        .query_map(params![owner_id, now - older_than, limit as i64], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect();
    rows
}

// ── Event lists (reconstructed history) ──────────────────────────────────────

/// Replace one repository's event list after a COMPLETE read, and remember
/// for which count it was read. One transaction: a crash never leaves half
/// a list.
pub fn replace_events(
    conn: &mut Connection,
    repo_id: i64,
    kind: EventKind,
    times: &[i64],
    value: i64,
    truncated: bool,
    now: i64,
) -> rusqlite::Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM github_events WHERE github_repository_id=?1 AND kind=?2",
        params![repo_id, kind.as_str()],
    )?;
    {
        let mut ins = tx.prepare(
            "INSERT INTO github_events (github_repository_id, kind, ts) VALUES (?1, ?2, ?3)",
        )?;
        for t in times {
            ins.execute(params![repo_id, kind.as_str(), t])?;
        }
    }
    tx.execute(
        "INSERT INTO github_backfill (github_repository_id, kind, synced_ms, synced_value, count, truncated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(github_repository_id, kind) DO UPDATE SET synced_ms=?3, synced_value=?4, count=?5, truncated=?6",
        params![repo_id, kind.as_str(), now, value, times.len() as i64, truncated],
    )?;
    tx.commit()
}

/// One repository's event times, oldest first.
pub fn events(conn: &Connection, repo_id: i64, kind: EventKind) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT ts FROM github_events WHERE github_repository_id=?1 AND kind=?2 ORDER BY ts",
    )?;
    let rows = stmt
        .query_map(params![repo_id, kind.as_str()], |r| r.get(0))?
        .collect();
    rows
}

/// Every current repository's events of one kind, oldest first — the
/// account-wide reconstruction.
pub fn owner_events(conn: &Connection, owner_id: i64, kind: EventKind) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT e.ts FROM github_events e JOIN github_repositories r
           ON r.github_repository_id = e.github_repository_id
         WHERE r.owner_id=?1 AND r.gone=0 AND e.kind=?2 ORDER BY e.ts",
    )?;
    let rows = stmt
        .query_map(params![owner_id, kind.as_str()], |r| r.get(0))?
        .collect();
    rows
}

/// What was read for a repository list: (synced_ms, synced_value, count, truncated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Synced {
    pub synced_ms: i64,
    pub synced_value: i64,
    pub count: i64,
    pub truncated: bool,
}

pub fn synced(conn: &Connection, repo_id: i64, kind: EventKind) -> rusqlite::Result<Option<Synced>> {
    conn.query_row(
        "SELECT synced_ms, synced_value, count, truncated FROM github_backfill
         WHERE github_repository_id=?1 AND kind=?2",
        params![repo_id, kind.as_str()],
        |r| {
            Ok(Synced {
                synced_ms: r.get(0)?,
                synced_value: r.get(1)?,
                count: r.get(2)?,
                truncated: r.get(3)?,
            })
        },
    )
    .optional()
}

/// The backfill candidates of an owner: every current repository (a fork's
/// stars are the owner's too — leaving them out makes the reconstructed
/// total fall short of the real one), both lists, with what was read so far.
pub fn backfill_candidates(
    conn: &Connection,
    owner_id: i64,
) -> rusqlite::Result<Vec<super::BackfillCandidate>> {
    let repos = current_repos(conn, owner_id)?;
    let mut out = Vec::new();
    for r in &repos {
        for kind in EventKind::ALL {
            let s = synced(conn, r.id, kind)?;
            out.push(super::BackfillCandidate {
                repo_id: r.id,
                full_name: r.full_name.clone(),
                kind,
                value: kind.value_of(r),
                synced_value: s.map(|s| s.synced_value),
                synced_ms: s.map(|s| s.synced_ms),
            });
        }
    }
    Ok(out)
}

// ── Repository extras cache ─────────────────────────────────────────────────

/// A cached extra: (fetched_ms, raw JSON).
pub fn extra(conn: &Connection, repo_id: i64, kind: ExtraKind) -> rusqlite::Result<Option<(i64, String)>> {
    conn.query_row(
        "SELECT fetched_ms, json FROM github_repo_extras WHERE github_repository_id=?1 AND kind=?2",
        params![repo_id, kind.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

pub fn put_extra(
    conn: &Connection,
    repo_id: i64,
    kind: ExtraKind,
    json: &str,
    now: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO github_repo_extras (github_repository_id, kind, fetched_ms, json) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(github_repository_id, kind) DO UPDATE SET fetched_ms=?3, json=?4",
        params![repo_id, kind.as_str(), now, json],
    )?;
    Ok(())
}

/// Every repository snapshot of an owner (export), with the CURRENT name.
pub fn all_repo_snapshots(
    conn: &Connection,
    owner_id: i64,
) -> rusqlite::Result<Vec<(i64, Repo, RepoNumbers)>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT s.ts, {cols}, s.stars, s.forks, s.watchers, s.open_issues
         FROM github_repository_snapshots s JOIN github_repositories r
           ON r.github_repository_id = s.github_repository_id
         WHERE r.owner_id=?1 ORDER BY s.ts, r.name",
        cols = REPO_COLS
            .split(',')
            .map(|c| format!("r.{}", c.trim()))
            .collect::<Vec<_>>()
            .join(", ")
    ))?;
    let rows = stmt
        .query_map([owner_id], |r| {
            let ts: i64 = r.get(0)?;
            let repo = repo_at(r, 1)?;
            let o = 1 + REPO_COL_COUNT;
            let n = RepoNumbers {
                stars: r.get(o)?,
                forks: r.get(o + 1)?,
                watchers: r.get(o + 2)?,
                open_issues: r.get(o + 3)?,
            };
            Ok((ts, repo, n))
        })?
        .collect();
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{delta, DEFAULT_INTERVAL_MS};

    const H: i64 = 3_600_000;
    const D: i64 = 86_400_000;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        init_schema(&c).unwrap();
        c
    }

    fn account(followers: i64) -> Account {
        Account {
            id: 7,
            login: "pepperonas".into(),
            name: None,
            avatar_url: None,
            html_url: None,
            followers,
            following: 2,
            public_repos: 2,
            ..Default::default()
        }
    }

    fn repo(id: i64, name: &str, stars: i64) -> Repo {
        Repo {
            id,
            name: name.into(),
            full_name: format!("pepperonas/{name}"),
            private: false,
            archived: false,
            fork: false,
            language: None,
            html_url: String::new(),
            description: None,
            license: None,
            stars,
            forks: 1,
            watchers: None,
            open_issues: 0,
            updated_at_ms: None,
            pushed_at_ms: None,
            ..Default::default()
        }
    }

    #[test]
    fn snapshots_follow_the_policy_and_never_duplicate() {
        let mut c = db();
        let repos = [repo(1, "a", 10), repo(2, "b", 5)];
        let r = record(&mut c, &account(100), &repos, 0, DEFAULT_INTERVAL_MS, true).unwrap();
        assert_eq!(
            r,
            Recorded {
                account_snapshot: true,
                repo_snapshots: 2
            }
        );
        // Ten minutes later, nothing changed: nothing stored.
        let r = record(
            &mut c,
            &account(100),
            &repos,
            10 * 60_000,
            DEFAULT_INTERVAL_MS,
            true,
        )
        .unwrap();
        assert_eq!(r, Recorded::default());
        // A star on one repo: account + that one repo.
        let changed = [repo(1, "a", 11), repo(2, "b", 5)];
        let r = record(
            &mut c,
            &account(100),
            &changed,
            20 * 60_000,
            DEFAULT_INTERVAL_MS,
            true,
        )
        .unwrap();
        assert_eq!(
            r,
            Recorded {
                account_snapshot: true,
                repo_snapshots: 1
            }
        );
        // An hour after that, unchanged: the regular account snapshot only.
        let r = record(
            &mut c,
            &account(100),
            &changed,
            20 * 60_000 + H,
            DEFAULT_INTERVAL_MS,
            true,
        )
        .unwrap();
        assert_eq!(
            r,
            Recorded {
                account_snapshot: true,
                repo_snapshots: 0
            }
        );
        assert_eq!(
            account_series(&c, 7, AccountField::TotalStars)
                .unwrap()
                .len(),
            3
        );
    }

    #[test]
    fn follower_and_star_deltas_from_stored_snapshots() {
        let mut c = db();
        let repos = |s: i64| [repo(1, "a", s)];
        record(&mut c, &account(120), &repos(300), 0, DEFAULT_INTERVAL_MS, true).unwrap();
        record(
            &mut c,
            &account(130),
            &repos(350),
            20 * D,
            DEFAULT_INTERVAL_MS,
            true,
        )
        .unwrap();
        record(
            &mut c,
            &account(142),
            &repos(384),
            40 * D,
            DEFAULT_INTERVAL_MS,
            true,
        )
        .unwrap();
        let f = account_series(&c, 7, AccountField::Followers).unwrap();
        let s = account_series(&c, 7, AccountField::TotalStars).unwrap();
        let now = 40 * D;
        assert_eq!(delta(&f, now, 7 * D), Some(12));
        assert_eq!(delta(&f, now, 30 * D), Some(22));
        assert_eq!(delta(&s, now, 30 * D), Some(84));
        assert_eq!(
            delta(&f, now, 90 * D),
            None,
            "history doesn't reach back 90 days"
        );
    }

    #[test]
    fn a_renamed_repository_keeps_its_history() {
        let mut c = db();
        record(
            &mut c,
            &account(1),
            &[repo(1, "old-name", 10)],
            0,
            DEFAULT_INTERVAL_MS,
            true,
        )
        .unwrap();
        record(
            &mut c,
            &account(1),
            &[repo(1, "new-name", 12)],
            D,
            DEFAULT_INTERVAL_MS,
            true,
        )
        .unwrap();
        let repos = current_repos(&c, 7).unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "new-name");
        assert!(find_repo(&c, 7, "old-name").unwrap().is_none());
        let found = find_repo(&c, 7, "NEW-NAME").unwrap().unwrap();
        assert_eq!(
            repo_series(&c, found.id).unwrap().len(),
            2,
            "both snapshots under one id"
        );
        assert!(find_repo(&c, 7, "pepperonas/new-name").unwrap().is_some());
    }

    #[test]
    fn deleted_repositories_are_marked_gone_and_keep_their_history() {
        let mut c = db();
        record(
            &mut c,
            &account(1),
            &[repo(1, "a", 1), repo(2, "b", 2)],
            0,
            DEFAULT_INTERVAL_MS,
            true,
        )
        .unwrap();
        record(
            &mut c,
            &account(1),
            &[repo(1, "a", 1)],
            D,
            DEFAULT_INTERVAL_MS,
            true,
        )
        .unwrap();
        assert_eq!(current_repos(&c, 7).unwrap().len(), 1);
        assert_eq!(repo_series(&c, 2).unwrap().len(), 1);
        // It comes back (made public again): not gone any more.
        record(
            &mut c,
            &account(1),
            &[repo(1, "a", 1), repo(2, "b", 2)],
            2 * D,
            DEFAULT_INTERVAL_MS,
            true,
        )
        .unwrap();
        assert_eq!(current_repos(&c, 7).unwrap().len(), 2);
    }

    #[test]
    fn private_flag_follows_github_and_watchers_survive_list_refreshes() {
        let mut c = db();
        let mut r = repo(1, "a", 1);
        r.private = true;
        record(&mut c, &account(1), &[r.clone()], 0, DEFAULT_INTERVAL_MS, true).unwrap();
        record_watchers(&mut c, 1, 8, 1).unwrap();
        r.private = false;
        record(&mut c, &account(1), &[r], D, DEFAULT_INTERVAL_MS, true).unwrap();
        let cur = &current_repos(&c, 7).unwrap()[0];
        assert!(!cur.private);
        assert_eq!(
            cur.watchers,
            Some(8),
            "a list refresh (no watchers) doesn't erase it"
        );
    }

    #[test]
    fn export_rows_carry_the_current_name() {
        let mut c = db();
        record(
            &mut c,
            &account(1),
            &[repo(1, "old", 10)],
            0,
            DEFAULT_INTERVAL_MS,
            true,
        )
        .unwrap();
        record(
            &mut c,
            &account(1),
            &[repo(1, "new", 11)],
            D,
            DEFAULT_INTERVAL_MS,
            true,
        )
        .unwrap();
        let rows = all_repo_snapshots(&c, 7).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|(_, r, _)| r.name == "new"));
        assert_eq!(rows[1].2.stars, 11);
    }

    #[test]
    fn a_cut_repository_list_marks_nothing_as_gone() {
        let mut c = db();
        record(&mut c, &account(1), &[repo(1, "a", 1), repo(2, "b", 1)], 0, DEFAULT_INTERVAL_MS, true)
            .unwrap();
        // The page cap cut the list: "b" is missing but not deleted.
        record(&mut c, &account(1), &[repo(1, "a", 1)], D, DEFAULT_INTERVAL_MS, false).unwrap();
        assert_eq!(current_repos(&c, 7).unwrap().len(), 2);
        // A complete list does mark it.
        record(&mut c, &account(1), &[repo(1, "a", 1)], 2 * D, DEFAULT_INTERVAL_MS, true).unwrap();
        assert_eq!(current_repos(&c, 7).unwrap().len(), 1);
    }

    #[test]
    fn watchers_get_a_history_and_list_refreshes_keep_it() {
        let mut c = db();
        record(&mut c, &account(1), &[repo(1, "a", 10)], 0, DEFAULT_INTERVAL_MS, true).unwrap();
        assert!(record_watchers(&mut c, 1, 4, H).unwrap(), "first count: a snapshot");
        assert!(!record_watchers(&mut c, 1, 4, 2 * H).unwrap(), "unchanged: none");
        assert!(record_watchers(&mut c, 1, 6, 3 * H).unwrap());
        // A star arrives through the list (no watchers there): the snapshot
        // carries the stored watchers instead of a gap.
        record(&mut c, &account(1), &[repo(1, "a", 11)], 4 * H, DEFAULT_INTERVAL_MS, true).unwrap();
        let s = repo_series(&c, 1).unwrap();
        let w: Vec<Option<i64>> = s.iter().map(|(_, n)| n.watchers).collect();
        assert_eq!(w, vec![None, Some(4), Some(6), Some(6)]);
        // Checked repositories wait a day; the rest come first.
        record(&mut c, &account(1), &[repo(1, "a", 11), repo(2, "b", 3)], 5 * H, DEFAULT_INTERVAL_MS, true)
            .unwrap();
        let due = watchers_due(&c, 7, 5 * H, D, 5).unwrap();
        assert_eq!(due, vec![(2, "pepperonas/b".to_string())]);
    }

    #[test]
    fn event_lists_are_replaced_whole_and_feed_the_owner_history() {
        let mut c = db();
        record(&mut c, &account(1), &[repo(1, "a", 3), repo(2, "b", 1)], 0, DEFAULT_INTERVAL_MS, true)
            .unwrap();
        replace_events(&mut c, 1, EventKind::Stars, &[30, 10, 20], 3, false, 100).unwrap();
        replace_events(&mut c, 2, EventKind::Stars, &[15], 1, false, 100).unwrap();
        assert_eq!(events(&c, 1, EventKind::Stars).unwrap(), vec![10, 20, 30]);
        assert_eq!(owner_events(&c, 7, EventKind::Stars).unwrap(), vec![10, 15, 20, 30]);
        // A re-read replaces (a removed star disappears), never appends.
        replace_events(&mut c, 1, EventKind::Stars, &[10, 30], 2, false, 200).unwrap();
        assert_eq!(events(&c, 1, EventKind::Stars).unwrap(), vec![10, 30]);
        let s = synced(&c, 1, EventKind::Stars).unwrap().unwrap();
        assert_eq!((s.synced_ms, s.synced_value, s.count, s.truncated), (200, 2, 2, false));
        assert!(synced(&c, 1, EventKind::Forks).unwrap().is_none());
        // Candidates: own repositories, both lists.
        let cands = backfill_candidates(&c, 7).unwrap();
        assert_eq!(cands.len(), 4);
        // A deleted repository leaves the owner history.
        record(&mut c, &account(1), &[repo(1, "a", 2)], D, DEFAULT_INTERVAL_MS, true).unwrap();
        assert_eq!(owner_events(&c, 7, EventKind::Stars).unwrap(), vec![10, 30]);
    }

    #[test]
    fn extras_cache_round_trip() {
        let c = db();
        assert!(extra(&c, 1, ExtraKind::Languages).unwrap().is_none());
        put_extra(&c, 1, ExtraKind::Languages, "{\"Rust\":1}", 5).unwrap();
        put_extra(&c, 1, ExtraKind::Languages, "{\"Rust\":2}", 6).unwrap();
        assert_eq!(extra(&c, 1, ExtraKind::Languages).unwrap(), Some((6, "{\"Rust\":2}".into())));
    }
}
