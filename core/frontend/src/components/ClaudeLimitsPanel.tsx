/**
 * `limits` / `usage` — the Claude subscription's usage limits (session, week,
 * per-model week, extra usage) as claude.ai shows them. Data and caching live
 * in `claude_limits.rs`: this panel asks every 30 s while it is open, and Rust
 * only touches the network when the poll interval is due. R forces a refresh,
 * Esc exits.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { AlertCircle, ChevronDown, Gauge, Loader2, RefreshCw } from "lucide-react";
import {
  claudeLimitsStatus,
  getLimitsForecastOpen,
  setClaudeLimitsPoll,
  setLimitsForecastOpen,
  type ClaudeLimit,
  type ClaudeLimitsStatus,
  type LimitForecast,
} from "../lib/ipc";
import {
  POLL_CHOICES,
  absoluteReset,
  barSegments,
  chartModel,
  forecastText,
  forecastTone,
  formatMoney,
  formatPercent,
  limitTone,
  limitsErrorText,
  limitsPhase,
  relativeReset,
  sinceLabel,
  standLabel,
  type LimitTone,
} from "../lib/claude-limits";

const ASK_EVERY_MS = 30_000;

const TONE_BAR: Record<LimitTone, string> = {
  ok: "bg-[color:var(--color-accent)]",
  warn: "bg-amber-500",
  crit: "bg-rose-500",
};

const TONE_TEXT: Record<LimitTone, string> = {
  ok: "text-emerald-600",
  warn: "text-amber-600",
  crit: "text-rose-600",
};

const TONE_STROKE: Record<LimitTone, string> = {
  ok: "var(--color-accent)",
  warn: "rgb(245 158 11)",
  crit: "rgb(244 63 94)",
};

/** Only weekly-ish windows get the chart (the tracker sends `series` there). */
function hasChart(f: LimitForecast | null): boolean {
  return !!f?.series && !!f.window && (f.series.actual.length > 1 || f.series.forecast.length > 1);
}

/**
 * Window chart (v0.196.0): hand-rolled SVG like StatsPanel — plan dashed,
 * actual, measured points, median dashed + band, 100 % crossing, "now",
 * up to 3 previous weeks as ghosts, nights 22–7 shaded, red zone over 100.
 * No animation (nothing to reduce for `prefers-reduced-motion`).
 */
function LimitChart({ f, tone }: { f: LimitForecast; tone: LimitTone }) {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(300);
  const [hoverX, setHoverX] = useState<number | null>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => setWidth(Math.max(120, el.clientWidth)));
    ro.observe(el);
    setWidth(Math.max(120, el.clientWidth));
    return () => ro.disconnect();
  }, []);
  const H = 110;
  const m = chartModel(f, width, H);
  if (!m || !f.window) return null;
  const s0 = Date.parse(f.window.start);
  const s1 = Date.parse(f.window.end);
  const tAt = (x: number) => s0 + (x / width) * (s1 - s0);
  const valueAt = (pts: [string, number][], t: number): number | null => {
    let best: number | null = null;
    let bd = Infinity;
    for (const [iso, v] of pts) {
      const d = Math.abs(Date.parse(iso) - t);
      if (d < bd) {
        bd = d;
        best = v;
      }
    }
    return bd < 3 * 3_600_000 ? best : null;
  };
  const series = f.series!;
  const hover =
    hoverX == null
      ? null
      : (() => {
          const t = tAt(hoverX);
          const fc = series.forecast.map((p) => [p[0], p[1]] as [string, number]);
          return {
            t,
            ist: valueAt(series.actual, t),
            plan: (100 * (t - s0)) / (s1 - s0),
            prog: valueAt(fc, t),
          };
        })();
  const stroke = TONE_STROKE[tone];
  return (
    <div ref={ref} className="relative mt-2" data-testid="limit-chart">
      <svg
        width={width}
        height={H}
        className="block overflow-visible"
        onMouseMove={(e) => setHoverX(e.nativeEvent.offsetX)}
        onMouseLeave={() => setHoverX(null)}
        role="img"
        aria-label="Verlauf und Prognose im Fenster"
      >
        {m.nights.map((n, i) => (
          <rect key={i} x={n.x} y={0} width={n.w} height={H} fill="currentColor" className="text-[color:var(--color-fg)]" opacity={0.04} />
        ))}
        {m.yMax > 100 && <rect x={0} y={0} width={width} height={m.y100} fill="rgb(244 63 94)" opacity={0.08} />}
        <line x1={0} x2={width} y1={m.y100} y2={m.y100} stroke="currentColor" className="text-[color:var(--color-border)]" vectorEffect="non-scaling-stroke" />
        {m.ghosts.map((d, i) => (
          <path key={i} d={d} fill="none" stroke="currentColor" className="text-[color:var(--color-muted)]" opacity={0.25} strokeWidth={1} vectorEffect="non-scaling-stroke" />
        ))}
        <path d={m.plan} fill="none" stroke="currentColor" className="text-[color:var(--color-muted)]" strokeDasharray="3 3" strokeWidth={1} vectorEffect="non-scaling-stroke" />
        {m.band && <path d={m.band} fill={stroke} opacity={0.15} />}
        <path d={m.median} fill="none" stroke={stroke} strokeDasharray="4 3" strokeWidth={1.5} vectorEffect="non-scaling-stroke" />
        <path d={m.actual} fill="none" stroke={stroke} strokeWidth={1.75} vectorEffect="non-scaling-stroke" />
        {m.measured.map((p, i) => (
          <circle key={i} cx={p.x} cy={p.y} r={2} fill={stroke} />
        ))}
        {m.nowX != null && (
          <line x1={m.nowX} x2={m.nowX} y1={0} y2={H} stroke="currentColor" className="text-[color:var(--color-fg)]" opacity={0.35} vectorEffect="non-scaling-stroke" />
        )}
        {m.hit && (
          <g data-testid="limit-chart-hit">
            <circle cx={m.hit.x} cy={m.hit.y} r={3.5} fill="rgb(244 63 94)" />
            <text x={Math.min(m.hit.x + 5, width - 70)} y={Math.max(m.hit.y - 5, 10)} fontSize={10} fill="rgb(244 63 94)">
              {absoluteReset(m.hit.ms)}
            </text>
          </g>
        )}
        {hoverX != null && (
          <line x1={hoverX} x2={hoverX} y1={0} y2={H} stroke="currentColor" className="text-[color:var(--color-muted)]" opacity={0.5} vectorEffect="non-scaling-stroke" />
        )}
      </svg>
      {hover && (
        <div
          className="pointer-events-none absolute top-0 rounded bg-[color:var(--color-bg)] px-1.5 py-0.5 text-[10px] tabular-nums text-[color:var(--color-fg)] shadow"
          style={{ left: Math.min(Math.max(0, (hoverX ?? 0) + 6), width - 130) }}
        >
          {absoluteReset(hover.t)}
          {hover.ist != null && ` · Ist ${formatPercent(hover.ist)}`}
          {` · Plan ${formatPercent(hover.plan)}`}
          {hover.prog != null && ` · Prognose ${formatPercent(hover.prog)}`}
        </div>
      )}
      <div className="mt-0.5 flex justify-between text-[10px] text-[color:var(--color-muted)]">
        <span>{absoluteReset(s0)}</span>
        <span>
          gestrichelt: Plan / Prognose
          {f.notes.includes("chat_invisible") && " · Chat-Nutzung fließt nur über den Kalibrierfaktor ein"}
        </span>
        <span>{absoluteReset(s1)}</span>
      </div>
    </div>
  );
}

function LimitRow({
  l,
  now,
  open = false,
  onToggle,
}: {
  l: ClaudeLimit;
  now: number;
  open?: boolean;
  onToggle?: (id: string) => void;
}) {
  const reset = l.resets_at ? Date.parse(l.resets_at) : NaN;
  const f = l.forecast ?? null;
  const seg = barSegments(l.percent, f);
  const fTone = forecastTone(f);
  const fText = forecastText(f, now);
  const chart = hasChart(f) && !!onToggle;
  return (
    <div className="rounded-lg bg-[color:var(--color-surface)] px-3 py-2">
      <div className="flex items-baseline justify-between gap-2">
        <div className="min-w-0 truncate font-medium text-[color:var(--color-fg)]">
          {chart && (
            <button
              type="button"
              onClick={() => onToggle!(l.id)}
              className="md3-press mr-1 inline-flex align-middle text-[color:var(--color-muted)]"
              aria-expanded={open}
              aria-label={open ? "Grafik zuklappen" : "Grafik aufklappen"}
              title={open ? "Grafik zuklappen" : "Verlauf und Prognose zeigen"}
            >
              <ChevronDown size={13} className={`transition-transform duration-(--duration-fast) ease-sharp ${open ? "" : "-rotate-90"}`} />
            </button>
          )}
          {l.name}
          {!l.known && (
            <span
              className="ml-1.5 rounded bg-[color:var(--color-bg)] px-1 text-[10px] font-normal text-[color:var(--color-muted)]"
              title="Von dieser Version nicht erkannter Limit-Typ — Rohname der API"
            >
              unbekannt
            </span>
          )}
        </div>
        <div className="shrink-0 font-mono text-[12px] tabular-nums text-[color:var(--color-fg)]">
          {formatPercent(l.percent)}
        </div>
      </div>
      <div className="relative mt-1.5 h-1.5 overflow-hidden rounded-full bg-[color:var(--color-bg)]">
        {seg.projection != null && (
          <div
            data-testid="bar-projection"
            className="absolute inset-y-0"
            style={{
              left: `${seg.fill}%`,
              width: `${seg.projection - seg.fill}%`,
              backgroundImage: `repeating-linear-gradient(135deg, ${TONE_STROKE[fTone ?? "ok"]} 0 2px, transparent 2px 5px)`,
              opacity: 0.55,
            }}
          />
        )}
        {seg.over && <div data-testid="bar-over" className="absolute inset-y-0 right-0 w-1 bg-rose-500" />}
        <div
          className={`relative h-full origin-left rounded-full transition-transform duration-(--duration-slow) ease-sharp ${TONE_BAR[limitTone(l.percent)]}`}
          style={{ transform: `scaleX(${seg.fill / 100})` }}
        />
        {seg.plan != null && (
          <div
            data-testid="bar-plan"
            className="absolute inset-y-0 w-px bg-[color:var(--color-fg)] opacity-60"
            style={{ left: `${seg.plan}%` }}
            title={`Plan: ${formatPercent(seg.plan)}`}
          />
        )}
      </div>
      <div className="mt-1 flex justify-between gap-2 text-[11px] text-[color:var(--color-muted)]">
        <span>
          {Number.isFinite(reset)
            ? `Zurückgesetzt ${relativeReset(reset, now)}`
            : "Kein Rücksetzzeitpunkt"}
        </span>
        <span className="tabular-nums">
          {l.money
            ? `${formatMoney(l.money.used, l.money.currency)} / ${formatMoney(l.money.limit, l.money.currency)}`
            : Number.isFinite(reset)
              ? absoluteReset(reset)
              : ""}
        </span>
      </div>
      {l.money && Number.isFinite(reset) && (
        <div className="text-right text-[11px] tabular-nums text-[color:var(--color-muted)]">
          {absoluteReset(reset)}
        </div>
      )}
      {fText && (
        <div
          data-testid="forecast-line"
          className={`mt-0.5 text-[11px] ${fTone ? TONE_TEXT[fTone] : "text-[color:var(--color-muted)]"}`}
        >
          {fText}
        </div>
      )}
      {chart && open && f && <LimitChart f={f} tone={fTone ?? "ok"} />}
    </div>
  );
}

/** True when some limit only has the local linear estimate. */
function anyLocal(limits: ClaudeLimit[]): boolean {
  return limits.some((l) => l.forecast?.source === "local" && l.forecast.basis !== "none");
}

function SectionTitle({ title, note }: { title: string; note?: string }) {
  return (
    <div className="flex items-baseline justify-between px-1 pt-2 text-[11px] font-semibold uppercase tracking-wide text-[color:var(--color-muted)]">
      <span>{title}</span>
      {note && <span className="font-normal normal-case tracking-normal">{note}</span>}
    </div>
  );
}

export function ClaudeLimitsPanel({
  focused,
  onExit,
}: {
  focused: boolean;
  onExit: () => void;
}) {
  const [st, setSt] = useState<ClaudeLimitsStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const seq = useRef(0);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [openCharts, setOpenCharts] = useState<string[]>([]);

  useEffect(() => {
    void getLimitsForecastOpen()
      .then((ids) => setOpenCharts(Array.isArray(ids) ? ids : []))
      .catch(() => undefined);
  }, []);

  const toggleChart = useCallback((id: string) => {
    setOpenCharts((prev) => {
      const next = prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id];
      void setLimitsForecastOpen(next).catch(() => undefined);
      return next;
    });
  }, []);

  const load = useCallback(async (force: boolean) => {
    const my = ++seq.current;
    setLoading(true);
    try {
      const s = await claudeLimitsStatus(force);
      if (my === seq.current) setSt(s);
    } catch (e) {
      if (my === seq.current)
        setSt((prev) => ({
          report: prev?.report ?? null,
          fetched_at_ms: prev?.fetched_at_ms ?? null,
          error: String(e),
          error_detail: null,
          retry_at_ms: null,
          poll_minutes: prev?.poll_minutes ?? 5,
          codex: prev?.codex ?? null,
          antigravity: prev?.antigravity ?? null,
        }));
    } finally {
      if (my === seq.current) setLoading(false);
      setNow(Date.now());
    }
  }, []);

  // Ask on open and every 30 s while open. Rust decides whether that is a
  // network request or the cache.
  useEffect(() => {
    void load(false);
    const t = window.setInterval(() => void load(false), ASK_EVERY_MS);
    return () => window.clearInterval(t);
  }, [load]);

  useEffect(() => {
    if (!focused) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onExit();
        return;
      }
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable))
        return;
      if (e.key === "r" || e.key === "R") {
        e.preventDefault();
        void load(true);
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
  }, [focused, onExit, load]);

  const report = st?.report ?? null;
  const phase = limitsPhase(!!report, st?.error ?? null, st?.fetched_at_ms ?? null, st?.poll_minutes ?? 5, now);
  const errText = limitsErrorText(st?.error ?? null);
  const year = new Date(now).getFullYear();

  return (
    <div className="flex h-full min-h-0 flex-col gap-2 p-3 text-[13px]">
      <div className="flex items-center justify-between gap-2">
        <div className="flex items-center gap-1.5 font-medium text-[color:var(--color-fg)]">
          <Gauge size={14} className="text-rose-500" />
          Claude-Limits
        </div>
        <div className="flex items-center gap-1.5 text-[11px] text-[color:var(--color-muted)]">
          {phase === "veraltet" && (
            <span className="rounded bg-amber-500/15 px-1.5 py-0.5 text-amber-600">veraltet</span>
          )}
          {st?.fetched_at_ms != null && <span>{standLabel(st.fetched_at_ms)}</span>}
          <select
            value={st?.poll_minutes ?? 5}
            onChange={async (e) => {
              await setClaudeLimitsPoll(Number(e.target.value));
              void load(false);
            }}
            className="rounded bg-[color:var(--color-surface)] px-1 py-0.5 text-[11px] text-[color:var(--color-fg)]"
            title="Abfrage-Intervall"
            aria-label="Abfrage-Intervall"
          >
            {POLL_CHOICES.map((m) => (
              <option key={m} value={m}>
                alle {m} min
              </option>
            ))}
          </select>
          <button
            type="button"
            onClick={() => void load(true)}
            className="md3-press rounded-md p-1 hover:bg-[color:var(--color-surface)]"
            title="Aktualisieren (R)"
            aria-label="Aktualisieren"
          >
            <RefreshCw size={13} className={loading ? "animate-spin" : ""} />
          </button>
        </div>
      </div>

      {errText && (
        <div
          className={`flex items-start gap-1.5 rounded-lg px-2.5 py-1.5 text-[12px] ${
            phase === "fehler" ? "bg-rose-500/10 text-rose-600" : "bg-amber-500/10 text-amber-700"
          }`}
        >
          <AlertCircle size={13} className="mt-0.5 shrink-0" />
          <span>
            {errText}
            {phase === "veraltet" && st?.fetched_at_ms != null && ` Angezeigt: ${standLabel(st.fetched_at_ms)}.`}
            {st?.retry_at_ms != null && ` Nächster Versuch ${relativeReset(st.retry_at_ms, now)}.`}
          </span>
        </div>
      )}

      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto pr-0.5">
        {phase === "laden" && (
          <div className="flex items-center gap-2 py-6 text-[color:var(--color-muted)]">
            <Loader2 size={14} className="animate-spin" /> lädt…
          </div>
        )}
        {(report || st?.codex || st?.antigravity) && (
          <SectionTitle title="Claude" />
        )}
        {report && (
          <div className="flex flex-col gap-1.5">
            {report.limits.map((l) => (
              <LimitRow key={l.id} l={l} now={now} open={openCharts.includes(l.id)} onToggle={toggleChart} />
            ))}
            {anyLocal(report.limits) && (
              <div className="px-1 text-[11px] text-[color:var(--color-muted)]" data-testid="tracker-hint">
                Hochrechnung linear aus dem aktuellen Stand — Token Tracker starten für Verlauf und genauere Prognose.
              </div>
            )}
            {report.limits.length === 0 && (
              <div className="py-4 text-[color:var(--color-muted)]">Die API meldet keine Limits.</div>
            )}

            {report.extra && (
              <div className="rounded-lg bg-[color:var(--color-surface)] px-3 py-2 text-[12px]">
                <div className="flex justify-between">
                  <span className="font-medium text-[color:var(--color-fg)]">Zusätzliche Nutzung</span>
                  <span className="text-[color:var(--color-muted)]">
                    {report.extra.enabled ? "aktiv" : "aus"}
                  </span>
                </div>
                <div className="mt-0.5 tabular-nums text-[color:var(--color-muted)]">
                  {formatMoney(report.extra.used, report.extra.currency)} von{" "}
                  {formatMoney(report.extra.limit, report.extra.currency)} · {formatPercent(report.extra.percent)}
                </div>
              </div>
            )}

            {report.breakdown.length > 0 && (
              <div className="px-1 pt-1 text-[11px] text-[color:var(--color-muted)]">
                Wochennutzung nach Bereich:{" "}
                {report.breakdown.map((b) => `${b.name} ${formatPercent(b.percent)}`).join(" · ")}
              </div>
            )}
            {report.legacy && (
              <div className="px-1 text-[11px] text-[color:var(--color-muted)]">
                Ältere API-Form (five_hour / seven_day) — weniger Details.
              </div>
            )}
          </div>
        )}

        {st?.codex && (
          <div className="flex flex-col gap-1.5">
            <SectionTitle
              title={`Codex${st.codex.plan ? ` · ${st.codex.plan}` : ""}`}
              note={
                st.codex.as_of
                  ? `Stand ${sinceLabel(Date.parse(st.codex.as_of), now)} (letzte Codex-Nutzung)`
                  : undefined
              }
            />
            {st.codex.limits.map((l) => (
              <LimitRow key={l.id} l={l} now={now} open={openCharts.includes(l.id)} onToggle={toggleChart} />
            ))}
          </div>
        )}

        {st?.antigravity && (
          <div className="flex flex-col gap-1.5">
            <SectionTitle title="Antigravity" />
            <div className="rounded-lg bg-[color:var(--color-surface)] px-3 py-2 text-[12px]">
              {st.antigravity.blocked && st.antigravity.resets_at ? (
                <>
                  <div className="font-medium text-rose-600">Kontingent erschöpft</div>
                  <div className="mt-0.5 text-[color:var(--color-muted)]">
                    Wieder frei {relativeReset(Date.parse(st.antigravity.resets_at), now)} ·{" "}
                    {absoluteReset(Date.parse(st.antigravity.resets_at))}
                  </div>
                </>
              ) : (
                <div className="text-[color:var(--color-fg)]">Keine aktive Sperre bekannt</div>
              )}
              <div className="mt-1 text-[11px] text-[color:var(--color-muted)]">
                Antigravity zeigt lokal keine Prozente — nur, wann ein Limit erreicht wurde.
              </div>
            </div>
          </div>
        )}
      </div>

      <div className="flex justify-between border-t border-[color:var(--color-border)] pt-1.5 text-[10px] text-[color:var(--color-muted)]">
        <span>Inoffizielle Schnittstelle · R aktualisiert · Esc zurück</span>
        <span>© {year} Martin Pfeffer | celox.io</span>
      </div>
    </div>
  );
}
