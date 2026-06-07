/**
 * Pure, testable game logic for the `space` Space Invaders easter egg.
 *
 * The stateful canvas render loop lives in `components/SpaceInvadersGame.tsx`.
 */

export const INTRO_MS = 1400;
export const WIN_SCORE_NOTE = "all invaders destroyed";

export const PLAYER_W = 44;
export const PLAYER_H = 16;
export const PLAYER_SPEED = 7;
export const BULLET_SPEED = 11;
export const ALIEN_BULLET_SPEED = 7;
export const FIRE_COOLDOWN_MS = 280;
export const ALIEN_SHOOT_INTERVAL_MS = 850;

export const ALIEN_COLS = 11;
export const ALIEN_ROWS = 5;
export const ALIEN_W = 26;
export const ALIEN_H = 18;
export const ALIEN_GAP_X = 10;
export const ALIEN_GAP_Y = 10;
export const FORMATION_STEP_X = 1.8;
export const FORMATION_DROP = 16;

export const INITIAL_LIVES = 3;
export const SCORE_PER_ALIEN = 10;
export const SCORE_ROW_BONUS = [30, 20, 20, 10, 10]; // top row worth more

export const REFERENCE_FRAME_MS = 1000 / 60;

export function frameScale(dtMs: number): number {
  const scale = dtMs / REFERENCE_FRAME_MS;
  return scale > 2.5 ? 2.5 : scale < 0 ? 0 : scale;
}

export function clamp(n: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, n));
}

export interface Alien {
  alive: boolean;
  x: number;
  y: number;
  row: number;
}

export interface Bullet {
  active: boolean;
  x: number;
  y: number;
  vy: number;
  fromPlayer: boolean;
}

export function formationWidth(): number {
  return ALIEN_COLS * ALIEN_W + (ALIEN_COLS - 1) * ALIEN_GAP_X;
}

/** Spawn the classic 5×11 grid, centred horizontally. */
export function createFormation(fieldW: number, topY: number): Alien[] {
  const totalW = formationWidth();
  const startX = (fieldW - totalW) / 2;
  const aliens: Alien[] = [];
  for (let row = 0; row < ALIEN_ROWS; row++) {
    for (let col = 0; col < ALIEN_COLS; col++) {
      aliens.push({
        alive: true,
        row,
        x: startX + col * (ALIEN_W + ALIEN_GAP_X),
        y: topY + row * (ALIEN_H + ALIEN_GAP_Y),
      });
    }
  }
  return aliens;
}

export function aliveAliens(aliens: Alien[]): Alien[] {
  return aliens.filter((a) => a.alive);
}

export function allDead(aliens: Alien[]): boolean {
  return aliens.every((a) => !a.alive);
}

export function formationExtents(aliens: Alien[]): {
  left: number;
  right: number;
  bottom: number;
} {
  const live = aliveAliens(aliens);
  if (live.length === 0) {
    return { left: 0, right: 0, bottom: 0 };
  }
  let left = Infinity;
  let right = -Infinity;
  let bottom = -Infinity;
  for (const a of live) {
    left = Math.min(left, a.x);
    right = Math.max(right, a.x + ALIEN_W);
    bottom = Math.max(bottom, a.y + ALIEN_H);
  }
  return { left, right, bottom };
}

/** Move the whole formation horizontally; returns whether a wall was hit. */
export function stepFormation(
  aliens: Alien[],
  dir: 1 | -1,
  fieldW: number,
  scale: number,
): { dir: 1 | -1; hitWall: boolean } {
  const step = FORMATION_STEP_X * dir * scale;
  for (const a of aliens) {
    if (a.alive) a.x += step;
  }
  const { left, right } = formationExtents(aliens);
  const margin = 12;
  if (left < margin && dir < 0) {
    return { dir: 1, hitWall: true };
  }
  if (right > fieldW - margin && dir > 0) {
    return { dir: -1, hitWall: true };
  }
  return { dir, hitWall: false };
}

export function dropFormation(aliens: Alien[]): void {
  for (const a of aliens) {
    if (a.alive) a.y += FORMATION_DROP;
  }
}

export function aliensReachedPlayer(aliens: Alien[], playerY: number): boolean {
  const { bottom } = formationExtents(aliens);
  return bottom >= playerY - 8;
}

export function movePlayer(
  x: number,
  keys: { left: boolean; right: boolean },
  fieldW: number,
  scale: number,
): number {
  const speed = PLAYER_SPEED * scale;
  let next = x;
  if (keys.left) next -= speed;
  if (keys.right) next += speed;
  return clamp(next, PLAYER_W / 2 + 8, fieldW - PLAYER_W / 2 - 8);
}

export function updateBullets(
  bullets: Bullet[],
  fieldH: number,
  scale: number,
): void {
  for (const b of bullets) {
    if (!b.active) continue;
    const speed = (b.fromPlayer ? BULLET_SPEED : ALIEN_BULLET_SPEED) * scale;
    b.y += b.vy * speed;
    if (b.y < -8 || b.y > fieldH + 8) b.active = false;
  }
}

export function bulletHitsAlien(b: Bullet, aliens: Alien[]): number {
  if (!b.active || !b.fromPlayer) return -1;
  for (let i = 0; i < aliens.length; i++) {
    const a = aliens[i];
    if (!a.alive) continue;
    if (
      b.x >= a.x &&
      b.x <= a.x + ALIEN_W &&
      b.y >= a.y &&
      b.y <= a.y + ALIEN_H
    ) {
      return i;
    }
  }
  return -1;
}

export function bulletHitsPlayer(
  b: Bullet,
  playerX: number,
  playerY: number,
): boolean {
  if (!b.active || b.fromPlayer) return false;
  return (
    b.x >= playerX - PLAYER_W / 2 &&
    b.x <= playerX + PLAYER_W / 2 &&
    b.y >= playerY - PLAYER_H / 2 &&
    b.y <= playerY + PLAYER_H / 2
  );
}

export function pickShooter(aliens: Alien[]): Alien | null {
  const live = aliveAliens(aliens);
  if (live.length === 0) return null;
  // Prefer bottom-row invaders (classic behaviour).
  const maxY = Math.max(...live.map((a) => a.y));
  const bottom = live.filter((a) => a.y >= maxY - 2);
  const pool = bottom.length > 0 ? bottom : live;
  return pool[Math.floor(Math.random() * pool.length)] ?? null;
}

export function alienScore(row: number): number {
  return SCORE_ROW_BONUS[row] ?? SCORE_PER_ALIEN;
}

export function spawnPlayerBullet(x: number, y: number): Bullet {
  return { active: true, x, y, vy: -1, fromPlayer: true };
}

export function spawnAlienBullet(x: number, y: number): Bullet {
  return { active: true, x, y, vy: 1, fromPlayer: false };
}
