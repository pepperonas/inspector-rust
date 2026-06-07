import { useEffect, useRef, useState } from "react";
import { usePauseOnPopupHidden } from "../hooks/usePauseOnPopupHidden";
import {
  BIRD_R,
  GROUND_H,
  INTRO_MS,
  PIPE_GAP,
  PIPE_WIDTH,
  birdX,
  clamp,
  flap,
  frameScale,
  groundY,
  initialState,
  randGapTop,
  step,
  type FlappyState,
} from "../lib/flappy";
import {
  clearSavedGame,
  commitHighScore,
  loadHighScore,
  loadSavedGame,
  saveGame,
} from "../lib/game-storage";

const STORAGE_KEY = "flappy";

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
    surface: v("--color-surface", "#15171d"),
    fg: v("--color-fg", "#f2f3f5"),
    accent: v("--color-accent", "#6366f1"),
    border: v("--color-border", "#2b2e38"),
    muted: v("--color-muted", "#9a9fac"),
  };
}

export function FlappyGame({ onExit }: Props) {
  const [saved] = useState(() => loadSavedGame<FlappyState>(STORAGE_KEY));

  const [phase, setPhase] = useState<Phase>(saved ? "playing" : "intro");
  const [score, setScore] = useState(saved?.score ?? 0);
  const [best, setBest] = useState(() => loadHighScore(STORAGE_KEY));
  // Render-safe mirror of `game.started` (don't read the ref during render):
  // drives the "click to fly" idle hint.
  const [flying, setFlying] = useState(saved?.started ?? false);
  const scoreRef = useRef(saved?.score ?? 0);
  // Resume gate: a resumed run starts frozen until the player presses a key /
  // clicks, so the loop doesn't advance before they're ready (and they don't
  // instantly fly into a pipe). Fresh games aren't gated.
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
  const stateRef = useRef({
    fieldW: 700,
    fieldH: 452,
    game: saved ?? initialState(452),
    lastTs: 0,
    running: false,
  });
  // A resumed run keeps its restored geometry on first mount; a fresh one
  // re-seeds against the real canvas height.
  const keepResumed = useRef(Boolean(saved));

  const resetMatch = () => {
    const s = stateRef.current;
    s.game = initialState(s.fieldH);
    s.lastTs = 0;
    s.running = true;
    scoreRef.current = 0;
    setScore(0);
    setFlying(false);
    liftGate(); // a fresh match runs immediately
    setPhase("playing");
  };

  // Intro → playing (fresh runs only; a resumed run skips the flourish).
  useEffect(() => {
    if (saved) return;
    const t = window.setTimeout(resetMatch, INTRO_MS);
    return () => window.clearTimeout(t);
    // resetMatch is intentionally not a dep — the intro runs once per mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [saved]);

  // Keyboard: Esc quits (saving a live run), flap keys, Space rematches.
  useEffect(() => {
    const doFlap = () => {
      flap(stateRef.current.game);
      setFlying(true);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        const g = stateRef.current.game;
        // Worth resuming only a started, still-alive run.
        if (phase === "playing" && g.started && !g.dead) {
          saveGame<FlappyState>(STORAGE_KEY, g);
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
      // First key on a resumed run just un-freezes — it doesn't also flap,
      // so the player keeps their position before taking control.
      if (phase === "playing" && resumeGateRef.current) {
        e.preventDefault();
        liftGate();
        return;
      }
      if (
        e.key === " " ||
        e.code === "Space" ||
        e.key === "ArrowUp" ||
        e.key === "w" ||
        e.key === "W"
      ) {
        e.preventDefault();
        if (phase === "playing") doFlap();
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
    // resetMatch / liftGate are intentionally not deps (stable enough for the
    // handler; re-subscribing on their identity would be churn).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [phase, onExit]);

  // Render + physics loop.
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
    if (keepResumed.current) {
      keepResumed.current = false; // restored coords already match the field
    } else {
      s.game = initialState(s.fieldH);
    }

    const colors = readThemeColors();
    let raf = 0;

    const endRun = () => {
      s.running = false;
      setBest(commitHighScore(STORAGE_KEY, scoreRef.current));
      clearSavedGame(STORAGE_KEY);
      setPhase("over");
    };

    const drawBird = (ts: number) => {
      const g = s.game;
      const bx = birdX(s.fieldW);
      // Idle bob before the first flap; in-flight tilt from velocity.
      const bob = g.started ? 0 : Math.sin(ts / 220) * 5;
      const by = g.birdY + bob;
      const tilt = g.started ? clamp(g.vy * 0.05, -0.5, 1.3) : 0;
      ctx.save();
      ctx.translate(bx, by);
      ctx.rotate(tilt);
      ctx.fillStyle = colors.accent;
      ctx.beginPath();
      ctx.arc(0, 0, BIRD_R, 0, Math.PI * 2);
      ctx.fill();
      // Beak.
      ctx.fillStyle = colors.fg;
      ctx.beginPath();
      ctx.moveTo(BIRD_R - 2, -3);
      ctx.lineTo(BIRD_R + 7, 0);
      ctx.lineTo(BIRD_R - 2, 3);
      ctx.closePath();
      ctx.fill();
      // Eye.
      ctx.fillStyle = colors.bg;
      ctx.beginPath();
      ctx.arc(4, -4, 2.4, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    };

    const render = (ts: number) => {
      const gy = groundY(s.fieldH);
      ctx.fillStyle = colors.bg;
      ctx.fillRect(0, 0, s.fieldW, s.fieldH);

      // Pipes.
      for (const p of s.game.pipes) {
        ctx.fillStyle = colors.accent;
        ctx.globalAlpha = 0.85;
        ctx.fillRect(p.x, 0, PIPE_WIDTH, p.gapTop);
        ctx.fillRect(p.x, p.gapTop + PIPE_GAP, PIPE_WIDTH, gy - (p.gapTop + PIPE_GAP));
        // Lip accents at the gap edges.
        ctx.globalAlpha = 1;
        ctx.fillStyle = colors.fg;
        ctx.fillRect(p.x - 2, p.gapTop - 8, PIPE_WIDTH + 4, 8);
        ctx.fillRect(p.x - 2, p.gapTop + PIPE_GAP, PIPE_WIDTH + 4, 8);
      }
      ctx.globalAlpha = 1;

      // Ground.
      ctx.fillStyle = colors.surface;
      ctx.fillRect(0, gy, s.fieldW, GROUND_H);
      ctx.fillStyle = colors.border;
      ctx.fillRect(0, gy, s.fieldW, 3);

      drawBird(ts);
    };

    const loop = (ts: number) => {
      // Frozen while the resume gate is up: keep rendering the held frame but
      // don't advance physics, and keep lastTs current so there's no dt jump
      // when the player un-freezes.
      if (resumeGateRef.current) {
        s.lastTs = ts;
        render(ts);
        raf = requestAnimationFrame(loop);
        return;
      }
      const dt = s.lastTs === 0 ? 1 : frameScale(ts - s.lastTs);
      s.lastTs = ts;

      if (s.game.started && !s.game.dead) {
        step(s.game, s.fieldW, s.fieldH, dt, randGapTop(s.fieldH));
        if (s.game.score !== scoreRef.current) {
          scoreRef.current = s.game.score;
          setScore(s.game.score);
        }
      }
      render(ts);
      if (s.game.dead) {
        endRun();
        return;
      }
      if (s.running) raf = requestAnimationFrame(loop);
    };

    s.running = true;
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, [phase]);

  const onPointerDown = () => {
    if (phase === "playing") {
      if (resumeGateRef.current) {
        liftGate(); // first click just resumes
        return;
      }
      flap(stateRef.current.game);
      setFlying(true);
    } else if (phase === "over") {
      resetMatch();
    }
  };

  const idleHint = phase === "playing" && !resumeGate && !flying;

  return (
    <div
      className={
        "flex h-full w-full flex-col bg-[var(--color-bg)] " +
        (phase === "intro" ? "flappy-rise" : "")
      }
    >
      <div className="flex h-12 shrink-0 items-center justify-between border-b border-[var(--color-border)] px-4">
        <span className="font-[var(--font-mono)] text-[12px] font-semibold uppercase tracking-[0.2em] text-[var(--color-accent)]">
          Learning to fly
        </span>
        <span className="font-[var(--font-mono)] text-[18px] font-bold tabular-nums">
          <span className="text-[var(--color-fg)]">{score}</span>
          <span className="px-2 text-[var(--color-muted)]">·</span>
          <span className="text-[12px] text-[var(--color-muted)]">best {best}</span>
        </span>
        <span className="text-[11px] text-[var(--color-muted)]">
          <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">
            Space
          </kbd>{" "}
          / click flap &nbsp;·&nbsp;{" "}
          <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">
            Esc
          </kbd>{" "}
          quit
        </span>
      </div>

      <div className="relative min-h-0 flex-1" onPointerDown={onPointerDown}>
        <canvas ref={canvasRef} className="h-full w-full cursor-pointer" />

        {phase === "intro" && (
          <div className="absolute inset-0 flex items-center justify-center">
            <span className="flappy-title font-[var(--font-mono)] text-[40px] font-black uppercase tracking-[0.18em] text-[var(--color-accent)]">
              Learning&nbsp;to&nbsp;Fly
            </span>
          </div>
        )}

        {idleHint && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
            <span className="rounded-full border border-[var(--color-border)] bg-[var(--color-surface)]/80 px-4 py-1.5 text-[13px] text-[var(--color-muted)] backdrop-blur-sm">
              Click or press Space to fly
            </span>
          </div>
        )}

        {phase === "playing" && resumeGate && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
            <span className="rounded-full border border-[var(--color-border)] bg-[var(--color-surface)]/85 px-4 py-1.5 text-[13px] text-[var(--color-fg)] backdrop-blur-sm">
              ▸ Resumed — press a key or click to continue
            </span>
          </div>
        )}

        {phase === "over" && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-[var(--color-bg)]/80 backdrop-blur-sm">
            <span className="font-[var(--font-mono)] text-[32px] font-black uppercase tracking-tight text-[var(--color-muted)]">
              Down you go 🐤
            </span>
            <span className="font-[var(--font-mono)] text-[16px] tabular-nums text-[var(--color-fg)]">
              Score {score} &nbsp;·&nbsp; best {best}
            </span>
            <span className="mt-1 text-[12px] text-[var(--color-muted)]">
              <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">
                Space
              </kbd>{" "}
              / click rematch &nbsp;·&nbsp;{" "}
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
