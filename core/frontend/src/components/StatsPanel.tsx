import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
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
  ChevronDown,
  ChevronRight,
} from "lucide-react";
import {
  getSystemStats,
  getStatsHistory,
  statsTopProcesses,
  type ProcRow,
  type SystemStats,
  type TopProcs,
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
  fmtNum,
  fmtPct,
  fmtUnit,
} from "../lib/format-stats";
import {
  cardForKey,
  groupTemps,
  loadLevel,
  memSegments,
  parseCollapsed,
  pushSample,
  shareOfTop,
  sparkPaths,
  splitCores,
  stackUpTo,
  toggleCollapsed,
  visibleDisks,
  type CoreReading,
  type LoadLevel,
  type MemKind,
  type TempGroupKey,
} from "../lib/stats-view";
import { IS_MAC } from "../lib/platform";
import { areaPath, linePath, seriesExtent } from "../lib/stats-chart";
import {
  STAT_TWEEN_MS,
  tweenAt,
  heatLevel,
  isHot,
  bytesFormatterFor,
  rateFormatterFor,
  temperatureLevel,
  temperatureColor,
  fanDuration,
} from "../lib/stats-anim";
import { prefersReducedMotion } from "../lib/md3-motion";
import { t, useT } from "../lib/i18n";

/**
 * Read-only live system-stats panel rendered in the right preview column —
 * entered by pressing Enter on the `stats` command row. Polls
 * `get_system_stats` every {@link POLL_MS}. Esc leaves (`onExit`); there's no
 * selection model (nothing to act on), so it doesn't take keyboard focus the
 * way Brightness/Sound do — only Esc is handled while `focused`.
 *
 * Sources are best-effort per OS (see `system_stats.rs`): CPU/mem/disk/net via
 * sysinfo, every available temp via Components/SMC, fans via SMC/hwmon, battery &
 * power draw via starship-battery.
 */
const POLL_MS = 1500;
/** The process lists cost a scan of every process — half as often. */
const PROC_POLL_MS = 3000;
/** The overview sparklines show the last minute. */
const TRAIL_SLOTS = Math.round(60_000 / POLL_MS);
const COLLAPSE_KEY = "ir.stats.collapsed.v1";

type CardId = "cpu" | "memory" | "battery" | "temps" | "fans" | "storage" | "network" | "host";

interface Trail {
  cpu: number[];
  mem: number[];
  net: number[];
  power: number[];
}
const EMPTY_TRAIL: Trail = { cpu: [], mem: [], net: [], power: [] };

/** What a card needs to know about the panel: which cards are folded, how to
 *  fold one, and which jump number it has (shown once the panel has the keys). */
const CardCtx = createContext<{
  collapsed: readonly string[];
  toggle: (id: string) => void;
  numbers: Readonly<Record<string, number>>;
  showKeys: boolean;
} | null>(null);

function readCollapsed(): string[] {
  try {
    return parseCollapsed(window.localStorage.getItem(COLLAPSE_KEY));
  } catch {
    return [];
  }
}

/** History time-frame options (seconds) for the dropdown. */
const RANGES: ReadonlyArray<{ readonly label: string; secs: number }> = [
  { get label() { return t("stats.range.1h"); }, secs: 3600 },
  { get label() { return t("stats.range.6h"); }, secs: 21600 },
  { get label() { return t("stats.range.24h"); }, secs: 86400 },
  { get label() { return t("stats.range.7d"); }, secs: 604800 },
];

type Mode = "live" | "history";

export function StatsPanel({
  focused,
  onExit,
}: {
  focused: boolean;
  onExit: () => void;
}) {
  useT();
  const [mode, setMode] = useState<Mode>("live");
  const [rangeSecs, setRangeSecs] = useState<number>(21600); // default 6h
  const [stats, setStats] = useState<SystemStats | null>(null);
  const [procs, setProcs] = useState<TopProcs | null>(null);
  const [trail, setTrail] = useState<Trail>(EMPTY_TRAIL);
  const [collapsed, setCollapsed] = useState<string[]>(readCollapsed);
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
            setTrail((cur) => ({
              cpu: pushSample(cur.cpu, s.cpu_usage, TRAIL_SLOTS),
              mem: pushSample(cur.mem, usedPct(s.mem_used, s.mem_total), TRAIL_SLOTS),
              net: pushSample(cur.net, s.net_rx_per_sec + s.net_tx_per_sec, TRAIL_SLOTS),
              power: pushSample(cur.power, s.battery?.power_watts ?? s.battery?.percent ?? 0, TRAIL_SLOTS),
            }));
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

  // The process lists: their own, slower poll — and only in the live view.
  useEffect(() => {
    if (mode !== "live") return;
    let on = true;
    const load = () => {
      if (scrollingRef.current) return;
      statsTopProcesses()
        .then((p) => {
          if (on) setProcs(p);
        })
        .catch(() => {});
    };
    load();
    // The first answer has no CPU shares yet (they need two readings).
    const soon = window.setTimeout(load, 1200);
    const id = window.setInterval(load, PROC_POLL_MS);
    return () => {
      on = false;
      window.clearTimeout(soon);
      window.clearInterval(id);
    };
  }, [mode]);

  const toggle = useCallback((id: string) => {
    setCollapsed((cur) => {
      const next = toggleCollapsed(cur, id);
      try {
        window.localStorage.setItem(COLLAPSE_KEY, JSON.stringify(next));
      } catch {
        // Private mode / blocked storage: the fold still works for this session.
      }
      return next;
    });
  }, []);

  // The cards on screen, in order — the digit keys jump by this list.
  const cardIds = useMemo<CardId[]>(() => {
    if (!stats) return [];
    const ids: CardId[] = ["cpu", "memory"];
    if (stats.battery) ids.push("battery");
    if (stats.temps.length > 0) ids.push("temps");
    if (stats.fans.length > 0) ids.push("fans");
    if (visibleDisks(stats.disks).length > 0) ids.push("storage");
    ids.push("network", "host");
    return ids;
  }, [stats]);
  const numbers = useMemo(() => Object.fromEntries(cardIds.map((id, i) => [id, i + 1])), [cardIds]);

  const jump = useCallback((id: CardId) => {
    setCollapsed((cur) => (cur.includes(id) ? cur.filter((c) => c !== id) : cur));
    // After the card unfolded (next frame), bring it to the top.
    window.requestAnimationFrame(() => {
      const box = scrollRef.current;
      const el = box?.querySelector<HTMLElement>(`[data-card="${id}"]`);
      if (box && el) box.scrollTo({ top: el.offsetTop - 12 });
    });
  }, []);

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
      // The search field keeps the DOM focus while this panel is open, so its
      // key presses belong to the panel; any other text field keeps its keys.
      const tgt = e.target as HTMLElement | null;
      if (tgt && (tgt.tagName === "SELECT" || (tgt.tagName === "INPUT" && !tgt.hasAttribute("data-main-search")))) {
        return;
      }
      // ⌘/Ctrl + digit jumps to that card. A chord, because a bare digit
      // would be swallowed from whatever is typed into the search field.
      if ((IS_MAC ? e.metaKey : e.ctrlKey) && !e.altKey && !e.shiftKey && mode === "live") {
        const target = cardForKey(e.key, cardIds);
        if (target) {
          e.preventDefault();
          e.stopPropagation();
          jump(target);
        }
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
  }, [focused, onExit, mode, cardIds, jump]);

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
          <Activity size={15} className="text-[var(--color-accent)]" /> {t("stats.system_stats")}
        </div>
        <div className="flex items-center gap-1.5">
          {mode === "history" && (
            <select
              value={rangeSecs}
              onChange={(e) => setRangeSecs(Number(e.target.value))}
              className="rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] px-1.5 py-0.5 text-[11px] text-[var(--color-fg)] outline-none"
              aria-label={t("stats.history_time_range")}
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
              {t("stats.live")}
            </ModeButton>
            <ModeButton active={mode === "history"} onClick={() => setMode("history")}>
              {t("stats.history")}
            </ModeButton>
          </div>
        </div>
      </div>

      {mode === "history" ? (
        <HistoryView rangeSecs={rangeSecs} />
      ) : error ? (
        <p className="text-[12px] text-[var(--color-muted)]">{error}</p>
      ) : stats === null ? (
        <p className="text-[12px] text-[var(--color-muted)]">{t("stats.reading_system")}</p>
      ) : (
        <CardCtx.Provider value={{ collapsed, toggle, numbers, showKeys: focused }}>
          <Overview s={stats} trail={trail} jump={jump} />
          <CpuSection s={stats} procs={procs} />
          <MemorySection s={stats} procs={procs} />
          {stats.battery && <BatterySection b={stats.battery} />}
          <TemperatureSection temps={stats.temps} />
          <FanSection fans={stats.fans} />
          <DisksSection s={stats} />
          <NetworkSection s={stats} />
          <HostSection s={stats} />
        </CardCtx.Provider>
      )}
      {focused && (
        <p className="mt-auto pt-1 text-[11px] text-[var(--color-muted)]">
          {mode === "live"
            ? t("stats.keys_live", { mod: IS_MAC ? "⌘" : "Ctrl+", n: Math.min(9, cardIds.length) })
            : t("stats.scroll_esc_close")}
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
  useT();
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
  if (!hist) return <p className="text-[12px] text-[var(--color-muted)]">{t("stats.loading_history")}</p>;
  const pts = hist.points;
  if (pts.length < 2) {
    const mins = Math.round(hist.interval_secs / 60);
    return (
      <div className="rounded-xl border border-[var(--color-border)] p-4 text-center">
        <p className="text-[12px] text-[var(--color-fg)]">{t("stats.collecting_history")}</p>
        <p className="mt-1 text-[11px] text-[var(--color-muted)]">
          {t("stats.collecting_hint", { n: mins || 1 })}
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
        fmt={(v) => fmtPct(v)}
      />
      <LineChartCard
        icon={<MemoryStick size={14} />}
        title={t("stats.memory")}
        pts={pts}
        pick={(p) => p.mem}
        tMin={tMin}
        tMax={tMax}
        domain={[0, 100]}
        fmt={(v) => fmtPct(v)}
      />
      <NetworkChartCard pts={pts} tMin={tMin} tMax={tMax} />
      {hasPower && (
        <LineChartCard
          icon={<Zap size={14} />}
          title={t("stats.power_draw")}
          pts={pts}
          pick={(p) => p.power}
          tMin={tMin}
          tMax={tMax}
          fmt={(v) => fmtUnit(v, "W")}
        />
      )}
      {hasTemp && (
        <LineChartCard
          icon={<Thermometer size={14} />}
          title={t("stats.cpu_temp")}
          pts={pts}
          pick={(p) => p.cpu_temp}
          tMin={tMin}
          tMax={tMax}
          fmt={(v) => fmtUnit(v, "°C")}
        />
      )}
      {hasBattery && (
        <LineChartCard
          icon={<BatteryCharging size={14} />}
          title={t("stats.battery")}
          pts={pts}
          pick={(p) => p.battery}
          tMin={tMin}
          tMax={tMax}
          domain={[0, 100]}
          fmt={(v) => fmtPct(v)}
        />
      )}
      <p className="px-1 pb-1 text-[10px] text-[var(--color-muted)]">
        {t("stats.history_axis", { range: rangeLabel, n: hist.sample_count })}
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
  useT();
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
        <span>{t("stats.min", { v: fmt(min) })}</span>
        <span>{t("stats.avg", { v: fmt(avg) })}</span>
        <span>{t("stats.max", { v: fmt(max) })}</span>
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
  useT();
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
      title={t("stats.network")}
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
          <span className="inline-block h-[2px] w-3" style={{ backgroundColor: "var(--color-accent)" }} /> {t("stats.download")}
        </span>
        <span className="flex items-center gap-1">
          <span className="inline-block h-[2px] w-3" style={{ backgroundColor: "#f59e0b" }} /> {t("stats.upload")}
        </span>
        <span className="ml-auto tabular-nums">{t("stats.peak", { v: humanRate(peak) })}</span>
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

// ── Value tweens (v0.115.0) ─────────────────────────────────────────────────
//
// Numbers and bars glide from the old to the new value instead of snapping —
// via ONE shared rAF loop that writes inline styles / textContent through
// refs. Deliberately NOT CSS transitions and NOT React state per frame: the
// v0.84.62 lesson stands (`will-change`/`transition` on the bars left ~15
// permanent compositor layers that janked scrolling), and per-frame setState
// would re-render the whole panel 60×/s. The loop runs only while tweens are
// active (≤ STAT_TWEEN_MS per poll), then stops — idle cost is zero.
//
// Writers: each animated element is written by its ref ONLY (the raspi-monitor
// lesson — two writers on one node fight). React renders the containers; a
// per-render `reassert` layout effect re-applies the last shown value so a
// React reconciliation can never leave a stale frame.

type TweenStep = (now: number) => boolean; // false = finished
const TWEENS = new Set<TweenStep>();
let tweenRaf = 0;
function pumpTweens(now: number) {
  for (const t of Array.from(TWEENS)) {
    if (!t(now)) TWEENS.delete(t);
  }
  tweenRaf = TWEENS.size > 0 ? requestAnimationFrame(pumpTweens) : 0;
}
function addTween(t: TweenStep): () => void {
  TWEENS.add(t);
  if (!tweenRaf) tweenRaf = requestAnimationFrame(pumpTweens);
  return () => {
    TWEENS.delete(t);
    if (TWEENS.size === 0 && tweenRaf) {
      cancelAnimationFrame(tweenRaf);
      tweenRaf = 0;
    }
  };
}

/**
 * Drive `apply(v)` from the previously shown value to `target` over
 * `STAT_TWEEN_MS`. Unchanged targets don't animate; the first value and
 * reduced motion snap. A new target mid-tween starts from the SHOWN value
 * (smooth handoff, no jump). Returns a stable `reassert` — call it in a
 * no-dep layout effect so every React render re-applies the current frame.
 */
function useStatTween(target: number, apply: (v: number) => void): () => void {
  const applyRef = useRef(apply);
  // Latest-ref via effect, not a render-time write (react-hooks/refs rule;
  // same pattern as BpmDetector's onExitRef). Declared BEFORE the target
  // effect below so on a render where both change, the fresh `apply` is in
  // place when the tween starts.
  useEffect(() => {
    applyRef.current = apply;
  }, [apply]);
  const shownRef = useRef<number | null>(null);
  // Stable identity; reads the refs only when INVOKED (from a layout effect) —
  // returning a ref's value during render would trip react-hooks/refs.
  const reassert = useCallback(() => {
    if (shownRef.current != null) applyRef.current(shownRef.current);
  }, []);
  // Mount snap BEFORE first paint: the containers render empty/unstyled and
  // the tween effect below runs only after paint — without this, the first
  // frame flashed an empty number / an unscaled bar.
  const targetRef = useRef(target);
  useEffect(() => {
    targetRef.current = target;
  }, [target]);
  useLayoutEffect(() => {
    if (shownRef.current == null) {
      shownRef.current = targetRef.current;
      applyRef.current(targetRef.current);
    }
  }, []);
  useEffect(() => {
    const run = (v: number) => {
      shownRef.current = v;
      applyRef.current(v);
    };
    const from = shownRef.current;
    if (from == null || from === target || prefersReducedMotion()) {
      run(target);
      return;
    }
    const start = performance.now();
    return addTween((now) => {
      const t = Math.min(1, (now - start) / STAT_TWEEN_MS);
      run(tweenAt(from, target, t));
      return t < 1;
    });
  }, [target]);
  return reassert;
}

/**
 * A number that glides to its new value. `fmt` is read fresh each frame, so a
 * target-locked formatter (e.g. `bytesFormatterFor(target)`) keeps unit and
 * decimals stable for the whole run — no width wobble mid-tween.
 */
function TweenNum({ value, fmt }: { value: number; fmt: (v: number) => string }) {
  const ref = useRef<HTMLSpanElement>(null);
  const fmtRef = useRef(fmt);
  useEffect(() => {
    fmtRef.current = fmt;
  }, [fmt]);
  const reassert = useStatTween(value, (v) => {
    const el = ref.current;
    if (el) el.textContent = fmtRef.current(v);
  });
  useLayoutEffect(reassert);
  return <span ref={ref} />;
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
  // Sized via `transform: scaleX` (no layout), value TWEENED through the
  // shared rAF engine — still no `transition`/`will-change` (the v0.84.62
  // scroll-jank lesson: those left ~15 permanent compositor layers; a bounded
  // rAF writing inline styles composits without pinning layers).
  //
  // HEAT: with the `loadColor` scale (no explicit `color` override — an
  // override like the battery's charging-green is a different semantic, not
  // load), the fill becomes a heating filament as it enters the amber band:
  // an ember glow ramps in (`heatLevel`, 70→90 %), and at ≥90 % — exactly
  // where `loadColor` turns red, the panel's own "ausgelastet" line — the
  // molten-flow gradient + breathing glow + a directional heat beam radiating
  // from the fill's tip into the empty track arm. Those animated layers are
  // MOUNTED only while hot (usually zero on screen), so idle cost stays zero.
  const fillRef = useRef<HTMLDivElement>(null);
  const beamRef = useRef<HTMLDivElement>(null);
  const colorRef = useRef(color);
  useEffect(() => {
    colorRef.current = color;
  }, [color]);
  const reassert = useStatTween(p, (v) => {
    const fill = fillRef.current;
    if (fill) {
      fill.style.transform = `scaleX(${clampPct(v) / 100})`;
      const heat = colorRef.current ? 0 : heatLevel(v);
      fill.style.backgroundColor = colorRef.current ?? loadColor(v);
      // Ember bleed, intensity following the tweened value through the amber
      // band. Paint-only; cards are `contain: content`, so the repaint stays
      // inside the card — and only hot bars pay it at all.
      fill.style.boxShadow =
        heat > 0
          ? `0 0 ${(4 + 6 * heat).toFixed(1)}px rgba(239, 68, 68, ${(0.3 + 0.5 * heat).toFixed(2)})`
          : "";
    }
    const beam = beamRef.current;
    if (beam) beam.style.left = `${clampPct(v)}%`;
  });
  useLayoutEffect(reassert);
  const hot = color == null && isHot(p); // from the TARGET — threshold state snaps (never lags the tween)
  return (
    <div className="relative h-1.5 w-full overflow-hidden rounded-full bg-[var(--color-border)]">
      <div
        ref={fillRef}
        className={
          "absolute inset-0 origin-left rounded-full" + (hot ? " stat-heat-flow" : "")
        }
      />
      {hot && (
        <div
          ref={beamRef}
          aria-hidden
          className="stat-heat-breathe pointer-events-none absolute inset-y-0 w-1/3"
          style={{
            background:
              "linear-gradient(90deg, rgba(254, 215, 170, 0.9), rgba(239, 68, 68, 0.35) 40%, transparent)",
          }}
        />
      )}
    </div>
  );
}

function Card({
  id,
  icon,
  title,
  right,
  children,
}: {
  /** With an id the card can be folded and jumped to (live view). */
  id?: CardId;
  icon: React.ReactNode;
  title: string;
  right?: React.ReactNode;
  children: React.ReactNode;
}) {
  const ctx = useContext(CardCtx);
  const foldable = id != null && ctx != null;
  const folded = foldable && ctx.collapsed.includes(id);
  const head = (
    <>
      <span className="flex min-w-0 items-center gap-1.5 text-[12px] font-medium">
        <span className="text-[var(--color-accent)]">{icon}</span>
        <span className="truncate">{title}</span>
        {foldable && ctx.showKeys && ctx.numbers[id] != null && ctx.numbers[id] <= 9 && (
          <kbd className="rounded border border-[var(--color-border)] px-1 text-[9px] font-normal leading-4 text-[var(--color-muted)]">
            {ctx.numbers[id]}
          </kbd>
        )}
      </span>
      <span className="flex shrink-0 items-center gap-1.5 text-[11px] text-[var(--color-muted)] tabular-nums">
        {right}
        {foldable && <ChevronDown size={12} className={folded ? "-rotate-90" : undefined} />}
      </span>
    </>
  );
  return (
    <div data-card={id} className="rounded-xl border border-[var(--color-border)] p-3 [contain:content]">
      {foldable ? (
        <button
          type="button"
          aria-expanded={!folded}
          onClick={() => ctx.toggle(id)}
          className="flex w-full items-center justify-between gap-2 text-left"
        >
          {head}
        </button>
      ) : (
        <div className="flex items-center justify-between gap-2">{head}</div>
      )}
      {!folded && <div className="mt-2">{children}</div>}
    </div>
  );
}

function Kv({ k, v }: { k: string; v: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-2 text-[11px]">
      <span className="text-[var(--color-muted)]">{k}</span>
      <span className="truncate text-right tabular-nums">{v}</span>
    </div>
  );
}

// ── Sections ────────────────────────────────────────────────────────────────

/** A card's headline value: tweened, and glowing ember while its resource is
 *  hot (≥90 % — `isHot` from the TARGET, so the state never lags the tween). */
function HeadlinePct({ pct }: { pct: number }) {
  return (
    <span className={isHot(pct) ? "stat-heat-num stat-heat-breathe" : undefined}>
      <TweenNum value={pct} fmt={(v) => fmtPct(v)} />
    </span>
  );
}

// ── Overview strip ──────────────────────────────────────────────────────────

const SPARK_W = 100;
const SPARK_H = 22;

function Tile({
  label,
  value,
  sub,
  color,
  series,
  top,
  onClick,
}: {
  label: string;
  value: React.ReactNode;
  sub: React.ReactNode;
  color?: string;
  series: readonly number[];
  top?: number;
  onClick: () => void;
}) {
  const { line, area } = sparkPaths(series, TRAIL_SLOTS, SPARK_W, SPARK_H, top);
  const stroke = color ?? "var(--color-accent)";
  return (
    <button
      type="button"
      onClick={onClick}
      className="relative min-w-0 overflow-hidden rounded-xl border border-[var(--color-border)] px-2 pb-6 pt-1.5 text-left hover:border-[var(--color-accent)] [contain:content]"
    >
      <div className="truncate text-[9px] font-medium uppercase tracking-wide text-[var(--color-muted)]">{label}</div>
      <div className="truncate text-[13px] font-semibold leading-5 tabular-nums" style={color ? { color } : undefined}>
        {value}
      </div>
      <div className="truncate text-[10px] text-[var(--color-muted)] tabular-nums">{sub}</div>
      <svg
        viewBox={`0 0 ${SPARK_W} ${SPARK_H}`}
        preserveAspectRatio="none"
        aria-hidden
        className="pointer-events-none absolute inset-x-0 bottom-0 w-full" style={{ height: SPARK_H }}
      >
        {area && <path d={area} fill={stroke} opacity={0.14} />}
        {line && <path d={line} fill="none" stroke={stroke} strokeWidth={1.25} vectorEffect="non-scaling-stroke" />}
      </svg>
    </button>
  );
}

const PRESSURE_COLOR = ["#22c55e", "#f59e0b", "#ef4444"] as const;

function pressureLabel(p: number): string {
  return p >= 2 ? t("stats.pressure.critical") : p === 1 ? t("stats.pressure.raised") : t("stats.pressure.normal");
}

function loadLabel(level: LoadLevel): string {
  switch (level) {
    case "over": return t("stats.load_level.over");
    case "busy": return t("stats.load_level.busy");
    case "normal": return t("stats.load_level.normal");
    default: return t("stats.load_level.idle");
  }
}

function Overview({ s, trail, jump }: { s: SystemStats; trail: Trail; jump: (id: CardId) => void }) {
  useT();
  const memPct = usedPct(s.mem_used, s.mem_total);
  const level = s.load_avg ? loadLevel(s.load_avg[0], s.logical_cores) : null;
  const b = s.battery;
  const disk = visibleDisks(s.disks)[0];
  return (
    <div className="grid grid-cols-4 gap-2" data-testid="stats-overview">
      <Tile
        label="CPU"
        value={fmtPct(s.cpu_usage)}
        sub={level ? loadLabel(level) : s.cpu_brand}
        color={s.cpu_usage >= 70 ? loadColor(s.cpu_usage) : undefined}
        series={trail.cpu}
        top={100}
        onClick={() => jump("cpu")}
      />
      <Tile
        label={t("stats.tile.memory")}
        value={s.memory ? pressureLabel(s.memory.pressure) : fmtPct(memPct)}
        sub={t("stats.tile.used", { v: fmtPct(memPct) })}
        color={s.memory ? PRESSURE_COLOR[Math.min(2, s.memory.pressure)] : undefined}
        series={trail.mem}
        top={100}
        onClick={() => jump("memory")}
      />
      <Tile
        label={t("stats.tile.net")}
        value={humanRate(s.net_rx_per_sec)}
        sub={`↑ ${humanRate(s.net_tx_per_sec)}`}
        series={trail.net}
        onClick={() => jump("network")}
      />
      {b ? (
        <Tile
          label={b.power_watts != null ? t("stats.tile.power") : t("stats.battery")}
          value={b.power_watts != null ? fmtUnit(b.power_watts, "W") : fmtPct(b.percent)}
          sub={b.power_watts != null ? t("stats.tile.battery", { v: fmtPct(b.percent) }) : batteryStateLabel(b.state)}
          series={trail.power}
          top={b.power_watts != null ? undefined : 100}
          onClick={() => jump("battery")}
        />
      ) : (
        <Tile
          label={t("stats.storage")}
          value={disk ? humanBytes(disk.available) : "—"}
          sub={t("stats.free")}
          series={trail.power}
          onClick={() => jump("storage")}
        />
      )}
    </div>
  );
}

// ── Layered bars ────────────────────────────────────────────────────────────

/** One fill of a layered bar: width tweened through the shared engine. */
function Layer({ pct, color }: { pct: number; color: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const reassert = useStatTween(clampPct(pct), (v) => {
    if (ref.current) ref.current.style.transform = `scaleX(${clampPct(v) / 100})`;
  });
  useLayoutEffect(reassert);
  return <div ref={ref} className="absolute inset-0 origin-left" style={{ backgroundColor: color }} />;
}

/**
 * A stacked bar. Each part is drawn as a layer as wide as everything up to
 * and including it, widest first — so the parts sit side by side without any
 * layout work (transform only, like `Bar`).
 */
function StackedBar({ parts, height = "h-2" }: { parts: { key: string; pct: number; color: string }[]; height?: string }) {
  const totals = stackUpTo(parts.map((p) => p.pct));
  const layers = parts.map((p, i) => ({ key: p.key, upTo: totals[i], color: p.color }));
  return (
    <div className={`relative ${height} w-full overflow-hidden rounded-full bg-[var(--color-border)]`}>
      {layers.reverse().map((l) => (
        <Layer key={l.key} pct={l.upTo} color={l.color} />
      ))}
    </div>
  );
}

function Dot({ color }: { color: string }) {
  return <span aria-hidden className="inline-block h-2 w-2 shrink-0 rounded-sm" style={{ backgroundColor: color }} />;
}

// ── Process lists ───────────────────────────────────────────────────────────

function ProcList({
  title,
  rows,
  value,
  amount,
  empty,
}: {
  title: string;
  rows: ProcRow[];
  value: (r: ProcRow) => string;
  amount: (r: ProcRow) => number;
  empty: string;
}) {
  const top = rows.length > 0 ? amount(rows[0]) : 0;
  return (
    <div className="mt-3">
      <div className="mb-1 text-[10px] font-medium uppercase tracking-wide text-[var(--color-muted)]">{title}</div>
      {rows.length === 0 ? (
        <p className="text-[11px] text-[var(--color-muted)]">{empty}</p>
      ) : (
        <div className="flex flex-col gap-1">
          {rows.map((r) => (
            <div key={r.name} className="grid items-center gap-2 text-[11px]" style={{ gridTemplateColumns: "minmax(0,1fr) 56px 64px" }}>
              <span className="truncate" title={r.name}>
                {r.name}
                {r.procs > 1 && <span className="text-[var(--color-muted)]"> ×{r.procs}</span>}
              </span>
              <span className="relative h-1 overflow-hidden rounded-full bg-[var(--color-border)]">
                <span
                  className="absolute inset-y-0 left-0 rounded-full bg-[var(--color-accent)]"
                  style={{ width: `${shareOfTop(amount(r), top)}%` }}
                />
              </span>
              <span className="text-right tabular-nums">{value(r)}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

// ── CPU ─────────────────────────────────────────────────────────────────────

const USER_COLOR = "var(--color-accent)";
const SYSTEM_COLOR = "#f59e0b";

function CpuSection({ s, procs }: { s: SystemStats; procs: TopProcs | null }) {
  useT();
  const split = s.cpu_user != null && s.cpu_system != null ? { user: s.cpu_user, system: s.cpu_system } : null;
  const groups = splitCores(s.per_core, s.perf_cores, s.eff_cores);
  const level = s.load_avg ? loadLevel(s.load_avg[0], s.logical_cores) : null;
  return (
    <Card id="cpu" icon={<Cpu size={14} />} title="CPU" right={<HeadlinePct pct={s.cpu_usage} />}>
      {split ? (
        <>
          <StackedBar
            parts={[
              { key: "user", pct: split.user, color: USER_COLOR },
              { key: "system", pct: split.system, color: SYSTEM_COLOR },
            ]}
          />
          <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] tabular-nums">
            <span className="flex items-center gap-1.5">
              <Dot color={USER_COLOR} /> {t("stats.cpu_user")} <TweenNum value={split.user} fmt={(v) => fmtPct(v)} />
            </span>
            <span className="flex items-center gap-1.5">
              <Dot color={SYSTEM_COLOR} /> {t("stats.cpu_system")} <TweenNum value={split.system} fmt={(v) => fmtPct(v)} />
            </span>
            <span className="ml-auto text-[var(--color-muted)]">
              {t("stats.cpu_idle")} {fmtPct(Math.max(0, 100 - split.user - split.system))}
            </span>
          </div>
        </>
      ) : (
        <Bar pct={s.cpu_usage} />
      )}
      <div className="mt-2 flex items-center justify-between gap-2 text-[11px] text-[var(--color-muted)]">
        <span className="truncate">{s.cpu_brand}</span>
        <span className="shrink-0 tabular-nums">
          {groups
            ? t("stats.cores_split", { p: groups.performance.length, e: groups.efficiency.length })
            : t("stats.cores", { n: s.logical_cores })}
          {s.cpu_freq_mhz > 0 ? ` · ${fmtUnit(s.cpu_freq_mhz / 1000, "GHz", 2)}` : ""}
        </span>
      </div>
      {s.load_avg && level && (
        <div className="mt-1 flex items-center justify-between gap-2 text-[11px]">
          <span className="text-[var(--color-muted)]" title={t("stats.load_title")}>
            {t("stats.load_line", {
              a: fmtNum(s.load_avg[0], 1),
              b: fmtNum(s.load_avg[1], 1),
              c: fmtNum(s.load_avg[2], 1),
            })}
          </span>
          <span
            className="shrink-0 font-medium"
            style={{ color: level === "over" ? "#ef4444" : level === "busy" ? "#f59e0b" : undefined }}
            data-testid="stats-load-level"
          >
            {loadLabel(level)}
          </span>
        </div>
      )}
      {s.per_core.length > 1 &&
        (groups ? (
          <>
            <CoreRow label={t("stats.cores_perf")} cores={groups.performance} />
            <CoreRow label={t("stats.cores_eff")} cores={groups.efficiency} />
          </>
        ) : (
          <CoreRow label={null} cores={s.per_core.map((pct, index) => ({ index, pct }))} />
        ))}
      {procs && (
        <ProcList
          title={t("stats.top_cpu")}
          rows={procs.by_cpu}
          amount={(r) => r.cpu_pct}
          value={(r) => fmtPct(r.cpu_pct, r.cpu_pct < 10 ? 1 : 0)}
          empty={procs.cpu_ready ? t("stats.top_cpu_none") : t("stats.top_measuring")}
        />
      )}
      {procs && procs.by_cpu.length > 0 && (
        <p className="mt-1 text-[10px] text-[var(--color-muted)]">{t("stats.top_cpu_note")}</p>
      )}
    </Card>
  );
}

function CoreRow({ label, cores }: { label: string | null; cores: CoreReading[] }) {
  return (
    <div className="mt-2">
      {label && (
        <div className="mb-1 text-[10px] font-medium uppercase tracking-wide text-[var(--color-muted)]">{label}</div>
      )}
      <div className="grid gap-1" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(28px, 1fr))" }}>
        {cores.map((c) => (
          <CoreCell key={c.index} index={c.index} pct={c.pct} />
        ))}
      </div>
    </div>
  );
}

/** One core: a mini bar with its number and its load underneath. Tweened like
 *  `Bar`, but deliberately WITHOUT the glow/beam layers: a dozen tiny cells
 *  with animated shadows would pay paint for little signal; `loadColor`'s red
 *  already marks a maxed core. */
function CoreCell({ index, pct }: { index: number; pct: number }) {
  useT();
  const ref = useRef<HTMLDivElement>(null);
  const reassert = useStatTween(clampPct(pct), (v) => {
    const el = ref.current;
    if (!el) return;
    el.style.transform = `scaleY(${clampPct(v) / 100})`;
    el.style.backgroundColor = loadColor(v);
  });
  useLayoutEffect(reassert);
  return (
    <div title={t("stats.core", { n: index + 1, pct: fmtNum(pct) })} className="min-w-0">
      <div className="relative h-7 overflow-hidden rounded-sm bg-[var(--color-border)]">
        <div ref={ref} className="absolute inset-0 origin-bottom" />
      </div>
      <div className="mt-0.5 text-center text-[9px] text-[var(--color-muted)] tabular-nums">
        <TweenNum value={clampPct(pct)} fmt={(v) => fmtNum(v)} />
      </div>
    </div>
  );
}

// ── Memory ──────────────────────────────────────────────────────────────────

const MEM_COLOR: Record<MemKind, string> = {
  wired: "#a855f7",
  active: "var(--color-accent)",
  compressed: "#f59e0b",
  inactive: "#64748b",
  free: "transparent",
};

function memLabel(kind: MemKind): string {
  switch (kind) {
    case "wired": return t("stats.mem.wired");
    case "active": return t("stats.mem.active");
    case "compressed": return t("stats.mem.compressed");
    case "inactive": return t("stats.mem.inactive");
    default: return t("stats.mem.free");
  }
}

function MemorySection({ s, procs }: { s: SystemStats; procs: TopProcs | null }) {
  useT();
  const memPct = usedPct(s.mem_used, s.mem_total);
  const swapPct = usedPct(s.swap_used, s.swap_total);
  const m = s.memory ?? null;
  const segments = m ? memSegments(m) : null;
  return (
    <Card
      id="memory"
      icon={<MemoryStick size={14} />}
      title={t("stats.memory")}
      right={
        <span className="flex items-center gap-2">
          {m && (
            <span
              className="flex items-center gap-1 font-medium"
              style={{ color: PRESSURE_COLOR[Math.min(2, m.pressure)] }}
              title={t("stats.pressure.title")}
              data-testid="stats-pressure"
            >
              <span aria-hidden className="h-1.5 w-1.5 rounded-full bg-current" />
              {pressureLabel(m.pressure)}
            </span>
          )}
          <HeadlinePct pct={memPct} />
        </span>
      }
    >
      {segments ? (
        <>
          <StackedBar
            parts={segments.filter((g) => g.kind !== "free").map((g) => ({ key: g.kind, pct: g.share * 100, color: MEM_COLOR[g.kind] }))}
          />
          <div className="mt-2 grid grid-cols-2 gap-x-4 gap-y-1">
            {segments.map((g) => (
              <div key={g.kind} className="flex items-center justify-between gap-2 text-[11px]">
                <span className="flex min-w-0 items-center gap-1.5 text-[var(--color-muted)]">
                  {g.kind === "free" ? (
                    <span aria-hidden className="inline-block h-2 w-2 shrink-0 rounded-sm border border-[var(--color-border)]" />
                  ) : (
                    <Dot color={MEM_COLOR[g.kind]} />
                  )}
                  <span className="truncate">{memLabel(g.kind)}</span>
                </span>
                <span className="shrink-0 tabular-nums">{humanBytes(g.bytes)}</span>
              </div>
            ))}
            <div className="flex items-center justify-between gap-2 text-[11px]">
              <span className="text-[var(--color-muted)]">{t("stats.mem.total")}</span>
              <span className="shrink-0 tabular-nums">{humanBytes(s.mem_total)}</span>
            </div>
          </div>
        </>
      ) : (
        <>
          <Bar pct={memPct} />
          <div className="mt-1 flex items-center justify-between text-[11px] text-[var(--color-muted)] tabular-nums">
            <span>
              <TweenNum value={s.mem_used} fmt={bytesFormatterFor(s.mem_used)} /> / {humanBytes(s.mem_total)}
            </span>
            {s.mem_available > 0 && (
              <span>
                <TweenNum value={s.mem_available} fmt={bytesFormatterFor(s.mem_available)} /> {t("stats.free")}
              </span>
            )}
          </div>
        </>
      )}
      {s.swap_total > 0 && (
        <div className="mt-3">
          <div className="mb-1 flex items-center justify-between gap-2 text-[11px] tabular-nums">
            <span className="text-[var(--color-muted)]">{t("stats.swap")}</span>
            <span>
              {humanBytes(s.swap_used)} / {humanBytes(s.swap_total)}
            </span>
          </div>
          <Bar pct={swapPct} color={SYSTEM_COLOR} />
          {m && (m.swapout_bps >= 1024 || m.swapin_bps >= 1024) && (
            <div className="mt-1 flex items-center gap-3 text-[11px] tabular-nums" data-testid="stats-swap-flow">
              {m.swapout_bps >= 1024 && (
                <span style={{ color: "#f59e0b" }}>{t("stats.swap_out", { v: humanRate(m.swapout_bps) })}</span>
              )}
              {m.swapin_bps >= 1024 && (
                <span className="text-[var(--color-muted)]">{t("stats.swap_in", { v: humanRate(m.swapin_bps) })}</span>
              )}
            </div>
          )}
        </div>
      )}
      {procs && (
        <ProcList
          title={t("stats.top_mem")}
          rows={procs.by_mem}
          amount={(r) => r.mem}
          value={(r) => humanBytes(r.mem)}
          empty={t("stats.top_measuring")}
        />
      )}
    </Card>
  );
}

// ── Battery ─────────────────────────────────────────────────────────────────

// The battery state arrives as the Rust enum name; show it in the app language.
function batteryStateLabel(state: string): string {
  switch (state) {
    case "Charging": return t("stats.battery.charging");
    case "Discharging": return t("stats.battery.discharging");
    case "Full": return t("stats.battery.full");
    case "Empty": return t("stats.battery.empty");
    case "Unknown": return t("stats.battery.unknown");
    default: return state;
  }
}

function BatterySection({ b }: { b: NonNullable<SystemStats["battery"]> }) {
  useT();
  const charging = b.state === "Charging" || b.state === "Full";
  const time =
    b.state === "Charging" && b.time_to_full_secs != null
      ? t("stats.to_full", { d: humanDuration(b.time_to_full_secs) })
      : b.state === "Discharging" && b.time_to_empty_secs != null
        ? t("stats.left", { d: humanDuration(b.time_to_empty_secs) })
        : null;
  return (
    <Card
      id="battery"
      icon={<BatteryCharging size={14} />}
      title={t("stats.battery_power")}
      right={<TweenNum value={b.percent} fmt={(v) => fmtPct(v)} />}
    >
      <Bar pct={b.percent} color={charging ? "#22c55e" : b.percent <= 15 ? "#ef4444" : "var(--color-accent)"} />
      <div className="mt-2 grid grid-cols-2 gap-x-4 gap-y-1">
        <Kv k={t("stats.state")} v={batteryStateLabel(b.state)} />
        {time && <Kv k={t("stats.time")} v={time} />}
        {b.power_watts != null && (
          <Kv
            k={t("stats.power_draw")}
            v={
              <span className="inline-flex items-center gap-1 font-medium">
                <Zap size={11} className="text-[var(--color-accent)]" />
                <TweenNum value={b.power_watts} fmt={(v) => fmtUnit(v, "W")} />
              </span>
            }
          />
        )}
        {b.health_percent != null && <Kv k={t("stats.health")} v={fmtPct(b.health_percent)} />}
        {b.cycle_count != null && <Kv k={t("stats.cycles")} v={fmtNum(b.cycle_count)} />}
        {b.temperature_c != null && (
          <Kv k={t("stats.temp")} v={<TweenNum value={b.temperature_c} fmt={(v) => fmtUnit(v, "°C")} />} />
        )}
        {(b.model || b.vendor) && <Kv k={t("stats.model")} v={b.model || b.vendor || ""} />}
      </div>
    </Card>
  );
}

// ── Temperatures + fans ─────────────────────────────────────────────────────

function TemperatureMeter({ celsius }: { celsius: number }) {
  const fillRef = useRef<HTMLDivElement>(null);
  const markerRef = useRef<HTMLDivElement>(null);
  const level = temperatureLevel(celsius);
  const reassert = useStatTween(level, (value) => {
    const pct = clampPct(value);
    if (fillRef.current) fillRef.current.style.transform = `scaleX(${pct / 100})`;
    if (markerRef.current) markerRef.current.style.left = `${pct}%`;
  });
  useLayoutEffect(reassert);
  return (
    <div className="relative h-2 overflow-visible rounded-full bg-[var(--color-border)]/70">
      <div
        ref={fillRef}
        className="absolute inset-0 origin-left rounded-full"
        style={{ background: "linear-gradient(90deg, var(--color-accent), #f59e0b 68%, #ef4444)" }}
      />
      <div
        ref={markerRef}
        className="absolute top-1/2 h-2.5 w-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-[var(--color-bg)]"
        style={{ backgroundColor: temperatureColor(celsius), boxShadow: `0 0 8px ${temperatureColor(celsius)}` }}
      />
    </div>
  );
}

function tempGroupLabel(key: TempGroupKey): string {
  switch (key) {
    case "cpu": return t("stats.temp_group.cpu");
    case "gpu": return t("stats.temp_group.gpu");
    case "battery": return t("stats.temp_group.battery");
    case "storage": return t("stats.temp_group.storage");
    default: return t("stats.temp_group.other");
  }
}

function TemperatureSection({ temps }: { temps: SystemStats["temps"] }) {
  useT();
  const [open, setOpen] = useState<readonly TempGroupKey[]>([]);
  if (temps.length === 0) return null;
  const groups = groupTemps(temps);
  const hottest = Math.max(...temps.map((x) => x.celsius));
  return (
    <Card
      id="temps"
      icon={<Thermometer size={14} />}
      title={t("stats.temperatures")}
      right={
        <span style={{ color: temperatureColor(hottest) }}>
          {t("stats.max", { v: fmtUnit(hottest, "°C") })}
        </span>
      }
    >
      <div className="flex flex-col gap-2.5">
        {groups.map((g) => {
          const isOpen = open.includes(g.key);
          return (
            <div key={g.key}>
              <button
                type="button"
                aria-expanded={isOpen}
                onClick={() => setOpen((cur) => (cur.includes(g.key) ? cur.filter((k) => k !== g.key) : [...cur, g.key]))}
                className="mb-1 flex w-full items-center justify-between gap-3 text-left text-[11px]"
              >
                <span className="flex min-w-0 items-center gap-1 text-[var(--color-muted)]">
                  <ChevronRight size={11} className={isOpen ? "rotate-90" : undefined} />
                  <span className="truncate">{tempGroupLabel(g.key)}</span>
                  {g.sensors.length > 1 && <span className="shrink-0">· {t("stats.sensors", { n: g.sensors.length })}</span>}
                </span>
                <span className="shrink-0 tabular-nums">
                  {g.sensors.length > 1 && (
                    <span className="mr-2 text-[var(--color-muted)]">{t("stats.avg", { v: fmtUnit(g.avg, "°C") })}</span>
                  )}
                  <span className="font-medium" style={{ color: temperatureColor(g.max) }}>
                    <TweenNum value={g.max} fmt={(v) => fmtUnit(v, "°C")} />
                  </span>
                </span>
              </button>
              <TemperatureMeter celsius={g.max} />
              {isOpen && (
                <div className="mt-1.5 grid grid-cols-2 gap-x-4 gap-y-0.5">
                  {g.sensors.map((sensor, i) => (
                    <div key={`${sensor.label}-${i}`} className="flex items-center justify-between gap-2 text-[10px]">
                      <span className="truncate text-[var(--color-muted)]" title={sensor.label}>{sensor.label}</span>
                      <span className="shrink-0 tabular-nums" style={{ color: temperatureColor(sensor.celsius) }}>
                        {fmtUnit(sensor.celsius, "°C")}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </Card>
  );
}

function FanSection({ fans }: { fans: SystemStats["fans"] }) {
  useT();
  if (fans.length === 0) return null;
  return (
    <Card id="fans" icon={<Fan size={14} />} title={t("stats.fans")} right={fmtNum(fans.length)}>
      <div className="flex flex-col gap-2">
        {fans.map((fan, index) => (
          <div key={`${fan.label}-${index}`} className="flex items-center gap-2.5">
            <Fan
              size={16}
              className={"shrink-0 text-[var(--color-accent)]" + (fan.rpm > 0 ? " stat-fan-spin" : "")}
              style={{ animationDuration: fanDuration(fan.rpm) }}
            />
            <div className="min-w-0 flex-1">
              <div className="mb-1 flex items-center justify-between gap-2 text-[11px]">
                <span className="truncate text-[var(--color-muted)]" title={fan.label}>{fan.label}</span>
                <span className="shrink-0 font-medium tabular-nums">
                  <TweenNum value={fan.rpm} fmt={(value) => fmtUnit(Math.max(0, value), t("stats.rpm"), 0)} />
                </span>
              </div>
              <Bar pct={clampPct(fan.rpm / 60)} color="var(--color-accent)" />
            </div>
          </div>
        ))}
      </div>
    </Card>
  );
}

// ── Storage, network, host ──────────────────────────────────────────────────

function DisksSection({ s }: { s: SystemStats }) {
  useT();
  const disks = visibleDisks(s.disks);
  if (disks.length === 0) return null;
  const io = s.disk_io ?? null;
  return (
    <Card
      id="storage"
      icon={<HardDrive size={14} />}
      title={t("stats.storage")}
      right={t("stats.free_n", { v: humanBytes(disks[0].available) })}
    >
      <div className="flex flex-col gap-2.5">
        {disks.map((d, i) => {
          const used = d.total - d.available;
          return (
            <div key={`${d.mount}-${i}`}>
              <div className="mb-1 flex items-center justify-between gap-2 text-[11px]">
                <span className="min-w-0 truncate" title={d.mount}>
                  {d.name || d.mount}
                  <span className="text-[var(--color-muted)]"> · {d.mount}</span>
                </span>
                <span className="shrink-0 tabular-nums text-[var(--color-muted)]">
                  {t("stats.free_of", { free: humanBytes(d.available), total: humanBytes(d.total) })}
                </span>
              </div>
              <Bar pct={usedPct(used, d.total)} />
            </div>
          );
        })}
      </div>
      {io && (
        <div className="mt-2.5 grid grid-cols-2 gap-x-4" data-testid="stats-disk-io">
          <Kv k={t("stats.disk_read")} v={<TweenNum value={io.read_bps} fmt={rateFormatterFor(io.read_bps)} />} />
          <Kv k={t("stats.disk_write")} v={<TweenNum value={io.write_bps} fmt={rateFormatterFor(io.write_bps)} />} />
        </div>
      )}
    </Card>
  );
}

function NetworkSection({ s }: { s: SystemStats }) {
  useT();
  const ifaces = s.net_ifaces ?? [];
  return (
    <Card id="network" icon={<ArrowDownUp size={14} />} title={t("stats.network")}>
      <div className="grid grid-cols-2 gap-x-4 text-[12px] tabular-nums">
        <span className="flex items-center justify-between gap-2">
          <span className="text-[11px] text-[var(--color-muted)]">↓ {t("stats.download")}</span>
          <TweenNum value={s.net_rx_per_sec} fmt={rateFormatterFor(s.net_rx_per_sec)} />
        </span>
        <span className="flex items-center justify-between gap-2">
          <span className="text-[11px] text-[var(--color-muted)]">↑ {t("stats.upload")}</span>
          <TweenNum value={s.net_tx_per_sec} fmt={rateFormatterFor(s.net_tx_per_sec)} />
        </span>
      </div>
      {ifaces.length > 0 && (
        <div className="mt-2 flex flex-col gap-1 border-t border-[var(--color-border)] pt-2">
          {ifaces.map((n) => (
            <div key={n.name} className="grid items-center gap-2 text-[11px]" style={{ gridTemplateColumns: "minmax(0,1fr) 72px 72px" }}>
              <span className="min-w-0 truncate">
                <span className="font-medium">{n.name}</span>
                {n.addr && <span className="font-[var(--font-mono)] text-[10px] text-[var(--color-muted)]"> {n.addr}</span>}
              </span>
              <span className="text-right tabular-nums">↓ {humanRate(n.rx_per_sec)}</span>
              <span className="text-right tabular-nums">↑ {humanRate(n.tx_per_sec)}</span>
            </div>
          ))}
        </div>
      )}
    </Card>
  );
}

function HostSection({ s }: { s: SystemStats }) {
  useT();
  return (
    <Card id="host" icon={<Server size={14} />} title={t("stats.host")} right={humanUptime(s.uptime_secs)}>
      <div className="grid grid-cols-1 gap-1">
        {s.host_name && <Kv k={t("stats.host")} v={s.host_name} />}
        {s.os_name && <Kv k={t("stats.os")} v={s.os_name.trim()} />}
        {s.kernel && <Kv k={t("stats.kernel")} v={s.kernel} />}
        {s.cpu_arch && <Kv k={t("stats.arch")} v={s.cpu_arch} />}
        <Kv k={t("stats.uptime")} v={humanUptime(s.uptime_secs)} />
      </div>
    </Card>
  );
}
