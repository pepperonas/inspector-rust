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
  Copy,
  RefreshCw,
  MicOff,
  History,
  Trash2,
  X,
  ScrollText,
  Search,
  ArrowLeft,
  Languages,
} from "lucide-react";
import {
  shazamListen,
  shazamIsListening,
  openSpotify,
  shazamHistoryList,
  shazamHistoryClear,
  shazamHistoryDelete,
  shazamLyrics,
  shazamLyricsTranslated,
  type ShazamMatch,
  type ShazamDone,
  type ShazamHistoryEntry,
  type TranslatedLyrics,
} from "../lib/ipc";
import { filterShazamHistory } from "../lib/shazam-filter";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";

type Phase = "listening" | "searching" | "result" | "nomatch" | "error" | "noperm";
const RECORD_SECONDS = 10;

const SPOTIFY_GREEN = "#1DB954";
const YT_RED = "#FF0000";
const SHAZAM_BLUE = "#0088FF";

/* Brand glyphs (Simple Icons paths, CC0) — lucide carries no brand icons, and
   the buttons should read as "open in <platform>" at a glance. */
function SpotifyIcon({ size = 16 }: { size?: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} fill="currentColor" aria-hidden>
      <path d="M12 0C5.4 0 0 5.4 0 12s5.4 12 12 12 12-5.4 12-12S18.66 0 12 0zm5.521 17.34c-.24.359-.66.48-1.021.24-2.82-1.74-6.36-2.101-10.561-1.141-.418.122-.779-.179-.899-.539-.12-.421.18-.78.54-.9 4.56-1.021 8.52-.6 11.64 1.32.42.18.479.659.301 1.02zm1.44-3.3c-.301.42-.841.6-1.262.3-3.239-1.98-8.159-2.58-11.939-1.38-.479.12-1.02-.12-1.14-.6-.12-.48.12-1.021.6-1.141C9.6 9.9 15 10.561 18.72 12.84c.361.181.54.78.241 1.2zm.12-3.36C15.24 8.4 8.82 8.16 5.16 9.301c-.6.179-1.2-.181-1.38-.721-.18-.601.18-1.2.72-1.381 4.26-1.26 11.28-1.02 15.721 1.621.539.3.719 1.02.419 1.56-.299.421-1.02.599-1.559.3z" />
    </svg>
  );
}
function YoutubeIcon({ size = 16 }: { size?: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} fill="currentColor" aria-hidden>
      <path d="M23.498 6.186a3.016 3.016 0 0 0-2.122-2.136C19.505 3.545 12 3.545 12 3.545s-7.505 0-9.377.505A3.017 3.017 0 0 0 .502 6.186C0 8.07 0 12 0 12s0 3.93.502 5.814a3.016 3.016 0 0 0 2.122 2.136c1.871.505 9.376.505 9.376.505s7.505 0 9.377-.505a3.015 3.015 0 0 0 2.122-2.136C24 15.93 24 12 24 12s0-3.93-.502-5.814zM9.545 15.568V8.432L15.818 12l-6.273 3.568z" />
    </svg>
  );
}
function ShazamIcon({ size = 16 }: { size?: number }) {
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} fill="currentColor" aria-hidden>
      <path d="M12 0C5.373 0 0 5.373 0 12s5.373 12 12 12 12-5.373 12-12S18.627 0 12 0zm4.06 16.135c-1.077 1.077-2.507 1.67-4.03 1.67h-.003c-1.523 0-2.952-.593-4.028-1.669l-2.32-2.32a1.144 1.144 0 0 1 1.617-1.617l2.32 2.32c.53.53 1.234.821 1.983.821.75 0 1.455-.292 1.985-.822.53-.53.821-1.234.821-1.983 0-.75-.292-1.454-.821-1.984l-1.52-1.52a1.144 1.144 0 0 1 1.617-1.617l1.52 1.52c1.077 1.076 1.67 2.506 1.67 4.029 0 1.523-.593 2.953-1.67 4.029zm.585-6.653a1.14 1.14 0 0 1-1.617 0l-2.32-2.32a2.788 2.788 0 0 0-1.983-.821c-.75 0-1.455.292-1.985.822-.53.53-.821 1.234-.821 1.983 0 .75.292 1.454.821 1.984l1.52 1.52a1.144 1.144 0 0 1-1.617 1.617l-1.52-1.52c-1.077-1.076-1.67-2.506-1.67-4.029 0-1.523.593-2.953 1.67-4.029C8.2 3.612 9.63 3.019 11.152 3.019c1.523 0 2.953.593 4.029 1.669l2.32 2.32c.447.447.447 1.171 0 1.617z" />
    </svg>
  );
}

/** Round brand icon-button (result card): white glyph on the brand colour. */
function BrandButton({
  title,
  color,
  onClick,
  children,
}: {
  title: string;
  color: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      aria-label={title}
      className="md3-press flex h-9 w-9 items-center justify-center rounded-full text-white shadow-sm hover:opacity-90"
      style={{ backgroundColor: color }}
    >
      {children}
    </button>
  );
}

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

/** Small round icon link button (history rows). */
function LinkChip({
  title,
  color,
  onClick,
  children,
}: {
  title: string;
  color?: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      aria-label={title}
      className="flex h-6 w-6 items-center justify-center rounded-full border border-[var(--color-border)] hover:bg-[var(--color-surface)]"
      style={color ? { color } : undefined}
    >
      {children}
    </button>
  );
}

export function ShazamPanel({
  focused,
  initialView = "recognize",
  viewSignal = 0,
  onExit,
}: {
  focused: boolean;
  /** `history` opens straight into the history list (no mic). Default: recognize. */
  initialView?: "recognize" | "history";
  /** Bumped by App.tsx on every shazam dispatch (a click on the "Identify
   *  song" / "shazam history" list row). The panel stays mounted across
   *  those clicks, so `initialView` alone can't re-apply the choice —
   *  this signal makes every click land, even one matching the current
   *  mode after the internal Listen⇄History toggle diverged from it. */
  viewSignal?: number;
  onExit: () => void;
}) {
  const [view, setView] = useState<"recognize" | "history" | "lyrics">(initialView);
  /** Where the lyrics view returns to on ←. */
  const [lyricsBack, setLyricsBack] = useState<"recognize" | "history">("recognize");
  const [lyricsMeta, setLyricsMeta] = useState<{ title: string; artist: string; cover: string } | null>(null);
  const [lyrics, setLyrics] = useState<{ status: "idle" | "loading" | "ok" | "none" | "error"; text: string }>({ status: "idle", text: "" });
  /** Which lyrics variant the view shows: original vs. bilingual (+DE). */
  const [lyricsMode, setLyricsMode] = useState<"orig" | "bi">("orig");
  /** German translation of the current track's lyrics (lazy — loaded on demand). */
  const [translated, setTranslated] = useState<{
    status: "idle" | "loading" | "ok" | "none" | "error";
    data: TranslatedLyrics | null;
    error: string;
  }>({ status: "idle", data: null, error: "" });
  /** History search query (title · artist · album · genre). */
  const [histQuery, setHistQuery] = useState("");
  const [phase, setPhase] = useState<Phase>("listening");
  const [progress, setProgress] = useState(0);
  const [match, setMatch] = useState<ShazamMatch | null>(null);
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const [history, setHistory] = useState<ShazamHistoryEntry[]>([]);
  const filteredHistory = filterShazamHistory(history, histQuery);
  const runIdRef = useRef(0);
  /** True while a native recording/recognition is in flight — guards the
   *  list-row click path from starting a SECOND concurrent `shazam_listen`
   *  (cpal would open the input device twice). */
  const busyRef = useRef(false);
  /** True when the panel mounted INTO an already-running background recognition
   *  (reconnect): it shows the live state and waits for the `shazam-done`
   *  event instead of starting its own recording. Our own `run()` sets its
   *  result directly, so it leaves this false and ignores the event. */
  const reconnectedRef = useRef(false);

  const loadHistory = useCallback(async () => {
    try {
      setHistory(await shazamHistoryList(500));
    } catch {
      /* ignore */
    }
  }, []);

  /** Apply a finished recognition (from the `shazam-done` event) to the view. */
  const applyDone = useCallback(
    (d: ShazamDone) => {
      if (d.matched) {
        setMatch(d.matched);
        setPhase("result");
        void loadHistory();
      } else if (d.error) {
        if (/permission|denied|NotAllowed|microphone|no audio/i.test(d.error)) setPhase("noperm");
        else {
          setError(d.error);
          setPhase("error");
        }
      } else {
        setPhase("nomatch");
      }
    },
    [loadHistory],
  );

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
    busyRef.current = true;
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
      busyRef.current = false;
      unlisten();
    }
  }, [loadHistory]);

  useEffect(() => {
    void loadHistory();
    // `shazam history` opens the list without touching the mic. `shazam`
    // starts listening — UNLESS a recognition is already running in the
    // backend (the overlay was closed mid-listen and reopened): then reconnect
    // to it (show live state, result arrives via `shazam-done`) instead of
    // opening the mic a second time.
    if (initialView !== "history") {
      void shazamIsListening().then((busy) => {
        if (busy) {
          reconnectedRef.current = true;
          setPhase("searching");
        } else {
          void run();
        }
      });
    }
    return () => {
      runIdRef.current += 1; // invalidate in-flight run on unmount
    };
    // Run once on mount for the chosen initial view.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Persistent `shazam-done` listener: applies the result of a reconnected
  // background run (our own `run()` sets its result directly, so it clears
  // reconnectedRef and this is a no-op for that path).
  useEffect(() => {
    let cancelled = false;
    let un: (() => void) | null = null;
    void listen<ShazamDone>("shazam-done", (e) => {
      if (!reconnectedRef.current) return;
      reconnectedRef.current = false;
      applyDone(e.payload);
    }).then((u) => {
      if (cancelled) u();
      else un = u;
    });
    return () => {
      cancelled = true;
      if (un) un();
    };
  }, [applyDone]);

  // Every later click on the "Identify song" / "shazam history" list row
  // bumps `viewSignal` (App.tsx) — apply it to the ALREADY-MOUNTED panel.
  // Without this, `initialView` was consumed once at mount and the other
  // row's click silently did nothing (field report). The ref skips the
  // mount-time value so the mount effect above stays the only initial run.
  const lastSignalRef = useRef(viewSignal);
  useEffect(() => {
    if (viewSignal === lastSignalRef.current) return;
    lastSignalRef.current = viewSignal;
    if (initialView === "history") {
      // Switch to the list; an in-flight recognition keeps running in the
      // background (view and run are orthogonal — returning to recognize
      // shows its live state, and a match still lands in the history).
      setView("history");
      void loadHistory();
    } else if (busyRef.current || reconnectedRef.current) {
      // Already listening/recognizing (our own run, or a reconnected background
      // run) — just bring the recognize view back, never start a SECOND
      // concurrent native recording.
      setView("recognize");
    } else {
      void run();
    }
  }, [viewSignal, initialView, run, loadHistory]);

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

  /** Fetch the plain (original) lyrics for a track. */
  const loadPlain = useCallback((artist: string, title: string) => {
    setLyrics({ status: "loading", text: "" });
    shazamLyrics(artist, title)
      .then((text) => setLyrics(text ? { status: "ok", text } : { status: "none", text: "" }))
      .catch((e) => setLyrics({ status: "error", text: String(e) }));
  }, []);

  /** Fetch the German translation (lrclib → keyless translate). */
  const loadTranslation = useCallback((artist: string, title: string) => {
    setTranslated({ status: "loading", data: null, error: "" });
    shazamLyricsTranslated(artist, title)
      .then((d) =>
        setTranslated(
          d ? { status: "ok", data: d, error: "" } : { status: "none", data: null, error: "" },
        ),
      )
      .catch((e) => setTranslated({ status: "error", data: null, error: String(e) }));
  }, []);

  /** Open the in-app lyrics view for a track. `mode` picks original vs. the
   *  bilingual (+DE) variant; both are lazy-loaded and cached until re-opened. */
  const openLyrics = useCallback(
    (
      title: string,
      artist: string,
      cover: string,
      from: "recognize" | "history",
      mode: "orig" | "bi" = "orig",
    ) => {
      setLyricsBack(from);
      setLyricsMeta({ title, artist, cover });
      setLyricsMode(mode);
      setLyrics({ status: "idle", text: "" });
      setTranslated({ status: "idle", data: null, error: "" });
      setView("lyrics");
      if (mode === "bi") loadTranslation(artist, title);
      else loadPlain(artist, title);
    },
    [loadPlain, loadTranslation],
  );

  /** Toggle Original ⇄ +DE inside the lyrics view; lazy-loads the variant the
   *  first time it's shown (no re-recognition). */
  const switchLyricsMode = useCallback(
    (mode: "orig" | "bi") => {
      setLyricsMode(mode);
      if (!lyricsMeta) return;
      if (mode === "bi" && translated.status === "idle")
        loadTranslation(lyricsMeta.artist, lyricsMeta.title);
      if (mode === "orig" && lyrics.status === "idle")
        loadPlain(lyricsMeta.artist, lyricsMeta.title);
    },
    [lyricsMeta, translated.status, lyrics.status, loadTranslation, loadPlain],
  );

  /** Browser fallback when the lyrics catalogue has no entry. */
  const searchLyricsInBrowser = useCallback(() => {
    if (!lyricsMeta) return;
    const q = encodeURIComponent(`songtext ${lyricsMeta.artist} ${lyricsMeta.title}`);
    void openUrl(`https://www.google.com/search?q=${q}`);
  }, [lyricsMeta]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        // Esc from the lyrics view goes BACK first; a second Esc exits.
        if (view === "lyrics") setView(lyricsBack);
        else onExit();
      } else if (
        (e.key === "l" || e.key === "L") &&
        view === "recognize" &&
        phase === "result" &&
        match
      ) {
        openLyrics(match.title, match.artist, match.cover_url, "recognize");
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
  }, [focused, phase, view, match, lyricsBack, copyTitle, run, onExit, openLyrics]);

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
          <div className="flex flex-wrap items-center gap-2">
            {match.shazam_url && (
              <BrandButton title="Open in Shazam" color={SHAZAM_BLUE} onClick={() => void openUrl(match.shazam_url)}>
                <ShazamIcon size={18} />
              </BrandButton>
            )}
            <BrandButton
              title="Open in Spotify (app if installed, else web)"
              color={SPOTIFY_GREEN}
              onClick={() => void openSpotify(match.spotify_url, match.title, match.artist)}
            >
              <SpotifyIcon size={18} />
            </BrandButton>
            <BrandButton title="Open on YouTube" color={YT_RED} onClick={() => void openUrl(match.youtube_url)}>
              <YoutubeIcon size={18} />
            </BrandButton>
            <button
              type="button"
              onClick={() => openLyrics(match.title, match.artist, match.cover_url, "recognize")}
              className="flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-xs font-semibold text-[var(--color-fg)] hover:bg-[var(--color-surface)]"
              title="Show the lyrics (L)"
            >
              <ScrollText size={13} /> Lyrics
            </button>
            <button
              type="button"
              onClick={() => openLyrics(match.title, match.artist, match.cover_url, "recognize", "bi")}
              className="flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-xs font-semibold text-[var(--color-fg)] hover:bg-[var(--color-surface)]"
              title="Lyrics with a German translation under each line"
            >
              <Languages size={13} /> Lyrics DE
            </button>
            <button
              type="button"
              onClick={() => void copyTitle()}
              className="flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-xs font-semibold text-[var(--color-fg)] hover:bg-[var(--color-surface)]"
            >
              <Copy size={13} /> {copied ? <span className="md3-success-pop">Copied!</span> : "Copy"}
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

      {/* ── Lyrics view ── */}
      {view === "lyrics" && lyricsMeta && (
        <div className="flex min-h-0 flex-1 flex-col gap-2">
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => setView(lyricsBack)}
              className="flex items-center gap-1 rounded-lg border border-[var(--color-border)] px-2 py-1 text-xs text-[var(--color-fg)] hover:bg-[var(--color-surface)]"
              title="Back (Esc)"
            >
              <ArrowLeft size={12} /> Back
            </button>
            {lyricsMeta.cover ? (
              <img src={lyricsMeta.cover} alt="" className="h-8 w-8 shrink-0 rounded object-cover" />
            ) : (
              <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded bg-[var(--color-surface)]">
                <Music size={13} className="text-[var(--color-muted)]" />
              </div>
            )}
            <div className="min-w-0">
              <div className="truncate text-xs font-semibold text-[var(--color-fg)]">{lyricsMeta.title}</div>
              <div className="truncate text-[11px] text-[var(--color-muted)]">{lyricsMeta.artist}</div>
            </div>
            {/* Original ⇄ +DE toggle */}
            <div className="ml-auto flex shrink-0 items-center rounded-lg border border-[var(--color-border)] p-0.5 text-[11px]">
              <button
                type="button"
                onClick={() => switchLyricsMode("orig")}
                className={`rounded-md px-2 py-0.5 font-semibold ${lyricsMode === "orig" ? "bg-[var(--color-surface)] text-[var(--color-fg)]" : "text-[var(--color-muted)] hover:text-[var(--color-fg)]"}`}
                title="Original lyrics"
              >
                Original
              </button>
              <button
                type="button"
                onClick={() => switchLyricsMode("bi")}
                className={`flex items-center gap-1 rounded-md px-2 py-0.5 font-semibold ${lyricsMode === "bi" ? "bg-[var(--color-surface)] text-[var(--color-fg)]" : "text-[var(--color-muted)] hover:text-[var(--color-fg)]"}`}
                title="German translation under each line"
              >
                <Languages size={11} /> +DE
              </button>
            </div>
          </div>

          {/* ── Original lyrics ── */}
          {lyricsMode === "orig" && lyrics.status === "loading" && (
            <div className="flex flex-1 items-center justify-center gap-2 text-xs text-[var(--color-muted)]">
              <Loader2 size={14} className="animate-spin" /> Loading lyrics…
            </div>
          )}
          {lyricsMode === "orig" && lyrics.status === "ok" && (
            <div className="min-h-0 flex-1 overflow-y-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
              <pre className="whitespace-pre-wrap font-sans text-[12.5px] leading-relaxed text-[var(--color-fg)]">
                {lyrics.text}
              </pre>
              <div className="mt-3 text-[10px] text-[var(--color-muted)]">Lyrics via lrclib.net</div>
            </div>
          )}
          {lyricsMode === "orig" && (lyrics.status === "none" || lyrics.status === "error") && (
            <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
              <ScrollText size={22} className="text-[var(--color-muted)]" />
              <div className="text-xs text-[var(--color-muted)]">
                {lyrics.status === "none"
                  ? "No lyrics in the catalogue for this track."
                  : lyrics.text}
              </div>
              <button
                type="button"
                onClick={searchLyricsInBrowser}
                className="flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-xs font-semibold text-[var(--color-fg)] hover:bg-[var(--color-surface)]"
              >
                <Search size={12} /> Search in the browser
              </button>
            </div>
          )}

          {/* ── Bilingual lyrics (+DE) ── */}
          {lyricsMode === "bi" && translated.status === "loading" && (
            <div className="flex flex-1 items-center justify-center gap-2 text-xs text-[var(--color-muted)]">
              <Loader2 size={14} className="animate-spin" /> Übersetze Songtext…
            </div>
          )}
          {lyricsMode === "bi" && translated.status === "ok" && translated.data && (
            <div className="min-h-0 flex-1 overflow-y-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
              {translated.data.src_lang === "de" ? (
                <>
                  <div className="mb-2 text-[11px] text-[var(--color-muted)]">
                    Dieser Song ist bereits auf Deutsch.
                  </div>
                  <pre className="whitespace-pre-wrap font-sans text-[12.5px] leading-relaxed text-[var(--color-fg)]">
                    {translated.data.segments.map((s) => s.orig).join("\n")}
                  </pre>
                </>
              ) : (
                <div className="space-y-2">
                  {translated.data.segments.map((s, i) => (
                    <div key={i}>
                      <div className="whitespace-pre-wrap text-[12.5px] leading-snug text-[var(--color-fg)]">
                        {s.orig}
                      </div>
                      <div className="whitespace-pre-wrap text-[11.5px] italic leading-snug text-[var(--color-muted)]">
                        {s.trans}
                      </div>
                    </div>
                  ))}
                </div>
              )}
              <div className="mt-3 text-[10px] text-[var(--color-muted)]">
                Lyrics via lrclib.net · Übersetzung via Google Translate
              </div>
            </div>
          )}
          {lyricsMode === "bi" && (translated.status === "none" || translated.status === "error") && (
            <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
              <ScrollText size={22} className="text-[var(--color-muted)]" />
              <div className="text-xs text-[var(--color-muted)]">
                {translated.status === "none"
                  ? "No lyrics in the catalogue for this track."
                  : translated.error}
              </div>
              <button
                type="button"
                onClick={searchLyricsInBrowser}
                className="flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-xs font-semibold text-[var(--color-fg)] hover:bg-[var(--color-surface)]"
              >
                <Search size={12} /> Search in the browser
              </button>
            </div>
          )}
        </div>
      )}

      {/* ── History view ── */}
      {view === "history" && (
        <div className="flex min-h-0 flex-1 flex-col gap-2">
          {history.length > 0 && (
            <div className="flex items-center gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1">
              <Search size={13} className="shrink-0 text-[var(--color-muted)]" />
              <input
                type="text"
                value={histQuery}
                onChange={(e) => setHistQuery(e.target.value)}
                placeholder="Search title · artist · album · genre…"
                className="w-full bg-transparent text-xs text-[var(--color-fg)] outline-none placeholder:text-[var(--color-muted)]"
              />
              {histQuery !== "" && (
                <button
                  type="button"
                  onClick={() => setHistQuery("")}
                  className="shrink-0 rounded p-0.5 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                  title="Clear search"
                >
                  <X size={12} />
                </button>
              )}
            </div>
          )}
          {history.length > 0 && (
            <div className="flex items-center justify-between text-xs text-[var(--color-muted)]">
              <span>
                {histQuery
                  ? `${filteredHistory.length} of ${history.length}`
                  : `${history.length} recognized`}
              </span>
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
            {filteredHistory.map((e) => (
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
                  {e.shazam_url && (
                    <LinkChip title="Open in Shazam" color={SHAZAM_BLUE} onClick={() => void openUrl(e.shazam_url)}>
                      <ShazamIcon size={13} />
                    </LinkChip>
                  )}
                  <LinkChip
                    title="Open in Spotify"
                    color={SPOTIFY_GREEN}
                    onClick={() => void openSpotify(e.spotify_url, e.title, e.artist)}
                  >
                    <SpotifyIcon size={13} />
                  </LinkChip>
                  <LinkChip title="Open on YouTube" color={YT_RED} onClick={() => void openUrl(e.youtube_url)}>
                    <YoutubeIcon size={13} />
                  </LinkChip>
                  <LinkChip title="Show lyrics" onClick={() => openLyrics(e.title, e.artist, e.cover_url, "history")}>
                    <ScrollText size={12} />
                  </LinkChip>
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
              <div className="md3-empty-float pt-8 text-center text-[var(--color-muted)]">
                No songs recognized yet.
              </div>
            )}
            {history.length > 0 && filteredHistory.length === 0 && (
              <div className="pt-8 text-center text-[var(--color-muted)]">
                No match for “{histQuery}”.
              </div>
            )}
          </div>
        </div>
      )}

      <div className="text-center text-[11px] text-[var(--color-muted)]">
        {view === "recognize"
          ? `Records ~${RECORD_SECONDS}s from the mic · R = again · L = lyrics · Esc to exit`
          : view === "lyrics"
            ? "Esc = back"
            : "History is stored locally · Esc to exit"}
      </div>
    </div>
  );
}
