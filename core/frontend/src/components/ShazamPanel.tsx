/**
 * `shazam` — identify the song currently playing. Records ~10 s from the mic,
 * generates the Shazam audio-signature (in Rust), queries Shazam's public API,
 * shows the match (cover · title · artist · album · genre · links to Shazam /
 * Spotify / YouTube), and persists every match to a local history. A header
 * toggle switches between the recognizer and the history list. Esc exits.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import {
  AudioLines,
  Loader2,
  Music,
  ExternalLink,
  Copy,
  RefreshCw,
  MicOff,
  History,
  Trash2,
  X,
} from "lucide-react";
import {
  shazamListen,
  shazamHistoryList,
  shazamHistoryClear,
  shazamHistoryDelete,
  type ShazamMatch,
  type ShazamHistoryEntry,
} from "../lib/ipc";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";

type Phase = "listening" | "searching" | "result" | "nomatch" | "error" | "noperm";
const RECORD_SECONDS = 10;

const SPOTIFY_GREEN = "#1DB954";
const YT_RED = "#FF0000";

function timeAgo(ms: number): string {
  const s = Math.max(0, Math.floor((Date.now() - ms) / 1000));
  if (s < 60) return "just now";
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  if (d < 7) return `${d}d ago`;
  return new Date(ms).toLocaleDateString();
}

/** Small round icon link button. */
function LinkChip({
  label,
  color,
  onClick,
}: {
  label: string;
  color?: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="rounded-full border border-[var(--color-border)] px-2 py-0.5 text-[10px] font-semibold hover:bg-[var(--color-surface)]"
      style={color ? { color } : undefined}
    >
      {label}
    </button>
  );
}

export function ShazamPanel({
  focused,
  initialView = "recognize",
  onExit,
}: {
  focused: boolean;
  /** `history` opens straight into the history list (no mic). Default: recognize. */
  initialView?: "recognize" | "history";
  onExit: () => void;
}) {
  const [view, setView] = useState<"recognize" | "history">(initialView);
  const [phase, setPhase] = useState<Phase>("listening");
  const [progress, setProgress] = useState(0);
  const [match, setMatch] = useState<ShazamMatch | null>(null);
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const [history, setHistory] = useState<ShazamHistoryEntry[]>([]);
  const runIdRef = useRef(0);

  const loadHistory = useCallback(async () => {
    try {
      setHistory(await shazamHistoryList(100));
    } catch {
      /* ignore */
    }
  }, []);

  const run = useCallback(async () => {
    const myRun = ++runIdRef.current;
    setView("recognize");
    setPhase("listening");
    setProgress(0);
    setMatch(null);
    setError("");
    setCopied(false);
    // Native recording (Rust/cpal) emits progress events while it records.
    const unlisten = await listen<number>("shazam-progress", (e) => {
      if (runIdRef.current !== myRun) return;
      setProgress(e.payload);
      if (e.payload >= 0.999) setPhase("searching");
    });
    try {
      const m = await shazamListen(RECORD_SECONDS);
      if (runIdRef.current !== myRun) return;
      if (m) {
        setMatch(m);
        setPhase("result");
        void loadHistory();
      } else {
        setPhase("nomatch");
      }
    } catch (e) {
      if (runIdRef.current !== myRun) return;
      const msg = String(e);
      if (/permission|denied|NotAllowed|no audio|no microphone|microphone/i.test(msg)) {
        setPhase("noperm");
      } else {
        setError(msg);
        setPhase("error");
      }
    } finally {
      unlisten();
    }
  }, [loadHistory]);

  useEffect(() => {
    void loadHistory();
    // `shazam history` opens the list without touching the mic; `shazam`
    // starts listening straight away.
    if (initialView !== "history") void run();
    return () => {
      runIdRef.current += 1; // invalidate in-flight run on unmount
    };
    // Run once on mount for the chosen initial view.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

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

  const clearHistory = useCallback(async () => {
    try {
      await shazamHistoryClear();
      setHistory([]);
    } catch {
      /* ignore */
    }
  }, []);

  const deleteEntry = useCallback(async (id: number) => {
    try {
      await shazamHistoryDelete(id);
      setHistory((h) => h.filter((e) => e.id !== id));
    } catch {
      /* ignore */
    }
  }, []);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onExit();
      } else if (e.key === "Enter" && view === "recognize") {
        if (phase === "result") void copyTitle();
        else if (phase === "nomatch" || phase === "error" || phase === "noperm") void run();
        else return;
      } else if (
        (e.key === "r" || e.key === "R") &&
        view === "recognize" &&
        phase !== "listening" &&
        phase !== "searching"
      ) {
        void run();
      } else {
        return;
      }
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, phase, view, copyTitle, run, onExit]);

  return (
    <div className="flex h-full flex-col gap-3 overflow-hidden p-3 text-sm">
      <div className="flex items-center gap-2 text-[var(--color-fg)]">
        <AudioLines size={16} className="text-rose-400" />
        <span className="font-semibold">Shazam</span>
        {/* view toggle */}
        <div className="ml-auto flex items-center gap-1 rounded-full bg-[var(--color-surface)] p-0.5 text-xs">
          <button
            type="button"
            onClick={() => setView("recognize")}
            className={
              "rounded-full px-2 py-0.5 " +
              (view === "recognize" ? "bg-rose-600 text-white" : "text-[var(--color-muted)]")
            }
          >
            Listen
          </button>
          <button
            type="button"
            onClick={() => {
              setView("history");
              void loadHistory();
            }}
            className={
              "flex items-center gap-1 rounded-full px-2 py-0.5 " +
              (view === "history" ? "bg-rose-600 text-white" : "text-[var(--color-muted)]")
            }
          >
            <History size={11} /> {history.length}
          </button>
        </div>
      </div>

      {/* ── Recognize view ── */}
      {view === "recognize" && (phase === "listening" || phase === "searching") && (
        <div className="flex flex-1 flex-col items-center justify-center gap-5">
          <div className="relative flex h-28 w-28 items-center justify-center">
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

      {view === "recognize" && phase === "result" && match && (
        <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto">
          <div className="flex gap-3">
            {match.cover_url ? (
              <img src={match.cover_url} alt="" className="h-24 w-24 shrink-0 rounded-lg object-cover shadow-md" />
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
                <ExternalLink size={13} /> Shazam
              </button>
            )}
            <button
              type="button"
              onClick={() => void openUrl(match.spotify_url)}
              className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-semibold text-white"
              style={{ backgroundColor: SPOTIFY_GREEN }}
            >
              <ExternalLink size={13} /> Spotify
            </button>
            <button
              type="button"
              onClick={() => void openUrl(match.youtube_url)}
              className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-semibold text-white"
              style={{ backgroundColor: YT_RED }}
            >
              <ExternalLink size={13} /> YouTube
            </button>
            <button
              type="button"
              onClick={() => void copyTitle()}
              className="flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-xs font-semibold text-[var(--color-fg)] hover:bg-[var(--color-surface)]"
            >
              <Copy size={13} /> {copied ? "Copied!" : "Copy"}
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

      {view === "recognize" &&
        (phase === "nomatch" || phase === "error" || phase === "noperm") && (
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

      {/* ── History view ── */}
      {view === "history" && (
        <div className="flex min-h-0 flex-1 flex-col gap-2">
          {history.length > 0 && (
            <div className="flex items-center justify-between text-xs text-[var(--color-muted)]">
              <span>{history.length} recognized</span>
              <button
                type="button"
                onClick={() => void clearHistory()}
                className="flex items-center gap-1 rounded px-1.5 py-0.5 hover:bg-[var(--color-surface)]"
              >
                <Trash2 size={12} /> Clear all
              </button>
            </div>
          )}
          <div className="min-h-0 flex-1 space-y-1 overflow-y-auto pr-1">
            {history.map((e) => (
              <div
                key={e.id}
                className="group flex items-center gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5"
              >
                {e.cover_url ? (
                  <img src={e.cover_url} alt="" className="h-9 w-9 shrink-0 rounded object-cover" />
                ) : (
                  <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded bg-[var(--color-bg)]">
                    <Music size={14} className="text-[var(--color-muted)]" />
                  </div>
                )}
                <div className="min-w-0 flex-1">
                  <div className="truncate text-xs font-semibold text-[var(--color-fg)]" title={e.title}>
                    {e.title}
                  </div>
                  <div className="truncate text-[11px] text-[var(--color-muted)]">
                    {e.artist} · {timeAgo(e.recognized_at)}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  {e.shazam_url && <LinkChip label="Sh" onClick={() => void openUrl(e.shazam_url)} />}
                  <LinkChip label="Sp" color={SPOTIFY_GREEN} onClick={() => void openUrl(e.spotify_url)} />
                  <LinkChip label="YT" color={YT_RED} onClick={() => void openUrl(e.youtube_url)} />
                  <button
                    type="button"
                    onClick={() => void deleteEntry(e.id)}
                    className="rounded p-0.5 text-[var(--color-muted)] opacity-0 hover:text-rose-400 group-hover:opacity-100"
                    title="Remove"
                  >
                    <X size={13} />
                  </button>
                </div>
              </div>
            ))}
            {history.length === 0 && (
              <div className="pt-8 text-center text-[var(--color-muted)]">
                No songs recognized yet.
              </div>
            )}
          </div>
        </div>
      )}

      <div className="text-center text-[11px] text-[var(--color-muted)]">
        {view === "recognize"
          ? `Records ~${RECORD_SECONDS}s from the mic · R = again · Esc to exit`
          : "History is stored locally · Esc to exit"}
      </div>
    </div>
  );
}
