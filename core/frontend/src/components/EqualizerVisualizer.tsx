import { useEffect, useRef, useState } from "react";
import { AudioLines, MicOff, Pin, RefreshCw, X } from "lucide-react";
import { setSuppressHide } from "../lib/ipc";
import { warmContext } from "../lib/warm-audio";
import { startFedMic } from "../lib/mic-feed";
import {
  clamp01,
  mixRgb,
  parseColor,
  rgba,
  smoothBars,
  spectrumBars,
  type Rgb,
} from "../lib/bpm-visual";
import { barGeometry, peakDecay } from "../lib/equalizer-visual";

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

/** All mutable per-frame animation state — lives in a ref so the rAF loop
 *  never touches React. */
interface VizState {
  bars: Float32Array; // smoothed spectrum, [0,1] per band
  peaks: Float32Array; // peak-hold markers, [0,1] per band
  level: number; // eased overall energy (for the top-bar label)
  lastT: number;
  bg: Rgb;
  cold: Rgb;
  warm: Rgb;
  hot: Rgb;
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
    peaks: new Float32Array(BAND_COUNT),
    level: 0,
    lastT: 0,
    bg: { r: 12, g: 13, b: 17 },
    cold: FALLBACK_ACCENT,
    warm: FALLBACK_ACCENT,
    hot: { r: 255, g: 255, b: 255 },
  });
  const levelRef = useRef(0);

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
    audioCtxRef.current = null;
    vizAnalyserRef.current = null;
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

        audioCtxRef.current = ctx;
        vizAnalyserRef.current = viz;
        const v = vizRef.current;
        v.bars.fill(0);
        v.peaks.fill(0);
        v.lastT = performance.now();
        refreshPalette();

        setErrorMessage(null);
        setPhase("listening");

        const freqBuf = new Uint8Array(viz.frequencyBinCount);
        // Opening the mic makes macOS reconfigure the shared audio device; for
        // ~300 ms the output can stutter. Defer the heavy draw until it settles
        // (same warm-up as the BPM detector).
        const startT = performance.now();
        const WARMUP_MS = 300;

        const tick = () => {
          if (cancelled || !vizAnalyserRef.current) return;
          const now = performance.now();
          const dt = Math.min(0.05, Math.max(0.001, (now - v.lastT) / 1000));
          v.lastT = now;

          if (now - startT >= WARMUP_MS) {
            vizAnalyserRef.current.getByteFrequencyData(freqBuf);
            // Bass gets fine resolution, highs are averaged — a natural music
            // spectrum. Attack fast (punchy), release slow (satisfying fall).
            smoothBars(v.bars, spectrumBars(freqBuf, BAND_COUNT, 0.68), dt, 0.6, 6);
            peakDecay(v.peaks, v.bars, dt, 0.5);
            // overall energy for the label (mean of the smoothed bars)
            let sum = 0;
            for (let i = 0; i < v.bars.length; i++) sum += v.bars[i];
            v.level = sum / v.bars.length;
            levelRef.current = v.level;
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

  // — soft central bloom that breathes with the overall level —
  const glow = clamp01(0.08 + v.level * 1.6);
  const bloom = ctx.createRadialGradient(
    cssW / 2,
    baseline,
    0,
    cssW / 2,
    baseline,
    Math.max(cssW, cssH) * 0.55,
  );
  bloom.addColorStop(0, rgba(v.warm, 0.14 * glow));
  bloom.addColorStop(0.6, rgba(v.warm, 0.04 * glow));
  bloom.addColorStop(1, rgba(v.warm, 0));
  ctx.fillStyle = bloom;
  ctx.fillRect(0, 0, cssW, cssH);

  ctx.globalCompositeOperation = "lighter";

  for (let i = 0; i < BAND_COUNT; i++) {
    const mag = clamp01(v.bars[i]);
    const x = padX + geo.x(i);
    const h = Math.max(2, mag * maxBarH);
    const y = baseline - h;
    const col = mixRgb(v.cold, v.hot, clamp01(mag * 1.1));

    // bar body — vertical gradient dim→bright, additive glow for loud crests
    const grad = ctx.createLinearGradient(0, baseline, 0, y);
    grad.addColorStop(0, rgba(mixRgb(v.cold, v.warm, 0.3), 0.5 + mag * 0.4));
    grad.addColorStop(1, rgba(col, 0.7 + mag * 0.3));
    ctx.fillStyle = grad;
    roundedTopRect(ctx, x, y, geo.barW, h, radius);
    ctx.fill();

    // gentle mirrored reflection under the baseline (fades fast)
    const refH = Math.min(h * 0.4, cssH - baseline - 2);
    if (refH > 1) {
      const rg = ctx.createLinearGradient(0, baseline, 0, baseline + refH);
      rg.addColorStop(0, rgba(col, 0.18 + mag * 0.12));
      rg.addColorStop(1, rgba(col, 0));
      ctx.fillStyle = rg;
      ctx.fillRect(x, baseline + 1, geo.barW, refH);
    }

    // — peak-hold marker —
    const peak = clamp01(v.peaks[i]);
    if (peak > mag + 0.01) {
      const py = baseline - peak * maxBarH;
      ctx.fillStyle = rgba(v.hot, 0.85);
      ctx.fillRect(x, py - 2, geo.barW, 2.5);
    }
  }

  // — baseline —
  ctx.globalCompositeOperation = "source-over";
  ctx.strokeStyle = rgba(mixRgb(v.warm, v.bg, 0.5), 0.35);
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(padX, baseline + 0.5);
  ctx.lineTo(cssW - padX, baseline + 0.5);
  ctx.stroke();

  // — bottom status line —
  ctx.save();
  ctx.textAlign = "center";
  ctx.textBaseline = "alphabetic";
  const minDim = Math.min(cssW, cssH);
  ctx.font = `500 ${Math.round(Math.max(10, minDim * 0.02))}px ui-sans-serif, system-ui, sans-serif`;
  ctx.fillStyle = rgba(mixRgb(v.warm, v.bg, 0.35), 0.7);
  ctx.fillText(
    `${BAND_COUNT}-band spectrum · log-scaled`,
    cssW / 2,
    cssH - Math.max(10, minDim * 0.03),
  );
  ctx.restore();
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
