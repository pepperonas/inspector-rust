import { afterEach, describe, expect, it, vi } from "vitest";
import {
  ALIEN_H,
  ALIEN_W,
  BULLET_SPEED,
  PLAYER_SPEED,
  PLAYER_W,
  SCORE_PER_ALIEN,
  aliensReachedPlayer,
  aliveAliens,
  allDead,
  alienScore,
  bulletHitsAlien,
  bulletHitsPlayer,
  clamp,
  createFormation,
  dropFormation,
  formationExtents,
  formationWidth,
  FORMATION_STEP_X,
  ALIEN_COLS,
  ALIEN_GAP_X,
  frameScale,
  movePlayer,
  pickShooter,
  spawnAlienBullet,
  spawnPlayerBullet,
  stepFormation,
  updateBullets,
  type Alien,
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

  it("bounces off the right wall too", () => {
    const aliens = createFormation(400, 40);
    let dir = 1 as 1 | -1;
    let bounced = false;
    for (let i = 0; i < 200; i++) {
      const r = stepFormation(aliens, dir, 400, 1);
      dir = r.dir;
      if (r.hitWall) {
        bounced = true;
        break;
      }
    }
    expect(bounced).toBe(true);
    expect(dir).toBe(-1);
  });

  it("only counts / moves living aliens", () => {
    const aliens = createFormation(700, 48);
    aliens.slice(0, 50).forEach((a) => (a.alive = false));
    expect(aliveAliens(aliens)).toHaveLength(5);
    expect(allDead(aliens)).toBe(false);

    // A dead alien is excluded from extents and is not dropped.
    const deadY = aliens[0].y;
    dropFormation(aliens);
    expect(aliens[0].y).toBe(deadY); // dead → unchanged
    expect(aliens[54].y).toBeGreaterThan(48); // last (alive) → dropped
  });

  it("formationExtents on an empty/all-dead formation is a zero box, not NaN", () => {
    expect(formationExtents([])).toEqual({ left: 0, right: 0, bottom: 0 });
    const aliens = createFormation(700, 48);
    aliens.forEach((a) => (a.alive = false));
    expect(formationExtents(aliens)).toEqual({ left: 0, right: 0, bottom: 0 });
  });
});

describe("frameScale", () => {
  it("is 1.0 at the reference 60fps frame", () => {
    expect(frameScale(1000 / 60)).toBeCloseTo(1, 10);
  });
  it("clamps a long stall to 2.5 and a negative dt to 0", () => {
    expect(frameScale(10_000)).toBe(2.5);
    expect(frameScale(-5)).toBe(0);
  });
});

describe("clamp", () => {
  it("bounds below, above and passes through inside", () => {
    expect(clamp(-3, 0, 10)).toBe(0);
    expect(clamp(42, 0, 10)).toBe(10);
    expect(clamp(5, 0, 10)).toBe(5);
  });
});

describe("movePlayer", () => {
  it("moves left/right by PLAYER_SPEED × scale", () => {
    expect(movePlayer(200, { left: false, right: true }, 700, 1)).toBe(200 + PLAYER_SPEED);
    expect(movePlayer(200, { left: true, right: false }, 700, 1)).toBe(200 - PLAYER_SPEED);
    expect(movePlayer(200, { left: true, right: true }, 700, 1)).toBe(200); // cancel out
    expect(movePlayer(200, { left: false, right: false }, 700, 1)).toBe(200);
  });
  it("cannot leave the playfield (clamped to the ship half-width margins)", () => {
    const lo = PLAYER_W / 2 + 8;
    const hi = 700 - PLAYER_W / 2 - 8;
    expect(movePlayer(0, { left: true, right: false }, 700, 5)).toBe(lo);
    expect(movePlayer(700, { left: false, right: true }, 700, 5)).toBe(hi);
  });
});

describe("updateBullets", () => {
  it("advances active bullets by their signed speed and deactivates off-field ones", () => {
    const player = spawnPlayerBullet(100, 300);
    updateBullets([player], 600, 1);
    expect(player.y).toBe(300 - BULLET_SPEED); // player bullets fly up (vy=-1)

    const gone = spawnPlayerBullet(100, -2);
    updateBullets([gone], 600, 1); // -2 - 11 = -13 < -8 → off the top
    expect(gone.active).toBe(false);

    const alien = spawnAlienBullet(100, 605);
    updateBullets([alien], 600, 1); // 605 + 7 = 612 > 608 (fieldH+8) → off the bottom
    expect(alien.active).toBe(false);

    // Still inside the ±8 margin → stays active.
    const near = spawnAlienBullet(100, 595);
    updateBullets([near], 600, 1); // 595 + 7 = 602 ≤ 608 → still on screen
    expect(near.active).toBe(true);
  });
  it("ignores already-inactive bullets", () => {
    const b = { active: false, x: 1, y: 1, vy: -1, fromPlayer: true };
    updateBullets([b], 600, 1);
    expect(b.y).toBe(1);
  });
});

describe("bulletHitsAlien / bulletHitsPlayer", () => {
  it("misses when the bullet is outside every alien box", () => {
    const aliens = createFormation(700, 48);
    const miss = bulletHitsAlien(spawnPlayerBullet(0, 0), aliens);
    expect(miss).toBe(-1);
  });
  it("ignores alien bullets and dead aliens for player-bullet collision", () => {
    const aliens = createFormation(700, 48);
    const t = aliens[0];
    // An alien bullet never hits an alien even if overlapping.
    expect(bulletHitsAlien(spawnAlienBullet(t.x + 4, t.y + 4), aliens)).toBe(-1);
    // A dead alien is skipped → the same spot hits nothing.
    t.alive = false;
    expect(bulletHitsAlien(spawnPlayerBullet(t.x + 4, t.y + 4), aliens)).toBe(-1);
  });
  it("hits exactly on the alien box edges (inclusive bounds)", () => {
    const aliens: Alien[] = [{ alive: true, row: 0, x: 100, y: 100 }];
    expect(bulletHitsAlien(spawnPlayerBullet(100, 100), aliens)).toBe(0); // top-left corner
    expect(bulletHitsAlien(spawnPlayerBullet(100 + ALIEN_W, 100 + ALIEN_H), aliens)).toBe(0); // bottom-right corner
    expect(bulletHitsAlien(spawnPlayerBullet(100 + ALIEN_W + 1, 100), aliens)).toBe(-1); // just outside
  });
  it("detects an alien bullet hitting the player and ignores player bullets", () => {
    const px = 350;
    const py = 560;
    expect(bulletHitsPlayer(spawnAlienBullet(px, py), px, py)).toBe(true);
    expect(bulletHitsPlayer(spawnAlienBullet(px + PLAYER_W, py), px, py)).toBe(false); // outside half-width
    expect(bulletHitsPlayer(spawnPlayerBullet(px, py), px, py)).toBe(false); // player's own bullet
  });
});

describe("aliensReachedPlayer", () => {
  it("is false high up and true once the formation descends onto the ship line", () => {
    const aliens = createFormation(700, 48);
    expect(aliensReachedPlayer(aliens, 560)).toBe(false);
    for (let i = 0; i < 40; i++) dropFormation(aliens);
    expect(aliensReachedPlayer(aliens, 560)).toBe(true);
  });
});

describe("alienScore", () => {
  it("falls back to the flat per-alien score for an out-of-range row", () => {
    expect(alienScore(99)).toBe(SCORE_PER_ALIEN);
    expect(alienScore(-1)).toBe(SCORE_PER_ALIEN);
  });
});

describe("pickShooter", () => {
  afterEach(() => vi.restoreAllMocks());

  it("returns null when nothing is alive", () => {
    expect(pickShooter([])).toBeNull();
    const aliens = createFormation(700, 48);
    aliens.forEach((a) => (a.alive = false));
    expect(pickShooter(aliens)).toBeNull();
  });

  it("prefers a bottom-row alien (deterministic via seeded Math.random)", () => {
    vi.spyOn(Math, "random").mockReturnValue(0);
    const aliens = createFormation(700, 48);
    const shooter = pickShooter(aliens)!;
    const { bottom } = formationExtents(aliens);
    // Its box bottom must equal the formation bottom → it's on the lowest rank.
    expect(shooter.y + ALIEN_H).toBe(bottom);
  });

  it("returns the sole survivor", () => {
    vi.spyOn(Math, "random").mockReturnValue(0.99);
    const aliens = createFormation(700, 48);
    aliens.forEach((a) => (a.alive = false));
    aliens[27].alive = true;
    expect(pickShooter(aliens)).toBe(aliens[27]);
  });
});

describe("formationWidth", () => {
  it("spans all columns with gaps only between them (no trailing gap)", () => {
    expect(formationWidth()).toBe(ALIEN_COLS * ALIEN_W + (ALIEN_COLS - 1) * ALIEN_GAP_X);
  });

  it("centres the created formation inside the field", () => {
    const fieldW = 800;
    const aliens = createFormation(fieldW, 40);
    const xs = aliens.map((a) => a.x);
    const left = Math.min(...xs);
    const right = Math.max(...xs) + ALIEN_W;
    // Equal margin on both sides → the row is centred.
    expect(left).toBeCloseTo(fieldW - right, 5);
    expect(right - left).toBe(formationWidth());
  });
});

describe("stepFormation — the ordinary (non-bouncing) frame", () => {
  // The existing wall tests use a formation that is already wider than the
  // field, so every call bounces. The common case — the formation simply
  // marching across a roomy field — was never exercised.
  const WIDE = formationWidth() + 400;

  it("marches by one step and reports no wall", () => {
    const aliens = createFormation(WIDE, 40);
    const before = aliens[0].x;
    const r = stepFormation(aliens, 1, WIDE, 1);
    expect(r).toEqual({ dir: 1, hitWall: false });
    expect(aliens[0].x).toBeCloseTo(before + FORMATION_STEP_X, 6);
  });

  it("scales the step with the frame time (frame-rate independence)", () => {
    const aliens = createFormation(WIDE, 40);
    const before = aliens[0].x;
    stepFormation(aliens, 1, WIDE, 2); // a frame that took twice as long
    expect(aliens[0].x).toBeCloseTo(before + FORMATION_STEP_X * 2, 6);
  });

  it("moves left when the direction is -1", () => {
    const aliens = createFormation(WIDE, 40);
    const before = aliens[0].x;
    const r = stepFormation(aliens, -1, WIDE, 1);
    expect(r.hitWall).toBe(false);
    expect(aliens[0].x).toBeCloseTo(before - FORMATION_STEP_X, 6);
  });

  it("only living aliens move", () => {
    const aliens = createFormation(WIDE, 40);
    aliens[0].alive = false;
    const deadX = aliens[0].x;
    stepFormation(aliens, 1, WIDE, 1);
    expect(aliens[0].x).toBe(deadX);
  });
});

describe("stepFormation — the direction guard stops bounce-lock", () => {
  // Each wall check is gated on the direction of travel. Without that gate a
  // formation sitting past a margin would flip every single frame and the
  // whole wave would be stuck vibrating against the wall.
  it("a formation already past the RIGHT margin keeps travelling left", () => {
    const FIELD = 800;
    const aliens = createFormation(FIELD, 40);
    for (const a of aliens) a.x += 200; // shove it over the right margin only
    expect(formationExtents(aliens).right).toBeGreaterThan(FIELD - 12);
    expect(formationExtents(aliens).left).toBeGreaterThan(12);

    // Moving away from the wall it is standing in → no reversal.
    const away = stepFormation(aliens, -1, FIELD, 1);
    expect(away).toEqual({ dir: -1, hitWall: false });

    // Moving into it → reversal.
    const into = stepFormation(aliens, 1, FIELD, 1);
    expect(into).toEqual({ dir: -1, hitWall: true });
  });

  it("a formation clear of both margins never reverses", () => {
    const WIDE = formationWidth() + 400;
    const aliens = createFormation(WIDE, 40);
    for (let i = 0; i < 20; i++) {
      expect(stepFormation(aliens, 1, WIDE, 1).dir).toBe(1);
    }
  });
});
