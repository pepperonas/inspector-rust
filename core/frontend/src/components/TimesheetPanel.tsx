import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ChevronLeft, ChevronRight, Circle, Clock, Pause } from "lucide-react";
import {
  trackGetDay,
  trackStatus,
  type DayReport,
  type TrackEvent,
  type TrackStatus,
} from "../lib/ipc";
import {
  colorMap,
  donutSegmentPath,
  donutSegments,
  formatClock,
  formatDuration,
  dayStartMs,
  localDateStr,
  paletteColor,
  shiftDay,
  timelineBand,
} from "../lib/timesheet";

/**
 * Timesheet tab — a day-navigable, charted view of tracked time (read-only in
 * this step; inline editing follows). `←/→` (or the buttons) page days, `t`
 * jumps to today. Charts are dependency-free inline SVG (shared with the HTML
 * export). While viewing *today* and tracking is active it polls so the open
 * interval grows live.
 */
export function TimesheetPanel() {
  const [date, setDate] = useState(() => localDateStr());
  const [report, setReport] = useState<DayReport | null>(null);
  const [status, setStatus] = useState<TrackStatus | null>(null);
  const [now, setNow] = useState(() => Date.now());

  const load = useCallback((d: string) => {
    trackGetDay(d)
      .then(setReport)
      .catch(() => setReport(null));
  }, []);

  useEffect(() => {
    load(date);
    setNow(Date.now());
    trackStatus().then(setStatus).catch(() => undefined);
  }, [date, load]);

  const isToday = date === localDateStr();
  useEffect(() => {
    if (!isToday) return;
    const id = window.setInterval(() => {
      setNow(Date.now());
      load(date);
    }, 5000);
    return () => window.clearInterval(id);
  }, [isToday, date, load]);

  useEffect(() => {
    let un: (() => void) | undefined;
    let cancelled = false;
    void listen("track-status-changed", () => {
      trackStatus().then(setStatus).catch(() => undefined);
      load(date);
    }).then((u) => {
      if (cancelled) u();
      else un = u;
    });
    return () => {
      cancelled = true;
      un?.();
    };
  }, [date, load]);

  // Keyboard day-nav (ignore when typing in an input).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
      if (e.key === "ArrowLeft") {
        e.preventDefault();
        setDate((d) => shiftDay(d, -1));
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        setDate((d) => shiftDay(d, 1));
      } else if (e.key === "t" || e.key === "T") {
        setDate(localDateStr());
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const appColors = useMemo(
    () => colorMap((report?.by_app ?? []).map((b) => b.key)),
    [report],
  );
  const dayStart = useMemo(() => dayStartMs(date), [date]);

  const eventDuration = (e: TrackEvent) =>
    e.duration_s ?? Math.max(0, Math.floor((now - e.started_at) / 1000));

  return (
    <div className="flex h-full flex-col overflow-hidden text-[var(--color-fg)]">
      {/* Day navigation + status */}
      <div className="flex shrink-0 items-center gap-2 border-b border-[var(--color-border)] px-4 py-2">
        <button
          type="button"
          onClick={() => setDate((d) => shiftDay(d, -1))}
          className="md3-press rounded-lg border border-[var(--color-border)] p-1 hover:bg-[var(--color-surface)]"
          title="Previous day (←)"
        >
          <ChevronLeft size={16} />
        </button>
        <input
          type="date"
          value={date}
          max={localDateStr()}
          onChange={(e) => e.target.value && setDate(e.target.value)}
          className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[13px] tabular-nums"
        />
        <button
          type="button"
          onClick={() => setDate((d) => shiftDay(d, 1))}
          disabled={isToday}
          className="md3-press rounded-lg border border-[var(--color-border)] p-1 enabled:hover:bg-[var(--color-surface)] disabled:opacity-40"
          title="Next day (→)"
        >
          <ChevronRight size={16} />
        </button>
        <button
          type="button"
          onClick={() => setDate(localDateStr())}
          className="md3-press rounded-lg border border-[var(--color-border)] px-2 py-1 text-[12px] hover:bg-[var(--color-surface)]"
          title="Jump to today (t)"
        >
          Today
        </button>

        <div className="ml-auto flex items-center gap-1 text-[12px]">
          {status?.active ? (
            status.paused ? (
              <span className="flex items-center gap-1 text-[var(--color-muted)]">
                <Pause size={13} /> paused (idle)
              </span>
            ) : (
              <span className="flex items-center gap-1 text-rose-400">
                <Circle size={11} className="animate-pulse fill-rose-500 text-rose-500" />
                recording{status.active_app ? ` · ${status.active_app}` : ""}
              </span>
            )
          ) : (
            <span className="text-[var(--color-muted)]">not tracking</span>
          )}
        </div>
      </div>

      {!report || report.events.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 text-center text-[var(--color-muted)]">
          <Clock size={28} className="opacity-50" />
          <p className="text-[13px]">No tracked time on this day.</p>
          <p className="text-[12px]">
            Type <kbd className="rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1 font-[var(--font-mono)]">track on</kbd>{" "}
            to start tracking.
          </p>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
          {/* Totals */}
          <div className="grid grid-cols-3 gap-3">
            <Stat label="Active" value={formatDuration(report.total_active_s)} accent />
            <Stat label="Idle" value={formatDuration(report.total_idle_s)} />
            <Stat label="Sessions" value={String(report.session_count)} />
          </div>

          {/* Day timeline (24h gantt) */}
          <Card title="Day timeline">
            <Timeline events={report.events} dayStart={dayStart} now={now} colors={appColors} />
          </Card>

          {/* App donut + Category bars */}
          <div className="grid grid-cols-2 gap-3">
            <Card title="By app">
              <Donut buckets={report.by_app} colors={appColors} />
            </Card>
            <Card title="By category">
              <Bars buckets={report.by_category} total={report.total_active_s} />
            </Card>
          </div>

          {report.by_host.length > 0 && (
            <Card title="Top hosts">
              <Bars buckets={report.by_host.slice(0, 6)} total={report.total_active_s} />
            </Card>
          )}

          {/* Event list */}
          <Card title={`Events (${report.events.length})`}>
            <div className="flex flex-col">
              {report.events.map((e) => (
                <div
                  key={e.id}
                  className="flex items-center gap-2 border-t border-[var(--color-border)]/60 py-1.5 text-[12px] first:border-t-0"
                >
                  <span className="w-[92px] shrink-0 tabular-nums text-[var(--color-muted)]">
                    {formatClock(e.started_at)}–{e.ended_at ? formatClock(e.ended_at) : "…"}
                  </span>
                  <span
                    className="h-2.5 w-2.5 shrink-0 rounded-full"
                    style={{ backgroundColor: e.is_idle ? "var(--color-muted)" : appColors[e.app_name] ?? paletteColor(0) }}
                  />
                  <span className="w-[120px] shrink-0 truncate font-medium">{e.app_name}</span>
                  <span className="min-w-0 flex-1 truncate text-[var(--color-muted)]">
                    {e.host ?? e.window_title ?? ""}
                  </span>
                  {e.is_idle && (
                    <span className="shrink-0 rounded-full bg-[var(--color-surface)] px-1.5 text-[10px] text-[var(--color-muted)]">
                      idle
                    </span>
                  )}
                  <span className="shrink-0 rounded-full bg-[var(--color-surface)] px-1.5 text-[10px] text-[var(--color-muted)]">
                    {e.source}
                  </span>
                  <span className="w-[64px] shrink-0 text-right tabular-nums">
                    {formatDuration(eventDuration(e))}
                  </span>
                </div>
              ))}
            </div>
          </Card>
          <p className="text-center text-[11px] text-[var(--color-muted)]">
            ← → change day · t today
          </p>
        </div>
      )}
    </div>
  );
}

// ── Small presentational pieces ──────────────────────────────────────────────

function Stat({ label, value, accent }: { label: string; value: string; accent?: boolean }) {
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3">
      <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">{label}</div>
      <div
        className={
          "mt-0.5 text-[20px] font-bold tabular-nums " +
          (accent ? "text-[var(--color-accent)]" : "")
        }
      >
        {value}
      </div>
    </div>
  );
}

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3">
      <div className="mb-2 text-[12px] font-medium text-[var(--color-muted)]">{title}</div>
      {children}
    </div>
  );
}

function Timeline({
  events,
  dayStart,
  now,
  colors,
}: {
  events: TrackEvent[];
  dayStart: number;
  now: number;
  colors: Record<string, string>;
}) {
  return (
    <div>
      <div className="relative h-7 w-full overflow-hidden rounded-md bg-[var(--color-surface)]">
        {events.map((e) => {
          const band = timelineBand(e.started_at, e.ended_at, dayStart, now);
          if (!band) return null;
          return (
            <div
              key={e.id}
              title={`${e.app_name} · ${formatClock(e.started_at)}`}
              className="absolute top-0 h-full"
              style={{
                left: `${band.leftPct}%`,
                width: `${Math.max(0.3, band.widthPct)}%`,
                backgroundColor: e.is_idle ? "var(--color-border)" : colors[e.app_name] ?? paletteColor(0),
                opacity: e.is_idle ? 0.5 : 0.9,
              }}
            />
          );
        })}
      </div>
      <div className="mt-1 flex justify-between text-[10px] tabular-nums text-[var(--color-muted)]">
        {[0, 6, 12, 18, 24].map((h) => (
          <span key={h}>{String(h).padStart(2, "0")}:00</span>
        ))}
      </div>
    </div>
  );
}

function Donut({
  buckets,
  colors,
}: {
  buckets: { key: string; seconds: number }[];
  colors: Record<string, string>;
}) {
  const top = buckets.slice(0, 7);
  const rest = buckets.slice(7).reduce((a, b) => a + b.seconds, 0);
  const entries = rest > 0 ? [...top, { key: "Other", seconds: rest }] : top;
  const segs = donutSegments(entries.map((e) => e.seconds));
  const total = entries.reduce((a, b) => a + b.seconds, 0);
  if (total <= 0) return <p className="text-[12px] text-[var(--color-muted)]">No active time.</p>;
  return (
    <div className="flex items-center gap-3">
      <svg viewBox="0 0 100 100" className="h-[110px] w-[110px] shrink-0">
        {segs.map((s, i) => (
          <path
            key={entries[i].key}
            d={donutSegmentPath(50, 50, 46, 28, s.start, s.end)}
            fill={entries[i].key === "Other" ? "var(--color-muted)" : colors[entries[i].key] ?? paletteColor(i)}
          />
        ))}
      </svg>
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        {entries.map((e, i) => (
          <div key={e.key} className="flex items-center gap-1.5 text-[11px]">
            <span
              className="h-2.5 w-2.5 shrink-0 rounded-full"
              style={{ backgroundColor: e.key === "Other" ? "var(--color-muted)" : colors[e.key] ?? paletteColor(i) }}
            />
            <span className="min-w-0 flex-1 truncate">{e.key}</span>
            <span className="shrink-0 tabular-nums text-[var(--color-muted)]">
              {formatDuration(e.seconds)}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function Bars({
  buckets,
  total,
}: {
  buckets: { key: string; seconds: number }[];
  total: number;
}) {
  if (buckets.length === 0)
    return <p className="text-[12px] text-[var(--color-muted)]">No data.</p>;
  const max = Math.max(1, ...buckets.map((b) => b.seconds));
  return (
    <div className="flex flex-col gap-1.5">
      {buckets.map((b) => (
        <div key={b.key} className="text-[11px]">
          <div className="mb-0.5 flex justify-between">
            <span className="min-w-0 truncate pr-2">{b.key}</span>
            <span className="shrink-0 tabular-nums text-[var(--color-muted)]">
              {formatDuration(b.seconds)}
              {total > 0 ? ` · ${Math.round((b.seconds / total) * 100)}%` : ""}
            </span>
          </div>
          <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--color-surface)]">
            <div
              className="h-full rounded-full bg-[var(--color-accent)]"
              style={{ width: `${(b.seconds / max) * 100}%` }}
            />
          </div>
        </div>
      ))}
    </div>
  );
}
