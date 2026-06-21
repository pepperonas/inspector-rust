import { useEffect, useState } from "react";
import { ChevronRight } from "lucide-react";
import { trackGetRange, type RangeReport } from "../lib/ipc";
import {
  categoryColor,
  formatDuration,
  shiftWeek,
  shortDayLabel,
  weekBounds,
} from "../lib/timesheet";

/**
 * Week overview: per-day stacked-by-category bars, an hours × days activity
 * heatmap, the week's category/app/project breakdowns, a productive-vs-idle
 * ratio, and Δ vs. the previous week. `date` is any day in the week (Mon–Sun is
 * derived). Clicking a day jumps to the day view.
 */
export function TimesheetWeek({
  date,
  onPickDay,
}: {
  date: string;
  onPickDay: (date: string) => void;
}) {
  const [report, setReport] = useState<RangeReport | null>(null);
  const [prev, setPrev] = useState<RangeReport | null>(null);
  useEffect(() => {
    const { from, to } = weekBounds(date);
    trackGetRange(from, to).then(setReport).catch(() => setReport(null));
    const pw = weekBounds(shiftWeek(date, -1));
    trackGetRange(pw.from, pw.to).then(setPrev).catch(() => setPrev(null));
  }, [date]);

  if (!report) {
    return <div className="flex flex-1 items-center justify-center text-[var(--color-muted)]">Loading…</div>;
  }

  const total = report.total_active_s + report.total_idle_s;
  const productivePct = total > 0 ? Math.round((report.total_active_s / total) * 100) : 0;
  let prevPct: number | null = null;
  if (prev) {
    const pt = prev.total_active_s + prev.total_idle_s;
    prevPct = pt > 0 ? Math.round((prev.total_active_s / pt) * 100) : 0;
  }
  const maxDay = Math.max(1, ...report.days.map((d) => d.active_s + d.idle_s));
  const maxHour = Math.max(1, ...report.days.flatMap((d) => d.hours));

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
      <div className="grid grid-cols-3 gap-3">
        <Stat
          label="Active (week)"
          value={formatDuration(report.total_active_s)}
          accent
          delta={prev ? durationDelta(report.total_active_s - prev.total_active_s) : undefined}
        />
        <Stat label="Idle (week)" value={formatDuration(report.total_idle_s)} />
        <Stat
          label="Productive"
          value={`${productivePct}%`}
          delta={prevPct != null ? pointDelta(productivePct - prevPct) : undefined}
        />
      </div>

      <Card title="Per day">
        <div className="flex flex-col gap-1.5">
          {report.days.map((d) => {
            return (
              <button
                key={d.date}
                type="button"
                onClick={() => onPickDay(d.date)}
                className="md3-press flex items-center gap-2 rounded-lg px-1 py-1 text-left text-[12px] hover:bg-[var(--color-surface)]"
                title="Open this day"
              >
                <span className="w-[96px] shrink-0 text-[var(--color-muted)]">{shortDayLabel(d.date)}</span>
                <div className="flex h-3 min-w-0 flex-1 overflow-hidden rounded-full bg-[var(--color-surface)]">
                  {/* Stacked by category, then idle (muted), scaled to the busiest day. */}
                  {d.by_category.map((c) => (
                    <div
                      key={c.key}
                      className="h-full"
                      title={`${c.key} · ${formatDuration(c.seconds)}`}
                      style={{ width: `${(c.seconds / maxDay) * 100}%`, backgroundColor: categoryColor(c.key) }}
                    />
                  ))}
                  <div
                    className="h-full bg-[var(--color-border)]"
                    style={{ width: `${(d.idle_s / maxDay) * 100}%` }}
                    title={`Idle · ${formatDuration(d.idle_s)}`}
                  />
                </div>
                <span className="w-[58px] shrink-0 text-right tabular-nums">{formatDuration(d.active_s)}</span>
              </button>
            );
          })}
        </div>
        <p className="mt-1 text-[10px] text-[var(--color-muted)]">
          stacked by category · <span className="inline-block h-2 w-2 rounded-full bg-[var(--color-border)]" /> idle ·
          click a day to open it
        </p>
      </Card>

      <Card title="Activity heatmap">
        <Heatmap days={report.days} maxHour={maxHour} />
      </Card>

      <div className="grid grid-cols-2 gap-3">
        <Card title="By category (week)">
          <Bars buckets={report.by_category} total={report.total_active_s} colorByKey={categoryColor} />
        </Card>
        <Card title="By app (week)">
          <Bars buckets={report.by_app.slice(0, 8)} total={report.total_active_s} />
        </Card>
      </div>

      {report.by_project.length > 0 && (
        <Card title="By project (week)">
          <Bars buckets={report.by_project} total={report.total_active_s} />
        </Card>
      )}
    </div>
  );
}

/** Hours × days grid; cell opacity ∝ active time that hour. */
function Heatmap({
  days,
  maxHour,
}: {
  days: { date: string; hours: number[] }[];
  maxHour: number;
}) {
  return (
    <div className="flex flex-col gap-0.5">
      {days.map((d) => (
        <div key={d.date} className="flex items-center gap-1">
          <span className="w-[84px] shrink-0 text-[10px] text-[var(--color-muted)]">{shortDayLabel(d.date)}</span>
          <div className="grid min-w-0 flex-1" style={{ gridTemplateColumns: "repeat(24, 1fr)", gap: "2px" }}>
            {d.hours.map((v, h) => (
              <div
                key={h}
                title={`${shortDayLabel(d.date)} ${String(h).padStart(2, "0")}:00 · ${formatDuration(v)}`}
                className="h-3.5 rounded-[2px]"
                style={{
                  backgroundColor: v > 0 ? "var(--color-accent)" : "var(--color-surface)",
                  opacity: v > 0 ? 0.15 + 0.85 * (v / maxHour) : 1,
                }}
              />
            ))}
          </div>
        </div>
      ))}
      <div className="flex items-center gap-1">
        <span className="w-[84px] shrink-0" />
        <div className="grid min-w-0 flex-1 text-[9px] tabular-nums text-[var(--color-muted)]" style={{ gridTemplateColumns: "repeat(24, 1fr)" }}>
          {Array.from({ length: 24 }, (_, h) => (
            <span key={h} className="text-center">
              {h % 6 === 0 ? h : ""}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}

// ── Small pieces ─────────────────────────────────────────────────────────────

interface Delta {
  text: string;
  positive: boolean;
}
function durationDelta(diffSec: number): Delta | undefined {
  if (Math.abs(diffSec) < 60) return undefined;
  return { text: `${diffSec >= 0 ? "+" : "−"}${formatDuration(Math.abs(diffSec))}`, positive: diffSec >= 0 };
}
function pointDelta(diffPts: number): Delta | undefined {
  if (diffPts === 0) return undefined;
  return { text: `${diffPts >= 0 ? "+" : "−"}${Math.abs(diffPts)}%`, positive: diffPts >= 0 };
}

function Stat({
  label,
  value,
  accent,
  delta,
}: {
  label: string;
  value: string;
  accent?: boolean;
  delta?: Delta;
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
            title="vs. previous week"
          >
            {delta.positive ? "▲" : "▼"} {delta.text}
          </span>
        )}
      </div>
      <div className={"mt-0.5 text-[20px] font-bold tabular-nums " + (accent ? "text-[var(--color-accent)]" : "")}>
        {value}
      </div>
    </div>
  );
}

/** Collapsible section; collapsed state persists in localStorage by title. */
function Card({ title, children }: { title: string; children: React.ReactNode }) {
  const key = `ts-collapse:${title}`;
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
        className="md3-press flex w-full items-center gap-1 text-left text-[12px] font-medium text-[var(--color-muted)]"
        title={collapsed ? "Expand" : "Collapse"}
      >
        <ChevronRight
          size={13}
          className={"shrink-0 transition-transform " + (collapsed ? "" : "rotate-90")}
        />
        <span className="min-w-0 flex-1 truncate">{title}</span>
      </button>
      {!collapsed && <div className="mt-2">{children}</div>}
    </div>
  );
}

function Bars({
  buckets,
  total,
  colorByKey,
}: {
  buckets: { key: string; seconds: number }[];
  total: number;
  colorByKey?: (key: string) => string;
}) {
  if (buckets.length === 0) return <p className="text-[12px] text-[var(--color-muted)]">No data.</p>;
  const max = Math.max(1, ...buckets.map((b) => b.seconds));
  return (
    <div className="flex flex-col gap-1.5">
      {buckets.map((b) => (
        <div key={b.key} className="text-[11px]">
          <div className="mb-0.5 flex items-center gap-1.5">
            {colorByKey && (
              <span className="h-2 w-2 shrink-0 rounded-full" style={{ backgroundColor: colorByKey(b.key) }} />
            )}
            <span className="min-w-0 flex-1 truncate pr-2">{b.key}</span>
            <span className="shrink-0 tabular-nums text-[var(--color-muted)]">
              {formatDuration(b.seconds)}
              {total > 0 ? ` · ${Math.round((b.seconds / total) * 100)}%` : ""}
            </span>
          </div>
          <div className="h-1.5 w-full overflow-hidden rounded-full bg-[var(--color-surface)]">
            <div
              className="h-full rounded-full"
              style={{
                width: `${(b.seconds / max) * 100}%`,
                backgroundColor: colorByKey ? colorByKey(b.key) : "var(--color-accent)",
              }}
            />
          </div>
        </div>
      ))}
    </div>
  );
}
