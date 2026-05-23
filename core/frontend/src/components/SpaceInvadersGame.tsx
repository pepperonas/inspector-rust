import { useEffect, useRef, useState } from "react";
import {
  ALIEN_H,
  ALIEN_SHOOT_INTERVAL_MS,
  ALIEN_W,
  FIRE_COOLDOWN_MS,
  INITIAL_LIVES,
  INTRO_MS,
  PLAYER_H,
  PLAYER_W,
  alienScore,
  allDead,
  aliensReachedPlayer,
  bulletHitsAlien,
  bulletHitsPlayer,
  createFormation,
  dropFormation,
  frameScale,
  movePlayer,
  pickShooter,
  spawnAlienBullet,
  spawnPlayerBullet,
  stepFormation,
  updateBullets,
  type Alien,
  type Bullet,
} from "../lib/space-invaders";

interface Props {
  onExit: () => void;
}

type Phase = "intro" | "playing" | "over";

function readThemeColors() {
  const cs = getComputedStyle(document.documentElement);
  const v = (name: string, fallback: string) =>
    cs.getPropertyValue(name).trim() || fallback;
  return {
    bg: v("--color-bg", "#0c0d11"),
    fg: v("--color-fg", "#f2f3f5"),
    accent: v("--color-accent", "#6366f1"),
    border: v("--color-border", "#2b2e38"),
    muted: v("--color-muted", "#9a9fac"),
    danger: "#f87171",
  };
}

export function SpaceInvadersGame({ onExit }: Props) {
  const [phase, setPhase] = useState<Phase>("intro");
  const [score, setScore] = useState(0);
  const [lives, setLives] = useState(INITIAL_LIVES);
  const scoreRef = useRef(0);
  const livesRef = useRef(INITIAL_LIVES);

  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const stateRef = useRef({
    fieldW: 700,
    fieldH: 452,
    playerX: 350,
    playerY: 400,
    keys: { left: false, right: false, fire: false },
    formationDir: 1 as 1 | -1,
    aliens: [] as Alien[],
    bullets: [] as Bullet[],
    lastFireAt: 0,
    lastAlienShotAt: 0,
    lastTs: 0,
    running: false,
  });

  const resetMatch = () => {
    scoreRef.current = 0;
    livesRef.current = INITIAL_LIVES;
    setScore(0);
    setLives(INITIAL_LIVES);
    const s = stateRef.current;
    s.playerX = s.fieldW / 2;
    s.playerY = s.fieldH - 48;
    s.formationDir = 1;
    s.aliens = createFormation(s.fieldW, 56);
    s.bullets = [];
    s.lastFireAt = 0;
    s.lastAlienShotAt = 0;
    s.lastTs = 0;
    s.running = true;
    setPhase("playing");
  };

  useEffect(() => {
    const t = window.setTimeout(() => {
      resetMatch();
    }, INTRO_MS);
    return () => window.clearTimeout(t);
  }, []);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
        return;
      }
      if (phase === "over" && (e.key === " " || e.code === "Space")) {
        e.preventDefault();
        resetMatch();
        return;
      }
      const s = stateRef.current;
      if (e.key === "ArrowLeft" || e.key === "a" || e.key === "A") s.keys.left = true;
      if (e.key === "ArrowRight" || e.key === "d" || e.key === "D") s.keys.right = true;
      if (e.key === " " || e.key === "ArrowUp" || e.key === "w" || e.key === "W") {
        s.keys.fire = true;
      }
    };
    const onKeyUp = (e: KeyboardEvent) => {
      const s = stateRef.current;
      if (e.key === "ArrowLeft" || e.key === "a" || e.key === "A") s.keys.left = false;
      if (e.key === "ArrowRight" || e.key === "d" || e.key === "D") s.keys.right = false;
      if (e.key === " " || e.key === "ArrowUp" || e.key === "w" || e.key === "W") {
        s.keys.fire = false;
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
    };
  }, [phase, onExit]);

  useEffect(() => {
    if (phase !== "playing") return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width;
    canvas.height = rect.height;
    const s = stateRef.current;
    s.fieldW = canvas.width;
    s.fieldH = canvas.height;
    s.playerX = s.fieldW / 2;
    s.playerY = s.fieldH - 48;
    if (s.aliens.length === 0) {
      s.aliens = createFormation(s.fieldW, 56);
    }

    const colors = readThemeColors();
    let raf = 0;

    const drawAlien = (a: Alien) => {
      if (!a.alive) return;
      const cx = a.x + ALIEN_W / 2;
      const cy = a.y + ALIEN_H / 2;
      ctx.fillStyle = a.row < 2 ? colors.danger : colors.muted;
      ctx.beginPath();
      ctx.ellipse(cx, cy, ALIEN_W / 2 - 2, ALIEN_H / 2 - 2, 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = colors.bg;
      ctx.fillRect(cx - 6, cy - 2, 4, 4);
      ctx.fillRect(cx + 2, cy - 2, 4, 4);
    };

    const render = () => {
      ctx.fillStyle = colors.bg;
      ctx.fillRect(0, 0, s.fieldW, s.fieldH);

      for (const a of s.aliens) drawAlien(a);

      ctx.fillStyle = colors.accent;
      ctx.beginPath();
      ctx.moveTo(s.playerX, s.playerY - PLAYER_H / 2);
      ctx.lineTo(s.playerX - PLAYER_W / 2, s.playerY + PLAYER_H / 2);
      ctx.lineTo(s.playerX + PLAYER_W / 2, s.playerY + PLAYER_H / 2);
      ctx.closePath();
      ctx.fill();

      for (const b of s.bullets) {
        if (!b.active) continue;
        ctx.fillStyle = b.fromPlayer ? colors.accent : colors.danger;
        ctx.fillRect(b.x - 2, b.y - 6, 4, 10);
      }
    };

    const step = (ts: number) => {
      const dt = s.lastTs === 0 ? 1 : frameScale(ts - s.lastTs);
      s.lastTs = ts;

      s.playerX = movePlayer(s.playerX, s.keys, s.fieldW, dt);

      if (s.keys.fire && ts - s.lastFireAt >= FIRE_COOLDOWN_MS) {
        const activePlayerBullets = s.bullets.filter(
          (b) => b.active && b.fromPlayer,
        ).length;
        if (activePlayerBullets < 2) {
          s.bullets.push(
            spawnPlayerBullet(s.playerX, s.playerY - PLAYER_H / 2 - 4),
          );
          s.lastFireAt = ts;
        }
      }

      const move = stepFormation(s.aliens, s.formationDir, s.fieldW, dt);
      s.formationDir = move.dir;
      if (move.hitWall) dropFormation(s.aliens);

      if (ts - s.lastAlienShotAt >= ALIEN_SHOOT_INTERVAL_MS) {
        const shooter = pickShooter(s.aliens);
        if (shooter) {
          s.bullets.push(
            spawnAlienBullet(
              shooter.x + ALIEN_W / 2,
              shooter.y + ALIEN_H,
            ),
          );
          s.lastAlienShotAt = ts;
        }
      }

      updateBullets(s.bullets, s.fieldH, dt);

      for (const b of s.bullets) {
        const idx = bulletHitsAlien(b, s.aliens);
        if (idx >= 0) {
          const row = s.aliens[idx].row;
          s.aliens[idx].alive = false;
          b.active = false;
          scoreRef.current += alienScore(row);
          setScore(scoreRef.current);
        }
        if (bulletHitsPlayer(b, s.playerX, s.playerY)) {
          b.active = false;
          livesRef.current -= 1;
          setLives(livesRef.current);
          s.bullets = s.bullets.filter((x) => !x.active || x.fromPlayer);
          if (livesRef.current <= 0) {
            s.running = false;
            setPhase("over");
            return;
          }
        }
      }

      s.bullets = s.bullets.filter((b) => b.active);

      if (allDead(s.aliens)) {
        s.running = false;
        setPhase("over");
        return;
      }

      if (aliensReachedPlayer(s.aliens, s.playerY)) {
        s.running = false;
        setPhase("over");
        return;
      }

      render();
      if (s.running) raf = requestAnimationFrame(step);
    };

    s.running = true;
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [phase]);

  const won = phase === "over" && allDead(stateRef.current.aliens) && lives > 0;

  return (
    <div
      className={
        "flex h-full w-full flex-col bg-[var(--color-bg)] " +
        (phase === "intro" ? "space-invaders-descend" : "")
      }
    >
      <div className="flex h-12 shrink-0 items-center justify-between border-b border-[var(--color-border)] px-4">
        <span className="font-[var(--font-mono)] text-[12px] font-semibold uppercase tracking-[0.2em] text-[var(--color-accent)]">
          Space
        </span>
        <span className="font-[var(--font-mono)] text-[18px] font-bold tabular-nums text-[var(--color-fg)]">
          {score}
        </span>
        <span className="text-[11px] text-[var(--color-muted)]">
          Lives:{" "}
          <span className="text-[var(--color-accent)]">{lives}</span>
          &nbsp;·&nbsp;{" "}
          <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">
            Esc
          </kbd>{" "}
          quit
        </span>
      </div>

      <div className="relative min-h-0 flex-1">
        <canvas ref={canvasRef} className="h-full w-full" />

        {phase === "intro" && (
          <div className="absolute inset-0 flex items-center justify-center">
            <span className="space-invaders-title font-[var(--font-mono)] text-[44px] font-black uppercase tracking-[0.35em] text-[var(--color-accent)]">
              SPACE
            </span>
          </div>
        )}

        {phase === "over" && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-[var(--color-bg)]/80 backdrop-blur-sm">
            <span
              className={
                "font-[var(--font-mono)] text-[32px] font-black uppercase tracking-tight " +
                (won ? "text-[var(--color-accent)]" : "text-[var(--color-muted)]")
              }
            >
              {won ? "Earth saved 👾" : "Invaders win"}
            </span>
            <span className="font-[var(--font-mono)] text-[16px] tabular-nums text-[var(--color-fg)]">
              Score {score}
            </span>
            <span className="mt-1 text-[12px] text-[var(--color-muted)]">
              <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">
                Space
              </kbd>{" "}
              rematch &nbsp;·&nbsp;{" "}
              <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">
                Esc
              </kbd>{" "}
              quit
            </span>
          </div>
        )}
      </div>
    </div>
  );
}
