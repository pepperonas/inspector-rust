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
import type { ListEntry } from "./types";

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

/** The search-bar row for a bare GitHub repo URL. Suppressed while an
 *  explicit command parses (e.g. `repo <url>` already has its own row). */
export function repoUrlEntry(query: string, hasCommand: boolean): ListEntry | null {
  if (hasCommand) return null;
  const r = parseRepoUrl(query);
  return r ? { kind: "repo-url", data: r } : null;
}
