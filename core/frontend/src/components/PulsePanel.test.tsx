import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";

const { pulseLive, pulseHistory } = vi.hoisted(() => ({
  pulseLive: vi.fn(),
  pulseHistory: vi.fn(),
}));
vi.mock("../lib/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/ipc")>()),
  pulseLive,
  pulseHistory,
}));

import { PulsePanel } from "./PulsePanel";
import type { PulseAgg, PulseLive, PulseSample } from "../lib/ipc";

const sample = (over: Partial<PulseSample> = {}): PulseSample => ({
  ts: 1_791_000_000,
  dur: 5,
  pressure: 4,
  swap_used: 20e9,
  swap_total: 24e9,
  swapout_bps: 3e6,
  swapin_bps: 1e6,
  pageout_bps: 0,
  compress_ps: 1200,
  decompress_ps: 800,
  ssd_write_bps: 12e6,
  ssd_read_bps: 4e6,
  swap_write_bps: 3e6,
  cpu_pct: 40,
  thermal: 0,
  wired: 3e9,
  active: 8e9,
  inactive: 4e9,
  free: 0.2e9,
  compressor: 6e9,
  ...over,
});

const liveData = (over: Partial<PulseLive> = {}): PulseLive => ({
  supported: true,
  enabled: true,
  latest: sample(),
  recent: [sample({ ts: 1 }), sample({ ts: 2, pressure: 1 })],
  groups: [
    { name: "Gradle-Daemon", footprint: 6e9, write_bps: 2e6, procs: 2 },
    { name: "Google Chrome", footprint: 4e9, write_bps: 0, procs: 40 },
  ],
  groups_at: 1_791_000_000,
  unreadable: 190,
  disk_found: true,
  capacity: 500e9,
  overhead: { samples: 10, sample_avg_ms: 0.4, sample_max_ms: 1, scans: 2, scan_avg_ms: 3, scan_max_ms: 4, rows_written: 3, cpu_secs: 0.6, cpu_pct: 0.05, running_secs: 1200 },
  db_bytes: 40_000,
  smart: null,
  smartctl_present: false,
  last_error: null,
  ...over,
});

const agg = (ts: number, over: Partial<PulseAgg> = {}): PulseAgg => ({
  ts,
  secs: 900,
  p_normal: 300,
  p_warn: 0,
  p_crit: 600,
  swap_min: 1,
  swap_avg: 2,
  swap_max: 3,
  swap_total: 4,
  written: 2e9,
  read: 1e9,
  swap_written: 5e8,
  swapped_in: 0,
  compressions: 0,
  cpu_avg: 30,
  causers: [{ name: "Docker", written: 1e8, peak_footprint: 5e9 }],
  ...over,
});

async function mount(focused = true) {
  const onExit = vi.fn();
  await act(async () => {
    render(<PulsePanel focused={focused} onExit={onExit} onOpenSettings={() => {}} />);
  });
  await act(async () => {
    await Promise.resolve();
  });
  return onExit;
}

beforeEach(() => {
  pulseLive.mockResolvedValue(liveData());
  pulseHistory.mockImplementation(async (range: string) => ({
    range,
    bucket_secs: 3600,
    buckets: [agg(0), agg(3600, { p_crit: 0, p_normal: 900 })],
    total: agg(0, { written: 4e9, swap_written: 1e9, p_crit: 600 }),
    days: [agg(86_400 * 5)],
    forecast: { tbw_bytes: 3e14, per_day: 50e9, days: 30, lifetime_written: null, years: 16.4 },
  }));
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("PulsePanel", () => {
  it("shows the pressure level, swap and the top groups live", async () => {
    await mount();
    expect(screen.getByTestId("pulse-level").textContent).toBe("Kritisch");
    expect(screen.getByText("20,0 GB / 24,0 GB")).toBeTruthy();
    const groups = screen.getByTestId("pulse-groups");
    expect(groups.textContent).toContain("Gradle-Daemon");
    expect(groups.textContent).toContain("6,00 GB");
    expect(screen.getByText(/davon ~25 % Swap/)).toBeTruthy();
  });

  it("names the honest limits and the missing smartctl", async () => {
    await mount();
    expect(screen.getByText(/lässt sich keinem Prozess zuordnen/)).toBeTruthy();
    expect(screen.getByText(/190 Prozesse anderer Nutzer/)).toBeTruthy();
    expect(screen.getByText(/brew install smartmontools/)).toBeTruthy();
  });

  it("arrow keys switch to the history and a digit picks the range", async () => {
    await mount();
    await act(async () => {
      fireEvent.keyDown(window, { key: "ArrowRight" });
      await Promise.resolve();
    });
    expect(pulseHistory).toHaveBeenCalledWith("24h");
    await act(async () => {
      fireEvent.keyDown(window, { key: "5" });
      await Promise.resolve();
    });
    expect(pulseHistory).toHaveBeenLastCalledWith("90d");
    expect(screen.getByTestId("pulse-band").children.length).toBe(2);
    expect(screen.getByTestId("pulse-forecast").textContent).toMatch(/^Schätzung:/);
    expect(screen.getByTestId("pulse-days").textContent).toContain("Docker");
  });

  it("keys do nothing while unfocused, Esc exits when focused", async () => {
    const onExit = await mount(false);
    fireEvent.keyDown(window, { key: "ArrowRight" });
    expect(pulseHistory).not.toHaveBeenCalled();
    cleanup();
    const onExit2 = await mount(true);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onExit2).toHaveBeenCalled();
    expect(onExit).not.toHaveBeenCalled();
  });

  it("a disabled sampler says so instead of showing stale data", async () => {
    pulseLive.mockResolvedValue(liveData({ enabled: false }));
    await mount();
    expect(screen.getByText(/Hintergrundmessung ist ausgeschaltet/)).toBeTruthy();
    expect(screen.queryByTestId("pulse-level")).toBeNull();
  });

  it("before the first sample there's a waiting note, not zeros", async () => {
    pulseLive.mockResolvedValue(liveData({ latest: null, recent: [] }));
    await mount();
    expect(screen.getByText(/Erste Messung läuft/)).toBeTruthy();
  });
});
