/**
 * Pure, testable game logic for the `rockthebox` Snake easter egg.
 *
 * The stateful canvas render loop lives in `components/SnakeGame.tsx`;
 * everything here is side-effect-free so it can be unit-tested without
 * a DOM / canvas / requestAnimationFrame.
 */

/** Play-field grid, in cells. */
export const GRID_COLS = 24;
export const GRID_ROWS = 16;

/** Snake length at the start of a game. `body[0]` is the head. */
export const INITIAL_LENGTH = 4;

/** Intro-animation duration (ms). Must match the `rockTheBox*` CSS
 *  keyframe durations in styles.css — the JS phase flip and the CSS
 *  flourish need to finish together. */
export const INTRO_MS = 1900;

/** Tick cadence — ms between snake steps. Starts at STEP_BASE_MS,
 *  shaved by STEP_SPEEDUP_MS per food eaten, never quicker than
 *  STEP_MIN_MS, so the game ramps up but stays controllable. */
export const STEP_BASE_MS = 145;
export const STEP_MIN_MS = 70;
export const STEP_SPEEDUP_MS = 5;

export type Direction = "up" | "down" | "left" | "right";

/** A grid-cell coordinate. */
export interface Point {
  x: number;
  y: number;
}

/** Unit step vector for a direction. */
export function dirDelta(d: Direction): Point {
  switch (d) {
    case "up":
      return { x: 0, y: -1 };
    case "down":
      return { x: 0, y: 1 };
    case "left":
      return { x: -1, y: 0 };
    case "right":
      return { x: 1, y: 0 };
  }
}

/** True if `a` and `b` point opposite ways — the snake may not reverse
 *  straight into its own neck, so such an input is dropped. */
export function isOpposite(a: Direction, b: Direction): boolean {
  return (
    (a === "up" && b === "down") ||
    (a === "down" && b === "up") ||
    (a === "left" && b === "right") ||
    (a === "right" && b === "left")
  );
}

/** Clamp `v` into the inclusive `[lo, hi]` range. */
export function clamp(v: number, lo: number, hi: number): number {
  if (v < lo) return lo;
  if (v > hi) return hi;
  return v;
}

/**
 * The starting snake: `length` cells laid out horizontally with the
 * head near the field centre and the body trailing left — so it's
 * already free to move "right" into open space. `body[0]` is the head.
 */
export function initialSnake(
  cols: number = GRID_COLS,
  rows: number = GRID_ROWS,
  length: number = INITIAL_LENGTH,
): Point[] {
  const cy = Math.floor(rows / 2);
  const headX = Math.floor(cols / 2);
  const body: Point[] = [];
  for (let i = 0; i < length; i++) {
    body.push({ x: clamp(headX - i, 0, cols - 1), y: cy });
  }
  return body;
}

/** Outcome of advancing the snake by one cell. */
export interface StepResult {
  /** The snake after the step (unchanged when `dead`). */
  snake: Point[];
  /** True if the head landed on the food this step. */
  ate: boolean;
  /** True if the step killed the snake (wall or self collision). */
  dead: boolean;
}

/**
 * Advance the snake one cell in `dir`.
 *
 * - Walking off any edge of the `cols × rows` field is death — classic
 *   Snake, no wrap-around.
 * - Running into the body is death, with one nuance: when the snake is
 *   *not* eating, its tail cell vacates this step, so moving the head
 *   into the current tail cell is allowed.
 * - Eating the food grows the snake (the tail is kept this step).
 */
export function step(
  snake: Point[],
  dir: Direction,
  food: Point,
  cols: number = GRID_COLS,
  rows: number = GRID_ROWS,
): StepResult {
  const head = snake[0];
  const d = dirDelta(dir);
  const nx = head.x + d.x;
  const ny = head.y + d.y;

  if (nx < 0 || nx >= cols || ny < 0 || ny >= rows) {
    return { snake, ate: false, dead: true };
  }

  const ate = nx === food.x && ny === food.y;
  // When not eating, the tail vacates — exclude it from collision.
  const bodyToCheck = ate ? snake : snake.slice(0, -1);
  if (bodyToCheck.some((s) => s.x === nx && s.y === ny)) {
    return { snake, ate: false, dead: true };
  }

  const next: Point[] = [{ x: nx, y: ny }, ...snake];
  if (!ate) next.pop();
  return { snake: next, ate, dead: false };
}

/**
 * Pick a uniformly-random free cell for the next food. `occupied` is
 * every cell the snake body covers. Returns `null` only when the board
 * is completely full — the snake has, improbably, won.
 */
export function spawnFood(
  occupied: Point[],
  cols: number = GRID_COLS,
  rows: number = GRID_ROWS,
  rng: () => number = Math.random,
): Point | null {
  const free: Point[] = [];
  for (let y = 0; y < rows; y++) {
    for (let x = 0; x < cols; x++) {
      if (!occupied.some((o) => o.x === x && o.y === y)) {
        free.push({ x, y });
      }
    }
  }
  if (free.length === 0) return null;
  // Math.min guards the rng() === 1 edge so the index stays in range.
  return free[Math.min(free.length - 1, Math.floor(rng() * free.length))];
}

/** Tick interval (ms between steps) for a given score — the speed
 *  ramp. Capped at STEP_MIN_MS so a long game stays playable. */
export function tickInterval(score: number): number {
  return Math.max(STEP_MIN_MS, STEP_BASE_MS - score * STEP_SPEEDUP_MS);
}
