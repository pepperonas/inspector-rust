import { useEffect, useRef, useState } from "react";
import {
  GRID_COLS,
  GRID_ROWS,
  INTRO_MS,
  type Direction,
  type Point,
  clamp,
  dirDelta,
  initialSnake,
  isOpposite,
  spawnFood,
  step,
  tickInterval,
} from "../lib/snake";

/**
 * `rockthebox` easter egg — the popup overlay transforms (with a
 * box-assembling intro flourish) into a game of Snake. Steer with the
 * arrow keys or WASD; eat the food to grow; a wall or your own tail
 * ends the run. Esc is the only way out.
 *
 * Entirely client-side: a `<canvas>` + requestAnimationFrame loop.
 * Pure maths lives in `lib/snake.ts`; this component owns the stateful
 * render loop, input, HUD, and the intro/over phases.
 */

interface Props {
  /** Called when the user presses Esc — App.tsx returns to the popup. */
  onExit: () => void;
}

type Phase = "intro" | "playing" | "over";

/** Theme-aware colours, read once from the live CSS custom properties
 *  so the board matches whatever theme the app is in. */
function readThemeColors() {
  const cs = getComputedStyle(document.documentElement);
  const v = (name: string, fallback: string) =>
    cs.getPropertyValue(name).trim() || fallback;
  return {
    bg: v("--color-bg", "#0c0d11"),
    surface: v("--color-surface", "#15171d"),
    fg: v("--color-fg", "#f2f3f5"),
    accent: v("--color-accent", "#6366f1"),
    border: v("--color-border", "#2b2e38"),
    muted: v("--color-muted", "#9a9fac"),
  };
}
type Colors = ReturnType<typeof readThemeColors>;

// ── Easing ──────────────────────────────────────────────────────────────
const easeOutCubic = (t: number) => 1 - Math.pow(1 - t, 3);
const easeOutBack = (t: number) => {
  const c1 = 1.70158;
  const c3 = c1 + 1;
  return 1 + c3 * Math.pow(t - 1, 3) + c1 * Math.pow(t - 1, 2);
};

// ── Geometry ────────────────────────────────────────────────────────────
interface Geom {
  cell: number;
  boardW: number;
  boardH: number;
  offX: number;
  offY: number;
}
/** Letterbox the GRID_COLS × GRID_ROWS board inside the canvas: pick the
 *  largest integer cell size that fits, then centre the board. */
function computeGeom(w: number, h: number): Geom {
  const cell = Math.max(4, Math.floor(Math.min(w / GRID_COLS, h / GRID_ROWS)));
  const boardW = cell * GRID_COLS;
  const boardH = cell * GRID_ROWS;
  return {
    cell,
    boardW,
    boardH,
    offX: Math.floor((w - boardW) / 2),
    offY: Math.floor((h - boardH) / 2),
  };
}

/** Filled rounded rectangle — `arcTo` is universally supported, unlike
 *  the newer `ctx.roundRect`. */
function fillRoundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
) {
  const rr = Math.max(0, Math.min(r, w / 2, h / 2));
  ctx.beginPath();
  ctx.moveTo(x + rr, y);
  ctx.arcTo(x + w, y, x + w, y + h, rr);
  ctx.arcTo(x + w, y + h, x, y + h, rr);
  ctx.arcTo(x, y + h, x, y, rr);
  ctx.arcTo(x, y, x + w, y, rr);
  ctx.closePath();
  ctx.fill();
}

/** Pixel centre of a grid cell. */
function cellCenter(geom: Geom, p: Point) {
  return {
    cx: geom.offX + p.x * geom.cell + geom.cell / 2,
    cy: geom.offY + p.y * geom.cell + geom.cell / 2,
  };
}

// ── Drawing primitives ──────────────────────────────────────────────────
function drawBoard(
  ctx: CanvasRenderingContext2D,
  geom: Geom,
  colors: Colors,
  alpha: number,
) {
  ctx.globalAlpha = alpha;
  ctx.fillStyle = colors.surface;
  ctx.fillRect(geom.offX, geom.offY, geom.boardW, geom.boardH);
  ctx.globalAlpha = 1;
}

function drawGridDots(
  ctx: CanvasRenderingContext2D,
  geom: Geom,
  colors: Colors,
  alphaAt: (x: number, y: number) => number,
) {
  ctx.fillStyle = colors.muted;
  const r = Math.max(0.8, geom.cell * 0.045);
  for (let y = 0; y < GRID_ROWS; y++) {
    for (let x = 0; x < GRID_COLS; x++) {
      const a = alphaAt(x, y);
      if (a <= 0) continue;
      ctx.globalAlpha = a;
      const { cx, cy } = cellCenter(geom, { x, y });
      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      ctx.fill();
    }
  }
  ctx.globalAlpha = 1;
}

function drawSegment(
  ctx: CanvasRenderingContext2D,
  geom: Geom,
  p: Point,
  scale: number,
  radiusFrac: number,
) {
  if (scale <= 0) return;
  const gap = geom.cell * 0.1;
  const size = (geom.cell - gap * 2) * scale;
  const { cx, cy } = cellCenter(geom, p);
  fillRoundRect(ctx, cx - size / 2, cy - size / 2, size, size, size * radiusFrac);
}

/** Two eyes on the head, looking the way the snake travels. */
function drawEyes(
  ctx: CanvasRenderingContext2D,
  geom: Geom,
  head: Point,
  dir: Direction,
  colors: Colors,
) {
  const { cx, cy } = cellCenter(geom, head);
  const d = dirDelta(dir);
  const fwd = geom.cell * 0.16;
  const spread = geom.cell * 0.2;
  const r = Math.max(1.4, geom.cell * 0.085);
  // Perpendicular to travel — the two eyes sit either side of it.
  const perpX = -d.y;
  const perpY = d.x;
  ctx.fillStyle = colors.bg;
  for (const sgn of [-1, 1]) {
    const ex = cx + d.x * fwd + perpX * spread * sgn;
    const ey = cy + d.y * fwd + perpY * spread * sgn;
    ctx.beginPath();
    ctx.arc(ex, ey, r, 0, Math.PI * 2);
    ctx.fill();
  }
}

function drawSnake(
  ctx: CanvasRenderingContext2D,
  geom: Geom,
  colors: Colors,
  snake: Point[],
  dir: Direction,
) {
  const n = snake.length;
  ctx.fillStyle = colors.accent;
  snake.forEach((seg, i) => {
    // Head full opacity; body fades gently toward the tail.
    ctx.globalAlpha = i === 0 ? 1 : clamp(1 - (i / n) * 0.55, 0.42, 1);
    drawSegment(ctx, geom, seg, 1, i === 0 ? 0.34 : 0.3);
  });
  ctx.globalAlpha = 1;
  drawEyes(ctx, geom, snake[0], dir, colors);
}

/** Glowing food pellet with an optional pulse (0–1). */
function drawFood(
  ctx: CanvasRenderingContext2D,
  geom: Geom,
  food: Point,
  colors: Colors,
  scale: number,
  pulse: number,
) {
  if (scale <= 0) return;
  const { cx, cy } = cellCenter(geom, food);
  const r = geom.cell * 0.32 * scale * (1 + 0.12 * pulse);
  ctx.save();
  ctx.shadowColor = colors.fg;
  ctx.shadowBlur = geom.cell * 0.5 * (0.6 + 0.4 * pulse);
  ctx.fillStyle = colors.fg;
  ctx.beginPath();
  ctx.arc(cx, cy, r, 0, Math.PI * 2);
  ctx.fill();
  ctx.restore();
  // Sparkle highlight.
  ctx.globalAlpha = 0.65;
  ctx.fillStyle = colors.bg;
  ctx.beginPath();
  ctx.arc(cx - r * 0.32, cy - r * 0.32, r * 0.24, 0, Math.PI * 2);
  ctx.fill();
  ctx.globalAlpha = 1;
}

// ── Frame renderers ─────────────────────────────────────────────────────
function renderGame(
  ctx: CanvasRenderingContext2D,
  canvas: HTMLCanvasElement,
  geom: Geom,
  colors: Colors,
  snake: Point[],
  dir: Direction,
  food: Point,
  ts: number,
) {
  ctx.fillStyle = colors.bg;
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  drawBoard(ctx, geom, colors, 1);
  ctx.strokeStyle = colors.border;
  ctx.lineWidth = 2;
  ctx.strokeRect(geom.offX + 1, geom.offY + 1, geom.boardW - 2, geom.boardH - 2);
  drawGridDots(ctx, geom, colors, () => 0.12);
  drawFood(ctx, geom, food, colors, 1, 0.5 + 0.5 * Math.sin(ts / 220));
  drawSnake(ctx, geom, colors, snake, dir);
}

/**
 * The intro flourish — a timeline of overlapping sub-animations that
 * "builds the box", then wakes the snake:
 *   1. the board fills in,
 *   2. a glowing outline draws itself clockwise around the box,
 *   3. the grid dots sweep in on a diagonal wave,
 *   4. the snake's segments pop into place one by one (head first),
 *   5. the food drops in with an expanding ring.
 * `t` is elapsed ms, clamped to [0, INTRO_MS].
 */
function renderIntro(
  ctx: CanvasRenderingContext2D,
  canvas: HTMLCanvasElement,
  geom: Geom,
  colors: Colors,
  snake: Point[],
  dir: Direction,
  food: Point,
  t: number,
) {
  ctx.fillStyle = colors.bg;
  ctx.fillRect(0, 0, canvas.width, canvas.height);

  // 1) Board surface fades in.
  drawBoard(ctx, geom, colors, easeOutCubic(clamp((t - 460) / 360, 0, 1)));

  // 2) Glowing outline draws on, clockwise from the top-left corner.
  //    A single dash the length of the revealed perimeter + a long gap
  //    makes `strokeRect` paint itself progressively.
  const boxP = easeOutCubic(clamp((t - 80) / 540, 0, 1));
  if (boxP > 0) {
    const x = geom.offX + 1;
    const y = geom.offY + 1;
    const w = geom.boardW - 2;
    const h = geom.boardH - 2;
    const perim = 2 * (w + h);
    ctx.save();
    ctx.strokeStyle = colors.accent;
    ctx.lineWidth = 2.5;
    ctx.shadowColor = colors.accent;
    ctx.shadowBlur = 14;
    ctx.setLineDash([perim * boxP, perim]);
    ctx.strokeRect(x, y, w, h);
    ctx.restore();
    ctx.setLineDash([]);
  }

  // 3) Grid dots sweep in on a diagonal wave.
  drawGridDots(ctx, geom, colors, (x, y) => {
    const cellDelay = ((x + y) / (GRID_COLS + GRID_ROWS)) * 460;
    return clamp((t - 600 - cellDelay) / 260, 0, 1) * 0.16;
  });

  // 4) Snake segments pop into place, head first, with a back-ease bounce.
  const POP = 320;
  ctx.fillStyle = colors.accent;
  snake.forEach((seg, i) => {
    const local = (t - (950 + i * 90)) / POP;
    if (local <= 0) return;
    const scale = local >= 1 ? 1 : easeOutBack(clamp(local, 0, 1));
    ctx.globalAlpha = i === 0 ? 1 : clamp(1 - (i / snake.length) * 0.55, 0.42, 1);
    drawSegment(ctx, geom, seg, scale, i === 0 ? 0.34 : 0.3);
  });
  ctx.globalAlpha = 1;
  if (t > 950 + POP) drawEyes(ctx, geom, snake[0], dir, colors);

  // 5) Food drops in with an expanding ring.
  const foodLocal = (t - 1450) / 300;
  if (foodLocal > 0) {
    const ring = clamp((t - 1450) / 430, 0, 1);
    if (ring < 1) {
      const { cx, cy } = cellCenter(geom, food);
      ctx.save();
      ctx.globalAlpha = (1 - ring) * 0.6;
      ctx.strokeStyle = colors.fg;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.arc(cx, cy, geom.cell * 0.3 + ring * geom.cell * 1.3, 0, Math.PI * 2);
      ctx.stroke();
      ctx.restore();
    }
    const scale = foodLocal >= 1 ? 1 : easeOutBack(clamp(foodLocal, 0, 1));
    drawFood(ctx, geom, food, colors, scale, 0.5);
  }
}

/** Mutable game state — kept out of React so the render loop never
 *  triggers a re-render. */
interface SnakeState {
  snake: Point[];
  dir: Direction;
  pendingDir: Direction;
  food: Point;
  /** Accumulated wall-clock ms since the last tick. */
  acc: number;
  /** Timestamp of the previous frame, for the delta. */
  lastTs: number;
  running: boolean;
}

export function SnakeGame({ onExit }: Props) {
  const [phase, setPhase] = useState<Phase>("intro");
  // Score (food eaten) + session best — React state for the HUD; the
  // game loop reads the score via a ref so it always sees the freshest
  // value without re-subscribing.
  const [score, setScore] = useState(0);
  const [best, setBest] = useState(0);
  const scoreRef = useRef(0);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  // Lazy-initialised once — see SnakeState.
  const stateRef = useRef<SnakeState>(null!);
  if (stateRef.current === null) {
    const snake = initialSnake();
    stateRef.current = {
      snake,
      dir: "right",
      pendingDir: "right",
      food: spawnFood(snake) ?? { x: 0, y: 0 },
      acc: 0,
      lastTs: 0,
      running: false,
    };
  }

  // Reset everything and start a fresh game. Declared before the effects
  // so the keydown handler's Space-rematch path can call it. A rematch
  // skips the intro — straight back to playing.
  const restart = () => {
    const s = stateRef.current;
    const snake = initialSnake();
    s.snake = snake;
    s.dir = "right";
    s.pendingDir = "right";
    s.food = spawnFood(snake) ?? { x: 0, y: 0 };
    s.acc = 0;
    s.lastTs = 0;
    s.running = true;
    scoreRef.current = 0;
    setScore(0);
    setPhase("playing");
  };

  // ── Esc to quit, Space to rematch, arrows / WASD to steer. ───────────
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
        restart();
        return;
      }
      if (phase !== "playing") return;
      let nd: Direction | null = null;
      if (e.key === "ArrowUp" || e.key === "w" || e.key === "W") nd = "up";
      else if (e.key === "ArrowDown" || e.key === "s" || e.key === "S") nd = "down";
      else if (e.key === "ArrowLeft" || e.key === "a" || e.key === "A") nd = "left";
      else if (e.key === "ArrowRight" || e.key === "d" || e.key === "D") nd = "right";
      if (nd) {
        e.preventDefault();
        // Buffer the input; the tick commits it. Reject a straight
        // reversal — it would drive the head into its own neck.
        const s = stateRef.current;
        if (!isOpposite(s.dir, nd)) s.pendingDir = nd;
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [phase, onExit]);

  // ── Intro animation — runs while phase === "intro". ──────────────────
  useEffect(() => {
    if (phase !== "intro") return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width;
    canvas.height = rect.height;
    const colors = readThemeColors();
    const geom = computeGeom(canvas.width, canvas.height);
    const s = stateRef.current;
    const start = performance.now();
    let raf = 0;

    const draw = (ts: number) => {
      const t = ts - start;
      renderIntro(ctx, canvas, geom, colors, s.snake, s.dir, s.food, Math.min(t, INTRO_MS));
      if (t >= INTRO_MS) {
        // Hand off to the game loop — keep the snake/food the intro
        // just assembled, so the board doesn't visibly jump.
        s.running = true;
        s.acc = 0;
        s.lastTs = 0;
        setPhase("playing");
        return;
      }
      raf = requestAnimationFrame(draw);
    };
    raf = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(raf);
  }, [phase]);

  // ── Game loop — runs while phase === "playing". ──────────────────────
  useEffect(() => {
    if (phase !== "playing") return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width;
    canvas.height = rect.height;
    const colors = readThemeColors();
    const geom = computeGeom(canvas.width, canvas.height);
    const s = stateRef.current;
    let raf = 0;

    const doTick = () => {
      // Commit the buffered direction (already reversal-checked on input).
      if (!isOpposite(s.dir, s.pendingDir)) s.dir = s.pendingDir;
      const res = step(s.snake, s.dir, s.food, GRID_COLS, GRID_ROWS);
      if (res.dead) {
        s.running = false;
        setBest((b) => Math.max(b, scoreRef.current));
        setPhase("over");
        return;
      }
      s.snake = res.snake;
      if (res.ate) {
        scoreRef.current += 1;
        setScore(scoreRef.current);
        const next = spawnFood(s.snake, GRID_COLS, GRID_ROWS);
        if (next) {
          s.food = next;
        } else {
          // Board full — the snake fills every cell. A perfect game.
          s.running = false;
          setBest((b) => Math.max(b, scoreRef.current));
          setPhase("over");
        }
      }
    };

    // Fixed-timestep ticks driven off a wall-clock accumulator, so the
    // snake advances at the same real speed on any display refresh rate.
    const loop = (ts: number) => {
      if (s.lastTs === 0) s.lastTs = ts;
      let dt = ts - s.lastTs;
      s.lastTs = ts;
      if (dt > 250) dt = 250; // clamp a stall so the snake can't lurch
      s.acc += dt;
      while (s.running && s.acc >= tickInterval(scoreRef.current)) {
        s.acc -= tickInterval(scoreRef.current);
        doTick();
      }
      renderGame(ctx, canvas, geom, colors, s.snake, s.dir, s.food, ts);
      if (s.running) raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [phase]);

  return (
    <div
      className={
        "flex h-full w-full flex-col bg-[var(--color-bg)] " +
        (phase === "intro" ? "rockthebox-rock" : "")
      }
    >
      {/* HUD */}
      <div className="flex h-12 shrink-0 items-center justify-between border-b border-[var(--color-border)] px-4">
        <span className="font-[var(--font-mono)] text-[12px] font-semibold uppercase tracking-[0.2em] text-[var(--color-accent)]">
          Rock the Box
        </span>
        <span className="font-[var(--font-mono)] text-[18px] font-bold tabular-nums">
          <span className="text-[var(--color-accent)]">{score}</span>
          <span className="px-2 text-[var(--color-muted)]">·</span>
          <span className="text-[12px] text-[var(--color-muted)]">best {best}</span>
        </span>
        <span className="text-[11px] text-[var(--color-muted)]">
          <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">
            ↑↓←→
          </kbd>{" "}
          / WASD steer &nbsp;·&nbsp;{" "}
          <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">
            Esc
          </kbd>{" "}
          quit
        </span>
      </div>

      {/* Play field */}
      <div className="relative min-h-0 flex-1">
        <canvas ref={canvasRef} className="h-full w-full" />

        {/* Intro overlay — the box-assembling flourish. */}
        {phase === "intro" && (
          <div className="absolute inset-0 flex items-center justify-center">
            <span className="rockthebox-title font-[var(--font-mono)] text-[40px] font-black uppercase tracking-tight text-[var(--color-accent)] [text-shadow:0_0_28px_var(--color-accent)]">
              Rock the Box
            </span>
          </div>
        )}

        {/* Game-over overlay. */}
        {phase === "over" && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-[var(--color-bg)]/80 backdrop-blur-sm">
            <span className="font-[var(--font-mono)] text-[32px] font-black uppercase tracking-tight text-[var(--color-accent)]">
              Game Over 🐍
            </span>
            <span className="font-[var(--font-mono)] text-[16px] tabular-nums text-[var(--color-fg)]">
              {score} {score === 1 ? "apple" : "apples"} &nbsp;·&nbsp; best {best}
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
