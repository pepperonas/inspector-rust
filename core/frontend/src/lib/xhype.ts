/**
 * `x!` — timeline + maths for the full-screen spectacle (v0.133.0).
 *
 * Six acts, ~15 s, one canvas, one rAF. Everything deterministic and
 * allocation-free in the hot path; this module holds the PURE parts so the
 * choreography is testable without a GPU:
 *
 *   I  IGNITION  a single ember wakes in the black
 *   II GRID      a technocratic horizon rushes the viewer
 *   III SLOP     the feed floods — glyph rain, corrupted text, chroma tear
 *   IV BURN      it all catches fire
 *   V  NOVA      white collapse → supernova → warp into space
 *   VI VOID      the stars go out
 *
 * ⚠️ **Photosensitivity is a real hazard here**, not a nicety: this piece is
 * meant to be violent, so `FLASH_MIN_GAP_MS` keeps FULL-SCREEN flashes below
 * the WCAG 2.3.1 three-per-second threshold. Localised sparks, embers and
 * scanline jitter are unconstrained — it's the whole-field luminance jumps
 * that trigger seizures. Never lower that gap to "make it punchier".
 */

export interface XAct {
  key: "ignition" | "grid" | "slop" | "burn" | "nova" | "void";
  /** Act start, ms from the beginning. */
  at: number;
  /** Duration, ms. */
  dur: number;
  /** Shown in the corner readout — the piece narrates itself. */
  caption: string;
}

export const ACTS: readonly XAct[] = [
  { key: "ignition", at: 0, dur: 1800, caption: "I · ZÜNDUNG" },
  { key: "grid", at: 1800, dur: 3200, caption: "II · RASTER" },
  { key: "slop", at: 5000, dur: 3200, caption: "III · SLOP" },
  { key: "burn", at: 8200, dur: 3200, caption: "IV · BRAND" },
  { key: "nova", at: 11400, dur: 2000, caption: "V · NOVA" },
  { key: "void", at: 13400, dur: 1800, caption: "VI · LEERE" },
];

/** Total runtime in ms. */
export const X_DURATION = ACTS[ACTS.length - 1].at + ACTS[ACTS.length - 1].dur;

/** Minimum gap between full-screen flashes — WCAG 2.3.1 allows at most three
 *  per second; 340 ms is just under that, deliberately. */
export const FLASH_MIN_GAP_MS = 340;

/** Which act is playing at `t` ms, and how far through it we are (0..1).
 *  Before the start → the first act at 0; past the end → `null`. */
export function actAt(t: number): { act: XAct; local: number; index: number } | null {
  if (t >= X_DURATION) return null;
  const clamped = Math.max(0, t);
  for (let i = ACTS.length - 1; i >= 0; i--) {
    const a = ACTS[i];
    if (clamped >= a.at) return { act: a, local: (clamped - a.at) / a.dur, index: i };
  }
  return { act: ACTS[0], local: 0, index: 0 };
}

/** May a full-screen flash fire now? Guards the photosensitivity threshold. */
export function flashAllowed(lastFlashAt: number | null, now: number): boolean {
  return lastFlashAt === null || now - lastFlashAt >= FLASH_MIN_GAP_MS;
}

export const clamp01 = (x: number): number => (x < 0 ? 0 : x > 1 ? 1 : x);
export const easeOut = (x: number): number => 1 - Math.pow(1 - clamp01(x), 3);
export const easeIn = (x: number): number => Math.pow(clamp01(x), 3);
/** 0 → 1 → 0 across the unit interval; the shape of every stab and flare. */
export const arc = (x: number): number => Math.sin(Math.PI * clamp01(x));

/** Deterministic hash noise in [0,1) — no `Math.random` in the render loop, so
 *  a frame can be reproduced (and tested). */
export function noise(i: number, seed = 1): number {
  let h = Math.imul(i ^ seed, 0x27d4eb2d) >>> 0;
  h ^= h >>> 15;
  h = Math.imul(h, 0x85ebca6b) >>> 0;
  h ^= h >>> 13;
  return (h >>> 0) / 4294967296;
}

/** Perspective projection for the rushing grid: a line at depth `z` (0 = at
 *  the viewer, 1 = at the horizon) maps to a screen y. */
export function horizonY(z: number, h: number, horizon: number): number {
  const d = Math.max(0.0001, 1 - clamp01(z));
  return horizon + (h - horizon) * d * d;
}

/** One warp star's radial position after `p` (0..1) of its flight. */
export function warpRadius(p: number, maxR: number): number {
  return easeIn(p) * maxR;
}

/** The palette. Black ground, ember heat, technocrat cold, white as
 *  punctuation only. */
export const PALETTE = {
  ember: "#ff3b0f",
  flame: "#ff8c1a",
  gold: "#ffc857",
  cold: "#7c3aed",
  cyan: "#22d3ee",
  bone: "#e8e4dc",
} as const;

/** Words that stab across the screen, per act. Deliberately abstract — the
 *  piece is about the FEELING (acceleration, noise, collapse), not a quote. */
export const WORDS: Readonly<Record<XAct["key"], readonly string[]>> = {
  ignition: ["X"],
  grid: ["SCHNELLER", "MEHR", "JETZT"],
  slop: ["SLOP", "RAUSCHEN", "MEHR", "MEHR", "MEHR"],
  burn: ["ALLES BRENNT", "KAPUTT"],
  nova: ["X!"],
  void: ["…"],
};

/** Corrupt a word with combining marks — the "AI slop" texture. `amount` 0..1
 *  scales how many marks pile on. Pure + deterministic. */
export function corrupt(word: string, amount: number, seed = 1): string {
  const MARKS = ["́", "̰", "҉", "͓", "ͫ", "͈", "͜"];
  const n = Math.round(clamp01(amount) * 3);
  if (n === 0) return word;
  let out = "";
  let i = 0;
  for (const ch of word) {
    out += ch;
    if (ch !== " ") {
      for (let k = 0; k < n; k++) {
        out += MARKS[Math.floor(noise(i * 7 + k, seed) * MARKS.length)];
      }
    }
    i++;
  }
  return out;
}
