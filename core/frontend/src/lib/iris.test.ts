import { describe, it, expect } from "vitest";
import {
  makeBlobs,
  irisAction,
  makeShardPath,
  burstIntensity,
  burstGapMs,
  makeBurst,
  BURST_GAP_CALM,
  BURST_GAP_LOUD,
  BURST_TINTS,
  BURST_MAX,
  BURST_SPAN_DB,
  METER_MIN_SPL,
  METER_MAX_SPL,
  parseIrisArg,
  clampThreshold,
  meterFraction,
  formatSpl,
  irisRowLabel,
  MIN_THRESHOLD_SPL,
  MAX_THRESHOLD_SPL,
  DEFAULT_THRESHOLD_SPL,
} from "./iris";

describe("parseIrisArg", () => {
  it("treats a bare `iris` as a toggle", () => {
    expect(parseIrisArg("")).toEqual({ kind: "toggle" });
    expect(parseIrisArg("   ")).toEqual({ kind: "toggle" });
  });

  it("treats 0 as the off switch", () => {
    expect(parseIrisArg("0")).toEqual({ kind: "off" });
    expect(parseIrisArg(" 0 ")).toEqual({ kind: "off" });
    // A negative SPL threshold is meaningless — same intent as 0.
    expect(parseIrisArg("-5")).toEqual({ kind: "off" });
  });

  it("arms at a positive threshold", () => {
    expect(parseIrisArg("55")).toEqual({ kind: "on", threshold: 55 });
    expect(parseIrisArg("72.5")).toEqual({ kind: "on", threshold: 72.5 });
  });

  it("accepts a comma decimal separator", () => {
    expect(parseIrisArg("57,5")).toEqual({ kind: "on", threshold: 57.5 });
  });

  it("clamps an out-of-band threshold instead of rejecting it", () => {
    expect(parseIrisArg("5")).toEqual({ kind: "on", threshold: MIN_THRESHOLD_SPL });
    expect(parseIrisArg("500")).toEqual({ kind: "on", threshold: MAX_THRESHOLD_SPL });
  });

  it("falls back to toggle on garbage rather than arming at a wrong level", () => {
    expect(parseIrisArg("abc")).toEqual({ kind: "toggle" });
    expect(parseIrisArg("5x5")).toEqual({ kind: "toggle" });
  });
});

describe("clampThreshold", () => {
  it("passes an in-band value through", () => {
    expect(clampThreshold(55)).toBe(55);
    expect(clampThreshold(MIN_THRESHOLD_SPL)).toBe(MIN_THRESHOLD_SPL);
    expect(clampThreshold(MAX_THRESHOLD_SPL)).toBe(MAX_THRESHOLD_SPL);
  });

  it("clamps both ends and survives NaN", () => {
    expect(clampThreshold(-100)).toBe(MIN_THRESHOLD_SPL);
    expect(clampThreshold(9999)).toBe(MAX_THRESHOLD_SPL);
    expect(clampThreshold(NaN)).toBe(DEFAULT_THRESHOLD_SPL);
  });
});

describe("meterFraction", () => {
  it("maps the display band onto 0..1", () => {
    expect(meterFraction(0)).toBe(0);
    expect(meterFraction(100)).toBe(1);
    expect(meterFraction(50)).toBeCloseTo(0.5, 5);
  });

  it("clamps a silent room (negative SPL) to the floor", () => {
    // -120 dBFS + 90 = -30 SPL — must read as empty, not as a negative bar.
    expect(meterFraction(-30)).toBe(0);
    expect(meterFraction(200)).toBe(1);
    expect(meterFraction(NaN)).toBe(0);
  });
});

describe("formatSpl", () => {
  it("shows one decimal, matching the raspi5 rounding", () => {
    expect(formatSpl(55)).toBe("55.0");
    expect(formatSpl(72.44)).toBe("72.4");
  });

  it("degrades to a dash instead of NaN", () => {
    expect(formatSpl(NaN)).toBe("—");
  });
});

describe("irisRowLabel", () => {
  it("says which way the toggle will flip", () => {
    expect(irisRowLabel({ kind: "toggle" }, false)).toMatch(/scharfschalten/i);
    expect(irisRowLabel({ kind: "toggle" }, true)).toMatch(/ausschalten/i);
  });

  it("distinguishes arming from retuning a live session", () => {
    expect(irisRowLabel({ kind: "on", threshold: 60 }, false)).toMatch(/scharfschalten/i);
    expect(irisRowLabel({ kind: "on", threshold: 60 }, true)).toMatch(/setzen/i);
    expect(irisRowLabel({ kind: "on", threshold: 60 }, true)).toContain("60.0");
  });

  it("does not promise an action when already off", () => {
    expect(irisRowLabel({ kind: "off" }, false)).toMatch(/bereits aus/i);
    expect(irisRowLabel({ kind: "off" }, true)).toMatch(/ausschalten/i);
  });
});

describe("makeBlobs", () => {
  // A deterministic stand-in for Math.random so the geometry is testable.
  const seq = (values: number[]) => {
    let i = 0;
    return () => values[i++ % values.length];
  };

  it("produces one blob per anchor", () => {
    expect(makeBlobs(seq([0.5])).length).toBe(8);
  });

  it("keeps every blob anchored near an edge, never over the middle", () => {
    // The centre of the screen must stay clear — that is what keeps the
    // machine usable while the vignette is lit.
    for (const b of makeBlobs(Math.random)) {
      const nearEdge =
        b.x <= 25 || b.x >= 75 || b.y <= 25 || b.y >= 75;
      expect(nearEdge, `blob at ${b.x},${b.y} drifted into the centre`).toBe(true);
    }
  });

  it("pushes each blob outward, away from the centre, for the dark state", () => {
    for (const b of makeBlobs(Math.random)) {
      if (b.x < 50) expect(b.ox).toBeLessThan(0);
      if (b.x > 50) expect(b.ox).toBeGreaterThan(0);
      if (b.y < 50) expect(b.oy).toBeLessThan(0);
      if (b.y > 50) expect(b.oy).toBeGreaterThan(0);
    }
  });

  it("gives every blob its own drift period so the motion never visibly loops", () => {
    const durs = makeBlobs(seq([0.5])).map((b) => b.dur);
    expect(new Set(durs).size).toBe(durs.length);
    for (const d of durs) expect(d).toBeGreaterThan(0);
  });

  it("varies the arrangement between calls — each monitor looks different", () => {
    const a = makeBlobs(Math.random);
    const b = makeBlobs(Math.random);
    const same = a.every((x, i) => x.x === b[i].x && x.y === b[i].y);
    expect(same).toBe(false);
  });

  it("produces finite, usable numbers throughout", () => {
    for (const b of makeBlobs(Math.random)) {
      for (const [k, v] of Object.entries(b)) {
        if (typeof v === "number") {
          expect(Number.isFinite(v), `${k} was ${v}`).toBe(true);
        }
      }
      expect(b.w).toBeGreaterThan(0);
      expect(b.h).toBeGreaterThan(0);
      expect(b.blur).toBeGreaterThan(0);
    }
  });
});

describe("burstIntensity", () => {
  it("is 0 at or below the threshold", () => {
    expect(burstIntensity(55, 55)).toBe(0);
    expect(burstIntensity(40, 55)).toBe(0);
  });

  it("saturates at 1 once far over", () => {
    expect(burstIntensity(80, 55)).toBe(1);
    expect(burstIntensity(200, 55)).toBe(1);
  });

  it("rises linearly in between", () => {
    expect(burstIntensity(55 + 12.5, 55)).toBeCloseTo(0.5, 5);
  });

  it("survives nonsense input instead of poisoning the cadence", () => {
    expect(burstIntensity(NaN, 55)).toBe(0);
    expect(burstIntensity(60, NaN)).toBe(0);
    expect(burstIntensity(60, 55, 0)).toBe(0);
  });
});

describe("burstGapMs", () => {
  it("uses the reference cadence when barely over the threshold", () => {
    expect(burstGapMs(0, () => 0)).toBeCloseTo(BURST_GAP_CALM[0], 5);
    expect(burstGapMs(0, () => 1)).toBeCloseTo(BURST_GAP_CALM[1], 5);
  });

  it("tightens to the loud cadence when far over", () => {
    expect(burstGapMs(1, () => 0)).toBeCloseTo(BURST_GAP_LOUD[0], 5);
    expect(burstGapMs(1, () => 1)).toBeCloseTo(BURST_GAP_LOUD[1], 5);
  });

  it("fires more often the louder it gets", () => {
    const mid = () => 0.5;
    expect(burstGapMs(1, mid)).toBeLessThan(burstGapMs(0.5, mid));
    expect(burstGapMs(0.5, mid)).toBeLessThan(burstGapMs(0, mid));
  });

  it("stays irregular at every volume — never a fixed beat", () => {
    // The reference is explicit that a constant gap "reads mechanical".
    for (const i of [0, 0.5, 1]) {
      expect(burstGapMs(i, () => 0)).toBeLessThan(burstGapMs(i, () => 1));
    }
  });

  it("clamps an out-of-range intensity", () => {
    expect(burstGapMs(-3, () => 0)).toBeCloseTo(BURST_GAP_CALM[0], 5);
    expect(burstGapMs(9, () => 0)).toBeCloseTo(BURST_GAP_LOUD[0], 5);
  });
});

describe("makeBurst", () => {
  it("keeps every burst on screen", () => {
    for (let i = 0; i < 200; i++) {
      const b = makeBurst(i, Math.random(), Math.random);
      expect(b.x).toBeGreaterThanOrEqual(0);
      expect(b.x).toBeLessThanOrEqual(100);
      expect(b.y).toBeGreaterThanOrEqual(0);
      expect(b.y).toBeLessThanOrEqual(100);
    }
  });

  it("never exceeds a translucent peak — it must not black out the screen", () => {
    for (let i = 0; i < 200; i++) {
      const b = makeBurst(i, 1, Math.random);
      expect(b.peak).toBeLessThanOrEqual(0.78);
      expect(b.peak).toBeGreaterThan(0);
    }
  });

  it("hits harder and shorter when loud", () => {
    const half = () => 0.5;
    const calm = makeBurst(1, 0, half);
    const loud = makeBurst(2, 1, half);
    expect(loud.peak).toBeGreaterThan(calm.peak);
    expect(loud.size).toBeGreaterThan(calm.size);
    expect(loud.life).toBeLessThan(calm.life);
  });

  it("only ever picks a real tint from the palette", () => {
    // `rand()` returning exactly 1 would index past the end without the clamp.
    expect(BURST_TINTS).toContain(makeBurst(1, 0.5, () => 1).tint);
    expect(BURST_TINTS).toContain(makeBurst(2, 0.5, () => 0).tint);
    for (let i = 0; i < 100; i++) {
      expect(BURST_TINTS).toContain(makeBurst(i, 0.5, Math.random).tint);
    }
  });

  it("produces a usable, finite animation in every field", () => {
    // Bounds updated with the sharpened look (v0.102.2): rotation is now the
    // full circle rather than the old ±20° (a shard reads differently at every
    // angle, an ellipse barely did), and the shortest streak at full intensity
    // is ~0.30 s — a flash, by design.
    for (let i = 0; i < 100; i++) {
      const b = makeBurst(i, Math.random(), Math.random);
      expect(b.life).toBeGreaterThan(0.25);
      expect(b.size).toBeGreaterThan(0);
      expect(b.rot).toBeGreaterThanOrEqual(0);
      expect(b.rot).toBeLessThan(360);
    }
  });
});

describe("cross-language contract", () => {
  // These four numbers are mirrored in `iris.rs`, which asserts the same
  // values. If one side moves without the other, the popup and the backend
  // would disagree about what a threshold means.
  it("matches the band and offset the Rust side pins", () => {
    expect(MIN_THRESHOLD_SPL).toBe(30);
    expect(MAX_THRESHOLD_SPL).toBe(100);
    expect(DEFAULT_THRESHOLD_SPL).toBe(55);
  });

  it("keeps the meter wide enough to show the whole threshold band", () => {
    // A threshold you cannot see on the meter cannot be calibrated.
    expect(METER_MIN_SPL).toBeLessThanOrEqual(MIN_THRESHOLD_SPL);
    expect(METER_MAX_SPL).toBeGreaterThanOrEqual(MAX_THRESHOLD_SPL);
  });

  it("keeps the concurrency cap small enough to stay readable", () => {
    // The reference caps at 2; more than a handful on screen at once stops
    // reading as discrete impulses.
    expect(BURST_MAX).toBeGreaterThan(0);
    expect(BURST_MAX).toBeLessThanOrEqual(5);
  });
});

describe("parseIrisArg — further edge cases", () => {
  it("accepts a redundant plus sign and padded zeros", () => {
    expect(parseIrisArg("+60")).toEqual({ kind: "on", threshold: 60 });
    expect(parseIrisArg("060")).toEqual({ kind: "on", threshold: 60 });
  });

  it("treats every spelling of zero as off", () => {
    for (const z of ["0", "0.0", "00", "0,0", "+0", "-0"]) {
      expect(parseIrisArg(z), `${z} should be off`).toEqual({ kind: "off" });
    }
  });

  it("does not mistake a lone separator for a number", () => {
    expect(parseIrisArg(".")).toEqual({ kind: "toggle" });
    expect(parseIrisArg(",")).toEqual({ kind: "toggle" });
    expect(parseIrisArg("-")).toEqual({ kind: "toggle" });
  });

  it("rejects infinity rather than arming at an impossible level", () => {
    expect(parseIrisArg("Infinity")).toEqual({ kind: "toggle" });
    expect(parseIrisArg("1e999")).toEqual({ kind: "toggle" });
  });
});

describe("meterFraction — monotonicity", () => {
  it("never decreases as the level rises", () => {
    let prev = -1;
    for (let spl = -40; spl <= 140; spl += 2) {
      const f = meterFraction(spl);
      expect(f).toBeGreaterThanOrEqual(prev);
      expect(f).toBeGreaterThanOrEqual(0);
      expect(f).toBeLessThanOrEqual(1);
      prev = f;
    }
  });
});

describe("makeBlobs — determinism", () => {
  it("is a pure function of its rand source", () => {
    const seeded = () => {
      let i = 0;
      const vals = [0.1, 0.9, 0.35, 0.72, 0.5, 0.05, 0.66, 0.21];
      return () => vals[i++ % vals.length];
    };
    expect(makeBlobs(seeded())).toEqual(makeBlobs(seeded()));
  });

  it("keeps the drift and the outward push bounded", () => {
    for (const b of makeBlobs(Math.random)) {
      // A runaway drift would walk a blob across the screen over a long
      // session; a runaway push would park it entirely off-screen when dark.
      expect(Math.abs(b.dx)).toBeLessThanOrEqual(9);
      expect(Math.abs(b.dy)).toBeLessThanOrEqual(9);
      expect(Math.abs(b.ox)).toBeLessThanOrEqual(22);
      expect(Math.abs(b.oy)).toBeLessThanOrEqual(22);
      expect(b.ds).toBeGreaterThan(0.5);
      expect(b.ds).toBeLessThan(1.6);
    }
  });
});

describe("burstIntensity — monotonicity", () => {
  it("never decreases as the room gets louder", () => {
    let prev = -1;
    for (let spl = 40; spl <= 100; spl += 1) {
      const i = burstIntensity(spl, 55);
      expect(i).toBeGreaterThanOrEqual(prev);
      prev = i;
    }
  });

  it("reaches full intensity exactly one span above the threshold", () => {
    expect(burstIntensity(55 + BURST_SPAN_DB, 55)).toBe(1);
    expect(burstIntensity(55 + BURST_SPAN_DB - 0.1, 55)).toBeLessThan(1);
  });

  it("follows the threshold, not an absolute level", () => {
    // Same 10 dB of headroom over two different thresholds → same intensity.
    expect(burstIntensity(45, 35)).toBeCloseTo(burstIntensity(75, 65), 6);
  });
});

describe("burstGapMs — safety across the whole range", () => {
  it("is always a positive, finite delay", () => {
    for (let i = 0; i <= 1.0001; i += 0.05) {
      for (const r of [0, 0.25, 0.5, 0.75, 1]) {
        const gap = burstGapMs(i, () => r);
        expect(Number.isFinite(gap)).toBe(true);
        expect(gap).toBeGreaterThan(0);
      }
    }
  });

  it("never schedules faster than the loud floor or slower than the calm ceiling", () => {
    for (let i = 0; i <= 1.0001; i += 0.05) {
      const fastest = burstGapMs(i, () => 0);
      const slowest = burstGapMs(i, () => 1);
      expect(fastest).toBeGreaterThanOrEqual(BURST_GAP_LOUD[0] - 1e-6);
      expect(slowest).toBeLessThanOrEqual(BURST_GAP_CALM[1] + 1e-6);
      expect(fastest).toBeLessThan(slowest);
    }
  });
});

describe("makeBurst — further invariants", () => {
  it("preserves the id it was given", () => {
    expect(makeBurst(42, 0.5, Math.random).id).toBe(42);
  });

  it("stays translucent even at the calmest setting", () => {
    for (let i = 0; i < 100; i++) {
      const b = makeBurst(i, 0, Math.random);
      expect(b.peak).toBeGreaterThan(0.2);
      expect(b.peak).toBeLessThan(0.5);
    }
  });

  it("never produces a burst that outlives its reaping margin", () => {
    // The component reaps at life + 600 ms; a life longer than a couple of
    // seconds would keep the concurrency cap occupied far too long.
    for (let i = 0; i < 200; i++) {
      expect(makeBurst(i, Math.random(), Math.random).life).toBeLessThan(2.2);
    }
  });

  it("varies between calls so no two impulses look alike", () => {
    const a = makeBurst(1, 0.5, Math.random);
    const b = makeBurst(2, 0.5, Math.random);
    expect(a.x === b.x && a.y === b.y && a.size === b.size).toBe(false);
  });
});

describe("irisAction — the toggle contract", () => {
  // The exact sequence the user reported broken in v0.102.1:
  // `iris 55` armed it, then a bare `iris` left it running.
  it("a bare iris after arming with a number disarms", () => {
    expect(irisAction(parseIrisArg("55"), false)).toEqual({ kind: "arm", threshold: 55 });
    expect(irisAction(parseIrisArg(""), true)).toEqual({ kind: "disarm" });
  });

  it("a bare iris arms when nothing is running", () => {
    expect(irisAction(parseIrisArg(""), false)).toEqual({ kind: "arm" });
  });

  it("iris 0 disarms a running session and is a no-op otherwise", () => {
    expect(irisAction(parseIrisArg("0"), true)).toEqual({ kind: "disarm" });
    expect(irisAction(parseIrisArg("0"), false)).toEqual({ kind: "none" });
  });

  it("an explicit number never disarms — it retunes", () => {
    // Otherwise `iris 55` would be unusable as "set the threshold to 55"
    // whenever a session happened to be running.
    expect(irisAction(parseIrisArg("70"), true)).toEqual({ kind: "retune", threshold: 70 });
  });

  it("toggling twice returns to the starting state", () => {
    let active = false;
    for (const expected of ["arm", "disarm", "arm", "disarm"]) {
      const a = irisAction(parseIrisArg(""), active);
      expect(a.kind).toBe(expected);
      active = a.kind === "arm";
    }
  });

  it("garbage falls back to the toggle rather than arming at a wrong level", () => {
    expect(irisAction(parseIrisArg("zzz"), true)).toEqual({ kind: "disarm" });
    expect(irisAction(parseIrisArg("zzz"), false)).toEqual({ kind: "arm" });
  });

  it("clamps a retune the same way arming does", () => {
    expect(irisAction(parseIrisArg("999"), true)).toEqual({
      kind: "retune",
      threshold: MAX_THRESHOLD_SPL,
    });
  });
});

describe("makeShardPath", () => {
  it("emits a valid CSS polygon", () => {
    const p = makeShardPath(Math.random, 7);
    expect(p.startsWith("polygon(")).toBe(true);
    expect(p.endsWith(")")).toBe(true);
    expect(p.split(",").length).toBe(7);
  });

  it("keeps every vertex inside the element box", () => {
    // A vertex outside 0–100 % would clip the shard flat against one side and
    // lose the spiky silhouette entirely.
    for (let i = 0; i < 100; i++) {
      const nums = makeShardPath(Math.random, 7)
        .replace("polygon(", "")
        .replace(")", "")
        .split(/[,\s]+/)
        .filter(Boolean)
        .map((t) => parseFloat(t));
      expect(nums.length).toBe(14);
      for (const n of nums) {
        expect(Number.isFinite(n)).toBe(true);
        expect(n).toBeGreaterThanOrEqual(0);
        expect(n).toBeLessThanOrEqual(100);
      }
    }
  });

  it("honours the requested vertex count", () => {
    expect(makeShardPath(() => 0.5, 5).split(",").length).toBe(5);
    expect(makeShardPath(() => 0.5, 9).split(",").length).toBe(9);
  });

  it("alternates reach, so the outline spikes instead of bulging", () => {
    // Even vertices reach out (~0.46–0.52 of the box), odd ones stay in
    // (~0.18–0.34). Without that contrast it is a blob again.
    const pts = makeShardPath(() => 0.5, 8)
      .replace("polygon(", "").replace(")", "")
      .split(",")
      .map((p) => p.trim().split(/\s+/).map(parseFloat));
    const radius = ([x, y]: number[]) => Math.hypot(x - 50, y - 50);
    const evens = pts.filter((_, i) => i % 2 === 0).map(radius);
    const odds = pts.filter((_, i) => i % 2 === 1).map(radius);
    expect(Math.min(...evens)).toBeGreaterThan(Math.max(...odds));
  });

  it("gives every impulse its own silhouette", () => {
    expect(makeShardPath(Math.random)).not.toBe(makeShardPath(Math.random));
  });
});

describe("makeBurst — sharpened impulses", () => {
  it("keeps the blur small enough for the edges to survive", () => {
    // Heavy blur is exactly what made v0.102.1 read as drifting bubbles.
    for (let i = 0; i < 200; i++) {
      expect(makeBurst(i, Math.random(), Math.random).blur).toBeLessThanOrEqual(6);
    }
  });

  it("carries a clip path and a usable aspect ratio", () => {
    for (let i = 0; i < 100; i++) {
      const b = makeBurst(i, Math.random(), Math.random);
      expect(b.clip.startsWith("polygon(")).toBe(true);
      expect(b.aspect).toBeGreaterThan(0);
      expect(b.aspect).toBeLessThanOrEqual(1);
    }
  });

  it("mixes thin streaks with broader shards", () => {
    const kinds = new Set(
      Array.from({ length: 200 }, (_, i) => makeBurst(i, 0.5, Math.random).streak),
    );
    expect(kinds.has(true)).toBe(true);
    expect(kinds.has(false)).toBe(true);
  });

  it("makes streaks thinner and shorter-lived than shards", () => {
    const streaks: number[] = [];
    const shards: number[] = [];
    for (let i = 0; i < 400; i++) {
      const b = makeBurst(i, 0, Math.random);
      (b.streak ? streaks : shards).push(b.life);
      if (b.streak) expect(b.aspect).toBeLessThan(0.25);
      else expect(b.aspect).toBeGreaterThan(0.5);
    }
    const avg = (a: number[]) => a.reduce((x, y) => x + y, 0) / a.length;
    expect(avg(streaks)).toBeLessThan(avg(shards));
  });

  it("uses the full rotation circle, not a narrow wedge", () => {
    const rots = Array.from({ length: 300 }, (_, i) => makeBurst(i, 0.5, Math.random).rot);
    expect(Math.min(...rots)).toBeLessThan(30);
    expect(Math.max(...rots)).toBeGreaterThan(330);
  });
});
