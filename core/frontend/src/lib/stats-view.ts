/**
 * Pure view logic of the live `stats` panel: how readings are grouped,
 * ranked and laid out. No DOM, no IPC — unit-tested.
 */

/** How busy the machine is, from the 1-minute load and the core count. */
export type LoadLevel = "idle" | "normal" | "busy" | "over";

/**
 * Load says how many processes want a core. Divided by the core count it
 * reads as: under 30 % idle, under 70 % normal, up to 100 % busy, above that
 * more work than cores.
 */
export function loadLevel(load1: number, cores: number): LoadLevel {
  if (!Number.isFinite(load1) || cores <= 0) return "idle";
  const ratio = load1 / cores;
  if (ratio > 1) return "over";
  if (ratio >= 0.7) return "busy";
  if (ratio >= 0.3) return "normal";
  return "idle";
}

export interface CoreReading {
  /** Position in the system's numbering (shown as "1", "2", …). */
  index: number;
  pct: number;
}

export interface CoreGroups {
  efficiency: CoreReading[];
  performance: CoreReading[];
}

/**
 * Split the per-core readings into efficiency and performance cores. Apple
 * Silicon numbers the efficiency cores first. `null` when the counts are
 * unknown or do not add up — then the cores are shown as one row.
 */
export function splitCores(perCore: number[], perf: number | null | undefined, eff: number | null | undefined): CoreGroups | null {
  if (!perf || !eff || perf + eff !== perCore.length) return null;
  const all = perCore.map((pct, index) => ({ index, pct }));
  return { efficiency: all.slice(0, eff), performance: all.slice(eff) };
}

export type MemKind = "wired" | "active" | "compressed" | "inactive" | "free";

export interface MemSegment {
  kind: MemKind;
  bytes: number;
  /** 0..1 of the sum of all segments. */
  share: number;
}

/** The order the stacked bar is drawn in: what cannot be given back first. */
export const MEM_ORDER: readonly MemKind[] = ["wired", "active", "compressed", "inactive", "free"];

/** The memory classes as shares of their sum (always adds up to 1). */
export function memSegments(m: Record<MemKind, number>): MemSegment[] {
  const clean = MEM_ORDER.map((kind) => ({ kind, bytes: Number.isFinite(m[kind]) && m[kind] > 0 ? m[kind] : 0 }));
  const sum = clean.reduce((a, s) => a + s.bytes, 0);
  return clean.map((s) => ({ ...s, share: sum > 0 ? s.bytes / sum : 0 }));
}

export type TempGroupKey = "cpu" | "gpu" | "battery" | "storage" | "other";

export const TEMP_ORDER: readonly TempGroupKey[] = ["cpu", "gpu", "battery", "storage", "other"];

/** Which part of the machine a sensor label belongs to. */
export function tempGroupOf(label: string): TempGroupKey {
  const l = label.toLowerCase();
  if (/battery|gas gauge/.test(l)) return "battery";
  if (/nand|ssd|nvme|drive|disk/.test(l)) return "storage";
  // Apple's "TP<n>g" are the graphics cores, "TP<n>s" the processor ones.
  if (/gpu|\btp\d+g\b/.test(l)) return "gpu";
  if (/cpu|core|package|tdie|tctl|tccd|\btc\d|\btp\d+s\b|[pe]acc/.test(l)) return "cpu";
  return "other";
}

export interface TempGroup<T> {
  key: TempGroupKey;
  max: number;
  avg: number;
  sensors: T[];
}

/** Sensors by part of the machine, in a fixed order, empty groups left out. */
export function groupTemps<T extends { label: string; celsius: number }>(temps: T[]): TempGroup<T>[] {
  return TEMP_ORDER.flatMap((key) => {
    const sensors = temps.filter((s) => tempGroupOf(s.label) === key);
    if (sensors.length === 0) return [];
    const max = Math.max(...sensors.map((s) => s.celsius));
    const avg = sensors.reduce((a, s) => a + s.celsius, 0) / sensors.length;
    return [{ key, max, avg, sensors }];
  });
}

export interface DiskLike {
  name: string;
  mount: string;
  total: number;
  available: number;
}

/**
 * One row per real volume: APFS shows the same container under several mount
 * points (`/`, `/System/Volumes/Data`, …) with identical numbers — keep the
 * first, which is the shortest path because `/` comes first.
 */
export function visibleDisks<T extends DiskLike>(disks: T[]): T[] {
  const seen = new Set<string>();
  const sorted = [...disks].sort((a, b) => a.mount.length - b.mount.length || a.mount.localeCompare(b.mount));
  return sorted.filter((d) => {
    if (d.total <= 0) return false;
    const key = `${d.total}|${d.available}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

/** Append a reading to a rolling window of `max` values (returns a new array). */
export function pushSample(buf: readonly number[], v: number, max: number): number[] {
  const next = [...buf, Number.isFinite(v) ? v : 0];
  return next.length > max ? next.slice(next.length - max) : next;
}

/**
 * SVG paths (line + filled area) of a rolling window in a `w`×`h` box. The
 * newest value sits on the right edge; a window that is not full yet starts
 * further right, so the curve never stretches. `top` fixes the scale (100
 * for percentages); without it the largest value fills the height.
 */
export function sparkPaths(values: readonly number[], slots: number, w: number, h: number, top?: number): { line: string; area: string } {
  if (values.length < 2 || slots < 2) return { line: "", area: "" };
  const peak = top ?? Math.max(...values, 0);
  const scale = peak > 0 ? peak : 1;
  const step = w / (slots - 1);
  const x0 = w - (values.length - 1) * step;
  const pts = values.map((v, i) => {
    const y = h - Math.max(0, Math.min(1, v / scale)) * h;
    return `${(x0 + i * step).toFixed(1)},${y.toFixed(1)}`;
  });
  const line = `M${pts.join(" L")}`;
  return { line, area: `${line} L${w.toFixed(1)},${h} L${x0.toFixed(1)},${h} Z` };
}

/** The card a digit key jumps to ("1" = first card); `null` for anything else. */
export function cardForKey<T>(key: string, cards: readonly T[]): T | null {
  if (!/^[1-9]$/.test(key)) return null;
  return cards[Number(key) - 1] ?? null;
}

/** Bar length of a list row relative to the largest row (0..100). */
export function shareOfTop(v: number, top: number): number {
  if (!Number.isFinite(v) || !Number.isFinite(top) || top <= 0 || v <= 0) return 0;
  return Math.min(100, (v / top) * 100);
}

/** Toggle a card id in the stored list of collapsed cards. */
export function toggleCollapsed(collapsed: readonly string[], id: string): string[] {
  return collapsed.includes(id) ? collapsed.filter((c) => c !== id) : [...collapsed, id];
}

/** Read the stored list of collapsed cards; anything unexpected is "none". */
export function parseCollapsed(raw: string | null): string[] {
  if (!raw) return [];
  try {
    const v: unknown = JSON.parse(raw);
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

/** Running totals of a stacked bar's parts (negative parts count as 0). */
export function stackUpTo(parts: readonly number[]): number[] {
  const out: number[] = [];
  parts.reduce((sum, v) => {
    const next = sum + (Number.isFinite(v) && v > 0 ? v : 0);
    out.push(next);
    return next;
  }, 0);
  return out;
}
