//! GitHub repository URL detection (v0.183.0). Pure; MIRRORS
//! `core/frontend/src/lib/repo-url.ts`. Both sides run the SAME fixture file
//! (`core/frontend/src/lib/repo-url-cases.json`) so they cannot drift.
//!
//! Only a repo counts (issue/PR links, profiles, gists and GitHub system
//! paths are rejected); `/tree/…` and `/blob/…` collapse to the repo. Owner
//! and repo are checked against GitHub's character rules, so a parsed value
//! can never carry a git option (`-…`) or a path escape (`..`).

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RepoUrl {
    pub owner: String,
    pub repo: String,
    pub web_url: String,
    pub clone_url: String,
}

const RESERVED_OWNERS: [&str; 29] = [
    "features", "orgs", "organizations", "marketplace", "settings", "login", "join",
    "topics", "explore", "sponsors", "about", "pricing", "enterprise", "collections",
    "trending", "notifications", "new", "apps", "search", "security", "site", "contact",
    "customer-stories", "readme", "codespaces", "issues", "pulls", "discussions", "events",
];

fn valid_owner(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 39
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-')
}

fn valid_repo(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 100
        && b[0] != b'-'
        && s != "."
        && s != ".."
        && b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

fn strip_prefix_ci<'a>(s: &'a str, p: &str) -> Option<&'a str> {
    if s.len() >= p.len() && s[..p.len()].eq_ignore_ascii_case(p) {
        Some(&s[p.len()..])
    } else {
        None
    }
}

pub fn parse_repo_url(text: &str) -> Option<RepoUrl> {
    let t = text.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) {
        return None;
    }
    let path: &str = if let Some(rest) = strip_prefix_ci(t, "git@github.com:") {
        rest
    } else {
        let mut s = t;
        if let Some(r) = strip_prefix_ci(s, "https://") {
            s = r;
        } else if let Some(r) = strip_prefix_ci(s, "http://") {
            s = r;
        }
        if let Some(r) = strip_prefix_ci(s, "www.") {
            s = r;
        }
        let rest = strip_prefix_ci(s, "github.com")?;
        if !rest.starts_with('/') {
            return None; // `github.com.evil.io/…` or a bare host
        }
        rest
    };
    let path = path.split(['?', '#']).next().unwrap_or("");
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() < 2 {
        return None;
    }
    if segs.len() > 2 && segs[2] != "tree" && segs[2] != "blob" {
        return None;
    }
    let owner = segs[0];
    let repo = segs[1];
    let repo = if repo.len() > 4 && repo[repo.len() - 4..].eq_ignore_ascii_case(".git") {
        &repo[..repo.len() - 4]
    } else {
        repo
    };
    if RESERVED_OWNERS.contains(&owner.to_ascii_lowercase().as_str()) {
        return None;
    }
    if !valid_owner(owner) || !valid_repo(repo) {
        return None;
    }
    Some(RepoUrl {
        owner: owner.to_string(),
        repo: repo.to_string(),
        web_url: format!("https://github.com/{owner}/{repo}"),
        clone_url: format!("https://github.com/{owner}/{repo}.git"),
    })
}

/// First repo URL among the whitespace-separated tokens of `text`.
/// Kept in lock-step with `findRepoUrl` (TS) for parity; the app calls the TS one.
#[allow(dead_code)]
pub fn find_repo_url(text: &str) -> Option<RepoUrl> {
    text.split_whitespace().find_map(|raw| {
        let tok = raw
            .trim_start_matches(['(', '"', '\'', '<', '['])
            .trim_end_matches([')', '"', '\'', '>', ']', ',', '.', ';', ':', '!', '?']);
        if !tok.to_ascii_lowercase().contains("github.com") {
            return None;
        }
        parse_repo_url(tok)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASES: &str = include_str!("../../frontend/src/lib/repo-url-cases.json");

    #[test]
    fn shared_fixture_matches() {
        let cases: Vec<serde_json::Value> = serde_json::from_str(CASES).unwrap();
        assert!(cases.len() >= 30, "fixture must not be empty");
        for c in cases {
            let input = c["input"].as_str().unwrap();
            let got = parse_repo_url(input);
            match c["expect"].as_array() {
                None => assert_eq!(got, None, "input {input:?}"),
                Some(e) => {
                    let (o, r) = (e[0].as_str().unwrap(), e[1].as_str().unwrap());
                    let got = got.unwrap_or_else(|| panic!("expected a repo for {input:?}"));
                    assert_eq!((got.owner.as_str(), got.repo.as_str()), (o, r), "input {input:?}");
                    assert_eq!(got.clone_url, format!("https://github.com/{o}/{r}.git"));
                }
            }
        }
    }

    #[test]
    fn find_trims_punctuation_and_skips_non_repo_links() {
        assert_eq!(find_repo_url("siehe (https://github.com/o/r).").unwrap().repo, "r");
        assert_eq!(
            find_repo_url("https://github.com/o/r/issues/1 und https://github.com/a/b").unwrap().owner,
            "a"
        );
        assert_eq!(find_repo_url("kein link"), None);
    }
}
