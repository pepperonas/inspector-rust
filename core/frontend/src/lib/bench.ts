/**
 * Pure helpers for the `benchmark` / `performance` command: formatting, and
 * the arithmetic behind the comparison view. Mirrors the Rust `bench` module's
 * data model (snake_case fields, the repo's IPC convention).
 */

export interface MachineInfo {
  os_name: string | null;
  os_version: string | null;
  kernel: string | null;
  arch: string | null;
  device_model: string | null;
  host_name: string | null;
  cpu_brand: string | null;
  physical_cores: number | null;
  logical_cores: number | null;
  mem_total_bytes: number | null;
}

export interface WorkloadResult {
  id: string;
  name: string;
  unit: string;
  rate: number;
  score: number;
  iterations: number;
  seconds: number;
}

export interface Section {
  score: number;
  workloads: WorkloadResult[];
}

export interface BenchRun {
  schema: number;
  id: string;
  finished_at_ms: number;
  duration_s: number;
  app_version: string;
  baseline_machine: string;
  machine: MachineInfo;
  threads: number;
  single: Section;
  multi: Section;
}

/** Shown wherever a value could not be read. Never a placeholder number. */
export const UNKNOWN = "nicht verfügbar";

/**
 * ⚠️ Measured, not assumed: two consecutive reference runs on the same idle
 * machine differed by up to 5 % per workload (sieve 1284 → 1212 Mkand/s, sort
 * 82.6 → 78.4 Melem/s). A comparison must therefore not present a 2 %
 * difference as a finding — below this it is noise.
 */
export const NOISE_FLOOR_PCT = 5;

/** Signed percentage change from `base` to `other`. */
export function deltaPct(base: number, other: number): number | null {
  if (!Number.isFinite(base) || !Number.isFinite(other) || base <= 0) return null;
  return ((other - base) / base) * 100;
}

/** Is a delta big enough to mean anything? See `NOISE_FLOOR_PCT`. */
export function isSignificant(delta: number | null): boolean {
  return delta !== null && Math.abs(delta) >= NOISE_FLOOR_PCT;
}

export function formatDelta(delta: number | null): string {
  if (delta === null) return "—";
  const s = `${delta >= 0 ? "+" : "−"}${Math.abs(delta).toFixed(1)} %`;
  return isSignificant(delta) ? s : `${s} (Rauschen)`;
}

/** Rate with a sensible number of digits — big rates do not need decimals. */
export function formatRate(rate: number, unit: string): string {
  if (!Number.isFinite(rate)) return UNKNOWN;
  const digits = rate >= 1000 ? 0 : rate >= 100 ? 1 : 2;
  return `${rate.toFixed(digits).replace(".", ",")} ${unit}`;
}

/** A short, honest column header for one run. */
export function machineLabel(m: MachineInfo): string {
  const parts = [m.device_model, m.cpu_brand].filter(Boolean) as string[];
  if (parts.length === 0) return m.host_name ?? UNKNOWN;
  return parts[0];
}

/** `macOS 26.6.2` — or as much of it as could be read. */
export function osLabel(m: MachineInfo): string {
  const name = m.os_name?.trim() || null;
  const ver = m.os_version?.trim() || null;
  if (!name && !ver) return UNKNOWN;
  if (!name) return ver!;
  if (!ver) return name;
  // ⚠️ On macOS the name already ENDS in the version; joining both printed
  // "MacOS 26.6.2  26.6.2" in a rendered report. Mirrors `bench_export::os_label`.
  return name.endsWith(ver) ? name : `${name} ${ver}`;
}

/** One row of the comparison table: a workload across every run. */
export interface CompareRow {
  id: string;
  name: string;
  unit: string;
  /** Per run, in the order given; `null` where that run lacks the workload. */
  cells: (WorkloadResult | null)[];
  /** Delta of each run against the FIRST one; `null` for the first itself. */
  deltas: (number | null)[];
}

/**
 * Line up runs for the comparison table, joined by workload id.
 *
 * ⚠️ Joined by ID, never by position: a run recorded by an older version can
 * have a different workload set, and lining those up by index would compare
 * SHA-256 against a prime sieve. A run that lacks a workload gets a gap.
 */
export function compareRows(runs: readonly BenchRun[], section: "single" | "multi"): CompareRow[] {
  if (runs.length === 0) return [];
  const order: string[] = [];
  const meta = new Map<string, { name: string; unit: string }>();
  for (const r of runs) {
    for (const w of r[section].workloads) {
      if (!meta.has(w.id)) {
        meta.set(w.id, { name: w.name, unit: w.unit });
        order.push(w.id);
      }
    }
  }
  return order.map((id) => {
    const cells = runs.map((r) => r[section].workloads.find((w) => w.id === id) ?? null);
    const base = cells[0];
    const deltas = cells.map((c, i) =>
      i === 0 || !base || !c ? null : deltaPct(base.score, c.score),
    );
    const m = meta.get(id)!;
    return { id, name: m.name, unit: m.unit, cells, deltas };
  });
}

/**
 * Runs recorded under a different workload schema than the newest one.
 * Comparing across a schema change would be comparing different work.
 */
export function mismatchedSchemas(runs: readonly BenchRun[]): BenchRun[] {
  if (runs.length === 0) return [];
  const newest = Math.max(...runs.map((r) => r.schema));
  return runs.filter((r) => r.schema !== newest);
}
