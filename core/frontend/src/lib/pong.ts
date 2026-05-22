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
 *  (logical px per 60 fps-frame). Mouse control sets the position
 *  directly. Multiplied by the frame-scale + the Shift boost. */
export const PLAYER_KEY_SPEED = 7;

/** Holding Shift while driving the paddle with keys multiplies its
 *  speed by this factor. */
export const SHIFT_SPEED_MULTIPLIER = 2;

/** Delay between losing a ball and the next serve (ms). Gives the
 *  player a beat to reposition. */
export const SERVE_DELAY_MS = 1000;

/** The reference frame duration the game's speed constants are tuned
 *  against — 60 fps. All per-frame movement is multiplied by
 *  [`frameScale`] so the game runs at the same wall-clock speed on a
 *  60 Hz, 120 Hz or 144 Hz display. */
export const REFERENCE_FRAME_MS = 1000 / 60;

/**
 * Frame-rate-independence factor: how much to scale this frame's
 * movement so the game advances at a fixed wall-clock speed
 * regardless of the display's refresh rate.
 *
 * `dtMs` is the time since the previous frame. On a 60 Hz display
 * `dtMs ≈ 16.7` → ~1.0; on 144 Hz `dtMs ≈ 6.9` → ~0.42 (smaller
 * steps, more often). Clamped to 2.5 so a long stall (backgrounded
 * tab, GC pause) can't teleport the ball across the field.
 */
export function frameScale(dtMs: number): number {
  return clamp(dtMs / REFERENCE_FRAME_MS, 0, 2.5);
}

/**
 * Swept paddle-collision test — tunnel-proof for fast balls.
 *
 * Returns true if the ball's leading edge **crossed** the paddle's
 * vertical face this frame *and* the ball overlapped the paddle's
 * Y-span at that moment. A per-frame point test misses a ball that
 * jumps clean past a thin paddle in one step; the crossing test
 * catches it however fast the ball moves.
 *
 * - `prevEdge` / `curEdge` — the ball's leading edge last frame / this
 *   frame (`ballX − BALL_R` for a leftward ball, `ballX + BALL_R` for
 *   a rightward one).
 * - `planeX` — the x of the paddle face the ball approaches.
 * - `approaching` — `"left"` (ball moving left → player paddle) or
 *   `"right"` (→ bot paddle).
 */
export function paddleHit(
  prevEdge: number,
  curEdge: number,
  planeX: number,
  approaching: "left" | "right",
  ballY: number,
  ballR: number,
  paddleCenterY: number,
  paddleH: number,
): boolean {
  const crossed =
    approaching === "left"
      ? prevEdge >= planeX && curEdge <= planeX
      : prevEdge <= planeX && curEdge >= planeX;
  if (!crossed) return false;
  const top = paddleCenterY - paddleH / 2;
  const bottom = paddleCenterY + paddleH / 2;
  // Ball radius included so a corner-graze still counts.
  return ballY + ballR >= top && ballY - ballR <= bottom;
}

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
