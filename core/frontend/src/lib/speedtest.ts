// Pure helpers for the `speedtest` panel (v0.199.0). Backend: speedtest.rs.

export type SpeedPhase = "meta" | "latency" | "download" | "upload";

export const PHASE_LABELS: Record<SpeedPhase, string> = {
  meta: "Verbinde mit Cloudflare…",
  latency: "Latenz",
  download: "Download",
  upload: "Upload",
};

/** Share of the whole run each phase takes — drives one overall progress bar.
 *  Rough wall-clock shares from real runs (download dominates). */
const PHASE_SPAN: Record<SpeedPhase, [number, number]> = {
  meta: [0, 0.03],
  latency: [0.03, 0.15],
  download: [0.15, 0.7],
  upload: [0.7, 1],
};

/** Overall 0..1 progress from a phase and its own 0..1 fraction. */
export function overallProgress(phase: SpeedPhase, fraction: number): number {
  const [a, b] = PHASE_SPAN[phase] ?? [0, 1];
  const f = Number.isFinite(fraction) ? Math.min(1, Math.max(0, fraction)) : 0;
  return a + (b - a) * f;
}

/** Bits/s → Mbit/s display string: three significant digits, German decimal
 *  comma ("452", "44,2", "8,37", "0,95"). `—` for no value. */
export function formatMbps(bps: number | null | undefined): string {
  if (bps == null || !Number.isFinite(bps) || bps < 0) return "—";
  const m = bps / 1_000_000;
  const digits = m >= 100 ? 0 : m >= 10 ? 1 : 2;
  return m.toFixed(digits).replace(".", ",");
}

/** Milliseconds → "39 ms" / "6,5 ms" (one decimal under 10). */
export function formatMs(ms: number | null | undefined): string {
  if (ms == null || !Number.isFinite(ms) || ms < 0) return "—";
  return `${(ms < 10 ? ms.toFixed(1) : Math.round(ms).toString()).replace(".", ",")} ms`;
}

export type Rating = "sehr gut" | "gut" | "mittel" | "langsam";

/** Plain-language verdict on a download rate (what it's good for). */
export function rateDownload(bps: number | null | undefined): Rating | null {
  if (bps == null || !Number.isFinite(bps)) return null;
  const m = bps / 1_000_000;
  if (m >= 250) return "sehr gut";
  if (m >= 50) return "gut";
  if (m >= 16) return "mittel";
  return "langsam";
}

/** What a download rate is enough for — one short line. */
export function downloadMeaning(bps: number | null | undefined): string | null {
  const r = rateDownload(bps);
  if (!r) return null;
  return {
    "sehr gut": "Reicht für mehrere 4K-Streams und große Downloads gleichzeitig.",
    gut: "Reicht für 4K-Streaming und Videokonferenzen.",
    mittel: "Reicht für HD-Streaming und Videokonferenzen.",
    langsam: "Reicht für Surfen und Mail; Video kann ruckeln.",
  }[r];
}

/** Relative change of `now` against the median of `previous` values, in
 *  percent (rounded). `null` when there is nothing to compare. */
export function deltaPercent(now: number | null | undefined, previous: (number | null | undefined)[]): number | null {
  if (now == null || !Number.isFinite(now)) return null;
  const prev = previous.filter((v): v is number => v != null && Number.isFinite(v) && v > 0).sort((a, b) => a - b);
  if (prev.length === 0) return null;
  const mid = prev.length / 2;
  const median = prev.length % 2 ? prev[Math.floor(mid)] : (prev[mid - 1] + prev[mid]) / 2;
  return Math.round(((now - median) / median) * 100);
}

/** Bar height share (0..1) of a value against the largest one shown. */
export function barShare(v: number | null | undefined, max: number): number {
  if (v == null || !Number.isFinite(v) || !(max > 0)) return 0;
  return Math.min(1, Math.max(0, v / max));
}

// ── History detail (v0.199.1) ───────────────────────────────────────────────

export interface MetricStats {
  median: number;
  min: number;
  max: number;
  /** How many runs had a value for this metric. */
  n: number;
}

/** Median / min / max of one metric over the runs that have a value. `null`
 *  when none has (a missing phase never counts as 0). */
export function metricStats(values: (number | null | undefined)[]): MetricStats | null {
  const v = values.filter((x): x is number => x != null && Number.isFinite(x)).sort((a, b) => a - b);
  if (v.length === 0) return null;
  const mid = v.length / 2;
  const median = v.length % 2 ? v[Math.floor(mid)] : (v[mid - 1] + v[mid]) / 2;
  return { median, min: v[0], max: v[v.length - 1], n: v.length };
}

/**
 * SVG polyline segments for a series drawn oldest → newest across `width`,
 * mapped from [`min`, `max`] onto `height` (y grows downwards). A missing value
 * BREAKS the line — a gap must not read as a drop. One point → a zero-length
 * segment so it stays visible. `max == min` draws a flat line in the middle.
 */
export function trendSegments(
  values: (number | null | undefined)[],
  max: number,
  width: number,
  height: number,
  min = 0,
): string[] {
  if (!Number.isFinite(max) || !Number.isFinite(min) || max < min || values.length === 0) return [];
  if (max === min && max === 0) return [];
  const span = max - min;
  const step = values.length > 1 ? width / (values.length - 1) : 0;
  const segs: string[] = [];
  let cur: string[] = [];
  values.forEach((v, i) => {
    if (v == null || !Number.isFinite(v)) {
      if (cur.length) segs.push(cur.join(" "));
      cur = [];
      return;
    }
    const x = values.length > 1 ? i * step : width / 2;
    const f = span > 0 ? Math.min(1, Math.max(0, (v - min) / span)) : 0.5;
    const y = height - f * height;
    cur.push(`${x.toFixed(1)},${y.toFixed(1)}`);
  });
  if (cur.length) segs.push(cur.join(" "));
  return segs.map((s) => (s.includes(" ") ? s : `${s} ${s}`));
}

/**
 * Y range for one trend: the data's own min..max, widened by 10 % of the span
 * on both sides so the line never sits on the frame — each metric gets its own
 * scale (upload at 20 Mbit/s is invisible on a 400-Mbit/s download axis).
 */
export function trendRange(values: (number | null | undefined)[]): { min: number; max: number } | null {
  const s = metricStats(values);
  if (!s) return null;
  const pad = (s.max - s.min) * 0.1;
  return { min: Math.max(0, s.min - pad), max: s.max + pad };
}
