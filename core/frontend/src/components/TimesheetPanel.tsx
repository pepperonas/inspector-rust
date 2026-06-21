import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Check,
  ChevronLeft,
  ChevronRight,
  Circle,
  Clock,
  GitMerge,
  Pause,
  Pencil,
  Download,
  Plus,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import {
  trackGetDay,
  trackStatus,
  trackUpdateEvent,
  trackDeleteEvent,
  trackMergeEvents,
  trackSetCategory,
  trackExport,
  trackBridgeInfo,
  trackBridgeRegenerate,
  trackExportExtension,
  trackDistinctCategories,
  trackAddEvent,
  trackCleanupDay,
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
  msToTimeInput,
  paletteColor,
  shiftDay,
  shiftWeek,
  timeInputToMs,
  timelineBand,
  weekBounds,
} from "../lib/timesheet";
import { TimesheetWeek } from "./TimesheetWeek";

/**
 * Timesheet tab — a day-navigable, charted view of tracked time (read-only in
 * this step; inline editing follows). `←/→` (or the buttons) page days, `t`
 * jumps to today. Charts are dependency-free inline SVG (shared with the HTML
 * export). While viewing *today* and tracking is active it polls so the open
 * interval grows live.
 */
interface EventDraft {
  app: string;
  category: string;
  start: string; // "HH:MM"
  end: string; // "HH:MM" ("" when the event is still open)
  idle: boolean;
  applyAll: boolean;
  wasOpen: boolean;
}

export function TimesheetPanel() {
  const [date, setDate] = useState(() => localDateStr());
  const [mode, setMode] = useState<"day" | "week">("day");
  const [report, setReport] = useState<DayReport | null>(null);
  const [status, setStatus] = useState<TrackStatus | null>(null);
  const [now, setNow] = useState(() => Date.now());
  // Inline editing: which rows are selected (for merge) + which one is open in
  // the editor + its working draft.
  const [selected, setSelected] = useState<Set<number>>(() => new Set());
  const [editingId, setEditingId] = useState<number | null>(null);
  const [draft, setDraft] = useState<EventDraft | null>(null);
  // Events area view: "timeline" (editable list, default) vs "grouped" by app.
  const [eventsView, setEventsView] = useState<"grouped" | "timeline">("timeline");
  // Known category names (for assign autocomplete).
  const [knownCategories, setKnownCategories] = useState<string[]>([]);
  // Manual "add entry" form open?
  const [adding, setAdding] = useState(false);

  const load = useCallback((d: string) => {
    trackGetDay(d)
      .then(setReport)
      .catch(() => setReport(null));
  }, []);

  useEffect(() => {
    load(date);
    setNow(Date.now());
    trackStatus().then(setStatus).catch(() => undefined);
    trackDistinctCategories().then(setKnownCategories).catch(() => undefined);
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

  // Keyboard nav reads the live mode without re-binding the listener.
  const modeRef = useRef(mode);
  useEffect(() => {
    modeRef.current = mode;
  }, [mode]);

  // Keyboard day-nav (ignore when typing in an input).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) return;
      if (e.key === "ArrowLeft") {
        e.preventDefault();
        setDate((d) => (modeRef.current === "week" ? shiftWeek(d, -1) : shiftDay(d, -1)));
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        setDate((d) => (modeRef.current === "week" ? shiftWeek(d, 1) : shiftDay(d, 1)));
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

  const refresh = () => {
    setEditingId(null);
    setDraft(null);
    load(date);
  };

  const toggleSelect = (id: number) =>
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  const beginEdit = (e: TrackEvent) => {
    setEditingId(e.id);
    setDraft({
      app: e.app_name,
      category: e.category ?? "",
      start: msToTimeInput(e.started_at),
      end: e.ended_at != null ? msToTimeInput(e.ended_at) : "",
      idle: e.is_idle,
      applyAll: false,
      wasOpen: e.ended_at == null,
    });
  };

  const saveEdit = async (e: TrackEvent) => {
    if (!draft) return;
    const cat = draft.category.trim();
    const startedAt = timeInputToMs(dayStart, draft.start, e.started_at) ?? e.started_at;
    const patch: Parameters<typeof trackUpdateEvent>[1] = {
      app_name: draft.app.trim() || e.app_name,
      category: cat, // "" clears the category
      is_idle: draft.idle,
      started_at: startedAt,
    };
    if (!draft.wasOpen && draft.end) {
      const endedAt = timeInputToMs(dayStart, draft.end, e.ended_at ?? undefined);
      if (endedAt != null) patch.ended_at = endedAt;
    }
    try {
      await trackUpdateEvent(e.id, patch);
      if (draft.applyAll && cat !== "") await trackSetCategory(patch.app_name ?? e.app_name, cat);
    } catch (err) {
      console.error("track update failed", err);
    }
    refresh();
  };

  const delEvent = async (id: number) => {
    try {
      await trackDeleteEvent(id);
    } catch (err) {
      console.error("track delete failed", err);
    }
    setSelected((s) => {
      const n = new Set(s);
      n.delete(id);
      return n;
    });
    refresh();
  };

  const doExport = (format: "csv" | "html") => {
    const from = dayStart;
    void trackExport(format, from, from + 86_400_000).catch((e) =>
      console.error("export failed", e),
    );
  };

  const doCleanup = async () => {
    try {
      const n = await trackCleanupDay(date, 15);
      if (n > 0) refresh();
    } catch (e) {
      console.error("cleanup failed", e);
    }
  };

  const mergeSelected = async () => {
    const ids = [...selected];
    if (ids.length < 2) return;
    try {
      await trackMergeEvents(ids);
    } catch (err) {
      console.error("track merge failed", err);
    }
    setSelected(new Set());
    refresh();
  };

  return (
    <div className="flex h-full flex-col overflow-hidden text-[var(--color-fg)]">
      {/* Day navigation + status */}
      <div className="flex shrink-0 items-center gap-2 border-b border-[var(--color-border)] px-4 py-2">
        {/* Day ↔ Week mode */}
        <div className="flex overflow-hidden rounded-full border border-[var(--color-border)]">
          {(["day", "week"] as const).map((mo) => (
            <button
              key={mo}
              type="button"
              onClick={() => setMode(mo)}
              className={
                "md3-press px-2.5 py-1 text-[12px] " +
                (mode === mo
                  ? "bg-[var(--color-accent)] font-semibold text-[var(--color-accent-fg)]"
                  : "hover:bg-[var(--color-surface)]")
              }
            >
              {mo === "day" ? "Day" : "Week"}
            </button>
          ))}
        </div>
        <button
          type="button"
          onClick={() => setDate((d) => (mode === "week" ? shiftWeek(d, -1) : shiftDay(d, -1)))}
          className="md3-press rounded-lg border border-[var(--color-border)] p-1 hover:bg-[var(--color-surface)]"
          title={mode === "week" ? "Previous week" : "Previous day (←)"}
        >
          <ChevronLeft size={16} />
        </button>
        {mode === "day" ? (
          <input
            type="date"
            value={date}
            max={localDateStr()}
            onChange={(e) => e.target.value && setDate(e.target.value)}
            className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[13px] tabular-nums"
          />
        ) : (
          <span className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[13px] tabular-nums">
            {weekBounds(date).from} – {weekBounds(date).to}
          </span>
        )}
        <button
          type="button"
          onClick={() => setDate((d) => (mode === "week" ? shiftWeek(d, 1) : shiftDay(d, 1)))}
          disabled={mode === "day" && isToday}
          className="md3-press rounded-lg border border-[var(--color-border)] p-1 enabled:hover:bg-[var(--color-surface)] disabled:opacity-40"
          title={mode === "week" ? "Next week" : "Next day (→)"}
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

        {mode === "day" && report && report.events.length > 0 && (
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={() => doExport("csv")}
              className="md3-press flex items-center gap-1 rounded-lg border border-[var(--color-border)] px-2 py-1 text-[12px] hover:bg-[var(--color-surface)]"
              title="Export this day as CSV → Downloads"
            >
              <Download size={13} /> CSV
            </button>
            <button
              type="button"
              onClick={() => doExport("html")}
              className="md3-press flex items-center gap-1 rounded-lg border border-[var(--color-border)] px-2 py-1 text-[12px] hover:bg-[var(--color-surface)]"
              title="Export this day as a self-contained HTML report → Downloads"
            >
              <Download size={13} /> HTML
            </button>
          </div>
        )}

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

      {mode === "week" ? (
        <TimesheetWeek
          date={date}
          onPickDay={(d) => {
            setDate(d);
            setMode("day");
          }}
        />
      ) : !report || report.events.length === 0 ? (
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
            <Stat
              label="Productive"
              value={`${
                report.total_active_s + report.total_idle_s > 0
                  ? Math.round(
                      (report.total_active_s / (report.total_active_s + report.total_idle_s)) * 100,
                    )
                  : 0
              }%`}
            />
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

          {report.by_project.length > 0 && (
            <Card title="By project">
              <Bars buckets={report.by_project} total={report.total_active_s} />
            </Card>
          )}

          {report.claude.length > 0 && (
            <Card title="Claude Code">
              <div className="flex flex-col">
                <div className="flex items-center gap-2 border-b border-[var(--color-border)]/60 pb-1 text-[11px] text-[var(--color-muted)]">
                  <span className="min-w-0 flex-1">Project</span>
                  <span className="w-[58px] text-right">Time</span>
                  <span className="w-[70px] text-right">Tokens in</span>
                  <span className="w-[70px] text-right">Tokens out</span>
                </div>
                {report.claude.map((c) => (
                  <div key={c.project} className="flex items-center gap-2 py-1 text-[12px]">
                    <span className="min-w-0 flex-1 truncate font-medium">{c.project}</span>
                    <span className="w-[58px] text-right tabular-nums">{formatDuration(c.seconds)}</span>
                    <span className="w-[70px] text-right tabular-nums text-[var(--color-muted)]">
                      {c.tokens_in.toLocaleString()}
                    </span>
                    <span className="w-[70px] text-right tabular-nums text-[var(--color-muted)]">
                      {c.tokens_out.toLocaleString()}
                    </span>
                  </div>
                ))}
              </div>
            </Card>
          )}

          {/* View toggle: grouped by app (expandable detail) vs the editable
              chronological timeline. */}
          <div className="flex items-center gap-2">
            <span className="text-[12px] font-medium text-[var(--color-muted)]">View</span>
            <div className="flex overflow-hidden rounded-full border border-[var(--color-border)]">
              {(["grouped", "timeline"] as const).map((v) => (
                <button
                  key={v}
                  type="button"
                  onClick={() => setEventsView(v)}
                  className={
                    "md3-press px-3 py-1 text-[12px] " +
                    (eventsView === v
                      ? "bg-[var(--color-accent)] font-semibold text-[var(--color-accent-fg)]"
                      : "hover:bg-[var(--color-surface)]")
                  }
                >
                  {v === "grouped" ? "By app" : "Timeline"}
                </button>
              ))}
            </div>
          </div>

          {/* Grouped view — every app expands to its detail (browser → hosts,
              else window titles). */}
          {eventsView === "grouped" &&
            (report.app_breakdown.length === 0 ? (
              <Card title="By app">
                <p className="text-[12px] text-[var(--color-muted)]">No activity.</p>
              </Card>
            ) : (
              <Card title={`By app (${report.app_breakdown.length})`}>
                {/* Native <details> so the toggle is browser-driven — no React
                    state to lose across the live-refresh re-renders. */}
                <div className="flex flex-col gap-1">
                  {report.app_breakdown.map((b) => {
                    const unit = b.source === "browser" ? "site" : "window";
                    return (
                      <details key={b.app} className="ts-app-group">
                        <summary className="md3-press flex w-full cursor-pointer list-none items-center gap-2 rounded-lg px-1 py-1 text-[12px] hover:bg-[var(--color-surface)]">
                          <ChevronRight
                            size={14}
                            className="ts-chevron shrink-0 text-[var(--color-muted)] transition-transform"
                          />
                          <span className="min-w-0 flex-1 truncate text-left font-medium">{b.app}</span>
                          {b.category && (
                            <span className="shrink-0 rounded-full bg-[var(--color-accent)]/15 px-1.5 text-[10px] text-[var(--color-accent)]">
                              {b.category}
                            </span>
                          )}
                          <span className="shrink-0 text-[var(--color-muted)]">
                            {b.details.length} {unit}
                            {b.details.length === 1 ? "" : "s"}
                          </span>
                          <span className="w-[58px] shrink-0 text-right tabular-nums">
                            {formatDuration(b.seconds)}
                          </span>
                        </summary>
                        <div className="ml-6 flex flex-col border-l border-[var(--color-border)] pl-2">
                          {/* Assign a category to the whole app (rule + back-fill). */}
                          <CategoryAssign
                            app={b.app}
                            current={b.category}
                            categories={knownCategories}
                            onAssigned={refresh}
                          />
                          {b.details.map((d) => (
                            <div
                              key={d.label}
                              className="flex items-center gap-2 py-0.5 text-[12px]"
                              title={d.label}
                            >
                              <span className="min-w-0 flex-1 truncate">{d.label}</span>
                              <span className="shrink-0 text-[10px] text-[var(--color-muted)]">
                                ×{d.count}
                              </span>
                              <span className="w-[58px] shrink-0 text-right tabular-nums text-[var(--color-muted)]">
                                {formatDuration(d.seconds)}
                              </span>
                            </div>
                          ))}
                        </div>
                      </details>
                    );
                  })}
                </div>
              </Card>
            ))}

          {/* Timeline view — selectable (merge) + inline-editable event list */}
          {eventsView === "timeline" && (
          <Card title={`Events (${report.events.length})`}>
            {/* Quick actions: add a manual entry · tidy idle/fragments */}
            <div className="mb-2 flex items-center gap-2">
              <button
                type="button"
                onClick={() => setAdding((a) => !a)}
                className="md3-press flex items-center gap-1 rounded-full border border-[var(--color-border)] px-2.5 py-1 text-[12px] hover:bg-[var(--color-surface)]"
              >
                <Plus size={13} /> Add entry
              </button>
              <button
                type="button"
                onClick={() => void doCleanup()}
                className="md3-press flex items-center gap-1 rounded-full border border-[var(--color-border)] px-2.5 py-1 text-[12px] hover:bg-[var(--color-surface)]"
                title="Delete idle spans + sub-15s fragments for this day"
              >
                <Sparkles size={13} /> Clean up
              </button>
            </div>
            {adding && (
              <AddEntryForm
                date={date}
                categories={knownCategories}
                onAdded={() => {
                  setAdding(false);
                  refresh();
                }}
                onCancel={() => setAdding(false)}
              />
            )}
            {selected.size >= 2 && (
              <div className="mb-2 flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => void mergeSelected()}
                  className="md3-press flex items-center gap-1 rounded-full bg-[var(--color-accent)] px-3 py-1 text-[12px] font-semibold text-[var(--color-accent-fg)]"
                >
                  <GitMerge size={13} /> Merge {selected.size}
                </button>
                <button
                  type="button"
                  onClick={() => setSelected(new Set())}
                  className="text-[12px] text-[var(--color-muted)] hover:underline"
                >
                  Clear
                </button>
              </div>
            )}
            <div className="flex flex-col">
              {report.events.map((e) => (
                <div key={e.id} className="border-t border-[var(--color-border)]/60 first:border-t-0">
                  <div
                    onDoubleClick={() => (editingId === e.id ? refresh() : beginEdit(e))}
                    className={
                      "group flex cursor-default items-center gap-2 py-1.5 text-[12px] " +
                      (selected.has(e.id) ? "bg-[var(--color-accent)]/8 -mx-1 rounded px-1" : "")
                    }
                    title="Double-click to edit"
                  >
                    <input
                      type="checkbox"
                      checked={selected.has(e.id)}
                      onChange={() => toggleSelect(e.id)}
                      className="shrink-0 accent-[var(--color-accent)]"
                      title="Select (for merge)"
                    />
                    <span className="w-[92px] shrink-0 tabular-nums text-[var(--color-muted)]">
                      {formatClock(e.started_at)}–{e.ended_at ? formatClock(e.ended_at) : "…"}
                    </span>
                    <span
                      className="h-2.5 w-2.5 shrink-0 rounded-full"
                      style={{ backgroundColor: e.is_idle ? "var(--color-muted)" : appColors[e.app_name] ?? paletteColor(0) }}
                    />
                    <span className="w-[120px] shrink-0 truncate font-medium">{e.app_name}</span>
                    <span className="min-w-0 flex-1 truncate text-[var(--color-muted)]">
                      {e.category ? `[${e.category}] ` : ""}
                      {e.host ?? e.window_title ?? ""}
                    </span>
                    {e.is_idle && (
                      <span className="shrink-0 rounded-full bg-[var(--color-surface)] px-1.5 text-[10px] text-[var(--color-muted)]">
                        idle
                      </span>
                    )}
                    <span className="w-[58px] shrink-0 text-right tabular-nums">
                      {formatDuration(eventDuration(e))}
                    </span>
                    <button
                      type="button"
                      onClick={() => (editingId === e.id ? refresh() : beginEdit(e))}
                      className="shrink-0 rounded p-1 text-[var(--color-muted)] opacity-0 hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)] group-hover:opacity-100"
                      title="Edit"
                    >
                      <Pencil size={13} />
                    </button>
                  </div>

                  {editingId === e.id && draft && (
                    <div
                      className="mb-2 flex flex-col gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2 text-[12px]"
                      onKeyDown={(ev) => {
                        if (ev.key === "Enter") void saveEdit(e);
                        else if (ev.key === "Escape") refresh();
                      }}
                    >
                      <div className="flex flex-wrap items-center gap-2">
                        <label className="flex items-center gap-1">
                          App
                          <input
                            value={draft.app}
                            onChange={(ev) => setDraft({ ...draft, app: ev.target.value })}
                            className="w-[120px] rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5"
                          />
                        </label>
                        <label className="flex items-center gap-1">
                          Category
                          <input
                            value={draft.category}
                            placeholder="—"
                            onChange={(ev) => setDraft({ ...draft, category: ev.target.value })}
                            className="w-[110px] rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5"
                          />
                        </label>
                        <label className="flex items-center gap-1">
                          Start
                          <input
                            type="time"
                            value={draft.start}
                            onChange={(ev) => setDraft({ ...draft, start: ev.target.value })}
                            className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5 tabular-nums"
                          />
                        </label>
                        {!draft.wasOpen && (
                          <label className="flex items-center gap-1">
                            End
                            <input
                              type="time"
                              value={draft.end}
                              onChange={(ev) => setDraft({ ...draft, end: ev.target.value })}
                              className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5 tabular-nums"
                            />
                          </label>
                        )}
                      </div>
                      <div className="flex flex-wrap items-center gap-3">
                        <label className="flex cursor-pointer items-center gap-1.5">
                          <input
                            type="checkbox"
                            checked={draft.idle}
                            onChange={(ev) => setDraft({ ...draft, idle: ev.target.checked })}
                            className="accent-[var(--color-accent)]"
                          />
                          Idle
                        </label>
                        <label className="flex cursor-pointer items-center gap-1.5">
                          <input
                            type="checkbox"
                            checked={draft.applyAll}
                            onChange={(ev) => setDraft({ ...draft, applyAll: ev.target.checked })}
                            className="accent-[var(--color-accent)]"
                          />
                          Apply category to all “{draft.app}”
                        </label>
                        <div className="ml-auto flex items-center gap-1.5">
                          <button
                            type="button"
                            onClick={() => void delEvent(e.id)}
                            className="md3-press flex items-center gap-1 rounded-full border border-rose-500/40 px-2.5 py-1 text-rose-400 hover:bg-rose-500/10"
                          >
                            <Trash2 size={13} /> Delete
                          </button>
                          <button
                            type="button"
                            onClick={refresh}
                            className="md3-press rounded-full border border-[var(--color-border)] p-1.5"
                            title="Cancel"
                          >
                            <X size={13} />
                          </button>
                          <button
                            type="button"
                            onClick={() => void saveEdit(e)}
                            className="md3-press flex items-center gap-1 rounded-full bg-[var(--color-accent)] px-3 py-1 font-semibold text-[var(--color-accent-fg)]"
                          >
                            <Check size={13} /> Save
                          </button>
                        </div>
                      </div>
                    </div>
                  )}
                </div>
              ))}
            </div>
          </Card>
          )}
          <p className="text-center text-[11px] text-[var(--color-muted)]">
            ← → change day · t today
            {eventsView === "grouped"
              ? " · ▸ expand an app for its detail"
              : " · double-click to edit · ☑ select to merge"}
          </p>
        </div>
      )}
      <BridgeFooter />
    </div>
  );
}

/** Collapsible "Browser extension" footer — shows the loopback bridge port +
 *  token (to paste into the extension's options) + a regenerate button. */
function BridgeFooter() {
  const [info, setInfo] = useState<{ port: number; token: string } | null>(null);
  const [open, setOpen] = useState(false);
  const [savedPath, setSavedPath] = useState<string | null>(null);
  useEffect(() => {
    if (open && !info) trackBridgeInfo().then(setInfo).catch(() => undefined);
  }, [open, info]);
  return (
    <details
      className="shrink-0 border-t border-[var(--color-border)] px-4 py-2 text-[12px]"
      onToggle={(e) => setOpen((e.target as HTMLDetailsElement).open)}
    >
      <summary className="cursor-pointer text-[var(--color-muted)]">Browser extension</summary>
      {info ? (
        <div className="mt-2 flex flex-col gap-1.5">
          <p className="text-[var(--color-muted)]">
            Paste these into the extension's Options (loopback only):
          </p>
          <div className="flex items-center gap-2">
            <span className="w-[52px] text-[var(--color-muted)]">Port</span>
            <code className="rounded bg-[var(--color-surface)] px-2 py-0.5">{info.port}</code>
          </div>
          <div className="flex items-center gap-2">
            <span className="w-[52px] text-[var(--color-muted)]">Token</span>
            <code className="min-w-0 flex-1 truncate rounded bg-[var(--color-surface)] px-2 py-0.5 font-[var(--font-mono)]">
              {info.token}
            </code>
            <button
              type="button"
              onClick={() => void navigator.clipboard?.writeText(info.token)}
              className="md3-press rounded-lg border border-[var(--color-border)] px-2 py-0.5"
            >
              Copy
            </button>
          </div>
          <div className="mt-1 flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={() => void trackBridgeRegenerate().then((t) => setInfo({ ...info, token: t }))}
              className="md3-press rounded-full border border-[var(--color-border)] px-3 py-1"
            >
              Regenerate token
            </button>
            <button
              type="button"
              onClick={() => void trackExportExtension().then(setSavedPath).catch(() => undefined)}
              className="md3-press rounded-full border border-[var(--color-border)] px-3 py-1"
            >
              Save extension to Downloads
            </button>
          </div>
          <p className="text-[11px] text-[var(--color-muted)]">
            Chrome can't auto-install it — save the folder, then in{" "}
            <code className="rounded bg-[var(--color-surface)] px-1">chrome://extensions</code>{" "}
            enable Developer mode → <strong>Load unpacked</strong> → pick the folder.
            {savedPath && <span className="text-[var(--color-accent)]"> Saved ✓</span>}
          </p>
        </div>
      ) : (
        <p className="mt-2 text-[var(--color-muted)]">Loading…</p>
      )}
    </details>
  );
}

/** Manual time-entry form (app · category · start · end on the viewed day). */
function AddEntryForm({
  date,
  categories,
  onAdded,
  onCancel,
}: {
  date: string;
  categories: string[];
  onAdded: () => void;
  onCancel: () => void;
}) {
  const [app, setApp] = useState("");
  const [category, setCategory] = useState("");
  const [start, setStart] = useState("09:00");
  const [end, setEnd] = useState("10:00");
  const [err, setErr] = useState<string | null>(null);
  const dayStart = dayStartMs(date);

  const submit = () => {
    const s = timeInputToMs(dayStart, start);
    const e = timeInputToMs(dayStart, end);
    if (!app.trim()) return setErr("App name required");
    if (s == null || e == null) return setErr("Invalid time");
    if (e <= s) return setErr("End must be after start");
    void trackAddEvent({
      appName: app.trim(),
      category: category.trim() || null,
      startedAt: s,
      endedAt: e,
    })
      .then(onAdded)
      .catch((x) => {
        console.error("add entry failed", x);
        setErr("Failed to add");
      });
  };

  return (
    <div
      className="mb-2 flex flex-col gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-2 text-[12px]"
      onKeyDown={(e) => {
        if (e.key === "Enter") submit();
        else if (e.key === "Escape") onCancel();
      }}
    >
      <div className="flex flex-wrap items-center gap-2">
        <input
          autoFocus
          value={app}
          placeholder="App / activity"
          onChange={(e) => setApp(e.target.value)}
          className="w-[150px] rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5"
        />
        <input
          list="add-entry-cats"
          value={category}
          placeholder="Category"
          onChange={(e) => setCategory(e.target.value)}
          className="w-[110px] rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5"
        />
        <datalist id="add-entry-cats">
          {categories.map((c) => (
            <option key={c} value={c} />
          ))}
        </datalist>
        <label className="flex items-center gap-1">
          Start
          <input
            type="time"
            value={start}
            onChange={(e) => setStart(e.target.value)}
            className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5 tabular-nums"
          />
        </label>
        <label className="flex items-center gap-1">
          End
          <input
            type="time"
            value={end}
            onChange={(e) => setEnd(e.target.value)}
            className="rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5 tabular-nums"
          />
        </label>
      </div>
      <div className="flex items-center gap-2">
        {err && <span className="text-rose-400">{err}</span>}
        <div className="ml-auto flex items-center gap-1.5">
          <button
            type="button"
            onClick={onCancel}
            className="md3-press rounded-full border border-[var(--color-border)] px-3 py-1"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={submit}
            className="md3-press rounded-full bg-[var(--color-accent)] px-3 py-1 font-semibold text-[var(--color-accent-fg)]"
          >
            Add
          </button>
        </div>
      </div>
    </div>
  );
}

/** Assign a category to a whole app (sets the rule + back-fills every event).
 *  An input with a datalist of known categories for fast, typo-free entry. */
function CategoryAssign({
  app,
  current,
  categories,
  onAssigned,
}: {
  app: string;
  current: string | null;
  categories: string[];
  onAssigned: () => void;
}) {
  const [value, setValue] = useState(current ?? "");
  const listId = `cats-${app.replace(/[^a-z0-9]/gi, "")}`;
  const dirty = value.trim() !== (current ?? "");
  const assign = () => {
    const v = value.trim();
    if (!v) return;
    void trackSetCategory(app, v).then(onAssigned).catch(() => undefined);
  };
  return (
    <div className="flex items-center gap-1.5 py-1 text-[12px]">
      <span className="text-[var(--color-muted)]">Category</span>
      <input
        list={listId}
        value={value}
        placeholder="e.g. Work"
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") assign();
        }}
        className="min-w-0 flex-1 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5"
      />
      <datalist id={listId}>
        {categories.map((c) => (
          <option key={c} value={c} />
        ))}
      </datalist>
      <button
        type="button"
        onClick={assign}
        disabled={!dirty || value.trim() === ""}
        className="md3-press rounded-full bg-[var(--color-accent)] px-2.5 py-0.5 font-semibold text-[var(--color-accent-fg)] disabled:opacity-40"
      >
        Set
      </button>
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
