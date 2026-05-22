import { describe, it, expect } from "vitest";
import {
  GRID_COLS,
  GRID_ROWS,
  INITIAL_LENGTH,
  INTRO_MS,
  STEP_BASE_MS,
  STEP_MIN_MS,
  type Point,
  clamp,
  dirDelta,
  initialSnake,
  isOpposite,
  spawnFood,
  step,
  tickInterval,
} from "./snake";

describe("constants", () => {
  it("has a non-degenerate grid", () => {
    expect(GRID_COLS).toBeGreaterThan(4);
    expect(GRID_ROWS).toBeGreaterThan(4);
  });
  it("intro duration matches the 1.9 s CSS flourish", () => {
    expect(INTRO_MS).toBe(1900);
  });
});

describe("clamp", () => {
  it("returns the value when in range", () => {
    expect(clamp(5, 0, 10)).toBe(5);
  });
  it("clamps to both bounds", () => {
    expect(clamp(-3, 0, 10)).toBe(0);
    expect(clamp(99, 0, 10)).toBe(10);
  });
});

describe("dirDelta", () => {
  it("maps each direction to its unit vector", () => {
    expect(dirDelta("up")).toEqual({ x: 0, y: -1 });
    expect(dirDelta("down")).toEqual({ x: 0, y: 1 });
    expect(dirDelta("left")).toEqual({ x: -1, y: 0 });
    expect(dirDelta("right")).toEqual({ x: 1, y: 0 });
  });
});

describe("isOpposite", () => {
  it("is true for the two reversal pairs", () => {
    expect(isOpposite("up", "down")).toBe(true);
    expect(isOpposite("down", "up")).toBe(true);
    expect(isOpposite("left", "right")).toBe(true);
    expect(isOpposite("right", "left")).toBe(true);
  });
  it("is false for same and perpendicular directions", () => {
    expect(isOpposite("up", "up")).toBe(false);
    expect(isOpposite("up", "left")).toBe(false);
    expect(isOpposite("right", "down")).toBe(false);
  });
});

describe("initialSnake", () => {
  it("has the configured length", () => {
    expect(initialSnake()).toHaveLength(INITIAL_LENGTH);
  });
  it("places the head near the field centre", () => {
    const snake = initialSnake();
    expect(snake[0]).toEqual({
      x: Math.floor(GRID_COLS / 2),
      y: Math.floor(GRID_ROWS / 2),
    });
  });
  it("trails the body left of the head on one row (facing right)", () => {
    const snake = initialSnake();
    for (let i = 1; i < snake.length; i++) {
      expect(snake[i].y).toBe(snake[0].y);
      expect(snake[i].x).toBe(snake[0].x - i);
    }
  });
  it("respects custom dimensions", () => {
    expect(initialSnake(10, 10, 3)).toEqual([
      { x: 5, y: 5 },
      { x: 4, y: 5 },
      { x: 3, y: 5 },
    ]);
  });
});

describe("step — movement", () => {
  it("advances the head and drags the tail when not eating", () => {
    const snake: Point[] = [
      { x: 5, y: 5 },
      { x: 4, y: 5 },
      { x: 3, y: 5 },
    ];
    const res = step(snake, "right", { x: 9, y: 9 }, 20, 20);
    expect(res.dead).toBe(false);
    expect(res.ate).toBe(false);
    expect(res.snake).toEqual([
      { x: 6, y: 5 },
      { x: 5, y: 5 },
      { x: 4, y: 5 },
    ]);
  });
  it("keeps its length constant on a plain move", () => {
    const snake = initialSnake();
    const res = step(snake, "up", { x: 0, y: 0 }, GRID_COLS, GRID_ROWS);
    expect(res.snake).toHaveLength(snake.length);
  });
});

describe("step — eating", () => {
  it("grows by one when the head lands on the food", () => {
    const snake: Point[] = [
      { x: 5, y: 5 },
      { x: 4, y: 5 },
      { x: 3, y: 5 },
    ];
    const res = step(snake, "right", { x: 6, y: 5 }, 20, 20);
    expect(res.ate).toBe(true);
    expect(res.dead).toBe(false);
    expect(res.snake).toHaveLength(4);
    expect(res.snake[0]).toEqual({ x: 6, y: 5 });
    // The tail cell is kept this step — that's the growth.
    expect(res.snake[3]).toEqual({ x: 3, y: 5 });
  });
});

describe("step — death", () => {
  it("dies walking off each of the four walls", () => {
    expect(step([{ x: 0, y: 5 }], "left", { x: 9, y: 9 }, 20, 20).dead).toBe(true);
    expect(step([{ x: 19, y: 5 }], "right", { x: 9, y: 9 }, 20, 20).dead).toBe(true);
    expect(step([{ x: 5, y: 0 }], "up", { x: 9, y: 9 }, 20, 20).dead).toBe(true);
    expect(step([{ x: 5, y: 19 }], "down", { x: 9, y: 9 }, 20, 20).dead).toBe(true);
  });
  it("returns the snake unchanged on death", () => {
    const snake: Point[] = [{ x: 0, y: 5 }];
    const res = step(snake, "left", { x: 9, y: 9 }, 20, 20);
    expect(res.snake).toBe(snake);
  });
  it("dies running into its own body", () => {
    // Head at {2,2}; moving up steps onto {2,1}, which is a body cell.
    const snake: Point[] = [
      { x: 2, y: 2 },
      { x: 1, y: 2 },
      { x: 1, y: 1 },
      { x: 2, y: 1 },
      { x: 3, y: 1 },
    ];
    expect(step(snake, "up", { x: 9, y: 9 }, 20, 20).dead).toBe(true);
  });
  it("allows the head to follow into the vacating tail cell", () => {
    // The tail moves out of {3,2} this step, so the head may move in.
    const snake: Point[] = [
      { x: 2, y: 2 },
      { x: 2, y: 1 },
      { x: 3, y: 1 },
      { x: 3, y: 2 },
    ];
    const res = step(snake, "right", { x: 9, y: 9 }, 20, 20);
    expect(res.dead).toBe(false);
    expect(res.snake[0]).toEqual({ x: 3, y: 2 });
  });
});

describe("step — wrap mode (the `rockthabox` variant)", () => {
  it("reappears on the opposite edge instead of dying — all four walls", () => {
    const left = step([{ x: 0, y: 5 }], "left", { x: 9, y: 9 }, 20, 20, true);
    expect(left.dead).toBe(false);
    expect(left.snake[0]).toEqual({ x: 19, y: 5 });

    const right = step([{ x: 19, y: 5 }], "right", { x: 9, y: 9 }, 20, 20, true);
    expect(right.dead).toBe(false);
    expect(right.snake[0]).toEqual({ x: 0, y: 5 });

    const up = step([{ x: 5, y: 0 }], "up", { x: 9, y: 9 }, 20, 20, true);
    expect(up.dead).toBe(false);
    expect(up.snake[0]).toEqual({ x: 5, y: 19 });

    const down = step([{ x: 5, y: 19 }], "down", { x: 9, y: 9 }, 20, 20, true);
    expect(down.dead).toBe(false);
    expect(down.snake[0]).toEqual({ x: 5, y: 0 });
  });
  it("still dies if it wraps straight into its own body", () => {
    // Head {0,5} wraps left to {19,5}, which a body cell occupies.
    const snake: Point[] = [
      { x: 0, y: 5 },
      { x: 1, y: 5 },
      { x: 19, y: 5 },
      { x: 18, y: 5 },
    ];
    expect(step(snake, "left", { x: 9, y: 9 }, 20, 20, true).dead).toBe(true);
  });
  it("eats food sitting on the opposite edge after a wrap", () => {
    const res = step([{ x: 0, y: 5 }], "left", { x: 19, y: 5 }, 20, 20, true);
    expect(res.ate).toBe(true);
    expect(res.snake[0]).toEqual({ x: 19, y: 5 });
  });
  it("an in-bounds move is unaffected by the wrap flag", () => {
    const a = step([{ x: 5, y: 5 }], "right", { x: 9, y: 9 }, 20, 20, false);
    const b = step([{ x: 5, y: 5 }], "right", { x: 9, y: 9 }, 20, 20, true);
    expect(a.snake).toEqual(b.snake);
  });
  it("classic mode (wrap off) still dies at the wall", () => {
    expect(step([{ x: 0, y: 5 }], "left", { x: 9, y: 9 }, 20, 20, false).dead).toBe(
      true,
    );
  });
});

describe("spawnFood", () => {
  it("never places food on the snake", () => {
    const snake = initialSnake();
    for (let i = 0; i < 200; i++) {
      const food = spawnFood(snake, GRID_COLS, GRID_ROWS)!;
      expect(snake.some((s) => s.x === food.x && s.y === food.y)).toBe(false);
    }
  });
  it("picks deterministically from the free cells given a fixed rng", () => {
    // 3×1 board, snake occupies {0,0}; free = [{1,0},{2,0}].
    const snake: Point[] = [{ x: 0, y: 0 }];
    expect(spawnFood(snake, 3, 1, () => 0)).toEqual({ x: 1, y: 0 });
    expect(spawnFood(snake, 3, 1, () => 0.99)).toEqual({ x: 2, y: 0 });
  });
  it("returns null when the board is completely full", () => {
    const full: Point[] = [];
    for (let y = 0; y < 4; y++) {
      for (let x = 0; x < 4; x++) full.push({ x, y });
    }
    expect(spawnFood(full, 4, 4)).toBeNull();
  });
});

describe("tickInterval — speed ramp", () => {
  it("starts at the base interval at score 0", () => {
    expect(tickInterval(0)).toBe(STEP_BASE_MS);
  });
  it("gets quicker as the score climbs", () => {
    expect(tickInterval(5)).toBeLessThan(tickInterval(0));
    expect(tickInterval(10)).toBeLessThan(tickInterval(5));
  });
  it("never drops below the minimum interval", () => {
    expect(tickInterval(1000)).toBe(STEP_MIN_MS);
    expect(tickInterval(50)).toBeGreaterThanOrEqual(STEP_MIN_MS);
  });
});
