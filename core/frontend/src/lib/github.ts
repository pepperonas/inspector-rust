/**
 * `github` — pure helpers for the GitHub analytics panel: argument grammar,
 * ranges, delta / time formatting, error texts and chart geometry. The data
 * and the snapshots live in Rust (`github/`); reference docs/github.md.
 */
import type { GithubDeltas, GithubPoint, GithubSortKey, GithubStatus } from "./ipc";
import type { SeriesPoint } from "./stats-chart";
import { t, type MessageKey } from "./i18n";

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
  { id: "stars", get label() { return t("github.sort.stars"); }, words: ["stars", "sterne"] },
  { id: "forks", get label() { return t("github.sort.forks"); }, words: ["forks"] },
  { id: "updated", get label() { return t("github.sort.updated"); }, words: ["updated", "recent", "aktualisiert"] },
  { id: "name", get label() { return t("github.sort.name"); }, words: ["name"] },
  { id: "growth7d", get label() { return t("github.sort.growth7d"); }, words: ["growth7d", "growth", "7d"] },
  { id: "growth30d", get label() { return t("github.sort.growth30d"); }, words: ["growth30d", "30d"] },
];

/** Every first word the grammar knows (Tab completion reads this). */
export const SUBCOMMANDS = [
  { value: "stats", get hint() { return t("github.sub.stats"); } },
  { value: "repos", get hint() { return t("github.sub.repos"); } },
  { value: "repo", get hint() { return t("github.sub.repo"); } },
  { value: "followers", get hint() { return t("github.sub.followers"); } },
  { value: "stars", get hint() { return t("github.sub.stars"); } },
  { value: "trending", get hint() { return t("github.sub.trending"); } },
  { value: "refresh", get hint() { return t("github.sub.refresh"); } },
  { value: "export", get hint() { return t("github.sub.export"); } },
  { value: "connect", get hint() { return t("github.sub.connect"); } },
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
      if (fmt !== "json" && fmt !== "csv") return { kind: "invalid", message: t("github.export_syntax") };
      const includePrivate = words.slice(2).some((w) => /^(private|privat|--private)$/i.test(w));
      return { kind: "export", format: fmt, includePrivate };
    }
    default:
      return { kind: "invalid", message: t("github.unknown", { word: head }) };
  }
}

/** The command row label. */
export function commandLabel(cmd: GithubCmd): string {
  switch (cmd.kind) {
    case "view":
      return t(`github.label.${cmd.view}` as MessageKey);
    case "repo":
      return `GitHub — ${cmd.name}`;
    case "refresh":
      return t("github.label.refresh");
    case "export":
      return t(cmd.includePrivate ? "github.label.export_private" : "github.label.export", { format: cmd.format.toUpperCase() });
    case "connect":
      return t("github.label.connect");
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
  if (ms == null) return t("github.ago.never");
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return t("github.ago.now");
  const m = Math.round(s / 60);
  if (m < 60) return t("github.ago.min", { n: m });
  const h = Math.round(m / 60);
  if (h < 48) return t("github.ago.h", { n: h });
  return t("github.ago.d", { n: Math.round(h / 24) });
}

/** `in 42 min`, `in 3h`; `now` once it's due. */
export function formatIn(ms: number, now: number): string {
  const m = Math.round((ms - now) / 60_000);
  if (m <= 0) return t("github.in.now");
  if (m < 60) return t("github.in.min", { n: m });
  return t("github.in.h", { n: Math.round(m / 60) });
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
  if (e.includes("github.not_connected")) return t("github.err.not_connected");
  if (e.includes("github.unauthorized")) return t("github.err.unauthorized");
  if (e.includes("github.forbidden")) return t("github.err.forbidden");
  if (e.includes("github.rate_limited")) return t("github.err.rate_limited");
  if (e.includes("github.network")) return t("github.err.network");
  if (e.includes("github.not_found")) return t("github.err.not_found");
  if (e.includes("github.bad_data")) return t("github.err.bad_data");
  if (e.includes("github.no_client_id"))
    return t("github.err.no_client_id");
  if (e.includes("github.device_expired")) return t("github.err.device_expired");
  if (e.includes("github.device_denied")) return t("github.err.device_denied");
  if (e.includes("github.keychain")) return t("github.err.keychain");
  if (e.includes("github.busy")) return t("github.err.busy");
  return e.length > 160 ? `${e.slice(0, 160)}…` : e;
}

/** The one-line freshness / problem banner over the cached data. */
export function statusLine(s: GithubStatus, now: number): { tone: "ok" | "warn" | "error"; text: string } {
  const age = formatAgo(s.last_ok_ms, now);
  if (s.rate_limited_until)
    return { tone: "warn", text: t("github.status.rate", { time: formatClockTime(s.rate_limited_until) }) };
  const cached = s.last_ok_ms != null;
  if (s.last_error?.includes("github.network") && cached)
    return { tone: "warn", text: t("github.status.offline", { age }) };
  const err = errorText(s.last_error);
  if (err) return { tone: "error", text: cached ? t("github.status.error_cached", { err, age }) : err };
  if (s.refreshing) return { tone: "ok", text: t("github.status.refreshing") };
  return { tone: "ok", text: t("github.status.updated", { age }) };
}

export const SOURCE_LABEL: Record<string, MessageKey> = {
  oauth: "github.source.oauth",
  pat: "github.source.pat",
  gh: "github.source.gh",
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

/** Label for repositories GitHub reports no language for. */
export const NO_LANGUAGE = "No language";

/**
 * Language mix of the repositories, by repository count or by stars, as
 * 0..100 shares (largest first, ties by name). Feeds the donut. Archived and
 * forked repositories count too — they are part of the account.
 */
export function languageShares(
  rows: ReadonlyArray<{ language: string | null; stars: number }>,
  by: "repos" | "stars",
): { name: string; value: number; pct: number }[] {
  const acc = new Map<string, number>();
  for (const r of rows) {
    const k = r.language ?? NO_LANGUAGE;
    acc.set(k, (acc.get(k) ?? 0) + (by === "repos" ? 1 : r.stars));
  }
  const total = [...acc.values()].reduce((a, b) => a + b, 0);
  if (total <= 0) return [];
  return [...acc.entries()]
    .filter(([, v]) => v > 0)
    .map(([name, value]) => ({ name, value, pct: (value / total) * 100 }))
    .sort((a, b) => b.value - a.value || a.name.localeCompare(b.name));
}

/**
 * Last activity per month: how many repositories saw their LAST push in each
 * of the `months` calendar months up to `now` (oldest first, local time).
 * Repositories without a push date or older than the window are not counted.
 * Needs no history — it comes from the current repository list.
 */
export function activityByMonth(
  rows: ReadonlyArray<{ pushed_at_ms: number | null; updated_at_ms: number | null }>,
  now: number,
  months = 12,
): { start: number; label: string; count: number }[] {
  const end = new Date(now);
  const buckets: { start: number; label: string; count: number }[] = [];
  for (let i = months - 1; i >= 0; i--) {
    const d = new Date(end.getFullYear(), end.getMonth() - i, 1);
    buckets.push({
      start: d.getTime(),
      label: d.toLocaleDateString("en-US", { month: "short" }),
      count: 0,
    });
  }
  for (const r of rows) {
    const t = r.pushed_at_ms ?? r.updated_at_ms;
    if (t == null || t > now || t < buckets[0].start) continue;
    let idx = buckets.length - 1;
    while (idx > 0 && t < buckets[idx].start) idx--;
    buckets[idx].count++;
  }
  return buckets;
}

/** Top `n` repositories by any numeric metric, relative to the biggest. */
export function metricBars<K extends "stars" | "forks" | "open_issues">(
  rows: ReadonlyArray<{ name: string } & Record<K, number>>,
  key: K,
  n = 8,
): { name: string; value: number; frac: number }[] {
  const top = [...rows]
    .filter((r) => r[key] > 0)
    .sort((a, b) => b[key] - a[key] || a.name.localeCompare(b.name))
    .slice(0, n);
  const max = Math.max(1, ...top.map((r) => r[key]));
  return top.map((r) => ({ name: r.name, value: r[key], frac: r[key] / max }));
}
