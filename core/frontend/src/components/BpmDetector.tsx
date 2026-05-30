import { useEffect, useRef, useState } from "react";
import { Mic, MicOff, RefreshCw, X } from "lucide-react";
import { BpmAnalyzer, type BpmEstimate } from "../lib/bpm";

/**
 * Live BPM-from-microphone overlay. Replaces the popup's content
 * while active, identical to the game easter eggs.
 *
 * ## Audio graph
 *
 *   mic → BiquadFilter(highpass 30 Hz) → BiquadFilter(lowpass 100 Hz, Q=1.5) → AnalyserNode → (silent sink)
 *
 * Two cascaded biquads form an effective 30-100 Hz **bandpass**,
 * which is the prime kick-drum range (sub at 30-50, fundamental at
 * 60-90, body at 100). v0.45.2 narrowed from a single lowpass at
 * 150 Hz → much less vocal / snare / hi-hat energy leaks into the
 * RMS, so the onset detector sees a cleaner kick-only signal.
 *
 *   • Highpass at 30 Hz removes room rumble + the BT-speaker's
 *     own low-frequency thump that has nothing to do with the music.
 *   • Lowpass at 100 Hz with Q=1.5 has a small resonance peak right
 *     at the kick fundamental — small built-in boost where it matters.
 *
 * The AnalyserNode exposes 1024-sample Float32 time-domain frames
 * at ~60 Hz. We feed each frame into `BpmAnalyzer.push` and render
 * `BpmAnalyzer.estimate` on the same rAF tick.
 *
 * Why no `connect(destination)` on the audio graph: we explicitly
 * do NOT want to monitor the mic through the speakers (feedback
 * loop, especially when the user is recording the speakers
 * themselves to detect the music's BPM). The graph ends at the
 * AnalyserNode.
 *
 * ## Permission
 *
 * `navigator.mediaDevices.getUserMedia({ audio: true })` triggers
 * the macOS / Win / Linux microphone prompt on first use. We render
 * one of three phases:
 *   - `requesting` : prompt is open
 *   - `listening`  : audio flowing
 *   - `denied`     : user said no (or no mic available)
 *
 * `Esc` exits at any phase.
 */

interface Props {
  onExit: () => void;
}

type Phase = "requesting" | "listening" | "denied";

interface VisibleState {
  phase: Phase;
  estimate: BpmEstimate;
  energy: number;
  errorMessage: string | null;
}

export function BpmDetector({ onExit }: Props) {
  const [state, setState] = useState<VisibleState>({
    phase: "requesting",
    estimate: { bpm: 0, confidence: 0, beatJustFired: false },
    energy: 0,
    errorMessage: null,
  });
  // `pulseKey` increments on every detected beat. Used as a `key`
  // on the central BPM number wrapper so React replays the
  // pulse-in animation each time.
  const [pulseKey, setPulseKey] = useState(0);

  // Hold the audio graph in refs so React re-renders don't recreate
  // the AudioContext (expensive, and only one is allowed per page
  // on Safari without explicit close).
  const audioCtxRef = useRef<AudioContext | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const analyzerRef = useRef<BpmAnalyzer>(new BpmAnalyzer());
  const rafRef = useRef<number | null>(null);

  // Esc to exit, mounted only on this component so it doesn't
  // collide with the popup's global Esc handler.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onExit]);

  // Tear-down helper: hoist because both unmount + retry paths use it.
  const teardown = () => {
    if (rafRef.current !== null) {
      cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
    }
    if (streamRef.current) {
      streamRef.current.getTracks().forEach((t) => t.stop());
      streamRef.current = null;
    }
    if (audioCtxRef.current) {
      // closing the context releases the AudioWorklet thread + frees
      // the BiquadFilterNode/AnalyserNode native resources
      void audioCtxRef.current.close().catch(() => undefined);
      audioCtxRef.current = null;
    }
    analyserRef.current = null;
    analyzerRef.current.reset();
  };

  // Boot the audio graph on mount. Re-entrant via `attempt` so the
  // "Retry" button after permission-denial can re-trigger getUserMedia.
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let cancelled = false;

    (async () => {
      try {
        // Hint the OS to apply its mic-side processing — echo
        // cancellation + noise suppression are off so we get the
        // raw signal (the lowpass + RMS already handle background
        // hum). autoGainControl is off to keep onset thresholds
        // meaningful.
        const stream = await navigator.mediaDevices.getUserMedia({
          audio: {
            echoCancellation: false,
            noiseSuppression: false,
            autoGainControl: false,
          },
        });
        if (cancelled) {
          stream.getTracks().forEach((t) => t.stop());
          return;
        }
        const ctx = new AudioContext();
        const source = ctx.createMediaStreamSource(stream);

        // Highpass cuts room rumble + BT-speaker low-end thump
        // (anything below 30 Hz is sub-bass we don't need).
        const highpass = ctx.createBiquadFilter();
        highpass.type = "highpass";
        highpass.frequency.value = 30;
        highpass.Q.value = 0.7;

        // Lowpass at 100 Hz isolates the kick-drum body. v0.45.2:
        // narrowed from 150 Hz to drop more vocals / snare attack
        // out of the energy signal — the user reported the high-end
        // bleed was making the BPM jump. Q=1.5 adds a small bump
        // right at the kick fundamental.
        const lowpass = ctx.createBiquadFilter();
        lowpass.type = "lowpass";
        lowpass.frequency.value = 100;
        lowpass.Q.value = 1.5;

        const analyser = ctx.createAnalyser();
        // 1024 samples ≈ 23 ms at 44.1 kHz — fine enough to localize
        // kick transients, coarse enough to keep RMS noise low.
        analyser.fftSize = 1024;
        // No smoothing: we want the raw energy, smoothing happens in
        // the BPM analyzer.
        analyser.smoothingTimeConstant = 0;

        source.connect(highpass);
        highpass.connect(lowpass);
        lowpass.connect(analyser);
        // No connect(ctx.destination) on purpose — see header comment.

        audioCtxRef.current = ctx;
        streamRef.current = stream;
        analyserRef.current = analyser;
        analyzerRef.current.reset();

        setState((s) => ({ ...s, phase: "listening", errorMessage: null }));

        const buf = new Float32Array(analyser.fftSize);
        const tick = () => {
          if (cancelled || !analyserRef.current) return;
          analyserRef.current.getFloatTimeDomainData(buf);
          const now = performance.now();
          analyzerRef.current.push(buf, now);
          const est = analyzerRef.current.estimate(now);
          const energy = analyzerRef.current.currentEnergy();
          setState((s) => ({ ...s, estimate: est, energy }));
          if (est.beatJustFired) {
            setPulseKey((k) => k + 1);
          }
          rafRef.current = requestAnimationFrame(tick);
        };
        rafRef.current = requestAnimationFrame(tick);
      } catch (err) {
        // getUserMedia throws `NotAllowedError` on deny + `NotFoundError`
        // when there's no input device. Both surface the same UI;
        // the message text varies.
        const e = err as Error;
        if (cancelled) return;
        setState((s) => ({
          ...s,
          phase: "denied",
          errorMessage: e.message || e.name || "Audio capture failed",
        }));
      }
    })();

    return () => {
      cancelled = true;
      teardown();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [attempt]);

  return (
    <div className="flex h-full w-full flex-col bg-[var(--color-bg)] text-[var(--color-fg)]">
      {/* Top bar with title + exit hint */}
      <div className="flex items-center justify-between border-b border-[var(--color-border)] px-4 py-2 text-[11px] uppercase tracking-wider text-[var(--color-muted)]">
        <div className="flex items-center gap-2">
          {state.phase === "listening" ? (
            <Mic size={12} className="text-[var(--color-accent)]" />
          ) : (
            <MicOff size={12} className="text-amber-500" />
          )}
          <span>BPM detector</span>
          {state.phase === "listening" && (
            <span className="text-[var(--color-muted)]">
              · {energyDescriptor(state.energy)}
            </span>
          )}
        </div>
        <button
          onClick={onExit}
          className="flex items-center gap-1 rounded border border-[var(--color-border)] px-2 py-0.5 text-[10px] hover:border-[var(--color-accent)] hover:text-[var(--color-accent)]"
        >
          <X size={11} />
          Esc
        </button>
      </div>

      {/* Body */}
      <div className="flex flex-1 flex-col items-center justify-center gap-6 px-6 py-8">
        {state.phase === "requesting" && (
          <div className="flex flex-col items-center gap-3 text-center">
            <Mic
              size={48}
              className="text-[var(--color-accent)] opacity-80 [animation:bpmPulse_1s_ease-in-out_infinite]"
            />
            <div className="text-[14px] font-medium">Mikrofon-Berechtigung anfragen…</div>
            <div className="max-w-sm text-[12px] text-[var(--color-muted)]">
              Falls macOS / dein OS keinen Prompt zeigt: Inspector
              Rust hat eventuell schon eine Antwort. Prüfe System
              Settings → Privacy &amp; Security → Microphone.
            </div>
          </div>
        )}

        {state.phase === "denied" && (
          <div className="flex flex-col items-center gap-4 text-center">
            <MicOff size={48} className="text-amber-500" />
            <div className="text-[14px] font-medium">Kein Audio-Input verfügbar</div>
            <div className="max-w-sm text-[12px] text-[var(--color-muted)]">
              {state.errorMessage ??
                "getUserMedia hat den Zugriff verweigert."}{" "}
              Schalte das Mikrofon in den System-Einstellungen für
              Inspector Rust frei und drücke „Retry“.
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

        {state.phase === "listening" && (
          <>
            {/* Big BPM number — pulses on every detected beat. */}
            <div
              key={pulseKey}
              className="flex flex-col items-center [animation:bpmBeatPulse_0.3s_ease-out]"
            >
              <div className="font-[var(--font-mono)] text-[88px] font-bold leading-none tabular-nums">
                {state.estimate.bpm > 0 ? Math.round(state.estimate.bpm) : "—"}
              </div>
              <div className="mt-1 text-[10px] uppercase tracking-[0.2em] text-[var(--color-muted)]">
                BPM
              </div>
            </div>

            {/* Confidence + status text */}
            <div className="flex flex-col items-center gap-2">
              <div className="text-[11px] text-[var(--color-muted)]">
                {state.estimate.bpm === 0
                  ? "Listening… spiele Musik in das Mikrofon"
                  : `4-Sekunden-Mittel · Confidence: ${Math.round(state.estimate.confidence * 100)}%`}
              </div>
              <ConfidenceBar value={state.estimate.confidence} />
            </div>

            {/* Energy meter — gives feedback that audio is flowing
                even before BPM locks. */}
            <div className="flex w-72 flex-col gap-1">
              <div className="flex justify-between text-[9px] uppercase tracking-wider text-[var(--color-muted)]">
                <span>Input level</span>
                <span>{state.energy > 0.001 ? "live" : "quiet"}</span>
              </div>
              <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--color-surface)]">
                <div
                  className="h-full bg-[var(--color-accent)] transition-[width] duration-75 ease-out"
                  style={{
                    width: `${Math.min(100, state.energy * 600)}%`,
                  }}
                />
              </div>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function ConfidenceBar({ value }: { value: number }) {
  const pct = Math.round(value * 100);
  const color =
    value >= 0.7
      ? "bg-emerald-500"
      : value >= 0.4
        ? "bg-amber-500"
        : "bg-rose-500";
  return (
    <div className="h-1 w-48 overflow-hidden rounded-full bg-[var(--color-surface)]">
      <div
        className={`h-full ${color} transition-[width] duration-200`}
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

function energyDescriptor(energy: number): string {
  if (energy < 0.005) return "silence";
  if (energy < 0.02) return "quiet";
  if (energy < 0.1) return "moderate";
  return "loud";
}
