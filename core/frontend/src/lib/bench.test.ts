import { describe, it, expect } from "vitest";
import {
  deltaPct, isSignificant, formatDelta, formatRate, machineLabel, osLabel,
  compareRows, mismatchedSchemas, NOISE_FLOOR_PCT, UNKNOWN,
  type BenchRun, type MachineInfo, type WorkloadResult,
} from "./bench";

const machine = (p: Partial<MachineInfo> = {}): MachineInfo => ({
  os_name: "macOS", os_version: "26.6.2", kernel: null, arch: "aarch64",
  device_model: "MacBookPro18,1", host_name: "host", cpu_brand: "Apple M1 Pro",
  physical_cores: 10, logical_cores: 10, mem_total_bytes: 32e9, ...p,
});

const wl = (id: string, score: number): WorkloadResult =>
  ({ id, name: id, unit: "MB/s", rate: score / 10, score, iterations: 3, seconds: 0.6 });

const run = (id: string, ids: [string, number][], schema = 1): BenchRun => ({
  schema, id, finished_at_ms: 0, duration_s: 8, app_version: "0",
  baseline_machine: "ref", machine: machine(), threads: 10,
  single: { score: 1000, workloads: ids.map(([i, s]) => wl(i, s)) },
  multi: { score: 5000, workloads: ids.map(([i, s]) => wl(i, s * 5)) },
});

describe("deltas", () => {
  it("is a signed percentage against the first run", () => {
    expect(deltaPct(1000, 1200)).toBeCloseTo(20);
    expect(deltaPct(1000, 800)).toBeCloseTo(-20);
  });
  it("refuses to divide by a zero or absent base", () => {
    expect(deltaPct(0, 100)).toBeNull();
    expect(deltaPct(NaN, 100)).toBeNull();
  });
  it("calls anything under the measured noise floor what it is", () => {
    // ⚠️ Two consecutive runs on the same idle machine differed by up to 5 %.
    // Presenting a 2 % difference as a finding would be inventing a result.
    expect(isSignificant(deltaPct(1000, 1020))).toBe(false);
    expect(isSignificant(deltaPct(1000, 1200))).toBe(true);
    expect(formatDelta(2)).toContain("Rauschen");
    expect(formatDelta(20)).not.toContain("Rauschen");
    expect(NOISE_FLOOR_PCT).toBe(5);
  });
});

describe("formatting", () => {
  it("drops decimals a big rate does not need", () => {
    expect(formatRate(10492.2, "MFLOP/s")).toBe("10492 MFLOP/s");
    expect(formatRate(21.5, "MB/s")).toBe("21,50 MB/s");
  });
  it("never invents a machine detail", () => {
    expect(machineLabel(machine({ device_model: null, cpu_brand: null, host_name: null }))).toBe(UNKNOWN);
    expect(osLabel(machine({ os_name: null, os_version: null }))).toBe(UNKNOWN);
    expect(osLabel(machine({ os_version: null }))).toBe("macOS");
    // ⚠️ The name already carries the version on macOS — do not print it twice.
    expect(osLabel(machine({ os_name: "MacOS 26.6.2", os_version: "26.6.2" }))).toBe("MacOS 26.6.2");
    expect(osLabel(machine({ os_name: "Ubuntu", os_version: "24.04" }))).toBe("Ubuntu 24.04");
  });
});

describe("compareRows", () => {
  it("joins by workload id, never by position", () => {
    // ⚠️ A run from another version can carry a different workload set;
    // lining those up by index would compare SHA-256 against a prime sieve.
    const a = run("a", [["sort", 1000], ["sha256", 1000]]);
    const b = run("b", [["sha256", 2000], ["sort", 900]]);
    const rows = compareRows([a, b], "single");
    const sha = rows.find((r) => r.id === "sha256")!;
    expect(sha.cells[0]!.score).toBe(1000);
    expect(sha.cells[1]!.score).toBe(2000);
    expect(sha.deltas[1]).toBeCloseTo(100);
  });

  it("leaves a gap where a run lacks the workload", () => {
    const a = run("a", [["sort", 1000], ["nbody", 1000]]);
    const b = run("b", [["sort", 1000]]);
    const rows = compareRows([a, b], "single");
    const nbody = rows.find((r) => r.id === "nbody")!;
    expect(nbody.cells[1]).toBeNull();
    expect(nbody.deltas[1]).toBeNull();
  });

  it("keeps the first run as the reference with no delta of its own", () => {
    const rows = compareRows([run("a", [["sort", 1000]]), run("b", [["sort", 1500]])], "single");
    expect(rows[0].deltas[0]).toBeNull();
    expect(rows[0].deltas[1]).toBeCloseTo(50);
  });

  it("handles the multi section and an empty list", () => {
    const rows = compareRows([run("a", [["sort", 100]])], "multi");
    expect(rows[0].cells[0]!.score).toBe(500);
    expect(compareRows([], "single")).toEqual([]);
  });
});

describe("mismatchedSchemas", () => {
  it("names runs recorded under a different workload set", () => {
    const old = run("old", [["sort", 1000]], 1);
    const now = run("now", [["sort", 1000]], 2);
    expect(mismatchedSchemas([now, old])).toEqual([old]);
    expect(mismatchedSchemas([now])).toEqual([]);
    expect(mismatchedSchemas([])).toEqual([]);
  });
});
