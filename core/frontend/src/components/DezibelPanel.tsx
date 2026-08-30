import { useEffect, useRef, useState } from "react";
import { Mic } from "lucide-react";
import { rms, rmsToDbfs, dbfsToLevel, smoothStep } from "../lib/audio-level";
import { warmContext } from "../lib/warm-audio";
import { startFedMic, type FedMic } from "../lib/mic-feed";

/**
 * `dezibel` / `db` — live microphone loudness in the preview column.
 *
 * The audio path and the animation are the BPM detector's dB readout, lifted
 * out of it rather than re-invented: the same native shared capture
 * (`startFedMic`), the same full-band analyser settings, the same
 * `smoothStep(cur, rmsToDbfs(rms(buf)), 0.5, 0.12)` attack/release, the same
 * `dbfsToLevel(db, -60, -6)` gauge mapping and the same ~7 Hz readout throttle
 * (see `BpmDetector.tsx`, the `dbRef` line in its rAF tick and the readout
 * interval). The visual language is the same too: a mono tabular number whose
 * glow, scale and opacity ride the level, over a thin meter bar.
 *
 * ⚠️ The value is read on every animation frame but pushed into React only
 * ~7×/s. That split is the point: the meter must not re-render the tree at
 * frame rate, and a number changing 60×/s is unreadable anyway.
 */
const READOUT_MS = 140;
/** dBFS window the gauge spans — identical to the BPM readout's. */
const FLOOR_DB = -60;
const CEIL_DB = -6;
/** Below this the input is treated as silence and shown as a dash. */
const SILENT_DB = -90;

type Phase = "requesting" | "listening" | "error";

export function DezibelPanel() {
  const [phase, setPhase] = useState<Phase>("requesting");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [db, setDb] = useState<number | null>(null);
  const [attempt, setAttempt] = useState(0);

  const dbRef = useRef(SILENT_DB);
  const rafRef = useRef<number | null>(null);
  const fedMicRef = useRef<FedMic | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);

  // Throttled readout — keeps React churn off the hot path (BpmDetector does
  // exactly this at the same cadence).
  useEffect(() => {
    const id = window.setInterval(() => {
      setDb(dbRef.current <= SILENT_DB ? null : Math.round(dbRef.current));
    }, READOUT_MS);
    return () => window.clearInterval(id);
  }, []);

  useEffect(() => {
    let cancelled = false;

    (async () => {
      try {
        // Native capture (Rust/cpal) through the SHARED warm context — the
        // webview's getUserMedia makes macOS reconfigure the shared audio
        // device and briefly stutters other apps' playback. `startFedMic` is
        // ref-counted, so running `bpm`/`disco` alongside opens no second
        // stream.
        const ctx = warmContext();
        const fed = await startFedMic(ctx);
        if (cancelled) {
          fed.stop();
          return;
        }
        fedMicRef.current = fed;

        const analyser = ctx.createAnalyser();
        analyser.fftSize = 2048;
        analyser.smoothingTimeConstant = 0.78;
        fed.source.connect(analyser);
        analyserRef.current = analyser;

        setErrorMessage(null);
        setPhase("listening");

        const buf = new Float32Array(analyser.fftSize);
        const tick = () => {
          if (cancelled || !analyserRef.current) return;
          analyserRef.current.getFloatTimeDomainData(buf);
          // Attack fast, release slow — a calm meter that still catches peaks.
          dbRef.current = smoothStep(dbRef.current, rmsToDbfs(rms(buf)), 0.5, 0.12);
          rafRef.current = requestAnimationFrame(tick);
        };
        rafRef.current = requestAnimationFrame(tick);
      } catch (err) {
        if (cancelled) return;
        const e = err as Error;
        setErrorMessage(e.message || e.name || "Audio capture failed");
        setPhase("error");
      }
    })();

    return () => {
      cancelled = true;
      if (rafRef.current !== null) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
      }
      // Stop the native stream + the feed nodes. NEVER close the context — it
      // is the shared warm one and stays warm for the next consumer.
      fedMicRef.current?.stop();
      fedMicRef.current = null;
      analyserRef.current?.disconnect();
      analyserRef.current = null;
      dbRef.current = SILENT_DB;
    };
  }, [attempt]);

  if (phase === "error") {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 px-6 text-center">
        <Mic size={22} className="text-[var(--color-muted)]" />
        <div className="text-[12px] text-[var(--color-fg)]">Mikrofon nicht verfügbar</div>
        <div className="text-[11px] text-[var(--color-muted)]">{errorMessage}</div>
        <button
          type="button"
          onClick={() => setAttempt((a) => a + 1)}
          className="md3-press rounded-full border border-[var(--color-border)] px-3 py-1 text-[11px] text-[var(--color-fg)]"
        >
          Erneut versuchen
        </button>
      </div>
    );
  }

  if (phase === "requesting") {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 px-6 text-center">
        <Mic size={22} className="text-[var(--color-accent)]" />
        <div className="text-[12px] text-[var(--color-muted)]">Mikrofon wird geöffnet…</div>
      </div>
    );
  }

  const accent = "var(--color-accent)";
  const norm = db === null ? 0 : dbfsToLevel(db, FLOOR_DB, CEIL_DB);
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 px-6">
      <div
        className="flex items-baseline gap-1.5 font-[var(--font-mono)] tabular-nums transition-all duration-150"
        style={{
          color: db === null ? "var(--color-muted)" : accent,
          textShadow: db === null ? "none" : `0 0 ${6 + norm * 16}px ${accent}`,
          transform: `scale(${1 + norm * 0.06})`,
          opacity: 0.6 + norm * 0.4,
        }}
      >
        <span className="text-[44px] font-semibold leading-none">{db === null ? "—" : db}</span>
        <span className="text-[16px] font-medium opacity-70">dBFS</span>
      </div>
      <div className="h-[6px] w-full max-w-[260px] overflow-hidden rounded-full bg-[var(--color-border)]/50">
        <div
          className="h-full rounded-full transition-[width] duration-150"
          style={{
            width: `${Math.round(norm * 100)}%`,
            background: accent,
            boxShadow: `0 0 ${2 + norm * 12}px ${accent}`,
          }}
        />
      </div>
      <div className="text-center text-[11px] leading-relaxed text-[var(--color-muted)]">
        0 dBFS ist Vollaussteuerung — Zimmerlautstärke liegt darunter, also negativ.
        <br />
        Skala {FLOOR_DB} … {CEIL_DB} dBFS · Esc schließt und gibt das Mikrofon frei.
      </div>
    </div>
  );
}
