//! GitHub statistics export — JSON (everything) and CSV (repository
//! snapshots). Pure: the caller passes the rows. Private repositories are
//! left out unless the user asked for them explicitly; credentials are never
//! part of the input, so they cannot end up in a file. Timestamps are UTC
//! ISO 8601 (`2026-10-01T12:00:00Z`).

use super::{AccountTotals, Repo, RepoNumbers};
use chrono::{TimeZone, Utc};

pub fn iso(ms: i64) -> String {
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(|t| t.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

/// Everything an export may contain.
pub struct ExportData<'a> {
    pub login: &'a str,
    pub current: Option<AccountTotals>,
    pub account_history: &'a [(i64, AccountTotals)],
    pub repos: &'a [Repo],
    pub repo_history: &'a [(i64, Repo, RepoNumbers)],
}

pub const CSV_HEADER: &str = "timestamp,repository,stars,forks,watchers,open_issues";

fn cell(s: &str) -> String {
    let s = if s.starts_with(['=', '+', '-', '@']) {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

/// Repository snapshots, oldest first.
pub fn csv(d: &ExportData, include_private: bool) -> String {
    let mut out = String::from(CSV_HEADER);
    out.push('\n');
    for (ts, repo, n) in d
        .repo_history
        .iter()
        .filter(|(_, r, _)| include_private || !r.private)
    {
        out.push_str(&format!(
            "{},{},{},{},{},{}\n",
            iso(*ts),
            cell(&repo.full_name),
            n.stars,
            n.forks,
            n.watchers.map(|w| w.to_string()).unwrap_or_default(),
            n.open_issues
        ));
    }
    out
}

pub fn json(d: &ExportData, include_private: bool, exported_at: i64) -> String {
    let keep = |r: &Repo| include_private || !r.private;
    let repos: Vec<_> = d
        .repos
        .iter()
        .filter(|r| keep(r))
        .map(|r| {
            serde_json::json!({
                "github_repository_id": r.id,
                "name": r.name,
                "full_name": r.full_name,
                "private": r.private,
                "archived": r.archived,
                "fork": r.fork,
                "language": r.language,
                "stars": r.stars,
                "forks": r.forks,
                "watchers": r.watchers,
                "open_issues": r.open_issues,
                "updated_at": r.updated_at_ms.map(iso),
                "html_url": r.html_url,
            })
        })
        .collect();
    let account_history: Vec<_> = d
        .account_history
        .iter()
        .map(|(ts, t)| {
            serde_json::json!({
                "timestamp": iso(*ts),
                "followers": t.followers,
                "following": t.following,
                "public_repos": t.public_repos,
                "total_stars": t.total_stars,
                "total_forks": t.total_forks,
            })
        })
        .collect();
    let repo_history: Vec<_> = d
        .repo_history
        .iter()
        .filter(|(_, r, _)| keep(r))
        .map(|(ts, r, n)| {
            serde_json::json!({
                "timestamp": iso(*ts),
                "github_repository_id": r.id,
                "repository": r.full_name,
                "stars": n.stars,
                "forks": n.forks,
                "watchers": n.watchers,
                "open_issues": n.open_issues,
            })
        })
        .collect();
    let private_left_out = !include_private && d.repos.iter().any(|r| r.private);
    let doc = serde_json::json!({
        "source": "Inspector Rust — GitHub analytics",
        "version": 1,
        "exported_at": iso(exported_at),
        "login": d.login,
        "account": d.current.map(|t| serde_json::json!({
            "followers": t.followers,
            "following": t.following,
            "public_repos": t.public_repos,
            "total_stars": t.total_stars,
            "total_forks": t.total_forks,
        })),
        "private_repositories_included": include_private,
        "private_repositories_left_out": private_left_out,
        "repositories": repos,
        "account_history": account_history,
        "repository_history": repo_history,
    });
    serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(id: i64, name: &str, private: bool) -> Repo {
        Repo {
            id,
            name: name.into(),
            full_name: format!("pepperonas/{name}"),
            private,
            archived: false,
            fork: false,
            language: Some("Rust".into()),
            html_url: format!("https://github.com/pepperonas/{name}"),
            description: None,
            license: None,
            stars: 128,
            forks: 17,
            watchers: Some(8),
            open_issues: 3,
            updated_at_ms: Some(1_791_633_600_000),
            pushed_at_ms: None,
        }
    }

    const T: i64 = 1_790_856_000_000; // 2026-10-01T12:00:00Z

    fn n(stars: i64) -> RepoNumbers {
        RepoNumbers {
            stars,
            forks: 15,
            watchers: Some(8),
            open_issues: 3,
        }
    }

    #[test]
    fn csv_matches_the_documented_format_and_hides_private_repos() {
        let public = repo(1, "inspector-rust", false);
        let secret = repo(2, "secret-project", true);
        let hist = [
            (T, public.clone(), n(112)),
            (T, secret.clone(), n(1)),
            (T + 86_400_000, public.clone(), n(114)),
        ];
        let d = ExportData {
            login: "pepperonas",
            current: None,
            account_history: &[],
            repos: &[public, secret],
            repo_history: &hist,
        };
        let out = csv(&d, false);
        assert_eq!(
            out,
            "timestamp,repository,stars,forks,watchers,open_issues\n\
             2026-10-01T12:00:00Z,pepperonas/inspector-rust,112,15,8,3\n\
             2026-10-02T12:00:00Z,pepperonas/inspector-rust,114,15,8,3\n"
        );
        assert!(!out.contains("secret"));
        assert!(
            csv(&d, true).contains("secret-project"),
            "only on explicit request"
        );
    }

    #[test]
    fn json_is_structured_and_says_what_it_left_out() {
        let public = repo(1, "inspector-rust", false);
        let secret = repo(2, "secret-project", true);
        let totals = AccountTotals {
            followers: 142,
            following: 3,
            public_repos: 47,
            total_stars: 384,
            total_forks: 91,
            repo_count: 2,
        };
        let d = ExportData {
            login: "pepperonas",
            current: Some(totals),
            account_history: &[(T, totals)],
            repos: &[public.clone(), secret.clone()],
            repo_history: &[(T, secret, n(1)), (T, public, n(112))],
        };
        let v: serde_json::Value = serde_json::from_str(&json(&d, false, T)).unwrap();
        assert_eq!(v["account"]["followers"], 142);
        assert_eq!(v["repositories"].as_array().unwrap().len(), 1);
        assert_eq!(v["repository_history"].as_array().unwrap().len(), 1);
        assert_eq!(v["private_repositories_left_out"], true);
        assert_eq!(v["account_history"][0]["timestamp"], "2026-10-01T12:00:00Z");
        assert_eq!(v["repositories"][0]["github_repository_id"], 1);
        let text = json(&d, false, T);
        for forbidden in ["token", "secret-project", "Authorization"] {
            assert!(!text.contains(forbidden), "{forbidden} leaked");
        }
        let with: serde_json::Value = serde_json::from_str(&json(&d, true, T)).unwrap();
        assert_eq!(with["repositories"].as_array().unwrap().len(), 2);
    }
}
