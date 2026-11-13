import { describe, expect, it } from "vitest";
import { parseSafeZone, SAFE_ZONE_PLATFORMS, SAFE_ZONES, safeRect, safeZoneFor } from "./safe-zones";

describe("safe zones", () => {
  it("every platform leaves most of the frame free", () => {
    for (const p of SAFE_ZONE_PLATFORMS) {
      const z = SAFE_ZONES[p];
      for (const v of [z.top, z.bottom, z.left, z.right]) expect(v > 0 && v < 0.5).toBe(true);
      expect(1 - z.top - z.bottom).toBeGreaterThan(0.4);
      expect(1 - z.left - z.right).toBeGreaterThan(0.6);
      // The caption area at the bottom is larger than the account row at the top.
      expect(z.bottom).toBeGreaterThan(z.top);
    }
  });

  it("only upright video with a chosen platform gets the overlay", () => {
    expect(safeZoneFor("9:16", "tiktok")).toBe(SAFE_ZONES.tiktok);
    expect(safeZoneFor("9:16", null)).toBeNull();
    expect(safeZoneFor("16:9", "tiktok")).toBeNull();
    expect(safeZoneFor("auto", "reels")).toBeNull();
  });

  it("the stored choice is off unless it names a platform", () => {
    expect(parseSafeZone("reels")).toBe("reels");
    expect(parseSafeZone("youtube")).toBeNull();
    expect(parseSafeZone(null)).toBeNull();
    expect(parseSafeZone("")).toBeNull();
  });

  it("safeRect is the free rectangle in percent", () => {
    expect(safeRect({ top: 0.1, bottom: 0.25, left: 0.05, right: 0.15 })).toEqual({ left: "5%", top: "10%", width: "80%", height: "65%" });
  });
});
