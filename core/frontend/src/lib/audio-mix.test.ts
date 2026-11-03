import { describe, expect, it } from "vitest";
import type { RecordTrackSpan } from "./ipc";
import { dbToGain, entryAt, fadeEnvelope, fileMsAt, gainAt, limiterCurve, LIMITER_CEILING_DB } from "./audio-mix";

const span = (over: Partial<RecordTrackSpan> = {}): RecordTrackSpan => ({
  index: 0,
  repeat: 0,
  progStartMs: 1_000,
  progEndMs: 11_000,
  fileStartMs: 0,
  fileEndMs: 10_000,
  fadeInMs: 1_000,
  fadeOutMs: 2_000,
  gainDb: 0,
  kind: "music",
  muted: false,
  ...over,
});

describe("gainAt", () => {
  it("ramps in over the fade-in and out over the fade-out, linearly", () => {
    const s = span();
    expect(gainAt(s, 1_000)).toBe(0);
    expect(gainAt(s, 1_500)).toBeCloseTo(0.5, 6);
    expect(gainAt(s, 2_000)).toBe(1);
    expect(gainAt(s, 6_000)).toBe(1);
    expect(gainAt(s, 10_000)).toBeCloseTo(0.5, 6);
    expect(gainAt(s, 10_999)).toBeCloseTo(0.0005, 4);
  });

  it("is 0 outside its entry and when muted", () => {
    expect(gainAt(span(), 999)).toBe(0);
    expect(gainAt(span(), 11_000)).toBe(0);
    expect(gainAt(span({ muted: true }), 6_000)).toBe(0);
  });

  it("applies the level, also above 0 dB", () => {
    expect(gainAt(span({ gainDb: -18 }), 6_000)).toBeCloseTo(10 ** (-18 / 20), 9);
    expect(gainAt(span({ gainDb: 12 }), 6_000)).toBeCloseTo(10 ** (12 / 20), 9);
    expect(gainAt(span({ gainDb: 12 }), 6_000)).toBeGreaterThan(1);
  });

  it("loop edges: fades only on the outer entries, full level across a repeat boundary", () => {
    // Rust puts the fade-in on the first repeat and the fade-out on the last.
    const a = span({ repeat: 0, progStartMs: 0, progEndMs: 4_000, fadeInMs: 1_000, fadeOutMs: 0 });
    const b = span({ repeat: 1, progStartMs: 4_000, progEndMs: 8_000, fadeInMs: 0, fadeOutMs: 1_000 });
    expect(gainAt(a, 3_999)).toBe(1);
    expect(gainAt(b, 4_000)).toBe(1);
    expect(gainAt(a, 4_000)).toBe(0);
    expect(entryAt([a, b], 0, 4_000)).toBe(b);
    expect(entryAt([a, b], 0, 3_999)).toBe(a);
    expect(entryAt([a, b], 0, 8_000)).toBeNull();
    expect(entryAt([a, b], 1, 2_000)).toBeNull();
    expect(gainAt(b, 7_500)).toBeCloseTo(0.5, 6);
  });

  it("fades longer than the entry share it in proportion", () => {
    // 4 s entry, 4 s in + 4 s out → 2 s each, the peak in the middle.
    expect(fadeEnvelope(2_000, 4_000, 4_000, 4_000)).toBeCloseTo(1, 6);
    expect(fadeEnvelope(1_000, 4_000, 4_000, 4_000)).toBeCloseTo(0.5, 6);
    expect(fadeEnvelope(3_000, 4_000, 4_000, 4_000)).toBeCloseTo(0.5, 6);
  });

  it("maps the playhead into the entry's file window", () => {
    expect(fileMsAt(span({ fileStartMs: 2_500 }), 3_000)).toBe(4_500);
  });
});

describe("limiterCurve", () => {
  it("never goes above −2 dBTP, also for inputs the node clamps at ±1", () => {
    const c = dbToGain(LIMITER_CEILING_DB);
    const curve = limiterCurve();
    expect(LIMITER_CEILING_DB).toBe(-2);
    let max = 0;
    for (const v of curve) max = Math.max(max, Math.abs(v));
    expect(max).toBeLessThanOrEqual(c + 1e-6);
    expect(Math.abs(curve[0])).toBeGreaterThan(c * 0.9);
    expect(curve[curve.length - 1]).toBeCloseTo(-curve[0], 6);
  });

  it("is transparent well below the ceiling and monotonic", () => {
    const curve = limiterCurve(-2, 4001);
    const at = (x: number) => curve[Math.round(((x + 1) / 2) * (curve.length - 1))];
    expect(at(0)).toBeCloseTo(0, 6);
    expect(at(0.25)).toBeCloseTo(0.25, 3);
    expect(at(-0.25)).toBeCloseTo(-0.25, 3);
    for (let i = 1; i < curve.length; i += 1) expect(curve[i]).toBeGreaterThanOrEqual(curve[i - 1]);
  });

  it("follows another ceiling", () => {
    const curve = limiterCurve(-6);
    expect(Math.max(...Array.from(curve, Math.abs))).toBeLessThanOrEqual(dbToGain(-6) + 1e-6);
  });
});
