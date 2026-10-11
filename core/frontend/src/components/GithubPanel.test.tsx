import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { GithubDashboard, GithubStatus } from "../lib/ipc";
import { parseGithubArg } from "../lib/github";

const ipc = vi.hoisted(() => ({
  status: null as unknown as GithubStatus,
  dash: null as GithubDashboard | null,
  refreshes: [] as boolean[],
  exports: [] as Array<[string, boolean]>,
  deviceStarts: 0,
  dashCalls: [] as Array<[string, string]>,
}));

vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(async () => {}) }));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn(async () => {}) }));
vi.mock("../lib/ipc", () => ({
  githubStatus: vi.fn(async () => ipc.status),
  githubDashboard: vi.fn(async (range: string, sort: string) => {
    ipc.dashCalls.push([range, sort]);
    return ipc.dash;
  }),
  githubRefresh: vi.fn(async (force: boolean) => {
    ipc.refreshes.push(force);
    return { gate: { kind: "go" }, fetched: true, account_snapshot: false, repo_snapshots: 0 };
  }),
  githubRepo: vi.fn(async () => {
    throw "github.not_found";
  }),
  githubExport: vi.fn(async (f: string, p: boolean) => {
    ipc.exports.push([f, p]);
    return `/Users/x/Downloads/github-octo.${f}`;
  }),
  githubDeviceStart: vi.fn(async () => {
    ipc.deviceStarts += 1;
    return { state: "idle" };
  }),
  githubDeviceCancel: vi.fn(async () => {}),
}));

import { GithubPanel } from "./GithubPanel";

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
    last_ok_ms: Date.now() - 4 * 3_600_000,
    last_checked_ms: null,
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

const none = { d24h: null, d7d: null, d30d: null, d90d: null };

function dash(): GithubDashboard {
  return {
    user_id: 7,
    kpis: [
      { key: "followers", value: 142, deltas: { d24h: 1, d7d: 4, d30d: 12, d90d: null } },
      { key: "stars", value: 384, deltas: none },
      { key: "repos", value: 2, deltas: none },
      { key: "forks", value: 9, deltas: none },
    ],
    followers: [[Date.now(), 142]],
    stars: [[Date.now(), 384]],
    rows: [
      {
        id: 1, name: "inspector-rust", full_name: "octo/inspector-rust", private: false, archived: false, fork: false,
        language: "Rust", html_url: "", description: null, license: null, stars: 380, forks: 9, watchers: null,
        open_issues: 3, updated_at_ms: null, pushed_at_ms: null, growth_7d: 5, growth_30d: null,
      },
      {
        id: 2, name: "secret", full_name: "octo/secret", private: true, archived: true, fork: false,
        language: null, html_url: "", description: null, license: null, stars: 4, forks: 0, watchers: null,
        open_issues: 0, updated_at_ms: null, pushed_at_ms: null, growth_7d: null, growth_30d: null,
      },
    ],
    growth: [],
    history_since_ms: Date.now(),
    snapshot_count: 1,
    private_count: 1,
  };
}

const noop = () => {};

afterEach(() => {
  cleanup();
  ipc.refreshes = [];
  ipc.exports = [];
  ipc.deviceStarts = 0;
  ipc.dashCalls = [];
});

describe("GithubPanel", () => {
  it("draws charts that need no history: languages, last-push activity, stars/forks by repository", async () => {
    ipc.status = status();
    const d = dash();
    d.rows[0].pushed_at_ms = Date.now();
    ipc.dash = d;
    render(<GithubPanel cmd={parseGithubArg("")} focused={false} runSignal={0} onExit={noop} onQuery={noop} />);
    const donut = await screen.findByTestId("gh-languages");
    // By repos: one Rust, one without language — both are named, nothing is invented.
    expect(donut.textContent).toContain("Rust");
    expect(donut.textContent).toContain("No language");
    expect(donut.querySelectorAll("path").length).toBe(2);
    // Switching to stars weighs by stars (380 vs 4) — the share changes.
    fireEvent.click(screen.getByRole("button", { name: "Stars" }));
    expect(donut.textContent).toContain("99%");
    const activity = screen.getByTestId("gh-activity");
    expect(activity.textContent).toContain("1 active");
    // Bars: a zero-fork repo is left out, not drawn as an empty bar.
    const bars = screen.getAllByTestId("gh-bars");
    expect(bars).toHaveLength(2);
    expect(bars[1].textContent).toContain("inspector-rust");
    expect(bars[1].textContent).not.toContain("secret");
  });

  it("shows the KPI cards from the cache; a short history says so instead of 0", async () => {
    ipc.status = status();
    ipc.dash = dash();
    render(<GithubPanel cmd={parseGithubArg("")} focused={false} runSignal={0} onExit={noop} onQuery={noop} />);
    await waitFor(() => expect(screen.getAllByTestId("gh-kpi")).toHaveLength(4));
    expect(screen.getAllByText("142").length).toBeGreaterThan(0);
    expect(screen.getByText(/Connected as @octo/)).toBeTruthy();
    // Stars have no history yet: the card says it is collecting.
    expect(screen.getAllByText("History collecting…").length).toBeGreaterThan(0);
    // Followers: 30-day change shown, 90 days unknown → a dash.
    expect(screen.getAllByText("+12").length).toBeGreaterThan(0);
    expect(screen.getAllByText("—").length).toBeGreaterThan(0);
    // One snapshot is not a line.
    expect(screen.getAllByText(/History collecting… \(1 snapshot\)/).length).toBe(2);
  });

  it("opens with a gated refresh — never a forced one", async () => {
    ipc.status = status();
    ipc.dash = dash();
    render(<GithubPanel cmd={parseGithubArg("")} focused={false} runSignal={0} onExit={noop} onQuery={noop} />);
    await waitFor(() => expect(ipc.refreshes).toEqual([false]));
  });

  it("offline: the cached data stays, with its age", async () => {
    ipc.status = status({ last_error: "github.network" });
    ipc.dash = dash();
    render(<GithubPanel cmd={parseGithubArg("")} focused={false} runSignal={0} onExit={noop} onQuery={noop} />);
    await waitFor(() =>
      expect(screen.getByTestId("gh-status-line").textContent).toBe(
        "Offline · Showing cached GitHub data · Last updated 4h ago",
      ),
    );
    expect(screen.getAllByTestId("gh-kpi")).toHaveLength(4);
  });

  it("marks private and archived repositories in the ranking", async () => {
    ipc.status = status();
    ipc.dash = dash();
    render(<GithubPanel cmd={parseGithubArg("repos")} focused={false} runSignal={0} onExit={noop} onQuery={noop} />);
    await waitFor(() => expect(screen.getAllByTestId("gh-repo-row")).toHaveLength(2));
    expect(screen.getByLabelText("private")).toBeTruthy();
    expect(screen.getByText("archived")).toBeTruthy();
  });

  it("a sort chip re-reads the dashboard in that order", async () => {
    ipc.status = status();
    ipc.dash = dash();
    render(<GithubPanel cmd={parseGithubArg("repos")} focused={false} runSignal={0} onExit={noop} onQuery={noop} />);
    await waitFor(() => screen.getByText("Growth 30d"));
    fireEvent.click(screen.getByText("Growth 30d"));
    await waitFor(() => expect(ipc.dashCalls.some(([, s]) => s === "growth30d")).toBe(true));
  });

  it("export runs on Enter only, without private repositories unless asked", async () => {
    ipc.status = status();
    ipc.dash = dash();
    const { rerender } = render(
      <GithubPanel cmd={parseGithubArg("export csv")} focused={false} runSignal={0} onExit={noop} onQuery={noop} />,
    );
    await waitFor(() => screen.getByText(/private repositories left out/));
    expect(ipc.exports).toEqual([]);
    rerender(<GithubPanel cmd={parseGithubArg("export csv")} focused runSignal={1001} onExit={noop} onQuery={noop} />);
    await waitFor(() => expect(ipc.exports).toEqual([["csv", false]]));
    await waitFor(() => screen.getByText("Exported: github-octo.csv"));
  });

  it("not connected: the connect card, and Connect starts the device flow", async () => {
    ipc.status = status({ connected: false, source: null, last_ok_ms: null });
    ipc.dash = null;
    render(<GithubPanel cmd={parseGithubArg("")} focused={false} runSignal={0} onExit={noop} onQuery={noop} />);
    await waitFor(() => screen.getByTestId("gh-connect"));
    fireEvent.click(screen.getByText("Connect GitHub"));
    await waitFor(() => expect(ipc.deviceStarts).toBe(1));
    // No refresh without a token.
    expect(ipc.refreshes).toEqual([]);
  });

  it("without a configured client id the browser login is off and says why", async () => {
    ipc.status = status({ connected: false, source: null, client_id_configured: false, last_ok_ms: null });
    ipc.dash = null;
    render(<GithubPanel cmd={parseGithubArg("")} focused={false} runSignal={0} onExit={noop} onQuery={noop} />);
    await waitFor(() => screen.getByTestId("gh-connect"));
    expect((screen.getByText("Connect GitHub").closest("button") as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/personal access token/)).toBeTruthy();
  });

  it("the device code is shown with a way to open GitHub", async () => {
    ipc.status = status({
      connected: false,
      source: null,
      last_ok_ms: null,
      device: { state: "waiting", user_code: "WDJB-MJHT", verification_uri: "https://github.com/login/device", expires_at_ms: Date.now() + 900_000 },
    });
    ipc.dash = null;
    render(<GithubPanel cmd={parseGithubArg("")} focused={false} runSignal={0} onExit={noop} onQuery={noop} />);
    await waitFor(() => screen.getByText("WDJB-MJHT"));
    expect(screen.getByText(/github\.com\/login\/device/)).toBeTruthy();
  });
});
