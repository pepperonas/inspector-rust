// Pure display helpers for the `limits` / `quota` panel (Claude subscription
// usage limits). The data comes from `claude_limits.rs`.

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
