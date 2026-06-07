import { describe, expect, it } from "vitest";
import {
  allDead,
  alienScore,
  bulletHitsAlien,
  createFormation,
  formationExtents,
  stepFormation,
} from "./space-invaders";

describe("space invaders formation", () => {
  it("creates 55 aliens centred on the field", () => {
    const aliens = createFormation(700, 48);
    expect(aliens).toHaveLength(55);
    const { left, right } = formationExtents(aliens);
    expect(left).toBeGreaterThanOrEqual(0);
    expect(right).toBeLessThanOrEqual(700);
  });

  it("detects all dead", () => {
    const aliens = createFormation(700, 48);
    aliens.forEach((a) => (a.alive = false));
    expect(allDead(aliens)).toBe(true);
  });

  it("scores top rows higher", () => {
    expect(alienScore(0)).toBeGreaterThan(alienScore(4));
  });

  it("registers player bullet hits", () => {
    const aliens = createFormation(700, 48);
    const target = aliens[0];
    const hit = bulletHitsAlien(
      {
        active: true,
        x: target.x + 4,
        y: target.y + 4,
        vy: -1,
        fromPlayer: true,
      },
      aliens,
    );
    expect(hit).toBe(0);
  });

  it("bounces formation off the left wall", () => {
    const aliens = createFormation(400, 40);
    let dir = -1 as 1 | -1;
    for (let i = 0; i < 200; i++) {
      const r = stepFormation(aliens, dir, 400, 1);
      dir = r.dir;
      if (r.hitWall) break;
    }
    expect(dir).toBe(1);
  });
});
