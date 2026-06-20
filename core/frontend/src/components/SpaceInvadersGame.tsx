import { useEffect, useRef, useState } from "react";
import { usePauseOnPopupHidden } from "../hooks/usePauseOnPopupHidden";
import {
  ALIEN_H,
  ALIEN_ROWS,
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
import { parseColor, mixRgb, rgba, clamp01, type Rgb } from "../lib/bpm-visual";
import {
  clearSavedGame,
  commitHighScore,
  loadHighScore,
  loadSavedGame,
  saveGame,
} from "../lib/game-storage";

const STORAGE_KEY = "spacer";

/** The slice of an in-progress game that survives an Esc → relaunch. */
interface SpaceSave {
  aliens: Alien[];
  bullets: Bullet[];
  playerX: number;
  playerY: number;
  formationDir: 1 | -1;
  score: number;
  lives: number;
}

interface Props {
  onExit: () => void;
}

type Phase = "intro" | "playing" | "over";

// ── Pixel-art invader sprites (classic shapes), two animation frames each ─────
const SQUID = [
  [
    "00011000",
    "00111100",
    "01111110",
    "11011011",
    "11111111",
    "00100100",
    "01011010",
    "10100101",
  ],
  [
    "00011000",
    "00111100",
    "01111110",
    "11011011",
    "11111111",
    "01000010",
    "10100101",
    "01000010",
  ],
];
const CRAB = [
  [
    "00100000100",
    "00010001000",
    "00111111100",
    "01101110110",
    "11111111111",
    "10111111101",
    "10100000101",
    "00011011000",
  ],
  [
    "00100000100",
    "10010001001",
    "10111111101",
    "11101110111",
    "11111111111",
    "00111111100",
    "00100000100",
    "01000000010",
  ],
];
const OCTOPUS = [
  [
    "000011110000",
    "011111111110",
    "111111111111",
    "111001100111",
    "111111111111",
    "000110011000",
    "001100001100",
    "011000000110",
  ],
  [
    "000011110000",
    "011111111110",
    "111111111111",
    "111001100111",
    "111111111111",
    "001100001100",
    "011000000110",
    "001100001100",
  ],
];
const PLAYER_BMP = [
  "0000001000000",
  "0000011100000",
  "0000011100000",
  "0111111111110",
  "1111111111111",
  "1111111111111",
  "0111111111110",
];

function spriteFor(row: number, frame: number): string[] {
  if (row === 0) return SQUID[frame];
  if (row <= 2) return CRAB[frame];
  return OCTOPUS[frame];
}

interface Sprite {
  cv: HTMLCanvasElement;
  coreW: number;
}
interface Particle {
  x: number;
  y: number;
  vx: number;
  vy: number;
  life: number;
  maxLife: number;
  color: Rgb;
}
interface Star {
  x: number;
  y: number;
  size: number;
  spd: number;
  a: number;
  tw: number;
  phase: number;
}

function readThemeColors() {
  const cs = getComputedStyle(document.documentElement);
  const v = (name: string, fallback: string) =>
    cs.getPropertyValue(name).trim() || fallback;
  return {
    bg: v("--color-bg", "#0c0d11"),
    fg: v("--color-fg", "#f2f3f5"),
    accent: v("--color-accent", "#b3c5ff"),
    border: v("--color-border", "#2b2e38"),
    muted: v("--color-muted", "#9a9fac"),
  };
}

export function SpaceInvadersGame({ onExit }: Props) {
  // Load any suspended game once (useState initializer).
  const [saved] = useState(() => loadSavedGame<SpaceSave>(STORAGE_KEY));

  const [phase, setPhase] = useState<Phase>(saved ? "playing" : "intro");
  const [playerWon, setPlayerWon] = useState(false);
  const [score, setScore] = useState(saved?.score ?? 0);
  const [lives, setLives] = useState(saved?.lives ?? INITIAL_LIVES);
  const [best, setBest] = useState(() => loadHighScore(STORAGE_KEY));
  const scoreRef = useRef(saved?.score ?? 0);
  const livesRef = useRef(saved?.lives ?? INITIAL_LIVES);
  // Resume gate: a resumed run starts frozen until the player presses a key,
  // so the loop doesn't advance before they're ready. Fresh games aren't gated.
  const [resumeGate, setResumeGate] = useState(Boolean(saved));
  const resumeGateRef = useRef(Boolean(saved));
  const liftGate = () => {
    resumeGateRef.current = false;
    setResumeGate(false);
  };
  // Clicking outside the overlay hides the popup mid-play — re-arm the gate so
  // reopening requires a keypress to continue (same as Esc-pause).
  usePauseOnPopupHidden(phase === "playing", () => {
    resumeGateRef.current = true;
    setResumeGate(true);
  });

  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  // A resumed game seeds its entities from the suspended run; otherwise a
  // fresh (empty) board the intro/reset fills in.
  const stateRef = useRef(
    saved
      ? {
          fieldW: 700,
          fieldH: 452,
          playerX: saved.playerX,
          playerY: saved.playerY,
          keys: { left: false, right: false, fire: false },
          formationDir: saved.formationDir,
          aliens: saved.aliens,
          bullets: saved.bullets,
          lastFireAt: 0,
          lastAlienShotAt: 0,
          lastTs: 0,
          running: true,
        }
      : {
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
        },
  );

  // Tells the play-loop effect to KEEP the restored player position on its
  // first mount instead of re-centring (fresh games always re-centre).
  const keepResumedPos = useRef(Boolean(saved));

  const resetMatch = () => {
    scoreRef.current = 0;
    livesRef.current = INITIAL_LIVES;
    setScore(0);
    setLives(INITIAL_LIVES);
    setPlayerWon(false);
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
    liftGate(); // a fresh match runs immediately
    setPhase("playing");
  };

  useEffect(() => {
    if (saved) return; // resumed — skip the intro, keep restored entities
    const t = window.setTimeout(() => {
      resetMatch();
    }, INTRO_MS);
    return () => window.clearTimeout(t);
    // resetMatch intentionally omitted — the intro runs once per mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [saved]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        // Suspend a live game so a re-trigger resumes it; a finished /
        // intro game has nothing worth keeping.
        const s = stateRef.current;
        if (phase === "playing") {
          saveGame<SpaceSave>(STORAGE_KEY, {
            aliens: s.aliens,
            bullets: s.bullets,
            playerX: s.playerX,
            playerY: s.playerY,
            formationDir: s.formationDir,
            score: scoreRef.current,
            lives: livesRef.current,
          });
        } else {
          clearSavedGame(STORAGE_KEY);
        }
        onExit();
        return;
      }
      if (phase === "over" && (e.key === " " || e.code === "Space")) {
        e.preventDefault();
        resetMatch();
        return;
      }
      // First key on a resumed run just un-freezes (doesn't also move/fire).
      if (phase === "playing" && resumeGateRef.current) {
        e.preventDefault();
        liftGate();
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
    // resetMatch / liftGate intentionally not deps (handler re-subscription
    // churn); the handler reads current phase via the dep + refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [phase, onExit]);

  useEffect(() => {
    if (phase !== "playing") return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const rect = canvas.getBoundingClientRect();
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    canvas.width = Math.round(rect.width * dpr);
    canvas.height = Math.round(rect.height * dpr);
    ctx.scale(dpr, dpr);
    const s = stateRef.current;
    s.fieldW = rect.width;
    s.fieldH = rect.height;
    if (keepResumedPos.current) {
      // First mount of a resumed game — the restored player position is
      // already in real field coords; don't snap it back to centre.
      keepResumedPos.current = false;
    } else {
      s.playerX = s.fieldW / 2;
      s.playerY = s.fieldH - 48;
      if (s.aliens.length === 0) {
        s.aliens = createFormation(s.fieldW, 56);
      }
    }

    // ── Theme-derived palette ────────────────────────────────────────────────
    const colors = readThemeColors();
    const accent = parseColor(colors.accent) ?? { r: 179, g: 197, b: 255 };
    const fg = parseColor(colors.fg) ?? { r: 242, g: 243, b: 245 };
    const muted = parseColor(colors.muted) ?? { r: 154, g: 159, b: 172 };
    const bg = parseColor(colors.bg) ?? { r: 12, g: 13, b: 17 };
    const white: Rgb = { r: 255, g: 255, b: 255 };
    const danger: Rgb = { r: 255, g: 110, b: 120 };
    const bgTop = mixRgb(bg, accent, 0.07);
    const bgBot = mixRgb(bg, { r: 0, g: 0, b: 0 }, 0.25);
    const accentBright = mixRgb(accent, white, 0.35);
    const starColor = mixRgb(fg, muted, 0.4);
    // Brighter near the top row (higher value), cooler toward the bottom.
    const rowColor = (row: number): Rgb => {
      const t = ALIEN_ROWS > 1 ? row / (ALIEN_ROWS - 1) : 0;
      return mixRgb(mixRgb(accent, white, 0.4 * (1 - t)), muted, 0.32 * t);
    };

    // ── Pre-render sprites (glow baked in once, so the per-frame loop is just
    //    cheap drawImage calls — no runtime shadowBlur over 55 aliens). ────────
    const PX = 4;
    const PAD = 7;
    const makeSprite = (bmp: string[], fill: Rgb, glow: Rgb): Sprite => {
      const rows = bmp.length;
      const cols = bmp[0].length;
      const cv = document.createElement("canvas");
      cv.width = cols * PX + PAD * 2;
      cv.height = rows * PX + PAD * 2;
      const c = cv.getContext("2d");
      if (c) {
        const paint = () => {
          for (let r = 0; r < rows; r++) {
            for (let col = 0; col < cols; col++) {
              if (bmp[r][col] === "1") {
                c.fillRect(PAD + col * PX, PAD + r * PX, PX + 0.6, PX + 0.6);
              }
            }
          }
        };
        // Pass 1: soft glow halo.
        c.shadowColor = rgba(glow, 0.85);
        c.shadowBlur = 7;
        c.fillStyle = rgba(fill, 1);
        paint();
        // Pass 2: crisp body on top of its own glow.
        c.shadowBlur = 0;
        c.fillStyle = rgba(mixRgb(fill, white, 0.15), 1);
        paint();
      }
      return { cv, coreW: cols * PX };
    };

    const sprites: Record<string, Sprite> = {};
    for (let row = 0; row < ALIEN_ROWS; row++) {
      const col = rowColor(row);
      for (let f = 0; f < 2; f++) {
        sprites[`${row}-${f}`] = makeSprite(spriteFor(row, f), col, col);
      }
    }
    const playerSprite = makeSprite(PLAYER_BMP, accentBright, accent);

    // ── Visual-only effects ──────────────────────────────────────────────────
    const stars: Star[] = Array.from({ length: 70 }, () => ({
      x: Math.random() * s.fieldW,
      y: Math.random() * s.fieldH,
      size: Math.random() < 0.8 ? 1 : 2,
      spd: 0.3 + Math.random() * 1.4,
      a: 0.25 + Math.random() * 0.5,
      tw: 0.6 + Math.random() * 1.6,
      phase: Math.random() * Math.PI * 2,
    }));
    let particles: Particle[] = [];
    let hitFlash = 0;
    let visLast = 0;

    const spawnBurst = (x: number, y: number, color: Rgb, n: number) => {
      for (let i = 0; i < n; i++) {
        const ang = Math.random() * Math.PI * 2;
        const spd = 0.6 + Math.random() * 3.2;
        particles.push({
          x,
          y,
          vx: Math.cos(ang) * spd,
          vy: Math.sin(ang) * spd - 0.6,
          life: 360 + Math.random() * 220,
          maxLife: 360,
          color,
        });
      }
    };

    let raf = 0;

    // End the game: stop the loop, record win/loss, finalise the persisted
    // high score, and drop the suspended-run blob (nothing to resume).
    const endRun = (won: boolean) => {
      s.running = false;
      setPlayerWon(won);
      setBest(commitHighScore(STORAGE_KEY, scoreRef.current));
      clearSavedGame(STORAGE_KEY);
      setPhase("over");
    };

    const capsule = (x: number, y: number, w: number, h: number) => {
      const r = w / 2;
      ctx.beginPath();
      ctx.moveTo(x, y + r);
      ctx.arc(x + r, y + r, r, Math.PI, 0);
      ctx.lineTo(x + w, y + h - r);
      ctx.arc(x + r, y + h - r, r, 0, Math.PI);
      ctx.closePath();
    };

    const render = (ts: number) => {
      const vdt = visLast ? Math.min(60, ts - visLast) : 16;
      visLast = ts;
      const w = s.fieldW;
      const h = s.fieldH;

      // Background gradient.
      const bgGrad = ctx.createLinearGradient(0, 0, 0, h);
      bgGrad.addColorStop(0, rgba(bgTop, 1));
      bgGrad.addColorStop(1, rgba(bgBot, 1));
      ctx.fillStyle = bgGrad;
      ctx.fillRect(0, 0, w, h);

      // Parallax starfield (drifts down, twinkles).
      for (const st of stars) {
        st.y += st.spd * vdt * 0.05;
        if (st.y > h) {
          st.y = 0;
          st.x = Math.random() * w;
        }
        const tw = 0.45 + 0.55 * (0.5 + 0.5 * Math.sin(ts * 0.002 * st.tw + st.phase));
        ctx.fillStyle = rgba(starColor, st.a * tw);
        ctx.fillRect(st.x, st.y, st.size, st.size);
      }

      // Aliens (cached glowing sprites + a gentle bob + leg-frame animation).
      const frame = Math.floor(ts / 420) % 2;
      for (const a of s.aliens) {
        if (!a.alive) continue;
        const spr = sprites[`${a.row}-${frame}`];
        const scale = ALIEN_W / spr.coreW;
        const dw = spr.cv.width * scale;
        const dh = spr.cv.height * scale;
        const bob = Math.sin(ts * 0.004 + a.x * 0.045) * 1.6;
        ctx.drawImage(spr.cv, a.x - PAD * scale, a.y + bob - PAD * scale, dw, dh);
      }

      // Player cannon + engine flame.
      {
        const scale = PLAYER_W / playerSprite.coreW;
        const dw = playerSprite.cv.width * scale;
        const dh = playerSprite.cv.height * scale;
        const px = s.playerX - PLAYER_W / 2 - PAD * scale;
        const py = s.playerY - PLAYER_H / 2 - PAD * scale;
        // Flame (flickering, additive).
        const flame = 6 + Math.random() * 6;
        const fy = s.playerY + PLAYER_H / 2 - 2;
        ctx.save();
        ctx.globalCompositeOperation = "lighter";
        const fGrad = ctx.createLinearGradient(0, fy, 0, fy + flame + 6);
        fGrad.addColorStop(0, rgba(accentBright, 0.85));
        fGrad.addColorStop(1, rgba(accent, 0));
        ctx.fillStyle = fGrad;
        ctx.beginPath();
        ctx.moveTo(s.playerX - 5, fy);
        ctx.lineTo(s.playerX, fy + flame + 6);
        ctx.lineTo(s.playerX + 5, fy);
        ctx.closePath();
        ctx.fill();
        ctx.restore();
        ctx.drawImage(playerSprite.cv, px, py, dw, dh);
      }

      // Bullets — glowing capsules (player accent, alien danger).
      for (const b of s.bullets) {
        if (!b.active) continue;
        const col = b.fromPlayer ? accentBright : danger;
        ctx.save();
        ctx.shadowColor = rgba(col, 0.95);
        ctx.shadowBlur = 9;
        ctx.fillStyle = rgba(col, 1);
        capsule(b.x - 2, b.y - 7, 4, 14);
        ctx.fill();
        ctx.restore();
      }

      // Explosion particles (additive bloom, fading + gravity).
      ctx.save();
      ctx.globalCompositeOperation = "lighter";
      for (const p of particles) {
        p.life -= vdt;
        if (p.life <= 0) continue;
        p.x += p.vx * vdt * 0.06;
        p.y += p.vy * vdt * 0.06;
        p.vy += 0.02 * vdt;
        const k = clamp01(p.life / p.maxLife);
        ctx.fillStyle = rgba(p.color, k);
        ctx.beginPath();
        ctx.arc(p.x, p.y, 1 + 2.4 * k, 0, Math.PI * 2);
        ctx.fill();
      }
      ctx.restore();
      particles = particles.filter((p) => p.life > 0);

      // Vignette for depth.
      const vg = ctx.createRadialGradient(w / 2, h / 2, h * 0.2, w / 2, h / 2, h * 0.78);
      vg.addColorStop(0, "rgba(0,0,0,0)");
      vg.addColorStop(1, "rgba(0,0,0,0.42)");
      ctx.fillStyle = vg;
      ctx.fillRect(0, 0, w, h);

      // Player-hit flash.
      if (hitFlash > 0) {
        hitFlash = Math.max(0, hitFlash - vdt * 0.004);
        ctx.fillStyle = rgba(danger, hitFlash * 0.3);
        ctx.fillRect(0, 0, w, h);
      }
    };

    const step = (ts: number) => {
      // Frozen behind the resume gate: keep visuals alive, don't advance logic.
      if (resumeGateRef.current) {
        s.lastTs = ts;
        render(ts);
        if (s.running) raf = requestAnimationFrame(step);
        return;
      }
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
            spawnAlienBullet(shooter.x + ALIEN_W / 2, shooter.y + ALIEN_H),
          );
          s.lastAlienShotAt = ts;
        }
      }

      updateBullets(s.bullets, s.fieldH, dt);

      for (const b of s.bullets) {
        const idx = bulletHitsAlien(b, s.aliens);
        if (idx >= 0) {
          const hit = s.aliens[idx];
          spawnBurst(hit.x + ALIEN_W / 2, hit.y + ALIEN_H / 2, rowColor(hit.row), 12);
          hit.alive = false;
          b.active = false;
          scoreRef.current += alienScore(hit.row);
          setScore(scoreRef.current);
        }
        if (bulletHitsPlayer(b, s.playerX, s.playerY)) {
          b.active = false;
          hitFlash = 1;
          spawnBurst(s.playerX, s.playerY, { r: 255, g: 110, b: 120 }, 18);
          livesRef.current -= 1;
          setLives(livesRef.current);
          s.bullets = s.bullets.filter((x) => !x.active || x.fromPlayer);
          if (livesRef.current <= 0) {
            endRun(false);
            return;
          }
        }
      }

      s.bullets = s.bullets.filter((b) => b.active);

      if (allDead(s.aliens)) {
        endRun(true);
        return;
      }

      if (aliensReachedPlayer(s.aliens, s.playerY)) {
        endRun(false);
        return;
      }

      render(ts);
      if (s.running) raf = requestAnimationFrame(step);
    };

    s.running = true;
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [phase]);

  const won = phase === "over" && playerWon;

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
        <span className="font-[var(--font-mono)] text-[18px] font-bold tabular-nums">
          <span className="text-[var(--color-fg)]">{score}</span>
          <span className="px-2 text-[var(--color-muted)]">·</span>
          <span className="text-[12px] text-[var(--color-muted)]">best {best}</span>
        </span>
        <span className="flex items-center gap-1.5 text-[11px] text-[var(--color-muted)]">
          {/* Lives as little ship glyphs */}
          <span className="mr-0.5">Lives</span>
          {Array.from({ length: Math.max(0, lives) }).map((_, i) => (
            <span key={i} className="text-[13px] leading-none text-[var(--color-accent)]">
              ▲
            </span>
          ))}
          <span className="mx-1">·</span>
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
              SPACER
            </span>
          </div>
        )}

        {phase === "playing" && resumeGate && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
            <span className="rounded-full border border-[var(--color-border)] bg-[var(--color-surface)]/85 px-4 py-1.5 text-[13px] text-[var(--color-fg)] backdrop-blur-sm">
              ▸ Resumed — press a key to continue
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
              Score {score} &nbsp;·&nbsp; best {best}
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
