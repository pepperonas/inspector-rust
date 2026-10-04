import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Gauge, X } from "lucide-react";
import {
  gestureCalibrateApply,
  gestureCalibrateCancel,
  gestureCalibrateStart,
  type GestureCalibrationProposal,
  type GestureCalibrationStatus,
  type GestureConfig,
} from "../lib/ipc";
import { calibrationView, CALIBRATION_STEPS, describeChange } from "../lib/gesture-calibrate";

/**
 * Guided calibration (`gestures calibrate`): a countdown, three measured
 * steps, then a proposal that is only saved after the user confirms it. The
 * progress comes from the panel's live poll (`status`), so this card has no
 * timer of its own.
 */
export function GestureCalibration({
  status,
  supported,
  autoStart,
  onApplied,
}: {
  status: GestureCalibrationStatus | undefined;
  /** The platform reports raw contacts (macOS). */
  supported: boolean;
  /** Start once on mount (`gestures calibrate` + Enter). */
  autoStart: boolean;
  onApplied: (cfg: GestureConfig) => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const autoStarted = useRef(false);

  const start = useCallback(async () => {
    setError(null);
    try {
      await gestureCalibrateStart();
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    if (!autoStart || autoStarted.current || !supported) return;
    autoStarted.current = true;
    void start();
  }, [autoStart, supported, start]);

  const cancel = () => {
    setError(null);
    void gestureCalibrateCancel();
  };

  const apply = async () => {
    setBusy(true);
    try {
      onApplied(await gestureCalibrateApply());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const view = calibrationView(status);

  return (
    <section className="flex flex-col gap-2 rounded-lg border border-[var(--color-border)] p-3" aria-label="Kalibrierung">
      <div className="flex items-center gap-2">
        <Gauge size={14} className="text-[var(--color-accent)]" />
        <span className="font-medium">Kalibrierung</span>
        {view.kind !== "idle" && view.kind !== "done" && view.kind !== "failed" && (
          <button
            type="button"
            onClick={cancel}
            className="ml-auto rounded px-2 py-0.5 text-[var(--color-muted)] hover:bg-[var(--color-border)]/40"
          >
            Abbrechen
          </button>
        )}
      </div>

      {!supported ? (
        <p className="text-[var(--color-muted)]">
          Die Kalibrierung misst die Kontakte selbst — die liefert bisher nur macOS.
        </p>
      ) : view.kind === "idle" ? (
        <>
          <p className="text-[var(--color-muted)]">
            Misst in rund 20 Sekunden, wie groß Handballen, Daumen und Finger auf diesem Trackpad sind, und
            schlägt passende Schwellen vor. Gespeichert wird erst, wenn du zustimmst.
          </p>
          <ol className="flex list-decimal flex-col gap-0.5 pl-4 text-[11px] text-[var(--color-muted)]">
            {CALIBRATION_STEPS.map((s) => (
              <li key={s.title}>{s.instruction}</li>
            ))}
          </ol>
          <button
            type="button"
            onClick={() => void start()}
            className="md3-press self-start rounded-md bg-[var(--color-accent)] px-3 py-1 text-[var(--color-accent-fg)]"
          >
            Kalibrieren
          </button>
        </>
      ) : view.kind === "countdown" ? (
        <p className="text-sm">
          Gleich geht es los <span className="tabular-nums font-semibold">{view.seconds}</span> — lies schon mal
          Schritt 1: {CALIBRATION_STEPS[0].instruction}
        </p>
      ) : view.kind === "step" ? (
        <div className="flex flex-col gap-1.5">
          <span className="text-[11px] text-[var(--color-muted)]">
            Schritt {view.index} von {view.of} · {view.title}
          </span>
          <p className="text-sm font-medium">{view.instruction}</p>
          <div className="h-1.5 overflow-hidden rounded-full bg-[var(--color-border)]/60">
            <div
              role="progressbar"
              aria-valuenow={Math.round(view.progress * 100)}
              aria-valuemin={0}
              aria-valuemax={100}
              className="h-full origin-left bg-[var(--color-accent)]"
              style={{ transform: `scaleX(${view.progress})` }}
            />
          </div>
          <span className="tabular-nums text-[11px] text-[var(--color-muted)]">noch {view.seconds} s</span>
        </div>
      ) : view.kind === "computing" ? (
        <p className="text-[var(--color-muted)]">Werte werden berechnet …</p>
      ) : view.kind === "failed" ? (
        <>
          <p className="text-amber-400">{view.error}</p>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={() => void start()}
              className="md3-press rounded-md border border-[var(--color-border)] px-3 py-1 hover:bg-[var(--color-border)]/40"
            >
              Nochmal
            </button>
            <button type="button" onClick={cancel} className="rounded px-2 py-1 text-[var(--color-muted)]">
              Schließen
            </button>
          </div>
        </>
      ) : status?.state === "done" ? (
        <ProposalView proposal={status.proposal} busy={busy} onApply={() => void apply()} onDiscard={cancel} />
      ) : null}

      {error && <p className="text-amber-400">{error}</p>}
    </section>
  );
}

function ProposalView({
  proposal,
  busy,
  onApply,
  onDiscard,
}: {
  proposal: GestureCalibrationProposal;
  busy: boolean;
  onApply: () => void;
  onDiscard: () => void;
}) {
  const rows = proposal.changes.map(describeChange);
  const where =
    proposal.device?.builtin === false
      ? "einem externen Trackpad"
      : proposal.device?.builtin === true
        ? "dem eingebauten Trackpad"
        : "diesem Trackpad";
  return (
    <div className="flex flex-col gap-2">
      <span className="text-[11px] text-[var(--color-muted)]">Gemessen auf {where}.</span>
      {rows.length === 0 ? (
        <p>Deine Schwellen passen schon — es gibt nichts zu ändern.</p>
      ) : (
        <table className="w-full tabular-nums">
          <tbody>
            {rows.map((r) => (
              <tr key={r.key}>
                <td className="py-0.5">{r.label}</td>
                <td className="py-0.5 text-right text-[var(--color-muted)]">{r.from}</td>
                <td className="px-1 py-0.5 text-[var(--color-muted)]">→</td>
                <td className="py-0.5 font-semibold">{r.to}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {proposal.warnings.length > 0 && (
        <ul className="flex flex-col gap-0.5 text-[11px] text-amber-400">
          {proposal.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      )}
      <div className="flex gap-2">
        {rows.length > 0 && (
          <button
            type="button"
            onClick={onApply}
            disabled={busy}
            className="md3-press flex items-center gap-1 rounded-md bg-[var(--color-accent)] px-3 py-1 text-[var(--color-accent-fg)] disabled:opacity-60"
          >
            <Check size={12} /> Übernehmen
          </button>
        )}
        <button
          type="button"
          onClick={onDiscard}
          className="flex items-center gap-1 rounded-md border border-[var(--color-border)] px-3 py-1 hover:bg-[var(--color-border)]/40"
        >
          <X size={12} /> {rows.length > 0 ? "Verwerfen" : "Schließen"}
        </button>
      </div>
    </div>
  );
}
