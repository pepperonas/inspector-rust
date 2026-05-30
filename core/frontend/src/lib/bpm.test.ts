import { describe, expect, it } from "vitest";
import { BPM_CONFIG, BpmAnalyzer } from "./bpm";

/** Synthesize a single audio chunk at a given RMS level. */
function chunk(rms: number, n = 128): Float32Array {
  // RMS = sqrt(mean(sample²)) → setting every sample to `rms` works,
  // since RMS of a constant signal equals the absolute value.
  return new Float32Array(n).fill(rms);
}

/** Simulate `seconds` of pumping `bpm` beats into the analyzer.
 *  Quiet baseline + loud "beat" chunks spaced at the target interval.
 *  Returns the final estimate. */
function simulateBeats(
  bpm: number,
  seconds: number,
  startMs = 0,
  baseline = 0.02,
  beatEnergy = 0.5,
): { analyzer: BpmAnalyzer; finalEstimate: ReturnType<BpmAnalyzer["estimate"]> } {
  const analyzer = new BpmAnalyzer();
  const beatIntervalMs = 60000 / bpm;
  // Feed 100 chunks/sec — coarse enough to be fast, fine enough to
  // catch beat boundaries within ~10 ms.
  const chunkIntervalMs = 10;
  let nextBeatAt = startMs;
  for (let t = startMs; t < startMs + seconds * 1000; t += chunkIntervalMs) {
    // A beat fires if the next-beat timestamp falls inside this chunk.
    const isBeat = t >= nextBeatAt && t < nextBeatAt + chunkIntervalMs;
    if (isBeat) nextBeatAt += beatIntervalMs;
    analyzer.push(chunk(isBeat ? beatEnergy : baseline), t);
  }
  return {
    analyzer,
    finalEstimate: analyzer.estimate(startMs + seconds * 1000),
  };
}

describe("BpmAnalyzer", () => {
  it("returns BPM=0 + confidence=0 before any audio", () => {
    const a = new BpmAnalyzer();
    const est = a.estimate(0);
    expect(est.bpm).toBe(0);
    expect(est.confidence).toBe(0);
    expect(est.beatJustFired).toBe(false);
  });

  it("returns BPM=0 with only a couple of onsets", () => {
    const { finalEstimate } = simulateBeats(120, 1.2); // ~2 beats
    expect(finalEstimate.bpm).toBe(0);
    expect(finalEstimate.confidence).toBe(0);
  });

  it("locks onto 120 BPM within ~10 seconds", () => {
    const { finalEstimate } = simulateBeats(120, 10);
    expect(finalEstimate.bpm).toBeGreaterThan(115);
    expect(finalEstimate.bpm).toBeLessThan(125);
    expect(finalEstimate.confidence).toBeGreaterThan(0.7);
  });

  it("locks onto 90 BPM", () => {
    const { finalEstimate } = simulateBeats(90, 10);
    expect(finalEstimate.bpm).toBeGreaterThan(85);
    expect(finalEstimate.bpm).toBeLessThan(95);
  });

  it("locks onto 175 BPM (drum'n'bass / metal range)", () => {
    const { finalEstimate } = simulateBeats(175, 10);
    expect(finalEstimate.bpm).toBeGreaterThan(170);
    expect(finalEstimate.bpm).toBeLessThan(180);
  });

  it("octave-corrects a half-time reading", () => {
    // 50 BPM is below BPM_MIN=60 → should double to 100.
    const { finalEstimate } = simulateBeats(50, 10);
    expect(finalEstimate.bpm).toBeGreaterThan(95);
    expect(finalEstimate.bpm).toBeLessThan(105);
  });

  it("octave-corrects a double-time reading", () => {
    // 240 BPM is above BPM_MAX=200 → should halve to 120.
    const { finalEstimate } = simulateBeats(240, 10);
    expect(finalEstimate.bpm).toBeGreaterThan(115);
    expect(finalEstimate.bpm).toBeLessThan(125);
  });

  it("reports low confidence for irregular onsets", () => {
    const a = new BpmAnalyzer();
    // Prime the moving average with a few chunks of baseline so the
    // threshold has something to be relative to.
    for (let t = 0; t < 500; t += 10) a.push(chunk(0.02), t);
    // Now drop 5 onsets at *jittered* intervals.
    [600, 850, 1300, 1450, 1900, 2050, 2800, 3100].forEach((t) => {
      a.push(chunk(0.5), t);
      a.push(chunk(0.02), t + 10);
    });
    const est = a.estimate(3500);
    // Jitter is ~ ±300 ms on a ~500 ms median → confidence well below 0.7.
    expect(est.confidence).toBeLessThan(0.7);
  });

  it("beatJustFired is true exactly once per beat", () => {
    const a = new BpmAnalyzer();
    // Baseline.
    for (let t = 0; t < 500; t += 10) a.push(chunk(0.02), t);
    let fires = 0;
    const beats = [500, 1000, 1500, 2000, 2500];
    for (let t = 500; t <= 2500; t += 10) {
      const isBeat = beats.includes(t);
      a.push(chunk(isBeat ? 0.5 : 0.02), t);
      if (a.estimate(t).beatJustFired) fires++;
    }
    expect(fires).toBe(beats.length);
  });

  it("respects the refractory period (no double-trigger inside it)", () => {
    const a = new BpmAnalyzer();
    for (let t = 0; t < 500; t += 10) a.push(chunk(0.02), t);
    // Two loud chunks 100 ms apart (well under the 250 ms refractory).
    a.push(chunk(0.6), 600);
    a.push(chunk(0.6), 700);
    // Only the first should count.
    let fires = 0;
    if (a.estimate(600).beatJustFired) fires++;
    if (a.estimate(700).beatJustFired) fires++;
    // The 700 ms push set justFiredBeat=false because of refractory;
    // estimate consumes the edge, so the count is 1 if push@600 set
    // it (correct) and push@700 didn't.
    expect(fires).toBeLessThanOrEqual(1);
  });

  it("reset() clears all state", () => {
    const { analyzer } = simulateBeats(120, 8);
    expect(analyzer.estimate(8000).bpm).toBeGreaterThan(0);
    analyzer.reset();
    expect(analyzer.estimate(8000).bpm).toBe(0);
    expect(analyzer.currentEnergy()).toBe(0);
  });

  it("currentEnergy reflects the last pushed chunk", () => {
    const a = new BpmAnalyzer();
    a.push(chunk(0.3), 0);
    expect(a.currentEnergy()).toBeCloseTo(0.3, 5);
    a.push(chunk(0.05), 100);
    expect(a.currentEnergy()).toBeCloseTo(0.05, 5);
  });
});

describe("BPM_CONFIG", () => {
  it("BPM range is sensible (60-200 covers all popular music genres)", () => {
    expect(BPM_CONFIG.BPM_MIN).toBe(60);
    expect(BPM_CONFIG.BPM_MAX).toBe(200);
  });
  it("refractory caps detectable rate at 200 BPM (v0.45.1: raised from 240 for BT-echo resistance)", () => {
    expect(60000 / BPM_CONFIG.ONSET_REFRACTORY_MS).toBe(200);
  });
  it("display average window covers 3-5 seconds (the user's expectation)", () => {
    expect(BPM_CONFIG.DISPLAY_AVG_WINDOW_MS).toBeGreaterThanOrEqual(3000);
    expect(BPM_CONFIG.DISPLAY_AVG_WINDOW_MS).toBeLessThanOrEqual(5000);
  });
});

describe("BpmAnalyzer.estimate — stale-reset", () => {
  it("keeps showing the last BPM through a brief onset drought (< STALE_RESET_MS)", () => {
    // Lock on at 120 BPM…
    const { analyzer } = simulateBeats(120, 10);
    const locked = analyzer.estimate(10_000).bpm;
    expect(locked).toBeGreaterThan(115);
    // …then feed pure baseline for 3 seconds (under the threshold).
    for (let t = 10_010; t <= 13_000; t += 10) {
      analyzer.push(chunk(0.02), t);
    }
    // Estimate still shows the last known BPM (we don't blank on
    // brief silence — bt dropouts shouldn't kill the display).
    const stillThere = analyzer.estimate(13_000).bpm;
    expect(stillThere).toBeGreaterThan(115);
    expect(stillThere).toBeLessThan(125);
  });

  it("decays BPM → 0 after sustained silence (> STALE_RESET_MS)", () => {
    const { analyzer } = simulateBeats(120, 10);
    expect(analyzer.estimate(10_000).bpm).toBeGreaterThan(115);
    // Pure baseline for 5+ seconds — longer than STALE_RESET_MS (4s).
    // We do still push chunks (the AVG window stays primed); just no
    // beat-energy chunks, so no onsets accumulate.
    for (let t = 10_010; t <= 16_000; t += 10) {
      analyzer.push(chunk(0.02), t);
    }
    expect(analyzer.estimate(16_000).bpm).toBe(0);
  });
});

describe("BpmAnalyzer.estimate — octave snap", () => {
  it("doesn't flip from 120 to 240 when a single rogue half-IOI lands in the window", () => {
    // Lock on at 120 BPM (500 ms IOI) first.
    const { analyzer } = simulateBeats(120, 10);
    const lockedAt120 = analyzer.estimate(10_000).bpm;
    expect(lockedAt120).toBeGreaterThan(115);
    expect(lockedAt120).toBeLessThan(125);
    // Now inject 8 onsets that look like 240 BPM (250 ms IOI) — a worst
    // case where some echoes happen to align into a double-tempo IOI
    // pattern in the recent window.
    for (let t = 10_500; t <= 12_500; t += 250) {
      // baseline so the moving avg stays calibrated
      analyzer.push(chunk(0.02), t - 50);
      analyzer.push(chunk(0.6), t); // strong onset
    }
    const after = analyzer.estimate(12_500).bpm;
    // Octave-snap should keep us in the 120 octave (loose tolerance
    // since EMA still drifts slightly toward the new pattern).
    expect(after).toBeGreaterThan(110);
    expect(after).toBeLessThan(140);
  });
});
