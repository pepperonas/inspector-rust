//! Clone cache + clone-to-folder for the `repo` command (v0.183.0).
//!
//! The pure helpers (target name, cache eviction, progress parsing, auth
//! environment, error classes) are unit-tested; the git/filesystem shell
//! below them is thin. Token handling: the token is passed to git ONLY as
//! `GIT_CONFIG_*` environment variables (never argv — argv is world-visible
//! in `ps`), only for `https://github.com/`, and is redacted from anything
//! that could reach the UI or the log.

use std::path::{Path, PathBuf};

pub const CACHE_MAX_REPOS: usize = 5;
pub const CACHE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// `base`, else `base (2)`, `base (3)` … — the first free name from 2 wins
/// (like Finder, so a gap is filled). `None` after `(999)`.
pub fn free_dir_name(base: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    if !exists(base) {
        return Some(base.to_string());
    }
    (2..=999).map(|n| format!("{base} ({n})")).find(|c| !exists(c))
}

#[derive(Clone, Debug, PartialEq)]
pub struct CacheEntry {
    pub key: String,
    pub mtime_ms: u64,
    pub bytes: u64,
}

/// Keys to delete so at most `max_n` repos / `max_bytes` remain. Newest wins;
/// `keep` (the repo just analysed) is never evicted and counts first.
pub fn cache_evictions(entries: &[CacheEntry], max_n: usize, max_bytes: u64, keep: &str) -> Vec<String> {
    let mut sorted: Vec<&CacheEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| {
        (b.key == keep).cmp(&(a.key == keep)).then(b.mtime_ms.cmp(&a.mtime_ms)).then(a.key.cmp(&b.key))
    });
    let (mut n, mut bytes) = (0usize, 0u64);
    let mut out = Vec::new();
    for e in sorted {
        if e.key == keep {
            n += 1;
            bytes = bytes.saturating_add(e.bytes);
            continue;
        }
        if n + 1 > max_n || bytes.saturating_add(e.bytes) > max_bytes {
            out.push(e.key.clone());
        } else {
            n += 1;
            bytes += e.bytes;
        }
    }
    out
}

/// One line of `git … --progress` stderr → (German phase, percent).
pub fn parse_git_progress(line: &str) -> Option<(String, u8)> {
    let l = line.trim().trim_start_matches("remote:").trim();
    let (phase, rest) = l.split_once(':')?;
    let phase_de = match phase.trim() {
        "Counting objects" | "Enumerating objects" => "Zähle Objekte",
        "Compressing objects" => "Komprimiere",
        "Receiving objects" => "Empfange Objekte",
        "Resolving deltas" => "Löse Deltas auf",
        "Updating files" | "Checking out files" => "Checke Dateien aus",
        _ => return None,
    };
    let pct_str = rest.trim_start().split('%').next()?.trim();
    let pct: u16 = pct_str.parse().ok()?;
    if pct > 100 {
        return None;
    }
    Some((phase_de.to_string(), pct as u8))
}

/// Environment for every git call of this feature. Token (if any) becomes an
/// `http.extraheader` for github.com ONLY, via `GIT_CONFIG_*` — never argv.
pub fn auth_env(token: Option<&str>) -> Vec<(String, String)> {
    use base64::Engine;
    let mut v = vec![("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())];
    if let Some(t) = token.map(str::trim).filter(|t| !t.is_empty()) {
        let basic = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{t}"));
        v.push(("GIT_CONFIG_COUNT".into(), "1".into()));
        v.push(("GIT_CONFIG_KEY_0".into(), "http.https://github.com/.extraheader".into()));
        v.push(("GIT_CONFIG_VALUE_0".into(), format!("AUTHORIZATION: basic {basic}")));
    }
    v
}

pub fn clone_args(url: &str, dest: &str, no_checkout: bool) -> Vec<String> {
    let mut a = vec!["clone".to_string()];
    if no_checkout {
        a.push("--no-checkout".into());
    }
    a.extend(["--progress", "--", url, dest].map(String::from));
    a
}

/// Remove the token and its header form from text bound for UI/log.
pub fn redact(text: &str, token: Option<&str>) -> String {
    use base64::Engine;
    let Some(t) = token.map(str::trim).filter(|t| !t.is_empty()) else {
        return text.to_string();
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{t}"));
    text.replace(&b64, "***").replace(t, "***")
}

pub fn classify_git_error(stderr: &str) -> &'static str {
    let s = stderr.to_ascii_lowercase();
    if s.contains("repository not found")
        || s.contains("authentication failed")
        || s.contains("could not read username")
        || s.contains("error: 403")
        || s.contains("error: 401")
    {
        "repo.auth"
    } else if s.contains("could not resolve host")
        || s.contains("failed to connect")
        || s.contains("network is unreachable")
        || s.contains("timed out")
    {
        "repo.network"
    } else {
        "repo.git"
    }
}

/// `~/claude` if it exists, else Downloads.
pub fn default_clone_dir(
    home: Option<&Path>,
    downloads: Option<&Path>,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if let Some(h) = home {
        let c = h.join("claude");
        if exists(&c) {
            return Some(c);
        }
    }
    downloads.map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn free_dir_name_counts_from_two_and_takes_the_first_free() {
        let taken: HashSet<&str> = ["repo", "repo (2)"].into_iter().collect();
        assert_eq!(free_dir_name("repo", |n| taken.contains(n)).as_deref(), Some("repo (3)"));
        assert_eq!(free_dir_name("repo", |_| false).as_deref(), Some("repo"));
        // A gap is filled, like Finder: (2) free although (3) exists.
        let gap: HashSet<&str> = ["repo", "repo (3)"].into_iter().collect();
        assert_eq!(free_dir_name("repo", |n| gap.contains(n)).as_deref(), Some("repo (2)"));
        // Cap at 999.
        assert_eq!(free_dir_name("repo", |_| true), None);
    }

    fn e(key: &str, mtime_ms: u64, bytes: u64) -> CacheEntry {
        CacheEntry { key: key.into(), mtime_ms, bytes }
    }

    #[test]
    fn cache_evictions_drop_the_oldest_beyond_count_and_size() {
        let entries = vec![e("a", 1, 10), e("b", 2, 10), e("c", 3, 10), e("d", 4, 10)];
        assert_eq!(cache_evictions(&entries, 2, u64::MAX, "d"), vec!["b".to_string(), "a".to_string()]);
        // Size cap: newest-first keep until the budget runs out.
        assert_eq!(cache_evictions(&entries, 10, 25, "d"), vec!["b".to_string(), "a".to_string()]);
        // The just-analysed repo is never evicted — even when it alone blows
        // the budget; then everything else goes.
        let big = vec![e("old", 1, 1000), e("new", 9, 1)];
        assert_eq!(cache_evictions(&big, 5, 100, "old"), vec!["new".to_string()]);
        assert!(cache_evictions(&[], 5, 100, "x").is_empty());
    }

    #[test]
    fn parse_git_progress_reads_real_git_lines() {
        assert_eq!(
            parse_git_progress("Receiving objects:  42% (420/1000), 1.20 MiB | 2.30 MiB/s"),
            Some(("Empfange Objekte".to_string(), 42))
        );
        assert_eq!(
            parse_git_progress("remote: Counting objects: 100% (10/10), done."),
            Some(("Zähle Objekte".to_string(), 100))
        );
        assert_eq!(parse_git_progress("Resolving deltas:   7% (3/40)"), Some(("Löse Deltas auf".to_string(), 7)));
        assert_eq!(parse_git_progress("Cloning into bare repository 'x'..."), None);
        assert_eq!(parse_git_progress("Receiving objects: 999% (1/1)"), None);
    }

    #[test]
    fn auth_env_never_puts_the_token_in_argv_and_always_disables_prompts() {
        let env = auth_env(Some("ghp_SECRET"));
        assert!(env.contains(&("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())));
        assert!(env.iter().any(|(k, v)| k == "GIT_CONFIG_KEY_0" && v == "http.https://github.com/.extraheader"));
        let header = env.iter().find(|(k, _)| k == "GIT_CONFIG_VALUE_0").unwrap().1.clone();
        assert!(header.starts_with("AUTHORIZATION: basic "));
        assert!(!header.contains("ghp_SECRET"), "token must be base64-encoded, not raw");
        let args = clone_args("https://github.com/o/r.git", "/tmp/x", true);
        assert!(args.iter().all(|a| !a.contains("ghp_SECRET")));
        // Without a token: prompt disabled, no auth header.
        let none = auth_env(None);
        assert_eq!(none, vec![("GIT_TERMINAL_PROMPT".to_string(), "0".to_string())]);
        assert_eq!(auth_env(Some("  ")), none);
    }

    #[test]
    fn clone_args_put_the_url_after_the_end_of_options_marker() {
        let a = clone_args("https://github.com/o/r.git", "/tmp/dest", true);
        assert_eq!(a, vec!["clone", "--no-checkout", "--progress", "--", "https://github.com/o/r.git", "/tmp/dest"]);
        let b = clone_args("https://github.com/o/r.git", "/tmp/dest", false);
        assert_eq!(b, vec!["clone", "--progress", "--", "https://github.com/o/r.git", "/tmp/dest"]);
    }

    #[test]
    fn redact_removes_token_and_its_base64_form() {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode("x-access-token:ghp_X");
        let s = format!("fatal: ghp_X and {b64}");
        let r = redact(&s, Some("ghp_X"));
        assert!(!r.contains("ghp_X") && !r.contains(&b64), "{r}");
        assert_eq!(redact("plain", None), "plain");
    }

    #[test]
    fn classify_git_error_names_auth_and_network() {
        assert_eq!(classify_git_error("remote: Repository not found.\nfatal: repository 'x' not found"), "repo.auth");
        assert_eq!(classify_git_error("fatal: could not read Username for 'https://github.com'"), "repo.auth");
        assert_eq!(classify_git_error("The requested URL returned error: 403"), "repo.auth");
        assert_eq!(classify_git_error("fatal: unable to access '…': Could not resolve host: github.com"), "repo.network");
        assert_eq!(classify_git_error("fatal: something else"), "repo.git");
    }

    #[test]
    fn default_clone_dir_prefers_claude_then_downloads() {
        let home = Path::new("/Users/u");
        let dl = Path::new("/Users/u/Downloads");
        assert_eq!(
            default_clone_dir(Some(home), Some(dl), |p| p == Path::new("/Users/u/claude")),
            Some(PathBuf::from("/Users/u/claude"))
        );
        assert_eq!(default_clone_dir(Some(home), Some(dl), |_| false), Some(dl.to_path_buf()));
        assert_eq!(default_clone_dir(None, None, |_| false), None);
    }
}
