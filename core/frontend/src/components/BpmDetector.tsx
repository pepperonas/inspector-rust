import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Mic, MicOff, Pin, RefreshCw, X } from "lucide-react";
import { BpmAnalyzer } from "../lib/bpm";
import { setSuppressHide } from "../lib/ipc";
import { warmContext } from "../lib/warm-audio";
import { startFedMic } from "../lib/mic-feed";
import { rms, rmsToDbfs, dbfsToLevel, smoothStep } from "../lib/audio-level";
import {
  clamp01,
  confidenceColor,
  easeOutCubic,
  lerp,
  mixRgb,
  parseColor,
  rgba,
  smoothBars,
  spectrumBars,
  type Rgb,
} from "../lib/bpm-visual";

/**
 * Live BPM-from-microphone overlay with a premium, beat-reactive canvas
 * visualization. Replaces the popup's content while active, identical to the
 * game easter eggs.
 *
 * ## Audio graph
 *
 *   mic ─┬─ HP 30 Hz → LP 100 Hz (Q 1.5) → AnalyserNode(detect) → BpmAnalyzer
 *        └────────────────────────────────→ AnalyserNode(viz, full spectrum)
 *
 * The **detection** analyser is bandpassed to the 30-100 Hz kick band (see the
 * BpmAnalyzer docs); the **viz** analyser taps the *raw* mic so the on-screen
 * spectrum ring shows the whole mix, not just the bass. Neither is connected to
 * `ctx.destination` — we never monitor the mic through the speakers (feedback).
 *
 * ## Rendering
 *
 * The animation is drawn straight to a `<canvas>` from a `requestAnimationFrame`
 * loop reading mutable refs — **no React re-render per frame** (the old version
 * `setState`-d 60×/s). Only the top-bar level descriptor is reconciled into
 * React state on a throttled ~7 Hz interval. The hero BPM number, spectrum ring,
 * beat shockwaves, particle bursts and breathing core are all canvas.
 *
 * `Esc` exits at any phase.
 */

interface Props {
  onExit: () => void;
}

type Phase = "requesting" | "listening" | "denied";

const BAR_COUNT = 84; // spectrum bars per half (mirrored → 168 around the ring)
const TWO_PI = Math.PI * 2;
const FALLBACK_ACCENT: Rgb = { r: 99, g: 102, b: 241 }; // indigo, if CSS read fails

interface Particle {
  x: number;
  y: number;
  vx: number;
  vy: number;
  life: number; // remaining, seconds
  max: number; // initial life
  size: number;
}

interface Shock {
  age: number; // seconds since spawn
  intensity: number; // 0..1
}

/** All mutable per-frame animation state — lives in a ref so the rAF loop
 *  never touches React. */
interface VizState {
  bars: Float32Array; // smoothed spectrum (one side; mirrored when drawn)
  smoothEnergy: number; // eased bass intensity for the breathing core
  beatFlash: number; // 0..1, decays after each beat
  coreKick: number; // 0..1, springy kick on each beat
  rotation: number; // slow continuous spin of the spectrum ring (radians)
  particles: Particle[];
  shocks: Shock[];
  bpm: number;
  confidence: number;
  hasBeat: boolean; // last estimate had any BPM lock
  lastT: number;
  // palette (re-read from CSS periodically so theme changes apply)
  bg: Rgb;
  cold: Rgb;
  warm: Rgb;
  hot: Rgb;
}

export function BpmDetector({ onExit }: Props) {
  const [phase, setPhase] = useState<Phase>("requesting");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  // Throttled, low-frequency mirror of the live level for the top-bar label.
  const [levelLabel, setLevelLabel] = useState<string>("silence");
  // Live dBFS readout (full-band) for the subtle level display. `dbRef` is the
  // smoothed value the hot loop writes; `db` is the throttled UI mirror.
  const [db, setDb] = useState<number | null>(null);
  const dbRef = useRef(-100);
  // Pinned = click-outside won't dismiss (via suppress_hide); the viz also
  // recolours red. `pinnedRef` is the canonical value the keydown/palette read.
  const [pinned, setPinned] = useState(false);
  const pinnedRef = useRef(false);

  // Audio graph + analysis (refs so re-renders don't recreate the AudioContext).
  const audioCtxRef = useRef<AudioContext | null>(null);
  const fedMicRef = useRef<import("../lib/mic-feed").FedMic | null>(null);
  const detectAnalyserRef = useRef<AnalyserNode | null>(null);
  const vizAnalyserRef = useRef<AnalyserNode | null>(null);
  const analyzerRef = useRef<BpmAnalyzer>(new BpmAnalyzer());
  const rafRef = useRef<number | null>(null);

  // Canvas + animation state.
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const vizRef = useRef<VizState>({
    bars: new Float32Array(BAR_COUNT),
    smoothEnergy: 0,
    beatFlash: 0,
    coreKick: 0,
    rotation: 0,
    particles: [],
    shocks: [],
    bpm: 0,
    confidence: 0,
    hasBeat: false,
    lastT: 0,
    bg: { r: 10, g: 10, b: 14 },
    cold: FALLBACK_ACCENT,
    warm: FALLBACK_ACCENT,
    hot: { r: 255, g: 255, b: 255 },
  });
  // Latest live level, written by the rAF loop, read by the throttle interval.
  const levelRef = useRef(0);

  // Re-read theme colors from CSS into the viz palette.
  const refreshPalette = () => {
    const el = canvasRef.current ?? document.documentElement;
    const cs = getComputedStyle(el);
    // While pinned, the whole visualizer goes red instead of the theme accent —
    // a clear "this is pinned / floating" signal.
    const accent: Rgb = pinnedRef.current
      ? { r: 244, g: 63, b: 94 }
      : (parseColor(cs.getPropertyValue("--color-accent")) ?? FALLBACK_ACCENT);
    const bg = parseColor(cs.getPropertyValue("--color-bg")) ?? { r: 10, g: 10, b: 14 };
    const v = vizRef.current;
    v.bg = bg;
    // Luminous accent family: dim → accent → bright. Loud bars glow hot.
    v.cold = mixRgb(accent, bg, 0.45);
    v.warm = accent;
    v.hot = mixRgb(accent, { r: 255, g: 255, b: 255 }, 0.72);
  };

  // Toggle pin: suppress the popup's hide-on-blur so click-outside keeps the
  // detector floating, and recolour the viz. `pinnedRef` is canonical (the
  // keydown closure + palette read it without re-subscribing).
  const togglePin = () => {
    const next = !pinnedRef.current;
    pinnedRef.current = next;
    setPinned(next);
    void setSuppressHide(next).catch(() => undefined);
    refreshPalette();
  };

  // Esc to exit, Enter to pin/unpin (capture phase so they beat the popup's
  // global handler).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        // Always release the pin on exit so normal dismiss resumes.
        if (pinnedRef.current) {
          pinnedRef.current = false;
          void setSuppressHide(false).catch(() => undefined);
        }
        onExit();
      } else if (e.key === "Enter") {
        e.preventDefault();
        e.stopPropagation();
        togglePin();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [onExit]);

  // The popup hid with the detector UNPINNED (click-away / toggle hotkey):
  // exit. Without this, `bpmMode` survived the hide — the native mic capture
  // kept recording invisibly in the background and the next open showed the
  // detector instead of the search UI. A PINNED detector deliberately
  // survives (that is the pin's whole point; pinning also suppresses the
  // focus-loss hide, so for it this only fires on the explicit toggle-close).
  const onExitRef = useRef(onExit);
  useEffect(() => {
    onExitRef.current = onExit;
  }, [onExit]);
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen("popup-hidden", () => {
      if (!pinnedRef.current) onExitRef.current();
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Safety: clear suppress_hide if the detector unmounts while pinned.
  useEffect(
    () => () => {
      void setSuppressHide(false).catch(() => undefined);
    },
    [],
  );

  // Throttled top-bar level label (~7 Hz) — keeps React churn off the hot path.
  useEffect(() => {
    const id = window.setInterval(() => {
      refreshPalette();
      setLevelLabel(energyDescriptor(levelRef.current));
      setDb(dbRef.current <= -90 ? null : Math.round(dbRef.current));
    }, 140);
    return () => window.clearInterval(id);
  }, []);

  const teardown = () => {
    if (rafRef.current !== null) {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
    // Stop the native mic stream + tear down the feed source (disconnects it +
    // its muted pull-path). The AudioContext is the *shared warm* context —
    // never close it; it (and its silent output unit) stays warm for next time.
    fedMicRef.current?.stop();
    fedMicRef.current = null;
    detectAnalyserRef.current?.disconnect();
    vizAnalyserRef.current?.disconnect();
    audioCtxRef.current = null;
    detectAnalyserRef.current = null;
    vizAnalyserRef.current = null;
    analyzerRef.current.reset();
  };

  // Boot the audio graph. Re-entrant via `attempt` so "Retry" re-triggers it.
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let cancelled = false;

    (async () => {
      try {
        // Capture the mic NATIVELY (Rust/cpal, streamed in) instead of the
        // webview's getUserMedia — the latter makes macOS reconfigure the
        // shared audio device on mic-open and briefly pauses other apps'
        // playback (the same glitch fixed for Shazam). `startFedMic` returns a
        // ScriptProcessor source fed by the native stream; the entire analysis
        // graph downstream is UNCHANGED, so detection quality is identical.
        const ctx = warmContext();
        const fed = await startFedMic(ctx);
        const source = fed.source;
        if (cancelled) {
          fed.stop();
          return;
        }
        fedMicRef.current = fed;

        // — detection chain: HP 30 → LP 100 (Q 1.5) → analyser —
        const highpass = ctx.createBiquadFilter();
        highpass.type = "highpass";
        highpass.frequency.value = 30;
        highpass.Q.value = 0.7;
        const lowpass = ctx.createBiquadFilter();
        lowpass.type = "lowpass";
        lowpass.frequency.value = 100;
        lowpass.Q.value = 1.5;
        const detect = ctx.createAnalyser();
        detect.fftSize = 1024;
        detect.smoothingTimeConstant = 0;
        source.connect(highpass);
        highpass.connect(lowpass);
        lowpass.connect(detect);

        // — viz chain: raw mic → full-spectrum analyser (for the ring) —
        const viz = ctx.createAnalyser();
        viz.fftSize = 2048; // 1024 freq bins
        viz.smoothingTimeConstant = 0.78; // visually smooth, the punch comes from beats
        source.connect(viz);

        audioCtxRef.current = ctx;
        detectAnalyserRef.current = detect;
        vizAnalyserRef.current = viz;
        analyzerRef.current.reset();
        vizRef.current.lastT = performance.now();
        refreshPalette();

        setErrorMessage(null);
        setPhase("listening");

        const timeBuf = new Float32Array(detect.fftSize);
        const freqBuf = new Uint8Array(viz.frequencyBinCount);
        const vizTimeBuf = new Float32Array(viz.fftSize); // full-band, for dBFS
        // Opening the mic makes macOS reconfigure the shared input/output audio
        // device; for ~1 s the output can stutter while it settles. The AAA
        // visualizer's per-frame canvas/GPU work piles onto that exact window,
        // so we run only the *cheap* detection during a short warm-up and defer
        // the heavy spectrum read + particles + drawScene until the device has
        // settled. Detection isn't hurt (BpmAnalyzer needs ~3 s of baseline
        // anyway), and the user just sees the viz fade in a beat late.
        const startT = performance.now();
        const WARMUP_MS = 300;

        const tick = () => {
          if (cancelled || !detectAnalyserRef.current || !vizAnalyserRef.current) return;
          const now = performance.now();

          // — detection (always; cheap) —
          detectAnalyserRef.current.getFloatTimeDomainData(timeBuf);
          analyzerRef.current.push(timeBuf, now);
          const est = analyzerRef.current.estimate(now);
          const energy = analyzerRef.current.currentEnergy();
          levelRef.current = energy;

          // — full-band dBFS (always; cheap) for the subtle dB readout —
          // Attack fast, release slow — a calm meter that still catches peaks.
          vizAnalyserRef.current.getFloatTimeDomainData(vizTimeBuf);
          dbRef.current = smoothStep(dbRef.current, rmsToDbfs(rms(vizTimeBuf)), 0.5, 0.12);

          const v = vizRef.current;
          const dt = Math.min(0.05, Math.max(0.001, (now - v.lastT) / 1000));
          v.lastT = now;

          // — heavy viz, deferred until the audio device has settled —
          if (now - startT >= WARMUP_MS) {
            vizAnalyserRef.current.getByteFrequencyData(freqBuf);
            const intensity = clamp01(energy * 8); // bass kick → [0,1]
            smoothBars(v.bars, spectrumBars(freqBuf, BAR_COUNT, 0.62), dt, 0.55, 5);
            v.smoothEnergy = lerp(v.smoothEnergy, intensity, clamp01(dt * 9));
            v.bpm = est.bpm;
            v.confidence = est.confidence;
            v.hasBeat = est.bpm > 0;

            if (est.beatJustFired) {
              v.beatFlash = Math.max(v.beatFlash, 0.55 + intensity * 0.45);
              v.coreKick = 1;
              v.shocks.push({ age: 0, intensity });
              spawnParticles(v, intensity);
            }
            // decays (frame-rate corrected) + slow living rotation
            v.beatFlash *= Math.exp(-6 * dt);
            v.coreKick *= Math.exp(-9 * dt);
            v.rotation += dt * (0.06 + v.smoothEnergy * 0.22);
            stepParticles(v, dt);
            stepShocks(v, dt);

            drawScene(canvasRef.current, v);
          }
          rafRef.current = requestAnimationFrame(tick);
        };
        rafRef.current = requestAnimationFrame(tick);
      } catch (err) {
        const e = err as Error;
        if (cancelled) return;
        setErrorMessage(e.message || e.name || "Audio capture failed");
        setPhase("denied");
      }
    })();

    return () => {
      cancelled = true;
      teardown();
    };
  }, [attempt]);

  return (
    <div className="flex h-full w-full flex-col bg-[var(--color-bg)] text-[var(--color-fg)]">
      {/* Top bar */}
      <div className="z-10 flex items-center justify-between border-b border-[var(--color-border)] px-4 py-2 text-[11px] uppercase tracking-wider text-[var(--color-muted)]">
        <div className="flex items-center gap-2">
          {phase === "listening" ? (
            <Mic size={12} className="text-[var(--color-accent)]" />
          ) : (
            <MicOff size={12} className="text-amber-500" />
          )}
          <span>BPM detector</span>
          {phase === "listening" && (
            <span className="text-[var(--color-muted)]">· {levelLabel}</span>
          )}
          {pinned && <span className="font-semibold text-rose-500">· pinned</span>}
        </div>
        <div className="flex items-center gap-1.5">
          <button
            onClick={togglePin}
            title="Pin (Enter) — keep open when clicking away"
            className={
              "flex items-center gap-1 rounded border px-2 py-0.5 text-[10px] " +
              (pinned
                ? "border-rose-500 bg-rose-500/10 text-rose-500"
                : "border-[var(--color-border)] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]")
            }
          >
            <Pin size={11} className={pinned ? "fill-rose-500" : ""} />
            {pinned ? "Pinned" : "Pin"}
          </button>
          <button
            onClick={onExit}
            className="flex items-center gap-1 rounded border border-[var(--color-border)] px-2 py-0.5 text-[10px] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
          >
            <X size={11} />
            Esc
          </button>
        </div>
      </div>

      {/* Body */}
      <div className="relative flex-1 overflow-hidden">
        {phase === "listening" && (
          <canvas ref={canvasRef} className="absolute inset-0 h-full w-full" />
        )}

        {/* Subtle dB readout — secondary to the big BPM hero on the canvas: a
            small monospace number whose glow + a thin meter bar track the live
            level, smoothed. Goes red while pinned to match the viz. */}
        {phase === "listening" &&
          (() => {
            const accent = pinned ? "#f43f5e" : "var(--color-accent)";
            const norm = db === null ? 0 : dbfsToLevel(db, -60, -6);
            // Sits in the lower third — clear of both the centred BPM hero and
            // the canvas status line at the very bottom. Present but still
            // quieter than the hero: medium number + a wide glowing meter that
            // tracks the level, scale/opacity easing up with loudness.
            return (
              <div className="pointer-events-none absolute inset-x-0 bottom-[15%] flex flex-col items-center gap-2">
                <div
                  className="flex items-baseline gap-1 font-[var(--font-mono)] tabular-nums transition-[color,text-shadow,transform,opacity] duration-150"
                  style={{
                    color: db === null ? "var(--color-muted)" : accent,
                    textShadow: db === null ? "none" : `0 0 ${6 + norm * 16}px ${accent}`,
                    transform: `scale(${1 + norm * 0.06})`,
                    opacity: 0.6 + norm * 0.4,
                  }}
                >
                  <span className="text-[26px] font-semibold leading-none">
                    {db === null ? "—" : db}
                  </span>
                  <span className="text-[12px] font-medium opacity-70">dB</span>
                </div>
                <div className="h-[4px] w-56 max-w-[70%] overflow-hidden rounded-full bg-[var(--color-border)]/50">
                  <div
                    className="h-full rounded-full transition-[width] duration-150"
                    style={{
                      width: `${Math.round(norm * 100)}%`,
                      background: accent,
                      boxShadow: `0 0 ${2 + norm * 12}px ${accent}`,
                    }}
                  />
                </div>
              </div>
            );
          })()}

        {phase === "requesting" && (
          <div className="flex h-full flex-col items-center justify-center gap-3 px-6 text-center">
            <Mic
              size={48}
              className="text-[var(--color-accent)] opacity-80 [animation:bpmPulse_1s_ease-in-out_infinite]"
            />
            <div className="text-[14px] font-medium">Requesting microphone permission…</div>
            <div className="max-w-sm text-[12px] text-[var(--color-muted)]">
              If your OS shows no prompt, Inspector Rust may already have an answer
              on file. Check System Settings → Privacy &amp; Security → Microphone.
            </div>
          </div>
        )}

        {phase === "denied" && (
          <div className="flex h-full flex-col items-center justify-center gap-4 px-6 text-center">
            <MicOff size={48} className="text-amber-500" />
            <div className="text-[14px] font-medium">No audio input available</div>
            <div className="max-w-sm text-[12px] text-[var(--color-muted)]">
              {errorMessage ?? "Microphone unavailable."} Grant Inspector Rust
              microphone access in System Settings and press “Retry”.
            </div>
            <button
              onClick={() => setAttempt((n) => n + 1)}
              className="flex items-center gap-1.5 rounded border border-[var(--color-accent)] bg-[var(--color-accent)]/10 px-3 py-1.5 text-[12px] text-[var(--color-accent)] hover:bg-[var(--color-accent)]/20"
            >
              <RefreshCw size={12} />
              Retry
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

// ── Animation steppers (mutate the VizState ref) ─────────────────────────────

function spawnParticles(v: VizState, intensity: number): void {
  const count = Math.round(8 + intensity * 34);
  for (let i = 0; i < count; i++) {
    const ang = Math.random() * TWO_PI;
    const speed = (60 + intensity * 260) * (0.5 + Math.random() * 0.8);
    const max = 0.45 + Math.random() * 0.6;
    v.particles.push({
      x: 0,
      y: 0,
      vx: Math.cos(ang) * speed,
      vy: Math.sin(ang) * speed,
      life: max,
      max,
      size: 1 + Math.random() * 2.5 + intensity * 1.5,
    });
  }
  // hard cap — drop the oldest if a long loud passage piles them up
  if (v.particles.length > 260) v.particles.splice(0, v.particles.length - 260);
}

function stepParticles(v: VizState, dt: number): void {
  const drag = Math.exp(-1.6 * dt);
  for (const p of v.particles) {
    p.x += p.vx * dt;
    p.y += p.vy * dt;
    p.vx *= drag;
    p.vy *= drag;
    p.life -= dt;
  }
  v.particles = v.particles.filter((p) => p.life > 0);
}

const SHOCK_DUR = 0.95;
function stepShocks(v: VizState, dt: number): void {
  for (const s of v.shocks) s.age += dt;
  v.shocks = v.shocks.filter((s) => s.age < SHOCK_DUR);
}

// ── Canvas renderer ──────────────────────────────────────────────────────────

function drawScene(canvas: HTMLCanvasElement | null, v: VizState): void {
  if (!canvas) return;
  const cssW = canvas.clientWidth;
  const cssH = canvas.clientHeight;
  if (cssW === 0 || cssH === 0) return;
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  const bw = Math.round(cssW * dpr);
  const bh = Math.round(cssH * dpr);
  if (canvas.width !== bw || canvas.height !== bh) {
    canvas.width = bw;
    canvas.height = bh;
  }
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  const cx = cssW / 2;
  const cy = cssH * 0.47;
  const minDim = Math.min(cssW, cssH);
  const baseR = minDim * 0.2;

  // — background: theme bg + a soft central bloom that breathes with energy —
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.fillStyle = rgba(v.bg, 1);
  ctx.fillRect(0, 0, bw, bh);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  const glow = clamp01(0.12 + v.smoothEnergy * 0.5 + v.beatFlash * 0.5);
  const bloom = ctx.createRadialGradient(cx, cy, 0, cx, cy, baseR * 4.2);
  bloom.addColorStop(0, rgba(v.warm, 0.22 * glow));
  bloom.addColorStop(0.5, rgba(v.warm, 0.07 * glow));
  bloom.addColorStop(1, rgba(v.warm, 0));
  ctx.fillStyle = bloom;
  ctx.fillRect(0, 0, cssW, cssH);

  // Everything luminous is drawn additively for a real bloom look.
  ctx.globalCompositeOperation = "lighter";

  // — beat shockwaves —
  for (const s of v.shocks) {
    const t = s.age / SHOCK_DUR;
    const r = baseR * (1.05 + easeOutCubic(t) * (1.6 + s.intensity * 1.4));
    const alpha = (1 - t) * (0.18 + s.intensity * 0.3);
    ctx.beginPath();
    ctx.arc(cx, cy, r, 0, TWO_PI);
    ctx.lineWidth = lerp(3.5, 0.5, t) * (1 + s.intensity);
    ctx.strokeStyle = rgba(mixRgb(v.warm, v.hot, s.intensity), alpha);
    ctx.stroke();
  }

  // — spectrum ring (mirrored left/right for symmetry) —
  const ringInner = baseR * 1.06;
  const maxBar = minDim * 0.16;
  const positions = BAR_COUNT * 2;
  for (let j = 0; j < positions; j++) {
    const idx = j < BAR_COUNT ? j : positions - 1 - j; // mirror
    const mag = v.bars[idx];
    if (mag <= 0.002) continue;
    // start at top (−90°), go clockwise around the full circle, slowly spinning
    const ang = -Math.PI / 2 + (j / positions) * TWO_PI + v.rotation;
    const len = maxBar * (0.06 + mag * 0.94);
    const r0 = ringInner;
    const r1 = ringInner + len;
    const x0 = cx + Math.cos(ang) * r0;
    const y0 = cy + Math.sin(ang) * r0;
    const x1 = cx + Math.cos(ang) * r1;
    const y1 = cy + Math.sin(ang) * r1;
    const col = mixRgb(v.cold, v.hot, clamp01(mag * 1.15));
    ctx.beginPath();
    ctx.moveTo(x0, y0);
    ctx.lineTo(x1, y1);
    ctx.lineWidth = Math.max(1.5, (TWO_PI * ringInner) / positions - 1.4);
    ctx.lineCap = "round";
    ctx.strokeStyle = rgba(col, 0.55 + mag * 0.45);
    ctx.stroke();
  }

  // — breathing core orb —
  const coreR = baseR * (0.62 + v.smoothEnergy * 0.12 + v.coreKick * 0.18);
  const orb = ctx.createRadialGradient(cx, cy, 0, cx, cy, coreR);
  const coreCol = mixRgb(v.warm, v.hot, clamp01(0.25 + v.beatFlash * 0.6));
  orb.addColorStop(0, rgba(coreCol, 0.5 + v.beatFlash * 0.4));
  orb.addColorStop(0.55, rgba(v.warm, 0.18 + v.beatFlash * 0.2));
  orb.addColorStop(1, rgba(v.warm, 0));
  ctx.fillStyle = orb;
  ctx.beginPath();
  ctx.arc(cx, cy, coreR, 0, TWO_PI);
  ctx.fill();

  // thin core rim
  ctx.beginPath();
  ctx.arc(cx, cy, baseR * (0.96 + v.coreKick * 0.06), 0, TWO_PI);
  ctx.lineWidth = 1.5;
  ctx.strokeStyle = rgba(v.hot, 0.22 + v.beatFlash * 0.4);
  ctx.stroke();

  // — particles —
  for (const p of v.particles) {
    const lifeT = clamp01(p.life / p.max);
    ctx.beginPath();
    ctx.arc(cx + p.x, cy + p.y, p.size * (0.4 + lifeT * 0.6), 0, TWO_PI);
    ctx.fillStyle = rgba(mixRgb(v.warm, v.hot, 0.5), lifeT * 0.8);
    ctx.fill();
  }

  // — confidence sweep ring (just inside the spectrum) —
  if (v.hasBeat) {
    const cr = ringInner - minDim * 0.025;
    const sweep = clamp01(v.confidence) * TWO_PI;
    ctx.beginPath();
    ctx.arc(cx, cy, cr, -Math.PI / 2, -Math.PI / 2 + sweep);
    ctx.lineWidth = 2.5;
    ctx.lineCap = "round";
    ctx.strokeStyle = rgba(confidenceColor(v.confidence), 0.85);
    ctx.stroke();
  }

  // — hero BPM number + label (drawn normally, not additive) —
  ctx.globalCompositeOperation = "source-over";
  const heroCol = mixRgb(v.hot, { r: 255, g: 255, b: 255 }, clamp01(v.beatFlash));
  ctx.save();
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.shadowColor = rgba(v.warm, 0.9);
  ctx.shadowBlur = 16 + v.beatFlash * 46;
  const numSize = Math.round(minDim * 0.2);
  ctx.font = `700 ${numSize}px ui-monospace, SFMono-Regular, Menlo, monospace`;
  ctx.fillStyle = rgba(heroCol, 1);
  const label = v.bpm > 0 ? String(Math.round(v.bpm)) : "—";
  ctx.fillText(label, cx, cy - numSize * 0.04);

  ctx.shadowBlur = 0;
  ctx.font = `600 ${Math.round(minDim * 0.026)}px ui-sans-serif, system-ui, sans-serif`;
  ctx.fillStyle = rgba(mixRgb(v.warm, v.hot, 0.4), 0.85);
  ctx.fillText("B P M", cx, cy + numSize * 0.5);
  ctx.restore();

  // — bottom status line —
  ctx.save();
  ctx.textAlign = "center";
  ctx.textBaseline = "alphabetic";
  ctx.font = `500 ${Math.round(Math.max(10, minDim * 0.02))}px ui-sans-serif, system-ui, sans-serif`;
  ctx.fillStyle = rgba(mixRgb(v.warm, v.bg, 0.35), 0.7);
  const status =
    v.bpm > 0
      ? `4-second average · confidence ${Math.round(v.confidence * 100)}%`
      : "listening… calibrating baseline (~3s), then lock-in";
  ctx.fillText(status, cx, cssH - Math.max(14, minDim * 0.05));
  ctx.restore();
}

function energyDescriptor(energy: number): string {
  if (energy < 0.005) return "silence";
  if (energy < 0.02) return "quiet";
  if (energy < 0.1) return "moderate";
  return "loud";
}
