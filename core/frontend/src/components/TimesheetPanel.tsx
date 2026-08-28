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
  Play,
  Pencil,
  Download,
  Plus,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import {
  trackGetDay,
  trackSetPaused,
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
  trackSetProject,
  trackDistinctProjects,
  trackExportProjects,
  trackAddEvent,
  trackCleanupDay,
  trackCleanupRange,
  getTimesheetConfig,
  type DayReport,
  type TrackEvent,
  type TrackStatus,
} from "../lib/ipc";
import {
  colorMap,
  categoryColor,
  categoryColorMap,
  projectColor,
  NO_PROJECT,
  donutSegmentPath,
  donutSegments,
  formatClock,
  formatDuration,
  dayStartMs,
  dayEndMs,
  localDateStr,
  msToTimeInput,
  paletteColor,
  shiftDay,
  shiftWeek,
  shortDayLabel,
  timeInputToMs,
  timelineBand,
  weekBounds,
} from "../lib/timesheet";
import { TimesheetWeek } from "./TimesheetWeek";
import { SlotsView } from "./SlotsView";

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
  const [eventsView, setEventsView] = useState<"slots" | "grouped" | "timeline">("slots");
  // Known category / project names (for assign autocomplete).
  const [knownCategories, setKnownCategories] = useState<string[]>([]);
  const [knownProjects, setKnownProjects] = useState<string[]>([]);
  // Pending project assignment from a Gantt time-window drag.
  const [projAssign, setProjAssign] = useState<{ ids: number[]; t1: number; t2: number } | null>(null);
  // Daily focus goal (minutes, 0 = off) + previous-day totals for Δ comparison.
  const [dailyGoalMin, setDailyGoalMin] = useState(0);
  const [prevDay, setPrevDay] = useState<{ active: number; idle: number } | null>(null);
  // Gantt colour source + event-list search/drill-down filter.
  const [timelineColorBy, setTimelineColorBy] = useState<"app" | "category" | "project">("app");
  const [listFilter, setListFilter] = useState<{ kind: "app" | "category"; key: string } | null>(null);
  const [search, setSearch] = useState("");
  // Manual "add entry" form open?
  const [adding, setAdding] = useState(false);
  // Transient error banner (edit validation + IPC errors like the live-event
  // guard) — previously these only went to console.error, invisible in the app.
  const [panelErr, setPanelErr] = useState<string | null>(null);
  // Bumped after a week-scope cleanup so the (self-fetching) week view remounts.
  const [weekReload, setWeekReload] = useState(0);
  useEffect(() => {
    if (!panelErr) return;
    const t = window.setTimeout(() => setPanelErr(null), 6000);
    return () => window.clearTimeout(t);
  }, [panelErr]);
  // Rubber-band drag-selection over the timeline list.
  const listRef = useRef<HTMLDivElement>(null);
  const [bandRect, setBandRect] = useState<{ left: number; top: number; width: number; height: number } | null>(null);

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
    trackDistinctProjects().then(setKnownProjects).catch(() => undefined);
    getTimesheetConfig().then((c) => setDailyGoalMin(c.daily_goal_minutes)).catch(() => undefined);
    // Previous day's totals for the Δ comparison.
    trackGetDay(shiftDay(date, -1))
      .then((r) => setPrevDay({ active: r.total_active_s, idle: r.total_idle_s }))
      .catch(() => setPrevDay(null));
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
  const catColors = useMemo(
    () => categoryColorMap((report?.by_category ?? []).map((b) => b.key)),
    [report],
  );
  const dayStart = useMemo(() => dayStartMs(date), [date]);
  const dayEnd = useMemo(() => dayEndMs(date), [date]);

  // The timeline's colour + label for an event, by the chosen mode. Both the
  // Gantt bands AND the legend use these, so they can never diverge.
  const tlLabel = (e: TrackEvent): string =>
    timelineColorBy === "category"
      ? e.category && e.category !== ""
        ? e.category
        : "Uncategorized"
      : timelineColorBy === "project"
        ? e.project && e.project !== ""
          ? e.project
          : NO_PROJECT
        : e.app_name;
  const tlColor = (e: TrackEvent): string =>
    timelineColorBy === "category"
      ? categoryColor(e.category ?? "Uncategorized")
      : timelineColorBy === "project"
        ? projectColor(e.project && e.project !== "" ? e.project : NO_PROJECT)
        : appColors[e.app_name] ?? paletteColor(0);

  // Legend = the distinct (label, colour) pairs actually drawn in the Gantt
  // (events that produce a visible band), in the bar's left-to-right order,
  // plus an Idle entry if idle bands are shown.
  const legendItems = useMemo(() => {
    const out: { key: string; color: string }[] = [];
    const seen = new Set<string>();
    let hasIdle = false;
    for (const e of report?.events ?? []) {
      if (!timelineBand(e.started_at, e.ended_at, dayStart, now, dayEnd)) continue;
      if (e.is_idle) {
        hasIdle = true;
        continue;
      }
      const key = tlLabel(e);
      if (!seen.has(key)) {
        seen.add(key);
        out.push({ key, color: tlColor(e) });
      }
    }
    out.sort((a, b) => a.key.localeCompare(b.key));
    if (hasIdle) out.push({ key: "Idle", color: "var(--color-border)" });
    return out;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [report, timelineColorBy, appColors, dayStart, dayEnd, now]);

  // Timeline list after applying the drill-down filter + search.
  const shownEvents = useMemo(() => {
    const all = report?.events ?? [];
    const q = search.trim().toLowerCase();
    return all.filter((e) => {
      if (listFilter?.kind === "app" && e.app_name !== listFilter.key) return false;
      if (listFilter?.kind === "category" && (e.category ?? "Uncategorized") !== listFilter.key)
        return false;
      if (q) {
        const hay = `${e.app_name} ${e.window_title ?? ""} ${e.host ?? ""}`.toLowerCase();
        if (!hay.includes(q)) return false;
      }
      return true;
    });
  }, [report, listFilter, search]);

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
      if (endedAt != null) {
        // Reject inverted intervals (e.g. 23:50 → 00:10 typed as an overnight
        // span resolves against the same day) instead of persisting a
        // nonsensical reversed row that silently aggregates as 0s.
        if (endedAt <= startedAt) {
          setPanelErr("End must be after start — edit not saved.");
          return;
        }
        patch.ended_at = endedAt;
      }
    }
    try {
      await trackUpdateEvent(e.id, patch);
      if (draft.applyAll && cat !== "") await trackSetCategory(patch.app_name ?? e.app_name, cat);
    } catch (err) {
      console.error("track update failed", err);
      setPanelErr(String(err));
    }
    refresh();
  };

  const delEvent = async (id: number) => {
    try {
      await trackDeleteEvent(id);
    } catch (err) {
      console.error("track delete failed", err);
      setPanelErr(String(err));
    }
    setSelected((s) => {
      const n = new Set(s);
      n.delete(id);
      return n;
    });
    refresh();
  };

  // Exports the visible scope: the day in Day mode, Mon–Sun in Week mode.
  const doExport = (format: "csv" | "html" | "pdf") => {
    const [from, to] =
      mode === "week"
        ? (() => {
            const w = weekBounds(date);
            return [dayStartMs(w.from), dayEndMs(w.to)];
          })()
        : [dayStart, dayEnd];
    void trackExport(format, from, to).catch((e) => {
      console.error("export failed", e);
      setPanelErr(String(e));
    });
  };

  // Cleans the visible scope: the day in Day mode, the whole week in Week mode.
  const doCleanup = async () => {
    try {
      const n =
        mode === "week"
          ? await (() => {
              const w = weekBounds(date);
              return trackCleanupRange(w.from, w.to, 15);
            })()
          : await trackCleanupDay(date, 15);
      if (n > 0) {
        refresh();
        setWeekReload((k) => k + 1);
      }
    } catch (e) {
      console.error("cleanup failed", e);
      setPanelErr(String(e));
    }
  };

  const togglePause = async () => {
    if (!status) return;
    try {
      await trackSetPaused(!status.manual_paused);
      setStatus(await trackStatus());
    } catch (e) {
      setPanelErr(String(e));
    }
  };

  // Rubber-band selection: drag an empty area of the list to select the rows
  // the rectangle's vertical span covers. Ignores drags that start on a control.
  const onListPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    const target = e.target as HTMLElement;
    if (target.closest("input,button,textarea,select,a,label,[contenteditable='true']")) return;
    const list = listRef.current;
    if (!list) return;
    const rect = list.getBoundingClientRect();
    const startX = e.clientX;
    const startY = e.clientY;
    let moved = false;
    // Shift = add this band to the existing selection; otherwise replace it.
    const base = e.shiftKey ? new Set(selected) : new Set<number>();

    const onMove = (ev: PointerEvent) => {
      if (!moved && Math.hypot(ev.clientX - startX, ev.clientY - startY) < 5) return;
      if (!moved) {
        moved = true;
        document.body.style.userSelect = "none";
      }
      const x1 = Math.min(startX, ev.clientX);
      const x2 = Math.max(startX, ev.clientX);
      const y1 = Math.min(startY, ev.clientY);
      const y2 = Math.max(startY, ev.clientY);
      setBandRect({ left: x1 - rect.left, top: y1 - rect.top, width: x2 - x1, height: y2 - y1 });
      const ids = new Set<number>(base);
      list.querySelectorAll<HTMLElement>("[data-event-id]").forEach((row) => {
        const r = row.getBoundingClientRect();
        if (r.bottom >= y1 && r.top <= y2) {
          const id = Number(row.dataset.eventId);
          if (!Number.isNaN(id)) ids.add(id);
        }
      });
      setSelected(ids);
    };
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      document.body.style.userSelect = "";
      setBandRect(null);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const mergeSelected = async () => {
    const ids = [...selected];
    if (ids.length < 2) return;
    try {
      await trackMergeEvents(ids);
    } catch (err) {
      console.error("track merge failed", err);
      setPanelErr(String(err));
    }
    setSelected(new Set());
    refresh();
  };

  return (
    <div className="flex h-full flex-col overflow-hidden text-[var(--color-fg)]">
      {panelErr && (
        <div className="shrink-0 border-b border-rose-500/40 bg-rose-500/10 px-4 py-1.5 text-[12px]">
          {panelErr}
        </div>
      )}
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

        {(mode === "week" || (report && report.events.length > 0)) && (
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={() => doExport("csv")}
              className="md3-press flex items-center gap-1 rounded-lg border border-[var(--color-border)] px-2 py-1 text-[12px] hover:bg-[var(--color-surface)]"
              title={mode === "week" ? "Export this week (Mon–Sun) as CSV → Downloads" : "Export this day as CSV → Downloads"}
            >
              <Download size={13} /> CSV
            </button>
            <button
              type="button"
              onClick={() => doExport("html")}
              className="md3-press flex items-center gap-1 rounded-lg border border-[var(--color-border)] px-2 py-1 text-[12px] hover:bg-[var(--color-surface)]"
              title={mode === "week" ? "Export this week (Mon–Sun) as a self-contained HTML report → Downloads" : "Export this day as a self-contained HTML report → Downloads"}
            >
              <Download size={13} /> HTML
            </button>
            <button
              type="button"
              onClick={() => doExport("pdf")}
              className="md3-press flex items-center gap-1 rounded-lg border border-[var(--color-border)] px-2 py-1 text-[12px] hover:bg-[var(--color-surface)]"
              title={mode === "week" ? "Diese Woche (Mo–So) als PDF → Downloads" : "Diesen Tag als PDF → Downloads"}
            >
              <Download size={13} /> PDF
            </button>
            {mode === "week" && (
              <button
                type="button"
                onClick={() => void doCleanup()}
                className="md3-press flex items-center gap-1 rounded-lg border border-[var(--color-border)] px-2 py-1 text-[12px] hover:bg-[var(--color-surface)]"
                title="Delete idle spans + sub-15s fragments for the whole week"
              >
                <Sparkles size={13} /> Clean up week
              </button>
            )}
          </div>
        )}

        <div className="ml-auto flex items-center gap-2 text-[12px]">
          {status?.active ? (
            <>
              {status.manual_paused ? (
                <span className="flex items-center gap-1 text-amber-500">
                  <Pause size={13} /> paused
                </span>
              ) : status.paused ? (
                <span className="flex items-center gap-1 text-[var(--color-muted)]">
                  <Pause size={13} /> paused (idle)
                </span>
              ) : (
                <span className="flex items-center gap-1 text-rose-400">
                  <Circle size={11} className="animate-pulse fill-rose-500 text-rose-500" />
                  recording{status.active_app ? ` · ${status.active_app}` : ""}
                </span>
              )}
              <button
                type="button"
                onClick={() => void togglePause()}
                className="md3-press flex items-center gap-1 rounded-full border border-[var(--color-border)] px-2 py-0.5 text-[11px] hover:bg-[var(--color-surface)]"
                title={
                  status.manual_paused
                    ? "Resume recording"
                    : "Pause recording (session stays active; nothing is recorded until resume)"
                }
              >
                {status.manual_paused ? (
                  <>
                    <Play size={12} /> Resume
                  </>
                ) : (
                  <>
                    <Pause size={12} /> Pause
                  </>
                )}
              </button>
            </>
          ) : (
            <span className="text-[var(--color-muted)]">not tracking</span>
          )}
        </div>
      </div>

      {mode === "week" ? (
        <TimesheetWeek
          key={weekReload}
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
          {/* Insight sentence */}
          {(() => {
            const total = report.total_active_s + report.total_idle_s;
            const pct = total > 0 ? Math.round((report.total_active_s / total) * 100) : 0;
            const top = report.by_app[0];
            return (
              <p className="text-[13px] leading-snug">
                <span className="font-semibold">{isToday ? "Today" : shortDayLabel(date)}:</span>{" "}
                {formatDuration(report.total_active_s)} active
                {total > 0 ? `, ${pct}% productive` : ""}
                {top ? `, top: ${top.key} (${formatDuration(top.seconds)})` : ""}.
              </p>
            );
          })()}

          {/* Daily goal progress */}
          {dailyGoalMin > 0 && <GoalBar activeSec={report.total_active_s} goalMin={dailyGoalMin} />}

          {/* Totals (with Δ vs previous day) */}
          {(() => {
            const total = report.total_active_s + report.total_idle_s;
            const pct = total > 0 ? Math.round((report.total_active_s / total) * 100) : 0;
            let prevPct: number | null = null;
            if (prevDay) {
              const pt = prevDay.active + prevDay.idle;
              prevPct = pt > 0 ? Math.round((prevDay.active / pt) * 100) : 0;
            }
            return (
              <div className="grid grid-cols-2 gap-3">
                <Stat
                  label="Active"
                  value={formatDuration(report.total_active_s)}
                  accent
                  delta={prevDay ? durationDelta(report.total_active_s - prevDay.active) : undefined}
                />
                <Stat label="Idle" value={formatDuration(report.total_idle_s)} />
                <Stat
                  label="Productive"
                  value={`${pct}%`}
                  delta={prevPct != null ? pointDelta(pct - prevPct) : undefined}
                />
                <Stat
                  label="Focus"
                  value={formatDuration(report.longest_focus_s)}
                  sub={`longest · ${report.focus_segments} segment${report.focus_segments === 1 ? "" : "s"}`}
                />
              </div>
            );
          })()}

          {/* Day timeline (24h gantt) — drag a window to assign a project */}
          <Card title="Day timeline">
            <div className="mb-2 flex items-center gap-1.5 text-[11px] text-[var(--color-muted)]">
              <span>Colour</span>
              <div className="flex overflow-hidden rounded-full border border-[var(--color-border)]">
                {(["app", "category", "project"] as const).map((c) => (
                  <button
                    key={c}
                    type="button"
                    onClick={() => setTimelineColorBy(c)}
                    className={
                      "px-2 py-0.5 capitalize " +
                      (timelineColorBy === c
                        ? "bg-[var(--color-accent)] font-semibold text-[var(--color-accent-fg)]"
                        : "hover:bg-[var(--color-surface)]")
                    }
                  >
                    {c}
                  </button>
                ))}
              </div>
            </div>
            <Timeline
              events={report.events}
              dayStart={dayStart}
              dayEnd={dayEnd}
              now={now}
              colorOf={tlColor}
              onSelectRange={(ids, t1, t2) => setProjAssign(ids.length ? { ids, t1, t2 } : null)}
              highlight={projAssign ? { t1: projAssign.t1, t2: projAssign.t2 } : null}
            />
            {legendItems.length > 0 && (
              <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-[10px] text-[var(--color-muted)]">
                {legendItems.map((it) => (
                  <span key={it.key} className="flex items-center gap-1">
                    <span className="h-2 w-2 rounded-full" style={{ backgroundColor: it.color }} />
                    <span className="max-w-[140px] truncate">{it.key}</span>
                  </span>
                ))}
              </div>
            )}
            {projAssign && (
              <ProjectAssign
                pending={projAssign}
                projects={knownProjects}
                onAssigned={() => {
                  setProjAssign(null);
                  refresh();
                  trackDistinctProjects().then(setKnownProjects).catch(() => undefined);
                }}
                onCancel={() => setProjAssign(null)}
              />
            )}
          </Card>

          {/* App + category donuts (stable category colours) */}
          <div className="grid grid-cols-2 gap-3">
            <Card title="By app">
              <Donut
                buckets={report.by_app}
                colors={appColors}
                onPick={(key) => {
                  setListFilter({ kind: "app", key });
                  setEventsView("timeline");
                }}
              />
            </Card>
            <Card title="By category">
              <Donut
                buckets={report.by_category}
                colors={catColors}
                onPick={(key) => {
                  setListFilter({ kind: "category", key });
                  setEventsView("timeline");
                }}
              />
            </Card>
          </div>

          {report.by_host.length > 0 && (
            <Card title="Top hosts">
              <Bars buckets={report.by_host.slice(0, 6)} total={report.total_active_s} />
            </Card>
          )}

          {/* Entries grouped by project (expandable; incl. "(no project)"). */}
          <ProjectEntries events={report.events} now={now} />

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
              {(["slots", "grouped", "timeline"] as const).map((v) => (
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
                  {v === "slots" ? "Slots" : v === "grouped" ? "By app" : "Timeline"}
                </button>
              ))}
            </div>
          </div>

          {/* Slots — the consolidated, bookable blocks (derived, non-destructive). */}
          {eventsView === "slots" && (
            <Card id="slots" title="Slots">
              <SlotsView date={date} />
            </Card>
          )}

          {/* Grouped view — every app expands to its detail (browser → hosts,
              else window titles). */}
          {eventsView === "grouped" &&
            (report.app_breakdown.length === 0 ? (
              <Card id="by-app-grouped" title="By app">
                <p className="text-[12px] text-[var(--color-muted)]">No activity.</p>
              </Card>
            ) : (
              <Card id="by-app-grouped" title={`By app (${report.app_breakdown.length})`}>
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
                            <span
                              className="shrink-0 rounded-full px-1.5 text-[10px] font-medium"
                              style={{
                                color: categoryColor(b.category),
                                backgroundColor: `color-mix(in srgb, ${categoryColor(b.category)} 18%, transparent)`,
                              }}
                            >
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
          <Card id="events" title={`Events (${shownEvents.length}${shownEvents.length !== report.events.length ? ` / ${report.events.length}` : ""})`}>
            {/* Search + active drill-down filter chip */}
            <div className="mb-2 flex items-center gap-2">
              <input
                value={search}
                placeholder="Search app / title / host…"
                onChange={(e) => setSearch(e.target.value)}
                className="min-w-0 flex-1 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[12px]"
              />
              {listFilter && (
                <button
                  type="button"
                  onClick={() => setListFilter(null)}
                  className="md3-press flex shrink-0 items-center gap-1 rounded-full bg-[var(--color-accent)]/15 px-2 py-1 text-[11px] text-[var(--color-accent)]"
                  title="Clear filter"
                >
                  {listFilter.kind}: {listFilter.key} <X size={12} />
                </button>
              )}
            </div>
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
            <div ref={listRef} onPointerDown={onListPointerDown} className="relative flex flex-col">
              {bandRect && (
                <div
                  className="pointer-events-none absolute z-10 rounded border border-[var(--color-accent)] bg-[var(--color-accent)]/15"
                  style={{
                    left: bandRect.left,
                    top: bandRect.top,
                    width: bandRect.width,
                    height: bandRect.height,
                  }}
                />
              )}
              {shownEvents.length === 0 && (
                <p className="py-2 text-[12px] text-[var(--color-muted)]">No matching events.</p>
              )}
              {shownEvents.map((e) => (
                <div key={e.id} className="border-t border-[var(--color-border)]/60 first:border-t-0">
                  <div
                    data-event-id={e.id}
                    onDoubleClick={() => (editingId === e.id ? refresh() : beginEdit(e))}
                    className={
                      "group flex cursor-default items-center gap-2 py-1.5 text-[12px] " +
                      (selected.has(e.id) ? "bg-[var(--color-accent)]/8 -mx-1 rounded px-1" : "")
                    }
                    title="Double-click to edit · drag to select"
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
                    {e.source === "claude" ? (
                      <Sparkles size={11} className="shrink-0 text-[var(--color-accent)]" />
                    ) : (
                      <span
                        className="h-2.5 w-2.5 shrink-0 rounded-full"
                        style={{ backgroundColor: e.is_idle ? "var(--color-muted)" : appColors[e.app_name] ?? paletteColor(0) }}
                      />
                    )}
                    <span className="w-[120px] shrink-0 truncate font-medium">{e.app_name}</span>
                    <span className="min-w-0 flex-1 truncate text-[var(--color-muted)]">
                      {e.category ? `[${e.category}] ` : ""}
                      {e.source === "claude" ? (e.project ?? "") : (e.host ?? e.window_title ?? "")}
                    </span>
                    {e.source === "claude" && (
                      <span className="shrink-0 rounded-full bg-[var(--color-accent)]/15 px-1.5 text-[10px] text-[var(--color-accent)]">
                        claude
                      </span>
                    )}
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
              : " · double-click to edit · drag to select (⇧ adds)"}
          </p>
        </div>
      )}
      <ProjectExportFooter />
      <BridgeFooter />
    </div>
  );
}

/** Collapsible "Browser extension" footer — shows the loopback bridge port +
 *  token (to paste into the extension's options) + a regenerate button. */
/** Collapsible "Project export" footer — pick a date range, export per-project
 *  CSV/HTML for a client ("when · how long · on what"). */
function ProjectExportFooter() {
  const monthStart = () => {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-01`;
  };
  const [from, setFrom] = useState(monthStart());
  const [to, setTo] = useState(localDateStr());
  const [project, setProject] = useState(""); // "" = all
  const [detail, setDetail] = useState<"summary" | "daily" | "full">("full");
  const [projects, setProjects] = useState<string[]>([]);
  const [open, setOpen] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);
  useEffect(() => {
    if (open) trackDistinctProjects().then(setProjects).catch(() => undefined);
  }, [open]);
  const run = (fmt: "csv" | "html") => {
    setSaved(null);
    void trackExportProjects(fmt, from, to, project, detail)
      .then((p) => setSaved(p))
      .catch((e) => console.error("project export failed", e));
  };
  const selCls =
    "rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-1.5 py-0.5";
  return (
    <details
      className="shrink-0 border-t border-[var(--color-border)] px-4 py-2 text-[12px]"
      onToggle={(e) => setOpen((e.target as HTMLDetailsElement).open)}
    >
      <summary className="cursor-pointer text-[var(--color-muted)]">Project export (for a client)</summary>
      <div className="mt-2 flex flex-col gap-2">
        <p className="text-[var(--color-muted)]">
          Per-project list of when · how long · on what. Pick a single client so others
          aren't exposed, and a detail level. Assign time to projects by dragging a window
          on the day timeline.
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <label className="flex items-center gap-1">
            Project
            <select value={project} onChange={(e) => setProject(e.target.value)} className={selCls}>
              <option value="">All projects</option>
              {projects.map((p) => (
                <option key={p} value={p}>
                  {p}
                </option>
              ))}
            </select>
          </label>
          <label className="flex items-center gap-1">
            Detail
            <select
              value={detail}
              onChange={(e) => setDetail(e.target.value as "summary" | "daily" | "full")}
              className={selCls}
            >
              <option value="full">Full entries</option>
              <option value="daily">Per day</option>
              <option value="summary">Summary (totals)</option>
            </select>
          </label>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <label className="flex items-center gap-1">
            From
            <input
              type="date"
              value={from}
              max={to}
              onChange={(e) => e.target.value && setFrom(e.target.value)}
              className={selCls + " tabular-nums"}
            />
          </label>
          <label className="flex items-center gap-1">
            To
            <input
              type="date"
              value={to}
              max={localDateStr()}
              onChange={(e) => e.target.value && setTo(e.target.value)}
              className={selCls + " tabular-nums"}
            />
          </label>
          <button
            type="button"
            onClick={() => run("html")}
            className="md3-press flex items-center gap-1 rounded-full border border-[var(--color-border)] px-2.5 py-1 hover:bg-[var(--color-surface)]"
          >
            <Download size={13} /> HTML
          </button>
          <button
            type="button"
            onClick={() => run("csv")}
            className="md3-press flex items-center gap-1 rounded-full border border-[var(--color-border)] px-2.5 py-1 hover:bg-[var(--color-surface)]"
          >
            <Download size={13} /> CSV
          </button>
          {saved && <span className="text-[var(--color-accent)]">Saved ✓</span>}
        </div>
      </div>
    </details>
  );
}

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

/** Popover under the Gantt: assign the dragged time window's events to a project. */
function ProjectAssign({
  pending,
  projects,
  onAssigned,
  onCancel,
}: {
  pending: { ids: number[]; t1: number; t2: number };
  projects: string[];
  onAssigned: () => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState("");
  const assign = () => {
    const v = value.trim();
    if (!v) return;
    void trackSetProject(pending.ids, v).then(onAssigned).catch(() => undefined);
  };
  const secs = Math.round((pending.t2 - pending.t1) / 1000);
  return (
    <div
      className="mt-2 flex flex-wrap items-center gap-2 rounded-lg border border-[var(--color-accent)]/40 bg-[var(--color-surface)] p-2 text-[12px]"
      onKeyDown={(e) => {
        if (e.key === "Enter") assign();
        else if (e.key === "Escape") onCancel();
      }}
    >
      <span className="text-[var(--color-muted)]">
        {formatClock(pending.t1)}–{formatClock(pending.t2)} · {pending.ids.length} entr
        {pending.ids.length === 1 ? "y" : "ies"} · {formatDuration(secs)}
      </span>
      <input
        autoFocus
        list="proj-assign-cats"
        value={value}
        placeholder="Project / client"
        onChange={(e) => setValue(e.target.value)}
        className="min-w-0 flex-1 rounded border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5"
      />
      <datalist id="proj-assign-cats">
        {projects.map((p) => (
          <option key={p} value={p} />
        ))}
      </datalist>
      <button
        type="button"
        onClick={onCancel}
        className="md3-press rounded-full border border-[var(--color-border)] px-2.5 py-0.5"
      >
        Cancel
      </button>
      <button
        type="button"
        onClick={assign}
        disabled={pending.ids.length === 0 || value.trim() === ""}
        className="md3-press rounded-full bg-[var(--color-accent)] px-3 py-0.5 font-semibold text-[var(--color-accent-fg)] disabled:opacity-40"
      >
        Assign
      </button>
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

/** Active entries grouped by project (expandable). Untagged active time is its
 *  own "(no project)" group so it's easy to find + assign. */
function ProjectEntries({ events, now }: { events: TrackEvent[]; now: number }) {
  const dur = (e: TrackEvent) => e.duration_s ?? Math.max(0, Math.floor((now - e.started_at) / 1000));
  const groups = useMemo(() => {
    const m = new Map<string, { seconds: number; events: TrackEvent[] }>();
    for (const e of events) {
      if (e.is_idle) continue;
      const key = e.project && e.project !== "" ? e.project : NO_PROJECT;
      const g = m.get(key) ?? { seconds: 0, events: [] };
      g.seconds += dur(e);
      g.events.push(e);
      m.set(key, g);
    }
    return [...m.entries()]
      .map(([project, g]) => ({ project, ...g }))
      .sort((a, b) => b.seconds - a.seconds);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [events, now]);

  if (groups.length === 0) return null;
  return (
    <Card title="By project">
      <div className="flex flex-col gap-1">
        {groups.map((g) => (
          <details key={g.project} className="ts-app-group">
            <summary className="md3-press flex w-full cursor-pointer list-none items-center gap-2 rounded-lg px-1 py-1 text-[12px] hover:bg-[var(--color-surface)]">
              <ChevronRight size={14} className="ts-chevron shrink-0 text-[var(--color-muted)] transition-transform" />
              <span className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ backgroundColor: projectColor(g.project) }} />
              <span className="min-w-0 flex-1 truncate text-left font-medium">{g.project}</span>
              <span className="shrink-0 text-[var(--color-muted)]">
                {g.events.length} entr{g.events.length === 1 ? "y" : "ies"}
              </span>
              <span className="w-[58px] shrink-0 text-right tabular-nums">{formatDuration(g.seconds)}</span>
            </summary>
            <div className="ml-6 flex flex-col border-l border-[var(--color-border)] pl-2">
              {g.events.map((e) => (
                <div key={e.id} className="flex items-center gap-2 py-0.5 text-[12px]" title={e.window_title ?? e.app_name}>
                  <span className="w-[92px] shrink-0 tabular-nums text-[var(--color-muted)]">
                    {formatClock(e.started_at)}–{e.ended_at ? formatClock(e.ended_at) : "…"}
                  </span>
                  <span className="min-w-0 flex-1 truncate">
                    {e.app_name}
                    {e.window_title || e.host ? ` — ${e.window_title ?? e.host}` : ""}
                  </span>
                  <span className="w-[58px] shrink-0 text-right tabular-nums text-[var(--color-muted)]">
                    {formatDuration(dur(e))}
                  </span>
                </div>
              ))}
            </div>
          </details>
        ))}
      </div>
    </Card>
  );
}

// ── Small presentational pieces ──────────────────────────────────────────────

interface Delta {
  text: string;
  positive: boolean;
}
/** Δ chip for a duration change (seconds); positive = more (green). */
function durationDelta(diffSec: number): Delta | undefined {
  if (Math.abs(diffSec) < 60) return undefined;
  return { text: `${diffSec >= 0 ? "+" : "−"}${formatDuration(Math.abs(diffSec))}`, positive: diffSec >= 0 };
}
/** Δ chip for a percentage-point change. */
function pointDelta(diffPts: number): Delta | undefined {
  if (diffPts === 0) return undefined;
  return { text: `${diffPts >= 0 ? "+" : "−"}${Math.abs(diffPts)}%`, positive: diffPts >= 0 };
}

function Stat({
  label,
  value,
  accent,
  delta,
  sub,
}: {
  label: string;
  value: string;
  accent?: boolean;
  delta?: Delta;
  sub?: string;
}) {
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3">
      <div className="flex items-center justify-between">
        <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">{label}</div>
        {delta && (
          <span
            className={
              "text-[11px] font-semibold tabular-nums " +
              (delta.positive ? "text-emerald-400" : "text-rose-400")
            }
            title="vs. previous day"
          >
            {delta.positive ? "▲" : "▼"} {delta.text}
          </span>
        )}
      </div>
      <div
        className={
          "mt-0.5 text-[20px] font-bold tabular-nums " +
          (accent ? "text-[var(--color-accent)]" : "")
        }
      >
        {value}
      </div>
      {sub && <div className="text-[10px] text-[var(--color-muted)]">{sub}</div>}
    </div>
  );
}

/** Daily focus-goal progress bar. */
function GoalBar({ activeSec, goalMin }: { activeSec: number; goalMin: number }) {
  const goalSec = goalMin * 60;
  const pct = goalSec > 0 ? Math.min(100, Math.round((activeSec / goalSec) * 100)) : 0;
  const remain = Math.max(0, goalSec - activeSec);
  const done = activeSec >= goalSec;
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3">
      <div className="mb-1 flex items-center justify-between text-[12px]">
        <span className="font-medium text-[var(--color-muted)]">Daily goal</span>
        <span className="tabular-nums">
          {formatDuration(activeSec)} / {formatDuration(goalSec)}
          {done ? (
            <span className="ml-1 text-emerald-400">reached ✓</span>
          ) : (
            <span className="ml-1 text-[var(--color-muted)]">· {formatDuration(remain)} to go</span>
          )}
        </span>
      </div>
      <div className="h-2 w-full overflow-hidden rounded-full bg-[var(--color-surface)]">
        <div
          className="h-full rounded-full"
          style={{ width: `${pct}%`, backgroundColor: done ? "#34d399" : "var(--color-accent)" }}
        />
      </div>
    </div>
  );
}

/** A collapsible section. The collapsed state persists in localStorage keyed by
 *  `id` (or the title), so it survives popup close/reopen + tab switches. */
function Card({
  title,
  id,
  children,
}: {
  title: string;
  /** Stable key for persistence (use when the title is dynamic, e.g. a count). */
  id?: string;
  children: React.ReactNode;
}) {
  const key = `ts-collapse:${id ?? title}`;
  const [collapsed, setCollapsed] = useState<boolean>(() => {
    try {
      return localStorage.getItem(key) === "1";
    } catch {
      return false;
    }
  });
  const toggle = () =>
    setCollapsed((c) => {
      const n = !c;
      try {
        localStorage.setItem(key, n ? "1" : "0");
      } catch {
        /* ignore */
      }
      return n;
    });
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3">
      <button
        type="button"
        onClick={toggle}
        className="md3-press -m-1 flex w-[calc(100%+0.5rem)] items-center gap-1.5 rounded-lg p-1 text-left text-[12px] font-medium text-[var(--color-muted)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]"
        title={collapsed ? "Expand section" : "Collapse section"}
        aria-expanded={!collapsed}
      >
        <ChevronRight
          size={14}
          className={"shrink-0 transition-transform " + (collapsed ? "" : "rotate-90")}
        />
        <span className="min-w-0 flex-1 truncate">{title}</span>
      </button>
      {!collapsed && <div className="mt-2">{children}</div>}
    </div>
  );
}

function Timeline({
  events,
  dayStart,
  dayEnd,
  now,
  colorOf,
  onSelectRange,
  highlight,
}: {
  events: TrackEvent[];
  dayStart: number;
  /** Next local midnight (DST-correct; a transition day is 23/25 h). */
  dayEnd: number;
  now: number;
  colorOf: (e: TrackEvent) => string;
  /** Called on a drag-release with the active events overlapping [t1,t2]. */
  onSelectRange?: (ids: number[], t1: number, t2: number) => void;
  /** A committed window to keep highlighted (while the assign popover is open). */
  highlight?: { t1: number; t2: number } | null;
}) {
  const barRef = useRef<HTMLDivElement>(null);
  const [dragPx, setDragPx] = useState<{ x1: number; x2: number } | null>(null);

  const onPointerDown = (e: React.PointerEvent) => {
    if (!onSelectRange || e.button !== 0) return;
    const bar = barRef.current;
    if (!bar) return;
    const rect = bar.getBoundingClientRect();
    const startX = Math.max(0, Math.min(rect.width, e.clientX - rect.left));
    let moved = false;
    const onMove = (ev: PointerEvent) => {
      const cur = Math.max(0, Math.min(rect.width, ev.clientX - rect.left));
      if (!moved && Math.abs(cur - startX) < 4) return;
      moved = true;
      document.body.style.userSelect = "none";
      setDragPx({ x1: Math.min(startX, cur), x2: Math.max(startX, cur) });
    };
    const onUp = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      document.body.style.userSelect = "";
      setDragPx(null);
      if (!moved) return;
      const endX = Math.max(0, Math.min(rect.width, ev.clientX - rect.left));
      const f1 = Math.min(startX, endX) / rect.width;
      const f2 = Math.max(startX, endX) / rect.width;
      const span = dayEnd - dayStart;
      const t1 = dayStart + f1 * span;
      const t2 = dayStart + f2 * span;
      const ids = events
        .filter(
          (ev2) =>
            !ev2.is_idle &&
            ev2.source !== "claude" &&
            ev2.started_at < t2 &&
            (ev2.ended_at ?? now) > t1,
        )
        .map((ev2) => ev2.id);
      onSelectRange(ids, t1, t2);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const daySpan = dayEnd - dayStart;
  const hiLeft = highlight ? Math.max(0, ((highlight.t1 - dayStart) / daySpan) * 100) : 0;
  const hiWidth = highlight
    ? Math.min(100 - hiLeft, ((highlight.t2 - highlight.t1) / daySpan) * 100)
    : 0;

  return (
    <div>
      <div
        ref={barRef}
        onPointerDown={onPointerDown}
        className={
          "relative h-7 w-full select-none overflow-hidden rounded-md bg-[var(--color-surface)] " +
          (onSelectRange ? "cursor-crosshair" : "")
        }
        title={onSelectRange ? "Drag to select a time window → assign a project" : undefined}
      >
        {events.map((e) => {
          const band = timelineBand(e.started_at, e.ended_at, dayStart, now, dayEnd);
          if (!band) return null;
          return (
            <div
              key={e.id}
              title={`${e.app_name} · ${formatClock(e.started_at)}`}
              className="absolute top-0 h-full"
              style={{
                left: `${band.leftPct}%`,
                width: `${Math.max(0.3, band.widthPct)}%`,
                backgroundColor: e.is_idle ? "var(--color-border)" : colorOf(e),
                opacity: e.is_idle ? 0.5 : 0.9,
              }}
            />
          );
        })}
        {/* Committed window (popover open). */}
        {highlight && hiWidth > 0 && (
          <div
            className="pointer-events-none absolute top-0 h-full border-x-2 border-[var(--color-accent)] bg-[var(--color-accent)]/25"
            style={{ left: `${hiLeft}%`, width: `${hiWidth}%` }}
          />
        )}
        {/* Transient drag rectangle. */}
        {dragPx && (
          <div
            className="pointer-events-none absolute top-0 h-full border-x-2 border-[var(--color-accent)] bg-[var(--color-accent)]/30"
            style={{ left: dragPx.x1, width: dragPx.x2 - dragPx.x1 }}
          />
        )}
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
  onPick,
}: {
  buckets: { key: string; seconds: number }[];
  colors: Record<string, string>;
  /** Click a legend entry to drill down to it (excludes the "Other" bucket). */
  onPick?: (key: string) => void;
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
        {entries.map((e, i) => {
          const pickable = onPick && e.key !== "Other";
          return (
            <div
              key={e.key}
              onClick={pickable ? () => onPick(e.key) : undefined}
              className={
                "flex items-center gap-1.5 rounded text-[11px] " +
                (pickable ? "cursor-pointer hover:bg-[var(--color-surface)]" : "")
              }
              title={pickable ? `Show only ${e.key}` : undefined}
            >
              <span
                className="h-2.5 w-2.5 shrink-0 rounded-full"
                style={{ backgroundColor: e.key === "Other" ? "var(--color-muted)" : colors[e.key] ?? paletteColor(i) }}
              />
              <span className="min-w-0 flex-1 truncate">{e.key}</span>
              <span className="shrink-0 tabular-nums text-[var(--color-muted)]">
                {formatDuration(e.seconds)}
              </span>
            </div>
          );
        })}
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
