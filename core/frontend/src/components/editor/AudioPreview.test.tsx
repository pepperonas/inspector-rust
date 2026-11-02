// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import { AudioPreview, type ClickPreview } from "./AudioPreview";

const played: { src: string; volume: number }[] = [];
class FakeAudio {
  volume = 1;
  constructor(public src: string) {}
  play() {
    played.push({ src: this.src, volume: this.volume });
    return Promise.resolve();
  }
}

const clicks = (over: Partial<ClickPreview> = {}): ClickPreview => ({
  hits: [
    { progMs: 1_000, gain: 1, release: false },
    { progMs: 1_090, gain: 0.7, release: true },
    { progMs: 3_000, gain: 1, release: false },
  ],
  downSrc: "down.wav",
  upSrc: "up.wav",
  gainDb: -6,
  on: true,
  ...over,
});

function el(positionMs: number, playing: boolean, c: ClickPreview | null) {
  return (
    <AudioPreview
      screenSrc={null}
      micSrc={null}
      micOffsetMs={0}
      micGainDb={0}
      playing={playing}
      source={null}
      speed={1}
      positionMs={positionMs}
      clicks={c}
    />
  );
}

beforeEach(() => {
  played.length = 0;
  vi.useFakeTimers();
  vi.stubGlobal("Audio", FakeAudio);
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("AudioPreview click sound", () => {
  it("plays press and release on time, at the export's level, once", () => {
    const c = clicks();
    const r = render(el(900, true, c));
    vi.advanceTimersByTime(99);
    expect(played).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(played).toEqual([{ src: "down.wav", volume: 10 ** (-6 / 20) }]);
    for (const p of [966, 1_032, 1_098, 1_164]) r.rerender(el(p, true, c));
    vi.advanceTimersByTime(500);
    expect(played.map((x) => x.src)).toEqual(["down.wav", "up.wav"]);
    expect(played[1].volume).toBeCloseTo(0.7 * 10 ** (-6 / 20), 6);
  });

  it("is silent while paused, with the click sound off, and across a seek", () => {
    const r = render(el(900, false, clicks()));
    vi.advanceTimersByTime(500);
    r.rerender(el(900, true, clicks({ on: false })));
    vi.advanceTimersByTime(500);
    expect(played).toEqual([]);
    // Playing from 900, then a seek past the 3-s click: it does not sound.
    r.rerender(el(2_000, true, clicks()));
    r.rerender(el(3_500, true, clicks()));
    vi.advanceTimersByTime(2_000);
    expect(played.filter((x) => x.src === "down.wav")).toHaveLength(0);
  });

  it("a pause drops hits already scheduled", () => {
    const r = render(el(950, true, clicks()));
    r.rerender(el(950, false, clicks()));
    vi.advanceTimersByTime(500);
    expect(played).toEqual([]);
  });
});
