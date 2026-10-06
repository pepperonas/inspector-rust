import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";

const { speedtestRun, speedtestRunning, speedtestHistory, speedtestClearHistory, listen, live } = vi.hoisted(() => {
  const live = new Set<{ event: string; handler: (e: { payload: unknown }) => void }>();
  return {
    speedtestRun: vi.fn(),
    speedtestRunning: vi.fn(async () => false),
    speedtestHistory: vi.fn(async () => [] as unknown[]),
    speedtestClearHistory: vi.fn(async () => undefined),
    listen: vi.fn(async (event: string, handler: (e: { payload: unknown }) => void) => {
      const sub = { event, handler };
      live.add(sub);
      return () => void live.delete(sub);
    }),
    live,
  };
});
vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("../lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/ipc")>()),
  speedtestRun,
  speedtestRunning,
  speedtestHistory,
  speedtestClearHistory,
}));

import { SpeedtestPanel } from "./SpeedtestPanel";

const result = {
  at: 1_791_315_838_622,
  download_bps: 452_304_161,
  upload_bps: 44_196_779,
  latency_ms: 38.87,
  jitter_ms: 6.49,
  loaded_down_ms: 31.4,
  loaded_up_ms: 11.2,
  colo: "TXL",
  country: "DE",
  ip: "203.0.113.9",
};

async function emit(event: string, payload: unknown) {
  await act(async () => {
    for (const s of [...live]) if (s.event === event) s.handler({ payload });
    await Promise.resolve();
  });
}

beforeEach(() => {
  speedtestRun.mockReset();
  speedtestRunning.mockReset();
  speedtestRunning.mockResolvedValue(false);
  speedtestHistory.mockReset();
  speedtestHistory.mockResolvedValue([]);
  live.clear();
});
afterEach(cleanup);

describe("SpeedtestPanel", () => {
  it("starts a run on mount and shows the result", async () => {
    speedtestRun.mockResolvedValue(result);
    render(<SpeedtestPanel focused onExit={() => {}} />);
    await act(async () => {});
    await act(async () => {});
    expect(speedtestRun).toHaveBeenCalledTimes(1);
    expect(screen.getByText("452")).toBeTruthy();
    expect(screen.getByText("44,2")).toBeTruthy();
    expect(screen.getByText("39 ms")).toBeTruthy();
    expect(screen.getByText(/Verbindung sehr gut/)).toBeTruthy();
    expect(screen.getByText(/Cloudflare TXL · DE/)).toBeTruthy();
  });

  it("doesn't start a second run when one is already going, and picks up its end", async () => {
    speedtestRunning.mockResolvedValue(true);
    speedtestHistory.mockResolvedValue([]);
    render(<SpeedtestPanel focused onExit={() => {}} />);
    await act(async () => {});
    expect(speedtestRun).not.toHaveBeenCalled();

    await emit("speedtest-progress", { phase: "download", fraction: 0.5, value: 100_000_000 });
    expect(screen.getByText("Download")).toBeTruthy();
    expect(screen.getByText("100")).toBeTruthy();

    speedtestHistory.mockResolvedValue([{ id: 1, ...result }]);
    await emit("speedtest-done", null);
    await act(async () => {});
    expect(screen.getAllByText("452").length).toBeGreaterThan(0);
  });

  it("shows an error instead of a zero when the run fails", async () => {
    speedtestRun.mockRejectedValue(new Error("download: connection refused"));
    render(<SpeedtestPanel focused onExit={() => {}} />);
    await act(async () => {});
    await act(async () => {});
    expect(screen.getByRole("alert").textContent).toMatch(/connection refused/);
  });

  it("compares with the median of earlier runs", async () => {
    speedtestRun.mockResolvedValue(result);
    speedtestHistory.mockResolvedValue([
      { id: 3, ...result },
      { id: 2, ...result, at: 2, download_bps: 400e6, upload_bps: 40e6 },
      { id: 1, ...result, at: 1, download_bps: 500e6, upload_bps: 50e6 },
    ]);
    render(<SpeedtestPanel focused onExit={() => {}} />);
    await act(async () => {});
    await act(async () => {});
    // median(400, 500) = 450 → +1 %
    expect(screen.getAllByText(/ggü\. früher/).length).toBe(2);
    expect(screen.getByText(/Median der 2 früheren/)).toBeTruthy();
  });

  it("shows loaded latency in the result", async () => {
    speedtestRun.mockResolvedValue(result);
    render(<SpeedtestPanel focused onExit={() => {}} />);
    await act(async () => {});
    await act(async () => {});
    expect(screen.getAllByText("Ping unter Last").length).toBe(2);
    expect(screen.getByText("31 ms")).toBeTruthy();
    expect(screen.getByText("11 ms")).toBeTruthy();
  });

  it("history shows median/min/max, a trend and every value per run", async () => {
    speedtestRun.mockResolvedValue(result);
    speedtestHistory.mockResolvedValue([
      { id: 3, ...result },
      { id: 2, ...result, at: 2, download_bps: 300e6, latency_ms: 20, loaded_down_ms: null },
      { id: 1, ...result, at: 1, download_bps: 500e6, latency_ms: 10 },
    ]);
    render(<SpeedtestPanel focused onExit={() => {}} />);
    await act(async () => {});
    await act(async () => {});
    expect(screen.getByText(/3 Messungen/)).toBeTruthy();
    // ↓ row: median 452, min 300, max 500
    const row = screen.getByText("↓ Mbit/s").closest("tr")!;
    expect(row.textContent).toBe("↓ Mbit/s452300500");
    expect(screen.getByLabelText("Verlauf als Diagramm")).toBeTruthy();
    const runs = screen.getByLabelText("Messungen").querySelectorAll("li");
    expect(runs.length).toBe(3);
    // A missing loaded value shows as a dash, never 0.
    expect(runs[1].textContent).toMatch(/unter Last ↓ — \//);
    expect(runs[0].textContent).toMatch(/TXL DE/);
  });
});
