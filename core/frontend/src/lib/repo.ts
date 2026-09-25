/**
 * Pure display helpers for the `repo` panel (v0.123.0): chart scaling, commit
 * category colours, a month-timeline sparkline path, and formatting. Mirrors
 * the repo2viz aesthetic. The component owns rendering; every number here is
 * tested.
 */

import type { RangeKey } from "./ipc";

export const WEEKDAY_LABELS = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"] as const;

/** Category → colour (repo2viz-ish palette). Unknowns fall back to grey. */
export const CATEGORY_COLORS: Readonly<Record<string, string>> = {
  feat: "#81c995",
  fix: "#f28b82",
  refactor: "#c58af9",
  perf: "#fcc934",
  docs: "#8ab4f8",
  test: "#78d9ec",
  build: "#ff8bcb",
  ci: "#aecbfa",
  chore: "#9aa0a6",
  style: "#fdd663",
  revert: "#ee675c",
  other: "#5f6368",
};
export function categoryColor(cat: string): string {
  return CATEGORY_COLORS[cat] ?? "#5f6368";
}

/** Human integer with de-style thousands separators. */
export function formatNum(n: number): string {
  return new Intl.NumberFormat("de-DE").format(n);
}

/** ISO timestamp → "24.08.2026" (date only, robust to a bad string). */
export function shortDate(iso: string): string {
  const m = iso.match(/^(\d{4})-(\d{2})-(\d{2})/);
  if (!m) return iso || "—";
  return `${m[3]}.${m[2]}.${m[1]}`;
}

/** Bar width percent (0..100) for a value against the series max. */
export function barPct(value: number, max: number): number {
  if (max <= 0) return 0;
  return Math.max(0, Math.min(100, (value / max) * 100));
}

/** Peak bucket label of a numeric series given labels — "busiest hour/day". */
export function peakLabel(values: readonly number[], labels: readonly string[]): string {
  if (values.length === 0) return "—";
  let bi = 0;
  for (let i = 1; i < values.length; i++) if (values[i] > values[bi]) bi = i;
  return labels[bi] ?? String(bi);
}

/**
 * SVG polyline points for the month-commit sparkline, mapped into a `w×h`
 * box (0,0 top-left). Single point → a flat mid-line; empty → "".
 */
export function sparkPoints(commits: readonly number[], w: number, h: number, pad = 2): string {
  if (commits.length === 0) return "";
  const max = Math.max(1, ...commits);
  const innerW = w - pad * 2;
  const innerH = h - pad * 2;
  if (commits.length === 1) {
    const y = pad + innerH / 2;
    return `${pad},${y.toFixed(1)} ${(w - pad)},${y.toFixed(1)}`;
  }
  return commits
    .map((c, i) => {
      const x = pad + (i / (commits.length - 1)) * innerW;
      const y = pad + (1 - c / max) * innerH;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}

/** Total churn (ins+del) for the header readout. */
export function totalChurn(insertions: number, deletions: number): number {
  return insertions + deletions;
}

export const RANGES: readonly { key: RangeKey; label: string }[] = [
  { key: "d30", label: "30 T" },
  { key: "d90", label: "90 T" },
  { key: "d180", label: "180 T" },
  { key: "y1", label: "1 J" },
  { key: "all", label: "Gesamt" },
];

/** 0 (none) … 4 (max) — four visible steps like GitHub's calendar. */
export function heatLevel(v: number, max: number): 0 | 1 | 2 | 3 | 4 {
  if (v <= 0 || max <= 0) return 0;
  return Math.max(1, Math.min(4, Math.ceil((v / max) * 4))) as 1 | 2 | 3 | 4;
}

function dayOrdinal(date: string): number | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(date);
  if (!m) return null;
  return Math.floor(Date.UTC(+m[1], +m[2] - 1, +m[3]) / 86_400_000);
}

/** Week-column / weekday-row placement, Monday first — mirrors Rust
 *  `calendar_svg` (1970-01-01 was a Thursday → weekday = (ord+3) mod 7). */
export function calendarCells(days: readonly { date: string; commits: number }[]) {
  const parsed = days
    .map((d) => ({ d, o: dayOrdinal(d.date) }))
    .filter((x): x is { d: { date: string; commits: number }; o: number } => x.o !== null);
  if (parsed.length === 0) return [];
  const wd = (o: number) => (((o + 3) % 7) + 7) % 7;
  const first = Math.min(...parsed.map((x) => x.o));
  const start = first - wd(first);
  return parsed.map(({ d, o }) => ({
    col: Math.floor((o - start) / 7),
    row: wd(o),
    date: d.date,
    commits: d.commits,
  }));
}

export function repoErrorHint(err: string): { title: string; body: string } {
  if (err.includes("repo.no_target"))
    return {
      title: "Kein Repository.",
      body: "Eine GitHub-URL einfügen — oder im Finder einen Ordner mit .git auswählen.",
    };
  if (err.startsWith("repo.auth"))
    return {
      title: "Kein Zugriff auf das Repository.",
      body: "Privat oder nicht vorhanden. `gh auth login` im Terminal ausführen oder ein Token in Settings → Repositories hinterlegen.",
    };
  if (err.startsWith("repo.network"))
    return { title: "Keine Verbindung zu GitHub.", body: "Netz prüfen und erneut versuchen." };
  return { title: "Analyse fehlgeschlagen", body: err.replace(/^repo\.git:\s*/, "") };
}

const RANGE_TITLES: Record<RangeKey, string> = {
  d30: "letzte 30 Tage",
  d90: "letzte 90 Tage",
  d180: "letzte 180 Tage",
  y1: "letztes Jahr",
  all: "gesamt",
};

/** Activity heading names the RANGE — counting touched calendar months made a
 *  30-day window read "2 Monate". Mirrors Rust `RangeKey::label`. */
export function rangeTitle(range: RangeKey): string {
  return RANGE_TITLES[range];
}

/** Short bucket label: `25.09.` (day) · `ab 21.09.` (week) · `09/2026` (month). */
export function bucketLabel(start: string, granularity: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(start);
  if (!m) return start;
  if (granularity === "day") return `${m[3]}.${m[2]}.`;
  if (granularity === "week") return `ab ${m[3]}.${m[2]}.`;
  return `${m[2]}/${m[1]}`;
}

export function netTotal(insertions: number, deletions: number): number {
  return insertions - deletions;
}

export interface ChurnBar {
  x: number;
  w: number;
  addY: number;
  addH: number;
  delY: number;
  delH: number;
}

/**
 * Geometry of the code-changes chart — mirrors Rust `churn_svg`: ONE scale for
 * added (up) and removed (down) lines so proportions stay honest, the zero line
 * placed where that scale puts it (each side keeps ≥ 12 % when it has data),
 * and the cumulative net line scaled into the band around the zero line.
 */
export function churnGeometry(
  timeline: readonly { insertions: number; deletions: number }[],
  w: number,
  h: number,
  pad = 4,
): { zeroY: number; bars: ChurnBar[]; netPoints: [number, number][] } {
  if (timeline.length === 0) return { zeroY: h / 2, bars: [], netPoints: [] };
  const maxAdd = Math.max(0, ...timeline.map((b) => b.insertions));
  const maxDel = Math.max(0, ...timeline.map((b) => b.deletions));
  const span = h - 2 * pad;
  let up = (span * maxAdd) / Math.max(1, maxAdd + maxDel);
  if (maxDel > 0) up = Math.min(up, span * 0.88);
  if (maxAdd > 0) up = Math.max(up, span * 0.12);
  const zeroY = pad + up;
  const kAdd = maxAdd > 0 ? up / maxAdd : 0;
  const kDel = maxDel > 0 ? (span - up) / maxDel : 0;
  const k = kAdd > 0 && kDel > 0 ? Math.min(kAdd, kDel) : Math.max(kAdd, kDel);
  const bw = w / timeline.length;
  let net = 0;
  let hi = 0;
  let lo = 0;
  for (const b of timeline) {
    net += b.insertions - b.deletions;
    hi = Math.max(hi, net);
    lo = Math.min(lo, net);
  }
  const knUp = hi > 0 ? (zeroY - pad) / hi : Infinity;
  const knDn = lo < 0 ? (h - pad - zeroY) / -lo : Infinity;
  const kn = Number.isFinite(Math.min(knUp, knDn)) ? Math.min(knUp, knDn) : 0;
  net = 0;
  const bars: ChurnBar[] = [];
  const netPoints: [number, number][] = [];
  timeline.forEach((b, i) => {
    const addH = b.insertions * k;
    const delH = b.deletions * k;
    bars.push({ x: i * bw + bw * 0.15, w: Math.max(0.5, bw * 0.7), addY: zeroY - addH, addH, delY: zeroY, delH });
    net += b.insertions - b.deletions;
    netPoints.push([i * bw + bw / 2, zeroY - net * kn]);
  });
  return { zeroY, bars, netPoints };
}

/** Neutral change vs the previous period — fewer isn't automatically worse. */
export function deltaLabel(cur: number, prev: number): { text: string; title: string } {
  const text = cur > prev ? `↑ ${cur - prev}` : cur < prev ? `↓ ${prev - cur}` : "±0";
  return { text, title: `Vorperiode: ${prev}` };
}

export function githubErrorHint(err: string): string {
  if (err.startsWith("github.rate_limit")) return "GitHub-Abfragelimit erreicht — `gh auth login` oder ein Token in Settings → Repositories erhöht es.";
  if (err.startsWith("github.not_found")) return "Repo bei GitHub nicht gefunden oder kein Zugriff (privat?).";
  if (err.startsWith("repo.auth")) return "GitHub lehnt das Token ab — Token in Settings → Repositories prüfen.";
  if (err.startsWith("github.network")) return "GitHub nicht erreichbar — Git-Werte oben sind trotzdem aktuell.";
  return "GitHub-Werte nicht verfügbar.";
}

/**
 * Completeness of the four windows (24 h, previous 24 h, 7 d, previous 7 d)
 * for data that is complete only from `from` on (`null` = complete). Mirrors
 * Rust `windows_complete`: a window `(start, end]` is complete iff `from <= start`.
 */
export function windowsComplete(from: number | null, now: number): [boolean, boolean, boolean, boolean] {
  const ok = (start: number) => from === null || from <= start;
  const DAY = 86_400;
  return [ok(now - DAY), ok(now - 2 * DAY), ok(now - 7 * DAY), ok(now - 14 * DAY)];
}

