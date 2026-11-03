// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render } from "@testing-library/react";
import type { RecordTrackSpan } from "../../lib/ipc";
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

// ── the mix (stage 4) ────────────────────────────────────────────────────────

class FakeParam {
  value = 1;
  setTargetAtTime(v: number) {
    this.value = v;
  }
}
const mixLog: { routed: HTMLMediaElement[]; curve: Float32Array | null; ctx: number; state: string; gate: Promise<void> | null } = {
  routed: [],
  curve: null,
  ctx: 0,
  state: "running",
  gate: null,
};
class FakeCtx {
  currentTime = 0;
  state = mixLog.state;
  destination = {};
  constructor() {
    mixLog.ctx += 1;
  }
  createWaveShaper() {
    return {
      oversample: "none",
      set curve(c: Float32Array) {
        mixLog.curve = c;
      },
      connect() {},
    };
  }
  createMediaElementSource(el: HTMLMediaElement) {
    mixLog.routed.push(el);
    return { connect: () => undefined, el };
  }
  createGain() {
    const g = { gain: new FakeParam(), connect: () => undefined };
    return g;
  }
  resume() {
    return (mixLog.gate ?? Promise.resolve()).then(() => {
      this.state = "running";
    });
  }
  close() {
    return Promise.resolve();
  }
}

const trackSpan = (over: Partial<RecordTrackSpan> = {}): RecordTrackSpan => ({
  index: 0,
  repeat: 0,
  progStartMs: 1_000,
  progEndMs: 5_000,
  fileStartMs: 2_000,
  fileEndMs: 6_000,
  fadeInMs: 0,
  fadeOutMs: 0,
  gainDb: 12,
  kind: "music",
  muted: false,
  ...over,
});

function mixEl(over: Partial<Parameters<typeof AudioPreview>[0]> = {}) {
  return (
    <AudioPreview
      screenSrc="screen.mp4"
      micSrc={null}
      micOffsetMs={0}
      micGainDb={0}
      playing
      source={{ clip: 0, sourceMs: 0 }}
      speed={1}
      positionMs={2_000}
      tracks={[{ index: 0, src: "song.m4a" }]}
      spans={[trackSpan()]}
      {...over}
    />
  );
}

describe("AudioPreview mix", () => {
  beforeEach(() => {
    mixLog.routed = [];
    mixLog.curve = null;
    mixLog.ctx = 0;
    mixLog.state = "running";
    mixLog.gate = null;
    vi.useRealTimers();
    // Element playback is a no-op in the test DOM.
    vi.spyOn(HTMLMediaElement.prototype, "play").mockImplementation(() => Promise.resolve());
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => undefined);
  });

  it("levels above 0 dB reach the mix, which ends in the −2 dBTP limiter", () => {
    vi.stubGlobal("AudioContext", FakeCtx);
    const gains: number[] = [];
    vi.spyOn(FakeParam.prototype, "setTargetAtTime").mockImplementation(function (this: FakeParam, v: number) {
      this.value = v;
      gains.push(v);
    });
    render(mixEl({ systemGainDb: 6 }));
    expect(mixLog.ctx).toBe(1);
    expect(Math.max(...Array.from(mixLog.curve ?? [], Math.abs))).toBeLessThanOrEqual(10 ** (-2 / 20) + 1e-6);
    expect(gains).toContain(10 ** (6 / 20));
    expect(gains).toContain(10 ** (12 / 20));
  });

  it("plays each track from Rust's entry: file time under the playhead, its level and fades", () => {
    const { container } = render(mixEl({ spans: [trackSpan({ gainDb: -6, fadeInMs: 2_000 })] }));
    const el = container.querySelector('[data-testid="ed-audio-track"]') as HTMLAudioElement;
    // 1 s into the entry, whose file window starts at 2 s.
    expect(el.currentTime).toBe(3);
    // No WebAudio here: the volume is the level × the half-way fade.
    expect(el.volume).toBeCloseTo(10 ** (-6 / 20) * 0.5, 6);
    expect(el.crossOrigin).toBe("anonymous");
  });

  it("a muted track or a playhead outside every entry stays silent", () => {
    const pause = vi.mocked(HTMLMediaElement.prototype.pause);
    const r = render(mixEl({ spans: [trackSpan({ muted: true })] }));
    expect(pause).toHaveBeenCalled();
    pause.mockClear();
    r.rerender(mixEl({ positionMs: 6_000 }));
    expect(pause).toHaveBeenCalled();
  });

  it("a running take ducks everything", () => {
    const { container } = render(mixEl({ spans: [trackSpan({ gainDb: 0 })], duckDb: -12 }));
    const el = container.querySelector('[data-testid="ed-audio-track"]') as HTMLAudioElement;
    const sys = container.querySelector('[data-testid="ed-audio-system"]') as HTMLAudioElement;
    expect(el.volume).toBeCloseTo(10 ** (-12 / 20), 6);
    expect(sys.volume).toBeCloseTo(10 ** (-12 / 20), 6);
  });
  it("routes nothing until the context runs (a suspended context would be silent)", async () => {
    mixLog.state = "suspended";
    let start = () => undefined as void;
    mixLog.gate = new Promise<void>((r) => (start = r));
    vi.stubGlobal("AudioContext", FakeCtx);
    const { container } = render(mixEl({ spans: [trackSpan({ gainDb: -6 })] }));
    const el = container.querySelector('[data-testid="ed-audio-track"]') as HTMLAudioElement;
    expect(mixLog.routed).toHaveLength(0);
    expect(el.volume).toBeCloseTo(10 ** (-6 / 20), 6);
    // Once resume() answers, the elements go through the mix.
    await act(async () => {
      start();
      await mixLog.gate;
    });
    expect(mixLog.routed).toContain(el);
  });

  it("a failed CORS-mode load is retried without it, outside the mix, through the volume", async () => {
    vi.stubGlobal("AudioContext", FakeCtx);
    const corsAtLoad: boolean[] = [];
    const load = vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(function (this: HTMLMediaElement) {
      corsAtLoad.push(this.hasAttribute("crossorigin"));
    });
    const { container, rerender } = render(mixEl({ playing: false, spans: [trackSpan({ gainDb: -6 })] }));
    const el = container.querySelector('[data-testid="ed-audio-track"]') as HTMLAudioElement;
    expect(el.getAttribute("crossorigin")).toBe("anonymous");
    await act(async () => {
      el.dispatchEvent(new Event("error"));
    });
    expect(el.hasAttribute("crossorigin")).toBe(false);
    expect(load).toHaveBeenCalledTimes(1);
    expect(corsAtLoad, "the reload itself runs without CORS").toEqual([false]);
    rerender(mixEl({ spans: [trackSpan({ gainDb: -6 })] }));
    expect(el.hasAttribute("crossorigin"), "a re-render does not put it back").toBe(false);
    expect(mixLog.routed).not.toContain(el);
    expect(el.volume).toBeCloseTo(10 ** (-6 / 20), 6);
    // The system element still goes through the mix.
    expect(mixLog.routed).toContain(container.querySelector('[data-testid="ed-audio-system"]'));
    // A second failure does not loop.
    await act(async () => {
      el.dispatchEvent(new Event("error"));
    });
    expect(load).toHaveBeenCalledTimes(1);
  });
});
