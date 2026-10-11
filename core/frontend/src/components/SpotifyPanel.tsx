/**
 * `spotify` — preview panel (v0.206.0): what's playing in the Spotify
 * desktop app, the local play history, export and playback control.
 *
 * Shown while the command is typed (reads only); actions (controls, export)
 * run on Enter via `runSignal`. The history is recorded by the Rust backend
 * (`spotify/`) from Spotify's own playback notification — this panel only
 * reads it. Pure logic: `lib/spotify.ts`. Reference: docs/spotify.md.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  AlertTriangle,
  Download,
  Loader2,
  Music,
  Pause,
  Play,
  SkipBack,
  SkipForward,
} from "lucide-react";
import {
  SPOTIFY_DENIED,
  SPOTIFY_NOT_INSTALLED,
  SPOTIFY_NOT_RUNNING,
  getSpotifyConfig,
  spotifyControl,
  spotifyHistory,
  spotifyHistoryExport,
  spotifyNow,
  spotifyOpenAutomationSettings,
  type SpotifyNow,
  type SpotifyPlayRow,
} from "../lib/ipc";
import {
  formatClock,
  formatTotal,
  groupByDay,
  livePosition,
  STATUS_LABEL,
  timeOfDay,
  toQuery,
  type ExportFormat,
  type HistoryFilter,
  type SpotifyCmd,
} from "../lib/spotify";

/** History page size; more loads on demand. */
export const HISTORY_PAGE = 100;
/** How often the "now" view re-reads the player while it is shown. */
const NOW_POLL_MS = 3000;

/** Signals already handled — module-level so a remount never replays one. */
let lastHandledRun = 0;

interface Props {
  cmd: SpotifyCmd;
  focused: boolean;
  /** Bumped by App on Enter: run this command's action (control/export). */
  runSignal: number;
  onExit: () => void;
  /** Replace the search query (e.g. "show all" → `spotify history`). */
  onQuery: (q: string) => void;
}

export function errorText(e: string): string {
  if (e.includes(SPOTIFY_NOT_INSTALLED)) return "Spotify ist auf diesem Mac nicht installiert.";
  if (e.includes(SPOTIFY_NOT_RUNNING)) return "Spotify läuft nicht.";
  if (e.includes(SPOTIFY_DENIED))
    return "Inspector darf Spotify nicht steuern — in den Systemeinstellungen unter Automation erlauben.";
  return e;
}

function filterKey(f: HistoryFilter): string {
  return JSON.stringify([f.range ?? "", f.artist ?? "", f.text ?? ""]);
}

function Cover({ url, show, size }: { url: string | null; show: boolean; size: number }) {
  const [broken, setBroken] = useState(false);
  if (show && url && !broken) {
    return (
      <img
        src={url}
        alt=""
        width={size}
        height={size}
        onError={() => setBroken(true)}
        className="shrink-0 rounded-md object-cover"
        style={{ width: size, height: size }}
      />
    );
  }
  return (
    <div
      className="flex shrink-0 items-center justify-center rounded-md bg-[var(--color-surface)] text-emerald-500"
      style={{ width: size, height: size }}
    >
      <Music size={Math.round(size * 0.45)} />
    </div>
  );
}

function StatusChip({ row }: { row: SpotifyPlayRow }) {
  const tone =
    row.status === "playing"
      ? "bg-emerald-500/15 text-emerald-500"
      : row.status === "listened"
        ? "text-[var(--color-muted)]"
        : row.status === "skipped"
          ? "text-amber-500"
          : "text-[var(--color-muted)]";
  return <span className={`rounded px-1 text-[10px] uppercase tracking-wide ${tone}`}>{STATUS_LABEL[row.status]}</span>;
}

function HistoryRow({ row, artwork }: { row: SpotifyPlayRow; artwork: boolean }) {
  return (
    <div className="flex items-center gap-2 rounded-lg px-2 py-1.5" data-testid="spotify-row">
      <span className="w-10 shrink-0 text-xs tabular-nums text-[var(--color-muted)]">{timeOfDay(row.started_at)}</span>
      {artwork && <Cover url={row.artwork_url} show size={32} />}
      <div className="min-w-0 flex-1">
        <div className="truncate text-[var(--color-fg)]">{row.track || "Unbekannter Titel"}</div>
        <div className="truncate text-xs text-[var(--color-muted)]">
          {[row.artist, row.album].filter(Boolean).join(" · ") || "—"}
        </div>
      </div>
      <div className="shrink-0 text-right">
        <div className="text-xs tabular-nums text-[var(--color-fg)]">
          {formatClock(row.played_ms)}
          {row.duration_ms != null && <span className="text-[var(--color-muted)]"> / {formatClock(row.duration_ms)}</span>}
        </div>
        <StatusChip row={row} />
      </div>
    </div>
  );
}

function HistoryView({
  filter,
  artwork,
  revision,
  scrollRef,
  onExport,
  exportBusy,
}: {
  filter: HistoryFilter;
  artwork: boolean;
  revision: number;
  scrollRef: React.RefObject<HTMLDivElement | null>;
  onExport: (f: ExportFormat) => void;
  exportBusy: boolean;
}) {
  const [rows, setRows] = useState<SpotifyPlayRow[] | null>(null);
  const [total, setTotal] = useState(0);
  const [played, setPlayed] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const key = filterKey(filter);
  const seq = useRef(0);

  // (Re)load the first page when the filter changes — debounced while typing
  // — and when the backend reports a change.
  useEffect(() => {
    const my = ++seq.current;
    const timer = window.setTimeout(() => {
      spotifyHistory(toQuery(filter, new Date(), HISTORY_PAGE, 0))
        .then((p) => {
          if (seq.current !== my) return;
          setRows(p.rows);
          setTotal(p.total);
          setPlayed(p.played_ms);
          setError(null);
        })
        .catch((e) => seq.current === my && setError(String(e)));
    }, 150);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, revision]);

  const loadMore = () => {
    if (!rows || loadingMore) return;
    setLoadingMore(true);
    const my = seq.current;
    spotifyHistory(toQuery(filter, new Date(), HISTORY_PAGE, rows.length))
      .then((p) => {
        if (seq.current === my) setRows((cur) => [...(cur ?? []), ...p.rows]);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoadingMore(false));
  };

  const groups = useMemo(() => (rows ? groupByDay(rows, new Date()) : []), [rows]);
  const filtered = !!(filter.range || filter.artist || filter.text);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2">
      <div className="flex items-center gap-2 text-xs text-[var(--color-muted)]">
        <span data-testid="spotify-summary">
          {rows == null ? "…" : `${total} ${total === 1 ? "Eintrag" : "Einträge"} · ${formatTotal(played)} gehört`}
        </span>
        <span className="ml-auto flex items-center gap-1">
          {exportBusy ? (
            <Loader2 size={12} className="animate-spin" />
          ) : (
            <Download size={12} />
          )}
          {(["json", "csv", "md"] as const).map((f) => (
            <button
              key={f}
              type="button"
              disabled={exportBusy || total === 0}
              onClick={() => onExport(f)}
              className="rounded px-1.5 py-0.5 uppercase hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)] disabled:opacity-40"
            >
              {f}
            </button>
          ))}
        </span>
      </div>
      {error && <div className="text-xs text-amber-500">{errorText(error)}</div>}
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto pr-1">
        {rows != null && rows.length === 0 && (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-center text-sm text-[var(--color-muted)]">
            <Music size={24} />
            <div>{filtered ? "Keine Treffer." : "Noch nichts gehört — Spotify-Titel werden ab jetzt mitgeschrieben."}</div>
          </div>
        )}
        {groups.map((g) => (
          <div key={g.key} className="mb-2">
            <div className="sticky top-0 z-[1] bg-[var(--color-bg)] px-2 py-1 text-xs font-semibold text-[var(--color-fg)]">
              {g.label}
            </div>
            {g.rows.map((r) => (
              <HistoryRow key={r.id} row={r} artwork={artwork} />
            ))}
          </div>
        ))}
        {rows != null && rows.length < total && (
          <button
            type="button"
            onClick={loadMore}
            className="mx-auto my-2 block rounded-md px-3 py-1 text-xs text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]"
          >
            {loadingMore ? "Lädt…" : `Mehr laden (${total - rows.length} weitere)`}
          </button>
        )}
      </div>
    </div>
  );
}

function NowView({
  now,
  recent,
  onControl,
  busy,
  onShowHistory,
}: {
  now: SpotifyNow | null;
  recent: SpotifyPlayRow[];
  onControl: (a: "previous" | "toggle" | "next") => void;
  busy: boolean;
  onShowHistory: () => void;
}) {
  const [tick, setTick] = useState(() => Date.now());
  const playing = now?.state === "playing";
  useEffect(() => {
    if (!playing) return;
    const id = window.setInterval(() => setTick(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [playing]);

  if (!now) {
    return (
      <div className="flex flex-1 items-center justify-center text-[var(--color-muted)]">
        <Loader2 size={20} className="animate-spin" />
      </div>
    );
  }
  if (!now.installed) {
    return <Empty text="Spotify ist auf diesem Mac nicht installiert." />;
  }
  const t = now.track;
  const pos = t ? livePosition(now.position_ms, now.at_ms, playing, tick, t.duration_ms) : null;
  const share = t?.duration_ms && pos != null ? pos / t.duration_ms : 0;

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      {!now.running ? (
        <Empty text="Spotify läuft nicht. Sobald es spielt, erscheint der Titel hier und landet im Verlauf." />
      ) : !t ? (
        <Empty text="Gerade läuft nichts." />
      ) : (
        <div className="flex gap-3" data-testid="spotify-now">
          <Cover url={t.artwork_url} show={now.artwork_enabled} size={96} />
          <div className="flex min-w-0 flex-1 flex-col justify-center gap-0.5">
            <div className="text-[10px] uppercase tracking-wide text-emerald-500">
              {playing ? "Läuft" : now.state === "paused" ? "Pausiert" : "Gestoppt"}
            </div>
            <div className="truncate text-base font-semibold text-[var(--color-fg)]">{t.name || "Unbekannter Titel"}</div>
            <div className="truncate text-sm text-[var(--color-muted)]">{t.artist}</div>
            <div className="truncate text-xs text-[var(--color-muted)]">{t.album}</div>
            {pos != null && t.duration_ms != null && (
              <div className="mt-1 space-y-0.5">
                <div className="h-1 overflow-hidden rounded-full bg-[var(--color-border)]">
                  <div
                    className="h-full w-full origin-left rounded-full bg-emerald-500"
                    style={{ transform: `scaleX(${Math.min(1, Math.max(0, share))})` }}
                  />
                </div>
                <div className="flex justify-between text-[10px] tabular-nums text-[var(--color-muted)]">
                  <span>{formatClock(pos)}</span>
                  <span>{formatClock(t.duration_ms)}</span>
                </div>
              </div>
            )}
          </div>
        </div>
      )}

      {now.running && t && (
        <div className="flex items-center justify-center gap-2">
          {(["previous", "toggle", "next"] as const).map((a) => (
            <button
              key={a}
              type="button"
              disabled={busy}
              onClick={() => onControl(a)}
              aria-label={{ previous: "Vorheriger Titel", toggle: playing ? "Pause" : "Abspielen", next: "Nächster Titel" }[a]}
              className="rounded-full p-2 text-[var(--color-fg)] hover:bg-[var(--color-surface)] disabled:opacity-40"
            >
              {a === "previous" ? <SkipBack size={18} /> : a === "next" ? <SkipForward size={18} /> : playing ? <Pause size={20} /> : <Play size={20} />}
            </button>
          ))}
          {now.volume != null && (
            <span className="ml-2 text-xs tabular-nums text-[var(--color-muted)]">Vol {now.volume}</span>
          )}
        </div>
      )}

      {now.running && now.permission === "undetermined" && now.source === "recorder" && (
        <div className="text-center text-xs text-[var(--color-muted)]">
          Enter erlaubt Inspector, Spotify abzufragen und zu steuern (macOS fragt einmal nach).
        </div>
      )}
      {now.permission === "denied" && (
        <div className="flex items-center justify-center gap-2 text-xs text-amber-500">
          <AlertTriangle size={12} />
          <span>Steuerung nicht erlaubt.</span>
          <button type="button" className="underline" onClick={() => void spotifyOpenAutomationSettings()}>
            Systemeinstellungen öffnen
          </button>
        </div>
      )}
      {!now.history_enabled && (
        <div className="text-center text-xs text-[var(--color-muted)]">
          Der Verlauf wird nicht aufgezeichnet (Einstellungen → Spotify).
        </div>
      )}

      {recent.length > 0 && (
        <div className="min-h-0 flex-1 overflow-hidden">
          <div className="flex items-center px-2 pb-1 text-xs font-semibold text-[var(--color-fg)]">
            Zuletzt gehört
            <button
              type="button"
              onClick={onShowHistory}
              className="ml-auto font-normal text-[var(--color-muted)] hover:text-[var(--color-fg)]"
            >
              Alle anzeigen
            </button>
          </div>
          {recent.map((r) => (
            <HistoryRow key={r.id} row={r} artwork={now.artwork_enabled} />
          ))}
        </div>
      )}
    </div>
  );
}

function Empty({ text }: { text: string }) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 py-6 text-center text-sm text-[var(--color-muted)]">
      <Music size={24} />
      <div>{text}</div>
    </div>
  );
}

export function SpotifyPanel({ cmd, focused, runSignal, onExit, onQuery }: Props) {
  const view = cmd.kind === "history" ? "history" : "now";
  const [now, setNow] = useState<SpotifyNow | null>(null);
  const [recent, setRecent] = useState<SpotifyPlayRow[]>([]);
  const [revision, setRevision] = useState(0);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const [artwork, setArtwork] = useState(false);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const focusedRef = useRef(focused);
  useEffect(() => {
    void getSpotifyConfig()
      .then((c) => setArtwork(c.artwork))
      .catch(() => undefined);
  }, []);
  useEffect(() => {
    focusedRef.current = focused;
  }, [focused]);

  const refreshNow = useCallback((ask: boolean) => {
    spotifyNow(ask)
      .then(setNow)
      .catch(() => undefined);
  }, []);

  // Now: read at once (asking only once the user pressed Enter), then poll
  // gently while shown; any recorder change refreshes now + history.
  useEffect(() => {
    if (view !== "now") return;
    refreshNow(focused);
    const id = window.setInterval(() => refreshNow(false), NOW_POLL_MS);
    return () => window.clearInterval(id);
  }, [view, focused, refreshNow]);

  useEffect(() => {
    if (view !== "now") return;
    spotifyHistory({ limit: 5 })
      .then((p) => setRecent(p.rows.filter((r) => !r.open)))
      .catch(() => undefined);
  }, [view, revision]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listen("spotify-changed", () => {
      setRevision((r) => r + 1);
      if (view === "now") refreshNow(false);
    }).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [view, refreshNow]);

  const control = useCallback(
    async (action: Parameters<typeof spotifyControl>[0], value?: number) => {
      setBusy(true);
      try {
        await spotifyControl(action, value);
        setMessage(null);
      } catch (e) {
        setMessage({ ok: false, text: errorText(String(e)) });
      } finally {
        setBusy(false);
        // Spotify needs a moment to apply it.
        window.setTimeout(() => refreshNow(focusedRef.current), 350);
      }
    },
    [refreshNow],
  );

  const doExport = useCallback(
    async (format: ExportFormat) => {
      setBusy(true);
      try {
        const filter = cmd.kind === "history" ? cmd.filter : {};
        const path = await spotifyHistoryExport(format, toQuery(filter, new Date()));
        setMessage({ ok: true, text: `Exportiert: ${path.split("/").pop()}` });
      } catch (e) {
        setMessage({ ok: false, text: errorText(String(e)) });
      } finally {
        setBusy(false);
      }
    },
    [cmd],
  );

  // Enter (from App): run this command's action — once per signal.
  useEffect(() => {
    if (runSignal <= lastHandledRun) return;
    lastHandledRun = runSignal;
    if (cmd.kind === "control") void control(cmd.action, cmd.value);
    else if (cmd.kind === "history" && cmd.export) void doExport(cmd.export);
  }, [runSignal, cmd, control, doExport]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onExit();
        return;
      }
      const target = e.target as HTMLElement | null;
      if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA")) return;
      if (view === "history" && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
        e.preventDefault();
        scrollRef.current?.scrollBy({ top: e.key === "ArrowDown" ? 80 : -80 });
      } else if (view === "now" && !e.repeat && (e.key === " " || e.key === "ArrowLeft" || e.key === "ArrowRight")) {
        e.preventDefault();
        void control(e.key === " " ? "toggle" : e.key === "ArrowLeft" ? "previous" : "next");
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, view, onExit, control]);

  return (
    <div className="flex h-full flex-col gap-2 overflow-hidden p-3 text-sm" data-testid="spotify-panel">
      <div className="flex items-center gap-2 text-[var(--color-fg)]">
        <Music size={16} className="text-emerald-500" />
        <span className="font-semibold">{view === "history" ? "Spotify-Verlauf" : "Spotify"}</span>
        {view === "history" && cmd.kind === "history" && (cmd.filter.range || cmd.filter.artist || cmd.filter.text) && (
          <span className="truncate text-xs text-[var(--color-muted)]">
            {[
              cmd.filter.range && { today: "heute", yesterday: "gestern", week: "7 Tage", month: "30 Tage" }[cmd.filter.range],
              cmd.filter.artist && `von ${cmd.filter.artist}`,
              cmd.filter.text && `„${cmd.filter.text}“`,
            ]
              .filter(Boolean)
              .join(" · ")}
          </span>
        )}
      </div>

      {cmd.kind === "invalid" && (
        <div className="flex items-start gap-2 rounded-lg border border-amber-500/40 bg-amber-500/10 p-2 text-xs text-[var(--color-fg)]">
          <AlertTriangle size={14} className="mt-0.5 shrink-0 text-amber-400" />
          <span>{cmd.message}</span>
        </div>
      )}
      {cmd.kind === "control" && !focused && (
        <div className="text-xs text-[var(--color-muted)]">Enter führt den Befehl aus.</div>
      )}
      {message && (
        <div
          className={`rounded-lg px-2 py-1 text-xs ${message.ok ? "text-emerald-500" : "text-amber-500"}`}
          role="status"
          data-testid="spotify-message"
        >
          {message.text}
        </div>
      )}

      {view === "history" && cmd.kind === "history" ? (
        <HistoryView
          filter={cmd.filter}
          artwork={artwork}
          revision={revision}
          scrollRef={scrollRef}
          onExport={(f) => void doExport(f)}
          exportBusy={busy}
        />
      ) : (
        <NowView
          now={now}
          recent={recent}
          busy={busy}
          onControl={(a) => void control(a)}
          onShowHistory={() => onQuery("spotify history")}
        />
      )}
    </div>
  );
}
