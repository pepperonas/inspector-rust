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

const KEYRING_SERVICE: &str = "io.celox.inspector-rust";
const KEYRING_USER: &str = "github-token-v1";

pub fn cache_root() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("InspectorRust").join("repos"))
}

fn cache_dir_for(u: &crate::repo_url::RepoUrl) -> Result<PathBuf, String> {
    // owner/repo already passed GitHub's character rules (no `..`, no `/`).
    cache_root()
        .map(|r| r.join(&u.owner).join(&u.repo))
        .ok_or_else(|| "Kein Cache-Ordner".to_string())
}

/// Run git with `--progress` stderr parsed line by line (git separates
/// progress updates with `\r`). On failure: class sentinel + redacted tail.
fn run_git(
    cwd: Option<&Path>,
    args: &[String],
    token: Option<&str>,
    on_progress: &mut dyn FnMut(&str, u8),
) -> Result<(), String> {
    use std::io::Read;
    let mut cmd = std::process::Command::new("git");
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    cmd.args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
    for (k, v) in auth_env(token) {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|e| format!("git nicht gefunden: {e}"))?;
    let mut stderr = child.stderr.take().expect("piped");
    let mut buf = [0u8; 4096];
    let mut line = Vec::<u8>::new();
    let mut tail = String::new();
    loop {
        let n = stderr.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        for &b in &buf[..n] {
            if b == b'\r' || b == b'\n' {
                let l = String::from_utf8_lossy(&line).into_owned();
                if let Some((phase, pct)) = parse_git_progress(&l) {
                    on_progress(&phase, pct);
                } else if !l.trim().is_empty() {
                    tail.push_str(&l);
                    tail.push('\n');
                    if tail.len() > 4000 {
                        tail.drain(..tail.len() - 4000);
                    }
                }
                line.clear();
            } else {
                line.push(b);
            }
        }
    }
    let status = child.wait().map_err(|e| format!("git: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        let class = classify_git_error(&tail);
        Err(format!("{class}: {}", redact(tail.trim(), token)))
    }
}

pub(crate) fn ensure_cached_at(
    src: &str,
    dir: &Path,
    token: Option<&str>,
    on_progress: &mut dyn FnMut(&str, u8),
) -> Result<PathBuf, String> {
    if dir.join(".git").exists() {
        run_git(Some(dir), &["fetch", "--prune", "--progress", "origin"].map(String::from), token, on_progress)?;
        // Keep origin/HEAD pointing at the remote's default branch.
        let _ = run_git(Some(dir), &["remote", "set-head", "origin", "--auto"].map(String::from), token, &mut |_, _| {});
    } else {
        if let Some(p) = dir.parent() {
            std::fs::create_dir_all(p).map_err(|e| format!("Cache: {e}"))?;
        }
        let res = run_git(None, &clone_args(src, &dir.to_string_lossy(), true), token, on_progress);
        if res.is_err() {
            let _ = std::fs::remove_dir_all(dir);
        }
        res?;
    }
    // Touch for LRU eviction.
    let _ = filetime_touch(dir);
    Ok(dir.to_path_buf())
}

fn filetime_touch(dir: &Path) -> std::io::Result<()> {
    // Recreate a marker file; its mtime is the entry's "last used".
    std::fs::write(dir.join(".git/ir-last-used"), b"")
}


/// One lock per cache directory, process-wide. Every sequence that fills,
/// fetches, reads or copies a cache entry holds it — two panels (or a reopen
/// mid-clone) on the same repo otherwise race: the losing `git clone` failed
/// on "already exists" and its cleanup deleted the winner's directory.
fn cache_lock(dir: &Path) -> std::sync::Arc<parking_lot::Mutex<()>> {
    use std::collections::HashMap;
    use std::sync::{Arc, OnceLock};
    static LOCKS: OnceLock<parking_lot::Mutex<HashMap<PathBuf, Arc<parking_lot::Mutex<()>>>>> = OnceLock::new();
    LOCKS
        .get_or_init(Default::default)
        .lock()
        .entry(dir.to_path_buf())
        .or_default()
        .clone()
}

/// Ensure the cache entry (clone or fetch) and run `then` on it, all under
/// the entry's lock. A token GitHub rejects is retried once without it.
pub(crate) fn with_cached<T>(
    src: &str,
    dir: &Path,
    token: Option<&str>,
    on_progress: &mut dyn FnMut(&str, u8),
    then: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, String> {
    let lock = cache_lock(dir);
    let _held = lock.lock();
    retry_without_token(token, |t| ensure_cached_at(src, dir, t, &mut *on_progress))?;
    then(dir)
}

/// A stored token that expired or was revoked makes GitHub reject even
/// PUBLIC repos — so an auth failure with a token is retried once without it.
pub(crate) fn retry_without_token<T>(
    token: Option<&str>,
    mut f: impl FnMut(Option<&str>) -> Result<T, String>,
) -> Result<T, String> {
    match f(token) {
        Err(e) if token.is_some() && e.starts_with("repo.auth") => f(None),
        r => r,
    }
}

/// Analyse-side entry: ensure the cache and read its commits under the lock.
pub fn cached_commits(
    u: &crate::repo_url::RepoUrl,
    token: Option<&str>,
    on_progress: &mut dyn FnMut(&str, u8),
) -> Result<(PathBuf, Vec<crate::repo_stats::Commit>), String> {
    let dir = cache_dir_for(u)?;
    let env = auth_env(None); // `git log` is local — no token needed
    let commits = with_cached(&u.clone_url, &dir, token, on_progress, |d| {
        crate::repo_stats::commits_from_dir(d, Some("origin/HEAD"), &env)
    })?;
    Ok((dir, commits))
}

fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

/// Enforce CACHE_MAX_REPOS / CACHE_MAX_BYTES; `keep` is never removed.
pub fn prune_cache(keep: &Path) {
    let Some(root) = cache_root() else { return };
    let mut entries = Vec::new();
    for owner in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        for repo in std::fs::read_dir(owner.path()).into_iter().flatten().flatten() {
            let p = repo.path();
            let mtime_ms = std::fs::metadata(p.join(".git/ir-last-used"))
                .or_else(|_| std::fs::metadata(&p))
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            entries.push(CacheEntry { key: p.to_string_lossy().into_owned(), mtime_ms, bytes: dir_size(&p) });
        }
    }
    for k in cache_evictions(&entries, CACHE_MAX_REPOS, CACHE_MAX_BYTES, &keep.to_string_lossy()) {
        // Never delete an entry another analysis/clone is working in.
        let lock = cache_lock(Path::new(&k));
        let held = lock.try_lock();
        if held.is_some() {
            let _ = std::fs::remove_dir_all(&k);
        }
        drop(held);
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let t = e.file_type()?;
        let to = dst.join(e.file_name());
        if t.is_dir() {
            copy_dir_all(&e.path(), &to)?;
        } else if t.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(e.path())?, &to)?;
        } else {
            std::fs::copy(e.path(), &to)?;
        }
    }
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // APFS clone: near-free on the same volume. Falls back below.
        let ok = std::process::Command::new("cp")
            .arg("-cR")
            .arg(src)
            .arg(dst)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            return Ok(());
        }
        let _ = std::fs::remove_dir_all(dst);
    }
    copy_dir_all(src, dst).map_err(|e| format!("Kopieren fehlgeschlagen: {e}"))
}

pub(crate) fn clone_to_at(
    src: &str,
    repo_name: &str,
    cached: Option<&Path>,
    parent: &Path,
    token: Option<&str>,
    on_progress: &mut dyn FnMut(&str, u8),
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(parent).map_err(|e| format!("Zielordner {}: {e}", parent.display()))?;
    let name = free_dir_name(repo_name, |n| parent.join(n).exists())
        .ok_or_else(|| "Kein freier Ordnername (bis 999 belegt)".to_string())?;
    let dest = parent.join(&name);
    let res = (|| {
        match cached {
            Some(c) if c.join(".git").exists() => {
                on_progress("Kopiere aus dem Cache", 0);
                copy_tree(c, &dest)?;
                on_progress("Checke Dateien aus", 50);
                run_git(Some(&dest), &["reset", "--hard", "--quiet", "origin/HEAD"].map(String::from), token, &mut |_, _| {})?;
                let _ = std::fs::remove_file(dest.join(".git/ir-last-used"));
                on_progress("Checke Dateien aus", 100);
                Ok(())
            }
            _ => run_git(None, &clone_args(src, &dest.to_string_lossy(), false), token, on_progress),
        }
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(e);
    }
    Ok(dest)
}

pub fn clone_to(
    u: &crate::repo_url::RepoUrl,
    parent: &Path,
    token: Option<&str>,
    on_progress: &mut dyn FnMut(&str, u8),
) -> Result<PathBuf, String> {
    if let Some(c) = cache_dir_for(u).ok().filter(|d| d.join(".git").exists()) {
        // Freshen and copy under the entry's lock (see `cache_lock`).
        let mut prog = |p: &str, n: u8| on_progress(p, n);
        return with_cached(&u.clone_url, &c, token, &mut prog, |c| {
            clone_to_at(&u.clone_url, &u.repo, Some(c), parent, None, &mut |_, _| {})
        });
    }
    retry_without_token(token, |t| clone_to_at(&u.clone_url, &u.repo, None, parent, t, &mut *on_progress))
}

fn gh_path() -> Option<PathBuf> {
    ["/opt/homebrew/bin/gh", "/usr/local/bin/gh", "/usr/bin/gh"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

pub fn gh_available() -> bool {
    gh_path().is_some()
}

/// `gh auth token` first (the user is already logged in there), else the
/// keychain entry from Settings. Never logged.
pub fn github_token() -> Option<String> {
    if let Some(gh) = gh_path() {
        if let Ok(out) = std::process::Command::new(gh).args(["auth", "token"]).output() {
            let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if out.status.success() && !t.is_empty() {
                return Some(t);
            }
        }
    }
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .ok()?
        .get_password()
        .ok()
        .filter(|t| !t.trim().is_empty())
}

pub fn set_stored_token(t: &str) -> Result<(), String> {
    let e = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| e.to_string())?;
    if t.trim().is_empty() {
        let _ = e.delete_credential();
        return Ok(());
    }
    e.set_password(t.trim()).map_err(|e| e.to_string())
}

pub fn has_stored_token() -> bool {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .ok()
        .and_then(|e| e.get_password().ok())
        .is_some_and(|t| !t.trim().is_empty())
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

    fn git(dir: &Path, args: &[&str]) {
        let st = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@x")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@x")
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    fn upstream(tmp: &Path) -> PathBuf {
        let up = tmp.join("up");
        std::fs::create_dir_all(&up).unwrap();
        git(&up, &["init", "-q", "-b", "main"]);
        std::fs::write(up.join("a.txt"), "1").unwrap();
        git(&up, &["add", "."]);
        git(&up, &["commit", "-q", "-m", "feat: one"]);
        up
    }

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ir-repo-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn refetch_sees_new_upstream_commits() {
        let tmp = tmpdir("refetch");
        let up = upstream(&tmp);
        let cache = tmp.join("cache/o/r");
        let src = up.to_string_lossy().to_string();
        let mut noop = |_: &str, _: u8| {};
        ensure_cached_at(&src, &cache, None, &mut noop).unwrap();
        let n1 = crate::repo_stats::commits_from_dir(&cache, Some("origin/HEAD"), &auth_env(None)).unwrap().len();
        std::fs::write(up.join("b.txt"), "2").unwrap();
        git(&up, &["add", "."]);
        git(&up, &["commit", "-q", "-m", "fix: two"]);
        ensure_cached_at(&src, &cache, None, &mut noop).unwrap();
        let n2 = crate::repo_stats::commits_from_dir(&cache, Some("origin/HEAD"), &auth_env(None)).unwrap().len();
        assert_eq!((n1, n2), (1, 2));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn clone_from_cache_never_overwrites_and_checks_out_files() {
        let tmp = tmpdir("clone");
        let up = upstream(&tmp);
        let cache = tmp.join("cache/o/r");
        let src = up.to_string_lossy().to_string();
        let mut noop = |_: &str, _: u8| {};
        ensure_cached_at(&src, &cache, None, &mut noop).unwrap();
        let parent = tmp.join("dest");
        std::fs::create_dir_all(parent.join("r")).unwrap();
        std::fs::create_dir_all(parent.join("r (2)")).unwrap();
        std::fs::write(parent.join("r/keep.txt"), "mine").unwrap();
        let out = clone_to_at(&src, "r", Some(&cache), &parent, None, &mut noop).unwrap();
        assert_eq!(out, parent.join("r (3)"));
        assert_eq!(std::fs::read_to_string(out.join("a.txt")).unwrap(), "1");
        assert_eq!(std::fs::read_to_string(parent.join("r/keep.txt")).unwrap(), "mine");
        // Cache survives the clone (copy, not move).
        assert!(cache.join(".git").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn empty_upstream_analyses_as_zero_commits() {
        let tmp = tmpdir("empty");
        let up = tmp.join("up");
        std::fs::create_dir_all(&up).unwrap();
        git(&up, &["init", "-q", "-b", "main"]);
        let cache = tmp.join("cache/o/r");
        let mut noop = |_: &str, _: u8| {};
        ensure_cached_at(&up.to_string_lossy(), &cache, None, &mut noop).unwrap();
        let commits = crate::repo_stats::commits_from_dir(&cache, Some("origin/HEAD"), &auth_env(None)).unwrap();
        assert!(commits.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// Live probe against real GitHub (opt-in, never in CI):
    /// `IR_LIVE_REPO=https://github.com/o/r IR_LIVE_DEST=/tmp/x cargo test -p inspector-rust-core --lib live_github_roundtrip -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_github_roundtrip() {
        let (Ok(url), Ok(dest)) = (std::env::var("IR_LIVE_REPO"), std::env::var("IR_LIVE_DEST")) else { return };
        let t0 = std::time::Instant::now();
        let mut log = |p: &str, n: u8| if n.is_multiple_of(25) { eprintln!("  {p} {n} %") };
        let a = crate::repo_stats::analyze_remote(&url, &mut log).unwrap();
        let all = a.ranges.iter().find(|r| r.range == crate::repo_stats::RangeKey::All).unwrap();
        eprintln!("first analysis {:?}: {} commits, bus factor {}", t0.elapsed(), all.stats.commits, all.stats.bus_factor);
        let t1 = std::time::Instant::now();
        let b = crate::repo_stats::analyze_remote(&url, &mut |_, _| {}).unwrap();
        eprintln!("second analysis (fetch) {:?}", t1.elapsed());
        assert_eq!(b.ranges.len(), 5);
        let gh = crate::repo_url::parse_repo_url(&url).unwrap();
        let parent = std::path::Path::new(&dest);
        let p1 = clone_to(&gh, parent, github_token().as_deref(), &mut |_, _| {}).unwrap();
        let p2 = clone_to(&gh, parent, github_token().as_deref(), &mut |_, _| {}).unwrap();
        eprintln!("cloned to {} and {}", p1.display(), p2.display());
        assert_ne!(p1, p2);
        assert!(p2.to_string_lossy().ends_with(" (2)"));
        assert!(std::fs::read_dir(&p1).unwrap().count() > 1, "working tree checked out");
    }

    #[test]
    fn concurrent_analyses_of_one_repo_do_not_destroy_each_other() {
        // Reopening the panel mid-clone runs two ensure+log sequences on the
        // same cache dir at once — the loser used to delete the winner's dir.
        let tmp = tmpdir("race");
        let up = upstream(&tmp);
        let cache = tmp.join("cache/o/r");
        let src = up.to_string_lossy().to_string();
        let results: Vec<Result<usize, String>> = std::thread::scope(|sc| {
            let hs: Vec<_> = (0..6)
                .map(|_| {
                    let (src, cache) = (src.clone(), cache.clone());
                    sc.spawn(move || {
                        with_cached(&src, &cache, None, &mut |_, _| {}, |d| {
                            crate::repo_stats::commits_from_dir(d, Some("origin/HEAD"), &auth_env(None)).map(|c| c.len())
                        })
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for r in &results {
            assert_eq!(r.as_ref().ok(), Some(&1), "{results:?}");
        }
        assert!(cache.join(".git").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_rejected_token_is_retried_without_it() {
        let mut seen: Vec<Option<String>> = Vec::new();
        let r: Result<u8, String> = retry_without_token(Some("old"), |t| {
            seen.push(t.map(String::from));
            if t.is_some() { Err("repo.auth: remote: Invalid username or token".into()) } else { Ok(7) }
        });
        assert_eq!(r, Ok(7));
        assert_eq!(seen, vec![Some("old".to_string()), None]);
        // No token → no second attempt; other errors are not retried.
        let mut n = 0;
        let _: Result<u8, String> = retry_without_token(None, |_| { n += 1; Err("repo.auth: x".into()) });
        assert_eq!(n, 1);
        let mut m = 0;
        let _: Result<u8, String> = retry_without_token(Some("t"), |_| { m += 1; Err("repo.network: x".into()) });
        assert_eq!(m, 1);
    }

    #[test]
    fn a_missing_origin_head_on_a_filled_cache_is_repaired_not_read_as_empty() {
        let tmp = tmpdir("nohead");
        let up = upstream(&tmp);
        let cache = tmp.join("cache/o/r");
        ensure_cached_at(&up.to_string_lossy(), &cache, None, &mut |_, _| {}).unwrap();
        git(&cache, &["symbolic-ref", "-d", "refs/remotes/origin/HEAD"]);
        let n = crate::repo_stats::commits_from_dir(&cache, Some("origin/HEAD"), &auth_env(None)).unwrap().len();
        assert_eq!(n, 1, "a repo with commits must not read as empty");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn failed_checkout_after_cache_copy_leaves_no_half_folder() {
        // A cache entry without an origin/HEAD: the copy succeeds, the
        // checkout (`reset --hard origin/HEAD`) fails — the copied folder
        // must not be left behind.
        let tmp = tmpdir("halfcopy");
        let cache = tmp.join("cache/o/r");
        std::fs::create_dir_all(&cache).unwrap();
        git(&cache, &["init", "-q", "-b", "main"]);
        let parent = tmp.join("dest");
        let mut noop = |_: &str, _: u8| {};
        let res = clone_to_at("https://github.com/o/r.git", "r", Some(&cache), &parent, None, &mut noop);
        assert!(res.is_err());
        assert!(!parent.join("r").exists(), "half-copied folder left behind");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn failed_clone_leaves_no_half_folder() {
        let tmp = tmpdir("fail");
        let parent = tmp.join("dest");
        std::fs::create_dir_all(&parent).unwrap();
        let mut noop = |_: &str, _: u8| {};
        let err = clone_to_at(&tmp.join("missing").to_string_lossy(), "r", None, &parent, None, &mut noop);
        assert!(err.is_err());
        assert!(!parent.join("r").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
