import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup, fireEvent, waitFor } from "@testing-library/react";
import type { RepoAnalysis, RepoStats, RangeKey, GithubActivity } from "../lib/ipc";

const repoAnalyze = vi.fn<(t: string | null) => Promise<RepoAnalysis>>();
const repoExport = vi.fn<(s: RepoStats, r: RangeKey, f: "html" | "pdf") => Promise<string>>();
const repoClone = vi.fn<(u: string) => Promise<string>>();
const repoGithubActivity = vi.fn<(o: string, r: string) => Promise<GithubActivity>>();
vi.mock("../lib/ipc", () => ({
  repoAnalyze: (t: string | null) => repoAnalyze(t),
  repoExport: (s: RepoStats, r: RangeKey, f: "html" | "pdf") => repoExport(s, r, f),
  repoClone: (u: string) => repoClone(u),
  repoGithubActivity: (o: string, r: string) => repoGithubActivity(o, r),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => undefined }));

import { RepoPanel } from "./RepoPanel";

function stats(commits: number): RepoStats {
  return {
    name: "r", source: "https://github.com/o/r", commits, contributors: commits ? 1 : 0,
    first_commit: "2026-08-01T00:00:00+00:00", last_commit: "2026-08-24T00:00:00+00:00",
    active_days: commits, insertions: 30, deletions: 4,
    by_weekday: [1, 0, 0, 0, 0, 0, 0], by_hour: Array(24).fill(0), by_month: [],
    top_files: [], top_exts: [], top_authors: [], categories: [], longest_streak: 1, avg_msg_len: 3,
    heatmap: Array.from({ length: 7 }, () => Array(24).fill(0)), calendar: [],
    hotspots: [{ path: "src/a.rs", changes: 4, authors: 1 }], bus_factor: 1,
    dir_bus_factor: [], co_change: [],
    granularity: "day",
    timeline: [
      { start: "2026-08-24", commits: commits, insertions: 30, deletions: 4 },
      { start: "2026-08-25", commits: 0, insertions: 0, deletions: 0 },
    ],
  };
}
const analysis: RepoAnalysis = {
  name: "r", source: "https://github.com/o/r",
  github: { owner: "o", repo: "r", web_url: "https://github.com/o/r", clone_url: "https://github.com/o/r.git" },
  recent: {
    now: 0,
    day: { commits: 3, insertions: 10, deletions: 1, files: 4, authors: 1, tags: 0 },
    day_prev: { commits: 1, insertions: 0, deletions: 0, files: 0, authors: 0, tags: 0 },
    week: { commits: 9, insertions: 50, deletions: 5, files: 7, authors: 3, tags: 1 },
    week_prev: { commits: 12, insertions: 0, deletions: 0, files: 0, authors: 0, tags: 0 },
  },
  ranges: [
    { range: "d30", stats: stats(0) },
    { range: "d90", stats: stats(2) },
    { range: "d180", stats: stats(2) },
    { range: "y1", stats: stats(3) },
    { range: "all", stats: stats(7) },
  ],
};

const zero = { pushes: 0, prs_opened: 0, prs_merged: 0, issues_opened: 0, issues_closed: 0 };
const gh: GithubActivity = { day: { ...zero, pushes: 4 }, day_prev: zero, week: { ...zero, pushes: 20 }, week_prev: zero, pushes_capped: true };

beforeEach(() => {
  repoGithubActivity.mockResolvedValue(gh);
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
  it("the error card really retries (the hint promises it)", async () => {
    repoAnalyze.mockRejectedValueOnce("repo.network: Could not resolve host");
    const v = render(<RepoPanel arg="https://github.com/o/r" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => v.getByText(/Keine Verbindung/));
    fireEvent.click(v.getByRole("button", { name: /Erneut versuchen/ }));
    await waitFor(() => expect(repoAnalyze).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(v.getAllByText("7").length).toBeGreaterThan(0));
  });
  it("editing the argument re-analyses once, after typing settles", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const v = render(<RepoPanel arg="https://github.com/o/r" autoExport={false} focused onExit={() => {}} />);
      await waitFor(() => expect(repoAnalyze).toHaveBeenCalledTimes(1));
      v.rerender(<RepoPanel arg="https://github.com/o/re" autoExport={false} focused onExit={() => {}} />);
      v.rerender(<RepoPanel arg="https://github.com/o/rep" autoExport={false} focused onExit={() => {}} />);
      v.rerender(<RepoPanel arg="https://github.com/o/repo" autoExport={false} focused onExit={() => {}} />);
      await vi.advanceTimersByTimeAsync(1000);
      expect(repoAnalyze).toHaveBeenCalledTimes(2);
      expect(repoAnalyze).toHaveBeenLastCalledWith("https://github.com/o/repo");
    } finally {
      vi.useRealTimers();
    }
  });
  it("shows code changes near the top, before the charts", async () => {
    const v = render(<RepoPanel arg="https://github.com/o/r" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => v.getByText("Code-Änderungen"));
    const html = v.container.innerHTML;
    expect(html.indexOf("Code-Änderungen")).toBeLessThan(html.indexOf("Wochentag"));
    // Also in the KPI tiles — the card must show them too.
    expect(v.getAllByText("+30").length).toBeGreaterThanOrEqual(2);
    expect(v.getAllByText("−4").length).toBeGreaterThanOrEqual(2);
    expect(v.getByText(/\+26 netto/)).toBeTruthy();
  });
  it("titles activity by the chosen range, never by a month count", async () => {
    const v = render(<RepoPanel arg="https://github.com/o/r" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => v.getByRole("button", { name: "90 T" }));
    fireEvent.click(v.getByRole("button", { name: "90 T" }));
    expect(v.getByText("Aktivität · letzte 90 Tage")).toBeTruthy();
    expect(v.queryByText(/Monate/)).toBeNull();
  });
  it("shows the 24 h / 7 day activity above the range chips, with deltas", async () => {
    const v = render(<RepoPanel arg="https://github.com/o/r" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => v.getByText("Aktivität 24 h / 7 Tage"));
    const html = v.container.innerHTML;
    expect(html.indexOf("Aktivität 24 h / 7 Tage")).toBeLessThan(html.indexOf('aria-label="Zeitraum"'));
    expect(v.getByText("↑ 2")).toBeTruthy(); // commits 24 h: 3 vs 1
    expect(v.getByText("↓ 3")).toBeTruthy(); // commits 7 d: 9 vs 12
  });
  it("loads GitHub numbers for a GitHub repo and marks a capped push count", async () => {
    const v = render(<RepoPanel arg="https://github.com/o/r" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => expect(repoGithubActivity).toHaveBeenCalledWith("o", "r"));
    await waitFor(() => v.getByText("Pushes"));
    // "≥ ", "20" and the delta are separate nodes inside one cell.
    expect(v.getByText(/^≥ 20\b/)).toBeTruthy();
    // GitHub delivers events late (30 s – hours) — the card has to say so.
    expect(v.getByText(/Verzögerung/)).toBeTruthy();
  });
  it("no GitHub block for a local repo", async () => {
    repoAnalyze.mockResolvedValue({ ...analysis, github: null });
    const v = render(<RepoPanel arg="~/x" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => v.getByText("Aktivität 24 h / 7 Tage"));
    expect(repoGithubActivity).not.toHaveBeenCalled();
    expect(v.queryByText("Pushes")).toBeNull();
  });
  it("a GitHub error shows a hint and keeps the git numbers", async () => {
    repoGithubActivity.mockRejectedValue("github.rate_limit: HTTP 403");
    const v = render(<RepoPanel arg="https://github.com/o/r" autoExport={false} focused onExit={() => {}} />);
    await waitFor(() => v.getByText(/Abfragelimit/));
    expect(v.getByText("↑ 2")).toBeTruthy();
  });
});
