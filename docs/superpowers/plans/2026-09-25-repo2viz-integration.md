# repo2viz-Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eine GitHub-Repo-URL im Suchfeld zeigt nach Enter die Repo-Statistik (mit Zeiträumen, Heatmap, Kalender, Hotspots, Bus-Faktor, Co-Change) in der Vorschau, exportiert sie als HTML/PDF und klont das Repo mit einem Klick in einen festen Ordner.

**Architecture:** Erweitert den bestehenden `repo`-Befehl (`repo_stats.rs`, `RepoPanel.tsx`). Neu: `repo_url.rs`/`lib/repo-url.ts` (URL-Erkennung, gespiegelt über EINE Fixture-Datei), `repo_clone.rs` (Klon-Cache, Klonen, Token, Fortschritt), ein `Commit`-Modell in `repo_stats.rs`, aus dem pro Zeitraum aggregiert wird. Das Frontend bekommt eine neue `ListEntry`-Art `repo-url`, die über `dispatchCommand("repo", url)` in das bestehende Panel führt.

**Tech Stack:** Rust (Tauri 2 IPC, `std::process::Command` → `git`, `keyring`, `base64`, `dirs`), React 19 + TS + Vitest, Tailwind v4.

**Spec:** `docs/superpowers/specs/2026-09-25-repo2viz-integration-design.md`

## Global Constraints

- Kein neues Crate, kein neues npm-Package.
- Jeder blockierende Befehl (Netz, `git`) ist `pub async fn` + `tauri::async_runtime::spawn_blocking` (CLAUDE.md, „Adding a new IPC command").
- Git wird nie mit einer URL aufgerufen, die mit `-` beginnt; vor der URL steht immer `--`; Verzeichnisse per `.current_dir()`, nie `-C`.
- Das GitHub-Token steht **nie** in argv, nie im Log, nie in der DB; nur Umgebungsvariablen `GIT_CONFIG_COUNT/KEY_0/VALUE_0`, nur für `https://github.com/`.
- Immer `GIT_TERMINAL_PROMPT=0` bei jedem Git-Aufruf, der das Netz berührt (clone, fetch, remote set-head).
- Belegter Zielordner: `<repo> (2)`, `<repo> (3)` … bis `(999)`, erster freier Name ab 2 gewinnt.
- Zeiträume `d30 | d90 | d180 | y1 | all`, gezählt ab dem **letzten Commit** (nicht ab heute).
- Cache: höchstens 5 Repos oder 2 GB (`2 * 1024^3` Bytes).
- Co-Change ignoriert Commits mit mehr als 30 Dateien.
- Setting-Schlüssel: `repo.clone_dir`. Schlüsselbund: Service `io.celox.inspector-rust`, Nutzer `github-token-v1`.
- Die Dateien der parallelen dezibel-Session (`CHANGELOG.md`-Hunk, `core/frontend/src/lib/audio-level.ts`, `audio-level.test.ts`, `commandDocs.ts`, `commands.ts`, `docs/screenshots/popup-strip.png`) werden **nie** mitcommittet. Vor jedem Commit `git diff --cached --stat` prüfen; bei `commandDocs.ts`/`CHANGELOG.md` nur die eigenen Hunks stagen (`git add -p` bzw. `git apply --cached`).
- Commit-Messages enden mit:
  ```
  Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01WbJj9S9wRyEvzQE8REMWB4
  ```

## Review Focus

1. **Eine URL mit Pfad- oder Query-Anhang** (`github.com/o/r/tree/main/src?tab=x#readme`) muss als Repo `o/r` erkannt werden; `github.com/o/r/issues/5` dagegen nicht (sonst öffnet ein Issue-Link eine Klon-Analyse). → Fixture-Zeilen in Task 1.
2. **Wiederholte Analyse nach einem `git fetch`** muss die neuen Commits zeigen. Ein `--no-checkout`-Klon behält `HEAD` auf dem alten `main`; darum liest `git log` im Cache `origin/HEAD`. → Test in Task 4 (lokales Upstream-Repo, zweiter Commit, zweite Analyse zählt 2).
3. **Klonen, während der Zielordner schon `repo` und `repo (2)` enthält**, legt `repo (3)` an und überschreibt nichts. → Test in Task 2 (rein) + Task 4 (echtes Dateisystem).
4. **Ein Zeitraum ohne Commits** zeigt „keine Commits in diesem Zeitraum" statt leerer Karten oder NaN. → Test in Task 3 (Aggregat liefert 0er-Statistik) + Task 7 (Panel-Text).
5. **Token taucht nirgends in argv oder Fehlermeldungen auf**, auch wenn Git mit dem Header im stderr antwortet. → Test in Task 2 (`clone_args` enthält kein Token; `redact` entfernt es aus stderr).

---

### Task 1: URL-Erkennung (Rust + TS, eine gemeinsame Fixture)

**Files:**
- Create: `core/frontend/src/lib/repo-url-cases.json`
- Create: `core/frontend/src/lib/repo-url.ts`
- Create: `core/frontend/src/lib/repo-url.test.ts`
- Create: `core/rust-lib/src/repo_url.rs`
- Modify: `core/rust-lib/src/lib.rs` (Modulliste, neben `mod repo_stats;` Zeile ~73)

**Interfaces:**
- Produces (Rust): `pub struct RepoUrl { pub owner: String, pub repo: String, pub web_url: String, pub clone_url: String }` (Serialize, Deserialize, Clone, Debug, PartialEq, Eq); `pub fn parse_repo_url(text: &str) -> Option<RepoUrl>`; `pub fn find_repo_url(text: &str) -> Option<RepoUrl>`.
- Produces (TS): `export interface RepoUrl { owner: string; repo: string; web_url: string; clone_url: string }`; `export function parseRepoUrl(text: string): RepoUrl | null`; `export function findRepoUrl(text: string): RepoUrl | null`.

- [ ] **Step 1: Fixture schreiben** — `core/frontend/src/lib/repo-url-cases.json`:

```json
[
  { "input": "https://github.com/pepperonas/repo2viz", "expect": ["pepperonas", "repo2viz"] },
  { "input": "https://github.com/pepperonas/repo2viz/", "expect": ["pepperonas", "repo2viz"] },
  { "input": "https://github.com/pepperonas/repo2viz.git", "expect": ["pepperonas", "repo2viz"] },
  { "input": "http://github.com/pepperonas/repo2viz", "expect": ["pepperonas", "repo2viz"] },
  { "input": "github.com/pepperonas/repo2viz", "expect": ["pepperonas", "repo2viz"] },
  { "input": "www.github.com/pepperonas/repo2viz", "expect": ["pepperonas", "repo2viz"] },
  { "input": "https://GitHub.com/Pepperonas/Repo2Viz", "expect": ["Pepperonas", "Repo2Viz"] },
  { "input": "  https://github.com/o/r  ", "expect": ["o", "r"] },
  { "input": "https://github.com/o/r/tree/main/src/lib", "expect": ["o", "r"] },
  { "input": "https://github.com/o/r/blob/main/README.md", "expect": ["o", "r"] },
  { "input": "https://github.com/o/r/tree/main/src?tab=x#readme", "expect": ["o", "r"] },
  { "input": "https://github.com/o/r?tab=readme-ov-file", "expect": ["o", "r"] },
  { "input": "git@github.com:o/r.git", "expect": ["o", "r"] },
  { "input": "git@github.com:o/r", "expect": ["o", "r"] },
  { "input": "https://github.com/o/my.repo_name-2", "expect": ["o", "my.repo_name-2"] },
  { "input": "https://github.com/o/r/issues/5", "expect": null },
  { "input": "https://github.com/o/r/pull/7", "expect": null },
  { "input": "https://github.com/o/r/actions", "expect": null },
  { "input": "https://github.com/o/r/releases", "expect": null },
  { "input": "https://github.com/o/r/wiki", "expect": null },
  { "input": "https://github.com/o", "expect": null },
  { "input": "https://github.com/", "expect": null },
  { "input": "https://github.com/features/actions", "expect": null },
  { "input": "https://github.com/orgs/o/repositories", "expect": null },
  { "input": "https://github.com/marketplace/x", "expect": null },
  { "input": "https://gist.github.com/o/abc123", "expect": null },
  { "input": "https://gitlab.com/o/r", "expect": null },
  { "input": "https://github.com.evil.io/o/r", "expect": null },
  { "input": "https://github.com/-o/r", "expect": null },
  { "input": "https://github.com/o/-r", "expect": null },
  { "input": "https://github.com/o/..", "expect": null },
  { "input": "https://github.com/o/r x", "expect": null },
  { "input": "https://github.com/o;rm/r", "expect": null },
  { "input": "repo https://github.com/o/r", "expect": null },
  { "input": "", "expect": null }
]
```

- [ ] **Step 2: TS-Test schreiben** — `core/frontend/src/lib/repo-url.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import cases from "./repo-url-cases.json";
import { parseRepoUrl, findRepoUrl } from "./repo-url";

describe("parseRepoUrl (shared fixture with repo_url.rs)", () => {
  for (const c of cases as { input: string; expect: [string, string] | null }[]) {
    it(`${JSON.stringify(c.input)} → ${JSON.stringify(c.expect)}`, () => {
      const r = parseRepoUrl(c.input);
      if (c.expect === null) expect(r).toBeNull();
      else {
        expect(r).not.toBeNull();
        expect([r!.owner, r!.repo]).toEqual(c.expect);
        expect(r!.web_url).toBe(`https://github.com/${c.expect[0]}/${c.expect[1]}`);
        expect(r!.clone_url).toBe(`https://github.com/${c.expect[0]}/${c.expect[1]}.git`);
      }
    });
  }
});

describe("findRepoUrl (inside prose / a clip)", () => {
  it("finds a URL in the middle of text and trims trailing punctuation", () => {
    expect(findRepoUrl("schau mal: https://github.com/o/r.")).toMatchObject({ owner: "o", repo: "r" });
    expect(findRepoUrl("(siehe https://github.com/o/r)")).toMatchObject({ owner: "o", repo: "r" });
  });
  it("skips non-repo github links and returns the first repo link", () => {
    expect(findRepoUrl("https://github.com/o/r/issues/1 und https://github.com/a/b")).toMatchObject({ owner: "a", repo: "b" });
  });
  it("returns null when no repo link exists", () => {
    expect(findRepoUrl("nur text ohne link")).toBeNull();
  });
});
```

- [ ] **Step 3: Test laufen lassen, muss scheitern**

Run: `cd core/frontend && npx vitest run src/lib/repo-url.test.ts`
Expected: FAIL — `Failed to resolve import "./repo-url"`.

- [ ] **Step 4: TS implementieren** — `core/frontend/src/lib/repo-url.ts`:

```ts
/**
 * GitHub repository URL detection (v0.183.0) — a bare repo URL typed into the
 * search bar surfaces a "Repo analysieren" row. Pure; MIRRORS
 * `core/rust-lib/src/repo_url.rs`. Both sides run the SAME fixture
 * (`repo-url-cases.json`), so they cannot drift apart.
 *
 * Only a REPO counts: issue/PR/actions links, profiles, gists and GitHub's own
 * system paths are rejected — otherwise pasting an issue link would start a
 * clone. `/tree/…` and `/blob/…` collapse to the repo. Owner/repo are checked
 * against GitHub's character rules, so a parsed value can never smuggle a git
 * option or a path escape.
 */
export interface RepoUrl {
  owner: string;
  repo: string;
  web_url: string;
  clone_url: string;
}

const RESERVED_OWNERS = new Set([
  "features", "orgs", "organizations", "marketplace", "settings", "login", "join",
  "topics", "explore", "sponsors", "about", "pricing", "enterprise", "collections",
  "trending", "notifications", "new", "apps", "search", "security", "site", "contact",
  "customer-stories", "readme", "codespaces", "issues", "pulls", "discussions", "events",
]);

const OWNER_RE = /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})$/;
const REPO_RE = /^[A-Za-z0-9._][A-Za-z0-9._-]{0,99}$/;

export function parseRepoUrl(text: string): RepoUrl | null {
  const t = text.trim();
  if (!t || /\s/.test(t)) return null;
  let path: string;
  const scp = /^git@github\.com:(.+)$/i.exec(t);
  if (scp) {
    path = scp[1];
  } else {
    const m = /^(?:https?:\/\/)?(?:www\.)?github\.com(\/.*)?$/i.exec(t);
    if (!m || !m[1]) return null;
    path = m[1];
  }
  path = path.split(/[?#]/)[0];
  const segs = path.split("/").filter(Boolean);
  if (segs.length < 2) return null;
  if (segs.length > 2 && segs[2] !== "tree" && segs[2] !== "blob") return null;
  const owner = segs[0];
  const repo = segs[1].replace(/\.git$/i, "");
  if (RESERVED_OWNERS.has(owner.toLowerCase())) return null;
  if (!OWNER_RE.test(owner)) return null;
  if (!REPO_RE.test(repo) || repo === "." || repo === "..") return null;
  return {
    owner,
    repo,
    web_url: `https://github.com/${owner}/${repo}`,
    clone_url: `https://github.com/${owner}/${repo}.git`,
  };
}

/** First repo URL among the whitespace-separated tokens of `text` (a clip).
 *  Surrounding brackets/quotes and trailing punctuation are stripped. */
export function findRepoUrl(text: string): RepoUrl | null {
  for (const raw of text.split(/\s+/)) {
    const tok = raw.replace(/^[("'<[]+/, "").replace(/[)"'>\],.;:!?]+$/, "");
    if (!/github\.com/i.test(tok)) continue;
    const r = parseRepoUrl(tok);
    if (r) return r;
  }
  return null;
}
```

- [ ] **Step 5: TS-Test laufen lassen, muss bestehen**

Run: `cd core/frontend && npx vitest run src/lib/repo-url.test.ts`
Expected: PASS (alle Fixture-Zeilen + 3 findRepoUrl-Tests).

- [ ] **Step 6: Rust-Test + Modul schreiben** — `core/rust-lib/src/repo_url.rs`:

```rust
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
```

In `core/rust-lib/src/lib.rs` direkt unter `mod repo_stats;` einfügen:

```rust
mod repo_url;
```

- [ ] **Step 7: Rust-Test laufen lassen**

Run: `cargo test -p inspector-rust-core --lib repo_url`
Expected: PASS, 2 Tests. (Vor dem Schreiben des Moduls fehlschlagend prüfen: Modulzeile zuerst ohne Datei → Kompilierfehler; das genügt als Rot.)

- [ ] **Step 8: Mutationsprobe** — in `repo_url.rs` die Zeile `if segs.len() > 2 && segs[2] != "tree" && segs[2] != "blob"` auf `if false` ändern, `cargo test -p inspector-rust-core --lib repo_url` muss ROT werden (Issue-Links würden erkannt); zurücksetzen und Prüfsumme mit `git diff --stat core/rust-lib/src/repo_url.rs` kontrollieren (Datei ist neu → `git status` zeigt `??`, Inhalt mit Editor zurückgesetzt). Gleiches in `repo-url.ts` mit `if (segs.length > 2 && …)` → `if (false)`: vitest muss rot werden, zurücksetzen.

- [ ] **Step 9: Commit**

```bash
git add core/frontend/src/lib/repo-url-cases.json core/frontend/src/lib/repo-url.ts core/frontend/src/lib/repo-url.test.ts core/rust-lib/src/repo_url.rs core/rust-lib/src/lib.rs
git diff --cached --stat
git commit -m "feat(repo): GitHub repo URL detection, mirrored Rust/TS with one fixture

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01WbJj9S9wRyEvzQE8REMWB4"
```

---

### Task 2: Reine Klon-Helfer (Zielname, Cache-Räumung, Fortschritt, Auth-Umgebung, Fehlerklassen)

**Files:**
- Create: `core/rust-lib/src/repo_clone.rs` (in diesem Task nur die reinen Funktionen + Tests)
- Modify: `core/rust-lib/src/lib.rs` (`mod repo_clone;` unter `mod repo_url;`)

**Interfaces:**
- Produces:
  - `pub fn free_dir_name(base: &str, exists: impl Fn(&str) -> bool) -> Option<String>`
  - `pub struct CacheEntry { pub key: String, pub mtime_ms: u64, pub bytes: u64 }`
  - `pub fn cache_evictions(entries: &[CacheEntry], max_n: usize, max_bytes: u64, keep: &str) -> Vec<String>`
  - `pub const CACHE_MAX_REPOS: usize = 5;` `pub const CACHE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;`
  - `pub fn parse_git_progress(line: &str) -> Option<(String, u8)>`
  - `pub fn auth_env(token: Option<&str>) -> Vec<(String, String)>`
  - `pub fn clone_args(url: &str, dest: &str, no_checkout: bool) -> Vec<String>`
  - `pub fn redact(text: &str, token: Option<&str>) -> String`
  - `pub fn classify_git_error(stderr: &str) -> &'static str` — returns `"repo.auth"`, `"repo.network"` or `"repo.git"`
  - `pub fn default_clone_dir(home: Option<&Path>, downloads: Option<&Path>, exists: impl Fn(&Path) -> bool) -> Option<PathBuf>`

- [ ] **Step 1: Test-Modul schreiben** — `core/rust-lib/src/repo_clone.rs` zunächst nur mit Kopfkommentar, `use`s und Tests:

```rust
//! Clone cache + clone-to-folder for the `repo` command (v0.183.0).
//!
//! The pure helpers (target name, cache eviction, progress parsing, auth
//! environment, error classes) are unit-tested; the git/filesystem shell
//! below them is thin. Token handling: the token is passed to git ONLY as
//! `GIT_CONFIG_*` environment variables (never argv — argv is world-visible
//! in `ps`), only for `https://github.com/`, and is redacted from anything
//! that could reach the UI or the log.

use std::path::{Path, PathBuf};

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
```

`lib.rs`: `mod repo_clone;` unter `mod repo_url;`.

- [ ] **Step 2: Rot prüfen**

Run: `cargo test -p inspector-rust-core --lib repo_clone`
Expected: FAIL — Kompilierfehler `cannot find function free_dir_name`.

- [ ] **Step 3: Reine Funktionen implementieren** — über dem `#[cfg(test)]`-Block in `repo_clone.rs`:

```rust
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
```

- [ ] **Step 4: Grün prüfen**

Run: `cargo test -p inspector-rust-core --lib repo_clone`
Expected: PASS, 8 Tests.

- [ ] **Step 5: Mutationsproben** (je einzeln, danach zurücksetzen und mit `cargo test` grün prüfen):
  - In `cache_evictions` die `if e.key == keep { … continue; }`-Verzweigung entfernen → Test `cache_evictions_drop_…` muss rot werden.
  - In `auth_env` das `.encode(...)` durch `format!("x-access-token:{t}")` ersetzen → `auth_env_never_…` muss rot werden.
  - In `clone_args` den `"--"`-Eintrag entfernen → `clone_args_put_…` muss rot werden.

- [ ] **Step 6: Commit**

```bash
git add core/rust-lib/src/repo_clone.rs core/rust-lib/src/lib.rs
git diff --cached --stat
git commit -m "feat(repo): pure clone helpers — target naming, cache eviction, progress, token env

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01WbJj9S9wRyEvzQE8REMWB4"
```

---

### Task 3: Commit-Modell + Zeiträume + neue Kennzahlen in `repo_stats.rs`

**Files:**
- Modify: `core/rust-lib/src/repo_stats.rs` (Struct `RepoStats` Zeile 33–58, `parse_git_log` Zeile 179–323, Tests ab 547)

**Interfaces:**
- Consumes: bestehende `weekday_hour`, `day_ordinal`, `classify_commit`, `extension_of`, `longest_streak`.
- Produces:
  - `pub struct Commit { pub iso: String, pub day: Option<i64>, pub author_key: String, pub author_name: String, pub subject: String, pub files: Vec<(String, u64, u64)> }`
  - `pub fn parse_commits(raw: &str) -> Vec<Commit>` (newest first, wie git)
  - `pub fn aggregate<'a>(commits: impl IntoIterator<Item = &'a Commit>) -> RepoStats`
  - `pub fn parse_git_log(raw: &str) -> RepoStats` (bleibt, = `aggregate(&parse_commits(raw))`)
  - `#[serde(rename_all = "lowercase")] pub enum RangeKey { D30, D90, D180, Y1, All }` mit `pub const ALL: [RangeKey; 5]`, `pub fn days(self) -> Option<i64>`, `pub fn parse(s: &str) -> Option<RangeKey>`, `pub fn label(self) -> &'static str`
  - `pub struct RangedStats { pub range: RangeKey, pub stats: RepoStats }`
  - `pub fn analyze_ranges(commits: &[Commit]) -> Vec<RangedStats>`
  - `pub fn bus_factor(counts: &[u64]) -> u32`
  - Neue `RepoStats`-Felder: `heatmap: [[u64; 24]; 7]`, `calendar: Vec<DayCount>`, `hotspots: Vec<Hotspot>`, `bus_factor: u32`, `dir_bus_factor: Vec<DirStat>`, `co_change: Vec<CoChange>`
  - `pub struct DayCount { pub date: String, pub commits: u64 }`, `pub struct Hotspot { pub path: String, pub changes: u64, pub authors: u64 }`, `pub struct DirStat { pub dir: String, pub commits: u64, pub authors: u64, pub bus_factor: u32 }`, `pub struct CoChange { pub a: String, pub b: String, pub count: u64 }`
  - `RepoStats` und alle Unter-Structs bekommen zusätzlich `Deserialize` (für `repo_export` in Task 5).

- [ ] **Step 1: Tests schreiben** — am Ende des `mod tests` in `repo_stats.rs` anhängen (bestehender Helfer `synth_log()` bleibt; neuer Helfer `log_of` baut Logs gezielt):

```rust
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
```

- [ ] **Step 2: Rot prüfen**

Run: `cargo test -p inspector-rust-core --lib repo_stats`
Expected: FAIL — Kompilierfehler (`parse_commits`, `aggregate`, `RangeKey` … fehlen).

- [ ] **Step 3: Implementieren.**

3a — `use serde::Serialize;` ersetzen durch `use serde::{Deserialize, Serialize};`. Jedes `#[derive(Serialize, Clone, Debug, …)]` an `RepoStats`, `MonthCount`, `FileStat`, `ExtStat`, `AuthorStat`, `CatCount` um `Deserialize` ergänzen.

3b — Neue Felder am Ende von `RepoStats` (vor der schließenden Klammer):

```rust
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
```

Neue Structs (neben `CatCount`):

```rust
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
    /// (path, insertions, deletions)
    pub files: Vec<(String, u64, u64)>,
}
```

`#[derive(Default)]` an `RepoStats` funktioniert mit `[[u64; 24]; 7]` (Arrays bis 32 implementieren `Default`). Wenn der Compiler `Default` für das 2D-Array ablehnt: `Default` für `RepoStats` von Hand nicht nötig — Rust implementiert `Default` für `[T; N]` mit `N ≤ 32`, `T: Default`; `[u64; 24]: Default` → `[[u64;24];7]: Default` gilt.

3c — `parse_git_log` (Zeile 179–323) ersetzen durch drei Funktionen. `parse_commits` übernimmt das Zerlegen, `aggregate` die bisherige Rechenlogik plus die neuen Kennzahlen:

```rust
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
            files,
        });
    }
    out
}

/// Pure: legacy entry point, kept for its callers + tests.
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
    let mut files: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut file_authors: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let mut exts: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut months: BTreeMap<String, u64> = BTreeMap::new();
    let mut cats: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut dirs: BTreeMap<String, BTreeMap<usize, u64>> = BTreeMap::new();
    let mut pairs: HashMap<(String, String), u64> = HashMap::new();
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
            let fe = files.entry(path.clone()).or_insert((0, 0));
            fe.0 += 1;
            fe.1 += churn;
            file_authors.entry(path.clone()).or_default().insert(a);
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
            let mut ps: Vec<&String> = c.files.iter().map(|(p, _, _)| p).collect();
            ps.sort();
            ps.dedup();
            for i in 0..ps.len() {
                for j in i + 1..ps.len() {
                    *pairs.entry((ps[i].clone(), ps[j].clone())).or_insert(0) += 1;
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
            (*changes >= 3 && n <= 2).then(|| Hotspot { path: path.clone(), changes: *changes, authors: n })
        })
        .collect();
    hs.sort_by(|a, b| b.changes.cmp(&a.changes).then(a.path.cmp(&b.path)));
    hs.truncate(10);
    stats.hotspots = hs;

    let mut fv: Vec<FileStat> = files
        .into_iter()
        .map(|(path, (changes, churn))| FileStat { path, changes, churn })
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
        .map(|((a, b), count)| CoChange { a, b, count })
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
            let stats = match (range.days(), anchor) {
                (Some(d), Some(a)) => aggregate(commits.iter().filter(|c| c.day.is_some_and(|x| x > a - d))),
                _ => aggregate(commits.iter()),
            };
            RangedStats { range, stats }
        })
        .collect()
}
```

Hinweis: Mit `x > a - d` liegt ein Commit 20 Tage vor dem Anker in D30 (20 < 30), einer genau 365 Tage davor NICHT in Y1 — genau das prüft `ranges_are_anchored_…` (2019-06-30 → 2020-06-30 sind 366 Tage).

- [ ] **Step 4: Grün prüfen**

Run: `cargo test -p inspector-rust-core --lib repo_stats`
Expected: PASS, alte + 7 neue Tests. `parse_git_log_computes_the_core_metrics` (bestehend) muss unverändert grün sein.

- [ ] **Step 5: Mutationsproben** (einzeln, danach zurücksetzen):
  - In `analyze_ranges` `x > a - d` → `x >= a - d - 1`: `ranges_are_anchored_…` muss rot werden.
  - `CO_CHANGE_MAX_FILES` auf `31` setzen: `co_change_ignores_…` muss rot werden.
  - In `bus_factor` `acc * 2 >= total` → `acc * 2 > total`: `bus_factor_is_…` muss rot werden.

- [ ] **Step 6: Commit**

```bash
git add core/rust-lib/src/repo_stats.rs
git diff --cached --stat
git commit -m "feat(repo): commit model, time ranges, heatmap, calendar, hotspots, bus factor, co-change

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01WbJj9S9wRyEvzQE8REMWB4"
```

---

### Task 4: Klon-Cache, Klonen, Token-Quelle, IPC

**Files:**
- Modify: `core/rust-lib/src/repo_clone.rs` (impure Teil unter den reinen Funktionen)
- Modify: `core/rust-lib/src/repo_stats.rs` (`git_log` bekommt `rev`, neue `RepoAnalysis`, `analyze_local`/`analyze_remote` liefern Commits)
- Modify: `core/rust-lib/src/commands.rs:1314-1392` (repo-Block)
- Modify: `core/rust-lib/src/lib.rs` (invoke_handler: neben `commands::repo_export,` Zeile ~874)

**Interfaces:**
- Consumes: Task 1 `RepoUrl`, `parse_repo_url`; Task 2 Helfer; Task 3 `Commit`, `parse_commits`, `analyze_ranges`, `RangedStats`, `RepoStats`, `RangeKey`.
- Produces (Rust):
  - `repo_stats::RepoAnalysis { name: String, source: String, github: Option<RepoUrl>, ranges: Vec<RangedStats> }` (Serialize)
  - `repo_stats::commits_from_dir(dir: &Path, rev: Option<&str>, env: &[(String, String)]) -> Result<Vec<Commit>, String>`
  - `repo_clone::ensure_cached(u: &RepoUrl, token: Option<&str>, on_progress: &mut dyn FnMut(&str, u8)) -> Result<PathBuf, String>`
  - `repo_clone::clone_to(u: &RepoUrl, parent: &Path, token: Option<&str>, on_progress: &mut dyn FnMut(&str, u8)) -> Result<PathBuf, String>`
  - `repo_clone::github_token() -> Option<String>`, `repo_clone::set_stored_token(t: &str) -> Result<(), String>`, `repo_clone::has_stored_token() -> bool`, `repo_clone::gh_available() -> bool`
  - IPC: `repo_analyze(app, target: Option<String>) -> RepoAnalysis` (Rückgabetyp geändert), `repo_export(app, stats: RepoStats, range: String, format: Option<String>) -> String` (Signatur geändert), `repo_clone(app, db, url: String) -> String`, `get_repo_config(db) -> RepoConfig { clone_dir: String, has_token: bool, gh_available: bool }`, `set_repo_clone_dir(db, dir: String) -> ()`, `set_github_token(token: String) -> ()`
  - Event `repo-progress` mit Payload `{ op: "analyze" | "clone", phase: String, percent: u8 }`

- [ ] **Step 1: Integrationstests schreiben** (lokale Git-Repos, kein Netz) — ans `mod tests` von `repo_clone.rs` anhängen. `ensure_cached_at`/`clone_to_at` sind testbare Varianten mit explizitem Cache-Pfad und Quell-URL:

```rust
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
```

Hinweis: Lokale Pfade als Quelle funktionieren für `git clone`, weil die URL-Prüfung (`parse_repo_url`) nur in der öffentlichen Funktion `ensure_cached`/`clone_to` stattfindet; `_at`-Varianten sind `pub(crate)` und nehmen eine bereits geprüfte Quelle.

- [ ] **Step 2: Rot prüfen**

Run: `cargo test -p inspector-rust-core --lib repo_clone`
Expected: FAIL — `ensure_cached_at`, `clone_to_at`, `commits_from_dir` fehlen.

- [ ] **Step 3: `repo_stats.rs` anpassen.** `git_log` ersetzen und `commits_from_dir` + `RepoAnalysis` ergänzen; `analyze_local`/`analyze_remote` auf `RepoAnalysis` umstellen:

```rust
/// Run `git log` with the repo2viz format in `dir` (`rev` e.g. `origin/HEAD`
/// for the no-checkout cache, whose local branch is stale after a fetch).
fn git_log(dir: &std::path::Path, rev: Option<&str>, env: &[(String, String)]) -> Result<String, String> {
    let fmt = format!("{REC}%H{FLD}%aI{FLD}%aN{FLD}%aE{FLD}%s");
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
        let err = String::from_utf8_lossy(&out.stderr);
        // An empty repo has no HEAD / origin/HEAD — that is "no commits", not a failure.
        if err.contains("does not have any commits") || err.contains("unknown revision") || err.contains("ambiguous argument") {
            return Ok(String::new());
        }
        return Err(format!("git log fehlgeschlagen: {}", err.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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
}

fn finish(name: String, source: String, github: Option<crate::repo_url::RepoUrl>, commits: &[Commit]) -> RepoAnalysis {
    let mut ranges = analyze_ranges(commits);
    for r in &mut ranges {
        r.stats.name = name.clone();
        r.stats.source = source.clone();
    }
    RepoAnalysis { name, source, github, ranges }
}

pub fn analyze_local(dir: &std::path::Path) -> Result<RepoAnalysis, String> {
    if !dir.join(".git").exists() {
        return Err("Kein Git-Repository (kein .git gefunden).".into());
    }
    let commits = commits_from_dir(dir, None, &[])?;
    let (name, _slug) = repo_identity(&dir.to_string_lossy());
    Ok(finish(name, dir.to_string_lossy().into_owned(), None, &commits))
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
        let dir = crate::repo_clone::ensure_cached(&gh, token.as_deref(), on_progress)?;
        let env = crate::repo_clone::auth_env(token.as_deref());
        let commits = commits_from_dir(&dir, Some("origin/HEAD"), &env)?;
        crate::repo_clone::prune_cache(&dir);
        return Ok(finish(gh.repo.clone(), gh.web_url.clone(), Some(gh), &commits));
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
    let _ = std::fs::remove_dir_all(&tmp);
    let (name, _slug) = repo_identity(url);
    Ok(finish(name, url.to_string(), None, &result?))
}
```

Den ungenutzten Konstanten-Rest `const T_GIT` und die Zeile `let _ = T_GIT;` entfernen. Den bestehenden Test `analyze_remote_rejects_flag_smuggling_and_junk` anpassen: Aufrufe `analyze_remote(x)` → `analyze_remote(x, &mut |_, _| {})`. `dump_for_a_sight_check` und die `build_html`-Tests bleiben (sie nutzen `parse_git_log`, das es weiter gibt).

- [ ] **Step 4: Impuren Teil in `repo_clone.rs` implementieren** (oberhalb `#[cfg(test)]`):

```rust
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

pub fn ensure_cached(
    u: &crate::repo_url::RepoUrl,
    token: Option<&str>,
    on_progress: &mut dyn FnMut(&str, u8),
) -> Result<PathBuf, String> {
    let dir = cache_dir_for(u)?;
    ensure_cached_at(&u.clone_url, &dir, token, on_progress)
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
        let _ = std::fs::remove_dir_all(&k);
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
    let cached = cache_dir_for(u).ok().filter(|d| d.join(".git").exists());
    if let Some(c) = &cached {
        // Freshen before copying so the clone is current.
        ensure_cached_at(&u.clone_url, c, token, on_progress)?;
    }
    clone_to_at(&u.clone_url, &u.repo, cached.as_deref(), parent, token, on_progress)
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
```

- [ ] **Step 5: Grün prüfen**

Run: `cargo test -p inspector-rust-core --lib repo_`
Expected: PASS (repo_url, repo_clone inkl. 3 Integrationstests, repo_stats).

- [ ] **Step 6: IPC in `commands.rs`** — den Block Zeile 1314–1392 (`resolve_repo_target` bis Ende `repo_export`) ersetzen. `resolve_repo_target` + `RepoTarget` bleiben unverändert; `run_repo`, `repo_analyze`, `repo_export` werden zu:

```rust
fn run_repo(
    target: RepoTarget,
    on_progress: &mut dyn FnMut(&str, u8),
) -> Result<crate::repo_stats::RepoAnalysis, String> {
    match target {
        RepoTarget::Remote(url) => crate::repo_stats::analyze_remote(&url, on_progress),
        RepoTarget::Local(path) => crate::repo_stats::analyze_local(&path),
    }
}

#[derive(serde::Serialize, Clone)]
struct RepoProgress {
    op: &'static str,
    phase: String,
    percent: u8,
}

fn progress_emitter(app: &AppHandle, op: &'static str) -> impl FnMut(&str, u8) {
    let app = app.clone();
    let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
    move |phase: &str, percent: u8| {
        // ~10 events/s is plenty; always pass 100 %.
        if percent == 100 || last.elapsed() >= std::time::Duration::from_millis(100) {
            last = std::time::Instant::now();
            let _ = app.emit("repo-progress", RepoProgress { op, phase: phase.to_string(), percent });
        }
    }
}

/// Analyse a repo (URL, local path, or the Finder selection when omitted).
#[tauri::command]
pub async fn repo_analyze(
    app: AppHandle,
    target: Option<String>,
) -> Result<crate::repo_stats::RepoAnalysis, String> {
    let t = resolve_repo_target(target)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut emit = progress_emitter(&app, "analyze");
        run_repo(t, &mut emit)
    })
    .await
    .map_err(|e| format!("repo task: {e}"))?
}

/// Write the panel's already-computed stats (one range) as HTML/PDF to
/// ~/Downloads — no re-clone (the `loc_export` pattern).
#[tauri::command]
pub async fn repo_export(
    app: AppHandle,
    stats: crate::repo_stats::RepoStats,
    range: String,
    format: Option<String>,
) -> Result<String, String> {
    let ext = match format.as_deref().unwrap_or("html") {
        "html" => "html",
        "pdf" => "pdf",
        other => return Err(format!("Unbekanntes Format: {other}")),
    };
    let range = crate::repo_stats::RangeKey::parse(&range).ok_or_else(|| format!("Unbekannter Zeitraum: {range}"))?;
    let (_name, slug) = crate::repo_stats::repo_identity(&stats.source);
    let html = crate::repo_stats::build_html(&stats, range);
    let dir = dirs::download_dir().ok_or_else(|| "Kein Downloads-Ordner".to_string())?;
    let suffix = if range == crate::repo_stats::RangeKey::All { String::new() } else { format!("-{}", serde_json::to_string(&range).unwrap_or_default().trim_matches('"')) };
    let out = dir.join(format!("{slug}-activity{suffix}.{ext}"));
    write_report(&app, html, &out)?;
    reveal_in_file_manager(&out);
    Ok(out.display().to_string())
}

#[derive(serde::Serialize)]
pub struct RepoConfig {
    clone_dir: String,
    has_token: bool,
    gh_available: bool,
}

fn repo_clone_dir(db: &DbHandle) -> Option<std::path::PathBuf> {
    let home = dirs::home_dir();
    match crate::settings::get(db, "repo.clone_dir").ok().flatten().filter(|s| !s.trim().is_empty()) {
        Some(s) => Some(crate::path_arg::expand_user(&s, home.as_deref())),
        None => crate::repo_clone::default_clone_dir(home.as_deref(), dirs::download_dir().as_deref(), |p| p.is_dir()),
    }
}

#[tauri::command]
pub fn get_repo_config(db: State<'_, DbHandle>) -> RepoConfig {
    RepoConfig {
        clone_dir: repo_clone_dir(&db).map(|p| p.display().to_string()).unwrap_or_default(),
        has_token: crate::repo_clone::has_stored_token(),
        gh_available: crate::repo_clone::gh_available(),
    }
}

#[tauri::command]
pub fn set_repo_clone_dir(db: State<'_, DbHandle>, dir: String) -> Result<(), String> {
    crate::settings::set(&db, "repo.clone_dir", dir.trim()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_github_token(token: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::repo_clone::set_stored_token(&token))
        .await
        .map_err(|e| format!("token task: {e}"))?
}

/// Clone a GitHub repo into the configured folder (`name (2)`, … when taken).
#[tauri::command]
pub async fn repo_clone(app: AppHandle, db: State<'_, DbHandle>, url: String) -> Result<String, String> {
    let u = crate::repo_url::parse_repo_url(&url).ok_or_else(|| "Keine GitHub-Repo-URL.".to_string())?;
    let parent = repo_clone_dir(&db).ok_or_else(|| "Kein Zielordner — in Settings → Repositories festlegen.".to_string())?;
    let out = tauri::async_runtime::spawn_blocking(move || {
        let token = crate::repo_clone::github_token();
        let mut emit = progress_emitter(&app, "clone");
        crate::repo_clone::clone_to(&u, &parent, token.as_deref(), &mut emit)
    })
    .await
    .map_err(|e| format!("clone task: {e}"))??;
    reveal_in_file_manager(&out);
    Ok(out.display().to_string())
}
```

`DbHandle`, `State`, `Emitter` sind in `commands.rs` bereits importiert (prüfen: `grep -n "^use" core/rust-lib/src/commands.rs | head`; falls `tauri::Emitter` fehlt, `use tauri::Emitter;` ergänzen). `build_html` bekommt in Task 5 den `range`-Parameter — bis dahin kompiliert `repo_export` nicht; deshalb in diesem Task temporär `crate::repo_stats::build_html(&stats)` aufrufen und `range` nur für den Dateinamen nutzen. In Task 5 wird der Aufruf umgestellt.

`lib.rs` invoke_handler — nach `commands::repo_export,` einfügen:

```rust
            commands::repo_clone,
            commands::get_repo_config,
            commands::set_repo_clone_dir,
            commands::set_github_token,
```

- [ ] **Step 7: Build + Tests**

Run: `cargo clippy -p inspector-rust-core --all-targets -- -D warnings && cargo test -p inspector-rust-core --lib repo_`
Expected: keine Warnungen, alle repo_-Tests PASS.

- [ ] **Step 8: Mutationsproben** (einzeln, danach zurücksetzen):
  - In `ensure_cached_at` den `fetch`-Aufruf im `if dir.join(".git").exists()`-Zweig auskommentieren → `refetch_sees_new_upstream_commits` muss rot werden (n2 bleibt 1).
  - Im Test `refetch_sees_…` `Some("origin/HEAD")` → `None` (liest den veralteten lokalen Branch) → der Test muss rot werden; das belegt, dass `origin/HEAD` tragend ist.
  - In `clone_to_at` den Aufräum-Zweig `let _ = std::fs::remove_dir_all(&dest);` entfernen → `failed_clone_leaves_no_half_folder` muss rot werden (falls git den Ordner gar nicht anlegt, bleibt er grün — dann ist die Mutation ungültig; stattdessen vor dem `run_git` `std::fs::create_dir_all(&dest)` einfügen und erneut prüfen).

- [ ] **Step 9: Commit**

```bash
git add core/rust-lib/src/repo_clone.rs core/rust-lib/src/repo_stats.rs core/rust-lib/src/commands.rs core/rust-lib/src/lib.rs
git diff --cached --stat
git commit -m "feat(repo): clone cache with fetch-on-repeat, clone-to-folder, GitHub token via env

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01WbJj9S9wRyEvzQE8REMWB4"
```

---

### Task 5: HTML/PDF-Export mit den neuen Karten

**Files:**
- Modify: `core/rust-lib/src/repo_stats.rs` (`build_html` Zeile ~455, `REPO_CSS`, Tests `build_html_*`, `html_escapes_injected_names`, `dump_for_a_sight_check`)
- Modify: `core/rust-lib/src/commands.rs` (`repo_export` ruft `build_html(&stats, range)`)

**Interfaces:**
- Consumes: Task 3 Felder + `RangeKey::label`.
- Produces: `pub fn build_html(stats: &RepoStats, range: RangeKey) -> String`; `pub fn heatmap_svg(h: &[[u64; 24]; 7]) -> String`; `pub fn calendar_svg(days: &[DayCount]) -> String`.

- [ ] **Step 1: Tests schreiben** — bestehende Aufrufe `build_html(&s)` in den Tests auf `build_html(&s, RangeKey::All)` ändern und anhängen:

```rust
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
        assert_eq!(calendar_svg(&[]), "");
    }

    #[test]
    fn empty_range_html_says_so() {
        let html = build_html(&aggregate(std::iter::empty::<&Commit>()), RangeKey::D30);
        assert!(html.contains("Keine Commits in diesem Zeitraum"));
    }
```

- [ ] **Step 2: Rot prüfen**

Run: `cargo test -p inspector-rust-core --lib repo_stats`
Expected: FAIL — `build_html` erwartet 1 Argument; `calendar_svg` fehlt.

- [ ] **Step 3: Implementieren** — über `build_html` einfügen:

```rust
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
```

`build_html` erhält die Signatur `pub fn build_html(stats: &RepoStats, range: RangeKey) -> String`. Direkt am Anfang:

```rust
    if stats.commits == 0 {
        return rs::shell(
            "Repo-Aktivität",
            &esc(&stats.name),
            &format!("{} · {}", esc(&stats.source), range.label()),
            "<p class=\"rp-lede\">Keine Commits in diesem Zeitraum.</p>",
            "Erstellt mit Inspector Rust, orientiert an repo2viz.",
        );
    }
```

Nach `author_rows` zusätzlich:

```rust
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
```

Im `body`-`format!` nach der Stunden-Sektion einfügen:

```
<section><h2>Heatmap Wochentag × Stunde</h2>{heat}</section>
{calendar}
```

und am Ende (nach Top-Mitwirkende):

```
<section><h2>Hotspots (viel geändert, ≤ 2 Autoren)</h2><table>
<thead><tr><th>Datei</th><th>Änderungen</th><th>Autoren</th></tr></thead><tbody>{hot}</tbody></table></section>
<section><h2>Bus-Faktor je Verzeichnis</h2><table>
<thead><tr><th>Verzeichnis</th><th>Commits</th><th>Autoren</th><th>Bus-Faktor</th></tr></thead><tbody>{dirs}</tbody></table></section>
<section><h2>Co-Change</h2><table>
<thead><tr><th>Dateipaar</th><th>Gemeinsam</th></tr></thead><tbody>{co}</tbody></table></section>
```

mit den Argumenten `heat = heatmap_svg(&stats.heatmap), calendar = calendar, hot = hot_rows, dirs = dir_rows, co = co_rows`. In `rs::stats(&[…])` eine Kachel ergänzen: `rs::Stat { label: "Bus-Faktor", value: stats.bus_factor.to_string(), unit: None },`. Den Untertitel in `rs::shell` auf `&format!("{} · {} · {} → {}", esc(&stats.source), range.label(), esc(&stats.first_commit), esc(&stats.last_commit))` ändern. `REPO_CSS` um `.heat, .cal { display:block; max-width:100% }` ergänzen; die bestehende Regel `td:nth-child(2), td:nth-child(3) …` um `td:nth-child(4), th:nth-child(4)` erweitern.

`commands.rs` → `repo_export`: `build_html(&stats)` → `build_html(&stats, range)`.

- [ ] **Step 4: Grün + Sichtprüfung**

Run: `cargo test -p inspector-rust-core --lib repo_stats`
Expected: PASS.
Run: `IR_DUMP_DIR=/private/tmp/claude-501/-Users-martin-claude-inspector-rust/9b7140e9-a82c-4bf7-a155-49d476347e56/scratchpad cargo test -p inspector-rust-core --lib dump_for_a_sight_check -- --ignored`, dann die HTML-Datei über `python3 -m http.server` im Scratchpad öffnen und mit Chrome headless `--screenshot` ansehen: Heatmap, Kalender, drei neue Tabellen sichtbar; kein Element überläuft die A4-Breite.

- [ ] **Step 5: Mutationsprobe** — in `calendar_svg` `(o + 3).rem_euclid(7)` → `(o + 4).rem_euclid(7)`: `calendar_svg_places_…` muss rot werden (Sonntag rutscht in die nächste Woche → 2 Spalten, gleiche Rect-Zahl; falls grün: Test um `assert!(svg.contains("x=\"0\""))` für beide Rects ergänzen, bevor weitergegangen wird). Zurücksetzen.

- [ ] **Step 6: Commit**

```bash
git add core/rust-lib/src/repo_stats.rs core/rust-lib/src/commands.rs
git diff --cached --stat
git commit -m "feat(repo): export heatmap, calendar, hotspots, bus factor, co-change per range

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01WbJj9S9wRyEvzQE8REMWB4"
```

---

### Task 6: Frontend-IPC-Typen + reine Anzeigehelfer

**Files:**
- Modify: `core/frontend/src/lib/ipc.ts:2850-2889` (repo-Block)
- Modify: `core/frontend/src/lib/repo.ts`
- Modify: `core/frontend/src/lib/repo.test.ts` (existiert; sonst anlegen)
- Modify: `core/frontend/src/lib/ipc.test.ts` (Vertrags-Pins für die geänderten/neuen Befehle)

**Interfaces:**
- Consumes: Rust-Wire-Formate aus Task 3/4.
- Produces (TS):
  - Types `RangeKey = "d30" | "d90" | "d180" | "y1" | "all"`, `RepoDayCount`, `RepoHotspot`, `RepoDirStat`, `RepoCoChange`, erweitertes `RepoStats` (+ `heatmap: number[][]`, `calendar`, `hotspots`, `bus_factor`, `dir_bus_factor`, `co_change`), `RangedStats { range: RangeKey; stats: RepoStats }`, `RepoAnalysis { name; source; github: RepoUrl | null; ranges: RangedStats[] }`, `RepoConfig { clone_dir: string; has_token: boolean; gh_available: boolean }`, `RepoProgress { op: "analyze" | "clone"; phase: string; percent: number }`.
  - `repoAnalyze(target: string | null): Promise<RepoAnalysis>`, `repoExport(stats: RepoStats, range: RangeKey, format: "html" | "pdf"): Promise<string>`, `repoClone(url: string): Promise<string>`, `getRepoConfig(): Promise<RepoConfig>`, `setRepoCloneDir(dir: string): Promise<void>`, `setGithubToken(token: string): Promise<void>`.
  - In `repo.ts`: `RANGES: readonly { key: RangeKey; label: string }[]`, `heatLevel(v: number, max: number): 0 | 1 | 2 | 3 | 4`, `calendarCells(days: { date: string; commits: number }[]): { col: number; row: number; date: string; commits: number }[]`, `repoErrorHint(err: string): { title: string; body: string }`.

- [ ] **Step 1: Tests** — in `core/frontend/src/lib/repo.test.ts` anhängen (Datei ggf. mit `import { describe, it, expect } from "vitest";` anlegen):

```ts
import { RANGES, heatLevel, calendarCells, repoErrorHint } from "./repo";

describe("repo ranges + heat + calendar", () => {
  it("offers the five ranges in order", () => {
    expect(RANGES.map((r) => r.key)).toEqual(["d30", "d90", "d180", "y1", "all"]);
  });
  it("maps a value to 0..4 and never divides by zero", () => {
    expect(heatLevel(0, 10)).toBe(0);
    expect(heatLevel(10, 10)).toBe(4);
    expect(heatLevel(1, 10)).toBe(1);
    expect(heatLevel(5, 0)).toBe(0);
  });
  it("places days Monday-first in week columns (same as Rust calendar_svg)", () => {
    const cells = calendarCells([
      { date: "2026-08-24", commits: 2 }, // Monday
      { date: "2026-08-30", commits: 1 }, // Sunday
      { date: "2026-08-31", commits: 1 }, // next Monday
    ]);
    expect(cells.map((c) => [c.col, c.row])).toEqual([[0, 0], [0, 6], [1, 0]]);
    expect(calendarCells([])).toEqual([]);
  });
  it("turns sentinels into human hints", () => {
    expect(repoErrorHint("repo.auth: remote: Repository not found").title).toMatch(/Kein Zugriff/);
    expect(repoErrorHint("repo.network: Could not resolve host").title).toMatch(/Keine Verbindung/);
    expect(repoErrorHint("repo.no_target").title).toMatch(/Kein Repository/);
    expect(repoErrorHint("irgendwas").title).toMatch(/fehlgeschlagen/);
  });
});
```

In `ipc.test.ts` beim Muster der bestehenden Pins (Datei zuerst lesen, dasselbe `invoke`-Mock verwenden) ergänzen: `repoExport(stats, "d90", "pdf")` → `invoke("repo_export", { stats, range: "d90", format: "pdf" })`; `repoClone("u")` → `invoke("repo_clone", { url: "u" })`; `setRepoCloneDir("/x")` → `invoke("set_repo_clone_dir", { dir: "/x" })`; `setGithubToken("t")` → `invoke("set_github_token", { token: "t" })`.

- [ ] **Step 2: Rot prüfen**

Run: `cd core/frontend && npx vitest run src/lib/repo.test.ts src/lib/ipc.test.ts`
Expected: FAIL (fehlende Exporte).

- [ ] **Step 3: Implementieren.** `repo.ts` anhängen:

```ts
import type { RangeKey } from "./ipc";

export const RANGES: readonly { key: RangeKey; label: string }[] = [
  { key: "d30", label: "30 T" },
  { key: "d90", label: "90 T" },
  { key: "d180", label: "180 T" },
  { key: "y1", label: "1 J" },
  { key: "all", label: "Gesamt" },
];

/** 0 (none) … 4 (max) — four visible steps like GitHub's calendar. */
export function heatLevel(v: number, max: number): 0 | 1 | 2 | 3 | 4 {
  if (v <= 0 || max <= 0) return 0;
  return Math.max(1, Math.min(4, Math.ceil((v / max) * 4))) as 1 | 2 | 3 | 4;
}

function dayOrdinal(date: string): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(date);
  if (!m) return null;
  return Math.floor(Date.UTC(+m[1], +m[2] - 1, +m[3]) / 86_400_000);
}

/** Week-column / weekday-row placement, Monday first — mirrors Rust
 *  `calendar_svg` (1970-01-01 was a Thursday → weekday = (ord+3) mod 7). */
export function calendarCells(days: readonly { date: string; commits: number }[]) {
  const parsed = days
    .map((d) => ({ d, o: dayOrdinal(d.date) }))
    .filter((x): x is { d: { date: string; commits: number }; o: number } => x.o !== null);
  if (parsed.length === 0) return [];
  const wd = (o: number) => (((o + 3) % 7) + 7) % 7;
  const first = Math.min(...parsed.map((x) => x.o));
  const start = first - wd(first);
  return parsed.map(({ d, o }) => ({
    col: Math.floor((o - start) / 7),
    row: wd(o),
    date: d.date,
    commits: d.commits,
  }));
}

export function repoErrorHint(err: string): { title: string; body: string } {
  if (err.includes("repo.no_target"))
    return {
      title: "Kein Repository.",
      body: "Eine GitHub-URL einfügen — oder im Finder einen Ordner mit .git auswählen.",
    };
  if (err.startsWith("repo.auth"))
    return {
      title: "Kein Zugriff auf das Repository.",
      body: "Privat oder nicht vorhanden. `gh auth login` im Terminal ausführen oder ein Token in Settings → Repositories hinterlegen.",
    };
  if (err.startsWith("repo.network"))
    return { title: "Keine Verbindung zu GitHub.", body: "Netz prüfen und mit R erneut versuchen." };
  return { title: "Analyse fehlgeschlagen", body: err.replace(/^repo\.git:\s*/, "") };
}
```

`ipc.ts`: den repo-Block (Zeile 2850–2889) ersetzen durch:

```ts
// ── repo — git activity stats (v0.123.0; ranges + clone v0.183.0) ──────────

import type { RepoUrl } from "./repo-url";

export type RangeKey = "d30" | "d90" | "d180" | "y1" | "all";
export interface RepoMonth { month: string; commits: number }
export interface RepoFile { path: string; changes: number; churn: number }
export interface RepoExt { ext: string; commits: number; churn: number }
export interface RepoAuthor { name: string; commits: number; churn: number }
export interface RepoCat { cat: string; commits: number }
export interface RepoDayCount { date: string; commits: number }
export interface RepoHotspot { path: string; changes: number; authors: number }
export interface RepoDirStat { dir: string; commits: number; authors: number; bus_factor: number }
export interface RepoCoChange { a: string; b: string; count: number }
export interface RepoStats {
  name: string;
  source: string;
  commits: number;
  contributors: number;
  first_commit: string;
  last_commit: string;
  active_days: number;
  insertions: number;
  deletions: number;
  by_weekday: number[];
  by_hour: number[];
  by_month: RepoMonth[];
  top_files: RepoFile[];
  top_exts: RepoExt[];
  top_authors: RepoAuthor[];
  categories: RepoCat[];
  longest_streak: number;
  avg_msg_len: number;
  heatmap: number[][];
  calendar: RepoDayCount[];
  hotspots: RepoHotspot[];
  bus_factor: number;
  dir_bus_factor: RepoDirStat[];
  co_change: RepoCoChange[];
}
export interface RangedStats { range: RangeKey; stats: RepoStats }
export interface RepoAnalysis { name: string; source: string; github: RepoUrl | null; ranges: RangedStats[] }
export interface RepoConfig { clone_dir: string; has_token: boolean; gh_available: boolean }
export interface RepoProgress { op: "analyze" | "clone"; phase: string; percent: number }

/** Analyse a repo. `target`: a git URL, a local path, or null for the
 *  Finder-selected .git folder. Sentinel "repo.no_target" = nothing to scan. */
export function repoAnalyze(target: string | null): Promise<RepoAnalysis> {
  return invoke("repo_analyze", { target });
}
/** Write one range's stats to ~/Downloads (no re-clone); returns the path. */
export function repoExport(stats: RepoStats, range: RangeKey, format: "html" | "pdf" = "html"): Promise<string> {
  return invoke("repo_export", { stats, range, format });
}
/** Clone into the configured folder (`name (2)` … when taken); returns the path. */
export function repoClone(url: string): Promise<string> {
  return invoke("repo_clone", { url });
}
export function getRepoConfig(): Promise<RepoConfig> {
  return invoke("get_repo_config");
}
export function setRepoCloneDir(dir: string): Promise<void> {
  return invoke("set_repo_clone_dir", { dir });
}
export function setGithubToken(token: string): Promise<void> {
  return invoke("set_github_token", { token });
}
```

Den `import type { RepoUrl }` an den Dateikopf zu den übrigen Imports verschieben (keine Imports mitten in der Datei).

- [ ] **Step 4: Grün prüfen**

Run: `cd core/frontend && npx vitest run src/lib/repo.test.ts src/lib/ipc.test.ts && npx tsc --noEmit`
Expected: Tests PASS. `tsc` meldet Fehler **nur** in `RepoPanel.tsx` (alter `repoExport`-Aufruf, `stats`-Typ) — die behebt Task 7. Andere Fehler sofort beheben.

- [ ] **Step 5: Commit** (zusammen mit Task 7, damit `tsc` im Commit grün ist — hier NICHT committen, sondern direkt Task 7 anschließen).

---

### Task 7: RepoPanel — Zeiträume, neue Karten, Klonen, Fortschritt, Fehlerhinweise

**Files:**
- Modify: `core/frontend/src/components/RepoPanel.tsx`
- Create: `core/frontend/src/components/RepoPanel.test.tsx`

**Interfaces:**
- Consumes: Task 6 (`repoAnalyze`, `repoExport`, `repoClone`, `RepoAnalysis`, `RangeKey`, `RANGES`, `heatLevel`, `calendarCells`, `repoErrorHint`), Event `repo-progress`.
- Produces: `RepoPanel` Props unverändert (`arg`, `autoExport`, `focused`, `onExit`).

- [ ] **Step 1: Test schreiben** — `core/frontend/src/components/RepoPanel.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@testing-library/react";
import type { RepoAnalysis, RepoStats, RangeKey } from "../lib/ipc";

const repoAnalyze = vi.fn<(t: string | null) => Promise<RepoAnalysis>>();
const repoExport = vi.fn<(s: RepoStats, r: RangeKey, f: "html" | "pdf") => Promise<string>>();
const repoClone = vi.fn<(u: string) => Promise<string>>();
vi.mock("../lib/ipc", () => ({
  repoAnalyze: (t: string | null) => repoAnalyze(t),
  repoExport: (s: RepoStats, r: RangeKey, f: "html" | "pdf") => repoExport(s, r, f),
  repoClone: (u: string) => repoClone(u),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => undefined }));

import { RepoPanel } from "./RepoPanel";

function stats(commits: number): RepoStats {
  return {
    name: "r", source: "https://github.com/o/r", commits, contributors: commits ? 1 : 0,
    first_commit: "2026-08-01T00:00:00+00:00", last_commit: "2026-08-24T00:00:00+00:00",
    active_days: commits, insertions: 1, deletions: 0,
    by_weekday: [1, 0, 0, 0, 0, 0, 0], by_hour: Array(24).fill(0), by_month: [],
    top_files: [], top_exts: [], top_authors: [], categories: [], longest_streak: 1, avg_msg_len: 3,
    heatmap: Array.from({ length: 7 }, () => Array(24).fill(0)), calendar: [],
    hotspots: [{ path: "src/a.rs", changes: 4, authors: 1 }], bus_factor: 1,
    dir_bus_factor: [], co_change: [],
  };
}
const analysis: RepoAnalysis = {
  name: "r", source: "https://github.com/o/r",
  github: { owner: "o", repo: "r", web_url: "https://github.com/o/r", clone_url: "https://github.com/o/r.git" },
  ranges: [
    { range: "d30", stats: stats(0) },
    { range: "d90", stats: stats(2) },
    { range: "d180", stats: stats(2) },
    { range: "y1", stats: stats(3) },
    { range: "all", stats: stats(7) },
  ],
};

beforeEach(() => {
  repoAnalyze.mockResolvedValue(analysis);
  repoExport.mockResolvedValue("/Users/u/Downloads/o-r-activity.html");
  repoClone.mockResolvedValue("/Users/u/claude/r (2)");
});
afterEach(() => { cleanup(); vi.clearAllMocks(); });

describe("RepoPanel", () => {
  it("starts on 'Gesamt' and switches ranges without re-analysing", async () => {
    const v = render(<RepoPanel arg="https://github.com/o/r" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => expect(v.getAllByText("7").length).toBeGreaterThan(0));
    fireEvent.click(v.getByRole("button", { name: "90 T" }));
    await waitFor(() => expect(v.getAllByText("2").length).toBeGreaterThan(0));
    expect(repoAnalyze).toHaveBeenCalledTimes(1);
  });
  it("shows the empty-range note instead of empty cards", async () => {
    const v = render(<RepoPanel arg="https://github.com/o/r" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => v.getByRole("button", { name: "30 T" }));
    fireEvent.click(v.getByRole("button", { name: "30 T" }));
    expect(v.getByText(/Keine Commits in diesem Zeitraum/)).toBeTruthy();
  });
  it("exports the SELECTED range's stats", async () => {
    const v = render(<RepoPanel arg="https://github.com/o/r" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => v.getByRole("button", { name: "1 J" }));
    fireEvent.click(v.getByRole("button", { name: "1 J" }));
    fireEvent.keyDown(window, { key: "p", metaKey: true });
    await waitFor(() => expect(repoExport).toHaveBeenCalled());
    const [s, r, f] = repoExport.mock.calls[0];
    expect([s.commits, r, f]).toEqual([3, "y1", "pdf"]);
  });
  it("clones with ⌘K and reports the folder it created", async () => {
    const v = render(<RepoPanel arg="https://github.com/o/r" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => v.getByRole("button", { name: /Klonen/ }));
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    await waitFor(() => expect(repoClone).toHaveBeenCalledWith("https://github.com/o/r"));
    await waitFor(() => expect(v.getByText(/r \(2\)/)).toBeTruthy());
  });
  it("offers no clone for a local repo", async () => {
    repoAnalyze.mockResolvedValue({ ...analysis, github: null });
    const v = render(<RepoPanel arg="~/x" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => v.getByRole("button", { name: "Gesamt" }));
    expect(v.queryByRole("button", { name: /Klonen/ })).toBeNull();
  });
  it("turns an auth failure into the gh-login hint", async () => {
    repoAnalyze.mockRejectedValue("repo.auth: remote: Repository not found");
    const v = render(<RepoPanel arg="https://github.com/o/private" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => expect(v.getByText(/Kein Zugriff/)).toBeTruthy());
  });
});
```

- [ ] **Step 2: Rot prüfen**

Run: `cd core/frontend && npx vitest run src/components/RepoPanel.test.tsx`
Expected: FAIL (Zeitraum-Knöpfe fehlen, `repoExport`-Aufruf hat alte Form).

- [ ] **Step 3: Implementieren** — Änderungen in `RepoPanel.tsx`:

1. Imports: `import { listen } from "@tauri-apps/api/event";` ergänzen; aus `lucide-react` zusätzlich `Download`; aus `../lib/ipc` `repoAnalyze, repoExport, repoClone, type RepoAnalysis, type RepoProgress, type RangeKey`; aus `../lib/repo` zusätzlich `RANGES, heatLevel, calendarCells, repoErrorHint`.
2. State `stats` ersetzen durch `const [analysis, setAnalysis] = useState<RepoAnalysis | null>(null);` und `const [range, setRange] = useState<RangeKey>("all");`, abgeleitet `const stats = analysis?.ranges.find((r) => r.range === range)?.stats ?? null;`. In `run()` `setStats(s)` → `setAnalysis(s)`.
3. Fortschritt:

```tsx
  const [progress, setProgress] = useState<RepoProgress | null>(null);
  useEffect(() => {
    let off: (() => void) | undefined;
    void listen<RepoProgress>("repo-progress", (e) => setProgress(e.payload)).then((u) => (off = u));
    return () => off?.();
  }, []);
```

Im Lade-Zustand unter `{phase}` anzeigen: `{progress?.op === "analyze" && <p className="text-[11px] tabular-nums text-[var(--color-muted)]">{progress.phase} · {progress.percent} %</p>}`.
4. Export: `doExport` nutzt den gewählten Zeitraum:

```tsx
  const doExport = useCallback(
    (fmt: ExportFormat = "html") => {
      if (!stats) return;
      setExporting(fmt);
      setNote("Exportiere…");
      repoExport(stats, range, fmt === "pdf" ? "pdf" : "html")
        .then((path) => setNote(`Gespeichert: ${path.split("/").pop()}`))
        .catch((e) => setNote(String(e)))
        .finally(() => setExporting(null));
    },
    [stats, range],
  );
```

5. Klonen:

```tsx
  const [cloning, setCloning] = useState(false);
  const doClone = useCallback(() => {
    const url = analysis?.github?.web_url;
    if (!url || cloning) return;
    setCloning(true);
    setNote("Klone…");
    repoClone(url)
      .then((path) => setNote(`Geklont nach ${path}`))
      .catch((e) => setNote(repoErrorHint(String(e)).title))
      .finally(() => {
        setCloning(false);
        setProgress(null);
      });
  }, [analysis, cloning]);
```

Im Keydown-Handler vor dem `e`-Zweig: `} else if (chord && (e.key === "k" || e.key === "K")) { e.preventDefault(); doClone(); }` und `doClone` in die Effekt-Abhängigkeiten aufnehmen. Den nackten-`e`-Zweig (`!typing && …`) unverändert lassen.
6. Fehlerzustand: den `noTarget`-Block ersetzen durch

```tsx
  if (err) {
    const hint = repoErrorHint(err);
    return (
      <Shell focused={focused}>
        <div className="rounded-xl border border-[var(--color-border)] p-4">
          <p className="text-[12px] font-medium">{hint.title}</p>
          <p className="mt-1 text-[11px] leading-snug text-[var(--color-muted)]">{hint.body}</p>
        </div>
      </Shell>
    );
  }
  if (!analysis || !stats) return <Shell focused={focused}>{null}</Shell>;
```

7. Unter der Kopfzeile (vor `{note && …}`) Zeitraum-Chips und Klon-Knopf:

```tsx
      <div className="flex items-center gap-1" role="group" aria-label="Zeitraum">
        {RANGES.map((r) => (
          <button
            key={r.key}
            type="button"
            onClick={() => setRange(r.key)}
            aria-pressed={range === r.key}
            className={
              "rounded-full border px-2 py-0.5 text-[10px] transition-colors duration-(--duration-fast) ease-sharp " +
              (range === r.key
                ? "border-[var(--color-accent)] bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                : "border-[var(--color-border)] text-[var(--color-muted)] hover:text-[var(--color-fg)]")
            }
          >
            {r.label}
          </button>
        ))}
      </div>
      <p className="-mt-1 text-[10px] text-[var(--color-muted)]">
        Zeiträume zählen ab dem letzten Commit ({shortDate(analysis.ranges.find((r) => r.range === "all")?.stats.last_commit ?? "")}).
      </p>
      {analysis.github && (
        <button
          type="button"
          onClick={doClone}
          disabled={cloning}
          className="flex items-center justify-center gap-1.5 rounded-lg border border-[var(--color-border)] px-2 py-1.5 text-[12px] hover:border-[var(--color-accent)] disabled:opacity-60"
        >
          <Download size={13} />
          {cloning
            ? progress?.op === "clone" ? `${progress.phase} · ${progress.percent} %` : "Klone…"
            : `Klonen (${analysis.github.owner}/${analysis.github.repo})`}
        </button>
      )}
```

8. Leerer Zeitraum: direkt nach dem `ExportRow` —

```tsx
      {stats.commits === 0 ? (
        <p className="rounded-xl border border-[var(--color-border)] p-4 text-[12px] text-[var(--color-muted)]">
          Keine Commits in diesem Zeitraum.
        </p>
      ) : (
        <>
          {/* bestehende KPI-/Chart-/Listen-Karten unverändert hier hinein */}
          {/* + die neuen Karten aus Punkt 9 */}
        </>
      )}
```

Die bestehenden Karten (KPI-Kacheln bis zu den zwei Tabellen) in dieses Fragment verschieben; die Fußzeile (`… Zeilen bewegt …`) bleibt außerhalb. In der KPI-Kachelgruppe `<Kpi value={String(stats.bus_factor)} label="Bus-Faktor" />` ergänzen (Raster bleibt `grid-cols-3`, 7 Kacheln → letzte Zeile hat eine; stattdessen „Zeilen aus" behalten und das Raster auf `grid-cols-4` umstellen, 8 Plätze bei 7 Kacheln ist akzeptabel).
9. Neue Karten (im Fragment, nach „Uhrzeit"):

```tsx
      <Card title="Heatmap Wochentag × Stunde">
        <div className="grid gap-[2px]" style={{ gridTemplateColumns: "18px repeat(24, 1fr)" }}>
          {stats.heatmap.map((row, d) => {
            const max = Math.max(1, ...stats.heatmap.flat());
            return [
              <span key={`l${d}`} className="text-[9px] text-[var(--color-muted)]">{WEEKDAY_LABELS[d]}</span>,
              ...row.map((v, h) => (
                <span
                  key={`${d}-${h}`}
                  title={`${WEEKDAY_LABELS[d]} ${h} Uhr: ${v}`}
                  className="aspect-square rounded-[2px]"
                  style={{ background: "var(--color-accent)", opacity: [0.07, 0.3, 0.5, 0.75, 1][heatLevel(v, max)] }}
                />
              )),
            ];
          })}
        </div>
      </Card>
      {(range === "y1" || range === "all") && stats.calendar.length > 0 && (
        <Card title="Beitragskalender">
          {(() => {
            const cells = calendarCells(stats.calendar);
            const cols = Math.max(...cells.map((c) => c.col)) + 1;
            const max = Math.max(1, ...cells.map((c) => c.commits));
            return (
              <svg viewBox={`0 0 ${cols * 10} 70`} className="w-full" role="img" aria-label="Beitragskalender">
                {cells.map((c) => (
                  <rect key={c.date} x={c.col * 10} y={c.row * 10} width={8} height={8} rx={1.5} fill="#2e9e5b" fillOpacity={[0.07, 0.3, 0.5, 0.75, 1][heatLevel(c.commits, max)]}>
                    <title>{`${c.date}: ${c.commits}`}</title>
                  </rect>
                ))}
              </svg>
            );
          })()}
        </Card>
      )}
      {stats.hotspots.length > 0 && (
        <Card title="Hotspots · viel geändert, ≤ 2 Autoren">
          <RankList
            rows={stats.hotspots.map((h) => ({ label: h.path, bar: barPct(h.changes, stats.hotspots[0].changes), value: `${h.changes}× · ${h.authors} Autor${h.authors === 1 ? "" : "en"}` }))}
            mono
          />
        </Card>
      )}
      {stats.dir_bus_factor.length > 0 && (
        <Card title={`Bus-Faktor · gesamt ${stats.bus_factor}`}>
          <div className="flex flex-col gap-0.5 text-[11px]">
            {stats.dir_bus_factor.map((d) => (
              <div key={d.dir} className="flex justify-between gap-2">
                <span className="truncate font-[var(--font-mono)]">{d.dir}</span>
                <span className="shrink-0 tabular-nums text-[var(--color-muted)]">{formatNum(d.commits)} · {d.authors} Autoren · BF {d.bus_factor}</span>
              </div>
            ))}
          </div>
        </Card>
      )}
      {stats.co_change.length > 0 && (
        <Card title="Co-Change · oft zusammen geändert">
          <div className="flex flex-col gap-0.5 text-[11px]">
            {stats.co_change.map((p) => (
              <div key={`${p.a}|${p.b}`} className="flex justify-between gap-2">
                <span className="min-w-0 truncate font-[var(--font-mono)]" title={`${p.a} ↔ ${p.b}`}>{p.a} ↔ {p.b}</span>
                <span className="shrink-0 tabular-nums text-[var(--color-muted)]">{p.count}×</span>
              </div>
            ))}
          </div>
        </Card>
      )}
```

10. Fußzeilen-Hinweis `{focused && …}`: Text auf `⌘E HTML · ⌘P PDF · ⌘K klonen · Esc schließen` ändern (`⌘K` nur, wenn `analysis.github`).

- [ ] **Step 4: Grün prüfen**

Run: `cd core/frontend && npx vitest run src/components/RepoPanel.test.tsx src/lib/repo.test.ts src/lib/ipc.test.ts && npx tsc --noEmit && npx eslint src/components/RepoPanel.tsx src/lib/repo.ts src/lib/ipc.ts src/lib/repo-url.ts`
Expected: alle PASS, 0 Fehler.

- [ ] **Step 5: Mutationsprobe** — in `doExport` `range` → `"all"` fest: Test `exports the SELECTED range's stats` muss rot werden. Zurücksetzen.

- [ ] **Step 6: Commit (Tasks 6 + 7)**

```bash
git add core/frontend/src/lib/ipc.ts core/frontend/src/lib/ipc.test.ts core/frontend/src/lib/repo.ts core/frontend/src/lib/repo.test.ts core/frontend/src/components/RepoPanel.tsx core/frontend/src/components/RepoPanel.test.tsx
git diff --cached --stat
git commit -m "feat(repo): panel ranges, heatmap, calendar, hotspots, bus factor, co-change, clone (⌘K)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01WbJj9S9wRyEvzQE8REMWB4"
```

---

### Task 8: `repo-url`-Zeile im Suchfeld + Knopf in der Clip-Vorschau

**Files:**
- Modify: `core/frontend/src/lib/types.ts` (ListEntry-Union Zeile ~231; `CUSTOM_COMMAND_KINDS` Zeile ~306)
- Modify: `core/frontend/src/App.tsx` (neben `socialEntry` Zeile ~2616; `combined` Zeile ~2731 + Abhängigkeitsliste; `activate` Zeile ~4419; `PreviewPanel`-Props Zeile ~5380)
- Modify: `core/frontend/src/components/HistoryItem.tsx` (Icon Zeile ~104, Label Zeile ~190)
- Modify: `core/frontend/src/components/PreviewPanel.tsx` (Vorschau-Zweig neben `social` Zeile ~410; Clip-Textzweig Zeile ~1356; Props)
- Create: `core/frontend/src/lib/repo-entry.test.ts`

**Interfaces:**
- Consumes: Task 1 `parseRepoUrl`, `findRepoUrl`, `RepoUrl`.
- Produces: `ListEntry` `{ kind: "repo-url"; data: RepoUrl }`; `PreviewPanel` Prop `onAnalyzeRepo?: (webUrl: string) => void`; reine Funktion `repoUrlEntry(query: string, hasCommand: boolean): ListEntry | null` in `core/frontend/src/lib/repo-url.ts`.

- [ ] **Step 1: Test schreiben** — `core/frontend/src/lib/repo-entry.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import { repoUrlEntry } from "./repo-url";
import { CUSTOM_COMMAND_KINDS } from "./types";

describe("repo-url list entry", () => {
  it("appears for a bare repo URL and carries owner/repo", () => {
    const e = repoUrlEntry("https://github.com/o/r", false);
    expect(e).toMatchObject({ kind: "repo-url", data: { owner: "o", repo: "r" } });
  });
  it("does not appear while an explicit command is typed (repo <url> has its own row)", () => {
    expect(repoUrlEntry("https://github.com/o/r", true)).toBeNull();
  });
  it("does not appear for issue links or text", () => {
    expect(repoUrlEntry("https://github.com/o/r/issues/1", false)).toBeNull();
    expect(repoUrlEntry("hallo", false)).toBeNull();
  });
  it("is a custom command row (red accent, outranks apps)", () => {
    expect(CUSTOM_COMMAND_KINDS.has("repo-url")).toBe(true);
  });
});
```

- [ ] **Step 2: Rot prüfen**

Run: `cd core/frontend && npx vitest run src/lib/repo-entry.test.ts`
Expected: FAIL (`repoUrlEntry` fehlt).

- [ ] **Step 3: Implementieren.**

`types.ts`: Union-Zeile `| { kind: "social"; data: SocialTarget };` ersetzen durch

```ts
  | { kind: "social"; data: SocialTarget }
  | { kind: "repo-url"; data: RepoUrl };
```

oben `import type { RepoUrl } from "./repo-url";` ergänzen; in `CUSTOM_COMMAND_KINDS` nach `"social",` die Zeile `"repo-url",` einfügen.

`repo-url.ts` anhängen:

```ts
import type { ListEntry } from "./types";

/** The search-bar row for a bare GitHub repo URL. Suppressed while an
 *  explicit command parses (e.g. `repo <url>` already has its own row). */
export function repoUrlEntry(query: string, hasCommand: boolean): ListEntry | null {
  if (hasCommand) return null;
  const r = parseRepoUrl(query);
  return r ? { kind: "repo-url", data: r } : null;
}
```

(Zirkulärer Typ-Import `types.ts` ↔ `repo-url.ts` ist nur `import type` und damit unkritisch.)

`App.tsx`, unter `socialEntry`:

```tsx
  // A bare GitHub repo URL → "Repo analysieren" row (v0.183.0). Enter opens
  // the `repo` panel via dispatchCommand, so the panel/canonicalisation path
  // is the one the `repo` command already uses.
  const repoUrlRow = useMemo<ListEntry | null>(
    () => repoUrlEntry(query, !!parsedCommand),
    [query, parsedCommand],
  );
```

Import `import { repoUrlEntry } from "./lib/repo-url";`. In `combined` direkt nach `...(socialEntry ? [socialEntry] : []),` → `...(repoUrlRow ? [repoUrlRow] : []),`; `repoUrlRow` in die Abhängigkeitsliste nach `socialEntry,`. In `activate` nach dem `social`-Zweig:

```tsx
      } else if (target.kind === "repo-url") {
        await dispatchCommand("repo", target.data.web_url);
        return;
```

(Prüfen, dass `activate` `async` ist: `grep -n "const activate" core/frontend/src/App.tsx`; ist es nicht async, `void dispatchCommand(...)` verwenden.)

Am `<PreviewPanel …>` (Zeile ~5380) die Prop ergänzen: `onAnalyzeRepo={(url) => void dispatchCommand("repo", url)}`.

`HistoryItem.tsx`: `GitBranch` zu den lucide-Imports; Icon-Zweig nach `social`: `if (entry.kind === "repo-url") return <GitBranch size={size} className={cls} />;`; `const isRepoUrl = entry.kind === "repo-url";`; im `label`-Ausdruck vor dem `isSocial`-Zweig:

```tsx
        : isRepoUrl && entry.kind === "repo-url"
          ? `Repo analysieren · ${entry.data.owner}/${entry.data.repo}`
```

`PreviewPanel.tsx`: Prop `onAnalyzeRepo?: (webUrl: string) => void;` in die Props-Schnittstelle und die Destrukturierung. Zweig vor `if (entry.kind === "social")`:

```tsx
  if (entry.kind === "repo-url") {
    return (
      <div className="flex h-full flex-col gap-3 p-4">
        <div className="flex items-center gap-2 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
          <GitBranch size={12} className="text-rose-500" />
          <span>GitHub-Repository</span>
        </div>
        <p className="text-[15px] font-medium">{entry.data.owner}/{entry.data.repo}</p>
        <p className="break-all font-[var(--font-mono)] text-[11px] text-[var(--color-muted)]">{entry.data.web_url}</p>
        <p className="text-[12px] leading-snug text-[var(--color-muted)]">
          Enter analysiert die Historie: Commits, Zeiträume, Heatmap, Hotspots, Bus-Faktor —
          danach Export als HTML/PDF und Klonen mit ⌘K.
        </p>
      </div>
    );
  }
```

Im Clip-Textzweig nach dem `SocialDownloadBar`-Block:

```tsx
      {(() => {
        const gh = onAnalyzeRepo ? findRepoUrl(clip.content_text) : null;
        return gh ? (
          <button
            type="button"
            onClick={() => onAnalyzeRepo!(gh.web_url)}
            className="mt-2 flex items-center gap-1.5 self-start rounded-lg border border-[var(--color-border)] px-2 py-1 text-[12px] hover:border-[var(--color-accent)]"
          >
            <GitBranch size={12} /> Repo analysieren · {gh.owner}/{gh.repo}
          </button>
        ) : null;
      })()}
```

Imports: `GitBranch` (lucide), `findRepoUrl` aus `../lib/repo-url`.

- [ ] **Step 4: Grün prüfen**

Run: `cd core/frontend && npx vitest run src/lib/repo-entry.test.ts src/components/PreviewPanel.test.tsx && npx tsc --noEmit && npx eslint src`
Expected: PASS, 0 Fehler. Falls ein bestehender Test die Zahl der `ListEntry`-Arten oder der `CUSTOM_COMMAND_KINDS` pinnt, den Pin mit der neuen Art ergänzen (nicht lockern).

- [ ] **Step 5: Mutationsprobe** — in `combined` `repoUrlRow` hinter `appEntry` verschieben: Kein Test deckt die Position ab → ergänzen: in `repo-entry.test.ts` ist die Rangfolge nicht prüfbar, weil `combined` in `App.tsx` lebt. Deshalb Quelltext-Pin hinzufügen (Muster wie andere Source-Pins im Repo, z. B. `readme-badges.test.ts`, das Dateien über `import.meta.glob` liest):

```ts
import appSrc from "../App.tsx?raw";
it("repo-url row is spliced before the app-launcher hit", () => {
  const code = appSrc.replace(/\/\/.*$/gm, "");
  expect(code.indexOf("...(repoUrlRow ?")).toBeGreaterThan(-1);
  expect(code.indexOf("...(repoUrlRow ?")).toBeLessThan(code.indexOf("...(appEntry ?"));
});
```

Prüfen, dass `?raw` für `.tsx` im Vitest liefert (die Animations-Regel warnt nur für `.css?raw`); liefert es einen leeren String, die Datei stattdessen per `readFileSync(new URL("../App.tsx", import.meta.url), "utf8")` lesen. Dann Mutation (Zeile hinter `appEntry` verschieben) → rot; zurücksetzen.

- [ ] **Step 6: Commit**

```bash
git add core/frontend/src/lib/types.ts core/frontend/src/lib/repo-url.ts core/frontend/src/lib/repo-entry.test.ts core/frontend/src/App.tsx core/frontend/src/components/HistoryItem.tsx core/frontend/src/components/PreviewPanel.tsx
git diff --cached --stat
git commit -m "feat(repo): bare GitHub URL in the search bar opens the repo panel; clip preview button

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01WbJj9S9wRyEvzQE8REMWB4"
```

---

### Task 9: Settings „Repositories" (Klon-Ordner + Token)

**Files:**
- Modify: `core/frontend/src/components/SettingsPanel.tsx` (neue Funktion `RepositoriesSection` neben `PagespeedSection` Zeile ~3529; Einbindung neben `<PagespeedSection />` Zeile ~3024)
- Modify: `core/frontend/src/lib/settings-sections.ts` (Eintrag nach `pagespeed`)
- Modify: `core/frontend/src/lib/settings-sections.test.ts` (Pin)

**Interfaces:**
- Consumes: Task 6 `getRepoConfig`, `setRepoCloneDir`, `setGithubToken`.
- Produces: Settings-Anker `settings-repos`.

- [ ] **Step 1: Test** — in `settings-sections.test.ts` anhängen:

```ts
it("finds the Repositories section by German and English names", () => {
  expect(matchSettingsSection("klonen")?.id).toBe("repos");
  expect(matchSettingsSection("repositories")?.id).toBe("repos");
  expect(matchSettingsSection("github")?.id).toBe("repos");
});
```

(Import von `matchSettingsSection` ist in der Datei vorhanden; prüfen.)

- [ ] **Step 2: Rot prüfen**

Run: `cd core/frontend && npx vitest run src/lib/settings-sections.test.ts`
Expected: FAIL.

- [ ] **Step 3: Implementieren.** Registry-Eintrag nach der `pagespeed`-Zeile:

```ts
  { id: "repos", label: "Repositories", names: ["repos", "repositories", "repository", "klonen", "clone", "github", "git", "github token"] },
```

`SettingsPanel.tsx`:

```tsx
function RepositoriesSection() {
  const [cfg, setCfg] = useState<RepoConfig | null>(null);
  const [dir, setDir] = useState("");
  const [token, setToken] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const load = () =>
    getRepoConfig()
      .then((c) => {
        setCfg(c);
        setDir(c.clone_dir);
      })
      .catch(() => {});
  useEffect(() => {
    void load();
  }, []);
  return (
    <Section
      icon={<GitBranch size={16} className="text-[var(--color-accent)]" />}
      title="Repositories"
      subtitle="Wohin „Klonen“ im `repo`-Panel ein GitHub-Repository legt, und der Zugang für private Repos."
      id="repos"
    >
      <Row label="Klon-Ordner" help="Ist der Ordner eines Repos schon belegt, entsteht „name (2)“, „name (3)“ … — nichts wird überschrieben.">
        <div className="flex gap-2">
          <input
            className="w-full rounded border border-[var(--color-border)] bg-transparent px-2 py-1 font-[var(--font-mono)] text-[12px]"
            value={dir}
            onChange={(e) => setDir(e.target.value)}
            spellCheck={false}
          />
          <button
            type="button"
            className="shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-[12px]"
            onClick={() => {
              void setRepoCloneDir(dir)
                .then(() => {
                  setNote("Ordner gespeichert.");
                  return load();
                })
                .catch((e) => setNote(String(e)));
            }}
          >
            Speichern
          </button>
        </div>
      </Row>
      <Row
        label="GitHub-Token"
        help={
          cfg?.gh_available
            ? "`gh` ist installiert — ist es eingeloggt (`gh auth login`), wird dessen Token genutzt. Ein Token hier dient als Rückfall."
            : "Für private Repos. Liegt im Schlüsselbund, geht nur an github.com und nie in eine Kommandozeile."
        }
      >
        <div className="flex gap-2">
          <input
            type="password"
            className="w-full rounded border border-[var(--color-border)] bg-transparent px-2 py-1 text-[12px]"
            placeholder={cfg?.has_token ? "gespeichert" : "kein Token hinterlegt"}
            value={token}
            onChange={(e) => setToken(e.target.value)}
            spellCheck={false}
          />
          <button
            type="button"
            className="shrink-0 rounded border border-[var(--color-border)] px-2 py-1 text-[12px]"
            onClick={() => {
              void setGithubToken(token)
                .then(() => {
                  setNote(token.trim() ? "Token gespeichert." : "Token entfernt.");
                  setToken("");
                  return load();
                })
                .catch((e) => setNote(String(e)));
            }}
          >
            Speichern
          </button>
        </div>
      </Row>
      {note && <p className="mt-2 text-[11px] text-[var(--color-muted)]">{note}</p>}
    </Section>
  );
}
```

Imports: `GitBranch` (lucide, falls nicht vorhanden), `getRepoConfig, setRepoCloneDir, setGithubToken, type RepoConfig` aus `../lib/ipc`. Einbindung: direkt nach `<PagespeedSection />` die Zeile `<RepositoriesSection />`.

- [ ] **Step 4: Grün prüfen**

Run: `cd core/frontend && npx vitest run src/lib/settings-sections.test.ts && npx tsc --noEmit && npx eslint src/components/SettingsPanel.tsx`
Expected: PASS, 0 Fehler.

- [ ] **Step 5: Commit**

```bash
git add core/frontend/src/components/SettingsPanel.tsx core/frontend/src/lib/settings-sections.ts core/frontend/src/lib/settings-sections.test.ts
git diff --cached --stat
git commit -m "feat(repo): Settings → Repositories (clone folder + GitHub token)

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01WbJj9S9wRyEvzQE8REMWB4"
```

---

### Task 10: Doku, Gesamtprüfung, Version, Installation, Push

**Files:**
- Modify: `core/frontend/src/lib/commandDocs.ts` (CommandDoc `repo` — ⚠️ Datei enthält fremde dezibel-Änderungen)
- Modify: `features.txt`, `CHANGELOG.md` (⚠️ fremder Hunk), `CLAUDE.md` (Abschnitt „repo / export"), `docs/repo.md`
- Modify: Versionsdateien (wie beim letzten Release: `grep -rn "0.182.0" --include=*.json --include=*.toml . | grep -v node_modules | grep -v target`)
- README-Matrix über `node scripts/gen-docs.mjs`

- [ ] **Step 1: CommandDoc `repo`** — in `commandDocs.ts` den Eintrag mit `command: "repo"` suchen und ergänzen: Beschreibung um „eine GitHub-URL auch ohne `repo` davor; Zeiträume 30 T/90 T/180 T/1 J/Gesamt ab dem letzten Commit; Heatmap, Kalender, Hotspots, Bus-Faktor, Co-Change; ⌘K klont in den Klon-Ordner (Settings → Repositories), belegte Ordner werden zu `name (2)` …; private Repos über `gh auth login` oder Token". Beispiele ergänzen: `{ input: "https://github.com/pepperonas/repo2viz", result: "Repo-Zeile ohne Stichwort, Enter analysiert" }`, `{ input: "repo https://github.com/o/r", result: "⌘K klont nach ~/claude/r" }`. Caveat ergänzen: „Analysierte Repos bleiben im Cache (max. 5 bzw. 2 GB), damit die nächste Analyse nur noch `git fetch` braucht." Version auf `0.183.0` für die Erweiterung setzen, falls das Feld existiert.

- [ ] **Step 2: Übrige Doku**
  - `features.txt`: Zeile ergänzen `GitHub-URL im Suchfeld → Repo-Statistik (Zeiträume, Heatmap, Kalender, Hotspots, Bus-Faktor, Co-Change), Export HTML/PDF, Klonen mit ⌘K in einen festen Ordner (name (2) … bei Belegung), private Repos via gh/Token`.
  - `CHANGELOG.md`: neuer Abschnitt `## [0.183.0] - 2026-09-25` mit Added (URL-Erkennung, Zeiträume, Karten, Klonen, Settings Repositories, Cache) und Changed (`repo_export` nutzt die Panel-Statistik statt erneut zu klonen; GitHub-Analysen laufen über einen Cache statt eines temporären bare-Klons).
  - `CLAUDE.md`: den Abschnitt „### repo / export — git activity stats" um die v0.183.0-Architektur ergänzen: gespiegelte URL-Erkennung mit gemeinsamer Fixture, Klon-Cache + `origin/HEAD`-Falle (No-Checkout-Klon hat einen veralteten lokalen Branch nach `fetch`), Token nur als `GIT_CONFIG_*`-Umgebung, `free_dir_name`, Export ohne Re-Clone, Settings-Anker `repos`.
  - `docs/repo.md`: Abschnitte „GitHub-URL direkt", „Zeiträume", „Neue Kennzahlen", „Klonen", „Private Repos", „Cache".

- [ ] **Step 3: Version** auf `0.183.0` heben (alle Stellen aus dem `grep` oben; `Cargo.lock` über `cargo update -p inspector-rust-core --offline` bzw. den ersten Build).

- [ ] **Step 4: Gesamtprüfung**

Run: `node scripts/gen-docs.mjs && bash scripts/check.sh && pnpm test`
Expected: clippy/tsc/eslint/gen-docs grün, beide Suiten grün, Badge-Update läuft durch. Bei „No space left on device": `rm -rf target/debug` und wiederholen (bekannte Falle, siehe Memory `libsqlite3-sys-bindgen-missing`).

- [ ] **Step 5: Installation + Live-Probe**

Run: `bash scripts/install-macos.sh`
Dann live prüfen: `https://github.com/pepperonas/repo2viz` ins Suchfeld → rote Zeile „Repo analysieren · pepperonas/repo2viz" steht über App-Treffern → Enter → Fortschritt, Statistik, Zeitraum-Chips schalten ohne Neuladen → ⌘P legt PDF in Downloads → ⌘K klont nach `~/claude/repo2viz (2)` (weil `~/claude/repo2viz` existiert) → zweite Analyse ist spürbar schneller (Cache). Privates Repo (eigenes privates Repo aus `gh repo list --visibility private -L 1`) analysieren → funktioniert über `gh auth token`. Einen Screenshot mit `scripts/screenshot-macos.sh repo-url "https://github.com/pepperonas/repo2viz" 8 --preview-only` erstellen und ansehen.

- [ ] **Step 6: Commit + Push** (nur eigene Hunks; dezibel-Dateien ausschließen)

```bash
git add features.txt CLAUDE.md docs/repo.md README.md README.de.md
git add -p core/frontend/src/lib/commandDocs.ts   # nur die repo-Hunks
git add -p CHANGELOG.md                            # nur der 0.183.0-Abschnitt
git add <Versionsdateien aus Step 3> Cargo.lock
git diff --cached --stat   # darf audio-level*, commands.ts, popup-strip.png NICHT enthalten
git commit -m "docs(repo): v0.183.0 — GitHub URL stats, ranges, clone; version bump

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01WbJj9S9wRyEvzQE8REMWB4"
git push
```
