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
  CRT_ON_MS,
  CRT_OFF_MS,
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
