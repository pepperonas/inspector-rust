import { describe, it, expect } from "vitest";
import {
  STAT_TWEEN_MS,
  HEAT_START,
  HEAT_FULL,
  easeOutCubic,
  tweenAt,
  heatLevel,
  isHot,
  bytesFormatterFor,
  rateFormatterFor,
} from "./stats-anim";
import { humanBytes } from "./format-stats";

describe("stat tween math", () => {
  it("the tween is shorter than the poll interval (the value must come to rest)", () => {
    // POLL_MS in StatsPanel is 1500 — pinned here so a future tween-duration
    // bump can't silently outlast the poll and leave numbers never settling.
    expect(STAT_TWEEN_MS).toBeLessThan(1500);
  });

  it("easeOutCubic starts at 0, ends at 1, decelerates, clamps", () => {
    expect(easeOutCubic(0)).toBe(0);
    expect(easeOutCubic(1)).toBe(1);
    // Decelerate: the first half covers most of the distance.
    expect(easeOutCubic(0.5)).toBeGreaterThan(0.8);
    expect(easeOutCubic(-1)).toBe(0);
    expect(easeOutCubic(2)).toBe(1);
  });

  it("tweenAt interpolates between the endpoints, in both directions", () => {
    expect(tweenAt(0, 100, 0)).toBe(0);
    expect(tweenAt(0, 100, 1)).toBe(100);
    expect(tweenAt(100, 0, 1)).toBe(0);
    const mid = tweenAt(20, 80, 0.5);
    expect(mid).toBeGreaterThan(20);
    expect(mid).toBeLessThan(80);
  });
});

describe("heatLevel", () => {
  it("is cold below the amber threshold and ramps across the amber band", () => {
    // Thresholds derive from the panel's loadColor scale (≥70 amber, ≥90 red).
    expect(HEAT_START).toBe(70);
    expect(HEAT_FULL).toBe(90);
    expect(heatLevel(0)).toBe(0);
    expect(heatLevel(69.9)).toBe(0);
    expect(heatLevel(70)).toBe(0);
    expect(heatLevel(80)).toBeCloseTo(0.5);
    expect(heatLevel(90)).toBe(1);
    expect(heatLevel(100)).toBe(1);
  });

  it("isHot means the red zone — exactly where loadColor turns red", () => {
    expect(isHot(89.9)).toBe(false);
    expect(isHot(90)).toBe(true);
    expect(isHot(100)).toBe(true);
  });

  it("tolerates garbage without arming the glow", () => {
    expect(heatLevel(NaN)).toBe(0);
    expect(heatLevel(Infinity)).toBe(0);
    expect(isHot(NaN)).toBe(false);
  });
});

describe("unit-stable byte formatting for tweens", () => {
  it("locks unit AND decimals to the target for the whole run", () => {
    // Target 1.4 GB → every interpolated value renders in GB with 1 decimal,
    // so the string width never jumps mid-tween (the raspi-monitor lesson).
    const gb = 1.4 * 1024 ** 3;
    const f = bytesFormatterFor(gb);
    expect(f(gb)).toBe(humanBytes(gb)); // agrees with the panel's formatter at rest
    expect(f(900 * 1024 ** 2)).toBe("0.9 GB"); // NOT "900 MB"
    expect(f(gb / 2)).toBe("0.7 GB");
  });

  it("matches humanBytes at the target across the unit ladder", () => {
    for (const v of [0.5, 512, 8 * 1024, 512 * 1024 ** 2, 1.4 * 1024 ** 3, 42 * 1024 ** 3]) {
      expect(bytesFormatterFor(v)(v)).toBe(humanBytes(v));
    }
  });

  it("clamps negatives and survives a zero/garbage target", () => {
    // Decimals stay locked to the target (1.0 KB → one decimal), so the clamp
    // renders "0.0 KB" — width-stable, not "0 KB".
    expect(bytesFormatterFor(1024)(-5)).toBe("0.0 KB");
    expect(bytesFormatterFor(0)(0)).toBe("0 B");
    expect(bytesFormatterFor(NaN)(512)).toBe("512 B");
  });

  it("rate variant appends /s like humanRate", () => {
    const f = rateFormatterFor(2 * 1024 ** 2);
    expect(f(2 * 1024 ** 2)).toBe("2.0 MB/s");
    expect(f(1024 ** 2 / 2)).toBe("0.5 MB/s"); // unit stays MB/s mid-tween
  });
});
