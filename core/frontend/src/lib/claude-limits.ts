// Pure display helpers for the `limits` / `usage` panel (Claude subscription
// usage limits). The data comes from `claude_limits.rs`.

import type { LimitForecast } from "./ipc";

export const LIMITS_TZ = "Europe/Berlin";

/** Bar fill 0..100 (the API may report > 100 on an overrun). */
export function barFill(percent: number): number {
  if (!Number.isFinite(percent)) return 0;
  return Math.max(0, Math.min(100, percent));
}

export type LimitTone = "ok" | "warn" | "crit";

/** Colour band of a usage bar: < 75 ok, < 90 warn, else crit. */
export function limitTone(percent: number): LimitTone {
  if (percent >= 90) return "crit";
  if (percent >= 75) return "warn";
  return "ok";
}

/** "6 %" / "99,7 %" — integers without a decimal, others with one. */
export function formatPercent(percent: number): string {
  if (!Number.isFinite(percent)) return "–";
  const r = Math.round(percent * 10) / 10;
  const s = Number.isInteger(r)
    ? String(r)
    : r.toLocaleString("de-DE", { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  return `${s} %`;
}

/**
 * Relative time until `targetMs` from `nowMs`: "in 3 Std. 12 Min.",
 * "in 2 T. 5 Std.", "in 4 Min.", "jetzt". Past times → "jetzt".
 */
export function relativeReset(targetMs: number, nowMs: number): string {
  const diff = targetMs - nowMs;
  if (!Number.isFinite(diff) || diff < 60_000) return "jetzt";
  const min = Math.floor(diff / 60_000);
  const days = Math.floor(min / 1440);
  const hours = Math.floor((min % 1440) / 60);
  const mins = min % 60;
  if (days > 0) return hours > 0 ? `in ${days} T. ${hours} Std.` : `in ${days} T.`;
  if (hours > 0) return mins > 0 ? `in ${hours} Std. ${mins} Min.` : `in ${hours} Std.`;
  return `in ${mins} Min.`;
}

/** Absolute reset time in Europe/Berlin: "Sa., 10.10., 01:00". */
export function absoluteReset(targetMs: number, tz: string = LIMITS_TZ): string {
  return new Intl.DateTimeFormat("de-DE", {
    timeZone: tz,
    weekday: "short",
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(targetMs));
}

/** "Stand 14:32" in Europe/Berlin. */
export function standLabel(ms: number, tz: string = LIMITS_TZ): string {
  return (
    "Stand " +
    new Intl.DateTimeFormat("de-DE", { timeZone: tz, hour: "2-digit", minute: "2-digit" }).format(
      new Date(ms),
    )
  );
}

/** Money with its currency, German format. */
export function formatMoney(amount: number, currency: string): string {
  try {
    return new Intl.NumberFormat("de-DE", { style: "currency", currency }).format(amount);
  } catch {
    return `${amount.toFixed(2)} ${currency}`;
  }
}

/** Human text for a backend error code. */
export function limitsErrorText(code: string | null): string | null {
  switch (code) {
    case null:
      return null;
    case "limits.token_expired":
      return "Token abgelaufen — Claude Code einmal starten.";
    case "limits.no_token":
      return "Kein Claude-Code-Login gefunden — Claude Code starten und anmelden.";
    case "limits.rate_limited":
      return "Zu viele Abfragen (429) — neuer Versuch später.";
    case "limits.network":
      return "Anthropic nicht erreichbar.";
    case "limits.schema":
      return "Unerwartete Antwort der API — Format hat sich geändert.";
    default:
      return code;
  }
}

/**
 * Panel state: "laden" (nothing yet), "fehler" (error and no data), "veraltet"
 * (data, but the last attempt failed or it is older than 2 poll intervals),
 * "aktuell".
 */
export type LimitsPhase = "laden" | "fehler" | "veraltet" | "aktuell";

export function limitsPhase(
  hasReport: boolean,
  error: string | null,
  fetchedAtMs: number | null,
  pollMinutes: number,
  nowMs: number,
): LimitsPhase {
  if (!hasReport) return error ? "fehler" : "laden";
  if (error) return "veraltet";
  if (fetchedAtMs != null && nowMs - fetchedAtMs > 2 * pollMinutes * 60_000) return "veraltet";
  return "aktuell";
}

/** "gerade eben", "vor 12 Min.", "vor 3 Std.", "vor 2 T." */
export function sinceLabel(thenMs: number, nowMs: number): string {
  const d = nowMs - thenMs;
  if (!Number.isFinite(d) || d < 60_000) return "gerade eben";
  const min = Math.floor(d / 60_000);
  if (min < 60) return `vor ${min} Min.`;
  const h = Math.floor(min / 60);
  if (h < 24) return `vor ${h} Std.`;
  return `vor ${Math.floor(h / 24)} T.`;
}

export const POLL_CHOICES = [2, 5, 10, 15, 30, 60];

// ── Projection (v0.196.0) ───────────────────────────────────────────────────
// The forecast comes ready-made from Rust (Token Tracker or local linear);
// these helpers only turn it into text, colours and geometry.


/** Status colour: reserve → ok, ahead → warn, exhausts → crit, else none. */
export function forecastTone(f: LimitForecast | null | undefined): LimitTone | null {
  switch (f?.status) {
    case "reserve":
      return "ok";
    case "ahead":
      return "warn";
    case "exhausts":
      return "crit";
    default:
      return null;
  }
}

export interface BarSegments {
  /** Measured usage, 0..100. */
  fill: number;
  /** The even line through the window, 0..100 (null = unknown). */
  plan: number | null;
  /** Where the projection ends at the reset, 0..100 (null = no projection
   *  beyond the measured fill). */
  projection: number | null;
  /** The projection runs past 100 → red cap at the end. */
  over: boolean;
}

export function barSegments(percent: number, f: LimitForecast | null | undefined): BarSegments {
  const fill = barFill(percent);
  const plan = f?.pace ? barFill(f.pace.planPercent) : null;
  const median = f?.atReset?.median;
  const projection = median != null && Number.isFinite(median) && median > fill ? barFill(median) : null;
  return { fill, plan, projection, over: median != null && median > 100 };
}

/** "Mi 22 Uhr" / "Mi 22:30 Uhr" in Europe/Berlin. */
function dayHour(ms: number, tz: string): string {
  const parts = new Intl.DateTimeFormat("de-DE", {
    timeZone: tz,
    weekday: "short",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  }).formatToParts(new Date(ms));
  const get = (t: string) => parts.find((p) => p.type === t)?.value ?? "";
  const wd = get("weekday").replace(".", "");
  const h = String(Number(get("hour")));
  const m = get("minute");
  return m === "00" ? `${wd} ${h} Uhr` : `${wd} ${h}:${m} Uhr`;
}

/** "Do ~14:20" */
function dayTime(ms: number, tz: string): string {
  const parts = new Intl.DateTimeFormat("de-DE", {
    timeZone: tz,
    weekday: "short",
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  }).formatToParts(new Date(ms));
  const get = (t: string) => parts.find((p) => p.type === t)?.value ?? "";
  return `${get("weekday").replace(".", "")} ~${get("hour")}:${get("minute")}`;
}

/** "~1:40 h" / "~25 min" */
function inHours(diffMs: number): string {
  const min = Math.max(0, Math.round(diffMs / 60_000));
  if (min < 60) return `~${min} min`;
  return `~${Math.floor(min / 60)}:${String(min % 60).padStart(2, "0")} h`;
}

function windowMinutes(f: LimitForecast): number | null {
  if (!f.window) return null;
  const s = Date.parse(f.window.start);
  const e = Date.parse(f.window.end);
  return Number.isFinite(s) && Number.isFinite(e) ? (e - s) / 60_000 : null;
}

/** "7 Punkte Reserve" / "8 Punkte voraus" / "genau im Plan". */
export function paceText(deltaPoints: number): string {
  const n = Math.round(Math.abs(deltaPoints));
  if (n < 1) return "genau im Plan";
  const unit = n === 1 ? "Punkt" : "Punkte";
  return deltaPoints < 0 ? `${n} ${unit} Reserve` : `${n} ${unit} voraus`;
}

/**
 * The status line under a bar:
 * - "7 Punkte Reserve · voraussichtlich 78 % · Reset Sa 01:00"
 * - "leer Do ~14:20 (Mi 22 – Fr 9 Uhr)"   (week)
 * - "leer in ~1:40 h"                      (session)
 * - "… · grobe Schätzung" when the confidence is rough.
 * `null` when there is nothing to say (unknown / no forecast).
 */
export function forecastText(
  f: LimitForecast | null | undefined,
  nowMs: number,
  tz: string = LIMITS_TZ,
): string | null {
  if (!f || f.status === "unknown") return null;
  if (f.status === "idle") return "noch nichts verbraucht";
  const parts: string[] = [];
  const short = (windowMinutes(f) ?? 0) <= 360;
  const ex = f.exhaustsAt;
  if (f.status === "exhausts" && ex?.median) {
    const at = Date.parse(ex.median);
    if (short || at - nowMs < 60_000) {
      parts.push(at - nowMs < 60_000 ? "aufgebraucht" : `leer in ${inHours(at - nowMs)}`);
    } else {
      let range = "";
      if (ex.early && ex.late) range = ` (${dayHour(Date.parse(ex.early), tz)} – ${dayHour(Date.parse(ex.late), tz)})`;
      else if (ex.early) range = ` (frühestens ${dayHour(Date.parse(ex.early), tz)})`;
      parts.push(`leer ${dayTime(at, tz)}${range}`);
    }
  } else {
    if (f.pace) parts.push(paceText(f.pace.deltaPoints));
    if (f.notes.includes("too_early")) parts.push("zu früh für eine Hochrechnung");
    else if (f.atReset) parts.push(`voraussichtlich ${Math.round(f.atReset.median)} %`);
    if (f.window && !short) {
      const end = Date.parse(f.window.end);
      if (Number.isFinite(end)) parts.push(`Reset ${dayTime(end, tz).replace("~", "")}`);
    }
  }
  if (f.confidence === "rough") parts.push("grobe Schätzung");
  return parts.join(" · ");
}

// ── Window chart geometry (pure) ────────────────────────────────────────────

export interface ChartModel {
  width: number;
  height: number;
  /** Top of the y axis: 100, or 120 when anything runs over. */
  yMax: number;
  /** y of the 100 % line (the red zone sits above it). */
  y100: number;
  plan: string;
  actual: string;
  /** Median from now to the reset (dashed). */
  median: string;
  /** Closed area between low and high. */
  band: string;
  measured: { x: number; y: number }[];
  ghosts: string[];
  nowX: number | null;
  /** First point where the median reaches 100 (null = it doesn't). */
  hit: { x: number; y: number; ms: number } | null;
  /** Night shading 22–7 h, as x ranges. */
  nights: { x: number; w: number }[];
}

/** Hour (0–23) of `ms` in `tz`. */
function hourIn(ms: number, tz: string): number {
  // ⚠️ formatToParts, not format: de-DE renders a bare hour as "01 Uhr",
  // which Number() turns into NaN — no night would ever be shaded.
  const h = new Intl.DateTimeFormat("de-DE", { timeZone: tz, hour: "2-digit", hourCycle: "h23" })
    .formatToParts(new Date(ms))
    .find((p) => p.type === "hour")?.value;
  return Number(h);
}

function path(points: { x: number; y: number }[]): string {
  return points.map((p, i) => `${i ? "L" : "M"}${p.x.toFixed(1)} ${p.y.toFixed(1)}`).join(" ");
}

/**
 * Geometry for the window chart, from a tracker forecast with `series`.
 * `null` when there is no window or series to draw.
 */
export function chartModel(
  f: LimitForecast | null | undefined,
  width: number,
  height: number,
  tz: string = LIMITS_TZ,
): ChartModel | null {
  if (!f?.window || !f.series) return null;
  const s0 = Date.parse(f.window.start);
  const s1 = Date.parse(f.window.end);
  if (!Number.isFinite(s0) || !Number.isFinite(s1) || s1 <= s0) return null;
  const series = f.series;
  const peak = Math.max(
    0,
    ...series.actual.map((p) => p[1]),
    ...series.measured.map((p) => p[1]),
    ...series.forecast.map((p) => p[3]),
    ...series.ghosts.flatMap((g) => g.points.map((p) => p[1])),
  );
  const yMax = peak > 100 ? 120 : 100;
  const x = (ms: number) => ((Math.min(Math.max(ms, s0), s1) - s0) / (s1 - s0)) * width;
  const y = (pct: number) => height - (Math.min(Math.max(pct, 0), yMax) / yMax) * height;
  const at = (iso: string) => Date.parse(iso);

  const fc = series.forecast.filter((p) => Number.isFinite(at(p[0])));
  const median = fc.map((p) => ({ x: x(at(p[0])), y: y(p[1]) }));
  const band =
    fc.length > 1
      ? `${path(fc.map((p) => ({ x: x(at(p[0])), y: y(p[3]) })))} ${fc
          .slice()
          .reverse()
          .map((p) => `L${x(at(p[0])).toFixed(1)} ${y(p[2]).toFixed(1)}`)
          .join(" ")} Z`
      : "";

  let hit: ChartModel["hit"] = null;
  for (let i = 0; i < fc.length; i++) {
    if (fc[i][1] >= 100) {
      let ms = at(fc[i][0]);
      if (i > 0 && fc[i - 1][1] < 100) {
        const [t0, v0] = [at(fc[i - 1][0]), fc[i - 1][1]];
        ms = t0 + ((100 - v0) / (fc[i][1] - v0)) * (ms - t0);
      }
      hit = { x: x(ms), y: y(100), ms };
      break;
    }
  }

  const nights: ChartModel["nights"] = [];
  let open: number | null = null;
  const HOUR = 3_600_000;
  for (let t = Math.floor(s0 / HOUR) * HOUR; t <= s1; t += HOUR) {
    const h = hourIn(t, tz);
    const night = h >= 22 || h < 7;
    if (night && open == null) open = Math.max(t, s0);
    if (!night && open != null) {
      nights.push({ x: x(open), w: x(t) - x(open) });
      open = null;
    }
  }
  if (open != null) nights.push({ x: x(open), w: width - x(open) });

  const nowMs = f.now ? at(f.now) : NaN;
  return {
    width,
    height,
    yMax,
    y100: y(100),
    plan: path([
      { x: 0, y: y(0) },
      { x: width, y: y(100) },
    ]),
    actual: path(series.actual.filter((p) => Number.isFinite(at(p[0]))).map((p) => ({ x: x(at(p[0])), y: y(p[1]) }))),
    median: path(median),
    band,
    measured: series.measured.filter((p) => Number.isFinite(at(p[0]))).map((p) => ({ x: x(at(p[0])), y: y(p[1]) })),
    ghosts: series.ghosts.slice(0, 3).map((g) => path(g.points.map(([off, pct]) => ({ x: x(s0 + off * 60_000), y: y(pct) })))),
    nowX: Number.isFinite(nowMs) ? x(nowMs) : null,
    hit,
    nights,
  };
}
