import { useEffect, useState } from "react";
import { trackGetRange, type RangeReport } from "../lib/ipc";
import { formatDuration, shortDayLabel, weekBounds } from "../lib/timesheet";

/**
 * Week overview for the Timesheet tab: per-day active/idle bars + the week's
 * category & project breakdowns + a productive-vs-idle ratio. `date` is any day
 * in the week (Mon–Sun is derived). Clicking a day jumps back to the day view.
 */
export function TimesheetWeek({
  date,
  onPickDay,
}: {
  date: string;
  onPickDay: (date: string) => void;
}) {
  const [report, setReport] = useState<RangeReport | null>(null);
  useEffect(() => {
    const { from, to } = weekBounds(date);
    trackGetRange(from, to)
      .then(setReport)
      .catch(() => setReport(null));
  }, [date]);

  if (!report) {
    return <div className="flex flex-1 items-center justify-center text-[var(--color-muted)]">Loading…</div>;
  }

  const total = report.total_active_s + report.total_idle_s;
  const productivePct = total > 0 ? Math.round((report.total_active_s / total) * 100) : 0;
  const maxDay = Math.max(1, ...report.days.map((d) => d.active_s + d.idle_s));

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
      <div className="grid grid-cols-3 gap-3">
        <Stat label="Active (week)" value={formatDuration(report.total_active_s)} accent />
        <Stat label="Idle (week)" value={formatDuration(report.total_idle_s)} />
        <Stat label="Productive" value={`${productivePct}%`} />
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
                  <div
                    className="h-full bg-[var(--color-accent)]"
                    style={{ width: `${(d.active_s / maxDay) * 100}%` }}
                  />
                  <div
                    className="h-full bg-[var(--color-border)]"
                    style={{ width: `${(d.idle_s / maxDay) * 100}%` }}
                  />
                </div>
                <span className="w-[58px] shrink-0 text-right tabular-nums">{formatDuration(d.active_s)}</span>
              </button>
            );
          })}
        </div>
        <p className="mt-1 text-[10px] text-[var(--color-muted)]">
          <span className="inline-block h-2 w-2 rounded-full bg-[var(--color-accent)]" /> active ·{" "}
          <span className="inline-block h-2 w-2 rounded-full bg-[var(--color-border)]" /> idle · click a day to open it
        </p>
      </Card>

      <div className="grid grid-cols-2 gap-3">
        <Card title="By category (week)">
          <Bars buckets={report.by_category} total={report.total_active_s} />
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

function Stat({ label, value, accent }: { label: string; value: string; accent?: boolean }) {
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3">
      <div className="text-[11px] uppercase tracking-wide text-[var(--color-muted)]">{label}</div>
      <div className={"mt-0.5 text-[20px] font-bold tabular-nums " + (accent ? "text-[var(--color-accent)]" : "")}>
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

function Bars({ buckets, total }: { buckets: { key: string; seconds: number }[]; total: number }) {
  if (buckets.length === 0) return <p className="text-[12px] text-[var(--color-muted)]">No data.</p>;
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
            <div className="h-full rounded-full bg-[var(--color-accent)]" style={{ width: `${(b.seconds / max) * 100}%` }} />
          </div>
        </div>
      ))}
    </div>
  );
}
