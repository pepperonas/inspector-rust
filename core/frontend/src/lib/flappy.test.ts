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
  aiShouldFlap,
  aiTargetY,
  birdX,
  clamp,
  flap,
  frameScale,
  groundY,
  hitsGround,
  hitsPipe,
  initialState,
  randGapTop,
  GROUND_H,
  GAP_MARGIN,
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
  it("dies when it flies into the ceiling (v0.84.70)", () => {
    const s = fresh();
    s.birdY = 2;
    s.vy = -10;
    step(s, W, H, 1, 100);
    expect(s.birdY).toBe(BIRD_R);
    expect(s.dead).toBe(true);
  });

  it("clamps at the ceiling without dying when invincible (AI autopilot)", () => {
    const s = fresh();
    s.birdY = 2;
    s.vy = -10;
    step(s, W, H, 1, 100, true);
    expect(s.birdY).toBe(BIRD_R);
    expect(s.vy).toBe(0);
    expect(s.dead).toBe(false);
  });

  it("invincible step survives ground + pipes (flies forever)", () => {
    const s = fresh();
    s.birdY = groundY(H) + 50; // well into the ground
    s.vy = 8;
    step(s, W, H, 1, 100, true);
    expect(s.dead).toBe(false);
    expect(s.birdY).toBe(groundY(H) - BIRD_R); // clamped above the ground
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

describe("AI autopilot", () => {
  it("aiTargetY aims at the next gap centre ahead of the bird", () => {
    const s = initialState(H);
    // A pipe ahead of the bird with a known gap.
    s.pipes = [{ x: birdX(W) + 100, gapTop: 120, scored: false }];
    expect(aiTargetY(s, W, H)).toBe(120 + PIPE_GAP / 2);
  });

  it("aiTargetY falls back to mid-field with no pipe in range", () => {
    const s = initialState(H);
    s.pipes = [];
    expect(aiTargetY(s, W, H)).toBeCloseTo(H * 0.45, 5);
  });

  it("aiShouldFlap only when below target and not already rising", () => {
    expect(aiShouldFlap(200, 1, 150)).toBe(true); // below target, falling
    expect(aiShouldFlap(100, 1, 150)).toBe(false); // above target
    expect(aiShouldFlap(200, -5, 150)).toBe(false); // rising fast already
  });
});

describe("randGapTop — fields too short for a proper gap", () => {
  // A tiny popup (or a stubborn window manager) can hand the game a field
  // shorter than GAP_MARGIN + PIPE_GAP + GAP_MARGIN. The band then inverts and
  // a naive `lo + rng*(hi-lo)` would return values ABOVE `lo`, i.e. a gap
  // hanging below the ground — an unplayable, instantly-fatal pipe.
  const tooShort = GAP_MARGIN + PIPE_GAP + GAP_MARGIN + GROUND_H - 1;

  it("falls back to a centred gap instead of an inverted band", () => {
    for (const h of [10, 100, tooShort]) {
      const g = randGapTop(h, () => 0.99);
      expect(Number.isFinite(g)).toBe(true);
      // Never above the top margin…
      expect(g).toBeGreaterThanOrEqual(GAP_MARGIN);
      // …and never dependent on the rng, since there is no band to sample.
      expect(randGapTop(h, () => 0)).toBe(g);
      expect(randGapTop(h, () => 1)).toBe(g);
    }
  });

  it("a field one pixel taller than the degenerate case samples normally again", () => {
    const ok = tooShort + 2;
    const lo = randGapTop(ok, () => 0);
    const hi = randGapTop(ok, () => 1);
    expect(lo).toBe(GAP_MARGIN);
    expect(hi).toBeGreaterThanOrEqual(lo);
    expect(hi + PIPE_GAP).toBeLessThanOrEqual(groundY(ok));
  });

  it("always returns a whole number of pixels", () => {
    for (const h of [10, 200, 480, 601]) {
      expect(Number.isInteger(randGapTop(h, () => 0.37))).toBe(true);
    }
  });
});
