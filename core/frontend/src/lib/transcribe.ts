/**
 * Pure helpers for the `transcribe` preview panel: per-file status lines,
 * overall progress and a remaining-time estimate. The job itself lives in
 * the Rust backend (`transcribe.rs`) — these only read its snapshot.
 */
import type { TranscribeFile, TranscribeJob } from "./ipc";

export function isJobActive(job: TranscribeJob | null | undefined): boolean {
  return (
    !!job &&
    (job.phase === "starting" || job.phase === "loading_model" || job.phase === "running")
  );
}

/** 0..1 over the whole batch — finished files count fully, a failed or
 *  cancelled file counts as settled too (the bar shows how much of the
 *  batch is behind us, not how much succeeded). */
export function overallProgress(job: TranscribeJob): number {
  if (job.files.length === 0) return job.phase === "finished" ? 1 : 0;
  const sum = job.files.reduce(
    (acc, f) => acc + (f.state === "queued" ? 0 : f.state === "running" ? f.progress : 1),
    0,
  );
  return Math.min(1, Math.max(0, sum / job.files.length));
}

export interface JobCounts {
  done: number;
  failed: number;
  cancelled: number;
  total: number;
}

export function jobCounts(job: TranscribeJob): JobCounts {
  const c: JobCounts = { done: 0, failed: 0, cancelled: 0, total: job.files.length };
  for (const f of job.files) {
    if (f.state === "done") c.done++;
    else if (f.state === "failed") c.failed++;
    else if (f.state === "cancelled") c.cancelled++;
  }
  return c;
}

/** Below this much progress the estimate is noise (language detection and
 *  audio decoding happen before Whisper reports any frames). */
const ETA_MIN_PROGRESS = 0.05;
/** …and it needs a few seconds of runtime to mean anything. */
const ETA_MIN_ELAPSED_MS = 4000;

/** Remaining milliseconds for a running file, or null while unknown. */
export function remainingMs(file: TranscribeFile, now: number): number | null {
  if (file.state !== "running" || file.started_ms == null) return null;
  const elapsed = now - file.started_ms;
  if (file.progress < ETA_MIN_PROGRESS || elapsed < ETA_MIN_ELAPSED_MS) return null;
  if (file.progress >= 1) return 0;
  return (elapsed * (1 - file.progress)) / file.progress;
}

/** `42 s`, `3:05 min`, `1:02 h` — compact, never negative. */
export function formatDuration(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000));
  if (total < 60) return `${total} s`;
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) return `${h}:${String(m).padStart(2, "0")} h`;
  return `${m}:${String(s).padStart(2, "0")} min`;
}

/** The small status line under a file name. */
export function fileStatus(file: TranscribeFile, now: number): string {
  switch (file.state) {
    case "queued":
      return "Wartet";
    case "running": {
      if (file.progress <= 0) return "Audio wird gelesen…";
      const pct = `${Math.floor(file.progress * 100)} %`;
      const left = remainingMs(file, now);
      return left == null ? pct : `${pct} · noch ~${formatDuration(left)}`;
    }
    case "done": {
      const took =
        file.started_ms != null && file.finished_ms != null
          ? ` · ${formatDuration(file.finished_ms - file.started_ms)}`
          : "";
      const lang = file.language ? ` · ${file.language}` : "";
      return `Fertig${lang}${took}`;
    }
    case "failed":
      return file.error ? `Fehler: ${file.error}` : "Fehlgeschlagen";
    case "cancelled":
      return "Abgebrochen";
  }
}

/** Headline for the whole job. */
export function phaseLabel(job: TranscribeJob): string {
  const c = jobCounts(job);
  switch (job.phase) {
    case "starting":
      return "Whisper startet…";
    case "loading_model":
      return "Modell wird geladen…";
    case "running":
      return c.total > 1 ? `Transkribiere ${Math.min(c.done + c.failed + 1, c.total)} von ${c.total}` : "Transkribiere";
    case "finished":
      return c.failed > 0 ? `Fertig — ${c.done} von ${c.total} erstellt` : c.total === 1 ? "Fertig" : `Fertig — ${c.total} Transkripte`;
    case "cancelled":
      return `Abgebrochen — ${c.done} von ${c.total} fertig`;
    case "failed":
      return "Fehlgeschlagen";
  }
}
