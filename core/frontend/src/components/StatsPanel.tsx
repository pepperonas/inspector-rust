import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
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
import {
  STAT_TWEEN_MS,
  tweenAt,
  heatLevel,
  isHot,
  bytesFormatterFor,
  rateFormatterFor,
} from "../lib/stats-anim";
import { prefersReducedMotion } from "../lib/md3-motion";

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
      <TweenNum value={pct} fmt={(v) => `${v.toFixed(0)}%`} />
    </span>
  );
}

function CpuSection({ s }: { s: SystemStats }) {
  const cores = s.physical_cores
    ? `${s.physical_cores}C / ${s.logical_cores}T`
    : `${s.logical_cores} threads`;
  return (
    <Card
      icon={<Cpu size={14} />}
      title="CPU"
      right={<HeadlinePct pct={s.cpu_usage} />}
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
            <CoreCell key={i} index={i} pct={u} />
          ))}
        </div>
      )}
    </Card>
  );
}

/** One per-core mini bar — tweened like `Bar`, but deliberately WITHOUT the
 *  glow/beam layers: 8–10 tiny cells with animated shadows would pay paint
 *  for little signal; `loadColor`'s red already marks a maxed core. */
function CoreCell({ index, pct }: { index: number; pct: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const reassert = useStatTween(clampPct(pct), (v) => {
    const el = ref.current;
    if (!el) return;
    el.style.transform = `scaleY(${clampPct(v) / 100})`;
    el.style.backgroundColor = loadColor(v);
  });
  useLayoutEffect(reassert);
  return (
    <div
      title={`Core ${index}: ${pct.toFixed(0)}%`}
      className="relative h-7 overflow-hidden rounded-sm bg-[var(--color-border)]"
    >
      <div ref={ref} className="absolute inset-0 origin-bottom" />
    </div>
  );
}

function MemorySection({ s }: { s: SystemStats }) {
  const memPct = usedPct(s.mem_used, s.mem_total);
  const swapPct = usedPct(s.swap_used, s.swap_total);
  return (
    <Card icon={<MemoryStick size={14} />} title="Memory" right={<HeadlinePct pct={memPct} />}>
      <Bar pct={memPct} />
      <div className="mt-1 flex items-center justify-between text-[11px] text-[var(--color-muted)] tabular-nums">
        <span>
          <TweenNum value={s.mem_used} fmt={bytesFormatterFor(s.mem_used)} /> /{" "}
          {humanBytes(s.mem_total)}
        </span>
        <span>
          <TweenNum value={s.mem_available} fmt={bytesFormatterFor(s.mem_available)} /> free
        </span>
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
      right={<TweenNum value={b.percent} fmt={(v) => `${v.toFixed(0)}%`} />}
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
            <TweenNum value={b.power_watts} fmt={(v) => `${v.toFixed(1)} W`} />
          </span>
        )}
      </div>
      <div className="mt-2 grid grid-cols-2 gap-x-3 gap-y-1">
        {b.health_percent != null && (
          <Kv k="Health" v={`${b.health_percent.toFixed(0)}%`} />
        )}
        {b.cycle_count != null && <Kv k="Cycles" v={`${b.cycle_count}`} />}
        {b.temperature_c != null && (
          <Kv k="Temp" v={<TweenNum value={b.temperature_c} fmt={(v) => `${v.toFixed(1)}°C`} />} />
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
            <Kv
              key={t.label}
              k={t.label}
              v={<TweenNum value={t.celsius} fmt={(v) => `${v.toFixed(1)}°C`} />}
            />
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
              <span className="tabular-nums">
                <TweenNum value={f.rpm} fmt={(v) => `${Math.round(v)} rpm`} />
              </span>
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
          <TweenNum value={s.net_rx_per_sec} fmt={rateFormatterFor(s.net_rx_per_sec)} />
        </span>
        <span className="flex items-center gap-1.5">
          <span className="text-[var(--color-muted)]">↑</span>
          <TweenNum value={s.net_tx_per_sec} fmt={rateFormatterFor(s.net_tx_per_sec)} />
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
