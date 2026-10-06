import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ArrowDown, ArrowUp, Gauge, Loader2, RefreshCw, Timer, Trash2 } from "lucide-react";
import {
  speedtestClearHistory,
  speedtestHistory,
  speedtestRun,
  speedtestRunning,
  type SpeedtestHistoryEntry,
  type SpeedtestProgress,
  type SpeedtestResult,
} from "../lib/ipc";
import {
  PHASE_LABELS,
  barShare,
  deltaPercent,
  downloadMeaning,
  formatMbps,
  formatMs,
  overallProgress,
  rateDownload,
} from "../lib/speedtest";

type Status = "running" | "done" | "error";

const HISTORY_SHOWN = 8;

/**
 * `speedtest` — internet speed in the preview (v0.199.0). Mounting the panel
 * (Enter on the command row) starts a run; the run lives in the backend, so
 * closing the panel doesn't abort it and a reopened panel reconnects.
 */
export function SpeedtestPanel({ focused, onExit }: { focused: boolean; onExit: () => void }) {
  const [status, setStatus] = useState<Status>("running");
  const [progress, setProgress] = useState<SpeedtestProgress | null>(null);
  const [result, setResult] = useState<SpeedtestResult | null>(null);
  const [error, setError] = useState("");
  const [history, setHistory] = useState<SpeedtestHistoryEntry[]>([]);
  const startedRef = useRef(false);

  const loadHistory = useCallback(async () => {
    try {
      setHistory(await speedtestHistory(30));
    } catch {
      /* history is a bonus — never block the measurement on it */
    }
  }, []);

  const run = useCallback(async () => {
    setStatus("running");
    setProgress(null);
    setError("");
    try {
      const r = await speedtestRun();
      setResult(r);
      setStatus("done");
      void loadHistory();
    } catch (e) {
      const msg = String(e).replace(/^.*Error:\s*/, "");
      // Another run is already going (reopened panel) — wait for its result.
      if (msg === "speedtest.busy") return;
      setError(msg);
      setStatus("error");
    }
  }, [loadHistory]);

  // Progress + the end of a run we didn't start ourselves (reconnect).
  useEffect(() => {
    let alive = true;
    const unP = listen<SpeedtestProgress>("speedtest-progress", (e) => {
      if (alive) setProgress(e.payload);
    });
    const unD = listen<string | null>("speedtest-done", (e) => {
      if (!alive) return;
      void loadHistory().then(() => {
        if (!alive) return;
        if (e.payload) {
          setError(e.payload);
          setStatus("error");
        } else {
          setStatus((s) => (s === "running" ? "done" : s));
        }
      });
    });
    return () => {
      alive = false;
      void unP.then((f) => f());
      void unD.then((f) => f());
    };
  }, [loadHistory]);

  // Start once on mount — unless a run is already going.
  useEffect(() => {
    if (startedRef.current) return;
    startedRef.current = true;
    void loadHistory();
    void speedtestRunning()
      .catch(() => false)
      .then((busy) => {
        if (!busy) void run();
      });
  }, [loadHistory, run]);

  useEffect(() => {
    if (!focused) return;
    const handler = (event: KeyboardEvent) => {
      const t = event.target as HTMLElement | null;
      const typing = !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA");
      if (event.key === "Escape") {
        event.preventDefault();
        onExit();
      } else if (!typing && event.key.toLowerCase() === "r" && status !== "running") {
        event.preventDefault();
        void run();
      }
    };
    window.addEventListener("keydown", handler, true);
    return () => window.removeEventListener("keydown", handler, true);
  }, [focused, onExit, run, status]);

  // After a reconnect we have no `result` object, only the history's newest row.
  const shown: SpeedtestResult | SpeedtestHistoryEntry | null =
    result ?? (status === "done" ? (history[0] ?? null) : null);
  const earlier = history.filter((h) => !shown || h.at !== shown.at);

  return (
    <div className="flex h-full flex-col gap-3 overflow-y-auto p-3 text-sm">
      <div className="flex items-center gap-2">
        <Gauge size={17} className="text-[var(--color-accent)]" />
        <span className="font-semibold">Speedtest</span>
        <button
          onClick={() => void run()}
          disabled={status === "running"}
          className="ml-auto rounded p-1.5 text-[var(--color-muted)] hover:bg-[var(--color-bg)] disabled:opacity-40"
          title="Neu messen (R)"
          aria-label="Neu messen"
        >
          <RefreshCw size={14} className={status === "running" ? "animate-spin" : ""} />
        </button>
      </div>

      {status === "running" && <Running progress={progress} />}

      {status === "error" && (
        <div role="alert" className="rounded-lg border border-red-400/40 bg-red-400/10 p-3 text-red-300">
          {error || "Messung fehlgeschlagen."}
          <div className="mt-2 text-xs text-[var(--color-muted)]">
            Braucht eine Internetverbindung. R misst erneut.
          </div>
        </div>
      )}

      {status === "done" && shown && <Result r={shown} earlier={earlier} />}

      {history.length > 0 && (
        <HistoryList
          rows={history.slice(0, HISTORY_SHOWN)}
          onClear={async () => {
            await speedtestClearHistory().catch(() => {});
            setHistory([]);
          }}
        />
      )}

      <p className="mt-auto text-[10px] leading-relaxed text-[var(--color-muted)]">
        Gemessen gegen speed.cloudflare.com (sieht deine IP-Adresse). Eine Messung überträgt je nach
        Leitung bis zu ~200 MB. Ergebnisse bleiben lokal. R misst neu, Esc schließt.
      </p>
    </div>
  );
}

function Running({ progress }: { progress: SpeedtestProgress | null }) {
  const phase = progress?.phase ?? "meta";
  const overall = progress ? overallProgress(progress.phase, progress.fraction) : 0;
  const value =
    progress?.value == null
      ? "…"
      : phase === "latency"
        ? formatMs(progress.value)
        : `${formatMbps(progress.value)}`;
  return (
    <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4" aria-live="polite">
      <div className="flex items-center gap-2 text-xs uppercase tracking-widest text-[var(--color-muted)]">
        <Loader2 size={12} className="animate-spin" />
        {PHASE_LABELS[phase]}
      </div>
      <div className="mt-2 flex items-baseline gap-2 tabular-nums">
        <span className="font-[var(--font-mono)] text-4xl font-semibold">{value}</span>
        {phase !== "latency" && phase !== "meta" && progress?.value != null && (
          <span className="text-[var(--color-muted)]">Mbit/s</span>
        )}
      </div>
      <div className="mt-3 h-1.5 overflow-hidden rounded-full bg-[var(--color-bg)]">
        <div
          className="h-full origin-left rounded-full bg-[var(--color-accent)] transition-transform duration-(--duration-fast) ease-sharp"
          style={{ transform: `scaleX(${overall})` }}
        />
      </div>
    </div>
  );
}

function Result({
  r,
  earlier,
}: {
  r: SpeedtestResult | SpeedtestHistoryEntry;
  earlier: SpeedtestHistoryEntry[];
}) {
  const rating = rateDownload(r.download_bps);
  const meaning = downloadMeaning(r.download_bps);
  const country = "country" in r ? r.country : null;
  return (
    <>
      <div className="grid grid-cols-2 gap-2">
        <Big
          icon={<ArrowDown size={14} />}
          label="Download"
          value={formatMbps(r.download_bps)}
          unit="Mbit/s"
          delta={deltaPercent(r.download_bps, earlier.map((h) => h.download_bps))}
        />
        <Big
          icon={<ArrowUp size={14} />}
          label="Upload"
          value={formatMbps(r.upload_bps)}
          unit="Mbit/s"
          delta={deltaPercent(r.upload_bps, earlier.map((h) => h.upload_bps))}
        />
      </div>
      <div className="grid grid-cols-2 gap-2 text-xs">
        <Small icon={<Timer size={12} />} label="Latenz" value={formatMs(r.latency_ms)} />
        <Small label="Jitter" value={formatMs(r.jitter_ms)} />
      </div>
      {rating && (
        <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2 text-xs">
          <span className="font-semibold">Verbindung {rating}.</span> {meaning}
        </div>
      )}
      <div className="text-xs text-[var(--color-muted)]">
        Server: Cloudflare {r.colo ?? "?"}
        {country ? ` · ${country}` : ""}
        {earlier.length > 0 ? ` · Vergleich mit dem Median der ${earlier.length} früheren Messungen` : ""}
      </div>
    </>
  );
}

function Big({
  icon,
  label,
  value,
  unit,
  delta,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  unit: string;
  delta: number | null;
}) {
  return (
    <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
      <div className="flex items-center gap-1.5 text-xs uppercase tracking-widest text-[var(--color-muted)]">
        <span className="text-[var(--color-accent)]">{icon}</span>
        {label}
      </div>
      <div className="mt-1 flex items-baseline gap-1.5 tabular-nums">
        <span className="font-[var(--font-mono)] text-3xl font-semibold">{value}</span>
        <span className="text-xs text-[var(--color-muted)]">{unit}</span>
      </div>
      {delta != null && (
        <div className={`mt-1 text-xs ${delta >= 0 ? "text-emerald-400" : "text-amber-400"}`}>
          {delta >= 0 ? "+" : ""}
          {delta} % ggü. früher
        </div>
      )}
    </div>
  );
}

function Small({ icon, label, value }: { icon?: React.ReactNode; label: string; value: string }) {
  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2">
      <div className="flex items-center gap-1 text-[var(--color-muted)]">
        {icon}
        {label}
      </div>
      <div className="mt-0.5 font-medium tabular-nums">{value}</div>
    </div>
  );
}

function HistoryList({ rows, onClear }: { rows: SpeedtestHistoryEntry[]; onClear: () => void }) {
  const max = Math.max(0, ...rows.map((r) => r.download_bps ?? 0), ...rows.map((r) => r.upload_bps ?? 0));
  const fmt = new Intl.DateTimeFormat("de-DE", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" });
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-2">
      <div className="mb-1 flex items-center text-xs text-[var(--color-muted)]">
        Verlauf
        <button
          onClick={onClear}
          className="ml-auto rounded p-1 hover:bg-[var(--color-surface)]"
          title="Verlauf löschen"
          aria-label="Verlauf löschen"
        >
          <Trash2 size={12} />
        </button>
      </div>
      <ul className="flex flex-col gap-1">
        {rows.map((h) => (
          <li key={h.id} className="grid grid-cols-[5.5rem_1fr_3.5rem] items-center gap-2 text-[11px] tabular-nums">
            <span className="text-[var(--color-muted)]">{fmt.format(new Date(h.at))}</span>
            <span className="flex flex-col gap-0.5" title={`↓ ${formatMbps(h.download_bps)} · ↑ ${formatMbps(h.upload_bps)} Mbit/s`}>
              <span className="h-1 rounded-full bg-[var(--color-accent)]" style={{ width: `${barShare(h.download_bps, max) * 100}%` }} />
              <span className="h-1 rounded-full bg-[var(--color-muted)]" style={{ width: `${barShare(h.upload_bps, max) * 100}%` }} />
            </span>
            <span className="text-right">{formatMbps(h.download_bps)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
