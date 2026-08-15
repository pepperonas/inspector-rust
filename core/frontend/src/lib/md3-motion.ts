/**
 * Material Design 3 **Expressive** motion tokens + a faithful spring simulator.
 *
 * MD3's motion system is built on springs defined by a *stiffness* and a
 * *damping ratio*. Two families:
 *   • **Spatial** — position/size/shape changes; configured with damping < 1 so
 *     they overshoot and bounce (that's the "expressive" feel).
 *   • **Effects** — colour/opacity changes; critically damped (ζ = 1, no bounce).
 * Each family has Standard (subdued) and Expressive (livelier) schemes in
 * fast/default/slow variants. The exact token values below are from the M3 spec.
 *
 * We can't get real spring physics from a CSS `transition`, so `simulateSpring`
 * computes the analytic step response of a unit-mass second-order system and
 * `springKeyframes`/`popInKeyframes` turn that into Web-Animations keyframes
 * (the overshoot is baked into the samples → play them with `easing: "linear"`).
 * The pure math is unit-tested; only `playEntrance` touches the DOM.
 *
 * Sources:
 *   https://m3.material.io/styles/motion/overview/how-it-works
 *   https://m3.material.io/styles/motion/easing-and-duration/tokens-specs
 */

/** MD3 easing curves (cubic-bezier), also exported to CSS as custom properties. */
export const MD3_EASING = {
  emphasized: "cubic-bezier(0.2, 0, 0, 1)",
  emphasizedDecelerate: "cubic-bezier(0.05, 0.7, 0.1, 1)",
  emphasizedAccelerate: "cubic-bezier(0.3, 0, 0.8, 0.15)",
  standard: "cubic-bezier(0.2, 0, 0, 1)",
  standardDecelerate: "cubic-bezier(0, 0, 0, 1)",
  standardAccelerate: "cubic-bezier(0.3, 0, 1, 1)",
} as const;

/** MD3 duration scale (milliseconds). */
export const MD3_DURATION = {
  short1: 50, short2: 100, short3: 150, short4: 200,
  medium1: 250, medium2: 300, medium3: 350, medium4: 400,
  long1: 450, long2: 500, long3: 550, long4: 600,
} as const;

export interface SpringToken {
  /** Spring constant `k` (mass is taken as 1). */
  stiffness: number;
  /** Damping ratio `ζ` — < 1 underdamped (overshoots), 1 critically damped. */
  dampingRatio: number;
}

/** The MD3 spring motion token table (stiffness, damping ratio). */
export const MD3_SPRING = {
  spatial: {
    standard: {
      fast: { stiffness: 1400, dampingRatio: 0.9 },
      default: { stiffness: 700, dampingRatio: 0.9 },
      slow: { stiffness: 300, dampingRatio: 0.9 },
    },
    expressive: {
      fast: { stiffness: 800, dampingRatio: 0.6 },
      default: { stiffness: 380, dampingRatio: 0.8 },
      slow: { stiffness: 200, dampingRatio: 0.8 },
    },
  },
  effects: {
    // Effects springs are critically damped in both schemes (no overshoot).
    standard: {
      fast: { stiffness: 3800, dampingRatio: 1 },
      default: { stiffness: 1600, dampingRatio: 1 },
      slow: { stiffness: 800, dampingRatio: 1 },
    },
    expressive: {
      fast: { stiffness: 3800, dampingRatio: 1 },
      default: { stiffness: 1600, dampingRatio: 1 },
      slow: { stiffness: 800, dampingRatio: 1 },
    },
  },
} as const satisfies Record<string, Record<string, Record<string, SpringToken>>>;

export interface SpringSimulation {
  /** Normalised position samples, evenly spaced over `[0, durationMs]`. Start
   *  at 0; underdamped springs overshoot past 1 before settling; last is 1. */
  samples: number[];
  /** Time (ms) for the spring to settle within the threshold. */
  durationMs: number;
}

/**
 * Analytic unit-step response of a mass-spring-damper (mass = 1) — the position
 * `x(t)` of a system driven from 0 to 1. Underdamped (ζ < 1) overshoots and
 * rings; critically damped (ζ = 1) rises without overshoot; overdamped (ζ > 1)
 * is handled for completeness. Returns evenly-spaced samples over the settling
 * time plus that duration.
 */
export function simulateSpring(
  stiffness: number,
  dampingRatio: number,
  opts: { sampleCount?: number; settleThreshold?: number; maxMs?: number } = {},
): SpringSimulation {
  const sampleCount = Math.max(2, opts.sampleCount ?? 60);
  const settle = opts.settleThreshold ?? 0.001;
  const maxMs = opts.maxMs ?? 4000;
  const wn = Math.sqrt(Math.max(1e-6, stiffness)); // natural frequency (mass = 1)
  const z = Math.max(0, dampingRatio);

  const pos = (t: number): number => {
    if (z < 1) {
      const wd = wn * Math.sqrt(1 - z * z);
      const e = Math.exp(-z * wn * t);
      return 1 - e * (Math.cos(wd * t) + ((z * wn) / wd) * Math.sin(wd * t));
    }
    if (z === 1) {
      const e = Math.exp(-wn * t);
      return 1 - e * (1 + wn * t);
    }
    const s = wn * Math.sqrt(z * z - 1);
    const r1 = -z * wn + s;
    const r2 = -z * wn - s;
    return 1 - (r1 * Math.exp(r2 * t) - r2 * Math.exp(r1 * t)) / (r1 - r2);
  };

  // Settling time = last instant the response is still outside the threshold,
  // plus a small margin. Robust for the ringing (underdamped) case.
  const dt = 0.001; // 1 ms scan
  const maxT = maxMs / 1000;
  let lastViolation = 0;
  for (let t = 0; t <= maxT; t += dt) {
    if (Math.abs(pos(t) - 1) >= settle) lastViolation = t;
  }
  const settleT = Math.min(maxT, lastViolation + 0.02);

  const samples: number[] = [];
  for (let i = 0; i < sampleCount; i++) {
    samples.push(pos((settleT * i) / (sampleCount - 1)));
  }
  samples[0] = 0;
  samples[sampleCount - 1] = 1; // pin the endpoints exactly
  return { samples, durationMs: Math.max(1, Math.round(settleT * 1000)) };
}

/** Linear interpolation. */
function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

/**
 * Render a spring-driven scale as a CSS `@keyframes` rule — real M3 spring
 * physics baked into evenly-spaced percentage steps (play the animation with
 * `animation-timing-function: linear`; the overshoot/ring lives in the
 * samples). Pure + unit-tested; the window palette injects the result once.
 */
export function springScaleCss(
  name: string,
  fromScale: number,
  toScale: number,
  token: SpringToken,
): { css: string; durationMs: number } {
  const sim = simulateSpring(token.stiffness, token.dampingRatio, { sampleCount: 28 });
  const n = sim.samples.length;
  const steps = sim.samples
    .map((v, i) => {
      const pct = ((100 * i) / (n - 1)).toFixed(2);
      const scale = lerp(fromScale, toScale, v).toFixed(4);
      return `${pct}% { transform: scale(${scale}); }`;
    })
    .join(" ");
  return { css: `@keyframes ${name} { ${steps} }`, durationMs: sim.durationMs };
}

/**
 * Build Web-Animations keyframes for a "pop-in" entrance from a spring token:
 * fade in while rising (`riseY` px) and scaling up (`fromScale` → 1). The
 * spring's overshoot makes it settle past the target and back — the expressive
 * bounce. Play with `{ duration: durationMs, easing: "linear" }`.
 */
export function popInKeyframes(
  token: SpringToken,
  opts: { fromScale?: number; riseY?: number; sampleCount?: number } = {},
): { keyframes: Keyframe[]; durationMs: number } {
  const fromScale = opts.fromScale ?? 0.96;
  const riseY = opts.riseY ?? 10;
  const { samples, durationMs } = simulateSpring(token.stiffness, token.dampingRatio, {
    sampleCount: opts.sampleCount ?? 60,
  });
  const n = samples.length;
  const keyframes: Keyframe[] = samples.map((x, i) => {
    const scale = lerp(fromScale, 1, x); // overshoots past 1 when x > 1
    const y = (1 - x) * riseY; // goes slightly negative on overshoot (rises past)
    // Opacity is an "effects" property — ramp it in faster than the spatial
    // settle so the panel is legible well before the bounce finishes.
    const opacity = Math.max(0, Math.min(1, x * 1.8));
    return {
      offset: i / (n - 1),
      opacity,
      transform: `translateY(${y.toFixed(3)}px) scale(${scale.toFixed(4)})`,
    };
  });
  return { keyframes, durationMs };
}

/** Respect the OS "reduce motion" accessibility preference. */
export function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

/**
 * Play an MD3 spring entrance on an element via the Web Animations API (so it
 * re-triggers on demand, unlike a CSS animation bound at mount). No-op when the
 * element is missing, `element.animate` is unavailable, or the user prefers
 * reduced motion.
 */
export function playEntrance(
  el: HTMLElement | null,
  token: SpringToken = MD3_SPRING.spatial.expressive.fast,
  opts: { fromScale?: number; riseY?: number } = {},
): void {
  if (!el || typeof el.animate !== "function") return;
  // The shell is "primed" to the entrance START state (opacity 0 + slightly
  // small) WHILE the popup is hidden (see App.tsx's popup-hidden handler), so the
  // OS `show()` — which makes the window visible *before* this handler runs —
  // never flashes a frame of full-opacity content. `settle` removes that prime
  // (back to the CSS-natural state) once the entrance has played.
  const settle = () => {
    el.style.opacity = "";
    el.style.transform = "";
  };
  if (prefersReducedMotion()) {
    settle();
    return;
  }
  // Clear any `forwards`-filled exit animation still applied from the last
  // dismiss, so the entrance starts from a clean state.
  el.getAnimations?.().forEach((a) => a.cancel());
  const { keyframes, durationMs } = popInKeyframes(token, opts);
  // `fill: "forwards"` holds the final (opacity 1) frame so there's no flash back
  // to the primed opacity:0 between the animation finishing and `settle` running.
  const anim = el.animate(keyframes, { duration: durationMs, easing: "linear", fill: "forwards" });
  anim.addEventListener(
    "finish",
    () => {
      settle();
      anim.cancel();
    },
    { once: true },
  );
}

/** Duration (ms) of the dismiss animation — short + within the feedback budget,
 *  shorter than the springy entrance (exits accelerate away). */
export const EXIT_MS = 130;

// ── CRT TV power-on / power-off (the popup's signature open/close) ────────────
// Modeled on zauberkoch's cathode-tube animation: the shell powers ON like a
// picture tube — a bright dot blooms, snaps out into a horizontal scanline, then
// the picture stretches vertically out of the line with a phosphor flash that
// settles — and powers OFF in reverse (collapse to the scanline, run to a dot,
// burn out). Runs on the shell element via WAAPI (scaleX/scaleY + brightness
// about its centre); deliberate cathode easings, not springs. `prefers-
// reduced-motion` → the caller skips it (instant show / plain settle).

/** Duration (ms) of the TV power-on. */
/// Duration (ms) of the TV power-on.
///
/// PERCEIVED-LATENCY BUDGET (v0.107.0). The native window is on screen ~20 ms
/// after the hotkey (measured: 17–38 ms for position+show+focus), so what the
/// user experiences as "the popup is slow" was almost entirely this animation:
/// at 440 ms with the old curve the shell was a 1 %-height scanline for the
/// first 150 ms and only reached full height at ~360 ms. Nothing was readable
/// until then, even though the search field already accepted keystrokes.
///
/// The rule now: **legible before ~120 ms** (inside the window where a UI still
/// reads as instant), with only the phosphor settle running past it. The CRT
/// identity — dot → scanline → picture — is unchanged, it just plays at the
/// speed of a real tube instead of a slow-motion one.
export const CRT_ON_MS = 190;
/// Duration (ms) of the TV power-off — the popup stays visible this long on
/// close, because `hidePopup` AWAITS it before the hide IPC. It must stay
/// below `CRT_ON_MS` (pinned by a test): once you have decided to leave, any
/// wait is pure cost. Halved alongside the power-on in v0.107.0.
export const CRT_OFF_MS = 150;
/** Residual scale for the "scanline runs to a dot" pinch. */
const CRT_DOT_X = 0.02;
const CRT_DOT_Y = 0.012;

/** The collapsed START state (a bright dot at the shell's centre). Applied to
 *  the shell WHILE the popup is hidden so the OS `show()` — which reveals the
 *  window before the JS handler runs — shows the dark tube + dot, never a flash
 *  of full-opacity content, before `playCrtOn` blooms it. */
export function primeCrtHidden(el: HTMLElement | null): void {
  if (!el) return;
  setCrtScrollbarsHidden(true); // keep the scrollbar hidden from prime through power-on
  el.style.opacity = "1";
  el.style.transform = `scaleX(${CRT_DOT_X}) scaleY(${CRT_DOT_Y})`;
  el.style.filter = "brightness(2.6)";
}

/** Hide/show the (custom) scrollbar thumb for the CRT scale — a scaled +
 *  brightness-filtered scrollbar renders oddly at the shell's edge. Reflow-free
 *  (only the thumb colour changes; the gutter width is untouched). */
function setCrtScrollbarsHidden(on: boolean): void {
  if (typeof document !== "undefined") {
    document.documentElement.classList.toggle("crt-anim", on);
  }
}

function clearCrtStyle(el: HTMLElement): void {
  el.style.opacity = "";
  el.style.transform = "";
  el.style.filter = "";
  setCrtScrollbarsHidden(false);
}

/**
 * TV power-ON: dot → scanline → picture. No-op-with-settle when motion is
 * unavailable / reduced (the shell is left at its natural state so a primed dot
 * can't get stuck). Re-triggers on every open like `playEntrance`.
 */
export function playCrtOn(el: HTMLElement | null): void {
  if (!el) return;
  const settle = () => clearCrtStyle(el);
  if (typeof el.animate !== "function" || prefersReducedMotion()) {
    settle();
    return;
  }
  setCrtScrollbarsHidden(true); // hidden through the power-on; `settle` restores
  // Drop any forwards-filled power-off still applied from the last dismiss.
  el.getAnimations?.().forEach((a) => a.cancel());
  const anim = el.animate(
    [
      // Offsets are front-loaded: the width snap costs 22 % (~42 ms) and full
      // height lands at 62 % (~118 ms) — everything after that is the
      // brightness settle, which you can already read through. The old curve
      // spent 34 % on the scanline and only opened up at 82 %.
      { transform: `scaleX(${CRT_DOT_X}) scaleY(${CRT_DOT_Y})`, filter: "brightness(2.6)", opacity: 1, offset: 0, easing: "cubic-bezier(0.2, 0.85, 0.25, 1)" },
      { transform: `scaleX(1) scaleY(${CRT_DOT_Y})`, filter: "brightness(2.2)", opacity: 1, offset: 0.22, easing: "cubic-bezier(0.12, 0.7, 0.3, 1)" },
      { transform: "scaleX(1) scaleY(1.045)", filter: "brightness(1.2)", opacity: 1, offset: 0.62, easing: "ease-out" },
      { transform: "scaleX(1) scaleY(1)", filter: "brightness(1)", opacity: 1, offset: 1 },
    ],
    { duration: CRT_ON_MS, easing: "linear", fill: "forwards" },
  );
  anim.addEventListener(
    "finish",
    () => {
      settle();
      anim.cancel();
    },
    { once: true },
  );
}

/**
 * TV power-OFF: picture → scanline → dot → burnout. Resolves when finished (so
 * the caller hides the OS window only after it played), or immediately when
 * motion is unavailable / reduced. `fill: "forwards"` holds the burnt-out frame
 * until the window is actually hidden; `playCrtOn` cancels it on the next open.
 */
export function playCrtOff(el: HTMLElement | null): Promise<void> {
  if (!el || typeof el.animate !== "function" || prefersReducedMotion()) {
    return Promise.resolve();
  }
  setCrtScrollbarsHidden(true); // hide the scrollbar for the whole collapse
  el.getAnimations?.().forEach((a) => a.cancel());
  const anim = el.animate(
    [
      { transform: "scaleX(1) scaleY(1)", filter: "brightness(1)", opacity: 1, offset: 0, easing: "cubic-bezier(0.5, 0, 0.85, 0.35)" },
      { transform: `scaleX(1) scaleY(${CRT_DOT_Y})`, filter: "brightness(2.4)", opacity: 1, offset: 0.55, easing: "ease-in" },
      { transform: `scaleX(1.04) scaleY(${CRT_DOT_Y})`, filter: "brightness(2.8)", opacity: 1, offset: 0.72, easing: "cubic-bezier(0.55, 0, 0.85, 0.35)" },
      { transform: `scaleX(${CRT_DOT_X}) scaleY(${CRT_DOT_Y})`, filter: "brightness(3)", opacity: 0, offset: 1 },
    ],
    { duration: CRT_OFF_MS, easing: "linear", fill: "forwards" },
  );
  // Restore the scrollbar once collapsed (the window hides right after; the next
  // open's prime re-hides it). Keeps the class from leaking if the popup lingers.
  return anim.finished
    .then(() => setCrtScrollbarsHidden(false))
    .catch(() => setCrtScrollbarsHidden(false));
}

/**
 * Play the dismiss animation — the **reverse of [`playEntrance`]**: the panel
 * accelerates away (fades out while dropping + scaling down) on the MD3
 * emphasized-accelerate curve. Resolves when the animation finishes (so the
 * caller can hide the OS window only after it has played), or **immediately**
 * when motion is unavailable / reduced (no added dismiss latency then).
 *
 * Uses `fill: "forwards"` so the panel stays invisible until the window is
 * actually hidden — `playEntrance` cancels it on the next summon.
 */
export function playExit(
  el: HTMLElement | null,
  opts: { toScale?: number; dropY?: number } = {},
): Promise<void> {
  if (!el || typeof el.animate !== "function" || prefersReducedMotion()) {
    return Promise.resolve();
  }
  el.getAnimations?.().forEach((a) => a.cancel());
  const toScale = opts.toScale ?? 0.96;
  const dropY = opts.dropY ?? 8;
  const anim = el.animate(
    [
      { opacity: 1, transform: "translateY(0px) scale(1)" },
      { opacity: 0, transform: `translateY(${dropY}px) scale(${toScale})` },
    ],
    { duration: EXIT_MS, easing: "cubic-bezier(0.3, 0, 0.8, 0.15)", fill: "forwards" },
  );
  return anim.finished.then(() => undefined).catch(() => undefined);
}
