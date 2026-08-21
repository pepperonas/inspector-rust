import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  MD3_SPRING,
  MD3_EASING,
  MD3_DURATION,
  simulateSpring,
  popInKeyframes,
  springScaleCss,
  primeCrtHidden,
  playCrtOn,
  playCrtOff,
  playEntrance,
  playExit,
  prefersReducedMotion,
  CRT_ON_MS,
  CRT_OFF_MS,
  EXIT_MS,
} from "./md3-motion";

describe("MD3 token tables", () => {
  it("carries the spec spring values", () => {
    expect(MD3_SPRING.spatial.expressive.fast).toEqual({ stiffness: 800, dampingRatio: 0.6 });
    expect(MD3_SPRING.spatial.standard.default).toEqual({ stiffness: 700, dampingRatio: 0.9 });
    // effects springs are critically damped in both schemes
    expect(MD3_SPRING.effects.expressive.default.dampingRatio).toBe(1);
    expect(MD3_SPRING.effects.standard.fast.dampingRatio).toBe(1);
  });
  it("has the canonical easing curves + duration scale", () => {
    expect(MD3_EASING.emphasized).toBe("cubic-bezier(0.2, 0, 0, 1)");
    expect(MD3_EASING.emphasizedDecelerate).toBe("cubic-bezier(0.05, 0.7, 0.1, 1)");
    expect(MD3_DURATION.medium2).toBe(300);
    expect(MD3_DURATION.short4).toBe(200);
  });
});

describe("simulateSpring", () => {
  it("starts at 0, ends pinned at 1, finite positive duration", () => {
    const { samples, durationMs } = simulateSpring(380, 0.8);
    expect(samples[0]).toBe(0);
    expect(samples[samples.length - 1]).toBe(1);
    expect(durationMs).toBeGreaterThan(0);
    expect(Number.isFinite(durationMs)).toBe(true);
    expect(samples.every((s) => Number.isFinite(s))).toBe(true);
  });

  it("underdamped (expressive spatial) overshoots past 1", () => {
    const { samples } = simulateSpring(800, 0.6); // ζ=0.6 → bouncy
    expect(Math.max(...samples)).toBeGreaterThan(1.02);
  });

  it("critically damped (effects) never overshoots", () => {
    const { samples } = simulateSpring(1600, 1);
    // allow a hair over 1 only at the pinned endpoint
    expect(Math.max(...samples.slice(0, -1))).toBeLessThanOrEqual(1.0001);
  });

  it("stiffer springs settle faster", () => {
    const fast = simulateSpring(1400, 0.9).durationMs;
    const slow = simulateSpring(300, 0.9).durationMs;
    expect(fast).toBeLessThan(slow);
  });

  it("honours the requested sample count", () => {
    expect(simulateSpring(700, 0.9, { sampleCount: 24 }).samples).toHaveLength(24);
  });

  it("is monotonic non-decreasing for a critically damped rise", () => {
    const { samples } = simulateSpring(800, 1);
    for (let i = 1; i < samples.length - 1; i++) {
      expect(samples[i]).toBeGreaterThanOrEqual(samples[i - 1] - 1e-9);
    }
  });
});

describe("popInKeyframes", () => {
  it("produces ordered offsets 0→1, ending at the identity transform", () => {
    const { keyframes, durationMs } = popInKeyframes(MD3_SPRING.spatial.expressive.fast);
    expect(keyframes[0].offset).toBe(0);
    expect(keyframes[keyframes.length - 1].offset).toBe(1);
    for (let i = 1; i < keyframes.length; i++) {
      expect(keyframes[i].offset as number).toBeGreaterThan(keyframes[i - 1].offset as number);
    }
    const last = keyframes[keyframes.length - 1];
    expect(last.opacity).toBe(1);
    expect(last.transform).toBe("translateY(0.000px) scale(1.0000)");
    expect(durationMs).toBeGreaterThan(0);
  });

  it("starts faded out and below the resting scale", () => {
    const { keyframes } = popInKeyframes(MD3_SPRING.spatial.expressive.fast, { fromScale: 0.9 });
    expect(keyframes[0].opacity).toBe(0);
    expect(keyframes[0].transform).toContain("scale(0.9000)");
  });
});

describe("springScaleCss", () => {
  it("emits a named @keyframes rule pinned to the from/to scales", () => {
    const { css, durationMs } = springScaleCss("t-spring", 1, 1.2, MD3_SPRING.spatial.expressive.fast);
    expect(css.startsWith("@keyframes t-spring {")).toBe(true);
    expect(css).toContain("0.00% { transform: scale(1.0000); }");
    expect(css).toContain("100.00% { transform: scale(1.2000); }");
    expect(durationMs).toBeGreaterThan(0);
  });

  it("an underdamped (expressive) spring overshoots past the target", () => {
    const { css } = springScaleCss("t-over", 0.8, 1.1, MD3_SPRING.spatial.expressive.fast);
    const scales = [...css.matchAll(/scale\((\d+\.\d+)\)/g)].map((m) => Number(m[1]));
    expect(Math.max(...scales)).toBeGreaterThan(1.1);
  });

  it("a critically damped (effects) spring never overshoots", () => {
    const { css } = springScaleCss("t-crit", 1, 1.5, MD3_SPRING.effects.standard.default);
    const scales = [...css.matchAll(/scale\((\d+\.\d+)\)/g)].map((m) => Number(m[1]));
    expect(Math.max(...scales)).toBeLessThanOrEqual(1.5001);
  });
});

describe("CRT power-on / power-off", () => {
  beforeEach(() => {
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("primeCrtHidden sets the collapsed-dot start state; null is a no-op", () => {
    const el = document.createElement("div");
    primeCrtHidden(el);
    expect(el.style.opacity).toBe("1");
    expect(el.style.transform).toContain("scaleX(0.02)");
    expect(el.style.transform).toContain("scaleY(0.012)");
    expect(el.style.filter).toContain("brightness");
    expect(() => primeCrtHidden(null)).not.toThrow();
  });

  it("playCrtOn clears the primed styles when WAAPI is unavailable (settle)", () => {
    // happy-dom elements have no `.animate` → the settle path runs.
    const el = document.createElement("div");
    primeCrtHidden(el);
    playCrtOn(el);
    expect(el.style.opacity).toBe("");
    expect(el.style.transform).toBe("");
    expect(el.style.filter).toBe("");
    expect(() => playCrtOn(null)).not.toThrow();
  });

  it("playCrtOn animates dot → scanline → picture with a brightness flash", () => {
    const calls: unknown[][] = [];
    const el = {
      style: {} as CSSStyleDeclaration,
      getAnimations: () => [],
      animate: (kf: unknown, opts: unknown) => {
        calls.push([kf, opts]);
        return { addEventListener: (_e: string, cb: () => void) => cb(), cancel: () => {} };
      },
    } as unknown as HTMLElement;
    playCrtOn(el);
    expect(calls).toHaveLength(1);
    const [kf, opts] = calls[0] as [Array<Record<string, unknown>>, Record<string, unknown>];
    expect(kf).toHaveLength(4);
    // first frame is the collapsed dot, last is the settled full picture
    expect(String(kf[0].transform)).toContain("scaleX(0.02)");
    expect(kf[3].transform).toBe("scaleX(1) scaleY(1)");
    // a phosphor-bright flash at the start, settling to brightness(1)
    expect(String(kf[0].filter)).toContain("brightness(2.6)");
    expect(kf[3].filter).toBe("brightness(1)");
    expect(opts.duration).toBe(CRT_ON_MS);
    expect(opts.fill).toBe("forwards");
  });

  it("playCrtOff collapses picture → scanline → dot → burnout and resolves", async () => {
    const calls: unknown[][] = [];
    const el = {
      style: {} as CSSStyleDeclaration,
      getAnimations: () => [],
      animate: (kf: unknown, opts: unknown) => {
        calls.push([kf, opts]);
        return { finished: Promise.resolve() };
      },
    } as unknown as HTMLElement;
    await expect(playCrtOff(el)).resolves.toBeUndefined();
    const [kf, opts] = calls[0] as [Array<Record<string, unknown>>, Record<string, unknown>];
    expect(kf).toHaveLength(4);
    expect(kf[0].transform).toBe("scaleX(1) scaleY(1)"); // full picture
    expect(kf[0].opacity).toBe(1);
    expect(String(kf[1].transform)).toContain("scaleY(0.012)"); // collapsed scanline
    expect(kf[3].opacity).toBe(0); // burnt out
    expect(opts.duration).toBe(CRT_OFF_MS);
  });

  it("playCrtOff resolves immediately without WAAPI", async () => {
    const el = document.createElement("div"); // no `.animate`
    await expect(playCrtOff(el)).resolves.toBeUndefined();
    await expect(playCrtOff(null)).resolves.toBeUndefined();
  });

  it("hides the scrollbar (crt-anim on <html>) from prime through settle", () => {
    const el = document.createElement("div");
    primeCrtHidden(el);
    expect(document.documentElement.classList.contains("crt-anim")).toBe(true);
    // no WAAPI here → playCrtOn takes the settle path, which restores the bar
    playCrtOn(el);
    expect(document.documentElement.classList.contains("crt-anim")).toBe(false);
  });
});

describe("simulateSpring — damping regimes + guards", () => {
  it("an overdamped spring (ζ > 1) rises without ever overshooting", () => {
    const { samples } = simulateSpring(400, 1.6);
    // Every interior sample stays at or below the target…
    expect(Math.max(...samples.slice(0, -1))).toBeLessThanOrEqual(1.0001);
    // …and the rise is monotonic (no ring at all).
    for (let i = 1; i < samples.length - 1; i++) {
      expect(samples[i]).toBeGreaterThanOrEqual(samples[i - 1] - 1e-9);
    }
    expect(samples[0]).toBe(0);
    expect(samples[samples.length - 1]).toBe(1);
    expect(samples.every(Number.isFinite)).toBe(true);
  });

  it("an overdamped spring settles more slowly than a critically damped one", () => {
    // Same stiffness, more damping → sluggish. (Physics sanity: over-damping
    // is the "no bounce but slow" regime, not a faster one.)
    expect(simulateSpring(400, 2.5).durationMs).toBeGreaterThan(
      simulateSpring(400, 1).durationMs,
    );
  });

  it("clamps the sample count to at least two (a keyframe list needs both ends)", () => {
    expect(simulateSpring(700, 0.9, { sampleCount: 1 }).samples).toHaveLength(2);
    expect(simulateSpring(700, 0.9, { sampleCount: 0 }).samples).toHaveLength(2);
    expect(simulateSpring(700, 0.9, { sampleCount: -5 }).samples).toHaveLength(2);
  });

  it("never runs longer than maxMs, even for a spring that rings forever", () => {
    // ζ = 0 is undamped — it oscillates for all time, so the scan must be
    // bounded by maxMs rather than looping to the settle threshold.
    const undamped = simulateSpring(200, 0, { maxMs: 500 });
    expect(undamped.durationMs).toBeLessThanOrEqual(500);
    expect(undamped.samples.every(Number.isFinite)).toBe(true);
    expect(simulateSpring(200, 0).durationMs).toBeLessThanOrEqual(4000);
  });

  it("a negative damping ratio is clamped to 0 rather than blowing up", () => {
    const { samples, durationMs } = simulateSpring(300, -1, { maxMs: 300 });
    expect(samples.every(Number.isFinite)).toBe(true);
    expect(durationMs).toBeGreaterThan(0);
  });

  it("a zero / negative stiffness degrades to finite samples (no NaN keyframes)", () => {
    for (const k of [0, -100]) {
      const { samples, durationMs } = simulateSpring(k, 0.9, { maxMs: 200 });
      expect(samples.every(Number.isFinite)).toBe(true);
      expect(samples[0]).toBe(0);
      expect(samples[samples.length - 1]).toBe(1);
      expect(durationMs).toBeGreaterThan(0);
    }
  });

  it("a tighter settle threshold takes longer to reach", () => {
    const loose = simulateSpring(380, 0.8, { settleThreshold: 0.05 }).durationMs;
    const tight = simulateSpring(380, 0.8, { settleThreshold: 0.0001 }).durationMs;
    expect(tight).toBeGreaterThan(loose);
  });
});

describe("prefersReducedMotion", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("is true when the OS asks for reduced motion", () => {
    vi.stubGlobal("matchMedia", (q: string) => ({ matches: q.includes("reduce") }));
    expect(prefersReducedMotion()).toBe(true);
  });

  it("is false when the media query does not match", () => {
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
    expect(prefersReducedMotion()).toBe(false);
  });

  it("is false (never throws) when matchMedia is unavailable", () => {
    // A webview / test env without the API must not break every animation.
    vi.stubGlobal("matchMedia", undefined);
    expect(prefersReducedMotion()).toBe(false);
  });
});

/** A minimal WAAPI-capable stand-in — happy-dom elements have no `.animate`. */
function fakeEl(opts: { finished?: Promise<unknown>; existing?: { cancel: () => void }[] } = {}) {
  const calls: { keyframes: Keyframe[]; options: KeyframeAnimationOptions }[] = [];
  let finishCb: (() => void) | null = null;
  let cancelled = 0;
  const el = {
    style: {} as CSSStyleDeclaration,
    getAnimations: () => opts.existing ?? [],
    animate: (keyframes: Keyframe[], options: KeyframeAnimationOptions) => {
      calls.push({ keyframes, options });
      return {
        finished: opts.finished ?? Promise.resolve(),
        addEventListener: (_e: string, cb: () => void) => {
          finishCb = cb;
        },
        cancel: () => {
          cancelled++;
        },
      };
    },
  } as unknown as HTMLElement;
  return {
    el,
    calls,
    finish: () => finishCb?.(),
    cancelled: () => cancelled,
  };
}

describe("playEntrance", () => {
  beforeEach(() => {
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
  });
  afterEach(() => vi.unstubAllGlobals());

  it("plays the spring keyframes linearly and holds the final frame", () => {
    // The overshoot lives in the samples, so the animation itself MUST be
    // linear — an easing on top would double-ease the spring.
    const f = fakeEl();
    playEntrance(f.el);
    expect(f.calls).toHaveLength(1);
    const { keyframes, options } = f.calls[0];
    expect(options.easing).toBe("linear");
    expect(options.fill).toBe("forwards");
    expect(options.duration).toBeGreaterThan(0);
    expect(keyframes[0].offset).toBe(0);
    expect(keyframes[keyframes.length - 1].offset).toBe(1);
    expect(keyframes[0].opacity).toBe(0);
    expect(keyframes[keyframes.length - 1].opacity).toBe(1);
  });

  it("clears the primed inline styles once the entrance finishes", () => {
    // App.tsx primes the shell to opacity 0 WHILE hidden; if `settle` never ran
    // the popup would stay invisible after the animation.
    const f = fakeEl();
    f.el.style.opacity = "0";
    f.el.style.transform = "scale(.97)";
    playEntrance(f.el);
    expect(f.el.style.opacity).toBe("0"); // still primed while animating
    f.finish();
    expect(f.el.style.opacity).toBe("");
    expect(f.el.style.transform).toBe("");
    expect(f.cancelled()).toBe(1); // the finished animation releases its fill
  });

  it("cancels a leftover forwards-filled animation before starting", () => {
    // A `fill: forwards` exit from the previous dismiss would otherwise pin the
    // shell at opacity 0 and the entrance would play invisibly.
    let cancels = 0;
    const f = fakeEl({ existing: [{ cancel: () => cancels++ }, { cancel: () => cancels++ }] });
    playEntrance(f.el);
    expect(cancels).toBe(2);
  });

  it("with reduced motion it settles instantly and animates nothing", () => {
    vi.stubGlobal("matchMedia", () => ({ matches: true }));
    const f = fakeEl();
    f.el.style.opacity = "0";
    playEntrance(f.el);
    expect(f.calls).toHaveLength(0);
    expect(f.el.style.opacity).toBe("");
  });

  it("honours the fromScale / riseY options", () => {
    const f = fakeEl();
    playEntrance(f.el, MD3_SPRING.spatial.expressive.fast, { fromScale: 0.5, riseY: 40 });
    const first = f.calls[0].keyframes[0];
    expect(String(first.transform)).toContain("scale(0.5000)");
    expect(String(first.transform)).toContain("translateY(40.000px)");
  });

  it("is a no-op for a missing element or an environment without WAAPI", () => {
    expect(() => playEntrance(null)).not.toThrow();
    expect(() => playEntrance(document.createElement("div"))).not.toThrow();
  });
});

describe("playExit", () => {
  beforeEach(() => {
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
  });
  afterEach(() => vi.unstubAllGlobals());

  it("accelerates away — fades out while dropping and shrinking", () => {
    const f = fakeEl();
    void playExit(f.el);
    const { keyframes, options } = f.calls[0];
    expect(keyframes[0].opacity).toBe(1);
    expect(keyframes[1].opacity).toBe(0);
    expect(keyframes[0].transform).toBe("translateY(0px) scale(1)");
    expect(String(keyframes[1].transform)).toContain("translateY(8px)");
    expect(String(keyframes[1].transform)).toContain("scale(0.96)");
    expect(options.duration).toBe(EXIT_MS);
    expect(options.fill).toBe("forwards"); // stays hidden until the window hides
    expect(options.easing).toBe("cubic-bezier(0.3, 0, 0.8, 0.15)");
  });

  it("honours custom toScale / dropY", () => {
    const f = fakeEl();
    void playExit(f.el, { toScale: 0.5, dropY: 30 });
    expect(String(f.calls[0].keyframes[1].transform)).toBe("translateY(30px) scale(0.5)");
  });

  it("resolves so the caller can hide the window afterwards", async () => {
    const f = fakeEl();
    await expect(playExit(f.el)).resolves.toBeUndefined();
  });

  it("resolves even when the animation is cancelled mid-flight", async () => {
    // A re-show cancels the exit → WAAPI rejects `finished`. hidePopup awaits
    // this promise, so a rejection would leave the popup stuck on screen.
    const f = fakeEl({ finished: Promise.reject(new Error("cancelled")) });
    await expect(playExit(f.el)).resolves.toBeUndefined();
  });

  it("with reduced motion it resolves immediately without animating", async () => {
    // No added dismiss latency for reduced-motion users.
    vi.stubGlobal("matchMedia", () => ({ matches: true }));
    const f = fakeEl();
    await expect(playExit(f.el)).resolves.toBeUndefined();
    expect(f.calls).toHaveLength(0);
  });

  it("resolves for a missing element or without WAAPI", async () => {
    await expect(playExit(null)).resolves.toBeUndefined();
    await expect(playExit(document.createElement("div"))).resolves.toBeUndefined();
  });

  it("is shorter than the spring entrance (exits accelerate away)", () => {
    expect(EXIT_MS).toBeLessThan(
      popInKeyframes(MD3_SPRING.spatial.expressive.fast).durationMs,
    );
  });
});

describe("CRT — reduced motion + failure paths", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    document.documentElement.classList.remove("crt-anim");
  });

  it("playCrtOn settles instead of animating when motion is reduced", () => {
    vi.stubGlobal("matchMedia", () => ({ matches: true }));
    const f = fakeEl();
    primeCrtHidden(f.el);
    playCrtOn(f.el);
    expect(f.calls).toHaveLength(0);
    // A primed dot must never be left stuck on screen.
    expect(f.el.style.transform).toBe("");
    expect(f.el.style.filter).toBe("");
    expect(document.documentElement.classList.contains("crt-anim")).toBe(false);
  });

  it("playCrtOff resolves immediately (no dismiss latency) under reduced motion", async () => {
    vi.stubGlobal("matchMedia", () => ({ matches: true }));
    const f = fakeEl();
    await expect(playCrtOff(f.el)).resolves.toBeUndefined();
    expect(f.calls).toHaveLength(0);
  });

  it("playCrtOff restores the scrollbar even when the collapse is cancelled", async () => {
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
    const f = fakeEl({ finished: Promise.reject(new Error("cancelled")) });
    await playCrtOff(f.el);
    // The class must not leak — the next open would render with a hidden bar.
    expect(document.documentElement.classList.contains("crt-anim")).toBe(false);
  });

  it("the power-off is quicker than the power-on (dismiss must feel instant)", () => {
    expect(CRT_OFF_MS).toBeLessThan(CRT_ON_MS);
  });

  it("keeps the popup inside its perceived-latency budget", () => {
    // v0.107.0 budget, widened once in v0.112.3 (see CRT_ON_MS). The native
    // window is up ~20 ms after the hotkey (measured 17–38 ms in Rust), so the
    // ENTIRE felt latency of "Ctrl+Space is slow" lives in this animation.
    // Two hard rules:
    //   1. the whole power-on stays inside the ~250 ms window a UI can spend
    //      before it stops reading as instant, and
    //   2. the shell reaches FULL HEIGHT (= the list becomes readable) well
    //      before that — everything after is just the phosphor settle.
    expect(CRT_ON_MS).toBeLessThanOrEqual(250);
    const el = {
      style: {} as CSSStyleDeclaration,
      getAnimations: () => [],
      animate: (kf: unknown) => {
        const frames = kf as Array<Record<string, unknown>>;
        // The frame that first reaches full height is the legibility moment.
        const legible = frames.find(
          (f) => typeof f.transform === "string" && !/scaleY\(0\./.test(f.transform),
        )!;
        expect((legible.offset as number) * CRT_ON_MS).toBeLessThanOrEqual(150);
        return { addEventListener: (_e: string, cb: () => void) => cb(), cancel: () => {} };
      },
    } as unknown as HTMLElement;
    playCrtOn(el);
  });
});
