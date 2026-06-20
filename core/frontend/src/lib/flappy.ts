/**
 * Pure, testable game logic for the `learningtofly` Flappy-Bird easter egg.
 *
 * Faithful to the original (Dong Nguyen, 2013): a fixed-x bird with vertical
 * velocity only; gravity accelerates it downward every frame; a flap sets a
 * fixed upward impulse. Pipe pairs (a fixed-height gap) scroll left at a
 * constant speed and spawn at a fixed horizontal spacing with a randomised
 * gap position. Passing a pipe scores +1. Touching a pipe or the ground ends
 * the run; the ceiling is a clamp (the bird can't leave the top — no death,
 * matching the original). Physics is frame-rate-independent via `frameScale`.
 *
 * The stateful canvas render loop lives in `components/FlappyGame.tsx`.
 */

export const INTRO_MS = 1500;
export const REFERENCE_FRAME_MS = 1000 / 60;

/** dt → 60fps-frame units, clamped so a hitch can't teleport the bird. */
export function frameScale(dtMs: number): number {
  const scale = dtMs / REFERENCE_FRAME_MS;
  return scale > 2.5 ? 2.5 : scale < 0 ? 0 : scale;
}

export function clamp(n: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, n));
}

// ── Tunables (px and px/frame at 60fps) ─────────────────────────────────────
export const GRAVITY = 0.42; // downward acceleration per frame²
export const FLAP_VY = -7.4; // upward impulse applied on a flap
export const MAX_FALL_VY = 11; // terminal downward velocity
export const PIPE_SPEED = 2.8; // leftward scroll speed
export const PIPE_GAP = 150; // vertical opening between top + bottom pipe
export const PIPE_WIDTH = 60;
export const PIPE_SPACING = 250; // horizontal distance between pipe pairs
export const BIRD_X_FRAC = 0.28; // bird x as a fraction of the field width
export const BIRD_R = 13; // bird collision radius
export const GROUND_H = 28; // visual ground strip at the bottom
export const GAP_MARGIN = 38; // min distance of a gap from top / ground

export interface Pipe {
  /** Left edge x (moves leftward each step). */
  x: number;
  /** Top of the gap (y of the bottom edge of the top pipe). */
  gapTop: number;
  /** Whether the bird has already passed (scored) this pair. */
  scored: boolean;
}

export interface FlappyState {
  birdY: number;
  vy: number;
  pipes: Pipe[];
  score: number;
  dead: boolean;
  /** px scrolled since the last spawn — drives fixed-spacing spawning. */
  sinceSpawnX: number;
  /** Physics is frozen until the first flap (classic "tap to start"). */
  started: boolean;
}

/** The y of the ground surface (collision floor) for a field of height `h`. */
export function groundY(fieldH: number): number {
  return fieldH - GROUND_H;
}

/** The bird's fixed x for a field of width `w`. */
export function birdX(fieldW: number): number {
  return Math.round(fieldW * BIRD_X_FRAC);
}

/** A fresh run: bird hovering a little above centre, no pipes, not started.
 *  `sinceSpawnX` is primed to `PIPE_SPACING` so the first started step spawns
 *  a pipe at the right edge straight away (a ~3 s lead-in to react). */
export function initialState(fieldH: number): FlappyState {
  return {
    birdY: Math.round(fieldH * 0.42),
    vy: 0,
    pipes: [],
    score: 0,
    dead: false,
    sinceSpawnX: PIPE_SPACING,
    started: false,
  };
}

/** Apply a flap: fixed upward impulse, and start the run if idle. */
export function flap(s: FlappyState): void {
  if (s.dead) return;
  s.vy = FLAP_VY;
  s.started = true;
}

/** A random valid gap-top for a field of height `fieldH`. */
export function randGapTop(fieldH: number, rng: () => number = Math.random): number {
  const lo = GAP_MARGIN;
  const hi = groundY(fieldH) - PIPE_GAP - GAP_MARGIN;
  if (hi <= lo) return Math.max(lo, (groundY(fieldH) - PIPE_GAP) / 2);
  return Math.round(lo + rng() * (hi - lo));
}

/** Circle-vs-rect overlap (bird circle vs a pipe segment). */
function circleHitsRect(
  cx: number,
  cy: number,
  r: number,
  rx: number,
  ry: number,
  rw: number,
  rh: number,
): boolean {
  const nx = clamp(cx, rx, rx + rw);
  const ny = clamp(cy, ry, ry + rh);
  const dx = cx - nx;
  const dy = cy - ny;
  return dx * dx + dy * dy < r * r;
}

/** Does the bird collide with `pipe` (either the top or bottom segment)? */
export function hitsPipe(
  bx: number,
  by: number,
  r: number,
  pipe: Pipe,
  fieldH: number,
): boolean {
  // Top segment: from the field top down to gapTop.
  if (circleHitsRect(bx, by, r, pipe.x, 0, PIPE_WIDTH, pipe.gapTop)) return true;
  // Bottom segment: from gapTop+GAP down to the ground.
  const bottomY = pipe.gapTop + PIPE_GAP;
  return circleHitsRect(bx, by, r, pipe.x, bottomY, PIPE_WIDTH, groundY(fieldH) - bottomY);
}

/** Has the bird touched the ground? */
export function hitsGround(by: number, r: number, fieldH: number): boolean {
  return by + r >= groundY(fieldH);
}

// ── AI autopilot (the "learning to fly" easter egg) ─────────────────────────
// Engaged when the player holds the flap key as the bird hits the ground: the
// bird becomes `invincible` and this controller keeps it threading the gaps
// forever. Pure so it's unit-testable.

/** Target y the autopilot aims for: the centre of the next gap ahead of the
 *  bird, or mid-field when no pipe is in range. */
export function aiTargetY(s: FlappyState, fieldW: number, fieldH: number): number {
  const bx = birdX(fieldW);
  let target = fieldH * 0.45;
  let best = Infinity;
  for (const p of s.pipes) {
    if (p.x + PIPE_WIDTH > bx - 6) {
      const ahead = p.x - bx;
      if (ahead < best) {
        best = ahead;
        target = p.gapTop + PIPE_GAP / 2;
      }
    }
  }
  return target;
}

/** Autopilot flap decision: flap once the bird has sunk past the target and
 *  isn't already rising — a stable sawtooth around `targetY`. */
export function aiShouldFlap(birdY: number, vy: number, targetY: number): boolean {
  return birdY > targetY && vy > -1.5;
}

/**
 * Advance one frame. Mutates and returns `s`. `dt` is the frame-scale from
 * `frameScale`; `nextGapTop` is the gap-top to use **iff** a pipe spawns this
 * step (the caller supplies a fresh random each call — cheap, and keeps the
 * function deterministic for tests). No-op while the run hasn't started or is
 * already dead.
 */
export function step(
  s: FlappyState,
  fieldW: number,
  fieldH: number,
  dt: number,
  nextGapTop: number,
  invincible = false,
): FlappyState {
  if (!s.started || s.dead) return s;

  // Gravity + integrate vertical position.
  s.vy = Math.min(s.vy + GRAVITY * dt, MAX_FALL_VY);
  s.birdY += s.vy * dt;

  // Ceiling: flying into the top edge ends the run (v0.84.70). When
  // `invincible` (the AI autopilot), it just clamps so the bird stays on-screen.
  if (s.birdY < BIRD_R) {
    s.birdY = BIRD_R;
    if (invincible) {
      if (s.vy < 0) s.vy = 0;
    } else {
      s.dead = true;
      return s;
    }
  }

  // Scroll pipes left + spawn at fixed spacing.
  const dx = PIPE_SPEED * dt;
  for (const p of s.pipes) p.x -= dx;
  s.sinceSpawnX += dx;
  if (s.sinceSpawnX >= PIPE_SPACING) {
    s.sinceSpawnX -= PIPE_SPACING;
    s.pipes.push({ x: fieldW, gapTop: nextGapTop, scored: false });
  }
  // Drop pipes that have fully exited on the left.
  s.pipes = s.pipes.filter((p) => p.x + PIPE_WIDTH > -2);

  // Score: a pair counts once its right edge passes the bird's x.
  const bx = birdX(fieldW);
  for (const p of s.pipes) {
    if (!p.scored && p.x + PIPE_WIDTH < bx) {
      p.scored = true;
      s.score += 1;
    }
  }

  // Collisions → death (the AI autopilot is `invincible`: it clamps off the
  // ground and passes through pipes, flying forever).
  if (hitsGround(s.birdY, BIRD_R, fieldH)) {
    s.birdY = groundY(fieldH) - BIRD_R;
    if (invincible) {
      if (s.vy > 0) s.vy = 0;
    } else {
      s.dead = true;
      return s;
    }
  }
  if (!invincible) {
    for (const p of s.pipes) {
      if (hitsPipe(bx, s.birdY, BIRD_R, p, fieldH)) {
        s.dead = true;
        return s;
      }
    }
  }
  return s;
}
