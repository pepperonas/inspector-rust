/**
 * `transcribe` — progress of the background Whisper job in the preview
 * column, one row per file (v0.205.0).
 *
 * The job runs in the Rust backend (`transcribe.rs`), so this panel is only
 * a view: Esc closes it (or the whole overlay) and the batch keeps going;
 * typing `transcribe` again reconnects to the same job. The snapshot comes
 * in as a prop from App, which owns the single `transcribe-progress`
 * subscription. Pure status maths: `lib/transcribe.ts`.
 */
import { useEffect, useState } from "react";
import {
  AlertTriangle,
  Check,
  Clock,
  FileAudio,
  Loader2,
  X,
} from "lucide-react";
import { revealPath, transcribeCancel, type TranscribeFile, type TranscribeJob } from "../lib/ipc";
import {
  fileStatus,
  isJobActive,
  overallProgress,
  phaseLabel,
} from "../lib/transcribe";

interface Props {
  job: TranscribeJob | null;
  focused: boolean;
  onExit: () => void;
  /** Enter on a finished job: start a new run with the current arguments. */
  onRestart: () => void;
}

function StateIcon({ state }: { state: TranscribeFile["state"] }) {
  switch (state) {
    case "running":
      return <Loader2 size={14} className="shrink-0 animate-spin text-rose-400" />;
    case "done":
      return <Check size={14} className="shrink-0 text-emerald-400" />;
    case "failed":
      return <AlertTriangle size={14} className="shrink-0 text-amber-400" />;
    case "cancelled":
      return <X size={14} className="shrink-0 text-[var(--color-muted)]" />;
    default:
      return <Clock size={14} className="shrink-0 text-[var(--color-muted)]" />;
  }
}

function Bar({ value, tone }: { value: number; tone: string }) {
  return (
    <div className="h-1 w-full overflow-hidden rounded-full bg-[var(--color-border)]">
      <div
        className={`h-full w-full origin-left rounded-full transition-transform duration-(--duration-base) ease-sharp ${tone}`}
        style={{ transform: `scaleX(${Math.max(0, Math.min(1, value))})` }}
      />
    </div>
  );
}

function barTone(state: TranscribeFile["state"]): string {
  if (state === "done") return "bg-emerald-500";
  if (state === "failed") return "bg-amber-500";
  if (state === "cancelled") return "bg-[var(--color-muted)]";
  return "bg-rose-500";
}

export function TranscribePanel({ job, focused, onExit, onRestart }: Props) {
  const active = isJobActive(job);
  const [now, setNow] = useState(() => Date.now());
  const [armed, setArmed] = useState(false);

  // The remaining-time estimate needs a clock; tick only while running.
  useEffect(() => {
    if (!active) return;
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [active]);

  // A cancel arm only counts for the running job it was armed on.
  const [armedFor, setArmedFor] = useState<number | null>(null);
  const isArmed = armed && active && armedFor === job?.id;

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.repeat) return;
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        if (isArmed) setArmed(false);
        else onExit();
      } else if (e.key === "Enter" && !active) {
        e.preventDefault();
        e.stopPropagation();
        onRestart();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, isArmed, active, onExit, onRestart]);

  const cancel = () => {
    if (!isArmed) {
      setArmed(true);
      setArmedFor(job?.id ?? null);
      return;
    }
    setArmed(false);
    void transcribeCancel();
  };

  if (!job) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 p-3 text-sm text-[var(--color-muted)]">
        <Loader2 size={24} className="animate-spin" />
        <div>Transkription wird gestartet…</div>
      </div>
    );
  }

  const total = overallProgress(job);

  return (
    <div className="flex h-full flex-col gap-2 overflow-hidden p-3 text-sm" data-testid="transcribe-panel">
      <div className="flex items-center gap-2 text-[var(--color-fg)]">
        <FileAudio size={16} className="text-rose-400" />
        <span className="font-semibold">Audio/Video → Text</span>
        {active && (
          <button
            type="button"
            onClick={cancel}
            className={`ml-auto rounded-md px-2 py-0.5 text-xs transition-colors ${
              isArmed
                ? "bg-red-600 text-white"
                : "text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]"
            }`}
          >
            {isArmed ? "Wirklich abbrechen?" : "Abbrechen"}
          </button>
        )}
      </div>

      <div className="space-y-1">
        <div className="flex items-baseline gap-2">
          <span className="text-[var(--color-fg)]" data-testid="transcribe-phase">
            {phaseLabel(job)}
          </span>
          {job.files.length > 0 && (
            <span className="ml-auto text-xs tabular-nums text-[var(--color-muted)]">
              {Math.floor(total * 100)} %
            </span>
          )}
        </div>
        {job.files.length > 0 && (
          <Bar value={total} tone={active ? "bg-rose-500" : job.phase === "finished" ? "bg-emerald-500" : "bg-[var(--color-muted)]"} />
        )}
      </div>

      {job.phase === "failed" && job.error && (
        <div className="panel-enter flex items-start gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 p-2 text-xs text-[var(--color-fg)]">
          <AlertTriangle size={14} className="mt-0.5 shrink-0 text-amber-400" />
          <span className="break-words">{job.error}</span>
        </div>
      )}

      <div className="min-h-0 flex-1 space-y-1 overflow-y-auto pr-1">
        {job.files.map((f) => {
          const clickable = f.state === "done" && !!f.output;
          return (
            <div
              key={f.path}
              data-testid="transcribe-file"
              data-state={f.state}
              role={clickable ? "button" : undefined}
              title={clickable ? `${f.output} — im Finder zeigen` : f.path}
              onClick={clickable ? () => void revealPath(f.output as string) : undefined}
              className={`space-y-1 rounded-lg px-2 py-1.5 ${
                f.state === "running" ? "bg-[var(--color-surface)]" : ""
              } ${clickable ? "cursor-pointer hover:bg-[var(--color-surface)]" : ""}`}
            >
              <div className="flex items-center gap-2">
                <StateIcon state={f.state} />
                <span className="min-w-0 flex-1 truncate text-[var(--color-fg)]">{f.name}</span>
              </div>
              <Bar value={f.state === "queued" ? 0 : f.progress} tone={barTone(f.state)} />
              <div
                className={`truncate text-xs tabular-nums ${
                  f.state === "failed" ? "text-amber-400" : "text-[var(--color-muted)]"
                }`}
              >
                {fileStatus(f, now)}
              </div>
            </div>
          );
        })}
      </div>

      <div className="text-xs text-[var(--color-muted)]">
        {active
          ? "Esc schließt — die Transkription läuft im Hintergrund weiter"
          : "Fertige Datei anklicken zeigt sie im Finder · Enter startet neu · Esc schließt"}
      </div>
    </div>
  );
}
