import { describe, expect, it } from "vitest";
import {
  commandLabel,
  errorText,
  formatAgo,
  formatDelta,
  formatIn,
  historyCollecting,
  parseGithubArg,
  RANGES,
  SORTS,
  starBars,
  statusLine,
  timeDomain,
  toSeries,
} from "./github";
import type { GithubStatus } from "./ipc";

const MIN = 60_000;
const H = 60 * MIN;

function status(over: Partial<GithubStatus> = {}): GithubStatus {
  return {
    connected: true,
    source: "oauth",
    client_id_configured: true,
    pat_stored: false,
    gh_available: false,
    user_id: 7,
    login: "octo",
    name: null,
    avatar_url: null,
    html_url: null,
    last_ok_ms: 0,
    last_checked_ms: 0,
    last_error: null,
    rate: null,
    scopes: "",
    next_refresh_ms: null,
    rate_limited_until: null,
    refreshing: false,
    device: { state: "idle" },
    config: { auto_refresh: true, interval_h: 1, private_repos: false },
    ...over,
  };
}

describe("parseGithubArg", () => {
  it("defaults to the dashboard", () => {
    expect(parseGithubArg("")).toEqual({ kind: "view", view: "dashboard" });
    expect(parseGithubArg("stats")).toEqual({ kind: "view", view: "dashboard" });
  });

  it("reads repos with an optional sort key", () => {
    expect(parseGithubArg("repos")).toEqual({ kind: "view", view: "repos" });
    expect(parseGithubArg("repos growth7d")).toEqual({ kind: "view", view: "repos", sort: "growth7d" });
    expect(parseGithubArg("repos Forks")).toEqual({ kind: "view", view: "repos", sort: "forks" });
    // An unknown sort word just shows the ranking — never an error.
    expect(parseGithubArg("repos banana")).toEqual({ kind: "view", view: "repos" });
  });

  it("takes the rest as the repository name", () => {
    expect(parseGithubArg("repo inspector-rust")).toEqual({ kind: "repo", name: "inspector-rust" });
    // `repo` alone has nothing to show yet → the ranking to pick from.
    expect(parseGithubArg("repo")).toEqual({ kind: "view", view: "repos" });
  });

  it("export: format and private only on explicit request", () => {
    expect(parseGithubArg("export")).toEqual({ kind: "export", format: "json", includePrivate: false });
    expect(parseGithubArg("export csv")).toEqual({ kind: "export", format: "csv", includePrivate: false });
    expect(parseGithubArg("export csv private")).toEqual({ kind: "export", format: "csv", includePrivate: true });
    expect(parseGithubArg("export xml").kind).toBe("invalid");
    // The word must stand alone — `privates` is not consent.
    expect(parseGithubArg("export json privates")).toEqual({ kind: "export", format: "json", includePrivate: false });
  });

  it("knows the remaining views and actions", () => {
    expect(parseGithubArg("followers")).toEqual({ kind: "view", view: "followers" });
    expect(parseGithubArg("stars")).toEqual({ kind: "view", view: "stars" });
    expect(parseGithubArg("trending")).toEqual({ kind: "view", view: "trending", sort: "growth7d" });
    expect(parseGithubArg("refresh")).toEqual({ kind: "refresh" });
    expect(parseGithubArg("connect")).toEqual({ kind: "connect" });
    expect(parseGithubArg("nonsense").kind).toBe("invalid");
  });

  it("labels every command", () => {
    expect(commandLabel(parseGithubArg(""))).toContain("statistics");
    expect(commandLabel(parseGithubArg("export csv private"))).toBe("GitHub — export CSV (incl. private)");
    expect(commandLabel(parseGithubArg("repo x"))).toBe("GitHub — x");
  });
});

describe("deltas", () => {
  it("shows a missing history as a dash, never as zero", () => {
    expect(formatDelta(null)).toBe("—");
    expect(formatDelta(0)).toBe("±0");
    expect(formatDelta(12)).toBe("+12");
    expect(formatDelta(-3)).toBe("−3");
    expect(formatDelta(1200)).toBe("+1,200");
  });

  it("knows when the history is still collecting", () => {
    expect(historyCollecting({ d24h: null, d7d: null, d30d: null, d90d: null })).toBe(true);
    expect(historyCollecting({ d24h: 0, d7d: null, d30d: null, d90d: null })).toBe(false);
  });
});

describe("time", () => {
  it("formats ages and countdowns", () => {
    expect(formatAgo(null, 0)).toBe("never");
    expect(formatAgo(10 * H - 30_000, 10 * H)).toBe("just now");
    expect(formatAgo(10 * H - 42 * MIN, 10 * H)).toBe("42 min ago");
    expect(formatAgo(10 * H - 4 * H, 10 * H)).toBe("4h ago");
    expect(formatAgo(0, 72 * H)).toBe("3d ago");
    expect(formatIn(10 * MIN, 0)).toBe("in 10 min");
    expect(formatIn(3 * H, 0)).toBe("in 3h");
    expect(formatIn(0, 5)).toBe("now");
  });
});

describe("statusLine", () => {
  it("offline keeps the cache visible and says how old it is", () => {
    const s = status({ last_ok_ms: 0, last_error: "github.network" });
    expect(statusLine(s, 4 * H)).toEqual({
      tone: "warn",
      text: "Offline · Showing cached GitHub data · Last updated 4h ago",
    });
  });

  it("a rate limit names the time of the next possible refresh", () => {
    const reset = new Date(2026, 9, 11, 14, 5).getTime();
    expect(statusLine(status({ rate_limited_until: reset }), reset - H).text).toBe(
      "Rate limit reached · Next refresh available: 14:05",
    );
  });

  it("a revoked token is an error, still over the cached data", () => {
    const line = statusLine(status({ last_error: "github.unauthorized", last_ok_ms: 0 }), 2 * H);
    expect(line.tone).toBe("error");
    expect(line.text).toContain("rejected");
    expect(line.text).toContain("2h ago");
  });

  it("all good: when it was updated", () => {
    expect(statusLine(status({ last_ok_ms: 0 }), 5 * MIN)).toEqual({ tone: "ok", text: "Last updated 5 min ago" });
  });
});

describe("errorText", () => {
  it("maps every sentinel and never echoes long raw text", () => {
    for (const k of ["not_connected", "unauthorized", "forbidden", "rate_limited", "network", "not_found", "bad_data", "no_client_id", "device_expired", "device_denied", "keychain", "busy"]) {
      expect(errorText(`github.${k}`)).not.toContain("github.");
    }
    expect(errorText(null)).toBeNull();
    expect(errorText("x".repeat(400))!.length).toBeLessThan(170);
  });
});

describe("charts", () => {
  it("converts snapshot points to seconds and finds the common domain", () => {
    const a = toSeries([[1000, 1], [3000, 2]]);
    expect(a).toEqual([{ t: 1, v: 1 }, { t: 3, v: 2 }]);
    expect(timeDomain([a, toSeries([[5000, 9]])])).toEqual([1, 5]);
    expect(timeDomain([[]])).toBeNull();
  });

  it("bars: the top repositories, relative to the biggest", () => {
    const bars = starBars([
      { name: "b", stars: 10 },
      { name: "a", stars: 40 },
      { name: "c", stars: 0 },
    ], 2);
    expect(bars.map((b) => b.name)).toEqual(["a", "b"]);
    expect(bars[1].frac).toBeCloseTo(0.25);
    // No stars anywhere: no division by zero.
    expect(starBars([{ name: "x", stars: 0 }])[0].frac).toBe(0);
  });

  it("offers the documented ranges and sorts", () => {
    expect(RANGES.map((r) => r.label)).toEqual(["7D", "30D", "90D", "1Y", "ALL"]);
    expect(SORTS.map((s) => s.id)).toEqual(["stars", "forks", "updated", "name", "growth7d", "growth30d"]);
  });
});

describe("contract with the Rust side", () => {
  async function rust(file: string): Promise<string> {
    const { readFileSync } = (await import("node:" + "fs")) as unknown as {
      readFileSync(path: string, encoding: "utf8"): string;
    };
    const cwd = (globalThis as unknown as { process: { cwd(): string } }).process.cwd();
    return readFileSync(`${cwd}/../rust-lib/src/github/${file}`, "utf8");
  }

  it("every sort key is a Rust SortKey variant (serde snake_case)", async () => {
    const rs = await rust("mod.rs");
    const body = rs.slice(rs.indexOf("pub enum SortKey"), rs.indexOf("}", rs.indexOf("pub enum SortKey")));
    const variants = [...body.matchAll(/^\s+(\w+),/gm)].map((m) => m[1].toLowerCase());
    expect(SORTS.map((s) => s.id).sort()).toEqual(variants.sort());
  });

  it("every range the chips offer is one the backend understands", async () => {
    const rs = await rust("service.rs");
    for (const r of RANGES.filter((x) => x.id !== "all")) expect(rs).toContain(`"${r.id}" =>`);
  });

  it("every error sentinel has a text", async () => {
    const all = (await rust("mod.rs")) + (await rust("service.rs"));
    // Constants named ERR_*, plus the messages the device flow sets inline.
    const sentinels = [
      ...[...all.matchAll(/ERR_[A-Z_]+: &str = "(github\.[a-z_]+)"/g)].map((m) => m[1]),
      ...[...all.matchAll(/"(github\.(?:device|keychain)[a-z_]*)"/g)].map((m) => m[1]),
    ];
    expect(sentinels.length).toBeGreaterThan(8);
    for (const s of new Set(sentinels)) expect(errorText(s), s).not.toBe(s);
  });
});
