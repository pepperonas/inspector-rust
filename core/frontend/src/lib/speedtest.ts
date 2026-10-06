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
