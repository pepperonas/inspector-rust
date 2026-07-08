/**
 * `calendar` — month-view calendar in the right preview column (same inline
 * family as sound/stats/uptime). Shows directly while the command is typed;
 * Enter hands the arrow keys to it: ←/→ month, ↑/↓ year, T/Home today, Esc
 * exits. Click a day for a full-date readout with the distance from today.
 * All date math is the pure, unit-tested `lib/calendar.ts`.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import {
  CalendarDays,
  ChevronLeft,
  ChevronRight,
  ChevronsLeft,
  ChevronsRight,
} from "lucide-react";
import { addMonths, dayDelta, monthMatrix, parseCalendarArg } from "../lib/calendar";

interface Ymd {
  year: number;
  month: number;
  day: number;
}

function todayYmd(): Ymd {
  const n = new Date();
  return { year: n.getFullYear(), month: n.getMonth(), day: n.getDate() };
}

/** "in 3 days" / "5 days ago" / "today" (+ a weeks hint for larger spans). */
function deltaLabel(days: number): string {
  if (days === 0) return "today";
  const abs = Math.abs(days);
  const base =
    days > 0
      ? `in ${abs} day${abs === 1 ? "" : "s"}`
      : `${abs} day${abs === 1 ? "" : "s"} ago`;
  if (abs >= 14) {
    const weeks = Math.round(abs / 7);
    return `${base} (≈${weeks} wk)`;
  }
  return base;
}

const MONTH_FMT = new Intl.DateTimeFormat(undefined, { month: "long", year: "numeric" });
const READOUT_FMT = new Intl.DateTimeFormat(undefined, {
  weekday: "long",
  day: "numeric",
  month: "long",
  year: "numeric",
});
/** Mon-first weekday header labels, localized (2026-06-01 was a Monday). */
const WEEKDAYS: string[] = (() => {
  const fmt = new Intl.DateTimeFormat(undefined, { weekday: "short" });
  return Array.from({ length: 7 }, (_, i) => fmt.format(new Date(2026, 5, 1 + i)));
})();

export function CalendarPanel({
  focused,
  arg,
  onExit,
  onInteract,
}: {
  focused: boolean;
  /** Live command argument (`calendar märz 1990`) — jumps the view while typing. */
  arg: string;
  onExit: () => void;
  /** Called after a mouse interaction so the parent keeps the search field focused. */
  onInteract?: () => void;
}) {
  const today = useMemo(() => todayYmd(), []);
  const [view, setView] = useState<{ year: number; month: number }>({
    year: today.year,
    month: today.month,
  });
  const [selected, setSelected] = useState<Ymd | null>(null);
  // Slide direction of the last month change (for the directional entrance).
  const [dir, setDir] = useState<"left" | "right">("right");
  const lastArg = useRef<string>("");

  // A typed argument (`calendar märz 1990`) drives the view live; clearing it
  // leaves the view where it was.
  useEffect(() => {
    if (arg === lastArg.current) return;
    lastArg.current = arg;
    const target = parseCalendarArg(arg, today.year);
    if (target) {
      setDir("right");
      setView(target);
    }
  }, [arg, today.year]);

  const go = (deltaMonths: number) => {
    setDir(deltaMonths > 0 ? "right" : "left");
    setView((v) => addMonths(v.year, v.month, deltaMonths));
  };
  const goToday = () => {
    setDir("right");
    setView({ year: today.year, month: today.month });
    setSelected(today);
  };

  // Keyboard while focused: ←/→ month, ↑/↓ year, PgUp/PgDn month, T/Home today.
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      switch (e.key) {
        case "ArrowLeft":
          go(-1);
          break;
        case "ArrowRight":
          go(1);
          break;
        case "ArrowUp":
          go(-12);
          break;
        case "ArrowDown":
          go(12);
          break;
        case "PageUp":
          go(-1);
          break;
        case "PageDown":
          go(1);
          break;
        case "Home":
        case "t":
        case "T":
          goToday();
          break;
        case "Escape":
          onExit();
          break;
        default:
          return;
      }
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focused, onExit]);

  const rows = useMemo(() => monthMatrix(view.year, view.month), [view]);
  const monthLabel = MONTH_FMT.format(new Date(view.year, view.month, 1));
  const readout = selected
    ? {
        text: READOUT_FMT.format(new Date(selected.year, selected.month, selected.day)),
        delta: deltaLabel(dayDelta(today, selected)),
      }
    : null;

  const navBtn =
    "flex h-7 w-7 items-center justify-center rounded-full text-[var(--color-muted)] " +
    "transition-colors hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)] md3-press";

  return (
    <div
      className="flex h-full flex-col gap-2 overflow-y-auto p-4 text-[var(--color-fg)]"
      onPointerUp={() => onInteract?.()}
    >
      {/* Header: month + year, nav controls */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2 text-[13px] font-medium">
          <CalendarDays size={15} className="text-[var(--color-accent)]" />
          <span
            key={`${view.year}-${view.month}`}
            className={dir === "right" ? "md3-opener-in-right" : "md3-opener-in-left"}
          >
            {monthLabel}
          </span>
        </div>
        <div className="flex items-center gap-0.5">
          <button
            type="button"
            title="Previous year (↑)"
            aria-label="Previous year"
            className={navBtn}
            onClick={() => go(-12)}
          >
            <ChevronsLeft size={14} />
          </button>
          <button
            type="button"
            title="Previous month (←)"
            aria-label="Previous month"
            className={navBtn}
            onClick={() => go(-1)}
          >
            <ChevronLeft size={14} />
          </button>
          <button
            type="button"
            title="Jump to today (T)"
            onClick={goToday}
            className="rounded-full px-2.5 py-1 text-[11px] font-medium text-[var(--color-accent)] transition-colors hover:bg-[var(--color-surface)] md3-press"
          >
            Today
          </button>
          <button
            type="button"
            title="Next month (→)"
            aria-label="Next month"
            className={navBtn}
            onClick={() => go(1)}
          >
            <ChevronRight size={14} />
          </button>
          <button
            type="button"
            title="Next year (↓)"
            aria-label="Next year"
            className={navBtn}
            onClick={() => go(12)}
          >
            <ChevronsRight size={14} />
          </button>
        </div>
      </div>

      {/* Weekday header (Mon-first) + ISO-week gutter */}
      <div className="grid grid-cols-[24px_repeat(7,1fr)] gap-x-1 text-center">
        <span className="text-[9px] font-medium uppercase text-[var(--color-muted)] opacity-60 self-end">
          wk
        </span>
        {WEEKDAYS.map((w, i) => (
          <span
            key={w}
            className={
              "pb-0.5 text-[10px] font-medium " +
              (i >= 5
                ? "text-[var(--color-accent)] opacity-80"
                : "text-[var(--color-muted)]")
            }
          >
            {w}
          </span>
        ))}
      </div>

      {/* Month grid — keyed on the month so the directional slide replays */}
      <div
        key={`grid-${view.year}-${view.month}`}
        className={
          "grid grid-cols-[24px_repeat(7,1fr)] gap-x-1 gap-y-1 " +
          (dir === "right" ? "md3-opener-in-right" : "md3-opener-in-left")
        }
      >
        {rows.map((row) => (
          <div key={row.isoWeek + "-" + row.days[0].day} className="contents">
            <span className="flex items-center justify-center text-[9px] tabular-nums text-[var(--color-muted)] opacity-60">
              {row.isoWeek}
            </span>
            {row.days.map((c) => {
              const isToday =
                c.year === today.year && c.month === today.month && c.day === today.day;
              const isSelected =
                selected != null &&
                c.year === selected.year &&
                c.month === selected.month &&
                c.day === selected.day;
              return (
                <button
                  type="button"
                  key={`${c.year}-${c.month}-${c.day}`}
                  onClick={() =>
                    setSelected({ year: c.year, month: c.month, day: c.day })
                  }
                  className={
                    "flex h-8 items-center justify-center rounded-lg text-[12px] tabular-nums transition-colors md3-press " +
                    (isSelected
                      ? "bg-[var(--color-accent)] font-semibold text-[var(--color-accent-fg)] "
                      : isToday
                        ? "font-semibold text-[var(--color-accent)] ring-1 ring-inset ring-[var(--color-accent)] "
                        : c.inMonth
                          ? "text-[var(--color-fg)] hover:bg-[var(--color-surface)] "
                          : "text-[var(--color-muted)] opacity-45 hover:bg-[var(--color-surface)] hover:opacity-80 ")
                  }
                >
                  {c.day}
                </button>
              );
            })}
          </div>
        ))}
      </div>

      {/* Selected-day readout */}
      {readout && (
        <div className="md3-banner-in rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
          <div className="text-[12px] font-medium">{readout.text}</div>
          <div className="text-[11px] text-[var(--color-muted)]">{readout.delta}</div>
        </div>
      )}

      <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">
        {focused
          ? "←→ month · ↑↓ year · T today · click a day · Esc close"
          : "Enter to navigate — or type e.g. “calendar märz 1990”"}
      </p>
    </div>
  );
}
