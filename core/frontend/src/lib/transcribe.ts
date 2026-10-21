/**
 * Pure helpers for the `transcribe` preview panel: per-file status lines,
 * overall progress and a remaining-time estimate. The job itself lives in
 * the Rust backend (`transcribe.rs`) — these only read its snapshot.
 */
import type { LiveJob, TranscribeFile, TranscribeJob } from "./ipc";
import { t } from "./i18n";

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
      return t("tr.queued");
    case "running": {
      if (file.progress <= 0) return t("tr.reading");
      const pct = `${Math.floor(file.progress * 100)} %`;
      const left = remainingMs(file, now);
      return left == null ? pct : t("tr.pct_left", { pct, left: formatDuration(left) });
    }
    case "done": {
      const took =
        file.started_ms != null && file.finished_ms != null
          ? ` · ${formatDuration(file.finished_ms - file.started_ms)}`
          : "";
      const lang = file.language ? ` · ${file.language}` : "";
      return `${t("tr.done")}${lang}${took}`;
    }
    case "failed":
      return file.error ? t("tr.error", { msg: file.error }) : t("tr.failed");
    case "cancelled":
      return t("tr.cancelled");
  }
}

/** Headline for the whole job. */
export function phaseLabel(job: TranscribeJob): string {
  const c = jobCounts(job);
  switch (job.phase) {
    case "starting":
      return t("tr.phase.starting");
    case "loading_model":
      return t("tr.phase.loading");
    case "running":
      return c.total > 1 ? t("tr.phase.running_n", { i: Math.min(c.done + c.failed + 1, c.total), n: c.total }) : t("tr.phase.running");
    case "finished":
      return c.failed > 0 ? t("tr.phase.finished_some", { done: c.done, n: c.total }) : c.total === 1 ? t("tr.done") : t("tr.phase.finished_n", { n: c.total });
    case "cancelled":
      return t("tr.phase.cancelled_n", { done: c.done, n: c.total });
    case "failed":
      return t("tr.failed");
  }
}

// ── Live dictation (`transcribe --start` / `--stop`) ─────────────────────────

/** `--start` / `--stop` in the argument, or null for a file run. Mirrors the
 *  Rust parser's flag words (`transcribe::parse_args`). */
export function liveAction(arg: string): "start" | "stop" | null {
  const words = arg.trim().split(/\s+/);
  if (words.includes("--stop")) return "stop";
  if (words.includes("--start") || words.some(isSongFlag)) return "start";
  return null;
}

/** `--song` (`--lyrics`, `--system`): transcribe what this Mac plays. */
export function isSongFlag(word: string): boolean {
  return word === "--song" || word === "--lyrics" || word === "--system";
}

/** The language given with `--start`, if any (`--language de`, `-l de`, `--language=de`). */
export function liveLanguage(arg: string): string | null {
  const words = arg.trim().split(/\s+/);
  for (let i = 0; i < words.length; i++) {
    const w = words[i];
    if (w.startsWith("--language=")) return w.slice(11) || null;
    if ((w === "--language" || w === "-l") && words[i + 1]) return words[i + 1];
  }
  return null;
}

export function isLiveActive(job: LiveJob | null | undefined): boolean {
  return !!job && (job.state === "starting" || job.state === "recording" || job.state === "finalizing");
}

/** Still listening — Enter means "stop". */
export function isLiveRecording(job: LiveJob | null | undefined): boolean {
  return !!job && (job.state === "starting" || job.state === "recording");
}

/** `0:07`, `12:30`, `1:02:03` — the recording clock. */
export function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/** What the command row says: Enter's meaning right now. */
export function liveRowLabel(job: LiveJob | null | undefined, arg: string): string {
  if (isLiveRecording(job)) return t("trx.live.row_stop");
  if (job?.state === "finalizing") return t("trx.live.row_finalizing");
  const lang = liveLanguage(arg);
  return lang ? t("trx.live.row_start_lang", { lang }) : t("trx.live.row_start");
}

/** The text a finished dictation produced (what was copied). */
export function liveResultText(job: LiveJob): string {
  if (job.final_text && job.final_text.trim()) return job.final_text;
  return [job.committed, job.partial].map((x) => x.trim()).filter(Boolean).join(" ");
}
