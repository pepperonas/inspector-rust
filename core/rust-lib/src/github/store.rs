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

use super::{should_snapshot, Account, AccountTotals, Point, Repo, RepoNumbers};
use rusqlite::{params, Connection, OptionalExtension};

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
        CREATE INDEX IF NOT EXISTS idx_github_repository_snapshots ON github_repository_snapshots(github_repository_id, ts);",
    )
}

pub fn upsert_account(conn: &Connection, a: &Account, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO github_accounts (github_user_id, login, name, avatar_url, html_url, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
         ON CONFLICT(github_user_id) DO UPDATE SET login=?2, name=?3, avatar_url=?4, html_url=?5, updated_at=?6",
        params![a.id, a.login, a.name, a.avatar_url, a.html_url, now],
    )?;
    Ok(())
}

/// login, name, avatar url.
pub type Profile = (String, Option<String>, Option<String>);

pub fn account(conn: &Connection, user_id: i64) -> rusqlite::Result<Option<Profile>> {
    conn.query_row(
        "SELECT login, name, avatar_url FROM github_accounts WHERE github_user_id=?1",
        [user_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()
}

/// Store the current repositories: insert new ones, update renamed /
/// re-flagged ones by id, mark those missing from this fetch as gone.
pub fn upsert_repos(
    conn: &Connection,
    owner_id: i64,
    repos: &[Repo],
    now: i64,
) -> rusqlite::Result<()> {
    for r in repos {
        conn.execute(
            "INSERT INTO github_repositories (github_repository_id, owner_id, name, full_name, private, archived, fork,
                language, html_url, description, license, stars, forks, watchers, open_issues, updated_at_ms,
                pushed_at_ms, last_seen, gone)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,0)
             ON CONFLICT(github_repository_id) DO UPDATE SET owner_id=?2, name=?3, full_name=?4, private=?5,
                archived=?6, fork=?7, language=?8, html_url=?9, description=?10, license=?11, stars=?12, forks=?13,
                watchers=COALESCE(?14, watchers), open_issues=?15, updated_at_ms=?16, pushed_at_ms=?17,
                last_seen=?18, gone=0",
            params![
                r.id, owner_id, r.name, r.full_name, r.private, r.archived, r.fork, r.language, r.html_url,
                r.description, r.license, r.stars, r.forks, r.watchers, r.open_issues, r.updated_at_ms,
                r.pushed_at_ms, now
            ],
        )?;
    }
    conn.execute(
        "UPDATE github_repositories SET gone=1 WHERE owner_id=?1 AND last_seen < ?2",
        params![owner_id, now],
    )?;
    Ok(())
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
) -> rusqlite::Result<Recorded> {
    let tx = conn.transaction()?;
    upsert_account(&tx, account, now)?;
    upsert_repos(&tx, account.id, repos, now)?;
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
        let n = RepoNumbers::of(r);
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
    Ok(Repo {
        id: r.get(0)?,
        name: r.get(1)?,
        full_name: r.get(2)?,
        private: r.get(3)?,
        archived: r.get(4)?,
        fork: r.get(5)?,
        language: r.get(6)?,
        html_url: r.get(7)?,
        description: r.get(8)?,
        license: r.get(9)?,
        stars: r.get(10)?,
        forks: r.get(11)?,
        watchers: r.get(12)?,
        open_issues: r.get(13)?,
        updated_at_ms: r.get(14)?,
        pushed_at_ms: r.get(15)?,
    })
}

const REPO_COLS: &str =
    "github_repository_id, name, full_name, private, archived, fork, language, html_url,
    description, license, stars, forks, watchers, open_issues, updated_at_ms, pushed_at_ms";

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

/// Store the subscribers count from the detail endpoint.
pub fn set_watchers(conn: &Connection, repo_id: i64, watchers: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE github_repositories SET watchers=?2 WHERE github_repository_id=?1",
        params![repo_id, watchers],
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
            let repo = Repo {
                id: r.get(1)?,
                name: r.get(2)?,
                full_name: r.get(3)?,
                private: r.get(4)?,
                archived: r.get(5)?,
                fork: r.get(6)?,
                language: r.get(7)?,
                html_url: r.get(8)?,
                description: r.get(9)?,
                license: r.get(10)?,
                stars: r.get(11)?,
                forks: r.get(12)?,
                watchers: r.get(13)?,
                open_issues: r.get(14)?,
                updated_at_ms: r.get(15)?,
                pushed_at_ms: r.get(16)?,
            };
            let n = RepoNumbers {
                stars: r.get(17)?,
                forks: r.get(18)?,
                watchers: r.get(19)?,
                open_issues: r.get(20)?,
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
        }
    }

    #[test]
    fn snapshots_follow_the_policy_and_never_duplicate() {
        let mut c = db();
        let repos = [repo(1, "a", 10), repo(2, "b", 5)];
        let r = record(&mut c, &account(100), &repos, 0, DEFAULT_INTERVAL_MS).unwrap();
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
        record(&mut c, &account(120), &repos(300), 0, DEFAULT_INTERVAL_MS).unwrap();
        record(
            &mut c,
            &account(130),
            &repos(350),
            20 * D,
            DEFAULT_INTERVAL_MS,
        )
        .unwrap();
        record(
            &mut c,
            &account(142),
            &repos(384),
            40 * D,
            DEFAULT_INTERVAL_MS,
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
        )
        .unwrap();
        record(
            &mut c,
            &account(1),
            &[repo(1, "new-name", 12)],
            D,
            DEFAULT_INTERVAL_MS,
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
        )
        .unwrap();
        record(
            &mut c,
            &account(1),
            &[repo(1, "a", 1)],
            D,
            DEFAULT_INTERVAL_MS,
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
        )
        .unwrap();
        assert_eq!(current_repos(&c, 7).unwrap().len(), 2);
    }

    #[test]
    fn private_flag_follows_github_and_watchers_survive_list_refreshes() {
        let mut c = db();
        let mut r = repo(1, "a", 1);
        r.private = true;
        record(&mut c, &account(1), &[r.clone()], 0, DEFAULT_INTERVAL_MS).unwrap();
        set_watchers(&c, 1, 8).unwrap();
        r.private = false;
        record(&mut c, &account(1), &[r], D, DEFAULT_INTERVAL_MS).unwrap();
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
        )
        .unwrap();
        record(
            &mut c,
            &account(1),
            &[repo(1, "new", 11)],
            D,
            DEFAULT_INTERVAL_MS,
        )
        .unwrap();
        let rows = all_repo_snapshots(&c, 7).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|(_, r, _)| r.name == "new"));
        assert_eq!(rows[1].2.stars, 11);
    }
}
