import { useEffect, useRef, useState } from "react";
import { Check, Copy, Loader2, Mic, Music, RotateCcw, Speaker, Square, Trash2 } from "lucide-react";
import type { LiveJob } from "../lib/ipc";
import { formatClock, isLiveRecording, liveResultText, modeOfSource, type StartMode } from "../lib/transcribe";
import { t, useT } from "../lib/i18n";

/**
 * Live dictation in the preview column (`transcribe --start`).
 *
 * The session lives in Rust; this only renders the snapshot. Finished
 * sentences are shown normally, the one still being spoken greyed — it is
 * replaced on every update. Stopping (Enter on the `transcribe --stop` row or
 * the button) runs the accurate pass; its text then replaces the live one and
 * is already in the clipboard.
 */
export function TranscribeLivePanel({
  job,
  startError,
  onStart,
  onStop,
  onCancel,
  onCopy,
}: {
  job: LiveJob | null;
  startError: string | null;
  /** Dictation (default), this Mac's audio (`song`) or another device
   *  through the microphone (`room`). */
  onStart: (mode?: StartMode) => void;
  onStop: () => void;
  onCancel: () => void;
  onCopy: (text: string) => void;
}) {
  useT();
  const textRef = useRef<HTMLDivElement>(null);
  const [copied, setCopied] = useState(false);
  const recording = isLiveRecording(job);
  const finished = !!job && (job.state === "done" || job.state === "failed" || job.state === "cancelled");
  const text = job ? (finished && job.state !== "cancelled" ? liveResultText(job) : job.committed) : "";
  const partial = job && !finished ? job.partial : "";

  // Follow the newest words, like a terminal.
  useEffect(() => {
    const el = textRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [text, partial]);
  useEffect(() => setCopied(false), [job?.id]);

  return (
    <div className="flex h-full flex-col gap-3 p-4" data-testid="transcribe-live">
      <div className="flex items-center gap-2 text-[11px] uppercase tracking-wide text-[var(--color-muted)]">
        <Mic size={12} className={recording ? "text-rose-500" : undefined} />
        <span>{t("trx.live.title")}</span>
        {(job?.source === "system" || job?.source === "room") && (
          <span className="normal-case tracking-normal" data-testid="live-source">
            · {t(job.source === "system" ? "trx.live.source_system" : "trx.live.source_room")}
          </span>
        )}
        {job?.language && <span className="normal-case tracking-normal">· {job.language}</span>}
      </div>

      {startError && !recording && (
        <div className="rounded-lg border border-rose-400/40 bg-rose-500/10 p-3 text-[12px] text-rose-400">
          {startError}
        </div>
      )}

      {!job || (!recording && !finished && job.state !== "finalizing") ? (
        <Idle onStart={onStart} />
      ) : (
        <>
          <StatusRow job={job} />
          {(job.source === "system" || job.source === "room") && <SongRow job={job} finished={finished} />}
          <div
            ref={textRef}
            className="min-h-0 flex-1 overflow-y-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-[13px] leading-6"
          >
            {!text && !partial ? (
              <p className="text-[var(--color-muted)]">
                {recording ? t("trx.live.speak") : job.state === "cancelled" ? t("trx.live.discarded") : t("trx.live.nothing")}
              </p>
            ) : (
              <p className="whitespace-pre-wrap">
                {text}
                {partial && (
                  <span className="text-[var(--color-muted)]" data-testid="live-partial">
                    {text ? " " : ""}
                    {partial}
                  </span>
                )}
              </p>
            )}
          </div>
          {job.error && <p className="text-[11px] text-rose-400">{job.error}</p>}
          <div className="flex items-center gap-2">
            {recording ? (
              <>
                <button
                  type="button"
                  onClick={onStop}
                  className="md3-press flex items-center gap-1.5 rounded-lg bg-rose-600 px-3 py-1.5 text-[12px] font-medium text-white"
                >
                  <Square size={11} fill="currentColor" /> {t("trx.live.stop")}
                </button>
                <span className="text-[11px] text-[var(--color-muted)]">{t("trx.live.enter_stops")}</span>
                <button
                  type="button"
                  onClick={onCancel}
                  title={t("trx.live.cancel_title")}
                  className="md3-press ml-auto rounded-lg border border-[var(--color-border)] p-1.5 text-[var(--color-muted)] hover:border-rose-400 hover:text-rose-400"
                >
                  <Trash2 size={12} />
                </button>
              </>
            ) : finished ? (
              <>
                {job.copied && (
                  <span className="confirm-enter flex items-center gap-1 text-[11px] text-emerald-400">
                    <Check size={12} /> {t("trx.live.copied")}
                  </span>
                )}
                <div className="ml-auto flex items-center gap-2">
                  {text && (
                    <button
                      type="button"
                      onClick={() => {
                        onCopy(text);
                        setCopied(true);
                      }}
                      className="md3-press flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] hover:border-[var(--color-accent)]"
                    >
                      {copied ? <Check size={12} /> : <Copy size={12} />} {t("trx.live.copy")}
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={() => onStart(modeOfSource(job.source))}
                    className="md3-press flex items-center gap-1.5 rounded-lg bg-[var(--color-accent)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-accent-fg)]"
                  >
                    <RotateCcw size={12} /> {t("trx.live.again")}
                  </button>
                </div>
              </>
            ) : null}
          </div>
        </>
      )}
    </div>
  );
}

function Idle({ onStart }: { onStart: (mode?: StartMode) => void }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
      <Mic size={28} className="text-[var(--color-muted)]" />
      <p className="max-w-[280px] text-[12px] leading-5 text-[var(--color-muted)]">{t("trx.live.idle")}</p>
      <div className="flex flex-wrap items-center justify-center gap-2">
        <button
          type="button"
          onClick={() => onStart()}
          className="md3-press flex items-center gap-1.5 rounded-lg bg-rose-600 px-3 py-1.5 text-[12px] font-medium text-white"
        >
          <Mic size={12} /> {t("trx.live.start")}
        </button>
        <button
          type="button"
          onClick={() => onStart("song")}
          title={t("trx.live.song_title")}
          className="md3-press flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] hover:border-[var(--color-accent)]"
        >
          <Music size={12} /> {t("trx.live.song")}
        </button>
        <button
          type="button"
          onClick={() => onStart("room")}
          title={t("trx.live.room_title")}
          className="md3-press flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] hover:border-[var(--color-accent)]"
        >
          <Speaker size={12} /> {t("trx.live.room")}
        </button>
      </div>
    </div>
  );
}

function StatusRow({ job }: { job: LiveJob }) {
  if (job.state === "finalizing") {
    return (
      <div className="flex items-center gap-2 text-[12px] text-[var(--color-muted)]">
        <Loader2 size={13} className="animate-spin text-[var(--color-accent)]" />
        {t("trx.live.finalizing")}
      </div>
    );
  }
  if (job.state !== "starting" && job.state !== "recording") return null;
  return (
    <div className="flex items-center gap-3 text-[12px]">
      <span className="flex items-center gap-1.5 font-[var(--font-mono)] text-rose-500">
        <span
          aria-hidden
          className="h-2.5 w-2.5 rounded-full bg-rose-500 motion-safe:animate-pulse"
        />
        {formatClock(job.elapsed_ms)}
      </span>
      <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-[var(--color-surface)]" aria-hidden>
        <div
          className="h-full origin-left rounded-full bg-rose-500"
          style={{ transform: `scaleX(${Math.max(0, Math.min(1, job.level))})` }}
          data-testid="live-level"
        />
      </div>
      {job.ducked_from != null && (
        <span className="text-[11px] text-[var(--color-muted)]" data-testid="live-ducked">
          {t("trx.live.ducked", { from: job.ducked_from })}
        </span>
      )}
      {job.loading_model && (
        <span className="flex items-center gap-1 text-[11px] text-[var(--color-muted)]">
          <Loader2 size={11} className="animate-spin" /> {t("trx.live.loading", { model: job.loading_model })}
        </span>
      )}
    </div>
  );
}

/** The recognised song (song sessions): what Shazam found and whether the
 *  catalogue has its lyrics — those are copied instead of what was heard. */
function SongRow({ job, finished }: { job: LiveJob; finished: boolean }) {
  const song = job.song;
  if (!song) {
    if (finished) return null;
    return (
      <div className="flex items-center gap-2 text-[11px] text-[var(--color-muted)]" data-testid="live-song">
        <Music size={12} className="motion-safe:animate-pulse" />
        {job.recognize_tries ? t("trx.live.song_searching_n", { n: job.recognize_tries }) : t("trx.live.song_searching")}
      </div>
    );
  }
  return (
    <div
      className="flex items-center gap-2.5 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2"
      data-testid="live-song"
    >
      {song.cover_url ? (
        <img src={song.cover_url} alt="" className="h-9 w-9 shrink-0 rounded-md object-cover" />
      ) : (
        <Music size={18} className="shrink-0 text-[var(--color-muted)]" />
      )}
      <div className="min-w-0 flex-1">
        <div className="truncate text-[12px] font-medium">{song.title}</div>
        <div className="truncate text-[11px] text-[var(--color-muted)]">{song.artist}</div>
      </div>
      <span className="shrink-0 text-[10px] text-[var(--color-muted)]">
        {song.lyrics ? (finished ? t("trx.live.song_lyrics_used") : t("trx.live.song_lyrics_found")) : t("trx.live.song_no_lyrics")}
      </span>
    </div>
  );
}
