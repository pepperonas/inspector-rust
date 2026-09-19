export type LightBand = {
  label: string;
  description: string;
  color: string;
};

/** Logarithmic 0–100 meter: useful from darkness through direct daylight. */
export function luxLevel(lux: number): number {
  if (!Number.isFinite(lux) || lux <= 0) return 0;
  return Math.min(100, (Math.log10(1 + lux) / Math.log10(100_001)) * 100);
}

export function lightBand(lux: number): LightBand {
  if (lux < 10) {
    return { label: "Dark", description: "Very little ambient light", color: "#6366f1" };
  }
  if (lux < 100) {
    return { label: "Dim", description: "Soft indoor light", color: "#8b5cf6" };
  }
  if (lux < 500) {
    return { label: "Indoor", description: "Typical room or office light", color: "#f59e0b" };
  }
  if (lux < 10_000) {
    return { label: "Bright", description: "Bright room or shaded daylight", color: "#f97316" };
  }
  return { label: "Daylight", description: "Strong outdoor light", color: "#eab308" };
}

export function formatLux(lux: number): string {
  if (lux < 100) return lux.toFixed(1);
  return Math.round(lux).toLocaleString("en-US");
}
