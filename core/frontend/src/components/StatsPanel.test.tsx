import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SystemStats, TopProcs } from "../lib/ipc";

const GB = 1024 ** 3;
const STATS: SystemStats = {
  host_name: "host", os_name: "macOS", kernel: "1", cpu_arch: "arm64", uptime_secs: 3600,
  cpu_brand: "Test CPU", cpu_usage: 40, cpu_freq_mhz: 3200, physical_cores: 4, logical_cores: 4,
  per_core: [10, 20, 30, 40], load_avg: [5, 4, 3],
  mem_total: 16 * GB, mem_used: 8 * GB, mem_available: 0, swap_total: 2 * GB, swap_used: GB,
  disks: [
    { name: "Macintosh HD", mount: "/", fs: "apfs", total: 500 * GB, available: 100 * GB, removable: false, kind: "SSD" },
    { name: "Macintosh HD", mount: "/System/Volumes/Data", fs: "apfs", total: 500 * GB, available: 100 * GB, removable: false, kind: "SSD" },
  ],
  net_rx_per_sec: 1024, net_tx_per_sec: 2048,
  temps: [{ label: "PMU tdie1", celsius: 50 }, { label: "PMU tdie2", celsius: 61 }, { label: "NAND CH0 temp", celsius: 35 }],
  fans: [],
  battery: null,
  cpu_user: 30, cpu_system: 10, perf_cores: 2, eff_cores: 2,
  memory: { pressure: 1, wired: 2 * GB, active: 6 * GB, inactive: 4 * GB, compressed: 2 * GB, free: 2 * GB, swapout_bps: 2 * 1024 * 1024, swapin_bps: 0 },
  disk_io: { read_bps: 1024, write_bps: 4096 },
  net_ifaces: [{ name: "en0", rx_per_sec: 1024, tx_per_sec: 2048, addr: "192.0.2.7" }],
};
const PROCS: TopProcs = {
  cpu_ready: true,
  by_cpu: [{ name: "Compiler", cpu_pct: 180, mem: GB, procs: 3 }],
  by_mem: [{ name: "Browser", cpu_pct: 1, mem: 3 * GB, procs: 12 }],
};

const getSystemStats = vi.fn(async () => STATS);
const statsTopProcesses = vi.fn(async () => PROCS);
vi.mock("../lib/ipc", () => ({
  getSystemStats: () => getSystemStats(),
  statsTopProcesses: () => statsTopProcesses(),
  getStatsHistory: vi.fn(async () => ({ points: [], range_secs: 3600, sample_count: 0, interval_secs: 60 })),
}));

import { StatsPanel } from "./StatsPanel";

async function open(focused = true) {
  const view = render(<StatsPanel focused={focused} onExit={() => {}} />);
  await act(async () => {
    await vi.advanceTimersByTimeAsync(50);
  });
  return view;
}

beforeEach(() => {
  vi.useFakeTimers();
  window.localStorage.clear();
  getSystemStats.mockClear();
  statsTopProcesses.mockClear();
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("StatsPanel — live view", () => {
  it("shows the overview and the detail a reading carries", async () => {
    await open();
    expect(screen.getByTestId("stats-overview")).toBeTruthy();
    // Memory pressure in words, never "0 B free".
    expect(screen.getByTestId("stats-pressure").textContent).toContain("Raised");
    expect(screen.queryByText(/0 B free/)).toBeNull();
    // Load 5 on 4 cores is more work than cores.
    expect(screen.getByTestId("stats-load-level").textContent).toBe("more work than cores");
    // Cores by kind, processes by app, swap direction, disk rates, interface.
    expect(screen.getByText("Performance cores")).toBeTruthy();
    expect(screen.getByText("Efficiency cores")).toBeTruthy();
    expect(screen.getByText("Compiler")).toBeTruthy();
    expect(screen.getByText("Browser")).toBeTruthy();
    expect(screen.getByTestId("stats-swap-flow").textContent).toContain("2.0 MB/s");
    expect(screen.getByTestId("stats-disk-io")).toBeTruthy();
    expect(screen.getByText("192.0.2.7")).toBeTruthy();
  });

  it("lists one volume once and sums sensors per part of the machine", async () => {
    await open();
    expect(screen.queryByText(/\/System\/Volumes\/Data/)).toBeNull();
    expect(screen.getByText("Processor")).toBeTruthy();
    expect(screen.getByText("SSD")).toBeTruthy();
    // Single sensors appear only after the group is opened.
    expect(screen.queryByText("PMU tdie2")).toBeNull();
    fireEvent.click(screen.getByText("Processor"));
    expect(screen.getByText("PMU tdie2")).toBeTruthy();
  });

  it("polls the process lists more slowly than the readings", async () => {
    await open();
    // Past the one early re-read that fetches the first CPU shares.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1300);
    });
    getSystemStats.mockClear();
    statsTopProcesses.mockClear();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(6000);
    });
    expect(getSystemStats.mock.calls.length).toBe(4);
    expect(statsTopProcesses.mock.calls.length).toBe(2);
  });

  it("stops polling when it closes", async () => {
    const view = await open();
    view.unmount();
    getSystemStats.mockClear();
    statsTopProcesses.mockClear();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10000);
    });
    expect(getSystemStats).not.toHaveBeenCalled();
    expect(statsTopProcesses).not.toHaveBeenCalled();
  });

  it("folds a card and remembers it", async () => {
    const view = await open();
    expect(screen.getByText("Performance cores")).toBeTruthy();
    fireEvent.click(document.querySelector('[data-card="cpu"] button')!);
    expect(screen.queryByText("Performance cores")).toBeNull();
    expect(JSON.parse(window.localStorage.getItem("ir.stats.collapsed.v1") ?? "[]")).toEqual(["cpu"]);
    view.unmount();
    await open();
    expect(screen.queryByText("Performance cores")).toBeNull();
    // The other cards are untouched.
    expect(screen.getByTestId("stats-pressure")).toBeTruthy();
  });

  it("Cmd/Ctrl + digit unfolds and jumps; a bare digit is left to the search field", async () => {
    window.localStorage.setItem("ir.stats.collapsed.v1", JSON.stringify(["memory"]));
    await open();
    expect(screen.queryByTestId("stats-swap-flow")).toBeNull();
    const bare = new KeyboardEvent("keydown", { key: "2", bubbles: true, cancelable: true });
    window.dispatchEvent(bare);
    expect(bare.defaultPrevented).toBe(false);
    expect(screen.queryByTestId("stats-swap-flow")).toBeNull();
    const chord = new KeyboardEvent("keydown", { key: "2", metaKey: true, ctrlKey: true, bubbles: true, cancelable: true });
    await act(async () => {
      window.dispatchEvent(chord);
    });
    expect(chord.defaultPrevented).toBe(true);
    expect(screen.getByTestId("stats-swap-flow")).toBeTruthy();
  });

  it("takes no keys while it does not have the keyboard", async () => {
    window.localStorage.setItem("ir.stats.collapsed.v1", JSON.stringify(["memory"]));
    await open(false);
    const chord = new KeyboardEvent("keydown", { key: "2", metaKey: true, ctrlKey: true, bubbles: true, cancelable: true });
    await act(async () => {
      window.dispatchEvent(chord);
    });
    expect(chord.defaultPrevented).toBe(false);
    expect(screen.queryByTestId("stats-swap-flow")).toBeNull();
  });

  it("scrolls from the search field, but leaves other text fields alone", async () => {
    await open();
    const search = document.createElement("input");
    search.setAttribute("data-main-search", "");
    const other = document.createElement("input");
    document.body.append(search, other);
    const fromSearch = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true });
    search.dispatchEvent(fromSearch);
    expect(fromSearch.defaultPrevented).toBe(true);
    const fromOther = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true });
    other.dispatchEvent(fromOther);
    expect(fromOther.defaultPrevented).toBe(false);
    search.remove();
    other.remove();
  });
});
