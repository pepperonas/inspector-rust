import { useEffect, useRef, useState } from "react";
import {
  BALL_BASE_SPEED,
  BALL_R,
  PADDLE_H,
  PADDLE_INSET,
  PADDLE_W,
  PLAYER_KEY_SPEED,
  SERVE_DELAY_MS,
  SHIFT_SPEED_MULTIPLIER,
  WIN_SCORE,
  botBehavior,
  clamp,
  frameScale,
  nextBallSpeed,
  paddleBounce,
  paddleHit,
  serveBall,
} from "../lib/pong";

/**
 * `getshaky` easter egg — the popup overlay transforms (with a shaky
 * intro flourish, hence the name) into a game of Pong against a
 * ramp-up bot, first to 5. Esc is the only way out.
 *
 * Entirely client-side: a `<canvas>` + requestAnimationFrame loop.
 * Pure maths lives in `lib/pong.ts`; this component owns the stateful
 * render loop, input, HUD, and the intro/over phases.
 */

interface Props {
  /** Called when the user presses Esc — App.tsx returns to the popup. */
  onExit: () => void;
}

type Phase = "intro" | "playing" | "over";

/** Theme-aware colours, read once from the live CSS custom properties
 *  so the Pong board matches whatever theme the app is in. */
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
  };
}

export function PongGame({ onExit }: Props) {
  const [phase, setPhase] = useState<Phase>("intro");
  // Scores are React state for the HUD; the game loop reads them via a
  // ref so it always sees the freshest value without re-subscribing.
  const [playerScore, setPlayerScore] = useState(0);
  const [botScore, setBotScore] = useState(0);
  const scoreRef = useRef({ player: 0, bot: 0 });

  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const rootRef = useRef<HTMLDivElement | null>(null);
  // Mutable game state — kept out of React so the 60 fps loop never
  // triggers a re-render. Only score changes + phase flips do.
  const stateRef = useRef({
    fieldW: 700,
    fieldH: 452,
    ballX: 350,
    ballY: 226,
    ballVx: BALL_BASE_SPEED,
    ballVy: 0,
    ballSpeed: BALL_BASE_SPEED,
    playerY: 226,
    botY: 226,
    keys: { up: false, down: false, shift: false },
    running: false,
    // 0 → ball is live; > 0 → wall-clock ts at which the next serve fires.
    serveAt: 0,
    // Timestamp of the previous frame, for the frame-scale delta.
    lastTs: 0,
  });

  // Reset every entity + score and (re)start a match. Declared before
  // the effects so the keydown handler's Space-rematch path can call it.
  const restart = () => {
    scoreRef.current = { player: 0, bot: 0 };
    setPlayerScore(0);
    setBotScore(0);
    const s = stateRef.current;
    const serve = serveBall(s.fieldW, s.fieldH, Math.random() < 0.5);
    s.ballX = serve.x;
    s.ballY = serve.y;
    s.ballVx = serve.vx;
    s.ballVy = serve.vy;
    s.ballSpeed = BALL_BASE_SPEED;
    s.playerY = s.fieldH / 2;
    s.botY = s.fieldH / 2;
    s.serveAt = 0;
    s.lastTs = 0;
    s.running = true;
    setPhase("playing");
  };

  // ── Intro phase: a ~1.3 s shaky transformation, then kick off. ──────
  useEffect(() => {
    const toPlaying = window.setTimeout(() => {
      setPhase("playing");
      const s = stateRef.current;
      const serve = serveBall(s.fieldW, s.fieldH, Math.random() < 0.5);
      s.ballX = serve.x;
      s.ballY = serve.y;
      s.ballVx = serve.vx;
      s.ballVy = serve.vy;
      s.ballSpeed = BALL_BASE_SPEED;
      s.running = true;
    }, 1300);
    return () => window.clearTimeout(toPlaying);
  }, []);

  // ── Esc to quit (the only abort, per spec) + paddle keys. ───────────
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
        return;
      }
      if (phase === "over" && (e.key === " " || e.code === "Space")) {
        // Rematch — not an abort, so it doesn't violate "only Esc quits".
        e.preventDefault();
        restart();
        return;
      }
      const s = stateRef.current;
      if (e.key === "ArrowUp" || e.key === "w" || e.key === "W") s.keys.up = true;
      if (e.key === "ArrowDown" || e.key === "s" || e.key === "S") s.keys.down = true;
      s.keys.shift = e.shiftKey;
    };
    const onKeyUp = (e: KeyboardEvent) => {
      const s = stateRef.current;
      if (e.key === "ArrowUp" || e.key === "w" || e.key === "W") s.keys.up = false;
      if (e.key === "ArrowDown" || e.key === "s" || e.key === "S") s.keys.down = false;
      s.keys.shift = e.shiftKey;
    };
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
    };
  }, [phase, onExit]);

  // ── Mouse control — moving the cursor sets the player paddle Y.
  // Listener is on `window` so the paddle keeps tracking even when
  // the cursor leaves the canvas area (header, footer, edges, or
  // entirely off-window with `cursor: none`). The original concern
  // that motivated the v0.37.1 canvas-only listener — keys fighting
  // a stationary mouse — is now solved differently: **if the user
  // is currently pressing W / S / ↑ / ↓, mouse updates are
  // skipped**. So keys win during keystrokes, mouse wins between.
  // Best of both: works outside the canvas + doesn't fight the
  // keyboard. v0.38.1+.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const onMove = (e: MouseEvent) => {
      const s = stateRef.current;
      // Keys take priority while held — don't snap the paddle to
      // mouse Y mid-keystroke.
      if (s.keys.up || s.keys.down) return;
      const rect = canvas.getBoundingClientRect();
      // Even when the cursor is outside the canvas rect, mapping by
      // (clientY - rect.top) / rect.height still produces a usable
      // logical Y — `clamp` below pins it to the field bounds, so
      // sliding the cursor off the top/bottom edge parks the paddle
      // at the corresponding extreme instead of overshooting.
      const logicalY = ((e.clientY - rect.top) / rect.height) * s.fieldH;
      s.playerY = clamp(logicalY, PADDLE_H / 2, s.fieldH - PADDLE_H / 2);
    };
    window.addEventListener("mousemove", onMove);
    return () => window.removeEventListener("mousemove", onMove);
  }, []);

  // ── The game loop — runs while phase === "playing". ─────────────────
  useEffect(() => {
    if (phase !== "playing") return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    // Size the logical field to the canvas's real pixel box.
    const rect = canvas.getBoundingClientRect();
    canvas.width = rect.width;
    canvas.height = rect.height;
    const s = stateRef.current;
    s.fieldW = canvas.width;
    s.fieldH = canvas.height;

    const colors = readThemeColors();
    let raf = 0;

    // Park the ball at centre and schedule the next serve `SERVE_DELAY_MS`
    // out. The serve velocity is computed now but held until `serveAt`.
    const parkAndScheduleServe = (towardPlayer: boolean, ts: number) => {
      const serve = serveBall(s.fieldW, s.fieldH, towardPlayer);
      s.ballX = serve.x;
      s.ballY = serve.y;
      s.ballVx = serve.vx;
      s.ballVy = serve.vy;
      s.ballSpeed = BALL_BASE_SPEED;
      s.serveAt = ts + SERVE_DELAY_MS;
    };

    const render = () => {
      ctx.fillStyle = colors.bg;
      ctx.fillRect(0, 0, s.fieldW, s.fieldH);

      // Centre dashed line.
      ctx.strokeStyle = colors.border;
      ctx.lineWidth = 2;
      ctx.setLineDash([8, 10]);
      ctx.beginPath();
      ctx.moveTo(s.fieldW / 2, 0);
      ctx.lineTo(s.fieldW / 2, s.fieldH);
      ctx.stroke();
      ctx.setLineDash([]);

      // Player paddle (accent).
      ctx.fillStyle = colors.accent;
      ctx.fillRect(PADDLE_INSET, s.playerY - PADDLE_H / 2, PADDLE_W, PADDLE_H);
      // Bot paddle (muted).
      ctx.fillStyle = colors.muted;
      ctx.fillRect(
        s.fieldW - PADDLE_INSET - PADDLE_W,
        s.botY - PADDLE_H / 2,
        PADDLE_W,
        PADDLE_H,
      );

      // Ball (foreground).
      ctx.fillStyle = colors.fg;
      ctx.beginPath();
      ctx.arc(s.ballX, s.ballY, BALL_R, 0, Math.PI * 2);
      ctx.fill();
    };

    const step = (ts: number) => {
      // ── Frame-scale: normalise movement to a 60 fps wall-clock step ─
      const dt = s.lastTs === 0 ? 1 : frameScale(ts - s.lastTs);
      s.lastTs = ts;

      // ── Player paddle (keys; mouse already wrote playerY live) ──────
      const keySpeed =
        PLAYER_KEY_SPEED * (s.keys.shift ? SHIFT_SPEED_MULTIPLIER : 1) * dt;
      if (s.keys.up) s.playerY -= keySpeed;
      if (s.keys.down) s.playerY += keySpeed;
      s.playerY = clamp(s.playerY, PADDLE_H / 2, s.fieldH - PADDLE_H / 2);

      // ── Bot paddle — rubber-band AI (v0.38.0+) ──────────────────────
      // `botBehavior` factors in BOTH scores: leads badly when
      // ahead by 2+, plays hardcore when the human is one point
      // from winning, baseline-hard otherwise. Predicts the ball's
      // intercept Y at the bot's column instead of just chasing the
      // current ballY → harder to beat with sharp-angle shots.
      //
      // `botCenterX` is the *center* of the paddle (used for the
      // prediction target); the variable named `botX` further down
      // is the paddle's LEFT edge (collision test). Different
      // X values — same paddle, different purposes.
      const botCenterX = s.fieldW - PADDLE_INSET - PADDLE_W / 2;
      const botMove = botBehavior({
        botScore: scoreRef.current.bot,
        playerScore: scoreRef.current.player,
        ballX: s.ballX,
        ballY: s.ballY,
        ballVx: s.ballVx,
        ballVy: s.ballVy,
        botX: botCenterX,
        fieldH: s.fieldH,
      });
      const botDelta = clamp(botMove.targetY - s.botY, -botMove.maxSpeed, botMove.maxSpeed) * dt;
      s.botY = clamp(s.botY + botDelta, PADDLE_H / 2, s.fieldH - PADDLE_H / 2);

      // ── Serve delay — ball is parked until `serveAt` elapses ────────
      if (s.serveAt > 0) {
        if (ts >= s.serveAt) {
          s.serveAt = 0;
        } else {
          if (s.running) raf = requestAnimationFrame(step);
          render();
          return;
        }
      }

      // ── Ball — swept so a fast ball can't tunnel a thin paddle ──────
      const prevX = s.ballX;
      s.ballX += s.ballVx * dt;
      s.ballY += s.ballVy * dt;

      // Top / bottom walls.
      if (s.ballY - BALL_R < 0) {
        s.ballY = BALL_R;
        s.ballVy = Math.abs(s.ballVy);
      } else if (s.ballY + BALL_R > s.fieldH) {
        s.ballY = s.fieldH - BALL_R;
        s.ballVy = -Math.abs(s.ballVy);
      }

      // Player paddle (left) — crossing test on the leading edge.
      const playerX = PADDLE_INSET + PADDLE_W;
      if (
        s.ballVx < 0 &&
        paddleHit(prevX - BALL_R, s.ballX - BALL_R, playerX, "left",
          s.ballY, BALL_R, s.playerY, PADDLE_H)
      ) {
        s.ballSpeed = nextBallSpeed(s.ballSpeed);
        const offset = (s.ballY - s.playerY) / (PADDLE_H / 2);
        const v = paddleBounce(offset, 1, s.ballSpeed);
        s.ballVx = v.vx;
        s.ballVy = v.vy;
        s.ballX = playerX + BALL_R;
      }

      // Bot paddle (right) — crossing test on the leading edge.
      const botX = s.fieldW - PADDLE_INSET - PADDLE_W;
      if (
        s.ballVx > 0 &&
        paddleHit(prevX + BALL_R, s.ballX + BALL_R, botX, "right",
          s.ballY, BALL_R, s.botY, PADDLE_H)
      ) {
        s.ballSpeed = nextBallSpeed(s.ballSpeed);
        const offset = (s.ballY - s.botY) / (PADDLE_H / 2);
        const v = paddleBounce(offset, -1, s.ballSpeed);
        s.ballVx = v.vx;
        s.ballVy = v.vy;
        s.ballX = botX - BALL_R;
      }

      // ── Scoring ─────────────────────────────────────────────────────
      if (s.ballX + BALL_R < 0) {
        // Ball left past the player's wall → bot scores.
        scoreRef.current.bot += 1;
        setBotScore(scoreRef.current.bot);
        if (scoreRef.current.bot >= WIN_SCORE) {
          s.running = false;
          setPhase("over");
          return;
        }
        parkAndScheduleServe(false, ts);
      } else if (s.ballX - BALL_R > s.fieldW) {
        scoreRef.current.player += 1;
        setPlayerScore(scoreRef.current.player);
        if (scoreRef.current.player >= WIN_SCORE) {
          s.running = false;
          setPhase("over");
          return;
        }
        parkAndScheduleServe(true, ts);
      }

      render();
      if (s.running) raf = requestAnimationFrame(step);
    };

    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [phase]);

  const playerWon = playerScore >= WIN_SCORE;

  return (
    <div
      ref={rootRef}
      className={
        "flex h-full w-full flex-col bg-[var(--color-bg)] " +
        (phase === "intro" ? "getshaky-shake" : "")
      }
    >
      {/* HUD */}
      <div className="flex h-12 shrink-0 items-center justify-between border-b border-[var(--color-border)] px-4">
        <span className="font-[var(--font-mono)] text-[12px] font-semibold uppercase tracking-[0.2em] text-[var(--color-accent)]">
          Get Shaky
        </span>
        <span className="font-[var(--font-mono)] text-[18px] font-bold tabular-nums">
          <span className="text-[var(--color-accent)]">{playerScore}</span>
          <span className="px-2 text-[var(--color-muted)]">—</span>
          <span className="text-[var(--color-muted)]">{botScore}</span>
        </span>
        <span className="text-[11px] text-[var(--color-muted)]">
          You · Bot &nbsp;·&nbsp; first to {WIN_SCORE} &nbsp;·&nbsp;{" "}
          <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">
            ↑↓
          </kbd>{" "}
          /{" "}
          <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">
            W/S
          </kbd>{" "}
          / mouse &nbsp;·&nbsp;{" "}
          <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">
            Esc
          </kbd>{" "}
          quit
        </span>
      </div>

      {/* Play field */}
      <div className="relative min-h-0 flex-1">
        {/* Hide the cursor over the canvas while the match is live —
            mouse controls the paddle, so the cursor would just be a
            visual distraction (especially during a fast rally). Keep
            it visible during `intro` (so the user sees the page they
            triggered) and during `over` (so the rematch button is
            click-targetable). v0.38.0+. */}
        <canvas
          ref={canvasRef}
          className="h-full w-full"
          style={phase === "playing" ? { cursor: "none" } : undefined}
        />

        {/* Intro overlay — the shaky transformation flourish. */}
        {phase === "intro" && (
          <div className="absolute inset-0 flex items-center justify-center">
            <span className="getshaky-title font-[var(--font-mono)] text-[44px] font-black uppercase tracking-tight text-[var(--color-accent)]">
              GET SHAKY
            </span>
          </div>
        )}

        {/* Game-over overlay. */}
        {phase === "over" && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-[var(--color-bg)]/80 backdrop-blur-sm">
            <span
              className={
                "font-[var(--font-mono)] text-[32px] font-black uppercase tracking-tight " +
                (playerWon ? "text-[var(--color-accent)]" : "text-[var(--color-muted)]")
              }
            >
              {playerWon ? "You win 🏓" : "Bot wins"}
            </span>
            <span className="font-[var(--font-mono)] text-[16px] tabular-nums text-[var(--color-fg)]">
              {playerScore} — {botScore}
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
