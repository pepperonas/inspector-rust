import { describe, it, expect } from "vitest";
import {
  BALL_BASE_SPEED,
  BALL_MAX_SPEED,
  BALL_R,
  PADDLE_H,
  REFERENCE_FRAME_MS,
  SERVE_DELAY_MS,
  SHIFT_SPEED_MULTIPLIER,
  WIN_SCORE,
  botBehavior,
  botMaxSpeed,
  clamp,
  pseudoNoise,
  frameScale,
  nextBallSpeed,
  paddleBounce,
  paddleHit,
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

describe("frameScale — frame-rate independence", () => {
  it("is ~1 at the 60 Hz reference frame duration", () => {
    expect(frameScale(REFERENCE_FRAME_MS)).toBeCloseTo(1);
  });
  it("is ~0.5 at double the refresh rate (120 Hz)", () => {
    expect(frameScale(REFERENCE_FRAME_MS / 2)).toBeCloseTo(0.5);
  });
  it("is ~0.42 at 144 Hz", () => {
    expect(frameScale(1000 / 144)).toBeCloseTo(0.4167, 2);
  });
  it("is ~2 at half the refresh rate (30 Hz)", () => {
    expect(frameScale(REFERENCE_FRAME_MS * 2)).toBeCloseTo(2);
  });
  it("clamps a long stall so the ball can't teleport", () => {
    expect(frameScale(10_000)).toBe(2.5);
  });
  it("is 0 for a zero-length delta", () => {
    expect(frameScale(0)).toBe(0);
  });
});

describe("constants for the new Pong behaviours", () => {
  it("the Shift boost doubles paddle speed", () => {
    expect(SHIFT_SPEED_MULTIPLIER).toBe(2);
  });
  it("the post-point serve delay is one second", () => {
    expect(SERVE_DELAY_MS).toBe(1000);
  });
});

describe("paddleHit — swept collision", () => {
  // Player paddle: face at x=36, centred vertically at y=226.
  const planeX = 36;
  const cy = 226;

  it("detects a leftward ball crossing the player's face", () => {
    expect(paddleHit(50, 30, planeX, "left", cy, BALL_R, cy, PADDLE_H)).toBe(
      true,
    );
  });
  it("detects a rightward ball crossing the bot's face", () => {
    expect(paddleHit(30, 50, planeX, "right", cy, BALL_R, cy, PADDLE_H)).toBe(
      true,
    );
  });
  it("returns false when the ball never reaches the face", () => {
    expect(paddleHit(80, 60, planeX, "left", cy, BALL_R, cy, PADDLE_H)).toBe(
      false,
    );
  });
  it("returns false when the ball moves away from the face", () => {
    expect(paddleHit(30, 50, planeX, "left", cy, BALL_R, cy, PADDLE_H)).toBe(
      false,
    );
  });
  it("catches a fast ball that tunnels clean past in one step", () => {
    // Leading edge jumps from well right of the face to well left.
    expect(paddleHit(120, -40, planeX, "left", cy, BALL_R, cy, PADDLE_H)).toBe(
      true,
    );
  });
  it("misses when the ball crosses above the paddle's Y-span", () => {
    const above = cy - PADDLE_H / 2 - BALL_R - 5;
    expect(
      paddleHit(50, 30, planeX, "left", above, BALL_R, cy, PADDLE_H),
    ).toBe(false);
  });
  it("misses when the ball crosses below the paddle's Y-span", () => {
    const below = cy + PADDLE_H / 2 + BALL_R + 5;
    expect(
      paddleHit(50, 30, planeX, "left", below, BALL_R, cy, PADDLE_H),
    ).toBe(false);
  });
  it("counts a corner graze (ball radius reaches the paddle edge)", () => {
    const graze = cy - PADDLE_H / 2 - BALL_R + 1;
    expect(
      paddleHit(50, 30, planeX, "left", graze, BALL_R, cy, PADDLE_H),
    ).toBe(true);
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

describe("pseudoNoise — deterministic [-1, +1] hash", () => {
  it("returns same value for same seed (deterministic)", () => {
    expect(pseudoNoise(123)).toBe(pseudoNoise(123));
    expect(pseudoNoise(0.5)).toBe(pseudoNoise(0.5));
  });
  it("stays inside [-1, +1] for any seed", () => {
    for (const seed of [0, 1, -5, 12345, 99.99, -1e6]) {
      const v = pseudoNoise(seed);
      expect(v).toBeGreaterThanOrEqual(-1);
      expect(v).toBeLessThanOrEqual(1);
    }
  });
  it("different seeds → different outputs (spot-check)", () => {
    const a = pseudoNoise(1);
    const b = pseudoNoise(2);
    expect(a).not.toBe(b);
  });
});

describe("botBehavior — rubber-band AI (v0.38.0+)", () => {
  // Boilerplate ball state — moving toward bot (+x), midfield, normal pace.
  const baseBall = {
    ballX: 700,
    ballY: 226,
    ballVx: 7,
    ballVy: 0,
    botX: 1280,
    fieldH: 452,
  };

  it("bot leading by 2+ → plays badly (low maxSpeed, large error)", () => {
    const move = botBehavior({ ...baseBall, botScore: 3, playerScore: 1 });
    expect(move.maxSpeed).toBeLessThan(5); // 9.5 * 0.45 = 4.275
  });
  it("bot leading by 1 → only slightly slow", () => {
    const move = botBehavior({ ...baseBall, botScore: 2, playerScore: 1 });
    expect(move.maxSpeed).toBeGreaterThan(6);
    expect(move.maxSpeed).toBeLessThan(8);
  });
  it("tied → standard hard", () => {
    const move = botBehavior({ ...baseBall, botScore: 2, playerScore: 2 });
    expect(move.maxSpeed).toBeCloseTo(9.5, 1);
  });
  it("bot behind by 2+ → harder than baseline", () => {
    const move = botBehavior({ ...baseBall, botScore: 0, playerScore: 2 });
    expect(move.maxSpeed).toBeGreaterThan(12);
  });
  it("player one-away-from-winning → HARDCORE override", () => {
    // Even with a huge bot lead, player on match point flips to hardcore.
    const move = botBehavior({ ...baseBall, botScore: 4, playerScore: WIN_SCORE - 1 });
    expect(move.maxSpeed).toBeGreaterThan(15); // 9.5 * 1.65 = 15.675
  });

  it("predicts ball intercept when ball moves toward bot", () => {
    // Ball at (700, 100) moving +x +vy=0 toward botX=1280:
    // predicted Y = 100 + 0*((1280-700)/7) = 100.
    const move = botBehavior({
      ...baseBall,
      ballX: 700,
      ballY: 100,
      ballVx: 7,
      ballVy: 0,
      botScore: 0,
      playerScore: 0,
    });
    // At tied (skill=1), error magnitude is 0 → targetY == predicted.
    expect(move.targetY).toBeCloseTo(100, 0);
  });

  it("predicts with vertical velocity (linear extrapolation)", () => {
    // dt = (1280-700)/7 = 82.86; ballY 100 + ballVy*82.86 = 100 + 2*82.86 = 265.71
    const move = botBehavior({
      ...baseBall,
      ballX: 700,
      ballY: 100,
      ballVx: 7,
      ballVy: 2,
      botScore: 0,
      playerScore: 0,
    });
    expect(move.targetY).toBeCloseTo(265.71, 0);
  });

  it("idle toward centre when ball is moving away from bot", () => {
    const move = botBehavior({
      ...baseBall,
      ballX: 800,
      ballY: 50,
      ballVx: -5, // moving away
      ballVy: 0,
      botScore: 0,
      playerScore: 0,
    });
    expect(move.targetY).toBeCloseTo(baseBall.fieldH / 2, 0);
  });

  it("higher skill = smaller deterministic error", () => {
    // Same ball state, varying skill — predicted target stays the
    // same; only the error magnitude changes.
    const hardcore = botBehavior({
      ...baseBall,
      botScore: 0,
      playerScore: WIN_SCORE - 1,
    });
    const easy = botBehavior({
      ...baseBall,
      botScore: 3,
      playerScore: 0,
    });
    // Easy bot's targetY can deviate up to ~60 px from prediction.
    // Hardcore should deviate <1 px (skill > 1).
    const predicted = baseBall.ballY; // ballVy=0
    expect(Math.abs(hardcore.targetY - predicted)).toBeLessThan(1);
    expect(Math.abs(easy.targetY - predicted)).toBeGreaterThan(
      Math.abs(hardcore.targetY - predicted),
    );
  });

  it("targetY always clamped inside the field", () => {
    // Crazy prediction that would extrapolate way off-field.
    const move = botBehavior({
      ...baseBall,
      ballX: 700,
      ballY: 226,
      ballVx: 0.5, // very slow — dt huge
      ballVy: 50,  // would extrapolate to ballY + 50*(580/0.5) = 58226
      botScore: 0,
      playerScore: 0,
    });
    expect(move.targetY).toBeGreaterThanOrEqual(0);
    expect(move.targetY).toBeLessThanOrEqual(baseBall.fieldH);
  });
});
