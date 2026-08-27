/**
 * Pure display rules for the `pagespeed` panel (v0.142.0).
 *
 * ⚠️ These mirror `pagespeed::band` / `pagespeed_export::band_color` in Rust.
 * The panel and the exported document must not disagree about what "good"
 * looks like, so the thresholds and the three colours are pinned on BOTH
 * sides — Lighthouse's own bands: 90–100 good, 50–89 average, 0–49 poor.
 */
export type Band = "good" | "average" | "poor" | "unknown";

export function scoreBand(score: number | null): Band {
  if (score === null || Number.isNaN(score)) return "unknown";
  if (score >= 90) return "good";
  if (score >= 50) return "average";
  return "poor";
}

export function bandColor(b: Band): string {
  switch (b) {
    case "good":
      return "#0cce6b";
    case "average":
      return "#ffa400";
    case "poor":
      return "#ff4e42";
    default:
      return "#9aa1ab";
  }
}
