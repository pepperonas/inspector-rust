import { describe, it, expect } from "vitest";
import {
  BIRD_R,
  FLAP_VY,
  GRAVITY,
  MAX_FALL_VY,
  PIPE_GAP,
  PIPE_SPACING,
  PIPE_SPEED,
  PIPE_WIDTH,
  birdX,
  clamp,
  flap,
  frameScale,
  groundY,
  hitsGround,
  hitsPipe,
  initialState,
  randGapTop,
  step,
  type FlappyState,
} from "./flappy";

const W = 700;
const H = 452;

function fresh(): FlappyState {
  const s = initialState(H);
  flap(s); // start it (sinceSpawnX is primed to spawn on the first step)
  return s;
}

describe("frameScale", () => {
  it("is ~1 at 60fps and clamps spikes", () => {
    expect(frameScale(1000 / 60)).toBeCloseTo(1, 5);
    expect(frameScale(1000)).toBe(2.5);
    expect(frameScale(-5)).toBe(0);
  });
});

describe("clamp", () => {
  it("bounds a value", () => {
    expect(clamp(5, 0, 10)).toBe(5);
    expect(clamp(-1, 0, 10)).toBe(0);
    expect(clamp(99, 0, 10)).toBe(10);
  });
});

describe("initialState / flap", () => {
  it("starts idle, no pipes, bird above centre", () => {
    const s = initialState(H);
    expect(s.started).toBe(false);
    expect(s.dead).toBe(false);
    expect(s.pipes).toEqual([]);
    expect(s.score).toBe(0);
    expect(s.birdY).toBeLessThan(H / 2);
  });
  it("flap sets the upward impulse and starts the run", () => {
    const s = initialState(H);
    flap(s);
    expect(s.vy).toBe(FLAP_VY);
    expect(s.started).toBe(true);
  });
  it("flap is a no-op once dead", () => {
    const s = initialState(H);
    s.dead = true;
    flap(s);
    expect(s.vy).toBe(0);
  });
});

describe("step — idle / dead are no-ops", () => {
  it("does nothing before the first flap", () => {
    const s = initialState(H);
    const before = JSON.stringify(s);
    step(s, W, H, 1, 100);
    expect(JSON.stringify(s)).toBe(before);
  });
  it("does nothing when dead", () => {
    const s = fresh();
    s.dead = true;
    const y = s.birdY;
    step(s, W, H, 1, 100);
    expect(s.birdY).toBe(y);
  });
});

describe("step — physics", () => {
  it("gravity accelerates downward and moves the bird down", () => {
    const s = fresh();
    s.vy = 0;
    s.birdY = 200;
    step(s, W, H, 1, 100);
    expect(s.vy).toBeCloseTo(GRAVITY, 5);
    expect(s.birdY).toBeGreaterThan(200);
  });
  it("caps the fall speed at terminal velocity", () => {
    const s = fresh();
    s.vy = MAX_FALL_VY + 5;
    s.birdY = 100;
    step(s, W, H, 1, 100);
    expect(s.vy).toBe(MAX_FALL_VY);
  });
  it("clamps at the ceiling without dying", () => {
    const s = fresh();
    s.birdY = 2;
    s.vy = -10;
    step(s, W, H, 1, 100);
    expect(s.birdY).toBe(BIRD_R);
    expect(s.vy).toBe(0);
    expect(s.dead).toBe(false);
  });
});

describe("step — pipes", () => {
  it("spawns a pipe on the first started step (sinceSpawnX primed)", () => {
    const s = fresh();
    step(s, W, H, 1, 120);
    expect(s.pipes.length).toBe(1);
    expect(s.pipes[0].x).toBe(W);
    expect(s.pipes[0].gapTop).toBe(120);
  });
  it("scrolls pipes left at PIPE_SPEED·dt", () => {
    const s = fresh();
    s.sinceSpawnX = 0; // don't spawn this step
    s.pipes = [{ x: 300, gapTop: 100, scored: false }];
    s.birdY = 175; // sit safely in the gap so we don't die
    step(s, W, H, 1, 100);
    expect(s.pipes[0].x).toBeCloseTo(300 - PIPE_SPEED, 5);
  });
  it("scores once the pipe's right edge passes the bird", () => {
    const s = fresh();
    s.sinceSpawnX = 0;
    // Pipe whose right edge is just left of the bird after one scroll.
    s.pipes = [{ x: 130, gapTop: 100, scored: false }];
    s.birdY = 175; // inside the gap (100..250) → no collision
    expect(s.score).toBe(0);
    step(s, W, H, 1, 100);
    expect(s.pipes[0].scored).toBe(true);
    expect(s.score).toBe(1);
    // Doesn't double-count on the next step.
    step(s, W, H, 1, 100);
    expect(s.score).toBe(1);
  });
  it("drops pipes that fully scroll off the left", () => {
    const s = fresh();
    s.sinceSpawnX = 0;
    s.pipes = [{ x: -PIPE_WIDTH - 1, gapTop: 100, scored: true }];
    s.birdY = 175;
    step(s, W, H, 1, 100);
    expect(s.pipes.length).toBe(0);
  });
});

describe("collision helpers", () => {
  it("hitsGround when the bird touches the floor", () => {
    expect(hitsGround(groundY(H) - BIRD_R - 1, BIRD_R, H)).toBe(false);
    expect(hitsGround(groundY(H) - BIRD_R + 1, BIRD_R, H)).toBe(true);
  });
  it("hitsPipe outside the gap, misses inside it", () => {
    const pipe = { x: birdX(W) - PIPE_WIDTH / 2, gapTop: 150, scored: false };
    // In the gap (150..300): no hit.
    expect(hitsPipe(birdX(W), 200, BIRD_R, pipe, H)).toBe(false);
    // Into the top pipe: hit.
    expect(hitsPipe(birdX(W), 40, BIRD_R, pipe, H)).toBe(true);
    // Into the bottom pipe (below gapTop+GAP): hit.
    expect(hitsPipe(birdX(W), 150 + PIPE_GAP + 30, BIRD_R, pipe, H)).toBe(true);
  });
});

describe("step — death", () => {
  it("dies on the ground and snaps to rest there", () => {
    const s = fresh();
    s.sinceSpawnX = 0;
    s.birdY = groundY(H) - BIRD_R + 0.5;
    s.vy = 5;
    step(s, W, H, 1, 100);
    expect(s.dead).toBe(true);
    expect(s.birdY).toBe(groundY(H) - BIRD_R);
  });
  it("dies when it flies into a pipe", () => {
    const s = fresh();
    s.sinceSpawnX = 0;
    s.pipes = [{ x: birdX(W) - PIPE_WIDTH / 2, gapTop: 250, scored: false }];
    s.birdY = 60; // up in the top pipe
    s.vy = 0;
    step(s, W, H, 1, 100);
    expect(s.dead).toBe(true);
  });
});

describe("randGapTop", () => {
  it("stays within the valid band for many draws", () => {
    for (let i = 0; i < 500; i++) {
      const g = randGapTop(H);
      expect(g).toBeGreaterThanOrEqual(0);
      expect(g + PIPE_GAP).toBeLessThanOrEqual(groundY(H));
    }
  });
  it("respects an injected rng for determinism", () => {
    expect(randGapTop(H, () => 0)).toBe(38); // GAP_MARGIN
  });
});

describe("constants sanity", () => {
  it("a pipe gap fits within the playable height", () => {
    expect(PIPE_GAP + 2 * 38).toBeLessThan(groundY(H));
    expect(PIPE_SPACING).toBeGreaterThan(PIPE_WIDTH);
  });
});
