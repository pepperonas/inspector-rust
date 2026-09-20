import { useEffect, useRef, useState } from "react";
import { Mic } from "lucide-react";
import { dbfsToDisplayDb, dbfsToLevel, smoothStep } from "../lib/audio-level";
import { subscribeMicLevel } from "../lib/mic-feed";

/**
 * `dezibel` / `db` — live microphone loudness in the preview column.
 *
 * ⚠️ The level is computed in RUST on the raw capture chunk (`chunk_dbfs`, the
 * calibrated `iris` path) and arrives as the `mic-level` event — this panel
 * does NOT build a Web-Audio graph. It used to (ScriptProcessor → analyser off
 * the shared warm AudioContext, copied from the BPM detector), which gave it
 * two failure modes a meter must not have: a suspended warm context starved
 * the analyser so the reading was "far too low", and the graph setup could
 * throw ("Audio capture failed") even when the mic was working. Reading the
 * one number Rust already computes removes both — see `lib/mic-feed.ts`
 * `subscribeMicLevel` and `mic_capture.rs` `chunk_dbfs`.
 *
 * Smoothing + display are unchanged: `smoothStep(cur, dbfs, 0.5, 0.12)` on a
 * 60 Hz rAF reading the latest event value (fast attack / slow release), the
 * `dbfsToLevel(db, -60, -6)` gauge, and the ~7 Hz React readout throttle. The
 * value is read every frame but pushed into React only ~7×/s: the meter must
 * not re-render at frame rate, and a number changing 60×/s is unreadable.
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
  // Latest dBFS from the `mic-level` event; the rAF loop smooths toward it so
  // the attack/release feel is frame-rate-based, not tied to the ~25 Hz events.
  const targetRef = useRef(SILENT_DB);
  const rafRef = useRef<number | null>(null);

  // Throttled readout — keeps React churn off the hot path (the meter must not
  // re-render the tree at frame rate).
  useEffect(() => {
    const id = window.setInterval(() => {
      setDb(dbRef.current <= SILENT_DB ? null : Math.round(dbRef.current));
    }, READOUT_MS);
    return () => window.clearInterval(id);
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unsubscribe: (() => void) | null = null;

    const tick = () => {
      // Attack fast, release slow — a calm meter that still catches peaks.
      dbRef.current = smoothStep(dbRef.current, targetRef.current, 0.5, 0.12);
      rafRef.current = requestAnimationFrame(tick);
    };

    (async () => {
      try {
        // The level is computed in Rust on the raw capture chunk and arrives as
        // `mic-level` — no Web-Audio graph here (see the header note). Shared,
        // ref-counted native capture, so running `bpm`/`disco` alongside opens
        // no second stream.
        const unsub = await subscribeMicLevel((dbfs) => {
          targetRef.current = dbfs;
        });
        if (cancelled) {
          unsub();
          return;
        }
        unsubscribe = unsub;
        setErrorMessage(null);
        setPhase("listening");
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
      // Release our ref on the shared native capture (stops it only when the
      // last consumer leaves).
      unsubscribe?.();
      unsubscribe = null;
      dbRef.current = SILENT_DB;
      targetRef.current = SILENT_DB;
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
  // Estimated sound pressure level (SPL) in positive dB (dBFS + 90).
  // The internal dBFS value remains negative below 0 dBFS and drives the meter mapping.
  const displayDb = db === null ? null : dbfsToDisplayDb(db);
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 px-6">
      <div
        className="flex items-baseline gap-1.5 font-[var(--font-mono)] tabular-nums transition-[color,text-shadow,transform,opacity] duration-150"
        style={{
          color: db === null ? "var(--color-muted)" : accent,
          textShadow: db === null ? "none" : `0 0 ${6 + norm * 16}px ${accent}`,
          transform: `scale(${1 + norm * 0.06})`,
          opacity: 0.6 + norm * 0.4,
        }}
      >
        <span className="text-[44px] font-semibold leading-none">
          {displayDb === null ? "—" : displayDb}
        </span>
        <span className="text-[16px] font-medium opacity-70">dB</span>
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
        Schalldruckpegel-Schätzung (SPL) · Zimmerlautstärke ca. 35–50 dB, Sprache 60–75 dB.
        <br />
        Skala ca. {FLOOR_DB + 90} … {CEIL_DB + 90} dB · Esc schließt und gibt das Mikrofon frei.
      </div>
    </div>
  );
}
