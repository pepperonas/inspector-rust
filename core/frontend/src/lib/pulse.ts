/**
 * Pure display logic for the `pulse` panel (memory pressure, swap, SSD
 * writes). Everything here is unit-tested; the component only draws.
 */
import type { PulseAgg, PulseForecast, PulseRange, PulseSample } from "./ipc";

export type PressureLevel = "normal" | "warn" | "critical" | "unknown";

/** kern.memorystatus_vm_pressure_level: 1 normal, 2 warn, 4 critical. */
export function pressureLevel(raw: number | null | undefined): PressureLevel {
  if (raw == null || raw <= 0) return "unknown";
  if (raw >= 4) return "critical";
  if (raw >= 2) return "warn";
  return "normal";
}

export const PRESSURE_LABEL: Record<PressureLevel, string> = {
  normal: "Normal",
  warn: "Erhöht",
  critical: "Kritisch",
  unknown: "Unbekannt",
};

/** Same traffic-light colours as the rest of the app (emerald/amber/rose). */
export const PRESSURE_COLOR: Record<PressureLevel, string> = {
  normal: "#10b981",
  warn: "#f59e0b",
  critical: "#f43f5e",
  unknown: "#94a3b8",
};

export const PULSE_RANGES: PulseRange[] = ["1h", "24h", "7d", "30d", "90d"];

export const RANGE_LABEL: Record<PulseRange, string> = {
  "1h": "1 h",
  "24h": "24 h",
  "7d": "7 T",
  "30d": "30 T",
  "90d": "90 T",
};

const nf = (digits: number) =>
  new Intl.NumberFormat("de-DE", { minimumFractionDigits: digits, maximumFractionDigits: digits });

/** Decimal bytes (as macOS and drive makers count). */
export function formatBytes(b: number | null | undefined): string {
  if (b == null || !Number.isFinite(b) || b < 0) return "–";
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  let v = b;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i += 1;
  }
  const digits = i === 0 ? 0 : v < 10 ? 2 : v < 100 ? 1 : 0;
  return `${nf(digits).format(v)} ${units[i]}`;
}

/** Bytes per second → "12,4 MB/s"; tiny rates read as 0. */
export function formatRate(bps: number | null | undefined): string {
  if (bps == null || !Number.isFinite(bps)) return "–";
  if (bps < 1000) return "0 MB/s";
  return `${formatBytes(bps)}/s`;
}

export function formatPerSec(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return "–";
  if (n >= 1000) return `${nf(1).format(n / 1000)} k/s`;
  return `${nf(0).format(n)}/s`;
}

/** Seconds → "3 h 12 min" / "12 min" / "< 1 min". */
export function formatMinutes(secs: number): string {
  if (!Number.isFinite(secs) || secs < 60) return secs > 0 ? "< 1 min" : "0 min";
  const m = Math.round(secs / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  return m % 60 ? `${h} h ${m % 60} min` : `${h} h`;
}

/** Fractions of measured time per pressure level; empty bucket → all 0. */
export function pressureShares(a: Pick<PulseAgg, "p_normal" | "p_warn" | "p_crit">) {
  const total = a.p_normal + a.p_warn + a.p_crit;
  if (total <= 0) return { normal: 0, warn: 0, critical: 0 };
  return { normal: a.p_normal / total, warn: a.p_warn / total, critical: a.p_crit / total };
}

/** The dominant level of a bucket (for the pressure band). Ties go to the
 *  worse level — a bucket that was half red should read red. */
export function dominantLevel(a: Pick<PulseAgg, "p_normal" | "p_warn" | "p_crit" | "secs">): PressureLevel {
  if (a.secs <= 0) return "unknown";
  if (a.p_crit > 0 && a.p_crit >= a.p_warn && a.p_crit >= a.p_normal) return "critical";
  if (a.p_warn > 0 && a.p_warn >= a.p_normal) return "warn";
  return "normal";
}

/** Swap share of the SSD writes, 0..1 (estimate; capped at 1). */
export function swapShare(written: number, swapWritten: number): number | null {
  if (!(written > 0)) return null;
  return Math.min(1, Math.max(0, swapWritten / written));
}

/** One series out of the recent samples, for a sparkline. */
export function series(samples: PulseSample[], key: keyof PulseSample): (number | null)[] {
  return samples.map((s) => {
    const v = s[key];
    return typeof v === "number" && Number.isFinite(v) ? v : null;
  });
}

/** Bar geometry for the writes chart: total and swap height per bucket,
 *  scaled to the largest total. */
export function writeBars(buckets: Pick<PulseAgg, "written" | "swap_written">[], height: number) {
  const max = Math.max(0, ...buckets.map((b) => b.written));
  return buckets.map((b) => {
    if (max <= 0) return { total: 0, swap: 0 };
    const total = (b.written / max) * height;
    const swap = Math.min(total, (b.swap_written / max) * height);
    return { total, swap };
  });
}

/** Label for a bucket start, depending on the bucket size. */
export function bucketLabel(ts: number, bucketSecs: number): string {
  const d = new Date(ts * 1000);
  if (bucketSecs >= 86_400) return d.toLocaleDateString("de-DE", { day: "2-digit", month: "2-digit" });
  return d.toLocaleTimeString("de-DE", { hour: "2-digit", minute: "2-digit" });
}

export function dayLabel(ts: number): string {
  return new Date(ts * 1000).toLocaleDateString("de-DE", { weekday: "short", day: "2-digit", month: "2-digit" });
}

/** The forecast sentence. Always says it's an estimate; never invents a
 *  figure from less than a day of data. */
export function forecastText(f: PulseForecast | null): string {
  if (!f) return "Keine interne SSD erkannt — keine Prognose.";
  if (f.years == null) return "Noch zu wenig Daten (mindestens ein Tag) für eine Prognose.";
  const years = f.years >= 100 ? "über 100 Jahre" : `ca. ${nf(f.years < 10 ? 1 : 0).format(f.years)} Jahre`;
  const base = f.lifetime_written != null ? "bis zur Nennhaltbarkeit" : "bis die Nennhaltbarkeit erreicht wäre (ab null gerechnet)";
  return `Schätzung: ${years} ${base}, bei ${formatBytes(f.per_day)} pro Tag (Schnitt über ${nf(f.days < 10 ? 1 : 0).format(f.days)} Tage, ${formatBytes(f.tbw_bytes)} TBW).`;
}

/** "vor 12 s" for the process-scan freshness. */
export function ageText(nowSecs: number, ts: number): string {
  if (!ts) return "noch nicht erfasst";
  const s = Math.max(0, Math.round(nowSecs - ts));
  return s < 60 ? `vor ${s} s` : `vor ${Math.round(s / 60)} min`;
}
