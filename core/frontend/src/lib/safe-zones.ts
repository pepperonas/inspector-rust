// Where a short-video platform's own interface covers a 9:16 video: caption and buttons at
// the bottom, account row at the top, the action column on the right. Fractions of the
// frame. Approximate guides from the platforms' ad-safe-zone guidance (2026-11) — the apps
// move these with every redesign, so the overlay is a hint, never a rule.
import type { RecordPlatform } from "./ipc";

export type SafeZonePlatform = Exclude<RecordPlatform, "youtube">;
export interface SafeZone { top: number; bottom: number; left: number; right: number }

export const SAFE_ZONE_PLATFORMS: readonly SafeZonePlatform[] = ["tiktok", "reels", "shorts"];

export const SAFE_ZONES: Record<SafeZonePlatform, SafeZone> = {
  tiktok: { top: 0.13, bottom: 0.25, left: 0.06, right: 0.14 },
  reels: { top: 0.14, bottom: 0.35, left: 0.06, right: 0.13 },
  shorts: { top: 0.12, bottom: 0.3, left: 0.05, right: 0.15 },
};

export const SAFE_ZONE_KEY = "ir.record.safezone.v1";

/** The stored choice (`null` = off; anything unknown is off). */
export function parseSafeZone(raw: string | null): SafeZonePlatform | null {
  return SAFE_ZONE_PLATFORMS.includes(raw as SafeZonePlatform) ? (raw as SafeZonePlatform) : null;
}

/** The overlay belongs to upright video only. */
export function safeZoneFor(aspect: string, choice: SafeZonePlatform | null): SafeZone | null {
  return aspect === "9:16" && choice ? SAFE_ZONES[choice] : null;
}

/** CSS percentages of the free rectangle inside the frame. */
export function safeRect(z: SafeZone): { left: string; top: string; width: string; height: string } {
  const pct = (v: number) => `${Math.round(v * 1000) / 10}%`;
  return { left: pct(z.left), top: pct(z.top), width: pct(1 - z.left - z.right), height: pct(1 - z.top - z.bottom) };
}
