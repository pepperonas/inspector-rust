/**
 * Pure helpers for the `tokens` panel — period windows + display formatting
 * for Claude Code usage pulled from the local Token Tracker.
 */

export type TokenPeriod = "today" | "7d" | "30d" | "all";

export const TOKEN_PERIODS: ReadonlyArray<{ id: TokenPeriod; label: string }> = [
  { id: "today", label: "Today" },
  { id: "7d", label: "7d" },
  { id: "30d", label: "30d" },
  { id: "all", label: "All" },
];

/** Local `YYYY-MM-DD` for a Date (machine TZ). */
export function formatYmd(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/**
 * Inclusive local-date window for a period chip, matching Token Tracker's
 * `getPeriodRange` (`7d`/`30d` = today − N calendar days).
 */
export function periodRange(
  period: TokenPeriod,
  today: Date = new Date(),
): { from: string | null; to: string } {
  const to = formatYmd(today);
  if (period === "today") return { from: to, to };
  if (period === "all") return { from: null, to };
  const daysBack = period === "7d" ? 7 : 30;
  const from = new Date(today.getFullYear(), today.getMonth(), today.getDate() - daysBack);
  return { from: formatYmd(from), to };
}

/** Token/cost fields shared by overview + list rows. */
export type TokenCostFields = {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_create_tokens: number;
  input_cost?: number;
  output_cost?: number;
  cache_read_cost?: number;
  cache_create_cost?: number;
  estimated_cost?: number;
  cost?: number;
  total_tokens?: number;
};

/** Display tokens — with or without cache (matches Tracker toggle). */
export function displayTokens(o: TokenCostFields, includeCache: boolean): number {
  if (includeCache) {
    return (
      o.input_tokens +
      o.output_tokens +
      o.cache_read_tokens +
      o.cache_create_tokens
    );
  }
  return o.input_tokens + o.output_tokens;
}

/** Display cost — with or without cache. */
export function displayCost(o: TokenCostFields, includeCache: boolean): number {
  if (o.input_cost !== undefined) {
    if (includeCache) {
      return (
        (o.input_cost ?? 0) +
        (o.output_cost ?? 0) +
        (o.cache_read_cost ?? 0) +
        (o.cache_create_cost ?? 0)
      );
    }
    return (o.input_cost ?? 0) + (o.output_cost ?? 0);
  }
  // List rows only carry a single `cost` (already includes cache).
  return o.cost ?? o.estimated_cost ?? 0;
}

/** Compact token count: `1.2B` / `340.5M` / `12.3k` / `420`. */
export function formatTokens(n: number): string {
  const v = Math.max(0, Math.round(n));
  if (v >= 1_000_000_000) return `${(v / 1_000_000_000).toFixed(1).replace(/\.0$/, "")}B`;
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
  if (v >= 10_000) return `${Math.round(v / 1000)}k`;
  if (v >= 1000) return `${(v / 1000).toFixed(1).replace(/\.0$/, "")}k`;
  return String(v);
}

/** USD cost with sensible precision. */
export function formatCost(n: number): string {
  const v = Math.max(0, n);
  if (v >= 100) return `$${v.toFixed(0)}`;
  if (v >= 10) return `$${v.toFixed(1)}`;
  return `$${v.toFixed(2)}`;
}

/** Active minutes → `3h 45m` / `42m`. */
export function formatActiveMin(min: number): string {
  const m = Math.max(0, Math.round(min));
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  const rest = m % 60;
  return rest === 0 ? `${h}h` : `${h}h ${rest}m`;
}

/** Short project path: keep the last 2 segments. */
export function shortProject(name: string): string {
  const parts = name.split(/[/\\]/).filter(Boolean);
  if (parts.length <= 2) return name;
  return parts.slice(-2).join("/");
}
