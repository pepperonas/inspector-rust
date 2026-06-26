import { useEffect, useRef, useState } from "react";
import {
  Activity,
  Cpu,
  MemoryStick,
  HardDrive,
  ArrowDownUp,
  Thermometer,
  Fan,
  BatteryCharging,
  Zap,
  Server,
} from "lucide-react";
import {
  getSystemStats,
  getStatsHistory,
  type SystemStats,
  type StatsHistory,
  type StatsHistoryPoint,
} from "../lib/ipc";
import {
  humanBytes,
  humanRate,
  humanUptime,
  humanDuration,
  clampPct,
  usedPct,
} from "../lib/format-stats";
import { areaPath, linePath, seriesExtent } from "../lib/stats-chart";

/**
 * Read-only live system-stats panel rendered in the right preview column —
 * entered by pressing Enter on the `stats` command row. Polls
 * `get_system_stats` every {@link POLL_MS}. Esc leaves (`onExit`); there's no
 * selection model (nothing to act on), so it doesn't take keyboard focus the
 * way Brightness/Sound do — only Esc is handled while `focused`.
 *
 * Sources are best-effort per OS (see `system_stats.rs`): CPU/mem/disk/net via
 * sysinfo, temps via Components (summarised), fans via SMC/hwmon, battery &
 * power draw via starship-battery.
 */
const POLL_MS = 1500;

/** History time-frame options (seconds) for the dropdown. */
const RANGES: ReadonlyArray<{ label: string; secs: number }> = [
  { label: "1h", secs: 3600 },
  { label: "6h", secs: 21600 },
  { label: "24h", secs: 86400 },
  { label: "7d", secs: 604800 },
];

type Mode = "live" | "history";

export function StatsPanel({
  focused,
  onExit,
}: {
  focused: boolean;
  onExit: () => void;
}) {
  const [mode, setMode] = useState<Mode>("live");
  const [rangeSecs, setRangeSecs] = useState<number>(21600); // default 6h
  const [stats, setStats] = useState<SystemStats | null>(null);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);
  const scrollRef = useRef<HTMLDivElement>(null);
  const tickRef = useRef<() => void>(() => {});
  // True while a wheel/trackpad scroll is in flight — see `onScroll`.
  const scrollingRef = useRef(false);
  const scrollEndRef = useRef<number | undefined>(undefined);

  // Live polling — only runs in live mode (history fetches separately).
  useEffect(() => {
    if (mode !== "live") {
      tickRef.current = () => {};
      return;
    }
    alive.current = true;
    const tick = () => {
      // Skip the re-render while the user is actively wheel/trackpad-scrolling:
      // a reconciliation landing mid-momentum stutters the scroll (the arrow-key
      // path doesn't hit this because each press is a discrete jump). We refresh
      // the instant scrolling settles (see `onScroll`) and on the normal tick.
      if (scrollingRef.current) return;
      getSystemStats()
        .then((s) => {
          if (alive.current) {
            setStats(s);
            setError(null);
          }
        })
        .catch((e) => {
          if (alive.current) setError(String(e));
        });
    };
    tickRef.current = tick;
    tick();
    const id = window.setInterval(tick, POLL_MS);
    return () => {
      alive.current = false;
      window.clearInterval(id);
      window.clearTimeout(scrollEndRef.current);
    };
  }, [mode]);

  // Mark "scrolling" on each scroll event and clear it ~200 ms after the last
  // one (momentum settled), then refresh once. Cheap: only touches refs +
  // a debounce timer — no per-frame re-render.
  const onScroll = () => {
    scrollingRef.current = true;
    if (scrollEndRef.current) window.clearTimeout(scrollEndRef.current);
    scrollEndRef.current = window.setTimeout(() => {
      scrollingRef.current = false;
      tickRef.current();
    }, 200);
  };

  // While the panel owns the keyboard: ↑/↓ (and PageUp/Down, Home/End) scroll
  // the panel, Esc leaves. Read-only otherwise (no selection/Enter). Scroll is
  // an instant `scrollBy` (not smooth) so held-key repeat steps responsively
  // instead of queueing momentum.
  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      // Let the mode toggle / range dropdown handle their own keys (so the
      // native <select> opens + arrow-selects instead of scrolling the panel).
      const tgt = e.target as HTMLElement | null;
      if (tgt && (tgt.tagName === "SELECT" || tgt.tagName === "BUTTON" || tgt.tagName === "INPUT")) {
        return;
      }
      const el = scrollRef.current;
      const STEP = 64;
      switch (e.key) {
        case "ArrowDown":
          if (!el) break;
          e.preventDefault();
          e.stopPropagation();
          el.scrollBy({ top: STEP });
          break;
        case "ArrowUp":
          if (!el) break;
          e.preventDefault();
          e.stopPropagation();
          el.scrollBy({ top: -STEP });
          break;
        case "PageDown":
          if (!el) break;
          e.preventDefault();
          e.stopPropagation();
          el.scrollBy({ top: el.clientHeight * 0.85 });
          break;
        case "PageUp":
          if (!el) break;
          e.preventDefault();
          e.stopPropagation();
          el.scrollBy({ top: -el.clientHeight * 0.85 });
          break;
        case "Home":
          if (!el) break;
          e.preventDefault();
          e.stopPropagation();
          el.scrollTo({ top: 0 });
          break;
        case "End":
          if (!el) break;
          e.preventDefault();
          e.stopPropagation();
          el.scrollTo({ top: el.scrollHeight });
          break;
        case "Escape":
          e.preventDefault();
          e.stopPropagation();
          onExit();
          break;
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [focused, onExit]);

  return (
    <div
      ref={scrollRef}
      onScroll={onScroll}
      // `translateZ(0)` + `contain: paint` promote the scrollport to its own GPU
      // compositor layer, so wheel/trackpad scrolling is a cheap GPU translate
      // instead of a main-thread repaint per frame (the trackpad-lag fix).
      style={{ transform: "translateZ(0)" }}
      className="flex h-full flex-col gap-3 overflow-y-auto p-4 text-[var(--color-fg)] [contain:paint]"
    >
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-2 text-[13px] font-medium">
          <Activity size={15} className="text-[var(--color-accent)]" /> System stats
        </div>
        <div className="flex items-center gap-1.5">
          {mode === "history" && (
            <select
              value={rangeSecs}
              onChange={(e) => setRangeSecs(Number(e.target.value))}
              className="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5 text-[11px] text-[var(--color-fg)] outline-none"
              aria-label="History time range"
            >
              {RANGES.map((r) => (
                <option key={r.secs} value={r.secs}>
                  {r.label}
                </option>
              ))}
            </select>
          )}
          <div className="flex items-center gap-0.5 rounded-lg border border-[var(--color-border)] p-0.5">
            <ModeButton active={mode === "live"} onClick={() => setMode("live")}>
              Live
            </ModeButton>
            <ModeButton active={mode === "history"} onClick={() => setMode("history")}>
              History
            </ModeButton>
          </div>
        </div>
      </div>

      {mode === "history" ? (
        <HistoryView rangeSecs={rangeSecs} />
      ) : error ? (
        <p className="text-[12px] text-[var(--color-muted)]">{error}</p>
      ) : stats === null ? (
        <p className="text-[12px] text-[var(--color-muted)]">Reading system…</p>
      ) : (
        <>
          <CpuSection s={stats} />
          <MemorySection s={stats} />
          {stats.battery && <BatterySection b={stats.battery} />}
          <SensorsSection s={stats} />
          <DisksSection s={stats} />
          <NetworkSection s={stats} />
          <HostSection s={stats} />
        </>
      )}
      {focused && (
        <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">
          ↑ ↓ scroll · Esc close
        </p>
      )}
    </div>
  );
}

function ModeButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={
        "rounded-md px-2 py-0.5 text-[11px] transition-colors " +
        (active
          ? "bg-[var(--color-accent)] font-medium text-[var(--color-accent-fg)]"
          : "text-[var(--color-muted)] hover:text-[var(--color-fg)]")
      }
    >
      {children}
    </button>
  );
}

// ── Historical view ───────────────────────────────────────────────────────────

const HISTORY_REFRESH_MS = 30000;

function HistoryView({ rangeSecs }: { rangeSecs: number }) {
  const [hist, setHist] = useState<StatsHistory | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    const fetchIt = () =>
      getStatsHistory(rangeSecs)
        .then((h) => {
          if (alive.current) {
            setHist(h);
            setErr(null);
          }
        })
        .catch((e) => {
          if (alive.current) setErr(String(e));
        });
    fetchIt();
    const id = window.setInterval(fetchIt, HISTORY_REFRESH_MS);
    return () => {
      alive.current = false;
      window.clearInterval(id);
    };
  }, [rangeSecs]);

  if (err) return <p className="text-[12px] text-[var(--color-muted)]">{err}</p>;
  if (!hist) return <p className="text-[12px] text-[var(--color-muted)]">Loading history…</p>;
  const pts = hist.points;
  if (pts.length < 2) {
    const mins = Math.round(hist.interval_secs / 60);
    return (
      <div className="rounded-xl border border-[var(--color-border)] p-4 text-center">
        <p className="text-[12px] text-[var(--color-fg)]">Collecting history…</p>
        <p className="mt-1 text-[11px] text-[var(--color-muted)]">
          A data point is recorded every {mins || 1} min. Come back in a little
          while to see the trend.
        </p>
      </div>
    );
  }

  const tMin = pts[0].ts;
  const tMax = pts[pts.length - 1].ts;
  const rangeLabel = RANGES.find((r) => r.secs === rangeSecs)?.label ?? `${rangeSecs}s`;
  const hasPower = pts.some((p) => p.power != null);
  const hasTemp = pts.some((p) => p.cpu_temp != null);
  const hasBattery = pts.some((p) => p.battery != null);

  return (
    <>
      <LineChartCard
        icon={<Cpu size={14} />}
        title="CPU"
        pts={pts}
        pick={(p) => p.cpu}
        tMin={tMin}
        tMax={tMax}
        domain={[0, 100]}
        fmt={(v) => `${v.toFixed(0)}%`}
      />
      <LineChartCard
        icon={<MemoryStick size={14} />}
        title="Memory"
        pts={pts}
        pick={(p) => p.mem}
        tMin={tMin}
        tMax={tMax}
        domain={[0, 100]}
        fmt={(v) => `${v.toFixed(0)}%`}
      />
      <NetworkChartCard pts={pts} tMin={tMin} tMax={tMax} />
      {hasPower && (
        <LineChartCard
          icon={<Zap size={14} />}
          title="Power draw"
          pts={pts}
          pick={(p) => p.power}
          tMin={tMin}
          tMax={tMax}
          fmt={(v) => `${v.toFixed(1)} W`}
        />
      )}
      {hasTemp && (
        <LineChartCard
          icon={<Thermometer size={14} />}
          title="CPU temp"
          pts={pts}
          pick={(p) => p.cpu_temp}
          tMin={tMin}
          tMax={tMax}
          fmt={(v) => `${v.toFixed(1)}°C`}
        />
      )}
      {hasBattery && (
        <LineChartCard
          icon={<BatteryCharging size={14} />}
          title="Battery"
          pts={pts}
          pick={(p) => p.battery}
          tMin={tMin}
          tMax={tMax}
          domain={[0, 100]}
          fmt={(v) => `${v.toFixed(0)}%`}
        />
      )}
      <p className="px-1 pb-1 text-[10px] text-[var(--color-muted)]">
        {rangeLabel} ago · {hist.sample_count} samples · now →
      </p>
    </>
  );
}

const CHART_W = 320;
const CHART_H = 60;

/** A single-series sparkline card. `pick` may return null (sensor absent at
 *  that sample) — those points are skipped. `domain` fixes the y-axis (e.g.
 *  [0,100] for percentages); omitted → auto-scaled to the data. */
function LineChartCard({
  icon,
  title,
  pts,
  pick,
  tMin,
  tMax,
  domain,
  fmt,
}: {
  icon: React.ReactNode;
  title: string;
  pts: StatsHistoryPoint[];
  pick: (p: StatsHistoryPoint) => number | null;
  tMin: number;
  tMax: number;
  domain?: [number, number];
  fmt: (v: number) => string;
}) {
  const series = pts
    .map((p) => ({ t: p.ts, v: pick(p) }))
    .filter((s): s is { t: number; v: number } => s.v != null);
  if (series.length === 0) return null;
  const values = series.map((s) => s.v);
  const [vMin, vMax] = domain ?? seriesExtent(values);
  const d = linePath(series, tMin, tMax, CHART_W, CHART_H, vMin, vMax);
  const a = areaPath(series, tMin, tMax, CHART_W, CHART_H, vMin, vMax);
  const cur = values[values.length - 1];
  const min = Math.min(...values);
  const max = Math.max(...values);
  const avg = values.reduce((s, v) => s + v, 0) / values.length;
  return (
    <Card icon={icon} title={title} right={fmt(cur)}>
      <Sparkline line={d} area={a} color="var(--color-accent)" />
      <div className="mt-1 flex items-center justify-between text-[10px] text-[var(--color-muted)] tabular-nums">
        <span>min {fmt(min)}</span>
        <span>avg {fmt(avg)}</span>
        <span>max {fmt(max)}</span>
      </div>
    </Card>
  );
}

function NetworkChartCard({
  pts,
  tMin,
  tMax,
}: {
  pts: StatsHistoryPoint[];
  tMin: number;
  tMax: number;
}) {
  const rx = pts.map((p) => ({ t: p.ts, v: p.net_rx }));
  const tx = pts.map((p) => ({ t: p.ts, v: p.net_tx }));
  const peak = Math.max(1, ...pts.map((p) => Math.max(p.net_rx, p.net_tx)));
  const vMax = peak * 1.15; // floor the axis at 0 — rates are non-negative
  const dRx = linePath(rx, tMin, tMax, CHART_W, CHART_H, 0, vMax);
  const dTx = linePath(tx, tMin, tMax, CHART_W, CHART_H, 0, vMax);
  const curRx = pts[pts.length - 1].net_rx;
  const curTx = pts[pts.length - 1].net_tx;
  return (
    <Card
      icon={<ArrowDownUp size={14} />}
      title="Network"
      right={`↓ ${humanRate(curRx)} · ↑ ${humanRate(curTx)}`}
    >
      <svg
        viewBox={`0 0 ${CHART_W} ${CHART_H}`}
        preserveAspectRatio="none"
        className="h-14 w-full"
      >
        <path d={dRx} fill="none" stroke="var(--color-accent)" strokeWidth={1.5} vectorEffect="non-scaling-stroke" />
        <path d={dTx} fill="none" stroke="#f59e0b" strokeWidth={1.5} vectorEffect="non-scaling-stroke" />
      </svg>
      <div className="mt-1 flex items-center gap-3 text-[10px] text-[var(--color-muted)]">
        <span className="flex items-center gap-1">
          <span className="inline-block h-[2px] w-3" style={{ backgroundColor: "var(--color-accent)" }} /> download
        </span>
        <span className="flex items-center gap-1">
          <span className="inline-block h-[2px] w-3" style={{ backgroundColor: "#f59e0b" }} /> upload
        </span>
        <span className="ml-auto tabular-nums">peak {humanRate(peak)}</span>
      </div>
    </Card>
  );
}

function Sparkline({ line, area, color }: { line: string; area: string; color: string }) {
  return (
    <svg
      viewBox={`0 0 ${CHART_W} ${CHART_H}`}
      preserveAspectRatio="none"
      className="h-14 w-full"
    >
      <path d={area} fill={color} opacity={0.12} />
      <path d={line} fill="none" stroke={color} strokeWidth={1.5} vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

// ── Shared bits ─────────────────────────────────────────────────────────────

/** Accent colour for a load percentage: green → amber → red. */
function loadColor(pct: number): string {
  if (pct >= 90) return "#ef4444"; // red-500
  if (pct >= 70) return "#f59e0b"; // amber-500
  return "var(--color-accent)";
}

function Bar({ pct, color }: { pct: number; color?: string }) {
  const p = clampPct(pct);
  // Size via `transform: scaleX` (no layout) and **snap** to the new value each
  // poll — no `transition`/`will-change`, which would otherwise leave ~15
  // permanent compositor layers (one per bar) that the compositor must blend on
  // every scroll frame → scroll jank. A stats readout doesn't need bar tweening.
  return (
    <div className="relative h-1.5 w-full overflow-hidden rounded-full bg-[var(--color-border)]">
      <div
        className="absolute inset-0 origin-left rounded-full"
        style={{ transform: `scaleX(${p / 100})`, backgroundColor: color ?? loadColor(p) }}
      />
    </div>
  );
}

function Card({
  icon,
  title,
  right,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  right?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="rounded-xl border border-[var(--color-border)] p-3 [contain:content]">
      <div className="mb-2 flex items-center justify-between">
        <div className="flex items-center gap-1.5 text-[12px] font-medium">
          <span className="text-[var(--color-accent)]">{icon}</span>
          {title}
        </div>
        {right && <span className="text-[11px] text-[var(--color-muted)]">{right}</span>}
      </div>
      {children}
    </div>
  );
}

function Kv({ k, v }: { k: string; v: string }) {
  return (
    <div className="flex items-center justify-between gap-2 text-[11px]">
      <span className="text-[var(--color-muted)]">{k}</span>
      <span className="truncate text-right tabular-nums">{v}</span>
    </div>
  );
}

// ── Sections ────────────────────────────────────────────────────────────────

function CpuSection({ s }: { s: SystemStats }) {
  const cores = s.physical_cores
    ? `${s.physical_cores}C / ${s.logical_cores}T`
    : `${s.logical_cores} threads`;
  return (
    <Card
      icon={<Cpu size={14} />}
      title="CPU"
      right={`${s.cpu_usage.toFixed(0)}%`}
    >
      <Bar pct={s.cpu_usage} />
      <div className="mt-1.5 truncate text-[11px] text-[var(--color-muted)]">
        {s.cpu_brand}
      </div>
      <div className="mt-0.5 flex items-center justify-between text-[11px] text-[var(--color-muted)]">
        <span>{cores}</span>
        <span className="tabular-nums">
          {s.cpu_freq_mhz > 0 ? `${(s.cpu_freq_mhz / 1000).toFixed(2)} GHz` : ""}
          {s.load_avg ? ` · load ${s.load_avg.map((l) => l.toFixed(1)).join(" ")}` : ""}
        </span>
      </div>
      {s.per_core.length > 1 && (
        <div className="mt-2 grid grid-cols-8 gap-1">
          {s.per_core.map((u, i) => (
            <div
              key={i}
              title={`Core ${i}: ${u.toFixed(0)}%`}
              className="relative h-7 overflow-hidden rounded-sm bg-[var(--color-border)]"
            >
              <div
                className="absolute inset-0 origin-bottom"
                style={{ transform: `scaleY(${clampPct(u) / 100})`, backgroundColor: loadColor(u) }}
              />
            </div>
          ))}
        </div>
      )}
    </Card>
  );
}

function MemorySection({ s }: { s: SystemStats }) {
  const memPct = usedPct(s.mem_used, s.mem_total);
  const swapPct = usedPct(s.swap_used, s.swap_total);
  return (
    <Card icon={<MemoryStick size={14} />} title="Memory" right={`${memPct.toFixed(0)}%`}>
      <Bar pct={memPct} />
      <div className="mt-1 flex items-center justify-between text-[11px] text-[var(--color-muted)] tabular-nums">
        <span>
          {humanBytes(s.mem_used)} / {humanBytes(s.mem_total)}
        </span>
        <span>{humanBytes(s.mem_available)} free</span>
      </div>
      {s.swap_total > 0 && (
        <div className="mt-2">
          <div className="mb-1 flex items-center justify-between text-[11px] text-[var(--color-muted)] tabular-nums">
            <span>Swap</span>
            <span>
              {humanBytes(s.swap_used)} / {humanBytes(s.swap_total)}
            </span>
          </div>
          <Bar pct={swapPct} />
        </div>
      )}
    </Card>
  );
}

function BatterySection({ b }: { b: NonNullable<SystemStats["battery"]> }) {
  const charging = b.state === "Charging" || b.state === "Full";
  const time =
    b.state === "Charging" && b.time_to_full_secs != null
      ? `${humanDuration(b.time_to_full_secs)} to full`
      : b.state === "Discharging" && b.time_to_empty_secs != null
        ? `${humanDuration(b.time_to_empty_secs)} left`
        : b.state;
  return (
    <Card
      icon={<BatteryCharging size={14} />}
      title="Battery & power"
      right={`${b.percent.toFixed(0)}%`}
    >
      <Bar
        pct={b.percent}
        color={charging ? "#22c55e" : b.percent <= 15 ? "#ef4444" : "var(--color-accent)"}
      />
      <div className="mt-1.5 flex items-center justify-between text-[11px] text-[var(--color-muted)]">
        <span>{time}</span>
        {b.power_watts != null && (
          <span className="flex items-center gap-1 font-medium text-[var(--color-fg)] tabular-nums">
            <Zap size={12} className="text-[var(--color-accent)]" />
            {b.power_watts.toFixed(1)} W
          </span>
        )}
      </div>
      <div className="mt-2 grid grid-cols-2 gap-x-3 gap-y-1">
        {b.health_percent != null && (
          <Kv k="Health" v={`${b.health_percent.toFixed(0)}%`} />
        )}
        {b.cycle_count != null && <Kv k="Cycles" v={`${b.cycle_count}`} />}
        {b.temperature_c != null && (
          <Kv k="Temp" v={`${b.temperature_c.toFixed(1)}°C`} />
        )}
        {(b.model || b.vendor) && (
          <Kv k="Model" v={b.model || b.vendor || ""} />
        )}
      </div>
    </Card>
  );
}

function SensorsSection({ s }: { s: SystemStats }) {
  if (s.temps.length === 0 && s.fans.length === 0) return null;
  return (
    <Card icon={<Thermometer size={14} />} title="Sensors">
      {s.temps.length > 0 && (
        <div className="grid grid-cols-2 gap-x-3 gap-y-1">
          {s.temps.map((t) => (
            <Kv key={t.label} k={t.label} v={`${t.celsius.toFixed(1)}°C`} />
          ))}
        </div>
      )}
      {s.fans.length > 0 && (
        <div className="mt-1.5 grid grid-cols-2 gap-x-3 gap-y-1">
          {s.fans.map((f) => (
            <div
              key={f.label}
              className="flex items-center justify-between gap-2 text-[11px]"
            >
              <span className="flex items-center gap-1 text-[var(--color-muted)]">
                <Fan size={11} /> {f.label}
              </span>
              <span className="tabular-nums">{f.rpm} rpm</span>
            </div>
          ))}
        </div>
      )}
    </Card>
  );
}

function DisksSection({ s }: { s: SystemStats }) {
  if (s.disks.length === 0) return null;
  return (
    <Card icon={<HardDrive size={14} />} title="Storage">
      <div className="flex flex-col gap-2">
        {s.disks.map((d, i) => {
          const used = d.total - d.available;
          const pct = usedPct(used, d.total);
          return (
            <div key={`${d.mount}-${i}`}>
              <div className="mb-1 flex items-center justify-between gap-2 text-[11px]">
                <span className="truncate text-[var(--color-muted)]" title={d.mount}>
                  {d.mount}
                </span>
                <span className="shrink-0 tabular-nums text-[var(--color-muted)]">
                  {humanBytes(used)} / {humanBytes(d.total)}
                </span>
              </div>
              <Bar pct={pct} />
            </div>
          );
        })}
      </div>
    </Card>
  );
}

function NetworkSection({ s }: { s: SystemStats }) {
  return (
    <Card icon={<ArrowDownUp size={14} />} title="Network">
      <div className="flex items-center justify-between text-[12px] tabular-nums">
        <span className="flex items-center gap-1.5">
          <span className="text-[var(--color-muted)]">↓</span>
          {humanRate(s.net_rx_per_sec)}
        </span>
        <span className="flex items-center gap-1.5">
          <span className="text-[var(--color-muted)]">↑</span>
          {humanRate(s.net_tx_per_sec)}
        </span>
      </div>
    </Card>
  );
}

function HostSection({ s }: { s: SystemStats }) {
  return (
    <Card icon={<Server size={14} />} title="Host">
      <div className="grid grid-cols-1 gap-1">
        {s.host_name && <Kv k="Host" v={s.host_name} />}
        {s.os_name && <Kv k="OS" v={s.os_name} />}
        {s.kernel && <Kv k="Kernel" v={s.kernel} />}
        {s.cpu_arch && <Kv k="Arch" v={s.cpu_arch} />}
        <Kv k="Uptime" v={humanUptime(s.uptime_secs)} />
      </div>
    </Card>
  );
}
