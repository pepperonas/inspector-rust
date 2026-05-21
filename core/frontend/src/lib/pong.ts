/**
 * Pure, testable game logic for the `getshaky` Pong easter egg.
 *
 * The stateful canvas render loop lives in `components/PongGame.tsx`;
 * everything here is side-effect-free so it can be unit-tested without
 * a DOM / canvas / requestAnimationFrame.
 */

/** First side to this many points wins the match. */
export const WIN_SCORE = 5;

/** Field + entity geometry, in logical pixels. The canvas is scaled to
 *  the popup's content area; all game maths uses these logical units. */
export const PADDLE_W = 12;
export const PADDLE_H = 76;
export const BALL_R = 8;
/** Inset of each paddle from its side wall. */
export const PADDLE_INSET = 24;

/** Player paddle travel speed when driven by the arrow / W-S keys
 *  (logical px per frame). Mouse control sets the position directly. */
export const PLAYER_KEY_SPEED = 7;

/** Ball speed: starts here, gains a little on every paddle hit so a
 *  long rally gets progressively tenser, capped so it stays playable. */
export const BALL_BASE_SPEED = 6;
export const BALL_SPEED_GAIN = 0.4;
export const BALL_MAX_SPEED = 12;

/** Clamp `v` into the inclusive `[lo, hi]` range. */
export function clamp(v: number, lo: number, hi: number): number {
  if (v < lo) return lo;
  if (v > hi) return hi;
  return v;
}

/**
 * Bot paddle's maximum tracking speed for a given bot score — the
 * "ramp-up" difficulty. Starts fair and beatable, gains a little with
 * every point the bot scores, so a 0–4 deficit genuinely tightens.
 *
 *   botScore 0 → 4.5    botScore 2 → 6.0    botScore 4 → 7.5
 */
export function botMaxSpeed(botScore: number): number {
  return 4.5 + clamp(botScore, 0, WIN_SCORE) * 0.75;
}

/** Next ball speed after a paddle hit — bumped by `BALL_SPEED_GAIN`,
 *  capped at `BALL_MAX_SPEED`. */
export function nextBallSpeed(current: number): number {
  return Math.min(current + BALL_SPEED_GAIN, BALL_MAX_SPEED);
}

/** A 2-D velocity. */
export interface Velocity {
  vx: number;
  vy: number;
}

/**
 * Velocity of the ball after it bounces off a paddle.
 *
 * `offset` is where the ball struck the paddle, normalised to
 * `[-1, +1]` (−1 = top edge, 0 = centre, +1 = bottom edge). The
 * vertical component is derived from that offset so the player can
 * "aim" by hitting with the paddle's edge — classic Pong feel.
 *
 * `dirX` is the new horizontal sign (+1 → moving right, −1 → left).
 * `speed` is the post-hit ball speed magnitude.
 */
export function paddleBounce(offset: number, dirX: 1 | -1, speed: number): Velocity {
  const clamped = clamp(offset, -1, 1);
  // Max deflection ≈ 60° from horizontal.
  const maxAngle = Math.PI / 3;
  const angle = clamped * maxAngle;
  return {
    vx: dirX * speed * Math.cos(angle),
    vy: speed * Math.sin(angle),
  };
}

/**
 * Serve a fresh ball from the centre of a `fieldW × fieldH` field.
 * `towardPlayer` decides which side it heads to (true → left/player).
 * The vertical component is a shallow random angle so serves vary.
 */
export function serveBall(
  fieldW: number,
  fieldH: number,
  towardPlayer: boolean,
  rng: () => number = Math.random,
): { x: number; y: number } & Velocity {
  // Shallow serve angle: ±~25° so it's never a vertical-ish lob.
  const angle = (rng() - 0.5) * (Math.PI / 3.5);
  const dirX = towardPlayer ? -1 : 1;
  return {
    x: fieldW / 2,
    y: fieldH / 2,
    vx: dirX * BALL_BASE_SPEED * Math.cos(angle),
    vy: BALL_BASE_SPEED * Math.sin(angle),
  };
}
