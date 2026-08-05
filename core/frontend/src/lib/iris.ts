/**
 * Pure helpers for the `iris` command — mic-triggered red screen vignette.
 *
 * Threshold unit is **SPL on the disco-controller convention** (`spl = dBFS +
 * 90`), so a value that works on the raspi5 dB-analysis page means the same
 * thing here. The clamp band mirrors `iris.rs` exactly; when you change one,
 * change the other (`threshold_is_clamped_into_the_supported_band` guards the
 * Rust side, `clampThreshold` the TS side).
 */

/** Mirrors `iris.rs::MIN_THRESHOLD_SPL` / `MAX_THRESHOLD_SPL`. */
export const MIN_THRESHOLD_SPL = 30;
export const MAX_THRESHOLD_SPL = 100;
/** Mirrors `iris.rs::DEFAULT_THRESHOLD_SPL` (= raspi5 `warn_thr`). */
export const DEFAULT_THRESHOLD_SPL = 55;

/** Display range of the calibration meter. Wider than the threshold band at
 *  the bottom: a silent room reads about -30 SPL, and pinning the bar to 30
 *  would make silence look like "almost at the threshold". */
export const METER_MIN_SPL = 0;
export const METER_MAX_SPL = 100;

/** What a typed `iris …` argument asks for. */
export type IrisArg =
  | { kind: "toggle" }
  | { kind: "off" }
  | { kind: "on"; threshold: number };

/**
 * Parse the argument of `iris [spl]`.
 *
 * - `""` → **toggle**: arming and disarming from one keyword is what makes the
 *   command feel like a switch rather than two commands.
 * - `"0"` (or any non-positive number) → **off**. 0 is free to repurpose as the
 *   off sentinel because a 0 SPL threshold would mean "always lit", which is
 *   never something anyone wants.
 * - a positive number → **on** at that threshold, clamped into the band.
 * - anything unparseable → toggle, so a typo never silently arms at a wrong
 *   level.
 *
 * Accepts a comma decimal separator (`iris 57,5`) like the `bruno` parser —
 * this is a German-keyboard app and the numeric keypad's decimal key is a comma.
 */
export function parseIrisArg(arg: string): IrisArg {
  const raw = (arg ?? "").trim();
  if (raw === "") return { kind: "toggle" };
  const n = Number(raw.replace(",", "."));
  if (!Number.isFinite(n)) return { kind: "toggle" };
  if (n <= 0) return { kind: "off" };
  return { kind: "on", threshold: clampThreshold(n) };
}

/** Clamp a threshold into the supported band. Mirrors `iris.rs::clamp_threshold`. */
export function clampThreshold(spl: number): number {
  if (!Number.isFinite(spl)) return DEFAULT_THRESHOLD_SPL;
  return Math.min(MAX_THRESHOLD_SPL, Math.max(MIN_THRESHOLD_SPL, spl));
}

/** Position of an SPL value on the meter, as a 0..1 fraction (clamped). */
export function meterFraction(spl: number): number {
  if (!Number.isFinite(spl)) return 0;
  const f = (spl - METER_MIN_SPL) / (METER_MAX_SPL - METER_MIN_SPL);
  return f < 0 ? 0 : f > 1 ? 1 : f;
}

/** One decimal, matching how raspi5 rounds before comparing (`round(spl,1)`). */
export function formatSpl(spl: number): string {
  if (!Number.isFinite(spl)) return "—";
  return spl.toFixed(1);
}

/** One drifting light blob of the vignette. All positions/sizes are in **%
 *  of the viewport**, so the same description works on any monitor. */
export type IrisBlob = {
  /** Anchor, % of viewport. */
  x: number;
  y: number;
  /** Size, % of viewport. */
  w: number;
  h: number;
  /** Where it sits while dark — pushed outward, so arming pulls the light
   *  inward like an aperture closing and disarming lets it drift back out. */
  ox: number;
  oy: number;
  /** Continuous drift target (%) + scale, the "fluid" part. */
  dx: number;
  dy: number;
  ds: number;
  /** Drift period / phase offset, seconds. */
  dur: number;
  del: number;
  /** Gaussian blur, px — what makes neighbouring blobs melt into each other. */
  blur: number;
  /** Hotter palette (orange core) for a minority, so the rim isn't one flat red. */
  hot: boolean;
};

/** Anchors: the four corners get the big lights, the four edge midpoints the
 *  smaller ones. Deliberately NOT a symmetric border — the ask was explicitly
 *  "random at the corners, not fixed red edges". */
const ANCHORS: ReadonlyArray<readonly [number, number, number]> = [
  // [x%, y%, size weight]
  [0, 0, 1.0],
  [100, 0, 1.0],
  [0, 100, 1.0],
  [100, 100, 1.0],
  [50, 0, 0.7],
  [50, 100, 0.7],
  [0, 50, 0.72],
  [100, 50, 0.72],
];

/**
 * Build the blob field.
 *
 * `rand` is injected so this is deterministic under test; the component passes
 * `Math.random`. Each monitor is its own window and therefore its own draw, so
 * no two screens (and no two arms) get the same arrangement — that irregularity
 * *is* the randomness the effect is after.
 *
 * The drift periods are deliberately spread over an irrational-ish range and
 * never shared: with mutually incommensurate periods the composite motion has
 * no visible loop point, which is what makes it read as organic rather than as
 * a looping animation.
 */
export function makeBlobs(rand: () => number): IrisBlob[] {
  const jitter = (amount: number) => (rand() * 2 - 1) * amount;
  return ANCHORS.map(([ax, ay, weight], i) => {
    // Push the anchor around so corners never look stamped from a template.
    const x = ax + jitter(14);
    const y = ay + jitter(14);
    const base = 46 * weight;
    const w = base * (0.78 + rand() * 0.55);
    const h = base * (0.78 + rand() * 0.55);
    // Outward = away from the centre; that vector drives both the entrance
    // (rush in) and the exit (drift back out).
    const outX = x < 50 ? -1 : 1;
    const outY = y < 50 ? -1 : 1;
    return {
      x,
      y,
      w,
      h,
      ox: outX * (10 + rand() * 12),
      oy: outY * (10 + rand() * 12),
      dx: jitter(9),
      dy: jitter(9),
      ds: 0.86 + rand() * 0.42,
      // 5.1 s .. ~12 s, each blob its own — see the note above.
      dur: 5.1 + i * 0.83 + rand() * 1.7,
      del: rand() * -6,
      // Modest on purpose: the radial gradients are already soft, and eight
      // large blurred layers per monitor is real GPU work for little extra
      // look. This only has to melt the overlaps together.
      blur: 18 + rand() * 26,
      hot: rand() < 0.4,
    };
  });
}

/** What pressing Enter on an `iris …` row should actually do. */
export type IrisAction =
  | { kind: "arm"; threshold?: number }
  | { kind: "retune"; threshold: number }
  | { kind: "disarm" }
  | { kind: "none" };

/**
 * Resolve a typed argument against the **live** armed state.
 *
 * Kept pure and separate from `App.tsx` because this is where the toggle got
 * it wrong once already (v0.102.1: `iris 55` then `iris` left it armed — the
 * panel had taken keyboard focus, so the second Enter never reached the
 * dispatcher at all). The rules:
 *
 * - a bare `iris` is a **toggle**, so it is the one input whose meaning
 *   depends on the current state;
 * - `iris 0` only ever disarms, and is a no-op when already off;
 * - an explicit number never disarms — it arms, or retunes a running session.
 *   Disarming on a number would make `iris 55` unusable as "set it to 55".
 *
 * `active` must come from the **backend** (`iris_status`), not from React
 * state: the popup can be reopened long after the session was armed, and an
 * external change must not desync the toggle.
 */
export function irisAction(arg: IrisArg, active: boolean): IrisAction {
  switch (arg.kind) {
    case "off":
      return active ? { kind: "disarm" } : { kind: "none" };
    case "toggle":
      return active ? { kind: "disarm" } : { kind: "arm" };
    case "on":
      return active
        ? { kind: "retune", threshold: arg.threshold }
        : { kind: "arm", threshold: arg.threshold };
  }
}

// ─── Bursts ──────────────────────────────────────────────────────────────
//
// The punch of the raspi5 dB-analysis page does NOT come from its background
// wash — that layer is static there (its `iris-breathe` keyframes are never
// defined). It comes from `spawnIrisWord()`: discrete impulses fired on a
// randomised gap, each snapping in hard, holding, then fading while it grows.
// These helpers are that mechanism, minus the lettering.

/** The reference's `IRIS_GAP_MIN`/`IRIS_GAP_MAX` — the cadence right at the
 *  threshold. Its own comment: "a fixed beat reads mechanical … wide spread,
 *  so it never settles into a rhythm". */
export const BURST_GAP_CALM: readonly [number, number] = [1700, 4800];
/** Cadence when the level is far over. The reference has no such mode — it
 *  fires blind — this is the level-reactive extension. */
export const BURST_GAP_LOUD: readonly [number, number] = [420, 1200];
/** dB above the threshold that counts as "as loud as it gets". */
export const BURST_SPAN_DB = 25;
/** Concurrent burst cap. The reference uses 2 and — importantly — counts the
 *  live elements from the DOM rather than from a counter: a counter drifts the
 *  moment one `animationend` fails to arrive (a throttled tab is enough) and
 *  then blocks spawning forever, silently. The React port keeps the same
 *  property by capping on the length of the live array. */
export const BURST_MAX = 3;
/** The reference's `IRIS_TINTS` — deliberately warm-graded (red → salmon →
 *  amber → near-white), which is what keeps it from reading as one flat red. */
export const BURST_TINTS = ["#ff5252", "#ff8a80", "#ffd684", "#fff1ee"] as const;

/** How deep into the screen (in % of the viewport) an impulse may sit from
 *  its nearest edge. Everything inside the remaining centre box stays clear —
 *  that is the usability guarantee, and it is unit-tested. */
export const BURST_EDGE_DEPTH = 24;

export type IrisBurst = {
  id: number;
  /** Position, % of viewport — always inside the edge band, see `edgePosition`. */
  x: number;
  y: number;
  /** Long-axis length in vmax. */
  size: number;
  /** Short axis as a fraction of `size`. A streak is a thin sliver. */
  aspect: number;
  /** Animation duration, seconds. */
  life: number;
  /** Peak opacity of the hold phase. */
  peak: number;
  /** Base rotation, deg. */
  rot: number;
  /** Slow rotation drift over the impulse's life, deg — a still flare reads
   *  frozen; a slightly turning one reads alive. */
  rotDrift: number;
  tint: string;
  /** Overlapping gradient lobes — irregular silhouette WITHOUT hard edges. */
  lobes: BurstLobe[];
  /** Feathering blur in px. The gradients do the softness; this only smooths. */
  blur: number;
  /** A thin, fast sliver rather than a broad flare. */
  streak: boolean;
};

/** One soft gradient lobe of an impulse, in % of the element box. */
export type BurstLobe = { cx: number; cy: number; r: number };

/**
 * A position inside the edge band: uniform along the perimeter, quadratically
 * biased toward the rim (`rand()² · depth` — most impulses hug the edge, a few
 * reach further in, none ever past [`BURST_EDGE_DEPTH`]). This replaces the
 * v0.102.2 full-screen spread: the impulses now live where the constant field
 * lives, and the middle of the screen stays genuinely clear.
 */
export function edgePosition(rand: () => number): { x: number; y: number } {
  const side = (rand() * 4) | 0;
  const along = 4 + rand() * 92;
  const depth = rand() * rand() * BURST_EDGE_DEPTH;
  switch (side) {
    case 0:
      return { x: along, y: depth };
    case 1:
      return { x: along, y: 100 - depth };
    case 2:
      return { x: depth, y: along };
    default:
      return { x: 100 - depth, y: along };
  }
}

/**
 * The lobes of one impulse: a main body at the centre plus up to two smaller
 * satellites at random offsets. Overlapping soft gradients give an irregular
 * outline without a single hard edge — this is what replaced the v0.102.2
 * `clip-path` shards ("nicht so harte Kanten, eher einen Verlauf"). A streak
 * stays single-lobed; it is already a sliver.
 */
export function makeBurstLobes(rand: () => number, streak: boolean): BurstLobe[] {
  const lobes: BurstLobe[] = [{ cx: 50, cy: 50, r: 52 + rand() * 12 }];
  if (streak) return lobes;
  const extra = (rand() * 3) | 0; // 0..2 satellites
  for (let i = 0; i < extra; i++) {
    lobes.push({ cx: 22 + rand() * 56, cy: 22 + rand() * 56, r: 18 + rand() * 22 });
  }
  return lobes;
}

/** How far over the threshold we are, as 0..1 over [`BURST_SPAN_DB`] dB. */
export function burstIntensity(
  spl: number,
  threshold: number,
  span: number = BURST_SPAN_DB,
): number {
  if (!Number.isFinite(spl) || !Number.isFinite(threshold) || span <= 0) return 0;
  const over = (spl - threshold) / span;
  return over < 0 ? 0 : over > 1 ? 1 : over;
}

/** Gap until the next burst. Interpolates the calm (reference) cadence toward
 *  the loud one, then picks randomly inside the resulting window — so it stays
 *  irregular at every volume instead of turning into a metronome. */
export function burstGapMs(intensity: number, rand: () => number): number {
  const t = intensity < 0 ? 0 : intensity > 1 ? 1 : intensity;
  const lo = BURST_GAP_CALM[0] + (BURST_GAP_LOUD[0] - BURST_GAP_CALM[0]) * t;
  const hi = BURST_GAP_CALM[1] + (BURST_GAP_LOUD[1] - BURST_GAP_CALM[1]) * t;
  return lo + rand() * (hi - lo);
}

/** One impulse. Ranges follow the reference (`size` 0.13–0.39 of the viewport,
 *  `life` 1.15–2.10 s, `peak` 0.30–0.54, `rot` ±20°), scaled by how far over
 *  the threshold we are: louder means bigger, brighter and a little shorter,
 *  which is what makes a loud room feel urgent rather than merely busy. */
export function makeBurst(id: number, intensity: number, rand: () => number): IrisBurst {
  const t = intensity < 0 ? 0 : intensity > 1 ? 1 : intensity;
  // A third of the impulses are thin, fast slivers. Mixing two silhouettes is
  // what keeps a volley from looking like one effect repeating.
  const streak = rand() < 0.36;
  const { x, y } = edgePosition(rand);
  return {
    id,
    x,
    y,
    size: (22 + rand() * 30) * (0.92 + t * 0.4) * (streak ? 1.35 : 1),
    aspect: streak ? 0.08 + rand() * 0.12 : 0.55 + rand() * 0.35,
    // Shorter than the reference's 1.15–2.10 s on purpose — the fade-out is
    // most of what lingers over the screen, and it was asked to go faster.
    life: (streak ? 0.42 + rand() * 0.3 : 0.9 + rand() * 0.7) * (1 - t * 0.28),
    peak: Math.min(0.82, 0.3 + rand() * 0.2 + t * 0.24),
    rot: rand() * 360,
    rotDrift: (rand() * 2 - 1) * 14,
    tint: BURST_TINTS[Math.min(BURST_TINTS.length - 1, (rand() * BURST_TINTS.length) | 0)],
    lobes: makeBurstLobes(rand, streak),
    // Small: the gradients already carry the softness, this only feathers.
    blur: streak ? 1.5 + rand() * 2.5 : 3 + rand() * 5,
    streak,
  };
}

/** Label for the command row — the row has to say which way it will flip. */
export function irisRowLabel(arg: IrisArg, active: boolean): string {
  switch (arg.kind) {
    case "off":
      return active ? "Iris ausschalten" : "Iris ist bereits aus";
    case "on":
      return active
        ? `Iris auf ${formatSpl(arg.threshold)} dB setzen`
        : `Iris bei ${formatSpl(arg.threshold)} dB scharfschalten`;
    case "toggle":
      return active ? "Iris ausschalten" : "Iris scharfschalten";
  }
}
