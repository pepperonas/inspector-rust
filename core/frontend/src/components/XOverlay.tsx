import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  X_DURATION,
  PALETTE,
  WORDS,
  actAt,
  arc,
  clamp01,
  corrupt,
  easeIn,
  easeOut,
  flashAllowed,
  horizonY,
  noise,
  warpRadius,
} from "../lib/xhype";

/**
 * `x!` — the full-screen spectacle (v0.133.0). Six acts, ~15 s, ONE canvas and
 * ONE rAF loop. Esc aborts; it closes itself when the piece ends.
 *
 * Performance rules, inherited from the iris/weather scenes: every particle
 * pool is allocated ONCE up front and mutated in place — no per-frame object
 * churn — and nothing here re-renders React (the loop draws, React only mounts
 * the canvas). All randomness is the deterministic `noise()`, so a frame can
 * be reasoned about (and the timeline is unit-tested).
 *
 * ⚠️ Full-screen luminance jumps go through `flashAllowed` (WCAG 2.3.1, under
 * three per second). Sparks, embers and scanline jitter are unbounded — those
 * are local, and it's whole-field flashes that are the seizure hazard.
 */

const EMBERS = 260;
const STARS = 320;
const RAIN = 140;
const GLYPHS = "アイウエオカキクケコ01<>{}[]/\\|=+*#%&@$XЖДЛФЯ";

export function XOverlay() {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d", { alpha: true });
    if (!ctx) return;

    let raf = 0;
    let done = false;
    const start = performance.now();
    let lastFlash: number | null = null;

    // ── Pools (allocated once) ──────────────────────────────────────────────
    const ember = Array.from({ length: EMBERS }, (_, i) => ({
      x: noise(i, 11),
      y: 1 + noise(i, 12),
      vx: (noise(i, 13) - 0.5) * 0.00035,
      vy: -(0.0009 + noise(i, 14) * 0.0022),
      r: 0.6 + noise(i, 15) * 2.6,
      life: noise(i, 16),
      hot: noise(i, 17),
    }));
    const star = Array.from({ length: STARS }, (_, i) => ({
      a: noise(i, 21) * Math.PI * 2,
      p: noise(i, 22),
      sp: 0.35 + noise(i, 23) * 0.9,
      w: 0.4 + noise(i, 24) * 1.8,
    }));
    const rain = Array.from({ length: RAIN }, (_, i) => ({
      x: noise(i, 31),
      y: noise(i, 32),
      sp: 0.25 + noise(i, 33) * 1.1,
      len: 6 + Math.floor(noise(i, 34) * 16),
      seed: Math.floor(noise(i, 35) * 9999),
    }));

    const resize = () => {
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      canvas.width = Math.floor(window.innerWidth * dpr);
      canvas.height = Math.floor(window.innerHeight * dpr);
      canvas.style.width = `${window.innerWidth}px`;
      canvas.style.height = `${window.innerHeight}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    resize();
    window.addEventListener("resize", resize);

    /** Big display word with an RGB tear — the signature typographic move. */
    const stab = (
      text: string,
      w: number,
      h: number,
      size: number,
      alpha: number,
      tear: number,
      y = h * 0.5,
    ) => {
      ctx.save();
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.font = `900 ${size}px "Helvetica Neue", Inter, system-ui, sans-serif`;
      ctx.globalCompositeOperation = "lighter";
      ctx.globalAlpha = alpha * 0.85;
      ctx.fillStyle = "#ff0040";
      ctx.fillText(text, w / 2 - tear, y);
      ctx.fillStyle = "#00ffe0";
      ctx.fillText(text, w / 2 + tear, y);
      ctx.globalAlpha = alpha;
      ctx.fillStyle = PALETTE.bone;
      ctx.fillText(text, w / 2, y);
      ctx.restore();
    };

    const frame = (now: number) => {
      if (done) return;
      const t = now - start;
      const cur = actAt(t);
      if (!cur) {
        void invoke("x_overlay_close").catch(() => undefined);
        return;
      }
      const w = window.innerWidth;
      const h = window.innerHeight;
      const { act, local } = cur;
      const k = act.key;

      // ── Ground ────────────────────────────────────────────────────────────
      ctx.globalCompositeOperation = "source-over";
      ctx.fillStyle = "#000";
      ctx.fillRect(0, 0, w, h);

      // ══ I IGNITION — one ember wakes ═════════════════════════════════════
      if (k === "ignition") {
        const grow = easeIn(local);
        const r = 2 + grow * Math.max(w, h) * 0.22;
        const g = ctx.createRadialGradient(w / 2, h / 2, 0, w / 2, h / 2, r);
        g.addColorStop(0, `rgba(255,240,220,${0.85 * (0.35 + grow)})`);
        g.addColorStop(0.25, `rgba(255,90,20,${0.55 * (0.3 + grow)})`);
        g.addColorStop(1, "rgba(0,0,0,0)");
        ctx.fillStyle = g;
        ctx.fillRect(0, 0, w, h);
        // A heartbeat before it bursts.
        const beat = Math.sin(local * Math.PI * 6) * 0.5 + 0.5;
        ctx.globalCompositeOperation = "lighter";
        ctx.fillStyle = `rgba(255,60,15,${0.12 * beat * grow})`;
        ctx.fillRect(0, 0, w, h);
        if (local > 0.82) stab("X", w, h, Math.min(w, h) * 0.34, easeIn((local - 0.82) / 0.18), 3);
      }

      // ══ II GRID — the technocrat horizon rushes in ═══════════════════════
      if (k === "grid") {
        const horizon = h * 0.42;
        const speed = 0.35 + easeIn(local) * 2.4;
        const scroll = (t / 1000) * speed;
        ctx.globalCompositeOperation = "lighter";
        // Horizon glow.
        const hg = ctx.createLinearGradient(0, horizon - h * 0.2, 0, horizon + h * 0.12);
        hg.addColorStop(0, "rgba(124,58,237,0)");
        hg.addColorStop(0.7, `rgba(124,58,237,${0.35 + local * 0.3})`);
        hg.addColorStop(1, `rgba(255,59,15,${0.5 + local * 0.4})`);
        ctx.fillStyle = hg;
        ctx.fillRect(0, horizon - h * 0.2, w, h * 0.32);
        // Depth lines.
        ctx.lineWidth = 1;
        for (let i = 0; i < 26; i++) {
          const z = 1 - ((i / 26 + scroll) % 1);
          const y = horizonY(z, h, horizon);
          const a = clamp01((1 - z) * 1.3) * 0.75;
          ctx.strokeStyle = `rgba(34,211,238,${a})`;
          ctx.beginPath();
          ctx.moveTo(0, y);
          ctx.lineTo(w, y);
          ctx.stroke();
        }
        // Vanishing rays.
        for (let i = -14; i <= 14; i++) {
          ctx.strokeStyle = `rgba(255,140,26,${0.16 + Math.abs(i) * 0.004})`;
          ctx.beginPath();
          ctx.moveTo(w / 2, horizon);
          ctx.lineTo(w / 2 + i * w * 0.14, h);
          ctx.stroke();
        }
        // Words stab past like signage on an autobahn.
        const list = WORDS.grid;
        const idx = Math.floor(local * list.length * 1.35) % list.length;
        const ph = (local * list.length * 1.35) % 1;
        stab(list[idx], w, h, Math.min(w, h) * (0.1 + ph * 0.13), arc(ph) * 0.9, 6 + ph * 22, h * 0.62);
      }

      // ══ III SLOP — the feed floods ═══════════════════════════════════════
      if (k === "slop") {
        ctx.globalCompositeOperation = "lighter";
        ctx.font = '600 15px "SF Mono", ui-monospace, monospace';
        ctx.textAlign = "left";
        ctx.textBaseline = "top";
        for (let i = 0; i < RAIN; i++) {
          const d = rain[i];
          d.y += d.sp * 0.012 * (1 + local * 2.4);
          if (d.y > 1.2) d.y -= 1.4;
          const px = d.x * w;
          const py = d.y * h;
          for (let j = 0; j < d.len; j++) {
            const a = (1 - j / d.len) * (0.25 + local * 0.5);
            const g = GLYPHS[Math.floor(noise(d.seed + j + Math.floor(t / 90), 41) * GLYPHS.length)];
            ctx.fillStyle = j === 0 ? `rgba(232,228,220,${a})` : `rgba(34,211,238,${a * 0.65})`;
            ctx.fillText(g, px, py - j * 17);
          }
        }
        // Horizontal tearing — the feed can't hold itself together.
        const slices = 7;
        for (let s = 0; s < slices; s++) {
          if (noise(s + Math.floor(t / 110), 51) > 0.72) {
            const sy = (s / slices) * h;
            const sh = h / slices;
            const dx = (noise(s + Math.floor(t / 110), 52) - 0.5) * 90 * local;
            ctx.globalCompositeOperation = "source-over";
            ctx.drawImage(canvas, 0, sy, w, sh, dx, sy, w, sh);
          }
        }
        const list = WORDS.slop;
        const idx = Math.floor(local * list.length) % list.length;
        const ph = (local * list.length) % 1;
        stab(
          corrupt(list[idx], local, idx + 1),
          w,
          h,
          Math.min(w, h) * 0.18,
          arc(ph),
          10 + arc(ph) * 26,
        );
      }

      // ══ IV BURN — it all catches fire ════════════════════════════════════
      if (k === "burn") {
        ctx.globalCompositeOperation = "lighter";
        for (let i = 0; i < EMBERS; i++) {
          const e = ember[i];
          e.x += e.vx * (1 + local);
          e.y += e.vy * (1 + local * 1.8);
          e.life += 0.006;
          if (e.y < -0.05) {
            e.y = 1.05;
            e.life = 0;
          }
          const px = e.x * w + Math.sin(t * 0.002 + i) * 9;
          const py = e.y * h;
          const fade = 1 - clamp01(e.life * 0.55);
          const r = e.r * (1 + e.hot);
          const g = ctx.createRadialGradient(px, py, 0, px, py, r * 4);
          g.addColorStop(0, `rgba(255,${Math.floor(180 + e.hot * 70)},120,${0.9 * fade})`);
          g.addColorStop(0.4, `rgba(255,${Math.floor(60 + e.hot * 60)},10,${0.5 * fade})`);
          g.addColorStop(1, "rgba(0,0,0,0)");
          ctx.fillStyle = g;
          ctx.fillRect(px - r * 4, py - r * 4, r * 8, r * 8);
        }
        // Furnace floor.
        const fg = ctx.createLinearGradient(0, h, 0, h * 0.45);
        fg.addColorStop(0, `rgba(255,59,15,${0.55 + local * 0.3})`);
        fg.addColorStop(1, "rgba(255,59,15,0)");
        ctx.fillStyle = fg;
        ctx.fillRect(0, h * 0.45, w, h * 0.55);
        const list = WORDS.burn;
        const idx = Math.floor(local * list.length) % list.length;
        const ph = (local * list.length) % 1;
        stab(list[idx], w, h, Math.min(w, h) * 0.16, arc(ph) * 0.95, 4 + arc(ph) * 12, h * 0.42);
      }

      // ══ V NOVA — collapse, detonate, warp ════════════════════════════════
      if (k === "nova") {
        const collapse = clamp01(local / 0.28);
        const blast = clamp01((local - 0.28) / 0.72);
        ctx.globalCompositeOperation = "lighter";
        if (local < 0.28) {
          // Everything falls into one point.
          const r = (1 - easeIn(collapse)) * Math.max(w, h) * 0.6 + 4;
          const g = ctx.createRadialGradient(w / 2, h / 2, 0, w / 2, h / 2, r);
          g.addColorStop(0, "rgba(255,255,255,0.95)");
          g.addColorStop(0.5, `rgba(255,140,26,${0.6 * (1 - collapse)})`);
          g.addColorStop(1, "rgba(0,0,0,0)");
          ctx.fillStyle = g;
          ctx.fillRect(0, 0, w, h);
        } else {
          // Warp field.
          const maxR = Math.hypot(w, h) * 0.62;
          for (let i = 0; i < STARS; i++) {
            const s = star[i];
            const p = (s.p + blast * s.sp) % 1;
            const r0 = warpRadius(p, maxR);
            const r1 = warpRadius(clamp01(p + 0.06), maxR);
            const a = clamp01(p * 2.2) * (1 - p * 0.35);
            ctx.strokeStyle = `rgba(232,228,220,${a})`;
            ctx.lineWidth = s.w;
            ctx.beginPath();
            ctx.moveTo(w / 2 + Math.cos(s.a) * r0, h / 2 + Math.sin(s.a) * r0);
            ctx.lineTo(w / 2 + Math.cos(s.a) * r1, h / 2 + Math.sin(s.a) * r1);
            ctx.stroke();
          }
          // The shock ring.
          const rr = easeOut(blast) * Math.hypot(w, h) * 0.7;
          ctx.strokeStyle = `rgba(255,140,26,${(1 - blast) * 0.8})`;
          ctx.lineWidth = 3 + (1 - blast) * 22;
          ctx.beginPath();
          ctx.arc(w / 2, h / 2, rr, 0, Math.PI * 2);
          ctx.stroke();
          stab("X!", w, h, Math.min(w, h) * (0.3 + blast * 0.25), 1 - blast * 0.7, 4 + blast * 40);
        }
        // ONE gated white flash at detonation — the only full-field jump.
        if (local > 0.26 && local < 0.34 && flashAllowed(lastFlash, now)) {
          lastFlash = now;
          ctx.fillStyle = "rgba(255,255,255,0.8)";
          ctx.fillRect(0, 0, w, h);
        }
      }

      // ══ VI VOID — the stars go out ═══════════════════════════════════════
      if (k === "void") {
        const fade = 1 - easeIn(local);
        ctx.globalCompositeOperation = "lighter";
        for (let i = 0; i < STARS; i++) {
          const s = star[i];
          const r = Math.hypot(w, h) * 0.5 * (0.25 + noise(i, 61) * 0.75);
          const tw = 0.35 + 0.65 * Math.sin(t * 0.004 + i);
          ctx.fillStyle = `rgba(232,228,220,${clamp01(tw) * fade * 0.7})`;
          ctx.fillRect(w / 2 + Math.cos(s.a) * r, h / 2 + Math.sin(s.a) * r, s.w, s.w);
        }
        const g = ctx.createRadialGradient(w / 2, h / 2, 0, w / 2, h / 2, 90 * fade + 6);
        g.addColorStop(0, `rgba(255,120,40,${fade * 0.9})`);
        g.addColorStop(1, "rgba(0,0,0,0)");
        ctx.fillStyle = g;
        ctx.fillRect(0, 0, w, h);
      }

      // ── Post: scanlines · grain · vignette · HUD ──────────────────────────
      ctx.globalCompositeOperation = "source-over";
      ctx.fillStyle = "rgba(0,0,0,0.22)";
      for (let y = 0; y < h; y += 3) ctx.fillRect(0, y, w, 1);

      ctx.globalCompositeOperation = "lighter";
      const gn = Math.floor(t / 40);
      for (let i = 0; i < 90; i++) {
        const a = noise(i + gn * 97, 71) * 0.05;
        ctx.fillStyle = `rgba(255,255,255,${a})`;
        ctx.fillRect(noise(i + gn * 31, 72) * w, noise(i + gn * 53, 73) * h, 2, 2);
      }

      ctx.globalCompositeOperation = "source-over";
      const vg = ctx.createRadialGradient(w / 2, h / 2, Math.min(w, h) * 0.28, w / 2, h / 2, Math.hypot(w, h) * 0.62);
      vg.addColorStop(0, "rgba(0,0,0,0)");
      vg.addColorStop(1, "rgba(0,0,0,0.92)");
      ctx.fillStyle = vg;
      ctx.fillRect(0, 0, w, h);

      // Technocrat HUD: the piece narrates its own act.
      ctx.font = '600 11px "SF Mono", ui-monospace, monospace';
      ctx.textAlign = "left";
      ctx.textBaseline = "alphabetic";
      ctx.fillStyle = "rgba(232,228,220,0.5)";
      ctx.fillText(act.caption, 26, h - 26);
      ctx.textAlign = "right";
      ctx.fillText(`${String(Math.floor(t)).padStart(5, "0")} MS · ESC`, w - 26, h - 26);
      // Progress hairline.
      ctx.fillStyle = "rgba(255,59,15,0.75)";
      ctx.fillRect(0, h - 2, w * (t / X_DURATION), 2);

      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);

    const onKey = (e: KeyboardEvent) => {
      // Any key gets you out — this thing takes the whole screen.
      e.preventDefault();
      done = true;
      void invoke("x_overlay_close").catch(() => undefined);
    };
    window.addEventListener("keydown", onKey);

    return () => {
      done = true;
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", resize);
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  return (
    <div style={{ position: "fixed", inset: 0, background: "#000", cursor: "none" }}>
      <canvas ref={canvasRef} style={{ display: "block" }} />
    </div>
  );
}

export default XOverlay;
