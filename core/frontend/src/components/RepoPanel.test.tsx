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
});
