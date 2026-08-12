import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AudioLines, MicOff, Pin, RefreshCw, X } from "lucide-react";
import { setSuppressHide } from "../lib/ipc";
import { warmContext } from "../lib/warm-audio";
import { startFedMic } from "../lib/mic-feed";
import { BpmAnalyzer } from "../lib/bpm";
import { rms, rmsToDbfs, smoothStep } from "../lib/audio-level";
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
import { barGeometry, peakDecay } from "../lib/equalizer-visual";
import { newFlap, updateFlap, flapView, type Flap } from "../lib/flap-counter";

/**
 * Live spectrum equalizer — a classic vertical-bar analyzer driven by the
 * microphone. Modeled 1:1 on the `bpm` feature (`BpmDetector`): same shared
 * native capture (`startFedMic`, ref-counted with BPM + disco), same
 * canvas + `requestAnimationFrame` loop over mutable refs (no React re-render
 * per frame), same pin mechanism (Enter → `setSuppressHide` + rose recolour,
 * Esc exits + releases the pin), same theme-token palette.
 *
 * ## Audio graph
 *
 *   mic (cpal, shared) ─ startFedMic ─ source ─→ AnalyserNode(viz, 2048)
 *
 * The FFT is done client-side by the AnalyserNode (exactly like the BPM
 * detector's spectrum ring); `spectrumBars` buckets it into log-spaced bands
 * and `smoothBars` + `peakDecay` do the temporal smoothing + peak-hold.
 * Nothing is connected to `ctx.destination` (no mic monitoring / feedback).
 */

interface Props {
  onExit: () => void;
}

type Phase = "requesting" | "listening" | "denied";

const BAND_COUNT = 28;
const FALLBACK_ACCENT: Rgb = { r: 99, g: 102, b: 241 }; // indigo, if CSS read fails

/** A glowing spark that rises off a band on a loud transient. */
interface Particle {
  x: number;
  y: number;
  vx: number;
  vy: number;
  life: number; // remaining, seconds
  max: number; // initial life
  size: number;
}

/** An expanding ring emitted on each confident beat. */
interface Shock {
  age: number; // seconds since spawn
  intensity: number; // 0..1
}

/** All mutable per-frame animation state — lives in a ref so the rAF loop
 *  never touches React. */
interface VizState {
  bars: Float32Array; // smoothed spectrum, [0,1] per band
  prev: Float32Array; // last frame's bars — to detect rising transients
  peaks: Float32Array; // peak-hold markers, [0,1] per band
  peakHeat: Float32Array; // 0..1 freshness of each peak cap → glow
  particles: Particle[];
  shocks: Shock[]; // beat shockwave rings
  level: number; // eased overall energy (for the top-bar label)
  flash: number; // 0..1 overall beat flash, decays each frame
  beatPunch: number; // 0..1 springy kick on each beat (lifts the bars)
  t: number; // running seconds — drives the shimmer
  lastT: number;
  // Beat + level readouts (mirror the BPM detector).
  bpm: number;
  confidence: number;
  hasBeat: boolean;
  db: number; // smoothed full-band dBFS
  bg: Rgb;
  cold: Rgb;
  warm: Rgb;
  hot: Rgb;
  dbFlap: Flap; // split-flap counter for the dB readout
  bpmFlap: Flap; // split-flap counter for the BPM hero
}

export function EqualizerVisualizer({ onExit }: Props) {
  const [phase, setPhase] = useState<Phase>("requesting");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [levelLabel, setLevelLabel] = useState<string>("silence");
  // Pinned = click-outside won't dismiss (via suppress_hide); the viz also
  // recolours rose. `pinnedRef` is the canonical value the keydown reads.
  const [pinned, setPinned] = useState(false);
  const pinnedRef = useRef(false);

  // Audio graph (refs so re-renders don't recreate the AudioContext).
  const audioCtxRef = useRef<AudioContext | null>(null);
  const fedMicRef = useRef<import("../lib/mic-feed").FedMic | null>(null);
  const vizAnalyserRef = useRef<AnalyserNode | null>(null);
  const rafRef = useRef<number | null>(null);

  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const vizRef = useRef<VizState>({
    bars: new Float32Array(BAND_COUNT),
    prev: new Float32Array(BAND_COUNT),
    peaks: new Float32Array(BAND_COUNT),
    peakHeat: new Float32Array(BAND_COUNT),
    particles: [],
    shocks: [],
    level: 0,
    flash: 0,
    beatPunch: 0,
    t: 0,
    lastT: 0,
    bpm: 0,
    confidence: 0,
    hasBeat: false,
    db: -100,
    bg: { r: 12, g: 13, b: 17 },
    cold: FALLBACK_ACCENT,
    warm: FALLBACK_ACCENT,
    hot: { r: 255, g: 255, b: 255 },
    dbFlap: newFlap(),
    bpmFlap: newFlap(),
  });
  const levelRef = useRef(0);
  // Detection graph (BPM + dB), mirroring the BPM detector.
  const detectAnalyserRef = useRef<AnalyserNode | null>(null);
  const analyzerRef = useRef<BpmAnalyzer>(new BpmAnalyzer());

  // Re-read theme colors from CSS into the viz palette. Rose while pinned.
  const refreshPalette = () => {
    const el = canvasRef.current ?? document.documentElement;
    const cs = getComputedStyle(el);
    const accent: Rgb = pinnedRef.current
      ? { r: 244, g: 63, b: 94 }
      : (parseColor(cs.getPropertyValue("--color-accent")) ?? FALLBACK_ACCENT);
    const bg = parseColor(cs.getPropertyValue("--color-bg")) ?? { r: 12, g: 13, b: 17 };
    const v = vizRef.current;
    v.bg = bg;
    v.cold = mixRgb(accent, bg, 0.4); // dim base of a quiet bar
    v.warm = accent;
    v.hot = mixRgb(accent, { r: 255, g: 255, b: 255 }, 0.72); // loud crest glows hot
  };

  // Toggle pin: suppress the popup's hide-on-blur so click-outside keeps the
  // equalizer floating, and recolour the viz. `pinnedRef` is canonical.
  const togglePin = () => {
    const next = !pinnedRef.current;
    pinnedRef.current = next;
    setPinned(next);
    void setSuppressHide(next).catch(() => undefined);
    refreshPalette();
  };

  // Esc exits (releasing any pin), Enter pins/unpins. Capture phase so they
  // beat the popup's global handlers.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
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

  // The popup hid with the visualizer UNPINNED: exit — otherwise
  // `equalizerMode` survived the hide, the native mic kept recording
  // invisibly, and the next open showed the equalizer instead of the search
  // UI. A PINNED visualizer deliberately survives (same contract as the BPM
  // detector; pinning also suppresses the focus-loss hide).
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

  // Safety: clear suppress_hide if the visualizer unmounts while pinned.
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
    }, 140);
    return () => window.clearInterval(id);
  }, []);

  const teardown = () => {
    if (rafRef.current !== null) {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
    // Stop the native mic stream + tear down the feed source. The AudioContext
    // is the *shared warm* context — never close it.
    fedMicRef.current?.stop();
    fedMicRef.current = null;
    vizAnalyserRef.current?.disconnect();
    detectAnalyserRef.current?.disconnect();
    audioCtxRef.current = null;
    vizAnalyserRef.current = null;
    detectAnalyserRef.current = null;
    analyzerRef.current.reset();
  };

  // Boot the audio graph. Re-entrant via `attempt` so "Retry" re-triggers it.
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let cancelled = false;

    (async () => {
      try {
        const ctx = warmContext();
        const fed = await startFedMic(ctx);
        if (cancelled) {
          fed.stop();
          return;
        }
        fedMicRef.current = fed;

        // Full-spectrum analyser tapped off the raw mic (client-side FFT).
        const viz = ctx.createAnalyser();
        viz.fftSize = 2048; // 1024 freq bins
        viz.smoothingTimeConstant = 0.7; // gentle; the punch comes from smoothBars
        fed.source.connect(viz);

        // Detection chain (BPM + beat), identical to the BPM detector: the mic
        // bandpassed to the 30–100 Hz kick band → analyser → BpmAnalyzer.
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
        fed.source.connect(highpass);
        highpass.connect(lowpass);
        lowpass.connect(detect);

        audioCtxRef.current = ctx;
        vizAnalyserRef.current = viz;
        detectAnalyserRef.current = detect;
        analyzerRef.current.reset();
        const v = vizRef.current;
        v.bars.fill(0);
        v.peaks.fill(0);
        v.lastT = performance.now();
        refreshPalette();

        setErrorMessage(null);
        setPhase("listening");

        const freqBuf = new Uint8Array(viz.frequencyBinCount);
        const detectTimeBuf = new Float32Array(detect.fftSize);
        const vizTimeBuf = new Float32Array(viz.fftSize); // full-band, for dBFS
        // Opening the mic makes macOS reconfigure the shared audio device; for
        // ~300 ms the output can stutter. Defer the heavy draw until it settles
        // (same warm-up as the BPM detector).
        const startT = performance.now();
        const WARMUP_MS = 300;

        const tick = () => {
          if (cancelled || !vizAnalyserRef.current || !detectAnalyserRef.current) return;
          const now = performance.now();
          const dt = Math.min(0.05, Math.max(0.001, (now - v.lastT) / 1000));
          v.lastT = now;

          // — BPM detection (always; cheap) —
          detectAnalyserRef.current.getFloatTimeDomainData(detectTimeBuf);
          analyzerRef.current.push(detectTimeBuf, now);
          const est = analyzerRef.current.estimate(now);
          const bassEnergy = analyzerRef.current.currentEnergy();
          v.bpm = est.bpm;
          v.confidence = est.confidence;
          v.hasBeat = est.bpm > 0;

          // — full-band dBFS (always; cheap), attack fast / release slow —
          vizAnalyserRef.current.getFloatTimeDomainData(vizTimeBuf);
          v.db = smoothStep(v.db, rmsToDbfs(rms(vizTimeBuf)), 0.5, 0.12);

          // Split-flap counters step the DISPLAYED dB/BPM one integer at a time
          // (with a readable dwell) so the numbers flap like an airport board
          // instead of flickering every frame.
          const dtMs = dt * 1000;
          updateFlap(v.dbFlap, v.db <= -90 ? NaN : Math.round(v.db), now, dtMs);
          updateFlap(v.bpmFlap, v.bpm > 0 ? Math.round(v.bpm) : NaN, now, dtMs);

          if (now - startT >= WARMUP_MS) {
            v.t += dt;
            v.prev.set(v.bars);
            vizAnalyserRef.current.getByteFrequencyData(freqBuf);
            // Bass gets fine resolution, highs are averaged — a natural music
            // spectrum. Attack fast (punchy), release slow (satisfying fall).
            smoothBars(v.bars, spectrumBars(freqBuf, BAND_COUNT, 0.68), dt, 0.62, 6);
            peakDecay(v.peaks, v.bars, dt, 0.5);
            // Per-band transients: a bar jumping up lights its peak cap and
            // flicks a spark off the top — that's the eye-candy.
            let sum = 0;
            let maxRise = 0;
            for (let i = 0; i < v.bars.length; i++) {
              sum += v.bars[i];
              const rise = v.bars[i] - v.prev[i];
              if (rise > 0.06 && v.bars[i] > 0.18) {
                v.peakHeat[i] = 1;
                if (rise > maxRise) maxRise = rise;
                if (v.particles.length < 90 && Math.random() < 0.5 + rise) {
                  spawnSpark(v, i);
                }
              }
              // cool the cap glow down
              v.peakHeat[i] = Math.max(0, v.peakHeat[i] - dt * 2.2);
            }
            v.level = sum / v.bars.length;
            levelRef.current = v.level;

            // — beat reaction (BPM-driven, like the BPM detector) —
            const intensity = clamp01(bassEnergy * 8);
            if (est.beatJustFired) {
              v.flash = Math.max(v.flash, 0.6 + intensity * 0.4);
              v.beatPunch = 1;
              v.shocks.push({ age: 0, intensity });
              // a burst of sparks fountaining up from the baseline
              const burst = Math.round(6 + intensity * 22);
              for (let k = 0; k < burst && v.particles.length < 140; k++) {
                spawnBeatSpark(v, intensity);
              }
            }
            v.flash = Math.max(v.flash * Math.exp(-5 * dt), Math.min(1, maxRise * 4));
            v.beatPunch *= Math.exp(-8 * dt);
            stepSparks(v, dt);
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
            <AudioLines size={12} className="text-[var(--color-accent)]" />
          ) : (
            <MicOff size={12} className="text-amber-500" />
          )}
          <span>Equalizer</span>
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

        {phase === "requesting" && (
          <div className="flex h-full flex-col items-center justify-center gap-3 px-6 text-center">
            <AudioLines
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

  // — background —
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.fillStyle = rgba(v.bg, 1);
  ctx.fillRect(0, 0, bw, bh);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  // Layout: bars sit in the horizontal margins, rising from a baseline near the
  // bottom. A little top/side padding keeps the crests off the edges.
  const padX = Math.min(48, cssW * 0.06);
  const fieldW = cssW - padX * 2;
  const baseline = cssH * 0.9; // bottom line the bars grow from
  const maxBarH = baseline - cssH * 0.1; // headroom above the tallest bar
  const geo = barGeometry(fieldW, BAND_COUNT, 0.36);
  const radius = Math.min(6, geo.barW * 0.5);

  const minDim = Math.min(cssW, cssH);

  // — reactive background bloom rising from the baseline + a soft vignette —
  const glow = clamp01(0.06 + v.level * 1.4 + v.flash * 0.5);
  const bloom = ctx.createRadialGradient(
    cssW / 2,
    baseline,
    0,
    cssW / 2,
    baseline,
    Math.max(cssW, cssH) * 0.6,
  );
  bloom.addColorStop(0, rgba(v.warm, 0.16 * glow));
  bloom.addColorStop(0.55, rgba(v.warm, 0.05 * glow));
  bloom.addColorStop(1, rgba(v.warm, 0));
  ctx.fillStyle = bloom;
  ctx.fillRect(0, 0, cssW, cssH);
  const vig = ctx.createRadialGradient(
    cssW / 2,
    cssH / 2,
    minDim * 0.3,
    cssW / 2,
    cssH / 2,
    Math.max(cssW, cssH) * 0.7,
  );
  vig.addColorStop(0, "rgba(0,0,0,0)");
  vig.addColorStop(1, "rgba(0,0,0,0.35)");
  ctx.fillStyle = vig;
  ctx.fillRect(0, 0, cssW, cssH);

  // Everything luminous is drawn additively for a real bloom look.
  ctx.globalCompositeOperation = "lighter";

  // — beat shockwaves: rings expanding from the baseline centre on each beat —
  const baseR = minDim * 0.14;
  for (const s of v.shocks) {
    const t = s.age / SHOCK_DUR;
    const r = baseR * (0.4 + easeOutCubic(t) * (3.2 + s.intensity * 2.2));
    const alpha = (1 - t) * (0.16 + s.intensity * 0.26);
    ctx.beginPath();
    ctx.arc(cssW / 2, baseline, r, Math.PI, 2 * Math.PI); // upper half-ring
    ctx.lineWidth = lerp(3.5, 0.5, t) * (1 + s.intensity);
    ctx.strokeStyle = rgba(mixRgb(v.warm, v.hot, s.intensity), alpha);
    ctx.stroke();
  }

  // Bars lift a touch on each beat (springy punch).
  const punch = 1 + v.beatPunch * 0.13;

  for (let i = 0; i < BAND_COUNT; i++) {
    const mag = clamp01(v.bars[i]);
    const x = padX + geo.x(i);
    const cxb = x + geo.barW / 2;
    // a subtle per-band shimmer so idle bars still feel alive
    const shimmer = 1 + Math.sin(v.t * 2.2 + i * 0.7) * 0.04;
    const h = Math.max(2, mag * maxBarH * shimmer * punch);
    const y = baseline - h;
    const col = mixRgb(v.cold, v.hot, clamp01(mag * 1.15));

    // soft glow halo behind the bar (wider + translucent) — cheap fake-bloom
    if (mag > 0.02) {
      const halo = ctx.createLinearGradient(0, baseline, 0, y - 8);
      halo.addColorStop(0, rgba(col, 0));
      halo.addColorStop(1, rgba(col, 0.2 * mag));
      ctx.fillStyle = halo;
      ctx.fillRect(x - geo.barW * 0.5, y - 8, geo.barW * 2, h + 8);
    }

    // bar body — vertical gradient dim→bright
    const grad = ctx.createLinearGradient(0, baseline, 0, y);
    grad.addColorStop(0, rgba(mixRgb(v.cold, v.warm, 0.35), 0.55 + mag * 0.35));
    grad.addColorStop(1, rgba(col, 0.8 + mag * 0.2));
    ctx.fillStyle = grad;
    roundedTopRect(ctx, x, y, geo.barW, h, radius);
    ctx.fill();

    // bright cap highlight — a near-white crest that reads as a glowing tip
    ctx.fillStyle = rgba(mixRgb(col, { r: 255, g: 255, b: 255 }, 0.55), 0.55 + mag * 0.45);
    roundedTopRect(ctx, x, y, geo.barW, Math.min(radius * 1.6, h), radius);
    ctx.fill();

    // mirrored reflection under the baseline (fades)
    const refH = Math.min(h * 0.45, cssH - baseline - 2);
    if (refH > 1) {
      const rg = ctx.createLinearGradient(0, baseline, 0, baseline + refH);
      rg.addColorStop(0, rgba(col, 0.2 + mag * 0.14));
      rg.addColorStop(1, rgba(col, 0));
      ctx.fillStyle = rg;
      ctx.fillRect(x, baseline + 1, geo.barW, refH);
    }

    // — glowing peak cap: a bloom on a fresh hit, cooling to a thin marker —
    const peak = clamp01(v.peaks[i]);
    if (peak > 0.02) {
      const py = baseline - peak * maxBarH;
      const heat = v.peakHeat[i];
      if (heat > 0.02) {
        const r = 3 + heat * 11;
        const gg = ctx.createRadialGradient(cxb, py, 0, cxb, py, r);
        gg.addColorStop(0, rgba(v.hot, 0.5 * heat));
        gg.addColorStop(1, rgba(v.hot, 0));
        ctx.fillStyle = gg;
        ctx.fillRect(cxb - r, py - r, r * 2, r * 2);
      }
      ctx.fillStyle = rgba(mixRgb(v.hot, { r: 255, g: 255, b: 255 }, heat * 0.6), 0.65 + heat * 0.35);
      ctx.fillRect(x, py - 2, geo.barW, 2.5);
    }
  }

  // — rising sparks flicked off loud transients —
  for (const p of v.particles) {
    const px = padX + p.x * fieldW;
    const py = baseline - clamp01(p.y) * maxBarH;
    const lifeT = clamp01(p.life / p.max);
    const r = p.size * (0.5 + lifeT);
    const gp = ctx.createRadialGradient(px, py, 0, px, py, r * 2.2);
    gp.addColorStop(0, rgba(mixRgb(v.warm, v.hot, 0.6), lifeT * 0.9));
    gp.addColorStop(1, rgba(v.warm, 0));
    ctx.fillStyle = gp;
    ctx.fillRect(px - r * 2.2, py - r * 2.2, r * 4.4, r * 4.4);
  }

  // — whole-scene beat flash (a warm veil that pulses on the beat) —
  if (v.flash > 0.01) {
    ctx.fillStyle = rgba(v.warm, 0.1 * v.flash);
    ctx.fillRect(0, 0, cssW, cssH);
  }

  ctx.globalCompositeOperation = "source-over";

  // — baseline with a level-reactive glow —
  const blGlow = clamp01(0.25 + v.level * 0.8 + v.flash * 0.4);
  ctx.strokeStyle = rgba(mixRgb(v.warm, v.hot, 0.3), 0.5 * blGlow);
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.moveTo(padX, baseline + 0.5);
  ctx.lineTo(cssW - padX, baseline + 0.5);
  ctx.stroke();

  // — BPM hero + dB readout (top HUD, glowing, flares on the beat) —
  drawReadouts(ctx, v, cssW, minDim);

  // — bottom status line —
  ctx.save();
  ctx.textAlign = "center";
  ctx.textBaseline = "alphabetic";
  ctx.font = `500 ${Math.round(Math.max(10, minDim * 0.02))}px ui-sans-serif, system-ui, sans-serif`;
  ctx.fillStyle = rgba(mixRgb(v.warm, v.bg, 0.35), 0.7);
  ctx.fillText(
    `${BAND_COUNT}-band spectrum · log-scaled`,
    cssW / 2,
    cssH - Math.max(10, minDim * 0.03),
  );
  ctx.restore();
}

/** The BPM hero (top-centre) + a dB readout under it — drawn on the canvas so
 *  they glow and flare with the beat without touching React. */
function drawReadouts(
  ctx: CanvasRenderingContext2D,
  v: VizState,
  cssW: number,
  minDim: number,
): void {
  ctx.save();
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  const cx = cssW / 2;
  const heroCol = mixRgb(v.hot, { r: 255, g: 255, b: 255 }, clamp01(v.flash));

  // BPM hero
  const bpmY = minDim * 0.16;
  const numSize = Math.round(minDim * 0.13);
  ctx.shadowColor = rgba(v.warm, 0.9);
  ctx.shadowBlur = 12 + v.flash * 36;
  ctx.font = `700 ${numSize}px ui-monospace, SFMono-Regular, Menlo, monospace`;
  ctx.fillStyle = rgba(heroCol, 1);
  const bpmView = flapView(v.bpmFlap);
  drawFlap(
    ctx,
    Number.isFinite(bpmView.value) ? String(bpmView.value) : "—",
    cx,
    bpmY,
    bpmView.scaleY,
  );
  ctx.shadowBlur = 0;
  ctx.font = `600 ${Math.round(minDim * 0.02)}px ui-sans-serif, system-ui, sans-serif`;
  ctx.fillStyle = rgba(mixRgb(v.warm, v.hot, 0.4), 0.8);
  ctx.fillText("B P M", cx, bpmY + numSize * 0.62);

  // confidence sweep dot under the BPM
  if (v.hasBeat) {
    const dotY = bpmY + numSize * 0.62 + minDim * 0.028;
    ctx.fillStyle = rgba(confidenceColor(v.confidence), 0.9);
    ctx.beginPath();
    ctx.arc(cx, dotY, Math.max(2, minDim * 0.006), 0, Math.PI * 2);
    ctx.fill();
  }

  // dB readout (smaller, quieter — mirrors the BPM detector's meter)
  const dbY = minDim * 0.33;
  const db = v.db <= -90 ? null : Math.round(v.db);
  const dbSize = Math.round(minDim * 0.045);
  ctx.font = `600 ${dbSize}px ui-monospace, SFMono-Regular, Menlo, monospace`;
  const dbCol = db === null ? mixRgb(v.warm, v.bg, 0.5) : v.warm;
  ctx.shadowColor = rgba(v.warm, 0.6);
  ctx.shadowBlur = db === null ? 0 : 4 + clamp01(v.level * 6) * 10;
  ctx.fillStyle = rgba(dbCol, db === null ? 0.6 : 0.95);
  // Number flaps, " dB" stays put: anchor the number|suffix boundary at a fixed
  // x (number right-aligned, suffix left-aligned) so a digit-width change never
  // shifts the readout horizontally.
  const dbView = flapView(v.dbFlap);
  const dbNum = db === null || !Number.isFinite(dbView.value) ? "—" : String(dbView.value);
  const suffix = " dB";
  const anchorX = cx - ctx.measureText(suffix).width / 2;
  ctx.textAlign = "right";
  drawFlap(ctx, dbNum, anchorX, dbY, db === null ? 1 : dbView.scaleY);
  ctx.textAlign = "left";
  ctx.fillText(suffix, anchorX, dbY);
  ctx.restore();
}

/** Draw a flap number honouring the current font/fill/shadow/textAlign, squashed
 *  vertically to `scaleY` (the mid-flip edge-on look). */
function drawFlap(
  ctx: CanvasRenderingContext2D,
  str: string,
  x: number,
  y: number,
  scaleY: number,
): void {
  ctx.save();
  ctx.translate(x, y);
  ctx.scale(1, Math.max(0.03, scaleY));
  ctx.fillText(str, 0, 0);
  ctx.restore();
}

// ── Spark particles ──────────────────────────────────────────────────────────

/** Emit a spark at the crest of band `i` (normalized coords, resolved to px at
 *  draw time so it's resolution-independent). */
function spawnSpark(v: VizState, i: number): void {
  const max = 0.5 + Math.random() * 0.6;
  v.particles.push({
    x: (i + 0.5) / BAND_COUNT + (Math.random() - 0.5) * 0.012,
    y: clamp01(v.bars[i]),
    vx: (Math.random() - 0.5) * 0.22,
    vy: 0.35 + Math.random() * 0.5, // up (normalized units/s)
    life: max,
    max,
    size: 1 + Math.random() * 2,
  });
}

/** A bigger, faster spark that fountains up from the baseline on a beat. */
function spawnBeatSpark(v: VizState, intensity: number): void {
  const max = 0.6 + Math.random() * 0.7;
  v.particles.push({
    x: Math.random(),
    y: 0, // baseline
    vx: (Math.random() - 0.5) * 0.4,
    vy: 0.5 + Math.random() * (0.6 + intensity),
    life: max,
    max,
    size: 1.2 + Math.random() * 2.5,
  });
}

/** Advance sparks: rise, gravity pulls them back, fade out. */
function stepSparks(v: VizState, dt: number): void {
  const G = 0.55; // gravity (norm/s²)
  for (const p of v.particles) {
    p.x += p.vx * dt;
    p.y += p.vy * dt;
    p.vy -= G * dt;
    p.life -= dt;
  }
  v.particles = v.particles.filter((p) => p.life > 0 && p.y > -0.05);
}

const SHOCK_DUR = 0.9;
/** Advance beat shockwave rings; drop the finished ones. */
function stepShocks(v: VizState, dt: number): void {
  for (const s of v.shocks) s.age += dt;
  v.shocks = v.shocks.filter((s) => s.age < SHOCK_DUR);
}

/** Path a rectangle with only its top two corners rounded (bars sit on the
 *  baseline, so the bottom stays square). */
function roundedTopRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  const rr = Math.max(0, Math.min(r, w / 2, h));
  ctx.beginPath();
  ctx.moveTo(x, y + h);
  ctx.lineTo(x, y + rr);
  ctx.quadraticCurveTo(x, y, x + rr, y);
  ctx.lineTo(x + w - rr, y);
  ctx.quadraticCurveTo(x + w, y, x + w, y + rr);
  ctx.lineTo(x + w, y + h);
  ctx.closePath();
}

function energyDescriptor(energy: number): string {
  if (energy < 0.01) return "silence";
  if (energy < 0.05) return "quiet";
  if (energy < 0.2) return "moderate";
  return "loud";
}
