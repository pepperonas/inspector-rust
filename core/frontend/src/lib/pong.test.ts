import { describe, it, expect } from "vitest";
import {
  BALL_BASE_SPEED,
  BALL_MAX_SPEED,
  WIN_SCORE,
  botMaxSpeed,
  clamp,
  nextBallSpeed,
  paddleBounce,
  serveBall,
} from "./pong";

describe("clamp", () => {
  it("returns the value when in range", () => {
    expect(clamp(5, 0, 10)).toBe(5);
  });
  it("clamps to the lower bound", () => {
    expect(clamp(-3, 0, 10)).toBe(0);
  });
  it("clamps to the upper bound", () => {
    expect(clamp(99, 0, 10)).toBe(10);
  });
  it("returns the bound exactly at the edges", () => {
    expect(clamp(0, 0, 10)).toBe(0);
    expect(clamp(10, 0, 10)).toBe(10);
  });
});

describe("WIN_SCORE", () => {
  it("is 5 — the spec'd first-to score", () => {
    expect(WIN_SCORE).toBe(5);
  });
});

describe("botMaxSpeed — ramp-up difficulty", () => {
  it("starts fair at bot score 0", () => {
    expect(botMaxSpeed(0)).toBeCloseTo(4.5);
  });
  it("ramps up with each bot point", () => {
    expect(botMaxSpeed(1)).toBeCloseTo(5.25);
    expect(botMaxSpeed(2)).toBeCloseTo(6.0);
    expect(botMaxSpeed(4)).toBeCloseTo(7.5);
  });
  it("is monotonically non-decreasing", () => {
    for (let s = 0; s < WIN_SCORE; s++) {
      expect(botMaxSpeed(s + 1)).toBeGreaterThanOrEqual(botMaxSpeed(s));
    }
  });
  it("clamps the score input so a stray value can't explode the speed", () => {
    expect(botMaxSpeed(999)).toBe(botMaxSpeed(WIN_SCORE));
    expect(botMaxSpeed(-3)).toBe(botMaxSpeed(0));
  });
});

describe("nextBallSpeed", () => {
  it("bumps the speed by a fixed gain per hit", () => {
    expect(nextBallSpeed(6)).toBeCloseTo(6.4);
  });
  it("never exceeds BALL_MAX_SPEED", () => {
    expect(nextBallSpeed(BALL_MAX_SPEED)).toBe(BALL_MAX_SPEED);
    expect(nextBallSpeed(BALL_MAX_SPEED - 0.1)).toBe(BALL_MAX_SPEED);
  });
  it("a long rally converges to the cap", () => {
    let speed = BALL_BASE_SPEED;
    for (let i = 0; i < 100; i++) speed = nextBallSpeed(speed);
    expect(speed).toBe(BALL_MAX_SPEED);
  });
});

describe("paddleBounce", () => {
  it("a centre hit returns a near-horizontal velocity", () => {
    const v = paddleBounce(0, 1, 8);
    expect(v.vx).toBeCloseTo(8); // full speed horizontal
    expect(v.vy).toBeCloseTo(0); // no vertical deflection
  });

  it("respects the horizontal direction sign", () => {
    expect(paddleBounce(0, 1, 8).vx).toBeGreaterThan(0);
    expect(paddleBounce(0, -1, 8).vx).toBeLessThan(0);
  });

  it("an edge hit deflects vertically", () => {
    const top = paddleBounce(-1, 1, 8);
    const bottom = paddleBounce(1, 1, 8);
    expect(top.vy).toBeLessThan(0); // hit the top edge → ball goes up
    expect(bottom.vy).toBeGreaterThan(0); // bottom edge → ball goes down
  });

  it("preserves overall speed magnitude (deflection just redistributes it)", () => {
    for (const offset of [-1, -0.5, 0, 0.5, 1]) {
      const v = paddleBounce(offset, 1, 10);
      const mag = Math.hypot(v.vx, v.vy);
      expect(mag).toBeCloseTo(10);
    }
  });

  it("clamps an out-of-range offset", () => {
    // offset 5 should behave like offset 1 (the bottom edge).
    const overshoot = paddleBounce(5, 1, 8);
    const edge = paddleBounce(1, 1, 8);
    expect(overshoot.vx).toBeCloseTo(edge.vx);
    expect(overshoot.vy).toBeCloseTo(edge.vy);
  });
});

describe("serveBall", () => {
  it("starts the ball at the field centre", () => {
    const b = serveBall(700, 452, true);
    expect(b.x).toBe(350);
    expect(b.y).toBe(226);
  });

  it("serves toward the player when towardPlayer is true", () => {
    // Deterministic rng → angle 0 → pure horizontal serve.
    const b = serveBall(700, 452, true, () => 0.5);
    expect(b.vx).toBeLessThan(0); // negative vx = moving left = toward player
  });

  it("serves toward the bot when towardPlayer is false", () => {
    const b = serveBall(700, 452, false, () => 0.5);
    expect(b.vx).toBeGreaterThan(0);
  });

  it("a zero-angle serve has the base speed horizontally", () => {
    const b = serveBall(700, 452, false, () => 0.5);
    expect(Math.abs(b.vx)).toBeCloseTo(BALL_BASE_SPEED);
    expect(b.vy).toBeCloseTo(0);
  });

  it("the serve magnitude is always the base speed", () => {
    for (const r of [0, 0.25, 0.5, 0.75, 1]) {
      const b = serveBall(700, 452, true, () => r);
      expect(Math.hypot(b.vx, b.vy)).toBeCloseTo(BALL_BASE_SPEED);
    }
  });
});
