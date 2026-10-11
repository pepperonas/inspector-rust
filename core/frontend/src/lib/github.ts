/**
 * `github` — pure helpers for the GitHub analytics panel: argument grammar,
 * ranges, delta / time formatting, error texts and chart geometry. The data
 * and the snapshots live in Rust (`github/`); reference docs/github.md.
 */
import type { GithubDeltas, GithubPoint, GithubSortKey, GithubStatus } from "./ipc";
import type { SeriesPoint } from "./stats-chart";

export type GithubView = "dashboard" | "repos" | "repo" | "followers" | "stars" | "trending";
export type GithubRange = "7d" | "30d" | "90d" | "1y" | "all";

export type GithubCmd =
  | { kind: "view"; view: Exclude<GithubView, "repo">; sort?: GithubSortKey }
  | { kind: "repo"; name: string }
  | { kind: "refresh" }
  | { kind: "export"; format: "json" | "csv"; includePrivate: boolean }
  | { kind: "connect" }
  | { kind: "invalid"; message: string };

export const RANGES: { id: GithubRange; label: string }[] = [
  { id: "7d", label: "7D" },
  { id: "30d", label: "30D" },
  { id: "90d", label: "90D" },
  { id: "1y", label: "1Y" },
  { id: "all", label: "ALL" },
];

export const SORTS: { id: GithubSortKey; label: string; words: string[] }[] = [
  { id: "stars", label: "Stars", words: ["stars", "sterne"] },
  { id: "forks", label: "Forks", words: ["forks"] },
  { id: "updated", label: "Recently updated", words: ["updated", "recent", "aktualisiert"] },
  { id: "name", label: "Name", words: ["name"] },
  { id: "growth7d", label: "Growth 7d", words: ["growth7d", "growth", "7d"] },
  { id: "growth30d", label: "Growth 30d", words: ["growth30d", "30d"] },
];

/** Every first word the grammar knows (Tab completion reads this). */
export const SUBCOMMANDS = [
  { value: "stats", hint: "Dashboard" },
  { value: "repos", hint: "Repository ranking" },
  { value: "repo", hint: "One repository" },
  { value: "followers", hint: "Follower history" },
  { value: "stars", hint: "Star history" },
  { value: "trending", hint: "Your fastest-growing repositories" },
  { value: "refresh", hint: "Ask GitHub now" },
  { value: "export", hint: "JSON / CSV to Downloads" },
  { value: "connect", hint: "Sign in with GitHub" },
] as const;

function sortFor(word: string | undefined): GithubSortKey | undefined {
  if (!word) return undefined;
  const w = word.toLowerCase();
  return SORTS.find((s) => s.words.includes(w))?.id;
}

/** `github …` → what to show / do. Empty and `stats` mean the dashboard. */
export function parseGithubArg(arg: string): GithubCmd {
  const words = arg.trim().split(/\s+/).filter(Boolean);
  const head = (words[0] ?? "").toLowerCase();
  switch (head) {
    case "":
    case "stats":
    case "dashboard":
      return { kind: "view", view: "dashboard" };
    case "repos":
    case "repositories": {
      const sort = sortFor(words[1]);
      return sort ? { kind: "view", view: "repos", sort } : { kind: "view", view: "repos" };
    }
    case "repo": {
      const name = words.slice(1).join(" ");
      return name ? { kind: "repo", name } : { kind: "view", view: "repos" };
    }
    case "followers":
    case "follower":
      return { kind: "view", view: "followers" };
    case "stars":
    case "sterne":
      return { kind: "view", view: "stars" };
    case "trending":
      return { kind: "view", view: "trending", sort: "growth7d" };
    case "refresh":
    case "aktualisieren":
      return { kind: "refresh" };
    case "connect":
    case "login":
      return { kind: "connect" };
    case "export": {
      const fmt = (words[1] ?? "json").toLowerCase();
      if (fmt !== "json" && fmt !== "csv") return { kind: "invalid", message: "export json | csv [private]" };
      const includePrivate = words.slice(2).some((w) => /^(private|privat|--private)$/i.test(w));
      return { kind: "export", format: fmt, includePrivate };
    }
    default:
      return { kind: "invalid", message: `Unknown: ${head} — try stats, repos, repo <name>, export` };
  }
}

/** The command row label. */
export function commandLabel(cmd: GithubCmd): string {
  switch (cmd.kind) {
    case "view":
      return {
        dashboard: "GitHub — statistics",
        repos: "GitHub — repository ranking",
        followers: "GitHub — followers",
        stars: "GitHub — stars",
        trending: "GitHub — trending repositories",
      }[cmd.view];
    case "repo":
      return `GitHub — ${cmd.name}`;
    case "refresh":
      return "GitHub — refresh now";
    case "export":
      return `GitHub — export ${cmd.format.toUpperCase()}${cmd.includePrivate ? " (incl. private)" : ""}`;
    case "connect":
      return "GitHub — connect";
    case "invalid":
      return cmd.message;
  }
}

/** Snapshot points (ms) → chart points (s). */
export function toSeries(points: GithubPoint[]): SeriesPoint[] {
  return points.map(([t, v]) => ({ t: t / 1000, v }));
}

/** `+12`, `−3`, `±0`; `—` where the history is too short (never `0`). */
export function formatDelta(d: number | null): string {
  if (d == null) return "—";
  if (d === 0) return "±0";
  return d > 0 ? `+${d.toLocaleString("en-US")}` : `−${Math.abs(d).toLocaleString("en-US")}`;
}

export function deltaTone(d: number | null): "up" | "down" | "flat" | "none" {
  if (d == null) return "none";
  return d > 0 ? "up" : d < 0 ? "down" : "flat";
}

/** All four windows unknown = the history hasn't reached 24 h yet. */
export function historyCollecting(d: GithubDeltas): boolean {
  return d.d24h == null && d.d7d == null && d.d30d == null && d.d90d == null;
}

export function formatAgo(ms: number | null, now: number): string {
  if (ms == null) return "never";
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h}h ago`;
  return `${Math.round(h / 24)}d ago`;
}

/** `in 42 min`, `in 3h`; `now` once it's due. */
export function formatIn(ms: number, now: number): string {
  const m = Math.round((ms - now) / 60_000);
  if (m <= 0) return "now";
  if (m < 60) return `in ${m} min`;
  return `in ${Math.round(m / 60)}h`;
}

export function formatClockTime(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

export function formatDate(ms: number | null): string {
  if (ms == null) return "—";
  const d = new Date(ms);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/** Plain-language text for a backend error sentinel — never echoes a token. */
export function errorText(e: string | null): string | null {
  if (!e) return null;
  if (e.includes("github.not_connected")) return "Not connected — sign in with GitHub.";
  if (e.includes("github.unauthorized")) return "GitHub rejected the token (expired or revoked). Connect again.";
  if (e.includes("github.forbidden")) return "The token lacks the permission for this.";
  if (e.includes("github.rate_limited")) return "GitHub rate limit reached.";
  if (e.includes("github.network")) return "Offline — GitHub could not be reached.";
  if (e.includes("github.not_found")) return "Not found.";
  if (e.includes("github.bad_data")) return "GitHub sent incomplete data.";
  if (e.includes("github.no_client_id"))
    return "Browser sign-in isn't configured in this build yet — use a personal access token below.";
  if (e.includes("github.device_expired")) return "The sign-in code expired. Start again.";
  if (e.includes("github.device_denied")) return "Sign-in was declined on GitHub.";
  if (e.includes("github.keychain")) return "The token couldn't be stored in the keychain.";
  if (e.includes("github.busy")) return "A refresh is already running.";
  return e.length > 160 ? `${e.slice(0, 160)}…` : e;
}

/** The one-line freshness / problem banner over the cached data. */
export function statusLine(s: GithubStatus, now: number): { tone: "ok" | "warn" | "error"; text: string } {
  const age = formatAgo(s.last_ok_ms, now);
  if (s.rate_limited_until)
    return { tone: "warn", text: `Rate limit reached · Next refresh available: ${formatClockTime(s.rate_limited_until)}` };
  const cached = s.last_ok_ms != null;
  if (s.last_error?.includes("github.network") && cached)
    return { tone: "warn", text: `Offline · Showing cached GitHub data · Last updated ${age}` };
  const err = errorText(s.last_error);
  if (err) return { tone: "error", text: cached ? `${err} · Showing cached data from ${age}` : err };
  if (s.refreshing) return { tone: "ok", text: "Refreshing…" };
  return { tone: "ok", text: `Last updated ${age}` };
}

export const SOURCE_LABEL: Record<string, string> = {
  oauth: "Signed in with GitHub",
  pat: "Personal access token",
  gh: "GitHub CLI login",
};

/** Bars for the stars-by-repo chart: the top `n`, widths relative to the max. */
export function starBars(rows: { name: string; stars: number }[], n = 10): { name: string; stars: number; frac: number }[] {
  const top = [...rows].sort((a, b) => b.stars - a.stars || a.name.localeCompare(b.name)).slice(0, n);
  const max = Math.max(1, ...top.map((r) => r.stars));
  return top.map((r) => ({ ...r, frac: r.stars / max }));
}

/** Common time domain of several series (seconds); null when empty. */
export function timeDomain(series: SeriesPoint[][]): [number, number] | null {
  const ts = series.flat().map((p) => p.t);
  if (ts.length === 0) return null;
  return [Math.min(...ts), Math.max(...ts)];
}
