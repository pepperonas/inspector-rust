import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ClaudeLimit, ClaudeLimitsStatus, LimitForecast } from "../lib/ipc";

const ipc = vi.hoisted(() => ({
  status: null as unknown as ClaudeLimitsStatus,
  open: [] as string[],
  saved: [] as string[][],
}));

vi.mock("../lib/ipc", () => ({
  claudeLimitsStatus: vi.fn(async () => ipc.status),
  setClaudeLimitsPoll: vi.fn(async (m: number) => m),
  getLimitsForecastOpen: vi.fn(async () => ipc.open),
  setLimitsForecastOpen: vi.fn(async (ids: string[]) => {
    ipc.saved.push(ids);
  }),
}));

import { ClaudeLimitsPanel } from "./ClaudeLimitsPanel";

const WINDOW = { start: "2026-10-03T23:00:00Z", end: "2026-10-10T23:00:00Z" };

function forecast(over: Partial<LimitForecast> = {}): LimitForecast {
  return {
    version: 1,
    basis: "linear",
    confidence: "rough",
    status: "reserve",
    window: WINDOW,
    now: "2026-10-07T11:00:00Z",
    pace: { planPercent: 50, deltaPoints: -20 },
    atReset: { median: 60, low: 48, high: 72 },
    exhaustsAt: null,
    k: null,
    notes: [],
    series: null,
    source: "local",
    ...over,
  };
}

function limit(id: string, f: LimitForecast | null): ClaudeLimit {
  return {
    id,
    name: "Woche · alle Modelle",
    kind: "weekly_all",
    group: "weekly",
    percent: 30,
    resets_at: WINDOW.end,
    severity: null,
    active: true,
    known: true,
    money: null,
    window_minutes: 10080,
    forecast: f,
  };
}

function status(limits: ClaudeLimit[]): ClaudeLimitsStatus {
  return {
    report: { limits, extra: null, breakdown: [], legacy: false },
    fetched_at_ms: Date.now(),
    error: null,
    error_detail: null,
    retry_at_ms: null,
    poll_minutes: 5,
    codex: null,
    antigravity: null,
  };
}

const SERIES = {
  actual: [
    ["2026-10-03T23:00:00Z", 0],
    ["2026-10-07T11:00:00Z", 30],
  ] as [string, number][],
  measured: [["2026-10-05T23:00:00Z", 15]] as [string, number][],
  forecast: [
    ["2026-10-07T11:00:00Z", 30, 30, 30],
    ["2026-10-10T23:00:00Z", 60, 48, 72],
  ] as [string, number, number, number][],
  ghosts: [],
};

async function mount() {
  await act(async () => {
    render(<ClaudeLimitsPanel focused={false} onExit={() => undefined} />);
  });
}

afterEach(() => {
  cleanup();
  ipc.open = [];
  ipc.saved = [];
});

describe("ClaudeLimitsPanel forecast (v0.196.0)", () => {
  it("shows the plan tick, the projection and the status line", async () => {
    ipc.status = status([limit("weekly_all-1", forecast())]);
    await mount();
    expect(screen.getByTestId("forecast-line").textContent).toContain("20 Punkte Reserve · voraussichtlich 60 %");
    expect(screen.getByTestId("bar-plan").style.left).toBe("50%");
    expect(screen.getByTestId("bar-projection").style.width).toBe("30%");
    expect(screen.queryByTestId("bar-over")).toBeNull();
  });

  it("local estimates carry the 'start Token Tracker' hint, tracker ones don't", async () => {
    ipc.status = status([limit("a", forecast())]);
    await mount();
    expect(screen.getByTestId("tracker-hint")).toBeTruthy();
    cleanup();
    ipc.status = status([limit("a", forecast({ source: "tracker", basis: "calibrated", confidence: "good" }))]);
    await mount();
    expect(screen.queryByTestId("tracker-hint")).toBeNull();
  });

  it("a linear forecast has no chart toggle; a tracker series does, and remembers it", async () => {
    ipc.status = status([limit("a", forecast())]);
    await mount();
    expect(screen.queryByLabelText("Grafik aufklappen")).toBeNull();
    cleanup();
    ipc.status = status([limit("a", forecast({ source: "tracker", series: SERIES }))]);
    await mount();
    expect(screen.queryByTestId("limit-chart")).toBeNull();
    await act(async () => {
      fireEvent.click(screen.getByLabelText("Grafik aufklappen"));
    });
    expect(screen.getByTestId("limit-chart")).toBeTruthy();
    expect(ipc.saved[ipc.saved.length - 1]).toEqual(["a"]);
  });

  it("a chart saved as open is open on the next mount", async () => {
    ipc.open = ["a"];
    ipc.status = status([limit("a", forecast({ source: "tracker", series: SERIES }))]);
    await mount();
    expect(screen.getByTestId("limit-chart")).toBeTruthy();
  });

  it("an overrun gets the red cap", async () => {
    ipc.status = status([limit("a", forecast({ status: "exhausts", atReset: { median: 130, low: 100, high: 160 } }))]);
    await mount();
    expect(screen.getByTestId("bar-over")).toBeTruthy();
  });
});
