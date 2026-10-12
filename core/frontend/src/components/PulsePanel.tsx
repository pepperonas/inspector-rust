import { useCallback, useEffect, useRef, useState } from "react";
import { Activity, HardDrive, Settings2 } from "lucide-react";
import {
  pulseHistory,
  pulseLive,
  type PulseAgg,
  type PulseHistory,
  type PulseLive,
  type PulseRange,
  type PulseSample,
} from "../lib/ipc";
import {
  PRESSURE_COLOR,
  PRESSURE_LABEL,
  PULSE_RANGES,
  RANGE_LABEL,
  ageText,
  bucketLabel,
  dayLabel,
  dominantLevel,
  forecastText,
  formatBytes,
  formatMinutes,
  formatPerSec,
  formatRate,
  pressureLevel,
  series,
  swapShare,
  writeBars,
} from "../lib/pulse";
import { trendRange, trendSegments } from "../lib/speedtest";
import { t, useT } from "../lib/i18n";

type View = "live" | "history";

const LIVE_POLL_MS = 2000;
const HISTORY_POLL_MS = 60_000;

/**
 * `pulse` — memory pressure, swap and SSD writes (backend `pulse/`). The
 * sampler runs in the backend for the whole app lifetime; this panel only
 * reads its RAM ring (live) and `pulse.db` (history).
 */
export function PulsePanel({
  focused,
  onExit,
  onOpenSettings,
}: {
  focused: boolean;
  onExit: () => void;
  onOpenSettings: () => void;
}) {
  useT();
  const [view, setView] = useState<View>("live");
  const [range, setRange] = useState<PulseRange>("24h");
  const [live, setLive] = useState<PulseLive | null>(null);
  const [hist, setHist] = useState<PulseHistory | null>(null);
  const [error, setError] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let alive = true;
    const tick = async () => {
      try {
        const l = await pulseLive();
        if (alive) setLive(l);
      } catch (e) {
        if (alive) setError(String(e));
      }
    };
    void tick();
    const id = window.setInterval(tick, LIVE_POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);

  const loadHistory = useCallback(async (r: PulseRange) => {
    try {
      setHist(await pulseHistory(r));
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    if (view !== "history") return;
    void loadHistory(range);
    const id = window.setInterval(() => void loadHistory(range), HISTORY_POLL_MS);
    return () => window.clearInterval(id);
  }, [view, range, loadHistory]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onExit();
        return;
      }
      if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        setView((v) => (v === "live" ? "history" : "live"));
      } else if (e.key >= "1" && e.key <= "5") {
        e.preventDefault();
        setView("history");
        setRange(PULSE_RANGES[Number(e.key) - 1]);
      } else if (e.key === "ArrowDown" || e.key === "PageDown") {
        e.preventDefault();
        scrollRef.current?.scrollBy({ top: e.key === "PageDown" ? 180 : 48 });
      } else if (e.key === "ArrowUp" || e.key === "PageUp") {
        e.preventDefault();
        scrollRef.current?.scrollBy({ top: e.key === "PageUp" ? -180 : -48 });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [focused, onExit]);

  return (
    <div ref={scrollRef} className="flex h-full flex-col gap-3 overflow-y-auto p-3 text-sm">
      <div className="flex items-center gap-2">
        <Activity size={17} className="text-[var(--color-accent)]" />
        <span className="font-semibold">Pulse</span>
        <div className="ml-auto flex gap-1 rounded-full border border-[var(--color-border)] p-0.5 text-xs" role="tablist">
          {(["live", "history"] as View[]).map((v) => (
            <button
              key={v}
              role="tab"
              aria-selected={view === v}
              onClick={() => setView(v)}
              className={`rounded-full px-2.5 py-0.5 ${
                view === v ? "bg-[var(--color-accent)] text-[var(--color-accent-fg)]" : "text-[var(--color-muted)]"
              }`}
            >
              {v === "live" ? t("pulse.live") : t("pulse.history")}
            </button>
          ))}
        </div>
        <button
          onClick={onOpenSettings}
          title={t("pulse.settings")}
          aria-label={t("pulse.pulse_settings")}
          className="rounded p-1.5 text-[var(--color-muted)] hover:bg-[var(--color-bg)]"
        >
          <Settings2 size={14} />
        </button>
      </div>

      {error && (
        <div role="alert" className="rounded-lg border border-red-400/40 bg-red-400/10 p-2 text-xs text-red-300">
          {error}
        </div>
      )}

      {live && !live.supported ? (
        <Notice text={t("pulse.pulse_is_currently_macos_only")} />
      ) : live && !live.enabled ? (
        <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4 text-xs">
          {t("pulse.background_measuring_is_turned_off")}{" "}
          <button className="underline" onClick={onOpenSettings}>
            {t("pulse.turn_it_on_in_settings")}
          </button>
          .
        </div>
      ) : view === "live" ? (
        <LiveView live={live} />
      ) : (
        <HistoryView hist={hist} range={range} onRange={setRange} />
      )}

      {live && <Footnote live={live} />}
    </div>
  );
}

function Notice({ text }: { text: string }) {
  return (
    <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4 text-xs text-[var(--color-muted)]">
      {text}
    </div>
  );
}

// ── Live ───────────────────────────────────────────────────────────────────

function LiveView({ live }: { live: PulseLive | null }) {
  const s = live?.latest;
  if (!live || !s) return <Notice text={t("pulse.first_measurement_running_every_5")} />;
  const level = pressureLevel(s.pressure);
  const color = PRESSURE_COLOR[level];
  const swapFrac = s.swap_total > 0 ? s.swap_used / s.swap_total : 0;
  const share = swapShare(s.ssd_write_bps, s.swap_write_bps);
  // The latest sample is ≤ 5 s old — a pure stand-in for "now" in render.
  const now = s.ts;
  return (
    <>
      <div
        className="rounded-xl border p-4"
        style={{ borderColor: color, background: `color-mix(in srgb, ${color} 10%, var(--color-surface))` }}
        aria-live="polite"
      >
        <div className="text-xs uppercase tracking-widest text-[var(--color-muted)]">{t("pulse.memory_pressure")}</div>
        <div className="mt-1 flex items-baseline gap-3">
          <span className="text-3xl font-semibold" style={{ color }} data-testid="pulse-level">
            {PRESSURE_LABEL[level]}
          </span>
          {s.thermal >= 2 && (
            <span className="rounded bg-rose-500/15 px-1.5 py-0.5 text-xs text-rose-500">
              {t("pulse.thermal")} {s.thermal >= 3 ? "kritisch" : "hoch"}
            </span>
          )}
        </div>
        <div className="mt-3 flex items-baseline justify-between text-xs">
          <span className="text-[var(--color-muted)]">{t("pulse.swap_used")}</span>
          <span className="font-[var(--font-mono)] tabular-nums">
            {formatBytes(s.swap_used)} / {formatBytes(s.swap_total)}
          </span>
        </div>
        <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-[var(--color-bg)]">
          <div
            className="h-full origin-left rounded-full bg-[var(--color-accent)]"
            style={{ transform: `scaleX(${Math.min(1, swapFrac)})` }}
          />
        </div>
        <div className="mt-2 text-[11px] text-[var(--color-muted)]">
          {t("pulse.mem_line", { comp: formatBytes(s.compressor), wired: formatBytes(s.wired), free: formatBytes(s.free) })}
        </div>
      </div>

      <div className="grid grid-cols-2 gap-2">
        <Rate label={t("pulse.swap_out")} value={formatRate(s.swapout_bps)} />
        <Rate
          label={t("pulse.ssd_writes")}
          value={live.disk_found ? formatRate(s.ssd_write_bps) : "–"}
          sub={share != null ? t("pulse.of_which_swap_pct", { pct: Math.round(share * 100) }) : undefined}
        />
        <Rate label={t("pulse.compression")} value={formatPerSec(s.compress_ps)} sub={t("pulse.swap_in", { rate: formatRate(s.swapin_bps) })} />
        <Rate label="CPU" value={s.cpu_pct != null ? `${Math.round(s.cpu_pct)} %` : "–"} />
      </div>

      <Sparklines samples={live.recent} />

      <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
        <div className="flex items-baseline justify-between text-xs uppercase tracking-widest text-[var(--color-muted)]">
          <span>{t("pulse.biggest_consumers")}</span>
          <span className="normal-case tracking-normal">{ageText(now, live.groups_at)}</span>
        </div>
        <ul className="mt-2 flex flex-col gap-1.5" data-testid="pulse-groups">
          {live.groups.slice(0, 5).map((g) => (
            <li key={g.name} className="flex items-baseline gap-2 text-xs">
              <span className="min-w-0 flex-1 truncate">
                {g.name}
                {g.procs > 1 && <span className="text-[var(--color-muted)]"> · {g.procs}</span>}
              </span>
              <span className="font-[var(--font-mono)] tabular-nums">{formatBytes(g.footprint)}</span>
              <span className="w-20 text-right font-[var(--font-mono)] tabular-nums text-[var(--color-muted)]">
                {formatRate(g.write_bps)}
              </span>
            </li>
          ))}
          {live.groups.length === 0 && <li className="text-xs text-[var(--color-muted)]">{t("pulse.no_process_scan_yet")}</li>}
        </ul>
      </div>
    </>
  );
}

function Rate({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
      <div className="text-[11px] uppercase tracking-widest text-[var(--color-muted)]">{label}</div>
      <div className="mt-1 font-[var(--font-mono)] text-lg font-semibold tabular-nums">{value}</div>
      {sub && <div className="text-[11px] text-[var(--color-muted)]">{sub}</div>}
    </div>
  );
}

const SPARK_W = 260;
const SPARK_H = 28;

function Sparklines({ samples }: { samples: PulseSample[] }) {
  if (samples.length < 2) return null;
  // Rates scale from 0 (a rate's zero is meaningful); the swap level scales
  // to its own range, else a 3-GB swing on a 20-GB base reads as flat.
  const rows: { label: string; key: keyof PulseSample; fmt: (n: number) => string; own?: boolean }[] = [
    { label: t("pulse.ssd_writes"), key: "ssd_write_bps", fmt: formatRate },
    { label: t("pulse.swap_out"), key: "swapout_bps", fmt: formatRate },
    { label: t("pulse.swap_used"), key: "swap_used", fmt: formatBytes, own: true },
  ];
  return (
    <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
      <div className="text-xs uppercase tracking-widest text-[var(--color-muted)]">{t("pulse.last_15_minutes")}</div>
      <div className="mt-2 flex h-2 overflow-hidden rounded" aria-label={t("pulse.memory_pressure_history")}>
        {samples.map((s, i) => (
          <div key={i} className="h-full flex-1" style={{ background: PRESSURE_COLOR[pressureLevel(s.pressure)] }} />
        ))}
      </div>
      {rows.map((r) => {
        const vals = series(samples, r.key);
        const max = Math.max(0, ...vals.map((v) => v ?? 0));
        const span = r.own ? trendRange(vals) : null;
        return (
          <div key={r.key} className="mt-2 flex items-center gap-2 text-[11px]">
            <span className="w-20 shrink-0 text-[var(--color-muted)]">{r.label}</span>
            <svg viewBox={`0 0 ${SPARK_W} ${SPARK_H}`} className="h-7 min-w-0 flex-1" preserveAspectRatio="none">
              {trendSegments(vals, span?.max ?? max, SPARK_W, SPARK_H, span?.min ?? 0).map((pts, i) => (
                <polyline key={i} points={pts} fill="none" stroke="var(--color-accent)" strokeWidth="1.5" />
              ))}
            </svg>
            <span className="w-16 text-right font-[var(--font-mono)] tabular-nums">{r.fmt(max)}</span>
          </div>
        );
      })}
    </div>
  );
}

// ── History ────────────────────────────────────────────────────────────────

const BAR_H = 64;

function HistoryView({
  hist,
  range,
  onRange,
}: {
  hist: PulseHistory | null;
  range: PulseRange;
  onRange: (r: PulseRange) => void;
}) {
  return (
    <>
      <div className="flex gap-1" role="tablist" aria-label={t("pulse.range")}>
        {PULSE_RANGES.map((r, i) => (
          <button
            key={r}
            role="tab"
            aria-selected={range === r}
            title={t("pulse.key_n", { n: i + 1 })}
            onClick={() => onRange(r)}
            className={`rounded-full border px-2.5 py-0.5 text-xs ${
              range === r
                ? "border-[var(--color-accent)] bg-[var(--color-accent)] text-[var(--color-accent-fg)]"
                : "border-[var(--color-border)] text-[var(--color-muted)]"
            }`}
          >
            {RANGE_LABEL[r]}
          </button>
        ))}
      </div>
      {!hist || hist.range !== range ? (
        <Notice text={t("pulse.loading")} />
      ) : hist.buckets.length === 0 ? (
        <Notice text={t("pulse.no_data_for_this_range")} />
      ) : (
        <HistoryBody hist={hist} />
      )}
    </>
  );
}

function HistoryBody({ hist }: { hist: PulseHistory }) {
  const tot = hist.total;
  const bars = writeBars(hist.buckets, BAR_H);
  const share = swapShare(tot.written, tot.swap_written);
  const first = hist.buckets[0];
  const last = hist.buckets[hist.buckets.length - 1];
  return (
    <>
      <div className="grid grid-cols-3 gap-2">
        <Rate label={t("pulse.written")} value={formatBytes(tot.written)} />
        <Rate
          label={t("pulse.of_which_swap")}
          value={formatBytes(tot.swap_written)}
          sub={share != null ? t("pulse.share_est", { pct: Math.round(share * 100) }) : undefined}
        />
        <Rate label={t("pulse.time_red")} value={formatMinutes(tot.p_crit)} sub={t("pulse.yellow_time", { time: formatMinutes(tot.p_warn) })} />
      </div>

      <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
        <div className="text-xs uppercase tracking-widest text-[var(--color-muted)]">{t("pulse.memory_pressure")}</div>
        <div className="mt-2 flex h-3 overflow-hidden rounded" data-testid="pulse-band">
          {hist.buckets.map((b) => (
            <div
              key={b.ts}
              className="h-full flex-1"
              title={t("pulse.bucket_red", { when: bucketLabel(b.ts, hist.bucket_secs), time: formatMinutes(b.p_crit) })}
              style={{ background: PRESSURE_COLOR[dominantLevel(b)] }}
            />
          ))}
        </div>
        <div className="mt-3 text-xs uppercase tracking-widest text-[var(--color-muted)]">
          {t("pulse.ssd_writes_title")}{" "}
          {hist.bucket_secs >= 86_400
            ? t("pulse.per_day")
            : hist.bucket_secs >= 3_600
              ? t("pulse.per_hour")
              : hist.bucket_secs >= 900
                ? t("pulse.per_15")
                : t("pulse.per_minute")}
        </div>
        <svg
          viewBox={`0 0 ${hist.buckets.length * 10} ${BAR_H}`}
          className="mt-1 h-16 w-full"
          preserveAspectRatio="none"
          data-testid="pulse-bars"
        >
          {bars.map((b, i) => (
            <g key={i}>
              <rect x={i * 10 + 1} y={BAR_H - b.total} width={8} height={b.total} fill="var(--color-accent)" opacity={0.35}>
                <title>
                  {t("pulse.bar_title", { when: bucketLabel(hist.buckets[i].ts, hist.bucket_secs), written: formatBytes(hist.buckets[i].written), swap: formatBytes(hist.buckets[i].swap_written) })}
                </title>
              </rect>
              <rect x={i * 10 + 1} y={BAR_H - b.swap} width={8} height={b.swap} fill="#f43f5e" />
            </g>
          ))}
        </svg>
        <div className="mt-1 flex justify-between text-[10px] text-[var(--color-muted)]">
          <span>{bucketLabel(first.ts, hist.bucket_secs)}</span>
          <span className="flex items-center gap-1">
            <span className="inline-block h-2 w-2 rounded-sm bg-rose-500" /> {t("pulse.swap_estimated")}
          </span>
          <span>{bucketLabel(last.ts, hist.bucket_secs)}</span>
        </div>
      </div>

      {tot.causers.length > 0 && <Causers aggs={tot} />}

      {hist.days.length > 0 && (
        <div className="grid grid-cols-2 gap-2" data-testid="pulse-days">
          {hist.days.slice(0, 8).map((d) => (
            <DayCard key={d.ts} d={d} />
          ))}
        </div>
      )}

      <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-xs">
        <div className="flex items-center gap-1.5 uppercase tracking-widest text-[var(--color-muted)]">
          <HardDrive size={12} /> {t("pulse.ssd_lifetime")}
        </div>
        <p className="mt-1" data-testid="pulse-forecast">
          {forecastText(hist.forecast)}
        </p>
      </div>
    </>
  );
}

function Causers({ aggs }: { aggs: PulseAgg }) {
  return (
    <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
      <div className="flex justify-between text-xs uppercase tracking-widest text-[var(--color-muted)]">
        <span>{t("pulse.main_causes")}</span>
        <span className="normal-case tracking-normal">{t("pulse.peak_written")}</span>
      </div>
      <ul className="mt-2 flex flex-col gap-1">
        {aggs.causers.slice(0, 5).map((c) => (
          <li key={c.name} className="flex items-baseline gap-2 text-xs">
            <span className="min-w-0 flex-1 truncate">{c.name}</span>
            <span className="font-[var(--font-mono)] tabular-nums">{formatBytes(c.peak_footprint)}</span>
            <span className="w-20 text-right font-[var(--font-mono)] tabular-nums text-[var(--color-muted)]">
              {formatBytes(c.written)}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function DayCard({ d }: { d: PulseAgg }) {
  const top = d.causers[0];
  return (
    <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-2.5 text-xs">
      <div className="flex items-center justify-between">
        <span className="font-semibold">{dayLabel(d.ts)}</span>
        <span className="h-2 w-2 rounded-full" style={{ background: PRESSURE_COLOR[dominantLevel(d)] }} />
      </div>
      <div className="mt-1 font-[var(--font-mono)] tabular-nums">{formatBytes(d.written)}</div>
      <div className="text-[var(--color-muted)]">{t("pulse.of_which_swap_size", { size: formatBytes(d.swap_written) })}</div>
      <div className="text-[var(--color-muted)]">rot {formatMinutes(d.p_crit)}</div>
      {top && <div className="truncate text-[var(--color-muted)]">↑ {top.name}</div>}
    </div>
  );
}

// ── Footnote ───────────────────────────────────────────────────────────────

function Footnote({ live }: { live: PulseLive }) {
  const o = live.overhead;
  return (
    <p className="mt-auto text-[10px] leading-relaxed text-[var(--color-muted)]">
      {t("pulse.fn.kernel")}
      {live.unreadable > 0 && ` ${t("pulse.fn.unreadable", { n: live.unreadable })}`}
      {" "}{t("pulse.fn.reset")}{" "}
      {live.smart?.percentage_used != null
        ? `${t("pulse.fn.smart", { pct: live.smart.percentage_used })} `
        : live.smartctl_present === false
          ? `${t("pulse.fn.smartctl")} `
          : ""}
      {t("pulse.fn.overhead", {
        cpu: o.running_secs > 60 ? `${o.cpu_pct.toFixed(2)} % CPU, ` : "",
        sample: o.sample_avg_ms.toFixed(1),
        scan: o.scan_avg_ms.toFixed(1),
        db: formatBytes(live.db_bytes),
      })}
      {live.last_error && ` ${t("pulse.fn.error", { err: live.last_error })}`}
    </p>
  );
}
