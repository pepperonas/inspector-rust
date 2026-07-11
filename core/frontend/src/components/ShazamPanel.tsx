/**
 * `shazam` — identify the song currently playing. Records ~10 s from the mic,
 * generates the Shazam audio-signature (in Rust), queries Shazam's public API,
 * and shows the match (cover · title · artist · album · genre · link). Inline
 * preview-panel family. Esc exits; Enter (on a result) copies "Title – Artist".
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { AudioLines, Loader2, Music, ExternalLink, Copy, RefreshCw, MicOff } from "lucide-react";
import { shazamRecognize, type ShazamMatch } from "../lib/ipc";
import { recordMic16k } from "../lib/mic-record";
import { openUrl } from "@tauri-apps/plugin-opener";

type Phase = "listening" | "searching" | "result" | "nomatch" | "error" | "noperm";
const RECORD_SECONDS = 10;

export function ShazamPanel({
  focused,
  onExit,
}: {
  focused: boolean;
  onExit: () => void;
}) {
  const [phase, setPhase] = useState<Phase>("listening");
  const [progress, setProgress] = useState(0);
  const [match, setMatch] = useState<ShazamMatch | null>(null);
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const runIdRef = useRef(0);

  const run = useCallback(async () => {
    const myRun = ++runIdRef.current;
    setPhase("listening");
    setProgress(0);
    setMatch(null);
    setError("");
    setCopied(false);
    try {
      const samples = await recordMic16k(RECORD_SECONDS, (p) => {
        if (runIdRef.current === myRun) setProgress(p);
      });
      if (runIdRef.current !== myRun) return;
      setPhase("searching");
      const m = await shazamRecognize(samples);
      if (runIdRef.current !== myRun) return;
      if (m) {
        setMatch(m);
        setPhase("result");
      } else {
        setPhase("nomatch");
      }
    } catch (e) {
      if (runIdRef.current !== myRun) return;
      const msg = String(e);
      if (/permission|denied|NotAllowed/i.test(msg)) {
        setPhase("noperm");
      } else {
        setError(msg);
        setPhase("error");
      }
    }
  }, []);

  useEffect(() => {
    void run();
    return () => {
      // Invalidate any in-flight run on unmount (we only ever increment).
      runIdRef.current += 1;
    };
  }, [run]);

  const copyTitle = useCallback(async () => {
    if (!match) return;
    try {
      const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
      await writeText(`${match.title} – ${match.artist}`);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  }, [match]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onExit();
      } else if (e.key === "Enter") {
        if (phase === "result") void copyTitle();
        else if (phase === "nomatch" || phase === "error" || phase === "noperm") void run();
        else return;
      } else if ((e.key === "r" || e.key === "R") && phase !== "listening" && phase !== "searching") {
        void run();
      } else {
        return;
      }
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, phase, copyTitle, run, onExit]);

  return (
    <div className="flex h-full flex-col gap-3 overflow-hidden p-3 text-sm">
      <div className="flex items-center gap-2 text-[var(--color-fg)]">
        <AudioLines size={16} className="text-rose-400" />
        <span className="font-semibold">Shazam</span>
        <span className="ml-auto text-xs text-[var(--color-muted)]">
          {phase === "listening"
            ? "Listening…"
            : phase === "searching"
              ? "Searching…"
              : phase === "result"
                ? "Match"
                : ""}
        </span>
      </div>

      {(phase === "listening" || phase === "searching") && (
        <div className="flex flex-1 flex-col items-center justify-center gap-5">
          <div className="relative flex h-28 w-28 items-center justify-center">
            {/* pulsing rings */}
            <span className="shazam-ring absolute inset-0 rounded-full border border-rose-400/50" />
            <span
              className="shazam-ring absolute inset-0 rounded-full border border-rose-400/40"
              style={{ animationDelay: "0.6s" }}
            />
            <span
              className="shazam-ring absolute inset-0 rounded-full border border-rose-400/30"
              style={{ animationDelay: "1.2s" }}
            />
            <div className="flex h-16 w-16 items-center justify-center rounded-full bg-rose-600 text-white shadow-lg">
              {phase === "listening" ? (
                <AudioLines size={26} className="shazam-bob" />
              ) : (
                <Loader2 size={26} className="animate-spin" />
              )}
            </div>
          </div>
          {phase === "listening" ? (
            <>
              <div className="h-1.5 w-48 overflow-hidden rounded-full bg-[var(--color-border)]">
                <div
                  className="h-full rounded-full bg-rose-500 transition-[width] duration-200"
                  style={{ width: `${Math.round(progress * 100)}%` }}
                />
              </div>
              <div className="text-xs text-[var(--color-muted)]">
                Play the song near the mic · {Math.ceil(RECORD_SECONDS * (1 - progress))}s
              </div>
            </>
          ) : (
            <div className="text-xs text-[var(--color-muted)]">Matching the fingerprint…</div>
          )}
        </div>
      )}

      {phase === "result" && match && (
        <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto">
          <div className="flex gap-3">
            {match.cover_url ? (
              <img
                src={match.cover_url}
                alt=""
                className="h-24 w-24 shrink-0 rounded-lg object-cover shadow-md"
              />
            ) : (
              <div className="flex h-24 w-24 shrink-0 items-center justify-center rounded-lg bg-[var(--color-surface)]">
                <Music size={30} className="text-[var(--color-muted)]" />
              </div>
            )}
            <div className="min-w-0 flex-1">
              <div className="truncate text-base font-semibold text-[var(--color-fg)]" title={match.title}>
                {match.title}
              </div>
              <div className="truncate text-sm text-[var(--color-muted)]" title={match.artist}>
                {match.artist}
              </div>
              <div className="mt-1 space-y-0.5 text-xs text-[var(--color-muted)]">
                {match.album && <div className="truncate">{match.album}</div>}
                <div className="flex gap-2">
                  {match.genre && <span>{match.genre}</span>}
                  {match.released && <span>· {match.released}</span>}
                </div>
              </div>
            </div>
          </div>
          <div className="flex flex-wrap gap-2">
            {match.shazam_url && (
              <button
                type="button"
                onClick={() => void openUrl(match.shazam_url)}
                className="flex items-center gap-1.5 rounded-lg bg-rose-600 px-3 py-1.5 text-xs font-semibold text-white hover:bg-rose-500"
              >
                <ExternalLink size={13} /> Open in Shazam
              </button>
            )}
            <button
              type="button"
              onClick={() => void copyTitle()}
              className="flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-xs font-semibold text-[var(--color-fg)] hover:bg-[var(--color-surface)]"
            >
              <Copy size={13} /> {copied ? "Copied!" : "Copy title"}
            </button>
            <button
              type="button"
              onClick={() => void run()}
              className="flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-xs font-semibold text-[var(--color-fg)] hover:bg-[var(--color-surface)]"
            >
              <RefreshCw size={13} /> Again
            </button>
          </div>
        </div>
      )}

      {(phase === "nomatch" || phase === "error" || phase === "noperm") && (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 px-4 text-center">
          {phase === "noperm" ? (
            <MicOff size={26} className="text-amber-400" />
          ) : (
            <Music size={26} className="text-[var(--color-muted)]" />
          )}
          <div className="text-[var(--color-fg)]">
            {phase === "nomatch"
              ? "No match found"
              : phase === "noperm"
                ? "Microphone access needed"
                : "Recognition failed"}
          </div>
          <div className="text-xs text-[var(--color-muted)]">
            {phase === "nomatch"
              ? "Try again with the music louder / clearer."
              : phase === "noperm"
                ? "Grant microphone access in System Settings → Privacy."
                : error}
          </div>
          <button
            type="button"
            onClick={() => void run()}
            className="mt-1 flex items-center gap-1.5 rounded-lg bg-rose-600 px-3 py-1.5 text-xs font-semibold text-white hover:bg-rose-500"
          >
            <RefreshCw size={13} /> Listen again
          </button>
        </div>
      )}

      <div className="text-center text-[11px] text-[var(--color-muted)]">
        Records ~{RECORD_SECONDS}s from the mic · R = again · Esc to exit
      </div>
    </div>
  );
}
